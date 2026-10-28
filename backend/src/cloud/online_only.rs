// ! Provider-managed online-only requests, for every cloud app that allows ! them. Apps on the Windows cloud files API (OneDrive, Dropbox, iCloud, ! Box…) get one request per uploaded file; Google Drive gets its own "Online ! only" command per Drive folder. No content is read or deleted.
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use serde::Serialize;
use super::google_drive::{self, CacheClear};
use super::sync_roots::{self, SyncRoot};
use super::{CloudFolder, CloudProvider};
const MAX_ERRORS: usize = 20;
const CHANGED: &str =
    "The cloud folders changed. Refresh Disk Analyzer before requesting online-only files.";
/// How long a request waits for Google Drive's storage window to be opened.
const DRIVE_WAIT: Duration = Duration::from_secs(120);
const DRIVE_POLL: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    pub visited: u64,
    pub requested: u64,
    pub already_online: u64,
    pub pending_uploads: u64,
    pub skipped: u64,
    pub failed: u64,
    /// Google Drive folders whose "Online only" was turned on.
    pub folders_requested: u64,
    /// Google Drive folders that were online-only already.
    pub folders_already_online: u64,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOnlyProgress {
    #[serde(flatten)]
    counts: Counts,
    current: Option<String>,
    /// Waiting for Google Drive's storage window, to press Clear cache there.
    waiting_for_drive: bool,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOnlyOutcome {
    #[serde(flatten)]
    counts: Counts,
    cancelled: bool,
    errors: Vec<String>,
    /// Google Drive's cache, when Drive was part of the request.
    drive_cache: Option<CacheClear>,
}
#[derive(Clone)]
struct Registration {
    root_id: u64,
    hydration: u16,
    provider_name: String,
}
#[derive(Clone)]
struct Placeholder {
    root_id: u64,
    in_sync: bool,
    modified: i64,
    on_disk: i64,
    pin: i32,
}
#[derive(Clone)]
struct Metadata {
    volume: u32,
    id: u64,
    directory: bool,
    unsafe_reparse: bool,
    links: u32,
    cloud: Option<Placeholder>,
}
trait CloudFiles {
    type Handle;
    fn open_root(&mut self, path: &Path) -> Result<Self::Handle, String>;
    fn open_child(&mut self, path: &Path) -> Result<Self::Handle, String>;
    fn metadata(&mut self, handle: &Self::Handle) -> Result<Metadata, String>;
    fn registration(&mut self, handle: &Self::Handle) -> Result<Registration, String>;
    fn entries(
        &mut self,
        handle: &Self::Handle,
        path: &Path,
    ) -> Result<Box<dyn Iterator<Item = Result<PathBuf, String>>>, String>;
    fn request_unpin(&mut self, handle: &Self::Handle) -> Result<(), String>;
}
/// Google Drive's "Online only" command (see google_drive.rs).
trait DriveMenu {
    fn is_online_only(&mut self, path: &Path) -> Result<bool, String>;
    /// False when it was on already.
    fn make_online_only(&mut self, path: &Path) -> Result<bool, String>;
    fn clear_cache(&mut self) -> Result<CacheClear, String>;
}
struct DriveShell;
impl DriveMenu for DriveShell {
    fn is_online_only(&mut self, path: &Path) -> Result<bool, String> {
        google_drive::is_online_only(path)
    }
    fn make_online_only(&mut self, path: &Path) -> Result<bool, String> {
        google_drive::make_online_only(path)
    }
    fn clear_cache(&mut self) -> Result<CacheClear, String> {
        google_drive::clear_cache()
    }
}
/// How a cloud app's files can be made online-only.
#[derive(Clone, Debug)]
enum Action {
    /// The Windows cloud files API, in these verified sync roots.
    Native(Vec<SyncRoot>),
    /// Google Drive's own Explorer command, for each of its folders.
    DriveMenu,
    /// Only with the app's own controls, for this reason.
    Guided(&'static str),
    Unavailable,
}
/// A cloud app on this PC.
#[derive(Clone, Debug)]
struct Cloud {
    /// "googleDrive", "mega", "dropbox", "oneDrive", or "sync:<app>" for any
    /// other app on the Windows cloud files API.
    id: String,
    name: String,
    /// Each folder's label and path.
    folders: Vec<(String, PathBuf)>,
    action: Action,
}
const UNAVAILABLE: &str =
    "No local cloud folder was detected. Check the desktop app's account and folder settings.";
const UNVERIFIED: &str = "Compatible Windows online-only support could not be verified for these folders. Use this cloud app's own storage controls.";
fn key(provider: CloudProvider) -> &'static str {
    match provider {
        CloudProvider::Dropbox => "dropbox",
        CloudProvider::GoogleDrive => "googleDrive",
        CloudProvider::Mega => "mega",
        CloudProvider::OneDrive => "oneDrive",
    }
}
/// The cloud apps on this PC: the four the app knows by name (always listed,
/// as the page shows them), then every other app with a Windows sync root.
fn clouds(
    detected: &[CloudFolder],
    roots: &[SyncRoot],
    files: &mut impl CloudFiles,
    drive: &mut impl DriveMenu,
) -> Vec<Cloud> {
    let detected_of = |provider| -> Vec<(String, PathBuf)> {
        detected
            .iter()
            .filter(|folder| folder.provider == provider)
            .map(|folder| (folder.label.clone(), folder.path.clone()))
            .collect()
    };
    let named = |provider: &str| -> Vec<SyncRoot> {
        roots
            .iter()
            .filter(|root| root.provider.eq_ignore_ascii_case(provider))
            .cloned()
            .collect()
    };
    let mut out = Vec::new();
    for provider in [
        CloudProvider::GoogleDrive,
        CloudProvider::Mega,
        CloudProvider::Dropbox,
        CloudProvider::OneDrive,
    ] {
        let folders = detected_of(provider);
        let id = key(provider).to_string();
        let name = provider.name().to_string();
        out.push(match provider {
            CloudProvider::Dropbox | CloudProvider::OneDrive => {
                native_cloud(id, name, named(provider.name()), folders, files)
            }
            CloudProvider::GoogleDrive => {
                let action = if folders.is_empty() {
                    Action::Unavailable
                } else if folders.iter().all(|(_, path)| drive.is_online_only(path).is_ok()) {
                    Action::DriveMenu
                } else {
                    Action::Guided(
                        "Google Drive manages offline availability and its streaming cache through its own desktop app.",
                    )
                };
                Cloud { id, name, folders, action }
            }
            CloudProvider::Mega => {
                let action = if folders.is_empty() {
                    Action::Unavailable
                } else {
                    Action::Guided(
                        "MEGA Desktop sync has no online-only conversion. MEGAcmd offers a separate on-demand mount (beta on Windows).",
                    )
                };
                Cloud { id, name, folders, action }
            }
        });
    }
    let mut others: BTreeMap<&str, Vec<SyncRoot>> = BTreeMap::new();
    for root in roots.iter().filter(|root| {
        !["OneDrive", "Dropbox"]
            .iter()
            .any(|known| root.provider.eq_ignore_ascii_case(known))
    }) {
        others.entry(&root.provider).or_default().push(root.clone());
    }
    for (provider, roots) in others {
        // "Nextcloud - name@host" names the app before its account.
        let name = roots[0].name.split(" - ").next().unwrap_or(provider).trim().to_string();
        out.push(native_cloud(format!("sync:{provider}"), name, roots, Vec::new(), files));
    }
    out
}
/// An app on the Windows cloud files API: its sync roots that pass every
/// check, or the app's own controls when none does.
fn native_cloud(
    id: String,
    name: String,
    roots: Vec<SyncRoot>,
    detected: Vec<(String, PathBuf)>,
    files: &mut impl CloudFiles,
) -> Cloud {
    let mut keys = HashSet::new();
    let roots: Vec<_> = roots
        .into_iter()
        .filter(|root| root_key(&root.path).is_ok_and(|key| keys.insert(key)))
        .collect();
    let verified: Vec<_> = roots.iter().filter(|root| verified(files, root).is_ok()).cloned().collect();
    if !verified.is_empty() {
        return Cloud {
            id,
            name,
            folders: verified.iter().map(|root| (root.name.clone(), root.path.clone())).collect(),
            action: Action::Native(verified),
        };
    }
    let mut folders: Vec<_> = roots.into_iter().map(|root| (root.name, root.path)).collect();
    for (label, path) in detected {
        if root_key(&path).is_ok_and(|key| keys.insert(key)) {
            folders.push((label, path));
        }
    }
    let action = if folders.is_empty() { Action::Unavailable } else { Action::Guided(UNVERIFIED) };
    Cloud { id, name, folders, action }
}
/// What one request covers.
#[derive(Debug, Default)]
struct Plan {
    roots: Vec<SyncRoot>,
    /// Google Drive folders.
    drive: Vec<PathBuf>,
    /// How long to wait for Drive's storage window to clear its cache.
    drive_wait: Duration,
}
/// The request for the apps `wanted`, when their folders are still exactly
/// the `expected` ones the page showed.
fn plan(clouds: &[Cloud], wanted: &[String], expected: &[String]) -> Result<Plan, String> {
    let wanted: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut plan = Plan::default();
    let mut actual = HashSet::new();
    let mut found = 0;
    for cloud in clouds.iter().filter(|cloud| wanted.contains(cloud.id.as_str())) {
        match &cloud.action {
            Action::Native(roots) => plan.roots.extend(roots.iter().cloned()),
            Action::DriveMenu => plan.drive.extend(cloud.folders.iter().map(|(_, path)| path.clone())),
            Action::Guided(_) | Action::Unavailable => {
                return Err(format!("Use {}'s storage controls for this app.", cloud.name));
            }
        }
        found += 1;
        for (_, path) in &cloud.folders {
            actual.insert(root_key(path)?);
        }
    }
    let expected: HashSet<_> = expected
        .iter()
        .map(|path| root_key(Path::new(path)))
        .collect::<Result<_, _>>()?;
    if found == 0 || found != wanted.len() || actual != expected {
        return Err(CHANGED.into());
    }
    plan.drive_wait = DRIVE_WAIT;
    Ok(plan)
}
fn execute_with(
    files: &mut impl CloudFiles,
    drive: &mut impl DriveMenu,
    plan: &Plan,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&CloudOnlyProgress) -> bool,
) -> Result<CloudOnlyOutcome, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(CloudOnlyOutcome {
            cancelled: true,
            ..Default::default()
        });
    }
    // Validate and hold every root before changing any file. Handles also hold the ancestors against rename/reparse replacement throughout traversal.
    let prepared: Vec<_> = plan
        .roots
        .iter()
        .map(|root| {
            let (handle, metadata) = verified(files, root)?;
            Ok((root, handle, metadata))
        })
        .collect::<Result<_, String>>()?;
    let mut outcome = CloudOnlyOutcome::default();
    let mut seen = HashSet::new();
    for (root, handle, metadata) in &prepared {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        Walker {
            files,
            root,
            root_id: metadata.id,
            volume: metadata.volume,
            cancel,
            progress: &mut progress,
            seen: &mut seen,
            outcome: &mut outcome,
        }
        .visit(&root.path, handle, 0);
    }
    for path in &plan.drive {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match drive.make_online_only(path) {
            Ok(true) => outcome.counts.folders_requested += 1,
            Ok(false) => outcome.counts.folders_already_online += 1,
            Err(error) => record_error(&mut outcome, path, error),
        }
        if !progress(&CloudOnlyProgress {
            counts: outcome.counts.clone(),
            current: Some(path.display().to_string()),
            waiting_for_drive: false,
        }) {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    // Online-only files still fill Drive's cache, and only Drive's own storage window empties it: wait for it to be opened, then press.
    let deadline = Instant::now() + plan.drive_wait;
    while !plan.drive.is_empty() && !cancel.load(Ordering::Relaxed) {
        match drive.clear_cache() {
            Ok(CacheClear::NotOpen) if Instant::now() < deadline => {
                if !progress(&CloudOnlyProgress {
                    counts: outcome.counts.clone(),
                    current: None,
                    waiting_for_drive: true,
                }) {
                    cancel.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(DRIVE_POLL);
            }
            Ok(found) => {
                outcome.drive_cache = Some(found);
                break;
            }
            Err(error) => {
                record_error(&mut outcome, Path::new("Google Drive"), error);
                break;
            }
        }
    }
    outcome.cancelled = cancel.load(Ordering::Relaxed);
    let _ = progress(&CloudOnlyProgress {
        counts: outcome.counts.clone(),
        current: None,
        waiting_for_drive: false,
    });
    Ok(outcome)
}
/// A sync root opened and held, once it is the registered root it claims to be.
fn verified<F: CloudFiles>(files: &mut F, root: &SyncRoot) -> Result<(F::Handle, Metadata), String> {
    let handle = files.open_root(&root.path)?;
    let metadata = files.metadata(&handle)?;
    let registration = files.registration(&handle)?;
    if !metadata.directory
        || metadata.unsafe_reparse
        || metadata.id == 0
        || metadata.id != registration.root_id
        || !compatible(root, &registration)
    {
        return Err(format!(
            "{} is not a compatible registered sync root. Refresh and use the provider's storage controls.",
            root.path.display()
        ));
    }
    Ok((handle, metadata))
}
fn record_error(outcome: &mut CloudOnlyOutcome, path: &Path, error: String) {
    outcome.counts.failed += 1;
    if outcome.errors.len() < MAX_ERRORS {
        outcome.errors.push(format!("{}: {error}", path.display()));
    }
}
fn root_key(path: &Path) -> Result<String, String> {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        || !matches!(components.next(), Some(Component::RootDir))
        || !components.all(|component| matches!(component, Component::Normal(_)))
        || path.as_os_str().is_empty()
        || path.to_string_lossy().contains('\0')
    {
        return Err(
            "A cloud folder must be an absolute local folder without traversal components.".into(),
        );
    }
    Ok(path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_start_matches(r"\\?\")
        .trim_end_matches('\\')
        .to_lowercase())
}
fn compatible(root: &SyncRoot, registration: &Registration) -> bool {
    let engine = registration.provider_name.to_lowercase();
    // OneDrive and Dropbox name their sync engines after themselves: another engine registered on their folder is left alone.
    matches!(registration.hydration, 1 | 2)
        && ["onedrive", "dropbox"]
            .into_iter()
            .find(|known| root.provider.eq_ignore_ascii_case(known))
            .is_none_or(|known| engine.contains(known))
}
struct Walker<'a, F: CloudFiles, P: FnMut(&CloudOnlyProgress) -> bool> {
    files: &'a mut F,
    root: &'a SyncRoot,
    root_id: u64,
    volume: u32,
    cancel: &'a AtomicBool,
    progress: &'a mut P,
    seen: &'a mut HashSet<(u32, u64)>,
    outcome: &'a mut CloudOnlyOutcome,
}
impl<F: CloudFiles, P: FnMut(&CloudOnlyProgress) -> bool> Walker<'_, F, P> {
    fn error(&mut self, path: &Path, error: String) {
        record_error(self.outcome, path, error);
    }
    fn emit(&mut self, path: &Path) {
        if !(self.progress)(&CloudOnlyProgress {
            counts: self.outcome.counts.clone(),
            current: Some(path.display().to_string()),
            waiting_for_drive: false,
        }) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn permitted(&mut self, handle: &F::Handle, metadata: &Metadata) -> bool {
        !metadata.unsafe_reparse
            && metadata.volume == self.volume
            && metadata.id != 0
            && self.files.registration(handle).is_ok_and(|registration| {
                registration.root_id == self.root_id && compatible(self.root, &registration)
            })
    }
    fn visit(&mut self, path: &Path, handle: &F::Handle, depth: usize) {
        if self.cancel.load(Ordering::Relaxed) {
            return;
        }
        let info = match self.files.metadata(handle) {
            Ok(info) => info,
            Err(error) => {
                self.error(path, error);
                self.emit(path);
                return;
            }
        };
        if !info.directory {
            self.outcome.counts.visited += 1;
        }
        if depth > 128
            || !self.permitted(handle, &info)
            || !self.seen.insert((info.volume, info.id))
        {
            self.outcome.counts.skipped += 1;
        } else if info.directory {
            if info.cloud.as_ref().is_some_and(|cloud| cloud.pin == 3) {
                self.outcome.counts.skipped += 1;
            } else {
                match self.files.entries(handle, path) {
                    Err(error) => self.error(path, error),
                    Ok(entries) => {
                        for child in entries {
                            if self.cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            match child {
                                Err(error) => self.error(path, error),
                                Ok(child) => {
                                    if child.parent().and_then(|parent| root_key(parent).ok())
                                        != root_key(path).ok()
                                    {
                                        self.outcome.counts.skipped += 1;
                                        continue;
                                    }
                                    match self.files.open_child(&child) {
                                        Ok(child_handle) => {
                                            self.visit(&child, &child_handle, depth + 1)
                                        }
                                        Err(error) => self.error(&child, error),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Recheck the same held object immediately before the request. A path-based second open could inspect a different file.
            match self.files.metadata(handle) {
                Err(error) => self.error(path, error),
                Ok(current) => {
                    if current.directory
                        || current.id != info.id
                        || current.links != 1
                        || !self.permitted(handle, &current)
                    {
                        self.outcome.counts.skipped += 1;
                    } else if let Some(cloud) = current.cloud.filter(|cloud| {
                        cloud.root_id == self.root_id
                            && matches!(cloud.pin, 0 | 1 | 2 | 4)
                            && cloud.on_disk >= 0
                            && cloud.modified >= 0
                    }) {
                        if !cloud.in_sync || cloud.modified != 0 {
                            self.outcome.counts.pending_uploads += 1;
                        } else if cloud.on_disk == 0 && cloud.pin == 2 {
                            self.outcome.counts.already_online += 1;
                        } else if !self.cancel.load(Ordering::Relaxed) {
                            match self.files.request_unpin(handle) {
                                Ok(()) => self.outcome.counts.requested += 1,
                                Err(error) => self.error(path, error),
                            }
                        }
                    } else {
                        self.outcome.counts.skipped += 1;
                    }
                }
            }
        }
        self.emit(path);
    }
}
// A request remains owned by the blocking worker even if its IPC future is dropped. Cancelling an old request can never affect a subsequent request.
#[derive(Clone, Default)]
pub struct OnlineOnlyState(Arc<Mutex<Jobs>>);
#[derive(Default)]
struct Jobs {
    active: Option<Job>,
    // A synchronous Stop IPC can arrive before the async Start is first polled. Remember its UUID without allowing unlimited retained cancellation data.
    early_stops: VecDeque<String>,
}
struct Job {
    id: String,
    cancel: Arc<AtomicBool>,
}
struct JobGuard {
    state: OnlineOnlyState,
    id: String,
    cancel: Arc<AtomicBool>,
}
impl OnlineOnlyState {
    fn begin(&self, id: String) -> Result<JobGuard, String> {
        uuid::Uuid::parse_str(&id).map_err(|_| "Invalid cloud request ID.".to_string())?;
        let mut job = self
            .0
            .lock()
            .map_err(|_| "Cloud request state is unavailable.".to_string())?;
        if job.active.is_some() {
            return Err("A cloud request is already running.".into());
        }
        let was_cancelled =
            if let Some(index) = job.early_stops.iter().position(|stopped| stopped == &id) {
                job.early_stops.remove(index);
                true
            } else {
                false
            };
        let cancel = Arc::new(AtomicBool::new(was_cancelled));
        job.active = Some(Job {
            id: id.clone(),
            cancel: cancel.clone(),
        });
        Ok(JobGuard {
            state: self.clone(),
            id,
            cancel,
        })
    }
    fn cancel(&self, id: &str) -> Result<(), String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid cloud request ID.".to_string())?;
        let mut job = self
            .0
            .lock()
            .map_err(|_| "Cloud request state is unavailable.".to_string())?;
        if let Some(active) = job.active.as_ref().filter(|active| active.id == id) {
            active.cancel.store(true, Ordering::Relaxed);
        } else if !job.early_stops.iter().any(|stopped| stopped == id) {
            if job.early_stops.len() == 64 {
                job.early_stops.pop_front();
            }
            job.early_stops.push_back(id.to_owned());
        }
        Ok(())
    }
}
impl Drop for JobGuard {
    fn drop(&mut self) {
        if let Ok(mut job) = self.state.0.lock()
            && job
                .active
                .as_ref()
                .is_some_and(|job| job.id == self.id && Arc::ptr_eq(&job.cancel, &self.cancel))
        {
            job.active = None;
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    providers: Vec<ProviderStatus>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderStatus {
    /// The cloud's ID (see `Cloud::id`).
    provider: String,
    name: String,
    folders: Vec<FolderStatus>,
    mode: &'static str,
    detail: &'static str,
}
#[derive(Serialize)]
struct FolderStatus {
    path: String,
    label: String,
}
impl From<Cloud> for ProviderStatus {
    fn from(cloud: Cloud) -> Self {
        let (mode, detail) = match cloud.action {
            Action::Native(_) => (
                "native",
                "Online-only requests are available. Each file's upload status is checked before requesting a change.",
            ),
            Action::DriveMenu => (
                "native",
                "Online-only requests go through Google Drive's own Offline access > Online only command for each Drive folder, then Drive's own Clear cache in its Storage on your computer window.",
            ),
            Action::Guided(detail) => ("guided", detail),
            Action::Unavailable => ("unavailable", UNAVAILABLE),
        };
        ProviderStatus {
            provider: cloud.id,
            name: cloud.name,
            mode,
            detail,
            folders: cloud
                .folders
                .into_iter()
                .map(|(label, path)| FolderStatus {
                    path: path.display().to_string(),
                    label,
                })
                .collect(),
        }
    }
}
/// The cloud apps on this PC, checked now.
fn current_clouds() -> Vec<Cloud> {
    clouds(
        &super::detection::detect_folders(),
        &sync_roots::sync_roots(),
        &mut native::WindowsFiles,
        &mut DriveShell,
    )
}
#[tauri::command]
pub async fn cloud_storage_status() -> Result<StorageStatus, String> {
    tauri::async_runtime::spawn_blocking(|| StorageStatus {
        providers: current_clouds().into_iter().map(ProviderStatus::from).collect(),
    })
    .await
    .map_err(|error| error.to_string())
}
/// Online-only requests for the cloud apps `clouds` (their IDs), all at once.
#[tauri::command]
pub async fn cloud_storage_online_only(
    clouds: Vec<String>,
    expected_folders: Vec<String>,
    request_id: String,
    on_event: tauri::ipc::Channel<CloudOnlyProgress>,
    state: tauri::State<'_, OnlineOnlyState>,
) -> Result<CloudOnlyOutcome, String> {
    let job = state.begin(request_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        if job.cancel.load(Ordering::Relaxed) {
            return Ok(CloudOnlyOutcome {
                cancelled: true,
                ..Default::default()
            });
        }
        let plan = plan(&current_clouds(), &clouds, &expected_folders)?;
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        execute_with(
            &mut native::WindowsFiles,
            &mut DriveShell,
            &plan,
            &job.cancel,
            |progress| {
                if progress.current.is_none()
                    || last.elapsed() >= std::time::Duration::from_millis(100)
                {
                    last = std::time::Instant::now();
                    on_event.send(progress.clone()).is_ok()
                } else {
                    true
                }
            },
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
pub fn cloud_storage_cancel(
    request_id: String,
    state: tauri::State<'_, OnlineOnlyState>,
) -> Result<(), String> {
    state.cancel(&request_id)
}
mod native {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::{CloudFilters::*, FileSystem::*};
    pub(super) struct WindowsFiles;
    pub(super) struct HeldFile {
        file: File,
        // Deny rename/delete of each ancestor while any child is inspected.
        _ancestors: Vec<File>,
    }
    fn open_attributes(path: &Path) -> Result<File, String> {
        OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .map_err(|error| error.to_string())
    }
    fn attribute_tag(file: &File) -> Result<FILE_ATTRIBUTE_TAG_INFO, String> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: valid attribute-only handle, writable fixed-size output.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(info)
    }
    fn unsafe_reparse(info: &FILE_ATTRIBUTE_TAG_INFO) -> bool {
        // CLOUD and CLOUD_1..CLOUD_F are placeholders, not name-surrogate links. Everything else (including junctions and symlinks) is refused.
        info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && (info.ReparseTag & !0x0000_f000) != 0x9000_001a
    }
    fn hresult(result: i32) -> Result<(), String> {
        if result < 0 {
            Err(format!(
                "Windows cloud storage error 0x{:08X}",
                result as u32
            ))
        } else {
            Ok(())
        }
    }
    fn placeholder(file: &File) -> Option<Placeholder> {
        // Aligned storage for the fixed prefix and bounded variable identity. Query errors mean "unverified", never permission to change a file.
        let mut buffer = vec![0u64; 8192];
        let mut returned = 0;
        // SAFETY: owned, aligned output, explicit buffer length, valid handle.
        let result = unsafe {
            CfGetPlaceholderInfo(
                file.as_raw_handle(),
                CF_PLACEHOLDER_INFO_STANDARD,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut returned,
            )
        };
        if result < 0
            || (returned as usize)
                < std::mem::offset_of!(CF_PLACEHOLDER_STANDARD_INFO, FileIdentity)
        {
            return None;
        }
        // SAFETY: prefix verified above; allocation also covers struct padding.
        let info = unsafe { &*buffer.as_ptr().cast::<CF_PLACEHOLDER_STANDARD_INFO>() };
        Some(Placeholder {
            root_id: info.SyncRootFileId as u64,
            in_sync: info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC,
            modified: info.ModifiedDataSize,
            on_disk: info.OnDiskDataSize,
            pin: info.PinState,
        })
    }
    impl CloudFiles for WindowsFiles {
        type Handle = HeldFile;
        fn open_root(&mut self, path: &Path) -> Result<HeldFile, String> {
            root_key(path)?;
            let mut current = PathBuf::new();
            let mut ancestors = Vec::new();
            for component in path.components() {
                current.push(component);
                if !current.has_root() {
                    continue;
                }
                let file = open_attributes(&current)?;
                let tag = attribute_tag(&file)?;
                if unsafe_reparse(&tag) || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                    return Err("Cloud folder crosses a link or is no longer a directory.".into());
                }
                ancestors.push(file);
            }
            let file = ancestors
                .pop()
                .ok_or_else(|| "Invalid cloud folder.".to_string())?;
            Ok(HeldFile {
                file,
                _ancestors: ancestors,
            })
        }
        fn open_child(&mut self, path: &Path) -> Result<HeldFile, String> {
            Ok(HeldFile {
                file: open_attributes(path)?,
                _ancestors: Vec::new(),
            })
        }
        fn metadata(&mut self, handle: &HeldFile) -> Result<Metadata, String> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: live handle and writable correctly sized structure.
            if unsafe { GetFileInformationByHandle(handle.file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let tag = attribute_tag(&handle.file)?;
            Ok(Metadata {
                volume: info.dwVolumeSerialNumber,
                id: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
                directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                unsafe_reparse: unsafe_reparse(&tag),
                links: info.nNumberOfLinks,
                cloud: placeholder(&handle.file),
            })
        }
        fn registration(&mut self, handle: &HeldFile) -> Result<Registration, String> {
            let mut buffer = vec![0u64; 8192];
            let mut returned = 0;
            // SAFETY: live attribute handle and aligned output buffer.
            hresult(unsafe {
                CfGetSyncRootInfoByHandle(
                    handle.file.as_raw_handle(),
                    CF_SYNC_ROOT_INFO_STANDARD,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                    &mut returned,
                )
            })?;
            if (returned as usize)
                < std::mem::offset_of!(CF_SYNC_ROOT_STANDARD_INFO, SyncRootIdentity)
            {
                return Err("Windows returned incomplete sync registration information.".into());
            }
            // SAFETY: prefix length validated; allocation covers struct padding.
            let info = unsafe { &*buffer.as_ptr().cast::<CF_SYNC_ROOT_STANDARD_INFO>() };
            let end = info
                .ProviderName
                .iter()
                .position(|ch| *ch == 0)
                .unwrap_or(info.ProviderName.len());
            Ok(Registration {
                root_id: info.SyncRootFileId as u64,
                hydration: info.HydrationPolicy.Primary,
                provider_name: String::from_utf16_lossy(&info.ProviderName[..end]),
            })
        }
        fn entries(
            &mut self,
            _: &HeldFile,
            path: &Path,
        ) -> Result<Box<dyn Iterator<Item = Result<PathBuf, String>>>, String> {
            // Only directory names are enumerated. File content is never read.
            let entries = std::fs::read_dir(path).map_err(|error| error.to_string())?;
            Ok(Box::new(entries.map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| error.to_string())
            })))
        }
        fn request_unpin(&mut self, handle: &HeldFile) -> Result<(), String> {
            // This is user intent only. The provider owns upload consistency and any eventual eviction; never force-dehydrate or delete data. SAFETY: held, verified placeholder handle; no recursive flag.
            hresult(unsafe {
                CfSetPinState(
                    handle.file.as_raw_handle(),
                    CF_PIN_STATE_UNPINNED,
                    CF_SET_PIN_FLAG_NONE,
                    std::ptr::null_mut(),
                )
            })
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    struct FakeFiles {
        nodes: HashMap<PathBuf, Metadata>,
        children: HashMap<PathBuf, Vec<PathBuf>>,
        changed: HashMap<PathBuf, Metadata>,
        reads: HashMap<PathBuf, usize>,
        requested: Vec<PathBuf>,
        registration: Registration,
        unsafe_root: bool,
        fail_requests: bool,
    }
    impl FakeFiles {
        fn new() -> Self {
            Self {
                nodes: HashMap::from([(PathBuf::from(r"C:\cloud"), folder(100))]),
                children: HashMap::new(),
                changed: HashMap::new(),
                reads: HashMap::new(),
                requested: Vec::new(),
                registration: Registration {
                    root_id: 100,
                    hydration: 2,
                    provider_name: "Microsoft OneDrive".into(),
                },
                unsafe_root: false,
                fail_requests: false,
            }
        }
        fn add(&mut self, name: &str, metadata: Metadata) -> PathBuf {
            let path = PathBuf::from(r"C:\cloud").join(name);
            self.nodes.insert(path.clone(), metadata);
            self.children
                .entry(PathBuf::from(r"C:\cloud"))
                .or_default()
                .push(path.clone());
            path
        }
        fn run(&mut self, cancel: &AtomicBool) -> Result<CloudOnlyOutcome, String> {
            execute_with(self, &mut FakeDrive::default(), &native_plan(&[r"C:\cloud"]), cancel, |_| true)
        }
    }
    #[derive(Default)]
    struct FakeDrive {
        online: HashSet<PathBuf>,
        failing: HashSet<PathBuf>,
        changed: Vec<PathBuf>,
        /// What each look at Drive's storage window finds; then Empty.
        storage: VecDeque<Result<CacheClear, String>>,
        storage_looks: usize,
    }
    impl DriveMenu for FakeDrive {
        fn is_online_only(&mut self, path: &Path) -> Result<bool, String> {
            if self.failing.contains(path) {
                return Err("Drive's menu changed".into());
            }
            Ok(self.online.contains(path))
        }
        fn make_online_only(&mut self, path: &Path) -> Result<bool, String> {
            if self.is_online_only(path)? {
                return Ok(false);
            }
            self.changed.push(path.to_path_buf());
            Ok(true)
        }
        fn clear_cache(&mut self) -> Result<CacheClear, String> {
            self.storage_looks += 1;
            self.storage.pop_front().unwrap_or(Ok(CacheClear::Empty))
        }
    }
    fn with_storage(looks: impl IntoIterator<Item = Result<CacheClear, String>>) -> FakeDrive {
        FakeDrive {
            storage: looks.into_iter().collect(),
            ..FakeDrive::default()
        }
    }
    #[test]
    fn drive_s_cache_is_cleared_once_its_storage_window_opens_and_never_waited_on_forever() {
        let mut files = FakeFiles::new();
        let plan = |wait| Plan {
            drive: vec![r"G:\My Drive".into()],
            drive_wait: Duration::from_secs(wait),
            ..Plan::default()
        };
        // Opened while waiting: pressed once, with the size the window showed.
        let mut drive = with_storage([Ok(CacheClear::NotOpen), Ok(CacheClear::NotOpen), Ok(CacheClear::Cleared(Some("94.4 GB".into())))]);
        let mut waited = 0;
        let outcome = execute_with(&mut files, &mut drive, &plan(60), &AtomicBool::new(false), |progress| {
            waited += progress.waiting_for_drive as usize;
            true
        })
        .unwrap();
        assert_eq!((drive.storage_looks, waited), (3, 2));
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["driveCache"], serde_json::json!({ "state": "cleared", "size": "94.4 GB" }));
        // Never opened: the request ends and says so.
        let mut drive = with_storage([Ok(CacheClear::NotOpen)]);
        let outcome = execute_with(&mut files, &mut drive, &plan(0), &AtomicBool::new(false), |_| true).unwrap();
        assert_eq!(outcome.drive_cache, Some(CacheClear::NotOpen));
        // Stop while waiting ends the wait, without pressing anything.
        let mut drive = with_storage(std::iter::repeat_with(|| Ok(CacheClear::NotOpen)).take(1000));
        let cancel = AtomicBool::new(false);
        let outcome = execute_with(&mut files, &mut drive, &plan(60), &cancel, |progress| {
            !progress.waiting_for_drive
        })
        .unwrap();
        assert!(outcome.cancelled && outcome.drive_cache.is_none());
        assert_eq!(drive.storage_looks, 1);
        // A window that changed is an error, not a guess.
        let mut drive = with_storage([Err("no Clear cache button".into())]);
        let outcome = execute_with(&mut files, &mut drive, &plan(60), &AtomicBool::new(false), |_| true).unwrap();
        assert_eq!(outcome.counts.failed, 1);
        // Without Google Drive in the request, its window is never looked at.
        let mut drive = FakeDrive::default();
        execute_with(&mut files, &mut drive, &native_plan(&[r"C:\cloud"]), &AtomicBool::new(false), |_| true).unwrap();
        assert_eq!(drive.storage_looks, 0);
    }
    fn root(provider: &str, path: &str) -> SyncRoot {
        SyncRoot {
            provider: provider.into(),
            name: format!("{provider} - Personal"),
            path: path.into(),
        }
    }
    fn native_plan(paths: &[&str]) -> Plan {
        Plan {
            roots: paths.iter().map(|path| root("OneDrive", path)).collect(),
            ..Plan::default()
        }
    }
    impl CloudFiles for FakeFiles {
        type Handle = PathBuf;
        fn open_root(&mut self, path: &Path) -> Result<PathBuf, String> {
            if self.unsafe_root {
                return Err("Root crosses a junction".into());
            }
            self.open_child(path)
        }
        fn open_child(&mut self, path: &Path) -> Result<PathBuf, String> {
            self.nodes
                .contains_key(path)
                .then(|| path.to_path_buf())
                .ok_or_else(|| "Not found".into())
        }
        fn metadata(&mut self, path: &PathBuf) -> Result<Metadata, String> {
            let count = self.reads.entry(path.clone()).or_default();
            *count += 1;
            if *count > 1
                && let Some(changed) = self.changed.get(path)
            {
                return Ok(changed.clone());
            }
            self.nodes
                .get(path)
                .cloned()
                .ok_or_else(|| "Not found".into())
        }
        fn registration(&mut self, _: &PathBuf) -> Result<Registration, String> {
            Ok(self.registration.clone())
        }
        fn entries(
            &mut self,
            _: &PathBuf,
            path: &Path,
        ) -> Result<Box<dyn Iterator<Item = Result<PathBuf, String>>>, String> {
            Ok(Box::new(
                self.children
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(Ok),
            ))
        }
        fn request_unpin(&mut self, path: &PathBuf) -> Result<(), String> {
            if self.fail_requests {
                return Err("Provider refused request".into());
            }
            self.requested.push(path.clone());
            Ok(())
        }
    }
    fn folder(id: u64) -> Metadata {
        Metadata {
            volume: 7,
            id,
            directory: true,
            unsafe_reparse: false,
            links: 1,
            cloud: None,
        }
    }
    fn uploaded(id: u64) -> Metadata {
        Metadata {
            volume: 7,
            id,
            directory: false,
            unsafe_reparse: false,
            links: 1,
            cloud: Some(Placeholder {
                root_id: 100,
                in_sync: true,
                modified: 0,
                on_disk: 400,
                pin: 1,
            }),
        }
    }
    fn detected_as(provider: CloudProvider, path: &str) -> CloudFolder {
        CloudFolder {
            provider,
            label: provider.name().into(),
            path: path.into(),
        }
    }
    #[test]
    fn only_uploaded_verified_placeholders_receive_requests_not_a_freed_success() {
        let mut files = FakeFiles::new();
        let good = files.add("uploaded.txt", uploaded(1));
        let mut pending = uploaded(2);
        pending.cloud.as_mut().unwrap().in_sync = false;
        files.add("pending.txt", pending);
        let mut modified = uploaded(3);
        modified.cloud.as_mut().unwrap().modified = 10;
        files.add("modified.txt", modified);
        let mut legacy = uploaded(4);
        legacy.cloud = None;
        files.add("legacy.txt", legacy);
        let mut excluded = uploaded(5);
        excluded.cloud.as_mut().unwrap().pin = 3;
        files.add("excluded.txt", excluded);
        let mut online = uploaded(6);
        let state = online.cloud.as_mut().unwrap();
        state.pin = 2;
        state.on_disk = 0;
        files.add("online.txt", online);
        let result = files.run(&AtomicBool::new(false)).unwrap();
        assert_eq!(files.requested, [good]);
        assert_eq!(
            (
                result.counts.visited,
                result.counts.requested,
                result.counts.already_online
            ),
            (6, 1, 1)
        );
        assert_eq!(
            (result.counts.pending_uploads, result.counts.skipped),
            (2, 2)
        );
        let json = serde_json::to_value(result).unwrap();
        assert!(json.get("freed").is_none() && json.get("completedBytes").is_none());
    }
    #[test]
    fn stale_or_arbitrary_expected_roots_are_rejected_before_any_request() {
        let mut files = FakeFiles::new();
        let clouds = clouds(&[], &[root("OneDrive", r"C:\cloud")], &mut files, &mut FakeDrive::default());
        let wanted = ["oneDrive".to_string()];
        assert!(plan(&clouds, &wanted, &[r"C:\cloud".into()]).is_ok());
        for expected in [vec![r"D:\other".to_string()], vec![], vec![r"C:\cloud".into(), r"D:\other".into()]] {
            assert!(plan(&clouds, &wanted, &expected).is_err(), "accepted {expected:?}");
        }
        // An app that is gone, or never was, fails the whole request.
        assert!(plan(&clouds, &["oneDrive".into(), "sync:Box".into()], &[r"C:\cloud".into()]).is_err());
        assert!(plan(&clouds, &[], &[]).is_err());
    }
    #[test]
    fn junctions_cross_root_files_and_hardlinks_are_skipped() {
        let mut files = FakeFiles::new();
        let mut link = folder(2);
        link.unsafe_reparse = true;
        let link_path = files.add("junction", link);
        let escape = link_path.join("outside.txt");
        files.nodes.insert(escape.clone(), uploaded(3));
        files.children.insert(link_path.clone(), vec![escape]);
        let mut other = uploaded(4);
        other.cloud.as_mut().unwrap().root_id = 200;
        files.add("other-root.txt", other);
        let mut hardlink = uploaded(5);
        hardlink.links = 2;
        files.add("hardlink.txt", hardlink);
        let outcome = files.run(&AtomicBool::new(false)).unwrap();
        assert!(files.requested.is_empty());
        assert!(!files.reads.keys().any(|path| path.ends_with("outside.txt")));
        assert_eq!(outcome.counts.skipped, 3);
    }
    #[test]
    fn upload_state_is_rechecked_on_the_same_handle_before_requesting() {
        let mut files = FakeFiles::new();
        let path = files.add("changed.txt", uploaded(1));
        let mut changed = uploaded(1);
        changed.cloud.as_mut().unwrap().in_sync = false;
        files.changed.insert(path, changed);
        let result = files.run(&AtomicBool::new(false)).unwrap();
        assert!(files.requested.is_empty());
        assert_eq!(result.counts.pending_uploads, 1);
    }
    #[test]
    fn cancellation_stops_before_the_next_request_and_never_claims_completion() {
        let mut files = FakeFiles::new();
        files.add("first.txt", uploaded(1));
        files.add("second.txt", uploaded(2));
        let cancel = AtomicBool::new(false);
        let mut drive = FakeDrive::default();
        let mut plan = native_plan(&[r"C:\cloud"]);
        plan.drive.push(r"G:\My Drive".into());
        let outcome = execute_with(
            &mut files,
            &mut drive,
            &plan,
            &cancel,
            |progress| {
                if progress.counts.requested == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
                true
            },
        )
        .unwrap();
        assert_eq!(files.requested.len(), 1);
        assert!(drive.changed.is_empty(), "Google Drive is not asked after Stop");
        assert!(outcome.cancelled);
    }
    #[test]
    fn duplicate_roots_and_file_ids_never_issue_duplicate_requests() {
        let mut files = FakeFiles::new();
        files.add("one.txt", uploaded(1));
        files.add("same-id.txt", uploaded(1));
        let result = execute_with(
            &mut files,
            &mut FakeDrive::default(),
            &native_plan(&[r"C:\cloud", r"C:\cloud"]),
            &AtomicBool::new(false),
            |_| true,
        )
        .unwrap();
        assert_eq!(result.counts.requested, 1);
        assert_eq!(files.requested.len(), 1);
    }
    #[test]
    fn unsafe_or_changed_root_registration_is_rejected_before_requests() {
        for case in 0..4 {
            let mut files = FakeFiles::new();
            files.add("one.txt", uploaded(1));
            match case {
                0 => files.unsafe_root = true,
                1 => files.registration.root_id = 999,
                2 => files.registration.hydration = 3,
                _ => files.registration.provider_name = "Other sync engine".into(),
            }
            assert!(files.run(&AtomicBool::new(false)).is_err());
            assert!(files.requested.is_empty());
        }
    }
    #[test]
    fn errors_are_bounded_and_failed_requests_are_not_counted_as_requested() {
        let mut files = FakeFiles::new();
        files.fail_requests = true;
        for id in 1..=40 {
            files.add(&format!("{id}.txt"), uploaded(id));
        }
        let result = files.run(&AtomicBool::new(false)).unwrap();
        assert_eq!(result.counts.failed, 40);
        assert_eq!(result.counts.requested, 0);
        assert_eq!(result.errors.len(), MAX_ERRORS);
    }
    #[test]
    fn mega_is_refused_and_google_drive_uses_its_own_menu_per_folder() {
        let mut files = FakeFiles::new();
        files.add("one.txt", uploaded(1));
        let found = [
            detected_as(CloudProvider::Mega, r"C:\Users\A\MEGA"),
            detected_as(CloudProvider::GoogleDrive, r"G:\My Drive"),
            detected_as(CloudProvider::GoogleDrive, r"H:\My Drive"),
        ];
        let mut drive = FakeDrive::default();
        drive.online.insert(r"H:\My Drive".into());
        let clouds = clouds(&found, &[], &mut files, &mut drive);
        let mega = [r"C:\Users\A\MEGA".to_string()];
        assert!(plan(&clouds, &["mega".into()], &mega).is_err());
        let drives = [r"G:\My Drive".to_string(), r"H:\My Drive".to_string()];
        let plan = plan(&clouds, &["googleDrive".into()], &drives).unwrap();
        assert!(plan.roots.is_empty());
        let outcome = execute_with(&mut files, &mut drive, &plan, &AtomicBool::new(false), |_| true).unwrap();
        assert_eq!(drive.changed, [PathBuf::from(r"G:\My Drive")]);
        assert_eq!((outcome.counts.folders_requested, outcome.counts.folders_already_online), (1, 1));
        assert!(files.requested.is_empty(), "Drive folders never get file requests");
        // A menu that changed fails that folder alone, with its reason.
        drive.failing.insert(r"G:\My Drive".into());
        let outcome = execute_with(&mut files, &mut drive, &plan, &AtomicBool::new(false), |_| true).unwrap();
        assert_eq!((outcome.counts.failed, outcome.counts.folders_already_online), (1, 1));
        assert!(outcome.errors[0].contains("menu changed"));
    }
    #[test]
    #[ignore = "reads this PC's cloud apps and Google Drive's menu; changes nothing"]
    fn this_pc_s_cloud_apps() {
        for cloud in current_clouds().into_iter().map(ProviderStatus::from) {
            let folders: Vec<_> = cloud.folders.iter().map(|folder| folder.path.as_str()).collect();
            println!("{} ({}): {} {folders:?}", cloud.name, cloud.provider, cloud.mode);
        }
    }
    #[test]
    fn every_sync_root_is_offered_under_its_app_and_unverified_ones_are_guided() {
        let mut files = FakeFiles::new();
        let mut icloud = root("iCloudDrive", r"C:\cloud");
        icloud.name = "iCloud Drive - a@b.c".into();
        let roots = [root("OneDrive", r"C:\cloud"), icloud, root("Box", r"C:\missing")];
        let mut drive = FakeDrive::default();
        drive.failing.insert(r"G:\My Drive".into());
        let found = [detected_as(CloudProvider::GoogleDrive, r"G:\My Drive")];
        let clouds = clouds(&found, &roots, &mut files, &mut drive);
        let modes: Vec<_> = clouds
            .into_iter()
            .map(ProviderStatus::from)
            .map(|cloud| (cloud.provider, cloud.name, cloud.mode, cloud.folders.len()))
            .collect();
        let expected = [
            ("googleDrive", "Google Drive", "guided", 1),
            ("mega", "MEGA", "unavailable", 0),
            ("dropbox", "Dropbox", "unavailable", 0),
            ("oneDrive", "OneDrive", "native", 1),
            ("sync:Box", "Box", "guided", 1),
            ("sync:iCloudDrive", "iCloud Drive", "native", 1),
        ];
        assert_eq!(
            modes,
            expected.map(|(id, name, mode, folders)| (id.to_string(), name.to_string(), mode, folders))
        );
    }
    #[test]
    fn job_ownership_serializes_workers_and_old_cancellation_cannot_touch_a_new_job() {
        let state = OnlineOnlyState::default();
        assert!(state.begin("bad-id".into()).is_err());
        let first_id = uuid::Uuid::new_v4().to_string();
        let first = state.begin(first_id.clone()).unwrap();
        assert!(state.begin(uuid::Uuid::new_v4().to_string()).is_err());
        state.cancel(&uuid::Uuid::new_v4().to_string()).unwrap();
        assert!(!first.cancel.load(Ordering::Relaxed));
        state.cancel(&first_id).unwrap();
        assert!(first.cancel.load(Ordering::Relaxed));
        drop(first);
        let second = state.begin(uuid::Uuid::new_v4().to_string()).unwrap();
        state.cancel(&first_id).unwrap();
        assert!(!second.cancel.load(Ordering::Relaxed));
        drop(second);
        assert!(state.0.lock().unwrap().active.is_none());
    }
    #[test]
    fn stop_received_before_the_async_start_is_not_lost() {
        let state = OnlineOnlyState::default();
        let id = uuid::Uuid::new_v4().to_string();
        state.cancel(&id).unwrap();
        let job = state.begin(id).unwrap();
        assert!(
            job.cancel.load(Ordering::Relaxed),
            "early Stop must be consumed by its own worker"
        );
        let mut files = FakeFiles::new();
        files.add("uploaded.txt", uploaded(1));
        let outcome = files.run(&job.cancel).unwrap();
        assert!(outcome.cancelled);
        assert!(files.requested.is_empty());
    }
    #[test]
    fn early_stops_are_bounded_deduplicated_and_never_pre_cancel_another_job() {
        let state = OnlineOnlyState::default();
        assert!(state.cancel("arbitrary data").is_err());
        for _ in 0..100 {
            state.cancel(&uuid::Uuid::new_v4().to_string()).unwrap();
        }
        let recent_id = uuid::Uuid::new_v4().to_string();
        state.cancel(&recent_id).unwrap();
        state.cancel(&recent_id).unwrap();
        assert_eq!(state.0.lock().unwrap().early_stops.len(), 64);
        let other = state.begin(uuid::Uuid::new_v4().to_string()).unwrap();
        assert!(!other.cancel.load(Ordering::Relaxed));
        drop(other);
        let stopped = state.begin(recent_id).unwrap();
        assert!(stopped.cancel.load(Ordering::Relaxed));
        assert_eq!(state.0.lock().unwrap().early_stops.len(), 63);
    }
    #[test]
    fn invalid_paths_and_traversal_are_refused_instead_of_becoming_scan_roots() {
        for path in [
            r"relative\cloud",
            r"C:\cloud\..\other",
            r"\\server\share",
            r"\\.\C:\cloud",
            "C:\\cloud\0name",
        ] {
            assert!(root_key(Path::new(path)).is_err(), "accepted {path:?}");
        }
        assert_eq!(
            root_key(Path::new(r"\\?\C:\Cloud")).unwrap(),
            root_key(Path::new("c:/cloud/")).unwrap()
        );
    }
    #[test]
    fn the_native_adapter_only_reads_metadata_and_refuses_an_ordinary_folder_as_a_sync_root() {
        let root = std::env::temp_dir().join(format!("myle-cloud-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("pending.txt");
        std::fs::write(&path, b"not uploaded").unwrap();
        {
            let mut files = native::WindowsFiles;
            let handle = files.open_root(&root).unwrap();
            assert!(files.metadata(&handle).unwrap().directory);
            let child = files.open_child(&path).unwrap();
            let info = files.metadata(&child).unwrap();
            assert!(!info.directory && info.cloud.is_none());
            let result = execute_with(
                &mut files,
                &mut FakeDrive::default(),
                &native_plan(&[root.to_str().unwrap()]),
                &AtomicBool::new(false),
                |_| true,
            );
            assert!(result.is_err());
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"not uploaded");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
