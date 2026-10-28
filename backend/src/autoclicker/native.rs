use super::{
    AutoClickerState,
    engine::{Cancel, Driver},
    model::{ClickType, MouseButton, Point},
};
use std::{
    cell::RefCell,
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint},
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT, MOUSEEVENTF_LEFTDOWN,
            MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
            MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, RegisterHotKey, SendInput,
            UnregisterHotKey, VK_ESCAPE, VK_F6, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor, GetCursorPos, GetMessageW,
            GetWindowThreadProcessId, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PM_REMOVE,
            PeekMessageW, SetCursorPos, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL,
            WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WindowFromPoint,
        },
    },
};
fn button_flags(button: MouseButton) -> (u32, u32, i32) {
    match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, VK_LBUTTON as i32),
        MouseButton::Right => (
            MOUSEEVENTF_RIGHTDOWN,
            MOUSEEVENTF_RIGHTUP,
            VK_RBUTTON as i32,
        ),
        MouseButton::Middle => (
            MOUSEEVENTF_MIDDLEDOWN,
            MOUSEEVENTF_MIDDLEUP,
            VK_MBUTTON as i32,
        ),
    }
}
fn mouse_input(flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
fn mouse_inputs(button: MouseButton, kind: ClickType) -> Vec<INPUT> {
    let (down, up, _) = button_flags(button);
    let count = if kind == ClickType::Double { 2 } else { 1 };
    (0..count)
        .flat_map(|_| [mouse_input(down), mouse_input(up)])
        .collect()
}
pub struct WindowsDriver;
impl Driver for WindowsDriver {
    fn click(
        &mut self,
        point: Option<Point>,
        button: MouseButton,
        kind: ClickType,
    ) -> Result<(), String> {
        if let Some(point) = point {
            // Refuse disconnected displays and clipped cursor positions rather than clicking elsewhere.
            let screen_point = POINT {
                x: point.x,
                y: point.y,
            };
            if unsafe { MonitorFromPoint(screen_point, MONITOR_DEFAULTTONULL) }.is_null() {
                return Err(format!(
                    "({}, {}) is outside the connected displays. Pick this location again.",
                    point.x, point.y
                ));
            }
            if unsafe { SetCursorPos(point.x, point.y) } == 0 {
                return Err(format!(
                    "Windows could not move the cursor: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let actual = cursor()?;
            if actual != point {
                return Err(
                    "Another program is restricting the cursor. Release it and try again.".into(),
                );
            }
        }
        let (_, up, virtual_key) = button_flags(button);
        if unsafe { GetAsyncKeyState(virtual_key) } < 0 {
            return Err("Release the mouse button before starting the clicker again.".into());
        }
        // Send the complete down/up pair(s) together: cancellation never splits an action.
        let inputs = mouse_inputs(button, kind);
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        if sent != inputs.len() as u32 {
            // A partial insertion can end on a down event. Always attempt the matching release.
            if sent % 2 == 1 {
                let release = mouse_input(up);
                unsafe {
                    SendInput(1, &release, size_of::<INPUT>() as i32);
                }
            }
            return Err("Windows blocked the click. The target may need the same administrator permissions as MYLE.".into());
        }
        Ok(())
    }
}
fn cursor() -> Result<Point, String> {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return Err("Windows could not read the cursor location.".into());
    }
    Ok(Point {
        x: point.x,
        y: point.y,
    })
}
/// The callback only captures data. All waits and UI notifications stay off the hook.
#[derive(Default)]
struct Capture {
    down: Option<Point>,
    result: Option<Point>,
    error: Option<String>,
}
impl Capture {
    fn mouse(&mut self, message: u32, point: Point, inside_myle: bool) -> bool {
        if message == WM_LBUTTONUP && self.down.is_some() {
            self.result = self.down.take();
            return true;
        }
        if message == WM_LBUTTONDOWN && !inside_myle && self.down.is_none() && self.result.is_none()
        {
            self.down = Some(point);
            return true;
        }
        false
    }
}
fn inside_myle(point: POINT) -> bool {
    let hit = unsafe { WindowFromPoint(point) };
    let mut process = 0;
    if hit.is_null() {
        return false;
    }
    // WebView2 child HWNDs can belong to the renderer process. Its root is MYLE's host window, which owns the Cancel control and must keep its clicks.
    let root = unsafe { GetAncestor(hit, GA_ROOT) };
    let window = if root.is_null() { hit } else { root };
    unsafe {
        GetWindowThreadProcessId(window, &mut process);
    }
    process == unsafe { GetCurrentProcessId() }
}
thread_local! { static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default()); }
unsafe extern "system" fn mouse_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && data != 0 {
        // SAFETY: Windows supplies an MSLLHOOKSTRUCT for this hook invocation.
        let event = unsafe { &*(data as *const MSLLHOOKSTRUCT) };
        if event.flags & LLMHF_INJECTED == 0 {
            let swallowed = CAPTURE.with(|capture| {
                let mut capture = capture.borrow_mut();
                let own_window = inside_myle(event.pt);
                let point = if message as u32 == WM_LBUTTONDOWN && !own_window {
                    match cursor() {
                        Ok(point) => point,
                        Err(error) => {
                            capture.error = Some(error);
                            return true;
                        }
                    }
                } else {
                    Point {
                        x: event.pt.x,
                        y: event.pt.y,
                    }
                };
                capture.mouse(message as u32, point, own_window)
            });
            if swallowed {
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, message, data) }
}
pub fn pick(cancel: &Cancel) -> Result<Option<Point>, String> {
    if cancel.stopped() {
        return Ok(None);
    }
    CAPTURE.with(|c| *c.borrow_mut() = Capture::default());
    let hook = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        return Err(format!(
            "Cannot start location picking: {}",
            std::io::Error::last_os_error()
        ));
    }
    struct Hook(windows_sys::Win32::UI::WindowsAndMessaging::HHOOK);
    impl Drop for Hook {
        fn drop(&mut self) {
            unsafe {
                UnhookWindowsHookEx(self.0);
            }
        }
    }
    let _hook = Hook(hook);
    let started = Instant::now();
    loop {
        if cancel.stopped() {
            return Ok(None);
        }
        let mut message = MSG::default();
        // A low-level hook is dispatched on the thread that installed it, through its message queue.
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                DispatchMessageW(&message);
            }
        }
        if let Some(result) = CAPTURE.with(|c| c.borrow_mut().result.take()) {
            return Ok(Some(result));
        }
        if let Some(error) = CAPTURE.with(|c| c.borrow_mut().error.take()) {
            return Err(error);
        }
        if cancel.stopped() || unsafe { GetAsyncKeyState(VK_ESCAPE as i32) } < 0 {
            return Ok(None);
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err("Location picking timed out. Try Pick location again.".into());
        }
        if !cancel.wait(10) {
            return Ok(None);
        }
    }
}
pub fn hotkeys(state: AutoClickerState) {
    const ID: i32 = 0x4d43;
    let mut message = MSG::default();
    // Create the queue before publishing the thread ID used by shutdown.
    unsafe {
        PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
    }
    state.set_hotkey_thread(unsafe { GetCurrentThreadId() });
    if unsafe { RegisterHotKey(null_mut(), ID, MOD_NOREPEAT, VK_F6 as u32) } == 0 {
        state.hotkey_result(Err("F6 is already in use or unavailable. Close the other hotkey app, then reopen MYLE. Start and Stop still work here.".into()));
        state.set_hotkey_thread(0);
        return;
    }
    state.hotkey_result(Ok(()));
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        if message.message == WM_HOTKEY && message.wParam == ID as usize {
            state.toggle();
        }
    }
    unsafe {
        UnregisterHotKey(null_mut(), ID);
    }
    state.set_hotkey_thread(0);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clicks_inside_myle_reach_the_cancel_control_but_a_captured_pair_is_consumed() {
        let mut capture = Capture::default();
        let point = Point { x: 20, y: 40 };
        assert!(!capture.mouse(WM_LBUTTONDOWN, point, true));
        assert!(!capture.mouse(WM_LBUTTONUP, point, true));
        assert!(capture.result.is_none());
        assert!(capture.mouse(WM_LBUTTONDOWN, point, false));
        assert!(capture.mouse(WM_LBUTTONUP, point, true));
        assert_eq!(capture.result, Some(point));
    }
    #[test]
    fn every_click_action_contains_matching_down_and_up_events() {
        for (button, down, up) in [
            (MouseButton::Left, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            (
                MouseButton::Right,
                MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP,
            ),
            (
                MouseButton::Middle,
                MOUSEEVENTF_MIDDLEDOWN,
                MOUSEEVENTF_MIDDLEUP,
            ),
        ] {
            for (kind, expected) in [
                (ClickType::Single, vec![down, up]),
                (ClickType::Double, vec![down, up, down, up]),
            ] {
                let inputs = mouse_inputs(button, kind);
                let flags: Vec<_> = inputs
                    .iter()
                    .map(|input| unsafe { input.Anonymous.mi.dwFlags })
                    .collect();
                assert_eq!(flags, expected);
                assert!(inputs.iter().all(|input| input.r#type == INPUT_MOUSE));
            }
        }
    }
}
