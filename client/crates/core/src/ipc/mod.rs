//! Протокол между интерфейсом (работает без прав) и службой (SYSTEM / root):
//! кадры `u32 LE длина + JSON`. Запрос и ответ повторяют CGI роутера —
//! action, query-параметры, сырое тело и HTTP-статус, — чтобы панель не знала,
//! с кем говорит. Транспорт: named pipe на Windows, unix-сокет на macOS/Linux.
//! На Android службы нет — ядро работает в процессе приложения, и этот модуль
//! там нужен только ради типов.

use std::collections::BTreeMap;
use std::io;

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{call, Listener, ENDPOINT};

#[cfg(all(unix, not(target_os = "android")))]
mod unix;
#[cfg(all(unix, not(target_os = "android")))]
pub use unix::{call, Listener, ENDPOINT};

const MAX_FRAME: u32 = 64 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
}

impl Request {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "lowercase")]
pub enum Body {
    Text(String),
    Base64(String),
}

impl Body {
    pub fn bytes(&self) -> anyhow::Result<Vec<u8>> {
        Ok(match self {
            Body::Text(s) => s.as_bytes().to_vec(),
            Body::Base64(s) => base64::engine::general_purpose::STANDARD.decode(s)?,
        })
    }

    pub fn text(&self) -> anyhow::Result<String> {
        Ok(match self {
            Body::Text(s) => s.clone(),
            Body::Base64(_) => String::from_utf8(self.bytes()?)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn json<T: Serialize + ?Sized>(v: &T) -> Self {
        Self {
            status: 200,
            body: serde_json::to_string(v).unwrap_or_else(|_| "{}".into()),
        }
    }

    /// Голый текст без JSON-конверта: панель читает так `bypass_strategy`
    /// (`requestRawText`), и обёртка уехала бы в редактор как есть.
    pub fn text(s: &str) -> Self {
        Self { status: 200, body: s.to_owned() }
    }

    /// Конверт `{ok:false,error}` — так CGI сообщает об ошибке, и панель
    /// переводит код в понятный текст (`translateApiError`).
    pub fn error(code: &str) -> Self {
        Self::json(&serde_json::json!({ "ok": false, "error": code }))
    }
}

pub async fn write_frame<W, T>(w: &mut W, v: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let buf = serde_json::to_vec(v).map_err(io::Error::other)?;
    let len = u32::try_from(buf.len())
        .ok()
        .filter(|&n| n <= MAX_FRAME)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "кадр слишком большой"))?;
    w.write_all(&len.to_le_bytes()).await?;
    w.write_all(&buf).await?;
    w.flush().await
}

/// `None` — собеседник закрыл канал между кадрами, это штатный конец.
pub async fn read_frame<R, T>(r: &mut R) -> io::Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len);
    if n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "кадр слишком большой"));
    }
    let mut buf = vec![0u8; n as usize];
    r.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Один запрос — одно соединение: интерфейс шлёт их редко, а так не нужно
/// мультиплексировать ответы.
#[cfg(not(target_os = "android"))]
async fn roundtrip<S>(mut conn: S, req: &Request) -> io::Result<Response>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_frame(&mut conn, req).await?;
    read_frame(&mut conn)
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "служба закрыла соединение"))
}
