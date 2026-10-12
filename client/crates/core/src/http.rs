//! Общий конструктор HTTP-клиента.
//!
//! reqwest 0.13 с фичей `rustls` проверяет сертификаты через
//! rustls-platform-verifier. На Android тому нужна инициализация из JNI
//! (Context + Kotlin-часть из AAR), без неё каждый HTTPS-запрос паникует
//! «Expect rustls-platform-verifier to be initialized». Паника роняла поток
//! tokio: «Проверить» в 2.10.0 крутилось до таймаута, а подписки, гео-база и
//! прочие загрузки на телефоне не работали вовсе. На Android берём корни из
//! системного хранилища сертификатов напрямую — платформенный верификатор
//! тогда не создаётся.

pub fn client_builder() -> reqwest::ClientBuilder {
    let b = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let b = b.tls_certs_only(android_roots().iter().cloned());
    b
}

#[cfg(target_os = "android")]
fn android_roots() -> &'static [reqwest::Certificate] {
    use std::sync::OnceLock;
    static ROOTS: OnceLock<Vec<reqwest::Certificate>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        // С Android 14 хранилище обновляется модулем conscrypt, старый каталог
        // остаётся на месте, но может отставать.
        for dir in ["/apex/com.android.conscrypt/cacerts", "/system/etc/security/cacerts"] {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            let certs: Vec<_> = rd
                .flatten()
                .filter_map(|e| std::fs::read(e.path()).ok())
                .filter_map(|pem| reqwest::Certificate::from_pem(&pem).ok())
                .collect();
            if !certs.is_empty() {
                tracing::info!("TLS: {} корневых сертификатов из {dir}", certs.len());
                return certs;
            }
        }
        tracing::error!("TLS: системное хранилище сертификатов не найдено");
        Vec::new()
    })
}
