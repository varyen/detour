use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

use super::{Request, Response};

pub const ENDPOINT: &str = "/var/run/detour.sock";

pub async fn call(req: &Request) -> io::Result<Response> {
    super::roundtrip(UnixStream::connect(ENDPOINT).await?, req).await
}

pub struct Listener(UnixListener);

impl Listener {
    /// Сокет остаётся от упавшего демона — bind на существующий путь падает,
    /// поэтому старый файл убираем. Демон под root, а интерфейс — под
    /// пользователем: доступ даём группе, в которой состоят обычные учётки
    /// (`staff`, gid 20 на любом macOS).
    pub fn bind() -> io::Result<Self> {
        let path = Path::new(ENDPOINT);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let l = UnixListener::bind(path)?;
        #[cfg(target_os = "macos")]
        std::os::unix::fs::chown(path, None, Some(20))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
        Ok(Self(l))
    }

    pub async fn accept(&mut self) -> io::Result<UnixStream> {
        self.0.accept().await.map(|(s, _)| s)
    }
}
