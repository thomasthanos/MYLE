//! Files kept in OneDrive as online-only files (game saves, project files).
//!
//! Such a file is only a placeholder on disk: OneDrive downloads it the moment
//! something reads it, but only while OneDrive runs. Without it Windows answers
//! "The cloud file provider is not running" (os error 362) and the backup of
//! that game fails. This starts the user's own OneDrive for a folder one of
//! their OneDrive accounts syncs, so the files can come down.

use std::io::Read;
use std::path::{Path, PathBuf};

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

/// ERROR_CLOUD_FILE_PROVIDER_NOT_RUNNING.
pub(crate) const PROVIDER_NOT_RUNNING: u32 = 362;

/// Starts OneDrive when one of `files` is inside a folder a signed-in OneDrive
/// account syncs. Returns that file, to watch for when it can be read, or
/// `None` when the files are not OneDrive's or OneDrive is not installed.
pub(crate) fn start_onedrive(files: &[PathBuf]) -> Option<PathBuf> {
    let folders = onedrive_folders();
    let probe = files
        .iter()
        .find(|file| folders.iter().any(|folder| is_inside(file, folder)))?;
    let exe = onedrive_exe()?;
    // The same switch Windows uses at sign-in. When OneDrive is already
    // starting, the new copy hands over to it and exits.
    std::process::Command::new(exe)
        .arg("/background")
        .spawn()
        .ok()?;
    Some(probe.clone())
}

/// True once the file's content is on this PC (reading it makes OneDrive
/// download it).
pub(crate) fn readable(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|mut file| file.read(&mut [0u8; 1]))
        .is_ok()
}

/// The folder each OneDrive account signed in on this PC syncs.
fn onedrive_folders() -> Vec<PathBuf> {
    let Ok(accounts) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\OneDrive\Accounts")
    else {
        return Vec::new();
    };
    accounts
        .enum_keys()
        .flatten()
        .filter_map(|name| {
            accounts
                .open_subkey(&name)
                .and_then(|account| account.get_value::<String, _>("UserFolder"))
                .ok()
        })
        .filter(|folder| !folder.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn onedrive_exe() -> Option<PathBuf> {
    let registered = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\OneDrive")
        .and_then(|key| key.get_value::<String, _>("OneDriveTrigger"))
        .ok()
        .map(PathBuf::from);
    let per_user = std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join(r"Microsoft\OneDrive\OneDrive.exe"));
    let per_machine = std::env::var_os("ProgramFiles")
        .map(|dir| PathBuf::from(dir).join(r"Microsoft OneDrive\OneDrive.exe"));
    [registered, per_user, per_machine]
        .into_iter()
        .flatten()
        .find(|exe| exe.is_file())
}

/// Ludusavi reports paths with forward slashes, the registry with backslashes.
fn is_inside(path: &Path, folder: &Path) -> bool {
    let normal = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let (path, folder) = (normal(path), normal(folder));
    !folder.is_empty() && (path == folder || path.starts_with(&format!("{folder}\\")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_inside_a_onedrive_folder_are_recognised_in_either_slash_style() {
        let folder = Path::new(r"C:\Users\Panos\OneDrive");
        assert!(is_inside(
            Path::new("C:/Users/Panos/OneDrive/Documents/My Games/save.dat"),
            folder
        ));
        assert!(is_inside(Path::new(r"c:\users\panos\onedrive\"), folder));
        assert!(!is_inside(
            Path::new("C:/Users/Panos/OneDrive - Work/save.dat"),
            folder
        ));
        assert!(!is_inside(Path::new("C:/Users/Panos/save.dat"), Path::new("")));
    }
}
