//! Incremental backups: only the files that changed since the newest full
//! backup and the increments made on it.
//!
//! An increment lives in `<backup root>\<App>\Incremental\`, never in a month
//! folder, and its name (`increment-V<base version>-<n>.zip`) never starts
//! with `<App>_` nor holds a version above its base's: the backup lists, the
//! version numbering and the import (here and in backup_projects) never take
//! it for a backup. It holds the added and modified files and a
//! `.backup-info.json` (`format: "zip-incremental"`) naming its base, the
//! increment before it, the files deleted since, and `state`: every file the
//! project then held with its size, CRC-32 and date, so a restore checks
//! every byte it puts back. It is written and checked like a full backup
//! (`archive::write_verified`). Nothing is ever deleted: full backups stay the
//! anchors, and a full backup is made instead whenever an increment would
//! make a long or risky chain.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use super::archive::{self, ZipJob};
use super::changes;
use super::compare::{self, FileInfo, Recorded, Side};
use super::engine::{self, BackupEntry, BackupOutcome, BackupRequest, SkippedItem};
use super::rules::{BACKUP_INFO_FILE, Rules};
use super::walk::{self, SourceFile};
use super::{Failure, Sink, Stage, naming};

/// The folder of the increments, next to the month folders.
pub(crate) const FOLDER: &str = "Incremental";
pub(crate) const FORMAT: &str = "zip-incremental";
/// At most this many increments on one full backup; then a full one.
const MAX_CHAIN: u32 = 10;
/// A full backup older than this gets no more increments.
const MAX_BASE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// More than this share of the project's bytes changed: a full backup.
const MAX_SHARE: f64 = 0.5;
/// Leftovers of interrupted backups older than this are removed.
const STALE_PARTIAL: Duration = Duration::from_secs(60 * 60);
/// An info file larger than this is not read (a project of ~500k files).
const MAX_INFO_BYTES: u64 = 64 * 1024 * 1024;

/// One file as an increment recorded it: size, CRC-32, date (Unix ms).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Stamp(pub u64, pub u32, pub i64);

/// An increment's `.backup-info.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Info {
    pub format: String,
    pub name: String,
    /// The full backup it builds on: `<month folder>/<name>.zip`.
    pub base: String,
    pub base_version: u32,
    /// The increment before it, none for the first.
    #[serde(default)]
    pub previous: Option<String>,
    pub sequence: u32,
    /// Files gone since the increment before it (or the base).
    #[serde(default)]
    pub deleted: Vec<String>,
    /// Every file of the project after this increment.
    pub state: BTreeMap<String, Stamp>,
    #[serde(default)]
    pub empty_folders: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    /// When the backup started (Unix ms).
    pub timestamp: i64,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub source_path: String,
    /// The files this increment holds, and their bytes.
    #[serde(default)]
    pub file_count: usize,
    #[serde(default)]
    pub total_bytes: u64,
}

pub(crate) struct Increment {
    pub path: PathBuf,
    pub info: Arc<Info>,
    pub size: u64,
    pub modified: Option<i64>,
}

impl Increment {
    /// `Incremental/<name>.zip`, as the page names it.
    pub(crate) fn id(&self) -> String {
        format!("{FOLDER}/{}.zip", self.info.name)
    }
}

/// Increments read once while unchanged (size and date), by path.
type Known = HashMap<PathBuf, (u64, Option<i64>, Arc<Info>)>;
static INFOS: LazyLock<Mutex<Known>> = LazyLock::new(Default::default);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn increment_name(base_version: u32, sequence: u32) -> String {
    format!("increment-V{base_version}-{sequence}")
}

/// The `.backup-info.json` at the top of the zip at `path`.
fn info_text(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("Can't open \"{}\": {error}", path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|error| format!("\"{}\" is not a readable zip: {error}", path.display()))?;
    let entry = archive
        .by_name(BACKUP_INFO_FILE)
        .map_err(|_| format!("\"{}\" has no {BACKUP_INFO_FILE}", path.display()))?;
    let mut text = String::new();
    entry
        .take(MAX_INFO_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| format!("Can't read the details of \"{}\": {error}", path.display()))?;
    Ok(text)
}

/// The increment at `path`, if it is one.
fn increment(path: &Path) -> Option<Increment> {
    let meta = fs::metadata(path).ok().filter(|meta| meta.is_file())?;
    let (size, modified) = (meta.len(), meta.modified().ok().map(naming::unix_millis));
    if let Some((known_size, known_modified, info)) = lock(&INFOS).get(path)
        && *known_size == size
        && *known_modified == modified
    {
        return Some(Increment { path: path.to_path_buf(), info: info.clone(), size, modified });
    }
    let info: Info = serde_json::from_str(&info_text(path).ok()?).ok()?;
    if info.format != FORMAT {
        return None;
    }
    let info = Arc::new(info);
    lock(&INFOS).insert(path.to_path_buf(), (size, modified, info.clone()));
    Some(Increment { path: path.to_path_buf(), info, size, modified })
}

/// The increments on the full backup `base`, oldest first. The chain stops
/// at the first one that is missing, can't be read or does not follow.
pub(crate) fn chain(app_dir: &Path, base: &BackupEntry) -> Vec<Increment> {
    let folder = app_dir.join(FOLDER);
    let mut chain: Vec<Increment> = Vec::new();
    if !folder.is_dir() {
        return chain;
    }
    for sequence in 1..=MAX_CHAIN {
        let path = folder.join(format!("{}.zip", increment_name(base.version, sequence)));
        let Some(next) = increment(&path) else {
            break;
        };
        let previous = chain.last().map(|known| known.info.name.clone());
        if next.info.base != base.id || next.info.sequence != sequence || next.info.previous != previous {
            break;
        }
        chain.push(next);
    }
    chain
}

/// What the files of a project were at its last backup.
struct Previous {
    files: BTreeMap<String, FileInfo>,
    /// Files written after this were not seen by that backup.
    reference: Option<SystemTime>,
}

fn millis(time: Option<SystemTime>) -> i64 {
    time.map(naming::unix_millis).unwrap_or(0)
}

fn reference(started_ms: i64, file_ms: Option<i64>) -> Option<SystemTime> {
    let at = |ms: i64| UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64);
    let earliest = match file_ms {
        Some(file_ms) => started_ms.min(file_ms),
        None => started_ms,
    };
    (earliest > 0).then(|| at(earliest).checked_sub(Duration::from_secs(2)).unwrap_or(UNIX_EPOCH))
}

/// The state an increment ends with, as a backup listing.
pub(crate) fn listing_of(info: &Info) -> BTreeMap<String, FileInfo> {
    info.state
        .iter()
        .map(|(key, stamp)| {
            (
                key.clone(),
                FileInfo {
                    stored: key.clone(),
                    size: stamp.0,
                    crc: Some(stamp.1),
                    recorded: Some(Recorded::Time(UNIX_EPOCH + Duration::from_millis(stamp.2.max(0) as u64))),
                },
            )
        })
        .collect()
}

/// An increment to write, or why a full backup is made instead.
pub(crate) struct Plan {
    base: BackupEntry,
    sequence: u32,
    previous: Option<String>,
    /// The added and modified files.
    files: Vec<SourceFile>,
    crcs: HashMap<String, u32>,
    deleted: Vec<String>,
    state: BTreeMap<String, Stamp>,
}

/// Plans an increment of `found` (the project as a backup would take it) on
/// the newest full backup. `Err`: why a full backup is made instead.
fn plan(app_dir: &Path, app_name: &str, found: &walk::Walk, now: SystemTime, sink: &dyn Sink) -> Result<Plan, String> {
    let base = engine::list_backups(app_dir, app_name)
        .into_iter()
        .find(|entry| !entry.broken)
        .ok_or("there is no full backup yet")?;
    if base.kind != "zip" {
        return Err("the newest full backup is a folder".into());
    }
    let base_path = engine::resolve_backup(app_dir, app_name, &base.id)?;
    let base_info: serde_json::Value = serde_json::from_str(&info_text(&base_path)?)
        .map_err(|_| "the newest full backup has no readable details".to_string())?;
    if base_info.get("createdBy").and_then(|v| v.as_str()) != Some("MYLE")
        || base_info.get("format").and_then(|v| v.as_str()) != Some("zip")
    {
        return Err("the newest full backup was not made by MYLE".into());
    }
    let base_started = base_info.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
    let age = naming::unix_millis(now).saturating_sub(base_started);
    if base_started <= 0 || age > MAX_BASE_AGE.as_millis() as i64 {
        return Err("the newest full backup is more than 30 days old".into());
    }
    let chain = chain(app_dir, &base);
    if chain.len() as u32 >= MAX_CHAIN {
        return Err(format!("{MAX_CHAIN} increments were made on the newest full backup"));
    }
    let previous = match chain.last() {
        Some(last) => Previous {
            files: listing_of(&last.info),
            reference: reference(last.info.timestamp, last.modified),
        },
        None => Previous {
            files: compare::list_files(&Side::Zip(base_path.clone()), &|| sink.cancelled())?,
            reference: reference(base_started, base.modified),
        },
    };

    let mut files = Vec::new();
    let mut crcs = HashMap::new();
    let mut state = BTreeMap::new();
    let mut changed_bytes = 0u64;
    for file in &found.files {
        if sink.cancelled() {
            return Err(super::CANCELLED.into());
        }
        let key = compare::normalize_name(&file.rel);
        if key == BACKUP_INFO_FILE {
            continue;
        }
        let known = previous.files.get(&key);
        let crc = match known {
            Some(info) if info.size == file.size && !changes::may_differ(info, file.modified, previous.reference) => {
                info.crc.ok_or("the last backup has no checksum for a file")?
            }
            _ => changes::crc_of(&file.abs).ok_or_else(|| format!("\"{}\" can't be read", file.rel))?,
        };
        let same = known.is_some_and(|info| info.size == file.size && info.crc == Some(crc));
        if !same {
            changed_bytes += file.size;
            crcs.insert(file.rel.clone(), crc);
            files.push(file.clone());
        }
        state.insert(key, Stamp(file.size, crc, millis(file.modified)));
    }
    let deleted: Vec<String> = previous.files.keys().filter(|key| !state.contains_key(*key)).cloned().collect();
    if files.is_empty() && deleted.is_empty() {
        return Err("nothing changed since the last backup".into());
    }
    if changed_bytes as f64 > found.total_bytes() as f64 * MAX_SHARE {
        return Err("most of the project changed".into());
    }
    Ok(Plan {
        sequence: chain.len() as u32 + 1,
        previous: chain.last().map(|last| last.info.name.clone()),
        base,
        files,
        crcs,
        deleted,
        state,
    })
}

/// Backs up only what changed since the newest backup of the project.
/// `Ok(Err(why))`: no increment fits, a full backup is to be made instead.
pub(crate) fn backup(request: &BackupRequest, sink: &dyn Sink) -> Result<Result<BackupOutcome, String>, Failure> {
    let project = request.project;
    let source = PathBuf::from(&project.source_path);
    if project.source_path.trim().is_empty() || !source.is_dir() {
        return Err(Failure::SourceMissing);
    }
    sink.stage(Stage::Preparing);
    let app_dir = engine::app_dir(request.backup_root, &project.app_name)?;
    if !app_dir.is_dir() {
        return Ok(Err("there is no full backup yet".into()));
    }
    naming::remove_stale_partials(&app_dir, STALE_PARTIAL);
    if let Some(staging) = request.staging {
        archive::clean_staging(staging);
    }
    let now = SystemTime::now();
    sink.stage(Stage::Scanning);
    let rules = Rules::compile(&request.rules, Some(&source)).map_err(Failure::Message)?;
    let mut skip_dirs = vec![request.backup_root.to_path_buf()];
    skip_dirs.extend(request.staging.map(Path::to_path_buf));
    let found = walk::walk(&source, &rules, &skip_dirs, &|| sink.cancelled())?;
    if found.files.is_empty() {
        return Ok(Err("the project folder has no files to back up".into()));
    }
    let plan = match plan(&app_dir, &project.app_name, &found, now, sink) {
        Ok(plan) => plan,
        Err(why) if why == super::CANCELLED => return Err(Failure::Cancelled),
        Err(why) => return Ok(Err(why)),
    };
    let name = increment_name(plan.base.version, plan.sequence);
    let final_path = app_dir.join(FOLDER).join(format!("{name}.zip"));
    if final_path.exists() {
        return Ok(Err(format!("{name}.zip is already there")));
    }
    let total_bytes: u64 = plan.files.iter().map(|file| file.size).sum();
    let info = Info {
        format: FORMAT.into(),
        name: name.clone(),
        base: plan.base.id.clone(),
        base_version: plan.base.version,
        previous: plan.previous.clone(),
        sequence: plan.sequence,
        deleted: plan.deleted.clone(),
        state: plan.state,
        empty_folders: found.empty_dirs.clone(),
        created_at: naming::iso_utc(now),
        timestamp: naming::unix_millis(now),
        created_by: "MYLE".into(),
        app_version: request.app_version.into(),
        source_path: project.source_path.clone(),
        file_count: plan.files.len(),
        total_bytes,
    };
    let info = serde_json::to_vec(&info).map_err(|error| Failure::Message(error.to_string()))?;
    let buffers = vec![(BACKUP_INFO_FILE.to_string(), info)];
    // The zip must hold exactly the planned files, as they were planned: a
    // file written in between would make `state` wrong.
    let mut completeness = |written: &archive::Written| -> Result<(), Failure> {
        sink.stage(Stage::CheckingCompleteness);
        let held = written.manifest.len().saturating_sub(1);
        for file in &plan.files {
            let planned = (file.size, plan.crcs[&file.rel]);
            if written.manifest.get(&file.rel).map(|entry| (entry.size, entry.crc)) != Some(planned) {
                return Err(Failure::Message(format!(
                    "\"{}\" changed while it was backed up. The increment was not saved; back up again.",
                    file.rel
                )));
            }
        }
        if held != plan.files.len() {
            return Err(Failure::Message("The increment does not hold the planned files. It was not saved.".into()));
        }
        Ok(())
    };
    let verified = archive::write_verified(
        &final_path,
        ZipJob { files: &plan.files, empty_dirs: &found.empty_dirs, buffers: &buffers },
        request.staging,
        request.delays,
        &mut completeness,
        sink,
        None,
    )?;
    let skipped: Vec<SkippedItem> = found
        .skipped
        .iter()
        .map(|item| SkippedItem { path: item.rel.clone(), reason: item.reason.describe().into() })
        .collect();
    Ok(Ok(BackupOutcome {
        name,
        zip_path: final_path.display().to_string(),
        month_folder: FOLDER.into(),
        version: plan.base.version,
        file_count: plan.files.len() as u64,
        total_bytes,
        zip_size: verified.zip_size,
        excluded_count: found.excluded.len(),
        skipped,
        linked_files: found.links,
        final_check_ok: verified.final_check_ok,
        created_at: naming::unix_millis(now),
        kind: "incremental",
        base: Some(plan.base.name),
        deleted_count: plan.deleted.len(),
    }))
}

// ─── Restoring ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub folder: String,
    pub files: usize,
    pub bytes: u64,
    /// The backups it was put together from, oldest first.
    pub from: Vec<String>,
}

/// Puts the project as last backed up (the newest full backup and its
/// increments) into `target`, an empty folder. Every file is checked against
/// the size and CRC-32 the backup recorded; on any difference nothing is
/// left behind. Nothing is ever replaced.
pub(crate) fn restore(app_dir: &Path, app_name: &str, target: &Path, cancelled: &dyn Fn() -> bool) -> Result<Restored, String> {
    let base = engine::list_backups(app_dir, app_name)
        .into_iter()
        .find(|entry| !entry.broken)
        .ok_or("This project has no backup yet.")?;
    if base.kind != "zip" {
        return Err("The newest backup is a folder: copy it from the backups folder instead.".into());
    }
    let base_path = engine::resolve_backup(app_dir, app_name, &base.id)?;
    let chain = chain(app_dir, &base);
    let base_files = compare::list_files(&Side::Zip(base_path.clone()), cancelled)?;
    let wanted: BTreeMap<String, (u64, u32)> = match chain.last() {
        Some(last) => last.info.state.iter().map(|(key, stamp)| (key.clone(), (stamp.0, stamp.1))).collect(),
        None => base_files
            .iter()
            .map(|(key, info)| Ok((key.clone(), (info.size, info.crc.ok_or("A file of the backup has no checksum.")?))))
            .collect::<Result<_, String>>()?,
    };
    // Newest first: a file comes from the last backup that holds it.
    let mut sources: Vec<(PathBuf, BTreeMap<String, FileInfo>)> = Vec::new();
    for increment in chain.iter().rev() {
        sources.push((increment.path.clone(), compare::list_files(&Side::Zip(increment.path.clone()), cancelled)?));
    }
    sources.push((base_path.clone(), base_files));
    let empty_folders = match chain.last() {
        Some(last) => last.info.empty_folders.clone(),
        None => base_dirs(&base_path)?,
    };

    fs::create_dir_all(target).map_err(|error| format!("Can't create \"{}\": {error}", target.display()))?;
    let is_empty = fs::read_dir(target)
        .map_err(|error| format!("Can't open \"{}\": {error}", target.display()))?
        .next()
        .is_none();
    if !is_empty {
        return Err("Choose an empty folder: a restore never replaces files.".into());
    }
    let result = put_together(target, &wanted, &sources, &empty_folders, cancelled);
    if result.is_err() {
        // The folder was empty: what is in it now came from this restore.
        if let Ok(entries) = fs::read_dir(target) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            }
        }
    }
    let bytes = result?;
    let mut from = vec![base.name.clone()];
    from.extend(chain.iter().map(|increment| increment.info.name.clone()));
    Ok(Restored { folder: target.display().to_string(), files: wanted.len(), bytes, from })
}

/// The empty folder entries of a full backup.
fn base_dirs(path: &Path) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|error| format!("Can't open \"{}\": {error}", path.display()))?;
    let archive = ZipArchive::new(BufReader::new(file))
        .map_err(|error| format!("\"{}\" is not a readable zip: {error}", path.display()))?;
    Ok(archive
        .file_names()
        .filter(|name| name.ends_with('/'))
        .map(|name| name.trim_end_matches('/').to_string())
        .collect())
}

fn put_together(
    target: &Path,
    wanted: &BTreeMap<String, (u64, u32)>,
    sources: &[(PathBuf, BTreeMap<String, FileInfo>)],
    empty_folders: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<u64, String> {
    let mut archives: Vec<Option<ZipArchive<BufReader<File>>>> = sources.iter().map(|_| None).collect();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];
    for (key, &(size, crc)) in wanted {
        if cancelled() {
            return Err(super::CANCELLED.into());
        }
        let (index, info) = sources
            .iter()
            .enumerate()
            .find_map(|(index, (_, files))| files.get(key).map(|info| (index, info)))
            .ok_or_else(|| format!("\"{key}\" is in none of the backups: the restore was stopped."))?;
        if archives[index].is_none() {
            let path = &sources[index].0;
            let file = File::open(path).map_err(|error| format!("Can't open \"{}\": {error}", path.display()))?;
            archives[index] = Some(
                ZipArchive::new(BufReader::new(file))
                    .map_err(|error| format!("\"{}\" is not a readable zip: {error}", path.display()))?,
            );
        }
        let archive = archives[index].as_mut().ok_or("The backup could not be opened.")?;
        let mut entry = archive
            .by_name(&info.stored)
            .map_err(|error| format!("Can't read \"{key}\" from the backup: {error}"))?;
        let destination = compare::folder_file(target, key)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("Can't create \"{}\": {error}", parent.display()))?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| format!("Can't create \"{}\": {error}", destination.display()))?;
        let mut hasher = crc32fast::Hasher::new();
        let mut written = 0u64;
        loop {
            let read = entry.read(&mut buffer).map_err(|error| format!("Can't read \"{key}\" from the backup: {error}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output
                .write_all(&buffer[..read])
                .map_err(|error| format!("Can't write \"{}\": {error}", destination.display()))?;
            written += read as u64;
        }
        output.sync_all().map_err(|error| format!("Can't write \"{}\": {error}", destination.display()))?;
        if (written, hasher.finalize()) != (size, crc) {
            return Err(format!("\"{key}\" is not what the backup recorded: the restore was stopped and nothing was kept."));
        }
        bytes += written;
    }
    for folder in empty_folders {
        let path = compare::folder_file(target, folder)?;
        fs::create_dir_all(&path).map_err(|error| format!("Can't create \"{}\": {error}", path.display()))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::archive::tests::{Quiet, temp};
    use crate::project_backups::store::{Project, Settings};

    struct Scene {
        root: PathBuf,
        settings: Settings,
        backup_root: PathBuf,
    }

    impl Scene {
        fn new(name: &str) -> Scene {
            let root = temp(&format!("increment-{name}"));
            fs::create_dir_all(root.join("source")).unwrap();
            fs::create_dir_all(root.join("cloud")).unwrap();
            let project = Project {
                id: format!("inc-{name}"),
                name: "App".into(),
                source_path: root.join("source").display().to_string(),
                app_name: "App".into(),
                ..Project::default()
            };
            let settings = Settings {
                cloud_folder: Some(root.join("cloud").display().to_string()),
                projects: vec![project],
                ..Settings::default()
            };
            Scene { backup_root: settings.backup_root().unwrap(), settings, root }
        }

        fn write(&self, rel: &str, body: &[u8]) {
            let path = self.root.join("source").join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }

        fn app_dir(&self) -> PathBuf {
            self.settings.backup_root().unwrap().join("App")
        }

        fn request(&self) -> BackupRequest<'_> {
            let project = &self.settings.projects[0];
            BackupRequest {
                project,
                rules: self.settings.rule_input(Some(project)),
                backup_root: &self.backup_root,
                staging: None,
                app_version: "9.17.0",
                delays: &[1],
            }
        }

        fn full(&self) -> BackupOutcome {
            engine::backup_project(&self.request(), &Quiet).unwrap()
        }

        fn increment(&self) -> Result<BackupOutcome, String> {
            backup(&self.request(), &Quiet).unwrap()
        }
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn names_in(zip: &str) -> Vec<String> {
        let archive = ZipArchive::new(BufReader::new(File::open(zip).unwrap())).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    #[test]
    fn an_increment_holds_only_what_changed_and_the_whole_state() {
        let scene = Scene::new("only");
        scene.write("a.txt", b"one");
        scene.write("src/b.rs", b"fn main() {}");
        scene.write("old.txt", b"old");
        let full = scene.full();
        scene.write("a.txt", b"uno");
        scene.write("new.txt", b"new");
        fs::remove_file(scene.root.join("source/old.txt")).unwrap();
        let outcome = scene.increment().unwrap();
        assert_eq!(outcome.kind, "incremental");
        assert_eq!(outcome.name, format!("increment-V{}-1", full.version));
        assert_eq!(names_in(&outcome.zip_path), [".backup-info.json", "a.txt", "new.txt"]);
        let info: Info = serde_json::from_str(&info_text(Path::new(&outcome.zip_path)).unwrap()).unwrap();
        assert_eq!(info.format, FORMAT);
        assert_eq!(info.base, format!("{}/{}.zip", full.month_folder, full.name));
        assert_eq!(info.deleted, ["old.txt"]);
        assert_eq!(info.state.keys().collect::<Vec<_>>(), ["a.txt", "new.txt", "src/b.rs"]);
        assert_eq!(info.state["a.txt"].1, crc32fast::hash(b"uno"));
        assert_eq!(info.state["src/b.rs"].1, crc32fast::hash(b"fn main() {}"));
    }

    #[test]
    fn increments_never_count_as_backups_nor_change_the_numbering() {
        let scene = Scene::new("numbering");
        scene.write("a.txt", b"one");
        scene.write("b.txt", b"two two two two");
        let full = scene.full();
        scene.write("a.txt", b"uno");
        scene.increment().unwrap();
        scene.write("a.txt", b"ena");
        let second = scene.increment().unwrap();
        assert_eq!(second.name, format!("increment-V{}-2", full.version));
        let listed = engine::list_backups(&scene.app_dir(), "App");
        assert_eq!(listed.len(), 1, "only the full backup is listed");
        assert_eq!(naming::next_version(&scene.app_dir(), "App"), full.version + 1);
        assert_eq!(chain(&scene.app_dir(), &listed[0]).len(), 2);
    }

    #[test]
    fn a_full_backup_is_made_when_an_increment_does_not_fit() {
        let scene = Scene::new("policy");
        scene.write("a.txt", b"one");
        assert_eq!(scene.increment().unwrap_err(), "there is no full backup yet");
        scene.write("big.bin", &[7u8; 4096]);
        scene.full();
        assert_eq!(scene.increment().unwrap_err(), "nothing changed since the last backup");
        scene.write("big.bin", &[8u8; 4096]);
        assert_eq!(scene.increment().unwrap_err(), "most of the project changed");
    }

    #[test]
    fn a_restore_puts_the_last_backed_up_project_together_and_checks_it() {
        let scene = Scene::new("restore");
        scene.write("a.txt", b"one");
        scene.write("keep/b.txt", b"bee bee bee bee bee");
        scene.write("gone.txt", b"gone");
        fs::create_dir_all(scene.root.join("source/empty")).unwrap();
        scene.full();
        scene.write("a.txt", b"uno");
        scene.increment().unwrap();
        scene.write("c.txt", b"sea");
        fs::remove_file(scene.root.join("source/gone.txt")).unwrap();
        scene.increment().unwrap();

        let target = scene.root.join("restored");
        let restored = restore(&scene.app_dir(), "App", &target, &|| false).unwrap();
        assert_eq!(restored.files, 3);
        assert_eq!(restored.from.len(), 3);
        assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"uno");
        assert_eq!(fs::read(target.join("keep/b.txt")).unwrap(), b"bee bee bee bee bee");
        assert_eq!(fs::read(target.join("c.txt")).unwrap(), b"sea");
        assert!(!target.join("gone.txt").exists());
        assert!(target.join("empty").is_dir());

        // Never into a folder with files in it.
        let error = restore(&scene.app_dir(), "App", &target, &|| false).unwrap_err();
        assert!(error.contains("empty folder"), "{error}");
        assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"uno");
    }

    #[test]
    fn a_restore_refuses_an_increment_that_does_not_match_its_record() {
        let scene = Scene::new("tampered");
        scene.write("a.txt", b"one");
        scene.write("b.txt", b"bee bee bee bee bee bee");
        scene.full();
        scene.write("a.txt", b"uno");
        let outcome = scene.increment().unwrap();
        // The increment's record says a.txt is "uno"; its content is not.
        let path = PathBuf::from(&outcome.zip_path);
        let mut info: Info = serde_json::from_str(&info_text(&path).unwrap()).unwrap();
        info.state.get_mut("a.txt").unwrap().1 ^= 1;
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("a.txt", options).unwrap();
        zip.write_all(b"uno").unwrap();
        zip.start_file(BACKUP_INFO_FILE, options).unwrap();
        zip.write_all(&serde_json::to_vec(&info).unwrap()).unwrap();
        zip.finish().unwrap();
        lock(&INFOS).remove(&path);

        let target = scene.root.join("restored");
        let error = restore(&scene.app_dir(), "App", &target, &|| false).unwrap_err();
        assert!(error.contains("not what the backup recorded"), "{error}");
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0, "nothing is left behind");
    }

    #[test]
    fn detection_compares_with_the_newest_increment() {
        let scene = Scene::new("detect");
        scene.write("a.txt", b"one");
        scene.write("b.txt", b"bee bee bee bee bee bee");
        scene.full();
        scene.write("a.txt", b"uno");
        scene.increment().unwrap();
        let project = &scene.settings.projects[0];
        let skip = changes::skip_dirs(&scene.settings);
        let result = changes::check_with(&scene.settings, project, &skip, true);
        assert_eq!(result.state, changes::ChangeState::UpToDate, "{result:?}");
        assert!(result.backup_id.as_deref().is_some_and(|id| id.starts_with("Incremental/")));
        scene.write("b.txt", b"BEE bee bee bee bee bee");
        let result = changes::check_with(&scene.settings, project, &skip, true);
        assert_eq!((result.state, result.modified), (changes::ChangeState::Changed, 1), "{result:?}");
    }
}
