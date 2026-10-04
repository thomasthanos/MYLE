//! HTTP clients with this platform's certificates.

use std::time::Duration;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A client builder that checks certificates the platform's way: Windows'
/// own store in the Windows app; on Android, Mozilla's root certificates
/// (Android's own check needs Java set up first); on iOS, the system's.
pub fn builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .filter_map(|cert| reqwest::Certificate::from_der(cert).ok()),
    );
    builder
}

/// A client for the account and its sync.
pub fn client(user_agent: &str) -> Result<reqwest::Client, String> {
    builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(err)
}
