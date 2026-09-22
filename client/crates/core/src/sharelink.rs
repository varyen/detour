//! Разбор share-ссылок — порт `panel/src/components/profiles/uri.ts`
//! (`parseShareLink` + `outboundFromDraft`). Поведение обязано совпадать:
//! эталоны в `tests/sharelinks.json` генерирует сам uri.ts
//! (`client/scripts/gen-sharelink-fixtures.mjs`).

use base64::Engine as _;
use serde_json::{json, Map, Value};

pub struct Parsed {
    pub name: String,
    pub server: String,
    pub outbound: Value,
    /// Транспорт, который просила ссылка (`type=`/`net=`), до сведения к
    /// поддерживаемым: xhttp и подобные uri.ts молча превращает в TCP.
    pub requested_transport: String,
}

#[derive(Default)]
struct Draft {
    kind: &'static str,
    server: String,
    port: String,
    uuid: String,
    password: String,
    username: String,
    method: String,
    flow: String,
    alter_id: String,
    tls: bool,
    sni: String,
    alpn: String,
    fingerprint: String,
    insecure: bool,
    reality_key: String,
    reality_short_id: String,
    transport: &'static str,
    path: String,
    host: String,
    service_name: String,
    obfs_password: String,
    congestion: String,
}

const NO_UTLS: [&str; 2] = ["hysteria2", "tuic"];

/// Как `atob`: ненулевые хвостовые биты не ошибка — поставщики подписок
/// такие строки выдают, и браузер их принимает.
const LENIENT_B64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

pub fn b64decode_bytes(raw: &str) -> Option<Vec<u8>> {
    let t: String = raw
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    LENIENT_B64.decode(t.trim_end_matches('=').as_bytes()).ok()
}

fn b64decode(raw: &str) -> Option<String> {
    let bytes = b64decode_bytes(raw)?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// `decodeURIComponent`, а при битой последовательности — строка как есть.
fn safe_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
            match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 3;
                    continue;
                }
                None => return s.to_owned(),
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

struct UriParts {
    userinfo: String,
    host: String,
    port: String,
    params: Vec<(String, String)>,
    label: String,
}

fn param<'a>(p: &'a [(String, String)], k: &str) -> &'a str {
    p.iter().rev().find(|(key, _)| key == k).map(|(_, v)| v.as_str()).unwrap_or("")
}

fn split_uri(rest: &str) -> UriParts {
    let mut body = rest;
    let mut label = String::new();
    if let Some(h) = body.rfind('#') {
        label = safe_decode(&body[h + 1..]);
        body = &body[..h];
    }
    let mut params = Vec::new();
    if let Some(q) = body.find('?') {
        for pair in body[q + 1..].split('&').filter(|p| !p.is_empty()) {
            match pair.find('=') {
                Some(eq) => params.push((safe_decode(&pair[..eq]), safe_decode(&pair[eq + 1..]))),
                None => params.push((safe_decode(pair), String::new())),
            }
        }
        body = &body[..q];
    }
    let (userinfo, host_port) = match body.rfind('@') {
        Some(at) => (body[..at].to_owned(), &body[at + 1..]),
        None => (String::new(), body),
    };
    let (host, port) = if let Some(v6) = host_port.strip_prefix('[') {
        let close = v6.find(']').unwrap_or(v6.len());
        let after = &v6[(close + 1).min(v6.len())..];
        (v6[..close].to_owned(), after.strip_prefix(':').unwrap_or("").to_owned())
    } else {
        match host_port.rfind(':') {
            Some(c) => (host_port[..c].to_owned(), host_port[c + 1..].to_owned()),
            None => (host_port.to_owned(), String::new()),
        }
    };
    let port = port.split('/').next().unwrap_or("").to_owned();
    UriParts { userinfo, host, port, params, label }
}

fn split_alpn(s: &str) -> String {
    s.split(',').collect::<Vec<_>>().join(", ")
}

fn apply_tls(d: &mut Draft, p: &[(String, String)]) {
    let security = param(p, "security").to_ascii_lowercase();
    d.tls = matches!(security.as_str(), "tls" | "reality" | "xtls");
    let sni = param(p, "sni");
    let sni = if sni.is_empty() { param(p, "peer") } else { sni };
    if !sni.is_empty() {
        d.sni = sni.into();
    }
    if !param(p, "alpn").is_empty() {
        d.alpn = split_alpn(param(p, "alpn"));
    }
    if !param(p, "fp").is_empty() && !NO_UTLS.contains(&d.kind) {
        d.fingerprint = param(p, "fp").into();
    }
    if param(p, "allowInsecure") == "1" || param(p, "insecure") == "1" {
        d.insecure = true;
    }
    if security == "reality" {
        d.tls = true;
        d.reality_key = param(p, "pbk").into();
        d.reality_short_id = param(p, "sid").into();
    }
}

fn apply_transport(d: &mut Draft, p: &[(String, String)]) -> String {
    let tr = [param(p, "type"), param(p, "network")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    match tr.as_str() {
        "ws" => {
            d.transport = "ws";
            d.path = param(p, "path").into();
            d.host = param(p, "host").into();
        }
        "grpc" => {
            d.transport = "grpc";
            let sn = param(p, "serviceName");
            d.service_name = if sn.is_empty() { param(p, "servicename") } else { sn }.into();
        }
        "h2" | "http" => {
            d.transport = "http";
            d.path = param(p, "path").into();
            d.host = param(p, "host").into();
        }
        _ => {}
    }
    tr
}

fn num(s: &str) -> Option<u64> {
    s.trim().parse::<u64>().ok().filter(|n| *n > 0)
}

fn list(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

fn userpass(userinfo: &str) -> (String, String) {
    match userinfo.find(':') {
        Some(c) => (safe_decode(&userinfo[..c]), safe_decode(&userinfo[c + 1..])),
        None => (String::new(), String::new()),
    }
}

/// Одна ссылка. `None` — схема не распознана или ссылка битая.
pub fn parse(raw: &str) -> Option<Parsed> {
    let line = raw.trim();
    let sep = line.find("://")?;
    let scheme = line[..sep].to_ascii_lowercase();
    if scheme.is_empty() {
        return None;
    }
    let rest = &line[sep + 3..];
    let mut d = Draft { kind: "vless", method: "aes-256-gcm".into(), tls: true, ..Default::default() };
    let name: String;
    let mut requested = String::new();

    if scheme == "vmess" {
        let text = b64decode(rest.split('#').next().unwrap_or(""))?;
        let j: Value = serde_json::from_str(&text).ok()?;
        let s = |k: &str| match j.get(k) {
            Some(Value::String(v)) => v.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        };
        let or = |a: String, b: String| if a.is_empty() { b } else { a };
        d.kind = "vmess";
        name = or(s("ps"), s("remarks"));
        d.server = or(s("add"), s("address"));
        d.port = or(s("port"), "443".into());
        d.uuid = s("id");
        d.alter_id = or(s("aid"), "0".into());
        d.tls = s("tls").to_ascii_lowercase() == "tls";
        d.sni = or(s("sni"), s("host"));
        if !s("alpn").is_empty() {
            d.alpn = split_alpn(&s("alpn"));
        }
        if !s("fp").is_empty() {
            d.fingerprint = s("fp");
        }
        let net = or(or(s("net"), s("network")), "tcp".into()).to_ascii_lowercase();
        requested = apply_transport(
            &mut d,
            &[
                ("type".into(), net),
                ("path".into(), s("path")),
                ("host".into(), s("host")),
                ("serviceName".into(), s("path")),
            ],
        );
    } else if scheme == "ss" {
        let mut body = rest.to_owned();
        let mut label = String::new();
        if let Some(h) = body.rfind('#') {
            label = safe_decode(&body[h + 1..]);
            body.truncate(h);
        }
        if let Some(q) = body.find('?') {
            body.truncate(q);
        }
        if !body.contains('@') {
            body = b64decode(&body)?;
        }
        let at = body.rfind('@')?;
        let mut userinfo = body[..at].to_owned();
        if !userinfo.contains(':') {
            if let Some(dec) = b64decode(&userinfo) {
                userinfo = dec;
            }
        }
        let host_port = &body[at + 1..];
        d.kind = "shadowsocks";
        d.tls = false;
        match host_port.rfind(':') {
            Some(c) => {
                d.server = host_port[..c].into();
                // `host:8388/?plugin=…` — хвост после порта не часть порта.
                d.port = host_port[c + 1..].split('/').next().unwrap_or("").into();
            }
            None => d.server = host_port.into(),
        }
        match userinfo.find(':') {
            Some(c) => {
                d.method = userinfo[..c].into();
                d.password = userinfo[c + 1..].into();
            }
            None => d.password = String::new(),
        }
        name = label;
    } else {
        let parts = split_uri(rest);
        let p = &parts.params;
        name = parts.label.clone();
        d.server = parts.host.clone();
        d.port = parts.port.clone();
        match scheme.as_str() {
            "vless" => {
                d.kind = "vless";
                d.uuid = parts.userinfo.clone();
                d.flow = param(p, "flow").into();
                apply_tls(&mut d, p);
                requested = apply_transport(&mut d, p);
            }
            "trojan" => {
                d.kind = "trojan";
                d.password = safe_decode(&parts.userinfo);
                apply_tls(&mut d, p);
                d.tls = true;
                requested = apply_transport(&mut d, p);
            }
            "hysteria2" | "hy2" => {
                d.kind = "hysteria2";
                d.password = safe_decode(&parts.userinfo);
                d.tls = true;
                let sni = param(p, "sni");
                d.sni = if sni.is_empty() { param(p, "peer") } else { sni }.into();
                d.alpn = param(p, "alpn").split(',').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
                d.insecure = param(p, "insecure") == "1";
                d.obfs_password = param(p, "obfs-password").into();
            }
            "tuic" => {
                d.kind = "tuic";
                match parts.userinfo.find(':') {
                    Some(c) => {
                        d.uuid = parts.userinfo[..c].into();
                        d.password = safe_decode(&parts.userinfo[c + 1..]);
                    }
                    None => d.uuid = parts.userinfo.clone(),
                }
                d.tls = true;
                d.sni = param(p, "sni").into();
                d.alpn = param(p, "alpn").split(',').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
                d.congestion = param(p, "congestion_control").into();
                d.insecure = param(p, "allow_insecure") == "1";
            }
            "socks" | "socks5" => {
                d.kind = "socks";
                d.tls = false;
                (d.username, d.password) = userpass(&parts.userinfo);
            }
            "http" | "https" => {
                d.kind = "http";
                d.tls = scheme == "https";
                (d.username, d.password) = userpass(&parts.userinfo);
            }
            _ => return None,
        }
        if d.port.is_empty() {
            d.port = "443".into();
        }
    }

    let server = d.server.trim().to_owned();
    Some(Parsed { name, server, outbound: outbound(&d), requested_transport: requested })
}

fn outbound(d: &Draft) -> Value {
    let mut o = Map::new();
    o.insert("type".into(), json!(d.kind));
    o.insert("tag".into(), json!("proxy"));
    o.insert("server".into(), json!(d.server.trim()));
    if let Some(p) = num(&d.port) {
        o.insert("server_port".into(), json!(p));
    }
    let nonempty = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
    match d.kind {
        "vless" => {
            o.insert("uuid".into(), json!(d.uuid.trim()));
            if let Some(f) = nonempty(&d.flow) {
                o.insert("flow".into(), json!(f));
            }
        }
        "vmess" => {
            o.insert("uuid".into(), json!(d.uuid.trim()));
            o.insert("security".into(), json!("auto"));
            o.insert("alter_id".into(), json!(d.alter_id.trim().parse::<i64>().unwrap_or(0)));
        }
        "trojan" => {
            o.insert("password".into(), json!(d.password));
        }
        "shadowsocks" => {
            o.insert("method".into(), json!(d.method));
            o.insert("password".into(), json!(d.password));
        }
        "hysteria2" => {
            o.insert("password".into(), json!(d.password));
            if let Some(ob) = nonempty(&d.obfs_password) {
                o.insert("obfs".into(), json!({ "type": "salamander", "password": ob }));
            }
        }
        "tuic" => {
            o.insert("uuid".into(), json!(d.uuid.trim()));
            o.insert("password".into(), json!(d.password));
            if let Some(c) = nonempty(&d.congestion) {
                o.insert("congestion_control".into(), json!(c));
            }
        }
        "socks" => {
            o.insert("version".into(), json!("5"));
            if let Some(u) = nonempty(&d.username) {
                o.insert("username".into(), json!(u));
            }
            if !d.password.is_empty() {
                o.insert("password".into(), json!(d.password));
            }
        }
        "http" => {
            if let Some(u) = nonempty(&d.username) {
                o.insert("username".into(), json!(u));
            }
            if !d.password.is_empty() {
                o.insert("password".into(), json!(d.password));
            }
        }
        _ => {}
    }

    let wants_tls = if NO_UTLS.contains(&d.kind) { true } else { d.tls };
    if wants_tls && d.kind != "socks" {
        let mut tls = Map::new();
        tls.insert("enabled".into(), json!(true));
        if let Some(s) = nonempty(&d.sni) {
            tls.insert("server_name".into(), json!(s));
        }
        let alpn = list(&d.alpn);
        if !alpn.is_empty() {
            tls.insert("alpn".into(), json!(alpn));
        }
        if d.insecure {
            tls.insert("insecure".into(), json!(true));
        }
        if let Some(k) = nonempty(&d.reality_key) {
            tls.insert(
                "reality".into(),
                json!({ "enabled": true, "public_key": k, "short_id": d.reality_short_id.trim() }),
            );
        }
        if let Some(fp) = nonempty(&d.fingerprint).filter(|_| !NO_UTLS.contains(&d.kind)) {
            tls.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
        }
        o.insert("tls".into(), Value::Object(tls));
    }

    if !d.transport.is_empty() && matches!(d.kind, "vless" | "vmess" | "trojan") {
        let mut tr = Map::new();
        tr.insert("type".into(), json!(d.transport));
        match d.transport {
            "ws" => {
                if let Some(p) = nonempty(&d.path) {
                    tr.insert("path".into(), json!(p));
                }
                if let Some(h) = nonempty(&d.host) {
                    tr.insert("headers".into(), json!({ "Host": h }));
                }
            }
            "grpc" => {
                if let Some(s) = nonempty(&d.service_name) {
                    tr.insert("service_name".into(), json!(s));
                }
            }
            "http" => {
                if let Some(p) = nonempty(&d.path) {
                    tr.insert("path".into(), json!(p));
                }
                if let Some(h) = nonempty(&d.host) {
                    tr.insert("host".into(), json!([h]));
                }
            }
            _ => {}
        }
        o.insert("transport".into(), Value::Object(tr));
    }
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        uri: String,
        name: Option<String>,
        outbound: Option<Value>,
    }

    /// Эталоны сгенерированы uri.ts — расхождение значит, что парсеры панели
    /// и службы разъехались.
    #[test]
    fn matches_panel_parser() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../tests/sharelinks.json")).expect("fixtures");
        assert!(cases.len() >= 10);
        for c in cases {
            let got = parse(&c.uri);
            match (&c.outbound, got) {
                (None, None) => {}
                (Some(want), Some(p)) => {
                    assert_eq!(&p.outbound, want, "outbound для {}", c.uri);
                    assert_eq!(Some(p.name), c.name, "имя для {}", c.uri);
                }
                (want, got) => panic!(
                    "{}: ожидали {}, получили {}",
                    c.uri,
                    if want.is_some() { "профиль" } else { "отказ" },
                    if got.is_some() { "профиль" } else { "отказ" }
                ),
            }
        }
    }
}
