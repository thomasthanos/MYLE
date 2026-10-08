//! Names and folders, the same as backup_projects writes them, so both apps
//! keep adding to the same backups:
//! `<cloud>\Projects Backup\<AppName>\<YYYY-MM Month>\<AppName>_D<day>_V<n>.zip`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::rules::PARTIAL_PREFIX;

pub(crate) const BACKUP_ROOT_NAME: &str = "Projects Backup";
/// backup_projects' copy of its project list, next to the backups.
pub(crate) const CONFIG_MIRROR_FILE: &str = ".backup-projects.json";
/// A folder backup_projects keeps beside the month folders; never a month.
const PRE_RELEASE_FOLDER: &str = "all - pre release backups";

/// backup_projects names the month folders in Greek; any language is read.
const GREEK_MONTHS: [&str; 12] = [
    "Ιανουάριος",
    "Φεβρουάριος",
    "Μάρτιος",
    "Απρίλιος",
    "Μάιος",
    "Ιούνιος",
    "Ιούλιος",
    "Αύγουστος",
    "Σεπτέμβριος",
    "Οκτώβριος",
    "Νοέμβριος",
    "Δεκέμβριος",
];

/// A calendar date and time (local, unless said otherwise).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
}

/// `2026-10 Οκτώβριος`.
pub(crate) fn month_folder(year: i64, month: u32) -> String {
    let name = GREEK_MONTHS[(month.clamp(1, 12) - 1) as usize];
    format!("{year:04}-{month:02} {name}")
}

/// `(year, month, label)` of a month folder in any language: `2026-10 October`.
pub(crate) fn parse_month_folder(name: &str) -> Option<(i64, u32, String)> {
    let bytes = name.as_bytes();
    if bytes.len() < 7
        || bytes[4] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let year: i64 = name[..4].parse().ok()?;
    let month: u32 = name[5..7].parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let label = name[7..].trim().to_string();
    Some((year, month, label))
}

/// The folders inside a project's backup folder that hold its backups.
pub(crate) fn is_month_folder(name: &str) -> bool {
    !name.eq_ignore_ascii_case(PRE_RELEASE_FOLDER)
        && !name.starts_with(PARTIAL_PREFIX)
        && parse_month_folder(name).is_some()
}

/// `<App>_D5_V3`, older `<App>_V3_D5` and plain `<App>_V3`, with or without
/// `.zip`: `(base name, day, version)`.
pub(crate) fn parse_backup_name(name: &str) -> (String, u32, u32) {
    let base = strip_zip(name).to_string();
    let parts: Vec<&str> = base.rsplit('_').take(2).collect();
    let number = |part: &str, prefix: char| -> Option<u32> {
        let rest = part.strip_prefix(prefix)?;
        (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())).then(|| rest.parse().ok())?
    };
    match parts.as_slice() {
        [last, before] if base.matches('_').count() >= 2 => {
            if let (Some(version), Some(day)) = (number(last, 'V'), number(before, 'D')) {
                return (base, day, version);
            }
            if let (Some(day), Some(version)) = (number(last, 'D'), number(before, 'V')) {
                return (base, day, version);
            }
            if let Some(version) = number(last, 'V') {
                return (base, 0, version);
            }
            (base, 0, 0)
        }
        [last, ..] => {
            let version = number(last, 'V').unwrap_or(0);
            (base, 0, version)
        }
        _ => (base, 0, 0),
    }
}

pub(crate) fn strip_zip(name: &str) -> &str {
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".zip") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// `<App>_D<day>_V<version>`.
pub(crate) fn backup_name(app: &str, day: u32, version: u32) -> String {
    format!("{app}_D{day}_V{version}")
}

/// Whether `name` (file or folder) is a backup of `app`: `<App>_…`, any case.
pub(crate) fn belongs_to(name: &str, app: &str) -> bool {
    name.len() > app.len() + 1
        && name.is_char_boundary(app.len())
        && name[..app.len()].eq_ignore_ascii_case(app)
        && name.as_bytes()[app.len()] == b'_'
        && !name.starts_with(PARTIAL_PREFIX)
}

/// One more than the highest version of `app` in any month folder of
/// `app_dir`: numbering carries on across months and both apps.
pub(crate) fn next_version(app_dir: &Path, app: &str) -> u32 {
    let mut highest = 0;
    let Ok(months) = fs::read_dir(app_dir) else {
        return 1;
    };
    for month in months.flatten() {
        let month_name = month.file_name().to_string_lossy().into_owned();
        if month_name.eq_ignore_ascii_case(PRE_RELEASE_FOLDER)
            || month_name.starts_with(PARTIAL_PREFIX)
        {
            continue;
        }
        if !month.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(month.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if belongs_to(&name, app) {
                highest = highest.max(parse_backup_name(&name).2);
            }
        }
    }
    highest + 1
}

/// Leftovers of interrupted backups (`__partial__…`) older than `age`; a
/// fresh one may belong to a backup running in the other app right now.
pub(crate) fn remove_stale_partials(app_dir: &Path, age: Duration) {
    let Ok(months) = fs::read_dir(app_dir) else {
        return;
    };
    let now = SystemTime::now();
    for month in months.flatten() {
        if !month.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(month.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(PARTIAL_PREFIX) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|elapsed| elapsed >= age);
            if old && entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

// ─── Time ───────────────────────────────────────────────────────────────────

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn civil_from_unix_millis(millis: i64) -> Civil {
    let days = millis.div_euclid(86_400_000);
    let rest = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (rest / 3_600_000) as u32,
        minute: (rest / 60_000 % 60) as u32,
        second: (rest / 1000 % 60) as u32,
        millis: (rest % 1000) as u32,
    }
}

pub(crate) fn unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    }
}

pub(crate) fn utc(time: SystemTime) -> Civil {
    civil_from_unix_millis(unix_millis(time))
}

/// The same moment on this PC's clock.
pub(crate) fn local(time: SystemTime) -> Civil {
    civil_from_unix_millis(unix_millis(time) + local_offset_millis(time))
}

#[cfg(windows)]
fn local_offset_millis(time: SystemTime) -> i64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    // 100 ns ticks since 1601-01-01.
    const TICKS_TO_UNIX: i64 = 116_444_736_000_000_000;
    let ticks = unix_millis(time) * 10_000 + TICKS_TO_UNIX;
    if ticks < 0 {
        return 0;
    }
    let utc = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    // SAFETY: both pointers refer to live FILETIME values.
    if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } == 0 {
        return 0;
    }
    let local_ticks = ((local.dwHighDateTime as i64) << 32) | local.dwLowDateTime as i64;
    (local_ticks - ticks) / 10_000
}

#[cfg(not(windows))]
fn local_offset_millis(_time: SystemTime) -> i64 {
    0
}

/// `2026-10-08T09:39:05.123Z`, as JavaScript's `toISOString`.
pub(crate) fn iso_utc(time: SystemTime) -> String {
    let c = utc(time);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second, c.millis
    )
}

/// `8/10/2026, 12:39:05 μ.μ.`, as backup_projects shows dates (el-GR).
pub(crate) fn greek_display(c: Civil) -> String {
    let (hour, suffix) = match c.hour {
        0 => (12, "π.μ."),
        1..=11 => (c.hour, "π.μ."),
        12 => (12, "μ.μ."),
        _ => (c.hour - 12, "μ.μ."),
    };
    format!(
        "{}/{}/{}, {}:{:02}:{:02} {}",
        c.day, c.month, c.year, hour, c.minute, c.second, suffix
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_folders_are_greek_and_any_language_is_read() {
        assert_eq!(month_folder(2026, 10), "2026-10 Οκτώβριος");
        assert_eq!(month_folder(2025, 3), "2025-03 Μάρτιος");
        assert_eq!(
            parse_month_folder("2026-10 Οκτώβριος"),
            Some((2026, 10, "Οκτώβριος".into()))
        );
        assert_eq!(
            parse_month_folder("2026-10 October"),
            Some((2026, 10, "October".into()))
        );
        assert_eq!(
            parse_month_folder("2026-10"),
            Some((2026, 10, String::new()))
        );
        assert_eq!(parse_month_folder("2026-13 Nope"), None);
        assert_eq!(parse_month_folder("all - pre release backups"), None);
        assert!(!is_month_folder("__partial__2026-10"));
    }

    #[test]
    fn backup_names_of_every_generation_are_parsed() {
        assert_eq!(
            parse_backup_name("steam_idler_D9_V10.zip"),
            ("steam_idler_D9_V10".into(), 9, 10)
        );
        assert_eq!(parse_backup_name("App_V3_D5"), ("App_V3_D5".into(), 5, 3));
        assert_eq!(parse_backup_name("App_V7.ZIP"), ("App_V7".into(), 0, 7));
        assert_eq!(parse_backup_name("App_backup"), ("App_backup".into(), 0, 0));
        assert_eq!(backup_name("App", 8, 12), "App_D8_V12");
        assert!(belongs_to("steam_idler_D9_V10.zip", "steam_idler"));
        assert!(belongs_to("STEAM_IDLER_D9_V10.zip", "steam_idler"));
        assert!(!belongs_to("steam_idler2_D9_V10.zip", "steam_idler"));
        assert!(!belongs_to(
            "__partial__steam_idler_D9_V10__1.zip",
            "__partial__steam"
        ));
    }

    #[test]
    fn numbering_continues_across_months_and_ignores_partials() {
        let root = std::env::temp_dir().join(format!("myle-naming-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("2026-09 Σεπτέμβριος")).unwrap();
        fs::create_dir_all(root.join("2026-10 October")).unwrap();
        fs::create_dir_all(root.join("all - pre release backups")).unwrap();
        fs::write(
            root.join("2026-09 Σεπτέμβριος").join("App_D30_V10.zip"),
            b"x",
        )
        .unwrap();
        fs::create_dir_all(root.join("2026-09 Σεπτέμβριος").join("App_D29_V9")).unwrap();
        fs::write(
            root.join("2026-10 October")
                .join("__partial__App_D1_V40__1.zip"),
            b"x",
        )
        .unwrap();
        fs::write(root.join("2026-10 October").join("Other_D1_V50.zip"), b"x").unwrap();
        fs::write(
            root.join("all - pre release backups")
                .join("App_D1_V99.zip"),
            b"x",
        )
        .unwrap();
        assert_eq!(next_version(&root, "App"), 11);
        assert_eq!(next_version(&root.join("missing"), "App"), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dates_match_javascript() {
        let time = UNIX_EPOCH + Duration::from_millis(1_791_452_345_123);
        assert_eq!(iso_utc(time), "2026-10-08T09:39:05.123Z");
        let c = utc(time);
        assert_eq!((c.year, c.month, c.day, c.hour), (2026, 10, 8, 9));
        assert_eq!(greek_display(c), "8/10/2026, 9:39:05 π.μ.");
        assert_eq!(
            greek_display(Civil { hour: 0, ..c }),
            "8/10/2026, 12:39:05 π.μ."
        );
        assert_eq!(
            greek_display(Civil { hour: 15, ..c }),
            "8/10/2026, 3:39:05 μ.μ."
        );
        assert_eq!(utc(UNIX_EPOCH + Duration::from_secs(951_782_400)).day, 29); // 2000-02-29
    }
}
