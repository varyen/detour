//! Только для разработки: тот же `/cgi-bin/detour-api`, что на роутере, но на
//! loopback. Панель в обычном браузере (`vite --mode desktop` с
//! `DETOUR_DEV_TARGET`) и Playwright ходят в службу без WebView.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine as _;
use detour_core::backend::Backend;
use detour_core::ipc::{Body, Request};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const MAX_BODY: usize = 64 << 20;

pub async fn serve(backend: Arc<Backend>, addr: SocketAddr) {
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(%addr, error = %e, "dev-http не поднялся");
            return;
        }
    };
    tracing::info!("dev-http: http://{addr}/cgi-bin/detour-api");
    loop {
        let Ok((sock, _)) = listener.accept().await else { continue };
        let b = backend.clone();
        tokio::spawn(async move {
            if let Err(e) = conn(b, sock).await {
                tracing::debug!(error = %e, "dev-http");
            }
        });
    }
}

async fn conn(backend: Arc<Backend>, sock: TcpStream) -> std::io::Result<()> {
    let mut r = BufReader::new(sock);
    let mut line = String::new();
    r.read_line(&mut line).await?;
    let mut head = line.split_whitespace();
    let is_post = head.next() == Some("POST");
    let target = head.next().unwrap_or("/").to_owned();

    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).await? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut raw = vec![0u8; len.min(MAX_BODY)];
    r.read_exact(&mut raw).await?;

    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let (status, body) = if path == "/cgi-bin/detour-api" {
        let mut params: BTreeMap<String, String> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (decode(k), decode(v))
            })
            .collect();
        let action = params.remove("action").unwrap_or_default();
        // Пустой POST — это тоже тело (очистить список), а не чтение.
        let body = is_post.then(|| match String::from_utf8(raw) {
            Ok(s) => Body::Text(s),
            Err(e) => Body::Base64(base64::engine::general_purpose::STANDARD.encode(e.as_bytes())),
        });
        let resp = backend.handle(Request { action, params, body }).await;
        (resp.status, resp.body)
    } else {
        (404, r#"{"ok":false,"error":"not_found"}"#.into())
    };

    let mut sock = r.into_inner();
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        if status < 400 { "OK" } else { "Error" },
        body.len()
    );
    sock.write_all(head.as_bytes()).await?;
    sock.write_all(body.as_bytes()).await?;
    sock.shutdown().await
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
