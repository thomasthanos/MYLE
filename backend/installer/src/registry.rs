//! The per-user registry entries: the "Installed apps" entry and the
//! remembered install folder. Names and values match what the NSIS setup
//! wrote, so an install made by it is found, updated and removed the same way.

use std::path::{Path, PathBuf};

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

use crate::product;

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

fn unquote(text: &str) -> &str {
    text.trim().trim_matches('"')
}

/// The folder of an existing install, if there is one.
pub fn install_dir() -> Option<PathBuf> {
    let remembered = hkcu()
        .open_subkey_with_flags(product::product_key(), KEY_READ)
        .and_then(|key| key.get_value::<String, _>(""))
        .ok();
    let listed = || {
        hkcu()
            .open_subkey_with_flags(product::uninstall_key(), KEY_READ)
            .and_then(|key| key.get_value::<String, _>("InstallLocation"))
            .ok()
    };
    remembered
        .or_else(listed)
        .map(|dir| PathBuf::from(unquote(&dir)))
        .filter(|dir| !dir.as_os_str().is_empty())
}

/// The version "Installed apps" lists, if the app is installed.
pub fn installed_version() -> Option<String> {
    hkcu()
        .open_subkey_with_flags(product::uninstall_key(), KEY_READ)
        .and_then(|key| key.get_value::<String, _>("DisplayVersion"))
        .ok()
        .filter(|version| !version.trim().is_empty())
}

/// Lists the app in "Installed apps" and remembers where it went.
pub fn register(dir: &Path, version: &str, size_kb: u32) -> Result<(), String> {
    let exe = dir.join(product::exe_name());
    let uninstaller = dir.join(product::UNINSTALLER);
    let (key, _) = hkcu()
        .create_subkey(product::uninstall_key())
        .map_err(|e| format!("Windows did not accept the uninstall entry: {e}"))?;
    let quoted = |path: &Path| format!("\"{}\"", path.display());
    let text: [(&str, String); 10] = [
        ("DisplayName", product::NAME.to_string()),
        ("DisplayIcon", quoted(&crate::shell_identity::icon_path(&exe))),
        ("DisplayVersion", version.to_string()),
        ("Publisher", product::PUBLISHER.to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", quoted(&uninstaller)),
        (
            "QuietUninstallString",
            format!("{} /S", quoted(&uninstaller)),
        ),
        ("MainBinaryName", product::exe_name()),
        ("InstallDate", today()),
        ("Comments", format!("{} for Windows", product::NAME)),
    ];
    for (name, value) in text {
        key.set_value(name, &value).map_err(|e| e.to_string())?;
    }
    key.set_value("NoModify", &1u32)
        .map_err(|e| e.to_string())?;
    key.set_value("NoRepair", &1u32)
        .map_err(|e| e.to_string())?;
    key.set_value("EstimatedSize", &size_kb)
        .map_err(|e| e.to_string())?;

    let (remembered, _) = hkcu()
        .create_subkey(product::product_key())
        .map_err(|e| e.to_string())?;
    remembered
        .set_value("", &dir.display().to_string())
        .map_err(|e| e.to_string())
}

/// Where the setup remembers what became of each shortcut (desktop, Start
/// menu, startup), next to the install folder, so a reinstall or an update
/// follows the user instead of guessing from whichever `.lnk` files happen
/// to exist. The values are `engine::ShortcutState` numbers.
const STATE_VALUES: [&str; 3] = ["DesktopShortcut", "StartMenuShortcut", "StartupShortcut"];

/// The remembered shortcut states, if an install has saved them.
pub fn shortcut_states() -> Option<[u32; 3]> {
    let key = hkcu()
        .open_subkey_with_flags(product::product_key(), KEY_READ)
        .ok()?;
    let mut states = [0; 3];
    for (state, name) in states.iter_mut().zip(STATE_VALUES) {
        *state = key.get_value::<u32, _>(name).ok()?;
    }
    Some(states)
}

pub fn remember_shortcut_states(states: [u32; 3]) {
    let Ok((key, _)) = hkcu().create_subkey(product::product_key()) else {
        return;
    };
    for (state, name) in states.into_iter().zip(STATE_VALUES) {
        let _ = key.set_value(name, &state);
    }
}

pub fn taskbar_preference() -> crate::shortcut_preferences::TaskbarPreference {
    use crate::shortcut_preferences::{TASKBAR_CHOICE, TASKBAR_PENDING, taskbar_preference};
    let key = hkcu().open_subkey(product::product_key()).ok();
    let read = |name: &str| key.as_ref().and_then(|key| key.get_value::<u32, _>(name).ok()).map(|value| value != 0);
    taskbar_preference(read(TASKBAR_CHOICE), read(TASKBAR_PENDING))
}

pub fn remember_taskbar_choice(enabled: bool) -> Result<(), String> {
    use crate::shortcut_preferences::{TASKBAR_CHOICE, TASKBAR_PENDING};
    let (key, _) = hkcu().create_subkey(product::product_key()).map_err(|error| error.to_string())?;
    for name in [TASKBAR_CHOICE, TASKBAR_PENDING] {
        key.set_value(name, &u32::from(enabled)).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Whether the app, when Windows starts it at sign-in, waits minimized in
/// the taskbar. Read by the app (`backend/src/startup.rs`), which can also
/// change it from Settings. Unset means minimized.
const START_MINIMIZED: &str = "StartMinimized";

pub fn start_minimized() -> bool {
    hkcu()
        .open_subkey_with_flags(product::product_key(), KEY_READ)
        .and_then(|key| key.get_value::<u32, _>(START_MINIMIZED))
        .map_or(true, |value| value != 0)
}

pub fn set_start_minimized(minimized: bool) {
    if let Ok((key, _)) = hkcu().create_subkey(product::product_key()) {
        let _ = key.set_value(START_MINIMIZED, &u32::from(minimized));
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Whether the old app's own "start with Windows" (a `Run` value, not a
/// Startup shortcut) is still set.
pub fn legacy_autostart() -> bool {
    hkcu()
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .is_ok_and(|run| run.get_raw_value(product::LEGACY_NAME).is_ok())
}

/// Drops the old `Run` value: the Startup shortcut (with `--autostart`, so
/// the app starts minimized) replaces it. Both would start the app twice.
pub fn remove_legacy_autostart() {
    if let Ok(run) = hkcu().open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE) {
        let _ = run.delete_value(product::LEGACY_NAME);
    }
}

/// Removes the "Installed apps" entry and the login-launch value the NSIS
/// setup's uninstaller also cleared. `forget_folder` drops the remembered
/// install folder too (a full clean-up); otherwise a reinstall reuses it.
/// Where the app registers the password extension's native messaging host
/// (`backend/src/passwords/browser.rs`): the browsers forget it on uninstall.
const NATIVE_HOSTS: [&str; 5] = [
    r"Software\Google\Chrome\NativeMessagingHosts\com.thomasthanos.myle",
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.thomasthanos.myle",
    r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.thomasthanos.myle",
    r"Software\Chromium\NativeMessagingHosts\com.thomasthanos.myle",
    r"Software\Mozilla\NativeMessagingHosts\com.thomasthanos.myle",
];

pub fn unregister(forget_folder: bool) {
    let _ = hkcu().delete_subkey_all(product::uninstall_key());
    remove_legacy_autostart();
    for key in NATIVE_HOSTS {
        let _ = hkcu().delete_subkey_all(key);
    }
    if forget_folder {
        let _ = hkcu().delete_subkey_all(product::product_key());
        // The publisher key only if nothing else of ours lives under it.
        let empty = hkcu()
            .open_subkey_with_flags(product::publisher_key(), KEY_READ)
            .is_ok_and(|key| {
                key.enum_keys().next().is_none() && key.enum_values().next().is_none()
            });
        if empty {
            let _ = hkcu().delete_subkey(product::publisher_key());
        }
    }
}

/// `YYYYMMDD`, as "Installed apps" expects `InstallDate`.
fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_dates_have_eight_digits() {
        let date = today();
        assert_eq!(date.len(), 8);
        assert!(date.starts_with("20"));
    }

    #[test]
    fn quoted_registry_paths_are_unquoted() {
        assert_eq!(unquote(r#""C:\Apps\MYLE""#), r"C:\Apps\MYLE");
        assert_eq!(unquote(r"C:\Apps\MYLE"), r"C:\Apps\MYLE");
    }
}
