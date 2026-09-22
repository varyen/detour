use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use super::{parse_body, Backend, Start};
use crate::ipc::{Request, Response};
use crate::lists;
use crate::settings::{RoutingMode, Settings, UdpMode};
use crate::store;

impl Backend {
    /// Нет активного профиля — пересобирать нечего, сохранённое применится
    /// при выборе VPN.
    async fn reapply(&self) -> Result<()> {
        if Settings::load(&self.store).active_chain().is_empty() {
            return Ok(());
        }
        self.apply(None, Start::IfRunning).await
    }

    pub(super) async fn settings(&self, req: &Request, body: String) -> Result<Response> {
        let mut s = Settings::load(&self.store);
        if req.body.is_none() {
            let mut v = s.raw().clone();
            let chain = s.active_chain();
            v.insert("active_chain".into(), json!(chain.join(",")));
            v.insert("active_profile".into(), json!(chain.last().cloned().unwrap_or_default()));
            v.insert("routing_mode".into(), json!(s.routing_mode().as_str()));
            v.insert("singbox_mode".into(), json!("single"));
            return Ok(Response::json(&Value::Object(v)));
        }
        let b = parse_body(&body)?;
        if let Some(m) = b.get("routing_mode").and_then(Value::as_str) {
            let m = RoutingMode::parse(m).ok_or_else(|| anyhow!("invalid routing_mode"))?;
            s.set("routing_mode", m.as_str());
        }
        if let Some(m) = b.get("singbox_mode").and_then(Value::as_str) {
            if m != "single" {
                bail!("режим «процесс на цель» недоступен: в TUN один процесс обслуживает всё");
            }
        }
        s.save(&self.store)?;
        self.reapply().await.map_err(|e| anyhow!("config build failed: {e:#}"))?;
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// Текстовый список: GET → `{key: текст}`, POST → запись, при `apply` —
    /// ещё и пересборка (у роутера `*_save_restart`, `route_map`).
    pub(super) async fn text_file(
        &self,
        req: &Request,
        file: &str,
        key: &str,
        body: String,
        apply: bool,
    ) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ key: self.store.read_text(file) })));
        }
        let mut text = body;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.store.write_text(file, &text)?;
        if apply {
            self.reapply().await.map_err(|e| anyhow!("config build failed: {e:#}"))?;
        }
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) async fn egress_blocklist(&self, req: &Request, body: String) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ "list": self.store.read_text(store::EGRESS_BLOCK) })));
        }
        let m = lists::parse_list(&body);
        let text: String = m.cidrs.iter().map(|c| format!("{}\n", c.trim_end_matches("/32"))).collect();
        self.store.write_text(store::EGRESS_BLOCK, &text)?;
        self.reapply().await?;
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) async fn udp_vpn(&self, req: &Request, body: String) -> Result<Response> {
        let mut s = Settings::load(&self.store);
        if req.body.is_none() {
            return Ok(Response::json(&json!({
                "mode": s.udp_mode().as_str(),
                "supported": true,
                "list": self.store.read_text(store::UDP_VPN),
            })));
        }
        let b = parse_body(&body)?;
        let mode = b
            .get("mode")
            .and_then(Value::as_str)
            .and_then(UdpMode::parse)
            .ok_or_else(|| anyhow!("invalid mode"))?;
        s.set("udp_vpn_mode", mode.as_str());
        s.save(&self.store)?;
        self.reapply().await?;
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) async fn udp_vpn_list(&self, req: &Request, body: String) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ "list": self.store.read_text(store::UDP_VPN) })));
        }
        self.store.write_text(store::UDP_VPN, &body)?;
        if Settings::load(&self.store).udp_mode() != UdpMode::Off {
            self.reapply().await?;
        }
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) async fn allvpn(&self, on: bool) -> Result<()> {
        if on && self.engine.pid().await.is_none() {
            bail!("sing-box is not running");
        }
        let mut s = Settings::load(&self.store);
        s.set("allvpn", if on { "1" } else { "0" });
        s.save(&self.store)?;
        self.reapply().await
    }
}
