//! Discord-style updater.
//!
//! 1. `check_for_update` reads the update feed on Cloudflare R2
//!    (`UPDATE_FEED`, published by `release.yml`) and compares its version
//!    with the running one. When the feed cannot be reached it asks the
//!    GitHub Releases API instead, which `release.yml` publishes as well.
//! 2. `install_update` downloads the installer while streaming progress to
//!    the splash, verifies its SHA-256 (from the feed, or the digest GitHub
//!    publishes for every asset), runs it silently and exits the app. The
//!    installer relaunches the new version (`/R`).

use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::apps::Jobs;
use crate::download::{self, err, parse_sha256_digest};

/// GitHub repository ("owner/name") whose Releases are checked.
/// It also holds the releases of the old Electron app (v4.x); this rewrite
/// starts at 7.0.0, so those always compare as older.
pub const GITHUB_REPO: &str = "thomasthanos/MYLE";

/// Set while an update is downloading, so the startup watchdog does not show
/// the main window on top of the splash mid-update.
static UPDATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_updating() -> bool {
    UPDATING.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set while the splash is asking the feed (and maybe GitHub) for a newer
/// version. Both requests together can take longer than the watchdog waits.
static CHECKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_checking() -> bool {
    CHECKING.load(std::sync::atomic::Ordering::Relaxed)
}

/// Clears `CHECKING` however the check ends.
struct CheckingGuard;

impl CheckingGuard {
    fn start() -> Self {
        CHECKING.store(true, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}

impl Drop for CheckingGuard {
    fn drop(&mut self) {
        CHECKING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Update feed on the R2 bucket behind downloads.thomast.uk. It is a file of
/// its own: the old Electron app reads `latest.yml` from the same bucket, and
/// that is left alone.
pub const UPDATE_FEED: &str = "https://downloads.thomast.uk/latest.json";

/// Release asset to install. Its version comes from the tag or update feed.
const ASSET_NAME: &str = "MYLE.exe";
/// Installers are only ever downloaded from these two places.
const DOWNLOAD_PREFIXES: [&str; 2] = ["https://downloads.thomast.uk/", "https://github.com/"];
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const USER_AGENT: &str = "MYLE-Updater";

/// Where downloaded installers wait to run. The installer relaunches the app,
/// so the file can only be removed on a later start.
pub fn update_dir() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("MakeYourLifeEasier")
        .join("update")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    /// Version is separate because the release filename stays MYLE.exe.
    #[serde(default)]
    pub version: Option<String>,
    pub url: String,
    pub size: u64,
    /// `"sha256:<hex>"`, as reported by GitHub.
    pub digest: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum UpdateCheck {
    UpToDate {
        current: String,
        latest: String,
    },
    Available {
        current: String,
        latest: String,
        notes: String,
        asset: UpdateAsset,
    },
    NotConfigured {
        current: String,
    },
    /// Started by the previous version right after it updated us: no need to
    /// ask the network again.
    JustUpdated {
        current: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum DownloadEvent {
    Started { total: Option<u64> },
    Progress { downloaded: u64, total: Option<u64> },
    Verifying,
    Installing,
    /// The new version is in place and starting; this one closes once its
    /// window is up.
    Restarting { version: String },
}

/// The argument the previous version starts us with after a live update.
const JUST_UPDATED_ARG: &str = "--just-updated";
/// Answered once: a later "Check for updates" really checks.
static JUST_UPDATED_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `latest.json` on R2, written by `release.yml`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Feed {
    version: String,
    #[serde(default)]
    notes: String,
    installer: FeedInstaller,
}

#[derive(Debug, Deserialize)]
struct FeedInstaller {
    name: String,
    url: String,
    size: u64,
    /// Lowercase hex.
    sha256: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateCheck, String> {
    let current = app.package_info().version.clone();

    #[cfg(debug_assertions)]
    if let Some(mode) = demo::mode() {
        return demo::check(&current, &mode);
    }

    if std::env::args().any(|arg| arg == JUST_UPDATED_ARG)
        && !JUST_UPDATED_SEEN.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        return Ok(UpdateCheck::JustUpdated {
            current: current.to_string(),
        });
    }

    if GITHUB_REPO.starts_with("OWNER/") {
        return Ok(UpdateCheck::NotConfigured {
            current: current.to_string(),
        });
    }

    let _checking = CheckingGuard::start();
    let client = download::http_client(USER_AGENT)?;
    // R2 first: no rate limit, unlike GitHub's 60 anonymous calls an hour per
    // address. GitHub is the fallback while the feed is unreachable or broken.
    match check_feed(&client, &current).await {
        Ok(check) => return Ok(check),
        Err(error) => eprintln!("update feed unavailable, asking GitHub: {error}"),
    }
    check_github(&client, &current).await
}

async fn check_feed(client: &reqwest::Client, current: &Version) -> Result<UpdateCheck, String> {
    let feed: Feed = client
        .get(UPDATE_FEED)
        .header("Cache-Control", "no-cache")
        .timeout(CHECK_TIMEOUT)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    evaluate_feed(current, feed)
}

/// The newest release of the Windows app (`v1.2.3`): the repository also
/// holds MYLE Passwords' (`mobile-v1.2.3`), which may be the newest of all.
fn windows_release(releases: Vec<GhRelease>) -> Option<GhRelease> {
    releases.into_iter().find(|release| {
        let tag = release.tag_name.trim();
        !release.draft
            && !release.prerelease
            && tag.strip_prefix(['v', 'V']).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

async fn check_github(client: &reqwest::Client, current: &Version) -> Result<UpdateCheck, String> {
    let response = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=30"
        ))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(CHECK_TIMEOUT)
        .send()
        .await
        .map_err(err)?;

    // No published release yet.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateCheck::UpToDate {
            current: current.to_string(),
            latest: current.to_string(),
        });
    }

    let releases: Vec<GhRelease> = response
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    match windows_release(releases) {
        Some(release) => evaluate(current, release),
        None => Ok(UpdateCheck::UpToDate {
            current: current.to_string(),
            latest: current.to_string(),
        }),
    }
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    asset: UpdateAsset,
    on_event: Channel<DownloadEvent>,
) -> Result<(), String> {
    // Share the app-wide exclusive lane with installers and destructive
    // system actions so an updater and firmware restart cannot cross while
    // either one is waiting on the operating system.
    let _exclusive = jobs.start_exclusive("application-update")?;

    #[cfg(debug_assertions)]
    if demo::mode().is_some() {
        return demo::install(&on_event).await;
    }

    let expected = asset
        .digest
        .as_deref()
        .and_then(parse_sha256_digest)
        .ok_or("the release asset has no SHA-256 digest, refusing to install")?;
    if !allowed_download(&asset.url) {
        return Err(format!("unexpected download location: {}", asset.url));
    }
    UPDATING.store(true, std::sync::atomic::Ordering::Relaxed);
    let _reset = ResetUpdating;

    let file_name = download::file_name_from(&asset.name, ASSET_NAME);
    let path = update_dir().join(file_name);

    let mut started = false;
    let actual = download::download_to(
        &download::http_client(USER_AGENT)?,
        &asset.url,
        &path,
        Some(asset.size),
        |downloaded, total| {
            let event = if started {
                DownloadEvent::Progress { downloaded, total }
            } else {
                started = true;
                DownloadEvent::Started { total }
            };
            let _ = on_event.send(event);
        },
        || false,
    )
    .await?;

    let _ = on_event.send(DownloadEvent::Verifying);
    if actual != expected {
        let _ = tokio::fs::remove_file(&path).await;
        return Err("the downloaded update failed SHA-256 verification".into());
    }

    let _ = on_event.send(DownloadEvent::Installing);

    // The seamless way, as the old app's updater did it: everything slow
    // happens while we are still on screen. The setup swaps the files in
    // place while we run (Windows lets a running program be renamed), the
    // new version starts, and we close once its window is up.
    if let Some(exe) = installed_exe() {
        match live_install(&path).await {
            Ok(()) => {
                let _ = on_event.send(DownloadEvent::Restarting {
                    version: asset.version.clone().unwrap_or_else(|| asset_version(&asset.name)),
                });
                return hand_over(&app, &exe).await;
            }
            Err(error) => eprintln!("live update failed, using the setup window: {error}"),
        }
    }

    // Per-user install: no UAC prompt. `/P` shows the setup's progress
    // window, which starts at once, waits for this app to quit, and opens the
    // new version half a second before it closes itself. `/UPDATE` keeps
    // shortcuts as the user left them; `/R` asks for the relaunch.
    let setup = std::process::Command::new(&path)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map_err(err)?;

    // Stay on screen until the setup's window is up, so there is never a
    // moment with neither; the setup waits for us to quit before it copies.
    let pid = setup.id();
    let shown = tokio::task::spawn_blocking(move || wait_for_window(pid, Duration::from_secs(10)))
        .await
        .unwrap_or(false);
    if !shown {
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    app.exit(0);
    Ok(())
}

/// This program, when it runs from its install folder (the setup leaves its
/// file list there). A copy elsewhere, a dev build, would only restart itself.
pub(crate) fn installed_exe() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent()?.join("install.json").is_file().then_some(exe)
}

/// Runs the setup silently in live mode and waits for it: it only swaps
/// files, so a minute is far more than it needs.
async fn live_install(setup: &std::path::Path) -> Result<(), String> {
    let setup = setup.to_path_buf();
    let run = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&setup)
            .args(["/S", "/UPDATE", "/LIVE"])
            .status()
    });
    let status = tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .map_err(|_| "the setup took too long".to_string())?
        .map_err(err)?
        .map_err(err)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the setup exited with {status}"))
    }
}

/// After a live update the previous version's files that were still in use
/// stay behind as `*.myle-old`. Once it has quit they can go: a few tries,
/// off the main thread.
pub fn sweep_leftovers_later() {
    let Some(dir) = installed_exe().and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    std::thread::spawn(move || {
        for wait in [3, 20, 90] {
            std::thread::sleep(Duration::from_secs(wait));
            if !sweep(&dir) {
                return;
            }
        }
    });
}

/// Deletes `*.myle-old` under `dir`; true if any is still there.
fn sweep(dir: &std::path::Path) -> bool {
    let mut left = false;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            // The app's own data lives in the install folder too.
            Ok(kind) if kind.is_dir() && entry.file_name() == "data" => {}
            Ok(kind) if kind.is_dir() => left |= sweep(&path),
            Ok(_) if entry.file_name().to_string_lossy().ends_with(".myle-old") => {
                left |= std::fs::remove_file(&path).is_err();
            }
            _ => {}
        }
    }
    left
}

/// Supports older feeds whose installer filename contained the version.
fn asset_version(name: &str) -> String {
    name.split('_').nth(1).unwrap_or_default().to_string()
}

/// Starts the new version and quits once its window is on screen, so one of
/// the two is always visible. The single-instance lock goes first, or the new
/// version would just hand its arguments to us and exit.
async fn hand_over(app: &AppHandle, exe: &std::path::Path) -> Result<(), String> {
    let (done, released) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        tauri_plugin_single_instance::destroy(&handle);
        let _ = done.send(());
    })
    .map_err(err)?;
    let _ = released.await;

    let child = std::process::Command::new(exe)
        .arg(JUST_UPDATED_ARG)
        .current_dir(exe.parent().unwrap_or(exe))
        .spawn()
        .map_err(|e| format!("the new version could not be started: {e}"))?;
    let pid = child.id();
    let _ = tokio::task::spawn_blocking(move || wait_for_window(pid, Duration::from_secs(15))).await;
    app.exit(0);
    Ok(())
}

/// Waits until `pid` has a visible window, for at most `timeout`. False if it
/// never did, or exited first.
fn wait_for_window(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Search {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn visit(window: HWND, search: LPARAM) -> i32 {
        // SAFETY: `search` is the struct passed to EnumWindows below.
        let search = unsafe { &mut *(search as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
            search.found = true;
            return 0;
        }
        1
    }

    // SAFETY: the handle is closed before returning.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return false;
    }
    let deadline = std::time::Instant::now() + timeout;
    let shown = loop {
        let mut search = Search { pid, found: false };
        // SAFETY: `search` outlives the synchronous enumeration.
        unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
        if search.found {
            break true;
        }
        let exited = unsafe { WaitForSingleObject(process, 50) } == WAIT_OBJECT_0;
        if exited || std::time::Instant::now() >= deadline {
            break false;
        }
    };
    unsafe { CloseHandle(process) };
    shown
}

/// Clears the flag however `install_update` ends.
struct ResetUpdating;

impl Drop for ResetUpdating {
    fn drop(&mut self) {
        UPDATING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

fn allowed_download(url: &str) -> bool {
    DOWNLOAD_PREFIXES.iter().any(|prefix| url.starts_with(prefix))
}

fn evaluate_feed(current: &Version, feed: Feed) -> Result<UpdateCheck, String> {
    let latest = parse_tag(&feed.version)?;
    if latest <= *current {
        return Ok(UpdateCheck::UpToDate {
            current: current.to_string(),
            latest: latest.to_string(),
        });
    }
    let installer = feed.installer;
    if !allowed_download(&installer.url) {
        return Err(format!("the update feed points outside the download hosts: {}", installer.url));
    }
    let digest = format!("sha256:{}", installer.sha256.trim());
    if parse_sha256_digest(&digest).is_none() {
        return Err("the update feed has no valid SHA-256 for the installer".into());
    }
    Ok(UpdateCheck::Available {
        current: current.to_string(),
        latest: latest.to_string(),
        notes: feed.notes,
        asset: UpdateAsset {
            name: installer.name,
            version: Some(latest.to_string()),
            url: installer.url,
            size: installer.size,
            digest: Some(digest),
        },
    })
}

fn evaluate(current: &Version, release: GhRelease) -> Result<UpdateCheck, String> {
    let latest = parse_tag(&release.tag_name)?;
    if latest <= *current {
        return Ok(UpdateCheck::UpToDate {
            current: current.to_string(),
            latest: latest.to_string(),
        });
    }
    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .ok_or_else(|| format!("release {} has no {ASSET_NAME} asset", release.tag_name))?;
    Ok(UpdateCheck::Available {
        current: current.to_string(),
        latest: latest.to_string(),
        notes: release.body.unwrap_or_default(),
        asset: UpdateAsset {
            name: asset.name,
            version: Some(latest.to_string()),
            url: asset.browser_download_url,
            size: asset.size,
            digest: asset.digest,
        },
    })
}

fn parse_tag(tag: &str) -> Result<Version, String> {
    let raw = tag.trim().trim_start_matches(['v', 'V']);
    Version::parse(raw).map_err(|e| format!("release tag {tag:?} is not a version: {e}"))
}

/// Debug builds only: `MYLE_UPDATER_DEMO=1` fakes an available update and a
/// download so the splash can be tested without a real release;
/// `MYLE_UPDATER_DEMO=offline` fakes a failed check.
#[cfg(debug_assertions)]
mod demo {
    use super::*;

    pub fn mode() -> Option<String> {
        std::env::var("MYLE_UPDATER_DEMO")
            .ok()
            .filter(|v| !v.is_empty())
    }

    pub fn check(current: &Version, mode: &str) -> Result<UpdateCheck, String> {
        if mode == "offline" {
            return Err("error sending request for url (https://api.github.com/…) (demo)".into());
        }
        Ok(UpdateCheck::Available {
            current: current.to_string(),
            latest: "9.9.9".into(),
            notes: String::new(),
            asset: UpdateAsset {
                name: ASSET_NAME.into(),
                version: Some("9.9.9".into()),
                url: String::new(),
                size: 7_400_000,
                digest: None,
            },
        })
    }

    pub async fn install(on_event: &Channel<DownloadEvent>) -> Result<(), String> {
        const TOTAL: u64 = 7_400_000;
        let _ = on_event.send(DownloadEvent::Started { total: Some(TOTAL) });
        for step in 1..=40u64 {
            tokio::time::sleep(Duration::from_millis(60)).await;
            let _ = on_event.send(DownloadEvent::Progress {
                downloaded: TOTAL * step / 40,
                total: Some(TOTAL),
            });
        }
        let _ = on_event.send(DownloadEvent::Verifying);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = on_event.send(DownloadEvent::Installing);
        tokio::time::sleep(Duration::from_millis(900)).await;
        let _ = on_event.send(DownloadEvent::Restarting {
            version: "9.9.9".into(),
        });
        tokio::time::sleep(Duration::from_millis(700)).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_the_installer_name() {
        assert_eq!(asset_version("MYLE_8.2.0_x64-setup.exe"), "8.2.0");
        assert_eq!(asset_version("setup.exe"), "");
    }

    fn release(tag: &str, assets: serde_json::Value) -> GhRelease {
        serde_json::from_value(
            serde_json::json!({ "tag_name": tag, "body": "notes", "assets": assets }),
        )
        .unwrap()
    }

    fn setup_asset() -> serde_json::Value {
        serde_json::json!([
            { "name": "latest.json", "browser_download_url": "https://github.com/o/r/latest.json", "size": 1, "digest": null },
            {
                "name": "MYLE.exe",
                "browser_download_url": "https://github.com/o/r/releases/download/v1.2.0/MYLE.exe",
                "size": 4200000,
                "digest": "sha256:ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
            }
        ])
    }

    #[test]
    fn newer_tag_is_available_with_the_setup_asset() {
        let current = Version::new(1, 1, 0);
        let UpdateCheck::Available {
            latest,
            asset,
            notes,
            ..
        } = evaluate(&current, release("v1.2.0", setup_asset())).unwrap()
        else {
            panic!("expected an update");
        };
        assert_eq!(latest, "1.2.0");
        assert_eq!(notes, "notes");
        assert_eq!(asset.name, "MYLE.exe");
        assert_eq!(asset.version.as_deref(), Some("1.2.0"));
        assert_eq!(asset.size, 4_200_000);
    }

    #[test]
    fn the_phone_apps_releases_are_passed_over() {
        let list: Vec<GhRelease> = serde_json::from_value(serde_json::json!([
            { "tag_name": "mobile-v9.2.4", "assets": [] },
            { "tag_name": "v9.3.0-beta.1", "prerelease": true, "assets": [] },
            { "tag_name": "v9.2.2", "body": "notes", "assets": setup_asset() },
            { "tag_name": "v9.2.1", "assets": [] }
        ]))
        .unwrap();
        assert_eq!(windows_release(list).unwrap().tag_name, "v9.2.2");
        let only_phones: Vec<GhRelease> =
            serde_json::from_value(serde_json::json!([{ "tag_name": "mobile-v1.0.0" }])).unwrap();
        assert!(windows_release(only_phones).is_none());
    }

    #[test]
    fn same_or_older_tag_is_up_to_date() {
        let current = Version::new(1, 2, 0);
        assert!(matches!(
            evaluate(&current, release("v1.2.0", setup_asset())),
            Ok(UpdateCheck::UpToDate { .. })
        ));
        assert!(matches!(
            evaluate(&current, release("1.1.9", setup_asset())),
            Ok(UpdateCheck::UpToDate { .. })
        ));
    }

    #[test]
    fn prerelease_sorts_below_release() {
        let current = Version::parse("1.2.0-beta.1").unwrap();
        assert!(matches!(
            evaluate(&current, release("v1.2.0", setup_asset())),
            Ok(UpdateCheck::Available { .. })
        ));
    }

    #[test]
    fn the_old_electron_releases_never_look_like_an_update() {
        // The repository's latest release until v7 ships: v4.8.1, with
        // electron-builder assets and no *_x64-setup.exe at all.
        let old = release(
            "v4.8.1",
            serde_json::json!([
                { "name": "latest.yml", "browser_download_url": "https://github.com/o/r/latest.yml", "size": 564 },
                { "name": "MakeYourLifeEasier-Setup.exe", "browser_download_url": "https://github.com/o/r/s.exe", "size": 87022576 }
            ]),
        );
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(current.major >= 7);
        assert!(matches!(
            evaluate(&current, old),
            Ok(UpdateCheck::UpToDate { .. })
        ));
    }

    fn feed(version: &str, url: &str, sha256: &str) -> Feed {
        serde_json::from_value(serde_json::json!({
            "version": version,
            "notes": "notes",
            "pubDate": "2026-09-27T00:00:00Z",
            "installer": {
                "name": "MYLE.exe",
                "url": url,
                "size": 13_606_875,
                "sha256": sha256
            }
        }))
        .unwrap()
    }

    const SHA: &str = "0d4767f0bb3520ee6060bbc444c3c9851ba58888c2cb6f23c711e67910b89824";

    #[test]
    fn a_newer_feed_offers_the_r2_installer_with_its_hash() {
        let url = "https://downloads.thomast.uk/MYLE.exe";
        let UpdateCheck::Available { latest, asset, .. } =
            evaluate_feed(&Version::new(7, 0, 0), feed("7.1.0", url, SHA)).unwrap()
        else {
            panic!("expected an update");
        };
        assert_eq!(latest, "7.1.0");
        assert_eq!(asset.url, url);
        assert_eq!(asset.name, "MYLE.exe");
        assert_eq!(asset.version.as_deref(), Some("7.1.0"));
        assert_eq!(asset.digest.as_deref(), Some(&*format!("sha256:{SHA}")));
    }

    #[test]
    fn the_feed_is_not_trusted_blindly() {
        let current = Version::new(7, 0, 0);
        let good = "https://downloads.thomast.uk/MYLE.exe";
        assert!(matches!(
            evaluate_feed(&current, feed("7.0.0", good, SHA)),
            Ok(UpdateCheck::UpToDate { .. })
        ));
        assert!(evaluate_feed(&current, feed("7.1.0", "https://evil.example/x.exe", SHA)).is_err());
        assert!(evaluate_feed(&current, feed("7.1.0", good, "not-a-hash")).is_err());
        assert!(evaluate_feed(&current, feed("latest", good, SHA)).is_err());
    }

    #[test]
    fn missing_setup_asset_is_an_error() {
        let current = Version::new(1, 0, 0);
        assert!(evaluate(&current, release("v2.0.0", serde_json::json!([]))).is_err());
    }

    #[test]
    fn bad_tag_is_an_error() {
        assert!(parse_tag("nightly").is_err());
        assert_eq!(parse_tag(" V3.4.5 ").unwrap(), Version::new(3, 4, 5));
    }

    #[test]
    fn check_result_serializes_for_the_frontend() {
        let json = serde_json::to_value(UpdateCheck::NotConfigured {
            current: "0.1.0".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "status": "notConfigured", "current": "0.1.0" })
        );
        let json = serde_json::to_value(DownloadEvent::Progress {
            downloaded: 5,
            total: Some(10),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "event": "progress", "data": { "downloaded": 5, "total": 10 } })
        );
        let json = serde_json::to_value(DownloadEvent::Verifying).unwrap();
        assert_eq!(json, serde_json::json!({ "event": "verifying" }));
    }
}
