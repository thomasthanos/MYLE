// ! Import/export of the selected-apps list through native file dialogs. ! The dialogs run on the Rust side, so the webview needs no file access.
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use crate::download::err;
const MAX_IMPORT_BYTES: u64 = 1024 * 1024;
/// Asks where to save and writes `json` there. Returns false if cancelled.
#[tauri::command]
pub async fn apps_export_list(app: AppHandle, json: String) -> Result<bool, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Export app list")
            .set_file_name("app-list.json")
            .add_filter("App list", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(err)?;
    let Some(path) = picked else { return Ok(false) };
    tokio::fs::write(path.into_path().map_err(err)?, json)
        .await
        .map_err(err)?;
    Ok(true)
}
/// Asks for a file and returns its text (the page validates it), or None if
/// cancelled.
#[tauri::command]
pub async fn apps_import_list(app: AppHandle) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Import app list")
            .add_filter("App list", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(err)?;
    let Some(path) = picked else { return Ok(None) };
    let path = path.into_path().map_err(err)?;
    if tokio::fs::metadata(&path).await.map_err(err)?.len() > MAX_IMPORT_BYTES {
        return Err("That file is too large to be an app list.".into());
    }
    tokio::fs::read_to_string(path).await.map(Some).map_err(err)
}
