// ! Cloud apps that sync a local folder: where their folder is, and starting ! the app when its files (or its drive) are needed. Shared by Game Saves and ! Project Backups; no cloud API or account is ever used.
pub(crate) mod detection;
mod google_drive;
pub(crate) mod launch;
pub(crate) mod onedrive;
pub(crate) mod online_only;
mod sync_roots;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
/// Cloud storage whose desktop app syncs a local folder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloudProvider {
    Dropbox,
    GoogleDrive,
    Mega,
    OneDrive,
}
impl CloudProvider {
    pub fn name(self) -> &'static str {
        match self {
            CloudProvider::Dropbox => "Dropbox",
            CloudProvider::GoogleDrive => "Google Drive",
            CloudProvider::Mega => "MEGA",
            CloudProvider::OneDrive => "OneDrive",
        }
    }
}
/// A folder a cloud app keeps in sync.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CloudFolder {
    pub provider: CloudProvider,
    /// Which account, when a provider has several ("Dropbox Business").
    pub label: String,
    pub path: PathBuf,
}
