//! Торренты на профиле, где они запрещены: блокировать, пускать напрямую или
//! через профиль, где они разрешены.
//!
//! Роутер узнаёт торрент-клиента по пакетам (DHT, uTP, трекеры) и полчаса
//! уводит его «не-веб» соединения. На устройстве проще и точнее: sing-box знает
//! процесс каждого соединения, поэтому весь трафик торрент-клиента уходит по
//! имени процесса (на Android — пакета), включая зашифрованный обмен с пирами.
//! Снифер `bittorrent` остаётся запасным путём для клиентов не из списка.

use crate::chains::ChainStore;
use crate::lists;
use crate::profiles;
use crate::store::{self, Store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Block,
    Direct,
    Via(String),
}

impl Action {
    pub fn mode(&self) -> &'static str {
        match self {
            Action::Block => "block",
            Action::Direct => "direct",
            Action::Via(_) => "via",
        }
    }

    pub fn via(&self) -> &str {
        match self {
            Action::Via(id) => id,
            _ => "",
        }
    }

    pub fn to_line(&self) -> String {
        match self {
            Action::Via(id) => format!("via {id}\n"),
            a => format!("{}\n", a.mode()),
        }
    }
}

pub fn parse(text: &str) -> Action {
    let line = text.lines().next().unwrap_or("").trim();
    match line {
        "direct" => Action::Direct,
        l if l.starts_with("via ") => {
            let id: String = l[4..].chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if id.is_empty() {
                Action::Block
            } else {
                Action::Via(id)
            }
        }
        _ => Action::Block,
    }
}

pub fn load(store: &Store) -> Action {
    parse(&store.read_text(store::TORRENT_ACTION))
}

/// Хопы цели: профиль — сам, именованная цепочка — её хопы.
pub fn target_hops(store: &Store, id: &str) -> Vec<String> {
    if profiles::exists(store, id) {
        return vec![id.to_owned()];
    }
    ChainStore::load(store).get(id).map(|c| c.hops.clone()).unwrap_or_default()
}

/// Торренты разрешены на ВСЕХ хопах цели.
pub fn target_allowed(store: &Store, id: &str) -> bool {
    let allow = lists::parse_id_list(&store.read_text(store::TORRENT_ALLOW));
    let hops = target_hops(store, id);
    !hops.is_empty() && hops.iter().all(|h| allow.contains(h))
}

/// Исполняемые файлы торрент-клиентов (как их видит `process_name`).
#[cfg(target_os = "windows")]
pub const APPS: &[&str] = &[
    "qbittorrent.exe", "uTorrent.exe", "utweb.exe", "BitTorrent.exe", "transmission-qt.exe",
    "transmission-daemon.exe", "deluge.exe", "deluged.exe", "tixati.exe", "BitComet.exe",
    "aria2c.exe", "Vuze.exe", "Free Download Manager.exe", "fdm.exe", "PicoTorrent.exe", "biglybt.exe",
];
#[cfg(target_os = "macos")]
pub const APPS: &[&str] = &[
    "qbittorrent", "qBittorrent", "Transmission", "transmission-daemon", "uTorrent", "uTorrent Web",
    "BitTorrent", "Deluge", "deluged", "Tixati", "aria2c", "Folx", "BiglyBT", "Motrix",
];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const APPS: &[&str] = &["qbittorrent", "qbittorrent-nox", "transmission-daemon", "deluged", "aria2c"];

/// Android-пакеты торрент-клиентов (`package_name`).
pub const PACKAGES: &[&str] = &[
    "com.utorrent.client", "com.bittorrent.client", "org.proninyaroslav.libretorrent",
    "com.delphicoder.flud", "com.delphicoder.flud.paid", "hu.tagsoft.ttorrent.lite", "hu.tagsoft.ttorrent.pro",
    "com.vuze.android.remote", "idm.internet.download.manager", "com.biglybt.android.client",
    "intelligems.torrdroid", "co.we.torrent", "com.frostwire.android",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines() {
        assert_eq!(parse(""), Action::Block);
        assert_eq!(parse("direct\n"), Action::Direct);
        assert_eq!(parse("via my_profile-1\n"), Action::Via("my_profile-1".into()));
        assert_eq!(parse("via ;rm -rf\n"), Action::Via("rm-rf".into()));
        assert_eq!(parse("via \n"), Action::Block);
        assert_eq!(parse("garbage"), Action::Block);
        assert_eq!(Action::Via("x".into()).to_line(), "via x\n");
    }
}
