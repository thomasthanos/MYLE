//! The administrator half of the cleaner.
//!
//! The elevated helper opens each configured root without following reparse
//! points and traverses its descendants by Windows file ID. Descendant paths
//! are never used for deletion, so replacing a checked folder with a junction
//! cannot redirect the cleaner elsewhere.

use std::collections::HashMap;
use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_INFO,
    FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_ID_BOTH_DIR_INFO, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, FileDispositionInfo, FileDispositionInfoEx, FileIdBothDirectoryInfo,
    FileIdBothDirectoryRestartInfo, FileIdType, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING, OpenFileById,
    SetFileInformationByHandle,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use super::targets::{self, Category, Target};
use crate::elevated_pipe::{self, Helper, Session};

/// `\\.\pipe\myle-cleaner-<32 hex digits>`, created by the app.
const HELPER: Helper = Helper {
    flag: "--cleaner-elevated-helper",
    pipe_prefix: r"\\.\pipe\myle-cleaner-",
    what: "administrator cleaner",
};
const MAX_DEPTH: usize = 128;
const SHARE_WITHOUT_RENAME: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Measure,
    Clean,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct Totals {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub skipped: u64,
    /// What the skipped files hold, so the page can size what is left in use.
    #[serde(default)]
    pub skipped_bytes: u64,
}

impl Totals {
    /// Files that could not be removed: those in use, and the ones this
    /// process decided not to touch. Their size is kept for the page.
    fn skip(&mut self, bytes: u64) {
        self.skipped += 1;
        self.skipped_bytes += bytes;
    }
}

/// One request to the helper: an action over the admin-only folders of
/// these category ids.
#[derive(Debug, Deserialize, Serialize)]
struct Request {
    action: Action,
    ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Response {
    Ok(HashMap<String, Totals>),
    Error(String),
}

/// The running helper: one UAC prompt, then every scan and clean of this app
/// session goes through it.
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

/// Runs `action` over the admin-only folders of `categories` and returns the
/// totals per category id. The first call asks for administrator approval;
/// later ones reuse the running helper without asking again.
pub async fn run(
    categories: &[&'static Category],
    action: Action,
) -> Result<HashMap<String, Totals>, String> {
    let ids: Vec<String> = categories
        .iter()
        .filter(|category| category.targets().iter().any(|target| target.admin))
        .map(|category| category.id.to_string())
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let request = Request { action, ids };

    let mut session = SESSION.lock().await;
    if let Some(open) = session.as_mut() {
        match open.ask(&request).await {
            Ok(response) => return answer(response),
            // The helper has gone: start a new one below.
            Err(_) => *session = None,
        }
    }
    let mut fresh = Session::start(&HELPER).await?;
    let response = fresh
        .ask(&request)
        .await
        .map_err(|error| format!("The administrator cleaner stopped: {error}"))?;
    *session = Some(fresh);
    answer(response)
}

/// Whether a helper is running, so the next scan or clean will not ask.
pub async fn is_running() -> bool {
    SESSION.lock().await.is_some()
}

fn answer(response: Response) -> Result<HashMap<String, Totals>, String> {
    match response {
        Response::Ok(totals) => Ok(totals),
        Response::Error(error) => Err(error),
    }
}

/// Called before Tauri starts. Returns `None` for a normal launch and an exit
/// code for the tightly scoped elevated helper mode.
pub fn run_helper_from_args() -> Option<i32> {
    let args = elevated_pipe::helper_args(&HELPER)?;
    let served = elevated_pipe::parse_serve_args(&HELPER, &args).and_then(|(pipe, app_pid)| serve(&pipe, app_pid));
    Some(match served {
        Ok(()) => 0,
        Err(error) => {
            // Release builds have no console, but this remains useful in tests
            // and when the executable is launched from a diagnostic terminal.
            eprintln!("Elevated cleaner: {error}");
            2
        }
    })
}

fn serve(pipe: &str, app_pid: u32) -> Result<(), String> {
    elevated_pipe::serve(
        &HELPER,
        pipe,
        app_pid,
        |request: Request| match handle(&request) {
            Ok(totals) => Response::Ok(totals),
            Err(error) => Response::Error(error),
        },
        || Response::Error("Invalid cleaner request.".into()),
    )
}

fn handle(request: &Request) -> Result<HashMap<String, Totals>, String> {
    let mut result = HashMap::new();
    for id in &request.ids {
        let Some(category) = targets::find(id) else {
            return Err("Unknown cleaner category.".into());
        };
        let mut total = Totals::default();
        for target in category.targets().iter().filter(|target| target.admin) {
            let one = visit_target(target, request.action)?;
            total.bytes += one.bytes;
            total.files += one.files;
            total.skipped += one.skipped;
        }
        result.insert(category.id.to_string(), total);
    }
    Ok(result)
}

fn visit_target(target: &Target, action: Action) -> Result<Totals, String> {
    let root_path = elevated_target_path(target)?;
    let root = match OwnedHandle::open_root(&root_path) {
        Ok(root) => root,
        Err(error) if error.raw_os_error() == Some(2) || error.raw_os_error() == Some(3) => {
            return Ok(Totals::default());
        }
        Err(error) => return Err(error.to_string()),
    };
    let info = root.info()?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
        != FILE_ATTRIBUTE_DIRECTORY
    {
        return Err("A cleaner root is not a normal directory.".into());
    }
    let root_path = normalized_final_path(root.0)?;
    let mut totals = Totals::default();
    visit_directory(&root, &root_path, target.patterns, action, 0, &mut totals)?;
    Ok(totals)
}

/// The elevated process must not trust inherited environment variables for
/// administrator-only roots. `%PROGRAMDATA%` is resolved through Windows'
/// Known Folders API; every other admin target is a compiled absolute path.
fn elevated_target_path(target: &Target) -> Result<PathBuf, String> {
    if let Some(suffix) = target.dir.strip_prefix("%PROGRAMDATA%") {
        return Ok(known_program_data()?.join(suffix.trim_start_matches(['\\', '/'])));
    }
    if target.dir.contains('%') {
        return Err("An elevated cleaner target uses an unsupported variable.".into());
    }
    Ok(PathBuf::from(target.dir))
}

fn known_program_data() -> Result<PathBuf, String> {
    let mut raw = null_mut();
    let status = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_ProgramData,
            KF_FLAG_DEFAULT as u32,
            null_mut(),
            &mut raw,
        )
    };
    if status < 0 || raw.is_null() {
        return Err(format!(
            "Windows could not locate ProgramData (HRESULT 0x{:08x}).",
            status as u32
        ));
    }
    let mut length = 0usize;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    Ok(path)
}

fn visit_directory(
    directory: &OwnedHandle,
    root_path: &str,
    patterns: &[&str],
    action: Action,
    depth: usize,
    totals: &mut Totals,
) -> Result<(), String> {
    if depth >= MAX_DEPTH {
        totals.skip(0);
        return Ok(());
    }

    let mut restart = true;
    loop {
        // usize gives the variable-length directory records their required
        // alignment. The API marks the final record with NextEntryOffset = 0.
        let mut buffer = [0usize; 8192];
        let class = if restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        restart = false;
        let ok = unsafe {
            GetFileInformationByHandleEx(
                directory.0,
                class,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(std::io::Error::from_raw_os_error(code as i32).to_string());
        }

        let bytes = size_of_val(&buffer);
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            if offset + size_of::<FILE_ID_BOTH_DIR_INFO>() > bytes {
                return Err("Windows returned an invalid directory record.".into());
            }
            let entry = unsafe { &*base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>() };
            let name_bytes = entry.FileNameLength as usize;
            let name_offset =
                (std::ptr::addr_of!(entry.FileName) as usize).saturating_sub(base as usize);
            if !name_bytes.is_multiple_of(2) || name_offset + name_bytes > bytes {
                return Err("Windows returned an invalid directory name.".into());
            }
            let name =
                unsafe { std::slice::from_raw_parts(entry.FileName.as_ptr(), name_bytes / 2) };
            let name = String::from_utf16_lossy(name);
            if name != "." && name != ".." {
                visit_entry(
                    directory, root_path, patterns, action, depth, entry, &name, totals,
                );
            }

            if entry.NextEntryOffset == 0 {
                break;
            }
            let next = entry.NextEntryOffset as usize;
            if next < size_of::<FILE_ID_BOTH_DIR_INFO>() || offset + next >= bytes {
                return Err("Windows returned an invalid directory offset.".into());
            }
            offset += next;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn visit_entry(
    parent: &OwnedHandle,
    root_path: &str,
    patterns: &[&str],
    action: Action,
    depth: usize,
    entry: &FILE_ID_BOTH_DIR_INFO,
    listed_name: &str,
    totals: &mut Totals,
) {
    if entry.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return;
    }
    let listed_directory = entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    // For the paths that cannot open the entry: the size the listing gave.
    let listed_bytes = if listed_directory {
        0
    } else {
        entry.EndOfFile as u64
    };
    let access = FILE_READ_ATTRIBUTES
        | if listed_directory {
            FILE_LIST_DIRECTORY
        } else {
            0
        }
        | if action == Action::Clean {
            DELETE | FILE_WRITE_ATTRIBUTES
        } else {
            0
        };
    let Ok(child) = OwnedHandle::open_id(parent.0, entry.FileId, access) else {
        totals.skip(listed_bytes);
        return;
    };
    let Ok(info) = child.info() else {
        totals.skip(listed_bytes);
        return;
    };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return;
    }
    let is_directory = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if is_directory != listed_directory {
        totals.skip(listed_bytes);
        return;
    }
    let listed_bytes = size_of_entry(&info);
    let Ok(final_path) = normalized_final_path(child.0) else {
        totals.skip(listed_bytes);
        return;
    };
    if !is_below(root_path, &final_path) {
        // The entry moved outside the allowed root after enumeration and must
        // not be touched. The identity check below closes the remaining race
        // between this observation and acquiring the pinned path handle.
        totals.skip(listed_bytes);
        return;
    }
    // Open the discovered name without delete sharing and prove it still
    // names the same volume/file ID. This second handle pins the name against
    // rename and is the exact handle later passed to the delete API.
    let Ok(pinned) = OwnedHandle::open_path(Path::new(&final_path), access) else {
        totals.skip(listed_bytes);
        return;
    };
    let Ok(pinned_info) = pinned.info() else {
        totals.skip(listed_bytes);
        return;
    };
    if !same_file(&info, &pinned_info)
        || pinned_info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        totals.skip(listed_bytes);
        return;
    }
    drop(child);
    let child = pinned;
    let info = pinned_info;

    if is_directory {
        if patterns.is_empty() {
            if visit_directory(&child, root_path, patterns, action, depth + 1, totals).is_err() {
                totals.skip(0);
            }
            if action == Action::Clean {
                let _ = child.delete(); // only succeeds when now empty
            }
        }
        return;
    }

    let actual_name = Path::new(&final_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(listed_name);
    if !targets::matches(actual_name, patterns) {
        return;
    }
    let size = size_of_entry(&info);
    match action {
        Action::Measure => {
            totals.bytes += size;
            totals.files += 1;
        }
        Action::Clean if info.nNumberOfLinks > 1 => {
            // OpenFileById does not identify which hard-link name will be
            // removed. Never let an outside link make that choice ambiguous.
            totals.skip(size);
        }
        Action::Clean => match child.delete() {
            Ok(()) => {
                totals.bytes += size;
                totals.files += 1;
            }
            Err(_error) => {
                #[cfg(test)]
                eprintln!("delete failed for {final_path}: {_error}");
                totals.skip(size);
            }
        },
    }
}

/// The size a handle reports. For a folder it is meaningless, and 0 keeps the
/// "still in use" figure about files only.
fn size_of_entry(info: &BY_HANDLE_FILE_INFORMATION) -> u64 {
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return 0;
    }
    ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64
}

fn is_below(root: &str, candidate: &str) -> bool {
    candidate.len() > root.len()
        && candidate.starts_with(root)
        && candidate.as_bytes().get(root.len()) == Some(&b'\\')
}

fn same_file(left: &BY_HANDLE_FILE_INFORMATION, right: &BY_HANDLE_FILE_INFORMATION) -> bool {
    left.dwVolumeSerialNumber == right.dwVolumeSerialNumber
        && left.nFileIndexHigh == right.nFileIndexHigh
        && left.nFileIndexLow == right.nFileIndexLow
}

fn normalized_final_path(handle: HANDLE) -> Result<String, String> {
    let needed = unsafe { GetFinalPathNameByHandleW(handle, null_mut(), 0, 0) };
    if needed == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut buffer = vec![0u16; needed as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(String::from_utf16_lossy(&buffer[..written as usize])
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase())
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn open_root(path: &Path) -> std::io::Result<Self> {
        Self::open_path(path, FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
    }

    fn open_path(path: &Path, access: u32) -> std::io::Result<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                access,
                SHARE_WITHOUT_RENAME,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        Self::from_raw(handle)
    }

    fn open_id(volume_hint: HANDLE, id: i64, access: u32) -> std::io::Result<Self> {
        let descriptor = FILE_ID_DESCRIPTOR {
            dwSize: size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: FileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: id },
        };
        let handle = unsafe {
            OpenFileById(
                volume_hint,
                &descriptor,
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        Self::from_raw(handle)
    }

    fn from_raw(handle: HANDLE) -> std::io::Result<Self> {
        if handle == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn info(&self) -> Result<BY_HANDLE_FILE_INFORMATION, String> {
        let mut info = unsafe { zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        if unsafe { GetFileInformationByHandle(self.0, &mut info) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(info)
        }
    }

    fn delete(&self) -> std::io::Result<()> {
        let extended = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        if unsafe {
            SetFileInformationByHandle(
                self.0,
                FileDispositionInfoEx,
                (&extended as *const FILE_DISPOSITION_INFO_EX).cast(),
                size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        } != 0
        {
            return Ok(());
        }

        // Older file systems may not implement FileDispositionInfoEx.
        let legacy = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                self.0,
                FileDispositionInfo,
                (&legacy as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } != 0
        {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevated_pipe::tests::{connect_to, runtime};
    use std::time::Duration;
    use tokio::net::windows::named_pipe::ServerOptions;

    fn temp_target(label: &str) -> (PathBuf, Target) {
        let root = std::env::temp_dir().join(format!(
            "myle-elevated-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let dir = Box::leak(root.to_string_lossy().into_owned().into_boxed_str());
        (
            root,
            Target {
                dir,
                patterns: &[],
                admin: true,
            },
        )
    }

    #[test]
    fn handle_walk_measures_and_deletes_normal_descendants() {
        let (root, target) = temp_target("normal");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.tmp"), vec![1; 20]).unwrap();
        std::fs::write(root.join("nested").join("b.tmp"), vec![2; 30]).unwrap();

        assert_eq!(
            visit_target(&target, Action::Measure).unwrap(),
            Totals {
                bytes: 50,
                files: 2,
                skipped: 0,
                skipped_bytes: 0,
            }
        );
        let cleaned = visit_target(&target, Action::Clean).unwrap();
        assert_eq!(cleaned.files, 2, "cleaned: {cleaned:?}");
        assert!(root.exists(), "the configured root itself stays");
        assert!(std::fs::read_dir(&root).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn handle_walk_does_not_follow_a_junction() {
        use std::os::windows::process::CommandExt;

        let (root, target) = temp_target("junction-root");
        let (outside, _) = temp_target("junction-outside");
        let sentinel = outside.join("keep.txt");
        std::fs::write(&sentinel, b"keep me").unwrap();
        let link = root.join("redirect");
        let status = std::process::Command::new("cmd.exe")
            .args([
                "/d",
                "/c",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &outside.to_string_lossy(),
            ])
            .creation_flags(0x0800_0000)
            .status()
            .unwrap();
        if status.success() {
            let total = visit_target(&target, Action::Clean).unwrap();
            assert_eq!(total.files, 0);
            assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep me");
        }
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn the_helper_answers_the_app_over_its_pipe() {
        runtime().block_on(async {
            let name = format!("{}{}", HELPER.pipe_prefix, uuid::Uuid::new_v4().simple());
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&name)
                .unwrap();
            // This test process is the pipe's server, as the app would be.
            let helper = {
                let name = name.clone();
                std::thread::spawn(move || serve(&name, std::process::id()))
            };
            let mut session = connect_to(server).await;

            let empty = Request {
                action: Action::Measure,
                ids: Vec::new(),
            };
            let answer: Response = session.ask(&empty).await.unwrap();
            assert!(matches!(answer, Response::Ok(t) if t.is_empty()));
            let unknown = Request {
                action: Action::Clean,
                ids: vec!["../../etc".into()],
            };
            let answer: Response = session.ask(&unknown).await.unwrap();
            assert!(matches!(answer, Response::Error(_)));

            // Closing the pipe (the app exiting) ends the helper. The runtime
            // has to keep running meanwhile: it finishes closing the pipe.
            drop(session);
            let ended = tokio::task::spawn_blocking(move || helper.join().unwrap());
            let ended = tokio::time::timeout(Duration::from_secs(20), ended)
                .await
                .expect("the helper did not end when the pipe closed")
                .unwrap();
            assert!(ended.is_ok());
        });
    }

    #[test]
    fn the_helper_refuses_a_pipe_the_app_does_not_serve() {
        runtime().block_on(async {
            let name = format!("{}{}", HELPER.pipe_prefix, uuid::Uuid::new_v4().simple());
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&name)
                .unwrap();
            let helper = std::thread::spawn(move || serve(&name, std::process::id() + 1));
            server.connect().await.unwrap();
            assert!(helper.join().unwrap().is_err());
        });
    }

    #[test]
    fn containment_requires_a_real_path_boundary() {
        assert!(is_below(r"\\?\c:\temp", r"\\?\c:\temp\a.txt"));
        assert!(!is_below(r"\\?\c:\temp", r"\\?\c:\temporary\a.txt"));
        assert!(!is_below(r"\\?\c:\temp", r"\\?\c:\temp"));
    }

    #[test]
    fn elevated_targets_do_not_depend_on_inherited_environment_paths() {
        for target in targets::CATEGORIES
            .iter()
            .flat_map(Category::targets)
            .filter(|target| target.admin)
        {
            let resolved = elevated_target_path(target).unwrap();
            assert!(resolved.is_absolute(), "{}", resolved.display());
            assert!(!resolved.to_string_lossy().contains('%'));
        }
    }
}
