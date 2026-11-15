//! A running program's file cannot be deleted, so `uninstall.exe` in the
//! install folder does not uninstall by itself: like an NSIS uninstaller, it
//! copies itself to a new folder in %TEMP% and runs the copy with `_?=` (the
//! folder to remove). The copy does the work, tells the original how it went
//! (one line on a pipe), waits for it to exit, then removes it and the
//! folder. The original exits with the copy's code, so whoever ran
//! the uninstaller still learns whether the user confirmed and it worked.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{FAILED, product};

fn copy_prefix() -> String {
    format!("{}-uninstall-", product::BINARY)
}

/// Runs the uninstall from a copy and returns its exit code, or `None` when
/// no copy could be started (nothing has been changed then).
pub fn run_from_temp(dir: &Path) -> Option<i32> {
    let exe = std::env::current_exe().ok()?;
    let temp = std::env::temp_dir();
    sweep(&temp);
    // A new folder of our own: nothing else is loaded from next to the copy.
    let folder = temp.join(format!(
        "{}{}",
        copy_prefix(),
        uuid::Uuid::new_v4().simple()
    ));
    let copy = folder.join(product::UNINSTALLER);
    let started = std::fs::create_dir_all(&folder)
        .and_then(|()| std::fs::copy(&exe, &copy))
        .and_then(|_| {
            Command::new(&copy)
                .args(arguments(dir))
                .current_dir(&folder)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        });
    let mut child = match started {
        Ok(child) => child,
        Err(_) => {
            let _ = std::fs::remove_dir_all(&folder);
            return None;
        }
    };

    let mut line = String::new();
    if let Some(pipe) = child.stdout.take() {
        let _ = BufReader::new(pipe).read_line(&mut line);
    }
    match line.trim().parse::<i32>() {
        // The copy is now waiting for this process to exit.
        Ok(code) => Some(code),
        // It stopped without a word (it crashed): its exit code will do.
        Err(_) => Some(
            child
                .wait()
                .ok()
                .and_then(|status| status.code())
                .unwrap_or(FAILED),
        ),
    }
}

/// The caller and the install folder. The folder goes last because `_?=`
/// takes the rest of the command line.
fn arguments(dir: &Path) -> [String; 2] {
    [
        format!("--parent={}", std::process::id()),
        format!("_?={}", dir.display()),
    ]
}

/// In the copy: tells the original uninstaller the exit code.
pub fn report(code: i32) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{code}");
    let _ = out.flush();
}

/// Removes the copies earlier uninstalls left in %TEMP%. One still running
/// cannot have its program deleted, and is left alone.
fn sweep(temp: &Path) {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return;
    };
    let prefix = copy_prefix();
    let old: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    for folder in old {
        let program = folder.join(product::UNINSTALLER);
        let free = match std::fs::remove_file(&program) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        if free {
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_gets_the_parent_and_folder_without_removal_flags() {
        let args = arguments(Path::new(r"C:\My Apps\MakeYourLifeEasier"));
        let parsed = crate::cli::parse(args);
        assert!(!parsed.silent && !parsed.passive);
        assert_eq!(parsed.parent, Some(std::process::id()));
        assert_eq!(
            parsed.in_place,
            Some(PathBuf::from(r"C:\My Apps\MakeYourLifeEasier"))
        );
    }

    #[test]
    fn earlier_copies_are_swept_and_other_folders_kept() {
        let temp = std::env::temp_dir().join(format!("myle-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let old = temp.join(format!("{}1", copy_prefix()));
        let other = temp.join("SomethingElse-uninstall-1");
        for dir in [&old, &other] {
            std::fs::create_dir_all(dir.join("profile")).unwrap();
            std::fs::write(dir.join(product::UNINSTALLER), b"").unwrap();
        }
        sweep(&temp);
        assert!(!old.exists());
        assert!(other.join(product::UNINSTALLER).exists());
        let _ = std::fs::remove_dir_all(&temp);
    }
}
