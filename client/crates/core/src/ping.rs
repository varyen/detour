//! Пинг серверов профилей — порт `detour-ping`: ICMP (с одним повтором),
//! не ответил — TCP-рукопожатие до `server_port`; у UDP-протоколов TCP-фолбэка
//! нет. При поднятом TUN запрос обязан уйти с физического интерфейса: иначе
//! «ответит» сам туннель и пинг покажет ерунду.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::now_epoch;

const NO_TCP: [&str; 4] = ["wireguard", "hysteria", "hysteria2", "tuic"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub rtt: i64,
    pub ok: bool,
    pub ts: i64,
    pub server: String,
    pub method: String,
}

/// Адрес и порт сервера; у WireGuard в endpoint-формате — первый пир.
pub fn endpoint(ob: &Value) -> Option<(String, u16)> {
    let port = |v: Option<&Value>| v.and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok());
    if let Some(s) = ob.get("server").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return Some((s.to_owned(), port(ob.get("server_port")).unwrap_or(443)));
    }
    let peer = ob.get("peers")?.as_array()?.first()?;
    Some((peer.get("address")?.as_str()?.to_owned(), port(peer.get("port")).unwrap_or(51820)))
}

/// Адрес, с которого система ходит наружу. При поднятом TUN это адрес
/// туннеля — тогда `None`, и вызывающий берёт запомненный до старта.
pub fn physical_ipv4() -> Option<Ipv4Addr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:53").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(a) if !(a.octets()[0] == 172 && a.octets()[1] == 19) && !a.is_unspecified() => Some(a),
        _ => None,
    }
}

async fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await.ok()?.collect();
    addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).copied()
}

#[cfg(windows)]
fn icmp_blocking(src: Option<Ipv4Addr>, dst: Ipv4Addr) -> Option<i64> {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex, ICMP_ECHO_REPLY,
    };
    let data = [0x44u8; 32];
    let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 64];
    unsafe {
        let h = IcmpCreateFile();
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return None;
        }
        let n = IcmpSendEcho2Ex(
            h,
            std::ptr::null_mut(),
            None,
            std::ptr::null(),
            src.map(|a| u32::from_ne_bytes(a.octets())).unwrap_or(0),
            u32::from_ne_bytes(dst.octets()),
            data.as_ptr() as *const c_void,
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr() as *mut c_void,
            reply.len() as u32,
            2000,
        );
        IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime as i64)
    }
}

#[cfg(not(windows))]
fn icmp_blocking(_src: Option<Ipv4Addr>, _dst: Ipv4Addr) -> Option<i64> {
    None
}

async fn icmp(src: Option<Ipv4Addr>, dst: Ipv4Addr) -> Option<i64> {
    for _ in 0..2 {
        if let Ok(Some(ms)) = tokio::task::spawn_blocking(move || icmp_blocking(src, dst)).await {
            return Some(ms.max(1));
        }
    }
    None
}

async fn tcp(src: Option<Ipv4Addr>, dst: SocketAddr) -> Option<i64> {
    let sock = match dst {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4().ok()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6().ok()?,
    };
    if let (Some(s), SocketAddr::V4(_)) = (src, dst) {
        sock.bind(SocketAddr::new(IpAddr::V4(s), 0)).ok()?;
    }
    let start = Instant::now();
    tokio::time::timeout(Duration::from_secs(3), sock.connect(dst)).await.ok()?.ok()?;
    Some((start.elapsed().as_millis() as i64).max(1))
}

pub async fn ping(ob: &Value, src: Option<Ipv4Addr>) -> Ping {
    let now = now_epoch();
    let Some((host, port)) = endpoint(ob) else {
        return Ping { rtt: -1, ok: false, ts: now, server: String::new(), method: "none".into() };
    };
    let fail = |method: &str| Ping { rtt: -1, ok: false, ts: now, server: host.clone(), method: method.into() };
    let Some(addr) = resolve(&host, port).await else { return fail("none") };
    // Loopback с чужим адресом отправителя недостижим, а в туннель он и так не уходит.
    let src = if addr.ip().is_loopback() { None } else { src };
    if let IpAddr::V4(v4) = addr.ip() {
        if let Some(ms) = icmp(src, v4).await {
            return Ping { rtt: ms, ok: true, ts: now, server: host.clone(), method: "icmp".into() };
        }
    }
    let kind = ob.get("type").and_then(Value::as_str).unwrap_or("");
    if NO_TCP.contains(&kind) {
        return fail("icmp");
    }
    match tcp(src, addr).await {
        Some(ms) => Ping { rtt: ms, ok: true, ts: now, server: host.clone(), method: "tcp".into() },
        None => fail("tcp"),
    }
}
