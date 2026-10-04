//! Website icons for the vault's list (`myle_vault::icons`).

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, State};

use super::PasswordsState;
pub use myle_vault::icons::icon_host;
use myle_vault::icons::{ICON_EVENT, lookup};

/// The icons already known for the vault's websites, but those the page has
/// (`skip`). The missing and the old ones are fetched in the background and
/// sent as `passwords-icon`.
#[tauri::command]
pub async fn passwords_icons(
    app: AppHandle,
    state: State<'_, PasswordsState>,
    skip: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let (known, pending) = lookup(&state.cell, skip)?;
    if let Some(pending) = pending {
        let cell = state.cell.clone();
        tauri::async_runtime::spawn(async move {
            pending
                .fetch(&cell, |fetched| {
                    let _ = app.emit(ICON_EVENT, fetched);
                })
                .await;
        });
    }
    Ok(known)
}
