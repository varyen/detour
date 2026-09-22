//! Страна сервера профиля — порт `detour-geo`: DB-IP Lite (CC-BY 4.0, ссылка
//! на db-ip.com в панели обязательна), CSV `start,end,cc` отсортирован по
//! началу диапазона. Файл (~11 МБ) не сохраняется: читается потоком одним
//! проходом навстречу отсортированным адресам. Это страна ВХОДНОГО узла, а
//! не выхода — панель фильтрует по флагу в имени, а узел показывает отдельно.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::ping;
use crate::profiles;
use crate::store::{now_epoch, Store};

const DB: &str = "lists/geo.json";
const URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/sapics/ip-location-db/main/dbip-country/dbip-country-ipv4.csv",
    "https://cdn.jsdelivr.net/gh/sapics/ip-location-db@main/dbip-country/dbip-country-ipv4.csv",
];
/// База обновляется раз в месяц; чаще перечитывать её незачем.
const FULL_REFRESH: i64 = 30 * 86400;
const MIN_LINES: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    ip: String,
    /// Код страны; `??` — адрес в базе не нашёлся.
    cc: String,
    ts: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct Db {
    #[serde(default)]
    updated: i64,
    #[serde(default)]
    url: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

fn load(store: &Store) -> Db {
    store.read_json(DB).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

fn save(store: &Store, db: &Db) -> Result<()> {
    store.write_json(DB, &serde_json::to_value(db)?)?;
    Ok(())
}

pub fn countries(store: &Store) -> BTreeMap<String, String> {
    load(store)
        .entries
        .into_iter()
        .filter(|(_, e)| e.cc.len() == 2 && e.cc != "??")
        .map(|(id, e)| (id, e.cc))
        .collect()
}

pub fn status(store: &Store) -> Value {
    let db = load(store);
    let known = db.entries.values().filter(|e| e.cc.len() == 2 && e.cc != "??").count();
    json!({
        "supported": true,
        "updated": db.updated,
        "count": db.entries.len(),
        "known": known,
        "url": db.url,
        "error": db.error,
        "attribution": "IP Geolocation by DB-IP",
        "attribution_url": "https://db-ip.com",
    })
}

async fn resolve_all(store: &Store) -> BTreeMap<String, Ipv4Addr> {
    let limit = Arc::new(tokio::sync::Semaphore::new(16));
    let mut set = JoinSet::new();
    for id in profiles::list_ids(store) {
        let Some((host, port)) = profiles::load(store, &id).and_then(|p| ping::endpoint(profiles::outbound(&p)?)) else {
            continue;
        };
        let limit = limit.clone();
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            let addrs = tokio::time::timeout(Duration::from_secs(4), tokio::net::lookup_host((host.as_str(), port))).await;
            let ip = addrs.ok()?.ok()?.find_map(|a| match a.ip() {
                std::net::IpAddr::V4(v4) => Some(v4),
                _ => None,
            })?;
            Some((id, ip))
        });
    }
    let mut out = BTreeMap::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some((id, ip))) = r {
            out.insert(id, ip);
        }
    }
    out
}

fn parse_line(line: &str) -> Option<(u32, u32, &str)> {
    let mut it = line.split(',');
    let a: Ipv4Addr = it.next()?.trim().parse().ok()?;
    let b: Ipv4Addr = it.next()?.trim().parse().ok()?;
    let cc = it.next()?.trim();
    Some((u32::from(a), u32::from(b), cc))
}

/// Один проход по CSV навстречу отсортированным адресам.
async fn lookup(targets: &[u32], proxy: Option<&str>) -> Result<(Vec<String>, String)> {
    let mut last_err = String::new();
    let mut attempts: Vec<(&str, Option<&str>)> = Vec::new();
    for url in URLS {
        if proxy.is_some() {
            attempts.push((url, proxy));
        }
        attempts.push((url, None));
    }
    {
        for (url, p) in attempts {
            let mut b = reqwest::Client::builder().timeout(Duration::from_secs(180));
            b = match p {
                Some(p) => b.proxy(reqwest::Proxy::all(p)?),
                None => b.no_proxy(),
            };
            let Ok(mut resp) = b.build()?.get(url).send().await else {
                last_err = format!("{url}: нет связи");
                continue;
            };
            if !resp.status().is_success() {
                last_err = format!("{url}: HTTP {}", resp.status());
                continue;
            }
            let mut cc = vec!["??".to_owned(); targets.len()];
            let mut i = 0;
            let mut lines = 0usize;
            let mut tail = Vec::<u8>::new();
            'stream: while let Ok(Some(chunk)) = resp.chunk().await {
                tail.extend_from_slice(&chunk);
                let Some(end) = tail.iter().rposition(|b| *b == b'\n') else { continue };
                let block: Vec<u8> = tail.drain(..=end).collect();
                for line in String::from_utf8_lossy(&block).lines() {
                    let Some((start, stop, code)) = parse_line(line) else { continue };
                    lines += 1;
                    while i < targets.len() && targets[i] < start {
                        i += 1;
                    }
                    while i < targets.len() && targets[i] <= stop {
                        cc[i] = code.to_owned();
                        i += 1;
                    }
                    if i >= targets.len() {
                        break 'stream;
                    }
                }
            }
            if lines < MIN_LINES && i < targets.len() {
                last_err = format!("{url}: короткий ответ ({lines} строк) — это не база");
                continue;
            }
            return Ok((cc, url.to_owned()));
        }
    }
    Err(anyhow!(last_err))
}

/// Скан: базу читаем, только если появились неизвестные адреса или прошёл
/// месяц с полного обновления (`force` — перечитать всё сейчас).
pub async fn scan(store: &Store, force: bool, proxy: Option<&str>) -> Result<()> {
    let ips = resolve_all(store).await;
    let mut db = load(store);
    let now = now_epoch();
    let full = force || now - db.updated >= FULL_REFRESH;
    let need: Vec<(String, Ipv4Addr)> = ips
        .iter()
        .filter(|(id, ip)| full || db.entries.get(*id).is_none_or(|e| e.ip != ip.to_string()))
        .map(|(id, ip)| (id.clone(), *ip))
        .collect();
    db.entries.retain(|id, _| ips.contains_key(id));
    if need.is_empty() {
        save(store, &db)?;
        return Ok(());
    }
    let mut order: Vec<(u32, usize)> = need.iter().enumerate().map(|(k, (_, ip))| (u32::from(*ip), k)).collect();
    order.sort();
    let sorted: Vec<u32> = order.iter().map(|(ip, _)| *ip).collect();
    match lookup(&sorted, proxy).await {
        Ok((codes, url)) => {
            for ((_, k), code) in order.iter().zip(codes) {
                let (id, ip) = &need[*k];
                db.entries.insert(id.clone(), Entry { ip: ip.to_string(), cc: code, ts: now });
            }
            if full {
                db.updated = now;
            }
            db.url = url;
            db.error.clear();
            save(store, &db)
        }
        Err(e) => {
            db.error = e.to_string();
            save(store, &db)?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_csv_line() {
        assert_eq!(parse_line("1.0.0.0,1.0.0.255,AU"), Some((16777216, 16777471, "AU")));
        assert!(parse_line("::1,::2,ZZ").is_none());
    }
}
