//! System Cleaner's Software Health: eight scans of what installed
//! software leaves, starts, may use and shows, and the reversible changes
//! offered for each finding.
//!
//! The page sends finding ids only. What a finding points at (a folder, a
//! registry key, a service) stays here, from the scan that found it, so a
//! change can only ever touch what a scan listed. Every change is
//! non-destructive: files go to the Recycle Bin, a registry key is exported
//! to a .reg file before it is deleted, and every switch is written to the
//! restore list with the value it had.

mod scans;
mod sys;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use winreg::RegValue;
use winreg::enums::{KEY_ALL_ACCESS, KEY_WOW64_64KEY, RegType};

/// The eight modules, in page order.
pub const MODULES: [&str; 8] = [
    "leftovers",
    "broken",
    "redundant",
    "hibernation",
    "permissions",
    "notifications",
    "installers",
    "suspicious",
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub module: &'static str,
    pub title: String,
    pub detail: String,
    pub path: Option<String>,
    pub size: Option<u64>,
    /// 0..100: how sure the scan is that this is safe to act on.
    pub confidence: Option<u8>,
    /// For switches: on now?
    pub enabled: Option<bool>,
    /// Short labels: "Unsigned", "Developer mode", "Needs administrator"…
    pub flags: Vec<String>,
    /// What can be done: "recycle", "forceClean", "deleteKey", "disable",
    /// "enable", "manual", "revoke", "allow", "mute", "unmute", "block", "open".
    pub actions: Vec<&'static str>,
}

/// What a finding points at; never leaves the backend.
#[derive(Clone, Debug)]
pub enum Target {
    Paths(Vec<PathBuf>),
    UninstallKey {
        hive: &'static str,
        path: String,
        dirs: Vec<PathBuf>,
    },
    Startup {
        hive: &'static str,
        approved: String,
        name: String,
    },
    Service {
        name: String,
        start: u32,
    },
    Consent {
        path: String,
    },
    Background {
        path: String,
    },
    Notify {
        path: String,
    },
    ChromeSite {
        exe: &'static str,
        prefs: PathBuf,
        key: String,
    },
    FirefoxNotify {
        exe: &'static str,
        user_js: PathBuf,
    },
    Open {
        exe: String,
        url: String,
    },
    Task {
        name: String,
    },
    Info,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Undo {
    RegImport {
        file: String,
    },
    Binary {
        hive: String,
        path: String,
        name: String,
        old: Option<Vec<u8>>,
    },
    Text {
        hive: String,
        path: String,
        name: String,
        old: Option<String>,
    },
    Dword {
        hive: String,
        path: String,
        names: Vec<String>,
        old: Vec<Option<u32>>,
    },
    Service {
        name: String,
        start: u32,
    },
    ChromeSite {
        exe: String,
        prefs: String,
        key: String,
        old: serde_json::Value,
    },
    UserJs {
        exe: String,
        path: String,
        old: Option<String>,
    },
    Task {
        name: String,
    },
    /// Files in the Recycle Bin: restored from there.
    Recycled {
        paths: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreEntry {
    pub id: String,
    /// Unix milliseconds.
    pub at: i64,
    pub module: String,
    pub title: String,
    pub undo: Undo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleReport {
    pub module: &'static str,
    pub findings: Vec<Finding>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum HealthEvent {
    Module {
        module: &'static str,
        done: usize,
        total: usize,
    },
    Found {
        module: &'static str,
        count: usize,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub id: String,
    pub ok: bool,
    pub message: Option<String>,
}

static FOUND: LazyLock<Mutex<HashMap<String, (Finding, Target)>>> = LazyLock::new(Default::default);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn health_dir() -> Result<PathBuf, String> {
    let dir = crate::storage::roaming_dir()?.join("software-health");
    std::fs::create_dir_all(dir.join("backups")).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn journal_path() -> Result<PathBuf, String> {
    Ok(health_dir()?.join("restore.json"))
}

fn read_journal() -> Vec<RestoreEntry> {
    journal_path()
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_journal(list: &[RestoreEntry]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(journal_path()?, bytes).map_err(|e| e.to_string())
}

fn remember(module: &str, title: &str, undo: Undo) -> Result<(), String> {
    let mut list = read_journal();
    list.insert(
        0,
        RestoreEntry {
            id: uuid::Uuid::new_v4().simple().to_string(),
            at: now_ms(),
            module: module.into(),
            title: title.into(),
            undo,
        },
    );
    list.truncate(500);
    write_journal(&list)
}

// ─── Commands ──────────────────────────────────────────────────────────────

/// Scans `modules` (all when empty), reporting each as it starts and ends.
#[tauri::command]
pub async fn cleaner_health_scan(
    modules: Vec<String>,
    on_event: Channel<HealthEvent>,
) -> Result<Vec<ModuleReport>, String> {
    let wanted: Vec<&'static str> = MODULES
        .iter()
        .copied()
        .filter(|m| modules.is_empty() || modules.iter().any(|w| w == m))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let total = wanted.len();
        let mut reports = Vec::new();
        let mut found = HashMap::new();
        for (done, module) in wanted.iter().enumerate() {
            let _ = on_event.send(HealthEvent::Module {
                module,
                done,
                total,
            });
            let (items, error) = match scans::scan(module) {
                Ok(items) => (items, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            let _ = on_event.send(HealthEvent::Found {
                module,
                count: items.len(),
            });
            let mut findings = Vec::new();
            for (finding, target) in items {
                findings.push(finding.clone());
                found.insert(finding.id.clone(), (finding, target));
            }
            reports.push(ModuleReport {
                module,
                findings,
                error,
            });
        }
        let _ = on_event.send(HealthEvent::Module {
            module: "",
            done: total,
            total,
        });
        let mut store = FOUND.lock().unwrap_or_else(|p| p.into_inner());
        store.retain(|_, (f, _)| !wanted.contains(&f.module));
        store.extend(found);
        reports
    })
    .await
    .map_err(|e| e.to_string())
}

/// Does `action` to the findings `ids` of the last scan.
#[tauri::command]
pub async fn cleaner_health_apply(
    ids: Vec<String>,
    action: String,
) -> Result<Vec<ApplyResult>, String> {
    let items: Vec<(String, Option<(Finding, Target)>)> = {
        let store = FOUND.lock().unwrap_or_else(|p| p.into_inner());
        ids.iter()
            .map(|id| (id.clone(), store.get(id).cloned()))
            .collect()
    };
    let mut results = Vec::new();
    for (id, item) in items {
        let Some((finding, target)) = item else {
            results.push(ApplyResult {
                id,
                ok: false,
                message: Some("Scan again: this item is no longer listed.".into()),
            });
            continue;
        };
        if !finding.actions.contains(&action.as_str()) {
            results.push(ApplyResult {
                id,
                ok: false,
                message: Some(format!("\"{action}\" is not offered for this item.")),
            });
            continue;
        }
        let outcome = apply_one(&finding, &target, &action).await;
        results.push(match outcome {
            Ok(()) => ApplyResult {
                id,
                ok: true,
                message: None,
            },
            Err(message) => ApplyResult {
                id,
                ok: false,
                message: Some(message),
            },
        });
    }
    Ok(results)
}

#[tauri::command]
pub fn cleaner_health_restore_list() -> Vec<RestoreEntry> {
    read_journal()
}

/// Puts back what a restore entry changed, and drops it from the list.
#[tauri::command]
pub async fn cleaner_health_restore(id: String) -> Result<(), String> {
    let list = read_journal();
    let entry = list
        .iter()
        .find(|e| e.id == id)
        .cloned()
        .ok_or("No such change on the restore list.")?;
    undo(&entry.undo).await?;
    let rest: Vec<RestoreEntry> = list.into_iter().filter(|e| e.id != id).collect();
    write_journal(&rest)
}

// ─── Changes ───────────────────────────────────────────────────────────────

fn writable(hive: &str, path: &str) -> Result<winreg::RegKey, String> {
    sys::root(hive)
        .create_subkey_with_flags(path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
        .map(|(key, _)| key)
        .map_err(|e| denied(hive, e))
}

fn denied(hive: &str, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::PermissionDenied && hive == "HKLM" {
        "Needs administrator: run MYLE as administrator for this one.".into()
    } else {
        error.to_string()
    }
}

/// Before a browser file is edited: the browser must be closed.
fn closed(exe: &str) -> Result<(), String> {
    if sys::running(exe) {
        Err(format!(
            "Close {} first: it rewrites its settings while open.",
            exe.trim_end_matches(".exe")
        ))
    } else {
        Ok(())
    }
}

async fn apply_one(finding: &Finding, target: &Target, action: &str) -> Result<(), String> {
    let module = finding.module;
    let title = &finding.title;
    match (target, action) {
        (Target::Paths(paths), "recycle") => {
            let present: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
            if present.is_empty() {
                return Ok(());
            }
            sys::recycle(&present)?;
            remember(
                module,
                title,
                Undo::Recycled {
                    paths: present
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect(),
                },
            )
        }
        (Target::UninstallKey { hive, path, dirs }, "deleteKey" | "forceClean") => {
            let file = health_dir()?.join("backups").join(format!(
                "{}-{}.reg",
                sys::norm(title),
                now_ms()
            ));
            sys::export_key(hive, path, &file)?;
            if action == "forceClean" {
                let present: Vec<PathBuf> = dirs.iter().filter(|d| d.is_dir()).cloned().collect();
                if !present.is_empty() {
                    sys::recycle(&present)?;
                    remember(
                        module,
                        &format!("{title}: folders"),
                        Undo::Recycled {
                            paths: present
                                .iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect(),
                        },
                    )?;
                }
            }
            sys::root(hive)
                .delete_subkey_all(path)
                .map_err(|e| denied(hive, e))?;
            remember(
                module,
                title,
                Undo::RegImport {
                    file: file.to_string_lossy().into_owned(),
                },
            )
        }
        (
            Target::Startup {
                hive,
                approved,
                name,
            },
            "disable" | "enable",
        ) => {
            let key = writable(hive, approved)?;
            let old = key.get_raw_value(name).ok().map(|v| v.bytes.into_owned());
            let mut bytes = vec![
                if action == "disable" { 3u8 } else { 2u8 },
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ];
            if action == "disable" {
                // When it was turned off (FILETIME), as Task Manager writes it.
                let ft = (now_ms() as u64 / 1000 + 11_644_473_600) * 10_000_000;
                bytes[4..12].copy_from_slice(&ft.to_le_bytes());
            }
            key.set_raw_value(
                name,
                &RegValue {
                    bytes: bytes.into(),
                    vtype: RegType::REG_BINARY,
                },
            )
            .map_err(|e| denied(hive, e))?;
            remember(
                module,
                &format!("{title} ({action}d)"),
                Undo::Binary {
                    hive: (*hive).into(),
                    path: approved.clone(),
                    name: name.clone(),
                    old,
                },
            )
        }
        (Target::Service { name, start }, "manual") => {
            set_service_start(name, "demand").await?;
            remember(
                module,
                &format!("{title}: Manual"),
                Undo::Service {
                    name: name.clone(),
                    start: *start,
                },
            )
        }
        (Target::Consent { path }, "revoke" | "allow") => {
            let key = writable("HKCU", path)?;
            let old = sys::string(&key, "Value");
            key.set_value("Value", &if action == "revoke" { "Deny" } else { "Allow" })
                .map_err(|e| e.to_string())?;
            remember(
                module,
                &format!("{title} ({action})"),
                Undo::Text {
                    hive: "HKCU".into(),
                    path: path.clone(),
                    name: "Value".into(),
                    old,
                },
            )
        }
        (Target::Background { path }, "revoke" | "allow") => {
            let names = vec!["Disabled".to_string(), "DisabledByUser".to_string()];
            set_dwords(
                module,
                title,
                action,
                "HKCU",
                path,
                &names,
                if action == "revoke" { 1 } else { 0 },
            )
        }
        (Target::Notify { path }, "mute" | "unmute") => {
            let names = vec!["Enabled".to_string()];
            set_dwords(
                module,
                title,
                action,
                "HKCU",
                path,
                &names,
                if action == "mute" { 0 } else { 1 },
            )
        }
        (Target::ChromeSite { exe, prefs, key }, "block") => {
            closed(exe)?;
            let text = std::fs::read_to_string(prefs).map_err(|e| e.to_string())?;
            let mut json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let site = json
                .pointer_mut("/profile/content_settings/exceptions/notifications")
                .and_then(|n| n.get_mut(key))
                .ok_or("The site is no longer in the browser's settings.")?;
            let old = site
                .get("setting")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            site["setting"] = serde_json::json!(2);
            std::fs::write(prefs, serde_json::to_vec(&json).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            remember(
                module,
                &format!("{title} (blocked)"),
                Undo::ChromeSite {
                    exe: (*exe).into(),
                    prefs: prefs.to_string_lossy().into_owned(),
                    key: key.clone(),
                    old,
                },
            )
        }
        (Target::FirefoxNotify { exe, user_js }, "block") => {
            closed(exe)?;
            let old = std::fs::read_to_string(user_js).ok();
            let mut text = old.clone().unwrap_or_default();
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            text.push_str("user_pref(\"permissions.default.desktop-notification\", 2); // MYLE: block new notification requests\n");
            std::fs::write(user_js, text).map_err(|e| e.to_string())?;
            remember(
                module,
                &format!("{title} (blocked)"),
                Undo::UserJs {
                    exe: (*exe).into(),
                    path: user_js.to_string_lossy().into_owned(),
                    old,
                },
            )
        }
        (Target::Open { exe, url }, "open") => std::process::Command::new(exe)
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open {exe}: {e}")),
        (Target::Task { name }, "disable") => {
            sys::run("schtasks.exe", &["/Change", "/TN", name, "/Disable"])
                .map_err(|e| task_error(&e))?;
            remember(
                module,
                &format!("{title} (disabled)"),
                Undo::Task { name: name.clone() },
            )
        }
        (Target::Task { name }, "enable") => {
            sys::run("schtasks.exe", &["/Change", "/TN", name, "/Enable"])
                .map(|_| ())
                .map_err(|e| task_error(&e))
        }
        _ => Err("Not offered for this item.".into()),
    }
}

fn task_error(error: &str) -> String {
    if error.to_lowercase().contains("denied") {
        "Needs administrator: run MYLE as administrator for this task.".into()
    } else {
        error.to_string()
    }
}

fn set_dwords(
    module: &str,
    title: &str,
    action: &str,
    hive: &str,
    path: &str,
    names: &[String],
    value: u32,
) -> Result<(), String> {
    let key = writable(hive, path)?;
    let old: Vec<Option<u32>> = names.iter().map(|n| sys::dword(&key, n)).collect();
    for name in names {
        key.set_value(name, &value).map_err(|e| denied(hive, e))?;
    }
    remember(
        module,
        &format!("{title} ({action})"),
        Undo::Dword {
            hive: hive.into(),
            path: path.into(),
            names: names.to_vec(),
            old,
        },
    )
}

/// `sc config <name> start= <kind>` through a UAC prompt.
async fn set_service_start(name: &str, kind: &str) -> Result<(), String> {
    let args = vec![
        "config".to_string(),
        name.to_string(),
        "start=".to_string(),
        kind.to_string(),
    ];
    let code = crate::apps::process::run_elevated("sc.exe", &args, true).await?;
    match code {
        0 => Ok(()),
        1223 => Err("The administrator prompt was declined.".into()),
        other => Err(format!("sc.exe ended with {other}.")),
    }
}

async fn undo(undo: &Undo) -> Result<(), String> {
    match undo {
        Undo::RegImport { file } => {
            // HKCU imports work as is; HKLM ones need the administrator prompt.
            if sys::run("reg.exe", &["import", file, "/reg:64"]).is_ok() {
                return Ok(());
            }
            let args = ["import".to_string(), file.clone(), "/reg:64".to_string()];
            match crate::apps::process::run_elevated("reg.exe", &args, true).await? {
                0 => Ok(()),
                code => Err(format!("reg import ended with {code}.")),
            }
        }
        Undo::Binary {
            hive,
            path,
            name,
            old,
        } => {
            let key = writable(hive, path)?;
            match old {
                Some(bytes) => key.set_raw_value(
                    name,
                    &RegValue {
                        bytes: bytes.clone().into(),
                        vtype: RegType::REG_BINARY,
                    },
                ),
                None => key.delete_value(name),
            }
            .map_err(|e| denied(hive, e))
        }
        Undo::Text {
            hive,
            path,
            name,
            old,
        } => {
            let key = writable(hive, path)?;
            match old {
                Some(text) => key.set_value(name, text),
                None => key.delete_value(name),
            }
            .map_err(|e| denied(hive, e))
        }
        Undo::Dword {
            hive,
            path,
            names,
            old,
        } => {
            let key = writable(hive, path)?;
            for (name, value) in names.iter().zip(old) {
                match value {
                    Some(v) => key.set_value(name, v),
                    None => key.delete_value(name).or(Ok(())),
                }
                .map_err(|e| denied(hive, e))?;
            }
            Ok(())
        }
        Undo::Service { name, start } => {
            let kind = match start {
                2 => "auto",
                3 => "demand",
                4 => "disabled",
                _ => "auto",
            };
            set_service_start(name, kind).await
        }
        Undo::ChromeSite {
            exe,
            prefs,
            key,
            old,
        } => {
            closed(exe)?;
            let text = std::fs::read_to_string(prefs).map_err(|e| e.to_string())?;
            let mut json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if let Some(site) = json
                .pointer_mut("/profile/content_settings/exceptions/notifications")
                .and_then(|n| n.get_mut(key))
            {
                site["setting"] = old.clone();
            }
            std::fs::write(prefs, serde_json::to_vec(&json).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
        Undo::UserJs { exe, path, old } => {
            closed(exe)?;
            match old {
                Some(text) => std::fs::write(path, text),
                None => std::fs::remove_file(path),
            }
            .map_err(|e| e.to_string())
        }
        Undo::Task { name } => sys::run("schtasks.exe", &["/Change", "/TN", name, "/Enable"])
            .map(|_| ())
            .map_err(|e| task_error(&e)),
        Undo::Recycled { .. } => Err(
            "These files are in the Recycle Bin: restore them from there (right-click, Restore)."
                .into(),
        ),
    }
}
