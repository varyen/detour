//! Демон на macOS. Аналог службы Windows: launchd держит `detour-svc run` в
//! переднем плане под root, сам поднимает его после перезагрузки и после
//! падения (`KeepAlive`). Интерфейс работает под обычным пользователем и
//! ходит в демон по unix-сокету.

use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
use anyhow::{bail, Context, Result};

pub const LABEL: &str = "com.detour.svc";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PLIST_DIR: &str = "/Library/LaunchDaemons";
const LOG: &str = "/Library/Logs/Detour";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plist_path() -> PathBuf {
    Path::new(PLIST_DIR).join(format!("{LABEL}.plist"))
}

/// XML пишем руками: тащить plist-крейт ради десятка строк незачем.
pub fn plist(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardOutPath</key>
    <string>{LOG}/svc.out.log</string>
    <key>StandardErrorPath</key>
    <string>{LOG}/svc.err.log</string>
</dict>
</plist>
"#,
        exe = exe.display()
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PF_CONF: &str = "/etc/pf.conf";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PF_ANCHOR_FILE: &str = "/etc/pf.anchors/detour";
const PF_MARK: &str = "# detour kill-switch";

/// Правила защиты от утечки живут в отдельном якоре, но pf их не увидит, пока
/// якорь не объявлен в основном наборе. Строки дописываются один раз.
pub fn pf_conf_with_anchor(text: &str) -> Option<String> {
    if text.contains(PF_MARK) {
        return None;
    }
    let mut out = text.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "{PF_MARK}\nanchor \"detour\"\nload anchor \"detour\" from \"/etc/pf.anchors/detour\"\n"
    ));
    Some(out)
}

/// Обратная операция при удалении демона.
pub fn pf_conf_without_anchor(text: &str) -> Option<String> {
    if !text.contains(PF_MARK) {
        return None;
    }
    let out: String = text
        .lines()
        .filter(|l| {
            let l = l.trim();
            l != PF_MARK && l != "anchor \"detour\"" && l != "load anchor \"detour\" from \"/etc/pf.anchors/detour\""
        })
        .map(|l| format!("{l}\n"))
        .collect();
    Some(out)
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> Result<()> {
    let out = Command::new("/bin/launchctl").args(args).output().context("launchctl не запустился")?;
    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    bail!("launchctl {}: {msg}", args.join(" "));
}

/// `bootout` возвращается раньше, чем launchd на самом деле выгрузит демон, и
/// `bootstrap` сразу следом падает с «Input/output error» — ждём, пока метка
/// исчезнет.
#[cfg(target_os = "macos")]
fn wait_unloaded() {
    for _ in 0..50 {
        let loaded = Command::new("/bin/launchctl")
            .args(["print", &format!("system/{LABEL}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !loaded {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(target_os = "macos")]
pub fn install() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let exe = std::env::current_exe().context("не найден путь к detour-svc")?;
    std::fs::create_dir_all(LOG)?;
    let path = plist_path();
    // Перерегистрация: старый демон сначала выгружаем, иначе launchctl
    // откажется загружать тот же Label.
    let _ = launchctl(&["bootout", &format!("system/{LABEL}")]);
    wait_unloaded();
    std::fs::write(&path, plist(&exe)).with_context(|| format!("не записать {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    // Файл якоря должен существовать раньше ссылки на него: иначе pfctl
    // отвергает весь /etc/pf.conf, и система остаётся без своих правил pf.
    std::fs::create_dir_all("/etc/pf.anchors")?;
    if !Path::new(PF_ANCHOR_FILE).exists() {
        std::fs::write(PF_ANCHOR_FILE, "").context("не создать файл якоря pf")?;
    }
    if let Some(conf) = std::fs::read_to_string(PF_CONF).ok().and_then(|t| pf_conf_with_anchor(&t)) {
        std::fs::write(PF_CONF, conf).context("не записать /etc/pf.conf")?;
        let _ = Command::new("/sbin/pfctl").args(["-f", PF_CONF]).status();
    }
    launchctl(&["bootstrap", "system", &path.to_string_lossy()])?;
    launchctl(&["enable", &format!("system/{LABEL}")])?;
    println!("демон {LABEL} установлен и запущен");
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let _ = launchctl(&["bootout", &format!("system/{LABEL}")]);
    let path = plist_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    if let Some(conf) = std::fs::read_to_string(PF_CONF).ok().and_then(|t| pf_conf_without_anchor(&t)) {
        let _ = std::fs::write(PF_CONF, conf);
        let _ = Command::new("/sbin/pfctl").args(["-f", PF_CONF]).status();
    }
    let _ = std::fs::remove_file(PF_ANCHOR_FILE);
    println!("демон {LABEL} удалён");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pf_anchor_added_once_and_removed() {
        let base = "scrub-anchor \"com.apple/*\"
";
        let with = pf_conf_with_anchor(base).unwrap();
        assert!(with.contains("anchor \"detour\""));
        assert!(pf_conf_with_anchor(&with).is_none(), "второй раз дописывать нечего");
        let back = pf_conf_without_anchor(&with).unwrap();
        assert_eq!(back, base);
        assert!(pf_conf_without_anchor(base).is_none());
    }

    #[test]
    fn plist_has_label_and_exe() {
        let x = plist(Path::new("/usr/local/bin/detour-svc"));
        assert!(x.contains("<string>com.detour.svc</string>"));
        assert!(x.contains("<string>/usr/local/bin/detour-svc</string>"));
        assert!(x.contains("<string>run</string>"));
        assert!(x.contains("<key>KeepAlive</key>"));
    }
}
