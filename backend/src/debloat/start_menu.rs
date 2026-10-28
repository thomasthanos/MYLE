// ! Windows 11 Start Menu customization: layout, alignment, Windows 11 24H2/25H2 ! All Apps view mode (Category / Grid / List), Recommended section visibility, ! quick folders next to the Power button (`VisiblePlaces`), and pinned apps ! presets / cleanup with backup & restore of `start2.bin`.
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use serde::{Deserialize, Serialize};
use winreg::RegKey;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_64KEY, REG_BINARY,
};
use winreg::RegValue;
use super::catalog::{Data, Hive};
use super::system::{self, Value};
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const START_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Start";
const USER_EXPLORER_POLICY: &str = r"Software\Policies\Microsoft\Windows\Explorer";
const MACHINE_EXPLORER_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\Explorer";
const POLICY_MANAGER_START: &str = r"SOFTWARE\Microsoft\PolicyManager\current\device\Start";
const POLICY_MANAGER_EDU: &str = r"SOFTWARE\Microsoft\PolicyManager\current\device\Education";
const PROVIDER_GUID: &str = "B5292708-1619-419B-9923-E5D9F3925E71";
const POLICY_PROVIDER_START: &str = r"SOFTWARE\Microsoft\PolicyManager\providers\B5292708-1619-419B-9923-E5D9F3925E71\default\Device\Start";
/// Official 16-byte little-endian GUIDs for `HKCU\...\Start\VisiblePlaces`.
pub const FOLDERS: &[(&str, [u8; 16])] = &[
    // Settings: {52730886-51AA-4243-9F7B-2776584659D4}
    ("settings", [0x86, 0x08, 0x73, 0x52, 0xAA, 0x51, 0x43, 0x42, 0x9F, 0x7B, 0x27, 0x76, 0x58, 0x46, 0x59, 0xD4]),
    // File Explorer: {148A24BC-D60C-4289-A080-6ED9BBA24882}
    ("explorer", [0xBC, 0x24, 0x8A, 0x14, 0x0C, 0xD6, 0x89, 0x42, 0xA0, 0x80, 0x6E, 0xD9, 0xBB, 0xA2, 0x48, 0x82]),
    // Downloads: {E367B32F-89DE-4355-BFCE-61F37B18A937}
    ("downloads", [0x2F, 0xB3, 0x67, 0xE3, 0xDE, 0x89, 0x55, 0x43, 0xBF, 0xCE, 0x61, 0xF3, 0x7B, 0x18, 0xA9, 0x37]),
    // Documents: {2D34D5CE-FA5A-4543-82F2-22E6EAF7773C}
    ("documents", [0xCE, 0xD5, 0x34, 0x2D, 0x5A, 0xFA, 0x43, 0x45, 0x82, 0xF2, 0x22, 0xE6, 0xEA, 0xF7, 0x77, 0x3C]),
    // Personal folder (UserProfile): {74BDB04A-F94A-4F68-8BD6-4398071DA8BC}
    ("userProfile", [0x4A, 0xB0, 0xBD, 0x74, 0x4A, 0xF9, 0x68, 0x4F, 0x8B, 0xD6, 0x43, 0x98, 0x07, 0x1D, 0xA8, 0xBC]),
    // Pictures: {383F07A0-E80A-4C80-B05A-86DB845DBC4D}
    ("pictures", [0xA0, 0x07, 0x3F, 0x38, 0x0A, 0xE8, 0x80, 0x4C, 0xB0, 0x5A, 0x86, 0xDB, 0x84, 0x5D, 0xBC, 0x4D]),
    // Music: {B00B0620-7F51-4C32-AA1E-34CC547F7315}
    ("music", [0x20, 0x06, 0x0B, 0xB0, 0x51, 0x7F, 0x32, 0x4C, 0xAA, 0x1E, 0x34, 0xCC, 0x54, 0x7F, 0x73, 0x15]),
    // Videos: {42B3A5C5-7D86-42F4-80A4-93FACA7A88B5}
    ("videos", [0xC5, 0xA5, 0xB3, 0x42, 0x86, 0x7D, 0xF4, 0x42, 0x80, 0xA4, 0x93, 0xFA, 0xCA, 0x7A, 0x88, 0xB5]),
    // Network: {FE758144-080D-42AE-8BDA-34ED97B66394}
    ("network", [0x44, 0x81, 0x75, 0xFE, 0x0D, 0x08, 0xAE, 0x42, 0x8B, 0xDA, 0x34, 0xED, 0x97, 0xB6, 0x63, 0x94]),
];
/// Fixed catalog of safe Start Menu pin targets for `ConfigureStartPins`.
const PIN_CATALOG: &[(&str, &str)] = &[
    ("explorer", r#"{"desktopAppId":"Microsoft.Windows.Explorer"}"#),
    ("settings", r#"{"packagedAppId":"windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"}"#),
    ("store", r#"{"packagedAppId":"Microsoft.WindowsStore_8wekyb3d8bbwe!App"}"#),
    ("terminal", r#"{"packagedAppId":"Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"}"#),
    ("calculator", r#"{"packagedAppId":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#),
    ("notepad", r#"{"packagedAppId":"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"}"#),
    ("snipping-tool", r#"{"packagedAppId":"Microsoft.ScreenSketch_8wekyb3d8bbwe!App"}"#),
    ("photos", r#"{"packagedAppId":"Microsoft.Windows.Photos_8wekyb3d8bbwe!App"}"#),
    ("paint", r#"{"packagedAppId":"Microsoft.Paint_8wekyb3d8bbwe!App"}"#),
    ("clock", r#"{"packagedAppId":"Microsoft.WindowsAlarms_8wekyb3d8bbwe!App"}"#),
    ("edge", r#"{"desktopAppId":"MSEdge"}"#),
    ("xbox", r#"{"packagedAppId":"Microsoft.GamingApp_8wekyb3d8bbwe!Microsoft.Xbox.App"}"#),
];
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMenuStatus {
    pub supported: bool,
    pub alignment: String,
    pub layout: String,
    pub all_apps_view: String,
    pub hide_recommended: bool,
    pub show_recent_apps: bool,
    pub show_most_used_apps: bool,
    pub show_recent_files: bool,
    pub show_recommendations: bool,
    pub show_account_notifications: bool,
    pub folders: Vec<String>,
    pub has_pins_backup: bool,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMenuUpdate {
    pub alignment: Option<String>,
    pub layout: Option<String>,
    pub all_apps_view: Option<String>,
    pub show_recent_apps: Option<bool>,
    pub show_most_used_apps: Option<bool>,
    pub show_recent_files: Option<bool>,
    pub show_recommendations: Option<bool>,
    pub show_account_notifications: Option<bool>,
    pub folders: Option<Vec<String>>,
}
fn read_dword(hive: Hive, path: &str, name: &str) -> Option<u32> {
    match system::read(hive, path, name) {
        Value::Dword { value } => Some(value),
        _ => None,
    }
}
fn read_visible_places() -> Vec<String> {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(START_KEY, KEY_READ | KEY_WOW64_64KEY) else {
        return Vec::new();
    };
    let Ok(raw) = key.get_raw_value("VisiblePlaces") else {
        return Vec::new();
    };
    let mut active = Vec::new();
    for chunk in raw.bytes.as_chunks::<16>().0 {
        for &(id, ref guid) in FOLDERS {
            if chunk == guid.as_slice() && !active.iter().any(|existing| existing == id) {
                active.push(id.to_string());
            }
        }
    }
    active
}
fn write_visible_places(ids: &[String]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(ids.len() * 16);
    for id in ids {
        if let Some((_, guid)) = FOLDERS.iter().find(|&&(known, _)| known == id.as_str()) {
            bytes.extend_from_slice(guid);
        }
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(START_KEY, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
        .map_err(|e| format!("{START_KEY}: {e}"))?;
    let raw = RegValue { vtype: REG_BINARY, bytes: bytes.into() };
    key.set_raw_value("VisiblePlaces", &raw)
        .map_err(|e| format!("{START_KEY}\\VisiblePlaces: {e}"))?;
    let _ = key.set_value("PlacesInitializedVersion", &2u32);
    Ok(())
}
fn start_bin_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(local.join(r"Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start2.bin"))
}
fn start_bin_backup_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(local.join(r"Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start2.bin.myle-backup"))
}
/// These are Windows 11's Start Menu settings: on another Windows they are
/// never written, whatever the page asks.
pub fn ensure_supported() -> Result<(), String> {
    if system::windows_info().windows11 {
        Ok(())
    } else {
        Err("Start Menu customization needs Windows 11.".into())
    }
}
pub fn status() -> StartMenuStatus {
    let info = system::windows_info();
    let alignment = match read_dword(Hive::User, ADVANCED, "TaskbarAl") {
        Some(0) => "left",
        _ => "center",
    }
    .to_string();
    let layout = match read_dword(Hive::User, ADVANCED, "Start_Layout") {
        Some(1) => "morePins",
        Some(2) => "moreRecommendations",
        _ => "default",
    }
    .to_string();
    let all_apps_view = match read_dword(Hive::User, START_KEY, "AllAppsViewMode") {
        Some(1) => "grid",
        Some(2) => "list",
        _ => "category",
    }
    .to_string();
    let hide_recommended = read_dword(Hive::Machine, MACHINE_EXPLORER_POLICY, "HideRecommendedSection") == Some(1)
        || read_dword(Hive::Machine, POLICY_MANAGER_START, "HideRecommendedSection") == Some(1)
        || read_dword(Hive::User, USER_EXPLORER_POLICY, "HideRecommendedSection") == Some(1);
    let show_recent_apps = read_dword(Hive::User, START_KEY, "HideRecentlyAddedApps") != Some(1);
    let show_most_used_apps = read_dword(Hive::User, ADVANCED, "Start_TrackProgs") == Some(1)
        || read_dword(Hive::User, START_KEY, "ShowFrequentList") == Some(1);
    let show_recent_files = read_dword(Hive::User, ADVANCED, "Start_TrackDocs") != Some(0);
    let show_recommendations = read_dword(Hive::User, ADVANCED, "Start_IrisRecommendations") != Some(0);
    let show_account_notifications = read_dword(Hive::User, ADVANCED, "Start_AccountNotifications") != Some(0);
    let folders = read_visible_places();
    let has_pins_backup = start_bin_backup_path().is_some_and(|p| p.is_file());
    StartMenuStatus {
        supported: info.windows11,
        alignment,
        layout,
        all_apps_view,
        hide_recommended,
        show_recent_apps,
        show_most_used_apps,
        show_recent_files,
        show_recommendations,
        show_account_notifications,
        folders,
        has_pins_backup,
    }
}
pub fn apply_user_update(update: StartMenuUpdate) -> Result<StartMenuStatus, String> {
    let mut restart_host = false;
    if let Some(alignment) = update.alignment.as_deref() {
        let val = if alignment == "left" { 0 } else { 1 };
        system::write(Hive::User, ADVANCED, "TaskbarAl", Data::Dword(val))?;
    }
    if let Some(layout) = update.layout.as_deref() {
        let val = match layout {
            "morePins" => 1,
            "moreRecommendations" => 2,
            _ => 0,
        };
        system::write(Hive::User, ADVANCED, "Start_Layout", Data::Dword(val))?;
    }
    if let Some(view) = update.all_apps_view.as_deref() {
        let val = match view {
            "grid" => 1,
            "list" => 2,
            _ => 0,
        };
        system::write(Hive::User, START_KEY, "AllAppsViewMode", Data::Dword(val))?;
        restart_host = true;
    }
    if let Some(show) = update.show_recent_apps {
        system::write(Hive::User, START_KEY, "HideRecentlyAddedApps", Data::Dword(if show { 0 } else { 1 }))?;
    }
    if let Some(show) = update.show_most_used_apps {
        let val = if show { 1 } else { 0 };
        system::write(Hive::User, ADVANCED, "Start_TrackProgs", Data::Dword(val))?;
        system::write(Hive::User, START_KEY, "ShowFrequentList", Data::Dword(val))?;
    }
    if let Some(show) = update.show_recent_files {
        system::write(Hive::User, ADVANCED, "Start_TrackDocs", Data::Dword(if show { 1 } else { 0 }))?;
    }
    if let Some(show) = update.show_recommendations {
        system::write(Hive::User, ADVANCED, "Start_IrisRecommendations", Data::Dword(if show { 1 } else { 0 }))?;
    }
    if let Some(show) = update.show_account_notifications {
        system::write(Hive::User, ADVANCED, "Start_AccountNotifications", Data::Dword(if show { 1 } else { 0 }))?;
    }
    if let Some(folders) = update.folders {
        write_visible_places(&folders)?;
    }
    system::broadcast_setting_change("TraySettings");
    system::broadcast_setting_change("Policy");
    if restart_host {
        restart_start_menu();
    }
    Ok(status())
}
pub fn apply_user_hide_recommended(hide: bool) -> Result<(), String> {
    if hide {
        system::write(Hive::User, USER_EXPLORER_POLICY, "HideRecommendedSection", Data::Dword(1))?;
        system::write(Hive::User, ADVANCED, "Start_TrackDocs", Data::Dword(0))?;
        system::write(Hive::User, ADVANCED, "Start_IrisRecommendations", Data::Dword(0))?;
    } else {
        system::restore(Hive::User, USER_EXPLORER_POLICY, "HideRecommendedSection", &Value::Absent, None)?;
        // Both parts hiding turned off come back, not only the recent files.
        system::write(Hive::User, ADVANCED, "Start_TrackDocs", Data::Dword(1))?;
        system::write(Hive::User, ADVANCED, "Start_IrisRecommendations", Data::Dword(1))?;
    }
    Ok(())
}
/// Runs inside the elevated administrator helper.
pub fn apply_machine_hide_recommended(hide: bool) -> Result<(), String> {
    if hide {
        system::write(Hive::Machine, MACHINE_EXPLORER_POLICY, "HideRecommendedSection", Data::Dword(1))?;
        system::write(Hive::Machine, POLICY_MANAGER_START, "HideRecommendedSection", Data::Dword(1))?;
        system::write(Hive::Machine, POLICY_MANAGER_EDU, "IsEducationEnvironment", Data::Dword(1))?;
    } else {
        system::restore(Hive::Machine, MACHINE_EXPLORER_POLICY, "HideRecommendedSection", &Value::Absent, None)?;
        system::restore(Hive::Machine, POLICY_MANAGER_START, "HideRecommendedSection", &Value::Absent, None)?;
        system::restore(Hive::Machine, POLICY_MANAGER_EDU, "IsEducationEnvironment", &Value::Absent, None)?;
    }
    Ok(())
}
pub fn build_pins_json(pin_ids: &[String]) -> Result<String, String> {
    let mut items = Vec::new();
    for id in pin_ids {
        let Some(&(_, entry)) = PIN_CATALOG.iter().find(|&&(known, _)| known == id.as_str()) else {
            return Err(format!("Unknown Start Menu pin id: {id}"));
        };
        if !items.contains(&entry) {
            items.push(entry);
        }
    }
    Ok(format!(r#"{{"pinnedList":[{}]}}"#, items.join(",")))
}
/// Validates that `json` was built solely from `PIN_CATALOG` entries before the
/// elevated helper writes it to `PolicyManager`.
pub fn valid_pins_json(json: &str) -> bool {
    let Some(inner) = json.strip_prefix(r#"{"pinnedList":["#).and_then(|s| s.strip_suffix("]}")) else {
        return false;
    };
    if inner.is_empty() {
        return true;
    }
    let mut rest = inner;
    loop {
        let Some(&(_, matched)) = PIN_CATALOG.iter().find(|&&(_, entry)| rest.starts_with(entry)) else {
            return false;
        };
        rest = &rest[matched.len()..];
        if rest.is_empty() {
            return true;
        }
        let Some(next) = rest.strip_prefix(',') else {
            return false;
        };
        rest = next;
    }
}
/// Runs inside the elevated administrator helper. `Some(json)` sets the
/// `ConfigureStartPins` policy; `None` removes the policy keys so the user can
/// freely pin and unpin apps again.
pub fn apply_machine_start_pins(json: Option<&str>) -> Result<(), String> {
    if let Some(json) = json {
        if !valid_pins_json(json) {
            return Err("Invalid Start Menu pin configuration.".into());
        }
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let (start_key, _) = hklm
            .create_subkey_with_flags(POLICY_MANAGER_START, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
            .map_err(|e| format!("{POLICY_MANAGER_START}: {e}"))?;
        start_key.set_value("ConfigureStartPins", &json).map_err(|e| e.to_string())?;
        start_key.set_value("ConfigureStartPins_ProviderSet", &1u32).map_err(|e| e.to_string())?;
        start_key.set_value("ConfigureStartPins_WinningProvider", &PROVIDER_GUID).map_err(|e| e.to_string())?;
        let (prov_key, _) = hklm
            .create_subkey_with_flags(POLICY_PROVIDER_START, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
            .map_err(|e| format!("{POLICY_PROVIDER_START}: {e}"))?;
        prov_key.set_value("ConfigureStartPins", &json).map_err(|e| e.to_string())?;
        prov_key.set_value("ConfigureStartPins_LastWrite", &1u32).map_err(|e| e.to_string())?;
    } else {
        for name in ["ConfigureStartPins", "ConfigureStartPins_ProviderSet", "ConfigureStartPins_WinningProvider"] {
            let _ = system::restore(Hive::Machine, POLICY_MANAGER_START, name, &Value::Absent, None);
        }
        for name in ["ConfigureStartPins", "ConfigureStartPins_LastWrite"] {
            let _ = system::restore(Hive::Machine, POLICY_PROVIDER_START, name, &Value::Absent, None);
        }
    }
    Ok(())
}
pub fn backup_start_bin() {
    if let (Some(src), Some(dst)) = (start_bin_path(), start_bin_backup_path())
        && src.is_file()
        && !dst.is_file()
    {
        let _ = std::fs::copy(src, dst);
    }
}
pub fn restore_start_bin() -> Result<(), String> {
    let src = start_bin_backup_path().ok_or("LocalAppData path not found.")?;
    let dst = start_bin_path().ok_or("LocalAppData path not found.")?;
    if !src.is_file() {
        return Err("No Start Menu backup was found.".into());
    }
    let _ = Command::new(system::windows_dir().join(r"System32\taskkill.exe"))
        .args(["/f", "/im", "StartMenuExperienceHost.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    std::thread::sleep(std::time::Duration::from_millis(250));
    std::fs::copy(&src, &dst).map_err(|e| format!("Could not restore start2.bin: {e}"))?;
    restart_start_menu();
    Ok(())
}
pub fn restart_start_menu() {
    let _ = Command::new(system::windows_dir().join(r"System32\taskkill.exe"))
        .args(["/f", "/im", "StartMenuExperienceHost.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_json_is_validated_strictly() {
        let empty = build_pins_json(&[]).unwrap();
        assert_eq!(empty, r#"{"pinnedList":[]}"#);
        assert!(valid_pins_json(&empty));
        let some = build_pins_json(&["explorer".into(), "settings".into(), "terminal".into()]).unwrap();
        assert!(valid_pins_json(&some));
        assert!(!valid_pins_json(r#"{"pinnedList":[{"desktopAppId":"evil.exe"}]}"#));
        assert!(build_pins_json(&["unknown".into()]).is_err());
    }
}
