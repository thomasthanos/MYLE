//! System Cleaner's "Downloads folder": old downloads that are not documents.
//!
//! The folder is Windows' own Downloads (the known folder, wherever the user
//! moved it). Kept: documents by their extension, and anything changed or
//! modified in the last seven days (Date modified, as Explorer shows it; not
//! created or accessed, which copying, unpacking and scanning reset). A
//! folder is judged by its own date, like in Explorer. Hidden and system
//! files (desktop.ini) are left out. Links and junctions are never followed
//! or removed. Deleting is
//! permanent, and only what the preview listed and still qualifies goes:
//! every path is checked again just before it is removed.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

/// Kept however old: documents people download to keep.
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "docm", "dot", "dotx", "xls", "xlsx", "xlsm", "xlsb", "ppt", "pptx", "pptm", "pps",
    "ppsx", "odt", "ods", "odp", "odg", "txt", "rtf", "csv", "tsv", "md", "markdown", "epub", "mobi", "azw3",
    "djvu", "xps", "oxps", "pages", "numbers", "key", "tex", "one", "vsdx",
    "pub", "wpd", "wps", "ott", "ots", "otp", "fb2", "cbz", "cbr",
];

/// Anything this new stays.
const RECENT: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A folder deeper than this is kept rather than walked.
const MAX_DEPTH: usize = 24;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    path: String,
    name: String,
    is_dir: bool,
    size: u64,
    /// Date modified, seconds since 1970.
    changed: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Kept {
    name: String,
    is_dir: bool,
    /// "document", "recent" or "link".
    reason: &'static str,
    changed: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    folder: String,
    items: Vec<Item>,
    total: u64,
    /// Entries left alone, and why.
    kept: Vec<Kept>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    deleted: usize,
    freed: u64,
    /// In use, or no longer what the preview showed: left in place.
    skipped: Vec<String>,
}

/// Windows' Downloads folder (FOLDERID_Downloads).
#[cfg(windows)]
fn downloads_folder() -> Result<PathBuf, String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None)
            .map_err(|e| format!("Windows did not say where Downloads is: {e}"))?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0 as *const _));
        let text = text.map_err(|e| e.to_string())?;
        Ok(PathBuf::from(text))
    }
}

#[cfg(not(windows))]
fn downloads_folder() -> Result<PathBuf, String> {
    Err("Only on Windows.".into())
}

fn is_document(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| DOCUMENTS.iter().any(|d| d.eq_ignore_ascii_case(e)))
}

fn changed(meta: &std::fs::Metadata) -> SystemTime {
    meta.modified().unwrap_or(SystemTime::now())
}

/// desktop.ini and the like: never listed, never touched.
fn is_hidden(meta: &std::fs::Metadata, name: &str) -> bool {
    if name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
        meta.file_attributes() & HIDDEN_OR_SYSTEM != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        name.starts_with('.')
    }
}

fn recent(meta: &std::fs::Metadata, now: SystemTime) -> bool {
    now.duration_since(changed(meta)).map_or(true, |age| age < RECENT)
}

/// A link, junction or other reparse point: never followed, never removed.
fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Why an entry directly in Downloads stays, or `None` when it may go.
fn keep_reason(path: &Path, meta: &std::fs::Metadata, now: SystemTime) -> Option<&'static str> {
    if is_link(meta) || !(meta.is_file() || meta.is_dir()) {
        Some("link")
    } else if recent(meta, now) {
        Some("recent")
    } else if meta.is_file() && is_document(path) {
        Some("document")
    } else {
        None
    }
}

/// Bytes in a file or folder; links inside are not followed.
fn size_of(path: &Path, meta: &std::fs::Metadata, depth: usize) -> u64 {
    if meta.is_file() {
        return meta.len();
    }
    if is_link(meta) || !meta.is_dir() || depth > MAX_DEPTH {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    std::fs::symlink_metadata(&path).ok().map(|meta| size_of(&path, &meta, depth + 1))
                })
                .sum()
        })
        .unwrap_or(0)
}

/// The size of an entry of Downloads if it may go now.
fn removable(path: &Path, now: SystemTime) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let name = path.file_name()?.to_string_lossy();
    if is_hidden(&meta, &name) || keep_reason(path, &meta, now).is_some() {
        return None;
    }
    Some(size_of(path, &meta, 0))
}

fn epoch(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn preview_in(folder: &Path, now: SystemTime) -> Result<Preview, String> {
    let mut items = Vec::new();
    let mut kept = Vec::new();
    for entry in std::fs::read_dir(folder).map_err(|e| format!("Could not read {}: {e}", folder.display()))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden(&meta, &name) {
            continue;
        }
        let changed = epoch(changed(&meta));
        match keep_reason(&path, &meta, now) {
            Some(reason) => kept.push(Kept { name, is_dir: meta.is_dir(), reason, changed }),
            None => items.push(Item {
                size: size_of(&path, &meta, 0),
                path: path.to_string_lossy().into_owned(),
                name,
                is_dir: meta.is_dir(),
                changed,
            }),
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.size));
    kept.sort_by_key(|item| std::cmp::Reverse(item.changed));
    let total = items.iter().map(|item| item.size).sum();
    Ok(Preview { folder: folder.to_string_lossy().into_owned(), items, total, kept })
}

fn delete_in(folder: &Path, paths: &[String], now: SystemTime) -> Outcome {
    let mut outcome = Outcome { deleted: 0, freed: 0, skipped: Vec::new() };
    for text in paths {
        let path = PathBuf::from(text);
        // Only entries directly in Downloads, as listed, and still removable.
        let direct = path.parent().is_some_and(|parent| parent == folder) && path.file_name().is_some();
        let size = direct.then(|| removable(&path, now)).flatten();
        let Some(size) = size else {
            outcome.skipped.push(text.clone());
            continue;
        };
        let done = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match done {
            Ok(()) => {
                outcome.deleted += 1;
                outcome.freed += size;
            }
            // In use (or partly removed): left as it is now.
            Err(_) => outcome.skipped.push(text.clone()),
        }
    }
    outcome
}

/// What would go, for the preview.
#[tauri::command]
pub async fn cleaner_downloads_preview() -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(|| preview_in(&downloads_folder()?, SystemTime::now()))
        .await
        .map_err(|e| e.to_string())?
}

/// Deletes, permanently, the listed entries that still qualify.
#[tauri::command]
pub async fn cleaner_downloads_delete(paths: Vec<String>) -> Result<Outcome, String> {
    if paths.len() > 20_000 {
        return Err("Too many items at once.".into());
    }
    tauri::async_runtime::spawn_blocking(move || Ok(delete_in(&downloads_folder()?, &paths, SystemTime::now())))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_new_files_and_folders_with_either_stay() {
        let root = std::env::temp_dir().join(format!("myle-downloads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("setup-folder")).unwrap();
        std::fs::create_dir_all(root.join("mixed")).unwrap();
        std::fs::write(root.join("installer.exe"), b"12345").unwrap();
        std::fs::write(root.join("report.PDF"), b"1").unwrap();
        std::fs::write(root.join("setup-folder").join("a.bin"), b"123").unwrap();
        std::fs::write(root.join("mixed").join("notes.docx"), b"1").unwrap();
        std::fs::write(root.join("desktop.ini"), b"[x]").unwrap();
        // "Now" a month ahead: everything counts as old.
        let later = SystemTime::now() + Duration::from_secs(30 * 24 * 3600);
        let preview = preview_in(&root, later).unwrap();
        let mut names: Vec<_> = preview.items.iter().map(|i| i.name.as_str()).collect();
        names.sort();
        // A folder goes by its own date, whatever is inside.
        assert_eq!(names, ["installer.exe", "mixed", "setup-folder"]);
        assert_eq!(preview.kept.len(), 1);
        assert_eq!(preview.kept[0].reason, "document");
        assert_eq!(preview.total, 9);
        // Today: all of it is new, so nothing goes.
        let today = preview_in(&root, SystemTime::now()).unwrap();
        assert!(today.items.is_empty());
        assert!(today.kept.iter().all(|k| k.reason == "recent" && k.name != "desktop.ini"));
        // Deleting checks again: a document, or a path outside, is skipped.
        let outside = std::env::temp_dir().join("elsewhere.exe").to_string_lossy().into_owned();
        let asked = vec![
            root.join("installer.exe").to_string_lossy().into_owned(),
            root.join("report.PDF").to_string_lossy().into_owned(),
            root.join("setup-folder").join("a.bin").to_string_lossy().into_owned(),
            outside,
        ];
        let outcome = delete_in(&root, &asked, later);
        assert_eq!(outcome.deleted, 1);
        assert_eq!(outcome.skipped.len(), 3);
        assert!(!root.join("installer.exe").exists());
        assert!(root.join("report.PDF").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
