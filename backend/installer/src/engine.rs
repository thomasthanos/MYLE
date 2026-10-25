//! Installing and uninstalling, without any window: the UI and the silent
//! command line both drive these and only differ in how they show progress.
//!
//! An install is all-or-nothing for the files: each one is written next to
//! its target, the old one is moved aside, and only when every file is in
//! place are the old ones deleted. A failure part-way puts the previous
//! version back, so an interrupted update never leaves a broken app.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cleanup::AfterExit;
use crate::shell::{self, Shortcut};
use crate::{payload, processes, product, registry};

/// Progress for the window (or nobody, when silent).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum Progress {
    Stage {
        stage: Stage,
    },
    /// Bytes written so far out of the whole install, and the current file.
    Files {
        done: u64,
        total: u64,
        file: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    ClosingApp,
    Preparing,
    Copying,
    Registering,
    Shortcuts,
    RemovingFiles,
    RemovingData,
    Finishing,
}

/// What the install does with the Desktop, Start menu and Startup shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutMode {
    /// Exactly these: each one ticked is created (or repaired), each one of
    /// ours left unticked is removed. Remembered for the next install.
    Choose(ShortcutChoice),
    /// An update: follows what the user wants now (`current_choice`). A
    /// Desktop and Start Menu shortcuts are repaired if missing. Explicit
    /// setup opt-outs and the Startup preference from Settings are kept. Nothing is
    /// removed. An install from before shortcuts were remembered gets the
    /// ones `migrate_shortcuts` picks.
    Keep,
    /// `/NS`: refresh the ones that exist, add nothing.
    RefreshOnly,
}

/// One flag per `Shortcut`, in `Shortcut::ALL` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShortcutChoice {
    pub desktop: bool,
    pub start_menu: bool,
    pub startup: bool,
}

impl ShortcutChoice {
    /// A new install: Desktop and Start menu. Starting with Windows is
    /// opt-in; nothing needs it (Game Saves' scheduled backups run on their
    /// own, from a scheduled task).
    pub const NEW_INSTALL: Self = Self {
        desktop: true,
        start_menu: true,
        startup: false,
    };

    pub const fn from_array([desktop, start_menu, startup]: [bool; 3]) -> Self {
        Self {
            desktop,
            start_menu,
            startup,
        }
    }

    pub const fn to_array(self) -> [bool; 3] {
        [self.desktop, self.start_menu, self.startup]
    }

    pub fn wants(self, kind: Shortcut) -> bool {
        match kind {
            Shortcut::Desktop => self.desktop,
            Shortcut::StartMenu => self.start_menu,
            Shortcut::Startup => self.startup,
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub dir: PathBuf,
    pub shortcuts: ShortcutMode,
    /// The in-app updater's seamless update (`/LIVE`): the app is still
    /// running. Files are swapped by renaming, which Windows allows even for
    /// a running program, and the old copies still in use stay behind as
    /// `*.myle-old` for the new version to sweep (see `sweep_leftovers`).
    pub live: bool,
}

/// A finished install. The app is in place either way; `warnings` names
/// the shortcuts Windows would not take, which the window shows.
#[derive(Debug)]
pub struct Installed {
    pub exe: PathBuf,
    pub warnings: Vec<String>,
}

/// What the install folder holds, written by the setup: the uninstaller and
/// the next update remove exactly these files and nothing else.
#[derive(Debug, Default, Serialize, Deserialize)]
struct InstalledFiles {
    version: String,
    files: Vec<String>,
}

const NEW: &str = "myle-new";
const OLD: &str = "myle-old";
/// A file held by a virus scanner or the indexer usually frees up quickly.
const BUSY_RETRIES: u32 = 24;
const BUSY_WAIT: Duration = Duration::from_millis(250);
const REPORT_EVERY: Duration = Duration::from_millis(60);

pub fn default_dir() -> PathBuf {
    shell::local_app_data()
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join(product::PUBLISHER)
        .join(product::LEGACY_BINARY)
}

/// Refuses folders an app must never be installed straight into.
pub fn check_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() || dir.parent().is_none() {
        return Err("Choose a full folder path, not a drive root.".into());
    }
    let off_limits = [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "USERPROFILE",
        "ProgramData",
    ];
    for var in off_limits {
        if let Some(value) = std::env::var_os(var)
            && shell::same_path(dir, Path::new(&value))
        {
            return Err(format!(
                "{} cannot be installed directly into {}.",
                product::NAME,
                dir.display()
            ));
        }
    }
    Ok(())
}

/// Closes whatever runs from `dir`; an error names what would not stop.
pub fn close_running(dir: &Path, grace: Duration) -> Result<(), String> {
    let left = processes::close_all(dir, grace);
    if left.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = left
        .iter()
        .filter_map(|p| p.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    Err(format!(
        "These programs would not close: {}. Close them and try again.",
        names.join(", ")
    ))
}

pub fn install(
    payload: &[u8],
    options: &InstallOptions,
    report: &mut dyn FnMut(Progress),
) -> Result<Installed, String> {
    let dir = options.dir.as_path();
    check_dir(dir)?;
    report(Progress::Stage {
        stage: Stage::Preparing,
    });
    if !options.live && !processes::running_in(dir).is_empty() {
        return Err(format!(
            "{} is still running. Close it and try again.",
            product::NAME
        ));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("The folder {} could not be created: {e}", dir.display()))?;
    probe_writable(dir)?;
    sweep_leftovers(dir);

    let (header, mut reader) =
        payload::open(payload).map_err(|e| format!("The setup file is damaged: {e}"))?;
    let previous = read_installed(dir);

    report(Progress::Stage {
        stage: Stage::Copying,
    });
    let placed = copy_files(dir, &header, &mut reader, report)?;
    // An older updater restarts the executable path it was launched from.
    // Give that path the new binary on the first upgrade, including /LIVE.
    let bridged = if product::BINARY != product::LEGACY_BINARY
        && dir.join(product::legacy_exe_name()).is_file()
    {
        if let Err(error) = bridge_legacy_exe(dir) {
            roll_back(placed);
            return Err(error);
        }
        true
    } else {
        false
    };
    commit(placed, options.live);
    if bridged && !options.live {
        remove_patiently(&sibling(&dir.join(product::legacy_exe_name()), OLD));
    }

    // Files the previous version had and this one does not.
    let mut current: Vec<String> = header.files.iter().map(|f| f.path.clone()).collect();
    if bridged {
        current.push(product::legacy_exe_name());
    }
    if let Some(previous) = previous {
        let stale: Vec<&String> = previous
            .files
            .iter()
            .filter(|old| !current.iter().any(|new| new.eq_ignore_ascii_case(old)))
            .collect();
        for path in stale {
            if let Ok(relative) = payload::relative_path(path) {
                let _ = std::fs::remove_file(dir.join(relative));
            }
        }
        remove_empty_dirs(dir, &previous.files);
    }
    write_installed(dir, &header.version, &current)?;

    report(Progress::Stage {
        stage: Stage::Registering,
    });
    let size_kb = u32::try_from(header.total_size().div_ceil(1024)).unwrap_or(u32::MAX);
    registry::register(dir, &header.version, size_kb)?;

    report(Progress::Stage {
        stage: Stage::Shortcuts,
    });
    // The app is installed by now: a shortcut Windows refuses is reported,
    // not a reason to fail the whole install or to skip the other shortcuts.
    let exe = dir.join(product::exe_name());
    let warnings = apply_shortcuts(&exe, options.shortcuts);

    report(Progress::Stage {
        stage: Stage::Finishing,
    });
    Ok(Installed { exe, warnings })
}

fn probe_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".myle-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|_| {
        format!(
            "Setup cannot write to {}. Choose a folder in your user profile.",
            dir.display()
        )
    })?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// A file put in place, and the previous one it replaced (kept until the
/// whole install succeeds).
struct Placed {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

fn copy_files(
    dir: &Path,
    header: &payload::Header,
    reader: &mut impl Read,
    report: &mut dyn FnMut(Progress),
) -> Result<Vec<Placed>, String> {
    let total = header.total_size();
    let mut done = 0u64;
    let mut placed = Vec::new();
    let mut last_report: Option<Instant> = None;
    for entry in &header.files {
        let result = (|| -> Result<Placed, String> {
            let relative = payload::relative_path(&entry.path).map_err(|e| e.to_string())?;
            let target = dir.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let fresh = sibling(&target, NEW);
            let mut file = std::fs::File::create(&fresh)
                .map_err(|e| format!("{} could not be written: {e}", entry.path))?;
            let mut left = entry.size;
            let mut buffer = vec![0u8; 256 * 1024];
            while left > 0 {
                let want = usize::try_from(left.min(buffer.len() as u64)).unwrap_or(buffer.len());
                reader
                    .read_exact(&mut buffer[..want])
                    .map_err(|e| format!("The setup file is damaged: {e}"))?;
                file.write_all(&buffer[..want])
                    .map_err(|e| format!("{} could not be written: {e}", entry.path))?;
                left -= want as u64;
                done += want as u64;
                if last_report.is_none_or(|at| at.elapsed() >= REPORT_EVERY) {
                    last_report = Some(Instant::now());
                    report(Progress::Files {
                        done,
                        total,
                        file: entry.path.clone(),
                    });
                }
            }
            file.flush().map_err(|e| e.to_string())?;
            drop(file);

            let backup = if target.exists() {
                let backup = sibling(&target, OLD);
                let _ = std::fs::remove_file(&backup);
                rename_patiently(&target, &backup)
                    .map_err(|e| format!("{} is in use: {e}", entry.path))?;
                Some(backup)
            } else {
                None
            };
            if let Err(e) = rename_patiently(&fresh, &target) {
                if let Some(backup) = &backup {
                    let _ = std::fs::rename(backup, &target);
                }
                let _ = std::fs::remove_file(&fresh);
                return Err(format!("{} could not be put in place: {e}", entry.path));
            }
            Ok(Placed { target, backup })
        })();
        match result {
            Ok(file) => placed.push(file),
            Err(error) => {
                roll_back(placed);
                if let Ok(relative) = payload::relative_path(&entry.path) {
                    let _ = std::fs::remove_file(sibling(&dir.join(relative), NEW));
                }
                return Err(error);
            }
        }
    }
    report(Progress::Files {
        done: total,
        total,
        file: String::new(),
    });
    Ok(placed)
}

/// Replaces the old executable with a compatibility launcher for the old
/// updater and any existing scheduled backup. Normal shortcuts use MYLE.exe.
fn bridge_legacy_exe(dir: &Path) -> Result<(), String> {
    let legacy = dir.join(product::legacy_exe_name());
    let fresh = sibling(&legacy, NEW);
    let backup = sibling(&legacy, OLD);
    std::fs::copy(dir.join(product::exe_name()), &fresh)
        .map_err(|e| format!("Could not prepare the previous executable name: {e}"))?;
    let _ = std::fs::remove_file(&backup);
    if let Err(error) = rename_patiently(&legacy, &backup) {
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("Could not replace the previous executable: {error}"));
    }
    if let Err(error) = rename_patiently(&fresh, &legacy) {
        let _ = std::fs::rename(&backup, &legacy);
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("Could not finish the executable rename: {error}"));
    }
    Ok(())
}

fn rename_patiently(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < BUSY_RETRIES => {
                attempt += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Puts the previous version back after a failed install.
fn roll_back(placed: Vec<Placed>) {
    for file in placed.into_iter().rev() {
        let _ = std::fs::remove_file(&file.target);
        if let Some(backup) = file.backup {
            let _ = std::fs::rename(&backup, &file.target);
        }
    }
}

/// The install worked: the previous files can go (a virus scanner may hold
/// one for a moment). In a live update the running app still holds its own
/// files: one try each, and what is left is swept later.
fn commit(placed: Vec<Placed>, live: bool) {
    for backup in placed.into_iter().filter_map(|file| file.backup) {
        if live {
            let _ = std::fs::remove_file(&backup);
        } else {
            remove_patiently(&backup);
        }
    }
}

/// Deletes the `*.myle-old` copies a live update could not remove while the
/// previous version was still running. One try each: whatever is still in
/// use goes next time.
pub fn sweep_leftovers(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let suffix = format!(".{OLD}");
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && entry.file_name() == DATA_DIR => {}
            Ok(kind) if kind.is_dir() => sweep_leftovers(&path),
            Ok(_) if entry.file_name().to_string_lossy().ends_with(&suffix) => {
                let _ = std::fs::remove_file(&path);
            }
            _ => {}
        }
    }
}

fn read_installed(dir: &Path) -> Option<InstalledFiles> {
    let text = std::fs::read_to_string(dir.join(product::INSTALL_MANIFEST)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_installed(dir: &Path, version: &str, files: &[String]) -> Result<(), String> {
    let record = InstalledFiles {
        version: version.to_string(),
        files: files.to_vec(),
    };
    let json = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(product::INSTALL_MANIFEST), json).map_err(|e| e.to_string())
}

/// Removes folders that held `files` once they are empty, deepest first.
fn remove_empty_dirs(dir: &Path, files: &[String]) {
    let mut folders: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| payload::relative_path(file).ok())
        .flat_map(|relative| {
            relative
                .ancestors()
                .skip(1)
                .filter(|ancestor| !ancestor.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .collect::<Vec<_>>()
        })
        .collect();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.components().count()));
    folders.dedup();
    for folder in folders {
        let _ = std::fs::remove_dir(dir.join(folder));
    }
}

/// Creates, refreshes or removes each shortcut as `mode` says. Returns what
/// could not be done; every shortcut is attempted either way.
fn apply_shortcuts(exe: &Path, mode: ShortcutMode) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut create = |kind: Shortcut| {
        if let Err(error) = shell::create_shortcut(kind, exe) {
            warnings.push(error);
        }
    };
    match mode {
        ShortcutMode::Choose(choice) => {
            for kind in Shortcut::ALL {
                if choice.wants(kind) {
                    create(kind);
                } else {
                    shell::remove_shortcut(kind, exe);
                }
            }
            retire_legacy_autostart(choice, exe);
            remember_shortcuts(choice, exe);
        }
        ShortcutMode::Keep => {
            // What the user wants now (or, over an install from before that
            // was remembered, what `migrate_shortcuts` repairs): a wanted
            // access shortcut that went missing is recreated. Explicit
            // setup opt-outs and the Startup preference in Settings are retained.
            let choice = current_choice().unwrap_or_else(|| migrate_shortcuts(exe));
            let present = Shortcut::ALL.map(|kind| shell::shortcut_present(kind, exe));
            for (kind, make) in Shortcut::ALL.into_iter().zip(update_plan(choice, present)) {
                if make {
                    create(kind);
                }
            }
            retire_legacy_autostart(choice, exe);
            remember_shortcuts(choice, exe);
        }
        ShortcutMode::RefreshOnly => {
            // Refresh what the user kept (its icon may have changed).
            for kind in Shortcut::ALL {
                if shell::shortcut_present(kind, exe) {
                    create(kind);
                }
            }
        }
    }
    warnings.extend(shell::refresh_taskbar_shortcuts(exe));
    crate::shell_identity::notify_update(exe);
    crate::shell_identity::notify_update(&crate::shell_identity::icon_path(exe));
    warnings
}

/// What the setup remembers per shortcut, in the registry (see
/// `registry::shortcut_states`).
mod state {
    /// Not wanted.
    pub const OFF: u32 = 0;
    /// Wanted, but not made yet (Windows refused it, or it was uninstalled):
    /// the next update or install tries again.
    pub const WANTED: u32 = 1;
    /// Wanted and made. Desktop/Start Menu are restored if missing on update.
    pub const MADE: u32 = 2;
}

/// What the user wants now, in `Shortcut::ALL` order: the remembered states,
/// Missing files never undo a saved checkbox. Settings explicitly writes OFF
/// when the user disables starting with Windows.
fn wanted_now(states: [u32; 3]) -> [bool; 3] {
    states.map(|saved| saved != state::OFF)
}

/// `None` before any install has remembered the shortcuts.
fn current_choice() -> Option<ShortcutChoice> {
    let states = registry::shortcut_states()?;
    Some(ShortcutChoice::from_array(wanted_now(states)))
}

fn states_after(choice: ShortcutChoice, present: [bool; 3]) -> [u32; 3] {
    let wanted = choice.to_array();
    std::array::from_fn(|i| match (wanted[i], present[i]) {
        (false, _) => state::OFF,
        (true, true) => state::MADE,
        (true, false) => state::WANTED,
    })
}

fn remember_shortcuts(choice: ShortcutChoice, exe: &Path) {
    let present = Shortcut::ALL.map(|kind| shell::shortcut_present(kind, exe));
    registry::remember_shortcut_states(states_after(choice, present));
}

/// Which shortcuts an update writes, in `Shortcut::ALL` order: every wanted
/// one (made again when it never got made), and any other of ours that is
/// there, refreshed but never removed.
fn update_plan(choice: ShortcutChoice, present: [bool; 3]) -> [bool; 3] {
    let chosen = choice.to_array();
    std::array::from_fn(|i| chosen[i] || present[i])
}

/// The Startup shortcut decides from now on: the old app's `Run` value would
/// start the app a second time, or when it was switched off. Kept only when
/// Windows refused the shortcut that should replace it.
fn retire_legacy_autostart(choice: ShortcutChoice, exe: &Path) {
    if !choice.startup || shell::shortcut_present(Shortcut::Startup, exe) {
        registry::remove_legacy_autostart();
    }
}

/// The first update over an install from before shortcut choices were
/// remembered. Those setups only ever refreshed what they found, so an
/// install could be left with the desktop shortcut alone for good. This
/// once: the Start menu entry every installed app has, and a Startup
/// shortcut in place of the old app's `Run` value, with Desktop enabled by default.
fn migrate_shortcuts(exe: &Path) -> ShortcutChoice {
    ShortcutChoice {
        desktop: true,
        start_menu: true,
        startup: shell::shortcut_present(Shortcut::Startup, exe) || registry::legacy_autostart(),
    }
}

/// The shortcuts the setup window starts with: what the user wants now,
/// else (over an install that predates remembering it) what the update
/// would repair, else a new install's (`ShortcutChoice::NEW_INSTALL`).
pub fn default_shortcuts(existing: Option<&Path>) -> ShortcutChoice {
    let exe = existing
        .map_or_else(default_dir, Path::to_path_buf)
        .join(product::exe_name());
    if let Some(choice) = current_choice() {
        return choice;
    }
    match existing {
        Some(_) => migrate_shortcuts(&exe),
        None => ShortcutChoice::NEW_INSTALL,
    }
}

/// The app's own data, removed only when asked: settings, the signed-in
/// account, Game Saves setup and restore safety copies, caches, the web
/// view's storage. Game Saves backups live in a folder the user chose and
/// are never touched.
///
/// Where they are: `%APPDATA%\ThomasThanos\MakeYourLifeEasier` and
/// `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data` (see the app's
/// `storage.rs`), plus the folders earlier versions used.
pub fn data_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(roaming) = shell::roaming_app_data() {
        folders.push(roaming.join(product::PUBLISHER).join(product::LEGACY_BINARY));
        folders.push(roaming.join(product::LEGACY_NAME));
        folders.push(roaming.join(product::NAME));
        folders.push(roaming.join(product::IDENTIFIER));
    }
    if let Some(local) = shell::local_app_data() {
        folders.push(local.join(product::PUBLISHER).join(product::LEGACY_BINARY).join(DATA_DIR));
        folders.push(local.join(product::LEGACY_NAME));
        folders.push(local.join(product::NAME));
        folders.push(local.join(product::IDENTIFIER));
    }
    folders.push(std::env::temp_dir().join(product::LEGACY_BINARY));
    folders.push(std::env::temp_dir().join(product::BINARY));
    folders
}

/// The app's local data inside its install folder; installs, updates and the
/// sweep of old copies leave it alone.
const DATA_DIR: &str = "data";

pub fn uninstall(
    dir: &Path,
    remove_data: bool,
    report: &mut dyn FnMut(Progress),
) -> Result<AfterExit, String> {
    report(Progress::Stage {
        stage: Stage::Preparing,
    });
    if !processes::running_in(dir).is_empty() {
        return Err(format!(
            "{} is still running. Close it and try again.",
            product::NAME
        ));
    }
    // This program and, when it runs as a copy, the uninstaller in the
    // folder that waits for it: deleted once they have exited.
    let in_use: Vec<PathBuf> = std::env::current_exe()
        .ok()
        .into_iter()
        .chain(processes::spared_exe())
        .collect();
    let mut after_exit = AfterExit::default();

    // The Game Saves page's scheduled backup would otherwise keep starting a
    // program that is no longer there.
    shell::run_hidden(
        &shell::system32("schtasks.exe"),
        &["/Delete", "/TN", product::GAME_SAVES_TASK, "/F"],
    );

    report(Progress::Stage {
        stage: Stage::Shortcuts,
    });
    let exe = dir.join(product::exe_name());
    for kind in Shortcut::ALL {
        shell::remove_shortcut(kind, &exe);
    }
    // The uninstall removed them, not the user: a reinstall offers the same
    // shortcuts again (when the data is kept, and with it this key).
    if let Some(states) = registry::shortcut_states() {
        registry::remember_shortcut_states(
            states.map(|s| if s == state::MADE { state::WANTED } else { s }),
        );
    }

    report(Progress::Stage {
        stage: Stage::RemovingFiles,
    });
    let mut files = read_installed(dir)
        .map(|installed| installed.files)
        .unwrap_or_else(product::legacy_files);
    files.push(product::INSTALL_MANIFEST.to_string());
    let total = files.len() as u64;
    for (index, file) in files.iter().enumerate() {
        let Ok(relative) = payload::relative_path(file) else {
            continue;
        };
        let path = dir.join(relative);
        report(Progress::Files {
            done: index as u64,
            total,
            file: file.clone(),
        });
        if in_use.iter().any(|exe| shell::same_file(exe, &path)) {
            after_exit.remove.push(path);
            continue;
        }
        remove_patiently(&path);
    }
    report(Progress::Files {
        done: total,
        total,
        file: String::new(),
    });
    remove_empty_dirs(dir, &files);
    after_exit.remove_if_empty.push(dir.to_path_buf());
    // %LOCALAPPDATA%\ThomasThanos, when it held nothing else.
    if let Some(parent) = dir.parent()
        && parent
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(product::PUBLISHER))
    {
        after_exit.remove_if_empty.push(parent.to_path_buf());
    }
    let _ = std::fs::remove_dir(dir);

    report(Progress::Stage {
        stage: Stage::Registering,
    });
    registry::unregister(remove_data);

    if remove_data {
        report(Progress::Stage {
            stage: Stage::RemovingData,
        });
        for folder in data_folders() {
            if folder.exists() && std::fs::remove_dir_all(&folder).is_err() {
                // Something still holds a file (the web view shutting down):
                // finish once this program is gone.
                after_exit.remove.push(folder.clone());
            }
            // %APPDATA%\ThomasThanos, when it held nothing else.
            if let Some(parent) = folder.parent()
                && parent
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(product::PUBLISHER))
            {
                after_exit.remove_if_empty.push(parent.to_path_buf());
            }
        }
    }
    report(Progress::Stage {
        stage: Stage::Finishing,
    });
    Ok(after_exit)
}

fn remove_patiently(path: &Path) {
    for _ in 0..BUSY_RETRIES {
        match std::fs::remove_file(path) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(BUSY_WAIT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("myle-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_legacy_executable_starts_the_new_build_after_an_update() {
        let dir = temp("legacy-binary");
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join(product::legacy_exe_name());
        std::fs::write(&old, b"old build").unwrap();
        std::fs::write(dir.join(product::exe_name()), b"new build").unwrap();
        bridge_legacy_exe(&dir).unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), b"new build");
        assert_eq!(std::fs::read(sibling(&old, OLD)).unwrap(), b"old build");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_default_install_folder_is_the_old_apps_one() {
        let local = shell::local_app_data()
            .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
            .unwrap();
        assert_eq!(default_dir(), local.join("ThomasThanos").join("MakeYourLifeEasier"));
    }

    #[test]
    fn uninstall_knows_the_data_folders_old_and_new() {
        let folders = data_folders();
        if let Some(roaming) = shell::roaming_app_data() {
            assert!(folders.contains(&roaming.join("ThomasThanos").join("MakeYourLifeEasier")));
            assert!(folders.contains(&roaming.join(product::NAME)));
            assert!(folders.contains(&roaming.join(product::IDENTIFIER)));
        }
        if let Some(local) = shell::local_app_data() {
            assert!(folders.contains(&default_dir().join("data")));
            assert!(folders.contains(&local.join(product::NAME)));
            assert!(folders.contains(&local.join(product::IDENTIFIER)));
        }
        assert!(
            !folders.contains(&default_dir()),
            "never the whole install folder"
        );
    }

    #[test]
    fn the_sweep_of_old_copies_leaves_the_data_folder_alone() {
        let dir = temp("sweep-data");
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("res")).unwrap();
        std::fs::write(dir.join("data").join("x.myle-old"), b"user data").unwrap();
        std::fs::write(dir.join("res").join("y.myle-old"), b"old copy").unwrap();
        sweep_leftovers(&dir);
        assert!(dir.join("data").join("x.myle-old").exists());
        assert!(!dir.join("res").join("y.myle-old").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn packed(files: &[(&str, &[u8])], version: &str) -> Vec<u8> {
        // Tests run side by side: every call packs from a folder of its own.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let source = temp(&format!("src-{version}-{call}"));
        for (path, bytes) in files {
            let path = source.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let mut out = Vec::new();
        payload::pack(&source, version, &mut out).unwrap();
        let _ = std::fs::remove_dir_all(source);
        out
    }

    #[test]
    fn copying_writes_every_file_and_replaces_the_old_ones() {
        let dir = temp("copy");
        std::fs::create_dir_all(dir.join("ludusavi")).unwrap();
        std::fs::write(dir.join("App.exe"), b"old").unwrap();
        let bytes = packed(
            &[("App.exe", b"new app"), ("ludusavi/l.exe", b"engine")],
            "2.0.0",
        );
        let (header, mut reader) = payload::open(&bytes).unwrap();
        let mut last = None;
        let placed = copy_files(&dir, &header, &mut reader, &mut |p| last = Some(p)).unwrap();
        assert!(
            dir.join("App.exe.myle-old").exists(),
            "kept until committed"
        );
        commit(placed, false);
        assert_eq!(std::fs::read(dir.join("App.exe")).unwrap(), b"new app");
        assert_eq!(
            std::fs::read(dir.join("ludusavi").join("l.exe")).unwrap(),
            b"engine"
        );
        assert!(!dir.join("App.exe.myle-old").exists());
        assert!(matches!(
            last,
            Some(Progress::Files {
                done: 13,
                total: 13,
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failure_part_way_puts_the_previous_version_back() {
        let dir = temp("rollback");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("App.exe"), b"old").unwrap();
        // The second file cannot be placed: a file stands where its folder goes.
        std::fs::write(dir.join("sub"), b"not a folder").unwrap();
        let bytes = packed(&[("App.exe", b"new"), ("sub/x.dat", b"data")], "2.0.0");
        let (header, mut reader) = payload::open(&bytes).unwrap();
        assert!(copy_files(&dir, &header, &mut reader, &mut |_| {}).is_err());
        assert_eq!(std::fs::read(dir.join("App.exe")).unwrap(), b"old");
        assert!(!dir.join("App.exe.myle-old").exists());
        assert!(!dir.join("App.exe.myle-new").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_live_update_replaces_a_running_program_and_sweeps_its_old_copy_later() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let dir = temp("live");
        std::fs::create_dir_all(dir.join("res")).unwrap();
        // A real running program from the install folder, as the app is
        // while its updater runs us.
        std::fs::copy(shell::system32("PING.EXE"), dir.join("App.exe")).unwrap();
        std::fs::write(dir.join("res").join("r.dat"), b"old res").unwrap();
        let mut running = std::process::Command::new(dir.join("App.exe"))
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let bytes = packed(&[("App.exe", b"new app"), ("res/r.dat", b"new res")], "2.0.0");
        let (header, mut reader) = payload::open(&bytes).unwrap();
        let placed = copy_files(&dir, &header, &mut reader, &mut |_| {}).unwrap();
        commit(placed, true);
        assert_eq!(std::fs::read(dir.join("App.exe")).unwrap(), b"new app");
        assert_eq!(std::fs::read(dir.join("res").join("r.dat")).unwrap(), b"new res");
        assert!(
            dir.join("App.exe.myle-old").exists(),
            "still running, left for the sweep"
        );
        assert!(!dir.join("res").join("r.dat.myle-old").exists());

        let _ = running.kill();
        let _ = running.wait();
        sweep_leftovers(&dir);
        assert!(!dir.join("App.exe.myle-old").exists());
        assert_eq!(std::fs::read(dir.join("App.exe")).unwrap(), b"new app");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_folders_are_removed_deepest_first() {
        let dir = temp("empty");
        std::fs::create_dir_all(dir.join("spicetify").join("theme")).unwrap();
        std::fs::create_dir_all(dir.join("ludusavi")).unwrap();
        std::fs::write(dir.join("ludusavi").join("user-file"), b"x").unwrap();
        remove_empty_dirs(
            &dir,
            &[
                "spicetify/theme/user.css".into(),
                "ludusavi/ludusavi.exe".into(),
            ],
        );
        assert!(!dir.join("spicetify").exists());
        assert!(
            dir.join("ludusavi").exists(),
            "a folder with other files stays"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn updates_restore_default_shortcuts_but_respect_explicit_choices() {
        use state::{MADE, OFF, WANTED};
        // Missing access shortcuts get repaired. Startup remains optional.
        let wanted = wanted_now([MADE, MADE, WANTED]);
        assert_eq!(wanted, [true, true, true]);
        // An unticked one stays off, even if a copy of ours is still there.
        assert_eq!(wanted_now([OFF, OFF, OFF]), [false; 3]);

        // A missing access shortcut stays wanted for the next repair attempt.
        let choice = ShortcutChoice::from_array(wanted);
        assert_eq!(states_after(choice, [true, false, true]), [MADE, WANTED, MADE]);
        assert_eq!(states_after(choice, [true, false, false]), [MADE, WANTED, WANTED]);
        assert_eq!(wanted_now([MADE; 3]), [true; 3]);
    }

    #[test]
    fn an_update_brings_back_chosen_shortcuts_and_removes_none() {
        let choice = ShortcutChoice {
            desktop: true,
            start_menu: true,
            startup: false,
        };
        // Desktop and Start menu went missing; a Startup one is still there.
        assert_eq!(update_plan(choice, [false, false, true]), [true, true, true]);
        // Nothing chosen and nothing there: nothing is made.
        assert_eq!(
            update_plan(ShortcutChoice::from_array([false; 3]), [false; 3]),
            [false; 3]
        );
    }

    #[test]
    fn system_folders_are_refused() {
        assert!(check_dir(Path::new(r"C:\")).is_err());
        assert!(check_dir(Path::new("relative")).is_err());
        if let Some(windows) = std::env::var_os("SystemRoot") {
            assert!(check_dir(Path::new(&windows)).is_err());
        }
        assert!(check_dir(&default_dir()).is_ok());
    }
}
