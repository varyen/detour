//! Защита от утечки и сторож движка.
//!
//! Прототип в Sandbox показал: если sing-box падает, TUN исчезает, маршруты
//! возвращаются и трафик спокойно идёт мимо VPN. На роутере такого нет —
//! там правила файрвола живут отдельно от движка и в этот момент рубят выход.
//!
//! Здесь ту же роль играет правило файрвола, включённое ровно на время, пока
//! VPN должен работать, но не работает: на Windows — правило брандмауэра, на
//! macOS — якорь pf. Пока туннель поднят, правило снято: трафик и так уходит
//! только в TUN. Правило снимается при остановке службы и при старте — иначе
//! упавшая служба оставила бы машину без сети.

use std::process::Stdio;

use serde_json::json;

use super::{Backend, Start};
use crate::ipc::Response;
use crate::settings::Settings;
use crate::store;

pub const RULE: &str = "Detour kill-switch";
/// Якорь pf на macOS. Ссылка на него добавляется в `/etc/pf.conf` при
/// установке демона, сюда же пишутся правила на время простоя.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const PF_ANCHOR: &str = "detour";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const PF_ANCHOR_FILE: &str = "/etc/pf.anchors/detour";

/// Всё наружу закрыто, локальные сети и loopback оставлены: иначе отвалились
/// бы и панель, и сам туннель, который поднимается через них.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn pf_rules() -> String {
    "table <detour_local> const { 127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 224.0.0.0/4 }
     block drop out quick inet from any to !<detour_local>
"
        .to_owned()
}

/// `netsh` вместо WFP напрямую: правило видно человеку в брандмауэре и
/// снимается той же командой вручную, если служба умерла совсем.
#[cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
fn netsh(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("netsh")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn netsh(_args: &[&str]) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn pfctl(args: &[&str]) -> bool {
    std::process::Command::new("/sbin/pfctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Backend {
    /// Включить или снять блокировку выхода в сеть. В режиме разработки
    /// (`--dev-http`) не трогаем брандмауэр рабочей машины вовсе.
    pub(super) fn killswitch(&self, on: bool) {
        if self.dev_mode() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if on {
                let _ = std::fs::create_dir_all("/etc/pf.anchors");
                let _ = std::fs::write(PF_ANCHOR_FILE, pf_rules());
                pfctl(&["-E"]);
                pfctl(&["-a", PF_ANCHOR, "-f", PF_ANCHOR_FILE]);
            } else {
                pfctl(&["-a", PF_ANCHOR, "-F", "rules"]);
            }
            return;
        }
        netsh(&["advfirewall", "firewall", "delete", "rule", &format!("name={RULE}")]);
        if on {
            netsh(&[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={RULE}"),
                "dir=out",
                "action=block",
                "enable=yes",
                "profile=any",
                "remoteip=0.0.0.0-9.255.255.255,11.0.0.0-126.255.255.255,128.0.0.0-172.15.255.255,172.32.0.0-192.167.255.255,192.169.0.0-255.255.255.255",
            ]);
        }
    }

    pub(super) fn killswitch_enabled(&self) -> bool {
        Settings::load(&self.store).flag("killswitch", false)
    }

    /// Намерение «VPN должен работать»: переживает падение службы, поэтому
    /// сторож поднимает движок и после аварийного перезапуска.
    pub(super) fn set_want_vpn(&self, want: bool) {
        let _ = self.store.set_flag(store::WANT_SINGBOX, want);
    }

    pub(super) fn want_vpn(&self) -> bool {
        self.store.flag(store::WANT_SINGBOX)
    }

    /// Раз в несколько секунд: движок должен работать, но не работает —
    /// поднять, а на время простоя закрыть выход, если включён kill-switch.
    pub(super) async fn guard_tick(&self) {
        if self.dev_mode() {
            return;
        }
        if !self.want_vpn() || Settings::load(&self.store).active_chain().is_empty() {
            self.killswitch(false);
            return;
        }
        if self.engine.pid().await.is_some() {
            return;
        }
        if self.killswitch_enabled() {
            self.killswitch(true);
        }
        tracing::warn!("sing-box не работает, хотя VPN включён — поднимаю");
        match self.apply(None, Start::Always).await {
            Ok(()) => {
                self.killswitch(false);
                tracing::info!("sing-box поднят сторожем");
            }
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "сторож не смог поднять sing-box"),
        }
    }

    pub(super) fn killswitch_status(&self) -> Response {
        Response::json(&json!({
            "ok": true,
            "supported": cfg!(windows) || cfg!(target_os = "macos"),
            "enabled": self.killswitch_enabled(),
            "rule": RULE,
        }))
    }

    pub(super) fn killswitch_set(&self, on: bool) -> anyhow::Result<Response> {
        let mut s = Settings::load(&self.store);
        s.set("killswitch", if on { "1" } else { "0" });
        s.save(&self.store)?;
        if !on {
            self.killswitch(false);
        }
        Ok(Response::json(&json!({ "ok": true, "enabled": on })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pf_rules_keep_local_networks() {
        let r = pf_rules();
        assert!(r.contains("block drop out quick inet from any to !<detour_local>"));
        for net in ["127.0.0.0/8", "192.168.0.0/16", "10.0.0.0/8"] {
            assert!(r.contains(net), "в таблице нет {net}");
        }
    }
}
