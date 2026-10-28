use std::path::{Component, Path, PathBuf};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ};
use winreg::{RegKey, RegValue};
use crate::download::err;
const USER_ENVIRONMENT: &str = "Environment";
pub fn add_user_path_entry(entry: &Path) -> Result<(), String> {
    update_user_path(entry, true)
}
pub fn remove_user_path_entry(entry: &Path) -> Result<(), String> {
    update_user_path(entry, false)
}
fn update_user_path(entry: &Path, add: bool) -> Result<(), String> {
    if !entry.is_absolute() {
        return Err("Refusing a non-absolute PATH entry.".into());
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(USER_ENVIRONMENT, KEY_READ | KEY_WRITE)
        .map_err(err)?;
    // The user PATH is normally REG_EXPAND_SZ with entries such as `%USERPROFILE%\.dotnet\tools`. Writing it back as a plain string would stop those from expanding, so the original type is kept. Anything but "not set yet" is an error: an unreadable PATH must never be replaced.
    let (before, kind) = match key.get_raw_value("Path") {
        Ok(raw) if matches!(raw.vtype, REG_SZ | REG_EXPAND_SZ) => {
            (read_wide(&raw.bytes), raw.vtype)
        }
        Ok(_) => return Err("The user PATH has an unexpected registry type.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), REG_EXPAND_SZ),
        Err(e) => return Err(e.to_string()),
    };
    let after = edit_path(&before, entry, add);
    if before != after {
        key.set_raw_value(
            "Path",
            &RegValue {
                bytes: write_wide(&after).into(),
                vtype: kind,
            },
        )
        .map_err(err)?;
        broadcast_environment_change();
    }
    Ok(())
}
fn read_wide(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}
fn write_wide(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
/// Lets Explorer (and every terminal started from it) pick up the new PATH
/// without signing out, the way the Environment Variables dialog does.
fn broadcast_environment_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let area: Vec<u16> = "Environment".encode_utf16().chain(Some(0)).collect();
    // SAFETY: `area` is NUL-terminated and outlives the call; the result pointer is optional and left null.
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
/// `%VAR%` references expanded, so an entry written either way is recognised.
fn expand_vars(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        let Some(len) = rest[start + 1..].find('%') else {
            break;
        };
        let name = &rest[start + 1..start + 1 + len];
        out.push_str(&rest[..start]);
        match std::env::var(name) {
            Ok(expanded) if !name.is_empty() => out.push_str(&expanded),
            _ => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}
fn edit_path(before: &str, entry: &Path, add: bool) -> String {
    let wanted = normalized(entry);
    let mut parts: Vec<String> = before
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter(|part| normalized(Path::new(&expand_vars(part))) != wanted)
        .map(str::to_string)
        .collect();
    if add {
        parts.push(
            entry
                .as_os_str()
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_string(),
        );
    }
    parts.join(";")
}
pub fn remove_allowlisted(target: &Path, allowlist: &[PathBuf]) -> Result<(), String> {
    if !target.is_absolute() || !allowlist.iter().any(|allowed| same_path(target, allowed)) {
        return Err(format!(
            "Refusing a path outside the Spotify Hub allowlist: {}",
            target.display()
        ));
    }
    if target
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("Refusing a non-normalized removal path.".into());
    }
    if !target.exists() && std::fs::symlink_metadata(target).is_err() {
        return Ok(());
    }
    validate_ancestors_and_canonical_target(target)
        .and_then(|()| validate_no_reparse_points(target))
        .map_err(|error| format!("Safety validation failed: {error}"))?;
    let metadata = std::fs::symlink_metadata(target).map_err(err)?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(target).map_err(err)
    } else {
        std::fs::remove_file(target).map_err(err)
    }
}
fn validate_ancestors_and_canonical_target(target: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in target.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = std::fs::symlink_metadata(&current)
            && is_reparse(&metadata)
        {
            return Err(format!(
                "Refusing a path beneath a reparse point: {}",
                current.display()
            ));
        }
    }
    let parent = target.parent().ok_or("The removal target has no parent.")?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(err)?;
    let canonical_target = std::fs::canonicalize(target).map_err(err)?;
    if canonical_target.parent() != Some(canonical_parent.as_path()) {
        return Err("The canonical removal target escaped its expected parent.".into());
    }
    Ok(())
}
/// Recursive deletion must never cross a junction, symlink, cloud placeholder
/// or other reparse point. Unknown linked content is reported and left intact.
pub(super) fn validate_no_reparse_points(target: &Path) -> Result<(), String> {
    // Validate every existing ancestor as well as the target tree. Checking only the final path is insufficient when (for example) a known AppData directory has itself been replaced by a junction.
    let mut ancestor = Some(target);
    while let Some(path) = ancestor {
        let metadata = std::fs::symlink_metadata(path).map_err(err)?;
        if is_reparse(&metadata) {
            return Err(format!(
                "Refusing to follow a reparse point: {}",
                path.display()
            ));
        }
        ancestor = path.parent();
    }
    validate_descendants_no_reparse(target)
}
fn validate_descendants_no_reparse(target: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(target).map_err(err)?;
    if is_reparse(&metadata) {
        return Err(format!(
            "Refusing to follow a reparse point: {}",
            target.display()
        ));
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(target).map_err(err)? {
        let entry = entry.map_err(err)?;
        validate_descendants_no_reparse(&entry.path())?;
    }
    Ok(())
}
#[cfg(windows)]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
#[cfg(not(windows))]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
fn normalized(path: &Path) -> String {
    path.as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}
fn same_path(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}
/// A same-volume directory replacement. Until `commit`, dropping or manually
/// rolling back restores the previous installation.
pub struct DirectorySwap {
    target: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}
impl DirectorySwap {
    pub fn apply(staged: &Path, target: &Path, suffix: &str) -> Result<Self, String> {
        if !staged.is_dir() {
            return Err(format!(
                "The staged folder is missing: {}",
                staged.display()
            ));
        }
        let parent = target
            .parent()
            .ok_or("The installation target has no parent.")?;
        std::fs::create_dir_all(parent).map_err(err)?;
        let target_name = target
            .file_name()
            .ok_or("The installation target has no name.")?
            .to_string_lossy();
        let backup = parent.join(format!(".{target_name}.myle-backup-{suffix}"));
        if backup.exists() {
            std::fs::remove_dir_all(&backup).map_err(err)?;
        }
        let had_target = target.exists();
        if had_target {
            validate_no_reparse_points(target)
                .map_err(|error| format!("Safety validation failed: {error}"))?;
            std::fs::rename(target, &backup).map_err(err)?;
        }
        if let Err(error) = std::fs::rename(staged, target) {
            if had_target {
                let _ = std::fs::rename(&backup, target);
            }
            return Err(error.to_string());
        }
        Ok(Self {
            target: target.to_path_buf(),
            backup: had_target.then_some(backup),
            committed: false,
        })
    }
    pub fn rollback(&mut self) -> Result<(), String> {
        if self.committed {
            return Ok(());
        }
        if self.target.exists() {
            std::fs::remove_dir_all(&self.target).map_err(err)?;
        }
        if let Some(backup) = &self.backup
            && backup.exists()
        {
            std::fs::rename(backup, &self.target).map_err(err)?;
        }
        self.committed = true;
        Ok(())
    }
    /// Makes the new directory authoritative. A locked old backup is returned
    /// to the caller for deferred cleanup; it must not turn a successful apply
    /// into a rollback after Spotify has already been patched.
    pub fn commit(mut self) -> Option<PathBuf> {
        self.committed = true;
        self.backup
            .take()
            .filter(|backup| backup.exists())
            .and_then(|backup| std::fs::remove_dir_all(&backup).err().map(|_| backup))
    }
}
impl Drop for DirectorySwap {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.rollback();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_edit_adds_once_and_removes_only_its_exact_entry() {
        let entry = Path::new(r"C:\Users\Me\AppData\Local\spicetify");
        let before = r"C:\Windows;C:\Tools\spicetify-helper";
        let added = edit_path(before, entry, true);
        assert_eq!(added.matches("spicetify").count(), 2);
        assert_eq!(edit_path(&added, entry, true), added);
        assert_eq!(edit_path(&added, entry, false), before);
    }
    #[test]
    fn path_edit_keeps_unexpanded_entries_and_recognises_them() {
        let profile = std::env::var("USERPROFILE").unwrap();
        let entry = Path::new(&profile).join(r"AppData\Local\spicetify");
        let before = r"%USERPROFILE%\.dotnet\tools;%USERPROFILE%\AppData\Local\spicetify";
        // The variable form of our own entry is removed; the others stay verbatim.
        assert_eq!(edit_path(before, &entry, false), r"%USERPROFILE%\.dotnet\tools");
        assert_eq!(expand_vars("%NO_SUCH_VAR_MYLE%\\x;50%"), "%NO_SUCH_VAR_MYLE%\\x;50%");
    }
    #[test]
    fn registry_strings_round_trip_through_utf16() {
        let text = r"C:\Ελληνικά;%USERPROFILE%\bin";
        assert_eq!(read_wide(&write_wide(text)), text);
    }
    #[test]
    fn removal_requires_exact_allowlist_membership() {
        let root = std::env::temp_dir().join(format!("myle-allowlist-{}", std::process::id()));
        let allowed = root.join("Spotify");
        let other = root.join("Other");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        assert!(remove_allowlisted(&other, std::slice::from_ref(&allowed)).is_err());
        assert!(other.exists());
        remove_allowlisted(&allowed, std::slice::from_ref(&allowed)).unwrap();
        assert!(!allowed.exists());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn directory_swap_can_roll_back() {
        let root = std::env::temp_dir().join(format!("myle-swap-{}", std::process::id()));
        let target = root.join("target");
        let staged = root.join("staged");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(target.join("version"), "old").unwrap();
        std::fs::write(staged.join("version"), "new").unwrap();
        let mut swap = DirectorySwap::apply(&staged, &target, "test").unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("version")).unwrap(),
            "new"
        );
        swap.rollback().unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("version")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
