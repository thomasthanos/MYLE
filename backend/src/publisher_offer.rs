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
    /// "What's new" after an update has offered it once already; Settings
    /// keeps offering it.
    offered: bool,
    /// Trusted for every account on this PC, which Windows' administrator
    /// prompt needs to name the publisher.
    machine: bool,
}

/// Marks that "What's new" offered it, so it does that once.
fn offered_marker() -> Option<std::path::PathBuf> {
    Some(crate::storage::local_dir().ok()?.join("internal").join("publisher-offered"))
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
        offered: offered_marker().is_some_and(|marker| marker.is_file()),
        machine: publisher::machine_trusted(),
    })
}

/// Trusts the certificate for every account on this PC, so the administrator
/// prompt names the publisher too. Windows asks for administrator rights;
/// `false` when the user said no.
#[tauri::command]
pub async fn publisher_trust_machine() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(publisher::trust_machine)
        .await
        .map_err(|error| error.to_string())?
}

/// Remembers that "What's new" offered it.
#[tauri::command(async)]
pub fn publisher_offered() -> Result<(), String> {
    let marker = offered_marker().ok_or("No app data folder.")?;
    if let Some(folder) = marker.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    std::fs::write(&marker, []).map_err(|error| error.to_string())
}

/// Trusts the certificate for this Windows user. Windows asks first, in a
/// window of its own; `false` when the user said no.
#[tauri::command]
pub async fn publisher_trust() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(publisher::trust)
        .await
        .map_err(|error| error.to_string())?
}
