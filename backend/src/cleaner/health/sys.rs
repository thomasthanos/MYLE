// ! Windows plumbing for Software Health: the registry, PowerShell, file ! signatures, the Recycle Bin, and the safety lists.
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub fn root(hive: &str) -> RegKey {
    match hive {
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        _ => RegKey::predef(HKEY_CURRENT_USER),
    }
}
/// Opens `hive\path` for reading, in the 64-bit view.
pub fn open(hive: &str, path: &str) -> Option<RegKey> {
    root(hive)
        .open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
        .ok()
}
pub fn subkeys(key: &RegKey) -> Vec<String> {
    key.enum_keys().flatten().collect()
}
pub fn string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
}
pub fn dword(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}
/// A command with no window; its stdout as text (empty on failure).
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err
        };
        Err(if msg.is_empty() {
            format!("{program} failed")
        } else {
            msg
        })
    }
}
/// A PowerShell script's output (UTF-8).
pub fn powershell(script: &str) -> Result<String, String> {
    let full = format!(
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; $ProgressPreference = 'SilentlyContinue'; {script}"
    );
    let ps = crate::apps::process::powershell();
    let encoded = crate::apps::process::encode_command(&full);
    run(
        &ps.to_string_lossy(),
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ],
    )
}
/// PowerShell's JSON: an array, or one object, or nothing.
pub fn json_list(text: &str) -> Vec<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text.trim()) {
        Ok(serde_json::Value::Array(list)) => list,
        Ok(serde_json::Value::Null) | Err(_) => Vec::new(),
        Ok(one) => vec![one],
    }
}
#[derive(Clone, Debug, Default)]
pub struct Signature {
    pub signed: bool,
    /// The signer's name (the certificate's CN), if signed.
    pub signer: String,
}
/// Authenticode signatures of `paths` (one PowerShell call for all).
pub fn signatures(paths: &[String]) -> HashMap<String, Signature> {
    let mut out = HashMap::new();
    let existing: Vec<&String> = paths
        .iter()
        .filter(|p| Path::new(p.as_str()).is_file())
        .collect();
    if existing.is_empty() {
        return out;
    }
    let list = std::env::temp_dir().join(format!("myle-sig-{}.txt", std::process::id()));
    if std::fs::write(
        &list,
        existing
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .is_err()
    {
        return out;
    }
    let script = format!(
        "Get-Content -LiteralPath {} | ForEach-Object {{ $s = Get-AuthenticodeSignature -LiteralPath $_; [pscustomobject]@{{ p = $_; ok = ($s.Status -eq 'Valid'); who = if ($s.SignerCertificate) {{ $s.SignerCertificate.GetNameInfo('SimpleName', $false) }} else {{ '' }} }} }} | ConvertTo-Json -Compress",
        crate::apps::process::ps_quote(&list.to_string_lossy())
    );
    if let Ok(text) = powershell(&script) {
        for item in json_list(&text) {
            let path = item["p"].as_str().unwrap_or_default().to_string();
            out.insert(
                path.to_lowercase(),
                Signature {
                    signed: item["ok"].as_bool().unwrap_or(false),
                    signer: item["who"].as_str().unwrap_or_default().to_string(),
                },
            );
        }
    }
    let _ = std::fs::remove_file(&list);
    out
}
/// The program a command line starts: `"C:\a b\x.exe" --y` → `C:\a b\x.exe`.
pub fn program_of(command: &str) -> Option<String> {
    let text = expand(command.trim());
    let path = if let Some(rest) = text.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let lower = text.to_lowercase();
        match lower.find(".exe") {
            Some(end) => text[..end + 4].to_string(),
            None => text.split_whitespace().next()?.to_string(),
        }
    };
    Some(path.trim().to_string()).filter(|p| !p.is_empty())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    Missing,
    Unknown,
}
pub fn path_status(path: &Path, directory: bool) -> Presence {
    match std::fs::metadata(path) {
        Ok(meta)
            if if directory {
                meta.is_dir()
            } else {
                meta.is_file()
            } =>
        {
            Presence::Present
        }
        Ok(_) => Presence::Unknown,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Presence::Missing,
        Err(_) => Presence::Unknown,
    }
}
fn program_status_in(command: &str, search: &[PathBuf]) -> Presence {
    let Some(program) = program_of(command) else {
        return Presence::Unknown;
    };
    if program.contains('%') {
        return Presence::Unknown;
    }
    let path = Path::new(&program);
    if path.is_absolute() {
        return path_status(path, false);
    }
    if program.contains(['\\', '/']) {
        return Presence::Unknown;
    }
    let mut unknown = false;
    for dir in search {
        let candidates = if path.extension().is_none() {
            vec![dir.join(&program).with_extension("exe"), dir.join(&program)]
        } else {
            vec![dir.join(&program)]
        };
        for candidate in candidates {
            match path_status(&candidate, false) {
                Presence::Present => return Presence::Present,
                Presence::Unknown => unknown = true,
                Presence::Missing => {}
            }
        }
    }
    if unknown || search.is_empty() {
        Presence::Unknown
    } else {
        Presence::Missing
    }
}
pub fn program_status(command: &str) -> Presence {
    let mut search = Vec::new();
    if let Some(windows) = env_dir("SystemRoot") {
        search.push(windows.join("System32"));
        search.push(windows);
    }
    if let Some(path) = std::env::var_os("PATH") {
        search.extend(std::env::split_paths(&path));
    }
    program_status_in(command, &search)
}
/// `%VAR%` expanded from the environment.
pub fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) if !name.is_empty() => out.push_str(&value),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
/// Lowercase letters and digits only: names compared however written.
pub fn norm(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
/// Windows itself, Microsoft, drivers, hardware vendors and MYLE: never
/// offered for any change.
pub fn protected_name(name: &str) -> bool {
    const NEVER: &[&str] = &[
        "microsoft",
        "windows",
        "msedge",
        "onedrive",
        "xbox",
        "intel",
        "nvidia",
        "amd",
        "realtek",
        "qualcomm",
        "thomasthanos",
        "makeyourlifeeasier",
        "myle",
        "driver",
        "dell",
        "alienware",
        "lenovo",
        "hewlettpackard",
        "asus",
        "synaptics",
        "dolby",
        "waves",
        "conexant",
        "broadcom",
        "mediatek",
        "elan",
        "logitechdrivers",
    ];
    let n = norm(name);
    NEVER.iter().any(|word| n.contains(word))
}
/// A path under the Windows folder (or a driver).
pub fn system_path(path: &str) -> bool {
    let p = path.to_lowercase();
    let windir = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .to_lowercase();
    p.starts_with(&windir) || p.ends_with(".sys") || p.contains(r"\windowsapps\")
}
pub fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}
/// Bytes in a folder (stops after `limit` entries; links not followed).
pub fn dir_size(path: &Path, limit: usize) -> u64 {
    let mut total = 0;
    let mut seen = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > limit {
                return total;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}
/// What a folder holds, for a person deciding about it: how many files (up
/// to `limit` entries looked at; links not followed) and the names of the
/// first few, shallowest first.
pub fn folder_contents(path: &Path, limit: usize, names: usize) -> (usize, Vec<String>) {
    let mut files = 0;
    let mut seen = 0;
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([path.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > limit {
                return (files, found);
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                queue.push_back(entry.path());
            } else {
                files += 1;
                if found.len() < names {
                    found.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    (files, found)
}
/// A folder of a known Windows location (`{6D809377-…}` for Program Files)
/// as UserAssist writes it, resolved to its path.
pub fn known_folder(id: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let hex: String = id.trim_matches(['{', '}']).chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let guid = windows_sys::core::GUID::from_u128(u128::from_str_radix(&hex, 16).ok()?);
    let mut raw = std::ptr::null_mut();
    // SAFETY: `raw` receives a CoTaskMemAlloc'd string, freed below.
    let result = unsafe { SHGetKnownFolderPath(&guid, 0, std::ptr::null_mut(), &mut raw) };
    if raw.is_null() {
        return None;
    }
    let path = (result >= 0).then(|| {
        // SAFETY: a successful call returns a NUL-terminated string.
        let length = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
        PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(raw, length) }))
    });
    // SAFETY: allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}
/// The newest modification inside a folder, two levels deep.
pub fn newest_change(path: &Path) -> Option<SystemTime> {
    let mut newest = std::fs::metadata(path).ok()?.modified().ok();
    let mut stack = vec![(path.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten().take(400) {
            if let Ok(meta) = entry.metadata() {
                if let Ok(m) = meta.modified() {
                    newest = Some(newest.map_or(m, |n| n.max(m)));
                }
                if meta.is_dir() && depth < 2 {
                    stack.push((entry.path(), depth + 1));
                }
            }
        }
    }
    newest
}
pub fn days_since(time: SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(time)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        / 86_400
}
/// Whether this user may remove the folder itself (opened for DELETE, as
/// moving it to the Recycle Bin needs). A folder an installer made as
/// administrator usually lets other users only read it.
pub fn can_delete(path: &Path) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    let mut wide = path.as_os_str().encode_wide_compat();
    wide.push(0);
    // SAFETY: NUL-terminated path; the handle is only opened and closed.
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return false;
        }
        CloseHandle(handle);
    }
    true
}
/// What a failed move to the Recycle Bin means (SHFileOperation's codes).
fn recycle_error(code: i32) -> String {
    match code {
        // DE_ACCESSDENIEDSRC, ERROR_ACCESS_DENIED
        0x78 | 5 => "Only an administrator can move it: run MYLE as administrator for this one.".into(),
        // ERROR_SHARING_VIOLATION: open in another program
        0x20 => "It is open in another program. Close it and try again.".into(),
        // DE_INVALIDFILES, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND
        0x7C | 2 | 3 => "It is no longer there, or its path is too long for the Recycle Bin. Scan again.".into(),
        _ => format!("Some files are in use or could not be moved to the Recycle Bin (code {code})."),
    }
}
/// Moves files or folders to the Recycle Bin (no prompt, no progress).
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    let mut from: Vec<u16> = Vec::new();
    for path in paths {
        if !path.is_absolute() || path.parent().is_none() {
            return Err(format!("Refused: {}", path.display()));
        }
        from.extend(path.as_os_str().encode_wide_compat());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as _,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted != 0 {
        return Err(recycle_error(code));
    }
    Ok(())
}
trait WideCompat {
    fn encode_wide_compat(&self) -> Vec<u16>;
}
impl WideCompat for std::ffi::OsStr {
    fn encode_wide_compat(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}
/// `reg export` of a key to `file` (a backup before it is changed).
pub fn export_key(hive: &str, path: &str, file: &Path) -> Result<(), String> {
    let full = format!("{hive}\\{path}");
    run(
        "reg.exe",
        &["export", &full, &file.to_string_lossy(), "/y", "/reg:64"],
    )
    .map(|_| ())
}
/// Whether a program of this name runs now (`chrome.exe`).
pub fn running(image: &str) -> bool {
    run(
        "tasklist.exe",
        &["/FI", &format!("IMAGENAME eq {image}"), "/NH", "/FO", "CSV"],
    )
    .map(|text| text.to_lowercase().contains(&image.to_lowercase()))
    .unwrap_or(false)
}
#[cfg(test)]
mod executable_tests {
    use super::*;
    #[test]
    fn uninstaller_on_search_path_is_not_missing() {
        let dir = std::env::temp_dir().join(format!("myle-health-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("uninstall.exe"), b"fixture").unwrap();
        assert_eq!(
            program_status_in("uninstall.exe /quiet", std::slice::from_ref(&dir)),
            Presence::Present
        );
        assert_eq!(
            program_status_in("missing.exe", std::slice::from_ref(&dir)),
            Presence::Missing
        );
        assert_eq!(
            program_status_in(
                "%MYLE_UNDEFINED_TEST_VARIABLE%\\app.exe",
                std::slice::from_ref(&dir)
            ),
            Presence::Unknown
        );
        assert_eq!(path_status(&dir, false), Presence::Unknown);
        std::fs::remove_file(dir.join("uninstall.exe")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
