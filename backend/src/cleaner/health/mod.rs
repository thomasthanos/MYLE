// ! System Cleaner's Software Health: eight scans of what installed ! software leaves, starts, may use and shows, and the reversible changes ! offered for each finding. ! ! The page sends finding ids only. What a finding points at (a folder, a ! registry key, a service) stays here, from the scan that found it, so a ! change can only ever touch what a scan listed. Every change is ! non-destructive: files go to the Recycle Bin, a registry key is exported ! to a .reg file before it is deleted, and every switch is written to the ! restore list with the value it had.
mod scans;
mod sys;
pub(crate) use scans::install_locations;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    pub cleanup_action: Option<&'static str>,
    pub recommended: bool,
    pub review_reason: Option<String>,
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
    /// A module started scanning.
    Started { module: &'static str },
    /// How far a module is, 0..1.
    Step { module: &'static str, fraction: f32 },
    /// A module finished: its findings, straight away.
    Result { report: ModuleReport },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub id: String,
    pub ok: bool,
    pub message: Option<String>,
}
static FOUND: LazyLock<Mutex<HashMap<String, (Finding, Target)>>> = LazyLock::new(Default::default);
static OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Target identity is independent of the scan, display name and switch state.
fn target_identity(target: &Target) -> String {
    let lower = |s: &str| s.replace('/', "\\").to_lowercase();
    match target {
        Target::Paths(paths) => {
            let mut paths: Vec<_> = paths.iter().map(|p| lower(&p.to_string_lossy())).collect();
            paths.sort();
            format!("paths:{paths:?}")
        }
        Target::UninstallKey { hive, path, .. } => {
            format!("uninstall:{}:{}", lower(hive), lower(path))
        }
        Target::Startup {
            hive,
            approved,
            name,
        } => format!(
            "startup:{}:{}:{}",
            lower(hive),
            lower(approved),
            lower(name)
        ),
        Target::Service { name, .. } => format!("service:{}", lower(name)),
        Target::Consent { path } => format!("consent:{}", lower(path)),
        Target::Background { path } => format!("background:{}", lower(path)),
        Target::Notify { path } => format!("notify:{}", lower(path)),
        Target::ChromeSite { prefs, key, .. } => {
            format!("chrome:{}:{key}", lower(&prefs.to_string_lossy()))
        }
        Target::FirefoxNotify { user_js, .. } => {
            format!("firefox:{}", lower(&user_js.to_string_lossy()))
        }
        Target::Open { exe, url } => format!("open:{}:{url}", lower(exe)),
        Target::Task { name } => format!("task:{}", lower(name)),
        Target::Info => "info".into(),
    }
}
fn prepare_finding(f: &mut Finding, target: &Target) {
    let identity = if matches!(target, Target::Info) {
        format!("info:{}:{:?}", f.title, f.path)
    } else {
        target_identity(target)
    };
    let digest: String = Sha256::digest(identity.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    f.id = format!("{}-{digest}", f.module);
    f.cleanup_action = match (f.module, target) {
        ("leftovers" | "redundant" | "installers", Target::Paths(_)) => Some("recycle"),
        ("leftovers" | "broken", Target::UninstallKey { .. }) => Some("deleteKey"),
        _ => None,
    };
    if f.cleanup_action.is_some() && !f.recommended && f.review_reason.is_none() {
        f.review_reason =
            Some("Review before cleaning; this may still belong to software you use.".into());
    }
}
/// One finding per id (the first): the page keys its list by id, and a
/// repeated one stops the list from showing at all.
fn keep_first_of_each_id(items: &mut Vec<(Finding, Target)>) {
    let mut ids = std::collections::HashSet::new();
    items.retain(|(finding, _)| ids.insert(finding.id.clone()));
}
fn target_claims(target: &Target) -> Vec<String> {
    match target {
        Target::Paths(paths) => paths
            .iter()
            .map(|p| {
                format!(
                    "path:{}",
                    p.to_string_lossy()
                        .replace('/', "\\")
                        .trim_end_matches('\\')
                        .to_lowercase()
                )
            })
            .collect(),
        _ => vec![target_identity(target)],
    }
}
#[cfg(test)]
fn claim_target(target: &Target, seen: &mut std::collections::HashSet<String>) -> bool {
    claim_keys(&target_claims(target), seen)
}
#[derive(Clone, Debug, Deserialize)]
pub struct BatchItem {
    pub id: String,
    pub action: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}
#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn a_repeated_finding_is_listed_once() {
        // Three processes of one program, as the hibernation scan once sent.
        let process = |detail: &str| Finding {
            id: String::new(),
            module: "hibernation",
            title: "claude".into(),
            detail: detail.into(),
            path: Some(r"C:\Program Files\Claude\claude.exe".into()),
            size: None,
            confidence: None,
            enabled: None,
            flags: vec![],
            actions: vec![],
            cleanup_action: None,
            recommended: false,
            review_reason: None,
        };
        let mut items: Vec<_> = ["first", "second", "third"].map(|detail| (process(detail), Target::Info)).into();
        items.push((Finding { title: "other".into(), ..process("other") }, Target::Info));
        for (finding, target) in &mut items {
            prepare_finding(finding, target);
        }
        keep_first_of_each_id(&mut items);
        let kept: Vec<_> = items.iter().map(|(f, _)| f.detail.as_str()).collect();
        assert_eq!(kept, ["first", "other"]);
    }
    #[test]
    fn target_identity_is_stable_and_windows_paths_ignore_case() {
        assert_eq!(
            target_identity(&Target::Paths(vec![PathBuf::from(r"C:\Cache\App")])),
            target_identity(&Target::Paths(vec![PathBuf::from(r"c:\cache\app")]))
        );
        assert_ne!(
            target_identity(&Target::Consent {
                path: "camera".into()
            }),
            target_identity(&Target::Consent {
                path: "microphone".into()
            })
        );
    }
    #[test]
    fn batch_rejects_duplicate_and_conflicting_targets_before_execution() {
        let target = Target::Paths(vec![PathBuf::from(r"C:\Cache")]);
        let mut seen = std::collections::HashSet::new();
        assert!(claim_target(&target, &mut seen));
        assert!(!claim_target(&target, &mut seen));
        let nested = Target::Paths(vec![PathBuf::from(r"C:\Cache\child")]);
        assert!(!claim_target(&nested, &mut seen));
    }
    #[test]
    fn overlap_does_not_discard_unprocessed_folders_or_registry_keys() {
        let covered = std::collections::HashSet::from(["path:c:\\cache".to_string()]);
        let target = Target::Paths(vec![
            PathBuf::from(r"C:\Cache\child"),
            PathBuf::from(r"C:\Other"),
        ]);
        let Some(Target::Paths(paths)) = uncovered_target(&target, "recycle", &covered) else {
            panic!("Other folder was lost");
        };
        assert_eq!(paths, vec![PathBuf::from(r"C:\Other")]);
        let target = Target::UninstallKey {
            hive: "HKCU",
            path: "Software\\Fixture".into(),
            dirs: vec![PathBuf::from(r"C:\Cache")],
        };
        let Some(Target::UninstallKey { dirs, .. }) =
            uncovered_target(&target, "forceClean", &covered)
        else {
            panic!("Registry key was lost");
        };
        assert!(dirs.is_empty());
        let child_covered = std::collections::HashSet::from(["path:c:\\cache\\child".to_string()]);
        assert!(
            uncovered_target(
                &Target::Paths(vec![PathBuf::from(r"C:\Cache")]),
                "recycle",
                &child_covered
            )
            .is_some()
        );
    }
    #[test]
    fn recommendation_never_uses_confidence_as_automatic_permission() {
        let mut f = Finding {
            id: String::new(),
            module: "leftovers",
            title: "old folder".into(),
            detail: String::new(),
            path: None,
            size: None,
            confidence: Some(100),
            enabled: None,
            flags: vec![],
            actions: vec!["recycle"],
            cleanup_action: None,
            recommended: false,
            review_reason: None,
        };
        prepare_finding(&mut f, &Target::Paths(vec![PathBuf::from(r"C:\old")]));
        assert!(!f.recommended);
        assert_eq!(f.cleanup_action, Some("recycle"));
        let original = f.id.clone();
        prepare_finding(&mut f, &Target::Paths(vec![PathBuf::from(r"C:\old")]));
        assert_eq!(original, f.id);
        f.module = "broken";
        f.actions = vec!["forceClean", "deleteKey"];
        prepare_finding(
            &mut f,
            &Target::UninstallKey {
                hive: "HKCU",
                path: "Software\\Example".into(),
                dirs: vec![PathBuf::from(r"C:\old")],
            },
        );
        assert_eq!(f.cleanup_action, Some("deleteKey"));
        assert!(!f.recommended);
    }
    #[tokio::test]
    async fn native_commands_refuse_concurrent_changes_and_stale_ids() {
        let guard = OPERATION.lock().await;
        assert!(
            apply_batch(vec![], None)
                .await
                .unwrap_err()
                .contains("busy")
        );
        assert!(
            cleaner_health_restore("no-such-fixture".into())
                .await
                .unwrap_err()
                .contains("busy")
        );
        drop(guard);
        let results = apply_batch(
            vec![BatchItem {
                id: "no-such-fixture".into(),
                action: "recycle".into(),
            }],
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(results[0].message.as_ref().unwrap().contains("Scan again"));
    }
    #[test]
    fn conflicting_actions_are_rejected_without_touching_targets() {
        let target = Target::UninstallKey {
            hive: "HKCU",
            path: "Software\\Fixture".into(),
            dirs: vec![],
        };
        let f = Finding {
            id: "fixture".into(),
            module: "broken",
            title: "fixture".into(),
            detail: String::new(),
            path: None,
            size: None,
            confidence: None,
            enabled: None,
            flags: vec![],
            actions: vec!["deleteKey", "forceClean"],
            cleanup_action: Some("deleteKey"),
            recommended: false,
            review_reason: None,
        };
        let requests = vec![
            (
                BatchItem {
                    id: f.id.clone(),
                    action: "deleteKey".into(),
                },
                Some((f.clone(), target.clone())),
            ),
            (
                BatchItem {
                    id: f.id.clone(),
                    action: "forceClean".into(),
                },
                Some((f.clone(), target)),
            ),
        ];
        assert!(
            validate_batch(&requests)
                .unwrap_err()
                .contains("different actions")
        );
    }
    #[tokio::test]
    async fn exported_registry_fixture_can_be_restored() {
        let path = format!("Software\\MYLE-Health-Test-{}", uuid::Uuid::new_v4());
        let root = sys::root("HKCU");
        let (key, _) = root.create_subkey(&path).unwrap();
        key.set_value("Fixture", &"original").unwrap();
        drop(key);
        let file = std::env::temp_dir().join(format!("myle-health-{}.reg", uuid::Uuid::new_v4()));
        sys::export_key("HKCU", &path, &file).unwrap();
        root.delete_subkey_all(&path).unwrap();
        undo(&Undo::RegImport {
            file: file.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
        let restored = root.open_subkey(&path).unwrap();
        assert_eq!(
            restored.get_value::<String, _>("Fixture").unwrap(),
            "original"
        );
        drop(restored);
        root.delete_subkey_all(&path).unwrap();
        std::fs::remove_file(file).unwrap();
    }
}
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
    crate::game_saves::atomic::write(&journal_path()?, &bytes)
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
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "Software Health is busy. Wait for the current operation.".to_string())?;
    let wanted: Vec<&'static str> = MODULES
        .iter()
        .copied()
        .filter(|m| modules.is_empty() || modules.iter().any(|w| w == m))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        // Every module at once: they read different places, and the slow ones (PowerShell, folder sizes) no longer hold the others back.
        let reports = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for module in wanted.iter().copied() {
                let on_event = &on_event;
                let reports = &reports;
                scope.spawn(move || {
                    let _ = on_event.send(HealthEvent::Started { module });
                    let tick = |fraction: f32| {
                        let _ = on_event.send(HealthEvent::Step { module, fraction });
                    };
                    let (mut items, error) = match scans::scan(module, &tick) {
                        Ok(items) => (items, None),
                        Err(error) => (Vec::new(), Some(error)),
                    };
                    for (finding, target) in &mut items {
                        prepare_finding(finding, target);
                    }
                    keep_first_of_each_id(&mut items);
                    let findings: Vec<Finding> = items.iter().map(|(f, _)| f.clone()).collect();
                    {
                        let mut store = FOUND.lock().unwrap_or_else(|p| p.into_inner());
                        store.retain(|_, (f, _)| f.module != module);
                        store.extend(items.into_iter().map(|(f, t)| (f.id.clone(), (f, t))));
                    }
                    let report = ModuleReport {
                        module,
                        findings,
                        error,
                    };
                    let _ = on_event.send(HealthEvent::Result {
                        report: report.clone(),
                    });
                    reports
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(report);
                });
            }
        });
        let mut list = reports.into_inner().unwrap_or_else(|p| p.into_inner());
        list.sort_by_key(|r| MODULES.iter().position(|m| *m == r.module));
        list
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
    apply_batch(
        ids.into_iter()
            .map(|id| BatchItem {
                id,
                action: action.clone(),
            })
            .collect(),
        None,
    )
    .await
}
#[tauri::command]
pub async fn cleaner_health_apply_batch(
    items: Vec<BatchItem>,
    on_event: Channel<BatchProgress>,
) -> Result<Vec<ApplyResult>, String> {
    apply_batch(items, Some(on_event)).await
}
async fn apply_batch(
    items: Vec<BatchItem>,
    on_event: Option<Channel<BatchProgress>>,
) -> Result<Vec<ApplyResult>, String> {
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "Software Health is busy. Wait for the current operation.".to_string())?;
    if items.len() > 10_000 {
        return Err("Too many selected items. Scan again.".into());
    }
    // Snapshot every target before any change. Inputs only contain scan IDs.
    let mut items: Vec<_> = {
        let store = FOUND.lock().unwrap_or_else(|p| p.into_inner());
        items
            .into_iter()
            .map(|item| {
                let found = store.get(&item.id).cloned();
                (item, found)
            })
            .collect()
    };
    validate_batch(&items)?;
    // Cover parents before their children so overlapping folder selections never skip an unprocessed part of a parent directory.
    items.sort_by_key(|(request, found)| {
        found.as_ref().map_or(usize::MAX, |(_, target)| {
            action_claims(target, &request.action)
                .iter()
                .filter(|claim| claim.starts_with("path:"))
                .map(|claim| claim.matches('\\').count())
                .min()
                .unwrap_or(usize::MAX)
        })
    });
    let total = items.len();
    let mut covered: HashMap<String, ApplyResult> = HashMap::new();
    let mut results = Vec::new();
    for (index, (request, item)) in items.into_iter().enumerate() {
        let BatchItem { id, action } = request;
        if let Some(channel) = &on_event {
            let _ = channel.send(BatchProgress {
                done: index,
                total,
                current: item
                    .as_ref()
                    .map_or_else(|| "Unavailable item".into(), |(f, _)| f.title.clone()),
            });
        }
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
        let claims = action_claims(&target, &action);
        let target = if action != "open" {
            let earlier = covered
                .iter()
                .filter(|(old, _)| claims.iter().any(|claim| overlaps(claim, old)))
                .map(|(_, result)| result)
                .find(|result| !result.ok);
            if let Some(earlier) = earlier {
                results.push(ApplyResult {
                    id,
                    ok: false,
                    message: earlier.message.clone(),
                });
                continue;
            }
            let seen = covered.keys().cloned().collect();
            let Some(remaining) = uncovered_target(&target, &action, &seen) else {
                results.push(ApplyResult {
                    id,
                    ok: true,
                    message: Some("Already covered by another selected item.".into()),
                });
                continue;
            };
            remaining
        } else {
            target
        };
        let claims = action_claims(&target, &action);
        let outcome = apply_one(&finding, &target, &action).await;
        let result = match outcome {
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
        };
        for claim in claims {
            covered.insert(claim, result.clone());
        }
        results.push(result);
    }
    if let Some(channel) = &on_event {
        let _ = channel.send(BatchProgress {
            done: total,
            total,
            current: String::new(),
        });
    }
    Ok(results)
}
type ScannedRequest = (BatchItem, Option<(Finding, Target)>);
fn validate_batch(items: &[ScannedRequest]) -> Result<(), String> {
    let mut actions = HashMap::new();
    for (request, found) in items {
        let Some((finding, target)) = found else {
            continue;
        };
        if !finding.actions.contains(&request.action.as_str()) {
            continue;
        }
        if let Some(previous) = actions.insert(target_identity(target), &request.action)
            && previous != &request.action
        {
            return Err(
                "The same item has different actions selected. Choose one action and review again."
                    .into(),
            );
        }
    }
    Ok(())
}
fn action_claims(target: &Target, action: &str) -> Vec<String> {
    let mut claims = target_claims(target);
    if action == "forceClean"
        && let Target::UninstallKey { dirs, .. } = target
    {
        claims.extend(target_claims(&Target::Paths(dirs.clone())));
    }
    claims
}
fn overlaps(a: &str, b: &str) -> bool {
    a == b
        || a.starts_with("path:")
            && b.starts_with("path:")
            && (a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\")))
}
fn uncovered_target(
    target: &Target,
    action: &str,
    seen: &std::collections::HashSet<String>,
) -> Option<Target> {
    let uncovered_paths = |paths: &[PathBuf]| {
        paths
            .iter()
            .filter(|path| {
                !target_claims(&Target::Paths(vec![(*path).clone()]))
                    .iter()
                    .any(|claim| {
                        seen.iter()
                            .any(|old| claim == old || claim.starts_with(&format!("{old}\\")))
                    })
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    match target {
        Target::Paths(paths) => {
            let paths = uncovered_paths(paths);
            (!paths.is_empty()).then_some(Target::Paths(paths))
        }
        Target::UninstallKey { hive, path, dirs } if action == "forceClean" => {
            if seen.contains(&target_identity(target)) {
                return None;
            }
            Some(Target::UninstallKey {
                hive,
                path: path.clone(),
                dirs: uncovered_paths(dirs),
            })
        }
        _ => (!seen.contains(&target_identity(target))).then(|| target.clone()),
    }
}
#[cfg(test)]
fn claim_keys(claims: &[String], seen: &mut std::collections::HashSet<String>) -> bool {
    if claims
        .iter()
        .any(|claim| seen.iter().any(|old| overlaps(claim, old)))
    {
        return false;
    }
    seen.extend(claims.iter().cloned());
    true
}
#[tauri::command]
pub fn cleaner_health_restore_list() -> Vec<RestoreEntry> {
    read_journal()
}
/// Puts back what a restore entry changed, and drops it from the list.
#[tauri::command]
pub async fn cleaner_health_restore(id: String) -> Result<(), String> {
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "Software Health is busy. Wait for the current operation.".to_string())?;
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
/// Opens a file or folder in Windows File Explorer.
#[tauri::command(async)]
pub fn cleaner_health_reveal(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("The path does not exist on disk: {path}"));
    }
    if target.is_file() {
        if tauri_plugin_opener::reveal_item_in_dir(&target).is_ok() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(crate::apps::process::explorer())
                .raw_arg(format!("/select,\"{path}\""))
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Explorer did not start: {e}"))
        }
        #[cfg(not(windows))]
        {
            tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
        }
    } else {
        if tauri_plugin_opener::open_path(&path, None::<&str>).is_ok() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new(crate::apps::process::explorer())
                .raw_arg(format!("\"{path}\""))
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Explorer did not start: {e}"))
        }
        #[cfg(not(windows))]
        {
            tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
        }
    }
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
            remember(
                module,
                title,
                Undo::Recycled {
                    paths: present
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect(),
                },
            )?;
            sys::recycle(&present)
        }
        (Target::UninstallKey { hive, path, dirs }, "deleteKey" | "forceClean") => {
            let file = health_dir()?.join("backups").join(format!(
                "{}-{}.reg",
                sys::norm(title),
                now_ms()
            ));
            sys::export_key(hive, path, &file)?;
            // Persist the backup before any removal, including a partially completed force-clean. A failed journal write leaves data intact.
            remember(
                module,
                title,
                Undo::RegImport {
                    file: file.to_string_lossy().into_owned(),
                },
            )?;
            if action == "forceClean" {
                let present: Vec<PathBuf> = dirs.iter().filter(|d| d.is_dir()).cloned().collect();
                if !present.is_empty() {
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
                    sys::recycle(&present)?;
                }
            }
            sys::root(hive)
                .delete_subkey_all(path)
                .map_err(|e| denied(hive, e))
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
            remember(
                module,
                &format!("{title} ({action}d)"),
                Undo::Binary {
                    hive: (*hive).into(),
                    path: approved.clone(),
                    name: name.clone(),
                    old,
                },
            )?;
            key.set_raw_value(
                name,
                &RegValue {
                    bytes: bytes.into(),
                    vtype: RegType::REG_BINARY,
                },
            )
            .map_err(|e| denied(hive, e))
        }
        (Target::Service { name, start }, "manual") => {
            remember(
                module,
                &format!("{title}: Manual"),
                Undo::Service {
                    name: name.clone(),
                    start: *start,
                },
            )?;
            set_service_start(name, "demand").await
        }
        (Target::Consent { path }, "revoke" | "allow") => {
            let key = writable("HKCU", path)?;
            let old = sys::string(&key, "Value");
            remember(
                module,
                &format!("{title} ({action})"),
                Undo::Text {
                    hive: "HKCU".into(),
                    path: path.clone(),
                    name: "Value".into(),
                    old,
                },
            )?;
            key.set_value("Value", &if action == "revoke" { "Deny" } else { "Allow" })
                .map_err(|e| e.to_string())
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
            remember(
                module,
                &format!("{title} (blocked)"),
                Undo::ChromeSite {
                    exe: (*exe).into(),
                    prefs: prefs.to_string_lossy().into_owned(),
                    key: key.clone(),
                    old,
                },
            )?;
            crate::game_saves::atomic::write(
                prefs,
                &serde_json::to_vec(&json).map_err(|e| e.to_string())?,
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
            remember(
                module,
                &format!("{title} (blocked)"),
                Undo::UserJs {
                    exe: (*exe).into(),
                    path: user_js.to_string_lossy().into_owned(),
                    old,
                },
            )?;
            crate::game_saves::atomic::write(user_js, text.as_bytes())
        }
        (Target::Open { exe, url }, "open") => std::process::Command::new(exe)
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open {exe}: {e}")),
        (Target::Task { name }, "disable") => {
            remember(
                module,
                &format!("{title} (disabled)"),
                Undo::Task { name: name.clone() },
            )?;
            sys::run("schtasks.exe", &["/Change", "/TN", name, "/Disable"])
                .map(|_| ())
                .map_err(|e| task_error(&e))
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
    remember(
        module,
        &format!("{title} ({action})"),
        Undo::Dword {
            hive: hive.into(),
            path: path.into(),
            names: names.to_vec(),
            old,
        },
    )?;
    for name in names {
        key.set_value(name, &value).map_err(|e| denied(hive, e))?;
    }
    Ok(())
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
