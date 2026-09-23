//! Обновление sing-box из релизов SagerNet на GitHub. Как и фид роутера,
//! держимся major.minor установленной версии: новый minor меняет схему
//! конфига (1.13 → 1.14 убирает старый формат DNS), его ставят только вместе
//! с новой версией Detour. Новый minor показывается как `upstream`.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use crate::engine::{self, Engine};
use crate::store::{self, Store};

pub const STATE: &str = "run/bins.json";
const RELEASES: &str = "https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40";
const MAX_ASSET: usize = 96 << 20;

fn semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|x| x.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

fn arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        "x86" => Some("386"),
        _ => None,
    }
}

pub fn asset_name(version: &str) -> Result<String> {
    if !cfg!(windows) {
        bail!("обновление sing-box из панели пока есть только на Windows");
    }
    let a = arch().ok_or_else(|| anyhow!("неизвестная архитектура"))?;
    Ok(format!("sing-box-{version}-windows-{a}.zip"))
}

async fn get(url: &str, proxy: Option<&str>, timeout: Duration) -> Result<reqwest::Response> {
    let mut b = reqwest::Client::builder().timeout(timeout).user_agent("detour-client");
    b = match proxy {
        Some(p) => b.proxy(reqwest::Proxy::all(p)?),
        None => b.no_proxy(),
    };
    let r = b.build()?.get(url).header("Accept", "application/vnd.github+json").send().await?;
    if !r.status().is_success() {
        bail!("{} ответил {}", url.split('/').nth(2).unwrap_or(url), r.status());
    }
    Ok(r)
}

/// Сначала через VPN (GitHub из РФ бывает медленным), потом напрямую.
async fn get_any(url: &str, proxy: Option<&str>, timeout: Duration) -> Result<reqwest::Response> {
    if let Some(p) = proxy {
        if let Ok(r) = get(url, Some(p), timeout).await {
            return Ok(r);
        }
    }
    get(url, None, timeout).await
}

pub fn state(store: &Store) -> Value {
    store
        .read_json(STATE)
        .unwrap_or_else(|| json!({ "status": "unknown", "message": "no bins state yet" }))
}

pub async fn check(store: &Store, engine: &Engine, proxy: Option<&str>) -> Result<Value> {
    let current = engine.version().await.unwrap_or_default();
    let pin = semver(&current).map(|(a, b, _)| (a, b)).unwrap_or((1, 13));
    let releases: Vec<Value> = get_any(RELEASES, proxy, Duration::from_secs(30)).await?.json().await?;
    let stable: Vec<(&Value, (u64, u64, u64))> = releases
        .iter()
        .filter(|r| !r["prerelease"].as_bool().unwrap_or(false) && !r["draft"].as_bool().unwrap_or(false))
        .filter_map(|r| Some((r, semver(r["tag_name"].as_str()?)?)))
        .collect();
    let best = |f: &dyn Fn(&(u64, u64, u64)) -> bool| stable.iter().filter(|(_, v)| f(v)).max_by_key(|(_, v)| *v).copied();
    let pinned = best(&|v| (v.0, v.1) == pin);
    let newest = best(&|_| true);
    let ver = |x: Option<(&Value, (u64, u64, u64))>| x.map(|(_, v)| format!("{}.{}.{}", v.0, v.1, v.2)).unwrap_or_default();
    let available = ver(pinned);
    let upstream = ver(newest);
    let changelog = pinned.and_then(|(r, _)| r["body"].as_str()).unwrap_or("");
    let changelog: String = changelog.chars().take(6000).collect();
    let upstream_newer = semver(&upstream) > semver(&available) && !upstream.is_empty();
    let st = json!({
        "current": current,
        "available": available,
        "upstream": upstream,
        "upstream_newer": upstream_newer,
        "asset": if available.is_empty() { String::new() } else { asset_name(&available).unwrap_or_default() },
        "last_check": humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        "changelog_b64": base64::engine::general_purpose::STANDARD.encode(changelog),
    });
    store.write_json(STATE, &st)?;
    Ok(st)
}

fn extract_exe(zip_bytes: &[u8], dest: &Path) -> Result<()> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).context("архив повреждён")?;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        if f.name().rsplit('/').next() == Some(engine::EXE) {
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)?;
            store::write_atomic(dest, &buf)?;
            return Ok(());
        }
    }
    bail!("в архиве нет {}", engine::EXE)
}

/// Скачать, проверить версией и подменить. Работающий exe на Windows нельзя
/// перезаписать, но можно переименовать — поэтому старый уезжает в `.old`,
/// а процесс перезапускает вызывающий.
pub async fn install(
    store: &Store,
    engine: &Engine,
    proxy: Option<&str>,
    log: &mut (dyn FnMut(String) + Send),
) -> Result<String> {
    let st = check(store, engine, proxy).await?;
    let version = st["available"].as_str().unwrap_or("").to_owned();
    if version.is_empty() {
        bail!("в релизах нет подходящей версии");
    }
    let asset = asset_name(&version)?;
    let url = format!("https://github.com/SagerNet/sing-box/releases/download/v{version}/{asset}");
    log(format!("скачиваю {asset}"));
    let mut resp = get_any(&url, proxy, Duration::from_secs(300)).await?;
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.extend_from_slice(&chunk);
        if buf.len() > MAX_ASSET {
            bail!("архив подозрительно большой");
        }
    }
    log(format!("получено {} КБ, распаковываю", buf.len() / 1024));

    let target = engine.local_binary().to_path_buf();
    let fresh = target.with_extension("new");
    extract_exe(&buf, &fresh)?;
    let got = engine::version_of(&fresh).await.unwrap_or_default();
    if got != version {
        let _ = std::fs::remove_file(&fresh);
        bail!("скачанный бинарник сообщает версию «{got}», ждали {version}");
    }
    let old = target.with_extension("old");
    let _ = std::fs::remove_file(&old);
    if target.exists() {
        std::fs::rename(&target, &old).context("не удалось отодвинуть старый sing-box")?;
    }
    std::fs::rename(&fresh, &target).context("не удалось поставить новый sing-box")?;
    log(format!("sing-box {version} установлен"));
    Ok(version)
}

// ---------- winws2 (zapret2) ----------

pub const DPI_STATE: &str = "run/dpi-bins.json";

/// Windows берёт winws2 из zapret2 (нужен WinDivert), macOS — tpws из
/// первого zapret: там готовая сборка `binaries/mac64`.
#[cfg(windows)]
const DPI_REPO: &str = "bol-van/zapret2";
#[cfg(not(windows))]
const DPI_REPO: &str = "bol-van/zapret";
#[cfg(windows)]
const DPI_ARCH_DIR: &str = "windows-x86_64/";
#[cfg(not(windows))]
const DPI_ARCH_DIR: &str = "mac64/";

fn dpi_zip_url(version: &str) -> String {
    let name = if cfg!(windows) { "zapret2" } else { "zapret" };
    format!("https://github.com/{DPI_REPO}/releases/download/v{version}/{name}-v{version}.zip")
}

/// Что забираем из архива: бинарник движка, его спутники и lua-стратегии.
/// Бинарники есть под несколько платформ — берём только свою.
fn dpi_wanted() -> Vec<&'static str> {
    if cfg!(windows) {
        let mut v = vec!["winws2.exe", "cygwin1.dll", "WinDivert.dll", "WinDivert64.sys"];
        v.extend(crate::dpi::LUA);
        v
    } else {
        vec!["tpws"]
    }
}

/// Файл из архива нужен, если это одна из целей и он из каталога нашей
/// платформы (lua лежат отдельно от бинарников).
fn dpi_take(path: &str, base: &str) -> bool {
    if !dpi_wanted().contains(&base) {
        return false;
    }
    base.ends_with(".lua") || path.contains(DPI_ARCH_DIR)
}

pub fn dpi_state(store: &Store) -> Value {
    store
        .read_json(DPI_STATE)
        .unwrap_or_else(|| json!({ "status": "unknown", "message": format!("{} ещё не проверяли", crate::dpi::NAME) }))
}

/// Последний релиз zapret2. Пиннинга по minor тут нет: стратегия и lua едут
/// одним архивом, версии между собой согласованы.
pub async fn dpi_check(store: &Store, dpi: &crate::dpi::Dpi, proxy: Option<&str>) -> Result<Value> {
    let current = dpi.version().await.unwrap_or_default();
    let url = format!("https://api.github.com/repos/{DPI_REPO}/releases/latest");
    let (available, changelog) = match dpi_latest_api(&url, proxy).await {
        Ok(v) => v,
        // Анонимный API режется лимитом по IP (403 за общим NAT), а страница
        // releases/latest просто редиректит на тег — лимита у неё нет.
        Err(e) => {
            let page = format!("https://github.com/{DPI_REPO}/releases/latest");
            let resp = get_any(&page, proxy, Duration::from_secs(30)).await.map_err(|_| e)?;
            (tag_from_release_url(resp.url().as_str()).unwrap_or_default(), String::new())
        }
    };
    let st = json!({
        "current": current,
        "available": available,
        "update_available": !available.is_empty() && available != current,
        "last_check": humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        "changelog_b64": base64::engine::general_purpose::STANDARD.encode(changelog),
    });
    store.write_json(DPI_STATE, &st)?;
    Ok(st)
}

async fn dpi_latest_api(url: &str, proxy: Option<&str>) -> Result<(String, String)> {
    let rel: Value = get_any(url, proxy, Duration::from_secs(30)).await?.json().await?;
    let available = rel["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_owned();
    let changelog = rel["body"].as_str().unwrap_or("").chars().take(6000).collect();
    Ok((available, changelog))
}

/// `…/releases/tag/v1.0.5.2` → `1.0.5.2`.
fn tag_from_release_url(url: &str) -> Option<String> {
    let tag = url.split("/releases/tag/").nth(1)?.split(['?', '#', '/']).next()?;
    let v = tag.trim_start_matches('v');
    (!v.is_empty()).then(|| v.to_owned())
}

/// Движок и его lua-скрипты кладутся рядом друг с другом: `supported()`
/// требует все три файла, иначе стратегия не соберётся.
fn extract_dpi(zip_bytes: &[u8], dir: &Path) -> Result<Vec<String>> {
    let want = dpi_wanted();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).context("архив повреждён")?;
    let mut got = Vec::new();
    std::fs::create_dir_all(dir)?;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let name = f.name().to_owned();
        let base = name.rsplit('/').next().unwrap_or("").to_owned();
        if !dpi_take(&name, &base) {
            continue;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)?;
        let dest = dir.join(&base);
        if let Err(e) = store::write_atomic(&dest, &buf) {
            // Драйвер WinDivert остаётся загруженным и после выхода winws2:
            // переписать его нельзя, а переименовать — можно.
            let old = dest.with_extension("old");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&dest, &old).map_err(|_| e)?;
            store::write_atomic(&dest, &buf)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if !base.ends_with(".lua") {
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
        }
        got.push(base);
    }
    for w in &want {
        if !got.iter().any(|g| g == w) {
            bail!("в архиве zapret2 нет {w}");
        }
    }
    Ok(got)
}

pub async fn dpi_install(
    store: &Store,
    dpi: &crate::dpi::Dpi,
    proxy: Option<&str>,
    log: &mut (dyn FnMut(String) + Send),
) -> Result<String> {
    let st = dpi_check(store, dpi, proxy).await?;
    let version = st["available"].as_str().unwrap_or("").to_owned();
    if version.is_empty() {
        bail!("в релизах zapret2 нет версии");
    }
    let url = dpi_zip_url(&version);
    log(format!("скачиваю {} {version}", DPI_REPO.rsplit('/').next().unwrap_or("zapret")));
    let mut resp = get_any(&url, proxy, Duration::from_secs(600)).await?;
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.extend_from_slice(&chunk);
        if buf.len() > MAX_ASSET {
            bail!("архив подозрительно большой");
        }
    }
    log(format!("получено {} КБ, распаковываю", buf.len() / 1024));
    let dir = store.path("bin");
    let files = extract_dpi(&buf, &dir)?;
    log(format!("поставлено файлов: {}", files.len()));
    match dpi.version().await {
        Some(v) => log(format!("{} {v} установлен", crate::dpi::NAME)),
        None => bail!("{} не запускается после установки", crate::dpi::NAME),
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_archive_picks_own_platform() {
        let arch = if cfg!(windows) { "winws2.exe" } else { "tpws" };
        let good = format!("zapret/binaries/{DPI_ARCH_DIR}{arch}");
        assert!(dpi_take(&good, arch));
        assert!(!dpi_take("zapret/binaries/linux-x86_64/tpws", "tpws"));
        assert!(!dpi_take("zapret/binaries/windows-x86/winws2.exe", "winws2.exe"));
        assert!(!dpi_take("zapret/init.d/openwrt/zapret", "zapret"));
        assert_eq!(dpi_take("zapret2/lua/zapret-lib.lua", "zapret-lib.lua"), cfg!(windows));
    }

    #[test]
    fn tag_from_redirect() {
        let u = "https://github.com/bol-van/zapret2/releases/tag/v1.0.5.2";
        assert_eq!(tag_from_release_url(u).as_deref(), Some("1.0.5.2"));
        assert_eq!(tag_from_release_url("https://github.com/bol-van/zapret2/releases"), None);
    }

    #[test]
    fn semver_orders_patches() {
        assert!(semver("v1.13.21") > semver("1.13.9"));
        assert_eq!(semver("1.14.0-beta.2"), Some((1, 14, 0)));
        assert!(semver("junk").is_none());
    }
}
