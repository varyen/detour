//! Настройки «из коробки». Клиент ставят люди, которые не будут разбираться в
//! режимах: после установки всё должно работать само — туннель для всего,
//! кроме российских адресов и исключений, обход DPI включён, после
//! перезагрузки и VPN, и обход поднимаются сами.
//!
//! Засевается один раз (маркер в `flags/`) и только то, чего человек ещё не
//! трогал: выключенное руками не включается обратно при следующем старте.

use super::bypass::{mode_of, MODE};
use super::Backend;
use crate::rulist;
use crate::settings::Settings;
use crate::store;
use crate::updater;

const SEEDED: &str = "flags/defaults.seeded";

/// На iOS движка обхода нет вовсе.
const DPI_PLATFORM: bool = !cfg!(target_os = "ios");

impl Backend {
    pub(super) fn seed_defaults(&self) {
        if self.store.exists(SEEDED) {
            return;
        }
        let mut s = Settings::load(&self.store);
        if s.get("routing_mode").is_none() {
            s.set("routing_mode", "all-except");
        }
        if DPI_PLATFORM && s.get("bypass_mode").is_none() {
            s.set("bypass_mode", MODE);
        }
        if let Err(e) = s.save(&self.store) {
            tracing::warn!(error = %e, "настройки по умолчанию не записались");
            return;
        }
        for flag in [store::AUTOSTART_SINGBOX, store::AUTOSTART_DPI] {
            if !self.store.exists(flag) {
                let _ = self.store.set_flag(flag, true);
            }
        }
        let _ = self.store.set_flag(SEEDED, true);
        tracing::info!("первый запуск: настройки по умолчанию записаны");
    }

    /// Докачка того, без чего настройки по умолчанию не работают: RU-подсети
    /// (без них «всё, кроме исключений» гонит в туннель и российские сайты) и
    /// движок обхода. Зовётся при старте и из часового обслуживания, так что
    /// неудача (нет сети) повторится сама.
    pub(super) async fn ensure_downloads(&self) {
        let c = rulist::load(&self.store);
        if c.enabled && c.count == 0 {
            let proxy = self.fetch_proxy().await;
            match rulist::update(&self.store, &c.source, proxy.as_deref()).await {
                Ok(_) => {
                    tracing::info!("российские подсети скачаны");
                    self.reapply_quiet().await;
                }
                Err(e) => tracing::warn!(error = %format!("{e:#}"), "российские подсети не скачались"),
            }
        }
        // Архив zapret2 с WinDivert антивирус на машине разработчика считает
        // трояном, поэтому стенд с --dev-http его сам не качает.
        if !crate::dpi::UPDATABLE || self.dev_mode() || self.dpi.present() {
            return;
        }
        if mode_of(&Settings::load(&self.store)) != MODE {
            return;
        }
        let proxy = self.fetch_proxy().await;
        let mut log = |l: String| tracing::info!("{l}");
        if let Err(e) = updater::dpi_install(&self.store, &self.dpi, proxy.as_deref(), &mut log).await {
            tracing::warn!(error = %format!("{e:#}"), "движок обхода не установился");
            return;
        }
        if self.store.flag(store::AUTOSTART_DPI) && self.dpi.pid().await.is_none() {
            self.dpi_boot().await;
            self.sync_dpi_routing().await;
        }
    }
}
