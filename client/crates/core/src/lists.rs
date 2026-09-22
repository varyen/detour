//! Текстовые списки в формате роутера (`proxy-domains.list` и родня, карта
//! маршрутов, `udp-vpn.list`, id-списки).

use std::net::IpAddr;

use crate::ids;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Matchers {
    /// Домен покрывает и все поддомены — как `ipset=/d/` у dnsmasq.
    pub domains: Vec<String>,
    pub cidrs: Vec<String>,
}

impl Matchers {
    pub fn is_empty(&self) -> bool {
        self.domains.is_empty() && self.cidrs.is_empty()
    }
}

fn strip_comment(line: &str) -> &str {
    let cut = [line.find("//"), line.find('#')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(line.len());
    line[..cut].trim()
}

/// `^[a-zA-Z0-9]([a-zA-Z0-9._-]*\.)+[a-zA-Z]{2,}$` — регэксп роутера.
pub fn is_domain(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return false;
    }
    let Some(dot) = s.rfind('.') else { return false };
    let tld = &s[dot + 1..];
    dot > 0 && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Адрес или подсеть в каноническом виде `a.b.c.d/nn`. Голый адрес
/// дополняется до /32 (/128): `ip_cidr` в sing-box ждёт префикс.
pub fn parse_cidr(s: &str) -> Option<String> {
    let (addr, len) = match s.split_once('/') {
        Some((a, l)) => (a, Some(l.parse::<u8>().ok()?)),
        None => (s, None),
    };
    let ip: IpAddr = addr.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = len.unwrap_or(max);
    (len <= max).then(|| format!("{ip}/{len}"))
}

pub fn parse_list(text: &str) -> Matchers {
    let mut m = Matchers::default();
    for line in text.lines() {
        let item = strip_comment(line);
        let item = item.strip_prefix("*.").unwrap_or(item);
        if item.is_empty() {
            continue;
        }
        if let Some(c) = parse_cidr(item) {
            if !m.cidrs.contains(&c) {
                m.cidrs.push(c);
            }
        } else if is_domain(item) {
            let d = item.to_ascii_lowercase();
            if !m.domains.contains(&d) {
                m.domains.push(d);
            }
        }
    }
    m
}

pub fn parse_id_list(text: &str) -> Vec<String> {
    ids::normalize_hops(text.split(['\n', ',']))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteSection {
    pub target: String,
    /// Точные имена (`example.com`) и суффиксы (`*.example.com`) раздельно:
    /// для HTTP/SOCKS-выхода точные имена идут правилом с `override_address`.
    pub exact: Vec<String>,
    pub suffix: Vec<String>,
    pub cidrs: Vec<String>,
    pub strict: bool,
    pub via_chain: bool,
}

impl RouteSection {
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.suffix.is_empty() && self.cidrs.is_empty()
    }
}

fn meta_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Карта маршрутов: секции `// === route:<id> ===`, опции `// meta: k=v`,
/// строки вне секций теряются (так же делает редактор панели).
pub fn parse_route_map(text: &str) -> Vec<RouteSection> {
    let mut out: Vec<RouteSection> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("//") {
            let rest = rest.trim();
            if let Some(head) = rest.strip_prefix("===") {
                let head = head.trim().trim_end_matches('=').trim();
                if let Some(id) = head.strip_prefix("route:") {
                    out.push(RouteSection {
                        target: ids::sanitize(id.trim()),
                        exact: Vec::new(),
                        suffix: Vec::new(),
                        cidrs: Vec::new(),
                        strict: true,
                        via_chain: false,
                    });
                }
                continue;
            }
            if let (Some(meta), Some(sec)) = (rest.strip_prefix("meta:"), out.last_mut()) {
                for kv in meta.split_whitespace() {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    match (k, meta_bool(v)) {
                        ("strict", Some(b)) => sec.strict = b,
                        ("via_chain", Some(b)) => sec.via_chain = b,
                        _ => {}
                    }
                }
            }
            continue;
        }
        let Some(sec) = out.last_mut() else { continue };
        let item = strip_comment(t);
        if item.is_empty() {
            continue;
        }
        if let Some(c) = parse_cidr(item) {
            sec.cidrs.push(c);
        } else if let Some(d) = item.strip_prefix("*.").filter(|d| is_domain(d)) {
            sec.suffix.push(d.to_ascii_lowercase());
        } else if is_domain(item) {
            sec.exact.push(item.to_ascii_lowercase());
        }
    }
    out.retain(|s| !s.target.is_empty());
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum UdpItem {
    Cidr(String),
    Domain(String),
    Ports(u16, u16),
    CidrPorts(String, u16, u16),
}

fn parse_ports(s: &str) -> Option<(u16, u16)> {
    let s = s.trim();
    let (a, b) = match s.split_once(['-', ':']) {
        Some((a, b)) => (a, b),
        None => (s, s),
    };
    let (a, b): (u16, u16) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a > 0 && a <= b).then_some((a, b))
}

/// `udp-vpn.list`: адрес/подсеть/домен, `:27015`, `p1-p2`, `p1:p2`,
/// `1.2.3.4:8211`, `10.0.0.0/24:27015`.
pub fn parse_udp_list(text: &str) -> Vec<UdpItem> {
    let mut out = Vec::new();
    for line in text.lines() {
        let item = strip_comment(line);
        if item.is_empty() {
            continue;
        }
        if let Some(p) = item.strip_prefix(':').and_then(parse_ports) {
            out.push(UdpItem::Ports(p.0, p.1));
        } else if let Some(c) = parse_cidr(item) {
            out.push(UdpItem::Cidr(c));
        } else if let Some(p) = parse_ports(item) {
            out.push(UdpItem::Ports(p.0, p.1));
        } else if let Some((addr, ports)) = item.rsplit_once(':') {
            match (parse_cidr(addr), parse_ports(ports)) {
                (Some(c), Some(p)) => out.push(UdpItem::CidrPorts(c, p.0, p.1)),
                _ => {}
            }
        } else if is_domain(item) {
            out.push(UdpItem::Domain(item.to_ascii_lowercase()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parsing_matches_router_rules() {
        let m = parse_list(
            "// заголовок\nExample.COM\n*.example.org // коммент\n203.0.113.7\n198.51.100.0/24\nСекция\nbad_\n",
        );
        assert_eq!(m.domains, vec!["example.com", "example.org"]);
        assert_eq!(m.cidrs, vec!["203.0.113.7/32", "198.51.100.0/24"]);
    }

    #[test]
    fn route_map_sections_and_meta() {
        let rm = parse_route_map(
            "stray.example.com\n// === route:nl_1 ===\n// meta: strict=0 via_chain=yes\nexample.com\n*.example.org\n10.0.0.0/8\n\n// === route:empty ===\n",
        );
        assert_eq!(rm.len(), 2);
        assert_eq!(rm[0].target, "nl_1");
        assert!(!rm[0].strict && rm[0].via_chain);
        assert_eq!(rm[0].exact, vec!["example.com"]);
        assert_eq!(rm[0].suffix, vec!["example.org"]);
        assert_eq!(rm[0].cidrs, vec!["10.0.0.0/8"]);
        assert!(rm[1].is_empty() && rm[1].strict);
    }

    #[test]
    fn udp_list_forms() {
        let v = parse_udp_list("19294:19344 // Discord\n:27015\n1.2.3.4:8211\n10.0.0.0/24:27015\n5.6.7.8\nvoice.example.com\n");
        assert_eq!(
            v,
            vec![
                UdpItem::Ports(19294, 19344),
                UdpItem::Ports(27015, 27015),
                UdpItem::CidrPorts("1.2.3.4/32".into(), 8211, 8211),
                UdpItem::CidrPorts("10.0.0.0/24".into(), 27015, 27015),
                UdpItem::Cidr("5.6.7.8/32".into()),
                UdpItem::Domain("voice.example.com".into()),
            ]
        );
    }
}
