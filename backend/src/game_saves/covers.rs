//! Game covers for the Game Saves list: Steam's portrait art for every game
//! with a Steam id in the save database. Steam's own cache on this PC comes
//! first, so nothing is asked for a game Steam already shows; else, when the
//! user allows it, Steam's image server, and what comes back is kept in the
//! local data folder.
//!
//! Older games keep the cover as `library_600x900.jpg`; newer ones as
//! `<hash>/library_capsule.jpg`, a path only Steam's store service knows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

/// A cover is a JPEG of about 40 KB; anything far bigger is not one.
const MAX_COVER: usize = 600 * 1024;
/// A game Steam has no cover for is asked again after a week.
const RETRY_MISSING_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
/// Covers asked of Steam at the same time.
const AT_ONCE: usize = 4;
/// Where older games keep the cover, and the name newer ones use.
const COVER: &str = "library_600x900.jpg";
const NEW_COVER: &str = "library_capsule.jpg";
/// Games asked about in one request to Steam's store service.
const PER_REQUEST: usize = 50;
const IMAGES: &str = "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps";

/// The covers found for `ids` (Steam app ids), as `data:` URLs. With
/// `online`, the ones not on this PC are fetched from Steam first.
#[tauri::command]
pub async fn game_saves_covers(ids: Vec<u32>, online: bool) -> Result<HashMap<u32, String>, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CLEANED: AtomicBool = AtomicBool::new(false);
    let folder = crate::storage::local_dir()?.join("game-saves").join("covers");
    if !CLEANED.swap(true, Ordering::Relaxed) {
        let f = folder.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || cleanup_legacy_markers(&f)).await;
    }
    let mut ids: Vec<u32> = ids.into_iter().filter(|id| *id > 0).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.truncate(5000);
    let (mut found, missing) = {
        let folder = folder.clone();
        tauri::async_runtime::spawn_blocking(move || on_this_pc(&ids, &folder))
            .await
            .map_err(|e| e.to_string())?
    };
    if online && !missing.is_empty() {
        let client = crate::download::http_client("MYLE")?;
        let paths = cover_paths(&client, &missing).await;
        let fetched: Vec<(u32, Option<Vec<u8>>)> = futures_util::stream::iter(missing)
            .map(|id| {
                let client = client.clone();
                let path = paths.get(&id).cloned().unwrap_or_else(|| COVER.to_string());
                async move { (id, fetch(&client, id, &path).await) }
            })
            .buffer_unordered(AT_ONCE)
            .collect()
            .await;
        let _ = std::fs::create_dir_all(&folder);
        for (id, bytes) in fetched {
            match bytes {
                Some(bytes) => {
                    let _ = super::atomic::write(&folder.join(format!("{id}.jpg")), &bytes);
                    found.insert(id, data_url(&bytes));
                }
                // Not there (or not now): noted, so it is not asked again at once.
                // (`.missing`, not the first version's `.none`: those were
                // written before the newer covers could be found.)
                None => {
                    let _ = std::fs::write(folder.join(format!("{id}.missing")), b"");
                }
            }
        }
    }
    Ok(found)
}

/// The covers on this PC, and the ids still worth asking Steam for.
fn on_this_pc(ids: &[u32], folder: &Path) -> (HashMap<u32, String>, Vec<u32>) {
    let steam = steam_cache();
    let mut found = HashMap::new();
    let mut missing = Vec::new();
    for &id in ids {
        let cover = steam
            .as_ref()
            .and_then(|cache| steam_cover(cache, id))
            .or_else(|| read_cover(&folder.join(format!("{id}.jpg"))));
        if let Some(bytes) = cover {
            found.insert(id, data_url(&bytes));
        } else if !recently_missing(&folder.join(format!("{id}.missing")))
            && !recently_missing(&folder.join(format!("{id}.none")))
        {
            missing.push(id);
        }
    }
    (found, missing)
}

/// Steam's own image cache, if Steam is installed.
fn steam_cache() -> Option<PathBuf> {
    let steam = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ)
        .and_then(|key| key.get_value::<String, _>("SteamPath"))
        .map(PathBuf::from)
        .ok()
        .or_else(|| std::env::var_os("ProgramFiles(x86)").map(|dir| PathBuf::from(dir).join("Steam")))?;
    let cache = steam.join("appcache").join("librarycache");
    cache.is_dir().then_some(cache)
}

/// A game's cover in Steam's cache: under the old name, or the newer one in
/// a hashed folder of its own.
fn steam_cover(cache: &Path, id: u32) -> Option<Vec<u8>> {
    let folder = cache.join(id.to_string());
    read_cover(&folder.join(COVER)).or_else(|| {
        std::fs::read_dir(&folder)
            .ok()?
            .flatten()
            .find_map(|entry| read_cover(&entry.path().join(NEW_COVER)))
    })
}

fn read_cover(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    is_cover(&bytes).then_some(bytes)
}

/// A JPEG of a sensible size, whatever the file or the server claims.
fn is_cover(bytes: &[u8]) -> bool {
    bytes.len() > 1024 && bytes.len() <= MAX_COVER && bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn recently_missing(marker: &Path) -> bool {
    std::fs::metadata(marker)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < RETRY_MISSING_AFTER)
}

/// Each game's cover path from Steam's store service (`IStoreBrowseService`,
/// public, no key), asked for many games per request. A game it does not
/// answer for is tried under the old name.
async fn cover_paths(client: &reqwest::Client, ids: &[u32]) -> HashMap<u32, String> {
    let mut paths = HashMap::new();
    for chunk in ids.chunks(PER_REQUEST) {
        let input = serde_json::json!({
            "ids": chunk.iter().map(|id| serde_json::json!({ "appid": id })).collect::<Vec<_>>(),
            "context": { "language": "english", "country_code": "US" },
            "data_request": { "include_assets": true },
        });
        let Ok(url) = reqwest::Url::parse_with_params(
            "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/",
            &[("input_json", input.to_string())],
        ) else {
            continue;
        };
        let Ok(response) = client.get(url).timeout(Duration::from_secs(15)).send().await else {
            continue;
        };
        let Ok(body) = response.json::<serde_json::Value>().await else {
            continue;
        };
        paths.extend(parse_cover_paths(&body));
    }
    paths
}

fn parse_cover_paths(body: &serde_json::Value) -> HashMap<u32, String> {
    let items = body["response"]["store_items"].as_array().cloned().unwrap_or_default();
    items
        .iter()
        .filter_map(|item| {
            let id = u32::try_from(item["appid"].as_u64()?).ok()?;
            let path = item["assets"]["library_capsule"].as_str()?;
            safe_path(path).then(|| (id, path.to_string()))
        })
        .collect()
}

/// A path under the game's own image folder: a hash folder and a file name.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() < 200
        && !path.starts_with('/')
        && !path.contains("..")
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
}

async fn fetch(client: &reqwest::Client, id: u32, path: &str) -> Option<Vec<u8>> {
    let url = format!("{IMAGES}/{id}/{path}");
    let response = client.get(url).timeout(Duration::from_secs(15)).send().await.ok()?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n as usize > MAX_COVER) {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    is_cover(&bytes).then(|| bytes.to_vec())
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))
}

/// Removes `.none` files left by an earlier version: if the cover was since
/// downloaded (`.jpg` exists), the `.none` is just clutter; otherwise it is
/// replaced with a `.missing` marker the current code understands.
fn cleanup_legacy_markers(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("none") { continue }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if folder.join(format!("{stem}.jpg")).exists() {
            let _ = std::fs::remove_file(&path);
        } else if !folder.join(format!("{stem}.missing")).exists() {
            let _ = std::fs::rename(&path, folder.join(format!("{stem}.missing")));
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_jpegs_of_a_sensible_size_are_covers() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.resize(40 * 1024, 0);
        assert!(is_cover(&jpeg));
        assert!(!is_cover(&jpeg[..512]), "too small to be a cover");
        assert!(!is_cover(b"<!doctype html><html>Not Found</html>"));
        let mut huge = jpeg.clone();
        huge.resize(MAX_COVER + 1, 0);
        assert!(!is_cover(&huge));
    }

    #[test]
    fn a_game_without_a_cover_is_not_asked_again_for_a_week() {
        let folder = std::env::temp_dir().join(format!("myle-covers-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let marker = folder.join("4000000005.missing");
        assert!(!recently_missing(&marker));
        std::fs::write(&marker, b"").unwrap();
        assert!(recently_missing(&marker));
        // Ids no Steam game has, so Steam's own cache cannot have them either.
        let (found, missing) = on_this_pc(&[4_000_000_001, 4_000_000_003], &folder);
        assert!(found.is_empty());
        assert_eq!(missing, vec![4_000_000_001, 4_000_000_003]);
        std::fs::write(folder.join("4000000001.missing"), b"").unwrap();
        assert_eq!(on_this_pc(&[4_000_000_001, 4_000_000_003], &folder).1, vec![4_000_000_003]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn steam_answers_are_read_for_safe_cover_paths_only() {
        let body = serde_json::json!({ "response": { "store_items": [
            { "appid": 1245620, "assets": { "library_capsule": "library_600x900.jpg" } },
            { "appid": 3669870, "assets": { "library_capsule": "7d4b4f430dc4d07e6562eb445456f9615cb52f77/library_capsule.jpg" } },
            { "appid": 5, "assets": { "library_capsule": "../../evil.jpg" } },
            { "appid": 6, "assets": { "library_capsule": "https://elsewhere.example/x.jpg" } },
            { "appid": 7 },
        ] } });
        let paths = parse_cover_paths(&body);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[&3669870], "7d4b4f430dc4d07e6562eb445456f9615cb52f77/library_capsule.jpg");
    }

    #[test]
    fn newer_covers_are_found_in_steams_hashed_folders() {
        let cache = std::env::temp_dir().join(format!("myle-steam-cache-{}", std::process::id()));
        let hashed = cache.join("3669870").join("7d4b4f430dc4d07e6562eb445456f9615cb52f77");
        std::fs::create_dir_all(&hashed).unwrap();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.resize(4096, 0);
        std::fs::write(hashed.join(NEW_COVER), &jpeg).unwrap();
        assert_eq!(steam_cover(&cache, 3669870), Some(jpeg));
        assert_eq!(steam_cover(&cache, 1), None);
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// Asks Steam for real covers, old and new: `cargo test --lib covers -- --ignored`.
    #[tokio::test]
    #[ignore = "uses the network"]
    async fn steam_gives_covers_by_app_id() {
        let client = crate::download::http_client("MYLE").unwrap();
        // Elden Ring keeps the old name; Control Resonant and Black Flag
        // Resynced only have the newer hashed path.
        let paths = cover_paths(&client, &[1245620, 3669870, 3751950]).await;
        for id in [1245620, 3669870, 3751950] {
            let path = paths.get(&id).cloned().unwrap_or_else(|| COVER.to_string());
            let cover = fetch(&client, id, &path).await;
            assert!(cover.is_some_and(|cover| is_cover(&cover)), "{id}: {path}");
        }
        assert!(fetch(&client, 1, COVER).await.is_none(), "no such app");
    }

    #[test]
    fn legacy_none_markers_are_cleaned_up() {
        let folder = std::env::temp_dir().join(format!("myle-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // .none with a .jpg already present -> .none removed
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.resize(4096, 0);
        std::fs::write(folder.join("100.jpg"), &jpeg).unwrap();
        std::fs::write(folder.join("100.none"), b"").unwrap();
        // .none without .jpg or .missing -> migrated to .missing
        std::fs::write(folder.join("200.none"), b"").unwrap();
        // .none with .missing already present -> .none removed
        std::fs::write(folder.join("300.none"), b"").unwrap();
        std::fs::write(folder.join("300.missing"), b"").unwrap();

        cleanup_legacy_markers(&folder);

        assert!(!folder.join("100.none").exists(), "redundant .none not removed");
        assert!(folder.join("100.jpg").exists(), "jpg must stay");
        assert!(!folder.join("200.none").exists(), "orphan .none not migrated");
        assert!(folder.join("200.missing").exists(), "orphan .none should become .missing");
        assert!(!folder.join("300.none").exists(), "duplicate .none not removed");
        assert!(folder.join("300.missing").exists(), ".missing must stay");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
