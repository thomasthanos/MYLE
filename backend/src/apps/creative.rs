//! Creative Hub: self-hosted packages (a zip or an installer) that the app
//! downloads with progress, unpacks, runs, and then cleans up after.
//!
//! The list lives in `catalog/creative-apps.json`, which is compiled into the
//! binary: every user gets exactly what ships, and nothing on the machine can
//! change it. The page only ever sends an id, so URLs, passwords and commands
//! never come from the webview.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::jobs::{JobHandle, Jobs};
use super::process::hidden;
use super::resolver::{self, Resolver};
use super::{Cleanup, JobEvent, JobOutcome, Stage};
use crate::download::{self, CANCELLED, err, parse_sha256_digest};

const DEFAULT_CATALOG: &str = include_str!("../../catalog/creative-apps.json");
const USER_AGENT: &str = "MakeYourLifeEasier-Apps";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreativeApp {
    id: String,
    name: String,
    description: String,
    category: String,
    icon: Option<String>,
    /// Where the package comes from. Left out while the entry is a placeholder.
    source: Option<Resolver>,
    setup: Setup,
    /// Shown before the download starts, and used when the server sends no size.
    size_hint: Option<u64>,
    /// Optional `"sha256:…"` of the package.
    digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Setup {
    /// Unpack the zip into a temp folder and run `run` (auto-detected if absent).
    Zip {
        run: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        /// For encrypted archives.
        password: Option<String>,
        /// Pull only `run` out of the archive instead of everything. Right for
        /// a self-contained installer inside a large zip.
        #[serde(default)]
        only_run: bool,
    },
    /// Run the downloaded installer directly.
    Installer {
        #[serde(default)]
        args: Vec<String>,
    },
    /// Unpack the zip into a folder and keep it there (photos, presets, packs).
    /// `to` defaults to `%USERPROFILE%\Downloads\<name>` and may use `%VAR%`.
    Extract {
        to: Option<String>,
        password: Option<String>,
    },
}

impl Setup {
    fn action_label(&self) -> &'static str {
        match self {
            Setup::Extract { .. } => "Download & Extract",
            _ => "Download & Setup",
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog

/// The list is fixed: it is compiled into the app and cannot be edited from
/// the outside, so what ships is exactly what every user gets.
///
/// A broken list costs the Creative Hub its cards, never the whole app: the
/// command runs on the main thread, where a panic aborts the process at start.
/// `bundled_catalog_parses` is what catches a bad file before a release.
fn catalog() -> Vec<CreativeApp> {
    parse_catalog(DEFAULT_CATALOG).unwrap_or_else(|e| {
        eprintln!("catalog/creative-apps.json is invalid: {e}");
        Vec::new()
    })
}

/// The list is private: the public repository ships the file empty, and a
/// build made from it has no Creative Hub cards rather than a broken list.
fn parse_catalog(text: &str) -> serde_json::Result<Vec<CreativeApp>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(text)
}

fn find(id: &str) -> Result<CreativeApp, String> {
    catalog()
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("unknown item: {id}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppInfo {
    id: String,
    name: String,
    description: String,
    category: String,
    icon: Option<String>,
    size_hint: Option<u64>,
    /// False while the entry has no download link yet.
    configured: bool,
    action_label: String,
}

#[tauri::command]
pub fn creative_catalog() -> Vec<CreativeAppInfo> {
    catalog()
        .into_iter()
        .map(|a| CreativeAppInfo {
            configured: a.source.is_some(),
            action_label: a.setup.action_label().to_string(),
            icon: a.icon.as_deref().and_then(resolve_icon),
            id: a.id,
            name: a.name,
            description: a.description,
            category: a.category,
            size_hint: a.size_hint,
        })
        .collect()
}

/// Icons bigger than this are ignored: a card icon is ~46 px.
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;

/// Turns the `icon` field into something the page can render:
/// * `https://…`, `data:…` — used as they are
/// * `/icons/app.svg` — a file shipped in the app's own `frontend/public/` folder
/// * any other path (`%VAR%` expanded) — read from disk into a data URL, so
///   the webview needs no file-system access
fn resolve_icon(icon: &str) -> Option<String> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with("https://")
        || icon.starts_with("http://")
        || icon.starts_with("data:")
        || icon.starts_with('/')
    {
        return Some(icon.to_string());
    }
    icon_data_url(&PathBuf::from(super::custom::expand_env(icon)))
}

fn icon_data_url(path: &Path) -> Option<String> {
    let mime = match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        _ => return None,
    };
    if std::fs::metadata(path).ok()?.len() > MAX_ICON_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    ))
}

// ---------------------------------------------------------------------------
// Download & setup

#[tauri::command]
pub async fn creative_install(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    cleanup: State<'_, Cleanup>,
    id: String,
    on_event: Channel<JobEvent>,
) -> Result<JobOutcome, String> {
    let entry = find(&id)?;
    let job = jobs.start(&id)?;
    // Everything lands in Downloads, where the user can see it, and stays
    // there: an installer that relaunches itself must not have its file
    // deleted from under it.
    let work_dir = downloads_dir();
    tokio::fs::create_dir_all(&work_dir).await.map_err(err)?;

    let result = run(&app, &job, &entry, &on_event, &work_dir, &cleanup).await;

    match result {
        Err(e) if e == CANCELLED || job.is_cancelled() => Ok(JobOutcome::Cancelled),
        other => other,
    }
}

async fn run(
    app: &AppHandle,
    job: &JobHandle,
    entry: &CreativeApp,
    on_event: &Channel<JobEvent>,
    work_dir: &Path,
    cleanup: &Cleanup,
) -> Result<JobOutcome, String> {
    let source = entry
        .source
        .as_ref()
        .ok_or("this item has no download link yet")?;

    let _ = on_event.send(JobEvent::Stage {
        stage: Stage::Resolving,
    });
    let resolved = resolver::resolve(app, &format!("creative:{}", entry.id), source).await?;

    let _ = on_event.send(JobEvent::Stage {
        stage: Stage::Downloading,
    });
    let file = work_dir.join(&resolved.file_name);
    let _ = on_event.send(JobEvent::File {
        name: resolved.file_name.clone(),
        total: resolved.size.or(entry.size_hint),
    });
    // The package itself is temporary: it goes when the app closes.
    cleanup.add(file.clone());
    let expected = entry.digest.as_deref().and_then(parse_sha256_digest);
    let mut last = -1.0f64;
    let mut next_unsized = 0u64;
    let hash = download::download_file(
        &download::http_client(USER_AGENT)?,
        &resolved.url,
        &file,
        resolved.size.or(entry.size_hint),
        |done, total| match total.filter(|t| *t > 0) {
            Some(total) => {
                let fraction = done as f64 / total as f64;
                if fraction - last >= 0.005 || fraction >= 1.0 {
                    last = fraction;
                    let _ = on_event.send(JobEvent::Progress {
                        fraction,
                        downloaded: Some(done),
                        total: Some(total),
                    });
                }
            }
            // Size unknown (or the catalog's guess was too small): the card
            // still shows how much has arrived, every few megabytes.
            None if done >= next_unsized => {
                next_unsized = done + 8 * 1024 * 1024;
                let _ = on_event.send(JobEvent::Progress {
                    fraction: 0.0,
                    downloaded: Some(done),
                    total: None,
                });
            }
            None => {}
        },
        || job.is_cancelled(),
        expected.is_some(),
    )
    .await?;

    if let Some(expected) = expected {
        let _ = on_event.send(JobEvent::Stage {
            stage: Stage::Verifying,
        });
        if hash.as_deref() != Some(expected.as_str()) {
            return Err("The download failed its SHA-256 check.".into());
        }
    }

    let (target, args) = match &entry.setup {
        Setup::Installer { args } => (file.clone(), args.clone()),
        Setup::Extract { to, password } => {
            let _ = on_event.send(JobEvent::Stage {
                stage: Stage::Extracting,
            });
            let dest = extract_destination(to.as_deref(), &entry.name);
            let (zip, out, pw, events) = (
                file.clone(),
                dest.clone(),
                password.clone(),
                on_event.clone(),
            );
            tauri::async_runtime::spawn_blocking(move || {
                super::custom::extract_zip(&zip, &out, pw.as_deref(), unpack_progress(events))
            })
            .await
            .map_err(err)??;
            // Show the user where it landed.
            let _ = hidden(super::process::explorer()).arg(&dest).spawn();
            return Ok(JobOutcome::Done {
                note: Some(format!("Saved to {}", dest.display())),
            });
        }
        Setup::Zip {
            run,
            args,
            password,
            only_run,
        } => {
            let _ = on_event.send(JobEvent::Stage {
                stage: Stage::Extracting,
            });
            let unpacked = work_dir.join(folder_name(&entry.name));
            let existed = unpacked.exists();
            let (zip, dest, pw) = (file.clone(), unpacked.clone(), password.clone());

            // A self-contained installer inside a big archive: pull out that one
            // file instead of unpacking thousands.
            let target = match (only_run, run) {
                (true, Some(name)) => {
                    let (name, pw, events) = (name.clone(), pw.clone(), on_event.clone());
                    tauri::async_runtime::spawn_blocking(move || {
                        super::custom::extract_zip_entry(
                            &zip,
                            &name,
                            &dest,
                            pw.as_deref(),
                            unpack_progress(events),
                        )
                    })
                    .await
                    .map_err(err)??
                }
                (_, run) => {
                    let events = on_event.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        super::custom::extract_zip(
                            &zip,
                            &dest,
                            pw.as_deref(),
                            unpack_progress(events),
                        )
                    })
                    .await
                    .map_err(err)??;
                    match run {
                        Some(name) => unpacked.join(name),
                        None => find_setup(&unpacked)
                            .ok_or("no installer (.exe or .msi) was found inside the package")?,
                    }
                }
            };
            if !target.exists() {
                return Err(format!("{} is not in the package", target.display()));
            }
            // Only clean up a folder we created: never touch one that was
            // already sitting in Downloads.
            if !existed {
                cleanup.add(unpacked.clone());
            }
            (target, args.clone())
        }
    };

    if job.is_cancelled() {
        return Err(CANCELLED.into());
    }
    let _ = on_event.send(JobEvent::Stage {
        stage: Stage::Installing,
    });
    let note = super::custom::run_installer(job, &target, &args).await?;
    let where_to = format!("Downloaded to {}", file.display());
    Ok(JobOutcome::Done {
        note: Some(note.map_or(where_to.clone(), |n| format!("{n} {where_to}"))),
    })
}

/// Picks the installer inside an unpacked package: prefers names that look like
/// a setup, and only looks at the top two levels.
fn find_setup(dir: &Path) -> Option<PathBuf> {
    fn collect(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 {
                    collect(&path, depth - 1, out);
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"))
            {
                out.push(path);
            }
        }
    }
    let mut found = Vec::new();
    collect(dir, 1, &mut found);
    let score = |p: &PathBuf| {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        u8::from(name.contains("setup") || name.contains("install")) * 2
            + u8::from(name.ends_with(".msi"))
    };
    found.sort_by(|a, b| {
        score(b)
            .cmp(&score(a))
            .then_with(|| a.as_os_str().len().cmp(&b.as_os_str().len()))
    });
    found.into_iter().next()
}

/// Reports unpack progress, at most once per percent.
fn unpack_progress(events: Channel<JobEvent>) -> impl FnMut(u64, u64) {
    let mut last = -1.0f64;
    move |done, total| {
        if total == 0 {
            return;
        }
        let fraction = (done as f64 / total as f64).min(1.0);
        if fraction - last >= 0.01 || fraction >= 1.0 {
            last = fraction;
            let _ = events.send(JobEvent::Progress {
                fraction,
                downloaded: Some(done),
                total: Some(total),
            });
        }
    }
}

/// The user's Downloads folder.
fn downloads_dir() -> PathBuf {
    PathBuf::from(super::custom::expand_env(r"%USERPROFILE%\Downloads"))
}

/// Where an `extract` package is unpacked: the configured folder, or
/// `%USERPROFILE%\Downloads\<name>`.
fn extract_destination(to: Option<&str>, name: &str) -> PathBuf {
    match to {
        Some(path) => PathBuf::from(super::custom::expand_env(path)),
        None => downloads_dir().join(folder_name(name)),
    }
}

/// A folder name Windows accepts, from an app's display name.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "Package".into()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------------------
// Clip Studio Paint: exe swap (revert to bundled version after a CELSYS update)

/// The source exe that our zip ships — extracted into Downloads by `creative_install`.
const CSP_RESTORE_EXE: &str = "CLIPStudioPaint-restoreversion.exe";
/// The canonical exe that Clip Studio runs.
const CSP_LIVE_EXE: &str = "CLIPStudioPaint.exe";
/// Parent company folder under Program Files.
const CSP_VENDOR: &str = "CELSYS";
/// Sub-folder that holds the actual paint exe. Version prefix changes on updates.
const CSP_SUBFOLDER_SUFFIX: &str = "CLIP STUDIO PAINT";
/// The zip unpacks into a folder named after the catalog entry.
const CSP_UNPACK_FOLDER: &str = "Clip Studio Paint EX";

/// Finds the Clip Studio install directory, ignoring the version number in the
/// path (`CLIP STUDIO 1.5`, `CLIP STUDIO 2.0`, …).
///
/// Returns the directory that contains `CLIPStudioPaint.exe`, or an error.
fn find_clip_studio_paint_dir() -> Result<std::path::PathBuf, String> {
    let vendor = PathBuf::from(r"C:\Program Files").join(CSP_VENDOR);
    let entries = std::fs::read_dir(&vendor)
        .map_err(|_| format!("Cannot read {:?} — is Clip Studio installed?", vendor))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase();
        if !name.starts_with("CLIP STUDIO") {
            continue;
        }
        let paint_dir = path.join(CSP_SUBFOLDER_SUFFIX);
        if paint_dir.join(CSP_LIVE_EXE).exists() {
            return Ok(paint_dir);
        }
    }
    Err(format!(
        "Clip Studio Paint installation not found under {:?}. \
         Please install it first.",
        vendor
    ))
}

/// Returns `true` when the restore exe (extracted by `creative_install`) is
/// present in Downloads and the swap button should be shown.
#[tauri::command]
pub fn creative_clip_studio_restore_available() -> bool {
    downloads_dir()
        .join(CSP_UNPACK_FOLDER)
        .join(CSP_RESTORE_EXE)
        .exists()
}

/// Swaps the live `CLIPStudioPaint.exe` with the bundled restore version.
///
/// The source exe is the one extracted into Downloads when the user ran
/// "Download & Setup" for Clip Studio. Elevation is required because the
/// target lives under `C:\Program Files`.
#[tauri::command]
pub async fn creative_clip_studio_swap_exe() -> Result<String, String> {
    // Where the zip was unpacked — mirrors `downloads_dir()` + `folder_name()`.
    let source = downloads_dir()
        .join(CSP_UNPACK_FOLDER)
        .join(CSP_RESTORE_EXE);

    if !source.exists() {
        return Err(format!(
            "Restore exe not found at {:?}.\n\
             Run \"Download & Setup\" for Clip Studio Paint first so the file \
             is available in your Downloads folder.",
            source
        ));
    }

    let paint_dir = find_clip_studio_paint_dir()?;
    let target = paint_dir.join(CSP_LIVE_EXE);

    // Build a PowerShell snippet that copies the file with admin rights.
    // Both paths are fixed — no user input ever reaches this script.
    let script = format!(
        "Copy-Item -LiteralPath '{src}' -Destination '{dst}' -Force",
        src = source.to_string_lossy().replace('\'', "''"),
        dst = target.to_string_lossy().replace('\'', "''"),
    );
    let script = format!(
        "try {{ {script}; exit 0 }} catch {{ Write-Error $_.Exception.Message; exit 1 }}"
    );

    let args: Vec<String> = [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
    ]
    .into_iter()
    .map(String::from)
    .chain([super::process::encode_command(&script)])
    .collect();

    let code = super::process::run_elevated("powershell.exe", &args, true).await?;
    match code {
        0 => Ok(format!(
            "Done — {} replaced with the bundled version.",
            CSP_LIVE_EXE
        )),
        super::process::ERROR_CANCELLED => Err("UAC prompt was cancelled.".into()),
        c => Err(format!("PowerShell exited with code {c}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_parses() {
        // Empty in the public repository; whatever a build carries must parse.
        let apps = parse_catalog(DEFAULT_CATALOG).unwrap();
        for app in &apps {
            assert!(
                !app.name.is_empty() && !app.description.is_empty() && !app.category.is_empty()
            );
        }
    }

    #[test]
    fn an_empty_list_has_no_cards_and_an_entry_like_the_readme_parses() {
        assert!(parse_catalog("").unwrap().is_empty());
        assert!(parse_catalog("  \n").unwrap().is_empty());
        assert!(
            parse_catalog("[").is_err(),
            "a broken list is still reported"
        );
        let apps = parse_catalog(
            r#"[{
                "id": "creative.video", "name": "Suite", "description": "Video tools",
                "category": "Video", "sizeHint": 1500000000,
                "source": { "type": "static", "url": "https://example.com/suite.zip" },
                "setup": { "type": "zip", "run": "setup.exe", "args": [] }
            }]"#,
        )
        .unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].setup.action_label(), "Download & Setup");
    }

    #[test]
    fn icons_can_be_urls_bundled_files_or_files_on_disk() {
        assert_eq!(
            resolve_icon("https://example.com/a.png").as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(
            resolve_icon("/icons/billias.svg").as_deref(),
            Some("/icons/billias.svg")
        );
        assert_eq!(
            resolve_icon("data:image/png;base64,AAA").as_deref(),
            Some("data:image/png;base64,AAA")
        );
        assert_eq!(resolve_icon("   "), None);
        assert_eq!(resolve_icon("C:\\nope\\missing.png"), None);
        assert_eq!(resolve_icon("C:\\nope\\notes.txt"), None);

        let dir = std::env::temp_dir().join(format!("myle-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("mark.svg");
        std::fs::write(&svg, b"<svg/>").unwrap();
        assert_eq!(
            resolve_icon(&svg.display().to_string()).as_deref(),
            Some("data:image/svg+xml;base64,PHN2Zy8+")
        );

        // Also works through an environment variable in the path.
        unsafe { std::env::set_var("MYLE_ICON_TEST_DIR", &dir) };
        assert!(resolve_icon("%MYLE_ICON_TEST_DIR%\\mark.svg").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_destinations() {
        let downloads = super::super::custom::expand_env("%USERPROFILE%\\Downloads");
        assert_eq!(
            extract_destination(None, "My photos"),
            PathBuf::from(&downloads).join("My photos")
        );
        assert_eq!(
            extract_destination(None, "a/b:c"),
            PathBuf::from(&downloads).join("a_b_c")
        );
        assert_eq!(
            extract_destination(Some("%USERPROFILE%\\Downloads\\Trip"), "x"),
            PathBuf::from(super::super::custom::expand_env(
                "%USERPROFILE%\\Downloads\\Trip"
            ))
        );
        assert_eq!(folder_name("   "), "Package");
    }

    #[test]
    fn action_labels_match_the_setup_kind() {
        assert_eq!(
            Setup::Extract {
                to: None,
                password: None
            }
            .action_label(),
            "Download & Extract"
        );
        assert_eq!(
            Setup::Installer { args: vec![] }.action_label(),
            "Download & Setup"
        );
        assert_eq!(
            Setup::Zip {
                run: None,
                args: vec![],
                password: None,
                only_run: false
            }
            .action_label(),
            "Download & Setup"
        );
    }

    #[test]
    fn setup_detection_prefers_installers() {
        let dir = std::env::temp_dir().join(format!("myle-setup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        std::fs::write(dir.join("bin").join("helper.exe"), b"x").unwrap();
        assert_eq!(find_setup(&dir), Some(dir.join("bin").join("helper.exe")));
        std::fs::write(dir.join("Setup.exe"), b"x").unwrap();
        assert_eq!(find_setup(&dir), Some(dir.join("Setup.exe")));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(find_setup(&dir), None);
    }
}
