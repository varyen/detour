/// Тот же санитайзинг, что `sanitize_name` в detour-api: id профилей и целей
/// общие для роутера и клиента (экспорт/импорт), поэтому правило одно.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect()
}

/// Список хопов: санитайзинг, без пустых, дубли — по первому вхождению.
/// Первый элемент набирается напрямую, последний — выход к сайту.
pub fn normalize_hops<I, S>(items: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for it in items {
        let id = sanitize(it.as_ref().trim());
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

pub fn parse_csv(s: &str) -> Vec<String> {
    normalize_hops(s.split(','))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hops_are_sanitized_and_deduplicated() {
        assert_eq!(parse_csv(" a,b ,,a, c.d "), vec!["a", "b", "cd"]);
        assert_eq!(sanitize("Нидерланды-1 😀"), "-1");
    }
}
