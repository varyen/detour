//! Сборка конфига sing-box для TUN.
//!
//! На роутере решение «VPN или напрямую» принимает файрвол по ipset, которые
//! наполняет dnsmasq, а в конфиге sing-box есть только sniff, карта маршрутов
//! и `final: proxy`. В клиенте файрвола нет: весь этот слой — режимы
//! `proxy-list`/`all-except`, whitelist, RU-подсети с исключениями,
//! `udp_vpn_mode`, «Все через VPN», блок-лист исходящих, fail-closed для
//! пропавших целей — выражается правилами маршрута и перехватом DNS.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use crate::awg::{self, Sidecars};
use crate::chains::ChainStore;
use crate::lists::{self, Matchers, UdpItem};
use crate::profiles;
use crate::settings::{RoutingMode, Settings, UdpMode};
use crate::store::{self, Store};

pub const TUN_NAME: &str = "Detour";
/// Адрес TUN: его исключает фильтр winws2, чтобы не ловить пакет дважды.
pub const TUN_GATEWAY: &str = "172.19.0.1";
/// Локальный вход в VPN для самой службы (подписки, проверки): так же, как
/// `render-mixed` на роутере, только постоянный и с паролем — иначе любой
/// процесс на машине мог бы ходить в туннель мимо правил.
pub const FETCH_PORT: u16 = 19484;
pub const FETCH_USER: &str = "detour";
const QUIC_TYPES: [&str; 3] = ["hysteria", "hysteria2", "tuic"];
const PROXY_TYPES: [&str; 7] = ["socks", "socks4", "socks4a", "socks5", "http", "http-proxy", "https-proxy"];
const REMOTE_DNS: &str = "1.1.1.1";

pub struct Params<'a> {
    pub chain: &'a [String],
    pub log_path: &'a Path,
    /// Каталог, куда пишутся rule-set файлы и на который ссылается конфиг.
    pub ruleset_dir: &'a Path,
    pub clash_port: u16,
    pub clash_secret: &'a str,
    /// Куда уходят домены DPI-обхода, пока движок работает: на Windows —
    /// напрямую (winws2 правит их на проводе), на macOS — в локальный SOCKS
    /// tpws. Выключенный движок = `DpiRoute::Off`, домены идут обычным путём.
    pub dpi: DpiRoute,
    /// Без TUN — только для разработки без прав администратора
    /// (`DETOUR_DEV_NO_TUN`): вместо туннеля вход `dev-in`, чей трафик идёт
    /// по тем же правилам, что шёл бы из TUN.
    pub tun: bool,
    /// Движок mihomo: AmneziaWG-профили идут в конфиг как есть (их переведёт
    /// `mihomo::convert`), сайдкар не нужен и ограничения «первым звеном» нет.
    pub awg_inline: bool,
}

pub const DEV_PORT: u16 = 18282;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DpiRoute {
    Off,
    Direct,
    Socks(u16),
}

pub struct Rendered {
    pub config: Value,
    pub rulesets: Vec<(PathBuf, Value)>,
    /// AmneziaWG-профили этой сборки — их поднимает сайдкар mihomo.
    pub awg: Sidecars,
}

#[derive(Clone)]
pub enum Hop {
    Outbound(Value),
    Endpoint(Value),
}

fn hop(store: &Store, id: &str, tag: &str, detour: Option<&str>, awg: &mut Sidecars, inline: bool) -> Result<Hop> {
    let profile = profiles::load(store, id).ok_or_else(|| anyhow!("профиль {id} не найден"))?;
    let mut ob = profiles::outbound(&profile)
        .cloned()
        .ok_or_else(|| anyhow!("у профиля {id} нет параметров подключения"))?;
    if awg::is_awg(&ob) && inline {
        let o = ob.as_object_mut().ok_or_else(|| anyhow!("outbound не объект"))?;
        o.insert("tag".into(), json!(tag));
        match detour {
            Some(d) => o.insert("detour".into(), json!(d)),
            None => o.remove("detour"),
        };
        return Ok(Hop::Outbound(ob));
    }
    if awg::is_awg(&ob) {
        if !awg::SUPPORTED {
            bail!("профиль {id}: AmneziaWG на этой платформе недоступен");
        }
        // socks на 127.0.0.1 через предыдущий хоп ушёл бы на удалённую сторону
        if detour.is_some() {
            bail!("AmneziaWG-профиль {id} может быть только первым звеном цепочки");
        }
        ob = awg::socks_outbound(awg.port_for(id, &ob));
    }
    prepare(ob, tag, detour).map_err(|e| anyhow!("профиль {id}: {e}"))
}

/// Outbound профиля с тегом и связкой `detour`, с обходами граблей 1.13:
/// uTLS на QUIC ломает каждый dial, WireGuard — только endpoint.
pub fn prepare(mut ob: Value, tag: &str, detour: Option<&str>) -> Result<Hop> {
    let obj = ob.as_object_mut().ok_or_else(|| anyhow!("outbound не объект"))?;
    obj.insert("tag".into(), json!(tag));
    match detour {
        Some(d) => obj.insert("detour".into(), json!(d)),
        None => obj.remove("detour"),
    };
    let t = obj.get("type").and_then(Value::as_str).unwrap_or("").to_owned();
    if t == awg::TYPE {
        bail!("AmneziaWG работает только через сайдкар mihomo");
    }
    if QUIC_TYPES.contains(&t.as_str()) {
        if let Some(tls) = obj.get_mut("tls").and_then(Value::as_object_mut) {
            tls.remove("utls");
        }
    }
    if t == "wireguard" {
        return to_endpoint(obj).map(Hop::Endpoint);
    }
    Ok(Hop::Outbound(ob))
}

/// Плоский WireGuard из формы панели → endpoint 1.13. Конвертер роутера
/// (`translate_wireguard_profile`) только переименовывал ключи и давал
/// невалидный endpoint; здесь — полная сборка `peers`.
fn to_endpoint(obj: &mut Map<String, Value>) -> Result<Value> {
    if let Some(la) = obj.remove("local_address") {
        obj.entry("address").or_insert(la);
    }
    let top_allowed = obj.remove("allowed_ips");
    if !obj.contains_key("peers") {
        let Some(server) = obj.remove("server") else {
            bail!("у WireGuard нет ни peers, ни server");
        };
        let mut peer = Map::new();
        peer.insert("address".into(), server);
        if let Some(p) = obj.remove("server_port") {
            peer.insert("port".into(), p);
        }
        if let Some(k) = obj.remove("peer_public_key") {
            peer.insert("public_key".into(), k);
        }
        for k in ["pre_shared_key", "reserved"] {
            if let Some(v) = obj.remove(k) {
                peer.insert(k.into(), v);
            }
        }
        peer.insert(
            "allowed_ips".into(),
            top_allowed.unwrap_or_else(|| json!(["0.0.0.0/0", "::/0"])),
        );
        obj.insert("peers".into(), json!([peer]));
    }
    for k in ["server_port", "peer_public_key", "pre_shared_key", "reserved", "system_interface", "interface_name", "gso", "network"] {
        obj.remove(k);
    }
    Ok(Value::Object(obj.clone()))
}

fn ruleset(tag: &str, m: &Matchers, dir: &Path) -> (Value, (PathBuf, Value)) {
    let mut rules = Vec::new();
    if !m.domains.is_empty() {
        rules.push(json!({ "domain_suffix": m.domains }));
    }
    if !m.cidrs.is_empty() {
        rules.push(json!({ "ip_cidr": m.cidrs }));
    }
    let path = dir.join(format!("{tag}.json"));
    let def = json!({ "type": "local", "tag": tag, "format": "source", "path": path });
    (def, (path, json!({ "version": 3, "rules": rules })))
}

struct Builder {
    outbounds: Vec<Value>,
    endpoints: Vec<Value>,
    awg: Sidecars,
}

impl Builder {
    fn push(&mut self, h: Hop) {
        match h {
            Hop::Outbound(v) => self.outbounds.push(v),
            Hop::Endpoint(v) => self.endpoints.push(v),
        }
    }
}

pub fn render(store: &Store, settings: &Settings, p: &Params) -> Result<Rendered> {
    if p.chain.is_empty() {
        bail!("no active profile");
    }
    let mut b = Builder { outbounds: Vec::new(), endpoints: Vec::new(), awg: Sidecars::default() };
    let n = p.chain.len();
    for (i, id) in p.chain.iter().enumerate() {
        let tag = if i + 1 == n { "proxy".to_owned() } else { format!("chain_{}", i + 1) };
        let detour = (i > 0).then(|| format!("chain_{i}"));
        let h = hop(store, id, &tag, detour.as_deref(), &mut b.awg, p.awg_inline)?;
        b.push(h);
    }

    let mut rules: Vec<Value> = vec![
        json!({ "action": "sniff" }),
        json!({ "inbound": ["fetch-in"], "outbound": "proxy" }),
        json!({ "protocol": "dns", "action": "hijack-dns" }),
    ];

    let egress = lists::parse_list(&store.read_text(store::EGRESS_BLOCK));
    if !egress.cidrs.is_empty() {
        rules.push(json!({ "ip_cidr": egress.cidrs, "action": "reject" }));
    }
    rules.push(json!({ "ip_is_private": true, "outbound": "direct" }));
    // Туннель только IPv4: TUN принимает TCP-рукопожатие сам, и если у
    // провайдера или у VPN-сервера нет IPv6, приложение не откатится на IPv4 —
    // соединение просто рвётся. DNS отдаёт только A, а IPv6, пришедший в TUN
    // по литералу, получает RST и мимо туннеля не уходит.
    rules.push(json!({ "ip_version": 6, "action": "reject" }));

    let allow = lists::parse_id_list(&store.read_text(store::TORRENT_ALLOW));
    if p.chain.iter().any(|h| !allow.contains(h)) {
        // UDP-детектор uTP ложно срабатывает на handshake WireGuard.
        rules.push(json!({ "protocol": ["bittorrent"], "network": ["tcp"], "action": "reject" }));
    }

    route_targets(store, p.chain, &mut b, &mut rules, p.awg_inline)?;

    // Сам mihomo ходит к AWG-серверу наружу, и без этого правила его UDP
    // вернулся бы в TUN и дальше в тот же socks — петля. Правило нужно и без
    // AWG в цепочке: временный mihomo проверки профилей должен ходить мимо
    // текущего VPN. На Android петлю режет исключение приложения из VpnService.
    if !cfg!(any(target_os = "android", target_os = "ios")) {
        rules.insert(3, json!({ "process_name": [awg::EXE], "outbound": "direct" }));
    }

    let mut rule_sets = Vec::new();
    let mut files = Vec::new();
    // Для DNS-правил — отдельный набор только из доменов: `ip_cidr` в DNS
    // сверяется с ответом, а не с запросом, и смысл списка поплыл бы.
    let mut add_set = |tag: &str, m: &Matchers| -> bool {
        if m.is_empty() {
            return false;
        }
        let (def, file) = ruleset(tag, m, p.ruleset_dir);
        rule_sets.push(def);
        files.push(file);
        if !m.domains.is_empty() {
            let only = Matchers { domains: m.domains.clone(), cidrs: Vec::new() };
            let (def, file) = ruleset(&format!("{tag}-dns"), &only, p.ruleset_dir);
            rule_sets.push(def);
            files.push(file);
        }
        true
    };

    if p.dpi != DpiRoute::Off && add_set("dpi", &lists::parse_list(&store.read_text(store::DPI_DOMAINS))) {
        if let DpiRoute::Socks(port) = p.dpi {
            b.outbounds.push(json!({
                "type": "socks", "tag": "dpi", "server": "127.0.0.1", "server_port": port, "version": "5",
            }));
            // Сам tpws ходит наружу через тот же туннель, и его соединение
            // снова попадает в правило обхода — петля. На Android от неё
            // спасает исключение своего пакета из VpnService, на macOS —
            // отбор по имени процесса.
            if !cfg!(any(target_os = "android", target_os = "ios")) {
                rules.push(json!({ "process_name": [crate::dpi::EXE], "outbound": "direct" }));
            }
        }
        let out = if matches!(p.dpi, DpiRoute::Socks(_)) { "dpi" } else { "direct" };
        rules.push(json!({ "rule_set": ["dpi"], "outbound": out }));
    }

    let mode = settings.routing_mode();
    let allvpn = settings.allvpn();
    let whitelist = lists::parse_list(&store.read_text(store::WHITELIST));
    let has_whitelist = add_set("whitelist", &whitelist);
    let has_ru = crate::rulist::load(store).enabled
        && add_set("ru-subnets", &lists::parse_list(&store.read_text(store::RU_SUBNETS)));
    let ru_exclude = lists::parse_list(&store.read_text(store::RU_EXCLUDE));
    let mut direct_sets: Vec<&str> = Vec::new();
    if has_whitelist {
        direct_sets.push("whitelist");
    }
    if has_ru {
        direct_sets.push("ru-subnets");
    }

    match settings.udp_mode() {
        UdpMode::Off => rules.push(json!({ "network": ["udp"], "outbound": "direct" })),
        UdpMode::List => {
            rules.extend(udp_list_rules(&lists::parse_udp_list(&store.read_text(store::UDP_VPN))));
            rules.push(json!({ "network": ["udp"], "outbound": "direct" }));
        }
        UdpMode::All if !allvpn && mode == RoutingMode::ProxyList => {
            if !direct_sets.is_empty() {
                rules.push(json!({ "network": ["udp"], "rule_set": direct_sets, "outbound": "direct" }));
            }
            rules.push(json!({ "network": ["udp"], "outbound": "proxy" }));
        }
        UdpMode::All => {}
    }

    let mut dns_rules: Vec<Value> = Vec::new();
    let (final_out, dns_final) = if allvpn {
        ("proxy", "remote")
    } else if mode == RoutingMode::AllExcept {
        if !ru_exclude.cidrs.is_empty() {
            rules.push(json!({ "ip_cidr": ru_exclude.cidrs, "outbound": "proxy" }));
        }
        if !direct_sets.is_empty() {
            rules.push(json!({ "rule_set": direct_sets, "outbound": "direct" }));
        }
        if !whitelist.domains.is_empty() {
            dns_rules.push(json!({ "rule_set": ["whitelist-dns"], "server": "local" }));
        }
        ("proxy", "remote")
    } else {
        let proxied = lists::parse_list(&store.read_text(store::PROXY_DOMAINS));
        if add_set("proxy-domains", &proxied) {
            rules.push(json!({ "rule_set": ["proxy-domains"], "outbound": "proxy" }));
            if !proxied.domains.is_empty() {
                dns_rules.push(json!({ "rule_set": ["proxy-domains-dns"], "server": "remote" }));
            }
        }
        ("direct", "local")
    };

    b.outbounds.push(json!({ "type": "direct", "tag": "direct" }));

    let mut tun = json!({
        "type": "tun",
        "tag": "tun-in",
        "address": [format!("{TUN_GATEWAY}/30"), "fdfe:dcba:9876::1/126"],
        "auto_route": true,
        "strict_route": true,
        "stack": "mixed",
    });
    if cfg!(windows) {
        tun["interface_name"] = json!(TUN_NAME);
    }
    let main_in = if p.tun {
        tun
    } else {
        json!({ "type": "mixed", "tag": "dev-in", "listen": "127.0.0.1", "listen_port": DEV_PORT })
    };

    let mut route = json!({
        "rules": rules,
        "final": final_out,
        "auto_detect_interface": true,
        "default_domain_resolver": "local",
    });
    if !rule_sets.is_empty() {
        route["rule_set"] = json!(rule_sets);
    }

    let mut config = json!({
        "log": { "level": "warn", "output": p.log_path, "timestamp": true },
        "dns": {
            "strategy": "ipv4_only",
            "servers": [
                { "type": "https", "tag": "remote", "server": REMOTE_DNS, "detour": "proxy" },
                { "type": "local", "tag": "local" },
            ],
            "rules": dns_rules,
            "final": dns_final,
        },
        "inbounds": [
            main_in,
            {
                "type": "mixed",
                "tag": "fetch-in",
                "listen": "127.0.0.1",
                "listen_port": FETCH_PORT,
                "users": [{ "username": FETCH_USER, "password": p.clash_secret }],
            },
        ],
        "outbounds": b.outbounds,
        "route": route,
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", p.clash_port),
                "secret": p.clash_secret,
            }
        },
    });
    if !b.endpoints.is_empty() {
        config["endpoints"] = json!(b.endpoints);
    }
    Ok(Rendered { config, rulesets: files, awg: b.awg })
}

/// Карта маршрутов. Цель — профиль или цепочка; пропавшая цель не уходит в
/// `final`, а режется (fail-closed, как «мёртвый» порт 12499 на роутере).
fn route_targets(store: &Store, active: &[String], b: &mut Builder, rules: &mut Vec<Value>, inline: bool) -> Result<()> {
    let sections = lists::parse_route_map(&store.read_text(store::ROUTE_MAP));
    let chains = ChainStore::load(store);
    let mut n = 0usize;
    for sec in sections.iter().filter(|s| !s.is_empty()) {
        let chain = chains.get(&sec.target);
        let is_profile = profiles::exists(store, &sec.target);
        if chain.is_none() && !is_profile {
            let mut all: Vec<String> = sec.exact.clone();
            all.extend(sec.suffix.iter().cloned());
            if !all.is_empty() {
                rules.push(json!({ "domain_suffix": all, "action": "reject" }));
            }
            if !sec.cidrs.is_empty() {
                rules.push(json!({ "ip_cidr": sec.cidrs, "action": "reject" }));
            }
            continue;
        }
        n += 1;

        let (tag, exit_id) = if let Some(c) = chain {
            let last = c.hops.len();
            for (i, id) in c.hops.iter().enumerate() {
                let tag = if i + 1 == last { format!("out_{}", c.id) } else { format!("rc{n}_{}", i + 1) };
                let detour = (i > 0).then(|| format!("rc{n}_{i}"));
                let h = hop(store, id, &tag, detour.as_deref(), &mut b.awg, inline)?;
                b.push(h);
            }
            (format!("out_{}", c.id), c.hops.last().cloned().unwrap_or_default())
        } else if let Some(pos) = active.iter().position(|h| *h == sec.target) {
            let tag = if pos + 1 == active.len() { "proxy".to_owned() } else { format!("chain_{}", pos + 1) };
            (tag, sec.target.clone())
        } else {
            let tag = format!("out_{}", sec.target);
            // «через цепочку» для AWG невозможно — цель идёт прямо в свой туннель
            let target_awg = profiles::load(store, &sec.target)
                .and_then(|p| profiles::outbound(&p).map(awg::is_awg))
                .unwrap_or(false);
            let detour = (sec.via_chain && (inline || !target_awg)).then_some("proxy");
            let h = hop(store, &sec.target, &tag, detour, &mut b.awg, inline)?;
            b.push(h);
            (tag, sec.target.clone())
        };

        let exit_type = profiles::load(store, &exit_id)
            .map(|p| profiles::profile_type(&p))
            .unwrap_or_default();
        if PROXY_TYPES.contains(&exit_type.as_str()) {
            // HTTP/SOCKS-прокси нужен CONNECT по имени, а в TUN назначение —
            // уже IP; для точных имён подменяем адрес на имя.
            for d in &sec.exact {
                rules.push(json!({ "action": "route", "domain": [d], "outbound": tag, "override_address": d }));
            }
            if !sec.suffix.is_empty() {
                rules.push(json!({ "domain_suffix": sec.suffix, "outbound": tag }));
            }
        } else {
            let mut all = sec.exact.clone();
            all.extend(sec.suffix.iter().cloned());
            if !all.is_empty() {
                rules.push(json!({ "domain_suffix": all, "outbound": tag }));
            }
        }
        if !sec.cidrs.is_empty() {
            rules.push(json!({ "ip_cidr": sec.cidrs, "outbound": tag }));
        }
    }
    Ok(())
}

fn udp_list_rules(items: &[UdpItem]) -> Vec<Value> {
    let mut cidrs = Vec::new();
    let mut domains = Vec::new();
    let mut ports = Vec::new();
    let mut out = Vec::new();
    for it in items {
        match it {
            UdpItem::Cidr(c) => cidrs.push(c.clone()),
            UdpItem::Domain(d) => domains.push(d.clone()),
            UdpItem::Ports(a, z) => ports.push(port_range(*a, *z)),
            UdpItem::CidrPorts(c, a, z) => out.push(json!({
                "network": ["udp"], "ip_cidr": [c], "port_range": [port_range(*a, *z)], "outbound": "proxy",
            })),
        }
    }
    if !cidrs.is_empty() {
        out.push(json!({ "network": ["udp"], "ip_cidr": cidrs, "outbound": "proxy" }));
    }
    if !domains.is_empty() {
        out.push(json!({ "network": ["udp"], "domain_suffix": domains, "outbound": "proxy" }));
    }
    if !ports.is_empty() {
        out.push(json!({ "network": ["udp"], "port_range": ports, "outbound": "proxy" }));
    }
    out
}

fn port_range(a: u16, z: u16) -> String {
    format!("{a}:{z}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("detour-render-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        Store::new(dir)
    }

    fn put_profile(s: &Store, id: &str, ob: Value) {
        s.write_json(&format!("profiles/{id}.json"), &json!({ "id": id, "name": id, "outbound": ob })).unwrap();
    }

    fn params<'a>(chain: &'a [String], dir: &'a Path) -> Params<'a> {
        Params { chain, log_path: dir, ruleset_dir: dir, clash_port: 19090, clash_secret: "s", dpi: DpiRoute::Off, tun: true, awg_inline: false }
    }

    #[test]
    fn chain_links_detour_and_strips_quic_utls() {
        let s = tmp_store("chain");
        put_profile(&s, "a", json!({ "type": "vless", "server": "vpn.example.com", "server_port": 443, "detour": "x" }));
        put_profile(&s, "b", json!({ "type": "hysteria2", "server": "vpn.example.com", "tls": { "enabled": true, "utls": { "enabled": true } } }));
        let chain = vec!["a".to_owned(), "b".to_owned()];
        let r = render(&s, &Settings::default(), &params(&chain, s.root())).unwrap();
        let obs = r.config["outbounds"].as_array().unwrap();
        assert_eq!(obs[0]["tag"], "chain_1");
        assert!(obs[0].get("detour").is_none());
        assert_eq!(obs[1]["tag"], "proxy");
        assert_eq!(obs[1]["detour"], "chain_1");
        assert!(obs[1]["tls"].get("utls").is_none());
        assert_eq!(r.config["route"]["final"], "direct");
        assert_eq!(r.config["dns"]["strategy"], "ipv4_only");
        let rules = r.config["route"]["rules"].as_array().unwrap();
        let private = rules.iter().position(|x| x["ip_is_private"] == true).unwrap();
        assert_eq!(rules[private + 1], json!({ "ip_version": 6, "action": "reject" }));
    }

    #[test]
    fn dpi_route_direct_or_socks() {
        let s = tmp_store("dpi-route");
        put_profile(&s, "a", json!({ "type": "trojan", "server": "vpn.example.com", "server_port": 443 }));
        s.write_text(store::DPI_DOMAINS, "blocked.example.com
").unwrap();
        let chain = vec!["a".to_owned()];
        let rule_out = |p: &Params| -> String {
            let r = render(&s, &Settings::default(), p).unwrap();
            let rules = r.config["route"]["rules"].as_array().unwrap().clone();
            let hit = rules.iter().find(|x| x["rule_set"] == json!(["dpi"]));
            let out = hit.map(|x| x["outbound"].as_str().unwrap_or("").to_owned()).unwrap_or_default();
            let has_socks = r.config["outbounds"].as_array().unwrap().iter().any(|o| o["tag"] == "dpi");
            format!("{out}{}", if has_socks { "+socks" } else { "" })
        };
        let mut p = params(&chain, s.root());
        assert_eq!(rule_out(&p), "", "выключенный движок не трогает маршруты");
        p.dpi = DpiRoute::Direct;
        assert_eq!(rule_out(&p), "direct", "winws2 правит трафик на проводе");
        p.dpi = DpiRoute::Socks(19487);
        assert_eq!(rule_out(&p), "dpi+socks", "tpws принимает соединения сам");

        let r = render(&s, &Settings::default(), &p).unwrap();
        let rules = r.config["route"]["rules"].as_array().unwrap();
        let own = rules.iter().position(|x| x["process_name"] == json!([crate::dpi::EXE]));
        let dpi = rules.iter().position(|x| x["rule_set"] == json!(["dpi"])).unwrap();
        if cfg!(any(target_os = "android", target_os = "ios")) {
            assert!(own.is_none(), "на Android петлю режет VpnService");
        } else {
            assert!(own.unwrap() < dpi, "трафик самого tpws уходит напрямую до правила обхода");
        }
    }

    #[test]
    fn flat_wireguard_becomes_endpoint() {
        let s = tmp_store("wg");
        put_profile(&s, "w", json!({
            "type": "wireguard", "server": "vpn.example.com", "server_port": 51820,
            "private_key": "k", "peer_public_key": "p", "local_address": ["10.0.0.2/32"], "mtu": 1280,
        }));
        let chain = vec!["w".to_owned()];
        let r = render(&s, &Settings::default(), &params(&chain, s.root())).unwrap();
        let ep = &r.config["endpoints"][0];
        assert_eq!(ep["tag"], "proxy");
        assert_eq!(ep["address"], json!(["10.0.0.2/32"]));
        assert_eq!(ep["peers"][0]["address"], "vpn.example.com");
        assert_eq!(ep["peers"][0]["public_key"], "p");
        assert!(ep.get("server").is_none() && ep.get("peer_public_key").is_none());
    }

    #[test]
    fn amneziawg_becomes_socks_to_sidecar() {
        let s = tmp_store("awg");
        put_profile(&s, "a", json!({
            "type": "amneziawg", "server": "203.0.113.5", "server_port": 51820,
            "private_key": "k", "peer_public_key": "p", "local_address": ["10.8.1.2/32"],
            "amnezia": { "jc": 4 },
        }));
        put_profile(&s, "b", json!({ "type": "vless", "server": "vpn.example.com", "server_port": 443 }));
        let chain = vec!["a".to_owned(), "b".to_owned()];
        let r = render(&s, &Settings::default(), &params(&chain, s.root())).unwrap();
        let obs = r.config["outbounds"].as_array().unwrap();
        assert_eq!(obs[0], json!({ "type": "socks", "server": "127.0.0.1", "server_port": awg::PORT_BASE, "version": "5", "tag": "chain_1" }));
        assert_eq!(obs[1]["detour"], "chain_1");
        assert_eq!(r.awg.list.len(), 1);
        let rules = r.config["route"]["rules"].as_array().unwrap();
        let own = rules.iter().any(|x| x["process_name"] == json!([awg::EXE]));
        assert_eq!(own, !cfg!(any(target_os = "android", target_os = "ios")));

        let wrong = vec!["b".to_owned(), "a".to_owned()];
        let err = render(&s, &Settings::default(), &params(&wrong, s.root())).err().unwrap();
        assert!(format!("{err:#}").contains("первым звеном"));
    }

    #[test]
    fn missing_route_target_is_rejected() {
        let s = tmp_store("rm");
        put_profile(&s, "a", json!({ "type": "vless", "server": "vpn.example.com" }));
        s.write_text(store::ROUTE_MAP, "// === route:gone ===\nexample.com\n").unwrap();
        let chain = vec!["a".to_owned()];
        let r = render(&s, &Settings::default(), &params(&chain, s.root())).unwrap();
        let rules = r.config["route"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| r["action"] == "reject" && r["domain_suffix"] == json!(["example.com"])));
    }
}
