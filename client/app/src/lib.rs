use std::collections::BTreeMap;

use detour_core::ipc::{Body, Request, Response};

#[cfg(target_os = "android")]
mod android;

/// На десктопе привилегированная часть — отдельная служба, и панель ходит в
/// неё по каналу. На Android службы нет: система не даст держать демона, а TUN
/// открывается только из процесса приложения, — поэтому ядро работает прямо
/// здесь, а «канал» вырождается в вызов функции.
#[cfg(target_os = "android")]
mod inproc {
    use std::sync::{Arc, OnceLock};

    use detour_core::backend::Backend;
    use detour_core::ipc::{Request, Response};

    static BACKEND: OnceLock<Arc<Backend>> = OnceLock::new();

    pub fn init(data: std::path::PathBuf) -> anyhow::Result<()> {
        let backend = Arc::new(Backend::new(data)?);
        let _ = BACKEND.set(backend.clone());
        tauri::async_runtime::spawn(async move {
            backend.boot().await;
            backend.run_background().await;
        });
        Ok(())
    }

    pub async fn call(req: &Request) -> Result<Response, String> {
        let Some(b) = BACKEND.get() else {
            return Err("ядро ещё не готово".to_owned());
        };
        Ok(b.handle(req.clone()).await)
    }
}

/// Единственная команда: панель шлёт сюда то же, что на роутере ушло бы в
/// `/cgi-bin/detour-api`, и получает статус и тело ответа.
#[tauri::command]
async fn api(
    action: String,
    params: Option<BTreeMap<String, String>>,
    body: Option<Body>,
) -> Result<Response, String> {
    let req = Request {
        action,
        params: params.unwrap_or_default(),
        body,
    };
    #[cfg(target_os = "android")]
    {
        inproc::call(&req).await
    }
    #[cfg(not(target_os = "android"))]
    {
        detour_core::ipc::call(&req)
            .await
            .map_err(|e| format!("service_unreachable: {e}"))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![api]);
    #[cfg(target_os = "android")]
    let builder = builder.setup(|app| {
        use tauri::Manager;
        // Каталог приложения — единственное место, куда можно писать без прав.
        let dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&dir)?;
        inproc::init(dir)?;
        Ok(())
    });
    builder
        .run(tauri::generate_context!())
        .expect("не удалось запустить окно Detour");
}
