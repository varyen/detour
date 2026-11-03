//! Функциональная проверка профилей — порт `detour-health`: временный
//! sing-box без TUN, у каждого профиля свой тег, задержка — через clash API
//! (`/proxies/<tag>/delay`) до каждой цели из списка, скорость — загрузкой
//! через локальный mixed-вход профиля. Один битый профиль не валит проверку
//! остальных: конфиг проверяется `sing-box check` и делится пополам, пока
//! отказник не останется один.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::engine::Engine;
use crate::render::{self, Hop};
use crate::store::{self, now_epoch};

const PARALLEL: usize = 4;
const READY_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verdict {
    pub ok: bool,
    pub ts: i64,
    /// Задержка первой цели, мс; -1 — не дошли.
    pub rtt: i64,
    /// Скорость загрузки, кбит/с; -1 — не мерили или не вышло.
    pub dl: i64,
    pub delays: Vec<i64>,
    /// Что ответил движок на первый неудачный замер.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
}

impl Verdict {
    fn down(n: usize) -> Self {
        Self { ok: false, ts: now_epoch(), rtt: -1, dl: -1, delays: vec![-1; n], err: None }
    }
}

pub struct Target {
    pub id: String,
    pub outbound: Value,
}

pub struct Speed {
    pub url: String,
    pub bytes: u64,
    pub skip: HashSet<String>,
}

pub struct Opts<'a> {
    pub urls: &'a [(String, String)],
    pub speed: Option<&'a Speed>,
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

fn random_hex() -> String {
    let mut b = [0u8; 12];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Prepared {
    index: usize,
    hop: Hop,
}

fn config(items: &[Prepared], clash: Option<(u16, &str)>, mixed: &BTreeMap<usize, u16>) -> Value {
    let mut outbounds = vec![json!({ "type": "direct", "tag": "direct" })];
    let mut endpoints = Vec::new();
    for p in items {
        match &p.hop {
            Hop::Outbound(v) => outbounds.push(v.clone()),
            Hop::Endpoint(v) => endpoints.push(v.clone()),
        }
    }
    let mut inbounds = Vec::new();
    let mut rules = Vec::new();
    for (i, port) in mixed {
        inbounds.push(json!({ "type": "mixed", "tag": format!("m{i}"), "listen": "127.0.0.1", "listen_port": port }));
        rules.push(json!({ "inbound": [format!("m{i}")], "outbound": format!("h{i}") }));
    }
    // auto_detect_interface — чтобы при поднятом TUN основной службы проба
    // шла с физического интерфейса, а не через сам туннель.
    let mut cfg = json!({
        "log": { "level": "error" },
        "dns": { "servers": [{ "type": "local", "tag": "local" }] },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": { "rules": rules, "final": "direct", "auto_detect_interface": true, "default_domain_resolver": "local" },
    });
    if !endpoints.is_empty() {
        cfg["endpoints"] = json!(endpoints);
    }
    if let Some((port, secret)) = clash {
        cfg["experimental"] = json!({ "clash_api": { "external_controller": format!("127.0.0.1:{port}"), "secret": secret } });
    }
    cfg
}

/// Отсев профилей, на которых sing-box отказывается собирать конфиг.
async fn validate(engine: &Engine, dir: &Path, items: Vec<Prepared>) -> (Vec<Prepared>, Vec<usize>) {
    let file = dir.join("check.json");
    let mut good = Vec::new();
    let mut bad = Vec::new();
    let mut work = vec![items];
    while let Some(chunk) = work.pop() {
        if chunk.is_empty() {
            continue;
        }
        let cfg = config(&chunk, None, &BTreeMap::new());
        let ok = std::fs::write(&file, serde_json::to_vec(&cfg).unwrap_or_default()).is_ok()
            && engine.check(&file).await.is_ok();
        if ok {
            good.extend(chunk);
        } else if chunk.len() == 1 {
            bad.push(chunk[0].index);
        } else {
            let mut left = chunk;
            let right = left.split_off(left.len() / 2);
            work.push(left);
            work.push(right);
        }
    }
    (good, bad)
}

async fn delay(client: &reqwest::Client, base: &str, secret: &str, tag: &str, url: &str) -> Result<i64, String> {
    let mut u = reqwest::Url::parse(&format!("{base}/proxies/{tag}/delay")).map_err(|e| e.to_string())?;
    u.query_pairs_mut().append_pair("timeout", "4000").append_pair("url", url);
    let resp = client.get(u).bearer_auth(secret).send().await.map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(d) = v.get("delay").and_then(Value::as_i64).filter(|d| *d > 0) {
        return Ok(d);
    }
    Err(v.get("message").and_then(Value::as_str).unwrap_or("нет ответа").to_owned())
}

/// Все цели по порядку, неудачная повторяется один раз; после первого
/// провала остальные не запрашиваются. `ok` — только если прошли все.
async fn probe_one(client: reqwest::Client, base: String, secret: String, tag: String, urls: Vec<String>) -> Verdict {
    let mut delays = vec![-1; urls.len()];
    let mut ok = !urls.is_empty();
    let mut err = None;
    for (i, url) in urls.iter().enumerate() {
        let mut d = delay(&client, &base, &secret, &tag, url).await;
        if d.is_err() {
            d = delay(&client, &base, &secret, &tag, url).await;
        }
        match d {
            Ok(ms) => delays[i] = ms,
            Err(e) => {
                ok = false;
                err = Some(e);
                break;
            }
        }
    }
    let rtt = delays.first().copied().unwrap_or(-1);
    Verdict { ok, ts: now_epoch(), rtt: if ok { rtt } else { -1 }, dl: -1, delays, err }
}

fn speed_budget(bytes: u64) -> Duration {
    Duration::from_secs(match bytes {
        b if b <= 8_000_000 => 5,
        b if b <= 30_000_000 => 8,
        b if b <= 75_000_000 => 12,
        _ => 15,
    })
}

/// кбит/с по тому, что успело прийти за отведённое время (как `curl -m`
/// с `%{speed_download}`: оборванная по времени загрузка тоже даёт замер).
async fn measure(port: u16, url: &str, bytes: u64) -> i64 {
    measure_via(&format!("socks5h://127.0.0.1:{port}"), url, bytes).await
}

async fn measure_via(proxy_url: &str, url: &str, bytes: u64) -> i64 {
    let Ok(proxy) = reqwest::Proxy::all(proxy_url) else { return -1 };
    let budget = speed_budget(bytes);
    let Ok(client) = crate::http::client_builder().proxy(proxy).timeout(budget + Duration::from_secs(2)).build() else {
        return -1;
    };
    let start = Instant::now();
    let Ok(mut resp) = client.get(url).send().await else { return -1 };
    let mut got: u64 = 0;
    while start.elapsed() < budget {
        match tokio::time::timeout(budget.saturating_sub(start.elapsed()), resp.chunk()).await {
            Ok(Ok(Some(c))) => got += c.len() as u64,
            _ => break,
        }
    }
    let secs = start.elapsed().as_secs_f64();
    if got == 0 || secs <= 0.0 {
        return -1;
    }
    (got as f64 * 8.0 / 1000.0 / secs) as i64
}

/// Порты mixed-входов для замера скорости: только тем, кого не исключили.
fn speed_ports(targets: &[Target], indices: impl Iterator<Item = usize>, opts: &Opts<'_>) -> BTreeMap<usize, u16> {
    let mut mixed = BTreeMap::new();
    if let Some(sp) = opts.speed {
        for i in indices {
            if !sp.skip.contains(&targets[i].id) {
                if let Some(port) = free_port() {
                    mixed.insert(i, port);
                }
            }
        }
    }
    mixed
}

pub async fn run(engine: &Engine, run_dir: &Path, targets: Vec<Target>, opts: &Opts<'_>) -> BTreeMap<String, Verdict> {
    let mut out = BTreeMap::new();
    if targets.is_empty() {
        return out;
    }
    // На телефоне sing-box вкомпилирован в приложение в единственном числе —
    // временным движком для всех профилей служит mihomo.
    let all_mihomo = cfg!(any(target_os = "android", target_os = "ios"));
    let (awg, plain): (Vec<usize>, Vec<usize>) =
        (0..targets.len()).partition(|&i| all_mihomo || crate::awg::is_awg(&targets[i].outbound));
    let (a, b) = tokio::join!(
        run_singbox(engine, run_dir, &targets, plain, opts),
        run_mihomo(engine, run_dir, &targets, awg, opts),
    );
    for (i, v) in a.into_iter().chain(b) {
        out.insert(targets[i].id.clone(), v);
    }
    out
}

async fn run_singbox(engine: &Engine, run_dir: &Path, targets: &[Target], indices: Vec<usize>, opts: &Opts<'_>) -> BTreeMap<usize, Verdict> {
    let n_urls = opts.urls.len();
    let mut out = BTreeMap::new();
    if indices.is_empty() {
        return out;
    }
    let dir = run_dir.join(format!("probe-{}", random_hex()));
    let _ = std::fs::create_dir_all(&dir);

    let mut prepared = Vec::new();
    for i in indices {
        match render::prepare(targets[i].outbound.clone(), &format!("h{i}"), None) {
            Ok(hop) => prepared.push(Prepared { index: i, hop }),
            Err(_) => {
                out.insert(i, Verdict::down(n_urls));
            }
        }
    }
    let (good, bad) = validate(engine, &dir, prepared).await;
    for i in bad {
        out.insert(i, Verdict::down(n_urls));
    }
    if good.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return out;
    }

    let secret = random_hex();
    let Some(clash_port) = free_port() else { return out };
    let mixed = speed_ports(targets, good.iter().map(|p| p.index), opts);
    let file = dir.join("probe.json");
    let cfg = config(&good, Some((clash_port, &secret)), &mixed);
    if std::fs::write(&file, serde_json::to_vec(&cfg).unwrap_or_default()).is_err() {
        return out;
    }
    let Ok(child) = engine.spawn_aux(&file, &dir) else { return out };
    let indices: Vec<usize> = good.iter().map(|p| p.index).collect();
    out.extend(drive(child, clash_port, &secret, &indices, &mixed, opts).await);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn mihomo_proxy(tag: &str, ob: &Value) -> anyhow::Result<Value> {
    if crate::awg::is_awg(ob) {
        return crate::awg::proxy(tag, ob);
    }
    let (Hop::Outbound(v) | Hop::Endpoint(v)) = render::prepare(ob.clone(), tag, None)?;
    crate::mihomo::proxy(&v).map_err(|e| anyhow::anyhow!(e))
}

/// Конфиг временного mihomo: по прокси `h<i>` на AWG-профиль, clash API для
/// задержки и mixed-вход на профиль для скорости.
fn mihomo_config(proxies: &[(usize, Value)], clash: Option<(u16, &str)>, mixed: &BTreeMap<usize, u16>) -> Value {
    let listeners: Vec<Value> = mixed
        .iter()
        .map(|(i, port)| json!({ "name": format!("m{i}"), "type": "mixed", "listen": "127.0.0.1", "port": port, "proxy": format!("h{i}") }))
        .collect();
    let mut cfg = json!({
        "log-level": "info",
        "allow-lan": false,
        "bind-address": "127.0.0.1",
        "ipv6": true,
        "mode": "rule",
        "profile": { "store-selected": false, "store-fake-ip": false },
        "proxies": proxies.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
        "rules": ["MATCH,REJECT"],
    });
    // На Android нет /etc/resolv.conf: системный резолвер Go стучится в
    // [::1]:53, и профиль с именем сервера вместо IP «не отвечает».
    if cfg!(any(target_os = "android", target_os = "ios")) {
        cfg["dns"] = json!({
            "enable": true,
            "ipv6": false,
            "default-nameserver": ["1.1.1.1", "8.8.8.8"],
            "nameserver": ["1.1.1.1", "8.8.8.8"],
        });
    }
    if !listeners.is_empty() {
        cfg["listeners"] = json!(listeners);
    }
    if let Some((port, secret)) = clash {
        cfg["external-controller"] = json!(format!("127.0.0.1:{port}"));
        cfg["secret"] = json!(secret);
    }
    cfg
}

/// AmneziaWG умеет только mihomo — пробы для него идут отдельным временным
/// mihomo при любом режиме движка. clash API у него тот же, что у sing-box.
async fn run_mihomo(engine: &Engine, run_dir: &Path, targets: &[Target], indices: Vec<usize>, opts: &Opts<'_>) -> BTreeMap<usize, Verdict> {
    let n_urls = opts.urls.len();
    let mut out = BTreeMap::new();
    // Нечем проверить — без вердикта, а не «лежит».
    if indices.is_empty() || !crate::awg::SUPPORTED || !engine.mihomo_binary().is_file() {
        return out;
    }
    let dir = run_dir.join(format!("probe-m-{}", random_hex()));
    let _ = std::fs::create_dir_all(&dir);
    let check = dir.join("check.yaml");

    let mut good = Vec::new();
    for i in indices {
        match mihomo_proxy(&format!("h{i}"), &targets[i].outbound) {
            Ok(p) => good.push((i, p)),
            Err(_) => {
                out.insert(i, Verdict::down(n_urls));
            }
        }
    }
    let valid = |items: &[(usize, Value)]| {
        std::fs::write(&check, serde_json::to_vec(&mihomo_config(items, None, &BTreeMap::new())).unwrap_or_default()).is_ok()
    };
    if !good.is_empty() && !(valid(&good) && engine.check_mihomo(&check).await.is_ok()) {
        let mut kept = Vec::new();
        for item in good {
            if valid(std::slice::from_ref(&item)) && engine.check_mihomo(&check).await.is_ok() {
                kept.push(item);
            } else {
                out.insert(item.0, Verdict::down(n_urls));
            }
        }
        good = kept;
    }
    if good.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return out;
    }

    let secret = random_hex();
    let Some(clash_port) = free_port() else { return out };
    let mixed = speed_ports(targets, good.iter().map(|(i, _)| *i), opts);
    let file = dir.join("probe.yaml");
    let cfg = mihomo_config(&good, Some((clash_port, &secret)), &mixed);
    if std::fs::write(&file, serde_json::to_vec(&cfg).unwrap_or_default()).is_err() {
        return out;
    }
    let Ok(child) = engine.spawn_aux_mihomo(&file, &dir) else { return out };
    let indices: Vec<usize> = good.iter().map(|(i, _)| *i).collect();
    let mut verdicts = drive(child, clash_port, &secret, &indices, &mixed, opts).await;
    // clash API на провал отвечает общим «error in the delay test»; настоящая
    // причина — в журнале самого mihomo.
    if verdicts.values().any(|v| !v.ok) {
        let log = std::fs::read_to_string(dir.join(crate::engine::AUX_LOG)).unwrap_or_default();
        for (i, v) in verdicts.iter_mut().filter(|(_, v)| !v.ok) {
            let tag = format!("h{i}");
            let line = log
                .lines()
                .rev()
                .find(|l| (l.contains("level=error") || l.contains("level=warning")) && (l.contains(&tag) || indices.len() == 1));
            if let Some(l) = line {
                let msg = l.split("msg=").nth(1).unwrap_or(l).trim().trim_matches('"');
                v.err = Some(msg.chars().take(300).collect());
            }
        }
    }
    out.extend(verdicts);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Проба через рабочий движок — там, где второй экземпляр не поднять
/// (Android, iOS): задержка — clash API живого конфига по выходу `proxy`,
/// скорость — загрузкой через его вход `fetch-in`. Годится только для
/// профиля, который сейчас активен.
pub async fn run_live(clash_port: u16, secret: &str, proxy_url: &str, opts: &Opts<'_>) -> Verdict {
    let client = crate::http::client_builder().no_proxy().timeout(Duration::from_secs(6)).build().expect("reqwest");
    let urls: Vec<String> = opts.urls.iter().map(|(_, u)| u.clone()).collect();
    let mut v = probe_one(client, format!("http://127.0.0.1:{clash_port}"), secret.to_owned(), "proxy".to_owned(), urls).await;
    if let Some(sp) = opts.speed.filter(|_| v.ok) {
        v.dl = measure_via(proxy_url, &sp.url, sp.bytes).await;
    }
    v
}

/// Дождаться clash API временного движка, снять задержки и скорость, убить.
async fn drive(
    mut child: tokio::process::Child,
    clash_port: u16,
    secret: &str,
    indices: &[usize],
    mixed: &BTreeMap<usize, u16>,
    opts: &Opts<'_>,
) -> BTreeMap<usize, Verdict> {
    let secret = secret.to_owned();
    let base = format!("http://127.0.0.1:{clash_port}");
    let client = crate::http::client_builder().no_proxy().timeout(Duration::from_secs(6)).build().expect("reqwest");
    let deadline = Instant::now() + READY_WAIT;
    let mut ready = false;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if client.get(format!("{base}/version")).bearer_auth(&secret).send().await.is_ok_and(|r| r.status().is_success()) {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    // Проба не поднялась — вердикта нет: «не проверили» не то же, что «не работает».
    if !ready {
        let _ = child.kill().await;
        return BTreeMap::new();
    }

    let urls: Vec<String> = opts.urls.iter().map(|(_, u)| u.clone()).collect();
    let limit = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
    let mut set = JoinSet::new();
    for &index in indices {
        let (client, base, secret, urls, limit) = (client.clone(), base.clone(), secret.clone(), urls.clone(), limit.clone());
        let tag = format!("h{index}");
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            (index, probe_one(client, base, secret, tag, urls).await)
        });
    }
    let mut verdicts: BTreeMap<usize, Verdict> = BTreeMap::new();
    while let Some(Ok((i, v))) = set.join_next().await {
        verdicts.insert(i, v);
    }

    if let Some(sp) = opts.speed {
        for (i, v) in verdicts.iter_mut().filter(|(_, v)| v.ok) {
            if let Some(port) = mixed.get(i) {
                v.dl = measure(*port, &sp.url, sp.bytes).await;
            }
        }
    }

    let _ = child.kill().await;
    verdicts
}

/// Цели проверки: `Название|URL` на строку; пусто — три встроенные.
pub fn urls(store: &crate::store::Store) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = store
        .read_text(store::HEALTH_URLS)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (label, url) = l.split_once('|').map(|(a, b)| (a.trim(), b.trim())).unwrap_or(("", l));
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return None;
            }
            let label = if label.is_empty() {
                url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
            } else {
                label
            };
            Some((label.to_owned(), url.to_owned()))
        })
        .collect();
    if v.is_empty() {
        v = vec![
            ("YouTube".into(), "https://www.youtube.com/generate_204".into()),
            ("Видео YouTube".into(), "https://redirector.googlevideo.com/generate_204".into()),
            ("Google".into(), "https://www.google.com/generate_204".into()),
        ];
    }
    v
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Обычный (не AWG) профиль через временный mihomo — путь проб на телефоне.
    /// `DETOUR_MIHOMO_BIN=<mihomo> DETOUR_PROBE_LINK=<файл со ссылкой> cargo test -p detour-core mihomo_probe -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn mihomo_probe_plain_profile() {
        let link = std::fs::read_to_string(std::env::var("DETOUR_PROBE_LINK").expect("DETOUR_PROBE_LINK")).unwrap();
        let parsed = crate::sharelink::parse(link.trim()).expect("ссылка не разобралась");
        let dir = std::env::temp_dir().join(format!("detour-probe-test-{}", random_hex()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = Engine::new(&dir, dir.join("run"), dir.join("stderr.log"));
        assert!(engine.mihomo_binary().is_file(), "нет mihomo: {}", engine.mihomo_binary().display());
        let targets = vec![Target { id: "t".into(), outbound: parsed.outbound }];
        let urls = vec![("Google".to_owned(), "https://www.google.com/generate_204".to_owned())];
        let speed = Speed { url: "https://speed.cloudflare.com/__down?bytes=8000000".into(), bytes: 8_000_000, skip: HashSet::new() };
        let opts = Opts { urls: &urls, speed: Some(&speed) };
        let out = run_mihomo(&engine, &dir, &targets, vec![0], &opts).await;
        let _ = std::fs::remove_dir_all(&dir);
        let v = out.get(&0).expect("нет вердикта");
        println!("verdict: {v:?}");
        assert!(v.ok && v.rtt > 0 && v.dl > 0, "{v:?}");
    }
}


