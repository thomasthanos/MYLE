use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::commands;
use super::engine::Engine;
use super::models::{BackupSchedule, GameSaveStatus, OperationKind, ScheduledBackupResult};
use super::parser;
use super::settings;
use super::state::{self, GameSavesState};

/// Two starts this close together are one run: Windows starts a missed run at
/// sign-in, and a schedule changed after a run may start it again that day.
const SAME_RUN: u64 = 2 * 60 * 60;
/// A last run further ahead than this was stamped by a clock that was wrong
/// (or is now): it must not hold every backup off until that time comes.
const CLOCK_SLACK: u64 = 60 * 60;
/// How long the backup waits for an operation of the app (a scan, a restore)
/// to end, trying again this often, before it gives this run up.
const BUSY_WAIT: Duration = Duration::from_secs(15 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(15);
/// How long it waits for the backup folder's drive. Google Drive's exists only
/// while Google Drive runs, and at sign-in a missed backup can start first.
const DRIVE_WAIT: Duration = Duration::from_secs(120);
const DRIVE_POLL: Duration = Duration::from_secs(2);

/// Entry point used by `main.rs` before Tauri creates any windows.
///
/// Exit codes: 0 success/not due, 2 another operation is running, 3 setup or
/// engine failure, 4 a backup completed with per-game failures.
pub fn run_headless_auto_backup() -> i32 {
    tauri::async_runtime::block_on(run())
}

async fn run() -> i32 {
    let root = match settings::headless_config_root() {
        Ok(root) => root,
        Err(error) => return report_error(3, &error),
    };
    // Nothing to wait for when no backup is due.
    match settings::load_from_root(&root) {
        Ok(value) if value.schedule == BackupSchedule::Off || !is_due(&value, parser::now()) => return 0,
        Ok(_) => {}
        Err(error) => return report_error(3, &error),
    }
    // The app may be scanning or restoring right now (it opens at sign-in,
    // when a missed backup starts too): wait for it rather than skip a day.
    let state = GameSavesState::default();
    let begin = || state.begin(&root, OperationKind::ScheduledBackup);
    let operation = match when_free(begin, BUSY_WAIT, BUSY_RETRY, &std::thread::sleep) {
        Ok(operation) => operation,
        Err(error) => {
            if error == state::BUSY {
                let skipped = Err(format!(
                    "Game Saves was busy with another operation for {} minutes. The next scheduled backup runs as usual.",
                    BUSY_WAIT.as_secs() / 60
                ));
                super::notice::show(&super::notice::Notice::from_result(&skipped, parser::now()));
            }
            return report_error(2, &error);
        }
    };
    // Read again under the lock: the app may have changed them meanwhile,
    // and a second start of this task may have run the backup already.
    let mut value = match settings::load_from_root(&root) {
        Ok(value) => value,
        Err(error) => return report_error(3, &error),
    };
    if value.schedule == BackupSchedule::Off {
        return 0;
    }

    let started = parser::now();
    if !is_due(&value, started) {
        return 0;
    }
    value.last_scheduled_attempt = Some(started);
    if let Err(error) = settings::save_to_root(&root, &value) {
        return report_error(3, &error);
    }

    let result = run_due(&root, &mut value, &operation).await;
    let completed = parser::now();
    // What happened, on the screen: the backup is done either way.
    super::notice::show(&super::notice::Notice::from_result(&result, completed));
    // A backup can take minutes, and the app may have changed the settings
    // meanwhile (a new custom game, another backup folder). Only the fields
    // this run owns are written onto a fresh copy, so none of that is lost.
    let database_games = value.database_games;
    if let Ok(fresh) = settings::load_from_root(&root) {
        value = fresh;
    }
    value.last_scheduled_attempt = Some(started);
    if database_games > 0 {
        value.database_games = database_games;
    }
    let exit = match result {
        Ok((processed_games, failed_games)) => {
            let success = failed_games.is_empty();
            value.last_scheduled_result = Some(ScheduledBackupResult {
                attempted_at: started,
                completed_at: completed,
                processed_games,
                failed_games: failed_games.clone(),
                error: None,
            });
            if success {
                value.last_scheduled_success = Some(completed);
                0
            } else {
                4
            }
        }
        Err(error) => {
            value.last_scheduled_result = Some(ScheduledBackupResult {
                attempted_at: started,
                completed_at: completed,
                processed_games: 0,
                failed_games: Vec::new(),
                error: Some(error.clone()),
            });
            report_error(3, &error)
        }
    };
    if let Err(error) = settings::save_to_root(&root, &value) {
        return report_error(3, &error);
    }
    exit
}

async fn run_due(
    root: &Path,
    value: &mut super::models::GameSavesSettings,
    operation: &super::state::OperationHandle,
) -> Result<(u64, Vec<String>), String> {
    let backup = value
        .backup_folder
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .ok_or("Automatic backup needs a backup folder.")?;
    let folder = Path::new(backup);
    let google_drive = google_drive_top(folder);
    let start_google_drive = || {
        let _ = crate::cloud::launch::start_google_drive();
    };
    wait_for_drive(
        &|| drive_ready(folder, google_drive.as_deref()),
        google_drive.is_some().then_some(&start_google_drive as &dyn Fn()),
        DRIVE_WAIT,
        DRIVE_POLL,
        &std::thread::sleep,
    );
    settings::validate_backup_folder_for_root(folder, root, value)?;
    fs::create_dir_all(backup).map_err(|error| error.to_string())?;

    let engine = Engine::headless(root.to_path_buf())?;
    engine.prepare(value)?;
    let (scan, metadata) = commands::scan_with_engine(&engine, value, operation, None).await?;
    if metadata.supported_games > 0 {
        value.database_games = metadata.supported_games;
    }
    let titles: Vec<String> = scan
        .on_this_pc
        .into_iter()
        .filter(|game| game.auto_backup)
        .filter(|game| {
            matches!(
                game.status,
                GameSaveStatus::NotBackedUp | GameSaveStatus::ChangedSinceBackup
            )
        })
        .map(|game| game.title)
        .collect();
    if titles.is_empty() {
        return Ok((0, Vec::new()));
    }

    let result = commands::backup_titles(&engine, backup, &titles, operation, &|_| {}).await?;
    Ok((result.processed_games, result.failed_games))
}

fn is_due(value: &super::models::GameSavesSettings, now: u64) -> bool {
    if value.schedule.interval_seconds().is_none() {
        return false;
    }
    // Windows starts the task when it is time; this only keeps two starts of
    // one run from backing up twice. (Half the interval, as before, also
    // skipped the run on a weekday chosen right after the last one.)
    value
        .last_scheduled_attempt
        .is_none_or(|last| last > now.saturating_add(CLOCK_SLACK) || now.saturating_sub(last) >= SAME_RUN)
}

/// Runs `begin` until it gets the operation lock, trying again every
/// `every` while another operation holds it, for up to `limit`.
fn when_free<T>(
    mut begin: impl FnMut() -> Result<T, String>,
    limit: Duration,
    every: Duration,
    sleep: &dyn Fn(Duration),
) -> Result<T, String> {
    let mut waited = Duration::ZERO;
    loop {
        match begin() {
            Err(error) if error == state::BUSY && waited < limit => {
                sleep(every);
                waited += every;
            }
            result => return result,
        }
    }
}

/// `G:\My Drive` for a folder in Google Drive for desktop's drive (or a
/// shared drive there), as Google Drive names it in its languages.
fn google_drive_top(folder: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let top: Vec<Component> = folder.components().take(3).collect();
    match top.as_slice() {
        // Its drive has a letter (not a network share).
        [Component::Prefix(drive), Component::RootDir, Component::Normal(name)]
            if matches!(drive.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                && crate::cloud::detection::is_google_drive_folder(&name.to_string_lossy()) =>
        {
            Some(top.iter().collect())
        }
        _ => None,
    }
}

/// Whether the backup folder can be written to: its drive is there and, on
/// Google Drive's, so is its top folder (it shows a moment after the drive).
fn drive_ready(folder: &Path, google_drive: Option<&Path>) -> bool {
    settings::drive_connected(folder) && google_drive.is_none_or(Path::is_dir)
}

/// Gives the backup folder's drive time to appear, starting Google Drive
/// first for a folder in it. Answers whether it did; a backup that goes on
/// without it fails with the reason, as before.
fn wait_for_drive(
    ready: &dyn Fn() -> bool,
    start: Option<&dyn Fn()>,
    limit: Duration,
    every: Duration,
    sleep: &dyn Fn(Duration),
) -> bool {
    if ready() {
        return true;
    }
    if let Some(start) = start {
        start();
    }
    let mut waited = Duration::ZERO;
    while waited < limit {
        sleep(every);
        waited += every;
        if ready() {
            return true;
        }
    }
    false
}

fn report_error(code: i32, error: &str) -> i32 {
    eprintln!("Game Saves automatic backup: {error}");
    code
}

#[cfg(test)]
mod tests {
    use super::super::models::GameSavesSettings;
    use super::*;

    #[test]
    fn duplicate_scheduler_launches_are_suppressed() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Daily,
            last_scheduled_attempt: Some(1_000_000),
            ..Default::default()
        };
        assert!(!is_due(&value, 1_000_100));
        assert!(is_due(&value, 1_000_000 + 12 * 60 * 60));
    }

    #[test]
    fn a_weekday_chosen_right_after_a_run_is_not_skipped() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            last_scheduled_attempt: Some(1_000_000),
            ..Default::default()
        };
        // Sunday's run, then Tuesday chosen: two days later it runs.
        assert!(is_due(&value, 1_000_000 + 2 * 24 * 60 * 60));
        assert!(!is_due(&value, 1_000_000 + 60 * 60), "an hour later it is the same run");
        assert!(!is_due(&GameSavesSettings { schedule: BackupSchedule::Off, ..value }, 9_000_000));
    }

    #[test]
    fn a_run_stamped_by_a_wrong_clock_does_not_stop_backups() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Daily,
            // A clock a year ahead stamped it.
            last_scheduled_attempt: Some(1_000_000 + 365 * 24 * 60 * 60),
            ..Default::default()
        };
        assert!(is_due(&value, 1_000_000));
        let slightly_ahead = GameSavesSettings { last_scheduled_attempt: Some(1_000_000 + 30 * 60), ..value };
        assert!(!is_due(&slightly_ahead, 1_000_000), "within the slack it is the same run");
    }

    #[test]
    fn a_busy_app_is_waited_for_and_then_given_up_on() {
        use std::cell::Cell;
        let slept = Cell::new(Duration::ZERO);
        let sleep = |by: Duration| slept.set(slept.get() + by);
        let tries = Cell::new(0);
        let begin = || {
            tries.set(tries.get() + 1);
            if tries.get() < 4 { Err(state::BUSY.to_string()) } else { Ok("lock") }
        };
        assert_eq!(when_free(begin, BUSY_WAIT, BUSY_RETRY, &sleep), Ok("lock"));
        assert_eq!((tries.get(), slept.get()), (4, BUSY_RETRY * 3));

        slept.set(Duration::ZERO);
        let always_busy = || Err::<(), _>(state::BUSY.to_string());
        assert_eq!(when_free(always_busy, BUSY_WAIT, BUSY_RETRY, &sleep), Err(state::BUSY.to_string()));
        assert_eq!(slept.get(), BUSY_WAIT, "fifteen minutes, then this run is given up");

        slept.set(Duration::ZERO);
        let broken = || Err::<(), _>("The Game Saves operation lock could not be created: denied".to_string());
        assert!(when_free(broken, BUSY_WAIT, BUSY_RETRY, &sleep).is_err());
        assert_eq!(slept.get(), Duration::ZERO, "only a busy lock is waited for");
    }

    #[test]
    fn google_drive_is_started_and_its_folder_waited_for() {
        use std::cell::Cell;
        assert_eq!(google_drive_top(Path::new(r"G:\My Drive\Make Your Life Easier\Game Saves Backups")), Some(PathBuf::from(r"G:\My Drive")));
        assert_eq!(google_drive_top(Path::new(r"H:\Ο Δίσκος μου\Saves")), Some(PathBuf::from(r"H:\Ο Δίσκος μου")));
        assert_eq!(google_drive_top(Path::new(r"G:\Shared drives\Team\Saves")), Some(PathBuf::from(r"G:\Shared drives")));
        assert_eq!(google_drive_top(Path::new(r"D:\Backups\Game Saves")), None);
        assert!(google_drive_top(Path::new(r"G:\My Drive")).is_some());
        assert_eq!(google_drive_top(Path::new(r"\\server\share\My Drive")), None, "a network share is no Google Drive");

        let polls = Cell::new(0);
        let started = Cell::new(false);
        let slept = Cell::new(Duration::ZERO);
        let sleep = |by: Duration| slept.set(slept.get() + by);
        let ready = || {
            polls.set(polls.get() + 1);
            started.get() && polls.get() > 5
        };
        let start = || started.set(true);
        assert!(wait_for_drive(&ready, Some(&start), DRIVE_WAIT, DRIVE_POLL, &sleep));
        assert!(started.get());
        assert_eq!(slept.get(), DRIVE_POLL * 5);

        // An unplugged disk: waited for, not started, given up after two minutes.
        slept.set(Duration::ZERO);
        assert!(!wait_for_drive(&|| false, None, DRIVE_WAIT, DRIVE_POLL, &sleep));
        assert_eq!(slept.get(), DRIVE_WAIT);
        // There already: no wait at all.
        slept.set(Duration::ZERO);
        assert!(wait_for_drive(&|| true, Some(&|| panic!("not started when there")), DRIVE_WAIT, DRIVE_POLL, &sleep));
        assert_eq!(slept.get(), Duration::ZERO);
    }
}
