// ! The last scan, kept on disk. ! ! A full scan asks the engine about every game in its database (~23,000), ! which takes close to a minute with a few large libraries. The page shows ! this copy at once, refreshes only the games it lists (seconds), and runs a ! full scan in the background when new games may have appeared.
use std::collections::HashSet;
use std::path::Path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use super::atomic;
use super::models::{GameSaveStatus, GameSavesScan, GameSavesSettings};
const FILE: &str = "scan-cache.json";
/// A full scan is due again after this long, to pick up games installed
/// into folders that were already known.
pub(crate) const DISCOVERY_EVERY: u64 = 24 * 60 * 60;
#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedScan {
    /// `fingerprint()` of the settings the last full scan used.
    pub fingerprint: String,
    pub scan: GameSavesScan,
}
/// What decides which games a full scan can find: the game folders, the
/// custom games and the database version. Anything else (the backup folder,
/// restore locations) is re-read by every quick refresh anyway.
pub(crate) fn fingerprint(settings: &GameSavesSettings) -> String {
    let mut hash = Sha256::new();
    let mut roots: Vec<_> = settings
        .roots
        .iter()
        .map(|root| format!("{}|{}", root.store.ludusavi_name(), root.path.to_lowercase()))
        .collect();
    roots.sort();
    for root in roots {
        hash.update(root.as_bytes());
        hash.update([0]);
    }
    hash.update([1]);
    for game in &settings.custom_games {
        hash.update(game.name.as_bytes());
        for path in &game.paths {
            hash.update([0]);
            hash.update(path.to_lowercase().as_bytes());
        }
        hash.update([0]);
        hash.update(game.install_path.as_deref().unwrap_or("").as_bytes());
        hash.update([2]);
    }
    hash.update(settings.database_updated_at.unwrap_or(0).to_le_bytes());
    crate::download::to_hex(&hash.finalize())
}
pub(crate) fn load(root: &Path) -> Option<CachedScan> {
    let text = std::fs::read_to_string(root.join(FILE)).ok()?;
    serde_json::from_str(&text).ok()
}
/// The raw disk cache still supplies titles for a quick refresh. Before its
/// history is published or used for actions, require the scanned destination
/// to match. A missing legacy destination proves neither backup nor absence.
pub(crate) fn validate_destination(scan: &mut GameSavesScan, settings: &GameSavesSettings) {
    let matches = scan.backup_folder.as_deref().zip(settings.backup_folder.as_deref())
        .is_some_and(|(scanned, selected)| !scanned.is_empty() && !selected.is_empty()
            && super::settings::same_location(Path::new(scanned), Path::new(selected)));
    if matches { return; }
    scan.backup_folder = None;
    scan.in_backup.clear();
    scan.stats.backup_games = 0;
    scan.backup_unreachable = None;
    for game in &mut scan.on_this_pc {
        game.has_backup = false;
        game.last_backup_at = None;
        game.snapshots.clear();
        if game.status != GameSaveStatus::Error {
            game.status = GameSaveStatus::Unknown;
        }
    }
}
/// Best effort: a failed write only costs the next visit a full scan.
pub(crate) fn save(root: &Path, cached: &CachedScan) {
    if let Ok(json) = serde_json::to_vec(cached) {
        let _ = atomic::write(&root.join(FILE), &json);
    }
}
pub(crate) fn discovery_due(cached: &CachedScan, settings: &GameSavesSettings, now: u64) -> bool {
    cached.fingerprint != fingerprint(settings)
        || now.saturating_sub(cached.scan.full_scan_at) >= DISCOVERY_EVERY
}
/// Every game a quick refresh re-checks: the ones found on this PC, the ones
/// in the backup (so a reinstalled game shows up), and the custom games.
pub(crate) fn known_titles(scan: &GameSavesScan, settings: &GameSavesSettings) -> Vec<String> {
    let mut seen = HashSet::new();
    scan.on_this_pc
        .iter()
        .chain(&scan.in_backup)
        .map(|game| game.title.as_str())
        .chain(settings.custom_games.iter().map(|game| game.name.as_str()))
        .filter(|title| seen.insert(title.to_lowercase()))
        .map(str::to_string)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_saves::models::{
        CustomGame, GameRoot, GameSaveEntry, GameSaveStatus, RootSource, RootStore,
    };
    #[test]
    fn cached_scan_retains_which_backup_destination_was_actually_scanned() {
        let root = std::env::temp_dir().join(format!("myle-scan-destination-{}", uuid::Uuid::new_v4()));
        let value = serde_json::json!({
            "fingerprint": "discovery-only",
            "scan": {
                "generatedAt": 42, "stats": { "localGames": 0, "backupGames": 0, "totalBytes": 0, "databaseGames": 0 },
                "onThisPc": [], "inBackup": [], "backupFolder": "G:\\My Drive\\Game Saves Backups"
            }
        });
        let cached: CachedScan = serde_json::from_value(value).unwrap();
        save(&root, &cached);
        let loaded = serde_json::to_value(load(&root).unwrap()).unwrap();
        assert_eq!(loaded["scan"]["backupFolder"].as_str(), Some(r"G:\My Drive\Game Saves Backups"), "history provenance must survive process restart");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_or_other_destination_history_is_unknown_until_verified_without_losing_discovery_titles() {
        use crate::game_saves::models::GameSaveSnapshot;
        let settings = GameSavesSettings { backup_folder: Some(r"G:\My Drive\Backups".into()), ..Default::default() };
        for destination in [None, Some(r"E:\Previous Backups".to_string())] {
            let mut backed_up = entry("Game");
            backed_up.last_backup_at = Some("2026-09-27T19:38:42Z".into());
            backed_up.snapshots.push(GameSaveSnapshot { id: "old.zip".into(), timestamp: "2026-09-27T19:38:42Z".into(), bytes: 5, label: None, is_safety: false });
            let mut first = entry("First");
            first.status = GameSaveStatus::NotBackedUp;
            let mut failed = entry("Problem");
            failed.status = GameSaveStatus::Error;
            let raw = GameSavesScan {
                backup_folder: destination,
                on_this_pc: vec![backed_up, first, failed],
                in_backup: vec![entry("Old game")],
                full_scan_at: 42,
                ..Default::default()
            };
            let mut published = raw.clone();
            validate_destination(&mut published, &settings);
            assert_eq!(published.on_this_pc[0].status, GameSaveStatus::Unknown);
            assert_eq!(published.on_this_pc[1].status, GameSaveStatus::Unknown, "an old first-backup result is also unverified");
            assert_eq!(published.on_this_pc[2].status, GameSaveStatus::Error);
            assert!(published.on_this_pc.iter().all(|game| !game.has_backup && game.last_backup_at.is_none() && game.snapshots.is_empty()));
            assert!(published.in_backup.is_empty());
            assert_eq!(published.backup_folder, None);
            assert_eq!(published.full_scan_at, 42, "destination changes do not force database rediscovery");
            assert_eq!(known_titles(&raw, &settings), vec!["Game", "First", "Problem", "Old game"], "the raw cache retains titles for quick refresh");
        }
    }
    #[test]
    fn matching_destination_keeps_verified_history_even_with_windows_path_spelling_changes() {
        let settings = GameSavesSettings { backup_folder: Some(r"G:\My Drive\Backups".into()), ..Default::default() };
        let mut scan = GameSavesScan { backup_folder: Some("g:/my drive/backups/".into()), on_this_pc: vec![entry("Game")], in_backup: vec![entry("Game")], ..Default::default() };
        let original = scan.clone();
        validate_destination(&mut scan, &settings);
        assert_eq!(scan, original);
    }
    fn entry(title: &str) -> GameSaveEntry {
        GameSaveEntry {
            id: format!("game-{title}"),
            title: title.into(),
            status: GameSaveStatus::BackedUp,
            platform_badges: Vec::new(),
            file_count: 1,
            total_bytes: 1,
            last_save_at: None,
            last_backup_at: None,
            paths: Vec::new(),
            auto_backup: true,
            has_local_data: true,
            has_backup: true,
            error: None,
            snapshots: Vec::new(),
            steam_id: None,
            unmapped_paths: Vec::new(),
            suggested_target: None,
        }
    }
    fn settings() -> GameSavesSettings {
        GameSavesSettings {
            roots: vec![GameRoot {
                id: "steam-1".into(),
                path: r"C:\Steam".into(),
                store: RootStore::Steam,
                source: RootSource::Automatic,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn the_cache_survives_a_round_trip_through_disk() {
        let root = std::env::temp_dir().join(format!("myle-scan-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cached = CachedScan {
            fingerprint: fingerprint(&settings()),
            scan: GameSavesScan {
                on_this_pc: vec![entry("Game")],
                full_scan_at: 42,
                ..Default::default()
            },
        };
        save(&root, &cached);
        assert_eq!(load(&root), Some(cached));
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn new_folders_or_custom_games_or_age_make_a_full_scan_due() {
        let base = settings();
        let cached = CachedScan {
            fingerprint: fingerprint(&base),
            scan: GameSavesScan {
                full_scan_at: 1_000,
                ..Default::default()
            },
        };
        assert!(!discovery_due(&cached, &base, 1_000 + 60));
        assert!(discovery_due(&cached, &base, 1_000 + DISCOVERY_EVERY));
        let mut more_roots = base.clone();
        more_roots.roots.push(GameRoot {
            id: "steam-2".into(),
            path: r"D:\SteamLibrary".into(),
            store: RootStore::Steam,
            source: RootSource::Automatic,
        });
        assert!(discovery_due(&cached, &more_roots, 1_000 + 60));
        let mut custom = base.clone();
        custom.custom_games.push(CustomGame {
            id: "custom-1".into(),
            name: "Mine".into(),
            paths: vec![r"C:\Saves".into()],
            install_path: None,
            auto_backup: true,
        });
        assert!(discovery_due(&cached, &custom, 1_000 + 60));
        // Only the backup folder changed: a quick refresh covers it.
        let mut other_backup = base.clone();
        other_backup.backup_folder = Some(r"E:\Backups".into());
        assert!(!discovery_due(&cached, &other_backup, 1_000 + 60));
    }
    #[test]
    fn known_titles_cover_both_tabs_and_custom_games_once() {
        let mut value = settings();
        value.custom_games.push(CustomGame {
            id: "custom-1".into(),
            name: "Mine".into(),
            paths: vec![r"C:\Saves".into()],
            install_path: None,
            auto_backup: true,
        });
        let scan = GameSavesScan {
            on_this_pc: vec![entry("Alpha"), entry("Mine")],
            in_backup: vec![entry("alpha"), entry("Old Game")],
            ..Default::default()
        };
        assert_eq!(known_titles(&scan, &value), vec!["Alpha", "Mine", "Old Game"]);
    }
}
