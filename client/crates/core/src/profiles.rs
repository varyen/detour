//! Профили — `profiles/<id>.json`, один файл на профиль, схема как на роутере.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::ids;
use crate::store::{self, Store};

fn rel(id: &str) -> String {
    format!("{}/{id}.json", store::PROFILES_DIR)
}

/// id в порядке имён файлов — у роутера это порядок glob `*.json`.
pub fn list_ids(store: &Store) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(store.path(store::PROFILES_DIR)) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_suffix(".json").map(str::to_owned)
        })
        .filter(|id| !id.is_empty() && ids::sanitize(id) == *id)
        .collect();
    ids.sort();
    ids
}

pub fn exists(store: &Store, id: &str) -> bool {
    !id.is_empty() && store.exists(&rel(id))
}

pub fn load(store: &Store, id: &str) -> Option<Value> {
    if id.is_empty() {
        return None;
    }
    store.read_json(&rel(id))
}

pub fn load_raw(store: &Store, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    std::fs::read_to_string(store.path(&rel(id))).ok()
}

pub fn outbound(profile: &Value) -> Option<&Value> {
    profile.get("outbound").filter(|o| o.is_object())
}

/// Тип для списка и выбора логики маршрута: `http` → `http-proxy` /
/// `https-proxy` по TLS, `socks` → `socks<версия>`, иначе тип outbound.
pub fn infer_type(outbound: Option<&Value>) -> String {
    let Some(ob) = outbound else { return "unknown".into() };
    let t = ob.get("type").and_then(Value::as_str).unwrap_or("");
    match t {
        "" => "unknown".into(),
        "http" => {
            let tls = ob
                .pointer("/tls/enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if tls { "https-proxy" } else { "http-proxy" }.into()
        }
        "socks" => {
            let v = ob.get("version").and_then(Value::as_str).unwrap_or("5");
            format!("socks{v}")
        }
        other => other.into(),
    }
}

pub fn profile_type(profile: &Value) -> String {
    match profile.get("type").and_then(Value::as_str) {
        Some(t) if !t.is_empty() => t.into(),
        _ => infer_type(outbound(profile)),
    }
}

fn has_server(v: &Value) -> bool {
    serde_json::to_string(v)
        .map(|s| s.contains("\"server\":"))
        .unwrap_or(false)
}

/// Полная перезапись профиля. Имя файла — из `id`, иначе из `name`; `type`
/// всегда пересчитывается из outbound. Отказ, если сохранение потеряло бы
/// параметры подключения: так сработала бы смена папки поверх профиля,
/// прочитанного не целиком.
pub fn save(store: &Store, mut body: Value) -> Result<String> {
    let obj = body.as_object_mut().ok_or_else(|| anyhow!("invalid json"))?;
    let raw_id = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| obj.get("name").and_then(Value::as_str))
        .unwrap_or("");
    let id = ids::sanitize(raw_id);
    if id.is_empty() {
        bail!("id required");
    }
    obj.insert("id".into(), Value::String(id.clone()));
    let t = infer_type(obj.get("outbound"));
    obj.insert("type".into(), Value::String(t));

    if let Some(old) = load(store, &id) {
        let has_uri = obj
            .get("uri")
            .and_then(Value::as_str)
            .is_some_and(|u| !u.is_empty());
        if has_server(&old) && !has_server(&body) && !has_uri {
            bail!("профиль потерял бы параметры подключения — сохранение отменено");
        }
    }
    store.write_json(&rel(&id), &body)?;
    Ok(id)
}

pub fn delete(store: &Store, id: &str) -> Result<()> {
    std::fs::remove_file(store.path(&rel(id))).map_err(|_| anyhow!("profile not found"))
}
