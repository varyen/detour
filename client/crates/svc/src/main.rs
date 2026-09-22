mod devhttp;
mod launchd;
#[cfg(windows)]
mod service;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use detour_core::backend::Backend;
use detour_core::ipc::{self, Request};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;

const USAGE: &str = "\
detour-svc run [--data DIR] [--dev-http 127.0.0.1:18080]   передний план (консоль, launchd)
detour-svc install | uninstall                             служба Windows / демон launchd
detour-svc service                                         точка входа для SCM (Windows)";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(windows)]
        Some("service") => service::run(),
        #[cfg(windows)]
        Some("install") => service::install(),
        #[cfg(windows)]
        Some("uninstall") => service::uninstall(),
        #[cfg(target_os = "macos")]
        Some("install") => launchd::install(),
        #[cfg(target_os = "macos")]
        Some("uninstall") => launchd::uninstall(),
        Some("run") => foreground(&args[1..]),
        _ => bail!("{USAGE}"),
    }
}

fn foreground(args: &[String]) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut data = None;
    let mut dev_http = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = Some(PathBuf::from(it.next().context("--data без значения")?)),
            "--dev-http" => {
                let v = it.next().context("--dev-http без значения")?;
                let addr: SocketAddr = v.parse().context("--dev-http: ждём адрес:порт")?;
                if !addr.ip().is_loopback() {
                    bail!("--dev-http слушает только loopback");
                }
                dev_http = Some(addr);
            }
            other => bail!("неизвестный аргумент {other}\n{USAGE}"),
        }
    }

    let data = data.unwrap_or_else(detour_core::paths::data_dir);
    let backend = Arc::new(Backend::new(data)?);
    if dev_http.is_some() {
        backend.forbid_tun();
    }
    let (tx, rx) = watch::channel(false);
    tokio::runtime::Runtime::new()?.block_on(async move {
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = tx.send(true);
        });
        serve(backend, rx, dev_http).await
    })
}

/// launchd останавливает демона SIGTERM, консоль — Ctrl+C.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

pub async fn serve(
    backend: Arc<Backend>,
    mut stop: watch::Receiver<bool>,
    dev_http: Option<SocketAddr>,
) -> Result<()> {
    let mut listener = ipc::Listener::bind()
        .with_context(|| format!("не удалось открыть {}", ipc::ENDPOINT))?;
    tracing::info!(endpoint = ipc::ENDPOINT, data = %backend.data_dir().display(), "служба слушает");

    if let Some(addr) = dev_http {
        tokio::spawn(devhttp::serve(backend.clone(), addr));
    }
    tokio::spawn(backend.clone().run_background());

    loop {
        tokio::select! {
            conn = listener.accept() => match conn {
                Ok(conn) => {
                    let b = backend.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_conn(b, conn).await {
                            tracing::debug!(error = %e, "клиент отвалился");
                        }
                    });
                }
                Err(e) => tracing::warn!(error = %e, "accept"),
            },
            _ = stop.changed() => break,
        }
    }
    tracing::info!("остановка");
    backend.shutdown().await;
    Ok(())
}

async fn handle_conn<S>(backend: Arc<Backend>, mut conn: S) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(req) = ipc::read_frame::<_, Request>(&mut conn).await? {
        let resp = backend.handle(req).await;
        ipc::write_frame(&mut conn, &resp).await?;
    }
    Ok(())
}
