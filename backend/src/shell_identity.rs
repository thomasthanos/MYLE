//! Shared Windows shell identity for the app and its installer.
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PVCHF_DEFAULT, PropVariantChangeType,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::IShellLinkW;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::{GUID, Interface};

pub const APP_ID: &str = "com.thomasthanos.makeyourlifeeasier";
pub const ICON_FILE: &str = "MYLE.ico";
pub const APP_ID_KEY: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// Older installations have only the executable; new ones ship a stable icon.
pub fn icon_path(exe: &Path) -> PathBuf {
    let icon = exe.with_file_name(ICON_FILE);
    if icon.is_file() { icon } else { exe.to_path_buf() }
}

pub fn set_link_identity(link: &IShellLinkW) -> windows::core::Result<()> {
    let source = PROPVARIANT::from(APP_ID);
    let mut value = PROPVARIANT::default();
    // SAFETY: valid variants, owned by this scope. SetValue copies the string.
    unsafe {
        PropVariantChangeType(&mut value, &source, PVCHF_DEFAULT, VT_LPWSTR)?;
        let properties: IPropertyStore = link.cast()?;
        properties.SetValue(&APP_ID_KEY, &value)?;
        properties.Commit()
    }
}

pub fn notify_update(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{SHCNE_UPDATEITEM, SHCNF_FLUSHNOWAIT, SHCNF_PATHW, SHChangeNotify};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a live NUL-terminated path; the shell copies it during the call.
    unsafe {
        SHChangeNotify(SHCNE_UPDATEITEM as i32, SHCNF_PATHW | SHCNF_FLUSHNOWAIT, wide.as_ptr().cast(), std::ptr::null());
    }
}
