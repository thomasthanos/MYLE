// ! The Windows Optimization page's Tools: Auto-Logon and the restart to the ! BIOS/UEFI. (Debloating is `crate::debloat`.)
mod autologon;
mod elevated;
mod firmware_restart;
mod models;
mod state;
use tauri::{AppHandle, State};
use uuid::Uuid;
use crate::apps::Jobs;
use crate::download;
pub use models::{AutoLogonOutcome, FirmwareRestartOutcome, WindowsOptimizationSnapshot};
pub use state::WindowsOptimizationState;
/// What the removed WinUtil/Sparkle launchers downloaded, deleted once.
fn remove_old_tool_cache() {
    if let Ok(dir) = crate::storage::local_dir().map(|dir| dir.join("windows-optimization")) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
#[tauri::command]
pub async fn windows_optimization_get_state(
    app: AppHandle,
    state: State<'_, WindowsOptimizationState>,
) -> Result<WindowsOptimizationSnapshot, String> {
    let operation = state.active_auto_logon();
    let auto_logon = tauri::async_runtime::spawn_blocking(move || {
        remove_old_tool_cache();
        autologon::snapshot(&app, operation)
    })
    .await
    .map_err(download::err)?;
    Ok(WindowsOptimizationSnapshot {
        auto_logon,
        firmware_restart: firmware_restart::snapshot(state.firmware_restart_active()),
    })
}
#[tauri::command]
pub async fn windows_optimization_restart_to_firmware(
    app: AppHandle,
    state: State<'_, WindowsOptimizationState>,
    jobs: State<'_, Jobs>,
    maintenance: State<'_, crate::maintenance::Running>,
    game_saves: State<'_, crate::game_saves::GameSavesState>,
) -> Result<FirmwareRestartOutcome, String> {
    let job_id = format!("windows-firmware-restart-{}", Uuid::new_v4().simple());
    // Keep this handle alive through capability detection, UAC and the shutdown.exe result so Install Apps, Spotify Hub and other exclusive maintenance actions cannot begin concurrently.
    let _exclusive = jobs.start_exclusive(&job_id)?;
    // Reserve the independent maintenance and Game Saves lanes for the full UAC/process window. The Game Saves guard holds its cross-process file lock, so a scheduled headless backup cannot race this restart.
    let _maintenance_reservation = maintenance.reserve_for_firmware_restart()?;
    let _game_saves_reservation = crate::game_saves::reserve_operations(&app, &game_saves)?;
    ensure_updater_idle(crate::updater::is_updating())?;
    // The guard clears the Processing state even if spawning, UAC or the native process fails and this function returns early through `?`.
    let _active = state.begin_firmware_restart()?;
    firmware_restart::restart().await
}
fn ensure_updater_idle(updater_active: bool) -> Result<(), String> {
    if updater_active {
        return Err("Wait for the application update to finish first.".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn windows_optimization_set_auto_logon(
    app: AppHandle,
    state: State<'_, WindowsOptimizationState>,
    jobs: State<'_, Jobs>,
    enabled: bool,
) -> Result<AutoLogonOutcome, String> {
    let job_id = format!("windows-auto-logon-{}", Uuid::new_v4().simple());
    let _exclusive = jobs.start_exclusive(&job_id)?;
    let operation = if enabled {
        models::AutoLogonOperation::Enable
    } else {
        models::AutoLogonOperation::Disable
    };
    let _running = state.begin_auto_logon(operation)?;
    autologon::set_auto_logon(&app, enabled).await
}
pub(crate) fn run_auto_logon_helper_from_args() -> Option<i32> {
    autologon::run_helper_from_args()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn firmware_restart_preflight_rejects_an_active_updater() {
        assert!(ensure_updater_idle(false).is_ok());
        assert!(ensure_updater_idle(true).is_err());
    }
    #[test]
    fn firmware_restart_ipc_contract_is_camel_case() {
        let snapshot = models::FirmwareRestartSnapshot {
            firmware_type: models::FirmwareType::LegacyBios,
            available: false,
            blocked_reason: Some("Legacy mode".into()),
            active: true,
        };
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "firmwareType": "legacyBios",
                "available": false,
                "blockedReason": "Legacy mode",
                "active": true
            })
        );
        assert_eq!(
            serde_json::to_value(models::FirmwareRestartOutcome {
                result: models::FirmwareRestartResult::NeedsAdmin,
                note: None,
            })
            .unwrap(),
            serde_json::json!({ "result": "needsAdmin", "note": null })
        );
    }
}
