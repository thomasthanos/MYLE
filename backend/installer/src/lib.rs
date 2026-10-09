//! Make Your Life Easier's own setup and uninstaller.
//!
//! One crate, two programs sharing everything but the payload:
//! - `setup.exe` carries the app (see `payload`) and installs it per user,
//!   with a window (`ui`), a progress-only window (`/P`, which the in-app
//!   updater uses as `/P /UPDATE /R`) or none (`/S`);
//! - `uninstall.exe` is installed next to the app and removes it again,
//!   running from a copy of itself in %TEMP% (see `relocate`).

mod cleanup;
mod cli;
mod engine;
pub mod payload;
mod processes;
mod product;
mod registry;
mod relocate;
mod shell;
#[path = "../../src/shown_window.rs"]
mod shown_window;
mod ui;
mod webview2;
#[path = "../../src/webview_policy.rs"]
mod webview_policy;

use std::path::{Path, PathBuf};
use std::time::Duration;

use cleanup::AfterExit;
use cli::Cli;

/// Exit codes of the silent modes (0 is success).
const FAILED: i32 = 1;
const DECLINED: i32 = 2;
const ALREADY_RUNNING: i32 = 3;
const STILL_RUNNING: i32 = 4;

pub fn setup_main(payload: &'static [u8]) -> i32 {
    let cli = cli::parse(std::env::args().skip(1));
    let Some(_lock) = shell::SingleInstance::acquire() else {
        return already_running(&cli);
    };
    if payload.is_empty() {
        if !cli.silent {
            shell::message(
                product::NAME,
                "This setup was built without the app inside.",
                true,
            );
        }
        return FAILED;
    }
    if cli.silent {
        return silent_install(payload, &cli);
    }
    if !webview2::installed() {
        let ok = shell::ask(
            &format!("{} Setup", product::NAME),
            &format!(
                "{} needs the Microsoft Edge WebView2 Runtime, which is not on this PC yet.\n\n\
                 Download and install it from Microsoft now?",
                product::NAME
            ),
        );
        if !ok {
            return DECLINED;
        }
        if let Err(error) = webview2::install(false) {
            shell::message(product::NAME, &error, true);
            return FAILED;
        }
    }
    let (code, after_exit) = ui::run(ui::Mode::Install { payload }, cli);
    after_exit.finish();
    code
}

/// `/S`: no window. The updater waits for nothing and reads no output, so
/// the exit code is the only report.
fn silent_install(payload: &'static [u8], cli: &Cli) -> i32 {
    let dir = cli
        .dir
        .clone()
        .or_else(registry::install_dir)
        .unwrap_or_else(engine::default_dir);
    // `/LIVE`: the app stays open and restarts itself once we are done.
    let live = cli.live && cli.update;
    // Otherwise the updater quits the app right after starting us: give it time.
    let grace = if cli.update {
        Duration::from_secs(20)
    } else {
        Duration::from_secs(3)
    };
    if !live && engine::close_running(&dir, grace).is_err() {
        return STILL_RUNNING;
    }
    if !cli.update && !webview2::installed() && webview2::install(true).is_err() {
        return FAILED;
    }
    let shortcuts = if cli.no_shortcuts {
        engine::ShortcutMode::RefreshOnly
    } else if cli.update {
        engine::ShortcutMode::Keep
    } else {
        engine::ShortcutMode::Choose(engine::ShortcutChoice::NEW_INSTALL)
    };
    let options = engine::InstallOptions {
        dir,
        shortcuts,
        live,
    };
    // A shortcut Windows refused does not fail a silent install: the app is
    // in place, and the updater could do nothing about it anyway.
    match engine::install(payload, &options, &mut |_| {}) {
        Ok(installed) => {
            if cli.relaunch && !live {
                let _ = shell::launch(&installed.exe, cli.update);
            }
            0
        }
        Err(_) => FAILED,
    }
}

pub fn uninstall_main() -> i32 {
    let cli = cli::parse(std::env::args().skip(1));
    let not_installed = || {
        if !cli.silent {
            shell::message(
                product::NAME,
                &format!("{} is not installed.", product::NAME),
                false,
            );
        }
        FAILED
    };

    // The copy in %TEMP% (or a caller that asked, as NSIS allows, for no copy).
    if let Some(dir) = cli.in_place.clone() {
        let parent = cli.parent.and_then(|pid| {
            processes::spare(pid);
            processes::Process::open(pid)
        });
        if !has_app(&dir) {
            if parent.is_some() {
                relocate::report(FAILED);
            }
            return not_installed();
        }
        let Some(_lock) = shell::SingleInstance::acquire() else {
            if parent.is_some() {
                relocate::report(ALREADY_RUNNING);
            }
            return already_running(&cli);
        };
        let (code, after_exit) = uninstall_from(&cli, dir);
        if let Some(parent) = parent {
            relocate::report(code);
            // Its file, and so the folder, can only go once it has exited.
            parent.wait(Duration::from_secs(60));
        }
        after_exit.finish();
        return code;
    }

    let Some(dir) = install_dir_to_remove() else {
        return not_installed();
    };
    if let Some(code) = relocate::run_from_temp(&cli, &dir) {
        return code;
    }
    // No copy could be started: uninstall from here, leaving this program
    // (and so its folder) behind.
    let Some(_lock) = shell::SingleInstance::acquire() else {
        return already_running(&cli);
    };
    let (code, after_exit) = uninstall_from(&cli, dir);
    after_exit.finish();
    code
}

fn already_running(cli: &Cli) -> i32 {
    if !cli.silent {
        shell::message(product::NAME, "Setup is already running.", false);
    }
    ALREADY_RUNNING
}

/// Removes the app in `dir`, with or without a window. Returns the exit code
/// and what can only be deleted later.
fn uninstall_from(cli: &Cli, dir: PathBuf) -> (i32, AfterExit) {
    // Without WebView2 there is no window: ask with a plain message box.
    let windowless = !cli.silent && !webview2::installed();
    if windowless
        && !shell::ask(
            &format!("Uninstall {}", product::NAME),
            &format!("Remove {} from this PC?", product::NAME),
        )
    {
        return (DECLINED, AfterExit::default());
    }
    if cli.silent || windowless {
        if engine::close_running(&dir, Duration::from_secs(3)).is_err() {
            return (STILL_RUNNING, AfterExit::default());
        }
        return match engine::uninstall(&dir, cli.purge, &mut |_| {}) {
            Ok(after_exit) => (0, after_exit),
            Err(_) => (FAILED, AfterExit::default()),
        };
    }
    ui::run(ui::Mode::Uninstall { dir }, cli.clone())
}

fn has_app(dir: &Path) -> bool {
    dir.join(product::exe_name()).is_file()
}

/// The registered install, or the folder this uninstaller sits in if the
/// app is there (an install whose registry entry is already gone).
fn install_dir_to_remove() -> Option<PathBuf> {
    registry::install_dir()
        .filter(|dir| has_app(dir))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(PathBuf::from))
                .filter(|dir| has_app(dir))
        })
}
