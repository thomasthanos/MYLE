use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use winreg::RegKey;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

use super::models::{DetectedFolder, GameRoot, RootSource, RootStore};

pub(crate) fn stable_id(prefix: &str, value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(prefix.as_bytes());
    hash.update([0]);
    hash.update(value.trim().to_lowercase().as_bytes());
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{}", &digest[..16])
}

/// Whether two titles name the same game, as its id (`stable_id`) does:
/// casing and surrounding spaces aside.
pub(crate) fn same_title(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Where backups go inside a cloud folder: never its root, and the same
/// place on every PC signed in to the same account.
const CLOUD_SUBFOLDER: [&str; 2] = ["Make Your Life Easier", "Game Saves Backups"];

/// `<cloud folder>\Make Your Life Easier\Game Saves Backups`. A folder that
/// already is (or is inside the first part of) that path is not nested again.
pub(crate) fn cloud_backup_folder(root: &Path) -> PathBuf {
    let names: Vec<String> = root
        .components()
        .rev()
        .take(2)
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let [app, backups] = CLOUD_SUBFOLDER.map(str::to_lowercase);
    if names.first() == Some(&backups) && names.get(1) == Some(&app) {
        root.to_path_buf()
    } else if names.first() == Some(&app) {
        root.join(CLOUD_SUBFOLDER[1])
    } else {
        root.join(CLOUD_SUBFOLDER[0]).join(CLOUD_SUBFOLDER[1])
    }
}

/// The synced folders of the cloud apps installed for this user, with where
/// Game Saves puts its backups inside each.
pub(crate) fn detect_cloud_folders() -> Vec<DetectedFolder> {
    crate::cloud::detection::detect_folders()
        .into_iter()
        .map(|folder| DetectedFolder {
            provider: folder.provider,
            label: folder.label,
            backup_path: cloud_backup_folder(&folder.path)
                .to_string_lossy()
                .into_owned(),
            path: folder.path.to_string_lossy().into_owned(),
        })
        .collect()
}

pub(crate) use crate::cloud::detection::google_my_drive;

pub(crate) fn detect_roots() -> Vec<GameRoot> {
    let mut out = Vec::new();

    if let Some(steam) = registry_string(
        HKEY_CURRENT_USER,
        KEY_READ,
        r"Software\Valve\Steam",
        "SteamPath",
    )
    .or_else(|| {
        registry_string(
            HKEY_LOCAL_MACHINE,
            KEY_READ | KEY_WOW64_32KEY,
            r"Software\Valve\Steam",
            "InstallPath",
        )
    }) {
        let steam = PathBuf::from(steam.replace('/', "\\"));
        push_root(&mut out, &steam, RootStore::Steam, RootSource::Automatic);
        detect_steam_libraries(&steam, &mut out);
    }

    detect_epic_libraries(&mut out);

    detect_gog(&mut out);
    detect_launcher_install(
        &mut out,
        RootStore::Uplay,
        &[
            (
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_32KEY,
                r"Software\Ubisoft\Launcher",
                "InstallDir",
            ),
            (
                HKEY_CURRENT_USER,
                KEY_READ,
                r"Software\Ubisoft\Launcher",
                "InstallDir",
            ),
        ],
    );
    detect_launcher_install(
        &mut out,
        RootStore::Origin,
        &[
            (
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_32KEY,
                r"Software\Origin",
                "ClientPath",
            ),
            (
                HKEY_LOCAL_MACHINE,
                KEY_READ | KEY_WOW64_64KEY,
                r"Software\Electronic Arts\EA Desktop",
                "InstallLocation",
            ),
        ],
    );

    out.sort_by(|a, b| {
        a.store
            .ludusavi_name()
            .cmp(b.store.ludusavi_name())
            .then(a.path.cmp(&b.path))
    });
    let mut seen = HashSet::new();
    out.retain(|root| seen.insert(root.id.clone()));
    out
}

fn detect_epic_libraries(out: &mut Vec<GameRoot>) {
    let Some(program_data) = std::env::var_os("PROGRAMDATA") else {
        return;
    };
    let manifests = Path::new(&program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    let Ok(entries) = fs::read_dir(manifests) else {
        return;
    };
    for entry in entries.flatten().take(4096) {
        if !entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > 2 * 1024 * 1024 {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(location) = json
            .get("InstallLocation")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
        else {
            continue;
        };
        let installed = PathBuf::from(location);
        push_root(
            out,
            installed.parent().unwrap_or(&installed),
            RootStore::Epic,
            RootSource::Automatic,
        );
    }
}

fn detect_launcher_install(
    out: &mut Vec<GameRoot>,
    store: RootStore,
    candidates: &[(winreg::HKEY, u32, &str, &str)],
) {
    for (hive, flags, key, value) in candidates {
        let Some(raw) = registry_string(*hive, *flags, key, value) else {
            continue;
        };
        let mut path = PathBuf::from(raw.trim_matches('"'));
        if path.is_file() {
            path.pop();
        }
        push_root(out, &path, store, RootSource::Automatic);
        break;
    }
}

fn detect_gog(out: &mut Vec<GameRoot>) {
    for flags in [KEY_READ | KEY_WOW64_64KEY, KEY_READ | KEY_WOW64_32KEY] {
        let hive = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(games) = hive.open_subkey_with_flags(r"Software\GOG.com\Games", flags) else {
            continue;
        };
        for id in games.enum_keys().flatten() {
            let Ok(game) = games.open_subkey_with_flags(id, flags) else {
                continue;
            };
            let Ok(path) = game.get_value::<String, _>("path") else {
                continue;
            };
            let path = PathBuf::from(path);
            push_root(
                out,
                path.parent().unwrap_or(&path),
                RootStore::Gog,
                RootSource::Automatic,
            );
        }
    }

    for flags in [KEY_READ | KEY_WOW64_64KEY, KEY_READ | KEY_WOW64_32KEY] {
        if let Some(client) = registry_string(
            HKEY_LOCAL_MACHINE,
            flags,
            r"Software\GOG.com\GalaxyClient\paths",
            "client",
        ) {
            let path = PathBuf::from(client);
            push_root(
                out,
                path.parent().unwrap_or(&path),
                RootStore::GogGalaxy,
                RootSource::Automatic,
            );
            break;
        }
    }
}

fn detect_steam_libraries(steam: &Path, out: &mut Vec<GameRoot>) {
    let file = steam.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("\"path\"") {
            continue;
        }
        let quoted: Vec<&str> = line.split('"').collect();
        if quoted.len() >= 4 {
            let path = quoted[3].replace(r"\\", r"\");
            push_root(
                out,
                Path::new(&path),
                RootStore::Steam,
                RootSource::Automatic,
            );
        }
    }
}

fn registry_string(hive: winreg::HKEY, flags: u32, key: &str, value: &str) -> Option<String> {
    RegKey::predef(hive)
        .open_subkey_with_flags(key, flags)
        .ok()?
        .get_value::<String, _>(value)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn push_root(out: &mut Vec<GameRoot>, path: &Path, store: RootStore, source: RootSource) {
    if !path.is_dir() {
        return;
    }
    let path = trim_separators(&path.to_string_lossy());
    let id = stable_id(store.ludusavi_name(), &path);
    out.push(GameRoot {
        id,
        path,
        store,
        source,
    });
}

/// Drops trailing separators, except the one that makes `D:\` a drive root:
/// `D:` on its own means "the current folder on D:", not the drive.
fn trim_separators(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_root_keeps_its_backslash() {
        assert_eq!(trim_separators(r"D:\"), r"D:\");
        assert_eq!(trim_separators(r"D:\Games\"), r"D:\Games");
        assert_eq!(
            trim_separators("C:/Program Files/Steam/"),
            "C:/Program Files/Steam"
        );
    }

    #[test]
    fn cloud_backups_get_a_folder_of_their_own_but_never_twice() {
        let expected = PathBuf::from(r"D:\Dropbox\Make Your Life Easier\Game Saves Backups");
        for chosen in [
            r"D:\Dropbox",
            r"D:\Dropbox\Make Your Life Easier",
            r"D:\Dropbox\make your life easier\GAME SAVES BACKUPS",
        ] {
            let folder = cloud_backup_folder(Path::new(chosen));
            assert!(
                folder
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&expected.to_string_lossy()),
                "{chosen} gave {}",
                folder.display()
            );
        }
    }

    #[test]
    fn identifiers_are_stable_and_case_insensitive() {
        assert_eq!(
            stable_id("steam", r"C:\Games"),
            stable_id("steam", r"c:\games")
        );
        assert_ne!(
            stable_id("steam", r"C:\Games"),
            stable_id("gog", r"C:\Games")
        );
    }
}
