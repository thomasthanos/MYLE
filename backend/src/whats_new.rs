//! "What's new": the notes of the versions an update brought, shown once on
//! the first start of a new version (and on request, from Settings).
//!
//! The notes are the GitHub release bodies, asked for without a sign-in
//! (the web view may only talk to the app, so Rust asks) and cached. Offline
//! or rate-limited, the cache and then this version's notes bundled at build
//! time (`docs/release-notes/<version>.md`) stand in; with neither, the
//! dialog says which version this is and links to its release page.
//!
//! The last version shown is kept in `whats-new.json` in the settings
//! folder. Versions before 9.9.0 never wrote it: when it is missing but
//! MYLE ran on this PC before (its settings or web view profile exist), this
//! version's notes are shown; on a first install nothing is.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::updater::GITHUB_REPO;

const STATE_FILE: &str = "whats-new.json";
const CACHE_FILE: &str = "whats-new-cache.json";
const BUNDLED: &str = include_str!(concat!(env!("OUT_DIR"), "/release-notes.md"));
const USER_AGENT: &str = "MYLE-WhatsNew";
const TIMEOUT: Duration = Duration::from_secs(8);
/// The most versions one dialog shows (a long-skipped update).
const MAX_VERSIONS: usize = 12;
/// The most versions the cache keeps.
const MAX_CACHED: usize = 40;

static PREVIOUS_INSTALL: OnceLock<bool> = OnceLock::new();

/// Called once at startup, before any window makes the web view profile:
/// whether MYLE ran on this PC before.
pub fn note_previous_install() {
    let _ = PREVIOUS_INSTALL.set(detect_previous_install());
}

fn detect_previous_install() -> bool {
    if std::env::args().any(|arg| arg == "--just-updated") {
        return true;
    }
    let profile = crate::storage::webview_dir().map(|dir| dir.join("EBWebView"));
    if profile.is_ok_and(|dir| dir.exists()) {
        return true;
    }
    crate::storage::roaming_dir()
        .ok()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .is_some_and(|mut entries| {
            entries.any(|entry| entry.is_ok_and(|entry| entry.file_name() != STATE_FILE))
        })
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeenState {
    last_seen: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Cached {
    notes: String,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionNotes {
    pub version: String,
    /// Markdown; `None` when no notes could be found.
    pub notes: Option<String>,
    /// "github", "cache", "bundled" or "none".
    pub source: &'static str,
    pub url: String,
    pub date: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub current: String,
    /// The version updated from, when known.
    pub previous: Option<String>,
    /// Newest first.
    pub versions: Vec<VersionNotes>,
    /// GitHub couldn't be asked (offline, or its limit was reached).
    pub offline: bool,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
}

fn state_path() -> Result<PathBuf, String> {
    crate::storage::roaming_dir().map(|dir| dir.join(STATE_FILE))
}

fn cache_path() -> Result<PathBuf, String> {
    crate::storage::local_dir().map(|dir| dir.join(CACHE_FILE))
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: Result<PathBuf, String>) -> T {
    path.ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: Result<PathBuf, String>, value: &T) -> Result<(), String> {
    let path = path?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, bytes).map_err(crate::download::err)?;
    std::fs::rename(&temp, &path).map_err(crate::download::err)
}

/// `v9.9.0` or `9.9.0` as a version.
fn version_of(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

fn release_url(version: &Version) -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/tag/v{version}")
}

/// What to show on this start: nothing when this version was shown already
/// (or is a first install), else the versions after the last one shown, up
/// to this one.
#[derive(Debug, PartialEq)]
enum Pending {
    Nothing,
    /// Remember this version without showing anything.
    Remember,
    Show {
        after: Option<Version>,
    },
}

fn pending(last_seen: Option<&str>, current: &Version, previous_install: bool) -> Pending {
    match last_seen.and_then(version_of) {
        Some(seen) if &seen == current => Pending::Nothing,
        // Went back to an older version: start over from this one.
        Some(seen) if &seen > current => Pending::Remember,
        Some(seen) => Pending::Show { after: Some(seen) },
        None if previous_install => Pending::Show { after: None },
        None => Pending::Remember,
    }
}

/// The releases to show, newest first: after `after` up to `current`, or
/// only `current` without `after`. Drafts never; pre-releases only when it
/// is the running version.
fn pick(
    releases: Vec<GhRelease>,
    after: Option<&Version>,
    current: &Version,
) -> Vec<(Version, GhRelease)> {
    let mut picked: Vec<(Version, GhRelease)> = releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| version_of(&release.tag_name).map(|version| (version, release)))
        .filter(|(version, release)| {
            let in_range = match after {
                Some(after) => version > after && version <= current,
                None => version == current,
            };
            in_range && (!release.prerelease || version == current)
        })
        .collect();
    picked.sort_by(|a, b| b.0.cmp(&a.0));
    picked.dedup_by(|a, b| a.0 == b.0);
    picked.truncate(MAX_VERSIONS);
    picked
}

async fn fetch(after: Option<&Version>, current: &Version) -> Result<Vec<GhRelease>, String> {
    let client = crate::download::http_client(USER_AGENT)?;
    let url = match after {
        Some(_) => format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=100"),
        None => format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/v{current}"),
    };
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(crate::download::err)?;
    if after.is_none() && response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let response = response.error_for_status().map_err(crate::download::err)?;
    if after.is_some() {
        response.json().await.map_err(crate::download::err)
    } else {
        Ok(vec![response.json().await.map_err(crate::download::err)?])
    }
}

/// The notes of the versions after `after` up to `current` (or of `current`
/// alone), from GitHub, else the cache, else the bundled notes.
fn assemble(
    fetched: Option<Vec<GhRelease>>,
    cache: &mut BTreeMap<String, Cached>,
    after: Option<&Version>,
    current: &Version,
    bundled: &str,
) -> Vec<VersionNotes> {
    let mut versions: Vec<VersionNotes> = Vec::new();
    let online = fetched.is_some();
    for (version, release) in pick(fetched.unwrap_or_default(), after, current) {
        let notes = release.body.unwrap_or_default();
        if notes.trim().is_empty() {
            continue;
        }
        cache.insert(
            version.to_string(),
            Cached {
                notes: notes.clone(),
                date: release.published_at.clone(),
            },
        );
        versions.push(VersionNotes {
            url: release_url(&version),
            version: version.to_string(),
            notes: Some(notes),
            source: "github",
            date: release.published_at,
        });
    }
    // Offline: the cached versions in the range.
    if !online {
        let mut cached: Vec<(Version, Cached)> = cache
            .iter()
            .filter_map(|(key, value)| version_of(key).map(|v| (v, value.clone())))
            .filter(|(version, _)| match after {
                Some(after) => version > after && version <= current,
                None => version == current,
            })
            .collect();
        cached.sort_by(|a, b| b.0.cmp(&a.0));
        cached.truncate(MAX_VERSIONS);
        versions.extend(cached.into_iter().map(|(version, value)| VersionNotes {
            url: release_url(&version),
            version: version.to_string(),
            notes: Some(value.notes),
            source: "cache",
            date: value.date,
        }));
    }
    // This version always has an entry: GitHub's, the cache's, the bundled
    // notes, or a line with a link.
    if !versions.iter().any(|v| v.version == current.to_string()) {
        let cached = cache.get(&current.to_string()).cloned();
        let entry = match (cached, bundled.trim().is_empty()) {
            (Some(value), _) => VersionNotes {
                url: release_url(current),
                version: current.to_string(),
                notes: Some(value.notes),
                source: "cache",
                date: value.date,
            },
            (None, false) => VersionNotes {
                url: release_url(current),
                version: current.to_string(),
                notes: Some(bundled.to_string()),
                source: "bundled",
                date: None,
            },
            (None, true) => VersionNotes {
                url: release_url(current),
                version: current.to_string(),
                notes: None,
                source: "none",
                date: None,
            },
        };
        versions.insert(0, entry);
    }
    // The cache keeps the newest versions only.
    while cache.len() > MAX_CACHED {
        let oldest = cache
            .keys()
            .min_by(|a, b| match (version_of(a), version_of(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                _ => a.cmp(b),
            })
            .cloned();
        match oldest {
            Some(key) => cache.remove(&key),
            None => break,
        };
    }
    versions
}

async fn collect(after: Option<Version>, current: &Version) -> WhatsNew {
    let fetched = fetch(after.as_ref(), current).await.ok();
    let offline = fetched.is_none();
    let mut cache: BTreeMap<String, Cached> = read_json(cache_path());
    let before = cache.len();
    let versions = assemble(fetched, &mut cache, after.as_ref(), current, BUNDLED);
    if !offline || cache.len() != before {
        let _ = write_json(cache_path(), &cache);
    }
    WhatsNew {
        current: current.to_string(),
        previous: after.map(|v| v.to_string()),
        versions,
        offline,
    }
}

fn current_version(app: &AppHandle) -> Version {
    app.package_info().version.clone()
}

/// What's new since the version last shown, once per version; `None` when
/// there is nothing to show (shown already, or a first install).
#[tauri::command]
pub async fn whats_new_pending(app: AppHandle) -> Result<Option<WhatsNew>, String> {
    let current = current_version(&app);
    let state: SeenState = read_json(state_path());
    let previous_install = *PREVIOUS_INSTALL.get_or_init(detect_previous_install);
    match pending(state.last_seen.as_deref(), &current, previous_install) {
        Pending::Nothing => Ok(None),
        Pending::Remember => {
            remember(&current)?;
            Ok(None)
        }
        Pending::Show { after } => Ok(Some(collect(after, &current).await)),
    }
}

fn remember(version: &Version) -> Result<(), String> {
    write_json(
        state_path(),
        &SeenState {
            last_seen: Some(version.to_string()),
        },
    )
}

/// The dialog was shown: not again for this version.
#[tauri::command]
pub fn whats_new_seen(app: AppHandle) -> Result<(), String> {
    remember(&current_version(&app))
}

/// This version's notes, for Settings.
#[tauri::command]
pub async fn whats_new_current(app: AppHandle) -> Result<WhatsNew, String> {
    Ok(collect(None, &current_version(&app)).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn release(tag: &str, body: &str) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            body: Some(body.into()),
            draft: false,
            prerelease: false,
            published_at: Some("2026-10-08T18:12:40Z".into()),
        }
    }

    #[test]
    fn it_shows_once_per_version_and_never_on_a_first_install() {
        let current = v("9.9.0");
        assert_eq!(pending(Some("9.9.0"), &current, true), Pending::Nothing);
        assert_eq!(
            pending(Some("9.7.0"), &current, true),
            Pending::Show {
                after: Some(v("9.7.0"))
            }
        );
        assert_eq!(pending(Some("10.0.0"), &current, true), Pending::Remember);
        // 9.8 and older never wrote it: an earlier install shows this one.
        assert_eq!(pending(None, &current, true), Pending::Show { after: None });
        assert_eq!(pending(None, &current, false), Pending::Remember);
        assert_eq!(pending(Some("garbage"), &current, false), Pending::Remember);
    }

    #[test]
    fn skipped_versions_are_all_shown_newest_first() {
        let releases = vec![
            release("v10.0.0", "future"),
            release("v9.9.0", "nine nine"),
            release("mobile-v1.2.0", "phone"),
            GhRelease {
                draft: true,
                ..release("v9.8.5", "draft")
            },
            GhRelease {
                prerelease: true,
                ..release("v9.8.2", "pre")
            },
            release("v9.8.0", "nine eight"),
            release("v9.7.0", "nine seven"),
        ];
        let picked: Vec<String> = pick(releases, Some(&v("9.7.0")), &v("9.9.0"))
            .into_iter()
            .map(|(version, _)| version.to_string())
            .collect();
        assert_eq!(picked, ["9.9.0", "9.8.0"]);
    }

    #[test]
    fn offline_the_cache_then_the_bundled_notes_then_a_link() {
        let current = v("9.9.0");
        let mut cache = BTreeMap::new();
        cache.insert(
            "9.8.0".to_string(),
            Cached {
                notes: "cached 9.8".into(),
                date: None,
            },
        );
        let shown = assemble(None, &mut cache, Some(&v("9.7.0")), &current, "bundled 9.9");
        assert_eq!(
            shown
                .iter()
                .map(|n| (n.version.as_str(), n.source))
                .collect::<Vec<_>>(),
            [("9.9.0", "bundled"), ("9.8.0", "cache")]
        );
        let none = assemble(None, &mut BTreeMap::new(), None, &current, "  ");
        assert_eq!(none[0].source, "none");
        assert_eq!(none[0].notes, None);
        assert_eq!(
            none[0].url,
            "https://github.com/thomasthanos/MYLE/releases/tag/v9.9.0"
        );
    }

    #[test]
    fn online_notes_are_cached_and_an_empty_body_falls_back() {
        let current = v("9.9.0");
        let mut cache = BTreeMap::new();
        let shown = assemble(
            Some(vec![release("v9.9.0", ""), release("v9.8.0", "nine eight")]),
            &mut cache,
            Some(&v("9.7.0")),
            &current,
            "bundled 9.9",
        );
        assert_eq!(
            shown
                .iter()
                .map(|n| (n.version.as_str(), n.source))
                .collect::<Vec<_>>(),
            [("9.9.0", "bundled"), ("9.8.0", "github")]
        );
        assert_eq!(cache["9.8.0"].notes, "nine eight");
        for i in 0..50 {
            cache.insert(
                format!("1.0.{i}"),
                Cached {
                    notes: "x".into(),
                    date: None,
                },
            );
        }
        assemble(Some(vec![]), &mut cache, None, &current, "");
        assert_eq!(cache.len(), MAX_CACHED);
        assert!(cache.contains_key("9.8.0"));
    }
}
