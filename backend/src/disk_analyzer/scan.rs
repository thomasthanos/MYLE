//! The walker: lists folders without opening a single file.
//!
//! Each folder is opened once and read in large batches with
//! `GetFileInformationByHandleEx` (FileIdExtdDirectoryInfo, or the older
//! classes where a file system refuses it): one call returns many entries,
//! each with its size, size on disk, attributes and reparse tag. A few
//! worker threads share a stack of folders to read and hand each finished
//! listing to this thread, the only one that touches the tree.
//!
//! Rules: junctions, symbolic links and mount points are listed as links and
//! never followed. Cloud placeholders are counted as files and never opened,
//! which would start a download. A folder that cannot be read is counted and
//! the walk carries on. Hard links are not known without opening files, so
//! a hard-linked file (WinSxS and the like) counts once per link.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_FUNCTION, ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
    FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FULL_DIR_INFO, FILE_ID_BOTH_DIR_INFO, FILE_ID_EXTD_DIR_INFO, FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileFullDirectoryInfo, FileIdBothDirectoryInfo,
    FileIdExtdDirectoryInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
};

use super::tree::{Entry, Kind, Listing, Tree, display_path, unix_seconds};

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;
/// IO_REPARSE_TAG_CLOUD and its variants CLOUD_1 … CLOUD_F (0x9000x01A).
const CLOUD_TAG: u32 = 0x9000_001A;
const CLOUD_TAG_MASK: u32 = 0xFFFF_0FFF;
const RECALL: u32 = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_OFFLINE;

/// One listing call fills at most this much (256 KB allows large folders to be read in fewer syscalls).
const BUFFER_BYTES: usize = 256 * 1024;
/// At most this many progress events a second.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub alloc: u64,
    /// The folder just read, as Windows shows it.
    pub current: String,
}

/// Lets a call through at most once every `every`, by an injectable clock.
pub struct Throttle<C: FnMut() -> Instant> {
    every: Duration,
    last: Option<Instant>,
    clock: C,
}

impl<C: FnMut() -> Instant> Throttle<C> {
    pub fn new(every: Duration, clock: C) -> Self {
        Self { every, last: None, clock }
    }

    pub fn ready(&mut self) -> bool {
        let now = (self.clock)();
        match self.last {
            Some(at) if now.duration_since(at) < self.every => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// A folder to read: its node and its `\\?\` path (no terminating NUL).
struct Job {
    id: u32,
    path: Vec<u16>,
}

struct Read {
    id: u32,
    path: Vec<u16>,
    listing: Result<Listing, u32>,
}

struct Work {
    stack: Vec<Job>,
    stop: bool,
}

/// How many threads list folders: scalable worker pool for fast SSD/NVMe I/O.
fn workers() -> usize {
    std::thread::available_parallelism().map_or(8, |n| (n.get() * 2).clamp(4, 24))
}

/// Walks `root` (a `\\?\` path to a folder). `None` when cancelled; an error
/// when the root itself cannot be read.
pub fn scan(
    root: &Path,
    scan_id: u64,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(&Progress),
) -> Result<Option<Tree>, String> {
    scan_with(root, scan_id, cancel, workers(), &mut on_progress)
}

fn scan_with(
    root: &Path,
    scan_id: u64,
    cancel: &AtomicBool,
    threads: usize,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<Option<Tree>, String> {
    let shown = display_path(root);
    let modified = std::fs::metadata(root)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);
    let mut tree = Tree::new(scan_id, shown.clone(), modified);
    let mut root_wide: Vec<u16> = root.as_os_str().encode_wide().collect();
    if root_wide.last() != Some(&0) {
        root_wide.push(0);
    }
    let work = Mutex::new(Work {
        stack: vec![Job { id: 0, path: root_wide }],
        stop: false,
    });
    let ready = Condvar::new();
    let (sender, results) = mpsc::channel::<Read>();
    let mut progress = Progress::default();
    let mut throttle = Throttle::new(PROGRESS_EVERY, Instant::now);

    let outcome = std::thread::scope(|scope| {
        for _ in 0..threads {
            let sender = sender.clone();
            let (work, ready) = (&work, &ready);
            scope.spawn(move || {
                let mut buffer = vec![0u64; BUFFER_BYTES / 8];
                let mut local_jobs = Vec::with_capacity(4);
                loop {
                    if local_jobs.is_empty() {
                        let mut work = work.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            if work.stop {
                                return;
                            }
                            let count = work.stack.len().min(4);
                            if count > 0 {
                                let start = work.stack.len() - count;
                                local_jobs.extend(work.stack.drain(start..).rev());
                                break;
                            }
                            work = ready.wait(work).unwrap_or_else(|p| p.into_inner());
                        }
                    }
                    let job = local_jobs.pop().unwrap();
                    let listing = if cancel.load(Ordering::Relaxed) { Err(0) } else { read_dir(&job.path, &mut buffer) };
                    if sender.send(Read { id: job.id, path: job.path, listing }).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);

        let stop = || {
            work.lock().unwrap_or_else(|p| p.into_inner()).stop = true;
            ready.notify_all();
        };
        // Folders queued or being read; only this thread changes it.
        let mut pending = 1usize;
        while pending > 0 {
            if cancel.load(Ordering::Relaxed) {
                stop();
                return Ok(false);
            }
            let first = match results.recv_timeout(PROGRESS_EVERY) {
                Ok(read) => read,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };

            let mut batch = vec![first];
            while batch.len() < 32 {
                match results.try_recv() {
                    Ok(read) => batch.push(read),
                    Err(_) => break,
                }
            }

            let mut all_jobs = Vec::new();
            let mut last_path = None;

            for read in batch {
                pending -= 1;
                match read.listing {
                    Ok(listing) => {
                        let ids = tree.add_children(read.id, &listing);
                        let mut name_at = 0usize;
                        for (id, entry) in ids.zip(&listing.entries) {
                            let name = &listing.names[name_at..name_at + entry.name_len as usize];
                            name_at += entry.name_len as usize;
                            match entry.kind {
                                Kind::Dir => {
                                    progress.dirs += 1;
                                    all_jobs.push(Job { id, path: child_path(&read.path, name) });
                                }
                                Kind::File | Kind::Cloud => {
                                    progress.files += 1;
                                    progress.bytes += entry.size;
                                    progress.alloc += entry.alloc;
                                }
                                Kind::Link => {}
                            }
                        }
                        last_path = Some(read.path);
                    }
                    Err(code) if read.id == 0 => {
                        stop();
                        return Err(format!(
                            "Could not read {}: {}",
                            shown.display(),
                            std::io::Error::from_raw_os_error(code as i32)
                        ));
                    }
                    Err(_) => tree.mark_inaccessible(read.id),
                }
            }

            if !all_jobs.is_empty() {
                pending += all_jobs.len();
                work.lock().unwrap_or_else(|p| p.into_inner()).stack.extend(all_jobs);
                ready.notify_all();
            }

            if throttle.ready() {
                if let Some(path) = last_path {
                    let path_units = if path.last() == Some(&0) {
                        &path[..path.len() - 1]
                    } else {
                        &path
                    };
                    progress.current = display_path(Path::new(&OsString::from_wide(path_units))).display().to_string();
                }
                on_progress(&progress);
            }
        }
        stop();
        Ok(true)
    });
    match outcome {
        Ok(true) => {
            tree.finish();
            Ok(Some(tree))
        }
        Ok(false) => Ok(None),
        Err(error) => Err(error),
    }
}

fn child_path(parent: &[u16], name: &[u8]) -> Vec<u16> {
    let parent_units = if parent.last() == Some(&0) {
        &parent[..parent.len() - 1]
    } else {
        parent
    };
    // SAFETY: the name came from `OsStr::as_encoded_bytes` or valid UTF-8.
    let name = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(name) };
    let needs_slash = parent_units.last() != Some(&(b'\\' as u16));
    let mut path = Vec::with_capacity(parent_units.len() + 1 + name.len() + 1);
    path.extend_from_slice(parent_units);
    if needs_slash {
        path.push(b'\\' as u16);
    }
    path.extend(name.encode_wide());
    path.push(0);
    path
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: an open handle of ours, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The listing classes, best first.
const CLASSES: [FILE_INFO_BY_HANDLE_CLASS; 3] = [FileIdExtdDirectoryInfo, FileIdBothDirectoryInfo, FileFullDirectoryInfo];

/// Reads a whole folder; a Win32 error code when it cannot be opened or read.
fn read_dir(path: &[u16], buffer: &mut [u64]) -> Result<Listing, u32> {
    // SAFETY: path is already NUL-terminated; the handle is closed by `Handle`.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        // SAFETY: no other call in between.
        return Err(unsafe { GetLastError() });
    }
    let handle = Handle(handle);
    let mut listing = Listing::default();
    let mut class = 0usize;
    let mut first = true;
    let bytes = std::mem::size_of_val(buffer);
    loop {
        // SAFETY: the buffer is 8-byte aligned and `bytes` long.
        let ok = unsafe {
            GetFileInformationByHandleEx(handle.0, CLASSES[class], buffer.as_mut_ptr().cast(), bytes as u32)
        };
        if ok == 0 {
            // SAFETY: right after the failed call.
            let code = unsafe { GetLastError() };
            if code == ERROR_NO_MORE_FILES {
                break;
            }
            let refused = matches!(
                code,
                ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED | ERROR_INVALID_LEVEL | ERROR_INVALID_FUNCTION
            );
            // This file system does not know the class: try an older one.
            if first && refused && class + 1 < CLASSES.len() {
                class += 1;
                continue;
            }
            return Err(code);
        }
        first = false;
        // SAFETY: Windows filled the buffer with a chain of records of `class`.
        unsafe { parse(buffer.as_ptr().cast(), bytes, CLASSES[class], &mut listing) };
    }
    Ok(listing)
}

/// The fields every listing class has, read where `class` keeps them.
struct Raw {
    next: u32,
    write_time: i64,
    size: i64,
    alloc: i64,
    attributes: u32,
    tag: u32,
    name: *const u16,
    name_units: usize,
}

/// # Safety
/// `at` points to a whole record of `class` inside the buffer.
unsafe fn raw(at: *const u8, class: FILE_INFO_BY_HANDLE_CLASS) -> Raw {
    use std::mem::offset_of;
    use std::ptr::read_unaligned as read;
    macro_rules! fields {
        ($t:ty, $tag:expr) => {{
            let attributes: u32 = unsafe { read(at.add(offset_of!($t, FileAttributes)).cast()) };
            let tag: u32 = ($tag)(attributes);
            Raw {
                next: unsafe { read(at.add(offset_of!($t, NextEntryOffset)).cast()) },
                write_time: unsafe { read(at.add(offset_of!($t, LastWriteTime)).cast()) },
                size: unsafe { read(at.add(offset_of!($t, EndOfFile)).cast()) },
                alloc: unsafe { read(at.add(offset_of!($t, AllocationSize)).cast()) },
                attributes,
                tag,
                name: unsafe { at.add(offset_of!($t, FileName)).cast() },
                name_units: unsafe { read::<u32>(at.add(offset_of!($t, FileNameLength)).cast()) } as usize / 2,
            }
        }};
    }
    // The older classes keep a reparse point's tag in EaSize.
    let ea_tag = |offset: usize| {
        move |attributes: u32| {
            if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 { unsafe { read::<u32>(at.add(offset).cast()) } } else { 0 }
        }
    };
    if class == FileIdExtdDirectoryInfo {
        fields!(FILE_ID_EXTD_DIR_INFO, |_: u32| unsafe {
            read::<u32>(at.add(offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag)).cast())
        })
    } else if class == FileIdBothDirectoryInfo {
        fields!(FILE_ID_BOTH_DIR_INFO, ea_tag(offset_of!(FILE_ID_BOTH_DIR_INFO, EaSize)))
    } else {
        fields!(FILE_FULL_DIR_INFO, ea_tag(offset_of!(FILE_FULL_DIR_INFO, EaSize)))
    }
}

/// # Safety
/// `buffer` holds `bytes` bytes written by GetFileInformationByHandleEx.
unsafe fn parse(buffer: *const u8, bytes: usize, class: FILE_INFO_BY_HANDLE_CLASS, listing: &mut Listing) {
    let mut offset = 0usize;
    let mut units = Vec::new();
    loop {
        if offset >= bytes {
            return;
        }
        // SAFETY: Windows wrote whole records, chained by NextEntryOffset.
        let record = unsafe { raw(buffer.add(offset), class) };
        units.clear();
        units.extend((0..record.name_units).map(|i| unsafe { std::ptr::read_unaligned(record.name.add(i)) }));
        let dot = units == [b'.' as u16] || units == [b'.' as u16, b'.' as u16];
        if !dot {
            let start = listing.names.len();
            push_name(&mut listing.names, &units);
            let name_len = (listing.names.len() - start) as u32;
            listing.entries.push(Entry {
                name_len,
                kind: kind_of(record.attributes, record.tag),
                size: record.size.max(0) as u64,
                alloc: record.alloc.max(0) as u64,
                modified: unix_seconds(record.write_time),
                system: record.attributes & FILE_ATTRIBUTE_SYSTEM != 0,
            });
        }
        if record.next == 0 {
            return;
        }
        offset += record.next as usize;
    }
}

/// UTF-16 to the encoded bytes of an `OsStr`: fast ASCII path when all units <= 0x7F,
/// plain UTF-8 when the name is valid Unicode (it nearly always is), WTF-8 otherwise.
fn push_name(names: &mut Vec<u8>, units: &[u16]) {
    // Fast path: if all UTF-16 code units are ASCII (<= 0x7F), copy directly!
    if units.iter().all(|&u| u <= 0x7F) {
        names.reserve(units.len());
        names.extend(units.iter().map(|&u| u as u8));
        return;
    }
    let start = names.len();
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(c) => {
                let mut buffer = [0u8; 4];
                names.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            }
            Err(_) => {
                names.truncate(start);
                names.extend_from_slice(OsString::from_wide(units).as_encoded_bytes());
                return;
            }
        }
    }
}

/// What an entry is, by its attributes and reparse tag.
fn kind_of(attributes: u32, tag: u32) -> Kind {
    let reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let cloud_tag = reparse && tag & CLOUD_TAG_MASK == CLOUD_TAG;
    if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        // A cloud folder is listed like any folder: listing it reads no file.
        // Every other reparse point on a folder (a junction, a mount point, a
        // symbolic link) leads elsewhere and is not followed.
        return if reparse && !cloud_tag { Kind::Link } else { Kind::Dir };
    }
    if reparse && matches!(tag, IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT | IO_REPARSE_TAG_APPEXECLINK) {
        return Kind::Link;
    }
    if cloud_tag || attributes & RECALL != 0 {
        return Kind::Cloud;
    }
    Kind::File
}

#[cfg(test)]
mod tests {
    use super::super::tree::{SizeMode, SortKey};
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::AtomicUsize;

    /// A folder under %TEMP% removed when the test ends.
    pub struct TempTree(pub PathBuf);

    impl TempTree {
        pub fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("myle-disk-{name}-{}", std::process::id()));
            let _ = remove_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = remove_all(&self.0);
        }
    }

    /// Removes a test folder, long paths and junctions included (a junction
    /// is removed itself, never what it points at).
    fn remove_all(path: &Path) -> std::io::Result<()> {
        std::fs::remove_dir_all(super::super::tree::long_path(path))
    }

    fn write(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(super::super::tree::long_path(parent)).unwrap();
        }
        std::fs::write(super::super::tree::long_path(path), vec![b'x'; bytes]).unwrap();
    }

    fn junction(link: &Path, target: &Path) -> bool {
        let cmd = crate::apps::process::system32("cmd.exe");
        Command::new(cmd)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|out| out.status.success())
    }

    fn run(root: &Path) -> Tree {
        let cancel = AtomicBool::new(false);
        scan(&super::super::tree::long_path(root), 1, &cancel, |_| {}).unwrap().unwrap()
    }

    fn find(tree: &Tree, name: &str) -> Option<u32> {
        (0..tree.len() as u32).find(|&i| tree.name(i) == name)
    }

    #[test]
    fn a_folder_tree_is_read_with_totals_names_and_order() {
        let temp = TempTree::new("walk");
        let root = &temp.0;
        write(&root.join("big.bin"), 300_000);
        write(&root.join("a").join("one.txt"), 1_000);
        write(&root.join("a").join("b").join("two.txt"), 2_000);
        std::fs::create_dir_all(root.join("empty")).unwrap();
        write(&root.join("Ελληνικά 📦").join("αρχείο.txt"), 10);
        // Deeper than MAX_PATH.
        let mut long = root.join("long");
        while long.as_os_str().len() < 300 {
            long.push("a-folder-with-a-long-name");
        }
        write(&long.join("deep.txt"), 7);

        let mut tree = run(root);
        assert_eq!(tree.files(0), 5);
        assert_eq!(tree.size(0, SizeMode::Size), 300_000 + 1_000 + 2_000 + 10 + 7);
        assert!(tree.size(0, SizeMode::Alloc) >= tree.size(0, SizeMode::Size) - 10, "clusters round up");
        let greek = find(&tree, "Ελληνικά 📦").expect("a Unicode folder");
        assert_eq!(tree.files(greek), 1);
        assert!(find(&tree, "αρχείο.txt").is_some());
        let deep = find(&tree, "deep.txt").expect("a file past 260 characters");
        assert!(tree.path(deep).as_os_str().len() > 260);
        let empty = find(&tree, "empty").unwrap();
        assert_eq!(tree.files(empty), 0);
        assert!(!tree.has_children(empty));
        let page = tree.children(0, 0, 10, SortKey::Size, SizeMode::Size);
        assert_eq!(tree.name(page.ids[0]), "big.bin", "largest first");
        assert!(tree.inaccessible.is_empty());
    }

    #[test]
    fn a_junction_is_listed_and_never_followed() {
        let outside = TempTree::new("outside");
        write(&outside.0.join("huge.bin"), 500_000);
        let temp = TempTree::new("junction");
        write(&temp.0.join("small.txt"), 10);
        if !junction(&temp.0.join("jump"), &outside.0) {
            eprintln!("skipped: mklink /J is not available here");
            return;
        }
        let tree = run(&temp.0);
        let jump = find(&tree, "jump").expect("the junction is listed");
        assert_eq!(tree.node(jump).kind, Kind::Link);
        assert_eq!(tree.size(0, SizeMode::Size), 10, "nothing behind the junction is counted");
        assert!(find(&tree, "huge.bin").is_none());
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_counted_and_the_walk_goes_on() {
        let temp = TempTree::new("denied");
        write(&temp.0.join("open").join("seen.txt"), 5);
        let locked = temp.0.join("locked");
        write(&locked.join("hidden.txt"), 5);
        let icacls = crate::apps::process::system32("icacls.exe");
        let denied = Command::new(&icacls)
            .arg(&locked)
            .args(["/deny", "*S-1-1-0:(RD)"])
            .output()
            .is_ok_and(|out| out.status.success());
        if !denied {
            eprintln!("skipped: icacls could not set the ACL");
            return;
        }
        let tree = run(&temp.0);
        let _ = Command::new(&icacls).arg(&locked).args(["/remove:d", "*S-1-1-0"]).output();
        let id = find(&tree, "locked").unwrap();
        assert_eq!(tree.inaccessible, vec![id]);
        assert!(find(&tree, "seen.txt").is_some());
        assert!(find(&tree, "hidden.txt").is_none());
    }

    #[test]
    fn the_root_that_cannot_be_read_is_an_error() {
        let cancel = AtomicBool::new(false);
        let missing = std::env::temp_dir().join(format!("myle-disk-missing-{}", std::process::id()));
        assert!(scan(&super::super::tree::long_path(&missing), 1, &cancel, |_| {}).is_err());
    }

    #[test]
    fn cancelling_stops_the_scan_early() {
        let temp = TempTree::new("cancel");
        for i in 0..40 {
            write(&temp.0.join(format!("d{i}")).join("f.txt"), 1);
        }
        let cancel = AtomicBool::new(true);
        let result = scan(&super::super::tree::long_path(&temp.0), 1, &cancel, |_| {}).unwrap();
        assert!(result.is_none());
        // Cancelled from the progress callback, as the page does.
        let cancel = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        let mut on_progress = |_: &Progress| {
            calls.fetch_add(1, Ordering::Relaxed);
            cancel.store(true, Ordering::Relaxed);
        };
        let result = scan_with(&super::super::tree::long_path(&temp.0), 1, &cancel, 1, &mut on_progress).unwrap();
        assert!(result.is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    /// Run by hand for the numbers in the PR: a whole drive (MYLE_SCAN_ROOT,
    /// or C:\), its time, its counts and what the results hold in memory.
    #[test]
    #[ignore]
    fn measure_a_whole_drive() {
        let root = std::env::var("MYLE_SCAN_ROOT").unwrap_or_else(|_| r"C:\".into());
        let cancel = AtomicBool::new(false);
        let events = AtomicUsize::new(0);
        let started = Instant::now();
        let root = std::fs::canonicalize(&root).unwrap();
        let mut tree = scan(&root, 1, &cancel, |_| {
            events.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap()
        .unwrap();
        let elapsed = started.elapsed();
        let treemap = tree.treemap(0, 10, tree.size(0, SizeMode::Alloc) / 2_000_000, SizeMode::Alloc, 20_000);
        let payload = serde_json::to_string(&treemap).unwrap().len();
        let top = tree.top_files(SizeMode::Alloc, 1000, None).len();
        eprintln!(
            "{}: {} files, {} folders, {} nodes, {:.1} GB on disk, {} unreadable folders, {:.1} s, {} progress events, \
             tree {:.0} MB ({} bytes a node with its name), treemap {} nodes / {:.0} KB of JSON, top {}",
            root.display(),
            tree.files(0),
            tree.dirs(0),
            tree.len(),
            tree.size(0, SizeMode::Alloc) as f64 / 1e9,
            tree.inaccessible.len(),
            elapsed.as_secs_f64(),
            events.load(Ordering::Relaxed),
            tree.memory() as f64 / 1_048_576.0,
            tree.memory() / tree.len(),
            treemap.ids.len(),
            payload as f64 / 1024.0,
            top,
        );
        // Time for the peak working set to be read from outside.
        std::thread::sleep(Duration::from_secs(4));
    }

    #[test]
    fn progress_is_throttled_to_ten_a_second() {
        let start = Instant::now();
        let mut now = start;
        let mut throttle = Throttle::new(PROGRESS_EVERY, || {
            now += Duration::from_millis(7);
            now
        });
        let sent = (0..10_000).filter(|_| throttle.ready()).count();
        // 10,000 calls 7 ms apart: 70 seconds, so at most 701 events, and
        // one every 15 calls (105 ms) in practice.
        assert!(sent <= 701, "{sent}");
        assert_eq!(sent, 10_000_usize.div_ceil(15));
    }

    #[test]
    fn entries_are_told_apart_by_attributes_and_tags() {
        const DIR: u32 = FILE_ATTRIBUTE_DIRECTORY;
        const REPARSE: u32 = FILE_ATTRIBUTE_REPARSE_POINT;
        assert_eq!(kind_of(DIR, 0), Kind::Dir);
        assert_eq!(kind_of(DIR | REPARSE, IO_REPARSE_TAG_MOUNT_POINT), Kind::Link);
        assert_eq!(kind_of(DIR | REPARSE, IO_REPARSE_TAG_SYMLINK), Kind::Link);
        assert_eq!(kind_of(DIR | REPARSE, 0x9000_601A), Kind::Dir, "a OneDrive folder is walked");
        assert_eq!(kind_of(0x20, 0), Kind::File);
        assert_eq!(kind_of(REPARSE, IO_REPARSE_TAG_SYMLINK), Kind::Link);
        assert_eq!(kind_of(REPARSE, 0x9000_001A), Kind::Cloud);
        assert_eq!(kind_of(FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, 0), Kind::Cloud);
        assert_eq!(kind_of(FILE_ATTRIBUTE_OFFLINE, 0), Kind::Cloud);
        assert_eq!(kind_of(REPARSE, 0x8000_0017), Kind::File, "a compressed (WOF) file is a file");
        let mut names = Vec::new();
        push_name(&mut names, &"é".encode_utf16().collect::<Vec<_>>());
        assert_eq!(names, "é".as_bytes());
    }
}
