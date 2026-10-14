use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::commands;
use super::engine::Engine;
use super::models::{BackupSchedule, GameSaveStatus, OperationKind, ScheduleWeekday, ScheduledBackupResult};
use super::parser;
use super::settings;
use super::state::{self, GameSavesState};

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

pub(crate) fn is_due(value: &super::models::GameSavesSettings, now: u64) -> bool {
    is_due_with_clock(value, now, &local_seconds)
}

fn is_due_with_clock(
    value: &super::models::GameSavesSettings,
    now: u64,
    local_time: &dyn Fn(u64) -> Option<u64>,
) -> bool {
    if value.schedule.interval_seconds().is_none() {
        return false;
    }
    let Some(local_now) = local_time(now) else { return false };
    let Some((hour, minute)) = value.schedule_time.split_once(':') else { return false };
    let (Ok(hour), Ok(minute)) = (hour.parse::<u64>(), minute.parse::<u64>()) else { return false };
    if hour >= 24 || minute >= 60 {
        return false;
    }
    const DAY: u64 = 24 * 60 * 60;
    let mut slot = local_now / DAY * DAY + hour * 60 * 60 + minute * 60;
    if slot > local_now {
        slot = slot.saturating_sub(DAY);
    }
    if value.schedule == BackupSchedule::Weekly {
        let wanted = match value.schedule_weekday {
            ScheduleWeekday::Sunday => 0,
            ScheduleWeekday::Monday => 1,
            ScheduleWeekday::Tuesday => 2,
            ScheduleWeekday::Wednesday => 3,
            ScheduleWeekday::Thursday => 4,
            ScheduleWeekday::Friday => 5,
            ScheduleWeekday::Saturday => 6,
        };
        // January 1, 1970 was Thursday (4 with Sunday numbered zero).
        let weekday = (slot / DAY + 4) % 7;
        slot = slot.saturating_sub((weekday + 7 - wanted) % 7 * DAY);
    }
    // Both sign-in and app startup may catch up a missed slot. The selected
    // calendar still decides when it is due; repeated starts cannot add runs.
    [value.last_scheduled_attempt, value.schedule_started_at]
        .into_iter()
        .flatten()
        .filter(|last| *last <= now.saturating_add(CLOCK_SLACK))
        .filter_map(local_time)
        .max()
        .is_none_or(|last| slot > last)
}

/// Unix seconds projected onto this PC's local wall clock, matching the
/// offset-free task boundaries. Apply the zone's DST rule for each timestamp,
/// including a last attempt from before a clock change.
fn local_seconds(seconds: u64) -> Option<u64> {
    local_seconds_in_zone(seconds, None)
}

fn local_seconds_in_zone(
    seconds: u64,
    zone: Option<&windows_sys::Win32::System::Time::TIME_ZONE_INFORMATION>,
) -> Option<u64> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime, SystemTimeToFileTime};
    const EPOCH: u64 = 116_444_736_000_000_000;
    let ticks = seconds.checked_mul(10_000_000)?.checked_add(EPOCH)?;
    let utc = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc_date = SYSTEMTIME::default();
    let mut local_date = SYSTEMTIME::default();
    let mut local = FILETIME::default();
    // SAFETY: all output pointers refer to initialized live values; the zone
    // pointer is a live borrowed fixture or null (the computer's current zone).
    let converted = unsafe {
        FileTimeToSystemTime(&utc, &mut utc_date) != 0
            && SystemTimeToTzSpecificLocalTime(zone.map_or(std::ptr::null(), |zone| zone), &utc_date, &mut local_date) != 0
            && SystemTimeToFileTime(&local_date, &mut local) != 0
    };
    if !converted {
        return None;
    }
    let local_ticks = (local.dwHighDateTime as u64) << 32 | local.dwLowDateTime as u64;
    local_ticks.checked_sub(EPOCH).map(|ticks| ticks / 10_000_000)
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

    // These fixtures name wall-clock times, independent of the test PC's zone.
    fn local_time(seconds: u64) -> u64 {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{FileTimeToSystemTime, TzSpecificLocalTimeToSystemTime, SystemTimeToFileTime};
        const EPOCH: u64 = 116_444_736_000_000_000;
        let ticks = seconds * 10_000_000 + EPOCH;
        let local = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let mut utc = FILETIME::default();
        let mut local_date = SYSTEMTIME::default();
        let mut utc_date = SYSTEMTIME::default();
        // SAFETY: pointers refer to initialized live values and a null zone
        // asks for this computer's zone; none of these calls changes it.
        assert!(unsafe {
            FileTimeToSystemTime(&local, &mut local_date) != 0
                && TzSpecificLocalTimeToSystemTime(std::ptr::null(), &local_date, &mut utc_date) != 0
                && SystemTimeToFileTime(&utc_date, &mut utc) != 0
        });
        (((utc.dwHighDateTime as u64) << 32 | utc.dwLowDateTime as u64) - EPOCH) / 10_000_000
    }

    #[test]
    fn a_sign_in_before_the_next_weekly_slot_does_not_run_early() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            // Monday October 12, 2026, noon; next Sunday is October 18.
            last_scheduled_attempt: Some(local_time(1_791_806_400)),
            ..Default::default()
        };
        assert!(!is_due(&value, local_time(1_791_979_200)), "Wednesday sign-in must wait until Sunday 03:00");
    }

    #[test]
    fn enabling_a_future_weekly_schedule_does_not_back_up_the_previous_week() {
        let mut json = serde_json::to_value(GameSavesSettings::default()).unwrap();
        json["schedule"] = "weekly".into();
        json["scheduleStartedAt"] = local_time(1_791_979_200).into(); // Wednesday noon.
        let value: GameSavesSettings = serde_json::from_value(json).unwrap();
        assert!(!is_due(&value, local_time(1_791_979_260)));
        assert!(is_due(&value, local_time(1_792_292_400)), "Sunday 03:00 is the first due slot");
    }

    #[test]
    fn a_weekly_run_missed_in_sleep_is_due_at_the_next_sign_in_or_app_start() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            last_scheduled_attempt: Some(local_time(1_791_115_200)), // October 4 noon.
            ..Default::default()
        };
        assert!(!is_due(&value, local_time(1_791_687_540)), "October 11, 02:59 is too early");
        assert!(is_due(&value, local_time(1_791_979_200)), "October 14 must catch up October 11");
    }

    #[test]
    fn a_new_weekly_slot_is_due_even_after_a_recent_catch_up() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            last_scheduled_attempt: Some(local_time(1_791_685_800)), // Sunday 02:30 catches up last week.
            ..Default::default()
        };
        assert!(is_due(&value, local_time(1_791_687_600)), "Sunday 03:00 is a different slot, even thirty minutes later");
    }

    #[test]
    fn daylight_saving_uses_the_date_of_each_run_and_never_repeats_a_weekly_slot() {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::Time::TIME_ZONE_INFORMATION;
        // Athens: March's last Sunday 03:00 -> 04:00; October's last Sunday
        // 04:00 -> 03:00. This fixture never changes the computer's time zone.
        let zone = TIME_ZONE_INFORMATION {
            Bias: -120,
            DaylightBias: -60,
            StandardDate: SYSTEMTIME { wMonth: 10, wDay: 5, wHour: 4, ..Default::default() },
            DaylightDate: SYSTEMTIME { wMonth: 3, wDay: 5, wHour: 3, ..Default::default() },
            ..Default::default()
        };
        let clock = |seconds| local_seconds_in_zone(seconds, Some(&zone));
        // October 25, 2026: 00:00 UTC is the first 03:00; 03:00 UTC is 05:00.
        assert_eq!(clock(1_792_886_400), Some(1_792_897_200));
        assert_eq!(clock(1_792_897_200), Some(1_792_904_400));
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            last_scheduled_attempt: Some(1_792_886_400),
            ..Default::default()
        };
        assert!(!is_due_with_clock(&value, 1_792_897_200, &clock), "the first 03:00 already covered this Sunday");
    }

    #[test]
    fn duplicate_scheduler_launches_are_suppressed() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Daily,
            last_scheduled_attempt: Some(local_time(1_791_687_600)), // Sunday 03:00.
            ..Default::default()
        };
        assert!(!is_due(&value, local_time(1_791_687_700)));
        assert!(!is_due(&value, local_time(1_791_730_800)), "same afternoon is still the same slot");
        assert!(is_due(&value, local_time(1_791_774_000)), "Monday 03:00 is the next slot");
    }

    #[test]
    fn a_weekday_chosen_right_after_a_run_is_not_skipped() {
        let value = GameSavesSettings {
            schedule: BackupSchedule::Weekly,
            schedule_weekday: ScheduleWeekday::Tuesday,
            last_scheduled_attempt: Some(local_time(1_791_687_600)),
            ..Default::default()
        };
        // Sunday's run, then Tuesday chosen: two days later it runs.
        assert!(is_due(&value, local_time(1_791_860_400)));
        assert!(!is_due(&value, local_time(1_791_691_200)), "an hour later it is the same run");
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
