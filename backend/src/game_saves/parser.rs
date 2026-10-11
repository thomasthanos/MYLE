use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::detection::{same_title, stable_id};
use super::models::{
    GameSaveEntry, GameSaveSnapshot, GameSaveStatus, GameSavesScan, GameSavesSettings,
    GameSavesStats,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiOutput {
    #[serde(default)]
    pub games: BTreeMap<String, ApiGame>,
    #[serde(default)]
    pub overall: Option<OperationStatus>,
    #[serde(default)]
    pub errors: Option<ApiErrors>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiErrors {
    #[serde(default)]
    pub some_games_failed: Option<bool>,
    /// Names passed in that the database does not know; the engine then
    /// processes nothing and exits with an error.
    #[serde(default)]
    pub unknown_games: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationStatus {
    #[serde(default)]
    pub processed_bytes: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiGame {
    #[serde(default)]
    pub change: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, ApiFile>,
    #[serde(default)]
    pub registry: BTreeMap<String, ApiRegistry>,
    #[serde(default)]
    pub backup_path: Option<String>,
    #[serde(default)]
    pub backups: Vec<ApiBackup>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiFile {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub error: Option<ApiError>,
    /// For a restore: how the backed-up file compares with the one on this PC
    /// ("New", "Different", "Same", "Unknown"); "Same" is not written. The
    /// file's key is the path it is written to (a restore location applied).
    #[serde(default)]
    pub change: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRegistry {
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiError {
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiBackup {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub when: String,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ManifestMetadata {
    pub supported_games: u64,
    pub badges: HashMap<String, Vec<String>>,
    /// Each game's Steam app id, for its cover. Shared: every scan copies
    /// the metadata, and there are some 50 000.
    pub steam_ids: std::sync::Arc<HashMap<String, u32>>,
}

pub(crate) fn parse_api(text: &str) -> Result<ApiOutput, String> {
    if text.trim().is_empty() {
        return Err("The Game Saves engine returned no data.".into());
    }
    serde_json::from_str(text)
        .map_err(|e| format!("The Game Saves engine returned invalid data: {e}"))
}

/// Parses only the stable, shallow fields needed by our UI. This deliberately
/// does not deserialize the 16+ MB YAML document and remains forward compatible
/// when the manifest adds fields.
pub(crate) fn parse_manifest_metadata(yaml: &str) -> ManifestMetadata {
    let mut result = ManifestMetadata::default();
    let mut title: Option<String> = None;
    let mut supported = false;
    let mut badges = Vec::<String>::new();
    let mut in_cloud = false;
    let mut in_steam = false;
    let mut steam_ids = HashMap::<String, u32>::new();

    let finish = |title: &mut Option<String>,
                  supported: &mut bool,
                  badges: &mut Vec<String>,
                  result: &mut ManifestMetadata| {
        if let Some(name) = title.take() {
            if *supported {
                result.supported_games += 1;
            }
            if !badges.is_empty() {
                badges.sort();
                badges.dedup();
                result.badges.insert(name, std::mem::take(badges));
            }
        }
        *supported = false;
        badges.clear();
    };

    for line in yaml.lines() {
        if line.is_empty() || line.starts_with('#') || line == "---" {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 0 && trimmed.ends_with(':') {
            finish(&mut title, &mut supported, &mut badges, &mut result);
            title = Some(yaml_key(&trimmed[..trimmed.len() - 1]));
            in_cloud = false;
            in_steam = false;
            continue;
        }
        if title.is_none() {
            continue;
        }
        if indent == 2 {
            in_cloud = trimmed == "cloud:";
            in_steam = trimmed == "steam:";
            if trimmed == "files:" || trimmed == "registry:" {
                supported = true;
            }
            continue;
        }
        if in_steam
            && indent == 4
            && let Some(id) = trimmed.strip_prefix("id:").and_then(|id| id.trim().parse::<u32>().ok())
            && let Some(name) = &title
        {
            steam_ids.insert(name.clone(), id);
            continue;
        }
        if in_cloud
            && indent == 4
            && let Some((provider, value)) = trimmed.split_once(':')
            && value.trim().eq_ignore_ascii_case("true")
        {
            let label = match provider.trim() {
                "steam" => "Steam Cloud",
                "epic" => "Epic Cloud",
                "gog" => "GOG Cloud",
                "origin" => "EA Cloud",
                "uplay" => "Ubisoft Cloud",
                other => other,
            };
            badges.push(label.into());
        }
    }
    finish(&mut title, &mut supported, &mut badges, &mut result);
    result.steam_ids = std::sync::Arc::new(steam_ids);
    result
}

fn yaml_key(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        raw[1..raw.len() - 1].replace("''", "'")
    } else if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        raw[1..raw.len() - 1]
            .replace(r#"\""#, "\"")
            .replace(r"\\", r"\")
    } else {
        raw.to_string()
    }
}

pub(crate) fn build_scan(
    local: ApiOutput,
    restore: ApiOutput,
    inventory: ApiOutput,
    metadata: &ManifestMetadata,
    settings: &GameSavesSettings,
) -> GameSavesScan {
    let mut inventory_games = inventory.games;
    let mut restore_games = restore.games;
    let mut local_rows = Vec::new();

    for (title, game) in local.games {
        if game.files.values().all(|file| file.ignored)
            && game.registry.values().all(|entry| entry.ignored)
        {
            continue;
        }
        let history = inventory_games.get(&title);
        local_rows.push(entry_from(
            &title, &game, history, true, true, metadata, settings,
        ));
    }

    let local_titles: HashSet<String> = local_rows
        .iter()
        .map(|game| game.title.to_ascii_lowercase())
        .collect();
    let mut backup_rows = Vec::new();
    for (title, history) in &mut inventory_games {
        let preview = restore_games.remove(title).unwrap_or_default();
        let local_present = local_titles.contains(&title.to_ascii_lowercase());
        backup_rows.push(entry_from(
            title,
            &preview,
            Some(history),
            false,
            local_present,
            metadata,
            settings,
        ));
    }

    local_rows.sort_by_key(|game| game.title.to_lowercase());
    backup_rows.sort_by_key(|game| game.title.to_lowercase());
    let total_bytes = local_rows.iter().map(|game| game.total_bytes).sum();
    GameSavesScan {
        generated_at: now(),
        stats: GameSavesStats {
            local_games: local_rows.len(),
            backup_games: backup_rows.len(),
            total_bytes,
            database_games: metadata.supported_games.max(settings.database_games),
        },
        on_this_pc: local_rows,
        in_backup: backup_rows,
        ..Default::default()
    }
}

fn entry_from(
    title: &str,
    game: &ApiGame,
    history: Option<&ApiGame>,
    local_view: bool,
    local_present: bool,
    metadata: &ManifestMetadata,
    settings: &GameSavesSettings,
) -> GameSaveEntry {
    let id = stable_id("game", title);
    let files: Vec<_> = game
        .files
        .iter()
        .filter(|(_, file)| !file.ignored)
        .collect();
    let registries: Vec<_> = game
        .registry
        .iter()
        .filter(|(_, item)| !item.ignored)
        .collect();
    let total_bytes = files.iter().map(|(_, file)| file.bytes).sum();
    let failed =
        files.iter().any(|(_, file)| file.failed) || registries.iter().any(|(_, item)| item.failed);
    let error = files
        .iter()
        .find_map(|(_, file)| file.error.as_ref().map(|error| error.message.clone()))
        .or_else(|| {
            registries
                .iter()
                .find_map(|(_, item)| item.error.as_ref().map(|error| error.message.clone()))
        });
    let paths = display_paths(files.iter().map(|(path, _)| path.as_str()));
    let last_save_at = files
        .iter()
        .filter_map(|(path, _)| fs::metadata(path).ok()?.modified().ok())
        .filter_map(system_time)
        .max();
    let mut snapshots = snapshots(history);
    snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    snapshots.truncate(3);
    let has_backup = !snapshots.is_empty();
    let has_local_data = local_present;
    let unknown = if !local_view && !local_present {
        restore_destination_unknown(&id, &paths, !registries.is_empty(), settings, &Here::current())
    } else {
        Unknown::default()
    };
    let needs_location = unknown.needs_location;

    let status = if failed || error.is_some() {
        GameSaveStatus::Error
    } else if needs_location {
        GameSaveStatus::NeedsLocation
    } else if !local_view && !local_present {
        GameSaveStatus::BackupOnly
    } else if !has_backup {
        GameSaveStatus::NotBackedUp
    } else {
        match game.change.as_deref() {
            Some("Same") => GameSaveStatus::BackedUp,
            Some("New" | "Different" | "Removed") => GameSaveStatus::ChangedSinceBackup,
            _ => GameSaveStatus::Unknown,
        }
    };

    let mut platform_badges = metadata.badges.get(title).cloned().unwrap_or_default();
    if settings.custom_games.iter().any(|game| same_title(&game.name, title)) {
        platform_badges.push("Added by you".into());
    }
    platform_badges.sort();
    platform_badges.dedup();
    let auto_backup = !settings.auto_backup_excluded_game_ids.contains(&id)
        && settings
            .custom_games
            .iter()
            .find(|game| same_title(&game.name, title))
            .is_none_or(|game| game.auto_backup);

    GameSaveEntry {
        id,
        title: title.into(),
        status,
        platform_badges,
        file_count: files.len() as u64,
        total_bytes,
        last_save_at,
        last_backup_at: snapshots.first().map(|snapshot| snapshot.timestamp.clone()),
        paths,
        auto_backup,
        has_local_data,
        has_backup,
        error,
        snapshots,
        steam_id: metadata.steam_ids.get(title).copied(),
        unmapped_paths: unknown.paths,
        suggested_target: unknown.suggested_target,
    }
}

/// This PC's own folders, as Windows names them (injected in tests).
pub(crate) struct Here {
    pub profile: Option<PathBuf>,
    /// The Documents known folder: moved into OneDrive by its folder backup
    /// (Known Folder Move), or wherever the user put it.
    pub documents: Option<PathBuf>,
    /// `C:\Users\Public`: shared by every user, the same on every PC.
    pub public: Option<PathBuf>,
}

impl Here {
    pub(crate) fn current() -> Here {
        use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, FOLDERID_Public};
        Here {
            profile: std::env::var_os("USERPROFILE").map(PathBuf::from),
            documents: crate::debloat::system::known_folder(&FOLDERID_Documents),
            public: crate::debloat::system::known_folder(&FOLDERID_Public),
        }
    }
}

/// What a backup-only game's restore lacks on this PC.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Unknown {
    pub needs_location: bool,
    /// The save folders no mapping of the game covers yet.
    pub paths: Vec<String>,
    /// For a folder under another Documents: the same place in this PC's.
    pub suggested_target: Option<String>,
}

/// Where a backed-up path in Documents goes on this PC, when Documents moved
/// (OneDrive's folder backup on one side and not the other, or another
/// user): `<profile>\Documents\…` or `<profile>\OneDrive…\Documents\…`
/// that is not this PC's Documents. `Some(the same place in this one)`.
fn moved_documents(normalized: &str, raw: &str, here: &Here) -> Option<String> {
    let documents = here.documents.as_deref()?;
    let current = normalized_path(documents);
    if within(normalized, &current) {
        return None;
    }
    // `c:\users\<name>\[onedrive…\]documents\rest`
    // Empty parts dropped on both sides, so the two line up.
    let parts: Vec<&str> = normalized.split('\\').filter(|part| !part.is_empty()).collect();
    let users = parts.iter().position(|part| *part == "users")?;
    let after = &parts[users + 2..];
    let rest_at = match after {
        ["documents", ..] => users + 3,
        [cloud, "documents", ..] if cloud.starts_with("onedrive") => users + 4,
        _ => return None,
    };
    let original: Vec<&str> = raw.split(['\\', '/']).filter(|part| !part.is_empty()).collect();
    let rest = original.get(rest_at..).unwrap_or_default();
    let mut target = documents.to_path_buf();
    target.extend(rest);
    Some(target.to_string_lossy().into_owned())
}

pub(crate) fn restore_destination_unknown(
    game_id: &str,
    paths: &[String],
    has_registry: bool,
    settings: &GameSavesSettings,
    here: &Here,
) -> Unknown {
    let mappings: Vec<String> = settings
        .path_mappings
        .iter()
        .filter(|mapping| mapping.game_id == game_id)
        .map(|mapping| normalized_path(Path::new(&mapping.source)))
        .collect();
    if paths.is_empty() {
        return Unknown { needs_location: !has_registry && mappings.is_empty(), ..Unknown::default() };
    }

    let profiles_root = here.profile.as_deref().and_then(Path::parent).map(normalized_path);
    let current_profile = here.profile.as_deref().map(normalized_path);
    // Folders every user shares: the same on any PC.
    let shared: Vec<String> = here
        .public
        .iter()
        .map(|public| normalized_path(public))
        .chain(profiles_root.iter().flat_map(|root| [format!("{root}\\default"), format!("{root}\\all users")]))
        .collect();

    let mut unknown = Unknown::default();
    for raw in paths {
        let path = Path::new(raw);
        let normalized = normalized_path(path);
        // A mapping of this game whose folder holds it: it has its place.
        if mappings.iter().any(|source| within(&normalized, source)) {
            continue;
        }
        let is_shared = shared.iter().any(|folder| within(&normalized, folder));
        let without_place = if !path.is_absolute() || raw.contains('<') || raw.contains('%') {
            true
        } else if let Some(target) = (!is_shared).then(|| moved_documents(&normalized, raw, here)).flatten() {
            unknown.suggested_target.get_or_insert(target);
            true
        } else if let (Some(profiles_root), Some(current_profile)) = (profiles_root.as_deref(), current_profile.as_deref())
            && within(&normalized, profiles_root)
            && !within(&normalized, current_profile)
            && !is_shared
        {
            true
        } else {
            let drive_root: PathBuf = path.components().take(2).collect();
            !drive_root.as_os_str().is_empty() && !drive_root.exists()
        };
        if without_place {
            unknown.paths.push(raw.clone());
        }
    }
    unknown.needs_location = !unknown.paths.is_empty();
    unknown
}

/// Whether a restore-location mapping from `source` covers `path`.
pub(crate) fn covered_by(path: &str, source: &str) -> bool {
    within(&normalized_path(Path::new(path)), &normalized_path(Path::new(source)))
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn within(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('\\'))
}

fn snapshots(history: Option<&ApiGame>) -> Vec<GameSaveSnapshot> {
    let Some(history) = history else {
        return Vec::new();
    };
    history
        .backups
        .iter()
        .map(|backup| GameSaveSnapshot {
            id: backup.name.clone(),
            timestamp: backup.when.clone(),
            bytes: history
                .backup_path
                .as_deref()
                .map(Path::new)
                .map(|root| path_size(&root.join(&backup.name), 0))
                .unwrap_or(0),
            label: backup.comment.clone(),
            is_safety: false,
        })
        .collect()
}

fn path_size(path: &Path, depth: u8) -> u64 {
    if depth > 32 {
        return 0;
    }
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| path_size(&entry.path(), depth + 1))
        .sum()
}

fn display_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut parents = Vec::<String>::new();
    let mut seen = HashSet::new();
    for path in paths {
        let path = PathBuf::from(path);
        let display = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(&path).to_path_buf()
        }
        .to_string_lossy()
        .into_owned();
        let key = display.to_ascii_lowercase();
        if seen.insert(key) {
            parents.push(display);
            if parents.len() == 8 {
                break;
            }
        }
    }
    parents
}

fn system_time(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

pub(crate) fn now() -> u64 {
    system_time(SystemTime::now()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_saves::models::RestorePathMapping;

    fn here(documents: &str) -> Here {
        Here {
            profile: Some(PathBuf::from(r"C:\Users\P")),
            documents: Some(PathBuf::from(documents)),
            public: Some(PathBuf::from(r"C:\Users\Public")),
        }
    }

    fn unknown_for(paths: &[&str], mappings: &[(&str, &str)], documents: &str) -> Unknown {
        let settings = GameSavesSettings {
            path_mappings: mappings
                .iter()
                .map(|(source, target)| RestorePathMapping { game_id: "g".into(), source: (*source).into(), target: (*target).into() })
                .collect(),
            ..GameSavesSettings::default()
        };
        let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
        restore_destination_unknown("g", &paths, false, &settings, &here(documents))
    }

    #[test]
    fn saves_from_before_onedrive_moved_documents_need_their_new_place() {
        let moved = unknown_for(&["C:/Users/P/Documents/My Games/X"], &[], r"C:\Users\P\OneDrive\Documents");
        assert!(moved.needs_location);
        assert_eq!(moved.suggested_target.as_deref(), Some(r"C:\Users\P\OneDrive\Documents\My Games\X"));
        // And the other way: OneDrive's Documents then, the plain one now.
        let back = unknown_for(&[r"C:\Users\P\OneDrive - Personal\Documents\My Games\X"], &[], r"C:\Users\P\Documents");
        assert!(back.needs_location);
        assert_eq!(back.suggested_target.as_deref(), Some(r"C:\Users\P\Documents\My Games\X"));
        // Where this PC keeps Documents: nothing to choose.
        assert!(!unknown_for(&["C:/Users/P/Documents/My Games/X"], &[], r"C:\Users\P\Documents").needs_location);
    }

    #[test]
    fn another_users_documents_are_offered_this_users() {
        let other = unknown_for(&["C:/Users/Old/Documents/My Games/X"], &[], r"C:\Users\P\Documents");
        assert!(other.needs_location);
        assert_eq!(other.suggested_target.as_deref(), Some(r"C:\Users\P\Documents\My Games\X"));
        assert!(unknown_for(&["C:/Users/Old/AppData/Local/X"], &[], r"C:\Users\P\Documents").needs_location);
    }

    #[test]
    fn saves_in_the_shared_public_folder_need_no_place() {
        assert!(!unknown_for(&["C:/Users/Public/Documents/Game"], &[], r"C:\Users\P\Documents").needs_location);
    }

    #[test]
    fn every_save_folder_of_a_game_needs_its_place() {
        let paths = ["C:/Users/Other/AppData/Local/G", "C:/Users/Other/Saved Games/G"];
        let one = unknown_for(&paths, &[(r"C:\Users\Other\AppData\Local\G", r"C:\Users\P\AppData\Local\G")], r"C:\Users\P\Documents");
        assert!(one.needs_location, "one folder placed, the other not yet");
        assert_eq!(one.paths, ["C:/Users/Other/Saved Games/G"]);
        let both = unknown_for(&paths, &[(r"C:\Users\Other", r"C:\Users\P")], r"C:\Users\P\Documents");
        assert!(!both.needs_location, "one mapping above both covers both");
        assert!(covered_by("C:/Users/Other/Saved Games/G", r"C:\Users\Other"));
        assert!(!covered_by(r"C:\Users\Others\G", r"C:\Users\Other"));
    }

    #[test]
    fn manifest_count_only_includes_entries_with_backup_definitions() {
        let yaml = r#"
Alias only:
  alias: Real Game
No paths:
  steam:
    id: 1
'Game: One':
  files:
    '<winAppData>/Game': {}
  cloud:
    steam: true
  steam:
    id: 1245620
Registry Game:
  registry:
    HKEY_CURRENT_USER/Software/Game: {}
  cloud:
    gog: true
"#;
        let metadata = parse_manifest_metadata(yaml);
        assert_eq!(metadata.supported_games, 2);
        assert_eq!(metadata.badges["Game: One"], vec!["Steam Cloud"]);
        assert_eq!(metadata.badges["Registry Game"], vec!["GOG Cloud"]);
        // Steam app ids, for the covers; "cloud: steam: true" is not one.
        assert_eq!(metadata.steam_ids.get("Game: One"), Some(&1245620));
        assert_eq!(metadata.steam_ids.get("No paths"), Some(&1));
        assert_eq!(metadata.steam_ids.get("Registry Game"), None);
    }

    #[test]
    fn pinned_manifest_supported_count_is_current_when_resource_is_present() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ludusavi/manifest.yaml");
        // The pinned manifest is tens of megabytes; a build without the
        // downloaded resources has only a small stand-in, which says nothing.
        if let Ok(text) = fs::read_to_string(path)
            && text.len() > 1_000_000
        {
            assert_eq!(parse_manifest_metadata(&text).supported_games, 22_976);
        }
    }

    /// The engine may spell a custom game's title with other capitals than
    /// the user did: it is still the user's game, with the user's choices.
    #[test]
    fn a_custom_game_is_known_whatever_the_capitals() {
        let local = parse_api(
            r#"{"games":{"auditfoo":{"change":"New","decision":"Processed","files":{"C:/save.dat":{"bytes":10,"failed":false,"ignored":false}},"registry":{}}}}"#,
        )
        .unwrap();
        let settings = GameSavesSettings {
            custom_games: vec![crate::game_saves::models::CustomGame {
                id: "custom-1".into(),
                name: "AuditFoo".into(),
                paths: Vec::new(),
                install_path: None,
                auto_backup: false,
            }],
            ..Default::default()
        };
        let scan = build_scan(
            local,
            ApiOutput::default(),
            ApiOutput::default(),
            &ManifestMetadata::default(),
            &settings,
        );
        let game = &scan.on_this_pc[0];
        assert!(game.platform_badges.iter().any(|badge| badge == "Added by you"));
        assert!(!game.auto_backup);
    }

    #[test]
    fn partial_failures_are_not_reported_as_backed_up() {
        let local = parse_api(
            r#"{"games":{"Game":{"change":"Same","decision":"Processed","files":{"C:/save.dat":{"bytes":10,"failed":true,"ignored":false,"error":{"message":"locked"}}},"registry":{}}}}"#,
        )
        .unwrap();
        let history = parse_api(
            r#"{"games":{"Game":{"backupPath":"D:/backup/Game","backups":[{"name":"x.zip","when":"2026-01-01T00:00:00Z","locked":false}]}}}"#,
        )
        .unwrap();
        let scan = build_scan(
            local,
            ApiOutput::default(),
            history,
            &ManifestMetadata::default(),
            &GameSavesSettings::default(),
        );
        assert_eq!(scan.on_this_pc[0].status, GameSaveStatus::Error);
        assert_eq!(scan.on_this_pc[0].error.as_deref(), Some("locked"));
    }
}
