// ! Whether Windows is locked (Win+L, the lock screen after sleep), so the ! vault can lock with it.
use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSFreeMemory, WTSINFOEXW,
    WTSQuerySessionInformationW, WTSSessionInfoEx,
};
pub fn windows_is_locked() -> bool {
    let mut buffer = std::ptr::null_mut();
    let mut size = 0u32;
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut size,
        )
    };
    if ok == 0 || buffer.is_null() {
        return false;
    }
    let locked = unsafe {
        let info = &*(buffer.cast::<WTSINFOEXW>());
        // WTS_SESSIONSTATE_LOCK is 0 on Windows 10 and 11 (1 is unlocked).
        info.Level == 1 && info.Data.WTSInfoExLevel1.SessionFlags == 0
    };
    unsafe { WTSFreeMemory(buffer.cast()) };
    locked
}
