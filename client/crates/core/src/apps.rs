//! Правила приложений: весь трафик выбранного приложения — мимо VPN или через
//! свой профиль/цепочку. Аналог «Правил → Устройства» роутера, только вместо
//! MAC — процесс (Windows, macOS) или пакет (Android).
//!
//! Файл — строки `match|mode|target|name`, как `devices.conf` роутера: mode
//! `direct` или `vpn`. `match` — имя исполняемого файла (`Telegram.exe`), путь
//! или каталог (`/Applications/Discord.app` — со всеми вспомогательными
//! процессами внутри) либо имя пакета Android.

use serde::Serialize;

use crate::store::{self, Store};

/// iOS: Packet Tunnel не знает, чьё соединение, — правила недоступны.
pub const SUPPORTED: bool = !cfg!(target_os = "ios");

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rule {
    #[serde(rename = "match")]
    pub matcher: String,
    /// `None` — мимо VPN, иначе id профиля или цепочки.
    pub target: Option<String>,
    pub name: String,
}

pub fn parse(text: &str) -> Vec<Rule> {
    let mut out: Vec<Rule> = Vec::new();
    for line in text.lines() {
        let mut f = line.splitn(4, '|');
        let matcher = f.next().unwrap_or("").trim().to_owned();
        let mode = f.next().unwrap_or("").trim();
        let target = f.next().unwrap_or("").trim();
        let name = f.next().unwrap_or("").trim().to_owned();
        if matcher.is_empty() || matcher.starts_with("//") || out.iter().any(|r| r.matcher == matcher) {
            continue;
        }
        let target = match mode {
            "direct" => None,
            "vpn" if !target.is_empty() => Some(target.to_owned()),
            _ => continue,
        };
        out.push(Rule { matcher, target, name });
    }
    out
}

pub fn serialize(rules: &[Rule]) -> String {
    let clean = |s: &str| s.replace(['|', '\n', '\r', '\t'], " ").trim().to_owned();
    rules
        .iter()
        .map(|r| match &r.target {
            None => format!("{}|direct||{}\n", clean(&r.matcher), clean(&r.name)),
            Some(t) => format!("{}|vpn|{}|{}\n", clean(&r.matcher), clean(t), clean(&r.name)),
        })
        .collect()
}

pub fn load(store: &Store) -> Vec<Rule> {
    if !SUPPORTED {
        return Vec::new();
    }
    parse(&store.read_text(store::APP_RULES))
}

/// Путь или каталог, а не голое имя файла.
pub fn is_path(m: &str) -> bool {
    m.contains('/') || m.contains('\\')
}

/// Матчер правила sing-box для набора приложений. На десктопе — всегда
/// `process_path_regex` без учёта регистра: `process_name` сравнивает строку
/// точно, и «telegram.exe» вместо «Telegram.exe» молча не сработал бы.
pub fn matcher(items: &[&str]) -> serde_json::Value {
    if cfg!(target_os = "android") {
        return serde_json::json!({ "package_name": items });
    }
    let re: Vec<String> = items.iter().map(|m| path_regex(m)).collect();
    serde_json::json!({ "process_path_regex": re })
}

fn path_regex(m: &str) -> String {
    let m = m.trim();
    if is_path(m) {
        let p = m.trim_end_matches(['/', '\\']);
        let p = if cfg!(windows) { p.replace('/', "\\") } else { p.to_owned() };
        // сам файл или всё внутри каталога (бандл .app, папка программы)
        format!("(?i)^{}($|[\\\\/])", regex_escape(&p))
    } else {
        format!("(?i)[\\\\/]{}$", regex_escape(m))
    }
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Приложение для выбора в панели: `match` — что писать в правило.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct App {
    #[serde(rename = "match")]
    pub matcher: String,
    pub name: String,
    /// Путь к файлу (десктоп) — подсказка, откуда процесс.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub system: bool,
}

/// Запущенные сейчас программы (десктоп). Android отдаёт список установленных
/// приложений сам — через `engine::tunnel`.
pub fn running() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for path in process_paths() {
        if let Some(app) = from_path(&path) {
            if !out.iter().any(|a| a.matcher.eq_ignore_ascii_case(&app.matcher)) {
                out.push(app);
            }
        }
    }
    out.sort_by_key(|a| (a.system, a.name.to_lowercase()));
    out
}

/// Windows — правило по имени файла (у Electron-программ все процессы
/// называются одинаково). macOS — по бандлу `.app`: так в правило попадают и
/// его «Helper»-процессы, у которых свои имена.
fn from_path(path: &str) -> Option<App> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    if let Some(i) = path.find(".app/") {
        let bundle = &path[..i + 4];
        let name = bundle.rsplit('/').next()?.trim_end_matches(".app").to_owned();
        let system = bundle.starts_with("/System/");
        return Some(App { matcher: bundle.to_owned(), name, path: bundle.to_owned(), system });
    }
    let file = path.rsplit(['/', '\\']).next()?.to_owned();
    if file.is_empty() || file.eq_ignore_ascii_case("detour-svc.exe") || file == "detour-svc" {
        return None;
    }
    let low = path.to_ascii_lowercase();
    let system = if low.starts_with('/') {
        ["/system/", "/usr/libexec/", "/usr/sbin/", "/sbin/", "/usr/bin/", "/bin/", "/library/apple/"]
            .iter()
            .any(|p| low.starts_with(p))
    } else {
        low.starts_with(r"c:\windows\") || !low.contains('\\')
    };
    let name = file.strip_suffix(".exe").or(file.strip_suffix(".EXE")).unwrap_or(&file).to_owned();
    Some(App { matcher: file, name, path: path.to_owned(), system })
}

#[cfg(windows)]
fn process_paths() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let mut out = Vec::new();
    // SAFETY: снимок и дескрипторы закрываются здесь же; буферы — свои.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..len]);
            let mut path = String::new();
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
            if !h.is_null() {
                let mut buf = [0u16; 1024];
                let mut n = buf.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) != 0 {
                    path = String::from_utf16_lossy(&buf[..n as usize]);
                }
                CloseHandle(h);
            }
            // без пути — системный процесс, дескриптор которого не открыть
            out.push(if path.is_empty() { exe } else { path });
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(target_os = "macos")]
fn process_paths() -> Vec<String> {
    std::process::Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn process_paths() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialize() {
        let r = parse("Telegram.exe|direct||Телеграм\n// x\n/Applications/Discord.app|vpn|nl|Дискорд\nbad|vpn||\nTelegram.exe|vpn|nl|\nx|weird|y|\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], Rule { matcher: "Telegram.exe".into(), target: None, name: "Телеграм".into() });
        assert_eq!(r[1].target.as_deref(), Some("nl"));
        assert_eq!(parse(&serialize(&r)), r);
        let dirty = vec![Rule { matcher: "a|b".into(), target: Some("t".into()), name: "n\nm".into() }];
        assert_eq!(serialize(&dirty), "a b|vpn|t|n m\n");
    }

    #[test]
    fn apps_from_paths() {
        let a = from_path("/Applications/Discord.app/Contents/Frameworks/Discord Helper.app/Contents/MacOS/Discord Helper").unwrap();
        assert_eq!((a.matcher.as_str(), a.name.as_str(), a.system), ("/Applications/Discord.app", "Discord", false));
        let w = from_path(r"C:\Users\u\AppData\Roaming\Telegram Desktop\Telegram.exe").unwrap();
        assert_eq!((w.matcher.as_str(), w.name.as_str()), ("Telegram.exe", "Telegram"));
        assert!(from_path(r"C:\Program Files\Detour\detour-svc.exe").is_none());
        assert!(from_path("/usr/libexec/trustd").unwrap().system);
        if cfg!(windows) {
            assert!(from_path(r"C:\Windows\System32\svchost.exe").unwrap().system);
            assert!(from_path("System").unwrap().system);
        }
    }

    #[test]
    fn regexes() {
        assert_eq!(path_regex("Telegram.exe"), r"(?i)[\\/]Telegram\.exe$");
        if cfg!(windows) {
            assert_eq!(path_regex(r"C:/Games/Steam/"), r"(?i)^C:\\Games\\Steam($|[\\/])");
        } else {
            assert_eq!(path_regex("/Applications/Discord.app"), r"(?i)^/Applications/Discord\.app($|[\\/])");
        }
    }
}
