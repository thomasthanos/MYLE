//! Copying a password: the clipboard is emptied 30 seconds later, unless
//! something else was copied in the app since.
//!
//! The phone does not let an app in the background read the clipboard
//! (Android), or asks the user first (iOS), so the app cannot check what is
//! on it then: it empties it regardless, as password managers on phones do.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

const CLEAR_AFTER: Duration = Duration::from_secs(30);
/// Counts copies: only the last one clears the clipboard.
static COPIES: AtomicU64 = AtomicU64::new(0);

pub fn copy_secret(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text.to_string())
        .map_err(|_| "Could not copy to the clipboard.".to_string())?;
    let copy = COPIES.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLEAR_AFTER).await;
        if COPIES.load(Ordering::SeqCst) == copy {
            let _ = app.clipboard().write_text(String::new());
        }
    });
    Ok(())
}
