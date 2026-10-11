//! The eight Software Health scans. Each returns findings paired with the
//! target the backend keeps to act on them. Every scan is read-only and
//! holds to the safety lists in `sys`: Windows, Microsoft, drivers, hardware
//! vendors and MYLE are never offered for a change.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::sys;
use super::{Finding, Target};

pub fn scan(module: &str) -> Result<Vec<(Finding, Target)>, String> {
    match module {
        "leftovers" => Ok(leftovers()),
        "broken" => Ok(broken()),
        "redundant" => Ok(redundant()),
        "hibernation" => Ok(hibernation()),
        "permissions" => Ok(permissions()),
        "notifications" => Ok(notifications()),
        "installers" => Ok(installers()),
        "suspicious" => Ok(suspicious()),
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
    }
}

// ─── Installed software, once, for several scans ─────────────────────────────

#[derive(Clone)]
struct Installed {
    name: String,
    install_location: Option<String>,
    uninstall: Option<String>,
    publisher: String,
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

fn uninstall_keys() -> Vec<(&'static str, String, Installed)> {
    let mut out = Vec::new();
    for hive in ["HKLM", "HKCU"] {
        let Some(root) = sys::open(hive, UNINSTALL) else {
            continue;
        };
        for name in sys::subkeys(&root) {
            let Some(key) = sys::open(hive, &format!("{UNINSTALL}\\{name}")) else {
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
                name,
                Installed {
                    name: display,
                    install_location: sys::string(&key, "InstallLocation"),
                    uninstall: sys::string(&key, "UninstallString")
                        .or_else(|| sys::string(&key, "QuietUninstallString")),
                    publisher: sys::string(&key, "Publisher").unwrap_or_default(),
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

/// Normalised vendor and product words of everything installed, for matching
/// a folder's name against software that is really there.
fn installed_tokens(list: &[Installed]) -> HashSet<String> {
    let mut set = HashSet::new();
    for app in list {
        for word in app.name.split(|c: char| !c.is_alphanumeric()) {
            let w = sys::norm(word);
            if w.len() >= 4 {
                set.insert(w);
            }
        }
        if let Some(location) = &app.install_location
            && let Some(folder) = Path::new(location).file_name()
        {
            let w = sys::norm(&folder.to_string_lossy());
            if w.len() >= 4 {
                set.insert(w);
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
fn live_tokens() -> HashSet<String> {
    let script = "$p = Get-Process | Where-Object Path | ForEach-Object { Split-Path (Split-Path $_.Path) -Leaf; Split-Path $_.Path -Leaf }; $s = Get-CimInstance Win32_Service | ForEach-Object { $_.PathName }; $a = Get-AppxPackage | ForEach-Object { $_.Name }; @($p) + @($s) + @($a) | ConvertTo-Json -Compress";
    let mut set = HashSet::new();
    if let Ok(text) = sys::powershell(script) {
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
    set
}

// ─── 1. Uninstallation leftovers ─────────────────────────────────────────────

fn leftovers() -> Vec<(Finding, Target)> {
    let list = installed_list();
    let mut tokens = installed_tokens(&list);
    tokens.extend(live_tokens());
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
    for root in roots {
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
            let confidence: u8 =
                (if folder.len() >= 6 { 60 } else { 45 }) + if idle > 180 { 20 } else { 5 };
            let size = sys::dir_size(&path, 20_000);
            if size == 0 {
                continue;
            }
            let mut f = finding("leftovers", name.clone());
            f.detail = format!(
                "In {}; no installed software matches this folder.",
                root.display()
            );
            f.path = Some(path.to_string_lossy().into_owned());
            f.size = Some(size);
            f.confidence = Some(confidence);
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
        let has_location = app
            .install_location
            .as_deref()
            .map(|l| Path::new(l).is_dir())
            .unwrap_or(false);
        let has_uninstaller = app
            .uninstall
            .as_deref()
            .and_then(sys::program_of)
            .map(|p| Path::new(&p).is_file())
            .unwrap_or(false);
        if has_location || has_uninstaller || app.uninstall.is_none() {
            continue;
        }
        // This is a dead record; Broken uninstalls offers Force Clean. Here,
        // only the plain orphaned key with nothing on disk at all.
        if app.install_location.is_some() {
            continue;
        }
        let mut f = finding("leftovers", format!("Registry: {}", app.name));
        f.detail = format!("An Uninstall entry under {hive} with no uninstaller and no folder.");
        f.path = Some(format!("{hive}\\{UNINSTALL}\\{key_name}"));
        f.confidence = Some(60);
        f.actions = vec!["deleteKey"];
        out.push((
            f,
            Target::UninstallKey {
                hive,
                path: format!("{UNINSTALL}\\{key_name}"),
                dirs: Vec::new(),
            },
        ));
    }

    out
}

// ─── 2. Broken uninstalls ────────────────────────────────────────────────────

fn broken() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    for (hive, key_name, app) in uninstall_keys() {
        if sys::protected_name(&app.name) || sys::protected_name(&app.publisher) {
            continue;
        }
        let location = app.install_location.clone();
        let location_missing = location
            .as_deref()
            .map(|l| !l.trim().is_empty() && !Path::new(l).is_dir())
            .unwrap_or(false);
        let uninstaller_missing = app
            .uninstall
            .as_deref()
            .and_then(sys::program_of)
            .map(|p| !Path::new(&p).is_file())
            .unwrap_or(app.uninstall.is_none());
        if !location_missing && !uninstaller_missing {
            continue;
        }
        let dirs: Vec<PathBuf> = location
            .as_deref()
            .map(PathBuf::from)
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
        f.path = Some(format!("{hive}\\{UNINSTALL}\\{key_name}"));
        f.confidence = Some(75);
        f.flags = why.iter().map(|w| w.to_string()).collect();
        f.actions = vec!["forceClean", "deleteKey"];
        out.push((
            f,
            Target::UninstallKey {
                hive,
                path: format!("{UNINSTALL}\\{key_name}"),
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

fn hibernation() -> Vec<(Finding, Target)> {
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
    let caps = [
        ("webcam", "Camera"),
        ("microphone", "Microphone"),
        ("location", "Location"),
    ];
    for (cap, label) in caps {
        for scope in ["", r"\NonPackaged"] {
            let base = format!("{CONSENT}\\{cap}{scope}");
            let Some(root) = sys::open("HKCU", &base) else {
                continue;
            };
            for app in sys::subkeys(&root) {
                let path = format!("{base}\\{app}");
                let Some(key) = sys::open("HKCU", &path) else {
                    continue;
                };
                let value = sys::string(&key, "Value").unwrap_or_default();
                if value != "Allow" {
                    continue;
                }
                let name = pretty_app(&app);
                if sys::protected_name(&name) {
                    continue;
                }
                let mut f = finding("permissions", format!("{name}: {label}"));
                f.detail = format!("This app may use your {}.", label.to_lowercase());
                f.enabled = Some(true);
                f.flags = vec![label.into()];
                f.actions = vec!["revoke"];
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
            let name = pretty_app(&app);
            if sys::protected_name(&name) {
                continue;
            }
            let mut f = finding("permissions", format!("{name}: background"));
            f.detail = "This app may keep running in the background.".into();
            f.enabled = Some(!disabled);
            f.flags = vec!["Background".into()];
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

// ─── 6. Disturbing notifications ─────────────────────────────────────────────

const NOTIFY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings";

fn notifications() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    if let Some(root) = sys::open("HKCU", NOTIFY) {
        for app in sys::subkeys(&root) {
            let path = format!("{NOTIFY}\\{app}");
            let Some(key) = sys::open("HKCU", &path) else {
                continue;
            };
            let enabled = sys::dword(&key, "Enabled").unwrap_or(1) == 1;
            let name = pretty_app(&app);
            if sys::protected_name(&name) {
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
    out.extend(chrome_sites("chrome.exe", r"Google\Chrome\User Data"));
    out.extend(chrome_sites("msedge.exe", r"Microsoft\Edge\User Data"));
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
            f.actions = vec!["recycle"];
            out.push((f, Target::Paths(vec![path])));
        }
    }
    out
}

// ─── 8. Suspicious software & extensions ─────────────────────────────────────

fn suspicious() -> Vec<(Finding, Target)> {
    let mut out = Vec::new();
    out.extend(browser_extensions("Chrome", r"Google\Chrome\User Data"));
    out.extend(browser_extensions("Edge", r"Microsoft\Edge\User Data"));
    out.extend(browser_extensions(
        "Brave",
        r"BraveSoftware\Brave-Browser\User Data",
    ));
    out.extend(suspicious_startup());
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
}
