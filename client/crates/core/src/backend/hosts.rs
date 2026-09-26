//! Приоритетный hosts и шифрование DNS (`hosts_*`, `secure_dns_set`).
//! Ответы — объект статуса `detour-hosts`, как у роутера; ошибка загрузки
//! источника — не ошибка запроса, а поле `error`.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::{parse_body, Backend, Start};
use crate::hosts;
use crate::ipc::{Request, Response};
use crate::settings::Settings;

fn enabled_flag(body: &str) -> Result<bool> {
    match parse_body(body)?.get("enabled").and_then(Value::as_bool) {
        Some(b) => Ok(b),
        None => bail!("enabled bool required"),
    }
}

impl Backend {
    /// Пересборка после правки. Сохранённое уже на диске: если конфиг не
    /// собрался, это говорится прямо, а не прячется в лог.
    async fn hosts_reconfig(&self) -> Option<Response> {
        if Settings::load(&self.store).active_chain().is_empty() {
            return None;
        }
        let e = self.apply(None, Start::IfRunning).await.err()?;
        Some(Response::error(&format!("сохранено, но конфиг не собрался: {e:#}")))
    }

    async fn hosts_apply(&self) -> Response {
        match self.hosts_reconfig().await {
            Some(err) => err,
            None => Response::json(&hosts::status(&self.store)),
        }
    }

    pub(super) async fn hosts_action(&self, req: &Request, body: String) -> Result<Response> {
        let store = &self.store;
        Ok(match req.action.as_str() {
            "hosts_status" => Response::json(&hosts::status(store)),
            "hosts_get" => Response::json(&json!({ "hosts": store.read_text(hosts::LIST) })),
            "hosts_custom_get" => Response::json(&json!({ "custom": store.read_text(hosts::CUSTOM) })),
            "hosts_set" => {
                let b = parse_body(&body)?;
                let mut c = hosts::load(store);
                if let Some(url) = b.get("url").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()) {
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        bail!("invalid url");
                    }
                    c.url = url.to_owned();
                }
                let turn_on = b.get("enabled").and_then(Value::as_bool);
                if let Some(on) = turn_on {
                    c.enabled = on;
                }
                hosts::save(store, &c)?;
                if turn_on == Some(true) && c.count == 0 {
                    if store.exists(hosts::LIST) || store.exists(hosts::CUSTOM) {
                        hosts::rebuild(store, &mut c)?;
                        hosts::save(store, &c)?;
                    }
                    if c.count == 0 {
                        let proxy = self.fetch_proxy().await;
                        let _ = hosts::refresh(store, proxy.as_deref()).await;
                    }
                }
                if turn_on.is_some() {
                    self.hosts_apply().await
                } else {
                    Response::json(&hosts::status(store))
                }
            }
            "hosts_refresh" => {
                let proxy = self.fetch_proxy().await;
                match hosts::refresh(store, proxy.as_deref()).await {
                    Ok(()) if hosts::load(store).enabled => self.hosts_apply().await,
                    _ => Response::json(&hosts::status(store)),
                }
            }
            "hosts_upload" => {
                let mut c = hosts::load(store);
                if body.trim().is_empty() {
                    bail!("no file uploaded");
                }
                hosts::set_raw(store, &mut c, &body, true)?;
                hosts::save(store, &c)?;
                self.hosts_apply().await
            }
            "hosts_exclude" | "hosts_custom_toggle" => {
                let on = enabled_flag(&body)?;
                let mut c = hosts::load(store);
                if req.action == "hosts_exclude" {
                    c.exclude_proxied = on;
                } else {
                    c.custom_enabled = on;
                }
                hosts::rebuild(store, &mut c)?;
                hosts::save(store, &c)?;
                self.hosts_apply().await
            }
            "hosts_custom_save" => {
                let mut text = body;
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                store.write_text(hosts::CUSTOM, &text)?;
                let mut c = hosts::load(store);
                hosts::rebuild(store, &mut c)?;
                hosts::save(store, &c)?;
                self.hosts_apply().await
            }
            "secure_dns_set" => {
                let b = parse_body(&body)?;
                let mode = b.get("mode").and_then(Value::as_str).unwrap_or("");
                let list = b.get("list").and_then(Value::as_str).unwrap_or("");
                hosts::set_secure(store, mode, list)?;
                match self.hosts_reconfig().await {
                    Some(err) => err,
                    None => Response::json(&json!({ "ok": true })),
                }
            }
            _ => Response::error("not_supported"),
        })
    }

    /// Плановое обновление источника (роутер — cron раз в 12 часов). Файл,
    /// загруженный руками, ссылкой не затирается.
    pub(super) async fn hosts_maintenance(&self) {
        let c = hosts::load(&self.store);
        if !c.enabled || c.from_file || crate::store::now_epoch() - c.updated < hosts::STALE_AFTER {
            return;
        }
        let proxy = self.fetch_proxy().await;
        if hosts::refresh(&self.store, proxy.as_deref()).await.is_ok() {
            self.reapply_quiet().await;
        }
    }
}
