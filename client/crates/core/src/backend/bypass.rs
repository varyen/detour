//! Пульт обхода DPI. Контракт action тот же, что у роутера (`bypass_*` и
//! `zapret_*`), но движок на каждой платформе один: Windows — winws2 (режим
//! `zapret2`), macOS — tpws (режим `zapret`). Чужое имя режима отклоняется,
//! и панель его в клиенте не показывает.

use anyhow::Result;
use serde_json::json;

use super::{Backend, Start};
use crate::ipc::{Request, Response};
use crate::store;

/// Имя единственного движка платформы в терминах панели: на Windows это
/// nfqws2-совместимый winws2, на macOS — tpws.
pub(super) const MODE: &str = if cfg!(windows) { "zapret2" } else { "zapret" };

/// Режим из настроек: `off` или движок платформы.
pub(super) fn mode_of(s: &crate::settings::Settings) -> String {
    match s.get("bypass_mode").as_deref() {
        Some(m) if m == MODE => MODE.to_owned(),
        _ => "off".to_owned(),
    }
}

impl Backend {
    pub(super) async fn bypass_status(&self) -> Response {
        let settings = crate::settings::Settings::load(&self.store);
        let running = if self.dpi.pid().await.is_some() { MODE } else { "none" };
        Response::json(&json!({
            "mode": mode_of(&settings),
            "autostart": i32::from(self.store.flag(store::AUTOSTART_DPI)),
            "running": running,
            "zapret2_supported": self.dpi.supported(),
            "platform": super::PLATFORM,
            "qnum": 0,
            "queued": 0,
            "strategy": self.dpi.strategy(&self.store),
        }))
    }

    /// Включение/выключение движка. Конфиг sing-box пересобирается: при
    /// работающем движке домены DPI уходят мимо VPN (на Windows — напрямую,
    /// на macOS — в локальный tpws), иначе идут обычным маршрутом.
    pub(super) async fn bypass_set(&self, req: &Request) -> Result<Response> {
        let mode = req.param("mode").unwrap_or("").to_owned();
        if mode != "off" && mode != MODE {
            return Ok(Response::error(&format!("на этой платформе движок один: off или {MODE}")));
        }
        self.dpi.stop().await;
        if mode == MODE {
            if let Err(e) = self.dpi.start(&self.store, self.tun_enabled()).await {
                self.set_dpi_on(false);
                self.sync_dpi_routing().await;
                return Ok(Response::error(&format!("{e:#}")));
            }
        }
        self.set_dpi_on(mode == MODE);
        let mut settings = crate::settings::Settings::load(&self.store);
        settings.set("bypass_mode", mode.clone());
        settings.save(&self.store)?;
        self.sync_dpi_routing().await;
        Ok(Response::json(&json!({ "ok": true, "mode": mode })))
    }

    pub(super) async fn bypass_stop(&self) -> Result<Response> {
        self.dpi.stop().await;
        self.set_dpi_on(false);
        self.sync_dpi_routing().await;
        Ok(Response::json(&json!({ "ok": true })))
    }

    pub(super) fn bypass_autostart(&self, req: &Request) -> Result<Response> {
        let on = matches!(req.param("on").unwrap_or(""), "1" | "on" | "true");
        self.store.set_flag(store::AUTOSTART_DPI, on)?;
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// GET — голая строка (панель читает её как текст), POST — новая стратегия.
    pub(super) async fn bypass_strategy(&self, req: &Request, body: Option<String>) -> Result<Response> {
        let Some(text) = body else {
            return Ok(Response::text(&format!("{}\n", self.dpi.strategy(&self.store))));
        };
        let line = text.lines().next().unwrap_or("").trim().to_owned();
        if let Err(e) = self.dpi.check(&self.store, &line).await {
            return Ok(Response::error(&e));
        }
        self.store.write_text(store::DPI_STRATEGY, &format!("{line}\n"))?;
        if self.dpi.pid().await.is_some() {
            self.dpi.stop().await;
            if let Err(e) = self.dpi.start(&self.store, self.tun_enabled()).await {
                self.set_dpi_on(false);
                self.sync_dpi_routing().await;
                return Ok(Response::error(&format!("{e:#}")));
            }
        }
        let _ = req;
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// Старый пульт из журнала: те же операции над единственным движком.
    pub(super) async fn zapret_service(&self, action: &str) -> Result<Response> {
        match action {
            "zapret_stop" => {
                self.dpi.stop().await;
                self.set_dpi_on(false);
                self.sync_dpi_routing().await;
            }
            "zapret_start" | "zapret_restart" => {
                self.dpi.stop().await;
                match self.dpi.start(&self.store, self.tun_enabled()).await {
                    Ok(_) => self.set_dpi_on(true),
                    Err(e) => {
                        self.set_dpi_on(false);
                        self.sync_dpi_routing().await;
                        return Ok(Response::error(&format!("{e:#}")));
                    }
                }
                let mut settings = crate::settings::Settings::load(&self.store);
                settings.set("bypass_mode", MODE);
                settings.save(&self.store)?;
                self.sync_dpi_routing().await;
            }
            _ => {
                self.store.set_flag(store::AUTOSTART_DPI, action == "zapret_enable")?;
            }
        }
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// Список доменов обхода: как `text_file`, но ещё и перечитывается движком.
    pub(super) async fn dpi_domains(&self, req: &Request, body: String, apply: bool) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ "domains": self.store.read_text(store::DPI_DOMAINS) })));
        }
        let mut text = body;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.store.write_text(store::DPI_DOMAINS, &text)?;
        self.dpi_reload().await;
        if apply {
            self.sync_dpi_routing().await;
        }
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// Домены DPI поменялись — движку нужен свежий hostlist.
    pub(super) async fn dpi_reload(&self) {
        if self.dpi.pid().await.is_some() {
            self.dpi.stop().await;
            if self.dpi.start(&self.store, self.tun_enabled()).await.is_err() {
                self.set_dpi_on(false);
            }
        }
    }

    /// Маршрутизация зависит от того, работает ли движок: пересобираем конфиг,
    /// но VPN, выключенный человеком, не поднимаем.
    async fn sync_dpi_routing(&self) {
        if !crate::settings::Settings::load(&self.store).active_chain().is_empty() {
            let _ = self.apply(None, Start::IfRunning).await;
        }
    }

    pub(super) async fn dpi_boot(&self) {
        if !self.store.flag(store::AUTOSTART_DPI) {
            return;
        }
        if mode_of(&crate::settings::Settings::load(&self.store)) != MODE {
            return;
        }
        match self.dpi.start(&self.store, self.tun_enabled()).await {
            Ok(pid) => {
                self.set_dpi_on(true);
                tracing::info!(pid, mode = MODE, "движок обхода запущен по автозапуску");
            }
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "движок обхода не запустился"),
        }
    }
}
