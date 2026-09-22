use std::ffi::c_void;
use std::io;
use std::time::{Duration, Instant};

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_PIPE_BUSY};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

use super::{Request, Response};

pub const ENDPOINT: &str = r"\\.\pipe\detour";

/// SYSTEM и администраторы — полный доступ, интерактивные пользователи — чтение
/// и запись. Без явного DACL служба под SYSTEM отдала бы обычному
/// пользователю только чтение, и интерфейс не смог бы отправить запрос.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

pub async fn call(req: &Request) -> io::Result<Response> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let pipe = loop {
        match ClientOptions::new().open(ENDPOINT) {
            Ok(c) => break c,
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    };
    super::roundtrip(pipe, req).await
}

struct SecurityDescriptor(*mut c_void);

// Дескриптор только читается ядром при создании экземпляра канала.
unsafe impl Send for SecurityDescriptor {}
unsafe impl Sync for SecurityDescriptor {}

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: *mut c_void = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

pub struct Listener {
    sd: SecurityDescriptor,
    next: NamedPipeServer,
}

impl Listener {
    pub fn bind() -> io::Result<Self> {
        let sd = SecurityDescriptor::from_sddl(PIPE_SDDL)?;
        let next = create_instance(&sd, true)?;
        Ok(Self { sd, next })
    }

    /// Ждёт клиента и сразу создаёт следующий экземпляр, чтобы между
    /// подключениями канал не пропадал из пространства имён.
    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        let res = self.next.connect().await;
        let fresh = create_instance(&self.sd, false)?;
        let conn = std::mem::replace(&mut self.next, fresh);
        res.map(|_| conn)
    }
}

fn create_instance(sd: &SecurityDescriptor, first: bool) -> io::Result<NamedPipeServer> {
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(ENDPOINT, &mut sa as *mut _ as *mut c_void)
    }
}
