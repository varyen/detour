//! Резервная копия настроек. Конверт совместим с роутерным
//! (`panel_export_config`): роутерные ключи те же, клиентские дописаны рядом —
//! роутер при импорте незнакомое пропускает, клиент понимает оба варианта.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use super::{parse_body, Backend, Start};
use crate::chains::ChainStore;
use crate::ipc::Response;
use crate::profiles;
use crate::settings::Settings;
use crate::store;
use crate::subscription as sub;

const FORBIDDEN: [&str; 9] = [
    "auth", "password", "passwd", "panel_user", "panel_password", "update_conf", "gh_token", "gh_owner", "gh_repo",
];

/// Роутерные текстовые разделы: пишутся, только если непустые (так делает
/// роутерный импорт — пустая строка в старом бэкапе не стирает список).
const ROUTER_TEXTS: [(&str, &str); 3] = [
    ("proxy_domains", store::PROXY_DOMAINS),
    ("whitelist_domains", store::WHITELIST),
    ("zapret_domains", store::DPI_DOMAINS),
];

/// Клиентские разделы: присутствие ключа — это значение, в том числе пустое.
const CLIENT_TEXTS: [(&str, &str); 7] = [
    ("route_map", store::ROUTE_MAP),
    ("udp_vpn_list", store::UDP_VPN),
    ("egress_blocklist", store::EGRESS_BLOCK),
    ("ru_subnets_exclude", store::RU_EXCLUDE),
    ("autoswitch_exclude", store::AUTOSWITCH_EXCLUDE),
    ("speedcheck_exclude", store::SPEEDCHECK_EXCLUDE),
    ("torrent_allow", store::TORRENT_ALLOW),
];

impl Backend {
    pub(super) fn export_config(&self) -> Response {
        let s = &self.store;
        let mut doc = json!({
            "version": 1,
            "exported_at": humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
            "router_version": crate::VERSION,
            "platform": super::PLATFORM,
            "settings": Value::Object(Settings::load(s).raw().clone()),
            "zapret_conf": "",
            "subscriptions": sub::list(s),
            "chains": serde_json::to_value(ChainStore::load(s)).unwrap_or_default(),
        });
        for (key, rel) in ROUTER_TEXTS.iter().chain(CLIENT_TEXTS.iter()) {
            doc[*key] = json!(s.read_text(rel));
        }
        Response::json(&doc)
    }

    pub(super) async fn import_config(&self, body: String) -> Result<Response> {
        let Value::Object(doc) = parse_body(&body).map_err(|_| anyhow!("config is not a valid JSON object"))? else {
            bail!("config is not a valid JSON object");
        };
        if let Some(k) = FORBIDDEN.iter().find(|k| doc.contains_key(**k)) {
            bail!("forbidden key in config: {k} — учётные данные панели из файла не переносятся");
        }
        let s = &self.store;

        if let Some(Value::Object(imported)) = doc.get("settings") {
            let current = Settings::load(s);
            let mut next: Map<String, Value> = imported.clone();
            // Цепочка из бэкапа ссылается на профили, которых здесь может не
            // быть (профили переносятся отдельным файлом) — тогда оставляем свою.
            let chain = Settings::load_from(imported.clone()).active_chain();
            if chain.is_empty() || chain.iter().any(|h| !profiles::exists(s, h)) {
                for k in ["active_chain", "active_profile"] {
                    match current.raw().get(k) {
                        Some(v) => next.insert(k.into(), v.clone()),
                        None => next.remove(k),
                    };
                }
            }
            Settings::load_from(next).save(s)?;
        }

        for (key, rel) in ROUTER_TEXTS {
            if let Some(t) = doc.get(key).and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                s.write_text(rel, t)?;
            }
        }
        for (key, rel) in CLIENT_TEXTS {
            if let Some(t) = doc.get(key).and_then(Value::as_str) {
                s.write_text(rel, t)?;
            }
        }

        if let Some(chains) = doc.get("chains").filter(|c| c.is_object()) {
            let parsed: ChainStore = serde_json::from_value(chains.clone()).map_err(|_| anyhow!("chains: неверный формат"))?;
            parsed.save(s)?;
        }

        let mut subs: Vec<Value> = doc.get("subscriptions").and_then(Value::as_array).cloned().unwrap_or_default();
        // Старый роутерный бэкап: одна подписка v1 под ключом `subscription`.
        if subs.is_empty() {
            if let Some(Value::Object(legacy)) = doc.get("subscription") {
                if legacy.get("url").and_then(Value::as_str).is_some() {
                    let mut l = legacy.clone();
                    l.entry("id").or_insert(json!("legacy"));
                    subs.push(Value::Object(l));
                }
            }
        }
        let mut skipped = Vec::new();
        for v in subs {
            let id = v.get("id").and_then(Value::as_str).unwrap_or("?").to_owned();
            if let Err(e) = sub::save(s, v) {
                skipped.push(format!("{id}: {e}"));
            }
        }

        let mut resp = json!({ "ok": true });
        if !Settings::load(s).active_chain().is_empty() {
            if let Err(e) = self.apply(None, Start::IfRunning).await {
                resp["warning"] = json!(format!("настройки восстановлены, но конфиг не собрался: {e:#}"));
            }
        }
        if !skipped.is_empty() {
            resp["skipped_subscriptions"] = json!(skipped);
        }
        Ok(Response::json(&resp))
    }
}
