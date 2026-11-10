//! Keeps the web views' caches from piling up. A WebView2 profile keeps every picture it loaded and
//! the compiled scripts of every version that ran: on the owner's PC that was 324 MB of a 397 MB
//! profile. Only caches go, never local storage, cookies, sign-ins or settings.
//!
//! A profile in use is left for the next start: a running web view (the previous version during
//! an update, or a second start) keeps its lock file open. Each cache folder is moved aside in one
//! rename and emptied in the background, so starting does not wait on thousands of files.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
/// A profile's caches, relative to its folder. Chromium rebuilds any of them.
const CACHES: [&str; 8] = [
    r"Default\Cache",
    r"Default\Code Cache",
    r"Default\GPUCache",
    r"Default\DawnWebGPUCache",
    r"Default\DawnGraphiteCache",
    "GrShaderCache",
    "GraphiteDawnCache",
    "ShaderCache",
];
/// Marks a folder moved aside, so a start that stopped halfway finishes the job next time.
const SET_ASIDE: &str = ".myle-cleared-";
/// Pictures are fetched again soon enough: two weeks keeps them from piling up between updates.
const EVERY_SECS: u64 = 14 * 86_400;
/// When the caches were last cleared, and by which version.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Cleared {
    pub version: String,
    pub at: u64,
}
/// After an update, the old version's compiled scripts are of no use, and pictures are kept two weeks.
pub(crate) fn due(last: Option<&Cleared>, version: &str, now: u64) -> bool {
    last.is_none_or(|last| last.version != version || now.saturating_sub(last.at) >= EVERY_SECS)
}
/// A running web view keeps its profile's lock file open.
pub(crate) fn in_use(profile: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    let lock = profile.join("lockfile");
    // Opening it for ourselves alone fails while anyone else has it open.
    lock.exists() && std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&lock).is_err()
}
/// Moves this profile's caches aside. `false` when the profile is in use and nothing was touched.
pub(crate) fn set_aside(profile: &Path) -> bool {
    if !profile.is_dir() {
        return true;
    }
    if in_use(profile) {
        return false;
    }
    for cache in CACHES {
        let folder = profile.join(cache);
        let Some(name) = folder.file_name().map(|name| name.to_string_lossy().into_owned()) else {
            continue;
        };
        if folder.is_dir() {
            let aside = folder.with_file_name(format!("{name}{SET_ASIDE}{}", uuid::Uuid::new_v4().simple()));
            let _ = std::fs::rename(&folder, aside);
        }
    }
    true
}
/// Deletes what was moved aside, by this start or one that stopped before finishing.
pub(crate) fn empty(profile: &Path) {
    for folder in [profile.to_path_buf(), profile.join("Default")] {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().contains(SET_ASIDE) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}
/// Clears these profiles' caches when due. It counts as done, and is recorded, only once every one
/// of them could be cleared.
pub(crate) fn clear_when_due(profiles: &[PathBuf], record: &Path, version: &str, now: u64) -> bool {
    let last: Option<Cleared> = std::fs::read(record).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if !due(last.as_ref(), version, now) {
        return false;
    }
    // Every profile is tried, also after one that is in use.
    let results: Vec<bool> = profiles.iter().map(|profile| set_aside(profile)).collect();
    let cleared = results.iter().all(|done| *done);
    if cleared {
        if let Some(folder) = record.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(bytes) = serde_json::to_vec(&Cleared { version: version.to_string(), at: now }) {
            let _ = crate::game_saves::atomic::write(record, &bytes);
        }
    }
    cleared
}
#[cfg(test)]
mod tests {
    use std::os::windows::fs::OpenOptionsExt;
    use super::*;
    const DAY: u64 = 86_400;
    /// A profile as WebView2 leaves it: caches next to what must stay.
    fn profile() -> PathBuf {
        let root = crate::test_dirs::temp_dir("webview-cache");
        for folder in [r"Default\Cache\Cache_Data", r"Default\Code Cache\js", r"Default\Local Storage\leveldb", r"Default\Network", "GrShaderCache"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join(r"Default\Cache\Cache_Data\data_0"), b"picture").unwrap();
        std::fs::write(root.join(r"Default\Code Cache\js\index"), b"script").unwrap();
        std::fs::write(root.join(r"Default\Local Storage\leveldb\000003.log"), b"settings").unwrap();
        std::fs::write(root.join(r"Default\Network\Cookies"), b"cookies").unwrap();
        std::fs::write(root.join(r"Default\Preferences"), b"{}").unwrap();
        root
    }
    /// Holds the lock file the way a running web view does.
    fn hold(profile: &Path) -> std::fs::File {
        std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(1 | 2).open(profile.join("lockfile")).unwrap()
    }
    fn leftovers(profile: &Path) -> usize {
        [profile.to_path_buf(), profile.join("Default")]
            .iter()
            .flat_map(|folder| std::fs::read_dir(folder).unwrap().flatten())
            .filter(|entry| entry.file_name().to_string_lossy().contains(SET_ASIDE))
            .count()
    }
    #[test]
    fn caches_go_and_everything_else_stays() {
        let profile = profile();
        assert!(set_aside(&profile));
        empty(&profile);
        assert!(!profile.join(r"Default\Cache").exists() && !profile.join(r"Default\Code Cache").exists() && !profile.join("GrShaderCache").exists());
        assert_eq!(std::fs::read(profile.join(r"Default\Local Storage\leveldb\000003.log")).unwrap(), b"settings");
        assert_eq!(std::fs::read(profile.join(r"Default\Network\Cookies")).unwrap(), b"cookies");
        assert!(profile.join(r"Default\Preferences").exists());
        assert_eq!(leftovers(&profile), 0);
    }
    #[test]
    fn moving_aside_is_quick_and_the_files_go_later() {
        let profile = profile();
        assert!(set_aside(&profile));
        assert!(!profile.join(r"Default\Cache").exists());
        assert!(leftovers(&profile) >= 3, "moved, not yet deleted");
    }
    #[test]
    fn a_profile_in_use_is_left_alone() {
        let profile = profile();
        let _held = hold(&profile);
        assert!(in_use(&profile));
        assert!(!set_aside(&profile));
        assert_eq!(std::fs::read(profile.join(r"Default\Cache\Cache_Data\data_0")).unwrap(), b"picture");
    }
    #[test]
    fn a_lock_file_left_by_a_closed_web_view_does_not_count() {
        let profile = profile();
        drop(hold(&profile));
        assert!(!in_use(&profile));
        assert!(set_aside(&profile));
    }
    #[test]
    fn a_profile_never_created_is_nothing_to_do() {
        assert!(set_aside(&profile().join("never")));
    }
    #[test]
    fn a_start_that_stopped_halfway_is_finished_later() {
        let profile = profile();
        std::fs::create_dir_all(profile.join(format!(r"Default\Cache{SET_ASIDE}earlier\Cache_Data"))).unwrap();
        empty(&profile);
        assert_eq!(leftovers(&profile), 0);
        assert!(profile.join(r"Default\Cache").exists(), "only what was set aside goes");
    }
    #[test]
    fn caches_are_due_after_an_update_or_two_weeks() {
        let last = Cleared { version: "11.0.0".to_string(), at: 1000 };
        assert!(due(None, "11.0.0", 1000));
        assert!(!due(Some(&last), "11.0.0", 1000 + 13 * DAY));
        assert!(due(Some(&last), "11.0.0", 1000 + 14 * DAY));
        assert!(due(Some(&last), "11.0.1", 1001));
    }
    #[test]
    fn clearing_is_recorded_only_once_every_profile_could_be_cleared() {
        let (free, busy) = (profile(), profile());
        let record = crate::test_dirs::temp_dir("webview-cache-record").join("webview-caches.json");
        let held = hold(&busy);
        assert!(!clear_when_due(&[free.clone(), busy.clone()], &record, "11.0.0", 1000));
        assert!(!record.exists(), "tried again at the next start");
        drop(held);
        assert!(clear_when_due(&[free.clone(), busy.clone()], &record, "11.0.0", 1000));
        assert!(!busy.join(r"Default\Cache").exists());
        assert!(!clear_when_due(&[free, busy], &record, "11.0.0", 1000 + DAY), "not due again for a while");
    }
}
