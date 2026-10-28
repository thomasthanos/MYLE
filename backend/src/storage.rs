// ! Where the app keeps its data: next to the install, the way the old app did. ! ! - settings, the account and Game Saves: `%APPDATA%\ThomasThanos\MakeYourLifeEasier` ! - caches and the web view's profile (local storage): ! `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data`, inside the install ! folder (`%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier`) ! ! Versions up to 7.0.x used Tauri's identifier-named folders; a development ! build used "Make Your Life Easier". Those move over once, and only whole: ! half a web view profile loses the settings kept in its local storage. When ! the move is not possible yet (the previous version still runs during an ! update and holds its profile), this run keeps using the old folder and the ! next start tries again.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use crate::download::err;
pub const PUBLISHER: &str = "ThomasThanos";
pub const FOLDER: &str = "MakeYourLifeEasier";
/// Earlier homes of the same data, oldest first.
const LEGACY: [&str; 2] = ["com.thomasthanos.makeyourlifeeasier", "Make Your Life Easier"];
struct Dirs {
    roaming: PathBuf,
    local: PathBuf,
}
/// Chosen once per run, by `prepare` or on first use.
static DIRS: OnceLock<Dirs> = OnceLock::new();
pub fn roaming_dir() -> Result<PathBuf, String> {
    dirs().map(|dirs| dirs.roaming.clone())
}
pub fn local_dir() -> Result<PathBuf, String> {
    dirs().map(|dirs| dirs.local.clone())
}
pub fn webview_dir() -> Result<PathBuf, String> {
    local_dir()
}
/// Called before any window: moves the data from an earlier home when it can
/// and picks the folders this run uses.
pub fn prepare() -> Result<(), String> {
    let roaming = settle(&roaming_target()?, &legacy_in(&base("APPDATA")?))?;
    let local = settle(&local_target()?, &legacy_in(&base("LOCALAPPDATA")?))?;
    let _ = DIRS.set(Dirs { roaming, local });
    Ok(())
}
/// Without `prepare` (the scheduled Game Saves backup, the helpers): nothing
/// moves, the data is read wherever it is now.
fn dirs() -> Result<&'static Dirs, String> {
    if let Some(dirs) = DIRS.get() {
        return Ok(dirs);
    }
    let roaming = current(&roaming_target()?, &legacy_in(&base("APPDATA")?));
    let local = current(&local_target()?, &legacy_in(&base("LOCALAPPDATA")?));
    Ok(DIRS.get_or_init(|| Dirs { roaming, local }))
}
fn roaming_target() -> Result<PathBuf, String> {
    Ok(base("APPDATA")?.join(PUBLISHER).join(FOLDER))
}
fn local_target() -> Result<PathBuf, String> {
    Ok(base("LOCALAPPDATA")?.join(PUBLISHER).join(FOLDER).join("data"))
}
fn legacy_in(base: &Path) -> Vec<PathBuf> {
    LEGACY.iter().map(|name| base.join(name)).collect()
}
/// The folder to use from now on: `target` once it exists or a legacy folder
/// could be moved there in one piece; otherwise the legacy folder, as is.
fn settle(target: &Path, legacy: &[PathBuf]) -> Result<PathBuf, String> {
    if target.exists() {
        return Ok(target.to_path_buf());
    }
    if let Some(old) = legacy.iter().find(|old| old.is_dir()) {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        return Ok(match std::fs::rename(old, target) {
            Ok(()) => target.to_path_buf(),
            // Something holds a file inside: not now, and never in parts.
            Err(_) => old.clone(),
        });
    }
    std::fs::create_dir_all(target).map_err(err)?;
    Ok(target.to_path_buf())
}
/// Where the data is right now, without moving anything.
fn current(target: &Path, legacy: &[PathBuf]) -> PathBuf {
    if target.exists() {
        return target.to_path_buf();
    }
    legacy
        .iter()
        .find(|old| old.is_dir())
        .cloned()
        .unwrap_or_else(|| target.to_path_buf())
}
fn base(variable: &str) -> Result<PathBuf, String> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!("The Windows {variable} application-data folder could not be located.")
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "myle-storage-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn an_earlier_folder_moves_over_whole() {
        let root = temp();
        let old = root.join("com.thomasthanos.makeyourlifeeasier");
        std::fs::create_dir_all(old.join("EBWebView")).unwrap();
        std::fs::write(old.join("EBWebView").join("Local State"), b"state").unwrap();
        let target = root.join("ThomasThanos").join("MakeYourLifeEasier").join("data");
        assert_eq!(settle(&target, std::slice::from_ref(&old)).unwrap(), target);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(target.join("EBWebView").join("Local State")).unwrap(),
            b"state"
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn a_folder_in_use_stays_where_it_is_and_is_used_as_is() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp();
        let old = root.join("old");
        std::fs::create_dir_all(old.join("EBWebView")).unwrap();
        let held = old.join("EBWebView").join("LOCK");
        std::fs::write(&held, b"").unwrap();
        // Open without delete sharing, as a running web view holds its files.
        let _handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1 | 2) // read | write, no delete
            .open(&held)
            .unwrap();
        let target = root.join("new").join("data");
        assert_eq!(settle(&target, std::slice::from_ref(&old)).unwrap(), old);
        assert!(held.exists(), "nothing moved");
        assert!(!target.exists());
        drop(_handle);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn the_new_folder_wins_once_it_exists() {
        let root = temp();
        let old = root.join("old");
        let target = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        assert_eq!(settle(&target, std::slice::from_ref(&old)).unwrap(), target);
        assert!(old.exists(), "left alone");
        assert_eq!(current(&target, std::slice::from_ref(&old)), target);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn without_prepare_the_data_is_read_where_it_is() {
        let root = temp();
        let old = root.join("old");
        std::fs::create_dir_all(&old).unwrap();
        let target = root.join("new");
        assert_eq!(current(&target, std::slice::from_ref(&old)), old);
        assert!(!target.exists(), "nothing created or moved");
        let _ = std::fs::remove_dir_all(root);
    }
}
