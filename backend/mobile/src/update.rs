//! A newer MYLE Passwords for Android: `mobile-latest.json` next to the
//! Windows app's feed (published by `.github/workflows/mobile.yml`).
//!
//! The app downloads the APK itself into its own cache folder (no browser,
//! nothing left in Downloads), checks its SHA-256 against the feed, and
//! opens Android's installer over it (`tauri-plugin-myle-mobile`); Android
//! installs it over this one because both are signed with the same key.
//! Each version has its own address, so no cache anywhere can hand out the
//! previous APK, and an update already downloaded is not downloaded again.
//! iPhones get theirs through the SideStore source.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_myle_mobile::{ALLOW_INSTALLS, MobileExt};

pub const FEED: &str = "https://downloads.thomast.uk/mobile-latest.json";
/// Sent while an update downloads: `UpdateProgress`.
const PROGRESS_EVENT: &str = "mobile://update-progress";
const USER_AGENT: &str = "MYLE-Passwords-Updater";

#[derive(Deserialize)]
struct Feed {
    version: String,
    notes: String,
    android: Option<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    url: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub version: String,
    /// The release's page.
    pub notes: String,
    /// The APK.
    pub url: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: u64,
}

/// The app's own folder for downloaded updates (inside its cache, which the
/// template's FileProvider shares with Android's installer).
fn update_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("updates"))
}

fn apk_name(version: &str) -> String {
    format!("MYLE-Passwords-{version}.apk")
}

/// The version an APK in the update folder is for.
fn apk_version(path: &Path) -> Option<semver::Version> {
    let name = path.file_name()?.to_str()?;
    semver::Version::parse(name.strip_prefix("MYLE-Passwords-")?.strip_suffix(".apk")?).ok()
}

/// At start: removes the updates that are installed now (or older), and
/// anything left half downloaded. A newer one, not installed yet, stays.
pub fn cleanup_old_updates(app: &AppHandle) {
    if let Ok(dir) = update_dir(app) {
        remove_installed(&dir, &app.package_info().version);
    }
}

fn remove_installed(dir: &Path, current: &semver::Version) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        let stale = match apk_version(&path) {
            Some(version) => version <= *current,
            None => true,
        };
        if stale && path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_of(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hex(&hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

/// A newer version than this one, if there is one (Android only).
#[tauri::command]
pub async fn mobile_update_check(app: AppHandle) -> Result<Option<Update>, String> {
    if !cfg!(target_os = "android") {
        return Ok(None);
    }
    let feed: Feed = myle_vault::http::client(USER_AGENT)?
        .get(FEED)
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(newer(&app.package_info().version, feed))
}

/// Downloads the update into the app's own folder (sending `PROGRESS_EVENT`)
/// and checks it; answers its path. One already downloaded is reused.
#[tauri::command]
pub async fn mobile_update_download(app: AppHandle, update: Update) -> Result<String, String> {
    let version = semver::Version::parse(&update.version).map_err(|e| e.to_string())?;
    let expected = update.sha256.as_deref().map(|hash| hash.trim().to_ascii_lowercase());
    let dir = update_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let destination = dir.join(apk_name(&version.to_string()));

    // Downloaded before (the user stepped away from the installer).
    if destination.is_file() && expected.is_some() && sha256_of(&destination) == expected {
        return Ok(destination.to_string_lossy().into_owned());
    }
    // Only this update's file stays.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for path in entries.flatten().map(|entry| entry.path()) {
            let _ = std::fs::remove_file(path);
        }
    }

    let response = myle_vault::http::client(USER_AGENT)?
        .get(&update.url)
        .header("Cache-Control", "no-cache")
        .timeout(Duration::from_secs(15 * 60))
        .send()
        .await
        .map_err(|e| format!("The download did not start: {e}"))?
        .error_for_status()
        .map_err(|e| format!("The download was refused: {e}"))?;
    let total = response.content_length().or(update.size).unwrap_or(0);
    let partial = destination.with_extension("apk.part");
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut last_sent = Instant::now();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&partial);
                return Err(format!("The download stopped: {e}"));
            }
        };
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        if last_sent.elapsed() >= Duration::from_millis(100) {
            let _ = app.emit(PROGRESS_EVENT, UpdateProgress { downloaded, total });
            last_sent = Instant::now();
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    let _ = app.emit(PROGRESS_EVENT, UpdateProgress { downloaded, total: total.max(downloaded) });

    let actual = hex(&hasher.finalize());
    if expected.as_deref().is_some_and(|expected| !expected.is_empty() && expected != actual) {
        let _ = std::fs::remove_file(&partial);
        return Err("The downloaded update is not the published one. Try again in a moment.".into());
    }
    std::fs::rename(&partial, &destination).map_err(|e| e.to_string())?;
    Ok(destination.to_string_lossy().into_owned())
}

/// Opens Android's installer over the downloaded update. Answers
/// `"allow-installs"` when Android's setting for it opened instead.
#[tauri::command]
pub async fn mobile_update_install(app: AppHandle, path: String) -> Result<(), String> {
    let inside = update_dir(&app)?;
    if !Path::new(&path).starts_with(&inside) {
        return Err("That is not a downloaded update.".into());
    }
    tauri::async_runtime::spawn_blocking(move || app.myle_mobile().install(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| if e.contains(ALLOW_INSTALLS) { ALLOW_INSTALLS.to_string() } else { e })
}

fn newer(current: &semver::Version, feed: Feed) -> Option<Update> {
    let offered = semver::Version::parse(feed.version.trim_start_matches('v')).ok()?;
    let android = feed.android?;
    (offered > *current && android.url.starts_with("https://")).then(|| Update {
        version: offered.to_string(),
        notes: feed.notes,
        url: android.url,
        size: android.size,
        sha256: android.sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(version: &str, url: &str) -> Feed {
        serde_json::from_value(serde_json::json!({
            "version": version,
            "notes": "https://github.com/thomasthanos/MYLE/releases/tag/mobile-v1.1.0",
            "android": { "url": url, "size": 1, "sha256": "00" },
        }))
        .unwrap()
    }

    #[test]
    fn only_a_newer_apk_over_https_is_offered() {
        let current = semver::Version::new(1, 0, 0);
        let apk = "https://downloads.thomast.uk/MYLE-Passwords-1.1.0.apk";
        let update = newer(&current, feed("1.1.0", apk)).unwrap();
        assert_eq!(update.version, "1.1.0");
        assert_eq!(update.sha256.as_deref(), Some("00"));
        assert!(newer(&current, feed("1.0.0", apk)).is_none());
        assert!(newer(&current, feed("0.9.0", apk)).is_none());
        assert!(newer(&current, feed("1.1.0", "http://example.com/x.apk")).is_none());
        assert!(newer(&current, feed("soon", apk)).is_none());
    }

    #[test]
    fn installed_updates_are_removed_and_a_newer_one_stays() {
        let dir = std::env::temp_dir().join(format!("myle-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["MYLE-Passwords-9.2.4.apk", "MYLE-Passwords-9.2.5.apk", "MYLE-Passwords-9.2.6.apk", "MYLE-Passwords-9.2.6.apk.part", "old.apk"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        remove_installed(&dir, &semver::Version::new(9, 2, 5));
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["MYLE-Passwords-9.2.6.apk"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
