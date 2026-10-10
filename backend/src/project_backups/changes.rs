//! Is a project's folder still what its newest backup holds?
//!
//! The folder is walked with the project's rules (what a backup would take)
//! and paired with the newest backup's files. A file is new or gone by its
//! name; changed when its size differs, or, with the same size, when it was
//! modified after the backup and its CRC-32 differs (only then is it read).
//! A fingerprint of the walk (names, sizes, dates) skips the whole check
//! when nothing moved since the last one.
//!
//! A watcher on the project folders tells the page, a few seconds after the
//! last write, which projects to check again.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::compare::{self, FileInfo, Side};
use super::rules::{BACKUP_INFO_FILE, Rules};
use super::{engine, store, walk};

/// At most this many files are listed per project (the counts are whole).
const LIST_LIMIT: usize = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeState {
    UpToDate,
    Changed,
    NoBackup,
    Missing,
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
    pub files: Vec<ChangedFile>,
    /// Unix milliseconds.
    pub checked_at: i64,
}

struct Cached {
    backup_id: Option<String>,
    fingerprint: u64,
    result: ProjectChanges,
}

static CACHE: LazyLock<Mutex<HashMap<String, Cached>>> = LazyLock::new(Default::default);

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn crc_of(path: &Path) -> Option<u32> {
    let mut file = std::fs::File::open(path).ok()?;
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

/// Checks one project. `fresh`: ignore the cached answer.
pub(crate) fn check(project_id: &str, fresh: bool) -> Result<ProjectChanges, String> {
    let settings = store::load()?;
    let project = settings.project(project_id)?.clone();
    let mut result = ProjectChanges {
        project_id: project_id.to_string(),
        state: ChangeState::Missing,
        backup_id: None,
        added: 0,
        modified: 0,
        deleted: 0,
        files: Vec::new(),
        checked_at: now_ms(),
    };
    let source = PathBuf::from(&project.source_path);
    if !source.is_dir() {
        return Ok(result);
    }
    let rules = Rules::compile(&settings.rule_input(Some(&project)), Some(&source))?;
    let found = walk::walk(&source, &rules, &[], &|| false)?;

    let latest = settings.backup_root().and_then(|root| {
        let dir = engine::app_dir(&root, &project.app_name).ok()?;
        let entry = engine::list_backups(&dir, &project.app_name)
            .into_iter()
            .find(|entry| !entry.broken)?;
        let path = engine::resolve_backup(&dir, &project.app_name, &entry.id).ok()?;
        Some((entry, path))
    });
    let backup_id = latest.as_ref().map(|(entry, _)| entry.id.clone());

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for file in &found.files {
        file.rel.hash(&mut hasher);
        file.size.hash(&mut hasher);
        file.modified.hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    if !fresh {
        let cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = cache.get(project_id)
            && cached.fingerprint == fingerprint
            && cached.backup_id == backup_id
        {
            let mut again = cached.result.clone();
            again.checked_at = result.checked_at;
            return Ok(again);
        }
    }

    let Some((entry, path)) = latest else {
        result.state = ChangeState::NoBackup;
        result.added = found.files.len();
        store_cache(project_id, None, fingerprint, &result);
        return Ok(result);
    };
    result.backup_id = Some(entry.id.clone());
    let side = compare::side_for(&path)?;
    let old = compare::list_files(&side, &|| false)?;
    let backup_time = entry
        .modified
        .map(|ms| UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64));

    let mut new: BTreeMap<String, &walk::SourceFile> = BTreeMap::new();
    for file in &found.files {
        let key = compare::normalize_name(&file.rel);
        if key != BACKUP_INFO_FILE {
            new.insert(key, file);
        }
    }
    let names = |keys: Vec<&String>| keys.into_iter().map(|k| (k.clone(), ())).collect::<BTreeMap<_, _>>();
    let pairs = compare::match_files(&names(old.keys().collect()), &names(new.keys().collect()));
    let mut files = Vec::new();
    for (display, pair) in &pairs {
        let change = match (&pair.old, &pair.new) {
            (None, Some(key)) => Some(("added", Some(new[key].size))),
            (Some(_), None) => Some(("deleted", None)),
            (Some(old_key), Some(new_key)) => {
                let (before, now) = (&old[old_key], new[new_key]);
                let changed = if before.size != now.size {
                    true
                } else {
                    let newer = match (now.modified, backup_time) {
                        (Some(modified), Some(at)) => modified > at,
                        _ => true,
                    };
                    newer && backup_crc(&side, before) != crc_of(&now.abs)
                };
                changed.then_some(("modified", Some(now.size)))
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
    store_cache(project_id, Some(entry.id), fingerprint, &result);
    Ok(result)
}

fn store_cache(project_id: &str, backup_id: Option<String>, fingerprint: u64, result: &ProjectChanges) {
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).insert(
        project_id.to_string(),
        Cached { backup_id, fingerprint, result: result.clone() },
    );
}

// ─── Watching the folders ───────────────────────────────────────────────────

/// Folders whose writes say nothing about the sources (build output, git).
const NOISE: &[&str] = &[".git", "node_modules", "target", "dist", "build", ".svelte-kit", ".next", "__pycache__", ".vs", ".idea", "bin", "obj"];

/// Event name: the ids of the projects whose folders changed.
pub const CHANGED_EVENT: &str = "project-backups-changed";

static WATCHER: LazyLock<Mutex<Option<notify::RecommendedWatcher>>> = LazyLock::new(Default::default);

/// Watches the projects' folders (again, after the list changed). Writes are
/// gathered for 3 seconds after the last one, then the page is told.
pub(crate) fn watch(app: tauri::AppHandle) -> Result<(), String> {
    use notify::{RecursiveMode, Watcher};
    use tauri::Emitter;

    let settings = store::load()?;
    let roots: Vec<(String, PathBuf)> = settings
        .projects
        .iter()
        .map(|p| (p.id.clone(), PathBuf::from(&p.source_path)))
        .filter(|(_, path)| path.is_dir())
        .collect();
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })
    .map_err(|e| e.to_string())?;
    for (_, root) in &roots {
        let _ = watcher.watch(root, RecursiveMode::Recursive);
    }
    std::thread::spawn(move || {
        let mut pending: Vec<String> = Vec::new();
        loop {
            let wait = if pending.is_empty() { Duration::from_secs(3600) } else { Duration::from_secs(3) };
            match rx.recv_timeout(wait) {
                Ok(path) => {
                    let Some((id, root)) = roots.iter().find(|(_, root)| path.starts_with(root)) else { continue };
                    let noisy = path
                        .strip_prefix(root)
                        .map(|rel| rel.components().any(|c| NOISE.iter().any(|n| c.as_os_str().eq_ignore_ascii_case(n))))
                        .unwrap_or(true);
                    if !noisy && !pending.contains(id) {
                        pending.push(id.clone());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !pending.is_empty() {
                        let _ = app.emit(CHANGED_EVENT, std::mem::take(&mut pending));
                    }
                }
                // The watcher was replaced: this thread ends with it.
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    *WATCHER.lock().unwrap_or_else(|p| p.into_inner()) = Some(watcher);
    Ok(())
}
