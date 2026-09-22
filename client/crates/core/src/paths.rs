use std::path::PathBuf;

/// Каталог данных службы. `DETOUR_DATA` переопределяет его для разработки,
/// чтобы консольный запуск без прав не лез в `%ProgramData%`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DETOUR_DATA") {
        return d.into();
    }
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("Detour")
}
