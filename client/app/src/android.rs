//! Мост ядра в андроидную часть приложения.
//!
//! Туннель поднимает `DetourVpnService`: дескриптор TUN система отдаёт только
//! через `VpnService`, поэтому ядро вместо запуска процесса sing-box просит
//! java-сторону, а та кормит конфигом libbox в том же процессе.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use jni::objects::{GlobalRef, JClass, JString, JValue};
use jni::{JNIEnv, JavaVM};

use detour_core::engine::tunnel::Tunnel;

struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Зовётся из `DetourBridge` на старте активности, до запуска ядра.
///
/// Ссылку на класс запоминаем здесь: с примонтированного потока JNI находит
/// только системные классы, а этот — из загрузчика приложения.
#[no_mangle]
pub extern "system" fn Java_io_github_varyen_detour_DetourBridge_nativeInit(
    mut env: JNIEnv,
    class: JClass,
    dpi_binary: JString,
) {
    if let Ok(path) = env.get_string(&dpi_binary) {
        let path: String = path.into();
        if !path.is_empty() {
            std::env::set_var("DETOUR_DPI_BIN", path);
        }
    }
    let (Ok(vm), Ok(class)) = (env.get_java_vm(), env.new_global_ref(&class)) else {
        return;
    };
    let _ = BRIDGE.set(Bridge { vm, class });
    detour_core::engine::tunnel::set(Box::new(AndroidTunnel));
}

fn with_java<T>(f: impl FnOnce(&mut JNIEnv, &JClass) -> Result<T>) -> Result<T> {
    let bridge = BRIDGE.get().context("мост в приложение ещё не поднят")?;
    let mut env = bridge.vm.attach_current_thread()?;
    // SAFETY: глобальная ссылка жива всё время работы процесса.
    let class = unsafe { JClass::from_raw(bridge.class.as_raw()) };
    f(&mut env, &class)
}

struct AndroidTunnel;

impl Tunnel for AndroidTunnel {
    fn start(&self, config: &Path) -> Result<u32> {
        let path = config.to_string_lossy().into_owned();
        let error = with_java(|env, class| {
            let arg = env.new_string(&path)?;
            let value = env.call_static_method(
                class,
                "tunnelStart",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&arg)],
            )?;
            let text: JString = value.l()?.into();
            let out = String::from(env.get_string(&text)?);
            Ok(out)
        })?;
        if error.is_empty() {
            Ok(1)
        } else {
            bail!("{error}")
        }
    }

    fn stop(&self) {
        let _ = with_java(|env, class| {
            env.call_static_method(class, "tunnelStop", "()V", &[])?;
            Ok(())
        });
    }

    fn running(&self) -> bool {
        with_java(|env, class| Ok(env.call_static_method(class, "tunnelRunning", "()Z", &[])?.z()?))
            .unwrap_or(false)
    }

    fn check(&self, config: &Path) -> Result<(), String> {
        let path = config.to_string_lossy().into_owned();
        let error = with_java(|env, class| {
            let arg = env.new_string(&path)?;
            let value = env.call_static_method(
                class,
                "tunnelCheck",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&arg)],
            )?;
            let text: JString = value.l()?.into();
            let out = String::from(env.get_string(&text)?);
            Ok(out)
        })
        .map_err(|e| format!("{e:#}"))?;
        if error.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn version(&self) -> Option<String> {
        with_java(|env, class| {
            let value = env.call_static_method(class, "tunnelVersion", "()Ljava/lang/String;", &[])?;
            let text: JString = value.l()?.into();
            let out = String::from(env.get_string(&text)?);
            Ok(out)
        })
        .ok()
        .filter(|v| !v.is_empty())
    }
}
