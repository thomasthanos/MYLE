//! A newer MYLE Passwords for Android: `mobile-latest.json` next to the
//! Windows app's feed (published by `.github/workflows/mobile.yml`).
//! The app downloads the APK directly in-app, verifies its SHA-256 hash,
//! deletes any old APK files to keep device storage clean, and triggers the
//! Android Package Installer over this one (same signing key).
//! iPhones get theirs through the SideStore source.

use std::path::{Path, PathBuf};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

pub const FEED: &str = "https://downloads.thomast.uk/mobile-latest.json";

#[derive(Deserialize)]
struct Feed {
    version: String,
    notes: String,
    android: Option<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    url: String,
    size: Option<u64>,
    sha256: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub version: String,
    pub notes: String,
    /// The APK URL.
    pub url: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// Directory where in-app update APKs are stored and managed.
fn update_dir(app: &AppHandle) -> Option<PathBuf> {
    let base = app
        .path()
        .app_cache_dir()
        .or_else(|_| app.path().cache_dir())
        .ok()?;
    Some(base.join("updates"))
}

/// Deletes all leftover update APKs in the app cache and tries cleaning public Downloads.
pub fn cleanup_old_updates(app: &AppHandle) {
    if let Some(dir) = update_dir(app) {
        clean_dir_apks(&dir);
    }

    // Best-effort cleanup of any leftover APKs in Android's public Downloads directory
    #[cfg(target_os = "android")]
    {
        for path in [
            "/storage/emulated/0/Download/MYLE-Passwords.apk",
            "/sdcard/Download/MYLE-Passwords.apk",
        ] {
            let _ = std::fs::remove_file(path);
        }
        for i in 1..=5 {
            let _ = std::fs::remove_file(format!("/storage/emulated/0/Download/MYLE-Passwords ({i}).apk"));
            let _ = std::fs::remove_file(format!("/sdcard/Download/MYLE-Passwords ({i}).apk"));
        }
    }
}

fn clean_dir_apks(dir: &Path) {
    if !dir.exists() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "apk") {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// A newer version than this one, if there is one (Android only).
#[tauri::command]
pub async fn mobile_update_check(app: AppHandle) -> Result<Option<Update>, String> {
    if !cfg!(target_os = "android") {
        return Ok(None);
    }
    let feed: Feed = myle_vault::http::client("MYLE-Passwords-Updater")?
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

/// Downloads the APK directly into app cache, emits progress, verifies SHA-256,
/// and removes all previous APKs so storage is never wasted.
#[tauri::command]
pub async fn mobile_update_download(
    app: AppHandle,
    url: String,
    sha256: Option<String>,
) -> Result<String, String> {
    let dir = update_dir(&app).ok_or_else(|| "Failed to resolve cache directory".to_string())?;
    let _ = std::fs::create_dir_all(&dir);
    // Delete any older APK before writing the new one
    clean_dir_apks(&dir);

    let destination = dir.join("MYLE-Passwords-update.apk");

    let response = myle_vault::http::client("MYLE-Passwords-Updater")?
        .get(&url)
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download response error: {e}"))?;

    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::create(&destination)
        .map_err(|e| format!("Failed to create destination file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut last_progress_emit = std::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Download stream error: {e}"))?;
        use std::io::Write;
        file.write_all(&chunk)
            .map_err(|e| format!("Failed writing chunk to disk: {e}"))?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;

        if last_progress_emit.elapsed() >= std::time::Duration::from_millis(80) || downloaded == total {
            let _ = app.emit(
                "mobile://update-progress",
                UpdateProgress { downloaded, total },
            );
            last_progress_emit = std::time::Instant::now();
        }
    }

    use std::io::Write;
    file.flush()
        .map_err(|e| format!("Failed to flush file: {e}"))?;
    drop(file);

    // Verify SHA-256 if provided
    let calculated_hash = format!("{:x}", hasher.finalize());
    if let Some(expected) = sha256 {
        let expected_clean = expected.trim().to_ascii_lowercase();
        if !expected_clean.is_empty() && calculated_hash != expected_clean {
            let _ = std::fs::remove_file(&destination);
            return Err("Downloaded update failed checksum verification. File was removed.".to_string());
        }
    }

    Ok(destination.to_string_lossy().to_string())
}

/// Triggers Android's native package installer sheet over the downloaded APK.
#[tauri::command]
pub async fn mobile_update_install(app: AppHandle, path: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        #[derive(Serialize)]
        struct InstallArgs {
            path: String,
        }
        let handle = app.state::<crate::InstallerPlugin<tauri::Wry>>();
        handle
            .0
            .run_mobile_plugin::<()>("install", InstallArgs { path })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, path);
        Err("In-app APK installation is only supported on Android devices".into())
    }
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
        let apk = "https://downloads.thomast.uk/MYLE-Passwords.apk";
        assert_eq!(newer(&current, feed("1.1.0", apk)).unwrap().version, "1.1.0");
        assert!(newer(&current, feed("1.0.0", apk)).is_none());
        assert!(newer(&current, feed("0.9.0", apk)).is_none());
        assert!(newer(&current, feed("1.1.0", "http://example.com/x.apk")).is_none());
        assert!(newer(&current, feed("soon", apk)).is_none());
    }

    #[test]
    fn clean_dir_apks_removes_only_apk_files() {
        let temp = std::env::temp_dir().join(format!("myle_update_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp);
        let apk = temp.join("old.apk");
        let txt = temp.join("keep.txt");
        std::fs::write(&apk, b"fake apk").unwrap();
        std::fs::write(&txt, b"keep me").unwrap();

        clean_dir_apks(&temp);

        assert!(!apk.exists());
        assert!(txt.exists());
        let _ = std::fs::remove_dir_all(&temp);
    }
}
