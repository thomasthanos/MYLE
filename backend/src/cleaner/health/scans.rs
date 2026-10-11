//! The eight Software Health scans. Each returns findings paired with the
//! target the backend keeps to act on them. Every scan is read-only and
//! holds to the safety lists in `sys`: Windows, Microsoft, drivers, hardware
//! vendors and MYLE are never offered for a change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::sys;
use super::{Finding, Target};

/// Reports how far a scan is, 0..1.
pub type Tick<'a> = &'a (dyn Fn(f32) + Sync);

pub fn scan(module: &str, tick: Tick) -> Result<Vec<(Finding, Target)>, String> {
    match module {
        "leftovers" => leftovers(tick),
        "broken" => Ok(broken()),
        "redundant" => Ok(redundant()),
        "hibernation" => Ok(hibernation(tick)),
        "permissions" => Ok(permissions()),
        "notifications" => Ok(notifications(tick)),
        "installers" => Ok(installers()),
        "suspicious" => Ok(suspicious(tick)),
        other => Err(format!("Unknown module {other}")),
    }
}

fn id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn finding(module: &'static str, title: impl Into<String>) -> Finding {
    Finding {
        id: id(),
        module,
        title: title.into(),
        detail: String::new(),
        path: None,
        size: None,
        confidence: None,
        enabled: None,
        flags: Vec::new(),
        actions: Vec::new(),
        cleanup_action: None,
        recommended: false,
        review_reason: None,
    }
}

// ─── Installed software, once, for several scans ─────────────────────────────

#[derive(Clone)]
struct Installed {
    name: String,
    install_location: Option<String>,
    uninstall: Option<String>,
    publisher: String,
    display_icon: Option<String>,
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

fn uninstall_keys() -> Vec<(&'static str, String, Installed)> {
    uninstall_keys_at(&[
        ("HKLM", UNINSTALL),
        ("HKCU", UNINSTALL),
        (
            "HKLM",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            "HKCU",
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ])
}

fn uninstall_keys_at(roots: &[(&'static str, &str)]) -> Vec<(&'static str, String, Installed)> {
    let mut out = Vec::new();
    for &(hive, uninstall_root) in roots {
        let Some(root) = sys::open(hive, uninstall_root) else {
            continue;
        };
        for name in sys::subkeys(&root) {
            let full_path = format!("{uninstall_root}\\{name}");
            let Some(key) = sys::open(hive, &full_path) else {
                continue;
            };
            // Updates and components without a display name are skipped.
            let Some(display) = sys::string(&key, "DisplayName") else {
                continue;
            };
            if sys::dword(&key, "SystemComponent") == Some(1) {
                continue;
            }
            out.push((
                hive,
                full_path,
                Installed {
                    name: display,
                    install_location: sys::string(&key, "InstallLocation"),
                    uninstall: sys::string(&key, "UninstallString")
                        .or_else(|| sys::string(&key, "QuietUninstallString")),
                    publisher: sys::string(&key, "Publisher").unwrap_or_default(),
                    display_icon: sys::string(&key, "DisplayIcon"),
                },
            ));
        }
    }
    out
}

fn installed_list() -> Vec<Installed> {
    uninstall_keys()
        .into_iter()
        .map(|(_, _, app)| app)
        .collect()
}

fn installation_status(app: &Installed) -> (sys::Presence, sys::Presence, sys::Presence) {
    let location = app
        .install_location
        .as_deref()
        .map(|p| sys::path_status(Path::new(&sys::expand(p.trim_matches('"'))), true))
        .unwrap_or(sys::Presence::Unknown);
    let uninstaller = app
        .uninstall
        .as_deref()
        .map(sys::program_status)
        .unwrap_or(sys::Presence::Unknown);
    let icon = app
        .display_icon
        .as_deref()
        .map(icon_path)
        .map(|p| sys::path_status(Path::new(&p), false))
        .unwrap_or(sys::Presence::Unknown);
    (location, uninstaller, icon)
}

fn icon_path(icon: &str) -> String {
    let text = sys::expand(icon.trim());
    let path = text
        .rsplit_once(',')
        .filter(|(_, index)| index.trim().parse::<i32>().is_ok())
        .map_or(text.as_str(), |(path, _)| path);
    path.trim().trim_matches('"').to_string()
}

fn broken_candidate(
    location: sys::Presence,
    uninstaller: sys::Presence,
    icon: sys::Presence,
) -> bool {
    let evidence = [location, uninstaller, icon];
    !evidence.contains(&sys::Presence::Present)
        && [location, uninstaller].contains(&sys::Presence::Missing)
}

fn installed_elsewhere(app: &Installed, list: &[Installed]) -> bool {
    list.iter().any(|other| {
        sys::norm(&other.name) == sys::norm(&app.name) && {
            let (folder, command, icon) = installation_status(other);
            [folder, command, icon].contains(&sys::Presence::Present)
        }
    })
}

fn registered_paths(app: &Installed) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(location) = &app.install_location {
        paths.push(PathBuf::from(sys::expand(location.trim_matches('"'))));
    }
    for exe in app
        .uninstall
        .as_deref()
        .and_then(sys::program_of)
        .into_iter()
        .chain(app.display_icon.as_deref().map(icon_path))
    {
        if !exe.is_empty()
            && !sys::system_path(&exe)
            && let Some(parent) = Path::new(&exe).parent()
        {
            // A shared Program Files or drive root is not an application folder.
            if parent.components().count() > 2 {
                paths.push(parent.to_path_buf());
            }
        }
    }
    paths
}

fn related_folder(folder: &Path, installed: &Path) -> bool {
    let normalize = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let folder = normalize(folder);
    let installed = normalize(installed);
    !installed.is_empty()
        && (folder == installed
            || folder.starts_with(&format!("{installed}\\"))
            || installed.starts_with(&format!("{folder}\\")))
}

fn orphan_folder_evidence(
    registered_location: bool,
    uninstaller: sys::Presence,
    icon: sys::Presence,
) -> bool {
    registered_location && uninstaller == sys::Presence::Missing && icon != sys::Presence::Present
}

fn orphan_folder_owner<'a>(path: &Path, list: &'a [Installed]) -> Option<&'a Installed> {
    list.iter().find(|app| {
        if sys::protected_name(&app.name) || sys::protected_name(&app.publisher) {
            return false;
        }
        // A name match or nearby vendor folder does not establish ownership.
        let registered_location = app.install_location.as_deref().is_some_and(|location| {
            Path::new(&sys::expand(location.trim_matches('"')))
                .as_os_str()
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .eq_ignore_ascii_case(path.to_string_lossy().trim_end_matches(['\\', '/']))
        });
        let (_, uninstaller, icon) = installation_status(app);
        orphan_folder_evidence(registered_location, uninstaller, icon)
    })
}

fn has_program_files(path: &Path, depth: usize, budget: &mut usize) -> bool {
    // An unreadable/truncated tree cannot be verified as abandoned.
    let Ok(entries) = std::fs::read_dir(path) else {
        return true;
    };
    for entry in entries {
        if *budget == 0 {
            return true;
        }
        *budget -= 1;
        let Ok(entry) = entry else {
            return true;
        };
        let Ok(kind) = entry.file_type() else {
            return true;
        };
        if kind.is_symlink() {
            return true;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if kind.is_file()
            && ["exe", "lnk", "sav", "save"].contains(
                &entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .as_str(),
            )
        {
            return true;
        }
        if kind.is_dir()
            && (["saves", "savegames", "savedgames", "mods", "profiles"].contains(&name.as_str())
                || depth == 0
                || has_program_files(&entry.path(), depth - 1, budget))
        {
            return true;
        }
    }
    false
}

/// Normalised vendor and product words of everything installed, for matching
/// a folder's name against software that is really there.
fn installed_tokens(list: &[Installed]) -> HashSet<String> {
    let mut set = HashSet::new();
    for app in list {
        set.insert(sys::norm(&app.name));
        for word in app.name.split(|c: char| !c.is_alphanumeric()) {
            let w = sys::norm(word);
            if w.len() >= 4 {
                set.insert(w);
            }
        }
        for location in registered_paths(app) {
            for folder in location.components().rev().take(2) {
                let w = sys::norm(&folder.as_os_str().to_string_lossy());
                if w.len() >= 4 {
                    set.insert(w);
                }
            }
        }
        let pub_norm = sys::norm(&app.publisher);
        if pub_norm.len() >= 4 {
            set.insert(pub_norm);
        }
    }
    set
}

/// Folder names of running programs, services and AppX packages: whatever
/// runs or is installed outside the Uninstall list still counts as present.
fn live_tokens() -> Result<HashSet<String>, String> {
    let script = "$ErrorActionPreference = 'Stop'; $p = Get-Process | Where-Object Path | ForEach-Object { Split-Path (Split-Path $_.Path) -Leaf; Split-Path $_.Path -Leaf }; $s = Get-CimInstance Win32_Service | ForEach-Object { $_.PathName }; $a = Get-AppxPackage | ForEach-Object { $_.Name }; $w = New-Object -ComObject WScript.Shell; $l = @([Environment]::GetFolderPath('StartMenu'),[Environment]::GetFolderPath('CommonStartMenu'),[Environment]::GetFolderPath('Desktop'),[Environment]::GetFolderPath('CommonDesktopDirectory')) | Where-Object { $_ } | ForEach-Object { Get-ChildItem -LiteralPath $_ -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.BaseName; $w.CreateShortcut($_.FullName).TargetPath } }; @($p) + @($s) + @($a) + @($l) | ConvertTo-Json -Compress";
    let mut set = HashSet::new();
    let text = sys::powershell(script).map_err(|e| format!("Installed apps and shortcuts could not be checked. Leftover folders were not classified: {e}"))?;
    {
        for item in sys::json_list(&text) {
            let Some(text) = item.as_str() else { continue };
            for part in text.split(['\\', '"', '.']) {
                let w = sys::norm(part);
                if w.len() >= 4 && w != "exe" && w != "program" && w != "programfiles" {
                    set.insert(w);
                }
            }
        }
    }
    Ok(set)
}

// ─── 1. Uninstallation leftovers ─────────────────────────────────────────────

fn leftovers(tick: Tick) -> Result<Vec<(Finding, Target)>, String> {
    let list = installed_list();
    let active: Vec<_> = list
        .iter()
        .filter(|app| {
            let (_, uninstaller, icon) = installation_status(app);
            !orphan_folder_evidence(true, uninstaller, icon)
        })
        .cloned()
        .collect();
    let mut tokens = installed_tokens(&active);
    tick(0.1);
    tokens.extend(live_tokens()?);
    tick(0.4);
    let mut out = Vec::new();

    let roots: Vec<PathBuf> = [
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ]
    .iter()
    .filter_map(|v| sys::env_dir(v))
    .collect();

    let mut seen = HashSet::new();
    let roots_total = roots.len().max(1) as f32;
    for (index, root) in roots.into_iter().enumerate() {
        tick(0.4 + 0.5 * index as f32 / roots_total);
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let path = entry.path();
            if !seen.insert(path.to_string_lossy().to_lowercase()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let folder = sys::norm(&name);
            if folder.len() < 4
                || sys::protected_name(&name)
                || sys::system_path(&path.to_string_lossy())
            {
                continue;
            }
            // Age and an absent inventory match never prove uninstallation.
            let Some(owner) = orphan_folder_owner(&path, &list) else {
                continue;
            };
            if active
                .iter()
                .flat_map(registered_paths)
                .any(|installed| related_folder(&path, &installed))
                || has_program_files(&path, 2, &mut 1500)
                || [
                    "kojimaproductions",
                    "fromsoftware",
                    "rockstargames",
                    "cdprojektred",
                ]
                .contains(&folder.as_str())
            {
                continue;
            }
            // Common shared folders no single app owns.
            const SHARED: &[&str] = &[
                "temp", "cache", "packages", "programs", "common", "data", "logs", "crashpad",
                "cef", "local", "roaming",
            ];
            if SHARED.contains(&folder.as_str()) {
                continue;
            }
            // Still installed? Then it is not a leftover.
            let known = tokens
                .iter()
                .any(|t| folder.contains(t) || t.contains(&folder));
            if known {
                continue;
            }
            // A vendor folder with sub-apps is ambiguous; needs a clear match.
            // Anything touched in the last 30 days is still in use.
            let idle = sys::newest_change(&path).map(sys::days_since).unwrap_or(0);
            if idle < 30 {
                continue;
            }
            let size = sys::dir_size(&path, 20_000);
            if size == 0 {
                continue;
            }
            let mut f = finding("leftovers", name.clone());
            f.detail = format!(
                "The registered uninstaller for {} is missing. No app executable was found in its old installation folder.",
                owner.name
            );
            f.path = Some(path.to_string_lossy().into_owned());
            f.size = Some(size);
            f.review_reason = Some("A missing uninstaller does not prove all remaining data is unwanted. Review the folder before moving it to the Recycle Bin.".into());
            if root
                .to_string_lossy()
                .to_lowercase()
                .contains("program files")
            {
                f.flags.push("Needs administrator".into());
            }
            f.actions = vec!["recycle"];
            out.push((f, Target::Paths(vec![path])));
        }
    }

    // Orphaned Uninstall keys: no uninstaller and no install folder.
    for (hive, key_name, app) in uninstall_keys() {
        if sys::protected_name(&app.name) || sys::protected_name(&app.publisher) {
            continue;
        }
        let (location_status, _, icon_status) = installation_status(&app);
        let has_location =
            location_status == sys::Presence::Present || icon_status == sys::Presence::Present;
        let has_uninstaller = app
            .uninstall
            .as_deref()
            .map(sys::program_status)
            .unwrap_or(sys::Presence::Unknown);
        if has_location
            || has_uninstaller != sys::Presence::Missing
            || installed_elsewhere(&app, &list)
        {
            continue;
        }
        // This is a dead record; Broken uninstalls offers Force Clean. Here,
        // only the plain orphaned key with nothing on disk at all.
        if app.install_location.is_some() {
            continue;
        }
        let mut f = finding("leftovers", format!("Registry: {}", app.name));
        f.detail = format!("An Uninstall entry under {hive} with no uninstaller and no folder.");
        f.path = Some(format!("{hive}\\{key_name}"));
        f.confidence = Some(60);
        f.review_reason = Some("No installation folder was registered. This entry alone cannot prove the application was uninstalled.".into());
        f.actions = vec!["deleteKey"];
        out.push((
            f,
            Target::UninstallKey {
                hive,
                path: key_name,
                dirs: Vec::new(),
            },
        ));
    }

    Ok(out)
}

// ─── 2. Broken uninstalls ────────────────────────────────────────────────────

fn broken() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    let list = installed_list();
    for (hive, key_name, app) in uninstall_keys() {
        if sys::protected_name(&app.name) || sys::protected_name(&app.publisher) {
            continue;
        }
        let location = app.install_location.clone();
        let (location_status, uninstaller_status, icon_status) = installation_status(&app);
        let location_missing = location_status == sys::Presence::Missing;
        let uninstaller_missing = uninstaller_status == sys::Presence::Missing;
        if !broken_candidate(location_status, uninstaller_status, icon_status)
            || installed_elsewhere(&app, &list)
        {
            continue;
        }
        let dirs: Vec<PathBuf> = location
            .as_deref()
            .map(|location| PathBuf::from(sys::expand(location)))
            .filter(|p| p.is_dir())
            .into_iter()
            .collect();
        let mut f = finding("broken", app.name.clone());
        let mut why = Vec::new();
        if uninstaller_missing {
            why.push("its uninstaller is gone");
        }
        if location_missing {
            why.push("its install folder is gone");
        }
        f.detail = format!(
            "This entry is broken: {}. Windows still lists it.",
            why.join(" and ")
        );
        f.path = Some(format!("{hive}\\{key_name}"));
        f.confidence = Some(75);
        f.recommended = location_missing && uninstaller_missing;
        if location_status == sys::Presence::Unknown || uninstaller_status == sys::Presence::Unknown
        {
            f.review_reason = Some("Some installation information could not be verified. Review this entry; remaining folders are kept by default.".into());
        }
        f.flags = why.iter().map(|w| w.to_string()).collect();
        f.actions = vec!["forceClean", "deleteKey"];
        out.push((
            f,
            Target::UninstallKey {
                hive,
                path: key_name,
                dirs,
            },
        ));
    }
    out
}

// ─── 3. Software redundant files (curated) ───────────────────────────────────

struct Rule {
    app: &'static str,
    /// Relative to %LOCALAPPDATA% (`L:`) or %APPDATA% (`R:`).
    rel: &'static str,
    what: &'static str,
}

const REDUNDANT: &[Rule] = &[
    Rule {
        app: "Google Chrome",
        rel: r"L:Google\Chrome\User Data\Crashpad\reports",
        what: "crash reports",
    },
    Rule {
        app: "Google Chrome",
        rel: r"L:Google\Chrome\User Data\GrShaderCache",
        what: "shader cache",
    },
    Rule {
        app: "Brave",
        rel: r"L:BraveSoftware\Brave-Browser\User Data\Crashpad\reports",
        what: "crash reports",
    },
    Rule {
        app: "Discord",
        rel: r"R:discord\Crashpads",
        what: "crash dumps",
    },
    Rule {
        app: "Discord",
        rel: r"R:discord\logs",
        what: "logs",
    },
    Rule {
        app: "Slack",
        rel: r"R:Slack\logs",
        what: "logs",
    },
    Rule {
        app: "Spotify",
        rel: r"L:Spotify\Storage",
        what: "media cache",
    },
    Rule {
        app: "Spotify",
        rel: r"L:Spotify\Browser\Crashpad",
        what: "crash dumps",
    },
    Rule {
        app: "Steam",
        rel: r"L:Steam\htmlcache",
        what: "web cache",
    },
    Rule {
        app: "Zoom",
        rel: r"R:Zoom\logs",
        what: "logs",
    },
    Rule {
        app: "Adobe",
        rel: r"L:Adobe\CameraRaw\Cache",
        what: "cache",
    },
    Rule {
        app: "Unity",
        rel: r"L:Unity\cache",
        what: "package cache",
    },
    Rule {
        app: "pip",
        rel: r"L:pip\cache",
        what: "download cache",
    },
    Rule {
        app: "npm",
        rel: r"L:npm-cache\_cacache",
        what: "download cache",
    },
];

fn redundant() -> Vec<(Finding, Target)> {
    let local = sys::env_dir("LOCALAPPDATA");
    let roaming = sys::env_dir("APPDATA");
    let mut out = Vec::new();
    for rule in REDUNDANT {
        let (base, rel) = match rule.rel.split_once(':') {
            Some(("L", rel)) => (local.clone(), rel),
            Some(("R", rel)) => (roaming.clone(), rel),
            _ => (None, rule.rel),
        };
        let Some(base) = base else { continue };
        let path = base.join(rel);
        if !path.is_dir() {
            continue;
        }
        let size = sys::dir_size(&path, 50_000);
        if size < 1_000_000 {
            continue;
        }
        let mut f = finding("redundant", format!("{}: {}", rule.app, rule.what));
        f.detail = "Rebuilt by the app when needed; settings and sign-ins are left alone.".into();
        f.path = Some(path.to_string_lossy().into_owned());
        f.size = Some(size);
        f.confidence = Some(90);
        f.recommended = true;
        f.actions = vec!["recycle"];
        out.push((f, Target::Paths(vec![path])));
    }
    out
}

// ─── 4. Software hibernation ─────────────────────────────────────────────────

const APPROVED_RUN: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_FOLDER: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";
const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

fn startup_enabled(approved_hive: &str, name: &str) -> bool {
    match sys::open(approved_hive, APPROVED_RUN).and_then(|key| key.get_raw_value(name).ok()) {
        Some(value) => value.bytes.first().map(|b| b % 2 == 0).unwrap_or(true),
        None => true,
    }
}

fn hibernation(tick: Tick) -> Vec<(Finding, Target)> {
    let mut out = Vec::new();

    for hive in ["HKLM", "HKCU"] {
        let Some(key) = sys::open(hive, RUN) else {
            continue;
        };
        for name in key.enum_values().flatten().map(|(n, _)| n) {
            let command = sys::string(&key, &name).unwrap_or_default();
            let program = sys::program_of(&command).unwrap_or_default();
            if sys::protected_name(&name)
                || sys::protected_name(&program)
                || sys::system_path(&program)
            {
                continue;
            }
            let enabled = startup_enabled(hive, &name);
            let mut f = finding("hibernation", name.clone());
            f.detail = format!("Starts with Windows ({hive}): {command}");
            f.path = Some(program);
            f.enabled = Some(enabled);
            f.flags = vec!["Startup".into()];
            f.actions = if enabled {
                vec!["disable"]
            } else {
                vec!["enable"]
            };
            out.push((
                f,
                Target::Startup {
                    hive,
                    approved: APPROVED_RUN.into(),
                    name,
                },
            ));
        }
    }

    tick(0.15);
    // The Startup folder (shortcuts), approved the same way.
    if let Some(roaming) = sys::env_dir("APPDATA") {
        let folder = roaming.join(r"Microsoft\Windows\Start Menu\Programs\Startup");
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.eq_ignore_ascii_case("desktop.ini") || sys::protected_name(&name) {
                    continue;
                }
                let enabled = match sys::open("HKCU", APPROVED_FOLDER)
                    .and_then(|k| k.get_raw_value(&name).ok())
                {
                    Some(v) => v.bytes.first().map(|b| b % 2 == 0).unwrap_or(true),
                    None => true,
                };
                let mut f = finding("hibernation", name.trim_end_matches(".lnk").to_string());
                f.detail = "A shortcut in your Startup folder.".into();
                f.path = Some(entry.path().to_string_lossy().into_owned());
                f.enabled = Some(enabled);
                f.flags = vec!["Startup folder".into()];
                f.actions = if enabled {
                    vec!["disable"]
                } else {
                    vec!["enable"]
                };
                out.push((
                    f,
                    Target::Startup {
                        hive: "HKCU",
                        approved: APPROVED_FOLDER.into(),
                        name,
                    },
                ));
            }
        }
    }

    tick(0.25);
    // Busy third-party background processes (information: CPU and memory).
    if let Ok(text) = sys::powershell(
        "Get-Process | Where-Object { $_.Path -and $_.MainWindowHandle -eq 0 } | Sort-Object WorkingSet64 -Descending | Select-Object -First 40 Name,Path,CPU,WorkingSet64 | ConvertTo-Json -Compress",
    ) {
        let mut shown = 0;
        for item in sys::json_list(&text) {
            let path = item["Path"].as_str().unwrap_or_default().to_string();
            let name = item["Name"].as_str().unwrap_or_default().to_string();
            if sys::system_path(&path)
                || sys::protected_name(&path)
                || sys::protected_name(&name)
                || shown >= 12
            {
                continue;
            }
            shown += 1;
            let ram = item["WorkingSet64"].as_u64().unwrap_or(0);
            let cpu = item["CPU"].as_f64().unwrap_or(0.0);
            let mut f = finding("hibernation", name);
            f.detail = format!(
                "Runs in the background: {:.0} MB memory, {:.0} s CPU so far.",
                ram as f64 / 1_048_576.0,
                cpu
            );
            f.path = Some(path);
            f.size = Some(ram);
            f.flags = vec!["Process".into()];
            out.push((f, Target::Info));
        }
    }

    tick(0.6);
    // Third-party automatic services that are running now.
    if let Ok(text) = sys::powershell(
        "Get-CimInstance Win32_Service | Where-Object { $_.StartMode -eq 'Auto' -and $_.State -eq 'Running' } | Select-Object Name,DisplayName,PathName | ConvertTo-Json -Compress",
    ) {
        for item in sys::json_list(&text) {
            let name = item["Name"].as_str().unwrap_or_default().to_string();
            let display = item["DisplayName"].as_str().unwrap_or(&name).to_string();
            let path = item["PathName"].as_str().unwrap_or_default();
            let program = sys::program_of(path).unwrap_or_default();
            if name.is_empty()
                || sys::protected_name(&display)
                || sys::protected_name(&name)
                || sys::system_path(&program)
                || program.is_empty()
            {
                continue;
            }
            let mut f = finding("hibernation", display);
            f.detail = format!("A third-party service set to start automatically: {name}");
            f.path = Some(program);
            f.enabled = Some(true);
            f.flags = vec!["Service".into(), "Auto".into()];
            f.confidence = Some(60);
            f.actions = vec!["manual"];
            out.push((f, Target::Service { name, start: 2 }));
        }
    }

    out
}

// ─── 5. Software permissions ─────────────────────────────────────────────────

const CONSENT: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

fn permissions() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    let names = app_names();
    let caps = [
        ("webcam", "Κάμερα"),
        ("microphone", "Μικρόφωνο"),
        ("location", "Τοποθεσία"),
    ];
    for (cap, label) in caps {
        for scope in ["", r"\NonPackaged"] {
            let base = format!("{CONSENT}\\{cap}{scope}");
            let Some(root) = sys::open("HKCU", &base) else {
                continue;
            };
            for app in sys::subkeys(&root) {
                if app == "NonPackaged" {
                    continue;
                }
                let path = format!("{base}\\{app}");
                let Some(key) = sys::open("HKCU", &path) else {
                    continue;
                };
                let value = sys::string(&key, "Value").unwrap_or_default();
                if value != "Allow" && value != "Deny" {
                    continue;
                }
                let name = friendly_app(&app, &names);
                if sys::protected_name(&name) || sys::protected_name(&app) {
                    continue;
                }
                let mut f = finding("permissions", name);
                f.detail = format!(
                    "{label}: {}.",
                    if value == "Allow" {
                        "η πρόσβαση επιτρέπεται"
                    } else {
                        "η πρόσβαση έχει αποκλειστεί"
                    }
                );
                f.path = Some(if app.contains('#') {
                    app.replace('#', "\\")
                } else {
                    app
                });
                f.enabled = Some(value == "Allow");
                f.flags = vec![label.into()];
                f.actions = if value == "Allow" {
                    vec!["revoke"]
                } else {
                    vec!["allow"]
                };
                out.push((f, Target::Consent { path }));
            }
        }
    }

    // Background apps (packaged), on by default; off is reversible.
    let bg = r"SOFTWARE\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications";
    if let Some(root) = sys::open("HKCU", bg) {
        for app in sys::subkeys(&root) {
            let path = format!("{bg}\\{app}");
            let Some(key) = sys::open("HKCU", &path) else {
                continue;
            };
            let disabled = sys::dword(&key, "Disabled").unwrap_or(0) == 1;
            let name = friendly_app(&app, &names);
            if sys::protected_name(&name) || sys::protected_name(&app) {
                continue;
            }
            let mut f = finding("permissions", name);
            f.detail = if disabled {
                "Η εκτέλεση στο παρασκήνιο έχει αποκλειστεί."
            } else {
                "Μπορεί να εκτελείται στο παρασκήνιο όταν δεν είναι ανοιχτή."
            }
            .into();
            f.path = Some(app);
            f.enabled = Some(!disabled);
            f.flags = vec!["Παρασκήνιο".into()];
            f.actions = if disabled {
                vec!["allow"]
            } else {
                vec!["revoke"]
            };
            out.push((f, Target::Background { path }));
        }
    }

    out
}

/// A consent-store app id into something readable: `Microsoft.Foo_8wek…` →
/// `Microsoft.Foo`; a packaged family name kept as is; a path's file name.
fn pretty_app(app: &str) -> String {
    if app.contains('#') {
        // NonPackaged keys use `C:#Program Files#App#app.exe`.
        return app
            .replace('#', "\\")
            .rsplit('\\')
            .next()
            .unwrap_or(app)
            .to_string();
    }
    app.split('_').next().unwrap_or(app).to_string()
}

fn app_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    // Windows resolves localized resource names here, including numeric
    // package publishers. Never execute an app to identify it.
    if let Ok(text) = sys::powershell(
        "$shell = New-Object -ComObject Shell.Application; @($shell.NameSpace('shell:AppsFolder').Items() | ForEach-Object { [pscustomobject]@{ id = $_.Path; name = $_.Name } }) | ConvertTo-Json -Compress",
    ) {
        for item in sys::json_list(&text) {
            if let (Some(id), Some(name)) = (item["id"].as_str(), item["name"].as_str())
                && !name.trim().is_empty()
            {
                names.insert(
                    id.split('!').next().unwrap_or(id).to_lowercase(),
                    name.to_string(),
                );
            }
        }
    }
    let list = installed_list();
    let mut desktop = HashSet::new();
    for app in &list {
        for command in [&app.uninstall, &app.display_icon].into_iter().flatten() {
            if let Some(exe) = sys::program_of(command) {
                names.insert(exe.to_lowercase(), app.name.clone());
            }
        }
    }
    for cap in ["webcam", "microphone", "location"] {
        if let Some(root) = sys::open("HKCU", &format!("{CONSENT}\\{cap}\\NonPackaged")) {
            for app in sys::subkeys(&root) {
                desktop.insert(app.replace('#', "\\"));
            }
        }
    }
    if !desktop.is_empty() {
        let paths = desktop
            .iter()
            .map(|p| crate::apps::process::ps_quote(p))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            "@({paths}) | ForEach-Object {{ $p = $_; $v = (Get-Item -LiteralPath $p -ErrorAction SilentlyContinue).VersionInfo; $n = $v.FileDescription; if (-not $n) {{ $n = $v.ProductName }}; if ($n) {{ [pscustomobject]@{{ id = $p; name = $n }} }} }} | ConvertTo-Json -Compress"
        );
        if let Ok(text) = sys::powershell(&script) {
            for item in sys::json_list(&text) {
                if let (Some(id), Some(name)) = (item["id"].as_str(), item["name"].as_str()) {
                    names
                        .entry(id.to_lowercase())
                        .or_insert_with(|| name.to_string());
                }
            }
        }
    }
    names
}

fn friendly_app(app: &str, names: &HashMap<String, String>) -> String {
    let id = app.replace('#', "\\");
    let key = id.split('!').next().unwrap_or(&id).to_lowercase();
    if let Some(name) = names.get(&key) {
        return name.clone();
    }
    let raw = if id.contains(['\\', '/']) {
        id.clone()
    } else {
        pretty_app(&id)
    };
    let raw = raw
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&raw)
        .trim_end_matches(".exe");
    if raw.starts_with('{')
        || !raw.chars().any(char::is_alphabetic)
        || raw.len() >= 8 && raw.chars().all(|c| c.is_ascii_hexdigit())
    {
        return "Unidentified app".into();
    }
    let product = raw
        .rsplit('.')
        .find(|part| part.chars().any(char::is_alphabetic))
        .unwrap_or(raw);
    if product == "WhatsAppDesktop" {
        return "WhatsApp Desktop".into();
    }
    let mut readable = String::new();
    let mut lower = false;
    for c in product.chars() {
        if c.is_uppercase() && lower {
            readable.push(' ');
        }
        readable.push(if c == '-' { ' ' } else { c });
        lower = c.is_lowercase();
    }
    readable
}

// ─── 6. Disturbing notifications ─────────────────────────────────────────────

const NOTIFY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings";

fn notifications(tick: Tick) -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    let names = app_names();
    if let Some(root) = sys::open("HKCU", NOTIFY) {
        for app in sys::subkeys(&root) {
            let path = format!("{NOTIFY}\\{app}");
            let Some(key) = sys::open("HKCU", &path) else {
                continue;
            };
            let enabled = sys::dword(&key, "Enabled").unwrap_or(1) == 1;
            let name = friendly_app(&app, &names);
            if sys::protected_name(&name) || sys::protected_name(&app) {
                continue;
            }
            let mut f = finding("notifications", name);
            f.detail = "Windows notifications from this app.".into();
            f.enabled = Some(enabled);
            f.flags = vec!["Windows".into()];
            f.actions = if enabled {
                vec!["mute"]
            } else {
                vec!["unmute"]
            };
            out.push((f, Target::Notify { path }));
        }
    }

    // Browser site push permissions.
    tick(0.3);
    out.extend(chrome_sites("chrome.exe", r"Google\Chrome\User Data"));
    tick(0.5);
    out.extend(chrome_sites("msedge.exe", r"Microsoft\Edge\User Data"));
    tick(0.7);
    out.extend(chrome_sites(
        "brave.exe",
        r"BraveSoftware\Brave-Browser\User Data",
    ));
    out.extend(firefox_sites());
    out
}

fn chrome_profiles(user_data: &Path) -> Vec<PathBuf> {
    let mut profiles = vec![user_data.join("Default")];
    if let Ok(entries) = std::fs::read_dir(user_data) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("Profile ") {
                profiles.push(entry.path());
            }
        }
    }
    profiles
        .into_iter()
        .filter(|p| p.join("Preferences").is_file())
        .collect()
}

fn chrome_sites(exe: &'static str, rel: &str) -> Vec<(Finding, Target)> {
    let Some(local) = sys::env_dir("LOCALAPPDATA") else {
        return Vec::new();
    };
    let user_data = local.join(rel);
    let mut out = Vec::new();
    for profile in chrome_profiles(&user_data) {
        let prefs = profile.join("Preferences");
        let Ok(text) = std::fs::read_to_string(&prefs) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(sites) = json
            .pointer("/profile/content_settings/exceptions/notifications")
            .and_then(|v| v.as_object())
        else {
            continue;
        };
        for (key, entry) in sites {
            if entry.get("setting").and_then(|s| s.as_i64()) != Some(1) {
                continue; // 1 = allow.
            }
            let site = key.split(',').next().unwrap_or(key).to_string();
            let mut f = finding("notifications", site.clone());
            f.detail = format!(
                "{} may show push notifications from this site.",
                exe.trim_end_matches(".exe")
            );
            f.enabled = Some(true);
            f.flags = vec![exe.trim_end_matches(".exe").to_string()];
            if sys::running(exe) {
                f.flags.push("Close the browser".into());
            }
            f.actions = vec!["block"];
            out.push((
                f,
                Target::ChromeSite {
                    exe,
                    prefs: prefs.clone(),
                    key: key.clone(),
                },
            ));
        }
    }
    out
}

fn firefox_sites() -> Vec<(Finding, Target)> {
    let Some(roaming) = sys::env_dir("APPDATA") else {
        return Vec::new();
    };
    let profiles = roaming.join(r"Mozilla\Firefox\Profiles");
    let Ok(entries) = std::fs::read_dir(&profiles) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let db = entry.path().join("permissions.sqlite");
        if !db.is_file() {
            continue;
        }
        // permissions.sqlite is a locked binary; counting needs the browser's
        // own tools. Offer a single switch to block new requests via user.js.
        let mut f = finding("notifications", "Firefox: site notifications");
        f.detail = "Block new notification requests in this Firefox profile (existing ones stay until you clear them in Settings).".into();
        f.path = Some(entry.path().to_string_lossy().into_owned());
        f.flags = vec!["Firefox".into()];
        if sys::running("firefox.exe") {
            f.flags.push("Close Firefox".into());
        }
        f.actions = vec!["block"];
        out.push((
            f,
            Target::FirefoxNotify {
                exe: "firefox.exe",
                user_js: entry.path().join("user.js"),
            },
        ));
    }
    out
}

// ─── 7. Installation files ───────────────────────────────────────────────────

fn is_installer(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with(".msi") || lower.ends_with(".msix") || lower.ends_with(".appx") {
        return true;
    }
    if lower.ends_with(".exe") {
        const WORDS: &[&str] = &[
            "setup",
            "install",
            "installer",
            "update",
            "-x64",
            "_x64",
            "-amd64",
            "_amd64",
            "-win64",
            "websetup",
            "vc_redist",
            "redist",
        ];
        return WORDS.iter().any(|w| lower.contains(w));
    }
    false
}

fn installers() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let dirs: Vec<(PathBuf, bool)> = [
        sys::env_dir("TEMP").map(|p| (p, false)),
        sys::env_dir("LOCALAPPDATA").map(|p| (p.join("Temp"), false)),
        crate::cleaner::downloads::downloads_folder()
            .ok()
            .map(|p| (p, true)),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (dir, downloads) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten().take(5000) {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_installer(&name) || meta.len() < 1_000_000 {
                continue;
            }
            // Older than 7 days, like the Downloads rule.
            let old = meta
                .modified()
                .map(|m| sys::days_since(m) >= 7)
                .unwrap_or(false);
            if !old || !seen.insert(path.to_string_lossy().to_lowercase()) {
                continue;
            }
            let mut f = finding("installers", name);
            f.detail = format!("An old installer in {}.", dir.display());
            if downloads {
                f.flags.push("Downloads".into());
            }
            f.path = Some(path.to_string_lossy().into_owned());
            f.size = Some(meta.len());
            f.confidence = Some(85);
            f.recommended = true;
            f.actions = vec!["recycle"];
            out.push((f, Target::Paths(vec![path])));
        }
    }
    out
}

// ─── 8. Suspicious software & extensions ─────────────────────────────────────

fn suspicious(tick: Tick) -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    out.extend(browser_extensions("Chrome", r"Google\Chrome\User Data"));
    tick(0.1);
    out.extend(browser_extensions("Edge", r"Microsoft\Edge\User Data"));
    tick(0.2);
    out.extend(browser_extensions(
        "Brave",
        r"BraveSoftware\Brave-Browser\User Data",
    ));
    tick(0.35);
    out.extend(suspicious_startup());
    tick(0.6);
    out.extend(suspicious_tasks());
    out
}

fn browser_extensions(browser: &'static str, rel: &str) -> Vec<(Finding, Target)> {
    let Some(local) = sys::env_dir("LOCALAPPDATA") else {
        return Vec::new();
    };
    let user_data = local.join(rel);
    let mut out = Vec::new();
    for profile in chrome_profiles(&user_data) {
        let prefs = profile.join("Preferences");
        let Ok(text) = std::fs::read_to_string(&prefs) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(exts) = json
            .pointer("/extensions/settings")
            .and_then(|v| v.as_object())
        else {
            continue;
        };
        for (ext_id, data) in exts {
            // location: 1 internal (store), 4 external reg, 5 command line /
            // unpacked, 10 component. Store-installed and components are fine.
            let location = data.get("location").and_then(|l| l.as_i64()).unwrap_or(0);
            let from_store = data
                .get("from_webstore")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let name = data
                .pointer("/manifest/name")
                .and_then(|n| n.as_str())
                .unwrap_or("Unknown extension")
                .to_string();
            if name.starts_with("__MSG") || data.get("manifest").is_none() {
                continue;
            }
            let mut flags = Vec::new();
            if matches!(location, 5) {
                flags.push("Unpacked / developer mode".to_string());
            }
            if matches!(location, 4) {
                flags.push("Side-loaded".to_string());
            }
            if data
                .get("was_installed_by_default")
                .and_then(|b| b.as_bool())
                == Some(false)
                && !from_store
                && !matches!(location, 10)
            {
                flags.push("Not from the Web Store".to_string());
            }
            if flags.is_empty() {
                continue;
            }
            let mut f = finding("suspicious", format!("{name} ({browser})"));
            f.detail = format!(
                "Extension {ext_id} in {}.",
                profile
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            f.flags = flags;
            f.confidence = Some(50);
            // Never auto-removed: show it, and open the extensions page to act.
            let exe = match browser {
                "Edge" => "msedge.exe",
                "Brave" => "brave.exe",
                _ => "chrome.exe",
            };
            f.actions = vec!["open"];
            out.push((
                f,
                Target::Open {
                    exe: exe.into(),
                    url: format!("{}extensions/", scheme(browser)),
                },
            ));
        }
    }
    out
}

fn scheme(browser: &str) -> &'static str {
    match browser {
        "Edge" => "edge://",
        "Brave" => "brave://",
        _ => "chrome://",
    }
}

fn suspicious_startup() -> Vec<(Finding, Target)> {
    let mut programs = Vec::new();
    for hive in ["HKLM", "HKCU"] {
        if let Some(key) = sys::open(hive, RUN) {
            for name in key.enum_values().flatten().map(|(n, _)| n) {
                if let Some(program) = sys::string(&key, &name).and_then(|c| sys::program_of(&c))
                    && Path::new(&program).is_file()
                    && !sys::system_path(&program)
                    && !sys::protected_name(&program)
                {
                    programs.push((hive, name, program));
                }
            }
        }
    }
    let paths: Vec<String> = programs.iter().map(|(_, _, p)| p.clone()).collect();
    let sigs = sys::signatures(&paths);
    let mut out = Vec::new();
    for (hive, name, program) in programs {
        let sig = sigs
            .get(&program.to_lowercase())
            .cloned()
            .unwrap_or_default();
        if sig.signed {
            continue;
        }
        let mut f = finding("suspicious", name.clone());
        f.detail = if sig.signer.is_empty() {
            format!("A startup program with no signature: {program}")
        } else {
            format!(
                "A startup program whose signature ({}) is not valid: {program}",
                sig.signer
            )
        };
        f.path = Some(program);
        f.enabled = Some(startup_enabled(hive, &name));
        f.flags = vec!["Startup".into(), "Unsigned".into()];
        f.confidence = Some(45);
        f.actions = vec!["disable"];
        out.push((
            f,
            Target::Startup {
                hive,
                approved: APPROVED_RUN.into(),
                name,
            },
        ));
    }
    out
}

fn suspicious_tasks() -> Vec<(Finding, Target)> {
    let script = "Get-ScheduledTask | Where-Object { $_.State -ne 'Disabled' -and $_.Principal.RunLevel -ne $null } | ForEach-Object { $a = $_.Actions | Where-Object { $_.Execute }; if ($a) { [pscustomobject]@{ name = $_.TaskPath + $_.TaskName; exe = $a[0].Execute } } } | ConvertTo-Json -Compress";
    let Ok(text) = sys::powershell(script) else {
        return Vec::new();
    };
    let items = sys::json_list(&text);
    let mut resolved: Vec<(String, String)> = Vec::new();
    for item in items {
        let name = item["name"].as_str().unwrap_or_default().to_string();
        let raw = item["exe"].as_str().unwrap_or_default();
        let exe = sys::expand(raw.trim_matches('"'));
        if name.is_empty() || exe.is_empty() {
            continue;
        }
        // Microsoft's own tasks live under \Microsoft\; skip them and system exes.
        if name.starts_with(r"\Microsoft\") || sys::system_path(&exe) || sys::protected_name(&exe) {
            continue;
        }
        if Path::new(&exe).is_file() {
            resolved.push((name, exe));
        }
    }
    let paths: Vec<String> = resolved.iter().map(|(_, e)| e.clone()).collect();
    let sigs = sys::signatures(&paths);
    let mut out = Vec::new();
    for (name, exe) in resolved {
        let sig = sigs.get(&exe.to_lowercase()).cloned().unwrap_or_default();
        if sig.signed {
            continue;
        }
        let mut f = finding("suspicious", name.trim_start_matches('\\').to_string());
        f.detail = format!("A scheduled task running an unsigned program: {exe}");
        f.path = Some(exe);
        f.enabled = Some(true);
        f.flags = vec!["Scheduled task".into(), "Unsigned".into()];
        f.confidence = Some(45);
        f.actions = vec!["disable", "enable"];
        out.push((f, Target::Task { name }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installers_are_told_from_ordinary_exes() {
        assert!(is_installer("npp.8.5.Installer.x64.exe"));
        assert!(is_installer("python-3.12.0-amd64.exe"));
        assert!(is_installer("package.msi"));
        assert!(!is_installer("game.exe"));
        assert!(!is_installer("notes.txt"));
    }

    #[test]
    fn app_ids_become_readable() {
        assert_eq!(
            pretty_app("Microsoft.WindowsStore_8wekyb3d8bbwe"),
            "Microsoft.WindowsStore"
        );
        assert_eq!(pretty_app(r"C:#Program Files#Foo#foo.exe"), "foo.exe");
    }

    #[test]
    fn existing_installation_evidence_prevents_false_broken_entries() {
        assert!(!broken_candidate(
            sys::Presence::Present,
            sys::Presence::Missing,
            sys::Presence::Unknown
        ));
        assert!(!broken_candidate(
            sys::Presence::Missing,
            sys::Presence::Present,
            sys::Presence::Unknown
        ));
        assert!(!broken_candidate(
            sys::Presence::Missing,
            sys::Presence::Missing,
            sys::Presence::Present
        ));
        assert!(!broken_candidate(
            sys::Presence::Unknown,
            sys::Presence::Unknown,
            sys::Presence::Unknown
        ));
        assert!(broken_candidate(
            sys::Presence::Missing,
            sys::Presence::Missing,
            sys::Presence::Unknown
        ));
    }

    #[test]
    fn registry_product_names_match_vendor_and_portable_folder_names() {
        let root_path = format!("Software\\MYLE-Health-Scan-Test-{}", uuid::Uuid::new_v4());
        let registry = sys::root("HKCU");
        for (key_name, display, folder) in [
            (
                "Java",
                "Java 8 Update 451",
                r"C:\Program Files (x86)\Java\jre1.8",
            ),
            (
                "Alienware",
                "Alienware Command Center",
                r"C:\Program Files\Alienware\Command Center",
            ),
            ("MO2", "Mod Organizer 2", r"C:\Tools\ModOrganizer"),
            ("Loot", "LOOT", r"C:\Tools\LOOT"),
        ] {
            let (key, _) = registry
                .create_subkey(format!("{root_path}\\WOW6432Node\\{key_name}"))
                .unwrap();
            key.set_value("DisplayName", &display).unwrap();
            key.set_value("InstallLocation", &folder).unwrap();
        }
        let fixture_root = format!("{root_path}\\WOW6432Node");
        let records = uninstall_keys_at(&[("HKCU", &fixture_root)]);
        assert_eq!(records.len(), 4);
        assert!(
            records
                .iter()
                .all(|(_, path, _)| path.starts_with(&fixture_root))
        );
        let tokens = installed_tokens(
            &records
                .into_iter()
                .map(|(_, _, app)| app)
                .collect::<Vec<_>>(),
        );
        for folder in ["Java", "Alienware", "ModOrganizer", "LOOT"] {
            let folder = sys::norm(folder);
            assert!(
                tokens
                    .iter()
                    .any(|t| folder.contains(t) || t.contains(&folder)),
                "{folder} missing from installed inventory"
            );
        }
        registry.delete_subkey_all(root_path).unwrap();
    }

    #[test]
    fn permission_names_resolve_packages_and_desktop_paths_without_showing_ids() {
        let names = std::collections::HashMap::from([(
            "5319275a.whatsappdesktop_cv1g1gvanyjgm".into(),
            "WhatsApp".into(),
        )]);
        assert_eq!(
            friendly_app("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App", &names),
            "WhatsApp"
        );
        assert_eq!(
            friendly_app(r"C:#Program Files#Discord#Discord.exe", &names),
            "Discord"
        );
        assert_eq!(
            friendly_app("{019AD101-4321-4567-89AB-112233445566}", &names),
            "Unidentified app"
        );
        assert_eq!(
            friendly_app(
                "5319275A.WhatsAppDesktop_cv1g1gvanyjgm",
                &Default::default()
            ),
            "WhatsApp Desktop"
        );
    }

    #[test]
    fn registered_paths_protect_vendor_folders_and_their_children() {
        assert!(related_folder(
            Path::new(r"C:\Program Files\Vendor"),
            Path::new(r"C:\Program Files\Vendor\App")
        ));
        assert!(related_folder(
            Path::new(r"C:\Program Files\Vendor\App\Data"),
            Path::new(r"c:\program files\vendor\app")
        ));
        assert!(!related_folder(
            Path::new(r"C:\Program Files\Vendor2"),
            Path::new(r"C:\Program Files\Vendor")
        ));
    }

    #[test]
    fn existing_icon_and_portable_executables_are_installation_evidence() {
        let dir =
            std::env::temp_dir().join(format!("myle-health-evidence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("App icon.ico"), b"fixture").unwrap();
        let mut app = Installed {
            name: "Fixture".into(),
            install_location: Some(dir.join("missing").to_string_lossy().into_owned()),
            uninstall: Some(dir.join("missing.exe").to_string_lossy().into_owned()),
            publisher: String::new(),
            display_icon: Some(format!("\"{}\",0", dir.join("App icon.ico").display())),
        };
        let (location, uninstaller, icon) = installation_status(&app);
        assert!(!broken_candidate(location, uninstaller, icon));
        app.display_icon = Some(format!("{},0", dir.join("App icon.ico").display()));
        let (location, uninstaller, icon) = installation_status(&app);
        assert!(!broken_candidate(location, uninstaller, icon));
        std::fs::write(dir.join("ModOrganizer.exe"), b"fixture").unwrap();
        assert!(has_program_files(&dir, 2, &mut 1500));
        std::fs::remove_file(dir.join("ModOrganizer.exe")).unwrap();
        std::fs::remove_file(dir.join("App icon.ico")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn deeply_nested_portable_apps_are_not_classified_as_abandoned() {
        let dir = std::env::temp_dir().join(format!("myle-health-deep-{}", uuid::Uuid::new_v4()));
        let nested = dir.join(r"a\b\c");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("LOOT.exe"), b"fixture").unwrap();
        assert!(has_program_files(&dir, 2, &mut 1500));
        std::fs::remove_file(nested.join("LOOT.exe")).unwrap();
        for path in [&nested, &dir.join(r"a\b"), &dir.join("a"), &dir] {
            std::fs::remove_dir(path).unwrap();
        }
    }

    #[test]
    fn unrecognized_folders_need_positive_uninstallation_evidence() {
        assert!(!orphan_folder_evidence(
            false,
            sys::Presence::Missing,
            sys::Presence::Unknown
        ));
        assert!(!orphan_folder_evidence(
            true,
            sys::Presence::Present,
            sys::Presence::Missing
        ));
        assert!(!orphan_folder_evidence(
            true,
            sys::Presence::Missing,
            sys::Presence::Present
        ));
        assert!(!orphan_folder_evidence(
            true,
            sys::Presence::Unknown,
            sys::Presence::Missing
        ));
        assert!(orphan_folder_evidence(
            true,
            sys::Presence::Missing,
            sys::Presence::Unknown
        ));
    }

    #[test]
    fn folder_ownership_requires_an_exact_registered_installation() {
        let dir = std::env::temp_dir().join(format!("myle-health-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        for name in [
            "LOOT",
            "ModOrganizer",
            "NuGet",
            "AvaloniaUI",
            "KojimaProductions",
        ] {
            assert!(orphan_folder_owner(&dir.join(name), &[]).is_none());
        }
        let mut app = Installed {
            name: "Fixture application".into(),
            install_location: Some(dir.to_string_lossy().into_owned()),
            uninstall: Some(format!(
                "\"{}\" /uninstall",
                dir.join("missing.exe").display()
            )),
            publisher: String::new(),
            display_icon: None,
        };
        assert!(orphan_folder_owner(&dir, std::slice::from_ref(&app)).is_some());
        assert!(orphan_folder_owner(&dir.join("Data"), std::slice::from_ref(&app)).is_none());
        assert!(orphan_folder_owner(dir.parent().unwrap(), std::slice::from_ref(&app)).is_none());
        std::fs::write(dir.join("uninstall.exe"), b"fixture").unwrap();
        app.uninstall = Some(format!("\"{}\"", dir.join("uninstall.exe").display()));
        assert!(orphan_folder_owner(&dir, std::slice::from_ref(&app)).is_none());
        std::fs::remove_file(dir.join("uninstall.exe")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
