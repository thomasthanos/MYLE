//! Finds the programs running out of the install folder (the app, its
//! bundled Ludusavi, a scheduled Game Saves run) and closes them: politely
//! first, so the app can save its state, then by force.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    EVENT_MODIFY_STATE, GetCurrentProcessId, OpenEventW, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, SetEvent, TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

use crate::shell;

#[derive(Clone, Debug)]
pub struct Running {
    pub pid: u32,
    pub path: PathBuf,
}

/// The uninstaller in the install folder that started this copy of it and
/// waits for it to finish (see `relocate`): never closed, and its file is
/// removed only once it has exited.
static SPARED: OnceLock<u32> = OnceLock::new();

pub fn spare(pid: u32) {
    let _ = SPARED.set(pid);
}

/// The spared process's executable, while it runs.
pub fn spared_exe() -> Option<PathBuf> {
    SPARED.get().copied().and_then(image_path)
}

/// A process to wait for, opened early so that its ID cannot be reused by
/// another process in the meantime.
pub struct Process(HANDLE);

impl Process {
    pub fn open(pid: u32) -> Option<Self> {
        // SAFETY: the handle is closed on drop.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        (!handle.is_null()).then_some(Self(handle))
    }

    /// Whether it exited within `timeout`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        unsafe { WaitForSingleObject(self.0, millis) == WAIT_OBJECT_0 }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Every process whose executable lives in `dir`, except this one and the
/// spared one.
pub fn running_in(dir: &Path) -> Vec<Running> {
    // Windows reports long names; the folder may be spelled with short ones.
    let dir = shell::real_path(dir);
    let own = unsafe { GetCurrentProcessId() };
    let spared = SPARED.get().copied();
    // SAFETY: a snapshot handle, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let pid = entry.th32ProcessID;
        if pid != own
            && pid != 0
            && Some(pid) != spared
            && let Some(path) = image_path(pid)
            && shell::is_within(&path, &dir)
        {
            found.push(Running { pid, path });
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    found
}

fn image_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: the handle is closed before returning.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(process) };
    (ok != 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..length as usize])))
}

/// The event the app waits on to quit (`backend/src/tray.rs`). Closing its
/// window may only hide it to the tray, so the setup asks through this first.
fn quit_event_name() -> Vec<u16> {
    format!(r"Local\{}-Quit", crate::product::LEGACY_BINARY)
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

/// Tells a running app (version 8.2 and later) to quit.
fn signal_quit() {
    let name = quit_event_name();
    // SAFETY: the handle is closed right after use.
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if !event.is_null() {
            SetEvent(event);
            CloseHandle(event);
        }
    }
}

/// Asks each process's windows to close, as the title bar's close button
/// would. The app shuts down cleanly when its window closes; one that keeps
/// running in the tray is told to quit first.
fn ask_to_close(pids: &[u32]) {
    signal_quit();
    unsafe extern "system" fn visit(window: HWND, targets: LPARAM) -> i32 {
        // SAFETY: `targets` is the slice passed to EnumWindows below.
        let targets = unsafe { &*(targets as *const Vec<u32>) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if targets.contains(&pid) {
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
        1
    }
    let targets: Vec<u32> = pids.to_vec();
    // SAFETY: `targets` outlives the synchronous enumeration.
    unsafe { EnumWindows(Some(visit), &targets as *const Vec<u32> as LPARAM) };
}

/// Waits until `pid` has a window on screen, for at most `timeout`. The
/// app's windows show themselves only once they have painted, so this is the
/// moment it can be seen. False if it never did, or exited first. Tao's
/// hidden helper windows do not count (see `shown_window`).
pub fn wait_for_window(pid: u32, timeout: Duration) -> bool {
    crate::shown_window::wait_for_window(pid, timeout) == crate::shown_window::Wait::Shown
}

fn wait_exit(pid: u32, timeout: Duration) -> bool {
    // SAFETY: the handle is closed before returning.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return true; // already gone
    }
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let exited = unsafe { WaitForSingleObject(process, millis) } == WAIT_OBJECT_0;
    unsafe { CloseHandle(process) };
    exited
}

fn terminate(pid: u32) {
    // SAFETY: the handle is closed before returning.
    let process = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return;
    }
    unsafe {
        TerminateProcess(process, 1);
        WaitForSingleObject(process, 5_000);
        CloseHandle(process);
    }
}

/// Closes everything running from `dir`. `grace` is how long the programs
/// get to exit on their own first: the in-app updater quits by itself right
/// after starting the setup. Returns what could not be stopped.
pub fn close_all(dir: &Path, grace: Duration) -> Vec<Running> {
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline && !running_in(dir).is_empty() {
        std::thread::sleep(Duration::from_millis(250));
    }

    let running = running_in(dir);
    if running.is_empty() {
        return running;
    }
    let pids: Vec<u32> = running.iter().map(|process| process.pid).collect();
    ask_to_close(&pids);
    let polite = Instant::now() + Duration::from_secs(8);
    for process in &running {
        let left = polite.saturating_duration_since(Instant::now());
        wait_exit(process.pid, left);
    }

    // Whatever is left (a window that asked "are you sure", Ludusavi
    // mid-backup, a headless scheduled run) is stopped.
    for process in running_in(dir) {
        terminate(process.pid);
    }
    running_in(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quit_event_is_the_one_the_app_waits_on() {
        let name = quit_event_name();
        let text = String::from_utf16(&name[..name.len() - 1]).unwrap();
        // backend/src/tray.rs waits on the same name.
        assert_eq!(text, r"Local\MakeYourLifeEasier-Quit");
    }

    #[test]
    fn waiting_for_a_window_ends_when_the_program_exits_without_one() {
        let mut child = std::process::Command::new(shell::system32("cmd.exe"))
            .args(["/c", "exit"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        assert!(!wait_for_window(child.id(), Duration::from_secs(10)));
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = child.wait();
    }
}
