//! Подписки — порт `router_files/subscription-refresh` с исправлениями:
//! один парсер ссылок с панелью (sharelink.rs), узлы с транспортами Xray
//! (xhttp и родня) пропускаются, а не превращаются в мёртвый TCP; профиль
//! другой группы с тем же id не перезаписывается; URL с токеном в лог не идёт.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use crate::profiles;
use crate::sharelink;
use crate::store::{self, Store};

pub const SUBS_DIR: &str = "subscriptions";
pub const DEFAULT_UA: &str = "sing-box/1.13.2";
const MAX_BODY: usize = 16 << 20;
const XRAY_ONLY: [&str; 5] = ["xhttp", "splithttp", "kcp", "mkcp", "httpupgrade"];
const SKIP_SB_TYPES: [&str; 5] = ["direct", "block", "dns", "selector", "urltest"];
const SKIP_V2RAY: [&str; 6] = ["freedom", "blackhole", "dns", "loopback", "balancer", "chain"];

fn rel(id: &str) -> String {
    format!("{SUBS_DIR}/{id}.json")
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn text(m: &Map<String, Value>, k: &str) -> String {
    m.get(k).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true" | "yes" | "on"),
        _ => false,
    }
}

pub fn list(store: &Store) -> Vec<Value> {
    let Ok(rd) = std::fs::read_dir(store.path(SUBS_DIR)) else { return Vec::new() };
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    names
        .iter()
        .filter_map(|n| store.read_json(&format!("{SUBS_DIR}/{n}")))
        .filter(|v| v.get("id").and_then(Value::as_str).is_some() && v.get("url").and_then(Value::as_str).is_some())
        .collect()
}

pub fn load(store: &Store, id: &str) -> Option<Map<String, Value>> {
    if !valid_id(id) {
        return None;
    }
    match store.read_json(&rel(id))? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// Поля `last_*` пишет только обновление: панель присылает черновик целиком,
/// вместе с загруженными ранее `last_*`, и без этого слияния сохранение
/// затирало бы результат обновления, прошедшего между загрузкой и сохранением.
pub fn save(store: &Store, body: Value) -> Result<()> {
    let Value::Object(mut m) = body else { bail!("invalid json") };
    let id = text(&m, "id");
    if !valid_id(&id) {
        bail!("id invalid: only a-z A-Z 0-9 . _ -");
    }
    let url = text(&m, "url");
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("url must start with http:// or https://");
    }
    m.retain(|k, _| !k.starts_with("last_") && !k.starts_with('_'));
    if let Some(old) = load(store, &id) {
        for (k, v) in old.into_iter().filter(|(k, _)| k.starts_with("last_")) {
            m.insert(k, v);
        }
    }
    store.write_json(&rel(&id), &Value::Object(m))?;
    Ok(())
}

pub fn delete(store: &Store, id: &str) -> Result<()> {
    if !valid_id(id) {
        bail!("id required");
    }
    std::fs::remove_file(store.path(&rel(id))).map_err(|_| anyhow!("not found"))
}

/// Адрес подписки без токена — для логов и ответов.
pub fn mask_url(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    format!("{scheme}://{host}/…")
}

/// id как у роутерного `make_profile_id`: профили, заведённые подпиской на
/// роутере и в клиенте, должны называться одинаково — на id ссылаются
/// маршруты и цепочки в перенесённой конфигурации.
pub fn make_profile_id(name: &str) -> String {
    let mut s = String::new();
    for ch in name.chars() {
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        if ('\u{0400}'..='\u{047F}').contains(&ch) {
            s.push_str(match lower {
                'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' => "e",
                'ж' => "zh", 'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k", 'л' => "l",
                'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s",
                'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch",
                'ш' => "sh", 'щ' => "sch", 'ы' => "y", 'э' => "e", 'ю' => "yu", 'я' => "ya",
                _ => "",
            });
        } else if ch.is_ascii() {
            s.push(ch.to_ascii_lowercase());
        } else {
            s.push('_');
        }
    }
    let mut id = String::new();
    for c in s.chars() {
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' { c } else { '_' };
        if c == '_' && id.ends_with('_') {
            continue;
        }
        id.push(c);
    }
    let id: String = id.trim_matches('_').chars().take(64).collect();
    if id.is_empty() {
        format!("profile_{}", store::now_epoch())
    } else {
        id
    }
}

#[derive(Debug, Default)]
pub struct Parsed {
    /// (имя, outbound, исходная ссылка)
    pub entries: Vec<(String, Value, String)>,
    pub bypass: Vec<String>,
    pub skipped: usize,
    pub kind: &'static str,
}

fn tls_from_stream(stream: &Value) -> Option<Value> {
    let sec = stream.get("security").and_then(Value::as_str).unwrap_or("none");
    let mut out = Map::new();
    out.insert("enabled".into(), json!(true));
    match sec {
        "reality" => {
            let r = stream.get("realitySettings").cloned().unwrap_or_default();
            if let Some(v) = r.get("serverName") {
                out.insert("server_name".into(), v.clone());
            }
            if let Some(fp) = r.get("fingerprint") {
                out.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
            }
            let mut reality = Map::new();
            reality.insert("enabled".into(), json!(true));
            if let Some(v) = r.get("publicKey") {
                reality.insert("public_key".into(), v.clone());
            }
            if let Some(v) = r.get("shortId") {
                reality.insert("short_id".into(), v.clone());
            }
            out.insert("reality".into(), Value::Object(reality));
        }
        "tls" | "xtls" => {
            let t = stream.get("tlsSettings").or_else(|| stream.get("xtlsSettings")).cloned().unwrap_or_default();
            if let Some(v) = t.get("serverName") {
                out.insert("server_name".into(), v.clone());
            }
            if truthy(t.get("allowInsecure")) {
                out.insert("insecure".into(), json!(true));
            }
            if let Some(a) = t.get("alpn").and_then(Value::as_array).filter(|a| !a.is_empty()) {
                out.insert("alpn".into(), Value::Array(a.clone()));
            }
            if let Some(fp) = t.get("fingerprint") {
                out.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
            }
        }
        _ => return None,
    }
    Some(Value::Object(out))
}

/// `Err(())` — транспорт только Xray: такой outbound ронял бы весь конфиг.
fn transport_from_stream(stream: &Value) -> Result<Option<Value>, ()> {
    let net = stream.get("network").and_then(Value::as_str).unwrap_or("tcp");
    let get = |k: &str| stream.get(k).cloned().unwrap_or_default();
    Ok(match net {
        "tcp" | "raw" => None,
        "ws" => {
            let ws = get("wsSettings");
            let mut t = json!({ "type": "ws" });
            if let Some(p) = ws.get("path") {
                t["path"] = p.clone();
            }
            match ws.get("headers") {
                Some(h) if h.as_object().is_some_and(|o| !o.is_empty()) => t["headers"] = h.clone(),
                _ => {
                    if let Some(host) = ws.get("host") {
                        t["headers"] = json!({ "Host": host });
                    }
                }
            }
            Some(t)
        }
        "grpc" => {
            let g = get("grpcSettings");
            let mut t = json!({ "type": "grpc" });
            if let Some(s) = g.get("serviceName") {
                t["service_name"] = s.clone();
            }
            Some(t)
        }
        "h2" | "http" => {
            let h = stream.get("httpSettings").or_else(|| stream.get("h2Settings")).cloned().unwrap_or_default();
            let mut t = json!({ "type": "http" });
            if let Some(p) = h.get("path") {
                t["path"] = p.clone();
            }
            match h.get("host") {
                Some(Value::Array(a)) => t["host"] = Value::Array(a.clone()),
                Some(v @ Value::String(_)) => t["host"] = json!([v]),
                _ => {}
            }
            Some(t)
        }
        "quic" => Some(json!({ "type": "quic" })),
        _ => return Err(()),
    })
}

fn first(v: &Value, k: &str) -> Value {
    v.get(k).and_then(Value::as_array).and_then(|a| a.first()).cloned().unwrap_or_default()
}

fn port(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Number(n)) => Value::Number(n.clone()),
        Some(Value::String(s)) => s.trim().parse::<u64>().map(|n| json!(n)).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// v2ray/Xray outbound → sing-box. `encryption` у vless не переносится: у
/// sing-box такого поля нет, и оно роняло бы конфиг.
fn v2ray_to_singbox(ob: &Value) -> Option<Value> {
    let proto = ob.get("protocol")?.as_str()?;
    if SKIP_V2RAY.contains(&proto) {
        return None;
    }
    let settings = ob.get("settings").cloned().unwrap_or_default();
    let stream = ob.get("streamSettings").cloned().unwrap_or_default();
    let mut o = Map::new();
    let with_stream = |o: &mut Map<String, Value>| -> Option<()> {
        if let Some(t) = tls_from_stream(&stream) {
            o.insert("tls".into(), t);
        }
        if let Some(t) = transport_from_stream(&stream).ok()? {
            o.insert("transport".into(), t);
        }
        Some(())
    };
    match proto {
        "vless" | "vmess" => {
            let vnext = first(&settings, "vnext");
            let user = first(&vnext, "users");
            o.insert("type".into(), json!(proto));
            o.insert("server".into(), vnext.get("address").cloned().unwrap_or_default());
            o.insert("server_port".into(), port(vnext.get("port")));
            o.insert("uuid".into(), user.get("id").cloned().unwrap_or_default());
            if proto == "vless" {
                if let Some(f) = user.get("flow").and_then(Value::as_str).filter(|f| !f.is_empty()) {
                    o.insert("flow".into(), json!(f));
                }
            } else {
                o.insert("security".into(), user.get("security").cloned().unwrap_or(json!("auto")));
                o.insert("alter_id".into(), user.get("alterId").cloned().unwrap_or(json!(0)));
            }
            with_stream(&mut o)?;
        }
        "trojan" => {
            let srv = first(&settings, "servers");
            o.insert("type".into(), json!("trojan"));
            o.insert("server".into(), srv.get("address").cloned().unwrap_or_default());
            o.insert("server_port".into(), port(srv.get("port")));
            o.insert("password".into(), srv.get("password").cloned().unwrap_or_default());
            with_stream(&mut o)?;
        }
        "shadowsocks" => {
            let srv = first(&settings, "servers");
            o.insert("type".into(), json!("shadowsocks"));
            o.insert("server".into(), srv.get("address").cloned().unwrap_or_default());
            o.insert("server_port".into(), port(srv.get("port")));
            o.insert("method".into(), srv.get("method").cloned().unwrap_or_default());
            o.insert("password".into(), srv.get("password").cloned().unwrap_or_default());
        }
        "hysteria" | "hysteria2" => {
            let hs = stream.get("hysteriaSettings").cloned().unwrap_or_default();
            let ver = settings
                .get("version")
                .or_else(|| hs.get("version"))
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                .unwrap_or(if proto == "hysteria2" { 2 } else { 1 });
            let server = settings.get("address").or_else(|| settings.get("server")).cloned().unwrap_or_default();
            o.insert("server".into(), server);
            o.insert("server_port".into(), port(settings.get("port").or_else(|| settings.get("server_port"))));
            if ver == 2 {
                o.insert("type".into(), json!("hysteria2"));
                if let Some(a) = hs.get("auth") {
                    o.insert("password".into(), a.clone());
                }
            } else {
                o.insert("type".into(), json!("hysteria"));
                if let Some(a) = hs.get("auth_str").or_else(|| hs.get("auth")) {
                    o.insert("auth_str".into(), a.clone());
                }
                for k in ["obfs", "up_mbps", "down_mbps"] {
                    if let Some(v) = settings.get(k) {
                        o.insert(k.into(), v.clone());
                    }
                }
            }
            if let Some(t) = tls_from_stream(&stream) {
                o.insert("tls".into(), t);
            }
        }
        _ => return None,
    }
    Some(Value::Object(o))
}

fn pick_proxy_outbound(doc: &Value) -> Option<&Value> {
    let outs = doc.get("outbounds")?.as_array()?;
    let usable = |o: &&Value| o.get("protocol").and_then(Value::as_str).is_some_and(|p| !SKIP_V2RAY.contains(&p));
    outs.iter()
        .filter(usable)
        .find(|o| o.get("tag").and_then(Value::as_str) == Some("proxy"))
        .or_else(|| outs.iter().find(usable))
}

fn looks_base64(body: &str) -> Option<String> {
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 16 || !compact.chars().all(|c| c.is_ascii_alphanumeric() || "+/_=-".contains(c)) {
        return None;
    }
    let bytes = sharelink::b64decode_bytes(&compact)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let t = text.trim_start();
    (text.contains("://") || t.starts_with('{') || t.starts_with('[') || text.contains("[Interface]")).then_some(text)
}

/// Импортируемый outbound: обязательны `server` и `server_port`; ссылки на
/// чужие теги (`detour`, `domain_resolver`) в нашем конфиге не существуют и
/// роняли бы его целиком; транспорты Xray — тоже.
fn usable(mut ob: Value) -> Option<Value> {
    let o = ob.as_object_mut()?;
    o.get("server")?.as_str()?;
    o.get("server_port")?.as_u64()?;
    let tr = o.get("transport").and_then(|t| t.get("type")).and_then(Value::as_str).unwrap_or("");
    if XRAY_ONLY.contains(&tr) {
        return None;
    }
    o.remove("detour");
    o.remove("domain_resolver");
    o.insert("tag".into(), json!("proxy"));
    Some(ob)
}

pub fn parse_body(body: &str, apply_routing: bool) -> Parsed {
    let mut p = Parsed { kind: "uri-list", ..Default::default() };
    let mut seen = HashSet::new();
    let mut add_bypass = |d: &str, p: &mut Parsed| {
        let d = d.trim_start_matches("domain:").trim_start_matches("domainSuffix:");
        if d.starts_with("geosite:") || d.starts_with("regexp:") || d.starts_with("full:") {
            return;
        }
        let d = d.trim().to_ascii_lowercase();
        if !d.is_empty() && seen.insert(d.clone()) {
            p.bypass.push(d);
        }
    };

    let docs = match serde_json::from_str::<Value>(body) {
        Ok(v @ Value::Object(_)) if v.get("outbounds").is_some() => vec![v],
        Ok(Value::Array(a)) if !a.is_empty() => a,
        _ => Vec::new(),
    };
    if !docs.is_empty() {
        p.kind = "json";
        for doc in &docs {
            if apply_routing {
                if let Some(rules) = doc.pointer("/routing/rules").and_then(Value::as_array) {
                    for r in rules.iter().filter(|r| r.get("outboundTag").and_then(Value::as_str) == Some("direct")) {
                        for d in r.get("domain").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                            add_bypass(d, &mut p);
                        }
                    }
                }
            }
            let name_of = |v: &Value, keys: &[&str]| {
                keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty())).unwrap_or("").to_owned()
            };
            if let Some(proxy) = pick_proxy_outbound(doc) {
                match v2ray_to_singbox(proxy).and_then(usable) {
                    Some(ob) => {
                        let name = name_of(doc, &["remarks", "name"]);
                        let name = if name.is_empty() { name_of(proxy, &["tag"]) } else { name };
                        p.entries.push((name, ob, String::new()));
                    }
                    None => p.skipped += 1,
                }
            } else if doc.get("type").and_then(Value::as_str).is_some() {
                match usable(doc.clone()) {
                    Some(ob) => p.entries.push((name_of(doc, &["tag", "name"]), ob, String::new())),
                    None => p.skipped += 1,
                }
            } else {
                let mut considered = false;
                for ob in doc.get("outbounds").and_then(Value::as_array).into_iter().flatten() {
                    let t = ob.get("type").and_then(Value::as_str).unwrap_or("");
                    if t.is_empty() || SKIP_SB_TYPES.contains(&t) {
                        continue;
                    }
                    considered = true;
                    match usable(ob.clone()) {
                        Some(o) => p.entries.push((name_of(ob, &["tag"]), o, String::new())),
                        None => p.skipped += 1,
                    }
                }
                if !considered {
                    p.skipped += 1;
                }
            }
        }
    }

    let mut text = body.to_owned();
    if p.entries.is_empty() {
        if let Some(decoded) = looks_base64(body) {
            p.kind = "uri-list-b64";
            text = decoded;
        } else if p.kind == "json" {
            p.kind = "uri-list";
        }
    }
    if p.entries.is_empty() {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                continue;
            }
            let parsed = sharelink::parse(line)
                .filter(|s| !XRAY_ONLY.contains(&s.requested_transport.as_str()))
                .and_then(|s| {
                    let name = if s.name.is_empty() { s.server.clone() } else { s.name.clone() };
                    usable(s.outbound).map(|ob| (name, ob))
                });
            match parsed {
                Some((name, ob)) => p.entries.push((name, ob, line.to_owned())),
                None => p.skipped += 1,
            }
        }
    }
    p
}

/// reqwest прячет причину («error sending request») в цепочке source —
/// человеку нужна именно она: отказ в соединении, таймаут, TLS.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        let part = s.to_string();
        if !msg.contains(&part) {
            msg = format!("{msg}: {part}");
        }
        src = s.source();
    }
    format!("network error — {msg}")
}

pub struct Fetched {
    pub body: String,
    pub headers: BTreeMap<String, String>,
}

const PREVIEW_HEADERS: [&str; 5] =
    ["profile-title", "profile-update-interval", "subscription-userinfo", "support-url", "content-type"];

/// Один запрос. Ошибки сформулированы, как у роутера, — панель их показывает.
pub async fn fetch(url: &str, ua: &str, proxy: Option<&str>, timeout: Duration) -> Result<Fetched, String> {
    let mut b = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(10));
    b = match proxy {
        Some(p) => b.proxy(reqwest::Proxy::all(p).map_err(|e| e.to_string())?),
        None => b.no_proxy(),
    };
    let client = b.build().map_err(|e| e.to_string())?;
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, ua)
        .header(reqwest::header::ACCEPT, "*/*")
        .send()
        .await
        .map_err(describe)?;
    let code = resp.status().as_u16();
    if code >= 400 {
        return Err(format!("server returned HTTP {code}, not a subscription feed"));
    }
    let headers = PREVIEW_HEADERS
        .iter()
        .filter_map(|h| Some((h.to_string(), resp.headers().get(*h)?.to_str().ok()?.to_owned())))
        .collect();
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(describe)? {
        buf.extend_from_slice(&chunk);
        if buf.len() > MAX_BODY {
            return Err("подписка больше 16 МБ — это не список серверов".into());
        }
    }
    let body = String::from_utf8_lossy(&buf).into_owned();
    if body.trim().is_empty() {
        return Err("empty response".into());
    }
    if body.to_ascii_lowercase().contains("<html") {
        return Err("server returned an HTML page, not a subscription feed".into());
    }
    Ok(Fetched { body, headers })
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub saved: usize,
    pub removed: usize,
    pub kept_stale: usize,
    pub skipped: usize,
    pub bypass_written: usize,
    pub kind: &'static str,
    pub via: &'static str,
    pub bytes: usize,
    /// id профилей, чей outbound изменился или которые удалены.
    pub touched: Vec<String>,
}

/// Сначала через VPN (если прокси службы поднят), потом напрямую. Успех —
/// только если из тела разобрался хотя бы один профиль.
pub async fn fetch_and_parse(
    sub: &Map<String, Value>,
    vpn_proxy: Option<&str>,
    log: &mut Vec<String>,
) -> Result<(Parsed, &'static str, usize), String> {
    let url = text(sub, "url");
    let ua = Some(text(sub, "user_agent")).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_UA.into());
    let apply_routing = truthy(sub.get("apply_routing"));
    let mut attempts: Vec<(&'static str, Option<&str>, u64)> = Vec::new();
    if let Some(p) = vpn_proxy {
        attempts.push(("vpn", Some(p), 25));
    }
    attempts.push(("direct", None, 30));
    let mut errors = Vec::new();
    for (via, proxy, secs) in attempts {
        match fetch(&url, &ua, proxy, Duration::from_secs(secs)).await {
            Ok(f) => {
                let parsed = parse_body(&f.body, apply_routing);
                if !parsed.entries.is_empty() {
                    log.push(format!("{via}: {} байт, формат {}", f.body.len(), parsed.kind));
                    return Ok((parsed, via, f.body.len()));
                }
                errors.push(format!("{via}: no profiles parsed (skipped={}, kind={})", parsed.skipped, parsed.kind));
            }
            Err(e) => errors.push(format!("{via}: {e}")),
        }
    }
    Err(errors.join("; "))
}

/// Разобранное тело → файлы профилей. Удаление устаревших идёт только по
/// непустой группе и не трогает профили активной и сохранённых цепочек.
pub fn apply_parsed(
    store: &Store,
    sub: &Map<String, Value>,
    parsed: &Parsed,
    protected: &[String],
) -> Result<Outcome> {
    let group = text(sub, "group");
    let label = if group.is_empty() { text(sub, "title") } else { group.clone() };
    let apply_routing = truthy(sub.get("apply_routing"));
    let mut out = Outcome { skipped: parsed.skipped, kind: parsed.kind, ..Default::default() };

    let existing: BTreeMap<String, Value> = profiles::list_ids(store)
        .into_iter()
        .filter_map(|id| Some((id.clone(), profiles::load(store, &id)?)))
        .collect();
    let group_of = |p: &Value| p.get("group").and_then(Value::as_str).unwrap_or("").to_owned();

    let mut taken: HashSet<String> = HashSet::new();
    let mut fresh: Vec<(String, Value)> = Vec::new();
    for (name, ob, uri) in &parsed.entries {
        let label_name = if name.is_empty() { ob["server"].as_str().unwrap_or("").to_owned() } else { name.clone() };
        let base = make_profile_id(&label_name);
        let mut id = base.clone();
        let mut n = 2;
        let foreign = |id: &str| existing.get(id).is_some_and(|p| group_of(p) != label);
        while taken.contains(&id) || foreign(&id) {
            id = format!("{}_{n}", base.chars().take(60).collect::<String>());
            n += 1;
        }
        taken.insert(id.clone());
        let mut prof = json!({
            "id": id,
            "name": label_name,
            "type": profiles::infer_type(Some(ob)),
            "uri": uri,
            "group": label,
            "outbound": ob,
        });
        if apply_routing {
            prof["routing_mode"] = json!("all-except");
        }
        fresh.push((id, prof));
    }

    if !group.is_empty() {
        for (id, p) in &existing {
            if group_of(p) != group || taken.contains(id) {
                continue;
            }
            if protected.contains(id) {
                out.kept_stale += 1;
            } else if std::fs::remove_file(store.path(&format!("{}/{id}.json", store::PROFILES_DIR))).is_ok() {
                out.removed += 1;
                out.touched.push(id.clone());
            }
        }
    }

    for (id, prof) in fresh {
        let changed = existing.get(&id).map(|old| old.get("outbound") != prof.get("outbound")).unwrap_or(true);
        store.write_json(&format!("{}/{id}.json", store::PROFILES_DIR), &prof)?;
        out.saved += 1;
        if changed {
            out.touched.push(id);
        }
    }

    if apply_routing && !group.is_empty() && !parsed.bypass.is_empty() {
        rewrite_whitelist_section(store, &group, &parsed.bypass)?;
        out.bypass_written = parsed.bypass.len();
    }
    Ok(out)
}

fn rewrite_whitelist_section(store: &Store, group: &str, domains: &[String]) -> Result<()> {
    let begin = format!("// === subscription:{group} ===");
    let end = format!("// === end subscription:{group} ===");
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    for line in store.read_text(store::WHITELIST).lines() {
        if line == begin {
            inside = true;
        } else if line == end {
            inside = false;
        } else if !inside {
            out.push(line.to_owned());
        }
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.push(String::new());
    out.push(begin);
    out.extend(domains.iter().cloned());
    out.push(end);
    out.push(String::new());
    store.write_text(store::WHITELIST, &out.join("\n"))?;
    Ok(())
}

pub fn record(store: &Store, id: &str, result: &Result<Outcome, String>, bytes: usize) -> Result<()> {
    let Some(mut m) = load(store, id) else { return Ok(()) };
    m.insert("last_refresh".into(), json!(store::now_epoch()));
    match result {
        Ok(o) => {
            m.insert("last_status".into(), json!("ok"));
            m.remove("last_error");
            m.insert("last_saved".into(), json!(o.saved));
            m.insert("last_removed".into(), json!(o.removed));
            m.insert("last_kept_stale".into(), json!(o.kept_stale));
            m.insert("last_skipped".into(), json!(o.skipped));
            m.insert("last_bypass_written".into(), json!(o.bypass_written));
            m.insert("last_source_kind".into(), json!(o.kind));
            m.insert("last_fetched_bytes".into(), json!(bytes));
            m.insert("last_via".into(), json!(o.via));
        }
        Err(e) => {
            m.insert("last_status".into(), json!("error"));
            m.insert("last_error".into(), json!(e));
        }
    }
    store.write_json(&rel(id), &Value::Object(m))?;
    Ok(())
}

/// Пора ли обновлять по расписанию: только с `autoupdate`, раз в
/// `interval_hours` (по умолчанию сутки).
pub fn due(sub: &Map<String, Value>, now: i64) -> bool {
    if !truthy(sub.get("autoupdate")) {
        return false;
    }
    let hours = sub.get("interval_hours").and_then(Value::as_f64).unwrap_or(24.0);
    let last = sub.get("last_refresh").and_then(Value::as_i64).unwrap_or(0);
    (now - last) as f64 >= hours * 3600.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_id_matches_router() {
        assert_eq!(make_profile_id("🇳🇱 Нидерланды Ц-1"), "niderlandy_ts-1");
        assert_eq!(make_profile_id("Example VPN / DE"), "example_vpn_de");
        assert!(make_profile_id("🔥🔥").starts_with("profile_"));
    }

    #[test]
    fn singbox_config_body() {
        let body = r#"{"outbounds":[
            {"type":"selector","tag":"select","outbounds":["a"]},
            {"type":"vless","tag":"🇳🇱 NL","server":"vpn.example.com","server_port":443,"uuid":"u","domain_resolver":"dns-remote","detour":"x"},
            {"type":"vless","tag":"xh","server":"vpn.example.com","server_port":443,"uuid":"u","transport":{"type":"xhttp"}},
            {"type":"direct","tag":"direct"}]}"#;
        let p = parse_body(body, false);
        assert_eq!(p.kind, "json");
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.skipped, 1);
        let ob = &p.entries[0].1;
        assert_eq!(ob["tag"], "proxy");
        assert!(ob.get("domain_resolver").is_none() && ob.get("detour").is_none());
    }

    #[test]
    fn v2ray_json_and_bypass() {
        let body = r#"[{"remarks":"DE","outbounds":[{"protocol":"vless","tag":"proxy",
            "settings":{"vnext":[{"address":"vpn.example.com","port":443,"users":[{"id":"u","flow":"xtls-rprx-vision","encryption":"none"}]}]},
            "streamSettings":{"network":"tcp","security":"reality","realitySettings":{"serverName":"www.example.com","fingerprint":"chrome","publicKey":"K","shortId":"ab"}}},
            {"protocol":"freedom","tag":"direct"}],
            "routing":{"rules":[{"outboundTag":"direct","domain":["domain:ya.ru","geosite:ru","Example.RU"]}]}}]"#;
        let p = parse_body(body, true);
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].0, "DE");
        let ob = &p.entries[0].1;
        assert_eq!(ob["tls"]["reality"]["public_key"], "K");
        assert!(ob.get("encryption").is_none());
        assert_eq!(p.bypass, vec!["ya.ru", "example.ru"]);
    }

    #[test]
    fn base64_uri_list_skips_xhttp() {
        let list = "vless://u@vpn.example.com:443?security=tls#A\nvless://u@vpn.example.com:443?type=xhttp#B\n# comment\nwireguard://x@y:1#C\n";
        let body = base64::engine::general_purpose::STANDARD.encode(list);
        use base64::Engine as _;
        let p = parse_body(&body, false);
        assert_eq!(p.kind, "uri-list-b64");
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.skipped, 2);
        assert_eq!(p.entries[0].2, "vless://u@vpn.example.com:443?security=tls#A");
    }

    #[test]
    fn masks_token() {
        assert_eq!(mask_url("https://sub.example.com/api/sub/SECRET123?x=1"), "https://sub.example.com/…");
    }
}
