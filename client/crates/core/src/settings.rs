//! `settings.json`. На роутере все значения — строки, а писатель каждый раз
//! переписывает фиксированный набор ключей (остальное теряется). Здесь файл —
//! это просто объект: незнакомые ключи (например, пришедшие импортом с
//! роутера) сохраняются, значения читаются терпимо к числам и bool.

use serde_json::{Map, Value};

use crate::ids;
use crate::store::{self, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMode {
    ProxyList,
    AllExcept,
}

impl RoutingMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "proxy-list" => Some(Self::ProxyList),
            "all-except" => Some(Self::AllExcept),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProxyList => "proxy-list",
            Self::AllExcept => "all-except",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpMode {
    Off,
    List,
    All,
}

impl UdpMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "list" => Some(Self::List),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::List => "list",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Settings(Map<String, Value>);

impl Settings {
    pub fn load(store: &Store) -> Self {
        match store.read_json(store::SETTINGS) {
            Some(Value::Object(m)) => Self(m),
            _ => Self::default(),
        }
    }

    pub fn load_from(map: Map<String, Value>) -> Self {
        Self(map)
    }

    pub fn save(&self, store: &Store) -> std::io::Result<()> {
        store.write_json(store::SETTINGS, &Value::Object(self.0.clone()))
    }

    pub fn raw(&self) -> &Map<String, Value> {
        &self.0
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.0.insert(key.into(), Value::String(value.into()));
    }

    pub fn flag(&self, key: &str, default: bool) -> bool {
        match self.get(key).as_deref() {
            Some("1" | "true" | "yes" | "on") => true,
            Some("0" | "false" | "no" | "off") => false,
            _ => default,
        }
    }

    /// Хопы активной цепочки. У роутера по умолчанию профиль `default`; в
    /// клиенте его нет, поэтому пусто — «ничего не выбрано».
    pub fn active_chain(&self) -> Vec<String> {
        let chain = ids::parse_csv(&self.get("active_chain").unwrap_or_default());
        if !chain.is_empty() {
            return chain;
        }
        ids::parse_csv(&self.get("active_profile").unwrap_or_default())
    }

    pub fn set_active_chain(&mut self, hops: &[String]) {
        self.set("active_chain", hops.join(","));
        self.set("active_profile", hops.last().cloned().unwrap_or_default());
    }

    pub fn routing_mode(&self) -> RoutingMode {
        self.get("routing_mode")
            .and_then(|s| RoutingMode::parse(&s))
            .unwrap_or(RoutingMode::ProxyList)
    }

    pub fn udp_mode(&self) -> UdpMode {
        if let Some(m) = self.get("udp_vpn_mode").and_then(|s| UdpMode::parse(&s)) {
            return m;
        }
        if self.flag("discord_voice_vpn", false) {
            return UdpMode::List;
        }
        UdpMode::Off
    }

    /// «Все через VPN». На роутере это iptables-цепочка, которая не переживает
    /// полный stop; в клиенте — флаг, переключающий сборку конфига.
    pub fn allvpn(&self) -> bool {
        self.flag("allvpn", false)
    }

    /// Режим движка: singbox | hybrid | mihomo — те же значения, что на роутере.
    /// На телефонах есть только sing-box (libbox), поэтому там всегда гибрид.
    pub fn engine_mode(&self) -> EngineMode {
        if !crate::engine::MIHOMO_ENGINE_SUPPORTED {
            return EngineMode::Hybrid;
        }
        match self.get("engine_mode").as_deref() {
            Some("singbox") => EngineMode::Singbox,
            Some("mihomo") => EngineMode::Mihomo,
            _ => EngineMode::Hybrid,
        }
    }

    /// В режиме mihomo цепочка с запретом торрентов идёт через sing-box.
    pub fn engine_torrent_singbox(&self) -> bool {
        self.flag("engine_torrent_singbox", true)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineMode {
    Singbox,
    Hybrid,
    Mihomo,
}

impl EngineMode {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineMode::Singbox => "singbox",
            EngineMode::Hybrid => "hybrid",
            EngineMode::Mihomo => "mihomo",
        }
    }
}
