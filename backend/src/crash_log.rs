//! A panic is written down before its thread unwinds: `internal\crash.log`
//! in the local data folder keeps the latest ones (the one before is kept as
//! `crash.old.log` once the log grows past 256 KB), so a feature that failed
//! can be traced afterwards. Rust's own message still goes to stderr. Only
//! processes running as the user write it, never an elevated helper.
//!
//! The release build unwinds (`panic = "unwind"`): a panic ends the command
//! or thread it happened in, and the cleanup guards on the way (rollbacks,
//! zeroized keys) still run, instead of the whole app closing at once.
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
const FILE: &str = "crash.log";
const OLD: &str = "crash.old.log";
const LIMIT: u64 = 256 * 1024;
/// How much of a panic's message is kept.
const MESSAGE_CHARS: usize = 2000;
/// Installs the hook, once, before anything else runs. An elevated helper
/// writes nothing: the folder belongs to the user, and an administrator
/// appending to (or renaming) files there would follow any link planted in
/// it.
pub fn install() {
    if crate::elevated_pipe::is_elevated(std::process::id()) {
        return;
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(folder) = folder() {
            let _ = append(&folder, &entry(info, SystemTime::now()));
        }
        default(info);
    }));
}
fn folder() -> Option<PathBuf> {
    Some(crate::storage::local_dir().ok()?.join("internal"))
}
fn entry(info: &PanicHookInfo<'_>, at: SystemTime) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)");
    let place = info.location().map(|place| format!("{}:{}", place.file(), place.line())).unwrap_or_default();
    let thread = std::thread::current();
    // Which part of MYLE: the app, or a helper started with its flag.
    let role = std::env::args().nth(1).filter(|flag| flag.starts_with("--")).unwrap_or_else(|| "app".into());
    format!(
        "{} MYLE {} ({role}, thread {}) panicked at {place}: {}\n",
        crate::project_backups::naming::iso_utc(at),
        env!("CARGO_PKG_VERSION"),
        thread.name().unwrap_or("unnamed"),
        message.chars().take(MESSAGE_CHARS).collect::<String>().replace(['\r', '\n'], " ")
    )
}
fn append(folder: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let file = folder.join(FILE);
    if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > LIMIT) {
        let _ = std::fs::rename(&file, folder.join(OLD));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(&file)?.write_all(line.as_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_panic_is_written_down_and_a_long_log_is_put_aside() {
        let folder = std::env::temp_dir().join(format!("myle-crash-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        append(&folder, "first\n").unwrap();
        append(&folder, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(folder.join(FILE)).unwrap(), "first\nsecond\n");
        std::fs::write(folder.join(FILE), vec![b'x'; LIMIT as usize + 1]).unwrap();
        append(&folder, "third\n").unwrap();
        assert_eq!(std::fs::read_to_string(folder.join(FILE)).unwrap(), "third\n");
        assert_eq!(std::fs::metadata(folder.join(OLD)).unwrap().len(), LIMIT + 1);
        let _ = std::fs::remove_dir_all(&folder);
    }
    #[test]
    fn an_entry_names_the_place_the_thread_and_the_message_on_one_line() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let kept = seen.clone();
        let previous = std::panic::take_hook();
        // Other tests run meanwhile: only this test's thread is recorded.
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() == Some("worker") {
                *kept.lock().unwrap() = entry(info, SystemTime::UNIX_EPOCH);
            }
        }));
        let _ = std::thread::Builder::new()
            .name("worker".into())
            .spawn(|| panic!("bad\nthing {}", 7))
            .unwrap()
            .join();
        std::panic::set_hook(previous);
        let line = seen.lock().unwrap().clone();
        assert!(line.starts_with("1970-01-01"), "{line}");
        assert!(line.contains("thread worker") && line.contains("crash_log.rs:") && line.ends_with("bad thing 7\n"), "{line}");
        assert_eq!(line.lines().count(), 1);
    }
}
