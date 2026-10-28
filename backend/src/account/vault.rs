// ! The signed-in session on disk, encrypted with Windows DPAPI: only this ! Windows user on this PC can read it back, like the old app's safeStorage.
use std::path::Path;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
pub fn save(path: &Path, plain: &[u8]) -> Result<(), String> {
    let sealed = protect(plain)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, sealed).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}
/// `None` when there is no session, or it was sealed by another user or PC.
pub fn load(path: &Path) -> Option<Vec<u8>> {
    unprotect(&std::fs::read(path).ok()?)
}
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let input = blob(plain);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: `input` points at `plain`, alive for the call; on success `output` is allocated by the system and freed below with LocalFree.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(take(output))
}
fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    let input = blob(sealed);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    (ok != 0).then(|| take(output))
}
fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    }
}
/// Copies a system-allocated blob out and frees it.
fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    // SAFETY: DPAPI returned `cbData` valid bytes at `pbData`, allocated with LocalAlloc; they are copied before the single LocalFree.
    unsafe {
        let bytes = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
        LocalFree(blob.pbData as _);
        bytes
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_sealed_session_opens_again_and_is_not_plain_text() {
        let path = std::env::temp_dir().join(format!("myle-vault-{}.bin", std::process::id()));
        let secret = br#"{"refresh_token":"abc123"}"#;
        save(&path, secret).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"abc123"));
        assert_eq!(load(&path).as_deref(), Some(&secret[..]));
        remove(&path);
        assert!(load(&path).is_none());
    }
}
