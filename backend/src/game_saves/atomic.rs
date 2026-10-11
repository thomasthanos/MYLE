use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

/// Writes a complete file and atomically swaps it into place. An existing
/// destination is replaced with `ReplaceFileW`, which keeps its attributes and
/// security, and moves the old contents to a backup name first, so a failure
/// part-way (antivirus or the indexer holding the file) can always be undone:
/// the destination is never left missing, and the only copy is never deleted.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid file path")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = temporary_path(path, "tmp");
    let backup = temporary_path(path, "old");
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);

        if !present(path) {
            return fs::rename(&temp, path).map_err(|error| error.to_string());
        }

        let replaced = wide(path);
        let replacement = wide(&temp);
        let old = wide(&backup);
        // SAFETY: the three UTF-16 buffers are NUL-terminated and remain alive
        // for the duration of the call; the reserved pointers are null.
        let ok = unsafe {
            ReplaceFileW(
                replaced.as_ptr(),
                replacement.as_ptr(),
                old.as_ptr(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            return settle_failed_replace(path, &temp, &backup).and(Err(error.to_string()));
        }
        let _ = fs::remove_file(&backup);
        Ok(())
    })();
    // The new file is dropped only while the destination is there: after a
    // failed replace it may be the only copy left.
    if result.is_err() && present(path) {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Puts things right after `ReplaceFileW` failed. Whatever its error said,
/// what counts is what is on disk: if the destination is gone, the new file
/// takes its place, or else the old contents come back from the backup name.
/// Answers `Ok` when the destination is there again (with old or new data).
fn settle_failed_replace(path: &Path, temp: &Path, backup: &Path) -> Result<(), String> {
    if present(path) {
        // Untouched: a stray backup of it is not needed.
        if present(backup) && present(temp) {
            let _ = fs::remove_file(backup);
        }
        return Ok(());
    }
    if present(temp) && fs::rename(temp, path).is_ok() {
        let _ = fs::remove_file(backup);
        return Ok(());
    }
    if present(backup) {
        return fs::rename(backup, path).map_err(|error| error.to_string());
    }
    Err(format!("{} could not be written and is missing", path.display()))
}

/// Whether something is at `path`; when that cannot be told (no access), it
/// counts as there, so nothing that might be the only copy is deleted.
fn present(path: &Path) -> bool {
    path.try_exists().unwrap_or(true)
}

fn temporary_path(path: &Path, kind: &str) -> PathBuf {
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("{kind}-{}-{sequence}", std::process::id()))
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomically_creates_and_replaces_a_file() {
        let root = std::env::temp_dir().join(format!(
            "myle-game-saves-atomic-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("settings.json");
        write(&path, b"first").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1, "no temporary or backup file is left");
        let _ = fs::remove_dir_all(root);
    }

    fn scene(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("myle-atomic-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        (root.clone(), path.clone(), path.with_extension("tmp-1"), path.with_extension("old-1"))
    }

    #[test]
    fn a_replace_that_removed_the_destination_puts_the_new_file_in_its_place() {
        let (root, path, temp, backup) = scene("moved");
        fs::write(&temp, b"new").unwrap();
        fs::write(&backup, b"old").unwrap();
        settle_failed_replace(&path, &temp, &backup).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!temp.exists() && !backup.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn when_the_new_file_is_gone_too_the_old_contents_come_back() {
        let (root, path, temp, backup) = scene("restored");
        fs::write(&backup, b"old").unwrap();
        settle_failed_replace(&path, &temp, &backup).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_untouched_destination_is_left_as_it_was() {
        let (root, path, temp, backup) = scene("untouched");
        fs::write(&path, b"old").unwrap();
        fs::write(&temp, b"new").unwrap();
        settle_failed_replace(&path, &temp, &backup).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"old");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nothing_left_to_put_back_is_an_error_not_a_silent_loss() {
        let (root, path, temp, backup) = scene("lost");
        assert!(settle_failed_replace(&path, &temp, &backup).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
