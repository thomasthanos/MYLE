// ! Recycle Bin size and emptying, through the Windows shell API. ! Neither call needs administrator rights.
use windows_sys::Win32::UI::Shell::{
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW, SHQUERYRBINFO,
    SHQueryRecycleBinW,
};
/// (bytes, items) across every drive.
pub fn query() -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // A null path means "all drives".
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    (hr == 0).then(|| (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}
/// Empties it without the shell's own confirmation, progress or sound.
pub fn empty() -> Result<(), String> {
    let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
    let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), flags) };
    match hr {
        // S_OK, or "it was already empty".
        0 | -2147418113 => Ok(()),
        other => Err(format!(
            "emptying the Recycle Bin failed (0x{:08X})",
            other as u32
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn querying_the_recycle_bin_works() {
        // Reads only: any machine answers, even with an empty bin.
        let (bytes, items) = query().expect("the shell should answer");
        assert!(bytes == 0 || items > 0, "{bytes} bytes but no items?");
    }
}
