//! Settings only; no key or credential crosses the webview boundary.
use tauri::AppHandle;
use tauri_plugin_myle_mobile::{MobileExt, ProviderStatus};

#[tauri::command]
pub async fn mobile_provider_status(app: AppHandle) -> Result<ProviderStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.myle_mobile().provider_status()).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mobile_provider_settings(app: AppHandle, target: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.myle_mobile().provider_settings(&target)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mobile_provider_biometry(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.myle_mobile().provider_biometry(enabled)).await.map_err(|e| e.to_string())?
}
