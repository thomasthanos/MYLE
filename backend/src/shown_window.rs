//! Whether a program has a window the user can actually see: the moment an
//! update can hand over from one program to the next without a gap.
//!
//! Shared by the app (`updater`) and the setup (`installer/src/processes.rs`).
//!
//! "Visible" (`IsWindowVisible`) is not enough. Tao, the windowing library
//! under Tauri, creates a hidden helper window as soon as its event loop
//! starts, long before any page is painted, and gives it `WS_VISIBLE` (it
//! needs paint messages) while hiding it with `WS_EX_LAYERED` and
//! `WS_EX_TOOLWINDOW`, no title and a 0 x 0 size. The tray icon's message
//! window is the same kind. Counting those, the previous version quit a
//! fraction of a second after starting the new one, which then took a few
//! seconds (WebView2 starting, the splash loading) to show anything.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};

/// How a wait for a program's first window ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// A real window of it is on screen.
    Shown,
    /// It exited without ever showing one.
    Exited,
    /// Still running, nothing on screen yet.
    TimedOut,
}

/// Whether a top-level window that Windows calls visible is one a person
/// sees: a titled, non-tool window with an area. Every window the app and
/// the setup open has a title ("MYLE", "MYLE Setup").
pub fn counts_as_shown(ex_style: u32, title_len: i32, width: i32, height: i32) -> bool {
    ex_style & WS_EX_TOOLWINDOW == 0 && title_len > 0 && width > 0 && height > 0
}

fn is_shown(window: HWND) -> bool {
    // SAFETY: plain queries on a window handle from EnumWindows; a window
    // that is gone by now makes them fail, which reads as "not shown".
    unsafe {
        if IsWindowVisible(window) == 0 {
            return false;
        }
        let ex_style = GetWindowLongW(window, GWL_EXSTYLE) as u32;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(window, &mut rect) == 0 {
            return false;
        }
        counts_as_shown(
            ex_style,
            GetWindowTextLengthW(window),
            rect.right - rect.left,
            rect.bottom - rect.top,
        )
    }
}

/// Whether `pid` has a window on screen right now.
pub fn has_shown_window(pid: u32) -> bool {
    struct Search {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn visit(window: HWND, search: LPARAM) -> i32 {
        // SAFETY: `search` is the struct passed to EnumWindows below.
        let search = unsafe { &mut *(search as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == search.pid && is_shown(window) {
            search.found = true;
            return 0; // stop
        }
        1
    }
    let mut search = Search { pid, found: false };
    // SAFETY: `search` outlives the synchronous enumeration.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    search.found
}

/// Waits until `pid` shows a real window, exits, or `timeout` passes. The
/// app's windows show themselves only once they have painted, so `Shown` is
/// the moment it can be seen.
pub fn wait_for_window(pid: u32, timeout: Duration) -> Wait {
    // SAFETY: the handle is closed before returning.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return Wait::Exited;
    }
    let deadline = Instant::now() + timeout;
    let outcome = loop {
        if has_shown_window(pid) {
            break Wait::Shown;
        }
        if unsafe { WaitForSingleObject(process, 50) } == WAIT_OBJECT_0 {
            break Wait::Exited;
        }
        if Instant::now() >= deadline {
            break Wait::TimedOut;
        }
    };
    unsafe { CloseHandle(process) };
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };

    #[test]
    fn the_hidden_helper_windows_do_not_count() {
        // Tao's event-loop window and the tray icon's: visible by style,
        // but tool windows with no title and no size.
        let helper = WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW;
        assert!(!counts_as_shown(helper, 0, 0, 0));
        assert!(!counts_as_shown(helper, 4, 300, 380));
        assert!(!counts_as_shown(0, 0, 300, 380));
        assert!(!counts_as_shown(0, 4, 0, 0));
    }

    #[test]
    fn the_splash_and_the_main_window_count() {
        // The splash: titled, sized, a normal window.
        assert!(counts_as_shown(0, 4, 300, 380));
        // The main window is transparent, which may make it layered.
        assert!(counts_as_shown(WS_EX_LAYERED, 4, 1280, 720));
    }

    #[test]
    fn a_program_that_exits_without_a_window_is_not_waited_for() {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut child = std::process::Command::new(std::path::PathBuf::from(system).join(r"System32\cmd.exe"))
            .args(["/c", "exit"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let outcome = wait_for_window(child.id(), Duration::from_secs(10));
        assert_eq!(outcome, Wait::Exited);
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = child.wait();
    }

    /// Real windows: one made the way tao makes its event-loop window is
    /// "visible" to Windows but not shown; a titled, sized one is.
    #[test]
    fn a_tao_style_helper_window_is_not_mistaken_for_the_splash() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
        };
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let class = wide("STATIC");
        let title = wide("MYLE");
        let pid = std::process::id();
        // The only test that makes windows, so nothing else is on screen.
        assert!(!has_shown_window(pid));
        // SAFETY: plain window creation on this thread; both are destroyed below.
        unsafe {
            let helper = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                std::ptr::null(),
                WS_VISIBLE | WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!helper.is_null());
            assert_ne!(IsWindowVisible(helper), 0, "Windows calls it visible");
            assert!(!has_shown_window(pid), "but nobody can see it");

            let splash = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_VISIBLE | WS_OVERLAPPEDWINDOW,
                100,
                100,
                300,
                380,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!splash.is_null());
            assert!(has_shown_window(pid));
            DestroyWindow(splash);
            DestroyWindow(helper);
        }
        assert!(!has_shown_window(pid));
    }
}
