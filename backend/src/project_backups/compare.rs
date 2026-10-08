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

use super::rules::{BACKUP_INFO_FILE, Rules};
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

fn strip<'a>(key: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        key
    } else {
        &key[prefix.len() + 1..]
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

fn read_stored(side: &Side, stored: &str, limit: Option<u64>) -> Result<Vec<u8>, String> {
    match side {
        Side::Zip(path) => {
            let file = File::open(path).map_err(|error| format!("Can't open the zip: {error}"))?;
            let mut archive = ZipArchive::new(BufReader::new(file))
                .map_err(|error| format!("Can't read the zip: {error}"))?;
            let entry = archive
                .by_name(stored)
                .map_err(|error| format!("\"{stored}\": {error}"))?;
            read_limited(entry, limit)
        }
        Side::Folder(root) => {
            let path = folder_file(root, stored)?;
            let file =
                File::open(&path).map_err(|error| format!("Can't read \"{stored}\": {error}"))?;
            read_limited(file, limit)
        }
    }
}

fn read_limited(reader: impl Read, limit: Option<u64>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    match limit {
        Some(limit) => reader.take(limit + 1).read_to_end(&mut bytes),
        None => {
            let mut reader = reader;
            reader.read_to_end(&mut bytes)
        }
    }
    .map_err(|error| format!("Can't read the file: {error}"))?;
    Ok(bytes)
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

fn folder_crc(side: &Side, info: &FileInfo) -> Result<u32, String> {
    let Side::Folder(root) = side else {
        return info.crc.ok_or_else(|| "Missing CRC".to_string());
    };
    let mut file = File::open(folder_file(root, &info.stored)?)
        .map_err(|error| format!("Can't read \"{}\": {error}", info.stored))?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
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
/// are still there.
pub(crate) fn compare(
    old: &Side,
    new: &Side,
    rules: &Rules,
    source: Option<&Path>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Comparison, String> {
    let old_files = list_files(old, cancelled)?;
    let new_files = list_files(new, cancelled)?;
    let pairs = match_files(&old_files, &new_files);
    let mut result = Comparison::default();
    for (path, pair) in pairs {
        if cancelled() {
            return Err(super::CANCELLED.into());
        }
        if path == BACKUP_INFO_FILE || rules.excluded_path(&path) {
            continue;
        }
        let old_info = pair.old.as_ref().and_then(|key| old_files.get(key));
        let new_info = pair.new.as_ref().and_then(|key| new_files.get(key));
        let status = match (old_info, new_info) {
            (Some(_), None) => Status::Deleted,
            (None, Some(_)) => Status::Added,
            (Some(a), Some(b)) => {
                if a.size != b.size || folder_crc(old, a)? != folder_crc(new, b)? {
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
            path,
            status,
            old_name: old_info.map(|info| info.stored.clone()),
            new_name: new_info.map(|info| info.stored.clone()),
            old_size: old_info.map(|info| info.size),
            new_size: new_info.map(|info| info.size),
            still_in_source,
        });
    }
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FileDiff {
    Text {
        rows: Vec<Row>,
        identical: bool,
    },
    Binary {
        old_size: Option<u64>,
        new_size: Option<u64>,
    },
    TooLarge {
        old_size: Option<u64>,
        new_size: Option<u64>,
    },
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

fn line_text(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

/// A side-by-side line diff of one file of two backups (either name may be
/// missing: an added or deleted file).
pub(crate) fn file_diff(
    old: &Side,
    old_name: Option<&str>,
    new: &Side,
    new_name: Option<&str>,
) -> Result<FileDiff, String> {
    let old_bytes = old_name
        .map(|name| read_stored(old, name, Some(MAX_DIFF_BYTES)))
        .transpose()?;
    let new_bytes = new_name
        .map(|name| read_stored(new, name, Some(MAX_DIFF_BYTES)))
        .transpose()?;
    let size = |bytes: &Option<Vec<u8>>| bytes.as_ref().map(|bytes| bytes.len() as u64);
    if [&old_bytes, &new_bytes].iter().any(|bytes| {
        bytes
            .as_ref()
            .is_some_and(|b| b.len() as u64 > MAX_DIFF_BYTES)
    }) {
        return Ok(FileDiff::TooLarge {
            old_size: size(&old_bytes),
            new_size: size(&new_bytes),
        });
    }
    if [&old_bytes, &new_bytes]
        .iter()
        .any(|bytes| bytes.as_ref().is_some_and(|b| is_binary(b)))
    {
        return Ok(FileDiff::Binary {
            old_size: size(&old_bytes),
            new_size: size(&new_bytes),
        });
    }
    let old_text = old_bytes
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let new_text = new_bytes
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    Ok(text_diff(&old_text, &new_text))
}

pub(crate) fn text_diff(old: &str, new: &str) -> FileDiff {
    let diff = TextDiff::from_lines(old, new);
    let old_lines: Vec<&str> = diff.old_slices().to_vec();
    let new_lines: Vec<&str> = diff.new_slices().to_vec();
    let groups = diff.grouped_ops(3);
    let identical = groups.is_empty();
    let mut rows = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if index > 0 || group.first().is_some_and(|op| op.old_range().start > 0) {
            rows.push(Row {
                kind: RowKind::Gap,
                old_line: None,
                old_text: None,
                new_line: None,
                new_text: None,
            });
        }
        for op in group {
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
    if let Some(last) = groups.last().and_then(|group| group.last())
        && last.old_range().end < old_lines.len()
    {
        rows.push(Row {
            kind: RowKind::Gap,
            old_line: None,
            old_text: None,
            new_line: None,
            new_text: None,
        });
    }
    FileDiff::Text { rows, identical }
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
        let FileDiff::Text { rows, identical } = text_diff(&old, &new) else {
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
        let FileDiff::Text { identical, rows } = text_diff("a\n", "a\n") else {
            panic!()
        };
        assert!(identical && rows.is_empty());

        let root = temp("binary");
        let zip = root.join("b.zip");
        make_zip(
            &zip,
            &[("img.png", b"\x89PNG\0\0data"), ("a.txt", b"one\n")],
        );
        let folder = root.join("f");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.txt"), "two\n").unwrap();
        let diff = file_diff(
            &Side::Zip(zip.clone()),
            Some("img.png"),
            &Side::Folder(folder.clone()),
            None,
        )
        .unwrap();
        assert!(matches!(
            diff,
            FileDiff::Binary {
                old_size: Some(10),
                new_size: None
            }
        ));
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
}
