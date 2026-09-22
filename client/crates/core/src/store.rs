//! Раскладка файлов в каталоге данных. Имена и форматы те же, что на роутере
//! (`/etc/sing-box/…`, `/etc/detour/…`), чтобы экспорт/импорт конфигурации
//! переносился между платформами как есть.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const SETTINGS: &str = "settings.json";
pub const CHAINS: &str = "chains.json";
pub const ROUTE_MAP: &str = "route-map.list";
pub const PROFILES_DIR: &str = "profiles";

pub const PROXY_DOMAINS: &str = "lists/proxy-domains.list";
pub const WHITELIST: &str = "lists/whitelist-domains.list";
pub const RU_SUBNETS: &str = "lists/ru-subnets.list";
pub const RU_EXCLUDE: &str = "lists/ru-subnets-exclude.list";
pub const UDP_VPN: &str = "lists/udp-vpn.list";
pub const DPI_DOMAINS: &str = "lists/zapret-domains.list";
pub const EGRESS_BLOCK: &str = "lists/blocked-egress-ips.list";

pub const AUTOSWITCH_EXCLUDE: &str = "flags/autoswitch-exclude.list";
pub const SPEEDCHECK_EXCLUDE: &str = "flags/speedcheck-exclude.list";
pub const TORRENT_ALLOW: &str = "flags/torrent-allow.list";
pub const AUTOSTART_SINGBOX: &str = "flags/autostart.singbox";
pub const AUTOSTART_DPI: &str = "flags/autostart.dpi";
/// Стратегия winws2 одной строкой — как `nfqws2.strategy` на роутере.
pub const DPI_STRATEGY: &str = "dpi.strategy";
/// Намерение «VPN должен работать»: по нему сторож поднимает упавший sing-box.
pub const WANT_SINGBOX: &str = "run/want.singbox";

pub const HEALTH_URLS: &str = "health-urls.list";
pub const CONFIG: &str = "run/config.json";
pub const RULESETS: &str = "run/rules";
pub const STAGING: &str = "run/staging";
pub const SINGBOX_LOG: &str = "logs/sing-box.log";
pub const SINGBOX_STDERR: &str = "logs/sing-box.stderr.log";

pub const UDP_VPN_SEED: &str = "19294:19344 // Discord voice\n";

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Отсутствующий файл — пустой текст: у роутера так же ведут себя все
    /// списки (`cat 2>/dev/null`).
    pub fn read_text(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap_or_default()
    }

    pub fn write_text(&self, rel: &str, text: &str) -> io::Result<()> {
        write_atomic(&self.path(rel), text.as_bytes())
    }

    pub fn read_json(&self, rel: &str) -> Option<Value> {
        let raw = std::fs::read(self.path(rel)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub fn write_json(&self, rel: &str, v: &Value) -> io::Result<()> {
        let buf = serde_json::to_vec_pretty(v).map_err(io::Error::other)?;
        write_atomic(&self.path(rel), &buf)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    pub fn flag(&self, rel: &str) -> bool {
        self.read_text(rel).trim() == "1"
    }

    pub fn set_flag(&self, rel: &str, on: bool) -> io::Result<()> {
        self.write_text(rel, if on { "1\n" } else { "0\n" })
    }
}

/// Запись через временный файл и rename: оборванная запись не оставляет
/// полупустой конфиг (на Windows `rename` заменяет существующий файл).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp~");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
