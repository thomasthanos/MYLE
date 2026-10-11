use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use std::os::windows::fs::OpenOptionsExt;

use super::models::{GameSaveEntry, GameSavesScan, OperationKind};

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
        let path = config_root.join("operation.lock");
        match create_lock(&path) {
            Ok(lock) => Ok(lock),
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                Err("Another Game Saves operation is already running.".into())
            }
            Err(error) => Err(lock_error(error)),
        }
    }
}

impl Drop for OperationFileLock {
    fn drop(&mut self) {
        // Release the exclusive Windows handle before removing the marker.
        self.file.take();
        let _ = fs::remove_file(&self.path);
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
