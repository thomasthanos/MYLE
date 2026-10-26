use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use std::os::windows::fs::OpenOptionsExt;

use super::models::{GameSaveEntry, GameSavesScan, GameSavesSettings, OperationKind};

#[derive(Clone, Default)]
pub struct GameSavesState(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    active: Option<ActiveOperation>,
    cached_scan: Option<GameSavesScan>,
    next_token: u64,
}

#[derive(Clone)]
struct ActiveOperation {
    kind: OperationKind,
    token: u64,
    /// A scan runs its engine passes side by side; cancel stops all of them.
    pids: Vec<u32>,
    cancelled: bool,
    /// Whether Cancel may stop the engine at once. Not while it writes saves
    /// (a restore, an undo): a save cut off half-written is lost, so Cancel
    /// then only stops before the next game.
    killable: bool,
}

pub(crate) struct OperationHandle {
    state: GameSavesState,
    token: u64,
    _lock: OperationFileLock,
}

pub(crate) struct OperationFileLock {
    path: PathBuf,
    #[allow(dead_code)]
    file: Option<File>,
}

/// Why an operation cannot begin while another process (the scheduled
/// backup, or the app while that backup waits) holds the lock.
pub(crate) const BUSY: &str = "Another Game Saves operation is already running.";

/// The operation locks this process holds. Taking or releasing one never runs
/// alongside `held_elsewhere`, whose look at the file would otherwise make it
/// fail (or be taken for another process's).
static HELD_HERE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn held_here() -> MutexGuard<'static, Vec<PathBuf>> {
    HELD_HERE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether another process holds the operation lock: with the app being
/// single-instance, that is the scheduled backup running on its own.
pub(crate) fn held_elsewhere(config_root: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE};
    let path = config_root.join(LOCK_FILE);
    let held = held_here();
    if held.contains(&path) {
        return false;
    }
    // The holder shares it with nobody: opening it at all fails meanwhile.
    let probe = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path);
    let busy = matches!(&probe, Err(error) if matches!(error.raw_os_error(), Some(32 | 33)));
    drop(probe);
    drop(held);
    busy
}

const LOCK_FILE: &str = "operation.lock";

/// Holds the same cross-process lock as a Game Saves operation without
/// starting one. Destructive system actions keep this guard alive so neither
/// the UI nor the scheduled headless process can begin while they are pending.
pub(crate) struct GameSavesReservation {
    _lock: OperationFileLock,
}

impl GameSavesState {
    pub(crate) fn begin(
        &self,
        config_root: &Path,
        kind: OperationKind,
    ) -> Result<OperationHandle, String> {
        {
            let inner = self.lock();
            if let Some(active) = &inner.active {
                return Err(format!(
                    "A Game Saves {} operation is already running.",
                    operation_name(active.kind)
                ));
            }
        }

        let operation_lock = OperationFileLock::acquire(config_root)?;
        let token = {
            let mut inner = self.lock();
            // The file lock prevents a second process from entering, but two
            // tasks in this process may have raced before either set `active`.
            if inner.active.is_some() {
                return Err("Another Game Saves operation just started.".into());
            }
            inner.next_token = inner.next_token.wrapping_add(1).max(1);
            let token = inner.next_token;
            inner.active = Some(ActiveOperation {
                kind,
                token,
                pids: Vec::new(),
                cancelled: false,
                killable: true,
            });
            token
        };
        Ok(OperationHandle {
            state: self.clone(),
            token,
            _lock: operation_lock,
        })
    }

    pub(crate) fn active_kind(&self) -> Option<OperationKind> {
        self.lock().active.as_ref().map(|active| active.kind)
    }

    pub(crate) fn reserve_idle(&self, config_root: &Path) -> Result<GameSavesReservation, String> {
        if self.active_kind().is_some() {
            return Err("Wait for the current Game Saves operation to finish.".into());
        }

        // Acquiring the file lock closes both races: an in-process operation
        // that passed the state check must acquire this same lock before it can
        // publish itself, and the scheduled task runs in another process and
        // therefore can only be excluded by the file handle.
        let operation_lock = OperationFileLock::acquire(config_root)?;
        if self.active_kind().is_some() {
            return Err("Wait for the current Game Saves operation to finish.".into());
        }

        Ok(GameSavesReservation {
            _lock: operation_lock,
        })
    }

    pub(crate) fn cache_scan(&self, scan: GameSavesScan) {
        self.lock().cached_scan = Some(scan);
    }

    pub(crate) fn cached_scan(&self) -> Option<GameSavesScan> {
        self.lock().cached_scan.clone()
    }

    /// Settings may recover or select another destination while this process
    /// already has a scan. Sanitize the stored copy as well as the UI copy.
    pub(crate) fn cached_scan_for(&self, settings: &GameSavesSettings) -> Option<GameSavesScan> {
        self.publish_cached_scan(settings, None)
    }

    /// A page load may have read disk before a fresh scan finished. Install
    /// that fallback only if memory is still empty, under the same lock used
    /// for destination validation and copying the published result.
    pub(crate) fn publish_cached_scan(
        &self,
        settings: &GameSavesSettings,
        fallback: Option<GameSavesScan>,
    ) -> Option<GameSavesScan> {
        let mut inner = self.lock();
        if inner.cached_scan.is_none() {
            inner.cached_scan = fallback;
        }
        let scan = inner.cached_scan.as_mut()?;
        super::scan_cache::validate_destination(scan, settings);
        Some(scan.clone())
    }

    pub(crate) fn find_game(&self, id: &str) -> Option<GameSaveEntry> {
        let inner = self.lock();
        let scan = inner.cached_scan.as_ref()?;
        scan.on_this_pc
            .iter()
            .chain(scan.in_backup.iter())
            .find(|game| game.id == id)
            .cloned()
    }

    pub(crate) fn cancel(&self) {
        for pid in self.cancel_marking() {
            crate::apps::process::kill_tree(pid);
        }
    }

    /// Marks the running operation cancelled; answers the engine processes
    /// to stop now (none while saves are being written).
    fn cancel_marking(&self) -> Vec<u32> {
        let mut inner = self.lock();
        let Some(active) = inner.active.as_mut() else {
            return Vec::new();
        };
        active.cancelled = true;
        if active.killable { active.pids.clone() } else { Vec::new() }
    }

    pub(crate) fn ensure_idle(&self) -> Result<(), String> {
        self.active_kind()
            .map(|_| Err("Wait for the current Game Saves operation to finish.".into()))
            .unwrap_or(Ok(()))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl OperationHandle {
    /// Records a running engine process (`running`), or forgets it once it
    /// has exited. A process started after Cancel is stopped straight away.
    pub(crate) fn track_pid(&self, pid: u32, running: bool) {
        let cancelled = {
            let mut inner = self.state.lock();
            let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.token == self.token)
            else {
                return;
            };
            if running {
                active.pids.push(pid);
            } else {
                active.pids.retain(|known| *known != pid);
            }
            running && active.cancelled && active.killable
        };
        if cancelled {
            crate::apps::process::kill_tree(pid);
        }
    }

    /// While `false`, Cancel no longer stops the engine mid-way: it only stops
    /// the operation before its next step (see `ActiveOperation::killable`).
    pub(crate) fn set_killable(&self, killable: bool) {
        if let Some(active) = self
            .state
            .lock()
            .active
            .as_mut()
            .filter(|active| active.token == self.token)
        {
            active.killable = killable;
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.state
            .lock()
            .active
            .as_ref()
            .filter(|active| active.token == self.token)
            .is_some_and(|active| active.cancelled)
    }
}

impl Drop for OperationHandle {
    fn drop(&mut self) {
        let mut inner = self.state.lock();
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.token == self.token)
        {
            inner.active = None;
        }
    }
}

impl OperationFileLock {
    pub(crate) fn acquire(config_root: &Path) -> Result<Self, String> {
        fs::create_dir_all(config_root).map_err(|error| error.to_string())?;
        let path = config_root.join(LOCK_FILE);
        let mut held = held_here();
        match create_lock(&path) {
            Ok(lock) => {
                held.push(path);
                Ok(lock)
            }
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => Err(BUSY.into()),
            Err(error) => Err(lock_error(error)),
        }
    }
}

impl Drop for OperationFileLock {
    fn drop(&mut self) {
        let mut held = held_here();
        // Release the exclusive Windows handle before removing the marker.
        self.file.take();
        let _ = fs::remove_file(&self.path);
        if let Some(index) = held.iter().position(|path| *path == self.path) {
            held.swap_remove(index);
        }
    }
}

fn create_lock(path: &Path) -> std::io::Result<OperationFileLock> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        // Windows releases this lock automatically even if the process dies.
        .share_mode(0)
        .open(path)?;
    writeln!(file, "{}", std::process::id())?;
    file.sync_all()?;
    Ok(OperationFileLock {
        path: path.to_path_buf(),
        file: Some(file),
    })
}

fn lock_error(error: std::io::Error) -> String {
    format!("The Game Saves operation lock could not be created: {error}")
}

fn operation_name(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Scan => "scan",
        OperationKind::UpdateDatabase => "database update",
        OperationKind::Backup => "backup",
        OperationKind::Restore => "restore",
        OperationKind::ScheduledBackup => "scheduled backup",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_fallback_cannot_replace_a_scan_that_completed_after_it_was_read() {
        let state = GameSavesState::default();
        let settings = GameSavesSettings { backup_folder: Some(r"G:\Backups".into()), ..Default::default() };
        let stale = GameSavesScan { generated_at: 100, backup_folder: settings.backup_folder.clone(), ..Default::default() };
        state.cache_scan(stale.clone());
        let fallback = state.cached_scan().unwrap();
        let fresh = GameSavesScan { generated_at: 200, ..stale };
        state.cache_scan(fresh.clone());
        assert_eq!(state.publish_cached_scan(&settings, Some(fallback)), Some(fresh.clone()));
        assert_eq!(state.cached_scan(), Some(fresh));
    }

    #[test]
    fn publishing_for_another_destination_also_removes_history_from_the_live_backend_cache() {
        let game = serde_json::json!({
            "id": "game-a", "title": "A", "status": "backedUp", "platformBadges": [],
            "fileCount": 1, "totalBytes": 5, "lastSaveAt": null, "lastBackupAt": "2026-09-27",
            "paths": ["C:/Saves"], "autoBackup": true, "hasLocalData": true, "hasBackup": true, "error": null,
            "snapshots": [{"id": "old.zip", "timestamp": "2026-09-27", "bytes": 5, "label": null, "isSafety": false}]
        });
        let scan: GameSavesScan = serde_json::from_value(serde_json::json!({
            "generatedAt": 42, "backupFolder": "E:/Previous backups",
            "stats": {"localGames": 1, "backupGames": 1, "totalBytes": 5, "databaseGames": 100},
            "onThisPc": [game.clone()], "inBackup": [game]
        })).unwrap();
        let state = GameSavesState::default();
        state.cache_scan(scan);
        let settings = GameSavesSettings { backup_folder: Some(r"G:\Recovered backups".into()), ..Default::default() };
        let published = state.cached_scan_for(&settings).unwrap();
        assert!(published.in_backup.is_empty());
        let actionable = state.find_game("game-a").unwrap();
        assert!(!actionable.has_backup);
        assert!(actionable.snapshots.is_empty(), "backend actions must not retain the stale restore choice");
        assert_eq!(state.cached_scan().unwrap(), published, "the stored cache, not just a returned clone, is sanitized");
    }

    #[test]
    fn operation_lock_is_exclusive_and_released() {
        let root =
            std::env::temp_dir().join(format!("myle-game-saves-lock-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let first = OperationFileLock::acquire(&root).unwrap();
        assert!(OperationFileLock::acquire(&root).is_err());
        drop(first);
        assert!(OperationFileLock::acquire(&root).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_lock_of_another_process_is_told_from_this_ones() {
        let root = std::env::temp_dir().join(format!("myle-game-saves-elsewhere-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(!held_elsewhere(&root), "no lock file: nothing runs");
        let state = GameSavesState::default();
        let operation = state.begin(&root, OperationKind::Scan).unwrap();
        assert!(!held_elsewhere(&root), "this process's own operation");
        drop(operation);
        assert!(!held_elsewhere(&root), "released");
        // Another process: the file held with no sharing, outside this
        // process's bookkeeping.
        let other = OpenOptions::new().read(true).write(true).create(true).truncate(true).share_mode(0).open(root.join(LOCK_FILE)).unwrap();
        assert!(held_elsewhere(&root));
        assert_eq!(state.begin(&root, OperationKind::Scan).err().as_deref(), Some(BUSY));
        drop(other);
        assert!(!held_elsewhere(&root), "a lock file left behind by a crash holds nothing");
        assert!(state.begin(&root, OperationKind::Scan).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn idle_reservation_blocks_ui_and_headless_operations_until_drop() {
        let root = std::env::temp_dir().join(format!(
            "myle-game-saves-reservation-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let state = GameSavesState::default();

        let reservation = state.reserve_idle(&root).unwrap();
        assert!(state.begin(&root, OperationKind::Scan).is_err());
        assert!(OperationFileLock::acquire(&root).is_err());

        drop(reservation);
        assert!(state.begin(&root, OperationKind::Scan).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cancel_never_stops_the_engine_while_it_writes_saves() {
        let root = std::env::temp_dir().join(format!(
            "myle-game-saves-cancel-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let state = GameSavesState::default();
        let operation = state.begin(&root, OperationKind::Restore).unwrap();
        state.lock().active.as_mut().unwrap().pids.push(4242);

        operation.set_killable(false);
        assert!(state.cancel_marking().is_empty(), "a restore writing saves is not killed");
        assert!(operation.is_cancelled(), "it stops before the next game instead");

        operation.set_killable(true);
        assert_eq!(state.cancel_marking(), vec![4242], "a scan or preview still stops at once");
        drop(operation);
        let _ = fs::remove_dir_all(&root);
    }
}
