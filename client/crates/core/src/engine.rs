//! sing-box дочерним процессом (десктоп). На телефонах дочерний процесс не
//! годится: TUN открывает только сама система, — поэтому конфиг поднимает
//! libbox: на Android в процессе приложения (VpnService), на iOS в отдельном
//! процессе-расширении (NEPacketTunnelProvider). Ядро дёргает его через мост
//! `tunnel`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[cfg(windows)]
pub const EXE: &str = "sing-box.exe";
#[cfg(not(windows))]
pub const EXE: &str = "sing-box";

/// Мост в платформенный туннель (Android, iOS): ставится приложением при старте.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod tunnel {
    use std::path::Path;
    use std::sync::OnceLock;

    pub trait Tunnel: Send + Sync {
        /// Поднять туннель по готовому конфигу; возвращает условный pid.
        fn start(&self, config: &Path) -> anyhow::Result<u32>;
        fn stop(&self);
        fn running(&self) -> bool;
        /// Разобрать конфиг, не трогая живой туннель.
        fn check(&self, config: &Path) -> Result<(), String>;
        /// Версия ядра, вкомпилированного в приложение.
        fn version(&self) -> Option<String>;
    }

    static TUNNEL: OnceLock<Box<dyn Tunnel>> = OnceLock::new();

    pub fn set(t: Box<dyn Tunnel>) {
        let _ = TUNNEL.set(t);
    }

    pub fn get() -> Option<&'static dyn Tunnel> {
        TUNNEL.get().map(AsRef::as_ref)
    }
}

/// Движок mihomo целиком (не сайдкар) — только там, где процессы можно
/// запускать самим: на телефонах sing-box вкомпилирован библиотекой.
pub const MIHOMO_ENGINE_SUPPORTED: bool = !cfg!(any(target_os = "android", target_os = "ios"));

pub struct Engine {
    /// Скачанное обновлением — побеждает поставленное установщиком.
    local: PathBuf,
    /// mihomo для режима движка «mihomo» — тот же бинарник, что у сайдкара AWG.
    mihomo: PathBuf,
    bundled: Option<PathBuf>,
    workdir: PathBuf,
    stderr_log: PathBuf,
    child: Mutex<Option<Child>>,
    version: Mutex<Option<(PathBuf, SystemTime, String)>>,
}

impl Engine {
    pub fn new(data: &Path, workdir: PathBuf, stderr_log: PathBuf) -> Self {
        let bundled = match std::env::var_os("DETOUR_SINGBOX") {
            Some(p) => Some(PathBuf::from(p)),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join(EXE)))
                .filter(|p| p.exists()),
        };
        let mihomo = crate::awg::Sidecar::new(data, workdir.clone(), stderr_log.clone()).binary();
        Self {
            local: data.join("bin").join(EXE),
            mihomo,
            bundled,
            workdir,
            stderr_log,
            child: Mutex::new(None),
            version: Mutex::new(None),
        }
    }

    pub fn binary(&self) -> PathBuf {
        if self.local.is_file() {
            return self.local.clone();
        }
        self.bundled.clone().unwrap_or_else(|| self.local.clone())
    }

    /// Куда кладёт бинарник обновление.
    pub fn local_binary(&self) -> &Path {
        &self.local
    }

    pub fn present(&self) -> bool {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            return tunnel::get().is_some();
        }
        #[allow(unreachable_code)]
        self.binary().is_file()
    }

    /// Пробы профилей поднимают второй экземпляр движка рядом с рабочим. На
    /// Android такого нет: ядро вкомпилировано в приложение в единственном
    /// числе, и проверять профили нечем.
    pub fn probes_supported(&self) -> bool {
        !cfg!(any(target_os = "android", target_os = "ios")) && self.present()
    }

    fn command_for(&self, bin: &Path, dir: &Path) -> Command {
        let mut c = Command::new(bin);
        c.current_dir(dir).stdin(Stdio::null());
        #[cfg(windows)]
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }

    fn command(&self) -> Command {
        self.command_for(&self.binary(), &self.workdir)
    }

    pub async fn version(&self) -> Option<String> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            return tunnel::get().and_then(|t| t.version());
        }
        #[allow(unreachable_code)]
        let bin = self.binary();
        let mtime = std::fs::metadata(&bin).and_then(|m| m.modified()).ok()?;
        let mut cache = self.version.lock().await;
        if let Some((p, t, v)) = cache.as_ref() {
            if *p == bin && *t == mtime {
                return Some(v.clone());
            }
        }
        let v = version_of(&bin).await?;
        *cache = Some((bin, mtime, v.clone()));
        Some(v)
    }

    /// `sing-box check` — живой конфиг не трогаем, пока новый не прошёл.
    pub async fn check(&self, config: &Path) -> Result<(), String> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let Some(t) = tunnel::get() else {
                return Err("туннель ещё не подключён приложением".to_owned());
            };
            return t.check(config);
        }
        #[allow(unreachable_code)]
        if !self.present() {
            return Err(format!("sing-box не найден: {}", self.binary().display()));
        }
        let out = self
            .command()
            .arg("check")
            .arg("-c")
            .arg(config)
            .arg("-D")
            .arg(&self.workdir)
            .arg("--disable-color")
            .output()
            .await
            .map_err(|e| format!("не удалось запустить sing-box: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let mut msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        if msg.is_empty() {
            msg = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        }
        Err(msg)
    }

    pub async fn start(&self, config: &Path) -> Result<u32> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let Some(t) = tunnel::get() else {
                bail!("туннель ещё не подключён приложением");
            };
            return t.start(config);
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
        self.stop().await;
        if !self.present() {
            bail!("sing-box не найден: {}", self.binary().display());
        }
        if let Some(dir) = self.stderr_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.stderr_log)?;
        let mut child = self
            .command()
            .arg("run")
            .arg("-c")
            .arg(config)
            .arg("-D")
            .arg(&self.workdir)
            .arg("--disable-color")
            .stdout(Stdio::null())
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить sing-box")?;
        let pid = child.id().unwrap_or(0);

        // TUN и маршруты поднимаются за доли секунды; если процесс умер сразу
        // — это ошибка конфига или прав, и причина лежит в хвосте stderr.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        if let Ok(Some(status)) = child.try_wait() {
            bail!("sing-box завершился сразу ({status}): {}", self.stderr_tail(8));
        }
        *self.child.lock().await = Some(child);
        Ok(pid)
        }
    }

    pub fn mihomo_binary(&self) -> &Path {
        &self.mihomo
    }

    /// Режим движка «mihomo»: тот же слот процесса, что у sing-box, поэтому
    /// pid()/stop() и всё, что на них опирается (сторож, счётчики), работают
    /// одинаково для обоих движков.
    pub async fn start_mihomo(&self, config: &Path) -> Result<u32> {
        if !MIHOMO_ENGINE_SUPPORTED {
            bail!("движок mihomo на этой платформе недоступен");
        }
        self.stop().await;
        if !self.mihomo.is_file() {
            bail!("mihomo не найден: {}", self.mihomo.display());
        }
        let dir = self.workdir.join("mihomo-engine");
        std::fs::create_dir_all(&dir)?;
        if let Some(d) = self.stderr_log.parent() {
            std::fs::create_dir_all(d)?;
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.stderr_log)?;
        let mut c = self.command_for(&self.mihomo, &dir);
        let mut child = c
            .arg("-d")
            .arg(&dir)
            .arg("-f")
            .arg(config)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить mihomo")?;
        let pid = child.id().unwrap_or(0);
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        if let Ok(Some(status)) = child.try_wait() {
            bail!("mihomo завершился сразу ({status}): {}", self.stderr_tail(8));
        }
        *self.child.lock().await = Some(child);
        Ok(pid)
    }

    /// `mihomo -t` — проверка перевода до того, как трогать живой движок.
    pub async fn check_mihomo(&self, config: &Path) -> Result<(), String> {
        if !self.mihomo.is_file() {
            return Err(format!("mihomo не найден: {}", self.mihomo.display()));
        }
        let dir = self.workdir.join("mihomo-check");
        let _ = std::fs::create_dir_all(&dir);
        let out = self
            .command_for(&self.mihomo, &dir)
            .arg("-t")
            .arg("-d")
            .arg(&dir)
            .arg("-f")
            .arg(config)
            .output()
            .await
            .map_err(|e| format!("не удалось запустить mihomo: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Err(text.lines().filter(|l| l.contains("level=error") || l.contains("failed")).collect::<Vec<_>>().join(" | "))
    }

    /// Отдельный процесс для проб (проверка профилей): свой конфиг, свой
    /// каталог, убивается вместе с дескриптором.
    pub fn spawn_aux(&self, config: &Path, dir: &Path) -> Result<Child> {
        std::fs::create_dir_all(dir)?;
        self.command_for(&self.binary(), dir)
            .arg("run")
            .arg("-c")
            .arg(config)
            .arg("-D")
            .arg(dir)
            .arg("--disable-color")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить sing-box для проверки")
    }

    /// Временный mihomo для проб AmneziaWG — sing-box его не поднимет.
    pub fn spawn_aux_mihomo(&self, config: &Path, dir: &Path) -> Result<Child> {
        if !self.mihomo.is_file() {
            bail!("mihomo не найден: {}", self.mihomo.display());
        }
        std::fs::create_dir_all(dir)?;
        self.command_for(&self.mihomo, dir)
            .arg("-d")
            .arg(dir)
            .arg("-f")
            .arg(config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить mihomo для проверки")
    }

    pub async fn stop(&self) {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        if let Some(t) = tunnel::get() {
            t.stop();
            return;
        }
        if let Some(mut c) = self.child.lock().await.take() {
            let _ = c.kill().await;
        }
    }

    pub async fn pid(&self) -> Option<u32> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            return tunnel::get().filter(|t| t.running()).map(|_| 1);
        }
        #[allow(unreachable_code)]
        let mut guard = self.child.lock().await;
        let child = guard.as_mut()?;
        match child.try_wait() {
            Ok(None) => child.id(),
            _ => {
                *guard = None;
                None
            }
        }
    }

    pub fn stderr_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(&self.stderr_log).unwrap_or_default();
        let v: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        v[v.len().saturating_sub(lines)..].join("\n")
    }
}

/// «sing-box version 1.13.21» → «1.13.21».
pub async fn version_of(bin: &Path) -> Option<String> {
    let mut c = Command::new(bin);
    c.arg("version").stdin(Stdio::null());
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    let out = c.output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.lines().next()?.rsplit(' ').next()?.trim().to_owned()).filter(|v| !v.is_empty())
}
