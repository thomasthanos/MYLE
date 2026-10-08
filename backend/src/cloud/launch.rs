//! Starting a cloud app that is installed but not running.
//!
//! - Google Drive for desktop shows its files on a drive of its own (`G:`),
//!   which only exists while `GoogleDriveFS.exe` runs: nothing can be written
//!   there before it starts. Each update installs into a new version folder,
//!   so the newest one is started.
//! - Dropbox's folder is an ordinary folder, writable without the app; the app
//!   is started so that a backup syncs (and online-only files can be read).

use std::path::{Path, PathBuf};

const GOOGLE_DRIVE_EXE: &str = "GoogleDriveFS.exe";
const DROPBOX_EXE: &str = "Dropbox.exe";

/// What happened when the app was asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Started {
    AlreadyRunning,
    Launched,
    NotInstalled,
}

/// Starts Google Drive for desktop unless it already runs.
pub(crate) fn start_google_drive() -> Result<Started, String> {
    if process_running(GOOGLE_DRIVE_EXE) {
        return Ok(Started::AlreadyRunning);
    }
    let Some(exe) = google_drive_exe() else {
        return Ok(Started::NotInstalled);
    };
    spawn(&exe, &[]).map(|_| Started::Launched)
}

/// Starts Dropbox unless it already runs, the way Windows starts it at sign-in
/// (in the tray, without opening its window).
pub(crate) fn start_dropbox() -> Result<Started, String> {
    if process_running(DROPBOX_EXE) {
        return Ok(Started::AlreadyRunning);
    }
    let Some(exe) = dropbox_exe() else {
        return Ok(Started::NotInstalled);
    };
    spawn(&exe, &["/systemstartup"]).map(|_| Started::Launched)
}

/// Starts `exe` on its own: not a child of MYLE's console or job, so it keeps
/// running when MYLE closes (or is updated).
fn spawn(exe: &Path, args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let command = |flags: u32| {
        let mut command = std::process::Command::new(exe);
        command
            .args(args)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags);
        command
    };
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    // A job that does not allow breaking away refuses the first form.
    command(detached | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .or_else(|_| command(detached).spawn())
        .map(|_| ())
        .map_err(|error| format!("{} could not be started: {error}", exe.display()))
}

/// `<Program Files>\Google\Drive File Stream\<newest version>\GoogleDriveFS.exe`.
pub(crate) fn google_drive_exe() -> Option<PathBuf> {
    program_files()
        .into_iter()
        .map(|root| root.join("Google").join("Drive File Stream"))
        .filter_map(|root| newest_version_with(&root, GOOGLE_DRIVE_EXE))
        .next()
}

fn dropbox_exe() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = program_files()
        .into_iter()
        .map(|root| root.join("Dropbox").join("Client").join(DROPBOX_EXE))
        .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            Path::new(&local)
                .join("Dropbox")
                .join("Client")
                .join(DROPBOX_EXE),
        );
    }
    if let Some(roaming) = std::env::var_os("APPDATA") {
        candidates.push(
            Path::new(&roaming)
                .join("Dropbox")
                .join("bin")
                .join(DROPBOX_EXE),
        );
    }
    candidates.into_iter().find(|exe| exe.is_file())
}

fn program_files() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(value) = std::env::var_os(variable) {
            let path = PathBuf::from(value);
            if !roots.iter().any(|known| known == &path) {
                roots.push(path);
            }
        }
    }
    roots
}

/// The highest version folder (`112.0.1.0`) inside `root` that holds `exe`.
pub(crate) fn newest_version_with(root: &Path, exe: &str) -> Option<PathBuf> {
    let mut versions: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let parts: Option<Vec<u64>> = name.split('.').map(|part| part.parse().ok()).collect();
            let candidate = entry.path().join(exe);
            match parts {
                Some(parts) if !parts.is_empty() && candidate.is_file() => Some((parts, candidate)),
                _ => None,
            }
        })
        .collect();
    versions.sort();
    versions.pop().map(|(_, exe)| exe)
}

/// Whether a program with this file name runs in this user's session.
pub(crate) fn process_running(name: &str) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut mine = 0u32;
    // SAFETY: plain calls on a zeroed entry with its size set; the snapshot
    // handle is closed below.
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut mine);
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        let mut found = false;
        while more && !found {
            let length = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case(name) {
                let mut session = u32::MAX;
                ProcessIdToSessionId(entry.th32ProcessID, &mut session);
                found = session == mine;
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_google_drive_version_folder_wins() {
        let root = std::env::temp_dir().join(format!("myle-drivefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for version in ["99.0.2.0", "112.0.1.0", "112.0.0.9", "not-a-version"] {
            std::fs::create_dir_all(root.join(version)).unwrap();
            std::fs::write(root.join(version).join(GOOGLE_DRIVE_EXE), b"").unwrap();
        }
        // A newer folder without the program (a half-finished update) is passed over.
        std::fs::create_dir_all(root.join("120.0.0.0")).unwrap();
        assert_eq!(
            newest_version_with(&root, GOOGLE_DRIVE_EXE),
            Some(root.join("112.0.1.0").join(GOOGLE_DRIVE_EXE))
        );
        assert_eq!(
            newest_version_with(&root.join("missing"), GOOGLE_DRIVE_EXE),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn this_program_is_seen_running() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy();
        assert!(process_running(&name));
        assert!(!process_running("certainly-not-running-myle.exe"));
    }
}
