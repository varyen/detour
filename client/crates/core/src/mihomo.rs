//! Перевод конфига sing-box в конфиг mihomo — порт `router_files/sb2mihomo.lua`.
//!
//! Рендер у проекта один: `render.rs` строит конфиг sing-box, а в режиме движка
//! «mihomo» он переводится сюда. Общая часть (исходящие, входы, правила
//! маршрута) обязана совпадать с Lua-версией роутера байт в байт по смыслу —
//! это держат эталоны `tests/fixtures/sb2mihomo`, которые генерирует Lua.
//! Сверх роутера здесь то, что есть только у клиента: TUN, DNS, rule-set
//! файлы, правила по процессу, частным адресам и версии IP.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

fn s(v: &Value, k: &str) -> Option<String> {
    match v.get(k) {
        Some(Value::String(x)) if !x.is_empty() => Some(x.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn num(v: &Value, k: &str) -> Option<i64> {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(x)) => x.trim().parse().ok(),
        _ => None,
    }
}

fn list(v: Option<&Value>) -> Vec<Value> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(x) => vec![x.clone()],
    }
}

fn strs(v: Option<&Value>) -> Vec<String> {
    list(v)
        .into_iter()
        .filter_map(|x| match x {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

fn port_range(r: &str) -> String {
    if let Some((a, b)) = r.split_once(':') {
        let a = if a.is_empty() { "1" } else { a };
        let b = if b.is_empty() { "65535" } else { b };
        return format!("{a}-{b}");
    }
    r.to_owned()
}

fn set(p: &mut Map<String, Value>, k: &str, v: Option<String>) {
    if let Some(v) = v {
        p.insert(k.into(), json!(v));
    }
}

// ---------------------------------------------------------------- TLS / транспорт

fn apply_tls(p: &mut Map<String, Value>, ob: &Value, sni_key: &str) -> bool {
    let Some(tls) = ob.get("tls").filter(|t| t.is_object()) else { return false };
    if tls.get("enabled") == Some(&json!(false)) {
        return false;
    }
    set(p, sni_key, s(tls, "server_name"));
    if tls.get("insecure") == Some(&json!(true)) {
        p.insert("skip-cert-verify".into(), json!(true));
    }
    let alpn = strs(tls.get("alpn"));
    if !alpn.is_empty() {
        p.insert("alpn".into(), json!(alpn));
    }
    if let Some(u) = tls.get("utls").filter(|u| u.is_object()) {
        if u.get("enabled") != Some(&json!(false)) {
            set(p, "client-fingerprint", s(u, "fingerprint"));
        }
    }
    if let Some(r) = tls.get("reality").filter(|r| r.is_object()) {
        if r.get("enabled") != Some(&json!(false)) {
            p.insert(
                "reality-opts".into(),
                json!({ "public-key": s(r, "public_key").unwrap_or_default(), "short-id": s(r, "short_id").unwrap_or_default() }),
            );
            p.entry("client-fingerprint").or_insert(json!("chrome"));
        }
    }
    true
}

fn apply_transport(p: &mut Map<String, Value>, ob: &Value, tls_on: bool) -> Result<(), String> {
    let Some(tr) = ob.get("transport").filter(|t| t.is_object()) else { return Ok(()) };
    let t = s(tr, "type").unwrap_or_default();
    match t.as_str() {
        "ws" | "httpupgrade" => {
            p.insert("network".into(), json!("ws"));
            let mut o = Map::new();
            set(&mut o, "path", s(tr, "path"));
            let host = tr
                .get("headers")
                .and_then(|h| s(h, "Host").or_else(|| s(h, "host")))
                .or_else(|| s(tr, "host"));
            if let Some(h) = host {
                o.insert("headers".into(), json!({ "Host": h }));
            }
            if let Some(n) = num(tr, "max_early_data") {
                o.insert("max-early-data".into(), json!(n));
            }
            set(&mut o, "early-data-header-name", s(tr, "early_data_header_name"));
            if t == "httpupgrade" {
                o.insert("v2ray-http-upgrade".into(), json!(true));
            }
            p.insert("ws-opts".into(), Value::Object(o));
        }
        "grpc" => {
            p.insert("network".into(), json!("grpc"));
            p.insert("grpc-opts".into(), json!({ "grpc-service-name": s(tr, "service_name").unwrap_or_default() }));
        }
        "http" => {
            let hosts = strs(tr.get("host"));
            if tls_on {
                p.insert("network".into(), json!("h2"));
                let mut o = Map::new();
                if !hosts.is_empty() {
                    o.insert("host".into(), json!(hosts));
                }
                set(&mut o, "path", s(tr, "path"));
                p.insert("h2-opts".into(), Value::Object(o));
            } else {
                p.insert("network".into(), json!("http"));
                let mut o = Map::new();
                if let Some(path) = s(tr, "path") {
                    o.insert("path".into(), json!([path]));
                }
                if !hosts.is_empty() {
                    o.insert("headers".into(), json!({ "Host": hosts }));
                }
                p.insert("http-opts".into(), Value::Object(o));
            }
        }
        "xhttp" | "splithttp" => {
            p.insert("network".into(), json!("xhttp"));
            let mut o = Map::new();
            set(&mut o, "path", s(tr, "path"));
            set(&mut o, "host", s(tr, "host"));
            set(&mut o, "mode", s(tr, "mode"));
            p.insert("xhttp-opts".into(), Value::Object(o));
        }
        other => return Err(format!("транспорт {other} не поддерживается mihomo")),
    }
    Ok(())
}

// ---------------------------------------------------------------- WireGuard

fn wireguard(p: &mut Map<String, Value>, ob: &Value) -> Result<(), String> {
    p.insert("type".into(), json!("wireguard"));
    set(p, "private-key", s(ob, "private_key"));
    for a in strs(ob.get("address").or_else(|| ob.get("local_address"))) {
        let host = a.split('/').next().unwrap_or(&a).to_owned();
        let key = if host.contains(':') { "ipv6" } else { "ip" };
        p.entry(key).or_insert(json!(host));
    }
    if !p.contains_key("ip") && !p.contains_key("ipv6") {
        return Err("у WireGuard нет адреса интерфейса".into());
    }
    if let Some(m) = num(ob, "mtu") {
        p.insert("mtu".into(), json!(m));
    }
    let mut peers = list(ob.get("peers"));
    if peers.is_empty() {
        peers = vec![json!({
            "address": ob.get("server"), "port": ob.get("server_port"),
            "public_key": ob.get("peer_public_key"), "pre_shared_key": ob.get("pre_shared_key"),
            "reserved": ob.get("reserved"), "allowed_ips": ob.get("allowed_ips"),
            "persistent_keepalive_interval": ob.get("persistent_keepalive_interval"),
        })];
    }
    let peer_fields = |dst: &mut Map<String, Value>, pe: &Value| {
        set(dst, "server", s(pe, "address"));
        if let Some(n) = num(pe, "port") {
            dst.insert("port".into(), json!(n));
        }
        set(dst, "public-key", s(pe, "public_key"));
        set(dst, "pre-shared-key", s(pe, "pre_shared_key"));
        if let Some(r) = pe.get("reserved").and_then(Value::as_array).filter(|r| !r.is_empty()) {
            dst.insert("reserved".into(), json!(r));
        }
        let allowed = strs(pe.get("allowed_ips"));
        dst.insert(
            "allowed-ips".into(),
            if allowed.is_empty() { json!(["0.0.0.0/0", "::/0"]) } else { json!(allowed) },
        );
    };
    if peers.len() == 1 {
        peer_fields(p, &peers[0]);
        if let Some(k) = num(&peers[0], "persistent_keepalive_interval") {
            p.insert("persistent-keepalive".into(), json!(k));
        }
        if !p.contains_key("server") || !p.contains_key("port") || !p.contains_key("public-key") {
            return Err("у WireGuard нет сервера, порта или ключа пира".into());
        }
    } else {
        let mut out = Vec::new();
        for pe in &peers {
            let mut d = Map::new();
            peer_fields(&mut d, pe);
            out.push(Value::Object(d));
        }
        p.insert("peers".into(), json!(out));
    }
    if let Some(o) = ob.get("amnezia").and_then(crate::awg::option) {
        p.insert("amnezia-wg-option".into(), o);
    }
    Ok(())
}

// ---------------------------------------------------------------- исходящие

/// Outbound/endpoint sing-box → прокси mihomo.
pub fn proxy(ob: &Value) -> Result<Value, String> {
    let t = s(ob, "type").unwrap_or_default();
    let mut p = Map::new();
    p.insert("name".into(), json!(s(ob, "tag").unwrap_or_default()));
    p.insert("udp".into(), json!(true));
    if t != "wireguard" && t != "amneziawg" {
        set(&mut p, "server", s(ob, "server"));
        if let Some(n) = num(ob, "server_port") {
            p.insert("port".into(), json!(n));
        }
    }
    if let Some(d) = s(ob, "detour").filter(|d| d != "direct") {
        p.insert("dialer-proxy".into(), json!(d));
    }
    if let Some(m) = num(ob, "routing_mark") {
        p.insert("routing-mark".into(), json!(m));
    }
    set(&mut p, "interface-name", s(ob, "bind_interface"));

    match t.as_str() {
        "vless" => {
            p.insert("type".into(), json!("vless"));
            set(&mut p, "uuid", s(ob, "uuid"));
            set(&mut p, "flow", s(ob, "flow"));
            set(&mut p, "packet-encoding", s(ob, "packet_encoding"));
            let tls = apply_tls(&mut p, ob, "servername");
            p.insert("tls".into(), json!(tls));
            apply_transport(&mut p, ob, tls)?;
        }
        "vmess" => {
            p.insert("type".into(), json!("vmess"));
            set(&mut p, "uuid", s(ob, "uuid"));
            p.insert("alterId".into(), json!(num(ob, "alter_id").unwrap_or(0)));
            p.insert("cipher".into(), json!(s(ob, "security").unwrap_or_else(|| "auto".into())));
            set(&mut p, "packet-encoding", s(ob, "packet_encoding"));
            let tls = apply_tls(&mut p, ob, "servername");
            p.insert("tls".into(), json!(tls));
            apply_transport(&mut p, ob, tls)?;
        }
        "trojan" => {
            p.insert("type".into(), json!("trojan"));
            set(&mut p, "password", s(ob, "password"));
            apply_tls(&mut p, ob, "sni");
            apply_transport(&mut p, ob, true)?;
        }
        "shadowsocks" => {
            p.insert("type".into(), json!("ss"));
            set(&mut p, "cipher", s(ob, "method"));
            set(&mut p, "password", s(ob, "password"));
            let uot = ob.get("udp_over_tcp");
            if uot == Some(&json!(true)) || uot.and_then(|u| u.get("enabled")) == Some(&json!(true)) {
                p.insert("udp-over-tcp".into(), json!(true));
            }
            if let Some(plugin) = s(ob, "plugin") {
                let mut opts = BTreeMap::new();
                for kv in s(ob, "plugin_opts").unwrap_or_default().split(';').filter(|x| !x.is_empty()) {
                    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                    opts.insert(k.to_owned(), v.to_owned());
                }
                match plugin.as_str() {
                    "obfs-local" | "simple-obfs" => {
                        p.insert("plugin".into(), json!("obfs"));
                        let mut o = Map::new();
                        o.insert("mode".into(), json!(opts.get("obfs").cloned().unwrap_or_else(|| "http".into())));
                        if let Some(h) = opts.get("obfs-host") {
                            o.insert("host".into(), json!(h));
                        }
                        p.insert("plugin-opts".into(), Value::Object(o));
                    }
                    "v2ray-plugin" => {
                        p.insert("plugin".into(), json!("v2ray-plugin"));
                        let mut o = Map::new();
                        o.insert("mode".into(), json!(opts.get("mode").cloned().unwrap_or_else(|| "websocket".into())));
                        o.insert("tls".into(), json!(opts.contains_key("tls")));
                        if let Some(h) = opts.get("host") {
                            o.insert("host".into(), json!(h));
                        }
                        if let Some(h) = opts.get("path") {
                            o.insert("path".into(), json!(h));
                        }
                        p.insert("plugin-opts".into(), Value::Object(o));
                    }
                    other => return Err(format!("плагин shadowsocks {other} не поддерживается")),
                }
            }
        }
        "hysteria2" => {
            p.insert("type".into(), json!("hysteria2"));
            set(&mut p, "password", s(ob, "password"));
            apply_tls(&mut p, ob, "sni");
            p.remove("client-fingerprint");
            if let Some(o) = ob.get("obfs").filter(|o| s(o, "password").is_some()) {
                p.insert("obfs".into(), json!(s(o, "type").unwrap_or_else(|| "salamander".into())));
                p.insert("obfs-password".into(), json!(s(o, "password")));
            }
            if let Some(n) = num(ob, "up_mbps") {
                p.insert("up".into(), json!(format!("{n} Mbps")));
            }
            if let Some(n) = num(ob, "down_mbps") {
                p.insert("down".into(), json!(format!("{n} Mbps")));
            }
            let ports: Vec<String> = strs(ob.get("server_ports")).iter().map(|x| port_range(x)).collect();
            if !ports.is_empty() {
                p.insert("ports".into(), json!(ports.join(",")));
            }
        }
        "hysteria" => {
            p.insert("type".into(), json!("hysteria"));
            set(&mut p, "auth-str", s(ob, "auth_str"));
            set(&mut p, "obfs", s(ob, "obfs"));
            apply_tls(&mut p, ob, "sni");
            p.remove("client-fingerprint");
            p.insert("up".into(), json!(format!("{} Mbps", num(ob, "up_mbps").unwrap_or(10))));
            p.insert("down".into(), json!(format!("{} Mbps", num(ob, "down_mbps").unwrap_or(50))));
        }
        "tuic" => {
            p.insert("type".into(), json!("tuic"));
            set(&mut p, "uuid", s(ob, "uuid"));
            set(&mut p, "password", s(ob, "password"));
            apply_tls(&mut p, ob, "sni");
            p.remove("client-fingerprint");
            set(&mut p, "congestion-controller", s(ob, "congestion_control"));
            set(&mut p, "udp-relay-mode", s(ob, "udp_relay_mode"));
            if ob.get("zero_rtt_handshake") == Some(&json!(true)) {
                p.insert("reduce-rtt".into(), json!(true));
            }
        }
        "socks" => {
            if let Some(v) = s(ob, "version").filter(|v| v != "5") {
                return Err(format!("SOCKS{v} mihomo не умеет (только SOCKS5)"));
            }
            p.insert("type".into(), json!("socks5"));
            set(&mut p, "username", s(ob, "username"));
            set(&mut p, "password", s(ob, "password"));
            if apply_tls(&mut p, ob, "sni") {
                p.insert("tls".into(), json!(true));
            }
        }
        "http" => {
            p.insert("type".into(), json!("http"));
            p.remove("udp");
            set(&mut p, "username", s(ob, "username"));
            set(&mut p, "password", s(ob, "password"));
            if apply_tls(&mut p, ob, "sni") {
                p.insert("tls".into(), json!(true));
            }
        }
        "wireguard" | "amneziawg" => wireguard(&mut p, ob)?,
        other => return Err(format!("тип {other} не поддерживается mihomo")),
    }
    Ok(Value::Object(p))
}

// ---------------------------------------------------------------- правила

fn target(name: &str) -> String {
    match name {
        "direct" => "DIRECT".into(),
        "block" => "REJECT".into(),
        other => other.into(),
    }
}

fn cidr(v: &str) -> (String, bool) {
    let v6 = v.contains(':');
    if v.contains('/') {
        (v.to_owned(), v6)
    } else {
        (format!("{v}/{}", if v6 { 128 } else { 32 }), v6)
    }
}

struct RuleCtx {
    providers: Map<String, Value>,
    /// тег rule-set sing-box → имя провайдера mihomo (клиент)
    rule_sets: BTreeMap<String, String>,
    /// счётчик inline-провайдеров длинных списков: rs1, rs2… — как у Lua
    n: usize,
}

fn rule_lines(r: &Value, ctx: &mut RuleCtx) -> Result<Vec<String>, String> {
    let action = s(r, "action").unwrap_or_default();
    if action == "sniff" || action == "hijack-dns" || r.get("protocol").is_some() {
        return Ok(Vec::new());
    }
    let tgt = if action == "reject" {
        "REJECT".to_owned()
    } else if let Some(o) = s(r, "outbound") {
        target(&o)
    } else {
        return Err(format!("правило без outbound: {r}"));
    };

    let mut out = Vec::new();
    let mut matchers = 0;
    let add = |kind: &str, values: Vec<String>, extra: &str, out: &mut Vec<String>, ctx: &mut RuleCtx| {
        if values.len() > 8 && (kind == "DOMAIN-SUFFIX" || kind == "IP-CIDR") {
            ctx.n += 1;
            let name = format!("rs{}", ctx.n);
            let behavior = if kind == "IP-CIDR" { "ipcidr" } else { "domain" };
            let payload: Vec<String> = values
                .iter()
                .map(|v| if behavior == "domain" { format!("+.{v}") } else { cidr(v).0 })
                .collect();
            ctx.providers.insert(name.clone(), json!({ "type": "inline", "behavior": behavior, "payload": payload }));
            out.push(format!("RULE-SET,{name},{tgt}{}", if behavior == "ipcidr" { ",no-resolve" } else { "" }));
            return;
        }
        for v in values {
            let (k, v) = if kind == "IP-CIDR" {
                let (c, v6) = cidr(&v);
                (if v6 { "IP-CIDR6" } else { "IP-CIDR" }, c)
            } else {
                (kind, v)
            };
            out.push(format!("{k},{v},{tgt}{extra}"));
        }
    };

    let net = strs(r.get("network"));
    let ports: Vec<String> = strs(r.get("port_range"))
        .iter()
        .map(|x| port_range(x))
        .chain(strs(r.get("port")))
        .collect();
    // сочетания, которые рендерит клиент: сеть + (порты | подсети | домены | rule-set)
    let combo = !net.is_empty()
        && (!ports.is_empty() || r.get("ip_cidr").is_some() || r.get("domain_suffix").is_some() || r.get("rule_set").is_some());
    if combo {
        let n = net[0].to_uppercase();
        let mut parts = Vec::new();
        if !ports.is_empty() {
            parts.push(format!("(DST-PORT,{})", ports.join("/")));
        }
        let mut alts = Vec::new();
        for v in strs(r.get("ip_cidr")) {
            let (c, v6) = cidr(&v);
            alts.push(format!("({},{c},no-resolve)", if v6 { "IP-CIDR6" } else { "IP-CIDR" }));
        }
        for v in strs(r.get("domain_suffix")) {
            alts.push(format!("(DOMAIN-SUFFIX,{v})"));
        }
        for t in strs(r.get("rule_set")) {
            let name = ctx.rule_sets.get(&t).cloned().ok_or_else(|| format!("нет rule-set {t}"))?;
            alts.push(format!("(RULE-SET,{name})"));
        }
        if alts.len() == 1 {
            parts.push(alts.remove(0));
        } else if !alts.is_empty() {
            parts.push(format!("(OR,({}))", alts.join(",")));
        }
        out.push(format!("AND,((NETWORK,{n}),{}),{tgt}", parts.join(",")));
        return Ok(out);
    }

    if let Some(v) = r.get("inbound") {
        matchers += 1;
        add("IN-NAME", strs(Some(v)), "", &mut out, ctx);
    }
    if let Some(v) = r.get("domain") {
        matchers += 1;
        add("DOMAIN", strs(Some(v)), "", &mut out, ctx);
    }
    if let Some(v) = r.get("domain_suffix") {
        matchers += 1;
        add("DOMAIN-SUFFIX", strs(Some(v)), "", &mut out, ctx);
    }
    if let Some(v) = r.get("ip_cidr") {
        matchers += 1;
        add("IP-CIDR", strs(Some(v)), ",no-resolve", &mut out, ctx);
    }
    if let Some(v) = r.get("process_name") {
        matchers += 1;
        add("PROCESS-NAME", strs(Some(v)), "", &mut out, ctx);
    }
    if let Some(v) = r.get("rule_set") {
        matchers += 1;
        for t in strs(Some(v)) {
            let name = ctx.rule_sets.get(&t).cloned().ok_or_else(|| format!("нет rule-set {t}"))?;
            out.push(format!("RULE-SET,{name},{tgt}"));
        }
    }
    if r.get("ip_is_private") == Some(&json!(true)) {
        matchers += 1;
        out.push(format!("GEOIP,lan,{tgt},no-resolve"));
    }
    if num(r, "ip_version") == Some(6) {
        matchers += 1;
        out.push(format!("IP-CIDR6,::/0,{tgt},no-resolve"));
    }
    if !net.is_empty() {
        matchers += 1;
        for n in &net {
            out.push(format!("NETWORK,{},{tgt}", n.to_uppercase()));
        }
    } else if !ports.is_empty() {
        matchers += 1;
        out.push(format!("DST-PORT,{},{tgt}", ports.join("/")));
    }
    if matchers > 1 {
        return Err(format!("правило с несколькими матчерами не переводится: {r}"));
    }
    if matchers == 0 {
        return Err(format!("правило без матчеров: {r}"));
    }
    Ok(out)
}

// ---------------------------------------------------------------- входы

fn listener(ib: &Value) -> Result<Value, String> {
    let t = s(ib, "type").unwrap_or_default();
    let mut l = Map::new();
    l.insert("name".into(), json!(s(ib, "tag").unwrap_or_default()));
    let listen = s(ib, "listen").map(|x| if x == "::" { "0.0.0.0".into() } else { x });
    set(&mut l, "listen", listen);
    if let Some(p) = num(ib, "listen_port") {
        l.insert("port".into(), json!(p));
    }
    match t.as_str() {
        "redirect" => {
            l.insert("type".into(), json!("redir"));
        }
        "tproxy" => {
            l.insert("type".into(), json!("tproxy"));
            l.insert("udp".into(), json!(true));
        }
        "mixed" | "socks" | "http" => {
            l.insert("type".into(), json!(t));
            if t != "http" {
                l.insert("udp".into(), json!(true));
            }
            if let Some(users) = ib.get("users").and_then(Value::as_array).filter(|u| !u.is_empty()) {
                let u: Vec<Value> = users
                    .iter()
                    .map(|u| json!({ "username": u.get("username"), "password": u.get("password") }))
                    .collect();
                l.insert("users".into(), json!(u));
            }
        }
        other => return Err(format!("вход {other} не переводится")),
    }
    Ok(Value::Object(l))
}

/// Rule-set sing-box (`{version, rules:[{domain_suffix},{ip_cidr}]}`) →
/// строки classical-провайдера mihomo.
fn ruleset_payload(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for r in v.get("rules").and_then(Value::as_array).cloned().unwrap_or_default() {
        for d in strs(r.get("domain_suffix")) {
            out.push(format!("DOMAIN-SUFFIX,{d}"));
        }
        for c in strs(r.get("ip_cidr")) {
            let (c, v6) = cidr(&c);
            out.push(format!("{},{c}", if v6 { "IP-CIDR6" } else { "IP-CIDR" }));
        }
    }
    out
}

// ---------------------------------------------------------------- весь конфиг

/// `files` — содержимое rule-set файлов сборки (путь → JSON), чтобы не читать
/// их с диска до записи; `None` — прочитать с диска по `path`.
pub fn convert(sb: &Value, files: Option<&[(std::path::PathBuf, Value)]>) -> Result<Value> {
    let mut errors = Vec::new();
    let mut conf = Map::new();
    conf.insert("mode".into(), json!("rule"));
    conf.insert("allow-lan".into(), json!(true));
    conf.insert("ipv6".into(), json!(true));
    conf.insert("unified-delay".into(), json!(true));
    conf.insert("tcp-concurrent".into(), json!(false));
    conf.insert("profile".into(), json!({ "store-selected": false, "store-fake-ip": false }));
    let lvl = sb.pointer("/log/level").and_then(Value::as_str).unwrap_or("warn");
    let lvl = match lvl {
        "warn" => "warning",
        "trace" => "debug",
        "fatal" | "panic" => "error",
        x => x,
    };
    conf.insert("log-level".into(), json!(lvl));

    // входы; TUN — отдельным блоком
    let mut listeners = Vec::new();
    let mut tun = None;
    for ib in list(sb.get("inbounds")) {
        if s(&ib, "type").as_deref() == Some("tun") {
            tun = Some(ib);
            continue;
        }
        match listener(&ib) {
            Ok(l) => listeners.push(l),
            Err(e) => errors.push(e),
        }
    }

    let mut proxies = Vec::new();
    for ob in list(sb.get("outbounds")).into_iter().chain(list(sb.get("endpoints"))) {
        let t = s(&ob, "type").unwrap_or_default();
        if t == "direct" || t == "block" || t == "dns" {
            continue;
        }
        match proxy(&ob) {
            Ok(p) => proxies.push(p),
            Err(e) => errors.push(format!("исходящий {}: {e}", s(&ob, "tag").unwrap_or_default())),
        }
    }
    let names: Vec<String> = proxies.iter().filter_map(|p| s(p, "name")).collect();
    for p in &mut proxies {
        if let Some(d) = s(p, "dialer-proxy") {
            if !names.contains(&d) {
                p.as_object_mut().unwrap().remove("dialer-proxy");
            }
        }
    }

    // rule-set файлы клиента → inline classical-провайдеры
    let mut ctx = RuleCtx { providers: Map::new(), rule_sets: BTreeMap::new(), n: 0 };
    for def in list(sb.pointer("/route/rule_set")) {
        let Some(tag) = s(&def, "tag") else { continue };
        let path = std::path::PathBuf::from(s(&def, "path").unwrap_or_default());
        let payload = match files.and_then(|f| f.iter().find(|(p, _)| *p == path)) {
            Some((_, v)) => ruleset_payload(v),
            None => std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .map(|v| ruleset_payload(&v))
                .unwrap_or_default(),
        };
        let name = format!("set-{tag}");
        ctx.providers
            .insert(name.clone(), json!({ "type": "inline", "behavior": "classical", "payload": payload }));
        ctx.rule_sets.insert(tag, name);
    }

    let mut rules = Vec::new();
    let mut sniff = false;
    let mut override_dst = false;
    let mut hijack = false;
    for r in list(sb.pointer("/route/rules")) {
        match s(&r, "action").as_deref() {
            Some("sniff") => sniff = true,
            Some("hijack-dns") => hijack = true,
            _ => {}
        }
        if s(&r, "override_address").is_some() {
            override_dst = true;
        }
        match rule_lines(&r, &mut ctx) {
            Ok(l) => rules.extend(l),
            Err(e) => errors.push(e),
        }
    }
    let fin = sb.pointer("/route/final").and_then(Value::as_str).unwrap_or("direct");
    rules.push(format!("MATCH,{}", target(fin)));
    if !ctx.providers.is_empty() {
        conf.insert("rule-providers".into(), Value::Object(ctx.providers.clone()));
    }

    if sniff {
        conf.insert(
            "sniffer".into(),
            json!({
                "enable": true, "parse-pure-ip": true, "override-destination": override_dst,
                "sniff": {
                    "TLS": { "ports": [443, 8443] },
                    "HTTP": { "ports": [80, "8080-8880"], "override-destination": override_dst },
                    "QUIC": { "ports": [443] },
                },
            }),
        );
    }

    if let Some(api) = sb.pointer("/experimental/clash_api") {
        set(&mut conf, "external-controller", s(api, "external_controller"));
        set(&mut conf, "secret", s(api, "secret"));
    }

    // DNS: у роутера его нет (dnsmasq), у клиента — перехват в TUN
    match sb.get("dns") {
        Some(dns) if tun.is_some() || hijack => conf.insert("dns".into(), convert_dns(dns, &ctx)),
        _ => conf.insert("dns".into(), json!({ "enable": false })),
    };
    if let Some(t) = tun {
        let mut o = Map::new();
        o.insert("enable".into(), json!(true));
        o.insert("stack".into(), json!(s(&t, "stack").unwrap_or_else(|| "mixed".into())));
        o.insert("auto-route".into(), json!(t.get("auto_route") == Some(&json!(true))));
        o.insert("strict-route".into(), json!(t.get("strict_route") == Some(&json!(true))));
        o.insert("auto-detect-interface".into(), json!(true));
        set(&mut o, "device", s(&t, "interface_name"));
        o.insert("inet4-address".into(), json!(strs(t.get("address")).into_iter().filter(|a| !a.contains(':')).collect::<Vec<_>>()));
        if hijack {
            o.insert("dns-hijack".into(), json!(["any:53", "tcp://any:53"]));
        }
        conf.insert("tun".into(), Value::Object(o));
    }
    if sb.pointer("/route/auto_detect_interface") == Some(&json!(true)) {
        conf.insert("find-process-mode".into(), json!("strict"));
    }

    if !errors.is_empty() {
        return Err(anyhow!(errors.join("\n")));
    }
    if !proxies.is_empty() {
        conf.insert("proxies".into(), json!(proxies));
    }
    if !listeners.is_empty() {
        conf.insert("listeners".into(), json!(listeners));
    }
    conf.insert("rules".into(), json!(rules));
    Ok(Value::Object(conf))
}

/// DNS клиента: DoH через VPN («remote») и системный («local»), правила по
/// rule-set'ам доменов — nameserver-policy.
fn convert_dns(dns: &Value, ctx: &RuleCtx) -> Value {
    let mut remote = Vec::new();
    for srv in list(dns.get("servers")) {
        if s(&srv, "tag").as_deref() == Some("remote") {
            let host = s(&srv, "server").unwrap_or_else(|| "1.1.1.1".into());
            let via = s(&srv, "detour").unwrap_or_else(|| "proxy".into());
            remote.push(format!("https://{host}/dns-query#{via}"));
        }
    }
    if remote.is_empty() {
        remote.push("https://1.1.1.1/dns-query#proxy".into());
    }
    let local = json!(["system"]);
    let pick = |tag: &str| if tag == "local" { local.clone() } else { json!(remote) };
    let mut policy = Map::new();
    for r in list(dns.get("rules")) {
        let Some(server) = s(&r, "server") else { continue };
        for t in strs(r.get("rule_set")) {
            if let Some(name) = ctx.rule_sets.get(&t) {
                policy.insert(format!("rule-set:{name}"), pick(&server));
            }
        }
    }
    let fin = s(dns, "final").unwrap_or_else(|| "local".into());
    let mut o = json!({
        "enable": true,
        "ipv6": dns.get("strategy") != Some(&json!("ipv4_only")),
        "enhanced-mode": "redir-host",
        "respect-rules": false,
        "nameserver": pick(&fin),
        "proxy-server-nameserver": local,
        "direct-nameserver": ["system"],
    });
    if !policy.is_empty() {
        o["nameserver-policy"] = Value::Object(policy);
    }
    o
}

/// JSON для mihomo: YAML-парсер принимает JSON как есть.
pub fn encode(conf: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(conf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Эталоны генерирует Lua-транслятор роутера (tools/awg-stand/gen_sb2mihomo_fixtures.py):
    /// общая часть обязана совпадать.
    #[test]
    fn matches_router_translator() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sb2mihomo");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).expect("fixtures") {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".sb.json") {
                continue;
            }
            let sb: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let want: Value = serde_json::from_str(
                &std::fs::read_to_string(p.with_file_name(name.replace(".sb.json", ".mihomo.json"))).unwrap(),
            )
            .unwrap();
            let got = convert(&sb, None).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(got, want, "{name}");
            n += 1;
        }
        assert!(n > 0, "нет эталонов");
    }

    #[test]
    fn client_tun_dns_and_rulesets() {
        let dir = std::env::temp_dir().join(format!("detour-mh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rs = dir.join("whitelist.json");
        std::fs::write(&rs, r#"{"version":3,"rules":[{"domain_suffix":["ya.ru"]},{"ip_cidr":["5.5.5.0/24"]}]}"#).unwrap();
        let sb = json!({
            "inbounds": [{ "type": "tun", "tag": "tun-in", "address": ["172.19.0.1/30", "fdfe::1/126"],
                           "auto_route": true, "strict_route": true, "stack": "mixed" }],
            "outbounds": [{ "type": "trojan", "tag": "proxy", "server": "vpn.example.com", "server_port": 443, "password": "x" },
                          { "type": "direct", "tag": "direct" }],
            "route": {
                "rules": [
                    { "action": "sniff" },
                    { "protocol": "dns", "action": "hijack-dns" },
                    { "process_name": ["mihomo.exe"], "outbound": "direct" },
                    { "ip_is_private": true, "outbound": "direct" },
                    { "ip_version": 6, "action": "reject" },
                    { "network": ["udp"], "rule_set": ["whitelist"], "outbound": "direct" },
                    { "network": ["udp"], "port_range": ["27000:27100"], "outbound": "proxy" },
                    { "rule_set": ["whitelist"], "outbound": "direct" },
                ],
                "rule_set": [{ "type": "local", "tag": "whitelist", "format": "source", "path": rs }],
                "final": "proxy", "auto_detect_interface": true,
            },
            "dns": { "strategy": "ipv4_only",
                     "servers": [{ "type": "https", "tag": "remote", "server": "1.1.1.1", "detour": "proxy" },
                                 { "type": "local", "tag": "local" }],
                     "rules": [{ "rule_set": ["whitelist"], "server": "local" }], "final": "remote" },
        });
        let c = convert(&sb, None).unwrap();
        assert_eq!(c["tun"]["enable"], true);
        assert_eq!(c["tun"]["dns-hijack"][0], "any:53");
        assert_eq!(c["dns"]["nameserver"][0], "https://1.1.1.1/dns-query#proxy");
        assert_eq!(c["dns"]["nameserver-policy"]["rule-set:set-whitelist"][0], "system");
        assert_eq!(c["dns"]["ipv6"], false);
        let rules: Vec<String> = c["rules"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_owned()).collect();
        assert_eq!(rules[0], "PROCESS-NAME,mihomo.exe,DIRECT");
        assert!(rules.contains(&"GEOIP,lan,DIRECT,no-resolve".to_owned()));
        assert!(rules.contains(&"IP-CIDR6,::/0,REJECT,no-resolve".to_owned()));
        assert!(rules.contains(&"AND,((NETWORK,UDP),(RULE-SET,set-whitelist)),DIRECT".to_owned()));
        assert!(rules.contains(&"AND,((NETWORK,UDP),(DST-PORT,27000-27100)),proxy".to_owned()));
        assert_eq!(rules.last().unwrap(), "MATCH,proxy");
        assert_eq!(c["rule-providers"]["set-whitelist"]["payload"], json!(["DOMAIN-SUFFIX,ya.ru", "IP-CIDR,5.5.5.0/24"]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
