use anyhow::{anyhow, bail, Result};
use serde_json::json;

use super::Backend;
use crate::ipc::{Request, Response};
use crate::store;

const TAIL_LINES: usize = 300;

impl Backend {
    /// Конфиг в клиенте собирается из профилей и списков при каждом
    /// применении — ручная правка была бы потеряна при следующей пересборке,
    /// поэтому только чтение.
    pub(super) fn singbox_config(&self, req: &Request) -> Result<Response> {
        if req.body.is_some() {
            bail!("конфиг собирается автоматически из профилей и списков — правьте их");
        }
        let body = std::fs::read_to_string(self.store.path(store::CONFIG)).unwrap_or_else(|_| "{}".into());
        Ok(Response { status: 200, body })
    }

    /// Имена — роутерные вкладки панели. У клиента нет своих журналов zapret,
    /// проверки и обновлений панели: для них файла нет, и панель покажет
    /// «пусто», а не ошибку.
    fn log_path(&self, name: &str) -> Result<&'static str> {
        match name {
            "singbox" => Ok(store::SINGBOX_LOG),
            "singbox_stderr" => Ok(store::SINGBOX_STDERR),
            "apply" | "update" => Ok("run/apply.log"),
            "zapret" => Ok("logs/dpi.log"),
            "health" => Ok("logs/health.log"),
            _ => bail!("unknown log"),
        }
    }

    pub(super) fn logs_view(&self, req: &Request) -> Result<Response> {
        let name = req.param("name").ok_or_else(|| anyhow!("unknown log"))?;
        let rel = self.log_path(name)?;
        let path = self.store.path(rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Response::json(&json!({ "ok": true, "name": name, "path": path, "log": "", "missing": true })));
        };
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
        Ok(Response::json(&json!({ "ok": true, "name": name, "path": path, "log": tail })))
    }

    pub(super) fn logs_clear(&self, req: &Request) -> Result<()> {
        let rel = self.log_path(req.param("name").unwrap_or(""))?;
        let path = self.store.path(rel);
        if path.exists() {
            std::fs::write(path, b"")?;
        }
        Ok(())
    }
}
