//! The Tauri commands of the Project Backups page. Every path the page sends
//! is checked here: projects are addressed by id, backups by an id from
//! their list, and the folders come from the settings.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::archive::{self, RETRY_DELAYS};
use super::compare::{self, Comparison, FileDiff, Side};
use super::engine::{self, BackupEntry, BackupOutcome, BackupRequest, Preview};
use super::rules::RuleInput;
use super::state::{Job, ProjectBackupsState, Running};
use super::store::{self, ImportReport, LastBackup, LastResult, Project, Settings};
use super::{Failure, Sink, Stage};
use crate::cloud::launch::{self, Started};
use crate::cloud::{CloudProvider, detection, onedrive};

/// How long to wait for Google Drive's drive to appear after starting it.
const DRIVE_WAIT: Duration = Duration::from_secs(120);
/// How long to wait for an online-only file once its cloud app starts.
const OFFLINE_WAIT: Duration = Duration::from_secs(60);
/// Stands for the project folder itself in a comparison.
const SOURCE_ID: &str = "source";

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum ProjectBackupsEvent {
    #[serde(rename_all = "camelCase")]
    Project {
        index: usize,
        total: usize,
        project_id: String,
        name: String,
    },
    Stage {
        stage: Stage,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        done_bytes: u64,
        total_bytes: u64,
        done_files: u64,
        total_files: u64,
    },
    Message {
        text: String,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudChoice {
    pub provider: CloudProvider,
    pub label: String,
    pub path: String,
    pub available: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    pub settings: Settings,
    pub clouds: Vec<CloudChoice>,
    pub backup_root: Option<String>,
    pub running: bool,
    pub default_exclusions: Vec<String>,
    /// Projects whose folder is not there (moved, renamed, a drive not
    /// connected).
    pub missing_sources: Vec<String>,
    /// Set on the first start, when backup_projects' projects were brought in.
    pub imported: Option<ImportReport>,
}

fn clouds() -> Vec<CloudChoice> {
    detection::detect_folders()
        .into_iter()
        .filter(|folder| {
            matches!(
                folder.provider,
                CloudProvider::GoogleDrive | CloudProvider::Dropbox
            )
        })
        .map(|folder| CloudChoice {
            provider: folder.provider,
            label: folder.label,
            available: folder.path.is_dir(),
            path: folder.path.display().to_string(),
        })
        .collect()
}

fn page_state(
    state: &ProjectBackupsState,
    settings: Settings,
    imported: Option<ImportReport>,
    clouds: Vec<CloudChoice>,
    missing_sources: Vec<String>,
) -> PageState {
    PageState {
        backup_root: settings
            .backup_root()
            .map(|root| root.display().to_string()),
        settings,
        clouds,
        running: state.is_running(),
        default_exclusions: store::default_patterns(),
        missing_sources,
        imported,
    }
}

fn missing_sources(settings: &Settings) -> Vec<String> {
    settings
        .projects
        .iter()
        .filter(|project| {
            project.source_path.trim().is_empty() || !Path::new(&project.source_path).is_dir()
        })
        .map(|project| project.id.clone())
        .collect()
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}

/// The page's data. On the first start this also picks Google Drive when it
/// is set up and brings in the projects backup_projects knows.
#[tauri::command]
pub async fn project_backups_get_state(
    state: State<'_, ProjectBackupsState>,
) -> Result<PageState, String> {
    let (settings, imported, clouds, missing) = blocking(|| -> Result<_, String> {
        let clouds = clouds();
        let _edit = store::edit_lock();
        let mut settings = store::load()?;
        let mut imported = None;
        if !settings.imported {
            if settings.provider.is_none()
                && let Some(drive) = clouds
                    .iter()
                    .find(|cloud| cloud.provider == CloudProvider::GoogleDrive)
                    .or_else(|| clouds.first())
            {
                settings.provider = Some(drive.provider);
                settings.cloud_folder = Some(drive.path.clone());
            }
            let root = settings.backup_root();
            let report = store::import(
                &mut settings,
                store::backup_projects_config().as_deref(),
                root.as_deref(),
            );
            settings.imported = true;
            store::save(&settings)?;
            imported = Some(report);
        }
        let missing = missing_sources(&settings);
        Ok((settings, imported, clouds, missing))
    })
    .await??;
    Ok(page_state(&state, settings, imported, clouds, missing))
}

/// Brings in backup_projects' projects again (those not here yet).
#[tauri::command]
pub async fn project_backups_import() -> Result<(Settings, ImportReport), String> {
    blocking(|| {
        let mut report = ImportReport::default();
        let settings = store::edit(|settings| {
            let root = settings.backup_root();
            report = store::import(
                settings,
                store::backup_projects_config().as_deref(),
                root.as_deref(),
            );
            Ok(())
        })?;
        Ok((settings, report))
    })
    .await?
}

/// Backs up to `provider`: its folder `path` when it is one of the detected
/// ones, otherwise a folder the user picks.
#[tauri::command]
pub async fn project_backups_set_provider(
    app: AppHandle,
    state: State<'_, ProjectBackupsState>,
    provider: CloudProvider,
    path: Option<String>,
) -> Result<Option<Settings>, String> {
    if state.is_running() {
        return Err("Wait for the backup to finish.".into());
    }
    if !matches!(
        provider,
        CloudProvider::GoogleDrive | CloudProvider::Dropbox
    ) {
        return Err("Choose Google Drive or Dropbox.".into());
    }
    let folder = match path {
        Some(path) => {
            let detected = blocking(clouds).await?;
            let wanted = PathBuf::from(path.trim());
            detected
                .iter()
                .find(|cloud| {
                    cloud.provider == provider
                        && super::walk::same_path(Path::new(&cloud.path), &wanted)
                })
                .map(|cloud| PathBuf::from(&cloud.path))
                .ok_or("That folder is not one of the cloud folders found on this PC.")?
        }
        None => {
            let title = format!("Choose your {} folder", provider.name());
            match pick_folder(app, title).await? {
                Some(folder) => folder,
                None => return Ok(None),
            }
        }
    };
    blocking(move || {
        store::edit(|settings| {
            settings.provider = Some(provider);
            settings.cloud_folder = Some(folder.display().to_string());
            Ok(())
        })
        .map(Some)
    })
    .await?
}

#[tauri::command]
pub async fn project_backups_pick_folder(
    app: AppHandle,
    title: String,
) -> Result<Option<String>, String> {
    Ok(pick_folder(app, title)
        .await?
        .map(|path| path.display().to_string()))
}

async fn pick_folder(app: AppHandle, title: String) -> Result<Option<PathBuf>, String> {
    blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
            .map(|path| path.into_path().map_err(|error| error.to_string()))
            .transpose()
    })
    .await?
}

/// Adds (empty id) or changes a project. `require_existing_source`: the
/// project folder must exist (the editor opened for a missing folder).
#[tauri::command]
pub async fn project_backups_save_project(
    project: Project,
    require_existing_source: bool,
) -> Result<(Settings, String), String> {
    blocking(move || {
        let mut id = String::new();
        let settings = store::edit(|settings| {
            let project = store::check_project(settings, project, require_existing_source)?;
            id = project.id.clone();
            match settings
                .projects
                .iter_mut()
                .find(|known| known.id == project.id)
            {
                Some(known) => *known = project,
                None => settings.projects.push(project),
            }
            Ok(())
        })?;
        Ok((settings, id))
    })
    .await?
}

/// Forgets a project. Its backups stay where they are.
#[tauri::command]
pub async fn project_backups_remove_project(project_id: String) -> Result<Settings, String> {
    blocking(move || {
        store::edit(|settings| {
            settings.project(&project_id)?;
            settings.projects.retain(|project| project.id != project_id);
            Ok(())
        })
    })
    .await?
}

#[tauri::command]
pub async fn project_backups_set_exclusions(
    patterns: Vec<String>,
    smart_build: bool,
    follow_gitignore: bool,
) -> Result<Settings, String> {
    blocking(move || {
        let patterns = store::clean_patterns(patterns)?;
        store::edit(|settings| {
            settings.exclusions = patterns;
            settings.smart_build = smart_build;
            settings.follow_gitignore = follow_gitignore;
            Ok(())
        })
    })
    .await?
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    /// A project's folder; without one, the folder of `project_id`.
    pub source_path: Option<String>,
    pub project_id: Option<String>,
    /// Unsaved changes to try out; the saved values otherwise.
    pub patterns: Option<Vec<String>>,
    pub extra_exclusions: Option<Vec<String>>,
    pub keep: Option<Vec<String>>,
    pub smart_build: Option<bool>,
    pub follow_gitignore: Option<bool>,
}

/// What a backup of a folder would hold and leave out, with the exclusions
/// being edited. A newer preview (or `project_backups_cancel_preview`)
/// stops this one.
#[tauri::command]
pub async fn project_backups_preview(
    state: State<'_, ProjectBackupsState>,
    request: PreviewRequest,
) -> Result<Preview, String> {
    let cancelled = state.begin_preview();
    blocking(move || {
        let settings = store::load()?;
        let project = request
            .project_id
            .as_deref()
            .map(|id| settings.project(id))
            .transpose()?;
        let source = request
            .source_path
            .clone()
            .filter(|path| !path.trim().is_empty())
            .or_else(|| project.map(|project| project.source_path.clone()))
            .ok_or("Choose the project folder first.")?;
        let source = PathBuf::from(source.trim());
        if !source.is_absolute() {
            return Err("The project folder must be a full path.".to_string());
        }
        let saved = settings.rule_input(project);
        let input = RuleInput {
            patterns: request
                .patterns
                .map(store::clean_patterns)
                .transpose()?
                .unwrap_or(saved.patterns),
            extra: request
                .extra_exclusions
                .map(store::clean_patterns)
                .transpose()?
                .unwrap_or(saved.extra),
            keep: request
                .keep
                .map(store::clean_patterns)
                .transpose()?
                .unwrap_or(saved.keep),
            smart_build: request.smart_build.unwrap_or(saved.smart_build),
            follow_gitignore: request.follow_gitignore.unwrap_or(saved.follow_gitignore),
        };
        let mut skip = Vec::new();
        skip.extend(settings.backup_root());
        skip.extend(archive::staging_dir());
        engine::preview(&source, &input, &skip, &cancelled)
    })
    .await?
}

#[tauri::command]
pub fn project_backups_cancel_preview(state: State<'_, ProjectBackupsState>) {
    state.cancel_preview();
}

// ─── Backing up ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
    pub project_id: String,
    pub name: String,
    pub ok: bool,
    /// `SOURCE_MISSING` (the page opens the editor on the folder field),
    /// `NO_PROVIDER`, `CANCELLED`.
    pub code: Option<String>,
    pub error: Option<String>,
    pub outcome: Option<BackupOutcome>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRun {
    pub results: Vec<ProjectResult>,
    pub cancelled: bool,
    pub settings: Settings,
}

struct ChannelSink<'a> {
    channel: &'a Channel<ProjectBackupsEvent>,
    running: &'a Running,
    last: std::sync::Mutex<Option<Instant>>,
}

impl Sink for ChannelSink<'_> {
    fn stage(&self, stage: Stage) {
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let _ = self.channel.send(ProjectBackupsEvent::Stage { stage });
    }

    fn progress(&self, done_bytes: u64, total_bytes: u64, done_files: u64, total_files: u64) {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let finished = done_bytes >= total_bytes;
        if !finished && last.is_some_and(|at| at.elapsed() < Duration::from_millis(120)) {
            return;
        }
        *last = Some(Instant::now());
        let _ = self.channel.send(ProjectBackupsEvent::Progress {
            done_bytes,
            total_bytes,
            done_files,
            total_files,
        });
    }

    fn cancelled(&self) -> bool {
        self.running.cancelled()
    }
}

impl<'a> ChannelSink<'a> {
    fn new(channel: &'a Channel<ProjectBackupsEvent>, running: &'a Running) -> Self {
        ChannelSink {
            channel,
            running,
            last: std::sync::Mutex::new(None),
        }
    }

    fn message(&self, text: impl Into<String>) {
        let _ = self
            .channel
            .send(ProjectBackupsEvent::Message { text: text.into() });
    }
}

/// Waits for `ready`, up to `limit`, unless cancelled.
fn wait_until(limit: Duration, sink: &dyn Sink, ready: impl Fn() -> bool) -> Result<bool, Failure> {
    let start = Instant::now();
    loop {
        if ready() {
            return Ok(true);
        }
        if sink.cancelled() {
            return Err(Failure::Cancelled);
        }
        if start.elapsed() >= limit {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The `Projects Backup` folder, ready to write to: starts Google Drive (and
/// waits for its drive) or Dropbox when needed.
fn ensure_destination(sink: &ChannelSink) -> Result<PathBuf, Failure> {
    let settings = store::load().map_err(Failure::Message)?;
    let (Some(provider), Some(folder)) = (settings.provider, settings.cloud_folder.clone()) else {
        return Err(Failure::Coded(
            "NO_PROVIDER",
            "Choose where the backups go: Google Drive or Dropbox.".into(),
        ));
    };
    let mut folder = PathBuf::from(folder);
    match provider {
        CloudProvider::GoogleDrive => {
            if !folder.is_dir() {
                sink.stage(Stage::StartingCloud);
                match launch::start_google_drive() {
                    Ok(Started::NotInstalled) => {
                        return Err(Failure::Coded(
                            "CLOUD_NOT_INSTALLED",
                            "Google Drive for desktop is not installed on this PC. Install it, or back up to Dropbox.".into(),
                        ));
                    }
                    Ok(Started::Launched) => {
                        sink.message("Starting Google Drive… (this can take up to two minutes)")
                    }
                    Ok(Started::AlreadyRunning) => {
                        sink.message("Waiting for Google Drive's drive to appear…")
                    }
                    Err(error) => return Err(Failure::Coded("CLOUD_NOT_READY", error)),
                }
                let appeared = wait_until(DRIVE_WAIT, sink, || folder.is_dir())?;
                if !appeared {
                    // The drive may have come up under another letter.
                    let drives: Vec<CloudChoice> = clouds()
                        .into_iter()
                        .filter(|cloud| {
                            cloud.provider == CloudProvider::GoogleDrive && cloud.available
                        })
                        .collect();
                    let [only] = drives.as_slice() else {
                        return Err(Failure::Coded(
                            "CLOUD_NOT_READY",
                            format!(
                                "Google Drive's folder \"{}\" did not appear within two minutes. Check that Google Drive is running and signed in, then try again.",
                                folder.display()
                            ),
                        ));
                    };
                    folder = PathBuf::from(&only.path);
                    let adopted = folder.display().to_string();
                    store::edit(|settings| {
                        settings.cloud_folder = Some(adopted);
                        Ok(())
                    })
                    .map_err(Failure::Message)?;
                    sink.message(format!("Google Drive is now at {}", folder.display()));
                }
            }
        }
        CloudProvider::Dropbox => {
            if let Ok(Started::Launched) = launch::start_dropbox() {
                sink.stage(Stage::StartingCloud);
                sink.message("Started Dropbox");
            }
            if !folder.is_dir() {
                return Err(Failure::Coded(
                    "DESTINATION_MISSING",
                    format!(
                        "The Dropbox folder \"{}\" was not found. Choose the Dropbox folder again.",
                        folder.display()
                    ),
                ));
            }
        }
        _ => {
            return Err(Failure::Coded(
                "NO_PROVIDER",
                "Choose Google Drive or Dropbox.".into(),
            ));
        }
    }
    let root = folder.join(super::naming::BACKUP_ROOT_NAME);
    std::fs::create_dir_all(&root).map_err(|error| {
        Failure::Coded(
            "DESTINATION_MISSING",
            format!("Can't create \"{}\": {error}", root.display()),
        )
    })?;
    Ok(root)
}

/// Starts the cloud app that keeps the online-only file `path` and waits
/// until the file can be read.
fn bring_online(path: &Path, sink: &ChannelSink) -> Result<bool, Failure> {
    sink.stage(Stage::StartingCloud);
    let started = onedrive::start_onedrive(&[path.to_path_buf()]).is_some()
        || detection::detect_folders().iter().any(|folder| {
            folder.provider == CloudProvider::Dropbox
                && store::inside(path, &folder.path)
                && matches!(
                    launch::start_dropbox(),
                    Ok(Started::Launched | Started::AlreadyRunning)
                )
        })
        || detection::detect_folders().iter().any(|folder| {
            folder.provider == CloudProvider::GoogleDrive
                && store::inside(path, &folder.path)
                && matches!(
                    launch::start_google_drive(),
                    Ok(Started::Launched | Started::AlreadyRunning)
                )
        });
    if !started {
        return Ok(false);
    }
    sink.message("Starting the cloud app that keeps the project's online-only files…");
    wait_until(OFFLINE_WAIT, sink, || onedrive::readable(path))
}

/// Closes `exe` (asking first, then forcing), never MYLE itself.
async fn close_program(exe: &str) {
    if store::is_own_program(exe) || !launch::process_running(exe) {
        return;
    }
    let own = format!("PID ne {}", std::process::id());
    let run = |force: bool| {
        let mut command = crate::apps::process::hidden(crate::apps::process::system32("taskkill.exe"));
        if force {
            command.arg("/F");
        }
        command
            .args(["/IM", exe, "/T", "/FI", own.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    let _ = run(false).status().await;
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if !launch::process_running(exe) {
            return;
        }
    }
    let _ = run(true).status().await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
}

/// The result code the page acts on for a failure.
fn code_of(failure: &Failure) -> Option<&'static str> {
    match failure {
        Failure::Cancelled => Some("CANCELLED"),
        Failure::SourceMissing => Some("SOURCE_MISSING"),
        Failure::CloudOffline(_) => Some("CLOUD_OFFLINE"),
        Failure::Coded(code, _) => Some(code),
        Failure::Message(_) => None,
    }
}

/// Backs up the given projects one after another. For each: the project
/// folder is checked, the destination made ready (a cloud app started),
/// only then is its program closed, and the backup made.
#[tauri::command]
pub async fn project_backups_backup(
    app: AppHandle,
    state: State<'_, ProjectBackupsState>,
    project_ids: Vec<String>,
    on_event: Channel<ProjectBackupsEvent>,
) -> Result<BackupRun, String> {
    let running = state.begin(Job::Backup)?;
    let version = app.package_info().version.to_string();
    let settings = blocking(store::load).await??;
    let projects: Vec<Project> = project_ids
        .iter()
        .map(|id| settings.project(id).cloned())
        .collect::<Result<_, _>>()?;
    if projects.is_empty() {
        return Err("Choose a project to back up.".into());
    }
    let running = std::sync::Arc::new(running);
    let mut results = Vec::new();
    let total = projects.len();
    for (index, project) in projects.into_iter().enumerate() {
        if running.cancelled() {
            break;
        }
        let _ = on_event.send(ProjectBackupsEvent::Project {
            index,
            total,
            project_id: project.id.clone(),
            name: project.name.clone(),
        });
        let result = backup_one(&project, &version, &on_event, &running).await;
        let (ok, code, error, outcome) = match result {
            Ok(outcome) => (true, None, None, Some(outcome)),
            Err(failure) => (
                false,
                code_of(&failure).map(str::to_string),
                Some(failure.text()),
                None,
            ),
        };
        // Without a destination, or cancelled, the others would fail the same way.
        let stop = matches!(
            code.as_deref(),
            Some(
                "CANCELLED"
                    | "NO_PROVIDER"
                    | "CLOUD_NOT_INSTALLED"
                    | "CLOUD_NOT_READY"
                    | "DESTINATION_MISSING"
            )
        );
        results.push(ProjectResult {
            project_id: project.id,
            name: project.name,
            ok,
            code,
            error,
            outcome,
        });
        if stop {
            break;
        }
    }
    let cancelled = running.cancelled();
    drop(running);
    let now = super::naming::unix_millis(std::time::SystemTime::now());
    let finished: Vec<(String, Option<LastBackup>, LastResult)> = results
        .iter()
        .map(|result| {
            let last = result.outcome.as_ref().map(|outcome| LastBackup {
                name: outcome.name.clone(),
                created_at: outcome.created_at,
                file_count: outcome.file_count,
                zip_size: outcome.zip_size,
            });
            let attempt = LastResult {
                at: now,
                ok: result.ok,
                cancelled: result.code.as_deref() == Some("CANCELLED"),
                code: result.code.clone(),
                error: result.error.clone(),
            };
            (result.project_id.clone(), last, attempt)
        })
        .collect();
    let settings = blocking(move || {
        store::edit(|settings| {
            for (id, last, attempt) in finished {
                if let Some(project) = settings
                    .projects
                    .iter_mut()
                    .find(|project| project.id == id)
                {
                    if last.is_some() {
                        project.last_backup = last;
                    }
                    project.last_result = Some(attempt);
                }
            }
            Ok(())
        })
    })
    .await??;
    Ok(BackupRun {
        results,
        cancelled,
        settings,
    })
}

async fn backup_one(
    project: &Project,
    version: &str,
    channel: &Channel<ProjectBackupsEvent>,
    running: &std::sync::Arc<Running>,
) -> Result<BackupOutcome, Failure> {
    if project.source_path.trim().is_empty() || !Path::new(&project.source_path).is_dir() {
        return Err(Failure::SourceMissing);
    }
    let (sink_channel, sink_running) = (channel.clone(), running.clone());
    let backup_root =
        blocking(move || ensure_destination(&ChannelSink::new(&sink_channel, &sink_running)))
            .await
            .map_err(Failure::Message)??;
    if let Some(exe) = project.close_app.as_deref() {
        let _ = channel.send(ProjectBackupsEvent::Stage {
            stage: Stage::ClosingApp,
        });
        let _ = channel.send(ProjectBackupsEvent::Message {
            text: format!("Closing {exe}…"),
        });
        close_program(exe).await;
    }
    let (job, version, channel, running) = (
        project.clone(),
        version.to_string(),
        channel.clone(),
        running.clone(),
    );
    blocking(move || run_one(&job, &backup_root, &version, &channel, &running))
        .await
        .map_err(Failure::Message)?
}

fn run_one(
    project: &Project,
    backup_root: &Path,
    version: &str,
    channel: &Channel<ProjectBackupsEvent>,
    running: &Running,
) -> Result<BackupOutcome, Failure> {
    let sink = ChannelSink::new(channel, running);
    let settings = store::load().map_err(Failure::Message)?;
    let staging = archive::staging_dir();
    let request = BackupRequest {
        project,
        rules: settings.rule_input(Some(project)),
        backup_root,
        staging: staging.as_deref(),
        app_version: version,
        delays: RETRY_DELAYS,
    };
    match engine::backup_project(&request, &sink) {
        Err(Failure::CloudOffline(path)) => {
            if !bring_online(&path, &sink)? {
                return Err(Failure::CloudOffline(path));
            }
            engine::backup_project(&request, &sink)
        }
        other => other,
    }
}

/// Cancels the running backup (`job` = `backup`) or comparison (`compare`).
#[tauri::command]
pub fn project_backups_cancel(state: State<'_, ProjectBackupsState>, job: Option<String>) -> bool {
    let job = match job.as_deref() {
        Some("backup") => Some(Job::Backup),
        Some("compare") => Some(Job::Compare),
        _ => None,
    };
    state.cancel(job)
}

/// Starts the cloud app of `provider` (an action on a failed backup).
#[tauri::command]
pub async fn project_backups_start_cloud(provider: CloudProvider) -> Result<String, String> {
    blocking(move || {
        let started = match provider {
            CloudProvider::GoogleDrive => launch::start_google_drive()?,
            CloudProvider::Dropbox => launch::start_dropbox()?,
            _ => return Err("Choose Google Drive or Dropbox.".to_string()),
        };
        Ok(match started {
            Started::Launched => format!("{} is starting.", provider.name()),
            Started::AlreadyRunning => format!("{} is already running.", provider.name()),
            Started::NotInstalled => {
                return Err(format!("{} is not installed on this PC.", provider.name()));
            }
        })
    })
    .await?
}

// ─── Listing and comparing ─────────────────────────────────────────────────

fn project_and_dir(project_id: &str) -> Result<(Settings, Project, PathBuf), String> {
    let settings = store::load()?;
    let project = settings.project(project_id)?.clone();
    let root = settings
        .backup_root()
        .ok_or("Choose where the backups go first.")?;
    let dir = engine::app_dir(&root, &project.app_name).map_err(|failure| failure.text())?;
    Ok((settings, project, dir))
}

/// Whether each project's folder differs from its newest backup.
#[tauri::command]
pub async fn project_backups_changes(project_ids: Vec<String>, fresh: bool) -> Result<Vec<super::changes::ProjectChanges>, String> {
    blocking(move || {
        Ok(project_ids
            .iter()
            .filter_map(|id| super::changes::check(id, fresh).ok())
            .collect())
    })
    .await?
}

/// (Re)starts watching the project folders for changes.
#[tauri::command]
pub async fn project_backups_watch(app: AppHandle) -> Result<(), String> {
    blocking(move || super::changes::watch(app)).await?
}

#[tauri::command]
pub async fn project_backups_list(project_id: String) -> Result<Vec<BackupEntry>, String> {
    blocking(move || {
        let (_, project, dir) = project_and_dir(&project_id)?;
        Ok(engine::list_backups(&dir, &project.app_name))
    })
    .await?
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareResult {
    /// The older of the two (`source` is always the newer side).
    pub old_id: String,
    pub new_id: String,
    pub comparison: Comparison,
}

/// The side of a comparison: a backup by id, or `source`.
fn side(project: &Project, dir: &Path, id: &str) -> Result<Side, String> {
    if id == SOURCE_ID {
        let source = PathBuf::from(&project.source_path);
        if !source.is_dir() {
            return Err("The project folder was not found.".into());
        }
        return Ok(Side::Folder(source));
    }
    compare::side_for(&engine::resolve_backup(dir, &project.app_name, id)?)
}

/// Puts the older backup first: by version, the project folder last.
fn order(dir: &Path, app_name: &str, a: String, b: String) -> (String, String) {
    if a == SOURCE_ID {
        return (b, a);
    }
    if b == SOURCE_ID {
        return (a, b);
    }
    let list = engine::list_backups(dir, app_name);
    match (
        list.iter().find(|entry| entry.id == a),
        list.iter().find(|entry| entry.id == b),
    ) {
        (Some(x), Some(y)) => {
            let (old, new) = engine::older_first(x, y);
            (old.id.clone(), new.id.clone())
        }
        _ => (a, b),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareProgress {
    pub done: u64,
    pub total: u64,
}

#[tauri::command]
pub async fn project_backups_compare(
    state: State<'_, ProjectBackupsState>,
    project_id: String,
    first_id: String,
    second_id: String,
    on_progress: Channel<CompareProgress>,
) -> Result<CompareResult, String> {
    let running = std::sync::Arc::new(state.begin(Job::Compare)?);
    blocking(move || {
        let (settings, project, dir) = project_and_dir(&project_id)?;
        let (old_id, new_id) = order(&dir, &project.app_name, first_id, second_id);
        let old = side(&project, &dir, &old_id)?;
        let new = side(&project, &dir, &new_id)?;
        let source = PathBuf::from(&project.source_path);
        let source = source.is_dir().then_some(source);
        let rules =
            super::rules::Rules::compile(&settings.rule_input(Some(&project)), source.as_deref())?;
        let cancelled = || running.cancelled();
        let last = std::sync::Mutex::new(None::<Instant>);
        let progress = |done: u64, total: u64| {
            let mut last = last.lock().unwrap_or_else(|p| p.into_inner());
            if done < total && last.is_some_and(|at| at.elapsed() < Duration::from_millis(150)) {
                return;
            }
            *last = Some(Instant::now());
            let _ = on_progress.send(CompareProgress { done, total });
        };
        let comparison =
            compare::compare(&old, &new, &rules, source.as_deref(), &cancelled, &progress)?;
        Ok(CompareResult {
            old_id,
            new_id,
            comparison,
        })
    })
    .await?
}

#[tauri::command]
pub async fn project_backups_file_diff(
    project_id: String,
    old_id: String,
    new_id: String,
    old_name: Option<String>,
    new_name: Option<String>,
) -> Result<FileDiff, String> {
    blocking(move || {
        let (_, project, dir) = project_and_dir(&project_id)?;
        let old = side(&project, &dir, &old_id)?;
        let new = side(&project, &dir, &new_id)?;
        compare::file_diff(&old, old_name.as_deref(), &new, new_name.as_deref())
    })
    .await?
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenTarget {
    /// The `Projects Backup` folder.
    Root,
    /// A project's backup folder (or one backup, selected, with `backup_id`).
    Backups,
    Source,
}

#[tauri::command]
pub async fn project_backups_open(
    target: OpenTarget,
    project_id: Option<String>,
    backup_id: Option<String>,
) -> Result<(), String> {
    let path = blocking(move || -> Result<PathBuf, String> {
        let settings = store::load()?;
        let root = settings
            .backup_root()
            .ok_or("Choose where the backups go first.");
        match target {
            OpenTarget::Root => {
                let root = root?;
                std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
                Ok(root)
            }
            OpenTarget::Backups | OpenTarget::Source => {
                let (_, project, dir) =
                    project_and_dir(project_id.as_deref().ok_or("Choose a project.")?)?;
                if matches!(target, OpenTarget::Source) {
                    let source = PathBuf::from(&project.source_path);
                    return if source.is_dir() {
                        Ok(source)
                    } else {
                        Err("The project folder was not found.".into())
                    };
                }
                match backup_id {
                    Some(id) => engine::resolve_backup(&dir, &project.app_name, &id),
                    None if dir.is_dir() => Ok(dir),
                    None => Err("This project has no backups yet.".into()),
                }
            }
        }
    })
    .await??;
    if path.is_file() {
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| error.to_string())
    } else {
        tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| error.to_string())
    }
}
