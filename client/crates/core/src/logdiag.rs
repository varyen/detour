//! Сводка над журналом sing-box: из сотен однотипных строк — одна-две
//! человеческие причины. Когда VPN-сервер недоступен, журнал заваливают
//! следствия (DNS идёт через туннель, и каждый запрос падает по таймауту), а
//! первопричина — «dial tcp <сервер>: i/o timeout» — теряется среди них.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// `bad` — VPN не работает, `warn` — следствие или частичная беда.
    pub level: &'static str,
    pub title: String,
    pub text: String,
}

#[derive(Default)]
struct Hit {
    count: usize,
    last: String,
}

/// `servers` — адреса VPN-серверов профилей (`host:port` → имя профиля):
/// только по ним таймаут — повод для тревоги, таймаут к обычному сайту в
/// режиме «напрямую» — нет.
pub fn diagnose(log: &str, servers: &[(String, String)]) -> Vec<Finding> {
    let mut timeout: Vec<(String, Hit)> = Vec::new();
    let mut refused: Vec<(String, Hit)> = Vec::new();
    let mut reset: Vec<(String, Hit)> = Vec::new();
    let mut dns_cancel = Hit::default();
    // Удалённый DNS ходит через VPN: цель, до которой не дозвонился DNS, —
    // это сервер VPN, даже если адрес не совпал ни с одним профилем.
    let mut via_dns: Vec<String> = Vec::new();
    let mut bump = |list: &mut Vec<(String, Hit)>, key: &str, ts: &str| match list.iter_mut().find(|(k, _)| k == key) {
        Some((_, h)) => {
            h.count += 1;
            h.last = ts.to_owned();
        }
        None => list.push((key.to_owned(), Hit { count: 1, last: ts.to_owned() })),
    };
    for line in log.lines() {
        let ts = time_of(line);
        if line.contains("dns: exchange failed") && line.contains("context canceled") {
            dns_cancel.count += 1;
            dns_cancel.last = ts.to_owned();
        }
        let Some(target) = dial_target(line) else { continue };
        if line.contains("dns: exchange failed") && !via_dns.contains(&target) {
            via_dns.push(target.clone());
        }
        if line.contains("i/o timeout") {
            bump(&mut timeout, &target, ts);
        } else if line.contains("connection refused") {
            bump(&mut refused, &target, ts);
        } else if line.contains("connection reset") {
            bump(&mut reset, &target, ts);
        }
    }

    let name = |target: &str| -> Option<String> {
        servers
            .iter()
            .find(|(addr, _)| addr.eq_ignore_ascii_case(target))
            .map(|(_, n)| format!("профиля «{n}»"))
            .or_else(|| via_dns.iter().any(|t| t == target).then(|| "VPN".to_owned()))
    };
    let mut out = Vec::new();
    let mut server_down = false;
    for (list, kind) in [(&timeout, 0), (&refused, 1), (&reset, 2)] {
        for (target, hit) in list.iter() {
            let Some(profile) = name(target) else { continue };
            server_down = true;
            let (title, text) = match kind {
                0 => (
                    format!("Сервер {profile} не отвечает"),
                    format!(
                        "Соединения с {target} не устанавливаются (неудачных попыток: {}, последняя — {}). Сервер выключен, \
                         либо провайдер блокирует его адрес или порт — так бывает в мобильных сетях. \
                         Проверьте профиль из другой сети (Wi-Fi) или переключитесь на другой профиль.",
                        hit.count, hit.last
                    ),
                ),
                1 => (
                    format!("Сервер {profile} отказывает в подключении"),
                    format!(
                        "{target} отвечает «порт закрыт» (попыток: {}, последняя — {}): на сервере не запущен VPN \
                         или в профиле неверный порт.",
                        hit.count, hit.last
                    ),
                ),
                _ => (
                    format!("Соединение с сервером {profile} обрывается"),
                    format!(
                        "Связь с {target} сбрасывается посреди подключения (обрывов: {}, последний — {}). \
                         Похоже на блокировку провайдером (DPI): попробуйте другой протокол или порт.",
                        hit.count, hit.last
                    ),
                ),
            };
            out.push((hit.count, Finding { level: "bad", title, text }));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<Finding> = out.into_iter().map(|(_, f)| f).collect();
    if dns_cancel.count >= 5 {
        let text = if server_down {
            format!(
                "DNS-запросов без ответа: {} (последний — {}). Это следствие: адреса сайтов \
                 узнаются через VPN, а сервер недоступен.",
                dns_cancel.count, dns_cancel.last
            )
        } else {
            format!(
                "DNS-запросов без ответа: {} (последний — {}): DNS через VPN отвечает слишком \
                 медленно или не отвечает вовсе.",
                dns_cancel.count, dns_cancel.last
            )
        };
        out.push(Finding { level: "warn", title: "Сайты не открываются: DNS не отвечает".into(), text });
    }
    out
}

/// `+0300 2026-10-25 19:17:27 ERROR …` → `19:17:27`.
fn time_of(line: &str) -> &str {
    line.split_whitespace().nth(2).filter(|t| t.len() == 8 && t.as_bytes()[2] == b':').unwrap_or("?")
}

/// Цель неудачного `dial tcp|udp <host:port>:` — последнего в строке: перед
/// ним бывает обёртка вида `dial rmnet_data4 (26):`.
fn dial_target(line: &str) -> Option<String> {
    let i = line.rfind("dial tcp ").map(|i| i + 9).or_else(|| line.rfind("dial udp ").map(|i| i + 9))?;
    let rest = &line[i..];
    let end = rest.find(": ")?;
    let t = rest[..end].trim();
    (!t.is_empty() && t.contains(':')).then(|| t.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
+0300 2026-10-25 19:17:27 ERROR [2540518094 6.0s] dns: exchange failed for sync.browser.yandex.net. IN A: context canceled
+0300 2026-10-25 19:17:27 ERROR [3081623324 22.58s] dns: exchange failed for alice.yandex.ru. IN HTTPS: dial rmnet_data4 (26): dial tcp 203.0.113.40:4430: i/o timeout
+0300 2026-10-25 19:17:27 ERROR [1 6.46s] dns: exchange failed for alice.yandex.ru. IN A: context canceled
+0300 2026-10-25 19:17:28 ERROR [2 6.46s] dns: exchange failed for suggest.dzen.ru. IN A: context canceled
+0300 2026-10-25 19:17:28 ERROR [3 6.46s] dns: exchange failed for yandex.ru. IN A: context canceled
+0300 2026-10-25 19:17:29 ERROR [921961619 22.87s] dns: exchange failed for yandex.ru. IN A: dial rmnet_data4 (26): dial tcp 203.0.113.40:4430: i/o timeout
+0300 2026-10-25 19:17:30 ERROR [4 6.46s] dns: exchange failed for dzen.ru. IN A: context canceled
+0300 2026-10-25 19:17:31 ERROR [5] outbound/direct[direct]: dial tcp 198.51.100.9:443: i/o timeout
";

    #[test]
    fn server_timeout_and_dns_consequence() {
        let servers = vec![("203.0.113.40:4430".to_owned(), "Example VPN TLS".to_owned())];
        let f = diagnose(LOG, &servers);
        assert_eq!(f.len(), 2, "{f:?}");
        assert_eq!(f[0].level, "bad");
        assert!(f[0].title.contains("Example VPN TLS"));
        assert!(f[0].text.contains("203.0.113.40:4430") && f[0].text.contains("попыток: 2") && f[0].text.contains("19:17:29"));
        assert_eq!(f[1].level, "warn");
        assert!(f[1].text.contains("без ответа: 5") && f[1].text.contains("следствие"));
    }

    #[test]
    fn unknown_targets_are_not_alarms() {
        let f = diagnose(LOG, &[]);
        assert_eq!(f[0].title, "Сервер VPN не отвечает", "цель DNS-запросов — сервер VPN");
        assert!(!f.iter().any(|x| x.text.contains("198.51.100.9")), "таймаут к обычному сайту — не тревога");
        assert!(diagnose("", &[]).is_empty());
    }

    #[test]
    fn dial_target_takes_last() {
        assert_eq!(dial_target("x: dial rmnet_data4 (26): dial tcp 1.2.3.4:443: i/o timeout").as_deref(), Some("1.2.3.4:443"));
        assert_eq!(dial_target("dial tcp [2001:db8::1]:443: connection refused").as_deref(), Some("[2001:db8::1]:443"));
        assert_eq!(dial_target("nothing here"), None);
    }
}
