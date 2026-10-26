//! Disk Analyzer: what is filling a drive or a folder.
//!
//! The page sends one root path to scan, then only node ids this backend
//! gave it. The finished scan stays here; the page asks for slices of it
//! (a folder's children, a treemap, the largest files) and never gets the
//! whole tree. Every id belongs to one scan: an id from an earlier scan is
//! refused. Moving items to the Recycle Bin goes by id, and the real path is
//! worked out and checked here, again, just before anything moves.

mod delete;
mod elevated;
mod mft;
mod protect;
mod scan;
mod tree;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::apps::Jobs;
use protect::{Guard, Verdict};
use tree::{Kind, SizeMode, SortKey, Tree, flag};

/// The most children asked for at once.
const MAX_CHILDREN: usize = 500;
const MAX_TOP_FILES: usize = 1000;
/// The most nodes in one treemap.
const TREEMAP_CAP: usize = 20_000;
/// The most ids in one request (details or the Recycle Bin).
const MAX_IDS: usize = 10_000;

#[derive(Default)]
struct Scanned {
    tree: Option<Tree>,
    guard: Option<Guard>,
}

#[derive(Default)]
struct Inner {
    /// The running scan's cancel flag.
    running: Mutex<Option<Arc<AtomicBool>>>,
    scanned: Mutex<Scanned>,
    next_id: AtomicU64,
}

/// The running scan and the last finished one.
#[derive(Clone, Default)]
pub struct DiskAnalyzerState(Arc<Inner>);

impl DiskAnalyzerState {
    /// A new scan: the one still running stops, and the last result is let
    /// go now rather than held next to the new one.
    fn begin(&self) -> (u64, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        if let Some(old) = lock(&self.0.running).replace(Arc::clone(&flag)) {
            old.store(true, Ordering::Relaxed);
        }
        *lock(&self.0.scanned) = Scanned::default();
        (self.0.next_id.fetch_add(1, Ordering::Relaxed) + 1, flag)
    }

    /// Keeps a finished scan, unless a newer one started meanwhile.
    fn keep(&self, flag: &Arc<AtomicBool>, tree: Tree, guard: Guard) -> bool {
        let mut running = lock(&self.0.running);
        if flag.load(Ordering::Relaxed) || !running.as_ref().is_some_and(|f| Arc::ptr_eq(f, flag)) {
            return false;
        }
        *running = None;
        *lock(&self.0.scanned) = Scanned { tree: Some(tree), guard: Some(guard) };
        true
    }

    fn end(&self, flag: &Arc<AtomicBool>) {
        let mut running = lock(&self.0.running);
        if running.as_ref().is_some_and(|f| Arc::ptr_eq(f, flag)) {
            *running = None;
        }
    }

    /// Stops the running scan, if any.
    pub fn cancel(&self) {
        if let Some(flag) = lock(&self.0.running).take() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn with<R>(&self, scan_id: u64, work: impl FnOnce(&mut Tree, &Guard) -> Result<R, String>) -> Result<R, String> {
        let mut scanned = lock(&self.0.scanned);
        let Scanned { tree: Some(tree), guard: Some(guard) } = &mut *scanned else {
            return Err(tree::STALE.into());
        };
        if tree.scan_id != scan_id {
            return Err(tree::STALE.into());
        }
        work(tree, guard)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn blocking<R: Send + 'static>(work: impl FnOnce() -> Result<R, String> + Send + 'static) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Drives and the folder picker

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    letter: String,
    /// `C:\`
    root: String,
    label: String,
    filesystem: String,
    total: u64,
    free: u64,
    /// "fixed", "removable", "network" or "other".
    kind: &'static str,
    /// The drive Windows runs from.
    system: bool,
    /// A fixed NTFS drive: the file table could be read directly (not built yet).
    mft_possible: bool,
}

fn drive(letter: char) -> Option<Drive> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW};
    const REMOVABLE: u32 = 2;
    const FIXED: u32 = 3;
    const REMOTE: u32 = 4;
    const RAMDISK: u32 = 6;
    let root = format!(r"{letter}:\");
    let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
    // SAFETY: NUL-terminated strings and buffers that outlive the calls.
    unsafe {
        let kind = match GetDriveTypeW(wide.as_ptr()) {
            FIXED => "fixed",
            REMOVABLE => "removable",
            REMOTE => "network",
            RAMDISK => "other",
            _ => return None,
        };
        let mut label = [0u16; 261];
        let mut filesystem = [0u16; 261];
        let (mut serial, mut max_name, mut flags) = (0u32, 0u32, 0u32);
        if GetVolumeInformationW(
            wide.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            &mut serial,
            &mut max_name,
            &mut flags,
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        ) == 0
        {
            return None; // no disc or card in it
        }
        let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
        if GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) == 0 {
            return None;
        }
        let text = |buffer: &[u16]| {
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            String::from_utf16_lossy(&buffer[..end])
        };
        let filesystem = text(&filesystem);
        let windows = std::env::var("SystemRoot").unwrap_or_default();
        Some(Drive {
            letter: letter.to_string(),
            system: windows.len() >= 2 && windows[..1].eq_ignore_ascii_case(&letter.to_string()),
            mft_possible: kind == "fixed" && filesystem.eq_ignore_ascii_case("NTFS"),
            root,
            label: text(&label),
            filesystem,
            total,
            free,
            kind,
        })
    }
}

/// The drives to offer. Each is asked on its own thread: a network drive
/// whose server is gone can take long to answer, and is left out.
#[tauri::command]
pub async fn disk_analyzer_drives() -> Result<Vec<Drive>, String> {
    blocking(|| {
        use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
        // SAFETY: no arguments.
        let mask = unsafe { GetLogicalDrives() };
        let (sender, answers) = std::sync::mpsc::channel();
        let letters: Vec<char> = (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| (b'A' + i) as char).collect();
        for &letter in &letters {
            let sender = sender.clone();
            std::thread::spawn(move || {
                let _ = sender.send(drive(letter));
            });
        }
        drop(sender);
        let deadline = Instant::now() + Duration::from_millis(2500);
        let mut drives = Vec::new();
        for _ in &letters {
            let left = deadline.saturating_duration_since(Instant::now());
            match answers.recv_timeout(left) {
                Ok(Some(drive)) => drives.push(drive),
                Ok(None) => {}
                Err(_) => break,
            }
        }
        drives.sort_by(|a, b| a.letter.cmp(&b.letter));
        Ok(drives)
    })
    .await
}

#[tauri::command]
pub async fn disk_analyzer_pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a folder to analyze")
            .blocking_pick_folder()
            .map(|path| path.into_path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .transpose()
    })
    .await
}

// ---------------------------------------------------------------------------
// Scanning

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanDone {
    scan_id: u64,
    root_id: u32,
    /// The scanned folder, as Windows shows it.
    root: String,
    files: u64,
    dirs: u64,
    bytes: u64,
    alloc: u64,
    /// Folders that could not be read.
    inaccessible: u32,
    /// The first of them.
    inaccessible_paths: Vec<String>,
    elapsed_ms: u64,
    /// Walked on the Windows drive, where hard links count once per link.
    windows_drive: bool,
    /// What the results hold in memory.
    memory: u64,
    /// "mft": read from the file table; "walk": folder by folder.
    method: &'static str,
    /// Why a fast scan asked for became a walk.
    fallback: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScanEvent {
    Progress {
        files: u64,
        dirs: u64,
        bytes: u64,
        alloc: u64,
        current: String,
        /// A fast scan's step: "approval", "table" or "build"; none for a walk.
        phase: Option<&'static str>,
    },
    Done(ScanDone),
    Cancelled,
}

impl ScanEvent {
    fn progress(p: &scan::Progress, phase: Option<&'static str>) -> Self {
        ScanEvent::Progress { files: p.files, dirs: p.dirs, bytes: p.bytes, alloc: p.alloc, current: p.current.clone(), phase }
    }
}

/// The folder to scan, in its `\\?\` form: a folder or drive that is there.
fn scan_root(root: &str) -> Result<PathBuf, String> {
    let root = root.trim();
    let path = Path::new(root);
    if root.is_empty() || !path.is_absolute() {
        return Err("Choose a drive or a folder.".into());
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| "That folder is not there any more.".to_string())?;
    if !std::fs::metadata(&canonical).is_ok_and(|m| m.is_dir()) {
        return Err("Choose a drive or a folder, not a file.".into());
    }
    Ok(tree::long_path(&canonical))
}

fn same_drive_as_windows(root: &Path) -> bool {
    let windows = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_default();
    protect::parts(root).first() == protect::parts(&windows).first()
}

/// Scans `root` and keeps the result. `None` when it was cancelled.
///
/// `fast`, for a whole fixed NTFS drive: read its file table through the
/// administrator helper (one UAC prompt while the helper runs). When the
/// prompt is declined or the table cannot be read, the drive is walked, and
/// the result says why.
#[tauri::command]
pub async fn disk_analyzer_scan(
    state: State<'_, DiskAnalyzerState>,
    root: String,
    fast: Option<bool>,
    on_event: Channel<ScanEvent>,
) -> Result<Option<ScanDone>, String> {
    let root = scan_root(&root)?;
    let (scan_id, cancel) = state.begin();
    let state = state.inner().clone();
    let mut fallback = None;
    if fast.unwrap_or(false)
        && let Some(letter) = fast_drive(&root)
    {
        let phase = |phase| {
            let name = match phase {
                elevated::Phase::Approval => "approval",
                elevated::Phase::Table => "table",
            };
            let _ = on_event.send(ScanEvent::progress(&scan::Progress::default(), Some(name)));
        };
        match elevated::read(letter, phase).await {
            Ok((bytes, read_ms)) => {
                return blocking(move || {
                    let started = Instant::now();
                    let snapshot = mft::Snapshot::decode(&bytes)?;
                    drop(bytes);
                    let _ = on_event.send(ScanEvent::progress(&scan::Progress::default(), Some("build")));
                    let built = mft::into_tree(&snapshot, scan_id, tree::display_path(&root), &cancel, |p| {
                        let _ = on_event.send(ScanEvent::progress(p, Some("build")));
                    });
                    drop(snapshot);
                    let elapsed_ms = read_ms + started.elapsed().as_millis() as u64;
                    finish_scan(&state, &cancel, &on_event, built, "mft", None, elapsed_ms)
                })
                .await;
            }
            Err(error) => fallback = Some(error),
        }
    }
    blocking(move || {
        let started = Instant::now();
        let walked = scan::scan(&root, scan_id, &cancel, |p| {
            let _ = on_event.send(ScanEvent::progress(p, None));
        });
        let elapsed_ms = started.elapsed().as_millis() as u64;
        finish_scan(&state, &cancel, &on_event, walked, "walk", fallback, elapsed_ms)
    })
    .await
}

/// Keeps a finished scan's tree and tells the page, unless it was cancelled
/// or a newer scan started.
fn finish_scan(
    state: &DiskAnalyzerState,
    cancel: &Arc<AtomicBool>,
    on_event: &Channel<ScanEvent>,
    outcome: Result<Option<Tree>, String>,
    method: &'static str,
    fallback: Option<String>,
    elapsed_ms: u64,
) -> Result<Option<ScanDone>, String> {
    let tree = match outcome {
        Ok(Some(tree)) => tree,
        Ok(None) => {
            state.end(cancel);
            let _ = on_event.send(ScanEvent::Cancelled);
            return Ok(None);
        }
        Err(error) => {
            state.end(cancel);
            return Err(error);
        }
    };
    let done = ScanDone {
        scan_id: tree.scan_id,
        root_id: 0,
        root: tree.root.display().to_string(),
        files: tree.files(0) as u64,
        dirs: tree.dirs(0) as u64,
        bytes: tree.size(0, SizeMode::Size),
        alloc: tree.size(0, SizeMode::Alloc),
        inaccessible: tree.inaccessible.len() as u32,
        inaccessible_paths: tree.inaccessible.iter().take(20).map(|&id| tree.path(id).display().to_string()).collect(),
        elapsed_ms,
        windows_drive: method == "walk" && same_drive_as_windows(&tree.root),
        memory: tree.memory() as u64,
        method,
        fallback,
    };
    let guard = Guard::for_this_pc(&tree.root);
    if !state.keep(cancel, tree, guard) {
        let _ = on_event.send(ScanEvent::Cancelled);
        return Ok(None);
    }
    let _ = on_event.send(ScanEvent::Done(done.clone()));
    Ok(Some(done))
}

/// The drive's letter, when `root` is a whole fixed NTFS drive.
fn fast_drive(root: &Path) -> Option<char> {
    let parts = protect::parts(root);
    let [only] = parts.as_slice() else { return None };
    let mut chars = only.strip_suffix(':')?.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if chars.next().is_some() || !letter.is_ascii_uppercase() {
        return None;
    }
    drive(letter).filter(|d| d.mft_possible).map(|_| letter)
}

/// Handles the fast scan's administrator helper before Tauri starts.
pub fn run_elevated_helper_from_args() -> Option<i32> {
    elevated::run_helper_from_args()
}

#[tauri::command]
pub fn disk_analyzer_cancel(state: State<'_, DiskAnalyzerState>) {
    state.cancel();
}

/// Lets the results go (off the main thread: a big tree takes a moment to free).
#[tauri::command]
pub async fn disk_analyzer_clear(state: State<'_, DiskAnalyzerState>) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || {
        let old = std::mem::take(&mut *lock(&state.0.scanned));
        drop(old);
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// Slices of the result

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    id: u32,
    name: String,
    /// "dir", "file", "link" or "cloud".
    kind: &'static str,
    ext: String,
    size: u64,
    alloc: u64,
    files: u32,
    dirs: u32,
    /// Seconds since 1970.
    modified: u32,
    has_children: bool,
    /// Never moved to the Recycle Bin; `reason` says why.
    protected: bool,
    reason: Option<String>,
    /// Allowed, with this warning: what it is and why to think twice.
    caution: Option<String>,
    inaccessible: bool,
    /// Another name of a file counted under its first name (fast scan only).
    hard_link: bool,
}

fn view(tree: &Tree, guard: &Guard, id: u32) -> NodeView {
    let node = tree.node(id);
    let verdict = if node.kind == Kind::Link {
        Verdict::Protected(delete::LINK.into())
    } else {
        guard.check(&tree.path(id), node.flags & flag::SYSTEM != 0)
    };
    NodeView {
        id,
        name: tree.name(id).to_string_lossy().into_owned(),
        kind: node.kind.label(),
        ext: tree.ext_of(id).to_string(),
        size: node.size,
        alloc: node.alloc,
        files: tree.files(id),
        dirs: tree.dirs(id),
        modified: node.modified,
        has_children: tree.has_children(id),
        protected: matches!(verdict, Verdict::Protected(_)),
        reason: match &verdict {
            Verdict::Protected(why) => Some(why.to_string()),
            _ => None,
        },
        caution: match verdict {
            Verdict::Caution(why) => Some(why.into_owned()),
            _ => None,
        },
        inaccessible: node.flags & flag::INACCESSIBLE != 0,
        hard_link: node.flags & flag::HARD_LINK != 0,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rest {
    count: u32,
    size: u64,
    alloc: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Children {
    items: Vec<NodeView>,
    total: u32,
    /// The children after this page: "N smaller items" and their total.
    rest: Option<Rest>,
}

#[tauri::command]
pub async fn disk_analyzer_children(
    state: State<'_, DiskAnalyzerState>,
    scan_id: u64,
    node_id: u32,
    offset: Option<u32>,
    limit: u32,
    sort: Option<SortKey>,
    size_mode: SizeMode,
) -> Result<Children, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, guard| {
            tree.check(scan_id, node_id)?;
            let limit = (limit as usize).clamp(1, MAX_CHILDREN);
            let page = tree.children(node_id, offset.unwrap_or(0) as usize, limit, sort.unwrap_or(SortKey::Size), size_mode);
            Ok(Children {
                items: page.ids.iter().map(|&id| view(tree, guard, id)).collect(),
                total: page.total,
                rest: (page.rest_count > 0).then_some(Rest {
                    count: page.rest_count,
                    size: page.rest_size,
                    alloc: page.rest_alloc,
                }),
            })
        })
    })
    .await
}

/// The current details of some items (for the Recycle Bin confirmation).
#[tauri::command]
pub async fn disk_analyzer_nodes(
    state: State<'_, DiskAnalyzerState>,
    scan_id: u64,
    node_ids: Vec<u32>,
) -> Result<Vec<NodeView>, String> {
    if node_ids.len() > MAX_IDS {
        return Err("Too many items at once.".into());
    }
    let state = state.inner().clone();
    blocking(move || {
        // What runs or is installed now, not when the scan ended: this
        // answers the question asked before anything moves.
        let root = state.with(scan_id, |tree, _| Ok(tree.root.clone()))?;
        let guard = Guard::for_this_pc(&root);
        state.with(scan_id, |tree, _| {
            node_ids
                .iter()
                .map(|&id| tree.check(scan_id, id).map(|()| view(tree, &guard, id)))
                .collect()
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    id: u32,
    name: String,
    size: u64,
    alloc: u64,
}

/// The scanned folder down to `node_id`, for the breadcrumbs.
#[tauri::command]
pub async fn disk_analyzer_trail(state: State<'_, DiskAnalyzerState>, scan_id: u64, node_id: u32) -> Result<Vec<Crumb>, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, _| {
            tree.check(scan_id, node_id)?;
            Ok(tree
                .trail(node_id)
                .into_iter()
                .map(|id| Crumb {
                    id,
                    name: tree.name(id).to_string_lossy().into_owned(),
                    size: tree.size(id, SizeMode::Size),
                    alloc: tree.size(id, SizeMode::Alloc),
                })
                .collect())
        })
    })
    .await
}

#[tauri::command]
pub async fn disk_analyzer_treemap(
    state: State<'_, DiskAnalyzerState>,
    scan_id: u64,
    node_id: u32,
    depth: u8,
    min_bytes: u64,
    size_mode: SizeMode,
) -> Result<tree::Treemap, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, _| {
            tree.check(scan_id, node_id)?;
            Ok(tree.treemap(node_id, depth.min(16), min_bytes, size_mode, TREEMAP_CAP))
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    #[serde(flatten)]
    node: NodeView,
    path: String,
    parent: u32,
    /// Its extension's colour, as in the treemap.
    rank: u8,
}

#[tauri::command]
pub async fn disk_analyzer_top_files(
    state: State<'_, DiskAnalyzerState>,
    scan_id: u64,
    limit: u32,
    size_mode: SizeMode,
    filter: Option<String>,
) -> Result<Vec<FileView>, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, guard| {
            let limit = (limit as usize).clamp(1, MAX_TOP_FILES);
            let ids = tree.top_files(size_mode, limit, filter.as_deref());
            let ranks = tree.ranks(size_mode).to_vec();
            Ok(ids
                .into_iter()
                .map(|id| FileView {
                    node: view(tree, guard, id),
                    path: tree.path(id).display().to_string(),
                    parent: tree.node(id).parent,
                    rank: ranks[tree.node(id).ext as usize],
                })
                .collect())
        })
    })
    .await
}

#[tauri::command]
pub async fn disk_analyzer_extensions(
    state: State<'_, DiskAnalyzerState>,
    scan_id: u64,
    node_id: Option<u32>,
    size_mode: SizeMode,
) -> Result<Vec<tree::ExtStat>, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, _| {
            let id = node_id.unwrap_or(0);
            tree.check(scan_id, id)?;
            Ok(tree.extensions(id, size_mode))
        })
    })
    .await
}

#[tauri::command]
pub async fn disk_analyzer_path(state: State<'_, DiskAnalyzerState>, scan_id: u64, node_id: u32) -> Result<String, String> {
    let state = state.inner().clone();
    blocking(move || {
        state.with(scan_id, |tree, _| {
            tree.check(scan_id, node_id)?;
            Ok(tree.path(node_id).display().to_string())
        })
    })
    .await
}

/// Opens Explorer with the item selected (a folder scanned whole: opened).
#[tauri::command]
pub async fn disk_analyzer_reveal(state: State<'_, DiskAnalyzerState>, scan_id: u64, node_id: u32) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || {
        let path = state.with(scan_id, |tree, _| {
            tree.check(scan_id, node_id)?;
            Ok(tree.path(node_id))
        })?;
        // The scanned folder itself (`C:\` would end its quotes with `\"`).
        if node_id == 0 {
            return tauri_plugin_opener::open_path(path.display().to_string(), None::<&str>).map_err(|e| e.to_string());
        }
        use std::os::windows::process::CommandExt;
        // Explorer reads its own command line: `/select,"<path>"`.
        std::process::Command::new(crate::apps::process::explorer())
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Explorer did not start: {e}"))
    })
    .await
}

// ---------------------------------------------------------------------------
// Recycle Bin

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum DeleteEvent {
    Progress { done: usize, total: usize, current: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    path: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutcome {
    recycled: u32,
    /// The ids that went.
    ids: Vec<u32>,
    /// Size on disk given back.
    freed_bytes: u64,
    /// The same items by their size.
    freed_size: u64,
    skipped: Vec<Skipped>,
}

struct Target {
    id: u32,
    path: PathBuf,
    is_dir: bool,
    link: bool,
}

/// Moves items of the current scan to the Recycle Bin and takes them out of
/// the results, so the page shows the new totals without scanning again.
#[tauri::command]
pub async fn disk_analyzer_delete(
    state: State<'_, DiskAnalyzerState>,
    jobs: State<'_, Jobs>,
    scan_id: u64,
    node_ids: Vec<u32>,
    on_event: Channel<DeleteEvent>,
) -> Result<DeleteOutcome, String> {
    if node_ids.is_empty() {
        return Err("Nothing is selected.".into());
    }
    if node_ids.len() > MAX_IDS {
        return Err("Too many items at once.".into());
    }
    // Refused while the System Cleaner holds every job; installs may run.
    let reservation = jobs.start("disk-analyzer-delete")?;
    let state = state.inner().clone();
    blocking(move || {
        let _reservation = reservation;
        let (root, targets) = state.with(scan_id, |tree, _| {
            let mut wanted: Vec<u32> = Vec::new();
            for &id in &node_ids {
                tree.check(scan_id, id)?;
                if !wanted.contains(&id) {
                    wanted.push(id);
                }
            }
            // A folder takes what is inside it along: drop what is inside.
            let chosen: std::collections::HashSet<u32> = wanted.iter().copied().collect();
            let targets = wanted
                .into_iter()
                .filter(|&id| {
                    let trail = tree.trail(id);
                    !trail[..trail.len() - 1].iter().any(|up| chosen.contains(up))
                })
                .map(|id| Target {
                    id,
                    path: tree.path(id),
                    is_dir: tree.node(id).kind == Kind::Dir,
                    link: tree.node(id).kind == Kind::Link,
                })
                .collect::<Vec<_>>();
            Ok((tree.root.clone(), targets))
        })?;
        // A fresh look at this PC's protected places.
        let guard = Guard::for_this_pc(&root);
        let _com = delete::Com::init();
        let total = targets.len();
        let mut recycled = Vec::new();
        let mut skipped = Vec::new();
        for (done, target) in targets.iter().enumerate() {
            let _ = on_event.send(DeleteEvent::Progress {
                done,
                total,
                current: target.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            });
            let result = if target.link {
                Err(delete::LINK.to_string())
            } else {
                delete::recheck(&target.path, target.is_dir, &root, &guard).and_then(|()| delete::recycle(&target.path))
            };
            match result {
                Ok(()) => recycled.push(target.id),
                Err(reason) => skipped.push(Skipped { path: target.path.display().to_string(), reason }),
            }
        }
        let (mut freed_size, mut freed_bytes) = (0u64, 0u64);
        // The results move on with what really went, if they are still this scan's.
        let _ = state.with(scan_id, |tree, _| {
            for &id in &recycled {
                let (size, alloc) = tree.remove(id);
                freed_size += size;
                freed_bytes += alloc;
            }
            Ok(())
        });
        Ok(DeleteOutcome { recycled: recycled.len() as u32, ids: recycled, freed_bytes, freed_size, skipped })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_scan_stops_the_old_one_and_only_the_newest_is_kept() {
        let state = DiskAnalyzerState::default();
        let (first_id, first) = state.begin();
        let (second_id, second) = state.begin();
        assert!(second_id > first_id);
        assert!(first.load(Ordering::Relaxed), "the old scan was told to stop");
        let root = PathBuf::from(r"C:\x");
        let guard = || Guard::new(&root, &protect::Places::default());
        assert!(!state.keep(&first, Tree::new(first_id, root.clone(), 0), guard()));
        assert!(state.keep(&second, Tree::new(second_id, root.clone(), 0), guard()));
        assert!(state.with(second_id, |_, _| Ok(())).is_ok());
        assert_eq!(state.with(first_id, |_, _| Ok(())).unwrap_err(), tree::STALE);
        state.cancel();
        let (_, third) = state.begin();
        state.cancel();
        assert!(third.load(Ordering::Relaxed));
        assert!(state.with(second_id, |_, _| Ok(())).is_err(), "a new scan lets the last result go");
    }

    #[test]
    fn deleting_waits_for_the_system_cleaner() {
        let jobs = Jobs::default();
        let cleaning = jobs.start_exclusive("system-cleaner-test").unwrap();
        assert!(jobs.start("disk-analyzer-delete").is_err());
        drop(cleaning);
        let install = jobs.start("some-app").unwrap();
        assert!(jobs.start("disk-analyzer-delete").is_ok(), "installs may run alongside");
        drop(install);
    }

    #[test]
    fn only_folders_that_are_there_are_scanned() {
        assert!(scan_root("").is_err());
        assert!(scan_root("relative\\path").is_err());
        assert!(scan_root(r"C:\definitely-not-here-myle").is_err());
        let exe = std::env::current_exe().unwrap();
        assert!(scan_root(&exe.to_string_lossy()).is_err(), "a file is not a folder");
        let temp = scan_root(&std::env::temp_dir().to_string_lossy()).unwrap();
        assert!(temp.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn events_and_views_serialize_for_the_page() {
        let walked = scan::Progress { files: 1, dirs: 2, bytes: 3, alloc: 4, current: "C:\\x".into() };
        let progress = serde_json::to_value(ScanEvent::progress(&walked, None)).unwrap();
        assert_eq!(progress["event"], "progress");
        assert_eq!(progress["data"]["files"], 1);
        assert_eq!(progress["data"]["phase"], serde_json::Value::Null);
        let reading = serde_json::to_value(ScanEvent::progress(&scan::Progress::default(), Some("table"))).unwrap();
        assert_eq!(reading["data"]["phase"], "table");
        assert_eq!(serde_json::to_value(ScanEvent::Cancelled).unwrap()["event"], "cancelled");
        let mut tree = tree::tests::sample();
        let guard = Guard::new(&tree.root, &protect::Places::default());
        let page = tree.children(0, 0, 10, SortKey::Size, SizeMode::Size);
        let link = page.ids.iter().copied().find(|&id| tree.node(id).kind == Kind::Link).unwrap();
        let json = serde_json::to_value(view(&tree, &guard, link)).unwrap();
        assert_eq!(json["kind"], "link");
        assert_eq!(json["protected"], true, "links are never removed");
        assert_eq!(json["hasChildren"], false);
        let root = serde_json::to_value(view(&tree, &guard, 0)).unwrap();
        assert_eq!(root["protected"], true, "the scanned folder itself");
        let map = serde_json::to_value(tree.treemap(0, 4, 1, SizeMode::Size, 100)).unwrap();
        assert!(map["ids"].is_array() && map["scanId"] == 7);
        assert_eq!(serde_json::from_str::<SizeMode>("\"alloc\"").unwrap(), SizeMode::Alloc);
        assert_eq!(serde_json::from_str::<SortKey>("\"modified\"").unwrap(), SortKey::Modified);
    }
}
