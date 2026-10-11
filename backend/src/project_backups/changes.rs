//! Is a project's folder still what its newest backup holds?
//!
//! The folder is walked with the project's rules (what a backup would take)
//! and paired with the newest backup's files. A file is new or gone by its
//! name; changed when its size differs or, with the same size, when its
//! CRC-32 differs. The CRC is read whenever the file may have changed: its
//! date is not the one the backup recorded, or it was written after the
//! backup started. A file that can't be read is counted apart, never as
//! changed nor as the same. A false "No changes" is the one answer that must
//! never be given, so anything unsure is read.
//!
//! Detection runs in the background for the whole app, page open or not: a
//! watcher on the project folders checks a project a few seconds after its
//! last write, and every project is checked every few minutes (a missed
//! event, a folder on a network or removable drive, which is never watched).
//! The latest answers are kept here for the page and sent to it.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use zip::ZipArchive;

use super::compare::{self, FileInfo, Recorded, Side};
use super::rules::{BACKUP_INFO_FILE, Early, Rules, TreeListing, parent_of};
use super::store::{Project, Settings};
use super::{archive, engine, store, walk};

/// At most this many files are listed per project (the counts are whole).
const LIST_LIMIT: usize = 300;
/// A file written this long before a backup started may still be newer than
/// what the backup holds (clock steps, FAT's 2-second dates).
const MARGIN: Duration = Duration::from_secs(2);
/// When the backup does not say when it started: this long before its date.
const NO_START_MARGIN: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeState {
    UpToDate,
    Changed,
    NoBackup,
    Missing,
    /// The cloud folder or the project's backups can't be reached now (the
    /// cloud app is not running): nothing is known, so nothing is counted.
    Unreachable,
    /// The check failed; `message` says why.
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    /// `added`, `modified` or `deleted`.
    pub status: &'static str,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChanges {
    pub project_id: String,
    pub state: ChangeState,
    /// The backup compared with.
    pub backup_id: Option<String>,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    /// Files open in another program that could not be read to compare:
    /// neither changed nor the same as far as this check knows.
    pub unreadable: usize,
    pub files: Vec<ChangedFile>,
    /// Unix milliseconds, when the check started: of two answers for a
    /// project, the one started later is the true one.
    pub checked_at: i64,
    /// Why the check failed, or what can't be reached.
    pub message: Option<String>,
}

impl ProjectChanges {
    fn new(project_id: &str, state: ChangeState) -> ProjectChanges {
        ProjectChanges {
            project_id: project_id.to_string(),
            state,
            backup_id: None,
            added: 0,
            modified: 0,
            deleted: 0,
            unreadable: 0,
            files: Vec::new(),
            checked_at: now_ms(),
            message: None,
        }
    }
}

struct Cached {
    fingerprint: u64,
    result: ProjectChanges,
}

/// The last answer of each project, by the walk that gave it.
static CACHE: LazyLock<Mutex<HashMap<String, Cached>>> = LazyLock::new(Default::default);

/// A backup's files by normalized name, shared between checks.
type Files = Arc<BTreeMap<String, FileInfo>>;

/// The newest backup's files, read once per backup: a cloud zip is read
/// again only when it is replaced.
struct Listing {
    path: PathBuf,
    size: Option<u64>,
    modified: Option<i64>,
    files: Files,
    /// When the backup started, as its `.backup-info.json` says.
    started: Option<SystemTime>,
}

static LISTINGS: LazyLock<Mutex<HashMap<String, Listing>>> = LazyLock::new(Default::default);

/// The latest answer of each project, for the page.
static RESULTS: LazyLock<Mutex<HashMap<String, ProjectChanges>>> = LazyLock::new(Default::default);

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn crc_of(path: &Path) -> Option<u32> {
    let mut file = File::open(path).ok()?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buf).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Some(hasher.finalize())
}

/// The CRC of a file of the backup: stored in a zip, read for a folder one.
fn backup_crc(side: &Side, info: &FileInfo) -> Option<u32> {
    match side {
        Side::Zip(_) => info.crc,
        Side::Folder(root) => crc_of(&root.join(&info.stored)),
    }
}

/// Folders a walk of the project never enters: the backups themselves (a
/// project folder may hold the cloud folder) and MYLE's staging folder.
pub(crate) fn skip_dirs(settings: &Settings) -> Vec<PathBuf> {
    let mut skip = Vec::new();
    skip.extend(settings.backup_root());
    skip.extend(archive::staging_dir());
    skip
}

/// Checks one project (its answer is kept for the page). `fresh`: compare
/// again even when the folder looks as it did at the last check.
pub(crate) fn check(settings: &Settings, project: &Project, fresh: bool) -> ProjectChanges {
    let result = check_with(settings, project, &skip_dirs(settings), fresh);
    remember(std::slice::from_ref(&result));
    result
}

pub(crate) fn check_with(
    settings: &Settings,
    project: &Project,
    skip_dirs: &[PathBuf],
    fresh: bool,
) -> ProjectChanges {
    let mut result = ProjectChanges::new(&project.id, ChangeState::Missing);
    if let Err(message) = examine(settings, project, skip_dirs, fresh, &mut result) {
        lock(&CACHE).remove(&project.id);
        result = ProjectChanges {
            message: Some(message),
            ..ProjectChanges::new(&project.id, ChangeState::Error)
        };
    }
    result
}

/// Where a project's backups are: `Ok(None)` when none can exist yet.
fn backups_folder(settings: &Settings, project: &Project) -> Result<Option<PathBuf>, ChangeState> {
    let Some(root) = settings.backup_root() else {
        return Ok(None);
    };
    // The cloud folder itself missing is the cloud app not running (or a
    // drive not there): what the backups hold is unknown, not empty.
    if !root.parent().is_some_and(Path::is_dir) {
        return Err(ChangeState::Unreachable);
    }
    let Ok(dir) = engine::app_dir(&root, &project.app_name) else {
        return Err(ChangeState::Error);
    };
    match std::fs::read_dir(&dir) {
        Ok(_) => Ok(Some(dir)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ChangeState::Unreachable),
    }
}

fn examine(
    settings: &Settings,
    project: &Project,
    skip_dirs: &[PathBuf],
    fresh: bool,
    result: &mut ProjectChanges,
) -> Result<(), String> {
    let source = PathBuf::from(&project.source_path);
    if project.source_path.trim().is_empty() || !source.is_dir() {
        result.state = ChangeState::Missing;
        return Ok(());
    }
    let dir = match backups_folder(settings, project) {
        Ok(dir) => dir,
        Err(ChangeState::Unreachable) => {
            result.state = ChangeState::Unreachable;
            result.message = Some("The backups folder can't be reached now; is the cloud app running?".into());
            return Ok(());
        }
        Err(_) => return Err(format!("\"{}\" can't be used as a backup folder name.", project.app_name)),
    };
    let rules = Rules::compile(&settings.rule_input(Some(project)), Some(&source))?;
    let found = walk::walk(&source, &rules, skip_dirs, &|| false)?;
    let latest = dir.as_ref().and_then(|dir| {
        let entry = engine::list_backups(dir, &project.app_name)
            .into_iter()
            .find(|entry| !entry.broken)?;
        let path = engine::resolve_backup(dir, &project.app_name, &entry.id).ok()?;
        Some((entry, path))
    });

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for file in &found.files {
        file.rel.hash(&mut hasher);
        file.size.hash(&mut hasher);
        file.modified.hash(&mut hasher);
    }
    if let Some((entry, _)) = &latest {
        (&entry.id, entry.size, entry.modified).hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    if !fresh {
        let cache = lock(&CACHE);
        if let Some(cached) = cache.get(&project.id)
            && cached.fingerprint == fingerprint
        {
            *result = ProjectChanges { checked_at: result.checked_at, ..cached.result.clone() };
            return Ok(());
        }
    }

    let Some((entry, path)) = latest else {
        result.state = ChangeState::NoBackup;
        result.added = found.files.len();
        store_cache(&project.id, fingerprint, result);
        return Ok(());
    };
    result.backup_id = Some(entry.id.clone());
    let side = compare::side_for(&path)?;
    let (old, started) = listing(&project.id, &side, &path, &entry)
        .map_err(|error| format!("The newest backup ({}) can't be read: {error}", entry.name))?;
    // Files written after this were not seen by the backup.
    let backup_time = entry.modified.map(|ms| UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64));
    let reference = match (started, backup_time) {
        (Some(started), Some(at)) => Some(started.min(at).checked_sub(MARGIN).unwrap_or(UNIX_EPOCH)),
        (Some(started), None) => started.checked_sub(MARGIN),
        (None, Some(at)) => at.checked_sub(NO_START_MARGIN),
        (None, None) => None,
    };
    compare_files(&old, &side, &found, &rules, reference, result);
    store_cache(&project.id, fingerprint, result);
    Ok(())
}

/// The newest backup's files and start, from the cache while it is the same
/// backup (same name, size and date).
fn listing(
    project_id: &str,
    side: &Side,
    path: &Path,
    entry: &engine::BackupEntry,
) -> Result<(Files, Option<SystemTime>), String> {
    if let Some(known) = lock(&LISTINGS).get(project_id)
        && known.path == path
        && known.size == entry.size
        && known.modified == entry.modified
    {
        return Ok((known.files.clone(), known.started));
    }
    let files = Arc::new(compare::list_files(side, &|| false)?);
    let started = backup_started(side);
    lock(&LISTINGS).insert(
        project_id.to_string(),
        Listing {
            path: path.to_path_buf(),
            size: entry.size,
            modified: entry.modified,
            files: files.clone(),
            started,
        },
    );
    Ok((files, started))
}

/// `.backup-info.json` at the top of a backup, or inside its one folder.
fn is_info_name(name: &str) -> bool {
    let key = compare::normalize_name(name);
    key == BACKUP_INFO_FILE || key.split_once('/').is_some_and(|(_, rest)| rest == BACKUP_INFO_FILE)
}

/// When the backup started: the `timestamp` its `.backup-info.json` holds
/// (MYLE and backup_projects both write the start, in Unix milliseconds).
fn backup_started(side: &Side) -> Option<SystemTime> {
    let text = match side {
        Side::Zip(path) => {
            let mut archive = ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok()?;
            let name = archive.file_names().find(|name| is_info_name(name))?.to_string();
            let entry = archive.by_name(&name).ok()?;
            let mut text = String::new();
            entry.take(1 << 20).read_to_string(&mut text).ok()?;
            text
        }
        Side::Folder(root) => std::fs::read_to_string(root.join(BACKUP_INFO_FILE)).ok()?,
    };
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stamp = json.get("timestamp")?;
    let ms = stamp.as_u64().or_else(|| stamp.as_f64().filter(|ms| *ms >= 0.0).map(|ms| ms as u64))?;
    (ms > 0).then(|| UNIX_EPOCH + Duration::from_millis(ms))
}

/// Whether a same-size file's content has to be read: its date is not the
/// one the backup recorded, or it was written after the backup started.
fn may_differ(info: &FileInfo, modified: Option<SystemTime>, reference: Option<SystemTime>) -> bool {
    let Some(modified) = modified else {
        return true;
    };
    if reference.is_none_or(|reference| modified > reference) {
        return true;
    }
    match info.recorded {
        Some(Recorded::Dos { date, time }) => {
            let now = archive::dos_time(Some(modified));
            (now.datepart(), now.timepart()) != (date, time)
        }
        Some(Recorded::Time(at)) => {
            let apart = modified.duration_since(at).or_else(|_| at.duration_since(modified));
            apart.is_ok_and(|apart| apart > MARGIN)
        }
        None => true,
    }
}

/// Pairs the backup's files with the folder's and counts the differences.
fn compare_files(
    old: &BTreeMap<String, FileInfo>,
    side: &Side,
    found: &walk::Walk,
    rules: &Rules,
    reference: Option<SystemTime>,
    result: &mut ProjectChanges,
) {
    let mut new: BTreeMap<String, &walk::SourceFile> = BTreeMap::new();
    for file in &found.files {
        let key = compare::normalize_name(&file.rel);
        if key != BACKUP_INFO_FILE {
            new.insert(key, file);
        }
    }
    let names = |keys: Vec<&String>| keys.into_iter().map(|k| (k.clone(), ())).collect::<BTreeMap<_, _>>();
    let pairs = compare::match_files(&names(old.keys().collect()), &names(new.keys().collect()));
    // What the folders hold on both sides tells build output from sources, as
    // for a comparison: a backup's file the rules now leave out is not "gone".
    let listing = TreeListing::from_paths(pairs.keys().map(String::as_str));
    let mut files = Vec::new();
    for (display, pair) in &pairs {
        if display == BACKUP_INFO_FILE {
            continue;
        }
        let change = match (&pair.old, &pair.new) {
            (None, Some(key)) => Some(("added", Some(new[key].size))),
            (Some(_), None) if rules.excluded_path(display, &listing) => None,
            (Some(_), None) => Some(("deleted", None)),
            (Some(old_key), Some(new_key)) => {
                let (before, now) = (&old[old_key], new[new_key]);
                if before.size != now.size {
                    Some(("modified", Some(now.size)))
                } else if may_differ(before, now.modified, reference) {
                    match crc_of(&now.abs) {
                        None => {
                            result.unreadable += 1;
                            None
                        }
                        // A backup file that can't be read counts as changed.
                        Some(crc) => (backup_crc(side, before) != Some(crc)).then_some(("modified", Some(now.size))),
                    }
                } else {
                    None
                }
            }
            (None, None) => None,
        };
        if let Some((status, size)) = change {
            match status {
                "added" => result.added += 1,
                "deleted" => result.deleted += 1,
                _ => result.modified += 1,
            }
            if files.len() < LIST_LIMIT {
                files.push(ChangedFile { path: display.clone(), status, size });
            }
        }
    }
    result.files = files;
    result.state = if result.added + result.modified + result.deleted == 0 {
        ChangeState::UpToDate
    } else {
        ChangeState::Changed
    };
}

fn store_cache(project_id: &str, fingerprint: u64, result: &ProjectChanges) {
    lock(&CACHE).insert(project_id.to_string(), Cached { fingerprint, result: result.clone() });
}

/// Keeps the answers for the page: an answer never replaces a later one.
fn remember(results: &[ProjectChanges]) {
    let mut kept = lock(&RESULTS);
    for result in results {
        if kept.get(&result.project_id).is_none_or(|known| known.checked_at <= result.checked_at) {
            kept.insert(result.project_id.clone(), result.clone());
        }
    }
}

/// The latest answer of every project still in the list.
pub(crate) fn latest(settings: &Settings) -> Vec<ProjectChanges> {
    let kept = lock(&RESULTS);
    settings.projects.iter().filter_map(|project| kept.get(&project.id).cloned()).collect()
}

// ─── Detecting in the background ────────────────────────────────────────────

/// Event name: the answers of a background check (`ProjectChanges[]`).
pub const RESULTS_EVENT: &str = "project-backups-changes";
/// Every project is checked this often, events or not.
const CHECK_ALL_EVERY: Duration = Duration::from_secs(5 * 60);
/// A project is checked this long after its last write…
const QUIET_FOR: Duration = Duration::from_secs(3);
/// …but no later than this after the first one (a log written every second).
const AT_MOST: Duration = Duration::from_secs(30);
/// While a backup runs, checks wait (they would read what it writes).
const BUSY_WAIT: Duration = Duration::from_secs(2);
const STARTUP_DELAY: Duration = Duration::from_secs(5);

enum Signal {
    Paths(Vec<PathBuf>),
    /// The watcher lost events (its buffer overflowed): every project.
    Lost,
    /// Check these now (after a backup of them).
    Check(Vec<String>),
    /// The project list changed: watch again and check everything.
    Rewatch,
}

static DETECTOR: LazyLock<Mutex<Option<Sender<Signal>>>> = LazyLock::new(Default::default);

/// Starts detecting changes, once for the app; later calls watch the
/// folders again (the project list changed) and check every project.
pub(crate) fn start(app: tauri::AppHandle) -> Result<(), String> {
    let mut detector = lock(&DETECTOR);
    if let Some(sender) = detector.as_ref()
        && sender.send(Signal::Rewatch).is_ok()
    {
        return Ok(());
    }
    let (sender, receiver) = mpsc::channel();
    let own = sender.clone();
    std::thread::Builder::new()
        .name("project-backups-changes".into())
        .spawn(move || detect(app, own, receiver))
        .map_err(|error| error.to_string())?;
    *detector = Some(sender);
    Ok(())
}

/// Checks these projects again soon: right after a backup of them.
pub(crate) fn recheck(project_ids: Vec<String>) {
    if let Some(sender) = lock(&DETECTOR).as_ref() {
        let _ = sender.send(Signal::Check(project_ids));
    }
}

/// A watched project folder and its exclusions (the patterns only: cheap).
struct Root {
    id: String,
    path: PathBuf,
    rules: Rules,
}

fn watch_roots(settings: &Settings) -> Vec<Root> {
    settings
        .projects
        .iter()
        .filter(|project| !project.source_path.trim().is_empty())
        .map(|project| Root {
            id: project.id.clone(),
            path: PathBuf::from(&project.source_path),
            rules: Rules::compile(&settings.rule_input(Some(project)), None).unwrap_or_else(|_| Rules::none()),
        })
        .filter(|root| root.path.is_dir())
        .collect()
}

/// Every project whose folder holds `path` (projects may be nested).
fn owners<'a>(path: &Path, roots: &'a [Root]) -> Vec<&'a Root> {
    roots.iter().filter(|root| path.starts_with(&root.path)).collect()
}

/// Whether a write at `rel` (inside a project) may change its backup: not
/// when a folder above it is left out by the exclusions (`node_modules`,
/// `.git`), unless a keep pattern holds it. Anything undecided counts.
fn relevant(rel: &str, rules: &Rules) -> bool {
    let mut excluded = false;
    let mut dir = parent_of(rel);
    while !dir.is_empty() {
        match rules.early_dir(dir) {
            Early::Keep => return true,
            Early::Excluded(_) => excluded = true,
            Early::Undecided => {}
        }
        dir = parent_of(dir);
    }
    !excluded
}

/// Projects waiting to be checked after writes in their folders.
#[derive(Default)]
struct Debouncer {
    pending: Vec<String>,
    first: Option<Instant>,
    last: Option<Instant>,
    now: bool,
}

impl Debouncer {
    /// A relevant write in `id`'s folder at `at`.
    fn touch(&mut self, at: Instant, id: &str) {
        if !self.pending.iter().any(|known| known == id) {
            self.pending.push(id.to_string());
        }
        self.first.get_or_insert(at);
        self.last = Some(at);
    }

    /// Checked as soon as possible.
    fn soon(&mut self, at: Instant, ids: Vec<String>) {
        for id in ids {
            self.touch(at, &id);
        }
        self.now = true;
    }

    fn due_at(&self) -> Option<Instant> {
        if self.pending.is_empty() {
            return None;
        }
        let first = self.first?;
        if self.now {
            return Some(first);
        }
        let last = self.last.unwrap_or(first);
        Some((last + QUIET_FOR).min(first + AT_MOST))
    }

    fn due(&self, now: Instant) -> bool {
        self.due_at().is_some_and(|at| now >= at)
    }

    fn take(&mut self) -> Vec<String> {
        let pending = std::mem::take(&mut self.pending);
        *self = Debouncer::default();
        pending
    }
}

/// Folders on removable or network drives are not watched (a watch keeps
/// the drive in use, so it could not be ejected): the periodic check covers them.
#[cfg(windows)]
fn watchable(path: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_REMOTE: u32 = 4;
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        // A share (`\\server\folder`): on the network.
        return false;
    }
    let root: Vec<u16> = format!("{}\\", &text[..2]).encode_utf16().chain(Some(0)).collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string alive for the call.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    !matches!(kind, DRIVE_REMOVABLE | DRIVE_REMOTE)
}

#[cfg(not(windows))]
fn watchable(_path: &Path) -> bool {
    true
}

fn watcher_for(roots: &[Root], sender: Sender<Signal>) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let _ = match event {
            Ok(event) if !event.need_rescan() && !event.paths.is_empty() => sender.send(Signal::Paths(event.paths)),
            _ => sender.send(Signal::Lost),
        };
    })
    .ok()?;
    for root in roots.iter().filter(|root| watchable(&root.path)) {
        let _ = watcher.watch(&root.path, RecursiveMode::Recursive);
    }
    Some(watcher)
}

fn detect(app: tauri::AppHandle, sender: Sender<Signal>, receiver: Receiver<Signal>) {
    use tauri::Manager;
    let busy = || app.state::<super::state::ProjectBackupsState>().is_running();
    let mut roots = Vec::new();
    let mut watcher: Option<notify::RecommendedWatcher> = None;
    let mut debouncer = Debouncer::default();
    // The first check of every project waits for the app to have started.
    let mut check_all_at = Instant::now() + STARTUP_DELAY;
    let mut rewatch = true;
    loop {
        if rewatch {
            rewatch = false;
            // The old watch is closed before the new one opens.
            watcher.take();
            roots = store::load().map(|settings| watch_roots(&settings)).unwrap_or_default();
            watcher = watcher_for(&roots, sender.clone());
        }
        let now = Instant::now();
        let mut wait = check_all_at.saturating_duration_since(now);
        if let Some(at) = debouncer.due_at() {
            wait = wait.min(at.saturating_duration_since(now));
        }
        if busy() {
            wait = wait.max(BUSY_WAIT);
        }
        match receiver.recv_timeout(wait) {
            Ok(Signal::Paths(paths)) => {
                let at = Instant::now();
                for path in paths {
                    for root in owners(&path, &roots) {
                        let rel = path
                            .strip_prefix(&root.path)
                            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_default();
                        if relevant(&rel, &root.rules) {
                            debouncer.touch(at, &root.id);
                        }
                    }
                }
            }
            // Every project, soon; however often the watcher fails, at
            // most one check of them all per `AT_MOST`.
            Ok(Signal::Lost) => check_all_at = check_all_at.min(Instant::now() + AT_MOST),
            Ok(Signal::Check(ids)) => debouncer.soon(Instant::now(), ids),
            Ok(Signal::Rewatch) => {
                rewatch = true;
                check_all_at = Instant::now();
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if busy() {
            continue;
        }
        let now = Instant::now();
        let ids = if now >= check_all_at {
            check_all_at = now + CHECK_ALL_EVERY;
            debouncer.take();
            None
        } else if debouncer.due(now) {
            Some(debouncer.take())
        } else {
            continue;
        };
        check_and_tell(&app, ids);
    }
}

/// Checks `ids` (every project: `None`) and sends the answers to the page.
fn check_and_tell(app: &tauri::AppHandle, ids: Option<Vec<String>>) {
    use tauri::Emitter;
    let Ok(settings) = store::load() else {
        return;
    };
    let skip = skip_dirs(&settings);
    let results: Vec<ProjectChanges> = settings
        .projects
        .iter()
        .filter(|project| ids.as_ref().is_none_or(|ids| ids.contains(&project.id)))
        .map(|project| check_with(&settings, project, &skip, false))
        .collect();
    if ids.is_none() {
        lock(&RESULTS).retain(|id, _| settings.projects.iter().any(|project| &project.id == id));
    }
    remember(&results);
    if !results.is_empty() {
        let _ = app.emit(RESULTS_EVENT, &results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::archive::tests::{Quiet, temp};
    use crate::project_backups::compare::tests::make_zip;
    use crate::project_backups::engine::{BackupRequest, backup_project};
    use std::fs;

    struct Scene {
        root: PathBuf,
        settings: Settings,
    }

    impl Scene {
        fn new(name: &str) -> Scene {
            let root = temp(&format!("changes-{name}"));
            let source = root.join("source");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(root.join("cloud")).unwrap();
            let project = Project {
                id: format!("p-{name}"),
                name: "App".into(),
                source_path: source.display().to_string(),
                app_name: "App".into(),
                ..Project::default()
            };
            let settings = Settings {
                cloud_folder: Some(root.join("cloud").display().to_string()),
                projects: vec![project],
                ..Settings::default()
            };
            Scene { root, settings }
        }

        fn source(&self) -> PathBuf {
            self.root.join("source")
        }

        fn write(&self, rel: &str, body: &[u8]) -> PathBuf {
            let path = self.source().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
            path
        }

        fn backup(&self) -> PathBuf {
            let project = &self.settings.projects[0];
            let backup_root = self.settings.backup_root().unwrap();
            let request = BackupRequest {
                project,
                rules: self.settings.rule_input(Some(project)),
                backup_root: &backup_root,
                staging: None,
                app_version: "9.17.0",
                delays: &[1],
            };
            PathBuf::from(backup_project(&request, &Quiet).unwrap().zip_path)
        }

        fn check(&self) -> ProjectChanges {
            let project = &self.settings.projects[0];
            // Every check compares again: these tests change files in place.
            check_with(&self.settings, project, &skip_dirs(&self.settings), true)
        }
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn set_modified(path: &Path, at: SystemTime) {
        File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
    }

    #[test]
    fn an_edit_made_while_the_backup_was_copied_is_a_change() {
        let scene = Scene::new("during");
        let config = scene.write("src/config.ts", b"version = \"1.2.3\"");
        scene.write("README.md", b"readme");
        let zip = scene.backup();
        let started = SystemTime::now();
        // Same size, written after the zip read it, before its copy ended.
        fs::write(&config, b"version = \"1.2.4\"").unwrap();
        set_modified(&config, started + Duration::from_secs(1));
        set_modified(&zip, started + Duration::from_secs(60));
        let result = scene.check();
        assert_eq!(result.state, ChangeState::Changed, "{result:?}");
        assert_eq!((result.added, result.modified, result.deleted), (0, 1, 0));
    }

    #[test]
    fn an_older_file_put_back_with_its_old_date_is_a_change() {
        let scene = Scene::new("older");
        let config = scene.write("config.json", b"{\"a\":1}");
        scene.backup();
        fs::write(&config, b"{\"a\":2}").unwrap();
        set_modified(&config, SystemTime::now() - Duration::from_secs(24 * 60 * 60));
        let result = scene.check();
        assert_eq!(result.state, ChangeState::Changed, "{result:?}");
        assert_eq!(result.modified, 1);
    }

    #[test]
    fn a_folder_as_it_was_backed_up_has_no_changes() {
        let scene = Scene::new("same");
        scene.write("a.txt", b"one");
        scene.write("src/b.rs", b"fn main() {}");
        scene.backup();
        let result = scene.check();
        assert_eq!(result.state, ChangeState::UpToDate, "{result:?}");
        assert_eq!(result.unreadable, 0);
    }

    #[test]
    fn a_cloud_folder_out_of_reach_is_not_a_project_without_backups() {
        let mut scene = Scene::new("offline");
        scene.write("a.txt", b"one");
        scene.settings.cloud_folder = Some(scene.root.join("G-drive-not-running").display().to_string());
        let result = scene.check();
        assert_eq!(result.state, ChangeState::Unreachable, "{result:?}");
        assert_eq!(result.added, 0, "nothing is counted as changed");
    }

    #[test]
    fn backups_kept_inside_the_project_folder_are_not_its_files() {
        let mut scene = Scene::new("inside");
        scene.write("a.txt", b"one");
        // The cloud folder is inside the project, and zips are kept.
        scene.settings.cloud_folder = Some(scene.source().join("cloud").display().to_string());
        fs::create_dir_all(scene.source().join("cloud")).unwrap();
        scene.settings.projects[0].keep = vec!["*.zip".into()];
        scene.backup();
        let result = scene.check();
        assert_eq!(result.state, ChangeState::UpToDate, "{result:?}");
    }

    fn zip_backup(scene: &Scene, entries: &[(&str, &[u8])]) -> PathBuf {
        let month = scene.settings.backup_root().unwrap().join("App").join("2026-10 Οκτώβριος");
        fs::create_dir_all(&month).unwrap();
        let zip = month.join("App_D5_V12.zip");
        make_zip(&zip, entries);
        zip
    }

    #[test]
    fn the_info_file_inside_a_wrapped_zip_is_not_a_deleted_file() {
        let scene = Scene::new("wrapped");
        scene.write("a.txt", b"one");
        zip_backup(&scene, &[("App/a.txt", b"one"), ("App/.backup-info.json", b"{}")]);
        let result = scene.check();
        assert_eq!(result.state, ChangeState::UpToDate, "{result:?}");
    }

    #[test]
    fn a_file_the_rules_now_leave_out_is_not_a_deleted_file() {
        let scene = Scene::new("rules");
        scene.write("a.txt", b"one");
        let zip = zip_backup(&scene, &[("a.txt", b"one"), ("node_modules/x/index.js", b"x")]);
        let result = scene.check();
        assert_eq!(result.state, ChangeState::UpToDate, "{result:?}");
        // The comparison with the folder says the same.
        let project = &scene.settings.projects[0];
        let rules = Rules::compile(&scene.settings.rule_input(Some(project)), Some(&scene.source())).unwrap();
        let comparison = compare::compare(&Side::Zip(zip), &Side::Folder(scene.source()), &rules, None, &|| false, &|_, _| {}).unwrap();
        assert_eq!(comparison.added + comparison.modified + comparison.deleted, 0);
    }

    #[test]
    fn a_backup_that_is_not_a_zip_is_an_error_not_silence() {
        let scene = Scene::new("broken");
        scene.write("a.txt", b"one");
        let month = scene.settings.backup_root().unwrap().join("App").join("2026-10 Οκτώβριος");
        fs::create_dir_all(&month).unwrap();
        fs::write(month.join("App_D5_V2.zip"), vec![b'x'; 100]).unwrap();
        let result = scene.check();
        assert_eq!(result.state, ChangeState::Error, "{result:?}");
        assert!(result.message.is_some());
    }

    #[test]
    fn the_newest_backup_is_read_once_until_it_is_replaced() {
        let scene = Scene::new("listing");
        scene.write("a.txt", b"one");
        let zip = scene.backup();
        assert_eq!(scene.check().state, ChangeState::UpToDate);
        // Same name, size and date: the listing is not read again.
        let id = scene.settings.projects[0].id.clone();
        let known = lock(&LISTINGS).get(&id).map(|listing| Arc::as_ptr(&listing.files));
        assert_eq!(scene.check().state, ChangeState::UpToDate);
        assert_eq!(lock(&LISTINGS).get(&id).map(|listing| Arc::as_ptr(&listing.files)), known);
        assert!(lock(&LISTINGS).get(&id).and_then(|listing| listing.started).is_some(), "the start is read from the info file");
        drop(zip);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_open_without_sharing_is_unreadable_not_modified() {
        use std::os::windows::fs::OpenOptionsExt;
        let scene = Scene::new("locked");
        let data = scene.write("data.db", b"0123456789");
        scene.backup();
        fs::write(&data, b"9876543210").unwrap();
        let _held = File::options().read(true).share_mode(0).open(&data).unwrap();
        let result = scene.check();
        assert_eq!(result.modified, 0, "{result:?}");
        assert_eq!(result.unreadable, 1);
    }

    #[test]
    fn writes_in_left_out_folders_say_nothing() {
        let rules = Rules::compile(
            &crate::project_backups::rules::RuleInput {
                patterns: store::default_patterns(),
                keep: vec!["node_modules/kept/".into()],
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert!(relevant("bin/cli.js", &rules));
        assert!(relevant("build/installer.nsh", &rules));
        assert!(relevant("src/main.rs", &rules));
        assert!(!relevant("node_modules/x/y.js", &rules));
        assert!(!relevant(".git/index", &rules));
        assert!(!relevant(".venv/lib/x.py", &rules));
        assert!(relevant("node_modules/kept/a.js", &rules), "a keep pattern wins");
    }

    #[test]
    fn a_write_in_nested_projects_is_told_to_both() {
        let roots = vec![
            Root { id: "a".into(), path: PathBuf::from("/code"), rules: Rules::none() },
            Root { id: "b".into(), path: PathBuf::from("/code/app"), rules: Rules::none() },
            Root { id: "c".into(), path: PathBuf::from("/other"), rules: Rules::none() },
        ];
        let ids: Vec<&str> = owners(Path::new("/code/app/src/x.rs"), &roots).iter().map(|root| root.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn checks_wait_for_quiet_but_never_longer_than_the_limit() {
        let start = Instant::now();
        let mut debouncer = Debouncer::default();
        assert_eq!(debouncer.due_at(), None);
        debouncer.touch(start, "a");
        assert!(!debouncer.due(start + Duration::from_secs(2)));
        assert!(debouncer.due(start + QUIET_FOR));
        // Writes every second keep it waiting, up to the limit.
        for second in 1..40 {
            debouncer.touch(start + Duration::from_secs(second), "a");
        }
        assert_eq!(debouncer.due_at(), Some(start + AT_MOST));
        assert_eq!(debouncer.take(), ["a"]);
        assert_eq!(debouncer.due_at(), None);
        // After a backup: at once.
        debouncer.soon(start, vec!["b".into()]);
        assert!(debouncer.due(start));
    }

    #[test]
    fn a_later_answer_is_never_replaced_by_an_earlier_one() {
        let mut newer = ProjectChanges::new("remember-test", ChangeState::UpToDate);
        newer.checked_at = 2_000;
        let mut older = ProjectChanges::new("remember-test", ChangeState::Changed);
        older.checked_at = 1_000;
        remember(&[newer]);
        remember(&[older]);
        assert_eq!(lock(&RESULTS)["remember-test"].state, ChangeState::UpToDate);
        lock(&RESULTS).remove("remember-test");
    }
}
