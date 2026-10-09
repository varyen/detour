//! VPN-сервер AmneziaWG на компьютере — порт роутерного `detour-server`.
//!
//! Сервер — отдельный процесс `detour-awgsrv` (Go, `client/awgsrv`): устройство
//! amneziawg-go на userspace-стеке gVisor, чьи соединения уходят в sing-box
//! через вход `server-in` (SOCKS5 с паролем). Поэтому клиенты сервера получают
//! те же правила маршрутизации, что и сам компьютер, без NAT и драйверов ОС.
//! Работает, пока работает движок: без sing-box соединениям некуда идти.
//!
//! Здесь — модель (настройки, клиенты), UAPI-конфиг для помощника, `.conf`
//! для приложения AmneziaVPN/AmneziaWG и учёт трафика с сессиями. JSON для
//! панели совпадает с роутерным `detour-server status`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::store::{now_epoch, Store};

pub const CONF: &str = "server/server.json";
pub const CLIENTS: &str = "server/clients.json";
pub const TRAFFIC: &str = "server/traffic.json";
pub const SESSIONS: &str = "server/sessions.jsonl";
pub const UAPI: &str = "run/awgsrv.uapi";
pub const STATUS: &str = "run/awgsrv.status";
pub const LOG: &str = "logs/awgsrv.log";
pub const VCONF: &str = "run/vless.json";
pub const VLOG: &str = "logs/vless.log";
/// clash API экземпляра VLESS — по нему видно, чьи соединения открыты и сколько
/// через них прошло.
pub const VCLASH_PORT: u16 = 19486;
pub const VSNI_DEFAULT: &str = "www.microsoft.com";
/// Вход sing-box для соединений клиентов сервера.
pub const SOCKS_PORT: u16 = 19485;
pub const SOCKS_USER: &str = "server";
pub const ONLINE_SEC: i64 = 180;
const SESS_KEEP: usize = 500;

#[cfg(windows)]
pub const EXE: &str = "detour-awgsrv.exe";
#[cfg(not(windows))]
pub const EXE: &str = "detour-awgsrv";

/// Сервер на устройстве возможен только там, где служба держит процесс:
/// Android и iOS сами сидят в VpnService/NetworkExtension.
pub const SUPPORTED: bool = cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux"));

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Conf {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub port: u16,
    /// Первые три октета /24: «10.66.0».
    #[serde(default)]
    pub net: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub dns: String,
    #[serde(default)]
    pub mtu: u16,
    #[serde(default)]
    pub privkey: String,
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub socks_pass: String,
    #[serde(default)]
    pub obf: Obf,
    /// Вход AmneziaWG (по умолчанию включён — так было до VLESS).
    #[serde(default = "yes")]
    pub awg: bool,
    /// Вход VLESS-Reality: отдельный sing-box, ключи Reality, TCP-порт.
    #[serde(default)]
    pub vless: bool,
    #[serde(default)]
    pub vport: u16,
    #[serde(default)]
    pub vsni: String,
    #[serde(default)]
    pub vpriv: String,
    #[serde(default)]
    pub vpub: String,
    #[serde(default)]
    pub vsid: String,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Obf {
    pub jc: u32,
    pub jmin: u32,
    pub jmax: u32,
    pub s1: u32,
    pub s2: u32,
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub h4: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub octet: u8,
    pub pubkey: String,
    pub privkey: String,
    pub psk: String,
    #[serde(default = "full")]
    pub mode: String,
    pub created: i64,
    /// Пользователь VLESS; у клиентов, заведённых до VLESS, выдаётся при старте.
    #[serde(default)]
    pub uuid: String,
    /// Маршрут клиента: "" — общие правила, "direct", "vpn:<профиль|цепочка>".
    #[serde(default)]
    pub route: String,
}

/// Логин клиента во входе `server-in`: по нему движок узнаёт клиента
/// (`auth_user`). AWG-помощник шлёт его сам (`-socks-per-client`), VLESS —
/// в своём выходе `u-<id>`.
pub fn socks_user(octet: u8) -> String {
    format!("{SOCKS_USER}-{octet}")
}

fn full() -> String {
    "full".into()
}

pub fn load(store: &Store) -> Conf {
    store
        .read_json(CONF)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| Conf { awg: true, ..Default::default() })
}

pub fn save(store: &Store, c: &Conf) -> Result<()> {
    std::fs::create_dir_all(store.path("server"))?;
    store.write_json(CONF, &serde_json::to_value(c)?)?;
    Ok(())
}

pub fn clients(store: &Store) -> Vec<Client> {
    store.read_json(CLIENTS).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub fn save_clients(store: &Store, list: &[Client]) -> Result<()> {
    std::fs::create_dir_all(store.path("server"))?;
    store.write_json(CLIENTS, &serde_json::to_value(list)?)?;
    Ok(())
}

pub fn rand_u32() -> u32 {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    u32::from_le_bytes(b)
}

fn range(lo: u32, hi: u32) -> u32 {
    lo + rand_u32() % (hi - lo + 1)
}

/// Параметры обфускации AWG 1.0 — их понимают все версии приложений Amnezia.
pub fn gen_obf() -> Obf {
    let s1 = range(15, 120);
    let mut s2 = range(15, 120);
    // S1 + 56 == S2 делает init и response одной длины — запрещено протоколом.
    while s2 == s1 || s1 + 56 == s2 {
        s2 = range(15, 120);
    }
    let mut h: Vec<u32> = Vec::new();
    while h.len() < 4 {
        let v = range(5, 2_147_483_647);
        if !h.contains(&v) {
            h.push(v);
        }
    }
    Obf { jc: range(4, 10), jmin: range(40, 80), jmax: range(300, 800), s1, s2, h1: h[0], h2: h[1], h3: h[2], h4: h[3] }
}

/// Помощник `detour-awgsrv`: из каталога данных (обновлённый) или рядом со службой.
pub fn binary(data: &Path) -> PathBuf {
    let local = data.join("bin").join(EXE);
    if local.is_file() {
        return local;
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(EXE)))
        .filter(|p| p.is_file())
        .unwrap_or(local)
}

fn helper(data: &Path, cmd: &str, input: Option<&str>) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut c = Command::new(binary(data));
    c.arg(cmd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let mut child = c.spawn().context("detour-awgsrv не запустился")?;
    if let Some(s) = input {
        child.stdin.take().ok_or_else(|| anyhow!("stdin"))?.write_all(s.as_bytes())?;
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || s.len() != 44 {
        bail!("detour-awgsrv {cmd}: не удалось");
    }
    Ok(s)
}

pub fn genkey(data: &Path) -> Result<String> {
    helper(data, "genkey", None)
}
pub fn pubkey(data: &Path, private: &str) -> Result<String> {
    helper(data, "pubkey", Some(private))
}
pub fn genpsk(data: &Path) -> Result<String> {
    helper(data, "genpsk", None)
}

fn hex(b64: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map(|b| b.iter().map(|x| format!("{x:02x}")).collect())
        .unwrap_or_default()
}

pub fn server_ip(c: &Conf) -> String {
    format!("{}.1", c.net)
}

/// UAPI-конфиг устройства для помощника: ключи в hex, только включённые клиенты.
pub fn uapi(c: &Conf, list: &[Client]) -> String {
    let o = &c.obf;
    let mut s = format!(
        "private_key={}\nlisten_port={}\njc={}\njmin={}\njmax={}\ns1={}\ns2={}\nh1={}\nh2={}\nh3={}\nh4={}\nreplace_peers=true\n",
        hex(&c.privkey), c.port, o.jc, o.jmin, o.jmax, o.s1, o.s2, o.h1, o.h2, o.h3, o.h4
    );
    for cl in list.iter().filter(|x| x.enabled && !x.pubkey.is_empty()) {
        s.push_str(&format!(
            "public_key={}\npreshared_key={}\nreplace_allowed_ips=true\nallowed_ip={}.{}/32\n",
            hex(&cl.pubkey),
            hex(&cl.psk),
            c.net,
            cl.octet
        ));
    }
    s
}

/// `.conf` для приложения. lan_cidr — сеть компьютера для режима «только дом».
pub fn client_conf(c: &Conf, cl: &Client, host: &str, lan_cidr: Option<&str>) -> String {
    let allowed = if cl.mode == "lan" {
        let mut a = format!("{}.0/24", c.net);
        if let Some(l) = lan_cidr {
            a.push_str(&format!(", {l}"));
        }
        a
    } else {
        // ::/0 — чтобы IPv6 клиента не ушёл мимо туннеля (сервер его не везёт).
        "0.0.0.0/0, ::/0".into()
    };
    let dns = if c.dns.is_empty() { server_ip(c) } else { c.dns.clone() };
    let o = &c.obf;
    format!(
        "[Interface]\nPrivateKey = {}\nAddress = {}.{}/32\nDNS = {}\nMTU = {}\nJc = {}\nJmin = {}\nJmax = {}\nS1 = {}\nS2 = {}\nH1 = {}\nH2 = {}\nH3 = {}\nH4 = {}\n\n[Peer]\nPublicKey = {}\nPresharedKey = {}\nAllowedIPs = {}\nEndpoint = {}:{}\nPersistentKeepalive = 25\n",
        cl.privkey, c.net, cl.octet, dns, c.mtu, o.jc, o.jmin, o.jmax, o.s1, o.s2, o.h1, o.h2, o.h3, o.h4,
        c.pubkey, cl.psk, allowed, host, c.port
    )
}

/// Пир из файла состояния помощника (формат IpcGet).
#[derive(Clone, Debug, Default)]
pub struct PeerState {
    pub handshake: i64,
    pub rx: u64,
    pub tx: u64,
    pub endpoint: String,
}

/// public_key (hex) → состояние.
pub fn parse_status(text: &str) -> BTreeMap<String, PeerState> {
    let mut out = BTreeMap::new();
    let mut cur: Option<(String, PeerState)> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "public_key" => {
                if let Some((key, st)) = cur.take() {
                    out.insert(key, st);
                }
                cur = Some((v.to_owned(), PeerState::default()));
            }
            "last_handshake_time_sec" => {
                if let Some((_, st)) = cur.as_mut() {
                    st.handshake = v.parse().unwrap_or(0);
                }
            }
            "rx_bytes" => {
                if let Some((_, st)) = cur.as_mut() {
                    st.rx = v.parse().unwrap_or(0);
                }
            }
            "tx_bytes" => {
                if let Some((_, st)) = cur.as_mut() {
                    st.tx = v.parse().unwrap_or(0);
                }
            }
            "endpoint" => {
                if let Some((_, st)) = cur.as_mut() {
                    st.endpoint = v.to_owned();
                }
            }
            _ => {}
        }
    }
    if let Some((key, st)) = cur {
        out.insert(key, st);
    }
    out
}

pub fn status_of<'a>(states: &'a BTreeMap<String, PeerState>, cl: &Client) -> Option<&'a PeerState> {
    states.get(&hex(&cl.pubkey))
}

/// Накопленное по клиенту: rx — от клиента, tx — к клиенту (как на роутере).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Traffic {
    pub lrx: u64,
    pub ltx: u64,
    pub trx: u64,
    pub ttx: u64,
    pub mon: String,
    pub mrx: u64,
    pub mtx: u64,
    pub day: String,
    pub drx: u64,
    pub dtx: u64,
    pub online: bool,
    pub sstart: i64,
    pub srx: u64,
    pub stx: u64,
    pub seen: i64,
    pub remote: String,
    /// Последние показания счётчиков VLESS (сбрасываются отдельно от AWG).
    #[serde(default)]
    pub vlrx: u64,
    #[serde(default)]
    pub vltx: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub start: i64,
    pub end: i64,
    pub rx: u64,
    pub tx: u64,
    pub remote: String,
}

/// Дни от эпохи → (год, месяц, день), алгоритм Howard Hinnant. Сутки — по UTC.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn day_month(now: i64) -> (String, String) {
    let (y, m, d) = civil(now.div_euclid(86_400));
    (format!("{y:04}{m:02}{d:02}"), format!("{y:04}{m:02}"))
}

fn delta(cur: u64, last: u64) -> u64 {
    if cur < last {
        cur
    } else {
        cur - last
    }
}

/// Один шаг учёта: счётчики помощника → накопленное и закрытые сессии.
/// Сброс счётчиков (перезапуск помощника) распознаётся по уменьшению.
/// Оба входа одного клиента: AWG по ключу, VLESS по id. Дельты считаются
/// раздельно (счётчики сбрасываются независимо), онлайн — по свежему из двух.
fn merged(
    cl: &Client,
    states: &BTreeMap<String, PeerState>,
    vstates: &BTreeMap<String, PeerState>,
    t: &Traffic,
) -> (PeerState, PeerState, u64, u64) {
    let a = status_of(states, cl).cloned().unwrap_or_default();
    let v = vstates.get(&cl.id).cloned().unwrap_or_default();
    let drx = delta(a.rx, t.lrx) + delta(v.rx, t.vlrx);
    let dtx = delta(a.tx, t.ltx) + delta(v.tx, t.vltx);
    let mut st = a.clone();
    if v.handshake > st.handshake {
        st.handshake = v.handshake;
        st.endpoint = v.endpoint.clone();
    }
    st.rx = a.rx + v.rx;
    st.tx = a.tx + v.tx;
    (st, v, drx, dtx)
}

pub fn account(
    list: &[Client],
    states: &BTreeMap<String, PeerState>,
    vstates: &BTreeMap<String, PeerState>,
    prev: &BTreeMap<String, Traffic>,
    now: i64,
) -> (BTreeMap<String, Traffic>, Vec<Session>) {
    let (day, mon) = day_month(now);
    let mut out = BTreeMap::new();
    let mut closed = Vec::new();
    for cl in list {
        let mut t = prev.get(&cl.id).cloned().unwrap_or_default();
        let a = status_of(states, cl).cloned().unwrap_or_default();
        let (st, v, drx, dtx) = merged(cl, states, vstates, &t);
        t.lrx = a.rx;
        t.ltx = a.tx;
        t.vlrx = v.rx;
        t.vltx = v.tx;
        t.trx += drx;
        t.ttx += dtx;
        if t.mon != mon {
            t.mon = mon.clone();
            t.mrx = 0;
            t.mtx = 0;
        }
        t.mrx += drx;
        t.mtx += dtx;
        if t.day != day {
            t.day = day.clone();
            t.drx = 0;
            t.dtx = 0;
        }
        t.drx += drx;
        t.dtx += dtx;
        // Handshake обновляется раз в 2 минуты, keepalive — раз в 25 с: рост
        // счётчиков точнее отвечает на «когда был в сети».
        if st.handshake > t.seen {
            t.seen = st.handshake;
        }
        if drx + dtx > 0 && now > t.seen {
            t.seen = now;
        }
        if !st.endpoint.is_empty() && st.endpoint != "(none)" {
            t.remote = st.endpoint.clone();
        }
        let online = st.handshake > 0 && now - st.handshake < ONLINE_SEC;
        if online {
            if !t.online {
                t.sstart = st.handshake;
                t.srx = 0;
                t.stx = 0;
            }
            t.srx += drx;
            t.stx += dtx;
        } else if t.online {
            closed.push(Session { id: cl.id.clone(), start: t.sstart, end: t.seen, rx: t.srx, tx: t.stx, remote: t.remote.clone() });
            t.srx = 0;
            t.stx = 0;
        }
        t.online = online;
        out.insert(cl.id.clone(), t);
    }
    (out, closed)
}

pub fn load_traffic(store: &Store) -> BTreeMap<String, Traffic> {
    store.read_json(TRAFFIC).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub fn append_sessions(store: &Store, closed: &[Session]) -> Result<()> {
    if closed.is_empty() {
        return Ok(());
    }
    let mut lines: Vec<String> = store.read_text(SESSIONS).lines().map(str::to_owned).collect();
    for s in closed {
        lines.push(serde_json::to_string(s)?);
    }
    let skip = lines.len().saturating_sub(SESS_KEEP);
    store.write_text(SESSIONS, &(lines[skip..].join("\n") + "\n"))?;
    Ok(())
}

pub fn sessions(store: &Store, id: &str) -> Vec<Session> {
    let mut v: Vec<Session> = store
        .read_text(SESSIONS)
        .lines()
        .filter_map(|l| serde_json::from_str::<Session>(l).ok())
        .filter(|s| id.is_empty() || s.id == id)
        .collect();
    v.reverse();
    v.truncate(100);
    v
}

/// Клиент в JSON для панели — тот же набор полей, что у роутера.
pub fn client_json(
    c: &Conf,
    cl: &Client,
    states: &BTreeMap<String, PeerState>,
    vstates: &BTreeMap<String, PeerState>,
    t: Option<&Traffic>,
    now: i64,
) -> Value {
    let t = t.cloned().unwrap_or_default();
    let (day, mon) = day_month(now);
    let (st, _, drx, dtx) = merged(cl, states, vstates, &t);
    let online = st.handshake > 0 && now - st.handshake < ONLINE_SEC;
    let (sst, srx, stx) = if online {
        if t.online {
            (t.sstart, t.srx + drx, t.stx + dtx)
        } else {
            (st.handshake, drx, dtx)
        }
    } else {
        (0, 0, 0)
    };
    let remote = if st.endpoint.is_empty() || st.endpoint == "(none)" { t.remote.clone() } else { st.endpoint.clone() };
    json!({
        "id": cl.id, "enabled": cl.enabled, "name": cl.name, "ip": format!("{}.{}", c.net, cl.octet),
        "mode": cl.mode, "created": cl.created, "awg": !cl.pubkey.is_empty(), "vless": !cl.uuid.is_empty(), "route": cl.route,
        "online": online, "handshake": st.handshake,
        "handshake_ago": if st.handshake > 0 { now - st.handshake } else { -1 },
        "last_seen": st.handshake.max(t.seen), "rx": st.rx, "tx": st.tx, "remote": remote,
        "session_start": sst, "session_rx": srx, "session_tx": stx,
        "day_rx": (if t.day == day { t.drx } else { 0 }) + drx, "day_tx": (if t.day == day { t.dtx } else { 0 }) + dtx,
        "month_rx": (if t.mon == mon { t.mrx } else { 0 }) + drx, "month_tx": (if t.mon == mon { t.mtx } else { 0 }) + dtx,
        "total_rx": t.trx + drx, "total_tx": t.ttx + dtx,
    })
}

/// Конфиг sing-box входа VLESS-Reality. Пользователь = id клиента; у каждого
/// свой SOCKS-выход в `server-in` движка — его соединения идут по тем же
/// правилам, что у AWG-клиентов, а clash API этого экземпляра показывает, чьё
/// соединение (по тегу выхода `u-<id>`). «Только дом» режется здесь же: всё,
/// кроме сети компьютера, — отказ.
pub fn vless_config(c: &Conf, list: &[Client], lan_cidr: Option<&str>, clash_secret: &str) -> Value {
    let on: Vec<&Client> = list.iter().filter(|x| x.enabled && !x.uuid.is_empty()).collect();
    let users: Vec<Value> = on.iter().map(|x| json!({ "name": x.id, "uuid": x.uuid, "flow": "xtls-rprx-vision" })).collect();
    let socks = |tag: String, user: String| {
        json!({
            "type": "socks", "tag": tag, "server": "127.0.0.1", "server_port": SOCKS_PORT, "version": "5",
            "username": user, "password": c.socks_pass,
        })
    };
    let mut outbounds = vec![socks("fallback".into(), SOCKS_USER.into())];
    let mut rules = vec![json!({ "action": "sniff" })];
    for x in &on {
        outbounds.push(socks(format!("u-{}", x.id), socks_user(x.octet)));
        if x.mode == "lan" {
            let mut cidr = vec![json!(format!("{}.0/24", c.net))];
            if let Some(l) = lan_cidr {
                cidr.push(json!(l));
            }
            rules.push(json!({ "auth_user": [x.id], "ip_cidr": cidr, "outbound": format!("u-{}", x.id) }));
            rules.push(json!({ "auth_user": [x.id], "action": "reject" }));
        } else {
            rules.push(json!({ "auth_user": [x.id], "outbound": format!("u-{}", x.id) }));
        }
    }
    json!({
        "log": { "level": "warn", "timestamp": true },
        "inbounds": [{
            "type": "vless", "tag": "in", "listen": "0.0.0.0", "listen_port": c.vport, "users": users,
            "tls": {
                "enabled": true, "server_name": c.vsni,
                "reality": {
                    "enabled": true,
                    "handshake": { "server": c.vsni, "server_port": 443, "detour": "fallback" },
                    "private_key": c.vpriv, "short_id": [c.vsid],
                },
            },
        }],
        "outbounds": outbounds,
        "route": { "rules": rules, "final": "fallback" },
        "experimental": { "clash_api": { "external_controller": format!("127.0.0.1:{VCLASH_PORT}"), "secret": clash_secret } },
    })
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Ссылка vless:// для v2rayNG/Hiddify/Streisand и самого Detour.
pub fn client_link(c: &Conf, cl: &Client, host: &str) -> String {
    format!(
        "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
        cl.uuid, host, c.vport, c.vsni, c.vpub, c.vsid, urlenc(&cl.name)
    )
}

pub fn new_uuid() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Соединения экземпляра VLESS из clash API → (id клиента, адрес, up, down).
pub fn parse_vless_conns(v: &Value) -> Vec<(String, String, String, u64, u64)> {
    let mut out = Vec::new();
    for x in v.get("connections").and_then(Value::as_array).into_iter().flatten() {
        let Some(user) = x
            .get("chains")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find_map(|t| t.strip_prefix("u-"))
        else {
            continue;
        };
        let m = x.get("metadata").cloned().unwrap_or_default();
        let ip = m.get("sourceIP").and_then(Value::as_str).unwrap_or("");
        let port = m.get("sourcePort").map(|p| p.as_str().map(str::to_owned).unwrap_or_else(|| p.to_string())).unwrap_or_default();
        let remote = if ip.is_empty() { String::new() } else { format!("{ip}:{port}") };
        let id = x.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let up = x.get("upload").and_then(Value::as_u64).unwrap_or(0);
        let down = x.get("download").and_then(Value::as_u64).unwrap_or(0);
        out.push((id, user.to_owned(), remote, up, down));
    }
    out
}

/// Накопитель счётчиков VLESS по клиентам: clash API отдаёт байты только
/// открытых соединений, поэтому опрос раз в несколько секунд складывает
/// приращения, а закрытое соединение теряет лишь хвост с последнего опроса.
#[derive(Default)]
pub struct VlessMeter {
    conns: BTreeMap<String, (u64, u64)>,
    pub users: BTreeMap<String, PeerState>,
}

impl VlessMeter {
    pub fn reset(&mut self) {
        self.conns.clear();
        self.users.clear();
    }

    pub fn feed(&mut self, conns: &[(String, String, String, u64, u64)], now: i64) {
        let mut seen = BTreeMap::new();
        for (cid, user, remote, up, down) in conns {
            let (pu, pd) = self.conns.get(cid).copied().unwrap_or((0, 0));
            let st = self.users.entry(user.clone()).or_default();
            st.rx += up.saturating_sub(pu);
            st.tx += down.saturating_sub(pd);
            st.handshake = now;
            if !remote.is_empty() {
                st.endpoint = remote.clone();
            }
            seen.insert(cid.clone(), (*up, *down));
        }
        self.conns = seen;
    }
}

pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && !matches!(c, '|' | '"' | '\\')).take(48).collect::<String>().trim().to_owned()
}

pub fn new_id() -> String {
    format!("{:08x}", rand_u32())
}

pub fn now() -> i64 {
    now_epoch()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cl(id: &str, key: &str) -> Client {
        Client { id: id.into(), enabled: true, name: id.into(), octet: 2, pubkey: key.into(), privkey: "p".into(), psk: "s".into(), mode: "full".into(), created: 0, uuid: String::new(), route: String::new() }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(day_month(0), ("19700101".into(), "197001".into()));
        assert_eq!(day_month(1_791_518_000), ("20261009".into(), "202610".into()));
    }

    #[test]
    fn obf_rules() {
        for _ in 0..200 {
            let o = gen_obf();
            assert!(o.s1 != o.s2 && o.s1 + 56 != o.s2);
            let h = [o.h1, o.h2, o.h3, o.h4];
            assert!(h.iter().all(|x| *x >= 5));
            assert!(h[0] != h[1] && h[1] != h[2] && h[2] != h[3] && h[0] != h[3]);
        }
    }

    #[test]
    fn status_and_sessions() {
        let key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let list = vec![cl("a", &key)];
        let st = parse_status(&format!(
            "listen_port=1\npublic_key={}\nendpoint=1.2.3.4:5\nlast_handshake_time_sec=1000\nrx_bytes=100\ntx_bytes=900\n",
            "07".repeat(32)
        ));
        let (t1, closed) = account(&list, &st, &BTreeMap::new(), &BTreeMap::new(), 1010);
        assert!(closed.is_empty());
        let a = &t1["a"];
        assert!(a.online && a.sstart == 1000 && a.trx == 100 && a.ttx == 900 && a.remote == "1.2.3.4:5");
        // помощник перезапустился — счётчики меньше прежних
        let st2 = parse_status(&format!("public_key={}\nlast_handshake_time_sec=1000\nrx_bytes=50\ntx_bytes=60\n", "07".repeat(32)));
        let (t2, _) = account(&list, &st2, &BTreeMap::new(), &t1, 1100);
        assert_eq!(t2["a"].trx, 150);
        // 3 минуты без handshake — сессия закрыта
        let (t3, closed) = account(&list, &st2, &BTreeMap::new(), &t2, 1000 + ONLINE_SEC + 5);
        assert!(!t3["a"].online);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].rx, 150);
    }

    #[test]
    fn uapi_has_only_enabled_peers() {
        let key = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
        let c = Conf { port: 5, net: "10.66.0".into(), privkey: key.clone(), obf: gen_obf(), ..Default::default() };
        let mut off = cl("b", &key);
        off.enabled = false;
        let u = uapi(&c, &[cl("a", &key), off]);
        assert_eq!(u.matches("public_key=").count(), 1);
        assert!(u.contains("allowed_ip=10.66.0.2/32"));
        assert!(u.contains(&"01".repeat(32)));
    }

    #[test]
    fn vless_link_and_config() {
        let c = Conf { net: "10.66.0".into(), vport: 443, vsni: "www.example.com".into(), vpub: "PUB".into(), vsid: "ab12".into(), socks_pass: "pw".into(), ..Default::default() };
        let mut a = cl("a", "");
        a.uuid = new_uuid();
        a.name = "Тест 1".into();
        let mut b = cl("b", "");
        b.uuid = new_uuid();
        b.mode = "lan".into();
        let l = client_link(&c, &a, "203.0.113.5");
        assert!(l.starts_with(&format!("vless://{}@203.0.113.5:443?", a.uuid)));
        assert!(l.contains("pbk=PUB&sid=ab12") && l.ends_with("#%D0%A2%D0%B5%D1%81%D1%82%201"));
        assert_eq!(a.uuid.len(), 36);
        let v = vless_config(&c, &[a, b], Some("192.168.1.0/24"), "s");
        assert_eq!(v["inbounds"][0]["users"].as_array().unwrap().len(), 2);
        let rules = v["route"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| r["auth_user"] == json!(["b"]) && r["action"] == "reject"));
        assert!(rules.iter().any(|r| r["auth_user"] == json!(["a"]) && r["outbound"] == "u-a"));
        assert_eq!(v["outbounds"][1]["password"], "pw");
        assert_eq!(v["outbounds"][1]["username"], "server-2");
    }

    #[test]
    fn vless_meter_accumulates_across_closed_conns() {
        let mut m = VlessMeter::default();
        let c = |id: &str, up, down| (id.to_owned(), "a".to_owned(), "1.2.3.4:5".to_owned(), up, down);
        m.feed(&[c("x", 10, 100)], 1);
        m.feed(&[c("x", 15, 300), c("y", 1, 1)], 2);
        m.feed(&[c("y", 2, 5)], 3);
        let st = &m.users["a"];
        assert_eq!((st.rx, st.tx, st.handshake), (17, 305, 3));
        let v = json!({ "connections": [{ "id": "q", "upload": 3, "download": 4, "chains": ["u-zz"], "metadata": { "sourceIP": "9.9.9.9", "sourcePort": "77" } }] });
        assert_eq!(parse_vless_conns(&v), vec![("q".into(), "zz".into(), "9.9.9.9:77".into(), 3, 4)]);
    }
}
