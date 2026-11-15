//! "Show ThomasT as the publisher" in Settings › About. The setup offers it on
//! its last page, but an update runs the setup passively or silently and never
//! shows that page, so a copy that came by update is offered it here. The
//! certificate and how it is trusted are the setup's own code, the same file.
use serde::Serialize;

// `forget` is the uninstaller's; the app only offers and checks.
#[allow(dead_code)]
#[path = "../installer/src/publisher.rs"]
mod publisher;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherState {
    name: &'static str,
    /// Windows already trusts it here, for this user or every user.
    trusted: bool,
}

/// The offer, or `None` when this copy of MYLE isn't signed with ThomasT's
/// certificate (a release from before it, or a build that isn't signed):
/// trusting it would change nothing Windows shows.
#[tauri::command(async)]
pub fn publisher_get() -> Option<PublisherState> {
    let exe = std::env::current_exe().ok()?;
    publisher::signs(&exe).then(|| PublisherState {
        name: publisher::NAME,
        trusted: publisher::is_trusted(),
    })
}

/// Trusts the certificate for this Windows user. Windows asks first, in a
/// window of its own; `false` when the user said no.
#[tauri::command]
pub async fn publisher_trust() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(publisher::trust)
        .await
        .map_err(|error| error.to_string())?
}
