// ! The folders Windows lists as cloud sync roots: OneDrive, Dropbox, iCloud, ! Box, Nextcloud, Proton Drive, and any other app built on the Windows cloud ! files API. Their files are made online-only the same way, whatever the app.
use std::path::PathBuf;
use windows::Storage::IStorageItem;
use windows::Storage::Provider::StorageProviderSyncRootManager;
use windows::Win32::UI::Shell::SHLoadIndirectString;
use windows::core::{HSTRING, Interface};
/// A folder a cloud app registered with Windows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SyncRoot {
    /// The app, as its Windows ID begins: "OneDrive", "Dropbox", "iCloudDrive"…
    pub provider: String,
    /// What Explorer calls the folder: "OneDrive - Personal".
    pub name: String,
    pub path: PathBuf,
}
/// The sync roots of this user. Empty when Windows can't list them.
pub(crate) fn sync_roots() -> Vec<SyncRoot> {
    let Ok(roots) = StorageProviderSyncRootManager::GetCurrentSyncRoots() else {
        return Vec::new();
    };
    roots
        .into_iter()
        .filter_map(|root| {
            // IDs read "Provider!SID!Account" (Microsoft's documented form).
            let id = root.Id().ok()?.to_string();
            let provider = id.split('!').next()?.trim().to_string();
            let path = root.Path().ok()?.cast::<IStorageItem>().ok()?.Path().ok()?;
            if provider.is_empty() || path.is_empty() {
                return None;
            }
            let name = root
                .DisplayNameResource()
                .ok()
                .map(|name| display_name(&name))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| provider.clone());
            Some(SyncRoot {
                provider,
                name,
                path: PathBuf::from(path.to_os_string()),
            })
        })
        .collect()
}
/// A display name as given, or read from the app's resources (`@file,-id`).
fn display_name(raw: &HSTRING) -> String {
    let text = raw.to_string_lossy();
    if !text.starts_with('@') {
        return text.trim().to_string();
    }
    let mut buffer = [0u16; 260];
    // SAFETY: `raw` is NUL-terminated; the buffer's length is passed with it.
    match unsafe { SHLoadIndirectString(raw, &mut buffer, None) } {
        Ok(()) => {
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            String::from_utf16_lossy(&buffer[..end]).trim().to_string()
        }
        Err(_) => String::new(),
    }
}
