// ! Where the app keeps its data: next to the install, the way the old app did. ! ! - settings, the account and Game Saves: `%APPDATA%\ThomasThanos\MakeYourLifeEasier` ! - caches and the web view's profile (local storage): ! `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data`, inside the install ! folder (`%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier`) ! ! Versions up to 7.0.x used Tauri's identifier-named folders; a development ! build used "Make Your Life Easier". Those move over once, and only whole: ! half a web view profile loses the settings kept in its local storage. When ! the move is not possible yet (the previous version still runs during an ! update and holds its profile), this run keeps using the old folder and the ! next start tries again.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use crate::download::err;
pub const PUBLISHER: &str = "ThomasThanos";
pub const FOLDER: &str = "MakeYourLifeEasier";
const INTERNAL: &str = "internal";
const STEAM: &str = "steam";
/// What has a tidier place than the top of the data folder: its name there, the folder it moves to,
/// and its name in that folder. Moved one by one, because each is used from wherever it is now.
const TIDIED: [(&str, &str, &str); 9] = [
    ("debloat-undo.json", INTERNAL, "debloat-undo.json"),
    ("passwords-hello.bin", INTERNAL, "passwords-hello.bin"),
    ("project-backups-staging", INTERNAL, "project-backups-staging"),
    ("resolver-cache.json", INTERNAL, "resolver-cache.json"),
    ("update.log", INTERNAL, "update.log"),
    ("webview-notice", INTERNAL, "webview-notice"),
    ("whats-new-cache.json", INTERNAL, "whats-new-cache.json"),
    ("steam-automation", STEAM, "automation"),
    ("steam-helper", STEAM, "helper"),
];
/// Folders nobody needs to open. Never single files: Windows refuses to overwrite a hidden file by
/// name, so the next save of one would fail. The web view's profile and the browser manifests stay
/// put because other programs hold their paths.
const HIDDEN: [&str; 3] = [INTERNAL, "EBWebView", "native-messaging"];
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
/// Where an item that has a tidier place is, by its old name in the data folder.
pub fn local_item(name: &str) -> Result<PathBuf, String> {
    Ok(item_in(&local_dir()?, name))
}
/// Called before any window: moves the data from an earlier home when it can
/// and picks the folders this run uses.
pub fn prepare() -> Result<(), String> {
    let roaming = settle(&roaming_target()?, &legacy_in(&base("APPDATA")?))?;
    let local = settle(&local_target()?, &legacy_in(&base("LOCALAPPDATA")?))?;
    tidy(&local);
    clear_web_caches(&local);
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
/// Moves loose items out of the way and hides what nobody needs to open. Any of it failing only
/// means the folder stays untidy until the next start.
fn tidy(root: &Path) {
    let _ = std::fs::create_dir_all(root.join(INTERNAL));
    let _ = std::fs::create_dir_all(root.join(STEAM));
    for (name, folder, new_name) in TIDIED {
        let (old, new) = (root.join(name), root.join(folder).join(new_name));
        if old.exists() && !new.exists() {
            let _ = std::fs::rename(old, new);
        }
    }
    for folder in HIDDEN {
        hide(&root.join(folder));
    }
}
fn hide(folder: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    let Ok(metadata) = std::fs::metadata(folder) else {
        return;
    };
    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call.
    unsafe { SetFileAttributesW(wide.as_ptr(), metadata.file_attributes() | FILE_ATTRIBUTE_HIDDEN) };
}
/// Before any web view starts: their caches are moved aside when due, and emptied in the background.
fn clear_web_caches(root: &Path) {
    let profiles = vec![
        root.join("EBWebView"),
        item_in(root, "webview-notice").join("EBWebView"),
        item_in(root, "steam-automation").join("webview-steam").join("EBWebView"),
    ];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|at| at.as_secs()).unwrap_or(0);
    crate::webview_cache::clear_when_due(&profiles, &root.join(INTERNAL).join("webview-caches.json"), env!("CARGO_PKG_VERSION"), now);
    std::thread::spawn(move || {
        for profile in profiles {
            crate::webview_cache::empty(&profile);
        }
    });
}
/// Where one item of the data folder is: its tidier place, unless it is still in the old one
/// (it was in use at the last start, or nothing has tidied this folder yet).
fn item_in(root: &Path, name: &str) -> PathBuf {
    let old = root.join(name);
    match TIDIED.iter().find(|(from, _, _)| *from == name) {
        Some((_, folder, new_name)) if !old.exists() || root.join(folder).join(new_name).exists() => root.join(folder).join(new_name),
        _ => old,
    }
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
        crate::test_dirs::temp_dir("storage")
    }
    fn is_hidden(path: &Path) -> bool {
        use std::os::windows::fs::MetadataExt;
        std::fs::metadata(path).unwrap().file_attributes() & 2 != 0
    }
    #[test]
    fn loose_items_move_to_a_tidier_place() {
        let root = temp();
        std::fs::write(root.join("update.log"), b"log").unwrap();
        std::fs::create_dir_all(root.join("steam-automation")).unwrap();
        std::fs::write(root.join("steam-automation").join("settings.json"), b"saved").unwrap();
        tidy(&root);
        assert!(!root.join("update.log").exists());
        assert_eq!(std::fs::read(root.join("internal").join("update.log")).unwrap(), b"log");
        assert_eq!(std::fs::read(root.join("steam").join("automation").join("settings.json")).unwrap(), b"saved");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn what_other_programs_point_at_stays_where_it_is() {
        let root = temp();
        std::fs::create_dir_all(root.join("EBWebView")).unwrap();
        std::fs::create_dir_all(root.join("native-messaging")).unwrap();
        std::fs::create_dir_all(root.join("game-saves")).unwrap();
        tidy(&root);
        assert!(root.join("EBWebView").is_dir() && root.join("native-messaging").is_dir() && root.join("game-saves").is_dir());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn a_moved_item_is_found_in_its_new_place() {
        let root = temp();
        std::fs::write(root.join("passwords-hello.bin"), b"key").unwrap();
        tidy(&root);
        assert_eq!(std::fs::read(item_in(&root, "passwords-hello.bin")).unwrap(), b"key");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn an_item_not_moved_yet_is_used_where_it_is() {
        let root = temp();
        std::fs::write(root.join("update.log"), b"log").unwrap();
        assert_eq!(item_in(&root, "update.log"), root.join("update.log"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn a_new_item_starts_in_the_new_place() {
        let root = temp();
        assert_eq!(item_in(&root, "steam-helper"), root.join("steam").join("helper"));
        assert_eq!(item_in(&root, "whats-new-cache.json"), root.join("internal").join("whats-new-cache.json"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn what_is_already_in_the_new_place_is_never_overwritten() {
        let root = temp();
        std::fs::create_dir_all(root.join("internal")).unwrap();
        std::fs::write(root.join("update.log"), b"old").unwrap();
        std::fs::write(root.join("internal").join("update.log"), b"new").unwrap();
        tidy(&root);
        assert_eq!(std::fs::read(item_in(&root, "update.log")).unwrap(), b"new");
        assert_eq!(std::fs::read(root.join("update.log")).unwrap(), b"old", "left alone, not lost");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn an_item_in_use_waits_for_the_next_start() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp();
        std::fs::create_dir_all(root.join("webview-notice")).unwrap();
        let held = root.join("webview-notice").join("LOCK");
        std::fs::write(&held, b"").unwrap();
        let handle = std::fs::OpenOptions::new().read(true).share_mode(1 | 2).open(&held).unwrap();
        tidy(&root);
        assert!(held.exists());
        assert_eq!(item_in(&root, "webview-notice"), root.join("webview-notice"));
        drop(handle);
        tidy(&root);
        assert_eq!(item_in(&root, "webview-notice"), root.join("internal").join("webview-notice"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn folders_nobody_opens_are_hidden_and_the_rest_are_not() {
        let root = temp();
        for folder in ["EBWebView", "native-messaging", "game-saves", "github-releases"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        tidy(&root);
        assert!(is_hidden(&root.join("internal")) && is_hidden(&root.join("EBWebView")) && is_hidden(&root.join("native-messaging")));
        assert!(!is_hidden(&root.join("game-saves")) && !is_hidden(&root.join("github-releases")) && !is_hidden(&root.join("steam")));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn a_missing_folder_is_not_a_problem() {
        let root = temp();
        tidy(&root);
        assert!(!root.join("EBWebView").exists(), "nothing is made just to hide it");
        let _ = std::fs::remove_dir_all(root);
    }
    // Windows refuses to overwrite a hidden file by name, which would break the next save.
    #[test]
    fn a_file_kept_in_a_hidden_folder_can_be_saved_again() {
        let root = temp();
        std::fs::write(root.join("update.log"), b"one").unwrap();
        tidy(&root);
        let file = item_in(&root, "update.log");
        assert!(!is_hidden(&file), "only the folder is hidden");
        std::fs::write(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(root);
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
