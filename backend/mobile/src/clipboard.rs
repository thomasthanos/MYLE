//! Copying a password: the clipboard is emptied 30 seconds later, unless
//! something else was copied in the app since.
//!
//! The phone does not let an app in the background read the clipboard
//! (Android), or asks the user first (iOS), so the app cannot check what is
//! on it then: it empties it regardless, as password managers on phones do.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_myle_mobile::MobileExt;

const CLEAR_AFTER: Duration = Duration::from_secs(30);
/// Counts copies: only the last one clears the clipboard.
static COPIES: AtomicU64 = AtomicU64::new(0);

/// The last copy, and when it was made (wall clock: the phone's timers stop
/// while it sleeps, and the app's while it is suspended).
static LAST: Mutex<Option<(u64, SystemTime)>> = Mutex::new(None);

/// Invalidate scheduled clears and empty a clipboard used by this app.
pub fn clear(app: &AppHandle) {
    if COPIES.fetch_add(1, Ordering::SeqCst) > 0 {
        empty(app);
    }
}

/// The phone's own way first; the plain clipboard where there is none.
fn empty(app: &AppHandle) {
    if app.myle_mobile().clear_clipboard().is_err() {
        let _ = app.clipboard().write_text(String::new());
    }
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

pub fn copy_secret(app: &AppHandle, text: &str) -> Result<(), String> {
    // Sensitive on Android, this device only and self-expiring on iOS; the plain clipboard where neither is (Windows).
    let phone = app.myle_mobile().copy_secret(text, CLEAR_AFTER.as_secs() as u32).is_ok();
    if !phone {
        app.clipboard()
            .write_text(text.to_string())
            .map_err(|_| "Could not copy to the clipboard.".to_string())?;
    }
    let copy = COPIES.fetch_add(1, Ordering::SeqCst) + 1;
    // iOS empties it itself when it expires, and only if it still holds this
    // copy: emptying it here too would also take whatever the user copied in
    // another app since.
    if phone && cfg!(target_os = "ios") {
        *LAST.lock().unwrap_or_else(|p| p.into_inner()) = None;
        return Ok(());
    }
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some((copy, SystemTime::now()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLEAR_AFTER).await;
        if COPIES.load(Ordering::SeqCst) == copy {
            empty(&app);
        }
    });
    Ok(())
}

/// The app is in front again: a copy whose half-minute ran out while it was
/// suspended (its timer did not run) is emptied now.
pub fn resumed(app: &AppHandle) {
    let last = *LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((copy, at)) = last
        && COPIES.load(Ordering::SeqCst) == copy
        && SystemTime::now().duration_since(at).is_ok_and(|age| age >= CLEAR_AFTER)
    {
        empty(app);
    }
}
