use std::collections::{BTreeMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;

use super::{parse_body, Backend, Job, Start};
use crate::ipc::{Request, Response};
use crate::lists;
use crate::ping::{self, Ping};
use crate::probe::{self, Speed, Target, Verdict};
use crate::profiles;
use crate::settings::Settings;
use crate::store::{self, now_epoch};

const DB: &str = "run/health.json";
const PING_DB: &str = "run/ping.json";
const SWITCH_COOLDOWN: i64 = 120;
const DEFAULT_SPEED_URL: &str = "https://speed.cloudflare.com/__down";

#[derive(Default, Serialize, Deserialize)]
struct HealthDb {
    #[serde(default)]
    results: BTreeMap<String, Verdict>,
    /// Когда профиль снова пора проверять плановым проходом.
    #[serde(default)]
    sched: BTreeMap<String, i64>,
    #[serde(default)]
    switch: Option<Value>,
}

pub(super) fn rand_between(lo: i64, hi: i64) -> i64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    lo + (u64::from_le_bytes(b) % (hi - lo).max(1) as u64) as i64
}

struct HealthCfg {
    enabled: bool,
    auto_switch: bool,
    speed: bool,
    speed_bytes: u64,
    speed_url: String,
}

impl Backend {
    fn health_cfg(&self) -> HealthCfg {
        let s = Settings::load(&self.store);
        let bytes = s
            .get("health_speed_bytes")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(8_000_000)
            .clamp(1_000_000, 300_000_000);
        HealthCfg {
            enabled: s.flag("health_check_enabled", true),
            auto_switch: s.flag("health_auto_switch", false),
            speed: s.flag("health_speed_enabled", true),
            speed_bytes: bytes,
            speed_url: s.get("health_speed_url").filter(|u| u.starts_with("http")).unwrap_or_default(),
        }
    }

    fn speed_opts(&self, cfg: &HealthCfg, honour_exclude: bool) -> Option<Speed> {
        if !cfg.speed {
            return None;
        }
        let url = if cfg.speed_url.is_empty() {
            format!("{DEFAULT_SPEED_URL}?bytes={}", cfg.speed_bytes)
        } else {
            cfg.speed_url.clone()
        };
        let skip = if honour_exclude {
            lists::parse_id_list(&self.store.read_text(store::SPEEDCHECK_EXCLUDE)).into_iter().collect()
        } else {
            HashSet::new()
        };
        Some(Speed { url, bytes: cfg.speed_bytes, skip })
    }

    fn health_db(&self) -> HealthDb {
        self.store.read_json(DB).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    fn save_health_db(&self, db: &HealthDb) {
        if let Ok(v) = serde_json::to_value(db) {
            let _ = self.store.write_json(DB, &v);
        }
    }

    /// WARP в одиночку из РФ не поднимается — проверять его отдельно бессмысленно.
    fn health_targets(&self, ids: &[String]) -> Vec<Target> {
        ids.iter()
            .filter_map(|id| {
                let p = profiles::load(&self.store, id)?;
                if p.get("warp").is_some() {
                    return None;
                }
                Some(Target { id: id.clone(), outbound: profiles::outbound(&p)?.clone() })
            })
            .collect()
    }

    async fn probe(&self, ids: &[String], speed: Option<Speed>) -> BTreeMap<String, Verdict> {
        let urls = probe::urls(&self.store);
        let opts = probe::Opts { urls: &urls, speed: speed.as_ref() };
        probe::run(&self.engine, &self.store.path("run"), self.health_targets(ids), &opts).await
    }

    /// Вердикты в базу. Проба без замера скорости не стирает прошлую скорость.
    async fn record(&self, verdicts: &BTreeMap<String, Verdict>, measured_speed: bool, reschedule: bool) {
        let _g = self.health_lock.lock().await;
        let mut db = self.health_db();
        let now = now_epoch();
        for (id, v) in verdicts {
            let mut v = v.clone();
            if !measured_speed {
                v.dl = db.results.get(id).map(|o| o.dl).unwrap_or(-1);
            }
            db.results.insert(id.clone(), v);
            if reschedule {
                db.sched.insert(id.clone(), now + rand_between(2700, 3600));
            }
        }
        let live: HashSet<String> = profiles::list_ids(&self.store).into_iter().collect();
        db.results.retain(|id, _| live.contains(id));
        db.sched.retain(|id, _| live.contains(id));
        self.save_health_db(&db);
    }

    pub(super) fn health_status(&self) -> Response {
        let cfg = self.health_cfg();
        let db = self.health_db();
        let urls: Vec<Value> = probe::urls(&self.store).into_iter().map(|(l, u)| json!({ "label": l, "url": u })).collect();
        Response::json(&json!({
            "now": now_epoch(),
            "supported": self.engine.probes_supported(),
            "enabled": cfg.enabled,
            "auto_switch": cfg.auto_switch,
            "speed": cfg.speed,
            "speed_bytes": cfg.speed_bytes,
            "urls": urls,
            "switch": db.switch,
            "results": db.results,
        }))
    }

    pub(super) async fn health_check(&self, req: &Request) -> Result<Response> {
        let Some(id) = req.param("id").filter(|s| !s.is_empty()) else {
            let _ = self.jobs.send(Job::HealthSweep);
            return Ok(Response::json(&json!({ "ok": true, "started": true })));
        };
        if !profiles::exists(&self.store, id) {
            return Err(anyhow!("profile not found"));
        }
        let cfg = self.health_cfg();
        let ids = vec![id.to_owned()];
        let speed = self.speed_opts(&cfg, false);
        let measured = speed.is_some();
        let verdicts = self.probe(&ids, speed).await;
        let result = verdicts.get(id).cloned();
        self.record(&verdicts, measured, true).await;
        Ok(Response::json(&json!({ "ok": true, "id": id, "result": result })))
    }

    pub(super) fn health_config(&self, body: String) -> Result<Response> {
        let b = parse_body(&body)?;
        let flag = |k: &str| match b.get(k) {
            Some(Value::Bool(v)) => Some(*v),
            Some(Value::Number(n)) => Some(n.as_i64() != Some(0)),
            Some(Value::String(s)) => Some(matches!(s.as_str(), "1" | "true" | "on")),
            _ => None,
        };
        let mut s = Settings::load(&self.store);
        for (key, setting) in [("enabled", "health_check_enabled"), ("auto_switch", "health_auto_switch"), ("speed", "health_speed_enabled")] {
            if let Some(v) = flag(key) {
                s.set(setting, if v { "1" } else { "0" });
            }
        }
        let bytes = match b.get("speed_bytes") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(v)) => v.parse().ok(),
            _ => None,
        };
        if let Some(n) = bytes {
            s.set("health_speed_bytes", n.clamp(1_000_000, 300_000_000).to_string());
        }
        s.save(&self.store)?;
        let cfg = self.health_cfg();
        Ok(Response::json(&json!({
            "ok": true, "enabled": cfg.enabled, "auto_switch": cfg.auto_switch,
            "speed": cfg.speed, "speed_bytes": cfg.speed_bytes,
        })))
    }

    pub(super) fn health_urls(&self, req: &Request, body: String) -> Result<Response> {
        if req.body.is_none() {
            return Ok(Response::json(&json!({ "list": self.store.read_text(store::HEALTH_URLS) })));
        }
        self.store.write_text(store::HEALTH_URLS, &body)?;
        Ok(Response::json(&json!({ "ok": true })))
    }

    /// Кандидат на автопереключение — из уже проверенных: рабочий, не
    /// текущий, не исключённый; побеждает меньшая задержка. Перед
    /// переключением не перепроверяется — так же, как на роутере.
    fn candidate(&self, db: &HealthDb, current: &str) -> Option<String> {
        let excluded = lists::parse_id_list(&self.store.read_text(store::AUTOSWITCH_EXCLUDE));
        db.results
            .iter()
            .filter(|(id, v)| v.ok && id.as_str() != current && !excluded.contains(id) && profiles::exists(&self.store, id))
            .min_by_key(|(_, v)| if v.rtt > 0 { v.rtt } else { 999_999 })
            .map(|(id, _)| id.clone())
    }

    async fn autoswitch(&self, from: &str) -> Option<String> {
        let cfg = self.health_cfg();
        if !cfg.auto_switch {
            return None;
        }
        let db = self.health_db();
        if let Some(ts) = db.switch.as_ref().and_then(|s| s.get("ts")).and_then(Value::as_i64) {
            if now_epoch() - ts < SWITCH_COOLDOWN {
                return None;
            }
        }
        let to = self.candidate(&db, from)?;
        if let Err(e) = self.apply(Some(vec![to.clone()]), Start::IfRunning).await {
            tracing::warn!(error = %format!("{e:#}"), from, to, "автопереключение не удалось");
            return None;
        }
        let _g = self.health_lock.lock().await;
        let mut db = self.health_db();
        db.switch = Some(json!({ "from": from, "to": to, "ts": now_epoch() }));
        self.save_health_db(&db);
        tracing::info!(from, to, "автопереключение");
        Some(to)
    }

    /// Проверка активного профиля. Провал подтверждается повторной проверкой:
    /// один сбой сети не повод дёргать соединение.
    pub(super) async fn health_active(&self) {
        let cfg = self.health_cfg();
        if !cfg.enabled || self.engine.pid().await.is_none() {
            return;
        }
        let chain = Settings::load(&self.store).active_chain();
        let [id] = chain.as_slice() else { return };
        let ids = vec![id.clone()];
        let mut v = self.probe(&ids, None).await;
        if v.get(id).is_some_and(|r| !r.ok) {
            v = self.probe(&ids, None).await;
        }
        let Some(verdict) = v.get(id).cloned() else { return };
        self.record(&v, false, false).await;
        if !verdict.ok {
            self.autoswitch(id).await;
        }
    }

    /// Плановый проход: каждый профиль раз в 45–60 минут, новые — со
    /// случайной фазой, чтобы не проверять всё разом. `all` — полная проверка.
    pub(super) async fn health_tick(&self, all: bool) {
        let cfg = self.health_cfg();
        if !cfg.enabled || !self.engine.probes_supported() {
            return;
        }
        let _sweep = self.sweep_lock.lock().await;
        let now = now_epoch();
        let ids = profiles::list_ids(&self.store);
        let due: Vec<String> = {
            let _g = self.health_lock.lock().await;
            let mut db = self.health_db();
            let first = db.results.is_empty();
            let mut due = Vec::new();
            for id in &ids {
                match db.sched.get(id) {
                    _ if all || first => due.push(id.clone()),
                    Some(t) if *t <= now => due.push(id.clone()),
                    Some(_) => {}
                    None => {
                        db.sched.insert(id.clone(), now + rand_between(0, 3600));
                    }
                }
            }
            self.save_health_db(&db);
            due
        };
        if due.is_empty() {
            return;
        }
        let speed = self.speed_opts(&cfg, true);
        let measured = speed.is_some();
        let verdicts = self.probe(&due, speed).await;
        self.record(&verdicts, measured, true).await;

        let chain = Settings::load(&self.store).active_chain();
        if let [active] = chain.as_slice() {
            if verdicts.get(active).is_some_and(|v| !v.ok) && self.engine.pid().await.is_some() {
                let again = self.probe(std::slice::from_ref(active), None).await;
                if again.get(active).is_some_and(|v| !v.ok) {
                    self.record(&again, false, false).await;
                    self.autoswitch(active).await;
                }
            }
        }
    }

    fn ping_db(&self) -> BTreeMap<String, Ping> {
        self.store.read_json(PING_DB).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    /// Адрес физического интерфейса: при работающем TUN — запомненный до старта.
    pub(super) async fn physical_source(&self) -> Option<Ipv4Addr> {
        if self.engine.pid().await.is_none() {
            let fresh = ping::physical_ipv4();
            if fresh.is_some() {
                *self.phys_src.lock().expect("phys_src") = fresh;
            }
            return fresh;
        }
        *self.phys_src.lock().expect("phys_src")
    }

    pub(super) fn ping_status(&self) -> Response {
        Response::json(&json!({ "now": now_epoch(), "results": self.ping_db() }))
    }

    pub(super) async fn ping_check(&self, req: &Request) -> Result<Response> {
        let id = req.param("id").unwrap_or("").to_owned();
        let p = profiles::load(&self.store, &id).ok_or_else(|| anyhow!("profile not found"))?;
        let ob = profiles::outbound(&p).cloned().unwrap_or_default();
        let r = ping::ping(&ob, self.physical_source().await).await;
        let _g = self.health_lock.lock().await;
        let mut db = self.ping_db();
        db.insert(id.clone(), r.clone());
        let _ = self.store.write_json(PING_DB, &serde_json::to_value(&db)?);
        let mut v = serde_json::to_value(r)?;
        v["id"] = json!(id);
        Ok(Response::json(&v))
    }

    pub(super) async fn ping_all(&self) {
        let src = self.physical_source().await;
        let limit = Arc::new(tokio::sync::Semaphore::new(16));
        let mut set = JoinSet::new();
        for id in profiles::list_ids(&self.store) {
            let Some(ob) = profiles::load(&self.store, &id).and_then(|p| profiles::outbound(&p).cloned()) else { continue };
            let limit = limit.clone();
            set.spawn(async move {
                let _permit = limit.acquire_owned().await;
                (id, ping::ping(&ob, src).await)
            });
        }
        let mut fresh = BTreeMap::new();
        while let Some(Ok((id, r))) = set.join_next().await {
            fresh.insert(id, r);
        }
        if fresh.is_empty() {
            return;
        }
        let _g = self.health_lock.lock().await;
        if let Ok(v) = serde_json::to_value(&fresh) {
            let _ = self.store.write_json(PING_DB, &v);
        }
    }
}
