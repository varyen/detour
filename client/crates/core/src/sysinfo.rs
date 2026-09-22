use std::path::Path;

use serde_json::{json, Value};

/// Блок `system` статуса. `cpu` — загрузка между двумя соседними вызовами
/// (панель опрашивает статус постоянно); первый вызов отдаёт `?`, как роутер
/// без данных. На macOS FFI не нужен: всё, что показывает плитка, отдают
/// штатные утилиты, а разбор их вывода вынесен в чистые функции с тестами.
pub fn system(data: &Path) -> Value {
    let mut v = json!({
        "cpu_cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "cpu": "?",
    });
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::sync::Mutex;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        use windows_sys::Win32::System::SystemInformation::{
            GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };
        use windows_sys::Win32::System::Threading::GetSystemTimes;

        let mb = |b: u64| b / (1024 * 1024);
        let secs = unsafe { GetTickCount64() } / 1000;
        v["uptime"] = json!(fmt_uptime(secs));
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 {
            v["memory"] = json!(format!("{}MB/{}MB", mb(m.ullTotalPhys - m.ullAvailPhys), mb(m.ullTotalPhys)));
        }

        let wide: Vec<u16> = data.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut free = 0u64;
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) } != 0 {
            v["disk_free"] = json!(format!("{}MB", mb(free)));
        }

        static PREV: Mutex<Option<(u64, u64)>> = Mutex::new(None);
        let ft = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        let (mut idle, mut kernel, mut user) = unsafe { std::mem::zeroed::<(FILETIME, FILETIME, FILETIME)>() };
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0 {
            // Время ядра включает простой.
            let (idle, total) = (ft(idle), ft(kernel) + ft(user));
            let mut prev = PREV.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((pi, pt)) = prev.replace((idle, total)) {
                let dt = total.saturating_sub(pt);
                if dt > 0 {
                    let busy = dt.saturating_sub(idle.saturating_sub(pi));
                    v["cpu"] = json!(((busy * 100 + dt / 2) / dt).to_string());
                }
            }
        }
    }
    #[cfg(unix)]
    {
        let cores = v["cpu_cores"].as_u64().unwrap_or(1).max(1);
        if let Some(sec) = out("sysctl", &["-n", "kern.boottime"]).as_deref().and_then(parse_boottime) {
            let now = crate::store::now_epoch();
            v["uptime"] = json!(fmt_uptime((now - sec).max(0) as u64));
        }
        let total = out("sysctl", &["-n", "hw.memsize"]).as_deref().and_then(parse_num);
        if let (Some(total), Some(free)) = (total, out("vm_stat", &[]).as_deref().and_then(parse_vm_stat_free)) {
            let mb = |b: u64| b / (1024 * 1024);
            v["memory"] = json!(format!("{}MB/{}MB", mb(total.saturating_sub(free)), mb(total)));
        }
        if let Some(kb) = out("df", &["-k", &data.to_string_lossy()]).as_deref().and_then(parse_df_avail) {
            v["disk_free"] = json!(format!("{}MB", kb / 1024));
        }
        // Мгновенной загрузки у macOS без FFI нет, поэтому берём среднюю за
        // минуту и переводим в проценты от числа ядер — как `uptime` в консоли.
        if let Some(load) = out("sysctl", &["-n", "vm.loadavg"]).as_deref().and_then(parse_loadavg) {
            v["cpu"] = json!(((load * 100.0 / cores as f64).round() as u64).min(100).to_string());
        }
    }
    #[cfg(not(any(windows, unix)))]
    let _ = data;
    v
}

// Разборщики компилируются везде ради тестов, а зовёт их только unix-ветка.
#[cfg_attr(not(unix), allow(dead_code))]
fn fmt_uptime(secs: u64) -> String {
    format!("{}d {}h {}m", secs / 86400, secs % 86400 / 3600, secs % 3600 / 60)
}

#[cfg(unix)]
fn out(bin: &str, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new(bin).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `{ sec = 1790000000, usec = 123456 } Mon Sep 22 …` → 1790000000.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_boottime(s: &str) -> Option<i64> {
    let rest = s.split("sec = ").nth(1)?;
    rest.split(|c: char| !c.is_ascii_digit()).find(|x| !x.is_empty())?.parse().ok()
}

#[cfg_attr(not(unix), allow(dead_code))]
fn parse_num(s: &str) -> Option<u64> {
    s.trim().parse().ok()
}

/// `vm_stat`: свободной считаем сумму свободных и неактивных страниц —
/// неактивные система отдаёт под нагрузкой не хуже свободных.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_vm_stat_free(s: &str) -> Option<u64> {
    let page = s
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)
        .and_then(|x| x.split(' ').next())
        .and_then(|x| x.parse::<u64>().ok())
        .unwrap_or(4096);
    let pages = |name: &str| -> u64 {
        s.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.rsplit(' ').next())
            .map(|x| x.trim_end_matches('.').parse::<u64>().unwrap_or(0))
            .unwrap_or(0)
    };
    Some((pages("Pages free:") + pages("Pages inactive:") + pages("Pages speculative:")) * page)
}

/// `df -k <путь>`: вторая строка, четвёртая колонка — свободно в килобайтах.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_df_avail(s: &str) -> Option<u64> {
    let line = s.lines().nth(1)?;
    line.split_whitespace().nth(3)?.parse().ok()
}

/// `{ 1.85 2.03 2.11 }` → 1.85.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_loadavg(s: &str) -> Option<f64> {
    s.split_whitespace().find_map(|w| w.trim_matches(|c| c == '{' || c == '}').parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_outputs() {
        assert_eq!(parse_boottime("{ sec = 1790000000, usec = 123456 } Mon Sep 22 20:13:20 2026\n"), Some(1_790_000_000));
        assert_eq!(parse_num(" 17179869184\n"), Some(17_179_869_184));
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                  Pages free:                               100.\n\
                  Pages active:                            2000.\n\
                  Pages inactive:                            50.\n\
                  Pages speculative:                         10.\n";
        assert_eq!(parse_vm_stat_free(vm), Some(160 * 16384));
        let df = "Filesystem 1024-blocks      Used Available Capacity  Mounted on\n\
                  /dev/disk3s5  971350180 320000000 600000000    35%    /System/Volumes/Data\n";
        assert_eq!(parse_df_avail(df), Some(600_000_000));
        assert_eq!(parse_loadavg("{ 1.85 2.03 2.11 }\n"), Some(1.85));
        assert_eq!(fmt_uptime(90_061), "1d 1h 1m");
    }
}
