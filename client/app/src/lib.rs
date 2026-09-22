use std::collections::BTreeMap;

use detour_core::ipc::{Body, Request, Response};

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
    detour_core::ipc::call(&req)
        .await
        .map_err(|e| format!("service_unreachable: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("не удалось запустить окно Detour");
}
