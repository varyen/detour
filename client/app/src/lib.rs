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
        let Some(b) = BACKEND.get().cloned() else {
            return Err("ядро ещё не готово".to_owned());
        };
        // Отдельной задачей: паника внутри обработчика иначе не возвращала
        // ответа вовсе, и панель висела на спиннере до своего таймаута.
        let req = req.clone();
        let action = req.action.clone();
        tauri::async_runtime::spawn(async move { b.handle(req).await })
            .await
            .map_err(|e| format!("ядро упало на «{action}»: {e}"))
    }
}

/// Единственная команда: панель шлёт сюда то же, что на роутере ушло бы в
/// `/cgi-bin/detour-api`, и получает статус и тело ответа.
#[tauri::command]
async fn api(
    app: tauri::AppHandle,
    action: String,
    params: Option<BTreeMap<String, String>>,
    body: Option<Body>,
) -> Result<Response, String> {
    let req = Request {
        action,
        params: params.unwrap_or_default(),
        body,
    };
    if req.action == "app_update_install" {
        return app_update_install(app, &req).await;
    }
    core_call(&req).await
}

/// Сохранение файла из панели. Нужно только на Android: там WebView молча
/// игнорирует загрузку blob-ссылки. Остальным платформам — `unsupported`,
/// и панель скачивает файл как в браузере.
#[tauri::command]
async fn save_file(name: String, content: String, mime: Option<String>) -> Result<String, String> {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let _ = (&name, &content, &mime);
    #[cfg(target_os = "android")]
    {
        let mime = mime.unwrap_or_else(|| "application/octet-stream".into());
        return tauri::async_runtime::spawn_blocking(move || android::save_file(&name, &content, &mime))
            .await
            .map_err(|e| e.to_string())?;
    }
    #[allow(unreachable_code)]
    Err("unsupported".into())
}

async fn core_call(req: &Request) -> Result<Response, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        inproc::call(req).await
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        detour_core::ipc::call(req)
            .await
            .map_err(|e| format!("service_unreachable: {e}"))
    }
}

/// Установка скачанной ядром сборки. На Windows и Android ставить может
/// только окно: установщику нужны права пользователя (UAC) или системный
/// диалог. Подпись сверяется здесь ещё раз — файл лежит вне окна.
async fn app_update_install(app: tauri::AppHandle, req: &Request) -> Result<Response, String> {
    let _ = (&app, req);
    #[cfg(target_os = "macos")]
    {
        // Подменяет бандл демон (root); окно потом перезапускается само.
        let r = core_call(req).await?;
        if r.status == 200 && r.body.contains("\"restart\":true") {
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                app.restart();
            });
        }
        return Ok(r);
    }
    #[cfg(any(target_os = "windows", target_os = "android"))]
    {
        let status = core_call(&Request {
            action: "panel_update_status".into(),
            params: Default::default(),
            body: None,
        })
        .await?;
        let st: serde_json::Value = serde_json::from_str(&status.body).unwrap_or_default();
        let Some(path) = st["downloaded"].as_str().map(std::path::PathBuf::from) else {
            return Ok(err_json("сборка ещё не скачана — нажмите «Обновить» ещё раз"));
        };
        if let Err(e) = detour_core::appupdate::verify_file(&path) {
            return Ok(err_json(&format!("{e:#}")));
        }
        #[cfg(target_os = "windows")]
        let res = launch_installer(path).await;
        #[cfg(target_os = "android")]
        let res = android::install_apk(&path);
        return Ok(match res {
            Ok(()) => Response::json(&serde_json::json!({ "ok": true })),
            Err(e) => err_json(&e),
        });
    }
    #[allow(unreachable_code)]
    Ok(err_json("на этой платформе приложение обновляется не из панели"))
}

#[allow(dead_code)]
fn err_json(msg: &str) -> Response {
    Response::json(&serde_json::json!({ "ok": false, "error": msg }))
}

/// Установщик NSIS с `/S` и запросом прав. Start-Process возвращается сразу
/// после запуска; ненулевой код — человек отказал в UAC. Окно установщик
/// закроет сам и откроет заново после установки.
#[cfg(target_os = "windows")]
async fn launch_installer(path: std::path::PathBuf) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let p = path.to_string_lossy().replace('\'', "''");
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command"])
            .arg(format!("Start-Process -FilePath '{p}' -ArgumentList '/S' -Verb RunAs"))
            .creation_flags(0x0800_0000)
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("не удалось запустить установщик: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err("установка отменена: без прав администратора Windows её не пустит".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![api, save_file]);
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
