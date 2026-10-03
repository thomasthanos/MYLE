//! User-invoked filling into a Windows program. The hotkey (Ctrl+Shift+L, or
//! Ctrl+Alt+Shift+L when another program holds that) remembers the program
//! in front and the field focused in it; the vault page lets the user pick a
//! linked login and what to type there: the user name, the password, or
//! both with Tab between.
//!
//! A login is linked to a program by its full path, so another program that
//! takes the same file name never gets it. A program that updates into a new
//! version folder (Discord's `app-1.0.9172`) stays linked.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, SendInput, UnregisterHotKey, VK_A, VK_CONTROL, VK_L, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetMessageW, GetWindowThreadProcessId,
    IsIconic, IsWindow, MSG, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_HOTKEY,
};
use zeroize::Zeroizing;

use super::{PasswordsState, browser};

const HOTKEY_ID: i32 = 0x4d59;
/// Long enough to unlock the vault first.
const TARGET_LIFETIME: Duration = Duration::from_secs(180);
pub const TARGET_EVENT: &str = "passwords-windows-target";
const BROWSERS: [&str; 4] = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"];
/// The window Store apps run in belongs to this program, not to the app.
const STORE_APP_HOST: &str = "applicationframehost.exe";
/// The hotkeys tried, in order.
const HOTKEYS: [(u32, &str); 2] = [
    (MOD_CONTROL | MOD_SHIFT, "Ctrl+Shift+L"),
    (MOD_CONTROL | MOD_ALT | MOD_SHIFT, "Ctrl+Alt+Shift+L"),
];

const HOTKEY_PENDING: u8 = u8::MAX;
const HOTKEY_TAKEN: u8 = u8::MAX - 1;

fn linked_exe(value: &str) -> &str {
    value.rsplit(['\\', '/']).next().unwrap_or(value).trim_matches('"')
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
}

/// A folder named like a version: `1.2.3`, `v2.0`, `app-1.0.9172`.
fn is_version(segment: &str) -> bool {
    let rest = segment
        .strip_prefix("app-")
        .or_else(|| segment.strip_prefix('v'))
        .unwrap_or(segment);
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The program's path with its version folders made alike.
fn program_key(path: &Path) -> String {
    path.components()
        .map(|part| {
            let part = part.as_os_str().to_string_lossy().to_lowercase();
            if is_version(&part) { "*".to_string() } else { part }
        })
        .collect::<Vec<_>>()
        .join("\\")
}

/// Whether a login's linked program is the program at `path`: the same
/// file, or the same program in another version folder.
fn same_program(linked: &Path, path: &Path) -> bool {
    linked.is_absolute() && (same_path(linked, path) || program_key(linked) == program_key(path))
}

fn focused_control(thread: u32) -> Option<usize> {
    let mut info = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
    if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 || info.hwndFocus.is_null() {
        return None;
    }
    Some(info.hwndFocus as usize)
}

/// Whether `pid` runs with administrator rights (and this app does not):
/// Windows then drops the keys MYLE would type into it.
fn runs_above_us(pid: u32) -> bool {
    fn elevated(process: HANDLE) -> Option<bool> {
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut size = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&raw mut elevation).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            (ok != 0).then_some(elevation.TokenIsElevated != 0)
        }
    }
    let me = elevated(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() }).unwrap_or(false);
    if me {
        return false;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        // A token this app may not even read belongs to a higher program.
        let theirs = elevated(process).unwrap_or(true);
        CloseHandle(process);
        theirs
    }
}

/// A Store app's window belongs to ApplicationFrameHost.exe; the app itself
/// is the process of a window inside it.
fn store_app_inside(window: HWND, host: u32) -> Option<(u32, u32)> {
    struct Search {
        host: u32,
        found: Option<(u32, u32)>,
    }
    unsafe extern "system" fn visit(child: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut Search) };
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(child, &mut pid) };
        if pid != 0 && pid != search.host {
            search.found = Some((pid, thread));
            return 0;
        }
        1
    }
    let mut search = Search { host, found: None };
    unsafe { EnumChildWindows(window, Some(visit), (&raw mut search) as LPARAM) };
    search.found
}

/// The program a window shows: its process, the thread that owns its
/// focus, and its file.
fn program_of(window: HWND) -> Option<(u32, u32, PathBuf)> {
    let mut pid = 0;
    let mut thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    let mut path = browser::process_path(pid)?;
    if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case(STORE_APP_HOST))
        && let Some((app, app_thread)) = store_app_inside(window, pid)
    {
        pid = app;
        thread = app_thread;
        path = browser::process_path(pid)?;
    }
    Some((pid, thread, path))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTargetInfo {
    exe: String,
    path: String,
    /// It runs as administrator: Windows does not let MYLE type into it.
    elevated: bool,
}

#[derive(Clone)]
struct Target {
    window: usize,
    pid: u32,
    thread: u32,
    focus: Option<usize>,
    exe: String,
    path: PathBuf,
    elevated: bool,
    at: Instant,
}

#[derive(Clone)]
pub struct WindowsFillState {
    target: Arc<Mutex<Option<Target>>>,
    /// Which of `HOTKEYS` is registered, or pending or taken.
    hotkey: Arc<AtomicU8>,
}

impl Default for WindowsFillState {
    fn default() -> Self {
        Self { target: Arc::default(), hotkey: Arc::new(AtomicU8::new(HOTKEY_PENDING)) }
    }
}

impl WindowsFillState {
    fn hotkey_label(&self) -> &'static str {
        HOTKEYS
            .get(usize::from(self.hotkey.load(Ordering::Acquire)))
            .map_or("the hotkey", |(_, label)| label)
    }
}

pub fn start(app: AppHandle, target: WindowsFillState) {
    std::thread::spawn(move || unsafe {
        let registered = HOTKEYS
            .iter()
            .position(|(modifiers, _)| RegisterHotKey(null_mut(), HOTKEY_ID, modifiers | MOD_NOREPEAT, VK_L as u32) != 0);
        let Some(index) = registered else {
            target.hotkey.store(HOTKEY_TAKEN, Ordering::Release);
            return;
        };
        target.hotkey.store(index as u8, Ordering::Release);
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message != WM_HOTKEY || message.wParam != HOTKEY_ID as usize {
                continue;
            }
            let window = GetForegroundWindow();
            if window.is_null() {
                continue;
            }
            let Some((pid, thread, path)) = program_of(window) else { continue };
            let Some(exe) = path.file_name().and_then(|name| name.to_str()).map(str::to_lowercase) else { continue };
            if pid == std::process::id() || BROWSERS.contains(&exe.as_str()) {
                continue;
            }
            let elevated = runs_above_us(pid);
            *target.target.lock().unwrap_or_else(|p| p.into_inner()) = Some(Target {
                window: window as usize,
                pid,
                thread,
                focus: focused_control(thread),
                exe: exe.clone(),
                path: path.clone(),
                elevated,
                at: Instant::now(),
            });
            let _ = app.emit(TARGET_EVENT, WindowsTargetInfo {
                exe,
                path: path.to_string_lossy().into_owned(),
                elevated,
            });
            browser::open_vault(&app);
        }
        target.hotkey.store(HOTKEY_TAKEN, Ordering::Release);
        UnregisterHotKey(null_mut(), HOTKEY_ID);
    });
}

/// The hotkey in use ("Ctrl+Shift+L"), or `None` when other programs hold
/// both. Waits a moment for a start that is still registering it.
#[tauri::command(async)]
pub fn passwords_windows_hotkey_status(target: State<'_, WindowsFillState>) -> Option<&'static str> {
    let until = Instant::now() + Duration::from_secs(2);
    while target.hotkey.load(Ordering::Acquire) == HOTKEY_PENDING && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let index = usize::from(target.hotkey.load(Ordering::Acquire));
    HOTKEYS.get(index).map(|(_, label)| *label)
}

/// A login linked to the program the hotkey was pressed in.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMatch {
    id: String,
    /// Linked by path (this version or another); else by file name only,
    /// which is not enough to fill.
    exact: bool,
}

/// The logins linked to that program.
#[tauri::command(async)]
pub fn passwords_windows_matches(
    target: State<'_, WindowsFillState>,
    state: State<'_, PasswordsState>,
) -> Result<Vec<ProgramMatch>, String> {
    let Some(selected) = target.target.lock().unwrap_or_else(|p| p.into_inner()).clone() else {
        return Ok(Vec::new());
    };
    let summaries = state.with_quiet(|vault| vault.summaries())?;
    Ok(summaries
        .into_iter()
        .filter_map(|entry| {
            let exact = entry.apps.iter().any(|app| same_program(Path::new(app.exe.trim_matches('"')), &selected.path));
            let by_name = entry.apps.iter().any(|app| {
                let linked = app.exe.trim_matches('"');
                !Path::new(linked).is_absolute() && linked_exe(linked).eq_ignore_ascii_case(&selected.exe)
            });
            (exact || by_name).then_some(ProgramMatch { id: entry.id, exact })
        })
        .collect())
}

/// Links login `id` to the program the hotkey was pressed in, by its full
/// path (in place of a link by file name, or to another copy).
#[tauri::command(async)]
pub fn passwords_windows_link(
    target: State<'_, WindowsFillState>,
    state: State<'_, PasswordsState>,
    id: String,
) -> Result<(), String> {
    let selected = target
        .target
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| format!("Press {} in the program again.", target.hotkey_label()))?;
    let name = selected.path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    state.with(|vault| vault.link_program(&id, &selected.path.to_string_lossy(), &name))
}

/// What the page fills: the user name, the password, or both with Tab
/// between (most sign-in windows: user name, then password).
#[tauri::command(async)]
pub fn passwords_windows_fill(
    target: State<'_, WindowsFillState>,
    state: State<'_, PasswordsState>,
    id: String,
    field: String,
) -> Result<(), String> {
    if !matches!(field.as_str(), "username" | "password" | "both") {
        return Err("Choose a user name or password.".into());
    }
    let hotkey = target.hotkey_label();
    let selected = target
        .target
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| format!("Press {hotkey} in the program again."))?;
    if selected.at.elapsed() > TARGET_LIFETIME {
        return Err(format!("The program selection expired. Press {hotkey} there again."));
    }
    let window = selected.window as HWND;
    let still_there = unsafe { IsWindow(window) } != 0
        && program_of(window).is_some_and(|(pid, thread, path)| {
            pid == selected.pid && thread == selected.thread && same_path(&path, &selected.path)
        });
    if !still_there {
        return Err(format!("That program window has closed. Press {hotkey} again."));
    }
    if selected.elevated {
        return Err("That program runs as administrator, so Windows does not let MYLE type into it. Copy the user name and password instead.".into());
    }

    // The link is checked again now: a stale choice on the page cannot type a
    // login into another program.
    let (username, password) = state.with(|vault| {
        let entry = vault
            .summaries()?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or("That login no longer exists.")?;
        if !entry.apps.iter().any(|app| same_program(Path::new(app.exe.trim_matches('"')), &selected.path)) {
            let by_name = entry.apps.iter().any(|app| linked_exe(&app.exe).eq_ignore_ascii_case(&selected.exe));
            return Err(if by_name {
                "This login is linked by file name only. Link it to the program shown above."
            } else {
                "This login is not linked to that program."
            }
            .into());
        }
        let username = Zeroizing::new(vault.username(&id)?);
        let password = if field == "username" { Zeroizing::new(String::new()) } else { vault.password(&id)? };
        Ok((username, password))
    })?;
    match field.as_str() {
        "username" if username.is_empty() => return Err("This login has no user name.".into()),
        "password" if password.is_empty() => return Err("This login has no password.".into()),
        "both" if username.is_empty() || password.is_empty() => {
            return Err("This login does not have both a user name and a password: fill the one it has.".into());
        }
        _ => {}
    }

    if !bring_forward(window) {
        return Err("Windows could not bring that program forward. Click its field and try again.".into());
    }
    // A program whose fields Windows can see: still the field it had when the
    // hotkey was pressed. (Store apps and some others draw their own fields:
    // then only the window is checked.)
    let focus = selected.focus;
    if focus.is_some() && focused_control(selected.thread) != focus {
        return Err(format!("The focused field changed. Click the intended field and press {hotkey} again."));
    }
    // Each value takes the place of what the field holds (Steam keeps the
    // last account name in its field: typing after it made a wrong one).
    let typing = |text: &str, focus: Option<usize>| {
        select_all()?;
        type_text(text, window, selected.thread, focus, hotkey)
    };
    // Where Windows can tell what the focused field is (UI Automation), it
    // is used: never a password typed where it shows, and "both" from the
    // user name field even when the password field was clicked.
    let password_field = focused_field_is_password();
    match field.as_str() {
        "username" => {
            if password_field == Some(true) {
                return Err("The field you clicked is a password field. Click the user name field, or choose Password.".into());
            }
            typing(&username, focus)?;
        }
        "password" => {
            if password_field == Some(false) {
                return Err(format!(
                    "The field you clicked shows what is typed in it. Click the password field and press {hotkey} again, or copy the password."
                ));
            }
            typing(&password, focus)?;
        }
        _ => {
            if password_field == Some(true) {
                press_with(VK_SHIFT, VK_TAB)?;
                std::thread::sleep(Duration::from_millis(120));
                if focused_field_is_password() == Some(true) {
                    return Err(format!("Click the user name field, then press {hotkey} again."));
                }
            }
            typing(&username, focused_control(selected.thread).or(focus))?;
            press(VK_TAB)?;
            std::thread::sleep(Duration::from_millis(120));
            typing(&password, focused_control(selected.thread))?;
        }
    }
    *target.target.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(())
}

/// Brings `window` forward (restored if minimized), as long as it takes.
fn bring_forward(window: HWND) -> bool {
    unsafe {
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window);
        for _ in 0..15 {
            if GetForegroundWindow() == window {
                // Let the program give its field the focus back (web-based
                // programs like Steam take a moment).
                std::thread::sleep(Duration::from_millis(150));
                return true;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    false
}

fn key(virtual_key: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: virtual_key, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    if unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) } != inputs.len() as u32 {
        return Err("Windows blocked typing into that program. Copy the field instead.".into());
    }
    Ok(())
}

fn press(virtual_key: u16) -> Result<(), String> {
    send(&[key(virtual_key, 0, 0), key(virtual_key, 0, KEYEVENTF_KEYUP)])
}

/// A key with a modifier held (Shift+Tab, Ctrl+A).
fn press_with(modifier: u16, virtual_key: u16) -> Result<(), String> {
    send(&[
        key(modifier, 0, 0),
        key(virtual_key, 0, 0),
        key(virtual_key, 0, KEYEVENTF_KEYUP),
        key(modifier, 0, KEYEVENTF_KEYUP),
    ])
}

/// Selects what the focused field holds, so the typing replaces it.
fn select_all() -> Result<(), String> {
    press_with(VK_CONTROL, VK_A)
}

/// What UI Automation says the focused field is: `Some(true)` a password
/// field, `Some(false)` a text field that shows what is typed, `None` when
/// the program does not say (many draw their own fields).
fn focused_field_is_password() -> Option<bool> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, UIA_EditControlTypeId};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        if element.CurrentControlType().ok()? != UIA_EditControlTypeId {
            return None;
        }
        Some(element.CurrentIsPassword().ok()?.as_bool())
    }
}

fn type_text(text: &str, window: *mut c_void, thread: u32, focus: Option<usize>, hotkey: &str) -> Result<(), String> {
    for unit in text.encode_utf16() {
        if unsafe { GetForegroundWindow() } != window || (focus.is_some() && focused_control(thread) != focus) {
            return Err(format!("The focused field changed while filling. Click it and press {hotkey} again."));
        }
        let mut inputs = [key(0, unit, KEYEVENTF_UNICODE), key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)];
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        // The scan code carries one UTF-16 unit of the secret.
        unsafe { std::ptr::write_volatile(&mut inputs, std::mem::zeroed()) };
        if sent != 2 {
            return Err("Windows blocked typing into that program. Copy the field instead.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_updates_into_a_new_folder_stays_linked() {
        let discord = Path::new(r"C:\Users\Me\AppData\Local\Discord\app-1.0.9172\Discord.exe");
        assert!(same_program(Path::new(r"C:\Users\Me\AppData\Local\Discord\app-1.0.9165\Discord.exe"), discord));
        assert!(same_program(Path::new(r"c:\users\me\appdata\local\discord\APP-1.0.9172\discord.exe"), discord));
        let versioned = Path::new(r"C:\Program Files\Tool\2.4.1\tool.exe");
        assert!(same_program(Path::new(r"C:\Program Files\Tool\v2.5\tool.exe"), versioned));
        // Another folder, another file, or only a name: not the same program.
        assert!(!same_program(Path::new(r"C:\Users\Me\Downloads\app-1.0.9172\Discord.exe"), discord));
        assert!(!same_program(Path::new(r"C:\Users\Me\AppData\Local\Discord\app-1.0.9172\Update.exe"), discord));
        assert!(!same_program(Path::new("Discord.exe"), discord));
        assert!(!is_version("Discord") && !is_version("2024") && !is_version("v") && !is_version("1..2"));
        assert!(is_version("1.0") && is_version("app-1.0.9172") && is_version("v10.2.3"));
    }

    #[test]
    fn this_process_is_not_above_itself() {
        assert!(!runs_above_us(std::process::id()));
    }
}
