//! Обход DPI на Windows: winws2 из zapret2 (перехват через WinDivert).
//! Замена роутерной пары tpws/nfqws2 — стратегия и lua-скрипты те же, что в
//! `detour-bypass`, поэтому строки стратегий переносятся с роутера как есть.
//!
//! Поверх TUN есть тонкость: sing-box переоткрывает прямые соединения со
//! своего сокета на физическом интерфейсе, там winws2 и видит ClientHello.
//! Но тот же пакет виден и на стыке с туннелем, поэтому адрес TUN исключается
//! фильтром WinDivert — иначе один пакет обрабатывается дважды.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::lists;
use crate::store::{self, Store};

#[cfg(windows)]
pub const EXE: &str = "winws2.exe";
#[cfg(not(windows))]
pub const EXE: &str = "winws2";

/// Лежат рядом с бинарником; без них стратегии не работают.
pub const LUA: [&str; 3] = ["zapret-lib.lua", "zapret-antidpi.lua", "zapret-auto.lua"];

/// Та же строка, что `NFQWS_STRATEGY_DEFAULT` на роутере.
pub const DEFAULT_STRATEGY: &str =
    "--filter-tcp=443 --filter-l7=tls --payload=tls_client_hello --lua-desync=tcpseg:pos=0,midsld:ip_id=rnd:repeats=2";

const HOSTS: &str = "run/dpi-hosts.txt";
const IPSET: &str = "run/dpi-ips.txt";
const FILTER: &str = "run/dpi-filter.txt";

pub struct Dpi {
    local: PathBuf,
    bundled: Option<PathBuf>,
    log: PathBuf,
    child: Mutex<Option<Child>>,
}

impl Dpi {
    pub fn new(data: &Path, log: PathBuf) -> Self {
        let bundled = match std::env::var_os("DETOUR_WINWS") {
            Some(p) => Some(PathBuf::from(p)),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join(EXE)))
                .filter(|p| p.exists()),
        };
        Self { local: data.join("bin").join(EXE), bundled, log, child: Mutex::new(None) }
    }

    pub fn binary(&self) -> PathBuf {
        if self.local.is_file() {
            return self.local.clone();
        }
        self.bundled.clone().unwrap_or_else(|| self.local.clone())
    }

    pub fn present(&self) -> bool {
        self.binary().is_file()
    }

    fn lua_dir(&self) -> PathBuf {
        self.binary().parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// Движок есть и укомплектован: без lua-скриптов стратегия не соберётся.
    pub fn supported(&self) -> bool {
        cfg!(windows) && self.present() && LUA.iter().all(|f| self.lua_dir().join(f).is_file())
    }

    /// «github version v1.0.5.2 (…)» → «1.0.5.2».
    pub async fn version(&self) -> Option<String> {
        if !self.present() {
            return None;
        }
        let mut c = Command::new(self.binary());
        c.arg("--version").stdin(Stdio::null());
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        let out = c.output().await.ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let word = text.split_whitespace().find(|w| w.starts_with('v') && w[1..].starts_with(|c: char| c.is_ascii_digit()))?;
        Some(word.trim_start_matches('v').to_owned())
    }

    pub fn strategy(&self, store: &Store) -> String {
        match store.read_text(store::DPI_STRATEGY).lines().next().map(str::trim) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => DEFAULT_STRATEGY.to_owned(),
        }
    }

    /// Списки и фильтр для winws2 из общего списка доменов DPI.
    fn prepare(&self, store: &Store, tun: bool) -> Result<Vec<String>> {
        let m = lists::parse_list(&store.read_text(store::DPI_DOMAINS));
        let mut args = vec![
            "--wf-tcp-out=80,443".to_owned(),
            "--wf-udp-out=443".to_owned(),
        ];
        if tun {
            // Пробелы в фильтре не переживают разбор строки, поэтому файлом.
            store.write_text(FILTER, &format!("ip.SrcAddr != {0} and ip.DstAddr != {0}\n", crate::render::TUN_GATEWAY))?;
            args.push(format!("--wf-raw-filter=@{}", store.path(FILTER).display()));
        }
        for f in LUA {
            args.push(format!("--lua-init=@{}", self.lua_dir().join(f).display()));
        }
        if !m.domains.is_empty() {
            store.write_text(HOSTS, &format!("{}\n", m.domains.join("\n")))?;
            args.push(format!("--hostlist={}", store.path(HOSTS).display()));
        }
        if !m.cidrs.is_empty() {
            store.write_text(IPSET, &format!("{}\n", m.cidrs.join("\n")))?;
            args.push(format!("--ipset={}", store.path(IPSET).display()));
        }
        if m.domains.is_empty() && m.cidrs.is_empty() {
            bail!("список доменов для обхода DPI пуст");
        }
        args.extend(self.strategy(store).split_whitespace().map(str::to_owned));
        Ok(args)
    }

    fn command(&self) -> Command {
        let mut c = Command::new(self.binary());
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        c
    }

    /// Проверка стратегии без перехвата: winws2 разбирает аргументы и выходит.
    pub async fn check(&self, store: &Store, strategy: &str) -> Result<(), String> {
        if !strategy.contains("--lua-desync=") {
            return Err("стратегия должна содержать --lua-desync=...".to_owned());
        }
        if !self.supported() {
            return Err("winws2 не установлен".to_owned());
        }
        let mut args = self.prepare(store, false).map_err(|e| e.to_string())?;
        // Хвост — стратегия из файла; подменяем на проверяемую.
        let keep = args.len() - self.strategy(store).split_whitespace().count();
        args.truncate(keep);
        args.extend(strategy.split_whitespace().map(str::to_owned));
        let out = self
            .command()
            .args(&args)
            .arg("--dry-run")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("не удалось запустить winws2: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if msg.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_owned() } else { msg })
    }

    pub async fn start(&self, store: &Store, tun: bool) -> Result<u32> {
        self.stop().await;
        if !self.supported() {
            bail!("winws2 не установлен: {}", self.binary().display());
        }
        let args = self.prepare(store, tun)?;
        if let Some(dir) = self.log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let _ = std::fs::remove_file(&self.log);
        let mut child = self
            .command()
            .args(&args)
            .arg(format!("--debug=@{}", self.log.display()))
            .kill_on_drop(true)
            .spawn()
            .context("не удалось запустить winws2")?;
        let pid = child.id().unwrap_or(0);
        // Драйвер WinDivert поднимается сразу; если фильтр не открылся,
        // процесс умирает за доли секунды, причина — в хвосте лога.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if let Ok(Some(status)) = child.try_wait() {
            bail!("winws2 завершился сразу ({status}): {}", self.log_tail(6));
        }
        *self.child.lock().await = Some(child);
        Ok(pid)
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

    pub fn log_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let v: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        v[v.len().saturating_sub(lines)..].join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_falls_back_to_default() {
        let dir = std::env::temp_dir().join(format!("detour-dpi-{}", store::now_epoch()));
        let s = Store::new(dir.clone());
        std::fs::create_dir_all(dir.join("flags")).unwrap();
        let d = Dpi::new(&dir, dir.join("dpi.log"));
        assert_eq!(d.strategy(&s), DEFAULT_STRATEGY);
        s.write_text(store::DPI_STRATEGY, "--filter-tcp=443 --lua-desync=tcpseg\n# коммент\n").unwrap();
        assert_eq!(d.strategy(&s), "--filter-tcp=443 --lua-desync=tcpseg");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prepare_builds_lists_and_filter() {
        let dir = std::env::temp_dir().join(format!("detour-dpi2-{}", store::now_epoch()));
        let s = Store::new(dir.clone());
        std::fs::create_dir_all(dir.join("run")).unwrap();
        std::fs::create_dir_all(dir.join("lists")).unwrap();
        s.write_text(store::DPI_DOMAINS, "example.com\n1.2.3.0/24\n").unwrap();
        let d = Dpi::new(&dir, dir.join("dpi.log"));
        let args = d.prepare(&s, true).unwrap();
        assert!(args.iter().any(|a| a.starts_with("--hostlist=")));
        assert!(args.iter().any(|a| a.starts_with("--ipset=")));
        assert!(args.iter().any(|a| a.starts_with("--wf-raw-filter=@")));
        assert!(args.contains(&"--lua-desync=tcpseg:pos=0,midsld:ip_id=rnd:repeats=2".to_owned()));
        assert_eq!(s.read_text(HOSTS).trim(), "example.com");
        assert!(s.read_text(FILTER).contains("172.19.0.1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
