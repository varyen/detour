//! Свой VPN-сервер на компьютере: действия `server_*` (контракт роутерного
//! detour-api), жизнь процесса `detour-awgsrv` (AmneziaWG) и второго sing-box
//! (вход VLESS-Reality).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::process::Stdio;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::{Backend, Start};
use crate::ipc::{Request, Response};
use crate::server::{self, Client, Conf, PeerState};
use crate::store;

/// Запущенный помощник и аргументы, с которыми он стартовал: их смена
/// (физический адрес, подсеть, MTU) требует перезапуска.
#[derive(Default)]
pub struct Proc {
    child: Mutex<Option<(Child, String)>>,
}

impl Proc {
    async fn alive(&self) -> Option<String> {
        let mut g = self.child.lock().await;
        let (c, sig) = g.as_mut()?;
        match c.try_wait() {
            Ok(None) => Some(sig.clone()),
            _ => {
                *g = None;
                None
            }
        }
    }

    async fn stop(&self) {
        if let Some((mut c, _)) = self.child.lock().await.take() {
            let _ = c.kill().await;
        }
    }
}

const FW_RULE: &str = "Detour VPN server";
const FW_RULE_TCP: &str = "Detour VPN server (VLESS)";

fn rand_hex(n: usize) -> Result<String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| anyhow!("getrandom: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

impl Backend {
    fn server_unsupported(&self) -> Option<&'static str> {
        if !server::SUPPORTED {
            return Some("На этом устройстве свой VPN-сервер недоступен: приложение само работает через системный VPN");
        }
        if !server::binary(self.store.root()).is_file() {
            return Some("Нет помощника detour-awgsrv — переустановите приложение");
        }
        None
    }

    /// Ключи Reality — тем же sing-box, что и движок.
    fn reality_keypair(&self) -> Result<(String, String)> {
        let mut cmd = std::process::Command::new(self.engine.local_binary());
        cmd.args(["generate", "reality-keypair"]).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().map_err(|e| anyhow!("sing-box generate: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_owned());
        match (get("PrivateKey:"), get("PublicKey:")) {
            (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => Ok((a, b)),
            _ => bail!("sing-box не выдал ключи Reality"),
        }
    }

    /// Ключи, порты, подсеть и обфускация при первом включении; ключи Reality
    /// и uuid клиентов — как только их можно выдать (в т. ч. после обновления).
    fn server_ensure(&self) -> Result<Conf> {
        let mut c = server::load(&self.store);
        let mut changed = false;
        if c.privkey.is_empty() {
            if let Some(r) = self.server_unsupported() {
                bail!(r);
            }
            let data = self.store.root();
            c.privkey = server::genkey(data)?;
            c.pubkey = server::pubkey(data, &c.privkey)?;
            // Случайный свободный: 51820 часто держит обычный WireGuard на той же машине.
            c.port = loop {
                let p = 20000 + (server::rand_u32() % 40000) as u16;
                if std::net::UdpSocket::bind(("0.0.0.0", p)).is_ok() {
                    break p;
                }
            };
            c.net = "10.66.0".into();
            c.mtu = 1376;
            c.obf = server::gen_obf();
            c.socks_pass = rand_hex(16)?;
            c.awg = true;
            changed = true;
        }
        if c.vpriv.is_empty() && self.engine.local_binary().is_file() {
            if let Ok((p, q)) = self.reality_keypair() {
                c.vpriv = p;
                c.vpub = q;
                c.vsid = rand_hex(8)?;
                c.vsni = server::VSNI_DEFAULT.into();
                c.vport = if std::net::TcpListener::bind(("0.0.0.0", 443)).is_ok() {
                    443
                } else {
                    loop {
                        let p = 20000 + (server::rand_u32() % 40000) as u16;
                        if std::net::TcpListener::bind(("0.0.0.0", p)).is_ok() {
                            break p;
                        }
                    }
                };
                changed = true;
            }
        }
        if changed {
            server::save(&self.store, &c)?;
        }
        let mut list = server::clients(&self.store);
        if list.iter().any(|x| x.uuid.is_empty()) {
            for x in list.iter_mut().filter(|x| x.uuid.is_empty()) {
                x.uuid = server::new_uuid();
            }
            server::save_clients(&self.store, &list)?;
        }
        Ok(c)
    }

    fn server_phys(&self) -> Option<Ipv4Addr> {
        *self.phys_src.lock().expect("phys_src")
    }

    /// Сеть компьютера /24 для режима клиента «только дом».
    fn server_lan(&self) -> Option<String> {
        self.server_phys().map(|a| {
            let o = a.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        })
    }

    fn server_host(&self, c: &Conf) -> String {
        if !c.endpoint.is_empty() {
            return c.endpoint.clone();
        }
        self.server_phys().map(|a| a.to_string()).unwrap_or_default()
    }

    fn vless_ready(c: &Conf) -> bool {
        !c.vpriv.is_empty() && c.vport > 0 && !c.socks_pass.is_empty()
    }

    /// Привести процессы и правила брандмауэра к желаемому: сервер включён,
    /// ключи есть, движок работает. Зовётся после apply и сторожем.
    pub(super) async fn server_sync(&self) {
        let c = server::load(&self.store);
        let base = self.server_unsupported().is_none()
            && c.enabled
            && self.engine.pid().await.is_some()
            && !self.dev_mode_blocks_server();
        let list = server::clients(&self.store);
        let udp = if base && c.awg && !c.privkey.is_empty() { self.awg_sync(&c, &list).await } else {
            self.server.stop().await;
            None
        };
        let tcp = if base && c.vless && Self::vless_ready(&c) { self.vless_sync(&c, &list).await } else {
            self.vless_stop().await;
            None
        };
        server_firewall(udp, tcp);
    }

    async fn awg_sync(&self, c: &Conf, list: &[Client]) -> Option<u16> {
        let uapi = server::uapi(c, list);
        let path = self.store.path(server::UAPI);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(uapi.as_str()) {
            let _ = store::write_atomic(&path, uapi.as_bytes());
        }
        let bind = self.server_phys().map(|a| a.to_string()).unwrap_or_default();
        let sig = format!("{}|{}|{}|{}", c.net, c.mtu, bind, c.socks_pass);
        if self.server.alive().await.as_deref() == Some(sig.as_str()) {
            return Some(c.port);
        }
        self.server.stop().await;
        if let Err(e) = self.server_spawn(c, &bind, sig).await {
            tracing::warn!(error = %format!("{e:#}"), "VPN-сервер не запустился");
            return None;
        }
        Some(c.port)
    }

    async fn vless_sync(&self, c: &Conf, list: &[Client]) -> Option<u16> {
        let cfg = server::vless_config(c, list, self.server_lan().as_deref(), &self.clash_secret);
        let text = serde_json::to_string_pretty(&cfg).unwrap_or_default();
        if self.vless.alive().await.as_deref() == Some(text.as_str()) {
            return Some(c.vport);
        }
        self.vless_stop().await;
        let path = self.store.path(server::VCONF);
        if let Err(e) = store::write_atomic(&path, text.as_bytes()) {
            tracing::warn!(error = %e, "конфиг VLESS не записан");
            return None;
        }
        match self.vless_spawn(text).await {
            Ok(()) => Some(c.vport),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "вход VLESS не запустился");
                None
            }
        }
    }

    async fn vless_spawn(&self, sig: String) -> Result<()> {
        let log = self.store.path(server::VLOG);
        if let Some(d) = log.parent() {
            std::fs::create_dir_all(d)?;
        }
        let f = std::fs::File::create(&log)?;
        let mut cmd = Command::new(self.engine.local_binary());
        cmd.arg("run").arg("-c").arg(self.store.path(server::VCONF)).current_dir(self.store.root());
        cmd.stdin(Stdio::null()).stdout(Stdio::from(f.try_clone()?)).stderr(Stdio::from(f)).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| anyhow!("sing-box (VLESS): {e}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        if let Ok(Some(st)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            bail!("sing-box (VLESS) завершился сразу ({st}): {}", tail.lines().last().unwrap_or(""));
        }
        self.vmeter.lock().expect("vmeter").reset();
        *self.vless.child.lock().await = Some((child, sig));
        Ok(())
    }

    async fn vless_stop(&self) {
        self.vless.stop().await;
        self.vmeter.lock().expect("vmeter").reset();
    }

    /// Раз в пару секунд: соединения VLESS → накопленные байты по клиентам.
    pub(super) async fn server_vless_poll(&self) {
        if self.vless.alive().await.is_none() {
            return;
        }
        let Some(v) = crate::traffic::snapshot(server::VCLASH_PORT, &self.clash_secret).await else {
            return;
        };
        let conns = server::parse_vless_conns(&v);
        self.vmeter.lock().expect("vmeter").feed(&conns, server::now());
    }

    fn vless_states(&self) -> BTreeMap<String, PeerState> {
        self.vmeter.lock().expect("vmeter").users.clone()
    }

    /// В режиме разработки сервер поднимается только с DETOUR_DEV_SERVER=1:
    /// иначе стенд открывал бы порт на рабочей машине.
    fn dev_mode_blocks_server(&self) -> bool {
        self.dev_mode() && std::env::var_os("DETOUR_DEV_SERVER").is_none()
    }

    async fn server_spawn(&self, c: &Conf, bind: &str, sig: String) -> Result<()> {
        let log = self.store.path(server::LOG);
        if let Some(d) = log.parent() {
            std::fs::create_dir_all(d)?;
        }
        let f = std::fs::File::create(&log)?;
        let mut cmd = Command::new(server::binary(self.store.root()));
        cmd.arg("-config")
            .arg(self.store.path(server::UAPI))
            .arg("-status")
            .arg(self.store.path(server::STATUS))
            .arg("-addr")
            .arg(format!("{}/24", server::server_ip(c)))
            .arg("-mtu")
            .arg(c.mtu.to_string())
            .arg("-socks")
            .arg(format!("127.0.0.1:{}", server::SOCKS_PORT))
            .arg("-socks-user")
            .arg(server::SOCKS_USER)
            .arg("-socks-pass")
            .arg(&c.socks_pass)
            .arg("-socks-per-client");
        if !bind.is_empty() {
            cmd.arg("-bind-ip").arg(bind);
        }
        // stdin держим открытым: служба умерла — помощник выходит сам.
        cmd.stdin(Stdio::piped()).stdout(Stdio::from(f.try_clone()?)).stderr(Stdio::from(f)).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd.spawn().map_err(|e| anyhow!("detour-awgsrv: {e}"))?;
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        if let Ok(Some(st)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            bail!("detour-awgsrv завершился сразу ({st}): {}", tail.lines().last().unwrap_or(""));
        }
        *self.server.child.lock().await = Some((child, sig));
        Ok(())
    }

    pub(super) async fn server_shutdown(&self) {
        self.server.stop().await;
        self.vless_stop().await;
        server_firewall(None, None);
    }

    /// Раз в минуту: накопленный трафик и сессии.
    pub(super) async fn server_tick(&self) {
        let list = server::clients(&self.store);
        if list.is_empty() {
            return;
        }
        let states = if self.server.alive().await.is_some() {
            server::parse_status(&self.store.read_text(server::STATUS))
        } else {
            Default::default()
        };
        let vstates = self.vless_states();
        let prev = server::load_traffic(&self.store);
        let (next, closed) = server::account(&list, &states, &vstates, &prev, server::now());
        let _ = self.store.write_json(server::TRAFFIC, &serde_json::to_value(&next).unwrap_or_default());
        let _ = server::append_sessions(&self.store, &closed);
    }

    pub(super) async fn server_status(&self) -> Response {
        let c = server::load(&self.store);
        let reason = self.server_unsupported();
        let arun = self.server.alive().await.is_some();
        let vrun = self.vless.alive().await.is_some();
        let engine = self.engine.pid().await.is_some();
        let states = if arun { server::parse_status(&self.store.read_text(server::STATUS)) } else { Default::default() };
        let vstates = self.vless_states();
        let traffic = server::load_traffic(&self.store);
        let now = server::now();
        let clients: Vec<Value> = server::clients(&self.store)
            .iter()
            .map(|cl| server::client_json(&c, cl, &states, &vstates, traffic.get(&cl.id), now))
            .collect();
        let phys = self.server_phys().map(|a| a.to_string()).unwrap_or_default();
        let private = self.server_phys().is_some_and(|a| a.is_private() || (a.octets()[0] == 100 && (64..128).contains(&a.octets()[1])));
        let vsup = reason.is_none() && (Self::vless_ready(&c) || self.engine.local_binary().is_file());
        Response::json(&json!({
            "ok": true,
            "platform": "client",
            "supported": reason.is_none(),
            "reason": reason.unwrap_or(""),
            "installed": reason.is_none(),
            "can_install": false,
            "backend": "userspace",
            "configured": !c.privkey.is_empty(),
            "enabled": c.enabled,
            "running": arun || vrun,
            "engine_running": engine,
            "routed": true,
            "awg_enabled": c.awg,
            "awg_running": arun,
            "vless": {
                "supported": vsup,
                "reason": if vsup { "" } else { reason.unwrap_or("Нет sing-box — дождитесь загрузки движка") },
                "can_install": false,
                "enabled": c.vless,
                "running": vrun,
                "port": c.vport,
                "sni": c.vsni,
                "pubkey": c.vpub,
                "short_id": c.vsid,
                "net": "",
            },
            "iface": "awgsrv",
            "port": c.port,
            "net": if c.net.is_empty() { String::new() } else { format!("{}.0/24", c.net) },
            "server_ip": if c.net.is_empty() { String::new() } else { server::server_ip(&c) },
            "endpoint": c.endpoint,
            "endpoint_effective": self.server_host(&c),
            "wan_ip": phys,
            "wan_private": private,
            "panel_domain": "",
            "dns": c.dns,
            "mtu": if c.mtu == 0 { 1376 } else { c.mtu },
            "pubkey": c.pubkey,
            "clients": clients,
            "now": now,
        }))
    }

    pub(super) async fn server_action(&self, req: &Request, body: String) -> Result<Response> {
        let v: Value = if body.trim().is_empty() { json!({}) } else { serde_json::from_str(&body).map_err(|_| anyhow!("ожидался JSON"))? };
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let n = |k: &str| v.get(k).and_then(Value::as_u64);
        let b = |k: &str| v.get(k).and_then(Value::as_bool);
        let id = s("id").unwrap_or_default();
        match req.action.as_str() {
            "server_set" => {
                let mut c = self.server_ensure()?;
                let mut reconfig = false;
                if let Some(e) = b("enabled") {
                    reconfig |= e != c.enabled;
                    c.enabled = e;
                }
                if let Some(e) = b("awg") {
                    c.awg = e;
                }
                if let Some(e) = b("vless") {
                    if e && !Self::vless_ready(&c) {
                        bail!("VLESS недоступен: нет ключей Reality (движок ещё не загружен?)");
                    }
                    c.vless = e;
                }
                if let Some(p) = n("port") {
                    if !(1024..=65535).contains(&p) {
                        bail!("порт 1024–65535");
                    }
                    c.port = p as u16;
                }
                if let Some(p) = n("vport") {
                    if !(1..=65535).contains(&p) {
                        bail!("порт 1–65535");
                    }
                    if p as u16 != c.vport && std::net::TcpListener::bind(("0.0.0.0", p as u16)).is_err() {
                        bail!("TCP-порт {p} уже занят");
                    }
                    c.vport = p as u16;
                }
                if let Some(sni) = s("sni") {
                    let ok = sni.contains('.')
                        && sni.chars().any(|ch| ch.is_ascii_alphabetic())
                        && sni.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-');
                    if !ok {
                        bail!("маскировочный сайт: домен, например www.microsoft.com");
                    }
                    c.vsni = sni;
                }
                if let Some(m) = n("mtu") {
                    if !(1200..=1500).contains(&m) {
                        bail!("MTU 1200–1500");
                    }
                    c.mtu = m as u16;
                }
                if let Some(net) = s("net") {
                    let net = net.trim_end_matches("/24").trim_end_matches(".0").to_owned();
                    let ok = net.split('.').count() == 3
                        && net.split('.').all(|o| o.parse::<u8>().is_ok())
                        && (net.starts_with("10.") || net.starts_with("192.168.") || net.starts_with("172."));
                    if !ok {
                        bail!("подсеть: частная /24, например 10.66.0.0/24");
                    }
                    c.net = net;
                }
                if let Some(e) = s("endpoint") {
                    if !e.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-') {
                        bail!("адрес: домен или IPv4");
                    }
                    c.endpoint = e;
                }
                if let Some(d) = s("dns") {
                    if !d.is_empty() && d.parse::<Ipv4Addr>().is_err() {
                        bail!("DNS: IPv4-адрес");
                    }
                    c.dns = d;
                }
                server::save(&self.store, &c)?;
                // Вход server-in живёт в конфиге sing-box — включение и
                // выключение требуют пересборки.
                if reconfig {
                    self.apply(None, Start::IfRunning).await.map_err(|e| anyhow!("сохранено, но конфиг не собрался: {e:#}"))?;
                }
                self.server_sync().await;
            }
            "server_client_add" => {
                self.server_ensure()?;
                let name = server::clean_name(&s("name").unwrap_or_default());
                if name.is_empty() {
                    bail!("нужно имя клиента");
                }
                let mode = s("mode").filter(|m| m == "lan").unwrap_or_else(|| "full".into());
                let mut list = server::clients(&self.store);
                let octet = (2u8..=254).find(|o| list.iter().all(|x| x.octet != *o)).ok_or_else(|| anyhow!("в подсети нет свободных адресов"))?;
                let data = self.store.root();
                let privkey = server::genkey(data)?;
                let cl = Client {
                    id: server::new_id(),
                    enabled: true,
                    name,
                    octet,
                    pubkey: server::pubkey(data, &privkey)?,
                    privkey,
                    psk: server::genpsk(data)?,
                    mode,
                    created: server::now(),
                    uuid: server::new_uuid(),
                    route: String::new(),
                };
                let new_id = cl.id.clone();
                list.push(cl);
                server::save_clients(&self.store, &list)?;
                self.server_sync().await;
                return Ok(Response::json(&json!({ "ok": true, "id": new_id })));
            }
            "server_client_set" => {
                let mut list = server::clients(&self.store);
                let cl = list.iter_mut().find(|x| x.id == id).ok_or_else(|| anyhow!("клиент не найден"))?;
                if let Some(nm) = s("name") {
                    let nm = server::clean_name(&nm);
                    if nm.is_empty() {
                        bail!("пустое имя");
                    }
                    cl.name = nm;
                }
                if let Some(m) = s("mode") {
                    if m != "full" && m != "lan" {
                        bail!("режим: full или lan");
                    }
                    cl.mode = m;
                }
                if let Some(e) = b("enabled") {
                    cl.enabled = e;
                }
                let mut reroute = b("enabled").is_some() && !cl.route.is_empty();
                if let Some(r) = s("route") {
                    let ok = r.is_empty()
                        || r == "direct"
                        || r.strip_prefix("vpn:").is_some_and(|t| !t.is_empty() && t.chars().all(|ch| ch.is_ascii_alphanumeric() || "_.-".contains(ch)));
                    if !ok {
                        bail!("маршрут: пусто, direct или vpn:<id>");
                    }
                    reroute |= r != cl.route;
                    cl.route = r;
                }
                server::save_clients(&self.store, &list)?;
                // Маршрут клиента — правила в конфиге движка.
                if reroute {
                    self.apply(None, Start::IfRunning).await.map_err(|e| anyhow!("сохранено, но конфиг не собрался: {e:#}"))?;
                }
                self.server_sync().await;
            }
            "server_client_del" => {
                let mut list = server::clients(&self.store);
                let routed = list.iter().any(|x| x.id == id && !x.route.is_empty());
                let before = list.len();
                list.retain(|x| x.id != id);
                if list.len() == before {
                    bail!("клиент не найден");
                }
                server::save_clients(&self.store, &list)?;
                if routed {
                    self.apply(None, Start::IfRunning).await.map_err(|e| anyhow!("удалён, но конфиг не собрался: {e:#}"))?;
                }
                self.server_sync().await;
            }
            "server_client_conf" => {
                let c = server::load(&self.store);
                let list = server::clients(&self.store);
                let cl = list.iter().find(|x| x.id == id).ok_or_else(|| anyhow!("клиент не найден"))?;
                let conf = server::client_conf(&c, cl, &self.server_host(&c), self.server_lan().as_deref());
                return Ok(Response::json(&json!({ "ok": true, "conf": conf })));
            }
            "server_client_link" => {
                let c = self.server_ensure()?;
                if !Self::vless_ready(&c) {
                    bail!("вход VLESS не настроен");
                }
                let list = server::clients(&self.store);
                let cl = list.iter().find(|x| x.id == id).ok_or_else(|| anyhow!("клиент не найден"))?;
                let link = server::client_link(&c, cl, &self.server_host(&c));
                return Ok(Response::json(&json!({ "ok": true, "link": link })));
            }
            "server_regen" => {
                let mut c = self.server_ensure()?;
                c.obf = server::gen_obf();
                server::save(&self.store, &c)?;
                // Параметры обфускации на лету не меняются надёжно — перезапуск.
                self.server.stop().await;
                self.server_sync().await;
            }
            "server_install" => bail!("помощник ставится вместе с приложением"),
            _ => bail!("unknown action"),
        }
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) fn server_sessions(&self, req: &Request) -> Response {
        let id = req.param("id").unwrap_or("");
        Response::json(&json!({ "ok": true, "sessions": server::sessions(&self.store, id) }))
    }
}

/// Windows: входящий UDP (AmneziaWG) и TCP (VLESS) на порты сервера. Служба
/// работает от SYSTEM, и брандмауэр молча режет неизвестный процесс без запроса.
#[cfg(windows)]
fn server_firewall(udp: Option<u16>, tcp: Option<u16>) {
    use std::os::windows::process::CommandExt;
    let run = |args: &[&str]| {
        let _ = std::process::Command::new("netsh")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    };
    static LAST: std::sync::Mutex<Option<(Option<u16>, Option<u16>)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().expect("fw");
    if *last == Some((udp, tcp)) {
        return;
    }
    for (name, proto, port) in [(FW_RULE, "UDP", udp), (FW_RULE_TCP, "TCP", tcp)] {
        run(&["advfirewall", "firewall", "delete", "rule", &format!("name={name}")]);
        if let Some(p) = port {
            run(&[
                "advfirewall", "firewall", "add", "rule", &format!("name={name}"), "dir=in", "action=allow",
                &format!("protocol={proto}"), &format!("localport={p}"), "profile=any", "enable=yes",
            ]);
        }
    }
    *last = Some((udp, tcp));
}

#[cfg(not(windows))]
fn server_firewall(_udp: Option<u16>, _tcp: Option<u16>) {
    let _ = (FW_RULE, FW_RULE_TCP);
}
