use std::path::Path;

use serde_json::{json, Value};

/// Блок `system` статуса. `cpu` — загрузка между двумя соседними вызовами
/// (панель опрашивает статус постоянно); первый вызов отдаёт `?`, как роутер
/// без данных.
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
        v["uptime"] = json!(format!("{}d {}h {}m", secs / 86400, secs % 86400 / 3600, secs % 3600 / 60));
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
    #[cfg(not(windows))]
    let _ = data;
    v
}
