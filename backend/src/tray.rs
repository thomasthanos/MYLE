//! The icon next to the clock, only when the user asked for it ("Keep
//! running in the tray" in Settings, off unless switched on): closing the
//! window then keeps the app running there, so Ctrl+Shift+L and the browser
//! extension keep working, and the icon's menu opens the app, locks the
//! vault or quits. Otherwise closing the window quits the app.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::startup;

const TRAY_ID: &str = "main";
/// The Settings switch, in the app's registry key. Unset means off: the app
/// never stays behind unless the user chose it.
const KEEP_IN_TRAY: &str = "KeepInTray";

fn enabled() -> bool {
    startup::flag(KEEP_IN_TRAY, false)
}

/// Whether closing the window should hide it to the tray instead of quitting.
pub fn keep_running() -> bool {
    enabled()
}

/// Brings the main window back, from the tray or from hiding.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Adds the icon if it is not there yet.
pub fn ensure(app: &AppHandle) -> tauri::Result<()> {
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }
    let open = MenuItem::with_id(app, "open", "Open MYLE", true, None::<&str>)?;
    let passwords = MenuItem::with_id(app, "passwords", "Password Manager", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "Lock the vault", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &passwords, &lock, &separator, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("MYLE")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "passwords" => crate::passwords::browser::open_vault(app),
            "lock" => crate::passwords::lock_now(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// The setup asks the app to quit through this event before it closes the
/// window: a closed window only hides to the tray, and a program still
/// running after that would have to be stopped by force. Must match
/// `quit_event_name` in `backend/installer/src/processes.rs`.
fn quit_event_name() -> Vec<u16> {
    format!(r"Local\{}-Quit", crate::storage::FOLDER)
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

/// Exits cleanly when the setup (an update, or the uninstaller) asks.
pub fn watch_quit(app: AppHandle) {
    use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
    let name = quit_event_name();
    // Manual reset, not signalled: the setup sets it once.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    if event.is_null() {
        return;
    }
    let event = event as usize;
    std::thread::spawn(move || {
        unsafe { WaitForSingleObject(event as _, INFINITE) };
        app.exit(0);
    });
}

pub fn remove(app: &AppHandle) {
    let _ = app.remove_tray_by_id(TRAY_ID);
}

/// On start: the icon is there whenever closing would keep the app running.
pub fn init(app: &AppHandle) {
    if keep_running() {
        let _ = ensure(app);
    }
}

#[tauri::command]
pub fn tray_get() -> bool {
    enabled()
}

#[tauri::command]
pub fn tray_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    startup::set_flag(KEEP_IN_TRAY, enabled)?;
    if keep_running() {
        ensure(&app).map_err(|e| e.to_string())
    } else {
        remove(&app);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_quit_event_is_the_one_the_setup_signals() {
        let name = super::quit_event_name();
        assert_eq!(name.last(), Some(&0));
        let text = String::from_utf16(&name[..name.len() - 1]).unwrap();
        // backend/installer/src/processes.rs signals the same name.
        assert_eq!(text, r"Local\MakeYourLifeEasier-Quit");
    }
}
