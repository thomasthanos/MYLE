//! Starting with Windows: the Startup shortcut the setup offers (it runs the
//! app with `--autostart`), and how the app opens then. Settings can switch
//! both; the setup reads the same registry values.

use std::path::{Path, PathBuf};

use serde::Serialize;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::storage::{FOLDER, PUBLISHER};

/// The shortcut's name, as the setup names it (`product::NAME`), and its
/// fallback name for when another file already had that one.
const NAME: &str = "MYLE";
const LEGACY_NAME: &str = "Make Your Life Easier";

/// Shared with the setup (`backend/installer/src/registry.rs`), in the key it
/// keeps the install folder in.
const START_MINIMIZED: &str = "StartMinimized";
/// The setup's remembered state of the Startup shortcut: 0 off, 2 made
/// (`engine::state` in the installer). Kept in step here, so an update
/// neither brings back a shortcut switched off in Settings nor treats one
/// switched on here as deleted.
const STARTUP_STATE: &str = "StartupShortcut";
const STATE_OFF: u32 = 0;
const STATE_MADE: u32 = 2;

fn key() -> String {
    format!(r"Software\{PUBLISHER}\{FOLDER}")
}

pub fn launched_at_sign_in() -> bool {
    std::env::args().any(|arg| arg == "--autostart")
}

/// What the user chose for starting with Windows, as Settings and the setup
/// remember it; `None` on an install from before it was remembered.
fn chosen_state() -> Option<u32> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(key())
        .and_then(|key| key.get_value::<u32, _>(STARTUP_STATE))
        .ok()
}

/// Keeps the Startup folder to the user's choice: with starting with Windows
/// switched off, a shortcut of ours found there (put back by an older setup,
/// or copied in) goes. `true` when this very start came from such a shortcut:
/// it ends here, unseen.
pub fn respect_choice() -> bool {
    if chosen_state() != Some(STATE_OFF) {
        return false;
    }
    for link in startup_links() {
        let _ = std::fs::remove_file(link);
    }
    launched_at_sign_in()
}

/// A yes/no setting in the app's registry key; `default` when unset.
pub(crate) fn flag(name: &str, default: bool) -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(key())
        .and_then(|key| key.get_value::<u32, _>(name))
        .map_or(default, |value| value != 0)
}

pub(crate) fn set_flag(name: &str, value: bool) -> Result<(), String> {
    set_value(name, u32::from(value))
}

/// Unset (an install from before the switch) means minimized, as it was.
pub fn start_minimized() -> bool {
    flag(START_MINIMIZED, true)
}

fn set_value(name: &str, value: u32) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(key())
        .map_err(|error| error.to_string())?;
    key.set_value(name, &value)
        .map_err(|error| error.to_string())
}

/// `...\Start Menu\Programs\Startup`, the folder Windows runs at sign-in.
fn startup_folder() -> Option<PathBuf> {
    let roaming = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(roaming).join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
}

fn startup_links() -> Vec<PathBuf> {
    let Some(folder) = startup_folder() else {
        return Vec::new();
    };
    [
        format!("{NAME}.lnk"),
        format!("{NAME} - {PUBLISHER}.lnk"),
        format!("{LEGACY_NAME}.lnk"),
        format!("{LEGACY_NAME} - {PUBLISHER}.lnk"),
    ]
        .into_iter()
        .map(|name| folder.join(name))
        .filter(|path| path.is_file())
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSettings {
    /// A Startup shortcut is there.
    enabled: bool,
    minimized: bool,
    /// Only the installed app can add itself (a dev build would point
    /// Windows at the build folder).
    can_change: bool,
}

#[tauri::command]
pub fn startup_get() -> StartupSettings {
    StartupSettings {
        enabled: !startup_links().is_empty(),
        minimized: start_minimized(),
        can_change: crate::updater::installed_exe().is_some(),
    }
}

#[tauri::command]
pub async fn startup_set_enabled(enabled: bool) -> Result<(), String> {
    let exe = crate::updater::installed_exe()
        .ok_or("Only the installed app can start with Windows.")?;
    if enabled {
        let folder = startup_folder().ok_or("The Startup folder could not be found.")?;
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let link = folder.join(format!("{NAME}.lnk"));
        let written = {
            let link = link.clone();
            tauri::async_runtime::spawn_blocking(move || write_link(&exe, &link))
                .await
                .map_err(|error| error.to_string())?
        };
        if written.is_err() || !link.is_file() {
            return Err("Windows would not create the Startup shortcut.".into());
        }
        set_value(STARTUP_STATE, STATE_MADE)
    } else {
        for link in startup_links() {
            std::fs::remove_file(&link).map_err(|error| error.to_string())?;
        }
        set_value(STARTUP_STATE, STATE_OFF)
    }
}

/// The same shortcut the setup writes (`write_link` in the installer), so the
/// setup and the uninstaller recognise it as the app's. Written through the
/// shell's own COM object, in place: any path, Greek user names included,
/// and no PowerShell to start.
fn write_link(exe: &Path, link_path: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    let folder = exe.parent().unwrap_or(exe);
    // SAFETY: balanced below when initialization succeeded (S_OK or S_FALSE).
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let saved = (|| unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()))?;
        link.SetArguments(&HSTRING::from("--autostart"))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from(NAME))?;
        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(link_path.as_os_str()), true)
    })()
    // Read (and the COM error dropped) before COM is uninitialized.
    .map_err(|error| error.message());
    if initialized {
        unsafe { CoUninitialize() };
    }
    saved
}

#[tauri::command]
pub fn startup_set_minimized(minimized: bool) -> Result<(), String> {
    set_value(START_MINIMIZED, u32::from(minimized))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Target, arguments, working folder and description, as the shell reads them.
    fn read_link(link_path: &Path) -> (PathBuf, String, PathBuf, String) {
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile, STGM_READ,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
        use windows::core::{HSTRING, Interface};

        let text = |buffer: &[u16]| {
            String::from_utf16_lossy(&buffer[..buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len())])
        };
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.cast::<IPersistFile>()
                .unwrap()
                .Load(&HSTRING::from(link_path.as_os_str()), STGM_READ)
                .unwrap();
            let (mut target, mut arguments, mut folder, mut description) =
                (vec![0u16; 1024], vec![0u16; 1024], vec![0u16; 1024], vec![0u16; 1024]);
            link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).unwrap();
            link.GetArguments(&mut arguments).unwrap();
            link.GetWorkingDirectory(&mut folder).unwrap();
            link.GetDescription(&mut description).unwrap();
            (
                PathBuf::from(text(&target)),
                text(&arguments),
                PathBuf::from(text(&folder)),
                text(&description),
            )
        }
    }

    /// Written where the path has Greek letters (a user name, say), and read
    /// back by the shell with every detail as the setup writes it.
    #[test]
    fn the_startup_shortcut_is_written_for_any_path() {
        let folder = std::env::temp_dir().join(format!("myle-startup-Θωμάς-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let exe = folder.join("MYLE.exe");
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        std::fs::copy(PathBuf::from(system).join("System32").join("whoami.exe"), &exe).unwrap();
        let link = folder.join("MYLE.lnk");
        write_link(&exe, &link).unwrap();

        let (target, arguments, working_folder, description) = read_link(&link);
        // The shell may expand an 8.3 path (RUNNER~1 on CI). Compare the
        // resolved locations while still checking every shortcut property.
        assert_eq!(target.canonicalize().unwrap(), exe.canonicalize().unwrap());
        assert_eq!(working_folder.canonicalize().unwrap(), folder.canonicalize().unwrap());
        assert_eq!(arguments, "--autostart");
        assert_eq!(description, NAME);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
