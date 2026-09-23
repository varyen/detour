//! Мост ядра в iOS-часть приложения. НЕ СОБИРАЛОСЬ: для iOS нужен Xcode с
//! iOS SDK, а под рукой только Command Line Tools (см. client/app/ios/README.md).
//!
//! Туннель на iOS живёт в отдельном процессе — расширении
//! `NEPacketTunnelProvider` с libbox внутри. Приложение им только управляет
//! через `NETunnelProviderManager`; эти вызовы — в Swift (`DetourTunnel.swift`),
//! откуда они торчат наружу C-функциями через `@_cdecl`, а линковщик Xcode
//! сводит их с этими объявлениями.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;

use anyhow::{bail, Result};

use detour_core::engine::tunnel::Tunnel;

extern "C" {
    /// Пустой указатель — туннель поднят, иначе текст ошибки (освобождать
    /// через `detour_string_free`).
    fn detour_tunnel_start(config: *const c_char) -> *mut c_char;
    fn detour_tunnel_stop();
    fn detour_tunnel_running() -> bool;
    fn detour_tunnel_check(config: *const c_char) -> *mut c_char;
    fn detour_tunnel_version() -> *mut c_char;
    fn detour_string_free(s: *mut c_char);
}

/// Строка из Swift в Rust с освобождением на той стороне, где выделяли.
fn take(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Swift отдаёт strdup-строку и не трогает её до free.
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { detour_string_free(ptr) };
    Some(s)
}

/// Расширение читает конфиг не с диска приложения (у него своя песочница), а
/// из параметров запуска — поэтому передаём содержимое, а не путь.
fn content(config: &Path) -> Result<CString> {
    Ok(CString::new(std::fs::read(config)?)?)
}

struct IosTunnel;

impl Tunnel for IosTunnel {
    fn start(&self, config: &Path) -> Result<u32> {
        let text = content(config)?;
        match take(unsafe { detour_tunnel_start(text.as_ptr()) }) {
            None => Ok(1),
            Some(e) => bail!("{e}"),
        }
    }

    fn stop(&self) {
        unsafe { detour_tunnel_stop() }
    }

    fn running(&self) -> bool {
        unsafe { detour_tunnel_running() }
    }

    fn check(&self, config: &Path) -> Result<(), String> {
        let text = content(config).map_err(|e| format!("{e:#}"))?;
        match take(unsafe { detour_tunnel_check(text.as_ptr()) }) {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }

    fn version(&self) -> Option<String> {
        take(unsafe { detour_tunnel_version() }).filter(|v| !v.is_empty())
    }
}

pub fn install() {
    detour_core::engine::tunnel::set(Box::new(IosTunnel));
}
