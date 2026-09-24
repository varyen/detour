//! AmneziaWG через сайдкар mihomo — так же, как на роутере (`detour-awg`).
//!
//! sing-box AmneziaWG не умеет, mihomo умеет все версии (1.0, 1.5, 2.0, 3.x).
//! Для sing-box AWG-профиль — socks-outbound на 127.0.0.1:<порт>, а туннель
//! держит mihomo: по socks-listener'у на каждый профиль, привязанному к своему
//! прокси. Порты раздаются при каждой сборке конфига заново, начиная с
//! `PORT_BASE`, — конфиг mihomo пишется в той же сборке, так что разъехаться
//! им не с чем.
//!
//! Петли нет по той же причине, что у tpws: на десктопе исходящие mihomo
//! уходят напрямую правилом по имени процесса, на Android приложение (а с ним
//! и дочерний mihomo) исключено из VpnService. На iOS сторонних процессов нет
//! вовсе, поэтому AWG там недоступен.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[cfg(windows)]
pub const EXE: &str = "mihomo.exe";
#[cfg(not(windows))]
pub const EXE: &str = "mihomo";

pub const TYPE: &str = "amneziawg";
pub const PORT_BASE: u16 = 19600;
pub const SUPPORTED: bool = !cfg!(target_os = "ios");

/// Ключи `amnezia-wg-option`, которые понимает mihomo, — те же, что пишет панель.
const KEYS: [&str; 30] = [
    "version", "jc", "jmin", "jmax", "s1", "s2", "s3", "s4",
    "h1", "h2", "h3", "h4", "i1", "i2", "i3", "i4", "i5",
    "j1", "j2", "j3", "itime",
    "header-protection-key", "content-padding-addition", "rekey-after-time",
    "rekey-timeout", "reject-after-time", "keepalive-timeout",
    "max-handshake-attempts", "random-trailers", "disable-cookies",
];
const NUMERIC: [&str; 9] = ["version", "jc", "jmin", "jmax", "s1", "s2", "s3", "s4", "itime"];
const BOOL: [&str; 2] = ["random-trailers", "disable-cookies"];

pub fn is_awg(ob: &Value) -> bool {
    ob.get("type").and_then(Value::as_str) == Some(TYPE)
}

/// Что видит sing-box вместо AWG-профиля.
pub fn socks_outbound(port: u16) -> Value {
    json!({ "type": "socks", "server": "127.0.0.1", "server_port": port, "version": "5" })
}

/// AWG-профили одной сборки: (id, порт, исходный outbound).
#[derive(Default, Clone)]
pub struct Sidecars {
    pub list: Vec<(String, u16, Value)>,
}

impl Sidecars {
    /// Порт профиля: повторная ссылка на тот же профиль (цель маршрута,
    /// совпавшая с хопом) получает тот же listener.
    pub fn port_for(&mut self, id: &str, ob: &Value) -> u16 {
        if let Some((_, p, _)) = self.list.iter().find(|(i, _, _)| i == id) {
            return *p;
        }
        let p = PORT_BASE + self.list.len() as u16;
        self.list.push((id.to_owned(), p, ob.clone()));
        p
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

fn num(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn host_of(a: &str) -> &str {
    a.split('/').next().unwrap_or(a)
}

/// Один прокси mihomo из outbound'а профиля (плоская форма панели).
pub fn proxy(name: &str, ob: &Value) -> Result<Value> {
    let s = |k: &str| ob.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let server = s("server").context("нет адреса сервера")?;
    let port = ob.get("server_port").and_then(num).context("нет порта сервера")?;
    let private_key = s("private_key").context("нет приватного ключа")?;
    let public_key = s("peer_public_key").context("нет публичного ключа сервера")?;
    let addrs: Vec<String> = match ob.get("local_address").or_else(|| ob.get("address")) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Some(Value::String(a)) => vec![a.clone()],
        _ => Vec::new(),
    };
    let ip4 = addrs.iter().map(|a| host_of(a)).find(|h| !h.contains(':'));
    let ip6 = addrs.iter().map(|a| host_of(a)).find(|h| h.contains(':'));
    if ip4.is_none() && ip6.is_none() {
        bail!("нет адреса интерфейса (Address)");
    }
    let mut p = Map::new();
    p.insert("name".into(), json!(name));
    p.insert("type".into(), json!("wireguard"));
    p.insert("server".into(), json!(server));
    p.insert("port".into(), json!(port));
    p.insert("private-key".into(), json!(private_key));
    p.insert("public-key".into(), json!(public_key));
    if let Some(v) = ip4 {
        p.insert("ip".into(), json!(v));
    }
    if let Some(v) = ip6 {
        p.insert("ipv6".into(), json!(v));
    }
    p.insert("udp".into(), json!(true));
    p.insert("mtu".into(), json!(ob.get("mtu").and_then(num).unwrap_or(1280)));
    p.insert("allowed-ips".into(), json!(["0.0.0.0/0", "::/0"]));
    if let Some(psk) = s("pre_shared_key") {
        p.insert("pre-shared-key".into(), json!(psk));
    }
    if let Some(k) = ob.get("persistent_keepalive_interval").and_then(num) {
        p.insert("persistent-keepalive".into(), json!(k));
    }
    if let Some(opt) = ob.get("amnezia").and_then(option) {
        p.insert("amnezia-wg-option".into(), opt);
    }

    Ok(Value::Object(p))
}

/// `outbound.amnezia` профиля → `amnezia-wg-option` mihomo (ключи, числа, флаги).
pub fn option(src: &Value) -> Option<Value> {
    let mut opt = Map::new();
    if let Some(src) = src.as_object() {
        for k in KEYS {
            let v = src.get(k).or_else(|| src.get(&k.replace('-', "_")));
            let Some(v) = v.filter(|v| !v.is_null() && v.as_str() != Some("")) else { continue };
            let v = if NUMERIC.contains(&k) {
                match num(v) {
                    Some(n) => json!(n),
                    None => continue,
                }
            } else if BOOL.contains(&k) {
                json!(matches!(v, Value::Bool(true)) || matches!(v.as_str(), Some("1" | "true")) || num(v) == Some(1))
            } else {
                match v {
                    Value::String(s) => json!(s),
                    other => json!(other.to_string()),
                }
            };
            opt.insert(k.into(), v);
        }
    }
    (!opt.is_empty()).then_some(Value::Object(opt))
}

/// Конфиг mihomo: по listener'у на профиль, всё прочее отвергается.
pub fn config(s: &Sidecars) -> Result<Value> {
    let mut proxies = Vec::new();
    let mut listeners = Vec::new();
    for (id, port, ob) in &s.list {
        let name = format!("awg-{id}");
        proxies.push(proxy(&name, ob).with_context(|| format!("AmneziaWG-профиль {id}"))?);
        listeners.push(json!({
            "name": format!("in-{id}"), "type": "socks", "listen": "127.0.0.1",
            "port": port, "udp": true, "proxy": name,
        }));
    }
    Ok(json!({
        "log-level": "warning",
        "allow-lan": false,
        "bind-address": "127.0.0.1",
        "ipv6": true,
        "mode": "rule",
        "profile": { "store-selected": false, "store-fake-ip": false },
        "proxies": proxies,
        "listeners": listeners,
        "rules": ["MATCH,REJECT"],
    }))
}

pub struct Sidecar {
    local: PathBuf,
    bundled: Option<PathBuf>,
    workdir: PathBuf,
    log: PathBuf,
    child: Mutex<Option<Child>>,
    version: Mutex<Option<(PathBuf, std::time::SystemTime, String)>>,
}

impl Sidecar {
    pub fn new(data: &Path, workdir: PathBuf, log: PathBuf) -> Self {
        // На Android бинарник лежит в APK (`nativeLibraryDir/libmihomo.so`),
        // путь знает только java-сторона.
        let bundled = match std::env::var_os("DETOUR_MIHOMO_BIN") {
            Some(p) => Some(PathBuf::from(p)),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join(EXE)))
                .filter(|p| p.exists()),
        };
        Self { local: data.join("bin").join(EXE), bundled, workdir, log, child: Mutex::new(None), version: Mutex::new(None) }
    }

    pub fn binary(&self) -> PathBuf {
        if self.local.is_file() {
            return self.local.clone();
        }
        self.bundled.clone().unwrap_or_else(|| self.local.clone())
    }

    pub fn present(&self) -> bool {
        SUPPORTED && self.binary().is_file()
    }

    fn command(&self) -> Command {
        let mut c = Command::new(self.binary());
        c.current_dir(&self.workdir).stdin(Stdio::null());
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        #[cfg(unix)]
        crate::dpi::close_inherited(&mut c);
        c
    }

    /// «Mihomo Meta v1.19.31 windows amd64 …» → «1.19.31». Кэш по mtime:
    /// статус опрашивается постоянно, а запуск 60-мегабайтного бинарника не бесплатен.
    pub async fn version(&self) -> Option<String> {
        if !self.present() {
            return None;
        }
        let bin = self.binary();
        let mtime = std::fs::metadata(&bin).and_then(|m| m.modified()).ok()?;
        let mut cache = self.version.lock().await;
        if let Some((p, t, v)) = cache.as_ref() {
            if *p == bin && *t == mtime {
                return Some(v.clone());
            }
        }
        // Рабочий каталог появляется с первым стартом сайдкара, а без него
        // запуск с current_dir падает ещё до exec.
        let _ = std::fs::create_dir_all(&self.workdir);
        let out = self.command().arg("-v").stdout(Stdio::piped()).output().await.ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let w = text.split_whitespace().find(|w| w.starts_with('v') && w[1..].starts_with(|c: char| c.is_ascii_digit()))?;
        let v = w.trim_start_matches('v').to_owned();
        *cache = Some((bin, mtime, v.clone()));
        Some(v)
    }

    pub async fn check(&self, config: &Path) -> Result<(), String> {
        if !self.present() {
            return Err(format!("mihomo не найден: {}", self.binary().display()));
        }
        std::fs::create_dir_all(&self.workdir).map_err(|e| e.to_string())?;
        let out = self
            .command()
            .arg("-t")
            .arg("-d")
            .arg(&self.workdir)
            .arg("-f")
            .arg(config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("не удалось запустить mihomo: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Err(text.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").to_owned())
    }

    pub async fn start(&self, config: &Path) -> Result<u32> {
        self.stop().await;
        if !SUPPORTED {
            bail!("AmneziaWG на этой платформе недоступен");
        }
        if !self.present() {
            bail!("mihomo не найден: {}", self.binary().display());
        }
        std::fs::create_dir_all(&self.workdir)?;
        if let Some(dir) = self.log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let f = std::fs::File::create(&self.log)?;
        let mut child = self
            .command()
            .arg("-d")
            .arg(&self.workdir)
            .arg("-f")
            .arg(config)
            .stdout(Stdio::from(f.try_clone()?))
            .stderr(Stdio::from(f))
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить mihomo")?;
        let pid = child.id().unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(800)).await;
        if let Ok(Some(status)) = child.try_wait() {
            bail!("mihomo завершился сразу ({status}): {}", self.log_tail(6));
        }
        *self.child.lock().await = Some(child);
        Ok(pid)
    }

    pub async fn stop(&self) {
        if let Some(mut c) = self.child.lock().await.take() {
            let _ = c.kill().await;
        }
    }

    pub async fn pid(&self) -> Option<u32> {
        let mut guard = self.child.lock().await;
        let child = guard.as_mut()?;
        match child.try_wait() {
            Ok(None) => child.id(),
            _ => {
                *guard = None;
                None
            }
        }
    }

    pub fn log_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let v: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        v[v.len().saturating_sub(lines)..].join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn awg_ob() -> Value {
        json!({
            "type": "amneziawg", "server": "203.0.113.5", "server_port": 51820,
            "private_key": "k", "peer_public_key": "p", "local_address": ["10.8.1.2/32", "fd00::2/128"],
            "persistent_keepalive_interval": 25,
            "amnezia": { "jc": 4, "jmin": "40", "h1": "100-200", "i1": "<r 32>", "version": 3, "random-trailers": true },
        })
    }

    #[test]
    fn proxy_maps_fields_and_options() {
        let p = proxy("awg-x", &awg_ob()).unwrap();
        assert_eq!(p["type"], "wireguard");
        assert_eq!(p["ip"], "10.8.1.2");
        assert_eq!(p["ipv6"], "fd00::2");
        assert_eq!(p["persistent-keepalive"], 25);
        let o = &p["amnezia-wg-option"];
        assert_eq!(o["jc"], 4);
        assert_eq!(o["jmin"], 40);
        assert_eq!(o["h1"], "100-200");
        assert_eq!(o["i1"], "<r 32>");
        assert_eq!(o["version"], 3);
        assert_eq!(o["random-trailers"], true);
    }

    #[test]
    fn ports_are_stable_within_a_build() {
        let mut s = Sidecars::default();
        assert_eq!(s.port_for("a", &awg_ob()), PORT_BASE);
        assert_eq!(s.port_for("b", &awg_ob()), PORT_BASE + 1);
        assert_eq!(s.port_for("a", &awg_ob()), PORT_BASE);
        let c = config(&s).unwrap();
        assert_eq!(c["listeners"][1]["port"], PORT_BASE + 1);
        assert_eq!(c["listeners"][1]["proxy"], "awg-b");
    }

    #[test]
    fn missing_keys_are_an_error() {
        let mut ob = awg_ob();
        ob.as_object_mut().unwrap().remove("private_key");
        assert!(proxy("x", &ob).is_err());
    }
}
