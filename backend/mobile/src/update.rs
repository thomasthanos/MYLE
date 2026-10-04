//! A newer MYLE Passwords for Android: `mobile-latest.json` next to the
//! Windows app's feed (published by `.github/workflows/mobile.yml`). The page
//! offers the APK, which the browser downloads and Android installs over this
//! one (same signing key). iPhones get theirs through the SideStore source.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    version: String,
    notes: String,
    /// The APK, for the browser to download.
    url: String,
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

fn newer(current: &semver::Version, feed: Feed) -> Option<Update> {
    let offered = semver::Version::parse(feed.version.trim_start_matches('v')).ok()?;
    let android = feed.android?;
    (offered > *current && android.url.starts_with("https://")).then(|| Update {
        version: offered.to_string(),
        notes: feed.notes,
        url: android.url,
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
}
