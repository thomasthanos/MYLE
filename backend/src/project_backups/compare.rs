//! Comparing two backups (zips, older folder backups, or the project folder
//! itself) file by file, and two versions of one text file line by line.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use icu_normalizer::ComposingNormalizerBorrowed;
use serde::Serialize;
use similar::{DiffTag, TextDiff};
use zip::ZipArchive;

use super::rules::{BACKUP_INFO_FILE, Rules, TreeListing};
use super::walk;

/// Text files larger than this are compared by size only.
const MAX_DIFF_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) enum Side {
    Zip(PathBuf),
    Folder(PathBuf),
}

#[derive(Clone, Debug)]
pub(crate) struct FileInfo {
    /// The name as stored, to read the file again.
    pub stored: String,
    pub size: u64,
    pub crc: Option<u32>,
}

/// `\` → `/`, Unicode NFC, no empty or `.` parts: the same file under any
/// tool's naming.
pub(crate) fn normalize_name(name: &str) -> String {
    let posix = name.replace('\\', "/");
    let nfc = ComposingNormalizerBorrowed::new_nfc().normalize(&posix);
    nfc.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Every file of a backup, by normalized name.
pub(crate) fn list_files(
    side: &Side,
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeMap<String, FileInfo>, String> {
    let mut files = BTreeMap::new();
    match side {
        Side::Zip(path) => {
            let file = File::open(path)
                .map_err(|error| format!("Can't open \"{}\": {error}", path.display()))?;
            let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|error| {
                format!("\"{}\" is not a readable zip: {error}", path.display())
            })?;
            for index in 0..archive.len() {
                let entry = archive
                    .by_index_raw(index)
                    .map_err(|error| format!("Can't read \"{}\": {error}", path.display()))?;
                let stored = entry.name().to_string();
                if stored.ends_with('/') || stored.ends_with('\\') {
                    continue;
                }
                let key = normalize_name(&stored);
                if key.is_empty() || key == BACKUP_INFO_FILE || files.contains_key(&key) {
                    continue;
                }
                files.insert(
                    key,
                    FileInfo {
                        stored,
                        size: entry.size(),
                        crc: Some(entry.crc32()),
                    },
                );
            }
        }
        Side::Folder(path) => {
            let found = walk::walk(path, &Rules::none(), &[], cancelled)?;
            for file in found.files {
                let key = normalize_name(&file.rel);
                if key == BACKUP_INFO_FILE {
                    continue;
                }
                files.insert(
                    key,
                    FileInfo {
                        stored: file.rel,
                        size: file.size,
                        crc: None,
                    },
                );
            }
        }
    }
    Ok(files)
}

/// The one folder every name is under, if there is one.
fn single_root(keys: &[&String]) -> Option<String> {
    let mut root: Option<&str> = None;
    for key in keys {
        let (first, _) = key.split_once('/')?;
        match root {
            None => root = Some(first),
            Some(existing) if existing.to_lowercase() == first.to_lowercase() => {}
            Some(_) => return None,
        }
    }
    root.map(str::to_string)
}

/// `key` without its first folder (every key is under it when `prefix` is set).
fn strip<'a>(key: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        key
    } else {
        key.split_once('/').map(|(_, rest)| rest).unwrap_or(key)
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Pair {
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Pairs the files of two backups by name, ignoring case and, when one side
/// has everything inside a single folder (a zip made by another tool:
/// `steam-idler/package.json`), that folder if it pairs more files.
pub(crate) fn match_files<T>(
    old: &BTreeMap<String, T>,
    new: &BTreeMap<String, T>,
) -> BTreeMap<String, Pair> {
    let fold = |key: &str| key.to_lowercase();
    let old_keys: Vec<&String> = old.keys().collect();
    let new_keys: Vec<&String> = new.keys().collect();
    let score = |old_strip: &str, new_strip: &str| {
        let names: HashSet<String> = new_keys
            .iter()
            .map(|key| fold(strip(key, new_strip)))
            .collect();
        old_keys
            .iter()
            .filter(|key| names.contains(&fold(strip(key, old_strip))))
            .count()
    };
    let old_root = single_root(&old_keys);
    let new_root = single_root(&new_keys);
    let mut candidates = vec![(String::new(), String::new())];
    if let Some(root) = &old_root {
        candidates.push((root.clone(), String::new()));
    }
    if let Some(root) = &new_root {
        candidates.push((String::new(), root.clone()));
    }
    if let (Some(a), Some(b)) = (&old_root, &new_root) {
        candidates.push((a.clone(), b.clone()));
    }
    let mut best = candidates[0].clone();
    let mut best_score = score("", "");
    for candidate in &candidates[1..] {
        let value = score(&candidate.0, &candidate.1);
        if value > best_score {
            best = candidate.clone();
            best_score = value;
        }
    }
    let (old_strip, new_strip) = best;

    let mut pairs: BTreeMap<String, Pair> = BTreeMap::new();
    let mut by_fold: HashMap<String, String> = HashMap::new();
    for key in &new_keys {
        let display = strip(key, &new_strip).to_string();
        if by_fold.contains_key(&fold(&display)) {
            continue;
        }
        by_fold.insert(fold(&display), display.clone());
        pairs.insert(
            display,
            Pair {
                old: None,
                new: Some((*key).clone()),
            },
        );
    }
    for key in &old_keys {
        let display = strip(key, &old_strip).to_string();
        if let Some(existing) = by_fold.get(&fold(&display)) {
            let pair = pairs.get_mut(existing).expect("paired name");
            if pair.old.is_none() {
                pair.old = Some((*key).clone());
            }
            continue;
        }
        by_fold.insert(fold(&display), display.clone());
        pairs.insert(
            display,
            Pair {
                old: Some((*key).clone()),
                new: None,
            },
        );
    }
    pairs
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Deleted,
    Modified,
    Added,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    /// How the page can show it: text, image or other binary.
    pub kind: FileKind,
    pub status: Status,
    pub old_name: Option<String>,
    pub new_name: Option<String>,
    pub old_size: Option<u64>,
    pub new_size: Option<u64>,
    /// A deleted file that is still in the project folder: the newer backup
    /// is the one missing it.
    pub still_in_source: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub changes: Vec<Change>,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub unchanged: usize,
}

/// A file inside `root` by its relative name, refusing names that leave it.
fn folder_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(format!("Invalid file name \"{rel}\""));
        }
        path.push(part);
    }
    Ok(path)
}

fn folder_crc(side: &Side, info: &FileInfo, cancelled: &dyn Fn() -> bool) -> Result<u32, String> {
    let Side::Folder(root) = side else {
        return info.crc.ok_or_else(|| "Missing CRC".to_string());
    };
    let mut file = File::open(folder_file(root, &info.stored)?)
        .map_err(|error| format!("Can't read \"{}\": {error}", info.stored))?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        if cancelled() {
            return Err(super::CANCELLED.into());
        }
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Can't read \"{}\": {error}", info.stored))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize())
}

/// The changes from `old` to `new`. Files `rules` leaves out of backups are
/// left out here too; `source` (the project folder) marks deleted files that
/// are still there. `progress` gets the files compared so far and in all.
pub(crate) fn compare(
    old: &Side,
    new: &Side,
    rules: &Rules,
    source: Option<&Path>,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64, u64),
) -> Result<Comparison, String> {
    let old_files = list_files(old, cancelled)?;
    let new_files = list_files(new, cancelled)?;
    let pairs = match_files(&old_files, &new_files);
    // What the folders hold on either side tells build output from sources
    // (a `Cargo.toml` next to `target`), as it did for the backup.
    let listing = TreeListing::from_paths(pairs.keys().map(String::as_str));
    let mut result = Comparison::default();
    let total = pairs.len() as u64;
    for (index, (path, pair)) in pairs.into_iter().enumerate() {
        if cancelled() {
            return Err(super::CANCELLED.into());
        }
        progress(index as u64, total);
        if path == BACKUP_INFO_FILE || rules.excluded_path(&path, &listing) {
            continue;
        }
        let old_info = pair.old.as_ref().and_then(|key| old_files.get(key));
        let new_info = pair.new.as_ref().and_then(|key| new_files.get(key));
        let status = match (old_info, new_info) {
            (Some(_), None) => Status::Deleted,
            (None, Some(_)) => Status::Added,
            (Some(a), Some(b)) => {
                if a.size != b.size
                    || folder_crc(old, a, cancelled)? != folder_crc(new, b, cancelled)?
                {
                    Status::Modified
                } else {
                    result.unchanged += 1;
                    continue;
                }
            }
            (None, None) => continue,
        };
        let still_in_source = status == Status::Deleted
            && source.is_some_and(|root| folder_file(root, &path).is_ok_and(|file| file.is_file()));
        match status {
            Status::Added => result.added += 1,
            Status::Modified => result.modified += 1,
            Status::Deleted => result.deleted += 1,
        }
        result.changes.push(Change {
            kind: FileKind::of(&path),
            path,
            status,
            old_name: old_info.map(|info| info.stored.clone()),
            new_name: new_info.map(|info| info.stored.clone()),
            old_size: old_info.map(|info| info.size),
            new_size: new_info.map(|info| info.size),
            still_in_source,
        });
    }
    progress(total, total);
    result.changes.sort_by(|a, b| {
        a.status
            .cmp(&b.status)
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    Same,
    Removed,
    Added,
    Changed,
    /// Unchanged lines left out.
    Gap,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub kind: RowKind,
    pub old_line: Option<usize>,
    pub old_text: Option<String>,
    pub new_line: Option<usize>,
    pub new_text: Option<String>,
}

/// How a changed file can be shown, from its name (the content decides in
/// the end: a "text" file with NUL bytes is shown as binary).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    Text,
    Image,
    Binary,
}

const IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
];

const BINARY_TYPES: &[&str] = &[
    "exe", "dll", "so", "dylib", "lib", "a", "bin", "dat", "db", "sqlite", "sqlite3", "pdf",
    "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4", "m4a", "wav", "ogg", "flac", "avi", "mov",
    "mkv", "webm", "psd", "ai", "blend", "fbx", "glb", "jar", "class", "wasm", "node", "pyd",
    "iso", "msi", "nupkg", "docx", "xlsx", "pptx", "icns", "tga", "tif", "tiff", "dds", "jks",
    "keystore", "pfx", "p12",
];

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default()
}

fn image_type(path: &str) -> Option<&'static str> {
    let ext = extension(path);
    IMAGE_TYPES
        .iter()
        .find(|(known, _)| *known == ext)
        .map(|(_, mime)| *mime)
}

impl FileKind {
    pub(crate) fn of(path: &str) -> FileKind {
        if image_type(path).is_some() {
            FileKind::Image
        } else if BINARY_TYPES.contains(&extension(path).as_str()) {
            FileKind::Binary
        } else {
            FileKind::Text
        }
    }
}

/// One side of a file shown as binary.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideInfo {
    pub size: u64,
    /// Hex SHA-256, for files up to `MAX_HASH_BYTES`.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum FileDiff {
    Text {
        rows: Vec<Row>,
        identical: bool,
        /// Only the line endings differ (CRLF / LF).
        line_endings_differ: bool,
        /// More rows than `MAX_ROWS`: the rest is left out.
        truncated: bool,
        old_lines: usize,
        new_lines: usize,
        /// Set when a side is not UTF-8 (`UTF-16 LE`).
        encoding: Option<String>,
    },
    Image {
        mime: String,
        /// `data:` URLs.
        old: Option<String>,
        new: Option<String>,
        old_info: Option<SideInfo>,
        new_info: Option<SideInfo>,
        /// The source, for SVG.
        rows: Option<Vec<Row>>,
    },
    Binary {
        old: Option<SideInfo>,
        new: Option<SideInfo>,
        /// Text too large to show line by line.
        too_large: bool,
    },
}

/// Images larger than this are described, not shown.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
/// Files larger than this get no SHA-256 (reading them takes too long).
const MAX_HASH_BYTES: u64 = 512 * 1024 * 1024;
/// Rows of a diff sent to the page at most.
const MAX_ROWS: usize = 20_000;

/// Runs `read` on the file `stored` of `side`.
fn with_reader<T>(
    side: &Side,
    stored: &str,
    read: impl FnOnce(&mut dyn Read) -> Result<T, String>,
) -> Result<T, String> {
    match side {
        Side::Zip(path) => {
            let file = File::open(path).map_err(|error| format!("Can't open the zip: {error}"))?;
            let mut archive = ZipArchive::new(BufReader::new(file))
                .map_err(|error| format!("Can't read the zip: {error}"))?;
            let mut entry = archive
                .by_name(stored)
                .map_err(|error| format!("\"{stored}\": {error}"))?;
            read(&mut entry)
        }
        Side::Folder(root) => {
            let path = folder_file(root, stored)?;
            let mut file =
                File::open(&path).map_err(|error| format!("Can't read \"{stored}\": {error}"))?;
            read(&mut file)
        }
    }
}

fn read_stored(side: &Side, stored: &str, limit: u64) -> Result<Vec<u8>, String> {
    with_reader(side, stored, |reader| {
        let mut bytes = Vec::new();
        reader
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Can't read the file: {error}"))?;
        Ok(bytes)
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Size and SHA-256 of the file `stored`, read to its end.
fn side_info(side: &Side, stored: &str, size: Option<u64>) -> Result<SideInfo, String> {
    use sha2::{Digest, Sha256};
    if size.is_some_and(|size| size > MAX_HASH_BYTES) {
        return Ok(SideInfo {
            size: size.unwrap_or(0),
            sha256: None,
        });
    }
    with_reader(side, stored, |reader| {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut total = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("Can't read the file: {error}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            total += read as u64;
        }
        Ok(SideInfo {
            size: total,
            sha256: Some(hex(&hasher.finalize())),
        })
    })
}

fn info_of(bytes: &[u8]) -> SideInfo {
    use sha2::{Digest, Sha256};
    SideInfo {
        size: bytes.len() as u64,
        sha256: Some(hex(&Sha256::digest(bytes))),
    }
}

/// The text of a file, or `None` for binary content. UTF-8 (with or
/// without a byte order mark) and UTF-16 with one; anything else that has no
/// NUL byte is read as UTF-8, replacing what is not.
pub(crate) fn decode_text(bytes: &[u8]) -> Option<(String, Option<&'static str>)> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some((String::from_utf8_lossy(rest).into_owned(), None));
    }
    let utf16 = |rest: &[u8], little: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Some((utf16(rest, true), Some("UTF-16 LE")));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Some((utf16(rest, false), Some("UTF-16 BE")));
    }
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return None;
    }
    Some((String::from_utf8_lossy(bytes).into_owned(), None))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// What changed in one file of two backups (either name may be missing: an
/// added or deleted file), read from the zips or folders as far as the
/// limits allow: a line diff for text, both pictures for images, size and
/// SHA-256 for anything else.
pub(crate) fn file_diff(
    old: &Side,
    old_name: Option<&str>,
    new: &Side,
    new_name: Option<&str>,
) -> Result<FileDiff, String> {
    let path = new_name.or(old_name).unwrap_or_default();
    if let Some(mime) = image_type(path) {
        let read = |side: &Side, name: Option<&str>| {
            name.map(|name| read_stored(side, name, MAX_IMAGE_BYTES))
                .transpose()
        };
        let (old_bytes, new_bytes) = (read(old, old_name)?, read(new, new_name)?);
        let fits = |bytes: &Option<Vec<u8>>| {
            bytes
                .as_ref()
                .is_none_or(|bytes| bytes.len() as u64 <= MAX_IMAGE_BYTES)
        };
        if fits(&old_bytes) && fits(&new_bytes) {
            let rows = (mime == "image/svg+xml")
                .then(|| {
                    let text = |bytes: &Option<Vec<u8>>| {
                        bytes
                            .as_deref()
                            .and_then(decode_text)
                            .map(|(text, _)| text)
                            .unwrap_or_default()
                    };
                    match text_diff(&text(&old_bytes), &text(&new_bytes)) {
                        FileDiff::Text { rows, .. } => Some(rows),
                        _ => None,
                    }
                })
                .flatten();
            return Ok(FileDiff::Image {
                mime: mime.to_string(),
                old: old_bytes.as_deref().map(|bytes| data_url(mime, bytes)),
                new: new_bytes.as_deref().map(|bytes| data_url(mime, bytes)),
                old_info: old_bytes.as_deref().map(info_of),
                new_info: new_bytes.as_deref().map(info_of),
                rows,
            });
        }
        return binary(old, old_name, new, new_name, false);
    }
    let read = |side: &Side, name: Option<&str>| {
        name.map(|name| read_stored(side, name, MAX_DIFF_BYTES))
            .transpose()
    };
    let (old_bytes, new_bytes) = (read(old, old_name)?, read(new, new_name)?);
    let too_large = [&old_bytes, &new_bytes].iter().any(|bytes| {
        bytes
            .as_ref()
            .is_some_and(|bytes| bytes.len() as u64 > MAX_DIFF_BYTES)
    });
    if too_large {
        return binary(old, old_name, new, new_name, true);
    }
    let decode = |bytes: &Option<Vec<u8>>| match bytes {
        None => Some(None),
        Some(bytes) => decode_text(bytes).map(Some),
    };
    let (Some(old_text), Some(new_text)) = (decode(&old_bytes), decode(&new_bytes)) else {
        return Ok(FileDiff::Binary {
            old: old_bytes.as_deref().map(info_of),
            new: new_bytes.as_deref().map(info_of),
            too_large: false,
        });
    };
    let encoding = [&old_text, &new_text]
        .iter()
        .find_map(|side| side.as_ref().and_then(|(_, encoding)| *encoding))
        .map(str::to_string);
    let old_text = old_text.map(|(text, _)| text).unwrap_or_default();
    let new_text = new_text.map(|(text, _)| text).unwrap_or_default();
    let mut diff = text_diff(&old_text, &new_text);
    if let FileDiff::Text { encoding: slot, .. } = &mut diff {
        *slot = encoding;
    }
    Ok(diff)
}

/// The same preview as `file_diff`, for two versions already in memory
/// (GitHub Releases: the committed file and the one in the folder). A side
/// is `None` when the file is missing there; `Err(size)` when it was too
/// large to read.
pub(crate) fn bytes_diff(
    path: &str,
    old: Option<Result<Vec<u8>, u64>>,
    new: Option<Result<Vec<u8>, u64>>,
) -> FileDiff {
    let too_large = |side: &Option<Result<Vec<u8>, u64>>| matches!(side, Some(Err(_)));
    let info = |side: &Option<Result<Vec<u8>, u64>>| match side {
        None => None,
        Some(Ok(bytes)) => Some(info_of(bytes)),
        Some(Err(size)) => Some(SideInfo { size: *size, sha256: None }),
    };
    if too_large(&old) || too_large(&new) {
        return FileDiff::Binary { old: info(&old), new: info(&new), too_large: true };
    }
    let old = old.map(|side| side.unwrap_or_default());
    let new = new.map(|side| side.unwrap_or_default());
    if let Some(mime) = image_type(path) {
        let fits = |bytes: &Option<Vec<u8>>| bytes.as_ref().is_none_or(|b| b.len() as u64 <= MAX_IMAGE_BYTES);
        if fits(&old) && fits(&new) {
            let rows = (mime == "image/svg+xml").then(|| {
                let text = |bytes: &Option<Vec<u8>>| bytes.as_deref().and_then(decode_text).map(|(t, _)| t).unwrap_or_default();
                match text_diff(&text(&old), &text(&new)) {
                    FileDiff::Text { rows, .. } => Some(rows),
                    _ => None,
                }
            }).flatten();
            return FileDiff::Image {
                mime: mime.to_string(),
                old: old.as_deref().map(|b| data_url(mime, b)),
                new: new.as_deref().map(|b| data_url(mime, b)),
                old_info: old.as_deref().map(info_of),
                new_info: new.as_deref().map(info_of),
                rows,
            };
        }
    }
    let decode = |bytes: &Option<Vec<u8>>| match bytes {
        None => Some(None),
        Some(bytes) => decode_text(bytes).map(Some),
    };
    let (Some(old_text), Some(new_text)) = (decode(&old), decode(&new)) else {
        return FileDiff::Binary {
            old: old.as_deref().map(info_of),
            new: new.as_deref().map(info_of),
            too_large: false,
        };
    };
    let encoding = [&old_text, &new_text]
        .iter()
        .find_map(|side| side.as_ref().and_then(|(_, encoding)| *encoding))
        .map(str::to_string);
    let mut diff = text_diff(
        &old_text.map(|(t, _)| t).unwrap_or_default(),
        &new_text.map(|(t, _)| t).unwrap_or_default(),
    );
    if let FileDiff::Text { encoding: slot, .. } = &mut diff {
        *slot = encoding;
    }
    diff
}

/// The most of a file `bytes_diff` needs: larger files are compared by
/// size and SHA-256 only.
pub(crate) fn preview_limit(path: &str) -> u64 {
    if image_type(path).is_some() { MAX_IMAGE_BYTES } else { MAX_DIFF_BYTES }
}

fn binary(
    old: &Side,
    old_name: Option<&str>,
    new: &Side,
    new_name: Option<&str>,
    too_large: bool,
) -> Result<FileDiff, String> {
    Ok(FileDiff::Binary {
        old: old_name
            .map(|name| side_info(old, name, None))
            .transpose()?,
        new: new_name
            .map(|name| side_info(new, name, None))
            .transpose()?,
        too_large,
    })
}

fn line_text(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

/// A side-by-side line diff of two texts, with three lines of context
/// around each change. Line endings alone don't make lines differ.
pub(crate) fn text_diff(old: &str, new: &str) -> FileDiff {
    let old_lf = old.replace("\r\n", "\n");
    let new_lf = new.replace("\r\n", "\n");
    let line_endings_differ = old != new && old_lf == new_lf;
    let diff = TextDiff::from_lines(&old_lf, &new_lf);
    let old_lines: Vec<&str> = diff.old_slices().to_vec();
    let new_lines: Vec<&str> = diff.new_slices().to_vec();
    let groups = diff.grouped_ops(3);
    let identical = groups.is_empty();
    let mut rows = Vec::new();
    let gap = || Row {
        kind: RowKind::Gap,
        old_line: None,
        old_text: None,
        new_line: None,
        new_text: None,
    };
    let mut truncated = false;
    'groups: for (index, group) in groups.iter().enumerate() {
        if index > 0 || group.first().is_some_and(|op| op.old_range().start > 0) {
            rows.push(gap());
        }
        for op in group {
            if rows.len() >= MAX_ROWS {
                truncated = true;
                break 'groups;
            }
            let (tag, olds, news) = op.as_tag_tuple();
            match tag {
                DiffTag::Equal => {
                    for (o, n) in olds.zip(news) {
                        rows.push(Row {
                            kind: RowKind::Same,
                            old_line: Some(o + 1),
                            old_text: Some(line_text(old_lines[o])),
                            new_line: Some(n + 1),
                            new_text: Some(line_text(new_lines[n])),
                        });
                    }
                }
                DiffTag::Delete => {
                    for o in olds {
                        rows.push(Row {
                            kind: RowKind::Removed,
                            old_line: Some(o + 1),
                            old_text: Some(line_text(old_lines[o])),
                            new_line: None,
                            new_text: None,
                        });
                    }
                }
                DiffTag::Insert => {
                    for n in news {
                        rows.push(Row {
                            kind: RowKind::Added,
                            old_line: None,
                            old_text: None,
                            new_line: Some(n + 1),
                            new_text: Some(line_text(new_lines[n])),
                        });
                    }
                }
                DiffTag::Replace => {
                    let olds: Vec<usize> = olds.collect();
                    let news: Vec<usize> = news.collect();
                    for i in 0..olds.len().max(news.len()) {
                        let o = olds.get(i).copied();
                        let n = news.get(i).copied();
                        let kind = match (o, n) {
                            (Some(_), Some(_)) => RowKind::Changed,
                            (Some(_), None) => RowKind::Removed,
                            _ => RowKind::Added,
                        };
                        rows.push(Row {
                            kind,
                            old_line: o.map(|o| o + 1),
                            old_text: o.map(|o| line_text(old_lines[o])),
                            new_line: n.map(|n| n + 1),
                            new_text: n.map(|n| line_text(new_lines[n])),
                        });
                    }
                }
            }
        }
    }
    if rows.len() > MAX_ROWS {
        rows.truncate(MAX_ROWS);
        truncated = true;
    }
    if !truncated
        && let Some(last) = groups.last().and_then(|group| group.last())
        && last.old_range().end < old_lines.len()
    {
        rows.push(gap());
    }
    FileDiff::Text {
        rows,
        identical,
        line_endings_differ,
        truncated,
        old_lines: old_lines.len(),
        new_lines: new_lines.len(),
        encoding: None,
    }
}

/// Whether `path` is a folder (an older folder backup) or a file.
pub(crate) fn side_for(path: &Path) -> Result<Side, String> {
    let meta = fs::metadata(path)
        .map_err(|error| format!("Can't open \"{}\": {error}", path.display()))?;
    Ok(if meta.is_dir() {
        Side::Folder(path.to_path_buf())
    } else {
        Side::Zip(path.to_path_buf())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::rules::{DEFAULT_PATTERNS, RuleInput};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-compare-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    pub(crate) fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, body) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(body).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    fn defaults() -> Rules {
        Rules::compile(
            &RuleInput {
                patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
                ..RuleInput::default()
            },
            None,
        )
        .unwrap()
    }

    #[test]
    fn names_are_normalized() {
        assert_eq!(normalize_name("src\\lib\\./a.rs"), "src/lib/a.rs");
        assert_eq!(normalize_name("/a//b/"), "a/b");
        // "ά" written as α + combining accent equals the composed letter.
        assert_eq!(normalize_name("\u{3b1}\u{301}.txt"), "\u{3ac}.txt");
    }

    #[test]
    fn a_wrapper_folder_is_ignored_when_it_pairs_more_files() {
        let mut old = BTreeMap::new();
        for key in ["steam-idler/package.json", "steam-idler/src/main.js"] {
            old.insert(key.to_string(), ());
        }
        let mut new = BTreeMap::new();
        for key in ["package.json", "SRC/main.js", "README.md"] {
            new.insert(key.to_string(), ());
        }
        let pairs = match_files(&old, &new);
        assert_eq!(pairs.len(), 3);
        assert_eq!(
            pairs["package.json"].old.as_deref(),
            Some("steam-idler/package.json")
        );
        assert_eq!(
            pairs["SRC/main.js"].old.as_deref(),
            Some("steam-idler/src/main.js")
        );
        assert!(pairs["README.md"].old.is_none());

        // A project with one real top folder keeps it.
        let mut one = BTreeMap::new();
        one.insert("src/a.rs".to_string(), ());
        let pairs = match_files(&one, &one.clone());
        assert!(pairs.contains_key("src/a.rs"));
    }

    #[test]
    fn zips_and_folders_compare_alike() {
        let root = temp("zips");
        let old = root.join("old.zip");
        make_zip(
            &old,
            &[
                ("App\\", b""),
                ("App\\package.json", b"{\"v\":1}"),
                ("App\\src\\gone.js", b"x"),
                ("App\\src\\same.js", b"same"),
                ("App\\node_modules\\dep.js", b"dep"),
                ("App\\.backup-info.json", b"{}"),
            ],
        );
        let new = root.join("new.zip");
        make_zip(
            &new,
            &[
                ("package.json", b"{\"v\":2}"),
                ("src/same.js", b"same"),
                ("src/new.js", b"n"),
                (".backup-info.json", b"{\"x\":1}"),
            ],
        );
        let source = root.join("source");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::write(source.join("src").join("gone.js"), "x").unwrap();
        let result = compare(
            &Side::Zip(old.clone()),
            &Side::Zip(new.clone()),
            &defaults(),
            Some(&source),
            &|| false,
            &|_, _| {},
        )
        .unwrap();
        let summary: Vec<(&str, Status, bool)> = result
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.status, c.still_in_source))
            .collect();
        assert_eq!(
            summary,
            [
                ("src/gone.js", Status::Deleted, true),
                ("package.json", Status::Modified, false),
                ("src/new.js", Status::Added, false),
            ]
        );
        assert_eq!(
            (
                result.added,
                result.modified,
                result.deleted,
                result.unchanged
            ),
            (1, 1, 1, 1)
        );

        // The same files as a folder: nothing changed.
        let folder = root.join("folder");
        fs::create_dir_all(folder.join("src")).unwrap();
        fs::write(folder.join("package.json"), "{\"v\":2}").unwrap();
        fs::write(folder.join("src").join("same.js"), "same").unwrap();
        fs::write(folder.join("src").join("new.js"), "n").unwrap();
        let result = compare(
            &Side::Zip(new),
            &Side::Folder(folder.clone()),
            &defaults(),
            None,
            &|| false,
            &|_, _| {},
        )
        .unwrap();
        assert!(result.changes.is_empty(), "{:?}", result.changes);
        assert_eq!(result.unchanged, 3);
        // Same size, different bytes: found by CRC.
        fs::write(folder.join("src").join("same.js"), "SAME").unwrap();
        let result = compare(
            &Side::Folder(folder.clone()),
            &Side::Zip(root.join("new.zip")),
            &defaults(),
            None,
            &|| false,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(result.modified, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn text_diffs_are_side_by_side_and_binaries_are_not() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old
            .replace("line 10\n", "line ten\n")
            .replace("line 20\n", "");
        let FileDiff::Text {
            rows, identical, ..
        } = text_diff(&old, &new)
        else {
            panic!()
        };
        assert!(!identical);
        let changed: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == RowKind::Changed)
            .collect();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].old_text.as_deref(), Some("line 10"));
        assert_eq!(changed[0].new_text.as_deref(), Some("line ten"));
        assert!(
            rows.iter()
                .any(|row| row.kind == RowKind::Removed && row.old_line == Some(20))
        );
        assert_eq!(rows[0].kind, RowKind::Gap);
        let FileDiff::Text {
            identical, rows, ..
        } = text_diff("a\n", "a\n")
        else {
            panic!()
        };
        assert!(identical && rows.is_empty());

        let root = temp("binary");
        let zip = root.join("b.zip");
        make_zip(
            &zip,
            &[("blob.dat", b"\x89PNG\0\0data"), ("a.txt", b"one\n")],
        );
        let folder = root.join("f");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.txt"), "two\n").unwrap();
        let diff = file_diff(
            &Side::Zip(zip.clone()),
            Some("blob.dat"),
            &Side::Folder(folder.clone()),
            None,
        )
        .unwrap();
        let FileDiff::Binary {
            old: Some(old),
            new: None,
            too_large: false,
        } = diff
        else {
            panic!("binary: {diff:?}")
        };
        assert_eq!(old.size, 10);
        assert_eq!(old.sha256.as_deref().map(str::len), Some(64));
        let diff = file_diff(
            &Side::Zip(zip),
            Some("a.txt"),
            &Side::Folder(folder.clone()),
            Some("a.txt"),
        )
        .unwrap();
        assert!(matches!(
            diff,
            FileDiff::Text {
                identical: false,
                ..
            }
        ));
        assert!(
            file_diff(
                &Side::Folder(folder.clone()),
                Some("../x"),
                &Side::Folder(folder),
                None
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn images_are_shown_and_texts_are_decoded() {
        let root = temp("images");
        let zip = root.join("a.zip");
        let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        make_zip(
            &zip,
            &[
                ("icons/app.png", png),
                ("logo.svg", b"<svg>\n<g/>\n</svg>\n"),
            ],
        );
        let folder = root.join("f");
        fs::create_dir_all(folder.join("icons")).unwrap();
        fs::write(
            folder.join("icons").join("app.png"),
            b"\x89PNG\r\n\x1a\nnew",
        )
        .unwrap();
        fs::write(folder.join("logo.svg"), "<svg>\n<g id=\"x\"/>\n</svg>\n").unwrap();
        let diff = file_diff(
            &Side::Zip(zip.clone()),
            Some("icons/app.png"),
            &Side::Folder(folder.clone()),
            Some("icons/app.png"),
        )
        .unwrap();
        let FileDiff::Image {
            mime,
            old: Some(old),
            new: Some(new),
            rows: None,
            ..
        } = diff
        else {
            panic!("an image")
        };
        assert_eq!(mime, "image/png");
        assert!(old.starts_with("data:image/png;base64,") && old != new);
        // SVG: both pictures and the source.
        let diff = file_diff(
            &Side::Zip(zip),
            Some("logo.svg"),
            &Side::Folder(folder),
            Some("logo.svg"),
        )
        .unwrap();
        assert!(
            matches!(diff, FileDiff::Image { rows: Some(ref rows), .. } if rows.iter().any(|row| row.kind == RowKind::Changed))
        );
        assert_eq!(FileKind::of("a/B.PNG"), FileKind::Image);
        assert_eq!(FileKind::of("app.exe"), FileKind::Binary);
        assert_eq!(FileKind::of("src/main.rs"), FileKind::Text);

        // UTF-16 with a byte order mark (.reg, some .rc files) is text.
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Windows\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            decode_text(&utf16),
            Some(("Windows\r\n".to_string(), Some("UTF-16 LE")))
        );
        assert_eq!(
            decode_text(b"\xEF\xBB\xBFhi").map(|(t, _)| t).as_deref(),
            Some("hi")
        );
        assert_eq!(decode_text(b"a\0b"), None);
        // CRLF against LF: the same text.
        let FileDiff::Text {
            identical,
            line_endings_differ,
            ..
        } = text_diff("a\r\nb\r\n", "a\nb\n")
        else {
            panic!()
        };
        assert!(identical && line_endings_differ);
        // An added file is shown whole; a huge one is cut.
        let FileDiff::Text {
            rows, new_lines, ..
        } = text_diff("", "x\ny\n")
        else {
            panic!()
        };
        assert_eq!((rows.len(), new_lines), (2, 2));
        let long: String = (0..MAX_ROWS + 50).map(|n| format!("{n}\n")).collect();
        let FileDiff::Text {
            rows, truncated, ..
        } = text_diff("", &long)
        else {
            panic!()
        };
        assert!(truncated && rows.len() == MAX_ROWS);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exclusions_in_a_comparison_follow_the_markers_in_the_backup() {
        // A Rust project: target/ beside Cargo.toml is output on both sides,
        // a game's "target" sprites folder is not.
        let root = temp("markers");
        let old = root.join("old.zip");
        make_zip(
            &old,
            &[
                ("Cargo.toml", b"[package]"),
                ("src/main.rs", b"fn main() {}"),
            ],
        );
        let new = root.join("new.zip");
        make_zip(
            &new,
            &[
                ("Cargo.toml", b"[package]"),
                ("src/main.rs", b"fn main() {}"),
                ("target/debug/app.exe", b"MZ"),
                ("game/target/sprite.txt", b"s"),
            ],
        );
        let rules = Rules::compile(
            &RuleInput {
                patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
                smart_build: true,
                ..RuleInput::default()
            },
            None,
        )
        .unwrap();
        let result = compare(
            &Side::Zip(old),
            &Side::Zip(new),
            &rules,
            None,
            &|| false,
            &|_, _| {},
        )
        .unwrap();
        let added: Vec<&str> = result.changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(added, ["game/target/sprite.txt"]);
        let _ = fs::remove_dir_all(root);
    }
}
