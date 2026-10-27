//! Provider-managed online-only requests. No content is read or deleted.
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;

use super::{CloudFolder, CloudProvider};

const MAX_ERRORS: usize = 20;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    pub visited: u64,
    pub requested: u64,
    pub already_online: u64,
    pub pending_uploads: u64,
    pub skipped: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOnlyProgress {
    #[serde(flatten)]
    counts: Counts,
    current: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOnlyOutcome {
    #[serde(flatten)]
    counts: Counts,
    cancelled: bool,
    errors: Vec<String>,
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

fn execute_with(
    files: &mut impl CloudFiles,
    provider: CloudProvider,
    expected: &[String],
    detected: &[CloudFolder],
    cancel: &AtomicBool,
    mut progress: impl FnMut(&CloudOnlyProgress) -> bool,
) -> Result<CloudOnlyOutcome, String> {
    if !matches!(provider, CloudProvider::OneDrive | CloudProvider::Dropbox) {
        return Err(format!(
            "Use {}'s storage controls for this provider.",
            provider.name()
        ));
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(CloudOnlyOutcome {
            cancelled: true,
            ..Default::default()
        });
    }
    let roots = selected_roots(provider, expected, detected)?;
    // Validate and hold every root before changing any file. Handles also hold
    // the ancestors against rename/reparse replacement throughout traversal.
    let prepared: Vec<_> = roots.iter().map(|path| {
        let handle = files.open_root(path)?;
        let metadata = files.metadata(&handle)?;
        let registration = files.registration(&handle)?;
        if !metadata.directory || metadata.unsafe_reparse || metadata.id == 0
            || metadata.id != registration.root_id || !compatible(provider, &registration)
        {
            return Err(format!("{} is not a compatible registered sync root. Refresh and use the provider's storage controls.", path.display()));
        }
        Ok((path, handle, metadata))
    }).collect::<Result<_, String>>()?;
    let mut outcome = CloudOnlyOutcome::default();
    let mut seen = HashSet::new();
    for (path, handle, metadata) in &prepared {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        Walker {
            files,
            provider,
            root_id: metadata.id,
            volume: metadata.volume,
            cancel,
            progress: &mut progress,
            seen: &mut seen,
            outcome: &mut outcome,
        }
        .visit(path, handle, 0);
    }
    outcome.cancelled = cancel.load(Ordering::Relaxed);
    let _ = progress(&CloudOnlyProgress {
        counts: outcome.counts.clone(),
        current: None,
    });
    Ok(outcome)
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

fn selected_roots(
    provider: CloudProvider,
    expected: &[String],
    detected: &[CloudFolder],
) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    let mut actual = HashSet::new();
    for folder in detected.iter().filter(|folder| folder.provider == provider) {
        if actual.insert(root_key(&folder.path)?) {
            roots.push(folder.path.clone());
        }
    }
    let expected: HashSet<_> = expected
        .iter()
        .map(|path| root_key(Path::new(path)))
        .collect::<Result<_, _>>()?;
    if actual.is_empty() || actual != expected {
        return Err(
            "The cloud folders changed. Refresh Disk Analyzer before requesting online-only files."
                .into(),
        );
    }
    Ok(roots)
}

fn compatible(provider: CloudProvider, registration: &Registration) -> bool {
    let name = registration.provider_name.to_lowercase();
    matches!(registration.hydration, 1 | 2)
        && match provider {
            CloudProvider::OneDrive => name.contains("onedrive"),
            CloudProvider::Dropbox => name.contains("dropbox"),
            _ => false,
        }
}

struct Walker<'a, F: CloudFiles, P: FnMut(&CloudOnlyProgress) -> bool> {
    files: &'a mut F,
    provider: CloudProvider,
    root_id: u64,
    volume: u32,
    cancel: &'a AtomicBool,
    progress: &'a mut P,
    seen: &'a mut HashSet<(u32, u64)>,
    outcome: &'a mut CloudOnlyOutcome,
}

impl<F: CloudFiles, P: FnMut(&CloudOnlyProgress) -> bool> Walker<'_, F, P> {
    fn error(&mut self, path: &Path, error: String) {
        self.outcome.counts.failed += 1;
        if self.outcome.errors.len() < MAX_ERRORS {
            self.outcome
                .errors
                .push(format!("{}: {error}", path.display()));
        }
    }

    fn emit(&mut self, path: &Path) {
        if !(self.progress)(&CloudOnlyProgress {
            counts: self.outcome.counts.clone(),
            current: Some(path.display().to_string()),
        }) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn permitted(&mut self, handle: &F::Handle, metadata: &Metadata) -> bool {
        !metadata.unsafe_reparse
            && metadata.volume == self.volume
            && metadata.id != 0
            && self.files.registration(handle).is_ok_and(|registration| {
                registration.root_id == self.root_id && compatible(self.provider, &registration)
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
            // Recheck the same held object immediately before the request. A
            // path-based second open could inspect a different file.
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

// A request remains owned by the blocking worker even if its IPC future is
// dropped. Cancelling an old request can never affect a subsequent request.
#[derive(Clone, Default)]
pub struct OnlineOnlyState(Arc<Mutex<Jobs>>);

#[derive(Default)]
struct Jobs {
    active: Option<Job>,
    // A synchronous Stop IPC can arrive before the async Start is first polled.
    // Remember its UUID without allowing unlimited retained cancellation data.
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
    provider: CloudProvider,
    name: &'static str,
    folders: Vec<FolderStatus>,
    mode: &'static str,
    detail: &'static str,
}

#[derive(Serialize)]
struct FolderStatus {
    path: String,
    label: String,
}

fn provider_status(provider: CloudProvider, detected: &[CloudFolder]) -> ProviderStatus {
    let folders: Vec<_> = detected
        .iter()
        .filter(|folder| folder.provider == provider)
        .collect();
    let (mode, detail) = if folders.is_empty() {
        (
            "unavailable",
            "No local cloud folder was detected. Check the desktop app's account and folder settings.",
        )
    } else {
        match provider {
            CloudProvider::GoogleDrive => (
                "guided",
                "Google Drive manages offline availability and its streaming cache through its own desktop app.",
            ),
            CloudProvider::Mega => (
                "guided",
                "MEGA Desktop sync has no online-only conversion. MEGAcmd offers a separate on-demand mount (beta on Windows).",
            ),
            CloudProvider::OneDrive | CloudProvider::Dropbox => {
                let mut files = native::WindowsFiles;
                let supported = folders.iter().all(|folder| {
                    let Ok(handle) = files.open_root(&folder.path) else {
                        return false;
                    };
                    let Ok(info) = files.metadata(&handle) else {
                        return false;
                    };
                    files.registration(&handle).is_ok_and(|registration| {
                        info.directory
                            && !info.unsafe_reparse
                            && info.id != 0
                            && info.id == registration.root_id
                            && compatible(provider, &registration)
                    })
                });
                if supported {
                    (
                        "native",
                        "Online-only requests are available. Each file's upload status is checked before requesting a change.",
                    )
                } else {
                    (
                        "guided",
                        "Compatible Windows online-only support could not be verified for every folder. Use this cloud app's own storage controls.",
                    )
                }
            }
        }
    };
    ProviderStatus {
        provider,
        name: provider.name(),
        mode,
        detail,
        folders: folders
            .into_iter()
            .map(|folder| FolderStatus {
                path: folder.path.display().to_string(),
                label: folder.label.clone(),
            })
            .collect(),
    }
}

#[tauri::command]
pub async fn cloud_storage_status() -> Result<StorageStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let detected = super::detection::detect_folders();
        StorageStatus {
            providers: [
                CloudProvider::GoogleDrive,
                CloudProvider::Mega,
                CloudProvider::Dropbox,
                CloudProvider::OneDrive,
            ]
            .into_iter()
            .map(|provider| provider_status(provider, &detected))
            .collect(),
        }
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cloud_storage_online_only(
    provider: CloudProvider,
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
        let detected = super::detection::detect_folders();
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        execute_with(
            &mut native::WindowsFiles,
            provider,
            &expected_folders,
            &detected,
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

#[tauri::command]
pub async fn cloud_storage_open_folder(
    provider: CloudProvider,
    path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = root_key(Path::new(&path))?;
        let known = super::detection::detect_folders()
            .into_iter()
            .find(|folder| {
                folder.provider == provider
                    && root_key(&folder.path).is_ok_and(|actual| actual == key)
            })
            .ok_or_else(|| {
                "This cloud folder changed. Refresh the cloud apps first.".to_string()
            })?;
        tauri_plugin_opener::open_path(known.path.display().to_string(), None::<&str>)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn cloud_storage_open_help(provider: CloudProvider) -> Result<(), String> {
    let url = match provider {
        CloudProvider::GoogleDrive => "https://support.google.com/drive/answer/13401938?hl=en",
        CloudProvider::Mega => "https://github.com/meganz/MEGAcmd/blob/master/contrib/docs/FUSE.md",
        CloudProvider::Dropbox => "https://help.dropbox.com/sync/make-files-online-only",
        CloudProvider::OneDrive => {
            "https://learn.microsoft.com/en-us/onedrive/files-on-demand-windows"
        }
    };
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|error| error.to_string())
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
        // CLOUD and CLOUD_1..CLOUD_F are placeholders, not name-surrogate
        // links. Everything else (including junctions and symlinks) is refused.
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
        // Aligned storage for the fixed prefix and bounded variable identity.
        // Query errors mean "unverified", never permission to change a file.
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
            // This is user intent only. The provider owns upload consistency
            // and any eventual eviction; never force-dehydrate or delete data.
            // SAFETY: held, verified placeholder handle; no recursive flag.
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
            execute_with(
                self,
                CloudProvider::OneDrive,
                &[r"C:\cloud".into()],
                &[detected(r"C:\cloud")],
                cancel,
                |_| true,
            )
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
    fn detected(path: &str) -> CloudFolder {
        CloudFolder {
            provider: CloudProvider::OneDrive,
            label: "OneDrive".into(),
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
        files.add("uploaded.txt", uploaded(1));
        assert!(
            execute_with(
                &mut files,
                CloudProvider::OneDrive,
                &[r"D:\other".into()],
                &[detected(r"C:\cloud")],
                &AtomicBool::new(false),
                |_| true
            )
            .is_err()
        );
        assert!(files.requested.is_empty());
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
        let outcome = execute_with(
            &mut files,
            CloudProvider::OneDrive,
            &[r"C:\cloud".into()],
            &[detected(r"C:\cloud")],
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
        assert!(outcome.cancelled);
    }

    #[test]
    fn duplicate_roots_and_file_ids_never_issue_duplicate_requests() {
        let mut files = FakeFiles::new();
        files.add("one.txt", uploaded(1));
        files.add("same-id.txt", uploaded(1));
        let result = execute_with(
            &mut files,
            CloudProvider::OneDrive,
            &[r"C:\cloud".into()],
            &[detected(r"C:\cloud"), detected(r"C:\cloud")],
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
    fn google_and_mega_cannot_use_the_native_action() {
        for provider in [CloudProvider::GoogleDrive, CloudProvider::Mega] {
            let mut files = FakeFiles::new();
            files.add("one.txt", uploaded(1));
            let mut root = detected(r"C:\cloud");
            root.provider = provider;
            assert!(
                execute_with(
                    &mut files,
                    provider,
                    &[r"C:\cloud".into()],
                    &[root],
                    &AtomicBool::new(false),
                    |_| true
                )
                .is_err()
            );
            assert!(files.requested.is_empty());
        }
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
            let folder = detected(root.to_str().unwrap());
            let result = execute_with(
                &mut files,
                CloudProvider::OneDrive,
                &[root.display().to_string()],
                &[folder],
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
