// ! Copying a password: kept out of Windows' clipboard history (Win+V) and ! cloud clipboard, and cleared again after a short while unless something ! else has been copied since.
use std::ptr::null_mut;
use std::time::Duration;
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
pub const CLEAR_AFTER: Duration = Duration::from_secs(30);
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
/// The clipboard is shared: another program may hold it for a moment.
fn open() -> Result<(), String> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(null_mut()) } != 0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Err("The clipboard is in use by another program. Try again.".into())
}
/// Puts `bytes` on the (open) clipboard as `format`.
fn put(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory).cast::<u8>();
        if target.is_null() {
            GlobalFree(memory);
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(memory);
        // On success the clipboard owns the memory.
        if SetClipboardData(format, memory).is_null() {
            GlobalFree(memory);
            return false;
        }
        true
    }
}
fn format(name: &str) -> u32 {
    unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) }
}
/// Copies `text` and clears it after `CLEAR_AFTER` if it is still there.
pub fn copy_secret(text: &str) -> Result<(), String> {
    open()?;
    let copied = unsafe {
        EmptyClipboard();
        let chars = wide(text);
        let bytes = std::slice::from_raw_parts(chars.as_ptr().cast::<u8>(), chars.len() * 2);
        let ok = put(CF_UNICODETEXT, bytes);
        // Windows' own markers for secrets: clipboard managers and the history skip it, and it never syncs to other devices.
        put(format("ExcludeClipboardContentFromMonitorProcessing"), &[0]);
        put(format("CanIncludeInClipboardHistory"), &0u32.to_le_bytes());
        put(format("CanUploadToCloudClipboard"), &0u32.to_le_bytes());
        CloseClipboard();
        ok
    };
    if !copied {
        return Err("Could not copy to the clipboard.".into());
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    std::thread::spawn(move || {
        std::thread::sleep(CLEAR_AFTER);
        clear_if_unchanged(sequence);
    });
    Ok(())
}
/// The picture on the clipboard (a snip, a copied image), if there is one.
pub fn read_image() -> Result<Option<super::qr::Grey>, String> {
    open()?;
    let dib = unsafe {
        let handle = GetClipboardData(CF_DIB);
        let data = if handle.is_null() { std::ptr::null() } else { GlobalLock(handle).cast::<u8>().cast_const() };
        let copy = (!data.is_null()).then(|| std::slice::from_raw_parts(data, GlobalSize(handle)).to_vec());
        if !data.is_null() {
            GlobalUnlock(handle);
        }
        CloseClipboard();
        copy
    };
    Ok(dib.and_then(|dib| super::qr::grey_from_dib(&dib)))
}
/// Empties the clipboard only if nothing has been copied since `sequence`.
pub fn clear_if_unchanged(sequence: u32) {
    if unsafe { GetClipboardSequenceNumber() } != sequence || open().is_err() {
        return;
    }
    unsafe {
        EmptyClipboard();
        CloseClipboard();
    }
}
