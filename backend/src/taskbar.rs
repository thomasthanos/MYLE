//! Taskbar pinning is requested by an explicit click in the running app.
use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::UI::Shell::{ITaskbarManagerDesktopAppSupportStatics, TaskbarManager};
use crate::shortcut_preferences::{TASKBAR_CHOICE, TASKBAR_PENDING, taskbar_preference};
const LAF_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModel\LimitedAccessFeatures\com.microsoft.windows.taskbar.pin";
const LAF_SEED: &str = "4096B239A7295B635C090E647E867B5707DA6AB6CB78340B01FE4E0C8F4953D4";
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinOffer {
    pending: bool,
    supported: bool,
    request_after_setup: bool,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PinResult {
    Pinned,
    NotAllowed { reason: PinBlockReason },
    NotPinned,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PinBlockReason {
    PinConsentNotificationsDisabled,
    Unknown,
}
enum PinRequest {
    NotAllowed(PinBlockReason),
    Pending(windows_future::IAsyncOperation<bool>),
}
fn pin_block_reason() -> PinBlockReason {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    // Windows uses its own notification sender for pin consent. Its setting can block the prompt even while general notifications are enabled. This is diagnostic only: IsPinningAllowed remains the authority, and an absent/new Windows setting never implies a particular reason.
    let enabled = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Windows.SystemToast.PinConsent")
        .and_then(|key| key.get_value::<u32, _>("Enabled"));
    if matches!(enabled, Ok(0)) {
        PinBlockReason::PinConsentNotificationsDisabled
    } else {
        PinBlockReason::Unknown
    }
}
fn preference() -> crate::shortcut_preferences::TaskbarPreference {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\ThomasThanos\MakeYourLifeEasier").ok();
    let read = |name: &str| key.as_ref().and_then(|key| key.get_value::<u32, _>(name).ok()).map(|value| value != 0);
    taskbar_preference(read(TASKBAR_CHOICE), read(TASKBAR_PENDING))
}
fn supported() -> bool {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};
    let restricted = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(LAF_KEY)
        .and_then(|key| key.get_value::<u32, _>(LAF_SEED))
        .is_ok_and(|value| value != 0);
    !restricted && windows::core::factory::<TaskbarManager, ITaskbarManagerDesktopAppSupportStatics>().is_ok()
}
#[tauri::command]
pub async fn taskbar_pin_offer() -> PinOffer {
    if crate::updater::installed_exe().is_none() || !preference().pending {
        return PinOffer { pending: false, supported: false, request_after_setup: false };
    }
    let supported = supported();
    if supported {
        let pinned = async {
            TaskbarManager::GetDefault()?.IsCurrentAppPinnedAsync()?.await
        }.await;
        if matches!(pinned, Ok(true)) {
            let _ = crate::startup::set_flag(TASKBAR_PENDING, false);
            return PinOffer { pending: false, supported, request_after_setup: false };
        }
    }
    PinOffer { pending: true, supported, request_after_setup: std::env::args().any(|arg| arg == "--pin-taskbar") }
}
#[tauri::command]
pub fn taskbar_pin_dismiss() -> Result<(), String> {
    crate::startup::set_flag(TASKBAR_PENDING, false)
}
#[tauri::command]
pub fn taskbar_open_notification_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-settings:notifications", None::<&str>)
        .map_err(|error| error.to_string())
}
#[tauri::command]
pub async fn taskbar_pin(app: AppHandle) -> Result<PinResult, String> {
    if !preference().enabled {
        return Err("Taskbar pinning is switched off in the setup choices.".into());
    }
    if crate::updater::installed_exe().is_none() || !supported() {
        return Err("Pin MYLE from the Windows Start menu on this version of Windows.".into());
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    app.clone().run_on_main_thread(move || {
        let request = (|| {
            let main = app.get_webview_window("main").ok_or("The MYLE window is not open.")?;
            if !main.is_focused().map_err(|error| error.to_string())? {
                return Err("Bring MYLE to the foreground and try again.".into());
            }
            let manager = TaskbarManager::GetDefault().map_err(|error| error.to_string())?;
            if !manager.IsPinningAllowed().map_err(|error| error.to_string())? {
                return Ok(PinRequest::NotAllowed(pin_block_reason()));
            }
            manager.RequestPinCurrentAppAsync()
                .map(PinRequest::Pending)
                .map_err(|error| error.to_string())
        })();
        let _ = send.send(request);
    }).map_err(|error| error.to_string())?;
    let request = receive.await.map_err(|error| error.to_string())??;
    let pinned = match request {
        PinRequest::NotAllowed(reason) => return Ok(PinResult::NotAllowed { reason }),
        PinRequest::Pending(operation) => operation.await.map_err(|error| error.to_string())?,
    };
    if pinned {
        crate::startup::set_flag(TASKBAR_PENDING, false)?;
        Ok(PinResult::Pinned)
    } else {
        Ok(PinResult::NotPinned)
    }
}
