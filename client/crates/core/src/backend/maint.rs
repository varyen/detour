use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};

use super::health::rand_between;
use super::{parse_body, Backend, Job, Start};
use crate::ipc::{Request, Response};
use crate::rulist;
use crate::settings::Settings;
use crate::store::{self, now_epoch};
use crate::updater;

const APPLY_LOG: &str = "run/apply.log";
const SENTINEL: &str = "===DETOUR_APPLY_DONE rc=";
const AUTOCHECK_EVERY: i64 = 6 * 3600;

/// Отказ для платформ, где движки зашиты в приложение.
fn bundled() -> Option<Response> {
    (!crate::dpi::UPDATABLE).then(|| {
        Response::json(&json!({
            "status": "error",
            "message": "движки зашиты в приложение и обновляются вместе с ним",
        }))
    })
}

impl Backend {
    // ---------- российские подсети ----------

    pub(super) fn rulist_status(&self) -> Response {
        Response::json(&rulist::status(&self.store))
    }

    fn rulist_reply(&self) -> Response {
        let mut v = rulist::status(&self.store);
        if let Some(o) = v.as_object_mut() {
            o.remove("supported");
        }
        Response::json(&v)
    }

    pub(super) async fn rulist_set(&self, body: String) -> Result<Response> {
        let (_, changed) = rulist::apply_settings(&self.store, &parse_body(&body)?)?;
        if changed {
            self.reapply_quiet().await;
        }
        Ok(self.rulist_reply())
    }

    /// Ошибка загрузки — не ошибка запроса: панель показывает её из `error`
    /// в объекте статуса, как у роутера.
    pub(super) async fn rulist_update(&self, body: String) -> Result<Response> {
        let source = if body.trim().is_empty() {
            rulist::load(&self.store).source
        } else {
            parse_body(&body)?.get("source").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| rulist::load(&self.store).source)
        };
        let proxy = self.fetch_proxy().await;
        if rulist::update(&self.store, &source, proxy.as_deref()).await.is_ok() {
            self.reapply_quiet().await;
        }
        Ok(self.rulist_reply())
    }

    pub(super) async fn rulist_exclude(&self, req: &Request, body: String) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ "exclude": self.store.read_text(store::RU_EXCLUDE) })));
        }
        self.store.write_text(store::RU_EXCLUDE, &body)?;
        self.reapply_quiet().await;
        Ok(self.rulist_reply())
    }

    async fn reapply_quiet(&self) {
        if Settings::load(&self.store).active_chain().is_empty() {
            return;
        }
        if let Err(e) = self.apply(None, Start::IfRunning).await {
            tracing::warn!(error = %format!("{e:#}"), "пересборка после правки RU-списка");
        }
    }

    // ---------- обновление sing-box ----------

    pub(super) async fn bins_check(&self) -> Response {
        if let Some(r) = bundled() {
            return r;
        }
        let proxy = self.fetch_proxy().await;
        match updater::check(&self.store, &self.engine, proxy.as_deref()).await {
            Ok(v) => Response::json(&v),
            Err(e) => Response::json(&json!({ "status": "error", "message": format!("проверка не удалась: {e:#}") })),
        }
    }

    // ---------- winws2 ----------

    pub(super) async fn dpi_bins_check(&self) -> Response {
        if let Some(r) = bundled() {
            return r;
        }
        let proxy = self.fetch_proxy().await;
        match updater::dpi_check(&self.store, &self.dpi, proxy.as_deref()).await {
            Ok(v) => Response::json(&v),
            Err(e) => Response::json(&json!({ "status": "error", "message": format!("проверка не удалась: {e:#}") })),
        }
    }

    pub(super) fn dpi_bins_status(&self) -> Response {
        Response::json(&updater::dpi_state(&self.store))
    }

    pub(super) fn dpi_bins_apply(&self) -> Response {
        if let Some(r) = bundled() {
            return r;
        }
        let _ = self.store.write_text(APPLY_LOG, &format!("установка {}: начинаю\n", crate::dpi::NAME));
        let _ = self.jobs.send(Job::DpiApply);
        Response::json(&json!({ "ok": true, "status": "started" }))
    }

    /// Движок ставится в каталог данных; работающий winws2 перед этим
    /// останавливается, иначе файл не перезаписать.
    pub(super) async fn dpi_apply_job(&self) {
        let proxy = self.fetch_proxy().await;
        let was_running = self.dpi.pid().await.is_some();
        self.dpi.stop().await;
        let mut lines = Vec::new();
        let res = updater::dpi_install(&self.store, &self.dpi, proxy.as_deref(), &mut |l| lines.push(l)).await;
        for l in &lines {
            self.log_line(l);
        }
        let rc = match res {
            Ok(_) => {
                if was_running {
                    match self.dpi.start(&self.store, self.tun_enabled()).await {
                        Ok(_) => self.log_line("обход DPI перезапущен"),
                        Err(e) => self.log_line(&format!("перезапуск не удался: {e:#}")),
                    }
                }
                0
            }
            Err(e) => {
                self.log_line(&format!("ошибка: {e:#}"));
                1
            }
        };
        self.log_line(&format!("{SENTINEL}{rc}"));
    }

    pub(super) fn bins_status(&self) -> Response {
        Response::json(&updater::state(&self.store))
    }

    /// Состояние «в работе» пишется до ответа: панель опрашивает журнал сразу.
    pub(super) fn bins_apply(&self) -> Response {
        if let Some(r) = bundled() {
            return r;
        }
        let _ = self.store.write_text(APPLY_LOG, "обновление sing-box: начинаю\n");
        let _ = self.jobs.send(Job::BinsApply);
        Response::json(&json!({ "ok": true, "status": "started" }))
    }

    fn log_line(&self, line: &str) {
        let mut text = self.store.read_text(APPLY_LOG);
        text.push_str(line);
        text.push('\n');
        let _ = self.store.write_text(APPLY_LOG, &text);
    }

    pub(super) async fn bins_apply_job(&self) {
        let proxy = self.fetch_proxy().await;
        let mut lines = Vec::new();
        let res = updater::install(&self.store, &self.engine, proxy.as_deref(), &mut |l| lines.push(l)).await;
        for l in &lines {
            self.log_line(l);
        }
        let rc = match res {
            Ok(_) => {
                if self.engine.pid().await.is_some() {
                    match self.apply(None, Start::IfRunning).await {
                        Ok(()) => self.log_line("sing-box перезапущен на новой версии"),
                        Err(e) => self.log_line(&format!("перезапуск не удался: {e:#}")),
                    }
                }
                let proxy = self.fetch_proxy().await;
                let _ = updater::check(&self.store, &self.engine, proxy.as_deref()).await;
                0
            }
            Err(e) => {
                self.log_line(&format!("ошибка: {e:#}"));
                1
            }
        };
        self.log_line(&format!("{SENTINEL}{rc}"));
    }

    pub(super) fn apply_log(&self) -> Response {
        let text = self.store.read_text(APPLY_LOG);
        let (log, rc) = match text.find(SENTINEL) {
            Some(i) => (text[..i].to_owned(), text[i + SENTINEL.len()..].trim().to_owned()),
            None => (text.clone(), String::new()),
        };
        Response::json(&json!({ "ok": true, "done": !rc.is_empty(), "rc": rc, "log": log }))
    }

    pub(super) async fn updates_overview(&self) -> Response {
        let st = updater::state(&self.store);
        let s = |k: &str| st.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let current = self.engine.version().await.unwrap_or_else(|| s("current"));
        let available = s("available");
        let has_update = !available.is_empty() && available != current;
        Response::json(&json!({
            "singbox": {
                "update_available": has_update,
                "available_version": available,
                "current_version": current,
                "last_check": s("last_check"),
                "upstream": s("upstream"),
                "upstream_newer": !has_update && st.get("upstream_newer").and_then(Value::as_bool).unwrap_or(false),
                "changelog_b64": s("changelog_b64"),
                "error": if s("status") == "error" { s("message") } else { String::new() },
            }
        }))
    }

    fn autocheck_on(&self) -> bool {
        Settings::load(&self.store).flag("autocheck", true)
    }

    pub(super) fn autocheck_status(&self) -> Response {
        Response::json(&json!({ "enabled": self.autocheck_on() }))
    }

    pub(super) fn autocheck_set(&self, body: String) -> Result<Response> {
        let on = parse_body(&body)?.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let mut s = Settings::load(&self.store);
        s.set("autocheck", if on { "1" } else { "0" });
        s.save(&self.store)?;
        Ok(Response::json(&json!({ "enabled": on })))
    }

    /// Раз в час: RU-список (если включено автообновление и прошло 6 суток) и
    /// проверка новой версии sing-box (раз в 6 часов).
    async fn maintenance(&self) {
        let c = rulist::load(&self.store);
        if c.enabled && c.auto && now_epoch() - c.updated >= rulist::STALE_AFTER {
            let proxy = self.fetch_proxy().await;
            if rulist::update(&self.store, &c.source, proxy.as_deref()).await.is_ok() {
                self.reapply_quiet().await;
            }
        }
        if self.autocheck_on() && crate::dpi::UPDATABLE && self.engine.present() {
            let st = updater::state(&self.store);
            let last = st
                .get("last_check")
                .and_then(Value::as_str)
                .and_then(|t| humantime::parse_rfc3339(t).ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if now_epoch() - last >= AUTOCHECK_EVERY {
                let proxy = self.fetch_proxy().await;
                let _ = updater::check(&self.store, &self.engine, proxy.as_deref()).await;
            }
        }
    }

    async fn run_job(&self, job: Job) {
        match job {
            Job::HealthSweep => self.health_tick(true).await,
            Job::BinsApply => self.bins_apply_job().await,
            Job::DpiApply => self.dpi_apply_job().await,
        }
    }
}

fn every<F, Fut>(b: &Arc<Backend>, first: u64, period: u64, f: F)
where
    F: Fn(Arc<Backend>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let b = b.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(first)).await;
        let mut t = tokio::time::interval(Duration::from_secs(period));
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            t.tick().await;
            f(b.clone()).await;
        }
    });
}

impl Backend {
    /// Все фоновые задачи службы: планировщик вместо cron роутера.
    pub async fn run_background(self: Arc<Self>) {
        if let Some(mut rx) = self.job_rx.lock().expect("job_rx").take() {
            let b = self.clone();
            tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    b.run_job(job).await;
                }
            });
        }
        self.boot().await;
        every(&self, 60, 600, |b| async move { b.refresh_due_subscriptions().await });
        every(&self, 90, 120, |b| async move { b.health_tick(false).await });
        every(&self, 20, 120, |b| async move { b.ping_all().await });
        every(&self, 300, 3600, |b| async move { b.maintenance().await });
        // Раз в секунду: соединение, открывшееся и закрывшееся между опросами,
        // в `/connections` не видно вовсе, и его байты приходится угадывать.
        every(&self, 1, 1, |b| async move { b.sample_traffic().await });
        every(&self, 60, 60, |b| async move { b.traffic_minute() });
        // Сторож: упавший движок = трафик мимо VPN. Пока он не заметил, утечка
        // идёт, поэтому интервал маленький.
        every(&self, 2, 2, |b| async move { b.guard_tick().await });
        every(&self, 45, 300, |b| async move { b.keepalive_tick().await });
        every(&self, 900, 86400, |b| async move { b.geo_daily().await });
        let b = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(rand_between(30, 60) as u64)).await;
                b.health_active().await;
            }
        });
    }
}
