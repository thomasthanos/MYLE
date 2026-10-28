// ! What the debloater reads and changes in Windows: registry values, ! services, scheduled tasks, the time format, Store apps, and Explorer. ! Reading needs no administrator rights; what changes the machine runs in ! the administrator helper.
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use winreg::RegKey;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_64KEY, REG_DWORD,
    REG_EXPAND_SZ, REG_SZ,
};
use super::catalog::{Data, Hive};
// --------------------------------------------------------------------------- Windows itself
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsInfo {
    pub build: u32,
    /// "Windows 11 Pro 25H2 (26200.6584)".
    pub name: String,
    pub windows11: bool,
}
pub fn windows_info() -> WindowsInfo {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ | KEY_WOW64_64KEY)
        .ok();
    let text = |name: &str| key.as_ref().and_then(|k| k.get_value::<String, _>(name).ok()).unwrap_or_default();
    let build: u32 = text("CurrentBuild").parse().unwrap_or(0);
    let ubr: u32 = key.as_ref().and_then(|k| k.get_value("UBR").ok()).unwrap_or(0);
    let windows11 = build >= 22000;
    // ProductName still says "Windows 10" on Windows 11.
    let edition = text("EditionID");
    let edition = match edition.as_str() {
        "Core" | "CoreSingleLanguage" | "CoreCountrySpecific" => "Home".to_string(),
        "Professional" => "Pro".to_string(),
        other => other.to_string(),
    };
    let version = text("DisplayVersion");
    let name = format!(
        "Windows {} {edition} {version} ({build}.{ubr})",
        if windows11 { 11 } else { 10 }
    )
    .replace("  ", " ");
    WindowsInfo { build, name, windows11 }
}
// --------------------------------------------------------------------------- Registry
/// A value as it is, or was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Value {
    Absent,
    Dword { value: u32 },
    Sz { value: String },
    /// Some other type, left alone.
    Other,
}
impl Value {
    pub fn matches(&self, data: Data) -> bool {
        match (self, data) {
            (Value::Dword { value }, Data::Dword(want)) => *value == want,
            (Value::Sz { value }, Data::Sz(want)) => value == want,
            _ => false,
        }
    }
}
fn root(hive: Hive) -> RegKey {
    RegKey::predef(match hive {
        Hive::User => HKEY_CURRENT_USER,
        Hive::Machine => HKEY_LOCAL_MACHINE,
    })
}
pub fn read(hive: Hive, path: &str, name: &str) -> Value {
    let Ok(key) = root(hive).open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY) else {
        return Value::Absent;
    };
    let Ok(raw) = key.get_raw_value(name) else {
        return Value::Absent;
    };
    match raw.vtype {
        REG_DWORD if raw.bytes.len() >= 4 => Value::Dword {
            value: u32::from_le_bytes([raw.bytes[0], raw.bytes[1], raw.bytes[2], raw.bytes[3]]),
        },
        REG_SZ | REG_EXPAND_SZ => {
            let wide: Vec<u16> = raw
                .bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .take_while(|&unit| unit != 0)
                .collect();
            Value::Sz {
                value: String::from_utf16_lossy(&wide),
            }
        }
        _ => Value::Other,
    }
}
/// The first key of `path` that does not exist yet, which writing a value
/// there creates (and undo removes again, if it is left empty).
pub fn first_missing(hive: Hive, path: &str) -> Option<String> {
    let base = root(hive);
    let mut sofar = String::new();
    for part in path.split('\\') {
        if !sofar.is_empty() {
            sofar.push('\\');
        }
        sofar.push_str(part);
        if base.open_subkey_with_flags(&sofar, KEY_READ | KEY_WOW64_64KEY).is_err() {
            return Some(sofar);
        }
    }
    None
}
pub fn write(hive: Hive, path: &str, name: &str, data: Data) -> Result<(), String> {
    let (key, _) = root(hive)
        .create_subkey_with_flags(path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
        .map_err(|e| format!("{path}: {e}"))?;
    match data {
        Data::Dword(value) => key.set_value(name, &value),
        Data::Sz(value) => key.set_value(name, &value.to_string()),
    }
    .map_err(|e| format!("{path}\\{name}: {e}"))
}
/// Puts a value back as it was: its old data, or no value at all. Keys that
/// applying created are removed again if nothing else is in them.
pub fn restore(hive: Hive, path: &str, name: &str, before: &Value, created: Option<&str>) -> Result<(), String> {
    match before {
        Value::Dword { value } => write_raw(hive, path, name, |key| key.set_value(name, value)),
        Value::Sz { value } => write_raw(hive, path, name, |key| key.set_value(name, value)),
        Value::Absent => {
            if let Ok(key) = root(hive).open_subkey_with_flags(path, KEY_ALL_ACCESS | KEY_WOW64_64KEY) {
                match key.delete_value(name) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("{path}\\{name}: {e}")),
                }
            }
            Ok(())
        }
        Value::Other => Ok(()),
    }?;
    if let Some(top) = created {
        remove_empty_keys(hive, path, top);
    }
    Ok(())
}
fn write_raw(
    hive: Hive,
    path: &str,
    name: &str,
    set: impl FnOnce(&RegKey) -> std::io::Result<()>,
) -> Result<(), String> {
    let (key, _) = root(hive)
        .create_subkey_with_flags(path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
        .map_err(|e| format!("{path}: {e}"))?;
    set(&key).map_err(|e| format!("{path}\\{name}: {e}"))
}
/// Whether `top` is `path` or one of its parent keys, below the hive's
/// first level: the only keys undo may remove (and only while empty).
pub fn key_on_path(path: &str, top: &str) -> bool {
    let (top, path) = (top.to_ascii_lowercase(), path.to_ascii_lowercase());
    top.contains('\\') && (path == top || path.starts_with(&format!("{top}\\")))
}
/// Deletes `path` and its parents up to `top` while each is empty.
fn remove_empty_keys(hive: Hive, path: &str, top: &str) {
    if !path.eq_ignore_ascii_case(top) && !path.to_ascii_lowercase().starts_with(&format!("{}\\", top.to_ascii_lowercase())) {
        return;
    }
    let base = root(hive);
    let mut current = path.to_string();
    loop {
        let empty = base
            .open_subkey_with_flags(&current, KEY_READ | KEY_WOW64_64KEY)
            .map(|key| key.enum_keys().next().is_none() && key.enum_values().next().is_none())
            .unwrap_or(false);
        if !empty || base.delete_subkey_with_flags(&current, KEY_WOW64_64KEY).is_err() {
            return;
        }
        if current.eq_ignore_ascii_case(top) {
            return;
        }
        match current.rfind('\\') {
            Some(at) => current.truncate(at),
            None => return,
        }
    }
}
// --------------------------------------------------------------------------- Services
mod services {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SERVICE_DOES_NOT_EXIST, GetLastError};
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW,
        QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW, SC_HANDLE,
        SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
        SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_NO_CHANGE,
        SERVICE_QUERY_CONFIG,
    };
    use super::super::catalog::Start;
    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: a handle this module opened and still owns.
            unsafe { CloseServiceHandle(self.0) };
        }
    }
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    /// `Ok(None)`: the service does not exist here.
    fn open(name: &str, access: u32) -> Result<Option<Handle>, String> {
        // SAFETY: plain calls; handles are closed by `Handle`.
        unsafe {
            let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if manager.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let manager = Handle(manager);
            let service = OpenServiceW(manager.0, wide(name).as_ptr(), access);
            if service.is_null() {
                let code = GetLastError();
                return if code == ERROR_SERVICE_DOES_NOT_EXIST {
                    Ok(None)
                } else {
                    Err(format!("{name}: {}", std::io::Error::from_raw_os_error(code as i32)))
                };
            }
            Ok(Some(Handle(service)))
        }
    }
    /// The start type, or `None` when the service is not on this PC (or
    /// starts with the system, which is never touched).
    pub fn start_type(name: &str) -> Result<Option<Start>, String> {
        let Some(service) = open(name, SERVICE_QUERY_CONFIG)? else {
            return Ok(None);
        };
        // SAFETY: the buffers are sized as Windows asks, and u64-aligned.
        unsafe {
            let mut needed = 0u32;
            QueryServiceConfigW(service.0, std::ptr::null_mut(), 0, &mut needed);
            if GetLastError() != ERROR_INSUFFICIENT_BUFFER {
                return Err(format!("{name}: {}", std::io::Error::last_os_error()));
            }
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
            let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
            if QueryServiceConfigW(service.0, config, needed, &mut needed) == 0 {
                return Err(format!("{name}: {}", std::io::Error::last_os_error()));
            }
            Ok(match (*config).dwStartType {
                SERVICE_AUTO_START => {
                    let mut delayed = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: 0 };
                    let size = size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32;
                    let ok = QueryServiceConfig2W(
                        service.0,
                        SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                        (&mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
                        size,
                        &mut needed,
                    );
                    Some(if ok != 0 && delayed.fDelayedAutostart != 0 { Start::AutoDelayed } else { Start::Auto })
                }
                SERVICE_DEMAND_START => Some(Start::Manual),
                SERVICE_DISABLED => Some(Start::Disabled),
                _ => None,
            })
        }
    }
    /// Needs administrator rights.
    pub fn set_start_type(name: &str, start: Start) -> Result<(), String> {
        let Some(service) = open(name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)? else {
            return Ok(());
        };
        let kind = match start {
            Start::Auto | Start::AutoDelayed => SERVICE_AUTO_START,
            Start::Manual => SERVICE_DEMAND_START,
            Start::Disabled => SERVICE_DISABLED,
        };
        // SAFETY: plain calls on an open handle; unused strings are null.
        unsafe {
            let ok = ChangeServiceConfigW(
                service.0,
                SERVICE_NO_CHANGE,
                kind,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if ok == 0 {
                return Err(format!("{name}: {}", std::io::Error::last_os_error()));
            }
            if matches!(start, Start::Auto | Start::AutoDelayed) {
                let mut delayed = SERVICE_DELAYED_AUTO_START_INFO {
                    fDelayedAutostart: i32::from(start == Start::AutoDelayed),
                };
                ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                    (&mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
                );
            }
        }
        Ok(())
    }
}
pub use services::{set_start_type, start_type};
// --------------------------------------------------------------------------- Scheduled tasks
mod tasks {
    use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoIncrementMTAUsage, CoInitializeEx, CoUninitialize,
    };
    /// Keeps COM's multithreaded apartment up for the rest of the process.
    /// Without it, the `CoUninitialize` below can be the last one and tear
    /// the apartment down while another thread (Shell calls, which use it
    /// without initializing COM themselves) still has objects in it.
    fn keep_mta() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        // SAFETY: no arguments but the cookie, which is never given back.
        ONCE.call_once(|| {
            let _ = unsafe { CoIncrementMTAUsage() };
        });
    }
    use windows::Win32::System::TaskScheduler::{IRegisteredTask, ITaskService, TaskScheduler};
    use windows::Win32::System::Variant::VARIANT;
    use windows::core::BSTR;
    /// `Ok(None)` when the task is not on this PC.
    fn with_task<T>(
        folder: &str,
        name: &str,
        act: impl FnOnce(&IRegisteredTask) -> windows::core::Result<T>,
    ) -> Result<Option<T>, String> {
        // SAFETY: COM is initialized for this thread for the duration, and every interface is dropped before it is uninitialized: the error's own error info too, which is read and let go inside the match.
        keep_mta();
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let result = match (|| {
                let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
                let empty = VARIANT::default();
                service.Connect(&empty, &empty, &empty, &empty)?;
                let folder = service.GetFolder(&BSTR::from(folder))?;
                let task = folder.GetTask(&BSTR::from(name))?;
                act(&task)
            })() {
                Ok(value) => Ok(Some(value)),
                // Not found: the file, or the path.
                Err(e) if matches!(e.code().0 as u32, 0x8007_0002 | 0x8007_0003) => Ok(None),
                Err(e) => Err(format!("{folder}\\{name}: {}", e.message())),
            };
            if initialized {
                CoUninitialize();
            }
            result
        }
    }
    pub fn task_enabled(folder: &str, name: &str) -> Result<Option<bool>, String> {
        // SAFETY: a live interface from `with_task`.
        with_task(folder, name, |task| unsafe { task.Enabled() }.map(|on| on.as_bool()))
    }
    /// Needs administrator rights for the tasks of Windows.
    pub fn set_task_enabled(folder: &str, name: &str, enabled: bool) -> Result<(), String> {
        let flag = if enabled { VARIANT_TRUE } else { VARIANT_FALSE };
        // SAFETY: a live interface from `with_task`.
        with_task(folder, name, |task| unsafe { task.SetEnabled(flag) }).map(|_| ())
    }
}
pub use tasks::{set_task_enabled, task_enabled};
// --------------------------------------------------------------------------- The time format
mod clock {
    use windows_sys::Win32::Globalization::{
        GetLocaleInfoW, LOCALE_SSHORTTIME, LOCALE_STIMEFORMAT, LOCALE_USER_DEFAULT, SetLocaleInfoW,
    };
    fn get(kind: u32) -> Result<String, String> {
        let mut buffer = [0u16; 128];
        // SAFETY: the buffer's length is passed with it.
        let length = unsafe { GetLocaleInfoW(LOCALE_USER_DEFAULT, kind, buffer.as_mut_ptr(), buffer.len() as i32) };
        if length <= 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(String::from_utf16_lossy(&buffer[..(length as usize).saturating_sub(1)]))
    }
    fn set(kind: u32, value: &str) -> Result<(), String> {
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        if unsafe { SetLocaleInfoW(LOCALE_USER_DEFAULT, kind, wide.as_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    /// The user's short and long time formats: "h:mm tt", "h:mm:ss tt".
    pub fn time_formats() -> Result<(String, String), String> {
        Ok((get(LOCALE_SSHORTTIME)?, get(LOCALE_STIMEFORMAT)?))
    }
    pub fn set_time_formats(short: &str, long: &str) -> Result<(), String> {
        set(LOCALE_STIMEFORMAT, long)?;
        set(LOCALE_SSHORTTIME, short)
    }
}
pub use clock::{set_time_formats, time_formats};
/// Whether a time format shows 24 hours: no 12-hour `h`, no AM/PM `t`.
pub fn is_24h(format: &str) -> bool {
    !unquoted(format).any(|c| c == 'h' || c == 't')
}
/// The characters of a format outside its 'quoted literals'.
fn unquoted(format: &str) -> impl Iterator<Item = char> + '_ {
    let mut quoted = false;
    format.chars().filter(move |&c| {
        if c == '\'' {
            quoted = !quoted;
            return false;
        }
        !quoted
    })
}
/// The same format with 24 hours: "h:mm tt" → "HH:mm", "tt h:mm" → "HH:mm".
pub fn to_24h(format: &str) -> String {
    if format.contains('\'') {
        // Literals would need care; the plain form is right everywhere.
        return if format.matches(':').count() >= 2 || format.contains('s') { "HH:mm:ss".into() } else { "HH:mm".into() };
    }
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'h' | 'H' => {
                while chars.peek().is_some_and(|&n| n == 'h' || n == 'H') {
                    chars.next();
                }
                out.push_str("HH");
            }
            't' => {
                while chars.peek() == Some(&'t') {
                    chars.next();
                }
            }
            other => out.push(other),
        }
    }
    out.trim().to_string()
}
/// The same format with 12 hours: "HH:mm" → "h:mm tt", "HH:mm:ss" → "h:mm:ss tt".
pub fn to_12h(format: &str) -> String {
    if !is_24h(format) {
        return format.to_string();
    }
    if format.contains('\'') {
        return if format.matches(':').count() >= 2 || format.contains('s') { "h:mm:ss tt".into() } else { "h:mm tt".into() };
    }
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'H' | 'h' => {
                while chars.peek().is_some_and(|&n| n == 'H' || n == 'h') {
                    chars.next();
                }
                out.push('h');
            }
            other => out.push(other),
        }
    }
    format!("{} tt", out.trim())
}
// --------------------------------------------------------------------------- Store apps and Edge
/// Names of the Store packages installed for this user (from the registry,
/// which answers in milliseconds where PowerShell takes seconds).
pub fn installed_packages() -> Vec<String> {
    let path = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(path, KEY_READ) else {
        return Vec::new();
    };
    let mut names: Vec<String> = key
        .enum_keys()
        .flatten()
        // Name_Version_Architecture_ResourceId_PublisherId
        .filter_map(|full| full.split('_').next().map(str::to_string))
        .collect();
    names.sort_unstable_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}
/// Whether a part of Windows (a Feature on Demand) is installed, from the
/// servicing store, which any user may read: `None` when this Windows has
/// no such package at all. `package` is the start of its package names
/// ("Microsoft-Windows-StepsRecorder-Package~"); only the neutral ones count
/// ("...~amd64~~10.0..."), not their language parts.
pub fn capability_installed(package: &str) -> Option<bool> {
    /// Installed, or installed once Windows restarts.
    const INSTALLED: [u32; 3] = [0x60, 0x65, 0x70];
    let packages = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\Packages", KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    let mut seen = false;
    for name in packages.enum_keys().flatten() {
        if !name.starts_with(package) || !name.contains("~~") {
            continue;
        }
        seen = true;
        let state = packages.open_subkey_with_flags(&name, KEY_READ).and_then(|key| key.get_value::<u32, _>("CurrentState"));
        if state.is_ok_and(|state| INSTALLED.contains(&state)) {
            return Some(true);
        }
    }
    seen.then_some(false)
}
pub fn program_files_x86() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramFilesX86)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
}
pub fn windows_dir() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Windows).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}
pub(crate) fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let mut raw = std::ptr::null_mut();
    // SAFETY: `raw` receives a CoTaskMemAlloc'd string, freed below.
    let status = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut raw) };
    if status < 0 || raw.is_null() {
        return None;
    }
    let mut length = 0usize;
    // SAFETY: the string is NUL-terminated.
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: `length` units were just counted.
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(raw, length) }));
    // SAFETY: allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(raw.cast()) };
    Some(path)
}
pub fn edge_installed() -> bool {
    program_files_x86()
        .join(r"Microsoft\Edge\Application\msedge.exe")
        .is_file()
}
// --------------------------------------------------------------------------- Telling Windows
/// Tells running programs that settings changed (`area`: "intl",
/// "TraySettings", "Policy", ...).
pub fn broadcast_setting_change(area: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let area: Vec<u16> = area.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `area` is NUL-terminated and outlives the call.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            std::ptr::null_mut(),
        );
    }
}
/// Restarts the user's Explorer (the taskbar and desktop), as the user: it
/// must never be started from the administrator helper.
pub fn restart_explorer() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system32 = windows_dir().join("System32");
    let _ = std::process::Command::new(system32.join("taskkill.exe"))
        .args(["/f", "/im", "explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    // Windows usually starts it again by itself; if not, start it.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if explorer_running() {
            return Ok(());
        }
    }
    std::process::Command::new(windows_dir().join("explorer.exe"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Explorer could not be started again: {e}"))
}
fn explorer_running() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    let mut mine = 0u32;
    // SAFETY: plain calls; the snapshot handle is closed below.
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut mine);
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return true;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        let mut found = false;
        while more && !found {
            let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case("explorer.exe") {
                let mut session = u32::MAX;
                ProcessIdToSessionId(entry.th32ProcessID, &mut session);
                found = session == mine;
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Read as the user, and fast enough for the page: the servicing store
    /// has thousands of packages.
    #[test]
    fn parts_of_windows_are_read_without_rights_and_quickly() {
        let started = std::time::Instant::now();
        assert_eq!(capability_installed("Microsoft-Windows-NoSuchThing-Package~"), None);
        for tweak in super::super::catalog::TWEAKS {
            for op in tweak.ops {
                if let super::super::catalog::Op::Capability { package, .. } = op {
                    println!("{:<22} {:?}", tweak.id, capability_installed(package));
                }
            }
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
        println!("{:?}", started.elapsed());
    }
    /// A key of this test's own under HKCU, removed afterwards.
    struct TestKey(String);
    impl TestKey {
        fn new() -> Self {
            let path = format!(r"Software\ThomasThanos\MYLE-tests\{}", uuid::Uuid::new_v4().simple());
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            let user = RegKey::predef(HKEY_CURRENT_USER);
            let _ = user.delete_subkey_all(&self.0);
            // The shared parent too, once no other test uses it.
            let _ = user.delete_subkey(r"Software\ThomasThanos\MYLE-tests");
        }
    }
    #[test]
    fn values_are_read_written_and_put_back_exactly() {
        let key = TestKey::new();
        let path = format!(r"{}\Policies\Deep", key.0);
        assert_eq!(read(Hive::User, &path, "Level"), Value::Absent);
        let created = first_missing(Hive::User, &path);
        assert_eq!(created, Some(format!(r"{}\Policies", key.0)));
        write(Hive::User, &path, "Level", Data::Dword(0)).unwrap();
        assert!(read(Hive::User, &path, "Level").matches(Data::Dword(0)));
        restore(Hive::User, &path, "Level", &Value::Absent, created.as_deref()).unwrap();
        assert_eq!(read(Hive::User, &path, "Level"), Value::Absent);
        assert!(first_missing(Hive::User, &path).is_some(), "the keys made for it are gone");
        write(Hive::User, &path, "Mode", Data::Sz("Allow")).unwrap();
        let before = read(Hive::User, &path, "Mode");
        write(Hive::User, &path, "Mode", Data::Sz("Deny")).unwrap();
        restore(Hive::User, &path, "Mode", &before, None).unwrap();
        assert_eq!(read(Hive::User, &path, "Mode"), Value::Sz { value: "Allow".into() });
    }
    #[test]
    fn a_default_value_and_its_keys_come_and_go() {
        let key = TestKey::new();
        let path = format!(r"{}\CLSID\{{test}}\InprocServer32", key.0);
        let created = first_missing(Hive::User, &path);
        write(Hive::User, &path, "", Data::Sz("")).unwrap();
        assert!(read(Hive::User, &path, "").matches(Data::Sz("")));
        restore(Hive::User, &path, "", &Value::Absent, created.as_deref()).unwrap();
        assert!(first_missing(Hive::User, &path).is_some());
    }
    #[test]
    fn undo_removes_only_keys_on_the_way_to_the_value() {
        let path = r"Software\Policies\Microsoft\Windows\Explorer";
        assert!(key_on_path(path, r"Software\Policies\Microsoft\Windows\Explorer"));
        assert!(key_on_path(path, r"software\policies\microsoft"));
        assert!(!key_on_path(path, "Software"), "never the hive's first level");
        assert!(!key_on_path(path, r"Software\Policies\Micro"), "whole key names only");
        assert!(!key_on_path(path, r"Software\Classes"));
    }
    #[test]
    fn time_formats_turn_to_24_hours() {
        assert_eq!(to_24h("h:mm tt"), "HH:mm");
        assert_eq!(to_24h("hh:mm:ss tt"), "HH:mm:ss");
        assert_eq!(to_24h("tt h:mm"), "HH:mm");
        assert_eq!(to_24h("H.mm"), "HH.mm");
        assert!(is_24h("HH:mm") && is_24h("H:mm:ss"));
        assert!(!is_24h("h:mm tt"));
        assert!(is_24h("HH'h'mm"), "a quoted h is a literal");
    }
    #[test]
    fn this_pc_answers() {
        let info = windows_info();
        assert!(info.build >= 10240, "{info:?}");
        assert!(info.name.starts_with("Windows 1"));
        // A service every Windows has, and one no Windows has.
        assert!(start_type("EventLog").unwrap().is_some());
        assert_eq!(start_type("MyleNoSuchService").unwrap(), None);
        assert_eq!(task_enabled(r"\Microsoft\Windows\NoSuchFolder", "Nope").unwrap(), None);
        let (short, long) = time_formats().unwrap();
        assert!(!short.is_empty() && !long.is_empty());
        assert!(!installed_packages().is_empty());
    }
}
