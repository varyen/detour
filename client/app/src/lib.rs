use std::collections::BTreeMap;

use detour_core::ipc::{Body, Request, Response};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;

/// На десктопе привилегированная часть — отдельная служба, и панель ходит в
/// неё по каналу. На телефонах службы нет: система не даст держать демона, —
/// поэтому ядро работает прямо здесь, а «канал» вырождается в вызов функции.
/// Туннель поднимает платформа: VpnService на Android, расширение на iOS.
#[cfg(any(target_os = "android", target_os = "ios"))]
mod inproc {
    use std::sync::{Arc, OnceLock};

    use detour_core::backend::Backend;
    use detour_core::ipc::{Request, Response};

    static BACKEND: OnceLock<Arc<Backend>> = OnceLock::new();

    pub fn init(data: std::path::PathBuf) -> anyhow::Result<()> {
        let backend = Arc::new(Backend::new(data)?);
        let _ = BACKEND.set(backend.clone());
        tauri::async_runtime::spawn(async move {
            // boot() — первым шагом run_background.
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
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        inproc::call(&req).await
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        detour_core::ipc::call(&req)
            .await
            .map_err(|e| format!("service_unreachable: {e}"))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![api]);
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.setup(|app| {
        use tauri::Manager;
        // Каталог приложения — единственное место, куда можно писать без прав.
        let dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&dir)?;
        // На Android мост ставит java-сторона в nativeInit, на iOS звать
        // некого — ставим сами.
        #[cfg(target_os = "ios")]
        ios::install();
        inproc::init(dir)?;
        Ok(())
    });
    builder
        .run(tauri::generate_context!())
        .expect("не удалось запустить окно Detour");
}
