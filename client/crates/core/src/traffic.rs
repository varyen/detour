//! Счётчики трафика по направлениям — замена роутерных detour-meter и
//! detour-trafficlog. Файрвольных счётчиков нет, поэтому источник — clash API
//! основного sing-box: `/connections` отдаёт байты каждого соединения и
//! outbound, `downloadTotal`/`uploadTotal` — точные итоги. Трафик соединений,
//! закрывшихся между опросами, в `/connections` уже не виден — эта часть
//! раскладывается по полосам пропорционально, отсюда `exact: false`.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::store::{now_epoch, Store};

const SERIES: &str = "run/traffic.json";
const MINUTES: usize = 1440;
const HOURS: usize = 720;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Lanes {
    pub direct: u64,
    pub vpn: u64,
    pub bypass: u64,
    pub rx: u64,
    pub tx: u64,
}

impl Lanes {
    fn sub(&self, o: &Lanes) -> Lanes {
        Lanes {
            direct: self.direct.saturating_sub(o.direct),
            vpn: self.vpn.saturating_sub(o.vpn),
            bypass: self.bypass.saturating_sub(o.bypass),
            rx: self.rx.saturating_sub(o.rx),
            tx: self.tx.saturating_sub(o.tx),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub ts: i64,
    pub direct: u64,
    pub vpn: u64,
    pub bypass: u64,
    pub rx: u64,
    pub tx: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Series {
    #[serde(default)]
    minute: VecDeque<Point>,
    #[serde(default)]
    hour: VecDeque<Point>,
}

#[derive(Clone, Copy, PartialEq)]
enum Lane {
    Direct,
    Vpn,
    Bypass,
}

#[derive(Default)]
pub struct Meter {
    conns: HashMap<String, (u64, u64, u8)>,
    totals: Option<(u64, u64)>,
    acc: Lanes,
    read_base: Option<(Instant, Lanes)>,
    minute_base: Option<Lanes>,
}

fn lane_of(c: &Value) -> Lane {
    let first = c.get("chains").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("");
    let rule = c.get("rule").and_then(Value::as_str).unwrap_or("");
    // mihomo пишет «DIRECT» и кладёт имя rule-set'а в rulePayload («set-dpi»)
    let payload = c.get("rulePayload").and_then(Value::as_str).unwrap_or("");
    let direct = first.is_empty() || first.eq_ignore_ascii_case("direct");
    match (direct, rule.contains("dpi") || payload.contains("dpi")) {
        (true, true) => Lane::Bypass,
        (true, false) => Lane::Direct,
        _ => Lane::Vpn,
    }
}

fn spread(lanes: &mut [u64; 3], rest: u64, weights: &[u64; 3]) {
    let sum: u128 = weights.iter().map(|&w| u128::from(w)).sum();
    let mut given = 0;
    for (lane, &w) in lanes.iter_mut().zip(weights) {
        let share = (u128::from(rest) * u128::from(w) / sum) as u64;
        *lane += share;
        given += share;
    }
    let top = (0..3).max_by_key(|&i| weights[i]).unwrap_or(0);
    lanes[top] += rest - given;
}

impl Meter {
    /// Один опрос `/connections`. Перезапуск sing-box обнуляет его итоги —
    /// тогда база начинается заново, накопленное не теряется.
    pub fn ingest(&mut self, snap: &Value) {
        let down = snap.get("downloadTotal").and_then(Value::as_u64).unwrap_or(0);
        let up = snap.get("uploadTotal").and_then(Value::as_u64).unwrap_or(0);
        let (d_rx, d_tx) = match self.totals {
            Some((pd, pu)) if down >= pd && up >= pu => (down - pd, up - pu),
            Some(_) => {
                self.conns.clear();
                (down, up)
            }
            None => (0, 0),
        };
        let first_sample = self.totals.is_none();
        self.totals = Some((down, up));

        let mut seen = HashMap::new();
        let mut by_lane = [0u64; 3];
        for c in snap.get("connections").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = c.get("id").and_then(Value::as_str) else { continue };
            let u = c.get("upload").and_then(Value::as_u64).unwrap_or(0);
            let d = c.get("download").and_then(Value::as_u64).unwrap_or(0);
            let lane = lane_of(c) as u8;
            let (pu, pd) = self.conns.remove(id).map(|(u, d, _)| (u, d)).unwrap_or((0, 0));
            if !first_sample {
                by_lane[lane as usize] += u.saturating_sub(pu) + d.saturating_sub(pd);
            }
            seen.insert(id.to_owned(), (u, d, lane));
        }
        // Что осталось в прежнем снимке — соединения, закрывшиеся после него:
        // их последние байты в `/connections` уже не попали.
        let mut vanished = [0u64; 3];
        for (u, d, lane) in self.conns.values() {
            vanished[*lane as usize] += u + d + 1;
        }
        self.conns = seen;
        if first_sample {
            // Точка ряда считается от старта sing-box, а не от первого
            // минутного тика: иначе трафик первой минуты пропадал.
            if self.minute_base.is_none() {
                self.minute_base = Some(self.acc);
            }
            return;
        }

        let total = d_rx + d_tx;
        let seen_sum: u64 = by_lane.iter().sum();
        if total > seen_sum {
            // Невидимый остаток: сначала хвосты закрывшихся соединений, иначе —
            // как делились видимые байты, иначе — как делилось всё до сих пор.
            let history = [self.acc.direct, self.acc.vpn, self.acc.bypass];
            let weights = [vanished, by_lane, history]
                .into_iter()
                .find(|w| w.iter().sum::<u64>() > 0)
                .unwrap_or([1, 0, 0]);
            spread(&mut by_lane, total - seen_sum, &weights);
        }
        self.acc.direct += by_lane[Lane::Direct as usize];
        self.acc.vpn += by_lane[Lane::Vpn as usize];
        self.acc.bypass += by_lane[Lane::Bypass as usize];
        self.acc.rx += d_rx;
        self.acc.tx += d_tx;
    }

    /// Ответ `traffic_counters`: прирост с прошлого вызова. Первый вызов —
    /// прогрев (`warming`), долей ещё нет.
    pub fn read(&mut self) -> Value {
        let now = Instant::now();
        let prev = self.read_base.replace((now, self.acc));
        let Some((t0, base)) = prev else {
            return json!({ "ok": true, "supported": true, "warming": true, "exact": false,
                "direct": 0, "vpn": 0, "bypass": 0, "total": 100,
                "bytes": { "direct": 0, "vpn": 0, "bypass": 0, "total": 0, "rx": 0, "tx": 0 }, "span": 0, "wan": "" });
        };
        let d = self.acc.sub(&base);
        let lanes = d.direct + d.vpn + d.bypass;
        let pct = |v: u64| if lanes == 0 { 0 } else { (v * 100 + lanes / 2) / lanes };
        json!({
            "ok": true, "supported": true, "warming": false, "exact": false,
            "direct": pct(d.direct), "vpn": pct(d.vpn), "bypass": pct(d.bypass), "total": 100,
            "bytes": { "direct": d.direct, "vpn": d.vpn, "bypass": d.bypass, "total": lanes, "rx": d.rx, "tx": d.tx },
            "span": now.duration_since(t0).as_secs().max(1), "wan": "",
        })
    }

    /// Раз в минуту: точка ряда за прошедшую минуту.
    pub fn minute_point(&mut self) -> Option<Point> {
        let prev = self.minute_base.replace(self.acc)?;
        let d = self.acc.sub(&prev);
        Some(Point { ts: now_epoch(), direct: d.direct, vpn: d.vpn, bypass: d.bypass, rx: d.rx, tx: d.tx })
    }

    /// sing-box остановлен: его соединения и итоги больше не действительны.
    /// Накопленное не трогаем — иначе перезапуск (в том числе авто-переключение
    /// профиля) стирал бы трафик текущей минуты из графика.
    pub fn reset(&mut self) {
        self.conns.clear();
        self.totals = None;
    }
}

pub async fn snapshot(port: u16, secret: &str) -> Option<Value> {
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().ok()?;
    let r = client.get(format!("http://127.0.0.1:{port}/connections")).bearer_auth(secret).send().await.ok()?;
    r.json().await.ok()
}

fn load(store: &Store) -> Series {
    store.read_json(SERIES).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub fn append(store: &Store, p: Point) {
    let mut s = load(store);
    s.minute.push_back(p);
    while s.minute.len() > MINUTES {
        s.minute.pop_front();
    }
    let hour = p.ts - p.ts % 3600;
    match s.hour.back_mut() {
        Some(h) if h.ts == hour => {
            h.direct += p.direct;
            h.vpn += p.vpn;
            h.bypass += p.bypass;
            h.rx += p.rx;
            h.tx += p.tx;
        }
        _ => s.hour.push_back(Point { ts: hour, ..p }),
    }
    while s.hour.len() > HOURS {
        s.hour.pop_front();
    }
    if let Ok(v) = serde_json::to_value(&s) {
        let _ = store.write_json(SERIES, &v);
    }
}

pub fn series(store: &Store, range: &str) -> Value {
    let s = load(store);
    let (step, pts) = if range == "hour" { (3600, s.hour) } else { (60, s.minute) };
    json!({ "ok": true, "supported": true, "step": step, "points": pts })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(down: u64, up: u64, conns: &[(&str, u64, u64, &str, &str)]) -> Value {
        json!({
            "downloadTotal": down, "uploadTotal": up,
            "connections": conns.iter().map(|(id, u, d, ch, rule)| json!({
                "id": id, "upload": u, "download": d, "chains": [ch], "rule": rule
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn lanes_and_unseen_remainder() {
        let mut m = Meter::default();
        m.ingest(&snap(0, 0, &[]));
        m.read();
        m.ingest(&snap(900, 100, &[("a", 50, 450, "proxy", "final"), ("b", 50, 150, "direct", "ip_is_private"), ("c", 0, 100, "direct", "rule_set=dpi")]));
        let r = m.read();
        assert_eq!(r["bytes"]["rx"], 900);
        assert_eq!(r["bytes"]["tx"], 100);
        // 800 байт видны по соединениям, 200 — закрывшиеся, разложены пропорционально.
        assert_eq!(r["bytes"]["total"], 1000);
        assert_eq!(r["bytes"]["vpn"], 625);
        assert_eq!(r["bytes"]["bypass"], 125);
        assert_eq!(r["vpn"], 63);
    }

    #[test]
    fn unseen_bytes_follow_closed_connections_then_history() {
        let mut m = Meter::default();
        m.ingest(&snap(0, 0, &[]));
        m.ingest(&snap(110, 0, &[("big", 0, 100, "proxy", "final"), ("d", 0, 10, "direct", "final")]));
        // «big» закрылся, докачав ещё 1000 байт, которых снимок уже не видел.
        m.ingest(&snap(1110, 0, &[("d", 0, 10, "direct", "final")]));
        assert_eq!(m.acc.vpn, 1100);
        assert_eq!(m.acc.direct, 10);
        // Соединение целиком между опросами: делится как всё накопленное.
        m.ingest(&snap(2220, 0, &[("d", 0, 10, "direct", "final")]));
        assert_eq!(m.acc.vpn, 2200);
        assert_eq!(m.acc.direct, 20);
    }

    #[test]
    fn restart_resets_base() {
        let mut m = Meter::default();
        m.ingest(&snap(1000, 1000, &[]));
        m.ingest(&snap(10, 0, &[]));
        assert_eq!(m.acc.rx, 10);
    }

    #[test]
    fn restart_keeps_current_minute() {
        let mut m = Meter::default();
        m.ingest(&snap(0, 0, &[]));
        m.ingest(&snap(5000, 0, &[]));
        // Перезапуск sing-box посреди минуты: его итоги сбрасываются, наши — нет.
        m.reset();
        m.ingest(&snap(0, 0, &[]));
        m.ingest(&snap(100, 0, &[]));
        let p = m.minute_point().expect("точка");
        assert_eq!(p.rx, 5100);
    }
}
