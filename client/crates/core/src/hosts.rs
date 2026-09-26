//! Приоритетный hosts и шифрование DNS — порт `detour-hosts` и
//! `secure_dns_set`. На роутере список уходит в dnsmasq (`addn-hosts`), и
//! закреплённые имена не попадают в ipset, то есть идут мимо туннеля. Здесь то
//! же выражается в конфиге sing-box: DNS-сервер `hosts` с `predefined`, правило
//! DNS на него и правило маршрута `→ direct` по тем же именам (см. `render`).
//! Шифрование DNS — DoH/DoT-сервер вместо системного для прямого резолва.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::lists;
use crate::settings::{RoutingMode, Settings};
use crate::store::{self, now_epoch, Store};
use crate::subscription;

const CONF: &str = "lists/hosts.json";
/// Последний скачанный или загруженный источник, без фильтра.
const RAW: &str = "lists/hosts-override.raw";
/// Отдаваемый список: отфильтрованный источник + свои записи.
pub const LIST: &str = "lists/hosts-override.list";
pub const CUSTOM: &str = "lists/hosts-custom.list";
/// Тот же источник по умолчанию, что у `detour-hosts` на роутере.
pub const DEFAULT_URL: &str = "https://raw.githubusercontent.com/Internet-Helper/GeoHideDNS/refs/heads/main/hosts/hosts";
/// Роутер обновляет список cron'ом раз в 12 часов.
pub const STALE_AFTER: i64 = 12 * 3600;
const MAX_NAMES: usize = 200_000;

/// Встроенные DoH, когда список в настройке пуст.
const BUILTIN_DOH: [&str; 1] = ["https://1.1.1.1/dns-query"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conf {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub exclude_proxied: bool,
    #[serde(default = "yes")]
    pub custom_enabled: bool,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub bytes: usize,
    #[serde(default)]
    pub excluded: usize,
    #[serde(default)]
    pub updated: i64,
    /// Источник пришёл файлом: плановое обновление по ссылке его не затирает.
    #[serde(default)]
    pub from_file: bool,
    #[serde(default)]
    pub error: String,
}

fn yes() -> bool {
    true
}

impl Default for Conf {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("defaults")
    }
}

pub fn load(store: &Store) -> Conf {
    let mut c: Conf = store.read_json(CONF).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    if c.url.is_empty() {
        c.url = DEFAULT_URL.into();
    }
    c
}

pub fn save(store: &Store, c: &Conf) -> Result<()> {
    store.write_json(CONF, &serde_json::to_value(c)?)?;
    Ok(())
}

pub fn status(store: &Store) -> Value {
    let c = load(store);
    let s = Settings::load(store);
    json!({
        "supported": true,
        "url": c.url,
        "enabled": c.enabled,
        "exclude_proxied": c.exclude_proxied,
        "custom_enabled": c.custom_enabled,
        "count": c.count,
        "bytes": c.bytes,
        "excluded": c.excluded,
        "updated": c.updated,
        "applied": c.enabled && c.count > 0,
        "error": c.error,
        "secure_dns_mode": secure_mode(&s),
        "secure_dns_list": s.get("secure_dns_list").unwrap_or_default(),
    })
}

fn is_host_line(line: &str) -> bool {
    let mut it = line.split_whitespace();
    let (Some(addr), Some(name)) = (it.next(), it.next()) else { return false };
    !addr.is_empty()
        && addr.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

fn count_hosts(text: &str) -> usize {
    text.lines().filter(|l| is_host_line(l)).count()
}

/// Похоже ли скачанное на hosts-файл, а не на страницу ошибки.
pub fn validate(text: &str) -> Result<()> {
    let head: String = text.trim_start().chars().take(32).collect::<String>().to_ascii_lowercase();
    if ["<!doctype", "<html", "<head", "<body"].iter().any(|t| head.contains(t)) {
        bail!("файл не похож на hosts-файл");
    }
    if count_hosts(text) == 0 {
        bail!("файл не похож на hosts-файл");
    }
    Ok(())
}

/// Имена, которые человек сам направил в туннель (или, в режиме «всё, кроме»,
/// сам вывел напрямую): закрепление их за адресом из hosts молча отменило бы
/// этот выбор — как `protected_file` у роутера.
fn protected(store: &Store) -> Vec<String> {
    let file = match Settings::load(store).routing_mode() {
        RoutingMode::AllExcept => store::WHITELIST,
        RoutingMode::ProxyList => store::PROXY_DOMAINS,
    };
    lists::parse_list(&store.read_text(file)).domains
}

fn covered(host: &str, prot: &std::collections::HashSet<String>) -> bool {
    let mut h = host;
    loop {
        if prot.contains(h) {
            return true;
        }
        match h.find('.') {
            Some(i) => h = &h[i + 1..],
            None => return false,
        }
    }
}

/// `RAW` → `LIST` (+ счётчики в `c`), как `do_filter` роутера.
pub fn rebuild(store: &Store, c: &mut Conf) -> Result<()> {
    let raw = store.read_text(RAW);
    let prot: std::collections::HashSet<String> =
        if c.exclude_proxied { protected(store).into_iter().collect() } else { Default::default() };
    let mut out = String::new();
    for line in raw.lines() {
        if is_host_line(line) && !prot.is_empty() {
            let drop = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .skip(1)
                .any(|h| covered(&h.to_ascii_lowercase(), &prot));
            if drop {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    c.excluded = count_hosts(&raw).saturating_sub(count_hosts(&out));
    let custom = store.read_text(CUSTOM);
    if c.custom_enabled && !custom.trim().is_empty() {
        out.push_str("\n# ---- custom entries (detour panel) ----\n");
        out.push_str(&custom);
        if !custom.ends_with('\n') {
            out.push('\n');
        }
    }
    c.count = count_hosts(&out);
    c.bytes = out.len();
    store.write_text(LIST, &out)?;
    Ok(())
}

pub fn set_raw(store: &Store, c: &mut Conf, text: &str, from_file: bool) -> Result<()> {
    validate(text)?;
    store.write_text(RAW, text)?;
    c.updated = now_epoch();
    c.from_file = from_file;
    c.error.clear();
    rebuild(store, c)
}

/// Скачать источник: сначала через VPN (если прокси службы поднят), потом
/// напрямую. Ошибка остаётся в `error`, как у роутера.
pub async fn refresh(store: &Store, proxy: Option<&str>) -> Result<()> {
    let mut c = load(store);
    if !(c.url.starts_with("http://") || c.url.starts_with("https://")) {
        c.error = "нет корректного URL".into();
        save(store, &c)?;
        bail!("{}", c.error);
    }
    let mut errors = Vec::new();
    let attempts: Vec<Option<&str>> = if proxy.is_some() { vec![proxy, None] } else { vec![None] };
    for p in attempts {
        match subscription::fetch(&c.url, "detour-hosts", p, Duration::from_secs(30)).await {
            Ok(f) => match set_raw(store, &mut c, &f.body, false) {
                Ok(()) => {
                    save(store, &c)?;
                    return Ok(());
                }
                Err(e) => errors.push(format!("скачанный {e:#}")),
            },
            Err(e) => errors.push(format!("ошибка загрузки: {e}")),
        }
    }
    errors.dedup();
    c.error = errors.join("; ");
    save(store, &c)?;
    Err(anyhow!("{}", c.error))
}

/// Имя → IPv4-адреса отдаваемого списка, если список включён. Туннель и DNS
/// клиента только IPv4 (`strategy: ipv4_only`), поэтому IPv6 отбрасывается.
pub fn served(store: &Store) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if !load(store).enabled {
        return map;
    }
    for line in store.read_text(LIST).lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut it = line.split_whitespace();
        let Some(addr) = it.next().and_then(|a| a.parse::<std::net::Ipv4Addr>().ok()) else { continue };
        for name in it {
            let n = name.trim_end_matches('.').to_ascii_lowercase();
            let ok = !n.is_empty()
                && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                && n.contains('.');
            if !ok {
                continue;
            }
            if map.len() >= MAX_NAMES && !map.contains_key(&n) {
                break;
            }
            let v = map.entry(n).or_default();
            let a = addr.to_string();
            if !v.contains(&a) {
                v.push(a);
            }
        }
    }
    map
}

// ---------- шифрование DNS ----------

pub fn secure_mode(s: &Settings) -> &'static str {
    if s.get("secure_dns_mode").as_deref() == Some("secure") {
        "secure"
    } else {
        "auto"
    }
}

/// Один адрес списка → DNS-сервер sing-box с тегом `tag`. Понимаются
/// `https://` (DoH) и `tls://` (DoT); имя сервера резолвится системным DNS.
pub fn secure_server(url: &str, tag: &str) -> Result<(Value, bool)> {
    let (kind, rest, def_port) = if let Some(r) = url.strip_prefix("https://") {
        ("https", r, 443u16)
    } else if let Some(r) = url.strip_prefix("tls://") {
        ("tls", r, 853u16)
    } else {
        bail!("{url}: нужен адрес вида https://… или tls://…");
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        let (h, p) = h.split_once(']').ok_or_else(|| anyhow!("{url}: неверный адрес"))?;
        (h, p.strip_prefix(':').map(str::parse::<u16>).transpose().map_err(|_| anyhow!("{url}: неверный порт"))?)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h, Some(p.parse::<u16>().map_err(|_| anyhow!("{url}: неверный порт"))?)),
            None => (hostport, None),
        }
    };
    let is_ip = host.parse::<std::net::IpAddr>().is_ok();
    if host.is_empty() || !(is_ip || lists::is_domain(host)) {
        bail!("{url}: неверное имя сервера");
    }
    let mut v = json!({ "type": kind, "tag": tag, "server": host });
    if let Some(p) = port.filter(|p| *p != def_port) {
        v["server_port"] = json!(p);
    }
    if kind == "https" && !path.is_empty() && path != "/dns-query" {
        v["path"] = json!(path);
    }
    Ok((v, !is_ip))
}

pub fn parse_secure_list(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// Адреса, которыми шифруется прямой резолв: из настройки или встроенные.
pub fn secure_urls(s: &Settings) -> Vec<String> {
    let list = parse_secure_list(&s.get("secure_dns_list").unwrap_or_default());
    if list.is_empty() {
        BUILTIN_DOH.iter().map(|u| u.to_string()).collect()
    } else {
        list
    }
}

pub fn set_secure(store: &Store, mode: &str, list: &str) -> Result<()> {
    if !matches!(mode, "secure" | "auto") {
        bail!("invalid mode");
    }
    let urls = parse_secure_list(list);
    if mode == "secure" {
        for u in &urls {
            secure_server(u, "check")?;
        }
    }
    let mut s = Settings::load(store);
    s.set("secure_dns_mode", mode);
    s.set("secure_dns_list", if mode == "secure" { urls.join(" ") } else { String::new() });
    s.save(store)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("detour-hosts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store::new(dir)
    }

    #[test]
    fn filter_excludes_proxied_and_keeps_custom() {
        let s = tmp_store("filter");
        s.write_text(store::PROXY_DOMAINS, "vpn.example.com\n").unwrap();
        s.write_text(CUSTOM, "203.0.113.20 mine.example.com\n").unwrap();
        let mut c = load(&s);
        c.enabled = true;
        set_raw(&s, &mut c, "# hosts\n203.0.113.1 a.example.com\n203.0.113.2 cdn.vpn.example.com\n2001:db8::1 six.example.com\n", false).unwrap();
        save(&s, &c).unwrap();
        assert_eq!(c.excluded, 1);
        assert_eq!(c.count, 3);
        let m = served(&s);
        assert_eq!(m.get("a.example.com"), Some(&vec!["203.0.113.1".to_owned()]));
        assert_eq!(m.get("mine.example.com"), Some(&vec!["203.0.113.20".to_owned()]));
        assert!(!m.contains_key("cdn.vpn.example.com"));
        assert!(!m.contains_key("six.example.com"), "IPv6 в туннель не идёт");

        c.exclude_proxied = false;
        c.custom_enabled = false;
        rebuild(&s, &mut c).unwrap();
        save(&s, &c).unwrap();
        let m = served(&s);
        assert!(m.contains_key("cdn.vpn.example.com"));
        assert!(!m.contains_key("mine.example.com"));
    }

    #[test]
    fn rejects_html_and_garbage() {
        assert!(validate("<!DOCTYPE html><html>").is_err());
        assert!(validate("just text\n").is_err());
        assert!(validate("203.0.113.1 a.example.com\n").is_ok());
    }

    #[test]
    fn secure_urls_become_servers() {
        let (v, needs) = secure_server("https://dns.example.com/dns-query", "local").unwrap();
        assert_eq!(v, json!({ "type": "https", "tag": "local", "server": "dns.example.com" }));
        assert!(needs);
        let (v, needs) = secure_server("https://203.0.113.53:8443/q", "local").unwrap();
        assert_eq!(v, json!({ "type": "https", "tag": "local", "server": "203.0.113.53", "server_port": 8443, "path": "/q" }));
        assert!(!needs);
        let (v, _) = secure_server("tls://dns.example.com", "local").unwrap();
        assert_eq!(v["type"], "tls");
        assert!(secure_server("udp://203.0.113.53", "x").is_err());
        assert!(secure_server("https://", "x").is_err());
    }
}
