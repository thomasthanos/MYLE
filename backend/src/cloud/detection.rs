//! The local folders that cloud apps (Dropbox, Google Drive for desktop,
//! MEGA, OneDrive) keep in sync, found without any cloud API or account.
//! Shared by Game Saves and Project Backups.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};

use super::{CloudFolder, CloudProvider};

/// The synced folders of the cloud apps installed for this user.
pub(crate) fn detect_folders() -> Vec<CloudFolder> {
    let mut found = Vec::new();
    detect_onedrive(&mut found);
    detect_dropbox(&mut found);
    detect_google_drive(&mut found);
    detect_mega(&mut found);

    found.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then(a.label.cmp(&b.label))
            .then(a.path.cmp(&b.path))
    });
    // OneDrive is usually reported twice (`OneDrive` and `OneDriveConsumer`),
    // and every source below may find the same folder again.
    let mut unique: Vec<CloudFolder> = Vec::with_capacity(found.len());
    for folder in found {
        if !unique
            .iter()
            .any(|known| same_folder(&known.path, &folder.path))
        {
            unique.push(folder);
        }
    }
    unique
}

fn detect_onedrive(out: &mut Vec<CloudFolder>) {
    for (label, variable) in [
        ("OneDrive", "OneDrive"),
        ("OneDrive Personal", "OneDriveConsumer"),
        ("OneDrive Business", "OneDriveCommercial"),
    ] {
        if let Some(path) = std::env::var_os(variable) {
            push_cloud(out, CloudProvider::OneDrive, label, Path::new(&path));
        }
    }
}

/// Dropbox writes its folders to `info.json`: in `%LOCALAPPDATA%` since
/// 2019, in `%APPDATA%` before that.
fn detect_dropbox(out: &mut Vec<CloudFolder>) {
    for variable in ["LOCALAPPDATA", "APPDATA"] {
        let Some(base) = std::env::var_os(variable) else {
            continue;
        };
        let info = Path::new(&base).join("Dropbox").join("info.json");
        let Ok(text) = fs::read_to_string(info) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for (key, label) in [("personal", "Dropbox"), ("business", "Dropbox Business")] {
            if let Some(path) = json
                .get(key)
                .and_then(|entry| entry.get("path"))
                .and_then(Value::as_str)
            {
                push_cloud(out, CloudProvider::Dropbox, label, Path::new(path));
            }
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        push_cloud(
            out,
            CloudProvider::Dropbox,
            "Dropbox",
            &Path::new(&profile).join("Dropbox"),
        );
    }
}

/// "My Drive" as Google Drive names it in the languages it ships in most.
const MY_DRIVE: &[&str] = &[
    "My Drive",
    "MyDrive",
    "Ο Δίσκος μου",
    "Meine Ablage",
    "Mon Drive",
    "Mi unidad",
    "Il mio Drive",
    "Meu Drive",
    "Mijn Drive",
    "Mój dysk",
    "Мой диск",
];

/// Drive for desktop shows the account as a drive of its own (usually
/// `G:\My Drive`) or, in mirror mode, as a folder in the user's profile.
/// The older Backup and Sync used `%USERPROFILE%\Google Drive`.
fn detect_google_drive(out: &mut Vec<CloudFolder>) {
    let mut mounts: Vec<PathBuf> = Vec::new();
    for variable in ["GoogleDrive", "GoogleDriveFS"] {
        if let Some(path) = std::env::var_os(variable) {
            mounts.push(PathBuf::from(path));
        }
    }
    // The drive letter each account was given, and the default one.
    if let Some(preferences) = registry_string(
        HKEY_CURRENT_USER,
        KEY_READ,
        r"Software\Google\DriveFS",
        "PerAccountPreferences",
    ) && let Ok(json) = serde_json::from_str::<Value>(&preferences)
    {
        let mut letters = Vec::new();
        collect_strings(&json, "mount_point_path", &mut letters);
        mounts.extend(letters.iter().filter_map(|value| mount_path(value)));
    }
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(value) = registry_string(
            hive,
            KEY_READ,
            r"Software\Google\DriveFS",
            "DefaultMountPoint",
        ) {
            mounts.extend(mount_path(&value));
        }
    }
    // Older Drive for desktop versions kept the mount point in a file per account.
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        && let Ok(accounts) = fs::read_dir(Path::new(&local).join("Google").join("DriveFS"))
    {
        for account in accounts.flatten().take(32) {
            let marker = account.path().join("mount_point_path");
            if fs::metadata(&marker).is_ok_and(|meta| meta.len() <= 4096)
                && let Ok(raw) = fs::read_to_string(&marker)
            {
                mounts.extend(mount_path(&raw));
            }
        }
    }
    mounts.extend(google_drive_volumes());
    for mount in &mounts {
        push_google_drive(out, mount);
    }

    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = Path::new(&profile);
        for name in MY_DRIVE.iter().copied().chain(["Google Drive"]) {
            push_cloud(
                out,
                CloudProvider::GoogleDrive,
                "Google Drive",
                &profile.join(name),
            );
        }
    }
}

/// `G`, `G:` or `G:\` as the drive root; anything else as given.
fn mount_path(value: &str) -> Option<PathBuf> {
    let value = value.trim().trim_matches('\0');
    let letter = value.trim_end_matches(['\\', '/']).trim_end_matches(':');
    if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(PathBuf::from(format!("{letter}:\\")))
    } else if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn collect_strings(value: &Value, key: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (name, child) in map {
                match child {
                    Value::String(text) if name == key => out.push(text.clone()),
                    _ => collect_strings(child, key, out),
                }
            }
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_strings(item, key, out)),
        _ => {}
    }
}

/// Local drives labelled "Google Drive". Network and removable drives are
/// not asked: a disconnected share can take many seconds to answer.
fn google_drive_volumes() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;

    // SAFETY: no arguments; a bit mask of the drive letters in use.
    let drives = unsafe { GetLogicalDrives() };
    let mut found = Vec::new();
    for index in 0..26u8 {
        if drives & (1 << index) == 0 {
            continue;
        }
        let root = format!("{}:\\", char::from(b'A' + index));
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives both calls; the
        // label buffer's length is passed with it.
        if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let mut label = [0u16; 64];
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        let length = label.iter().position(|&c| c == 0).unwrap_or(label.len());
        if ok != 0
            && String::from_utf16_lossy(&label[..length]).eq_ignore_ascii_case("Google Drive")
        {
            found.push(PathBuf::from(root));
        }
    }
    found
}

/// The "My Drive" folder of a Google Drive mount, or the folder itself when
/// it is one already. A drive root is never offered: Drive does not allow
/// files there.
pub(crate) fn google_my_drive(mount: &Path) -> Option<PathBuf> {
    if let Some(path) = MY_DRIVE
        .iter()
        .map(|name| mount.join(name))
        .find(|path| path.is_dir())
    {
        return Some(path);
    }
    if mount.parent().is_some() {
        return mount.is_dir().then(|| mount.to_path_buf());
    }
    // In another language: a personal account shows just the one folder
    // (plus hidden ones, and "Shared drives" only for work accounts).
    let folders: Vec<PathBuf> = fs::read_dir(mount)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .take(3)
        .collect();
    match folders.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

fn push_google_drive(out: &mut Vec<CloudFolder>, mount: &Path) {
    if let Some(path) = google_my_drive(mount) {
        push_cloud(out, CloudProvider::GoogleDrive, "Google Drive", &path);
    }
}

/// MEGA's desktop app keeps its settings encrypted, so its sync folder is
/// found where the app suggests it: `MEGA` (older versions: `MEGAsync`) in
/// the user's profile or Documents.
fn detect_mega(out: &mut Vec<CloudFolder>) {
    let Some(profile) = std::env::var_os("USERPROFILE") else {
        return;
    };
    let profile = Path::new(&profile);
    let mut bases = vec![profile.to_path_buf(), profile.join("Documents")];
    bases.extend(documents_folder());
    for base in bases {
        for name in ["MEGA", "MEGAsync"] {
            push_cloud(out, CloudProvider::Mega, "MEGA", &base.join(name));
        }
    }
}

/// The user's Documents folder, wherever it was moved (often into OneDrive).
fn documents_folder() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};

    let mut raw = std::ptr::null_mut();
    // SAFETY: `raw` receives a CoTaskMemAlloc'd string, freed below.
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut raw) };
    if raw.is_null() {
        return None;
    }
    let path = (result >= 0).then(|| {
        // SAFETY: a successful call returns a NUL-terminated string.
        let length = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
        let units = unsafe { std::slice::from_raw_parts(raw, length) };
        PathBuf::from(OsString::from_wide(units))
    });
    // SAFETY: allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}

fn push_cloud(out: &mut Vec<CloudFolder>, provider: CloudProvider, label: &str, path: &Path) {
    if path.parent().is_some() && path.is_dir() {
        out.push(CloudFolder {
            provider,
            label: label.into(),
            path: path.to_path_buf(),
        });
    }
}

/// Two folder paths name the same folder: case and trailing separators aside.
fn same_folder(a: &Path, b: &Path) -> bool {
    let text = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    text(a) == text(b)
}

fn registry_string(hive: winreg::HKEY, flags: u32, key: &str, value: &str) -> Option<String> {
    RegKey::predef(hive)
        .open_subkey_with_flags(key, flags)
        .ok()?
        .get_value::<String, _>(value)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_from_google_drive_settings_become_drive_roots() {
        assert_eq!(mount_path("G"), Some(PathBuf::from(r"G:\")));
        assert_eq!(mount_path("g:"), Some(PathBuf::from(r"g:\")));
        assert_eq!(mount_path(r"H:\"), Some(PathBuf::from(r"H:\")));
        assert_eq!(
            mount_path(r"C:\Users\A\My Drive"),
            Some(PathBuf::from(r"C:\Users\A\My Drive"))
        );
        assert_eq!(mount_path(" "), None);

        let json: Value = serde_json::from_str(
            r#"{"per_account_preferences":[{"key":"1","value":{"mount_point_path":"G"}},{"key":"2","value":{"mount_point_path":"H"}}]}"#,
        )
        .unwrap();
        let mut found = Vec::new();
        collect_strings(&json, "mount_point_path", &mut found);
        assert_eq!(found, ["G", "H"]);
    }

    #[test]
    fn my_drive_is_found_inside_a_mount_in_any_language() {
        let root = std::env::temp_dir().join(format!("myle-gdrive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Ο Δίσκος μου")).unwrap();
        fs::create_dir_all(root.join(".shortcut-targets-by-id")).unwrap();
        assert_eq!(google_my_drive(&root), Some(root.join("Ο Δίσκος μου")));
        let _ = fs::remove_dir_all(&root);
    }
}
