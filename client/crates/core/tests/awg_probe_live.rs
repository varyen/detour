//! Живая проверка проб AmneziaWG через временный mihomo. Нужны сеть, AWG-сервер
//! и бинарники: DETOUR_SINGBOX, DETOUR_MIHOMO_BIN, AWG_CONF (клиентский .conf).
//! `cargo test -p detour-core --test awg_probe_live -- --ignored --nocapture`

use std::collections::HashMap;

use detour_core::engine::Engine;
use detour_core::probe::{self, Opts, Speed, Target};
use serde_json::{json, Value};

fn outbound(conf: &str) -> Value {
    let kv: HashMap<String, String> = conf
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let (host, port) = kv["Endpoint"].rsplit_once(':').unwrap();
    let mut amnezia = serde_json::Map::new();
    for k in ["Jc", "Jmin", "Jmax", "S1", "S2", "S3", "S4", "H1", "H2", "H3", "H4", "I1"] {
        if let Some(v) = kv.get(k) {
            amnezia.insert(k.to_lowercase(), json!(v));
        }
    }
    json!({
        "type": "amneziawg", "server": host, "server_port": port.parse::<u16>().unwrap(),
        "private_key": kv["PrivateKey"], "peer_public_key": kv["PublicKey"],
        "local_address": [kv["Address"]], "amnezia": amnezia,
    })
}

#[test]
#[ignore]
fn awg_profiles_get_verdicts() {
    let conf = std::fs::read_to_string(std::env::var("AWG_CONF").expect("AWG_CONF")).unwrap();
    let good = outbound(&conf);
    let mut bad = good.clone();
    bad["peer_public_key"] = json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    let targets = vec![
        Target { id: "awg-good".into(), outbound: good },
        Target { id: "awg-bad".into(), outbound: bad },
        Target { id: "socks-dead".into(), outbound: json!({ "type": "socks", "server": "127.0.0.1", "server_port": 9 }) },
    ];
    let tmp = std::env::temp_dir().join(format!("detour-awg-probe-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let engine = Engine::new(&tmp, tmp.join("work"), tmp.join("engine.log"));
    assert!(engine.mihomo_binary().is_file(), "mihomo: {}", engine.mihomo_binary().display());
    let urls = vec![("Google".to_owned(), "https://www.google.com/generate_204".to_owned())];
    let speed = Speed { url: "https://speed.cloudflare.com/__down?bytes=2000000".into(), bytes: 2_000_000, skip: Default::default() };
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let out = rt.block_on(probe::run(&engine, &tmp, targets, &Opts { urls: &urls, speed: Some(&speed) }));
    for (id, v) in &out {
        println!("{id}: ok={} rtt={} dl={} delays={:?}", v.ok, v.rtt, v.dl, v.delays);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(out["awg-good"].ok && out["awg-good"].rtt > 0);
    assert!(out["awg-good"].dl > 0);
    assert!(!out["awg-bad"].ok);
    assert!(!out["socks-dead"].ok);
}
