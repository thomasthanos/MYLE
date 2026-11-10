//! Folders for tests, all under `%TEMP%\myle-tests`, one subfolder per test run. A run removes the
//! folders of runs that ended over an hour ago, so test runs no longer pile up in `%TEMP%`: they
//! had left hundreds of folders there, one per test that wrote a file.
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;
const PARENT: &str = "myle-tests";
/// Another run's folder is left alone this long, in case that run is still going.
const KEEP: Duration = Duration::from_secs(3600);
static SWEEP: Once = Once::new();
/// A new, empty folder for one test.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let parent = std::env::temp_dir().join(PARENT);
    let run = std::process::id().to_string();
    SWEEP.call_once(|| sweep(&parent, &run, KEEP));
    let dir = parent.join(run).join(format!("{name}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).expect("a folder for the test");
    dir
}
/// Removes the folders of other runs untouched for `keep`.
fn sweep(parent: &Path, mine: &str, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy() == mine {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age >= keep);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_folder_is_new_and_empty() {
        let (one, two) = (temp_dir("x"), temp_dir("x"));
        assert_ne!(one, two);
        assert!(one.is_dir() && std::fs::read_dir(&one).unwrap().next().is_none());
        assert!(one.starts_with(std::env::temp_dir().join(PARENT)));
    }
    #[test]
    fn old_runs_are_swept_and_this_one_and_recent_ones_are_kept() {
        let parent = temp_dir("sweep");
        for run in ["mine", "earlier"] {
            std::fs::create_dir_all(parent.join(run).join("data")).unwrap();
        }
        sweep(&parent, "mine", Duration::from_secs(3600));
        assert!(parent.join("earlier").exists(), "too recent to be sure it ended");
        sweep(&parent, "mine", Duration::ZERO);
        assert!(!parent.join("earlier").exists());
        assert!(parent.join("mine").join("data").exists(), "never this run's own");
    }
}
