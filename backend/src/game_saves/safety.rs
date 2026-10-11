//! The restore's safety net. Before Ludusavi writes a single save file, MYLE
//! copies every file the restore is about to overwrite into a slot of its own
//! (the files the fresh preview names, at the path really written, restore
//! locations included), byte for byte, and checks each copy by its SHA-256;
//! the files the restore will create are listed too. A game whose restore does
//! not finish cleanly is then put back exactly as it was, at once, and "Undo
//! last restore" puts every game of that restore back the same way.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::atomic;
use super::parser::ApiOutput;

/// The slot's list of what it holds.
pub(crate) const MANIFEST: &str = "manifest.json";
/// The copies themselves, by number, inside the slot.
const FILES: &str = "files";
/// Room left over on a drive after the copies or the restore, for Windows.
const SPARE_BYTES: u64 = 64 * 1024 * 1024;

/// A save file a restore will write, and the game it belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub game: String,
    pub path: PathBuf,
    /// The size the restore writes.
    pub bytes: u64,
}

/// What a slot holds: one entry per file the restore writes.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Manifest {
    pub version: u32,
    pub created_at: u64,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub game: String,
    pub target: String,
    /// Whether the file was there before the restore. When it was not, putting
    /// the game back removes it.
    pub existed: bool,
    /// The copy's name in the slot's `files` folder.
    #[serde(default)]
    pub copy: Option<String>,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub sha256: String,
    /// Read-only before the restore (Ludusavi clears the flag to write).
    #[serde(default)]
    pub readonly: bool,
}

impl Manifest {
    pub(crate) fn games(&self) -> Vec<String> {
        let mut games: Vec<String> = self.entries.iter().map(|entry| entry.game.clone()).collect();
        games.sort();
        games.dedup();
        games
    }
}

/// The files a restore will write, from its fresh preview (one game's): every
/// file not ignored whose backed-up copy differs from this PC's ("Same" files
/// are skipped by the restore and left alone here too).
pub(crate) fn targets(api: &ApiOutput) -> Vec<Target> {
    let mut found = BTreeMap::new();
    for (game, entry) in &api.games {
        for (path, file) in &entry.files {
            if file.ignored || file.change.as_deref() == Some("Same") {
                continue;
            }
            found.entry(windows_path(path)).or_insert_with(|| Target {
                game: game.clone(),
                path: windows_path(path),
                bytes: file.bytes,
            });
        }
    }
    found.into_values().collect()
}

/// Ludusavi writes paths with forward slashes ("C:/Users/…").
fn windows_path(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', "\\"))
}

/// Files among `targets` that another program holds open so that nothing else
/// may write them: the game (or its launcher) is still running. Writing then
/// would fail part-way through, or be undone when the game saves on exit.
pub(crate) fn locked(targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&target.path)
                .is_err_and(|error| in_use(&error))
        })
        .map(|target| target.path.display().to_string())
        .collect()
}

/// ERROR_SHARING_VIOLATION or ERROR_LOCK_VIOLATION: someone holds the file.
fn in_use(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}

/// Whether `folder`'s drive can take `needed` more bytes (with room to spare).
/// An error naming the drive when it cannot; unknown space counts as enough.
pub(crate) fn ensure_room(folder: &Path, needed: u64, what: &str) -> Result<(), String> {
    let Some(free) = free_space(folder) else {
        return Ok(());
    };
    if free < needed.saturating_add(SPARE_BYTES) {
        return Err(format!(
            "There is not enough free space for {what} on the drive of {} ({} MB free, {} MB needed).",
            folder.display(),
            free / (1024 * 1024),
            needed.saturating_add(SPARE_BYTES) / (1024 * 1024)
        ));
    }
    Ok(())
}

fn free_space(folder: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // The nearest folder that exists: the target's own may be created later.
    let existing = folder.ancestors().find(|path| path.is_dir())?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and alive for the call; the two other
    // out-parameters are optional and passed as null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

/// Copies every target that exists into `slot` and checks each copy, and
/// lists the targets that do not exist yet, then writes the slot's manifest.
/// Any failure is an error: the restore must not start without its copy.
pub(crate) fn take(slot: &Path, targets: &[Target], now: u64) -> Result<Manifest, String> {
    let files = slot.join(FILES);
    fs::create_dir_all(&files).map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        let shown = target.path.display();
        match fs::metadata(&target.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => entries.push(Entry {
                game: target.game.clone(),
                target: target.path.to_string_lossy().into_owned(),
                existed: false,
                copy: None,
                bytes: 0,
                sha256: String::new(),
                readonly: false,
            }),
            Err(error) => return Err(format!("{shown} could not be read: {error}")),
            Ok(meta) if meta.is_dir() => return Err(format!("{shown} is a folder where a save file goes")),
            Ok(meta) => {
                let name = index.to_string();
                let (bytes, sha256) = copy_verified(&target.path, &files.join(&name))
                    .map_err(|error| format!("{shown} could not be copied: {error}"))?;
                entries.push(Entry {
                    game: target.game.clone(),
                    target: target.path.to_string_lossy().into_owned(),
                    existed: true,
                    copy: Some(name),
                    bytes,
                    sha256,
                    readonly: meta.permissions().readonly(),
                });
            }
        }
    }
    let manifest = Manifest {
        version: 1,
        created_at: now,
        entries,
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    atomic::write(&slot.join(MANIFEST), &json)?;
    Ok(manifest)
}

/// The manifest of a slot MYLE copied itself; `None` for a slot made by an
/// older MYLE (a Ludusavi backup), which is put back by Ludusavi instead.
pub(crate) fn load(slot: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(slot.join(MANIFEST)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Puts the files of `games` (every game when `None`) back as they were
/// before the restore: each copy comes back (checked by its SHA-256 first),
/// files the restore created are removed, and read-only files are read-only
/// again. Answers what could not be put back, file by file.
pub(crate) fn put_back(slot: &Path, manifest: &Manifest, games: Option<&HashSet<String>>) -> Vec<String> {
    let mut failed = Vec::new();
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| games.is_none_or(|games| games.contains(&entry.game)))
    {
        let target = Path::new(&entry.target);
        let result = if entry.existed {
            restore_copy(slot, entry, target)
        } else {
            remove_created(target)
        };
        if let Err(error) = result {
            failed.push(format!("{}: {error}", entry.target));
        }
    }
    failed
}

/// After a game restored cleanly: its files that were read-only before are
/// read-only again (Ludusavi clears the flag to write them).
pub(crate) fn keep_readonly(manifest: &Manifest, games: &HashSet<String>) {
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.readonly && games.contains(&entry.game))
    {
        let _ = set_readonly(Path::new(&entry.target), true);
    }
}

fn restore_copy(slot: &Path, entry: &Entry, target: &Path) -> Result<(), String> {
    let copy = slot.join(FILES).join(entry.copy.as_deref().ok_or("its safety copy is missing")?);
    let (_, sha256) = hash_of(&copy).map_err(|error| format!("its safety copy cannot be read: {error}"))?;
    if sha256 != entry.sha256 {
        return Err("its safety copy is damaged".into());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    // Written beside the save and swapped in, so a failure part-way leaves
    // the save as it is rather than half-written.
    let staged = target.with_file_name(format!(
        "{}.myle-restore-{}",
        target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id()
    ));
    let _ = fs::remove_file(&staged);
    let result = (|| {
        copy_verified(&copy, &staged).map_err(|error| error.to_string())?;
        if target.exists() {
            set_readonly(target, false).map_err(|error| error.to_string())?;
        }
        // Replaces an existing file on Windows (MoveFileExW, REPLACE_EXISTING).
        fs::rename(&staged, target).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result?;
    if entry.readonly {
        set_readonly(target, true).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_created(target: &Path) -> Result<(), String> {
    match fs::remove_file(target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            set_readonly(target, false).map_err(|error| error.to_string())?;
            fs::remove_file(target).map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    if permissions.readonly() != readonly {
        permissions.set_readonly(readonly);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Copies `from` into the new file `to`, flushes it to disk, then reads the
/// copy back: it must hash the same as the original read. Answers the size and
/// the SHA-256.
fn copy_verified(from: &Path, to: &Path) -> io::Result<(u64, String)> {
    let mut source = File::open(from)?;
    let mut copy = OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        copy.write_all(&buffer[..read])?;
        bytes += read as u64;
    }
    copy.sync_all()?;
    drop(copy);
    let expected = crate::download::to_hex(&hasher.finalize());
    let (written, actual) = hash_of(to)?;
    if written != bytes || actual != expected {
        let _ = fs::remove_file(to);
        return Err(io::Error::other("the copy does not match the original"));
    }
    Ok((bytes, expected))
}

fn hash_of(path: &Path) -> io::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((bytes, crate::download::to_hex(&hasher.finalize())))
}

/// Reads every file of a snapshot zip to its end, which checks each one's
/// CRC: a damaged backup (a bad copy in a cloud folder, a disk error) then
/// stops the restore before any save is touched, instead of being half
/// written over a good save. A snapshot kept as a folder is not a zip and
/// passes.
pub(crate) fn verify_snapshot(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return Ok(());
    }
    let shown = path.display();
    let file = File::open(path).map_err(|error| format!("{shown} cannot be read: {error}"))?;
    let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
        .map_err(|error| format!("{shown} is not a readable backup: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("{shown} is damaged: {error}"))?;
        let name = entry.name().to_string();
        io::copy(&mut entry, &mut io::sink()).map_err(|error| format!("{shown} is damaged ({name}): {error}"))?;
    }
    Ok(())
}

/// Why backing `title` up now would destroy its older snapshots, if it would.
///
/// Ludusavi keeps a game's snapshots (`backup-*`) in the game's folder and
/// lists them in its `mapping.yaml`. When that file is missing, damaged, or no
/// longer lists snapshots it once did (a cloud folder that kept another PC's
/// copy and renamed this one), the next backup starts a new list and deletes
/// every snapshot it does not name. Such a game is not backed up until the
/// file is put right: its history is worth more than one more snapshot.
pub(crate) fn history_damage(backup: &Path, title: &str) -> Option<String> {
    let folder = backup.join(escape_folder_name(title));
    let entries = fs::read_dir(&folder).ok()?;
    let mut snapshots = Vec::new();
    let mut conflicts = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if lower.starts_with("backup-") {
            snapshots.push((name, entry.metadata().and_then(|meta| meta.modified()).ok()));
        } else if lower != "mapping.yaml" && lower.starts_with("mapping") && (lower.ends_with(".yaml") || lower.ends_with(".yml")) {
            conflicts.push(name);
        }
    }
    if snapshots.is_empty() {
        return None;
    }
    let mapping = folder.join("mapping.yaml");
    let shown = folder.display();
    if !conflicts.is_empty() {
        return Some(format!(
            "Not backed up, to keep its older backups: {shown} has copies of its backup list ({}), left by syncing from another PC. Keep the right one as mapping.yaml, then back up again.",
            conflicts.join(", ")
        ));
    }
    let text = match fs::read_to_string(&mapping) {
        Ok(text) => text,
        Err(_) => {
            return Some(format!(
                "Not backed up, to keep its older backups: the backup list (mapping.yaml) in {shown} is missing or cannot be read (in OneDrive, it may be online-only)."
            ));
        }
    };
    if !text.lines().any(|line| line.trim_start().starts_with("name:")) {
        return Some(format!("Not backed up, to keep its older backups: the backup list (mapping.yaml) in {shown} is damaged."));
    }
    // A snapshot older than the list that the list no longer names was lost
    // from it; a newer one is what an interrupted backup left (harmless).
    let listed_at = fs::metadata(&mapping).and_then(|meta| meta.modified()).ok();
    let forgotten: Vec<&str> = snapshots
        .iter()
        .filter(|(name, modified)| {
            !text.contains(name.as_str()) && matches!((modified, listed_at), (Some(made), Some(listed)) if *made < listed)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    if !forgotten.is_empty() {
        return Some(format!(
            "Not backed up, to keep its older backups: the backup list (mapping.yaml) in {shown} no longer names {}.",
            forgotten.join(", ")
        ));
    }
    None
}

/// The folder Ludusavi keeps a game's backups in (its `escape_folder_name`).
fn escape_folder_name(title: &str) -> String {
    let mut escaped: Vec<char> = title.chars().collect();
    if escaped.first() == Some(&'.') {
        escaped[0] = '_';
    }
    if escaped.last() == Some(&'.') {
        let last = escaped.len() - 1;
        escaped[last] = '_';
    }
    escaped
        .into_iter()
        .map(|character| if matches!(character, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') { '_' } else { character })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_saves::parser;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-safety-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn the_files_a_restore_writes_come_from_its_fresh_preview() {
        // A game the cached scan did not know as local, a redirected file,
        // a file that is the same already, and an ignored one.
        let api = parser::parse_api(
            r#"{"games":{"Game":{"decision":"Processed","change":"Different","files":{
                "C:/Users/me/AppData/Local/Game/save.dat":{"change":"Different","bytes":10},
                "D:/Saves/Game/profile.dat":{"change":"New","bytes":5,"originalPath":"C:/Users/old/Game/profile.dat"},
                "C:/Users/me/AppData/Local/Game/same.dat":{"change":"Same","bytes":3},
                "C:/Users/me/AppData/Local/Game/skip.dat":{"change":"Different","ignored":true,"bytes":3}
            }}}}"#,
        )
        .unwrap();
        let found = targets(&api);
        let paths: Vec<_> = found.iter().map(|target| target.path.to_string_lossy().into_owned()).collect();
        assert_eq!(paths, vec![r"C:\Users\me\AppData\Local\Game\save.dat", r"D:\Saves\Game\profile.dat"]);
        assert!(found.iter().all(|target| target.game == "Game"));
    }

    #[test]
    fn a_game_is_put_back_byte_for_byte_and_what_the_restore_made_is_removed() {
        let root = temp("put-back");
        let saves = root.join("saves");
        fs::create_dir_all(&saves).unwrap();
        let kept = saves.join("slot1.sav");
        let made = saves.join("slot2.sav");
        fs::write(&kept, b"before").unwrap();
        let targets = vec![
            Target { game: "Game".into(), path: kept.clone(), bytes: 5 },
            Target { game: "Game".into(), path: made.clone(), bytes: 5 },
        ];
        let slot = root.join("slot");
        let manifest = take(&slot, &targets, 1).unwrap();
        assert!(manifest.entries[0].existed && !manifest.entries[1].existed);

        // The restore writes both, then something goes wrong.
        fs::write(&kept, b"after!").unwrap();
        fs::write(&made, b"new").unwrap();
        assert!(put_back(&slot, &manifest, None).is_empty());
        assert_eq!(fs::read(&kept).unwrap(), b"before");
        assert!(!made.exists(), "a file the restore created is removed again");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_games_asked_for_are_put_back() {
        let root = temp("one-game");
        let a = root.join("a.sav");
        let b = root.join("b.sav");
        fs::write(&a, b"a1").unwrap();
        fs::write(&b, b"b1").unwrap();
        let targets = vec![
            Target { game: "A".into(), path: a.clone(), bytes: 2 },
            Target { game: "B".into(), path: b.clone(), bytes: 2 },
        ];
        let slot = root.join("slot");
        let manifest = take(&slot, &targets, 1).unwrap();
        fs::write(&a, b"a2").unwrap();
        fs::write(&b, b"b2").unwrap();
        let only_b: HashSet<String> = ["B".to_string()].into();
        assert!(put_back(&slot, &manifest, Some(&only_b)).is_empty());
        assert_eq!(fs::read(&a).unwrap(), b"a2", "a game that restored cleanly keeps its restore");
        assert_eq!(fs::read(&b).unwrap(), b"b1");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_damaged_safety_copy_is_never_written_over_a_save() {
        let root = temp("damaged");
        let save = root.join("save.sav");
        fs::write(&save, b"good").unwrap();
        let slot = root.join("slot");
        let manifest = take(&slot, &[Target { game: "G".into(), path: save.clone(), bytes: 4 }], 1).unwrap();
        fs::write(&save, b"restored").unwrap();
        fs::write(slot.join(FILES).join("0"), b"rot!").unwrap();
        let failed = put_back(&slot, &manifest, None);
        assert_eq!(failed.len(), 1);
        assert_eq!(fs::read(&save).unwrap(), b"restored", "left as it is rather than overwritten");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn read_only_saves_are_read_only_again() {
        let root = temp("readonly");
        let save = root.join("save.sav");
        fs::write(&save, b"keep").unwrap();
        set_readonly(&save, true).unwrap();
        let slot = root.join("slot");
        let manifest = take(&slot, &[Target { game: "G".into(), path: save.clone(), bytes: 4 }], 1).unwrap();
        assert!(manifest.entries[0].readonly);
        set_readonly(&save, false).unwrap();
        fs::write(&save, b"overwritten").unwrap();
        assert!(put_back(&slot, &manifest, None).is_empty());
        assert_eq!(fs::read(&save).unwrap(), b"keep");
        assert!(fs::metadata(&save).unwrap().permissions().readonly());
        set_readonly(&save, false).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_save_the_game_holds_open_is_reported_before_anything_is_written() {
        let root = temp("locked");
        let save = root.join("save.sav");
        fs::write(&save, b"in use").unwrap();
        let free = root.join("free.sav");
        fs::write(&free, b"free").unwrap();
        let held = OpenOptions::new().read(true).write(true).share_mode(0).open(&save).unwrap();
        let targets = vec![
            Target { game: "G".into(), path: save.clone(), bytes: 1 },
            Target { game: "G".into(), path: free.clone(), bytes: 1 },
            Target { game: "G".into(), path: root.join("missing.sav"), bytes: 1 },
        ];
        assert_eq!(locked(&targets), vec![save.display().to_string()]);
        drop(held);
        assert!(locked(&targets).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_damaged_snapshot_zip_is_refused_and_a_sound_one_passes() {
        let root = temp("zip");
        let path = root.join("backup-20260101T000000Z.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            zip.start_file("drive-C/Users/me/save.dat", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(&[7u8; 4096]).unwrap();
            zip.finish().unwrap();
        }
        verify_snapshot(&path).unwrap();
        // A flipped byte inside the compressed data fails its CRC.
        let mut bytes = fs::read(&path).unwrap();
        let middle = 60;
        bytes[middle] ^= 0xFF;
        fs::write(&path, &bytes).unwrap();
        assert!(verify_snapshot(&path).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_manifest_round_trips_and_an_older_slot_has_none() {
        let root = temp("manifest");
        let save = root.join("save.sav");
        fs::write(&save, b"x").unwrap();
        let slot = root.join("slot");
        let manifest = take(&slot, &[Target { game: "G".into(), path: save, bytes: 1 }], 9).unwrap();
        assert_eq!(load(&slot), Some(manifest));
        assert_eq!(load(&root.join("ludusavi-slot")), None);
        let _ = fs::remove_dir_all(root);
    }

    fn history(name: &str) -> (PathBuf, PathBuf) {
        let root = temp(name);
        let game = root.join("Hades");
        fs::create_dir_all(&game).unwrap();
        (root, game)
    }

    #[test]
    fn a_game_without_snapshots_is_backed_up() {
        let (root, game) = history("fresh");
        assert_eq!(history_damage(&root, "Hades"), None);
        assert_eq!(history_damage(&root, "Unknown"), None);
        fs::write(game.join("mapping.yaml"), "name: Hades\nbackups: []\n").unwrap();
        assert_eq!(history_damage(&root, "Hades"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshots_with_a_sound_list_are_backed_up() {
        let (root, game) = history("sound");
        fs::write(game.join("backup-20260101T000000Z.zip"), b"zip").unwrap();
        fs::write(game.join("mapping.yaml"), "name: Hades\nbackups:\n  - name: backup-20260101T000000Z.zip\n").unwrap();
        assert_eq!(history_damage(&root, "Hades"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_or_empty_list_protects_the_snapshots() {
        let (root, game) = history("missing");
        fs::write(game.join("backup-20260101T000000Z.zip"), b"zip").unwrap();
        assert!(history_damage(&root, "Hades").unwrap().contains("missing"));
        fs::write(game.join("mapping.yaml"), "").unwrap();
        assert!(history_damage(&root, "Hades").unwrap().contains("damaged"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_synced_conflict_copy_of_the_list_protects_the_snapshots() {
        let (root, game) = history("conflict");
        fs::write(game.join("backup-20260101T000000Z.zip"), b"zip").unwrap();
        fs::write(game.join("mapping.yaml"), "name: Hades\nbackups:\n  - name: backup-20260101T000000Z.zip\n").unwrap();
        fs::write(game.join("mapping-DESKTOP-7.yaml"), "name: Hades\n").unwrap();
        assert!(history_damage(&root, "Hades").unwrap().contains("mapping-DESKTOP-7.yaml"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_snapshot_the_list_forgot_protects_it_and_an_interrupted_one_does_not() {
        let (root, game) = history("forgotten");
        let old = game.join("backup-20250101T000000Z.zip");
        fs::write(&old, b"zip").unwrap();
        let list = game.join("mapping.yaml");
        fs::write(&list, "name: Hades\nbackups: []\n").unwrap();
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        File::options().write(true).open(&old).unwrap().set_modified(earlier).unwrap();
        assert!(history_damage(&root, "Hades").unwrap().contains("backup-20250101T000000Z.zip"));
        // Newer than the list: left by a backup that was stopped, not lost.
        File::options().write(true).open(&old).unwrap().set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(history_damage(&root, "Hades"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_names_follow_the_engine() {
        assert_eq!(escape_folder_name("Hades"), "Hades");
        assert_eq!(escape_folder_name("Half-Life 2: Episode One"), "Half-Life 2_ Episode One");
        assert_eq!(escape_folder_name(".hack//G.U."), "_hack__G.U_");
        assert_eq!(escape_folder_name("What?<>|*\"x"), "What______x");
    }
}
