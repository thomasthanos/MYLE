// ! Finds the current download URL for an app, with a 6-hour cache. ! Shared by the custom apps of the Install Apps page and by Creative Hub.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use crate::download::{self, err};
const USER_AGENT: &str = "MakeYourLifeEasier-Apps";
const CACHE_TTL_SECS: u64 = 6 * 60 * 60;
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Resolver {
    /// Latest GitHub release; `asset` is a regex on the asset file name.
    Github { repo: String, asset: String },
    /// First match of `pattern` in the vendor's page; an optional
    /// `(?P<version>…)` group gives the version.
    Page { url: String, pattern: String },
    /// A fixed URL (vendor CDN, Cloudflare R2, Backblaze B2, Dropbox…).
    Static { url: String },
    /// A file shared from Google Drive ("Anyone with the link").
    Gdrive {
        /// The file id, or the whole share link pasted from the browser.
        file_id: String,
        /// Name to save it as; Drive does not put it in the URL.
        file_name: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub url: String,
    pub file_name: String,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub size: Option<u64>,
    pub fetched_at: u64,
}
static CACHE: LazyLock<Mutex<Option<HashMap<String, Resolved>>>> =
    LazyLock::new(|| Mutex::new(None));
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn is_fresh(fetched_at: u64, now: u64) -> bool {
    now >= fetched_at && now - fetched_at < CACHE_TTL_SECS
}
fn cache_file(app: &AppHandle) -> Option<PathBuf> {
    let _ = app;
    crate::storage::local_dir()
        .ok()
        .map(|d| d.join("resolver-cache.json"))
}
fn cached(app: &AppHandle, key: &str, resolver: &Resolver) -> Option<Resolved> {
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let map = cache.get_or_insert_with(|| {
        cache_file(app)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    });
    map.get(key)
        .filter(|r| is_fresh(r.fetched_at, now_secs()))
        .filter(|r| conforms_to(r, resolver))
        .cloned()
}
/// Cache files are user-writable and therefore only a performance hint. A
/// cached result must obey the same URL/name constraints as a live resolver.
fn conforms_to(resolved: &Resolved, resolver: &Resolver) -> bool {
    if !resolved.url.starts_with("https://")
        || download::file_name_from(&resolved.file_name, "setup.exe") != resolved.file_name
    {
        return false;
    }
    match resolver {
        Resolver::Github { repo, asset } => {
            resolved
                .url
                .starts_with(&format!("https://github.com/{repo}/releases/download/"))
                && Regex::new(asset)
                    .is_ok_and(|pattern| full_match(&pattern, &resolved.file_name))
        }
        Resolver::Page { pattern, .. } => {
            Regex::new(pattern).is_ok_and(|pattern| full_match(&pattern, &resolved.url))
        }
        Resolver::Static { url } => resolved.url == *url,
        Resolver::Gdrive { file_id, file_name } => {
            gdrive_url(file_id).as_deref() == Ok(resolved.url.as_str())
                && download::file_name_from(file_name.as_deref().unwrap_or(""), "package.zip")
                    == resolved.file_name
        }
    }
}
fn full_match(pattern: &Regex, value: &str) -> bool {
    pattern
        .find(value)
        .is_some_and(|matched| matched.start() == 0 && matched.end() == value.len())
}
fn store(app: &AppHandle, key: &str, resolved: &Resolved) {
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    map.insert(key.to_string(), resolved.clone());
    if let (Some(file), Ok(json)) = (cache_file(app), serde_json::to_string(map)) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, json);
    }
}
pub async fn resolve(app: &AppHandle, key: &str, resolver: &Resolver) -> Result<Resolved, String> {
    if let Some(hit) = cached(app, key, resolver) {
        return Ok(hit);
    }
    let client = download::http_client(USER_AGENT)?;
    let resolved = match resolver {
        Resolver::Github { repo, asset } => {
            let body = client
                .get(format!(
                    "https://api.github.com/repos/{repo}/releases/latest"
                ))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .map_err(err)?
                .error_for_status()
                .map_err(err)?
                .text()
                .await
                .map_err(err)?;
            from_github(&body, asset)?
        }
        Resolver::Page { url, pattern } => {
            let body = client
                .get(url)
                .send()
                .await
                .map_err(err)?
                .error_for_status()
                .map_err(err)?
                .text()
                .await
                .map_err(err)?;
            from_page(&body, pattern)?
        }
        Resolver::Static { url } => Resolved {
            url: url.clone(),
            file_name: download::file_name_from(url, "setup.exe"),
            version: None,
            digest: None,
            size: None,
            fetched_at: 0,
        },
        Resolver::Gdrive { file_id, file_name } => Resolved {
            url: gdrive_url(file_id)?,
            file_name: download::file_name_from(file_name.as_deref().unwrap_or(""), "package.zip"),
            version: None,
            digest: None,
            size: None,
            fetched_at: 0,
        },
    };
    if !resolved.url.starts_with("https://") {
        return Err(format!("refusing a non-HTTPS download: {}", resolved.url));
    }
    let resolved = Resolved {
        fetched_at: now_secs(),
        ..resolved
    };
    store(app, key, &resolved);
    Ok(resolved)
}
/// Google Drive's direct-download endpoint. `confirm=t` skips the "can't scan
/// this file for viruses" page that large files otherwise get.
/// Accepts a bare file id or a pasted share link.
pub fn gdrive_url(file_id_or_link: &str) -> Result<String, String> {
    let id = gdrive_file_id(file_id_or_link).ok_or_else(|| {
        format!("{file_id_or_link:?} is not a Google Drive file id or share link")
    })?;
    Ok(format!(
        "https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t"
    ))
}
/// `https://drive.google.com/file/d/<id>/view?usp=sharing` -> `<id>`, and
/// `…?id=<id>` or a bare id are taken as they are.
fn gdrive_file_id(input: &str) -> Option<String> {
    let is_id = |s: &str| {
        (10..=128).contains(&s.len())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let input = input.trim();
    if is_id(input) {
        return Some(input.to_string());
    }
    let after_d = input
        .split("/d/")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next());
    let query_id = input
        .split("id=")
        .nth(1)
        .and_then(|rest| rest.split(['&', '#']).next());
    after_d
        .or(query_id)
        .filter(|s| is_id(s))
        .map(str::to_string)
}
fn from_github(release_json: &str, asset_pattern: &str) -> Result<Resolved, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        size: u64,
        #[serde(default)]
        digest: Option<String>,
    }
    let release: Release = serde_json::from_str(release_json).map_err(err)?;
    let pattern = Regex::new(asset_pattern).map_err(err)?;
    let asset = release
        .assets
        .into_iter()
        .find(|a| pattern.is_match(&a.name))
        .ok_or_else(|| {
            format!(
                "release {} has no asset matching {asset_pattern}",
                release.tag_name
            )
        })?;
    Ok(Resolved {
        url: asset.browser_download_url,
        file_name: download::file_name_from(&asset.name, "setup.exe"),
        version: Some(release.tag_name.trim_start_matches(['v', 'V']).to_string()),
        digest: asset.digest,
        size: Some(asset.size),
        fetched_at: 0,
    })
}
fn from_page(html: &str, pattern: &str) -> Result<Resolved, String> {
    let re = Regex::new(pattern).map_err(err)?;
    let caps = re
        .captures(html)
        .ok_or("the download link was not found on the vendor's page")?;
    let url = caps[0].to_string();
    Ok(Resolved {
        file_name: download::file_name_from(&url, "setup.exe"),
        version: caps.name("version").map(|m| m.as_str().to_string()),
        url,
        digest: None,
        size: None,
        fetched_at: 0,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn github_resolver_picks_the_matching_asset() {
        let json = r#"{"tag_name":"16.7","assets":[
            {"name":"Optimizer-16.7.zip","browser_download_url":"https://github.com/x/zip","size":1},
            {"name":"Optimizer-16.7.exe","browser_download_url":"https://github.com/hellzerg/optimizer/releases/download/16.7/Optimizer-16.7.exe","size":2048,"digest":null}]}"#;
        let r = from_github(json, r"^Optimizer-[\d.]+\.exe$").unwrap();
        assert_eq!(r.file_name, "Optimizer-16.7.exe");
        assert_eq!(r.version.as_deref(), Some("16.7"));
        assert_eq!(r.size, Some(2048));
        assert!(from_github(json, r"^nothing$").is_err());
    }
    #[test]
    fn page_resolver_extracts_url_and_version() {
        let html = r#"<a href="https://us.download.nvidia.com/nvapp/client/11.0.9.251/NVIDIA_app_v11.0.9.251.exe">Download</a>"#;
        let pattern = r"https://us\.download\.nvidia\.com/nvapp/client/(?P<version>[\d.]+)/NVIDIA_app_v[\d.]+\.exe";
        let r = from_page(html, pattern).unwrap();
        assert_eq!(r.version.as_deref(), Some("11.0.9.251"));
        assert_eq!(r.file_name, "NVIDIA_app_v11.0.9.251.exe");
        assert!(from_page("<html></html>", pattern).is_err());
    }
    #[test]
    fn resolver_json_uses_camel_case_fields() {
        let gdrive: Resolver = serde_json::from_str(
            r#"{"type":"gdrive","fileId":"1-lMdu7mGgCr6arl8KlaQOrQyuNpJM9ho","fileName":"a.zip"}"#,
        )
        .unwrap();
        let Resolver::Gdrive { file_id, file_name } = gdrive else {
            panic!("expected gdrive")
        };
        assert_eq!(file_id, "1-lMdu7mGgCr6arl8KlaQOrQyuNpJM9ho");
        assert_eq!(file_name.as_deref(), Some("a.zip"));
    }
    #[test]
    fn gdrive_urls_are_direct_downloads() {
        assert_eq!(
            gdrive_url("1A2b3C4d5E6f7G8h9I0jKlMnOpQrStUv").unwrap(),
            "https://drive.usercontent.google.com/download?id=1A2b3C4d5E6f7G8h9I0jKlMnOpQrStUv&export=download&confirm=t"
        );
        assert!(gdrive_url("short").is_err());
        assert!(gdrive_url("../../etc&x=1").is_err());
    }
    #[test]
    fn gdrive_accepts_pasted_share_links() {
        let id = "1Czx07cO2thXO8zkjplye_da2_h8_WSqB";
        for input in [
            id,
            &format!("  {id} "),
            &format!("https://drive.google.com/file/d/{id}/view?usp=sharing"),
            &format!("https://drive.google.com/open?id={id}"),
            &format!("https://drive.usercontent.google.com/download?id={id}&export=download"),
        ] {
            assert_eq!(
                gdrive_file_id(input).as_deref(),
                Some(id),
                "failed for {input}"
            );
        }
        assert_eq!(
            gdrive_file_id("https://drive.google.com/drive/folders/abc"),
            None
        );
        assert_eq!(gdrive_file_id(""), None);
    }
    #[test]
    fn cache_freshness() {
        assert!(is_fresh(1000, 1000));
        assert!(is_fresh(1000, 1000 + CACHE_TTL_SECS - 1));
        assert!(!is_fresh(1000, 1000 + CACHE_TTL_SECS));
        assert!(!is_fresh(2000, 1000)); // clock went backwards
    }
    #[test]
    fn cached_results_must_still_match_the_configured_resolver() {
        let page = Resolver::Page {
            url: "https://www.nvidia.com/en-us/software/nvidia-app/".into(),
            pattern: r"https://us\.download\.nvidia\.com/nvapp/client/[\d.]+/NVIDIA_app_v[\d.]+\.exe".into(),
        };
        let valid = Resolved {
            url: "https://us.download.nvidia.com/nvapp/client/11.0/NVIDIA_app_v11.0.exe".into(),
            file_name: "NVIDIA_app_v11.0.exe".into(),
            version: None,
            digest: None,
            size: None,
            fetched_at: 1,
        };
        assert!(conforms_to(&valid, &page));
        assert!(!conforms_to(
            &Resolved {
                url: "https://attacker.invalid/payload.exe".into(),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                ..valid.clone()
            },
            &page
        ));
        assert!(!conforms_to(
            &Resolved {
                url: format!("https://attacker.invalid/?next={}", valid.url),
                ..valid.clone()
            },
            &page
        ));
        assert!(!conforms_to(
            &Resolved {
                file_name: r"..\..\payload.exe".into(),
                ..valid
            },
            &page
        ));
    }
}
