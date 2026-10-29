//! Обновление самого приложения Detour из релизов varyen/detour на GitHub.
//!
//! Ядро проверяет последний релиз, скачивает сборку своей платформы и
//! сверяет её usign-подпись (`.sig` рядом в релизе, тот же ключ Ed25519, что
//! у пакетов роутера). Без подписи файл не ставится вовсе — подмена ассета
//! или ответа по пути до GitHub не превращается в чужой код с правами службы.
//!
//! Установку делает тот, у кого на это права:
//! - Windows — окно приложения запускает проверенный установщик NSIS с
//!   запросом прав (`/S`), тот гасит службу, ставит файлы и поднимает всё
//!   обратно;
//! - macOS — демон (root) монтирует образ, подменяет `Detour.app` и
//!   выходит, launchd поднимает его уже новым;
//! - Android — системный установщик APK (через java-сторону).
//!
//! На iOS приложение ставится в обход App Store, обновлять его некому.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use crate::store::{self, Store};
use crate::updater::{get_any, semver};

pub const STATE: &str = "run/app-update.json";
const DIR: &str = "update";
const REPO: &str = "varyen/detour";
const MAX_ASSET: usize = 256 << 20;
const PUBKEY: &str = include_str!("../../../../keys/release.usign.pub");

pub fn supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos", target_os = "android"))
}

/// Имя сборки этой платформы в релизе (так их называет build_release.py).
pub fn asset_name(v: &str) -> Option<String> {
    if cfg!(target_os = "windows") {
        Some(format!("detour-client-windows_{v}_x64-setup.exe"))
    } else if cfg!(target_os = "macos") {
        Some(format!("detour-client-macos_{v}_x64.dmg"))
    } else if cfg!(target_os = "android") {
        let abi = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "arm" => "armv7",
            "x86_64" => "x86_64",
            _ => return None,
        };
        Some(format!("detour-client-android_{v}_{abi}.apk"))
    } else {
        None
    }
}

pub fn state(store: &Store) -> Value {
    let mut st = store.read_json(STATE).unwrap_or_else(|| json!({ "status": "unknown" }));
    if let Some(o) = st.as_object_mut() {
        o.insert("current".into(), json!(crate::VERSION));
        o.insert("supported".into(), json!(supported()));
        let newer = newer(o.get("available").and_then(Value::as_str).unwrap_or(""));
        o.insert("update_available".into(), json!(newer));
        // Скачанный файл мог пропасть (чистка, переустановка) — тогда его нет.
        let have = o
            .get("downloaded")
            .and_then(Value::as_str)
            .map(|p| Path::new(p).is_file())
            .unwrap_or(false);
        if !have || !newer {
            o.remove("downloaded");
        }
    }
    st
}

fn newer(available: &str) -> bool {
    match (semver(available), semver(crate::VERSION)) {
        (Some(a), Some(c)) => a > c,
        _ => false,
    }
}

fn now() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

pub async fn check(store: &Store, proxy: Option<&str>) -> Result<Value> {
    if !supported() {
        bail!("на этой платформе приложение обновляется не из панели");
    }
    // Не каждый релиз несёт сборки приложения (бывают только роутерные), поэтому
    // берём самый свежий, где есть сборка этой платформы вместе с подписью.
    let api = format!("https://api.github.com/repos/{REPO}/releases?per_page=15");
    let (available, changelog) = match get_any(&api, proxy, Duration::from_secs(30)).await {
        Ok(r) => {
            let list: Value = r.json().await?;
            let rel = list
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false))
                .find(|r| {
                    let v = r["tag_name"].as_str().unwrap_or("").trim_start_matches('v');
                    let Some(want) = asset_name(v) else { return false };
                    let names: Vec<&str> =
                        r["assets"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str()).collect();
                    names.contains(&want.as_str()) && names.contains(&format!("{want}.sig").as_str())
                })
                .cloned()
                .unwrap_or(Value::Null);
            (
                rel["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_owned(),
                rel["body"].as_str().unwrap_or("").chars().take(8000).collect::<String>(),
            )
        }
        // Анонимный API режется лимитом по IP; лента релизов — нет. В ней не
        // видно ассетов, поэтому наличие сборки проверяем по её подписи.
        Err(e) => (latest_with_asset(proxy).await.map_err(|_| e)?, String::new()),
    };
    if available.is_empty() {
        // Ни в одном из свежих релизов нет сборки этой платформы — обновляться не на что.
        store.write_json(STATE, &json!({ "status": "ok", "available": "", "last_check": now() }))?;
        return Ok(state(store));
    }
    let asset = asset_name(&available).ok_or_else(|| anyhow!("для этой архитектуры сборки нет"))?;
    let mut st = json!({
        "status": "ok",
        "available": available,
        "asset": asset,
        "last_check": now(),
        "changelog_b64": base64::engine::general_purpose::STANDARD.encode(changelog),
    });
    // Уже скачанная и проверенная сборка той же версии остаётся в силе.
    if let Some(prev) = store.read_json(STATE) {
        if prev["available"] == st["available"] {
            if let Some(d) = prev.get("downloaded") {
                st["downloaded"] = d.clone();
            }
        }
    }
    store.write_json(STATE, &st)?;
    Ok(state(store))
}

fn atom_versions(atom: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in atom.split("/releases/tag/").skip(1) {
        let tag = part.split(['"', '<', '?', '#', '/']).next().unwrap_or("");
        let v = tag.trim_start_matches('v').to_owned();
        if semver(&v).is_some() && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

async fn latest_with_asset(proxy: Option<&str>) -> Result<String> {
    let feed = format!("https://github.com/{REPO}/releases.atom");
    let atom = get_any(&feed, proxy, Duration::from_secs(30)).await?.text().await?;
    for v in atom_versions(&atom) {
        let Some(asset) = asset_name(&v) else { continue };
        let sig = format!("https://github.com/{REPO}/releases/download/v{v}/{asset}.sig");
        if fetch(&sig, proxy, Duration::from_secs(30), 4096, None).await.is_ok() {
            return Ok(v);
        }
    }
    Ok(String::new())
}

/// Причина последней неудачной установки — для карточки «Приложение»: шторку
/// с журналом часто закрывают, не дождавшись конца. Пустая строка стирает.
pub fn set_install_error(store: &Store, msg: &str) {
    let mut st = store.read_json(STATE).unwrap_or_else(|| json!({}));
    if let Some(o) = st.as_object_mut() {
        if msg.is_empty() {
            o.remove("install_error");
        } else {
            o.insert("install_error".into(), json!(msg));
        }
        let _ = store.write_json(STATE, &st);
    }
}

fn mb(n: usize) -> f64 {
    n as f64 / 1048576.0
}

async fn fetch(
    url: &str,
    proxy: Option<&str>,
    timeout: Duration,
    cap: usize,
    mut progress: Option<&mut (dyn FnMut(String) + Send)>,
) -> Result<Vec<u8>> {
    let mut resp = get_any(url, proxy, timeout).await?;
    let total = resp.content_length().map(|n| n as usize);
    // Отметка раз в 10 % (или раз в 8 МБ, если размер не пришёл): на мобильной
    // сети сборка качается минутами, и без хода журнал выглядит зависшим.
    let step = total.map(|t| (t / 10).max(1 << 20)).unwrap_or(8 << 20);
    let mut next = step;
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.extend_from_slice(&chunk);
        if buf.len() > cap {
            bail!("файл больше {} МБ — это не наша сборка", cap >> 20);
        }
        if buf.len() >= next {
            next = buf.len() + step;
            if let Some(p) = progress.as_mut() {
                p(match total {
                    Some(t) => format!("скачано {:.1} из {:.1} МБ ({}%)", mb(buf.len()), mb(t), buf.len() * 100 / t.max(1)),
                    None => format!("скачано {:.1} МБ", mb(buf.len())),
                });
            }
        }
    }
    Ok(buf)
}

/// Перепроверить релизы и скачать свежую сборку. Версию из прошлой проверки
/// берём, только если GitHub сейчас не ответил: она могла устареть на
/// несколько релизов.
pub async fn download(store: &Store, proxy: Option<&str>, log: &mut (dyn FnMut(String) + Send)) -> Result<PathBuf> {
    log("проверяю последнюю версию".into());
    let st = match check(store, proxy).await {
        Ok(st) => st,
        Err(e) => {
            log(format!("проверка не удалась ({e:#}) — беру версию из прошлой проверки"));
            state(store)
        }
    };
    let available = st["available"].as_str().unwrap_or("").to_owned();
    if !newer(&available) {
        bail!("новой версии нет — установлена {}", crate::VERSION);
    }
    let asset = asset_name(&available).ok_or_else(|| anyhow!("для этой архитектуры сборки нет"))?;
    // Повтор после отказа установщика не должен заново тянуть сотню мегабайт.
    if let Some(p) = st["downloaded"].as_str().map(PathBuf::from) {
        if p.file_name().is_some_and(|n| n == asset.as_str()) && verify_file(&p).is_ok() {
            log(format!("версия {available} уже скачана и проверена"));
            return Ok(p);
        }
    }
    let base = format!("https://github.com/{REPO}/releases/download/v{available}/{asset}");
    log(format!("версия {available}, скачиваю {asset}"));
    let sig = fetch(&format!("{base}.sig"), proxy, Duration::from_secs(60), 4096, None)
        .await
        .context("подпись .sig не скачалась")?;
    let bytes = fetch(&base, proxy, Duration::from_secs(900), MAX_ASSET, Some(&mut *log))
        .await
        .context("сборка не скачалась")?;
    log(format!("скачано {:.1} МБ, проверяю подпись", mb(bytes.len())));
    verify(&bytes, &sig)?;
    log("подпись верна".into());

    let dir = store.path(DIR);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(&asset);
    store::write_atomic(&path, &bytes)?;
    store::write_atomic(&dir.join(format!("{asset}.sig")), &sig)?;

    let mut st = store.read_json(STATE).unwrap_or_else(|| json!({}));
    st["downloaded"] = json!(path.to_string_lossy());
    store.write_json(STATE, &st)?;
    Ok(path)
}

/// Скачанная проверенная сборка; подпись сверяется ещё раз прямо перед
/// установкой — между скачиванием и нажатием файл мог смениться.
pub fn ready(store: &Store) -> Result<PathBuf> {
    let st = state(store);
    let p = st["downloaded"].as_str().ok_or_else(|| anyhow!("сборка ещё не скачана"))?;
    let path = PathBuf::from(p);
    verify_file(&path)?;
    Ok(path)
}

pub fn verify_file(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("не читается {}", path.display()))?;
    let mut sig_path = path.as_os_str().to_owned();
    sig_path.push(".sig");
    let sig = std::fs::read(PathBuf::from(sig_path)).context("рядом нет подписи .sig")?;
    verify(&bytes, &sig)
}

/// usign: вторая строка файла — base64 от "Ed" + номер ключа (8) + подпись (64);
/// ключ — "Ed" + номер (8) + открытый ключ Ed25519 (32).
fn usign_blob(text: &[u8], want: usize) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(text).context("подпись не текст")?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .ok_or_else(|| anyhow!("пустая подпись"))?;
    let raw = base64::engine::general_purpose::STANDARD.decode(line).context("подпись не base64")?;
    if raw.len() != want || &raw[..2] != b"Ed" {
        bail!("это не подпись usign Ed25519");
    }
    Ok(raw)
}

fn verify(data: &[u8], sig_text: &[u8]) -> Result<()> {
    let pk = usign_blob(PUBKEY.as_bytes(), 42)?;
    let sig = usign_blob(sig_text, 74)?;
    if pk[2..10] != sig[2..10] {
        bail!("подпись сделана другим ключом — сборка не от Detour");
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &pk[10..])
        .verify(data, &sig[10..])
        .map_err(|_| anyhow!("подпись не сходится — файл повреждён или подменён"))
}

/// macOS: демон от root подменяет бандл, из которого запущен, на `.app` из
/// образа. Возвращает путь к бандлу.
#[cfg(target_os = "macos")]
pub fn install_macos(dmg: &Path, log: &mut (dyn FnMut(String) + Send)) -> Result<PathBuf> {
    use std::process::Command;
    let exe = std::env::current_exe()?;
    // …/Detour.app/Contents/MacOS/detour-svc
    let app = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        .ok_or_else(|| anyhow!("служба запущена не из Detour.app — обновите вручную"))?
        .to_path_buf();
    let mnt = std::env::temp_dir().join(format!("detour-update-{}", std::process::id()));
    std::fs::create_dir_all(&mnt)?;
    let ok = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mnt)
        .arg(dmg)
        .status()?
        .success();
    if !ok {
        bail!("образ не смонтировался");
    }
    let res = (|| -> Result<()> {
        let src = std::fs::read_dir(&mnt)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
            .ok_or_else(|| anyhow!("в образе нет .app"))?;
        let staged = app.with_extension("app.new");
        let _ = std::fs::remove_dir_all(&staged);
        log(format!("копирую {}", src.display()));
        if !Command::new("ditto").arg(&src).arg(&staged).status()?.success() {
            bail!("не удалось скопировать приложение");
        }
        let old = app.with_extension("app.old");
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(&app, &old).context("старое приложение не убралось")?;
        if let Err(e) = std::fs::rename(&staged, &app) {
            let _ = std::fs::rename(&old, &app);
            bail!("новое приложение не встало: {e}");
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(&mnt).status();
    let _ = std::fs::remove_dir(&mnt);
    res?;
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Настоящая подпись релиза: годится, испорченный байт — нет.
    #[test]
    fn verifies_release_signature() {
        let ipk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../releases/v2.9.0/detour_2.9.0_all.ipk");
        let Ok(mut data) = std::fs::read(&ipk) else {
            eprintln!("нет {} — пропускаю", ipk.display());
            return;
        };
        let sig = std::fs::read(ipk.with_extension("ipk.sig")).expect("sig");
        verify(&data, &sig).expect("подпись релиза должна сходиться");
        data[100] ^= 1;
        assert!(verify(&data, &sig).is_err());
        assert!(verify(b"x", b"untrusted comment: x\nAAAA\n").is_err());
    }

    #[test]
    fn asset_names() {
        if cfg!(windows) {
            assert_eq!(asset_name("2.10.0").unwrap(), "detour-client-windows_2.10.0_x64-setup.exe");
        }
        assert!(!newer("0.0.1"));
        assert!(newer("999.0.0"));
    }

    #[test]
    fn atom_tags() {
        let atom = r#"<link rel="alternate" href="https://github.com/varyen/detour/releases/tag/v2.18.0"/>
<id>tag:github.com,2008:Repository/1/v2.18.0</id>
<link rel="alternate" href="https://github.com/varyen/detour/releases/tag/v2.17.8"/>
<link rel="alternate" href="https://github.com/varyen/detour/releases/tag/nightly"/>"#;
        assert_eq!(atom_versions(atom), vec!["2.18.0", "2.17.8"]);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Живой GitHub: `DETOUR_LIVE=1 cargo test -p detour-core live -- --nocapture`.
    #[tokio::test]
    async fn check_real_releases() {
        if std::env::var("DETOUR_LIVE").is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("detour-appupd-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("run")).unwrap();
        let store = Store::new(dir.clone());
        let st = check(&store, None).await.expect("check");
        eprintln!("available={} update={}", st["available"], st["update_available"]);
        let v = st["available"].as_str().unwrap();
        let base = format!("https://github.com/{REPO}/releases/download/v{v}/{}", asset_name(v).unwrap());
        let sig = fetch(&format!("{base}.sig"), None, Duration::from_secs(60), 4096, None).await.unwrap();
        let mut steps = Vec::new();
        let bytes =
            fetch(&base, None, Duration::from_secs(600), MAX_ASSET, Some(&mut |l| steps.push(l))).await.unwrap();
        eprintln!("{}", steps.join("
"));
        assert!(steps.len() >= 5, "ход скачивания не отмечался: {steps:?}");
        verify(&bytes, &sig).expect("подпись сборки приложения");
        eprintln!("сборка {} МБ, подпись верна", bytes.len() >> 20);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Запасной путь без API: релиз без сборки приложения (как 2.18.0) пропускается.
    #[tokio::test]
    async fn feed_fallback_skips_router_only() {
        if std::env::var("DETOUR_LIVE").is_err() {
            return;
        }
        let v = latest_with_asset(None).await.expect("feed");
        eprintln!("feed → {v}");
        assert!(semver(&v).is_some());
        assert_ne!(v, "2.18.0");
    }
}
