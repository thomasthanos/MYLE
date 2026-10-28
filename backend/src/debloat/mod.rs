// ! The debloater: one-click Debloat, tweaks one by one, and Store app ! removal, with a real Undo. ! ! - `catalog.rs`: every tweak and app, fixed at compile time. The page ! sends only ids. ! - `detect.rs`: where each tweak stands, read without administrator ! rights. ! - `undo.rs`: what each change replaced, so Undo puts it back exactly. ! - The user's own settings (HKCU, the time format) are changed here, as ! the user; everything machine-wide goes through the administrator helper ! (`helper.rs`), started once per session with one UAC prompt.
mod catalog;
mod detect;
mod helper;
#[cfg(test)]
mod sandbox_tests;
mod start_menu;
pub(crate) mod system;
mod undo;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;
use crate::apps::Jobs;
use crate::download;
use crate::elevated_pipe::{self, Session};
use catalog::{Hive, Op, Tweak};
use helper::{HELPER, Reply, Request, RestorePoint};
use undo::{Before, Saved, Store};
pub use helper::run_helper_from_args as run_elevated_helper_from_args;
/// The running administrator helper: one UAC prompt, then every change of
/// this app session goes through it.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);
async fn ask(request: &Request) -> Result<Reply, String> {
    let mut session = SESSION.lock().await;
    if let Some(open) = session.as_mut() {
        match open.ask::<_, Reply>(request).await {
            Ok(reply) => return Ok(reply),
            // The helper has gone (idle for half an hour): start a new one.
            Err(_) => *session = None,
        }
    }
    let mut fresh = Session::start(&HELPER).await?;
    let reply = fresh
        .ask(request)
        .await
        .map_err(|error| format!("The administrator debloater stopped: {error}"))?;
    *session = Some(fresh);
    Ok(reply)
}
// --------------------------------------------------------------------------- Status
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebloatStatus {
    windows: system::WindowsInfo,
    tweaks: Vec<detect::TweakStatus>,
    apps: Vec<detect::AppStatus>,
    start_menu: start_menu::StartMenuStatus,
    /// The administrator helper is running: no prompt before the next change.
    admin_ready: bool,
}
#[tauri::command]
pub async fn debloat_status() -> Result<DebloatStatus, String> {
    // A change in progress holds the session: it is running, then.
    let admin_ready = SESSION.try_lock().map(|session| session.is_some()).unwrap_or(true);
    tauri::async_runtime::spawn_blocking(move || {
        let windows = system::windows_info();
        let packages = system::installed_packages();
        let store = Store::load();
        DebloatStatus {
            tweaks: detect::tweaks(windows.build, &packages, &store),
            apps: detect::apps(&packages, &store),
            start_menu: start_menu::status(),
            windows,
            admin_ready,
        }
    })
    .await
    .map_err(download::err)
}
// --------------------------------------------------------------------------- Restore point
#[tauri::command]
pub async fn debloat_restore_point(turn_on: bool) -> Result<RestorePoint, String> {
    let reply = ask(&Request::RestorePoint { turn_on }).await?;
    reply
        .restore_point
        .ok_or_else(|| reply.errors.join(" "))
}
// --------------------------------------------------------------------------- Running
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    Running,
    Done,
    /// Nothing to change: it was already so.
    Unchanged,
    Failed,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DebloatEvent {
    Step {
        id: String,
        label: String,
        state: StepState,
        detail: Option<String>,
    },
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebloatOutcome {
    /// Steps that changed something.
    changed: usize,
    failed: Vec<Failure>,
    /// Some changes are complete only after a restart.
    reboot: bool,
    /// Administrator approval was declined: nothing machine-wide was done.
    needs_admin: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    label: String,
    message: String,
}
struct Progress<'a> {
    channel: &'a Channel<DebloatEvent>,
}
impl Progress<'_> {
    fn step(&self, id: &str, label: &str, state: StepState, detail: Option<String>) {
        let _ = self.channel.send(DebloatEvent::Step {
            id: id.to_string(),
            label: label.to_string(),
            state,
            detail,
        });
    }
}
/// The user's own part of a tweak: HKCU values and the time format.
fn apply_user(tweak: &'static Tweak) -> (Vec<Saved>, Vec<String>) {
    let mut saved = Vec::new();
    let mut errors = Vec::new();
    for (index, op) in tweak.ops.iter().enumerate() {
        match *op {
            Op::Reg { hive: Hive::User, path, name, data, best_effort } => {
                let before = system::read(Hive::User, path, name);
                if before.matches(data) {
                    continue;
                }
                let created = system::first_missing(Hive::User, path);
                match system::write(Hive::User, path, name, data) {
                    Ok(()) => saved.push(Saved { op: index, before: Before::Reg { value: before, created } }),
                    Err(_) if best_effort => {}
                    Err(e) => errors.push(e),
                }
            }
            Op::Clock24 => match system::time_formats() {
                Ok((short, long)) if !system::is_24h(&short) || !system::is_24h(&long) => {
                    match system::set_time_formats(&system::to_24h(&short), &system::to_24h(&long)) {
                        Ok(()) => saved.push(Saved { op: index, before: Before::Clock { short, long } }),
                        Err(e) => errors.push(format!("The clock could not be changed: {e}")),
                    }
                }
                Ok(_) => {}
                Err(e) => errors.push(e),
            },
            _ => {}
        }
    }
    (saved, errors)
}
/// What turning a tweak off puts back: exactly what MYLE kept when it
/// applied it, and nothing else (a part that was on before MYLE ran stays
/// on). Only when MYLE kept nothing, the tweak being on before MYLE ever
/// ran, the Windows default for each part that is on.
fn undo_records(tweak: &Tweak, saved: Vec<Saved>, packages: &[String]) -> Vec<Saved> {
    if !saved.is_empty() {
        return saved;
    }
    tweak
        .ops
        .iter()
        .enumerate()
        .filter(|(_, op)| detect::op_applied(op, packages) == Some(true))
        .filter_map(|(index, op)| detect::default_before(op).map(|before| Saved { op: index, before }))
        .collect()
}
/// The user's own part of Undo; returns the operations put back.
fn undo_user(tweak: &'static Tweak, saved: &[Saved]) -> (Vec<usize>, Vec<String>) {
    let mut undone = Vec::new();
    let mut errors = Vec::new();
    for record in saved {
        let result = match (tweak.ops.get(record.op), &record.before) {
            (Some(Op::Reg { hive: Hive::User, path, name, .. }), Before::Reg { value, created }) => {
                // Only keys on the way to the value, never above it.
                let created = created.as_deref().filter(|top| system::key_on_path(path, top));
                system::restore(Hive::User, path, name, value, created)
            }
            (Some(Op::Clock24), Before::Clock { short, long }) => system::set_time_formats(short, long),
            _ => continue,
        };
        match result {
            Ok(()) => undone.push(record.op),
            Err(e) => errors.push(e),
        }
    }
    (undone, errors)
}
/// After Edge is gone: the user's own shortcuts and its autostart entries.
fn edge_user_cleanup() {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    for run in [
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    ] {
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(run, KEY_ALL_ACCESS) {
            let names: Vec<String> = key
                .enum_values()
                .flatten()
                .map(|(name, _)| name)
                .filter(|name| name.starts_with("MicrosoftEdgeAutoLaunch_"))
                .collect();
            for name in names {
                let _ = key.delete_value(name);
            }
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA").map(std::path::PathBuf::from) {
        for shortcut in [
            appdata.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\Microsoft Edge.lnk"),
            appdata.join(r"Microsoft\Internet Explorer\Quick Launch\Microsoft Edge.lnk"),
            appdata.join(r"Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk"),
        ] {
            let _ = std::fs::remove_file(shortcut);
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from) {
        let _ = std::fs::remove_file(profile.join(r"Desktop\Microsoft Edge.lnk"));
    }
}
/// The tweaks asked for, in the catalog's order, that apply here.
fn chosen(ids: &[String], build: u32) -> Vec<&'static Tweak> {
    catalog::TWEAKS
        .iter()
        .filter(|tweak| ids.iter().any(|id| id == tweak.id) && tweak.builds.contains(build))
        .collect()
}
#[tauri::command]
pub async fn debloat_run(
    jobs: State<'_, Jobs>,
    tweaks: Vec<String>,
    apps: Vec<String>,
    on_event: Channel<DebloatEvent>,
) -> Result<DebloatOutcome, String> {
    let _exclusive = jobs.start_exclusive(&format!("debloat-{}", Uuid::new_v4().simple()))?;
    let progress = Progress { channel: &on_event };
    let (build, packages) = tauri::async_runtime::spawn_blocking(|| (system::windows_info().build, system::installed_packages()))
        .await
        .map_err(download::err)?;
    let tweaks = chosen(&tweaks, build);
    let removals: Vec<(&'static catalog::App, Vec<String>)> = catalog::APPS
        .iter()
        .filter(|app| apps.iter().any(|id| id == app.id))
        .map(|app| {
            let installed = packages
                .iter()
                .filter(|package| catalog::removable(package).is_some_and(|found| found.id == app.id))
                .cloned()
                .collect::<Vec<_>>();
            (app, installed)
        })
        .filter(|(_, installed)| !installed.is_empty())
        .collect();
    let mut store = Store::load();
    let mut outcome = DebloatOutcome::default();
    let mut restart_explorer = false;
    let mut clock = false;
    for tweak in tweaks {
        progress.step(tweak.id, tweak.title, StepState::Running, None);
        let (saved, mut errors) = tauri::async_runtime::spawn_blocking(move || apply_user(tweak))
            .await
            .map_err(download::err)?;
        let mut changed = !saved.is_empty();
        clock |= saved.iter().any(|record| matches!(record.before, Before::Clock { .. }));
        for record in saved {
            store.record(tweak.id, record);
        }
        if detect::needs_admin(tweak) {
            match ask(&Request::Apply { tweak: tweak.id.to_string() }).await {
                Ok(reply) => {
                    changed |= !reply.saved.is_empty() || !reply.removed.is_empty();
                    if reply.saved.iter().any(|record| record.before == Before::Edge) {
                        edge_user_cleanup();
                    }
                    for record in reply.saved {
                        store.record(tweak.id, record);
                    }
                    errors.extend(reply.errors);
                }
                Err(e) if e == elevated_pipe::DECLINED => {
                    outcome.needs_admin = true;
                    progress.step(tweak.id, tweak.title, StepState::Failed, Some(e));
                    let _ = store.save();
                    return Ok(outcome);
                }
                Err(e) => errors.push(e),
            }
        }
        store.save()?;
        if changed {
            outcome.changed += 1;
            restart_explorer |= tweak.restart_explorer;
            outcome.reboot |= tweak.reboot;
        }
        let state = match (errors.is_empty(), changed) {
            (false, _) => StepState::Failed,
            (true, true) => StepState::Done,
            (true, false) => StepState::Unchanged,
        };
        if !errors.is_empty() {
            outcome.failed.push(Failure { label: tweak.title.into(), message: errors.join(" ") });
        }
        progress.step(tweak.id, tweak.title, state, (!errors.is_empty()).then(|| errors.join(" ")));
    }
    for (app, packages) in removals {
        let id = format!("app:{}", app.id);
        let label = format!("Remove {}", app.title);
        progress.step(&id, &label, StepState::Running, None);
        match ask(&Request::RemoveApps { packages }).await {
            Ok(reply) => {
                if !reply.removed.is_empty() {
                    store.removed_apps.insert(app.id.to_string());
                    outcome.changed += 1;
                }
                let failed = !reply.errors.is_empty();
                if failed {
                    outcome.failed.push(Failure { label: label.clone(), message: reply.errors.join(" ") });
                }
                progress.step(&id, &label, if failed { StepState::Failed } else { StepState::Done }, failed.then(|| reply.errors.join(" ")));
            }
            Err(e) if e == elevated_pipe::DECLINED => {
                outcome.needs_admin = true;
                progress.step(&id, &label, StepState::Failed, Some(e));
                break;
            }
            Err(e) => {
                outcome.failed.push(Failure { label: label.clone(), message: e.clone() });
                progress.step(&id, &label, StepState::Failed, Some(e));
            }
        }
        store.save()?;
    }
    finish(&progress, clock, restart_explorer).await;
    Ok(outcome)
}
/// Tells Windows what changed, and restarts Explorer when a tweak needs it.
async fn finish(progress: &Progress<'_>, clock: bool, restart_explorer: bool) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if clock {
            system::broadcast_setting_change("intl");
        }
        system::broadcast_setting_change("Policy");
        system::broadcast_setting_change("TraySettings");
        system::broadcast_setting_change("ImmersiveColorSet");
    })
    .await;
    if restart_explorer {
        progress.step("explorer", "Restart Explorer", StepState::Running, None);
        let result = tauri::async_runtime::spawn_blocking(system::restart_explorer)
            .await
            .map_err(download::err)
            .and_then(|result| result);
        match result {
            Ok(()) => progress.step("explorer", "Restart Explorer", StepState::Done, None),
            Err(e) => progress.step("explorer", "Restart Explorer", StepState::Failed, Some(e)),
        }
    }
}
// --------------------------------------------------------------------------- Undo
async fn reinstall_edge() -> Result<(), String> {
    let status = crate::apps::process::hidden("winget")
        .args([
            "install", "--id", "Microsoft.Edge", "--exact", "--silent",
            "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| format!("winget could not start: {e}"))?;
    if system::edge_installed() {
        Ok(())
    } else {
        Err(format!("Edge could not be installed again (winget {}). Get it from microsoft.com/edge.", status.code().unwrap_or(-1)))
    }
}
#[tauri::command]
pub async fn debloat_undo(
    jobs: State<'_, Jobs>,
    tweaks: Vec<String>,
    on_event: Channel<DebloatEvent>,
) -> Result<DebloatOutcome, String> {
    let _exclusive = jobs.start_exclusive(&format!("debloat-undo-{}", Uuid::new_v4().simple()))?;
    let progress = Progress { channel: &on_event };
    let (build, packages) = tauri::async_runtime::spawn_blocking(|| (system::windows_info().build, system::installed_packages()))
        .await
        .map_err(download::err)?;
    let mut store = Store::load();
    let mut outcome = DebloatOutcome::default();
    let mut restart_explorer = false;
    let mut clock = false;
    let chosen = chosen(&tweaks, build);
    for tweak in chosen {
        let label = format!("Undo: {}", tweak.title);
        progress.step(tweak.id, &label, StepState::Running, None);
        let saved_from_store = store.saved(tweak.id);
        let packages_for_tweak = packages.clone();
        let (saved, mut undone, mut errors) = tauri::async_runtime::spawn_blocking(move || {
            let saved = undo_records(tweak, saved_from_store, &packages_for_tweak);
            let (undone, errors) = undo_user(tweak, &saved);
            (saved, undone, errors)
        })
        .await
        .map_err(download::err)?;
        clock |= saved.iter().any(|record| matches!(record.before, Before::Clock { .. }) && undone.contains(&record.op));
        let for_helper: Vec<Saved> = saved
            .iter()
            .filter(|record| !undone.contains(&record.op) && !matches!(record.before, Before::Edge | Before::Clock { .. }))
            .cloned()
            .collect();
        if !for_helper.is_empty() {
            match ask(&Request::Undo { tweak: tweak.id.to_string(), saved: for_helper }).await {
                Ok(reply) => {
                    undone.extend(reply.undone);
                    errors.extend(reply.errors);
                }
                Err(e) if e == elevated_pipe::DECLINED => {
                    outcome.needs_admin = true;
                    progress.step(tweak.id, &label, StepState::Failed, Some(e));
                    break;
                }
                Err(e) => errors.push(e),
            }
        }
        if let Some(edge) = saved.iter().find(|record| record.before == Before::Edge) {
            match reinstall_edge().await {
                Ok(()) => undone.push(edge.op),
                Err(e) => errors.push(e),
            }
        }
        store.forget(tweak.id, &undone);
        store.save()?;
        if !undone.is_empty() {
            outcome.changed += 1;
            restart_explorer |= tweak.restart_explorer;
            outcome.reboot |= tweak.reboot;
        }
        if !errors.is_empty() {
            outcome.failed.push(Failure { label: label.clone(), message: errors.join(" ") });
        }
        let state = if errors.is_empty() { StepState::Done } else { StepState::Failed };
        progress.step(tweak.id, &label, state, (!errors.is_empty()).then(|| errors.join(" ")));
    }
    finish(&progress, clock, restart_explorer).await;
    Ok(outcome)
}
// --------------------------------------------------------------------------- Apps back from the Store
/// Opens the app's page in the Microsoft Store, to install it again.
#[tauri::command]
pub fn debloat_open_store(app: String) -> Result<(), String> {
    let app = catalog::find_app(&app).ok_or("Unknown app.")?;
    let url = match app.store_id {
        Some(id) => format!("ms-windows-store://pdp/?ProductId={id}"),
        None => format!(
            "ms-windows-store://search/?query={}",
            app.title.replace('&', "%26").replace(' ', "%20").replace('(', "%28").replace(')', "%29").replace('/', "%2F")
        ),
    };
    open(&url)
}
fn open(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    };
    if result as isize > 32 { Ok(()) } else { Err("The Microsoft Store could not be opened.".into()) }
}
// --------------------------------------------------------------------------- Start Menu customization
#[tauri::command]
pub async fn debloat_start_menu_set(update: start_menu::StartMenuUpdate) -> Result<start_menu::StartMenuStatus, String> {
    start_menu::ensure_supported()?;
    tauri::async_runtime::spawn_blocking(move || start_menu::apply_user_update(update))
        .await
        .map_err(download::err)?
}
#[tauri::command]
pub async fn debloat_start_menu_hide_recommended(hide: bool) -> Result<start_menu::StartMenuStatus, String> {
    start_menu::ensure_supported()?;
    tauri::async_runtime::spawn_blocking(move || start_menu::apply_user_hide_recommended(hide))
        .await
        .map_err(download::err)??;
    let reply = ask(&Request::SetHideRecommended { hide }).await?;
    if !reply.errors.is_empty() {
        return Err(reply.errors.join(" "));
    }
    tauri::async_runtime::spawn_blocking(|| {
        system::broadcast_setting_change("Policy");
        start_menu::restart_start_menu();
        start_menu::status()
    })
    .await
    .map_err(download::err)
}
#[tauri::command]
pub async fn debloat_start_menu_apply_pins(pins: Vec<String>) -> Result<start_menu::StartMenuStatus, String> {
    start_menu::ensure_supported()?;
    let json = start_menu::build_pins_json(&pins)?;
    tauri::async_runtime::spawn_blocking(start_menu::backup_start_bin)
        .await
        .map_err(download::err)?;
    let reply = ask(&Request::SetStartPins { json: Some(json) }).await?;
    if !reply.errors.is_empty() {
        return Err(reply.errors.join(" "));
    }
    tauri::async_runtime::spawn_blocking(|| {
        system::broadcast_setting_change("Policy");
        start_menu::restart_start_menu();
    })
    .await
    .map_err(download::err)?;
    tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
    let _ = ask(&Request::SetStartPins { json: None }).await;
    tauri::async_runtime::spawn_blocking(start_menu::status)
        .await
        .map_err(download::err)
}
#[tauri::command]
pub async fn debloat_start_menu_restore_pins() -> Result<start_menu::StartMenuStatus, String> {
    if SESSION.try_lock().map(|s| s.is_some()).unwrap_or(false) {
        let _ = ask(&Request::SetStartPins { json: None }).await;
    }
    tauri::async_runtime::spawn_blocking(|| {
        start_menu::restore_start_bin()?;
        Ok(start_menu::status())
    })
    .await
    .map_err(download::err)?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_puts_back_only_what_myle_kept() {
        // Whatever else of the tweak is on stays on: it was so before MYLE.
        let tweak = catalog::find("location").unwrap();
        let kept = vec![Saved { op: 0, before: Before::Reg { value: system::Value::Absent, created: None } }];
        assert_eq!(undo_records(tweak, kept.clone(), &[]), kept);
    }
    #[test]
    fn events_and_outcomes_keep_their_shape() {
        let event = DebloatEvent::Step { id: "telemetry".into(), label: "Turn off telemetry".into(), state: StepState::Done, detail: None };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"event":"step","data":{"id":"telemetry","label":"Turn off telemetry","state":"done","detail":null}}"#
        );
        let outcome = DebloatOutcome { changed: 2, failed: Vec::new(), reboot: true, needs_admin: false };
        assert_eq!(serde_json::to_string(&outcome).unwrap(), r#"{"changed":2,"failed":[],"reboot":true,"needsAdmin":false}"#);
    }
    #[test]
    fn only_catalog_tweaks_for_this_windows_are_chosen() {
        let picked = chosen(&["bing".into(), "nope".into(), "classic-context-menu".into(), "telemetry".into()], 26100);
        let ids: Vec<&str> = picked.iter().map(|tweak| tweak.id).collect();
        assert_eq!(ids, ["telemetry", "bing", "classic-context-menu"], "the catalog's order");
        let on_windows_10 = chosen(&["classic-context-menu".into(), "end-task".into()], 19045);
        assert!(on_windows_10.is_empty());
    }
}
