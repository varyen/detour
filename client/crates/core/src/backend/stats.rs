use anyhow::Result;
use serde_json::{json, Value};

use super::{parse_body, Backend, CLASH_PORT};
use crate::ipc::{Request, Response};
use crate::ping;
use crate::profiles;
use crate::settings::Settings;
use crate::store::now_epoch;
use crate::{geo, traffic};

const KEEPALIVE: &str = "run/keepalive.json";

impl Backend {
    /// Опрос clash API основного sing-box. Остановлен — счётчики сбрасываются:
    /// следующий запуск начнёт свои итоги с нуля.
    pub(super) async fn sample_traffic(&self) {
        if self.engine.pid().await.is_none() {
            self.meter.lock().expect("meter").reset();
            return;
        }
        if let Some(snap) = traffic::snapshot(CLASH_PORT, &self.clash_secret).await {
            self.meter.lock().expect("meter").ingest(&snap);
        }
    }

    pub(super) fn traffic_minute(&self) {
        let point = self.meter.lock().expect("meter").minute_point();
        if let Some(p) = point {
            traffic::append(&self.store, p);
        }
    }

    pub(super) async fn traffic_counters(&self) -> Response {
        if self.engine.pid().await.is_none() {
            return Response::json(&json!({ "ok": true, "supported": false }));
        }
        Response::json(&self.meter.lock().expect("meter").read())
    }

    pub(super) fn traffic_series(&self, req: &Request) -> Response {
        Response::json(&traffic::series(&self.store, req.param("range").unwrap_or("minute")))
    }

    pub(super) fn geo_status(&self) -> Response {
        Response::json(&geo::status(&self.store))
    }

    /// Синхронно, как на роутере; второй скан поверх идущего не запускается.
    pub(super) async fn geo_scan(&self, body: String) -> Result<Response> {
        let force = !body.trim().is_empty()
            && parse_body(&body)?.get("force").and_then(Value::as_bool).unwrap_or(false);
        let Ok(_guard) = self.geo_lock.try_lock() else {
            return Ok(Response::json(&json!({ "error": "scan already running" })));
        };
        let proxy = self.fetch_proxy().await;
        let _ = geo::scan(&self.store, force, proxy.as_deref()).await;
        let mut v = geo::status(&self.store);
        if let Some(o) = v.as_object_mut() {
            o.remove("supported");
        }
        Ok(Response::json(&v))
    }

    /// Раз в сутки: базу geo читает, только если появились новые адреса или
    /// прошёл месяц с полного обновления.
    pub(super) async fn geo_daily(&self) {
        let Ok(_guard) = self.geo_lock.try_lock() else { return };
        let proxy = self.fetch_proxy().await;
        let _ = geo::scan(&self.store, false, proxy.as_deref()).await;
    }

    pub(super) fn keepalive_status(&self) -> Response {
        Response::json(&self.store.read_json(KEEPALIVE).unwrap_or_else(|| json!({})))
    }

    /// Соединение с сервером выхода активной цепочки: ICMP, затем TCP.
    pub(super) async fn keepalive_check(&self) -> Response {
        let chain = Settings::load(&self.store).active_chain();
        let Some(id) = chain.last() else {
            return Response::json(&json!({ "ok": null, "error": "no active profile" }));
        };
        let ob = profiles::load(&self.store, id).and_then(|p| profiles::outbound(&p).cloned()).unwrap_or_default();
        let r = ping::ping(&ob, self.physical_source().await).await;
        let port = ping::endpoint(&ob).map(|(_, p)| p);
        let v = json!({
            "ok": r.ok, "server": r.server, "port": port, "profile": id,
            "rtt": r.rtt, "method": r.method, "last_checked": now_epoch(),
        });
        let _ = self.store.write_json(KEEPALIVE, &v);
        Response::json(&v)
    }

    pub(super) async fn keepalive_tick(&self) {
        if self.engine.pid().await.is_some() {
            self.keepalive_check().await;
        }
    }
}
