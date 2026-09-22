mod bypass;
mod config;
mod guard;
mod health;
mod maint;
mod profiles;
mod rules;
mod service;
mod stats;
mod subs;

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::dpi::Dpi;
use crate::engine::Engine;
use crate::ipc::{Request, Response};
use crate::lists;
use crate::render::{self, Params};
use crate::settings::Settings;
use crate::store::{self, Store};

#[cfg(target_os = "windows")]
pub const PLATFORM: &str = "windows";
#[cfg(target_os = "macos")]
pub const PLATFORM: &str = "macos";
#[cfg(target_os = "android")]
pub const PLATFORM: &str = "android";
#[cfg(target_os = "ios")]
pub const PLATFORM: &str = "ios";
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android", target_os = "ios")))]
pub const PLATFORM: &str = "linux";

const CLASH_PORT: u16 = 19090;
const CLASH_SECRET: &str = "run/clash.secret";

/// Запускать ли движок после пересборки.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Start {
    /// Перезапустить, только если уже работает: правка списков не должна
    /// включать VPN, который человек выключил.
    IfRunning,
    /// Выбор профиля или «Старт» — включить в любом случае.
    Always,
}

/// Долгие операции, на которые панель отвечает `started` и дальше опрашивает.
enum Job {
    HealthSweep,
    BinsApply,
    DpiApply,
}

pub struct Backend {
    store: Store,
    engine: Engine,
    dpi: Dpi,
    apply_lock: Mutex<()>,
    /// Одно обновление подписок за раз: плановое и ручное не должны
    /// одновременно переписывать одни и те же профили.
    subs_lock: Mutex<()>,
    /// Чтение-изменение-запись баз проверок и пингов.
    health_lock: Mutex<()>,
    /// Один плановый/полный проход проверки за раз.
    sweep_lock: Mutex<()>,
    /// Адрес физического интерфейса до подъёма TUN: с него ходят пинги.
    phys_src: std::sync::Mutex<Option<Ipv4Addr>>,
    jobs: mpsc::UnboundedSender<Job>,
    job_rx: std::sync::Mutex<Option<mpsc::UnboundedReceiver<Job>>>,
    meter: std::sync::Mutex<crate::traffic::Meter>,
    geo_lock: Mutex<()>,
    clash_secret: String,
    /// Режим разработки: TUN перехватил бы трафик всей машины разработчика.
    forbid_tun: std::sync::atomic::AtomicBool,
    /// Работает ли winws2: от этого зависит, идут ли домены DPI мимо VPN.
    dpi_on: std::sync::atomic::AtomicBool,
}

impl Backend {
    pub fn new(data: PathBuf) -> Result<Self> {
        // sing-box живёт со своим рабочим каталогом: относительные пути в
        // его аргументах и в конфиге указали бы не туда.
        let data = std::path::absolute(&data)?;
        for dir in ["profiles", "lists", "flags", "run", "logs", "bin"] {
            std::fs::create_dir_all(data.join(dir))?;
        }
        let store = Store::new(data.clone());
        if !store.exists(store::UDP_VPN) {
            store.write_text(store::UDP_VPN, store::UDP_VPN_SEED)?;
        }
        let clash_secret = match store.read_text(CLASH_SECRET).trim() {
            s if s.len() >= 32 => s.to_owned(),
            _ => {
                let mut b = [0u8; 16];
                getrandom::fill(&mut b).map_err(|e| anyhow!("getrandom: {e}"))?;
                let s: String = b.iter().map(|x| format!("{x:02x}")).collect();
                store.write_text(CLASH_SECRET, &s)?;
                s
            }
        };
        let engine = Engine::new(&data, data.join("run"), data.join(store::SINGBOX_STDERR));
        let dpi = Dpi::new(&data, data.join("logs/dpi.log"));
        // Старый бинарник, отодвинутый обновлением, пока служба работала на нём.
        let _ = std::fs::remove_file(engine.local_binary().with_extension("old"));
        let (jobs, job_rx) = mpsc::unbounded_channel();
        Ok(Self {
            store,
            engine,
            dpi,
            apply_lock: Mutex::new(()),
            subs_lock: Mutex::new(()),
            health_lock: Mutex::new(()),
            sweep_lock: Mutex::new(()),
            phys_src: std::sync::Mutex::new(crate::ping::physical_ipv4()),
            jobs,
            job_rx: std::sync::Mutex::new(Some(job_rx)),
            meter: std::sync::Mutex::new(crate::traffic::Meter::default()),
            geo_lock: Mutex::new(()),
            clash_secret,
            forbid_tun: std::sync::atomic::AtomicBool::new(false),
            dpi_on: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Запрет поднимать TUN (служба запущена с `--dev-http`). Конфиг с TUN
    /// по-прежнему собирается и проходит `sing-box check`, не стартует только
    /// сам движок. `DETOUR_ALLOW_TUN=1` снимает запрет — для стенда в ВМ.
    pub fn forbid_tun(&self) {
        self.forbid_tun.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn tun_enabled(&self) -> bool {
        std::env::var_os("DETOUR_DEV_NO_TUN").is_none()
    }

    /// Служба запущена для разработки (`--dev-http`): ни TUN, ни правил
    /// брандмауэра, ни сторожа — иначе стенд ломал бы рабочую машину.
    pub(super) fn dev_mode(&self) -> bool {
        self.forbid_tun.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_dpi_on(&self, on: bool) {
        self.dpi_on.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn data_dir(&self) -> &Path {
        self.store.root()
    }

    /// Старт службы: поднять VPN, если включён автозапуск и есть что поднимать.
    pub async fn boot(&self) {
        self.dpi_boot().await;
        if !self.store.flag(store::AUTOSTART_SINGBOX) {
            return;
        }
        if Settings::load(&self.store).active_chain().is_empty() {
            return;
        }
        if let Err(e) = self.apply(None, Start::Always).await {
            tracing::warn!(error = %format!("{e:#}"), "автозапуск sing-box не удался");
        }
    }

    pub async fn shutdown(&self) {
        self.engine.stop().await;
        self.dpi.stop().await;
        self.killswitch(false);
    }

    pub async fn handle(&self, req: Request) -> Response {
        match self.dispatch(&req).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::warn!(action = %req.action, error = %msg, "action failed");
                Response::error(&msg)
            }
        }
    }

    async fn dispatch(&self, req: &Request) -> Result<Response> {
        let body = || -> Result<String> {
            req.body.as_ref().map(|b| b.text()).transpose().map(Option::unwrap_or_default)
        };
        let ok = || Response::json(&json!({ "ok": true }));
        Ok(match req.action.as_str() {
            "check_auth" => Response::json(&json!({ "ok": true, "user": "" })),
            "panel_setup_status" => Response::json(&json!({ "setup_required": false })),
            "status" => self.status().await,

            "settings" => self.settings(req, body()?).await?,
            "profiles_list" => self.profiles_list(),
            "profile_get" => self.profile_get(req)?,
            "profile_save" => {
                self.profile_save(body()?).await?;
                ok()
            }
            "profile_delete" => {
                self.profile_delete(req)?;
                ok()
            }
            "profile_activate" => {
                self.profile_activate(req).await?;
                ok()
            }
            "profiles_export" => self.profiles_export(),
            "chains_list" => Response::json(&crate::chains::ChainStore::load(&self.store)),
            "chain_save" => {
                self.chain_save(body()?).await?;
                ok()
            }
            "chain_delete" => {
                self.chain_delete(body()?)?;
                ok()
            }
            "chain_activate" => {
                self.chain_activate(body()?).await?;
                ok()
            }
            "autoswitch_set" => self.id_flag(req, store::AUTOSWITCH_EXCLUDE, "eligible", false, "excluded", body()?)?,
            "speedcheck_set" => self.id_flag(req, store::SPEEDCHECK_EXCLUDE, "eligible", false, "excluded", body()?)?,
            "torrent_set" => self.torrent_set(req, body()?).await?,
            "torrent_status" => self.torrent_status().await,

            "route_map" => self.text_file(req, store::ROUTE_MAP, "routemap", body()?, true).await?,
            "domains" => self.text_file(req, store::PROXY_DOMAINS, "domains", body()?, false).await?,
            "domains_save_restart" => self.text_file(req, store::PROXY_DOMAINS, "domains", body()?, true).await?,
            "whitelist" => self.text_file(req, store::WHITELIST, "whitelist", body()?, false).await?,
            "whitelist_save_restart" => self.text_file(req, store::WHITELIST, "whitelist", body()?, true).await?,
            "zapret_domains" => self.dpi_domains(req, body()?, false).await?,
            "zapret_domains_save_restart" => self.dpi_domains(req, body()?, true).await?,
            "egress_blocklist" => self.egress_blocklist(req, body()?).await?,
            "udp_vpn" => self.udp_vpn(req, body()?).await?,
            "udp_vpn_list" => self.udp_vpn_list(req, body()?).await?,
            "subscriptions_list" => self.subscriptions_list(),
            "subscription_save_one" => {
                self.subscription_save(body()?)?;
                ok()
            }
            "subscription_delete_one" => {
                self.subscription_delete(req)?;
                ok()
            }
            "subscription_fetch" => self.subscription_fetch(body()?).await?,
            "subscription_refresh_one" => self.subscription_refresh_one(req).await,
            "subscription_refresh_all" => self.subscription_refresh_all().await,
            "panel_export_config" => self.export_config(),
            "panel_import_config" => self.import_config(body()?).await?,

            "rulist_status" => self.rulist_status(),
            "rulist_set" => self.rulist_set(body()?).await?,
            "rulist_update" => self.rulist_update(body()?).await?,
            "rulist_exclude" => self.rulist_exclude(req, body()?).await?,
            "self_intercept" => Response::json(&json!({ "targets": [], "full_targets": [], "eligible": [], "mode": "single" })),

            "singbox_config" => self.singbox_config(req)?,
            "singbox_start" | "singbox_restart" => {
                self.apply(None, Start::Always).await?;
                ok()
            }
            "singbox_stop" => {
                self.set_want_vpn(false);
                self.killswitch(false);
                self.engine.stop().await;
                ok()
            }
            "singbox_enable" | "singbox_disable" => {
                self.store.set_flag(store::AUTOSTART_SINGBOX, req.action == "singbox_enable")?;
                ok()
            }
            "allvpn_on" | "allvpn_off" => {
                self.allvpn(req.action == "allvpn_on").await?;
                ok()
            }
            "bypass_status" => self.bypass_status().await,
            "bypass_set" => self.bypass_set(req).await?,
            "bypass_stop" => self.bypass_stop().await?,
            "bypass_autostart" => self.bypass_autostart(req)?,
            "bypass_strategy" => {
                let text = req.body.as_ref().map(|b| b.text()).transpose()?;
                self.bypass_strategy(req, text).await?
            }
            "zapret_start" | "zapret_stop" | "zapret_restart" | "zapret_enable" | "zapret_disable" => {
                self.zapret_service(&req.action).await?
            }
            "zapret_config" => Response::error("not_supported"),
            "killswitch_status" => self.killswitch_status(),
            "killswitch_set" => self.killswitch_set(req.param("on").unwrap_or("") != "0")?,

            "logs_view" => self.logs_view(req)?,
            "logs_clear" => {
                self.logs_clear(req)?;
                ok()
            }
            "apply_log" => self.apply_log(),
            "bins_update_check" => self.bins_check().await,
            "bins_update_status" => self.bins_status(),
            "bins_update_apply" | "singbox_opkg_upgrade" => self.bins_apply(),
            // Канал у движка один, но панель зовёт его по имени платформы.
            "nfqws2_update_check" | "tpws_update_check" => self.dpi_bins_check().await,
            "nfqws2_update_status" | "tpws_update_status" => self.dpi_bins_status(),
            "nfqws2_update_apply" | "tpws_update_apply" => self.dpi_bins_apply(),
            "updates_overview" => self.updates_overview().await,
            "autocheck_status" => self.autocheck_status(),
            "autocheck_set" => self.autocheck_set(body()?)?,

            "health_status" => self.health_status(),
            "health_check" => self.health_check(req).await?,
            "health_config" => self.health_config(body()?)?,
            "health_urls" => self.health_urls(req, body()?)?,
            "ping_status" => self.ping_status(),
            "ping_check" => self.ping_check(req).await?,
            "traffic_counters" => self.traffic_counters().await,
            "traffic_series" => self.traffic_series(req),
            "geo_status" => self.geo_status(),
            "geo_scan" => self.geo_scan(body()?).await?,
            "keepalive_status" => self.keepalive_status(),
            "keepalive_check" => self.keepalive_check().await,
            "offload_status" | "swap_status" | "warp_status" => Response::json(&json!({ "supported": false })),

            _ => Response::error("not_supported"),
        })
    }

    /// Пересборка конфига и (пере)запуск движка. Конфиг сначала собирается во
    /// временный каталог и проходит `sing-box check`; живой конфиг и rule-set
    /// файлы меняются только после этого, а `settings` пишутся последними —
    /// как `build_config` → `write_settings` на роутере.
    async fn apply(&self, chain: Option<Vec<String>>, start: Start) -> Result<()> {
        let _guard = self.apply_lock.lock().await;
        let mut settings = Settings::load(&self.store);
        if let Some(c) = &chain {
            settings.set_active_chain(c);
            if let Some(mode) = self.exit_routing_mode(c) {
                settings.set("routing_mode", mode);
            }
        }
        let hops = settings.active_chain();
        if hops.is_empty() {
            bail!("no active profile");
        }

        let log = self.store.path(store::SINGBOX_LOG);
        let staging = self.store.path(store::STAGING);
        let _ = std::fs::remove_dir_all(&staging);
        let staged = self.render_to(&settings, &hops, &log, &staging.join("rules"))?;
        let staged_cfg = staging.join("config.json");
        self.write_rendered(&staged, &staged_cfg)?;
        self.engine
            .check(&staged_cfg)
            .await
            .map_err(|e| anyhow!("failed to render config: {e}"))?;

        let rules_dir = self.store.path(store::RULESETS);
        let live = self.render_to(&settings, &hops, &log, &rules_dir)?;
        self.write_rendered(&live, &self.store.path(store::CONFIG))?;
        prune(&rules_dir, &live);
        settings.save(&self.store)?;
        let _ = std::fs::remove_dir_all(&staging);

        let running = self.engine.pid().await.is_some();
        if start == Start::Always || running {
            if !running {
                // Пока TUN не поднят, система ещё ходит наружу с физического
                // адреса — запоминаем его для пингов мимо туннеля.
                if let Some(a) = crate::ping::physical_ipv4() {
                    *self.phys_src.lock().expect("phys_src") = Some(a);
                }
            }
            if self.tun_enabled()
                && self.forbid_tun.load(std::sync::atomic::Ordering::Relaxed)
                && std::env::var_os("DETOUR_ALLOW_TUN").is_none()
            {
                bail!("TUN в режиме разработки не поднимается: DETOUR_DEV_NO_TUN=1 даёт вход dev-in, DETOUR_ALLOW_TUN=1 — только на стенде");
            }
            // Намерение ставим до старта: на Android первый запуск обычно
            // упирается в диалог разрешения, и поднять туннель должен сторож,
            // когда человек согласится.
            self.set_want_vpn(true);
            self.engine.start(&self.store.path(store::CONFIG)).await?;
        }
        Ok(())
    }

    fn render_to(&self, settings: &Settings, hops: &[String], log: &Path, rules: &Path) -> Result<render::Rendered> {
        render::render(
            &self.store,
            settings,
            &Params {
                chain: hops,
                log_path: log,
                ruleset_dir: rules,
                clash_port: CLASH_PORT,
                clash_secret: &self.clash_secret,
                dpi: if self.dpi_on.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::dpi::route()
                } else {
                    render::DpiRoute::Off
                },
                tun: self.tun_enabled(),
            },
        )
    }

    fn write_rendered(&self, r: &render::Rendered, config: &Path) -> Result<()> {
        for (path, content) in &r.rulesets {
            store::write_atomic(path, &serde_json::to_vec(content)?)?;
        }
        store::write_atomic(config, &serde_json::to_vec_pretty(&r.config)?)?;
        Ok(())
    }

    /// Профиль-выход может нести свой режим маршрутизации — при активации он
    /// становится общим, как на роутере.
    fn exit_routing_mode(&self, chain: &[String]) -> Option<String> {
        let p = crate::profiles::load(&self.store, chain.last()?)?;
        let m = p.get("routing_mode")?.as_str()?;
        crate::settings::RoutingMode::parse(m).map(|m| m.as_str().to_owned())
    }

    async fn status(&self) -> Response {
        let settings = Settings::load(&self.store);
        let chain = settings.active_chain();
        let pid = self.engine.pid().await;
        let dpi_pid = self.dpi.pid().await;
        let dpi_version = self.dpi.version().await;
        let proxied = lists::parse_list(&self.store.read_text(store::PROXY_DOMAINS));
        let dpi = lists::parse_list(&self.store.read_text(store::DPI_DOMAINS));
        let version = self.engine.version().await.unwrap_or_else(|| "?".into());
        let active = chain.last().cloned().unwrap_or_default();
        let active_type = crate::profiles::load(&self.store, &active)
            .map(|p| crate::profiles::profile_type(&p))
            .unwrap_or_default();
        let targets = lists::parse_route_map(&self.store.read_text(store::ROUTE_MAP)).len();
        let none = || Value::String("null".into());

        Response::json(&json!({
            "platform": PLATFORM,
            "version": crate::VERSION,
            "binaries": {
                "singbox_present": self.engine.present(),
                "singbox_version": version,
                "bins_version": version,
                "tpws_present": !cfg!(windows) && self.dpi.present(),
                "tpws_version": if cfg!(windows) { None } else { dpi_version.clone() },
                "nfqws2_present": cfg!(windows) && self.dpi.present(),
                "nfqws2_supported": cfg!(windows) && self.dpi.supported(),
                "nfqws2_version": if cfg!(windows) { dpi_version } else { None },
            },
            "singbox": {
                "running": pid.is_some(),
                "pid": pid.map(|p| Value::String(p.to_string())).unwrap_or_else(none),
                "port": none(),
                "enabled": self.store.flag(store::AUTOSTART_SINGBOX),
                "allvpn": settings.allvpn(),
                "domains": proxied.domains.len(),
                "ips": proxied.cidrs.len(),
                "entries": proxied.domains.len() + proxied.cidrs.len(),
                "ipset_count": 0,
                "ipset_members": 0,
                "active_profile": active,
                "active_type": active_type,
                "external_ip": "",
                "external_ip_checked": 0,
                "external_ip_refreshing": false,
                "active_chain": chain,
                "routing_mode": settings.routing_mode().as_str(),
                "singbox_mode": "single",
                "route_targets": targets,
                "self_intercept": [],
            },
            "zapret": {
                "running": dpi_pid.is_some(),
                "pid": dpi_pid.map(|p| Value::String(p.to_string())).unwrap_or_else(none),
                "port": none(),
                "enabled": self.store.flag(store::AUTOSTART_DPI),
                "domains": dpi.domains.len(),
                "ips": dpi.cidrs.len(),
                "ipset_count": 0,
                "args": self.dpi.strategy(&self.store),
            },
            "system": crate::sysinfo::system(self.store.root()),
            "wan_link": { "ok": true, "supported": false, "degraded": false, "diagnosis": "", "advice": "" },
        }))
    }
}

/// Работающий sing-box следит за rule-set файлами, поэтому каталог не
/// пересоздаётся целиком: новые файлы уже записаны, убираются только лишние.
fn prune(dir: &Path, r: &render::Rendered) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !r.rulesets.iter().any(|(keep, _)| *keep == p) {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn parse_body(body: &str) -> Result<Value> {
    serde_json::from_str(body).map_err(|_| anyhow!("invalid json"))
}
