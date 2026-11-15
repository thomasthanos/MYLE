// ! The administrator half of the cleaner. ! ! The elevated helper opens each configured root without following reparse ! points and traverses its descendants by Windows file ID. Descendant paths ! are never used for deletion, so replacing a checked folder with a junction ! cannot redirect the cleaner elsewhere.
use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
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
use windows_sys::Win32::UI::Shell::FOLDERID_ProgramData;
use super::targets::{self, Category, Target};
use crate::elevated_pipe::{self, Helper, KeptHelper};
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
    /// Clean, and links met on the way removed too (the link itself, never
    /// what it points to): Spotify Hub's leftovers, whose folders go whole.
    Remove,
}
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct Totals {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub skipped: u64,
    /// Bytes belonging to skipped items.
    #[serde(default)]
    pub skipped_bytes: u64,
    #[serde(default)]
    pub in_use: u64,
    #[serde(default)]
    pub in_use_bytes: u64,
}
impl Totals {
    /// Files that could not be removed: those in use, and the ones this
    /// process decided not to touch. Their size is kept for the page.
    fn skip(&mut self, bytes: u64) {
        self.skipped += 1;
        self.skipped_bytes += bytes;
    }
    fn skip_error(&mut self, bytes: u64, is_directory: bool, error: &std::io::Error) {
        self.skip(bytes);
        if !is_directory && targets::is_in_use(error) {
            self.in_use += 1;
            self.in_use_bytes += bytes;
        }
    }
}
/// One request to the helper: an action over the admin-only folders of
/// these category ids, or Spotify Hub's protected leftovers.
#[derive(Debug, Deserialize, Serialize)]
struct Request {
    action: Action,
    ids: Vec<String>,
    /// Spotify Hub: folders and files of the app's own user to remove. Each
    /// must lie inside that user's profile folder; nothing is followed
    /// through a link and every item is checked by handle (`remove_item`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trees: Vec<PathBuf>,
    /// Spotify Hub: also the machine-wide Spotify uninstall entries.
    #[serde(default)]
    spotify_registry: bool,
}
/// Removes Spotify Hub's leftovers that need administrator rights: the
/// folders and files `paths` (inside the user's profile) and, with
/// `machine_registry`, Spotify's machine-wide uninstall entries. Through the
/// same helper and approval as the System Cleaner. Answers what was removed.
pub async fn remove_spotify_leftovers(paths: Vec<PathBuf>, machine_registry: bool) -> Result<HashMap<String, Totals>, String> {
    ask(Request { action: Action::Clean, ids: Vec::new(), trees: paths, spotify_registry: machine_registry }).await
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Response {
    Ok(HashMap<String, Totals>),
    Error(String),
}
/// The running helper: one UAC prompt, then every scan and clean of this app
/// session goes through it.
static KEPT: KeptHelper = KeptHelper::new(&HELPER);
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
    let request = Request { action, ids, trees: Vec::new(), spotify_registry: false };
    let pid = std::process::id();
    if elevated_pipe::is_elevated(pid) {
        return tauri::async_runtime::spawn_blocking(move || handle(&request, pid))
            .await
            .map_err(|error| error.to_string())?;
    }
    ask(request).await
}
async fn ask(request: Request) -> Result<HashMap<String, Totals>, String> {
    answer(KEPT.ask(&request).await?)
}
/// Whether a helper is running, so the next scan or clean will not ask.
pub async fn is_running() -> bool {
    KEPT.is_running().await
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
            // Release builds have no console, but this remains useful in tests and when the executable is launched from a diagnostic terminal.
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
        |request: Request| match handle(&request, app_pid) {
            Ok(totals) => Response::Ok(totals),
            Err(error) => Response::Error(error),
        },
        || Response::Error("Invalid cleaner request.".into()),
    )
}
fn handle(request: &Request, app_pid: u32) -> Result<HashMap<String, Totals>, String> {
    if !request.trees.is_empty() || request.spotify_registry {
        return spotify_leftovers(&request.trees, request.spotify_registry, app_pid);
    }
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
            total.skipped_bytes += one.skipped_bytes;
            total.in_use += one.in_use;
            total.in_use_bytes += one.in_use_bytes;
        }
        result.insert(category.id.to_string(), total);
    }
    Ok(result)
}
/// The Spotify uninstall entries Spotify's own installer leaves machine-wide.
const SPOTIFY_MACHINE_KEYS: [&str; 2] = [
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Spotify",
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Spotify",
];
fn spotify_leftovers(paths: &[PathBuf], machine_registry: bool, app_pid: u32) -> Result<HashMap<String, Totals>, String> {
    let mut result = HashMap::new();
    if !paths.is_empty() {
        // The app's user's profile, from Windows' own list (not this
        // process's environment: an administrator approving over the
        // shoulder runs this helper as another account).
        let profile = elevated_pipe::process_account(app_pid)
            .and_then(|sid| profile_folder(&sid))
            .ok_or("The app's user profile could not be found.")?;
        let profile = OwnedHandle::open_root(&profile).map_err(|e| e.to_string())?;
        let profile = normalized_final_path(profile.0)?;
        for path in paths {
            result.insert(path.to_string_lossy().into_owned(), remove_item(path, &profile)?);
        }
    }
    if machine_registry {
        let machine = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        for key in SPOTIFY_MACHINE_KEYS {
            match machine.delete_subkey_all(key) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{key}: {error}")),
            }
        }
    }
    Ok(result)
}
/// `ProfileImagePath` of the account `sid` (`C:\Users\<name>`).
fn profile_folder(sid: &str) -> Option<PathBuf> {
    let list = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\{sid}"))
        .ok()?;
    let raw: String = list.get_value("ProfileImagePath").ok()?;
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let path = PathBuf::from(raw.replace("%SystemDrive%", &system_drive).replace("%SYSTEMDRIVE%", &system_drive));
    path.is_absolute().then_some(path)
}
/// Removes one folder (with everything in it) or file inside `profile` (a
/// final path). The item is opened without following a link: a link is
/// refused, and so is an item whose real path is not inside the profile.
/// Below it, the same walk as the cleaner's: each entry opened by its id,
/// its real path checked to stay inside, pinned and deleted by handle.
fn remove_item(path: &Path, profile: &str) -> Result<Totals, String> {
    let mut totals = Totals::default();
    if !path.is_absolute() {
        return Err("A Spotify leftover path is not absolute.".into());
    }
    let access = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | DELETE;
    let item = match OwnedHandle::open_path(path, access) {
        Ok(item) => item,
        Err(error) if error.raw_os_error() == Some(2) || error.raw_os_error() == Some(3) => return Ok(totals),
        Err(_) => {
            totals.skip(0);
            return Ok(totals);
        }
    };
    let info = item.info()?;
    let final_path = normalized_final_path(item.0)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !is_below(profile, &final_path) {
        totals.skip(0);
        return Ok(totals);
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        visit_directory(&item, &final_path, &[], Action::Remove, 0, &mut totals)?;
        if item.delete().is_err() {
            totals.skip(0); // something inside stayed (in use, or refused)
        }
    } else if info.nNumberOfLinks > 1 {
        totals.skip(size_of_entry(&info));
    } else {
        match item.delete() {
            Ok(()) => {
                totals.bytes += size_of_entry(&info);
                totals.files += 1;
            }
            Err(_) => totals.skip(size_of_entry(&info)),
        }
    }
    Ok(totals)
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
    crate::storage::known_folder(&FOLDERID_ProgramData).ok_or_else(|| "Windows could not locate ProgramData.".to_string())
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
    // usize gives the variable-length directory records their required alignment. The API marks the final record with NextEntryOffset = 0.
    // On the heap, once per folder: 64 KB on the stack in every level of the recursion overflowed the helper's 1 MB main-thread stack about 15 folders deep.
    let mut buffer = vec![0usize; 8192];
    let bytes = buffer.len() * size_of::<usize>();
    loop {
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
                bytes as u32,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(std::io::Error::from_raw_os_error(code as i32).to_string());
        }
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
/// A link inside a folder that goes whole: the link itself is removed. It is
/// opened by its id without following it (`FILE_FLAG_OPEN_REPARSE_POINT`),
/// so its own path is checked and its own entry deleted; what it points to is
/// never opened.
fn remove_link(parent: &OwnedHandle, root_path: &str, entry: &FILE_ID_BOTH_DIR_INFO, totals: &mut Totals) {
    // Found by its id; deleted through its own path (a handle opened by id
    // cannot be deleted), opened again without following it and proven to
    // be the same entry, as `visit_entry` does for files.
    let removed = (|| {
        let found = OwnedHandle::open_id(parent.0, entry.FileId, FILE_READ_ATTRIBUTES).ok()?;
        let found_info = found.info().ok()?;
        let path = normalized_final_path(found.0).ok()?;
        if found_info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 || !is_below(root_path, &path) {
            return None;
        }
        let pinned = OwnedHandle::open_path(Path::new(&path), FILE_READ_ATTRIBUTES | DELETE).ok()?;
        let pinned_info = pinned.info().ok()?;
        if !same_file(&found_info, &pinned_info) || pinned_info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return None;
        }
        pinned.delete().ok()
    })();
    if removed.is_none() {
        totals.skip(0);
    }
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
        if action == Action::Remove {
            remove_link(parent, root_path, entry, totals);
        }
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
        | if action != Action::Measure {
            DELETE | FILE_WRITE_ATTRIBUTES
        } else {
            0
        };
    let child = match OwnedHandle::open_id(parent.0, entry.FileId, access) {
        Ok(child) => child,
        Err(error) => {
            totals.skip_error(listed_bytes, listed_directory, &error);
            return;
        }
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
        // The entry moved outside the allowed root after enumeration and must not be touched. The identity check below closes the remaining race between this observation and acquiring the pinned path handle.
        totals.skip(listed_bytes);
        return;
    }
    // Open the discovered name without delete sharing and prove it still names the same volume/file ID. This second handle pins the name against rename and is the exact handle later passed to the delete API.
    let pinned = match OwnedHandle::open_path(Path::new(&final_path), access) {
        Ok(pinned) => pinned,
        Err(error) => {
            totals.skip_error(listed_bytes, is_directory, &error);
            return;
        }
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
            if action != Action::Measure {
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
        Action::Clean | Action::Remove if info.nNumberOfLinks > 1 => {
            // OpenFileById does not identify which hard-link name will be removed. Never let an outside link make that choice ambiguous.
            totals.skip(size);
        }
        Action::Clean | Action::Remove => match child.delete() {
            Ok(()) => {
                totals.bytes += size;
                totals.files += 1;
            }
            Err(error) => {
                #[cfg(test)]
                eprintln!("delete failed for {final_path}: {error}");
                totals.skip_error(size, false, &error);
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
    #[test]
    fn a_spotify_leftover_goes_by_handle_and_never_through_a_link() {
        let base = std::env::temp_dir().join(format!("myle-leftovers-{}", uuid::Uuid::new_v4().simple()));
        let profile = base.join("profile");
        let outside = base.join("outside");
        let tree = profile.join(r"AppData\Roaming\Spotify");
        std::fs::create_dir_all(tree.join("Users")).unwrap();
        std::fs::write(tree.join(r"Users\prefs"), b"x").unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("system file"), b"keep me").unwrap();
        let link = |at: &Path| {
            std::process::Command::new(crate::apps::process::system32("cmd.exe"))
                .args(["/d", "/c", "mklink", "/J"])
                .arg(at)
                .arg(&outside)
                .output()
                .unwrap()
                .status
                .success()
        };
        // A junction inside the tree, as a program of the user's could slip in.
        assert!(link(&tree.join(r"Users\swapped")));
        let profile_final = normalized_final_path(OwnedHandle::open_root(&profile).unwrap().0).unwrap();

        let totals = remove_item(&tree, &profile_final).unwrap();
        assert!(!tree.exists(), "the tree is gone: {totals:?}");
        assert_eq!(std::fs::read(outside.join("system file")).unwrap(), b"keep me");

        // A link given as the item itself, and a path outside the profile, are refused.
        let as_item = profile.join("linked");
        assert!(link(&as_item));
        assert_eq!(remove_item(&as_item, &profile_final).unwrap().skipped, 1);
        assert_eq!(remove_item(&outside, &profile_final).unwrap().skipped, 1);
        assert!(outside.join("system file").exists());
        assert_eq!(remove_item(&profile.join("missing"), &profile_final).unwrap(), Totals::default());
        let _ = std::fs::remove_dir(&as_item);
        let _ = std::fs::remove_dir_all(&base);
    }
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
                ..Totals::default()
            }
        );
        let cleaned = visit_target(&target, Action::Clean).unwrap();
        assert_eq!(cleaned.files, 2, "cleaned: {cleaned:?}");
        assert!(root.exists(), "the configured root itself stays");
        assert!(std::fs::read_dir(&root).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn handle_walk_reports_bytes_held_by_an_open_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, target) = temp_target("locked");
        let path = root.join("in-use.tmp");
        std::fs::write(&path, [0; 48]).unwrap();
        let open = std::fs::File::options().read(true).share_mode(0).open(&path).unwrap();

        let cleaned = visit_target(&target, Action::Clean).unwrap();
        assert_eq!((cleaned.skipped, cleaned.skipped_bytes), (1, 48));
        assert_eq!((cleaned.in_use, cleaned.in_use_bytes), (1, 48));
        assert!(path.exists());

        drop(open);
        std::fs::remove_dir_all(root).unwrap();
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
                trees: Vec::new(),
                spotify_registry: false,
            };
            let answer: Response = session.ask(&empty).await.unwrap();
            assert!(matches!(answer, Response::Ok(t) if t.is_empty()));
            let unknown = Request {
                action: Action::Clean,
                ids: vec!["../../etc".into()],
                trees: Vec::new(),
                spotify_registry: false,
            };
            let answer: Response = session.ask(&unknown).await.unwrap();
            assert!(matches!(answer, Response::Error(_)));
            // Closing the pipe (the app exiting) ends the helper. The runtime has to keep running meanwhile: it finishes closing the pipe.
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
