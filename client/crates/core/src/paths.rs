use std::path::PathBuf;

/// Каталог данных службы. `DETOUR_DATA` переопределяет его для разработки,
/// чтобы консольный запуск без прав не лез в системный каталог.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DETOUR_DATA") {
        return d.into();
    }
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("Detour")
    }
    // Демон работает под root, и данные лежат там же, где у системных служб.
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/Detour")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        PathBuf::from("/var/lib/detour")
    }
}
