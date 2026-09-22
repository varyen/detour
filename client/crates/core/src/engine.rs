//! sing-box дочерним процессом (десктоп). На Android движок будет другим —
//! libbox внутри VpnService, — с тем же набором методов.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[cfg(windows)]
pub const EXE: &str = "sing-box.exe";
#[cfg(not(windows))]
pub const EXE: &str = "sing-box";

pub struct Engine {
    /// Скачанное обновлением — побеждает поставленное установщиком.
    local: PathBuf,
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
        Self {
            local: data.join("bin").join(EXE),
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
        self.binary().is_file()
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
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if let Ok(Some(status)) = child.try_wait() {
            bail!("sing-box завершился сразу ({status}): {}", self.stderr_tail(8));
        }
        *self.child.lock().await = Some(child);
        Ok(pid)
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

    pub async fn stop(&self) {
        if let Some(mut c) = self.child.lock().await.take() {
            let _ = c.kill().await;
        }
    }

    pub async fn pid(&self) -> Option<u32> {
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
