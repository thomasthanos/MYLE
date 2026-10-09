//! Backend of the Install Apps page: winget packages, custom (direct-download)
//! apps, job control and app-list import/export.

pub mod creative;
pub mod custom;
pub mod io;
mod jobs;
pub(crate) mod process;
mod resolver;
pub mod winget;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, State};

pub(crate) use jobs::JobHandle;
pub use jobs::Jobs;

/// Files and folders to remove when the app closes: downloaded packages and
/// the folders they were unpacked into. They stay while the app runs, so an
/// installer that relaunches itself keeps working.
///
/// The list is kept on disk as well, because a file can still be locked while
/// the app closes (an installer that is finishing up, a virus scanner). What
/// could not be removed is deleted on the next start instead.
#[derive(Clone, Default)]
pub struct Cleanup(Arc<Mutex<CleanupState>>);

#[derive(Default)]
struct CleanupState {
    paths: Vec<PathBuf>,
    file: Option<PathBuf>,
}

impl Cleanup {
    /// Loads the pending list and removes whatever is now free.
    pub fn init(&self, file: PathBuf) {
        {
            let mut state = self.lock();
            let pending: Vec<PathBuf> = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            state.file = Some(file);
            for path in pending {
                if !state.paths.contains(&path) {
                    state.paths.push(path);
                }
            }
        }
        self.run();
    }

    pub fn add(&self, path: PathBuf) {
        let mut state = self.lock();
        if !state.paths.contains(&path) {
            state.paths.push(path);
        }
        Self::save(&state);
    }

    /// Deletes what it can; anything still locked stays on the list for next time.
    /// MYLE's own download folders go as a whole too, so a file a job did not
    /// put on the list (or one left by a crash) does not stay behind.
    pub fn run(&self) {
        let mut state = self.lock();
        state.paths.retain(|path| !remove(path));
        Self::save(&state);
        drop(state);
        for dir in work_dirs() {
            remove(&dir);
        }
    }

    fn save(state: &CleanupState) {
        let Some(file) = &state.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string(&state.paths) {
            Ok(json) => drop(std::fs::write(file, json)),
            Err(_) => drop(std::fs::remove_file(file)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CleanupState> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The folders MYLE downloads and unpacks into, and nothing else: app
/// installers, and Spotify Hub's staging. Only folders named for MYLE, under
/// the temporary folder and Local AppData. The update folder is left to the
/// next start (the installer runs from it while the app closes).
pub fn work_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir().join(crate::storage::FOLDER).join("apps")];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(crate::storage::FOLDER).join("spotify-hub"));
    }
    dirs
}

/// True when the path is gone (deleted now, or never there).
fn remove(path: &std::path::Path) -> bool {
    for attempt in 0..3 {
        if !path.exists() {
            return true;
        }
        let done = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        if done.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200 * (attempt + 1)));
    }
    !path.exists()
}

/// Progress events streamed to the page while an app installs or updates.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum JobEvent {
    Stage {
        stage: Stage,
    },
    /// Bytes are filled in for our own downloads; winget only reports a share.
    Progress {
        fraction: f64,
        downloaded: Option<u64>,
        total: Option<u64>,
    },
    /// What is being downloaded, so the page can name it.
    File {
        name: String,
        total: Option<u64>,
    },
    Note {
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Resolving,
    Downloading,
    Verifying,
    Extracting,
    Installing,
}

/// How a finished job ended. Failures are returned as `Err(message)`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum JobOutcome {
    Done {
        note: Option<String>,
    },
    UpToDate,
    /// The installer's hash does not match the winget manifest. The page asks
    /// the user before retrying with `apps_install_ignoring_hash`.
    HashMismatch,
    Cancelled,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledReport {
    winget: Vec<winget::InstalledPackage>,
    /// Set when winget itself could not be queried (e.g. not installed).
    winget_error: Option<String>,
    custom: Vec<custom::CustomStatus>,
}

/// Installed state of every winget package plus the custom apps.
#[tauri::command]
pub async fn apps_installed(app: AppHandle) -> Result<InstalledReport, String> {
    let (winget, custom) = tokio::join!(winget::list_installed(), custom::statuses(&app));
    let (winget, winget_error) = match winget {
        Ok(list) => (list, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    Ok(InstalledReport {
        winget,
        winget_error,
        custom,
    })
}

/// Stops the running job for `id`: its process tree is killed and the job
/// reports `Cancelled`.
#[tauri::command(async)]
pub fn apps_cancel(jobs: State<'_, Jobs>, id: String) {
    jobs.cancel(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_files_and_folders_and_survives_a_restart() {
        let root = std::env::temp_dir().join(format!("myle-cleanup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("unpacked").join("release")).unwrap();
        std::fs::write(root.join("package.zip"), b"zip").unwrap();
        std::fs::write(
            root.join("unpacked").join("release").join("Setup.exe"),
            b"exe",
        )
        .unwrap();
        let list = root.join("pending-cleanup.json");

        let cleanup = Cleanup::default();
        cleanup.init(list.clone());
        cleanup.add(root.join("package.zip"));
        cleanup.add(root.join("unpacked"));
        // Registered paths are remembered on disk in case the app is killed.
        assert!(std::fs::read_to_string(&list).unwrap().contains("unpacked"));

        cleanup.run();
        assert!(!root.join("package.zip").exists());
        assert!(
            !root.join("unpacked").exists(),
            "the whole folder goes, not just the zip"
        );
        assert_eq!(std::fs::read_to_string(&list).unwrap(), "[]");

        // A fresh start sweeps leftovers from a previous run.
        std::fs::write(root.join("left-over.zip"), b"zip").unwrap();
        std::fs::write(
            &list,
            format!("[{:?}]", root.join("left-over.zip").display().to_string()),
        )
        .unwrap();
        Cleanup::default().init(list.clone());
        assert!(!root.join("left-over.zip").exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
