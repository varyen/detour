use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use detour_core::backend::Backend;
use tokio::sync::watch;
use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

const SERVICE_NAME: &str = "DetourSvc";
const DISPLAY_NAME: &str = "Detour";
const DESCRIPTION: &str = "Маршрутизация трафика через VPN и обход DPI (sing-box, winws2)";

define_windows_service!(ffi_service_main, service_main);

pub fn run() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).context("service_dispatcher")?;
    Ok(())
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!(error = %format!("{e:#}"), "служба упала");
    }
}

fn run_service() -> Result<()> {
    let data = detour_core::paths::data_dir();
    init_file_log(&data)?;

    let (tx, rx) = watch::channel(false);
    let handler = move |ev| match ev {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = tx.send(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(SERVICE_NAME, handler)?;
    let set = |state, accept| {
        status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
    };

    set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN)?;
    let res = (|| {
        let backend = Arc::new(Backend::new(data)?);
        tokio::runtime::Runtime::new()?.block_on(super::serve(backend, rx, None))
    })();
    set(ServiceState::Stopped, ServiceControlAccept::empty())?;
    res
}

fn init_file_log(data: &std::path::Path) -> Result<()> {
    let dir = data.join("logs");
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("svc.log"))?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .init();
    Ok(())
}

pub fn install() -> Result<()> {
    let mgr = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .context("нужны права администратора")?;
    let info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: DISPLAY_NAME.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec!["service".into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let svc = mgr.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
    svc.set_description(DESCRIPTION)?;
    svc.start::<&str>(&[])?;
    println!("служба {SERVICE_NAME} установлена и запущена");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("нужны права администратора")?;
    let svc = mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    if svc.query_status()?.current_state != ServiceState::Stopped {
        svc.stop()?;
        for _ in 0..50 {
            if svc.query_status()?.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    svc.delete()?;
    println!("служба {SERVICE_NAME} удалена");
    Ok(())
}
