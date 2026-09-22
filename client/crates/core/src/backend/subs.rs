use std::collections::HashSet;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::{parse_body, Backend, Start};
use crate::chains::ChainStore;
use crate::ipc::{Request, Response};
use crate::lists;
use crate::render::{FETCH_PORT, FETCH_USER};
use crate::settings::Settings;
use crate::store;
use crate::subscription as sub;

impl Backend {
    /// Вход в VPN для самой службы — только пока sing-box работает.
    pub(super) async fn fetch_proxy(&self) -> Option<String> {
        self.engine.pid().await?;
        Some(format!("socks5h://{FETCH_USER}:{}@127.0.0.1:{FETCH_PORT}", self.clash_secret))
    }

    /// Профили, которые обновление подписки не удаляет, даже если поставщик
    /// их убрал: активная цепочка и сохранённые цепочки (как на роутере).
    fn protected_ids(&self) -> Vec<String> {
        let mut ids = Settings::load(&self.store).active_chain();
        for c in ChainStore::load(&self.store).chains {
            ids.extend(c.hops);
        }
        ids
    }

    /// Профили, от которых зависит живой конфиг.
    fn referenced_ids(&self) -> HashSet<String> {
        let mut ids: HashSet<String> = self.protected_ids().into_iter().collect();
        for s in lists::parse_route_map(&self.store.read_text(store::ROUTE_MAP)) {
            ids.insert(s.target);
        }
        ids
    }

    pub(super) fn subscriptions_list(&self) -> Response {
        Response::json(&json!({ "ok": true, "subscriptions": sub::list(&self.store) }))
    }

    pub(super) fn subscription_save(&self, body: String) -> Result<()> {
        sub::save(&self.store, parse_body(&body)?)
    }

    pub(super) fn subscription_delete(&self, req: &Request) -> Result<()> {
        sub::delete(&self.store, req.param("id").unwrap_or(""))
    }

    /// Пробная загрузка для формы: тело и заголовки с лимитами поставщика.
    pub(super) async fn subscription_fetch(&self, body: String) -> Result<Response> {
        let b = parse_body(&body)?;
        let url = b.get("url").and_then(Value::as_str).unwrap_or("");
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(anyhow!("url must start with http:// or https://"));
        }
        let ua = b
            .get("user_agent")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(sub::DEFAULT_UA);
        let mut errors = Vec::new();
        let proxy = self.fetch_proxy().await;
        for (via, p) in [("vpn", proxy.as_deref()), ("direct", None)] {
            if via == "vpn" && p.is_none() {
                continue;
            }
            match sub::fetch(url, ua, p, Duration::from_secs(20)).await {
                Ok(f) => return Ok(Response::json(&json!({ "ok": true, "body": f.body, "headers": f.headers }))),
                Err(e) => errors.push(format!("{via}: {e}")),
            }
        }
        Err(anyhow!(errors.join("; ")))
    }

    pub(super) async fn subscription_refresh(&self, ids: Vec<String>) -> Response {
        let _guard = self.subs_lock.lock().await;
        let proxy = self.fetch_proxy().await;
        let protected = self.protected_ids();
        let mut log = Vec::new();
        let mut touched = Vec::new();
        let mut last_error = None;

        if ids.is_empty() {
            log.push("подписок нет".to_owned());
        }
        for id in &ids {
            let Some(s) = sub::load(&self.store, id) else {
                last_error = Some(format!("{id}: not found"));
                continue;
            };
            let url = s.get("url").and_then(Value::as_str).unwrap_or("");
            let group = s.get("group").and_then(Value::as_str).unwrap_or("");
            log.push(format!("refreshing {id} (group={group}, url={})", sub::mask_url(url)));
            let mut bytes = 0;
            let res = match sub::fetch_and_parse(&s, proxy.as_deref(), &mut log).await {
                Ok((parsed, via, n)) => {
                    bytes = n;
                    sub::apply_parsed(&self.store, &s, &parsed, &protected)
                        .map(|mut o| {
                            o.via = via;
                            o
                        })
                        .map_err(|e| format!("{e:#}"))
                }
                Err(e) => Err(e),
            };
            if let Err(e) = sub::record(&self.store, id, &res, bytes) {
                tracing::warn!(error = %e, "не удалось записать состояние подписки");
            }
            match res {
                Ok(o) => {
                    log.push(format!(
                        "{id}: сохранено {}, удалено {}, оставлено устаревших {}, пропущено {}",
                        o.saved, o.removed, o.kept_stale, o.skipped
                    ));
                    touched.extend(o.touched);
                }
                Err(e) => {
                    log.push(format!("{id}: ошибка — {e}"));
                    last_error = Some(e);
                }
            }
        }

        // У роутера работающий конфиг держит старый outbound до следующей
        // активации. Здесь пересобираем сразу, если изменилось то, на чём он стоит.
        let referenced = self.referenced_ids();
        if touched.iter().any(|id| referenced.contains(id)) && self.engine.pid().await.is_some() {
            match self.apply(None, Start::IfRunning).await {
                Ok(()) => log.push("конфиг пересобран".to_owned()),
                Err(e) => {
                    log.push(format!("конфиг не пересобрался: {e:#}"));
                    last_error.get_or_insert_with(|| format!("{e:#}"));
                }
            }
        }

        let output = log.join("\n");
        match last_error {
            None => Response::json(&json!({ "ok": true, "output": output })),
            Some(e) => Response::json(&json!({ "ok": false, "error": e, "output": output })),
        }
    }

    pub(super) async fn subscription_refresh_one(&self, req: &Request) -> Response {
        let id = req.param("id").unwrap_or("").to_owned();
        self.subscription_refresh(vec![id]).await
    }

    pub(super) async fn subscription_refresh_all(&self) -> Response {
        let ids = sub::list(&self.store)
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect();
        self.subscription_refresh(ids).await
    }

    /// Плановый проход: подписки с `autoupdate`, у которых подошёл срок.
    pub async fn refresh_due_subscriptions(&self) {
        let now = store::now_epoch();
        let due: Vec<String> = sub::list(&self.store)
            .into_iter()
            .filter_map(|v| match v {
                Value::Object(m) if sub::due(&m, now) => m.get("id").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            })
            .collect();
        if due.is_empty() {
            return;
        }
        let r = self.subscription_refresh(due).await;
        tracing::info!(result = %r.body, "плановое обновление подписок");
    }
}
