//! Российские подсети — порт `detour-rulist`. На роутере они живут в ipset
//! белого списка, здесь — rule-set `ru-subnets` (→ direct), а исключения
//! (у ipset это `nomatch`) — правило `ip_cidr → proxy` выше него.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::lists;
use crate::store::{self, now_epoch, Store};
use crate::subscription;

const CONF: &str = "lists/rulist.json";
const MIN_SUBNETS: usize = 1000;
/// Автообновление — не чаще раза в шесть суток.
pub const STALE_AFTER: i64 = 6 * 86400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conf {
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub url: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub auto: bool,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub updated: i64,
    #[serde(default = "never")]
    pub migrated: i64,
    #[serde(default)]
    pub error: String,
}

fn default_source() -> String {
    "maxmind".into()
}
fn yes() -> bool {
    true
}
fn never() -> i64 {
    -1
}

impl Default for Conf {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("defaults")
    }
}

pub fn load(store: &Store) -> Conf {
    let mut c: Conf = store.read_json(CONF).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    if !matches!(c.source.as_str(), "maxmind" | "rir") {
        c.source = default_source();
    }
    c
}

pub fn save(store: &Store, c: &Conf) -> Result<()> {
    store.write_json(CONF, &serde_json::to_value(c)?)?;
    Ok(())
}

pub fn urls(source: &str) -> [&'static str; 2] {
    match source {
        "rir" => [
            "https://www.ipdeny.com/ipblocks/data/aggregated/ru-aggregated.zone",
            "https://raw.githubusercontent.com/ipverse/rir-ip/master/country/ru/ipv4-aggregated.txt",
        ],
        _ => [
            "https://raw.githubusercontent.com/Loyalsoldier/geoip/release/text/ru.txt",
            "https://cdn.jsdelivr.net/gh/Loyalsoldier/geoip@release/text/ru.txt",
        ],
    }
}

pub fn label(source: &str) -> &'static str {
    match source {
        "rir" => "RIPE/RIR (регистрация)",
        _ => "MaxMind GeoLite2 (геолокация)",
    }
}

/// Работает ли список прямо сейчас: включён и не пуст.
pub fn active(store: &Store) -> bool {
    load(store).enabled && store.exists(store::RU_SUBNETS)
}

pub fn status(store: &Store) -> Value {
    let c = load(store);
    let excluded = lists::parse_list(&store.read_text(store::RU_EXCLUDE)).cidrs.len();
    json!({
        "supported": true,
        "source": c.source,
        "source_label": label(&c.source),
        "url": c.url,
        "enabled": c.enabled,
        "auto": c.auto,
        "count": c.count,
        "updated": c.updated,
        "migrated": c.migrated,
        "excluded": excluded,
        "live_entries": if c.enabled { c.count } else { 0 },
        "error": c.error,
    })
}

/// Только IPv4 /8–/32, как у роутера: IPv6 и мусор отбрасываются.
pub fn parse(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let Some(c) = lists::parse_cidr(t) else { continue };
        let Some((addr, len)) = c.split_once('/') else { continue };
        let ok_len = len.parse::<u8>().is_ok_and(|l| (8..=32).contains(&l));
        if addr.parse::<std::net::Ipv4Addr>().is_ok() && ok_len && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

pub async fn update(store: &Store, source: &str, proxy: Option<&str>) -> Result<usize> {
    let mut errors = Vec::new();
    let mut attempts: Vec<(&str, Option<&str>)> = Vec::new();
    for url in urls(source) {
        if proxy.is_some() {
            attempts.push((url, proxy));
        }
        attempts.push((url, None));
    }
    for (url, p) in attempts {
        match subscription::fetch(url, "detour", p, Duration::from_secs(40)).await {
            Ok(f) => {
                let nets = parse(&f.body);
                if nets.len() < MIN_SUBNETS {
                    errors.push(format!("{url}: всего {} подсетей — похоже, это не список", nets.len()));
                    continue;
                }
                let head = format!(
                    "// Российские подсети — список ведёт Detour, ручные правки затрутся.\n// Источник: {}\n// {url}\n// Обновлено: {}\n",
                    label(source),
                    humantime::format_rfc3339_seconds(std::time::SystemTime::now())
                );
                store.write_text(store::RU_SUBNETS, &(head + &nets.join("\n") + "\n"))?;
                let mut c = load(store);
                c.source = source.into();
                c.url = url.into();
                c.count = nets.len();
                c.updated = now_epoch();
                c.error.clear();
                save(store, &c)?;
                return Ok(nets.len());
            }
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }
    let msg = errors.join("; ");
    let mut c = load(store);
    c.error = msg.clone();
    save(store, &c)?;
    Err(anyhow!(msg))
}

pub fn apply_settings(store: &Store, body: &Value) -> Result<(Conf, bool)> {
    let mut c = load(store);
    let was = c.enabled;
    if let Some(s) = body.get("source").and_then(Value::as_str) {
        if !matches!(s, "maxmind" | "rir") {
            bail!("invalid source");
        }
        c.source = s.into();
    }
    if let Some(b) = body.get("auto").and_then(Value::as_bool) {
        c.auto = b;
    }
    if let Some(b) = body.get("enabled").and_then(Value::as_bool) {
        c.enabled = b;
    }
    save(store, &c)?;
    let changed = c.enabled != was;
    Ok((c, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_ipv4_prefixes() {
        let v = parse("# header\n5.3.0.0/16\n2a00::/16\n10.0.0.0/7\n77.88.8.8\nmusor\n5.3.0.0/16\n");
        assert_eq!(v, vec!["5.3.0.0/16", "77.88.8.8/32"]);
    }
}
