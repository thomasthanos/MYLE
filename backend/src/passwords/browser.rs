//! Filling in logins in Chrome, Edge and Firefox.
//!
//! The browser extension (`extension/`) talks to a "native messaging host"
//! the browser starts: this same program, run by the browser with the
//! extension's id as its argument (`run_native_host`). The host passes each
//! request over a named pipe to the app that is running, which answers from
//! the unlocked vault (`serve`).
//!
//! Who may ask:
//! - the browser starts the host only for our extension (the manifests list
//!   its id), and the host also checks the id it was given and that a
//!   browser started it;
//! - the app answers only a host that is this same program, and the host
//!   only talks to an app that is this same program;
//! - the app answers only while browser filling is on and the vault is
//!   unlocked, gives a password only for an entry saved for that site, and
//!   only for https pages (or this PC's own `localhost`);
//! - it answers only so many requests a minute: plenty for a person
//!   clicking, far too few for a program trying to empty the vault;
//! - look-ups from the browser never keep an unattended vault open; only
//!   what the extension does at the user's click (fill, save, a new
//!   password) counts as using it;
//! - a page can change only the login saved for its own host, never one of
//!   another subdomain (the password it replaces goes into the history).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ServerOptions;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;
use zeroize::Zeroizing;

use super::PasswordsState;
use super::hello::Consent;
use super::passkeys;
use super::vault::{EntryInput, Status};
use myle_vault::sites::saved_host;

/// The native messaging host's name, as the extension asks for it.
pub const HOST_NAME: &str = "com.thomasthanos.myle";
/// The extension's ids in Chrome, Edge and Brave: the Chrome Web Store's,
/// and the one the `key` in its manifest fixes when it is loaded unpacked
/// (the Store's zip leaves the `key` out, so the Store gives its own).
pub const CHROME_EXTENSION_IDS: [&str; 2] = [CHROME_STORE_ID, CHROME_FOLDER_ID];
/// The Chrome Web Store's copy (Chrome, Edge, Brave): it updates itself.
const CHROME_STORE_ID: &str = "mifjffbnaeeljjfboiglbcoaokgdilca";
/// The copy loaded from MYLE's folder ("Load unpacked"), for testing.
const CHROME_FOLDER_ID: &str = "gaelkhdpkgnffkfmaaklknijinjmmopo";
/// Its id in Firefox (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "myle-passwords@thomast.uk";
/// Browsers the host may be started by: program file name, and what to call it.
const BROWSERS: [(&str, &str); 4] = [
    ("chrome.exe", "Chrome"),
    ("msedge.exe", "Edge"),
    ("firefox.exe", "Firefox"),
    ("brave.exe", "Brave"),
];
/// Where each browser looks for native messaging hosts, the manifest it
/// reads, and the browser.
const HOST_KEYS: [(&str, &str, &str); 4] = [
    (r"Software\Google\Chrome\NativeMessagingHosts", "chrome.json", "Chrome"),
    (r"Software\Microsoft\Edge\NativeMessagingHosts", "chrome.json", "Edge"),
    (r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts", "chrome.json", "Brave"),
    (r"Software\Mozilla\NativeMessagingHosts", "firefox.json", "Firefox"),
];
/// Refused starts of the host kept in `host.log`, newest last.
const REFUSALS_KEPT: usize = 10;
/// A native message larger than this is not ours (a screenshot of a tab,
/// for its 2FA QR code, is the largest).
const MAX_MESSAGE: u32 = 8 * 1024 * 1024;
/// Website icons sent with a site's logins: each small, and all together
/// well under the browser's 1 MB limit for an answer.
const MAX_ICON: usize = 48 * 1024;
const ICONS_PER_ANSWER: usize = 384 * 1024;
/// Requests a minute that give out, check or change a password, and
/// look-ups of which logins a site has.
const SENSITIVE_PER_MINUTE: usize = 20;
const LOOKUPS_PER_MINUTE: usize = 120;
static SENSITIVE: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());
static LOOKUPS: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());

/// Counts a request against `bucket`; false when the minute's share is used.
fn allow(bucket: &Mutex<VecDeque<Instant>>, per_minute: usize) -> bool {
    let mut times = bucket.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    while times
        .front()
        .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60))
    {
        times.pop_front();
    }
    if times.len() >= per_minute {
        return false;
    }
    times.push_back(now);
    true
}

// ---------------------------------------------------------------------------
// Registration

fn manifest_dir() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join("native-messaging"))
}

/// What the browsers are told about the host: a registry key per browser,
/// pointing at a manifest that names this program.
struct Hosts {
    /// Put before every registry key: empty, except in tests.
    root: String,
    dir: PathBuf,
    exe: PathBuf,
}

impl Hosts {
    fn here() -> Result<Self, String> {
        Ok(Self {
            root: String::new(),
            dir: manifest_dir()?,
            exe: std::env::current_exe().map_err(|e| e.to_string())?,
        })
    }

    fn key(&self, browsers: &str) -> String {
        format!(r"{}{browsers}\{HOST_NAME}", self.root)
    }

    /// Each manifest's file name and text.
    fn manifests(&self) -> Result<[(&'static str, String); 2], String> {
        let description = "MYLE: fills in logins from your password vault";
        let chrome = json!({
            "name": HOST_NAME,
            "description": description,
            "path": self.exe,
            "type": "stdio",
            "allowed_origins": CHROME_EXTENSION_IDS.map(|id| format!("chrome-extension://{id}/")),
        });
        let firefox = json!({
            "name": HOST_NAME,
            "description": description,
            "path": self.exe,
            "type": "stdio",
            "allowed_extensions": [FIREFOX_EXTENSION_ID],
        });
        let text = |manifest: &Value| serde_json::to_string_pretty(manifest).map_err(|e| e.to_string());
        Ok([("chrome.json", text(&chrome)?), ("firefox.json", text(&firefox)?)])
    }

    fn write(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        for (name, text) in self.manifests()? {
            std::fs::write(self.dir.join(name), text).map_err(|e| e.to_string())?;
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, file, _) in HOST_KEYS {
            let (host, _) = hkcu.create_subkey(self.key(key)).map_err(|e| e.to_string())?;
            host.set_value("", &self.dir.join(file).to_string_lossy().into_owned())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Why a browser would not start this program as the host, if one would not.
    fn check(&self) -> Result<(), String> {
        let manifests = self.manifests()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, file, browser) in HOST_KEYS {
            let registered: String = hkcu
                .open_subkey(self.key(key))
                .and_then(|host| host.get_value(""))
                .map_err(|_| format!("{browser} has not been told where MYLE is."))?;
            let manifest = self.dir.join(file);
            if !registered.eq_ignore_ascii_case(&manifest.to_string_lossy()) {
                return Err(format!("{browser} looks for MYLE in another place."));
            }
            let expected = manifests.iter().find(|(name, _)| *name == file).map(|(_, text)| text);
            match std::fs::read_to_string(&manifest) {
                Ok(text) if Some(&text) == expected => {}
                Ok(_) => return Err(format!("{browser} would start another copy of MYLE.")),
                Err(_) => return Err(format!("The note that tells {browser} where MYLE is, is missing.")),
            }
        }
        Ok(())
    }

    fn remove(&self) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for (key, _, _) in HOST_KEYS {
            let _ = hkcu.delete_subkey_all(self.key(key));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Tells Chrome, Edge, Brave and Firefox where the host is, unless they
/// already know: checked on every start, unlock and look at the setup, so a
/// moved program or a deleted manifest mends itself.
pub fn ensure_registered() -> Result<(), String> {
    let hosts = Hosts::here()?;
    if hosts.check().is_ok() {
        return Ok(());
    }
    hosts.write()?;
    hosts.check()
}

/// Writes the registration again, whatever is there now.
pub fn register_hosts() -> Result<(), String> {
    let hosts = Hosts::here()?;
    hosts.write()?;
    hosts.check()
}

/// Browser filling switched off: the browsers forget the host.
pub fn unregister_hosts() {
    if let Ok(hosts) = Hosts::here() {
        hosts.remove();
    }
}

// ---------------------------------------------------------------------------
// Diagnostics: which browser last asked, and whom the host turned away. No
// addresses or logins, only the browser and when.

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub browser: String,
    /// Seconds since 1970.
    pub at: u64,
    /// Which copy of the extension: "store", "folder" or "firefox".
    #[serde(default)]
    pub copy: String,
    /// The extension's own version, when it says (1.5.1 and later).
    #[serde(default)]
    pub version: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The program that started the host (`vivaldi.exe`), if known.
    pub program: String,
    pub reason: String,
    pub at: u64,
}

/// Sent to the page when a browser asks for the first time in a while.
pub const CONTACT_EVENT: &str = "passwords-browser-contact";
static LAST_CONTACT: Mutex<Option<Contact>> = Mutex::new(None);

/// Notes a request from `browser`; true for the first in a minute (kept on
/// disk too, so the setup still knows after a restart).
fn note_contact(browser: &str, copy: &str, version: &str) -> bool {
    let now = super::vault::now();
    let mut last = LAST_CONTACT.lock().unwrap_or_else(|p| p.into_inner());
    let recent = last
        .as_ref()
        .is_some_and(|c| c.browser == browser && c.copy == copy && c.version == version && now.saturating_sub(c.at) < 60);
    let contact = Contact { browser: browser.to_string(), at: now, copy: copy.to_string(), version: version.to_string() };
    *last = Some(contact.clone());
    if !recent && let Ok(dir) = manifest_dir() {
        let _ = std::fs::write(dir.join("contact.json"), serde_json::to_vec(&contact).unwrap_or_default());
    }
    !recent
}

pub fn last_contact() -> Option<Contact> {
    if let Some(contact) = LAST_CONTACT.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return Some(contact);
    }
    let text = std::fs::read(manifest_dir().ok()?.join("contact.json")).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Called by the host when it turns a start away.
fn note_refusal(program: &str, reason: &str) {
    let Ok(dir) = manifest_dir() else { return };
    let path = dir.join("host.log");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().map(str::to_string).collect();
    lines.push(format!("{}\t{program}\t{reason}", super::vault::now()));
    let keep = lines.len().saturating_sub(REFUSALS_KEPT);
    let _ = std::fs::write(&path, lines[keep..].join("\n"));
}

pub fn last_refusal() -> Option<Refusal> {
    let text = std::fs::read_to_string(manifest_dir().ok()?.join("host.log")).ok()?;
    parse_refusal(text.lines().last()?)
}

fn parse_refusal(line: &str) -> Option<Refusal> {
    let mut parts = line.splitn(3, '\t');
    let at = parts.next()?.parse().ok()?;
    Some(Refusal {
        program: parts.next()?.to_string(),
        reason: parts.next()?.to_string(),
        at,
    })
}

// ---------------------------------------------------------------------------
// Processes

/// The pipe both sides meet at, one per Windows user.
fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let digest = Sha256::digest(format!("{domain}\\{user}").to_lowercase().as_bytes());
    let id: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!(r"\\.\pipe\myle-passwords-{id}")
}

pub(super) fn process_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

/// Whether `pid` runs this very program.
fn is_this_program(pid: u32) -> bool {
    match (process_path(pid), std::env::current_exe()) {
        (Some(other), Ok(mine)) => same_file(&other, &mine),
        _ => false,
    }
}

/// This process's parent, grandparent and so on (Chrome starts the host
/// through `cmd.exe`), as program file names, lowercase.
fn ancestors() -> Vec<String> {
    let mut parents = std::collections::HashMap::new();
    let mut names = std::collections::HashMap::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            names.insert(entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    let mut chain = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..4 {
        let Some(&parent) = parents.get(&pid) else { break };
        let Some(name) = names.get(&parent) else { break };
        chain.push(name.clone());
        pid = parent;
    }
    chain
}

// ---------------------------------------------------------------------------
// The host, started by the browser

/// `Some(exit code)` when a browser started this program as the extension's
/// native messaging host; `None` for a normal start.
pub fn run_native_host() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let asked = args.iter().any(|a| a.starts_with("chrome-extension://"))
        || args.iter().any(|a| a.ends_with(".json")) && args.iter().any(|a| a.contains('@'));
    if !asked {
        return None;
    }
    let chain = ancestors();
    let browser = chain.iter().find_map(|name| browser_name(name));
    let starter = chain.iter().find(|name| *name != "cmd.exe").map_or("unknown", String::as_str);
    let Some(copy) = which_copy(&args) else {
        note_refusal(starter, "another extension");
        return Some(3);
    };
    let Some(browser) = browser else {
        note_refusal(starter, "not a supported browser");
        return Some(3);
    };
    Some(match host_loop(browser, copy) {
        Ok(()) => 0,
        Err(_) => 2,
    })
}

/// What to call a browser, from its program file name; `None` for any
/// other program.
fn browser_name(program: &str) -> Option<&'static str> {
    BROWSERS
        .iter()
        .find(|(file, _)| program.eq_ignore_ascii_case(file))
        .map(|(_, name)| *name)
}

/// Which copy of our extension started the host, if one did. Chrome and
/// Edge pass the extension's origin, sometimes without the trailing slash;
/// Firefox the path of the manifest and the extension's id.
fn which_copy(args: &[String]) -> Option<&'static str> {
    args.iter().find_map(|arg| {
        if arg == FIREFOX_EXTENSION_ID {
            return Some("firefox");
        }
        let id = arg.strip_suffix('/').unwrap_or(arg).strip_prefix("chrome-extension://")?;
        match id {
            CHROME_STORE_ID => Some("store"),
            CHROME_FOLDER_ID => Some("folder"),
            _ => None,
        }
    })
}

#[cfg(test)]
fn started_by_our_extension(args: &[String]) -> bool {
    which_copy(args).is_some()
}

fn read_message(input: &mut impl Read) -> std::io::Result<Option<Value>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let length = u32::from_le_bytes(length);
    if length > MAX_MESSAGE {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut body = Zeroizing::new(vec![0u8; length as usize]);
    input.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?))
}

fn write_message(output: &mut impl Write, value: &Value) -> std::io::Result<()> {
    let body = Zeroizing::new(serde_json::to_vec(value)?);
    output.write_all(&(body.len() as u32).to_le_bytes())?;
    output.write_all(&body)?;
    output.flush()
}

/// How long the host waits for the app it started to answer, before it tells
/// the browser the app is not running after all.
const APP_START_WAIT: Duration = Duration::from_secs(20);
/// How often it looks for the app's pipe while it waits.
const APP_START_POLL: Duration = Duration::from_millis(250);

/// Passes each message from the browser to the app and back, saying which
/// browser it came from.
fn host_loop(browser: &str, copy: &str) -> std::io::Result<()> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    while let Some(message) = read_message(&mut input)? {
        // An open-and-close availability probe consumes the listening pipe
        // instance. Send the request on our first connection instead.
        let request = request_from(browser, copy, &message);
        let reply = match ask_app(&request) {
            Ok(reply) => reply,
            Err(AppError::Failed) => lost_app_reply(),
            Err(AppError::Unavailable) if is_open(&message) => {
                // The extension's own "open": start it (it shows the vault).
                let how = launch_app(browser, copy);
                json!({ "ok": how.is_some(), "detail": how.unwrap_or("could not start MYLE") })
            }
            Err(AppError::Unavailable) if !wants_the_app(&message) => {
                // A page's status poll or logins never starts the app.
                json!({ "ok": false, "error": "notRunning" })
            }
            Err(AppError::Unavailable) => {
                // No connection was made: start the app and wait until this
                // request can be dispatched. A lost answer is never retried.
                let how = launch_app(browser, copy);
                let answer = how.map_or(Err(AppError::Unavailable), |_| wait_for_app(&request, APP_START_WAIT));
                match answer {
                    Ok(answer) => answer,
                    Err(AppError::Failed) => lost_app_reply(),
                    Err(AppError::Unavailable) => json!({
                        "ok": false,
                        "error": "notRunning",
                        "detail": match how {
                            Some(how) => format!("{how}; MYLE did not answer in {}s", APP_START_WAIT.as_secs()),
                            None => "could not start MYLE".to_owned(),
                        },
                    }),
                }
            }
        };
        write_message(&mut output, &reply)?;
    }
    Ok(())
}

fn request_from(browser: &str, copy: &str, message: &Value) -> Value {
    let version = message.get("extVersion").and_then(Value::as_str).unwrap_or("");
    json!({ "browser": browser, "copy": copy, "version": version, "request": message })
}

/// Whether the host starts MYLE for this request when it is closed.
///
/// Only for passkeys. A passkey request means the user has just started a
/// sign-in with one, or a sign-in page has offered them (conditional
/// mediation): either way MYLE is what they are reaching for, and starting it
/// is what makes the click carry on instead of hearing "not running". Filling
/// in a login is not started here: the extension's menu has its own Open MYLE
/// button, and a page must not bring the app up by itself.
fn wants_the_app(message: &Value) -> bool {
    match message.get("type").and_then(Value::as_str) {
        Some("passkeyList") => message.get("wake").and_then(Value::as_bool) == Some(true),
        Some("passkeyGet") | Some("passkeyCreate") => true,
        _ => false,
    }
}

fn is_open(message: &Value) -> bool {
    message.get("type").and_then(Value::as_str) == Some("open")
}

/// The flag MYLE starts with to open on the Password Manager and ask to be
/// unlocked; also what the `myle:` link starts it with.
pub const OPEN_FLAG: &str = "--open-passwords";
/// MYLE's own link scheme (`myle://passwords`), registered for this user.
const PROTOCOL: &str = "myle";

/// Starts MYLE on the Password Manager page, and says how.
///
/// A program the browser starts lives in the browser's job: what it starts
/// in turn can be ended with it, and it holds the browser's pipes. So the
/// app is started on its own (no console, no inherited handles, out of the
/// job when the job allows it), and if it is still not up after a moment,
/// through Windows' shell with MYLE's `myle:` link, which starts it from
/// Explorer like a click on its shortcut.
fn launch_app(browser: &str, copy: &str) -> Option<&'static str> {
    if spawn_detached() {
        let status = request_from(browser, copy, &json!({ "type": "status" }));
        if wait_for_app(&status, Duration::from_secs(6)).is_ok() {
            return Some("started");
        }
        if open_link() {
            return Some("started, then asked Windows to open MYLE");
        }
        return Some("started");
    }
    open_link().then_some("asked Windows to open MYLE")
}

fn spawn_detached() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let start = |flags: u32| {
        Command::new(&exe)
            .arg(OPEN_FLAG)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
            .is_ok()
    };
    // Breaking away is refused in a job that does not allow it: then as is.
    start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB)
        || start(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
}

/// Opens `myle://passwords` through Explorer: the copy it starts is
/// Explorer's, not the browser's.
fn open_link() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    if register_protocol().is_err() {
        return false;
    }
    let windows = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    Command::new(windows.join("explorer.exe"))
        .arg(format!("{PROTOCOL}://passwords"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0100_0000 | 0x0000_0008)
        .spawn()
        .or_else(|_| {
            Command::new(windows.join("explorer.exe"))
                .arg(format!("{PROTOCOL}://passwords"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })
        .is_ok()
}

/// `myle:` links start this program on the Password Manager (for this user
/// only). Whatever the link says after the scheme is not passed on.
pub fn register_protocol() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = format!("\"{}\" {OPEN_FLAG}", exe.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROTOCOL}")).map_err(|e| e.to_string())?;
    let current: Option<String> = key.open_subkey(r"shell\open\command").and_then(|k| k.get_value("")).ok();
    if current.as_deref() == Some(command.as_str()) {
        return Ok(());
    }
    key.set_value("", &"URL:MYLE").map_err(|e| e.to_string())?;
    key.set_value("URL Protocol", &"").map_err(|e| e.to_string())?;
    let (open, _) = key.create_subkey(r"shell\open\command").map_err(|e| e.to_string())?;
    open.set_value("", &command).map_err(|e| e.to_string())
}

/// Waits, up to `limit`, for the app to answer on its pipe. Milliseconds
/// count: the app's window and vault come up in a couple of seconds.
fn wait_for_app(message: &Value, limit: Duration) -> Result<Value, AppError> {
    let until = Instant::now() + limit;
    loop {
        match ask_app(message) {
            Err(AppError::Unavailable) if Instant::now() < until => {
                std::thread::sleep(APP_START_POLL);
            }
            // The app may have committed a request with a lost answer.
            // Retry only while no connection has been established.
            answer => return answer,
        }
    }
}

fn open_app_pipe(name: &str, limit: Duration) -> std::io::Result<std::fs::File> {
    let until = Instant::now() + limit;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            // The app accepts an instance and replenishes its listener on
            // another task. A simultaneous request can reach that short gap.
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && Instant::now() < until => {
                std::thread::sleep(until.saturating_duration_since(Instant::now()).min(Duration::from_millis(10)));
            }
            result => return result,
        }
    }
}

/// One request, distinguishing unavailable connection from a lost answer.
fn ask_app(message: &Value) -> Result<Value, AppError> {
    ask_app_on(&pipe_name(), message)
}

#[derive(Debug, PartialEq, Eq)]
enum AppError {
    Unavailable,
    Failed,
}

fn ask_app_on(name: &str, message: &Value) -> Result<Value, AppError> {
    let pipe = open_app_pipe(name, Duration::from_secs(2)).map_err(|_| AppError::Unavailable)?;
    let mut server = 0u32;
    let ok = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut server) };
    if ok == 0 || !is_this_program(server) {
        return Err(AppError::Failed);
    }
    let mut writer = pipe.try_clone().map_err(|_| AppError::Failed)?;
    let mut line = Zeroizing::new(serde_json::to_string(message).map_err(|_| AppError::Failed)?);
    line.push('\n');
    writer.write_all(line.as_bytes()).map_err(|_| AppError::Failed)?;
    writer.flush().map_err(|_| AppError::Failed)?;
    let mut answer = Zeroizing::new(String::new());
    BufReader::new(pipe).read_line(&mut answer).map_err(|_| AppError::Failed)?;
    serde_json::from_str(answer.trim()).map_err(|_| AppError::Failed)
}

fn lost_app_reply() -> Value {
    json!({ "ok": false, "error": "hostExited", "detail": "MYLE's connection closed without an answer; the request was not retried" })
}

// ---------------------------------------------------------------------------
// The app's side

async fn browser_listener(name: &str) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    loop {
        match ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(name) {
            Ok(server) => return Ok(server),
            // A live update starts this copy while the previous one still
            // owns the pipe. Keep exclusive ownership and take over once it
            // exits, instead of disabling browser filling for this whole run.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                tokio::time::sleep(APP_START_POLL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Answers the host while the app runs. `filling`: browser filling is on.
pub fn serve(app: AppHandle, state: PasswordsState, filling: bool) {
    tauri::async_runtime::spawn(async move {
        let name = pipe_name();
        let Ok(mut server) = browser_listener(&name).await else {
            // A creation error other than the previous copy holding the pipe.
            return;
        };
        // This copy answers: the browsers start this program as the host (or,
        // with filling off, forget an old registration).
        tauri::async_runtime::spawn_blocking(move || {
            if filling {
                let _ = ensure_registered();
                let _ = register_protocol();
            } else {
                unregister_hosts();
            }
        });
        loop {
            if server.connect().await.is_err() {
                continue;
            }
            let client = server;
            server = match ServerOptions::new().reject_remote_clients(true).create(&name) {
                Ok(next) => next,
                Err(_) => return,
            };
            let (app, state) = (app.clone(), state.clone());
            tauri::async_runtime::spawn(async move {
                let mut pid = 0u32;
                let ok = unsafe { GetNamedPipeClientProcessId(client.as_raw_handle() as HANDLE, &mut pid) };
                if ok == 0 || !is_this_program(pid) {
                    return;
                }
                let mut client = tokio::io::BufReader::new(client);
                let mut line = Zeroizing::new(String::new());
                if client.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let reply = match serde_json::from_str::<Envelope>(line.trim()) {
                    Ok(Envelope { browser, copy, version, request }) => {
                        if note_contact(&browser, &copy, &version) {
                            let _ = app.emit(CONTACT_EVENT, last_contact());
                        }
                        if request.is_passkey() {
                            passkey_reply(&app, &state, &browser, &copy, request)
                                .await
                                .unwrap_or_else(|error| json!({ "ok": false, "error": error }))
                        } else {
                            answer(&app, &state, request)
                        }
                    }
                    Err(_) => json!({ "ok": false, "error": "badRequest" }),
                };
                let mut out = Zeroizing::new(reply.to_string());
                out.push('\n');
                let _ = client.get_mut().write_all(out.as_bytes()).await;
                let _ = client.get_mut().flush().await;
            });
        }
    });
}

/// What the host passes on: the browser's request, and which browser.
#[derive(Deserialize)]
struct Envelope {
    browser: String,
    #[serde(default)]
    copy: String,
    #[serde(default)]
    version: String,
    request: Request,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Request {
    Status,
    Open,
    Logins { url: String },
    Fill { id: String, url: String },
    /// The login's 2FA code, for the code field the user clicked.
    Totp { id: String, url: String },
    /// A 2FA key a site showed, kept at the user's click: for login `id`,
    /// or as a new login of its own.
    SaveTotp { url: String, id: Option<String>, secret: String },
    /// The 2FA QR code in a screenshot of the tab (a PNG, base64), from the
    /// toolbar popup.
    TotpFromImage { image: String },
    /// Whether a login of the site already has this 2FA key (so a setup page
    /// showing it again is not offered).
    TotpKnown { url: String, secret: String },
    /// A picture on a page (grey levels, base64), read for a 2FA QR code.
    TotpFromPixels { width: usize, height: usize, pixels: String },
    Known { url: String, username: String, password: String },
    Save { url: String, username: String, password: String },
    /// A login's user name, password or 2FA code onto the clipboard (marked
    /// secret and cleared after 30 seconds), from the toolbar popup at the
    /// user's click. The text itself never goes back to the browser.
    Copy { id: String, url: String, field: String },
    /// A strong new password, for a sign-up or password-change form.
    Generate,
    /// The sites (relying parties) the vault has passkeys for, never the
    /// passkeys: the extension remembers them (salted and hashed) so that a
    /// locked or closed MYLE can still be offered on those sites.
    PasskeySites,
    /// The passkeys a page may use (never their keys): for MYLE's prompt.
    PasskeyList {
        url: String,
        rp_id: Option<String>,
        #[serde(default)]
        allow: Vec<String>,
        /// The site's own request waits on it (not a menu on a field): a
        /// closed MYLE is started, a locked one asks to be unlocked.
        #[serde(default)]
        wake: bool,
    },
    /// A new passkey, for the page's `navigator.credentials.create()`.
    PasskeyCreate {
        #[serde(default)]
        operation_id: Option<String>,
        url: String,
        rp_id: Option<String>,
        #[serde(default)]
        rp_name: String,
        user_id: String,
        #[serde(default)]
        user_name: String,
        #[serde(default)]
        user_display_name: String,
        challenge: String,
        #[serde(default)]
        algorithms: Vec<i64>,
        #[serde(default)]
        exclude: Vec<String>,
        #[serde(default)]
        user_verification: String,
    },
    /// Signing in with a passkey, for the page's `navigator.credentials.get()`.
    PasskeyGet {
        #[serde(default)]
        operation_id: Option<String>,
        url: String,
        rp_id: Option<String>,
        challenge: String,
        credential_id: String,
        #[serde(default)]
        user_verification: String,
    },
    /// Withdraws this browser copy's operation for exactly this origin.
    PasskeyCancel { url: String, operation_id: String },
}

impl Request {
    fn is_passkey(&self) -> bool {
        matches!(self, Self::PasskeyList { .. } | Self::PasskeyCreate { .. } | Self::PasskeyGet { .. } | Self::PasskeyCancel { .. })
    }
}

#[derive(Serialize)]
struct Login {
    id: String,
    title: String,
    username: String,
    /// Saved for this exact host (else for the same site, another subdomain).
    exact: bool,
    /// The host it was saved for, shown when it is not the page's own.
    site: String,
    /// The website's icon from the vault's cache (a `data:` URL), if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    /// It has a 2FA key: its code can be filled in.
    totp: bool,
}

/// The page's host, if it is one we fill: https, or http on this PC.
fn page_host(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.trim_end_matches('.').to_lowercase();
    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    (parsed.scheme() == "https" || parsed.scheme() == "http" && local).then_some(host)
}

/// The part of a host that one owner controls (`accounts.google.com` →
/// `google.com`, `a.b.co.uk` → `b.co.uk`), from the Public Suffix List.
fn site(host: &str) -> String {
    psl::domain_str(host).unwrap_or(host).to_string()
}

/// How well an entry's saved address fits the page: `Some(true)` the same
/// host, `Some(false)` the same site, `None` another site.
fn fits(saved: &str, page: &str) -> Option<bool> {
    let saved = saved_host(saved)?;
    let page_bare = page.trim_start_matches("www.");
    if saved == page_bare {
        Some(true)
    } else if site(&saved) == site(page_bare) {
        Some(false)
    } else {
        None
    }
}

fn answer(app: &AppHandle, state: &PasswordsState, request: Request) -> Value {
    if let Request::Open = request {
        let locked = state.with_quiet(|vault| Ok(vault.status())).is_ok_and(|status| status == Status::Locked);
        if locked {
            ask_to_unlock(app);
        } else {
            open_vault(app);
        }
        return json!({ "ok": true });
    }
    let enabled = state.with_quiet(|vault| Ok(vault.prefs().browser_filling)).unwrap_or(false);
    let status = state.with_quiet(|vault| Ok(vault.status())).unwrap_or(Status::New);
    if let Request::Status = request {
        return json!({ "ok": true, "enabled": enabled, "state": status });
    }
    if !enabled {
        return json!({ "ok": false, "error": "disabled" });
    }
    let within_limit = match request {
        Request::Logins { .. } | Request::TotpKnown { .. } | Request::TotpFromPixels { .. } | Request::PasskeySites => {
            allow(&LOOKUPS, LOOKUPS_PER_MINUTE)
        }
        _ => allow(&SENSITIVE, SENSITIVE_PER_MINUTE),
    };
    if !within_limit {
        return json!({ "ok": false, "error": "busy" });
    }
    if let Request::TotpFromPixels { width, height, pixels } = &request {
        return match qr_in_pixels(*width, *height, pixels) {
            Some(link) => json!({ "ok": true, "link": link }),
            None => json!({ "ok": false, "error": "noQr" }),
        };
    }
    if let Request::TotpFromImage { image } = &request {
        // Decoded here and given back for the popup to show; kept only when
        // the user then picks the login for it.
        return match qr_in_screenshot(image) {
            Ok(link) => {
                let info = super::totp::Totp::parse(&link).map(|key| key.info()).ok();
                json!({ "ok": true, "link": link, "info": info })
            }
            Err(error) => json!({ "ok": false, "error": "noQr", "detail": error }),
        };
    }
    if let Request::Generate = request {
        // Asked at the user's click on a sign-up form: keep the vault open
        // until the form is sent and the new login can be saved.
        let _ = state.with_quiet(|vault| {
            vault.touch();
            Ok(())
        });
        let options = super::generator::Options {
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            avoid_ambiguous: true,
            ..Default::default()
        };
        return match super::generator::generate(&options) {
            Ok(password) => json!({ "ok": true, "password": password.as_str() }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
    }
    if status != Status::Unlocked {
        return json!({ "ok": false, "error": if status == Status::New { "noVault" } else { "locked" } });
    }
    if let Request::Copy { id, url, field } = &request {
        // Read under the vault's lock; copied after it, so the clipboard's
        // retries never hold the vault.
        let text = state.with_quiet(|vault| copy_text(vault, id, url, field));
        return match text.and_then(|text| super::clipboard::copy_secret(&text)) {
            Ok(()) => json!({ "ok": true }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
    }
    let result = state.with_quiet(|vault| match &request {
        Request::Logins { url } => {
            let host = page_host(url).ok_or("insecure")?;
            let mut logins: Vec<Login> = vault
                .summaries()?
                .into_iter()
                .filter_map(|entry| {
                    let (exact, saved) = entry
                        .urls
                        .iter()
                        .filter_map(|u| fits(u, &host).map(|exact| (exact, u)))
                        .max_by_key(|(exact, _)| *exact)?;
                    Some(Login {
                        site: saved_host(saved).unwrap_or_default(),
                        id: entry.id,
                        title: entry.title,
                        username: entry.username,
                        exact,
                        icon: None,
                        totp: entry.has_totp,
                    })
                })
                .collect();
            logins.sort_by(|a, b| {
                b.exact
                    .cmp(&a.exact)
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                    .then_with(|| a.username.to_lowercase().cmp(&b.username.to_lowercase()))
            });
            // Only what the app already has: the browser never makes it fetch.
            if vault.prefs().website_icons {
                let cache = vault.icons()?;
                let mut budget = ICONS_PER_ANSWER;
                for login in &mut logins {
                    let icon = super::icons::icon_host(&login.site)
                        .and_then(|host| cache.icons.get(&host))
                        .and_then(|cached| cached.data.as_ref())
                        .filter(|data| data.len() <= MAX_ICON.min(budget));
                    if let Some(icon) = icon {
                        budget -= icon.len();
                        login.icon = Some(icon.clone());
                    }
                }
            }
            Ok(json!({ "ok": true, "logins": logins }))
        }
        Request::PasskeySites => {
            let mut sites: Vec<String> = vault
                .summaries()?
                .into_iter()
                .flat_map(|entry| entry.passkeys.into_iter().map(|key| key.rp_id.trim().trim_end_matches('.').to_lowercase()))
                .filter(|site| !site.is_empty())
                .collect();
            sites.sort();
            sites.dedup();
            Ok(json!({ "ok": true, "sites": sites }))
        }
        Request::Fill { id, url } => {
            let host = page_host(url).ok_or("insecure")?;
            let entry = vault
                .summaries()?
                .into_iter()
                .find(|e| e.id == *id)
                .ok_or("notFound")?;
            // Only for the site it was saved for, whatever the page asks.
            if !entry.urls.iter().any(|u| fits(u, &host).is_some()) {
                return Err("wrongSite".to_string());
            }
            let password = vault.password(id)?;
            vault.touch();
            Ok(json!({ "ok": true, "username": entry.username, "password": password.as_str() }))
        }
        Request::TotpKnown { url, secret } => {
            let host = page_host(url).ok_or("insecure")?;
            let Ok(key) = super::totp::Totp::parse(secret) else {
                return Ok(json!({ "ok": true, "known": false }));
            };
            let mut known = false;
            for entry in vault.summaries()? {
                if entry.has_totp
                    && entry.urls.iter().any(|u| fits(u, &host).is_some())
                    && vault.totp(&entry.id).is_ok_and(|saved| saved.same_key(&key))
                {
                    known = true;
                    break;
                }
            }
            Ok(json!({ "ok": true, "known": known }))
        }
        Request::SaveTotp { url, id, secret } => {
            let host = page_host(url).ok_or("insecure")?;
            let key = super::totp::Totp::parse(secret).map_err(|_| "badKey".to_string())?;
            let saved = match id {
                Some(id) => {
                    let entry = vault
                        .summaries()?
                        .into_iter()
                        .find(|e| e.id == *id)
                        .ok_or("notFound")?;
                    // A login of this site; one that has a key already is
                    // changed only from its own host, as with a password.
                    let fit = entry.urls.iter().filter_map(|u| fits(u, &host)).max().ok_or("wrongSite")?;
                    if entry.has_totp && !fit {
                        return Err("wrongSite".to_string());
                    }
                    vault.set_totp(id, &key)?;
                    json!({ "ok": true, "id": id, "replaced": entry.has_totp })
                }
                None => {
                    let title = host.trim_start_matches("www.");
                    let id = vault.add_totp_login(title, &format!("https://{host}"), &key)?;
                    json!({ "ok": true, "id": id, "replaced": false })
                }
            };
            vault.touch();
            Ok(saved)
        }
        Request::Totp { id, url } => {
            let host = page_host(url).ok_or("insecure")?;
            let entry = vault
                .summaries()?
                .into_iter()
                .find(|e| e.id == *id)
                .ok_or("notFound")?;
            // The same rule as a password: only on the site it is saved for.
            if !entry.urls.iter().any(|u| fits(u, &host).is_some()) {
                return Err("wrongSite".to_string());
            }
            let code = vault.totp(id).map_err(|_| "noTotp".to_string())?.now();
            vault.touch();
            Ok(json!({ "ok": true, "code": code.code, "remaining": code.remaining }))
        }
        Request::Known { url, username, password } => {
            let host = page_host(url).ok_or("insecure")?;
            let same = same_login(vault, &host, username)?;
            let mut known = false;
            for id in &same.same_site {
                if vault.password(id)?.as_str() == password {
                    known = true;
                    break;
                }
            }
            Ok(json!({ "ok": true, "known": known, "update": same.exact.is_some() && !known }))
        }
        Request::Save { url, username, password } => {
            let host = page_host(url).ok_or("insecure")?;
            if password.is_empty() {
                return Err("empty".into());
            }
            let existing = same_login(vault, &host, username)?.exact;
            let input = match &existing {
                Some(id) => {
                    let e = vault.summaries()?.into_iter().find(|s| s.id == *id).ok_or("notFound")?;
                    EntryInput {
                        id: Some(id.clone()),
                        title: e.title,
                        username: e.username,
                        password: Some(password.clone()),
                        urls: e.urls,
                        apps: e.apps,
                        notes: e.notes,
                        favorite: e.favorite,
                        folder: e.folder,
                        totp: None,
                    }
                }
                None => EntryInput {
                    id: None,
                    title: host.trim_start_matches("www.").to_string(),
                    username: username.clone(),
                    password: Some(password.clone()),
                    urls: vec![format!("https://{host}")],
                    apps: Vec::new(),
                    notes: String::new(),
                    favorite: false,
                    folder: String::new(),
                    totp: None,
                },
            };
            vault.save(&input)?;
            vault.touch();
            Ok(json!({ "ok": true, "updated": existing.is_some() }))
        }
        Request::Status
        | Request::Open
        | Request::Copy { .. }
        | Request::Generate
        | Request::TotpFromImage { .. }
        | Request::TotpFromPixels { .. } => unreachable!("answered above"),
        Request::PasskeyList { .. } | Request::PasskeyCreate { .. } | Request::PasskeyGet { .. } | Request::PasskeyCancel { .. } => {
            unreachable!("answered by passkey_reply")
        }
    });
    if matches!(request, Request::Save { .. } | Request::SaveTotp { .. }) && result.is_ok() {
        let _ = app.emit(super::CHANGED_EVENT, ());
    }
    result.unwrap_or_else(|error| json!({ "ok": false, "error": error }))
}

/// What `Request::Copy` puts on the clipboard: only for a login saved for
/// the page's site, by the same rule as filling it.
fn copy_text(vault: &mut super::vault::Vault, id: &str, url: &str, field: &str) -> Result<Zeroizing<String>, String> {
    let host = page_host(url).ok_or("insecure")?;
    let entry = vault
        .summaries()?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("notFound")?;
    if !entry.urls.iter().any(|u| fits(u, &host).is_some()) {
        return Err("wrongSite".to_string());
    }
    let text = match field {
        "username" if !entry.username.is_empty() => Zeroizing::new(entry.username),
        "password" if entry.has_password => vault.password(id)?,
        "totp" if entry.has_totp => Zeroizing::new(vault.totp(id).map_err(|_| "noTotp".to_string())?.now().code),
        "username" | "password" | "totp" => return Err("empty".to_string()),
        _ => return Err("badRequest".to_string()),
    };
    vault.touch();
    Ok(text)
}

const PASSKEY_VERIFY_WAIT: Duration = Duration::from_secs(120);
const OPERATION_KEEP: Duration = Duration::from_secs(300);
const OPERATIONS_MAX: usize = 128;

#[derive(Clone, PartialEq, Eq)]
struct OperationKey {
    browser: String,
    copy: String,
    origin: String,
    id: String,
}

impl OperationKey {
    fn new(browser: &str, copy: &str, url: &str, id: &str) -> Result<Self, String> {
        if id.is_empty() || id.len() > 80 || page_host(url).is_none() {
            return Err("badRequest".into());
        }
        let origin = passkeys::origin_of(url).ok_or("insecure")?;
        Ok(Self { browser: browser.into(), copy: copy.into(), origin, id: id.into() })
    }
}

struct PasskeyOperation {
    until: Instant,
    keep_until: Instant,
    cancelled: Mutex<bool>,
    changed: tokio::sync::Notify,
}

impl PasskeyOperation {
    fn new(now: Instant) -> Self {
        Self {
            until: now + PASSKEY_VERIFY_WAIT,
            keep_until: now + OPERATION_KEEP,
            cancelled: Mutex::new(false),
            changed: tokio::sync::Notify::new(),
        }
    }

    fn cancel(&self) {
        *self.cancelled.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.changed.notify_waiters();
    }

    // Cancellation and the final vault commit share this lock. Verification
    // never holds it: once cancellation is accepted there can be no save.
    fn commit<T>(&self, run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let cancelled = self.cancelled.lock().unwrap_or_else(|p| p.into_inner());
        if *cancelled || Instant::now() >= self.until {
            return Err("cancelled".into());
        }
        run()
    }

    async fn verify<T>(&self, verification: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        self.commit(|| Ok(()))?;
        tokio::select! {
            biased;
            _ = &mut changed => Err("cancelled".into()),
            answer = tokio::time::timeout_at(self.until.into(), verification) => {
                answer.map_err(|_| "cancelled".to_string())?
            }
        }
    }
}

struct OperationRegistry(Vec<(OperationKey, Arc<PasskeyOperation>)>);

impl OperationRegistry {
    fn prune(&mut self, now: Instant) {
        self.0.retain(|(_, operation)| now < operation.keep_until);
    }

    fn start(&mut self, key: OperationKey, now: Instant) -> Result<OperationGuard, String> {
        self.prune(now);
        // A pre-arrived cancellation is a tombstone; reusing a consumed ID
        // must never restart the same request after its page withdrew it.
        if self.0.iter().any(|(found, _)| found == &key) {
            return Err("cancelled".into());
        }
        if self.0.len() >= OPERATIONS_MAX {
            return Err("busy".into());
        }
        let operation = Arc::new(PasskeyOperation::new(now));
        self.0.push((key, operation.clone()));
        Ok(OperationGuard(operation))
    }

    fn cancel(&mut self, key: OperationKey, now: Instant) -> Result<(), String> {
        self.prune(now);
        if let Some((_, operation)) = self.0.iter().find(|(found, _)| found == &key) {
            operation.cancel();
        } else {
            if self.0.len() >= OPERATIONS_MAX {
                return Err("busy".into());
            }
            let operation = Arc::new(PasskeyOperation::new(now));
            operation.cancel();
            self.0.push((key, operation));
        }
        Ok(())
    }
}

static OPERATIONS: Mutex<OperationRegistry> = Mutex::new(OperationRegistry(Vec::new()));

struct OperationGuard(Arc<PasskeyOperation>);

impl std::ops::Deref for OperationGuard {
    type Target = PasskeyOperation;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Drop for OperationGuard {
    fn drop(&mut self) { self.0.cancel(); }
}

fn start_operation(browser: &str, copy: &str, url: &str, id: Option<String>) -> Result<OperationGuard, String> {
    // Older extensions have no operation ID; their verification still expires.
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let key = OperationKey::new(browser, copy, url, &id)?;
    OPERATIONS.lock().unwrap_or_else(|p| p.into_inner()).start(key, Instant::now())
}

/// Passkeys. Using one may ask the user to confirm with Windows Hello, so
/// the vault is not held while that prompt shows.
async fn passkey_reply(app: &AppHandle, state: &PasswordsState, browser: &str, copy: &str, request: Request) -> Result<Value, String> {
    if let Request::PasskeyCancel { url, operation_id } = &request {
        let key = OperationKey::new(browser, copy, url, operation_id)?;
        OPERATIONS.lock().unwrap_or_else(|p| p.into_inner()).cancel(key, Instant::now())?;
        return Ok(json!({ "ok": true }));
    }
    let (enabled, status) = state.with_quiet(|vault| Ok((vault.prefs().browser_filling, vault.status())))?;
    if !enabled {
        return Err("disabled".into());
    }
    let within_limit = match request {
        Request::PasskeyList { .. } => allow(&LOOKUPS, LOOKUPS_PER_MINUTE),
        _ => allow(&SENSITIVE, SENSITIVE_PER_MINUTE),
    };
    if !within_limit {
        return Err("busy".into());
    }
    if status != Status::Unlocked {
        let wakes = !matches!(request, Request::PasskeyList { wake: false, .. });
        if status != Status::New && wakes {
            // A sign-in waits on MYLE: it comes forward and asks to be
            // unlocked (Windows Hello when it is on).
            ask_to_unlock(app);
        }
        return Err(if status == Status::New { "noVault" } else { "locked" }.into());
    }
    match request {
        Request::PasskeyList { url, rp_id, allow, .. } => {
            let host = page_host(&url).ok_or("insecure")?;
            let rp_id = passkeys::rp_id_for(&host, rp_id.as_deref())?;
            if allow.len() > 64 {
                return Err("badRequest".into());
            }
            let found = state.with_quiet(|vault| vault.passkeys_for(&rp_id, &allow))?;
            // Passkeys of this site the page did not ask for (its list names
            // others): MYLE says so instead of quietly stepping aside. Their
            // ids (not secret: sites list them) help the debug log.
            let unlisted: Vec<String> = if allow.is_empty() {
                Vec::new()
            } else {
                state
                    .with_quiet(|vault| vault.passkeys_for(&rp_id, &[]))?
                    .into_iter()
                    .map(|(_, _, key)| key.credential_id)
                    .filter(|id| !found.iter().any(|(_, _, key)| key.credential_id == *id))
                    .take(8)
                    .collect()
            };
            let list: Vec<Value> = found
                .iter()
                .map(|(_, title, key)| {
                    json!({
                        "credentialId": key.credential_id,
                        "userName": key.user_name,
                        "userDisplayName": key.user_display_name,
                        "title": title,
                    })
                })
                .collect();
            Ok(json!({ "ok": true, "rpId": rp_id, "passkeys": list, "unlisted": unlisted.len(), "unlistedIds": unlisted }))
        }
        Request::PasskeyCreate {
            operation_id,
            url,
            rp_id,
            rp_name,
            user_id,
            user_name,
            user_display_name,
            challenge,
            algorithms,
            exclude,
            user_verification,
        } => {
            let host = page_host(&url).ok_or("insecure")?;
            let origin = passkeys::origin_of(&url).ok_or("insecure")?;
            let rp_id = passkeys::rp_id_for(&host, rp_id.as_deref())?;
            if !algorithms.is_empty() && !algorithms.contains(&passkeys::ES256) {
                return Err("notSupported".into());
            }
            if exclude.len() > 64 {
                return Err("badRequest".into());
            }
            // The vault already holds a passkey the site lists as taken.
            if !exclude.is_empty() && !state.with_quiet(|vault| vault.passkeys_for(&rp_id, &exclude))?.is_empty() {
                return Err("excluded".into());
            }
            let operation = start_operation(browser, copy, &url, operation_id)?;
            let verified = operation.verify(verify_user(app, &user_verification, &rp_id)).await?;
            let user = passkeys::User { handle: user_id, name: user_name, display_name: user_display_name };
            let (passkey, credential) =
                passkeys::create(&rp_id, &rp_name, &user, &challenge, &origin, verified, super::vault::now())?;
            let title = if rp_name.trim().is_empty() { rp_id.clone() } else { rp_name.trim().to_string() };
            operation.commit(|| state.with_quiet(|vault| {
                // The login of that account on this site, if there is one.
                let same = same_login(vault, &host, &passkey.user_name)?;
                let target = same.exact.or_else(|| same.same_site.first().cloned());
                vault.add_passkey(target.as_deref(), passkey, &title, &format!("https://{rp_id}"))?;
                vault.touch();
                Ok(())
            }))?;
            let _ = app.emit(super::CHANGED_EVENT, ());
            Ok(json!({ "ok": true, "credential": credential }))
        }
        Request::PasskeyGet { url, rp_id, challenge, credential_id, user_verification, operation_id } => {
            let host = page_host(&url).ok_or("insecure")?;
            let origin = passkeys::origin_of(&url).ok_or("insecure")?;
            let rp_id = passkeys::rp_id_for(&host, rp_id.as_deref())?;
            let passkey = state
                .with_quiet(|vault| vault.passkeys_for(&rp_id, std::slice::from_ref(&credential_id)))?
                .into_iter()
                .map(|(_, _, key)| key)
                .next()
                .ok_or("notFound")?;
            let operation = start_operation(browser, copy, &url, operation_id)?;
            let verified = operation.verify(verify_user(app, &user_verification, &rp_id)).await?;
            let credential = passkeys::sign_in(&passkey, &challenge, &origin, verified)?;
            operation.commit(|| {
                state.with_quiet(|vault| {
                    vault.touch();
                    Ok(())
                })
            })?;
            Ok(json!({ "ok": true, "credential": credential }))
        }
        _ => Err("badRequest".into()),
    }
}

/// Whether the user confirmed it is them, when the site asks for that
/// ("preferred" unless it says otherwise): Windows Hello. Without Windows
/// Hello, a site that insists gets the master password asked in MYLE.
async fn verify_user(app: &AppHandle, wanted: &str, site: &str) -> Result<bool, String> {
    if wanted == "discouraged" {
        return Ok(false);
    }
    let message = format!("Use your passkey for {site}");
    let consent = tauri::async_runtime::spawn_blocking(move || super::hello::verify(&message))
        .await
        .map_err(|e| e.to_string())?;
    match consent {
        Consent::Verified => Ok(true),
        Consent::Refused => Err("notVerified".into()),
        Consent::Unavailable if wanted == "required" => {
            if super::ask_master(app, site).await { Ok(true) } else { Err("notVerified".into()) }
        }
        Consent::Unavailable => Ok(false),
    }
}

/// The otpauth:// link in a picture's grey levels, if it is a 2FA QR code.
fn qr_in_pixels(width: usize, height: usize, pixels: &str) -> Option<String> {
    use base64::Engine;
    if width == 0 || height == 0 || width > 2000 || height > 2000 {
        return None;
    }
    let pixels = base64::engine::general_purpose::STANDARD.decode(pixels).ok()?;
    if pixels.len() != width * height {
        return None;
    }
    super::qr::otpauth_in(&super::qr::Grey { width, height, pixels }).ok()
}

/// The otpauth:// link in a tab's screenshot (base64, or a data: URL).
fn qr_in_screenshot(image: &str) -> Result<String, String> {
    use base64::Engine;
    let data = image.split_once("base64,").map_or(image, |(_, data)| data);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| "That is not a picture.".to_string())?;
    super::qr::otpauth_in(&super::qr::grey_from_bytes(&bytes)?)
}

/// The logins saved with one user name for a page.
#[derive(Debug, Default, PartialEq)]
struct SameLogin {
    /// The one saved for this very host: the only one the page may change.
    exact: Option<String>,
    /// Every one on the same site (subdomains too); a typed password that
    /// any of them holds is already known.
    same_site: Vec<String>,
}

fn same_login(vault: &mut super::vault::Vault, host: &str, username: &str) -> Result<SameLogin, String> {
    let summaries = vault.summaries()?;
    Ok(same_login_in(
        summaries.iter().map(|e| (e.id.as_str(), e.username.as_str(), e.urls.as_slice())),
        host,
        username,
    ))
}

fn same_login_in<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str, &'a [String])>,
    host: &str,
    username: &str,
) -> SameLogin {
    let wanted = username.trim();
    let mut found = SameLogin::default();
    let mut on_host = Vec::new();
    for (id, name, urls) in entries {
        // A password-change form often has no user name field: then any
        // login of the site may be the one.
        if !wanted.is_empty() && !name.eq_ignore_ascii_case(wanted) {
            continue;
        }
        if let Some(exact) = urls.iter().filter_map(|u| fits(u, host)).max() {
            if exact {
                on_host.push(id.to_string());
            }
            found.same_site.push(id.to_string());
        }
    }
    // Without a user name, only a host with a single login says which one.
    if !wanted.is_empty() || on_host.len() == 1 {
        found.exact = on_host.into_iter().next();
    }
    found
}

/// Brings the app forward on the Password Manager page.
/// What the Password Manager hears when a website waits for the vault: it
/// asks to be unlocked at once.
pub const UNLOCK_EVENT: &str = "myle-unlock-wanted";

/// Brings MYLE to its vault and asks for it to be unlocked: at most every
/// few seconds, however many requests a sign-in makes.
pub fn ask_to_unlock(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(4)) {
            return;
        }
        *last = Some(Instant::now());
    }
    open_vault(app);
    let _ = app.emit(UNLOCK_EVENT, ());
}

pub fn open_vault(app: &AppHandle) {
    // Still starting: the splash hands over to the vault by itself.
    let starting = app.get_webview_window("splash").is_some();
    if let Some(window) = app.get_webview_window("main").filter(|_| !starting) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = app.emit("myle-navigate", "password-manager");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passkey_cancel_request_is_supported() {
        let request: Request = serde_json::from_value(json!({
            "type": "passkeyCancel", "url": "https://example.com/", "operationId": "operation"
        })).expect("native passkey cancellation must be accepted");
        assert!(request.is_passkey());
    }

    fn operation_key(id: &str) -> OperationKey {
        OperationKey::new("Edge", "folder", "https://example.com/", id).unwrap()
    }

    #[tokio::test]
    async fn passkey_cancellation_interrupts_verification_before_commit() {
        let mut registry = OperationRegistry(Vec::new());
        let key = operation_key("pending");
        let operation = registry.start(key.clone(), Instant::now()).unwrap();
        let (reply, answer) = tokio::sync::oneshot::channel::<bool>();
        let verification = operation.verify(async { Ok(answer.await.unwrap()) });
        tokio::pin!(verification);
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut verification).await.is_err());
        registry.cancel(key, Instant::now()).unwrap();
        assert_eq!(verification.await.unwrap_err(), "cancelled");
        assert!(reply.send(true).is_err(), "the obsolete verification no longer has a waiting consumer");
        let mut committed = false;
        assert!(operation.commit(|| { committed = true; Ok(()) }).is_err());
        assert!(!committed);
    }

    #[tokio::test]
    async fn passkey_cancellation_after_verification_still_prevents_commit() {
        let mut registry = OperationRegistry(Vec::new());
        let key = operation_key("ready");
        let operation = registry.start(key.clone(), Instant::now()).unwrap();
        assert!(operation.verify(async { Ok(true) }).await.unwrap());
        registry.cancel(key, Instant::now()).unwrap();
        let mut committed = false;
        assert!(operation.commit(|| { committed = true; Ok(()) }).is_err());
        assert!(!committed);
    }

    #[tokio::test]
    async fn passkey_verification_expires_before_it_can_commit() {
        let operation = PasskeyOperation {
            until: Instant::now() + Duration::from_millis(10),
            ..PasskeyOperation::new(Instant::now())
        };
        assert_eq!(operation.verify(std::future::pending::<Result<bool, String>>()).await.unwrap_err(), "cancelled");
        let mut committed = false;
        assert!(operation.commit(|| { committed = true; Ok(()) }).is_err());
        assert!(!committed);
    }

    #[test]
    fn passkey_cancellation_tombstones_are_scoped_bounded_and_expire() {
        let mut registry = OperationRegistry(Vec::new());
        let now = Instant::now();
        let key = operation_key("first");
        registry.cancel(key.clone(), now).unwrap();
        assert!(registry.start(key.clone(), now).is_err(), "an abort can reach native before its create/get");
        for key in [
            OperationKey::new("Chrome", "folder", "https://example.com/", "first").unwrap(),
            OperationKey::new("Edge", "store", "https://example.com/", "first").unwrap(),
            OperationKey::new("Edge", "folder", "https://other.com/", "first").unwrap(),
            operation_key("another-tab"),
        ] {
            let operation = registry.start(key, now).unwrap();
            assert!(operation.commit(|| Ok(())).is_ok(), "another browser, copy, origin or operation is unaffected");
        }
        while registry.0.len() < OPERATIONS_MAX {
            registry.cancel(operation_key(&format!("id-{}", registry.0.len())), now).unwrap();
        }
        assert!(registry.start(operation_key("full"), now).is_err());
        assert_eq!(registry.0.len(), OPERATIONS_MAX);
        assert!(registry.start(key, now + OPERATION_KEEP).is_ok());
        assert_eq!(registry.0.len(), 1);
    }

    #[test]
    fn only_https_pages_and_this_pc_are_filled() {
        assert_eq!(page_host("https://Accounts.Google.com/signin?x=1").as_deref(), Some("accounts.google.com"));
        assert_eq!(page_host("http://localhost:5173/login").as_deref(), Some("localhost"));
        assert_eq!(page_host("http://example.com/login"), None);
        assert_eq!(page_host("file:///C:/login.html"), None);
        assert_eq!(page_host("https://user@evil.com@bank.com/"), Some("bank.com".into()));
    }

    #[test]
    fn a_login_fits_its_own_site_and_never_a_look_alike() {
        assert_eq!(fits("https://github.com/login", "github.com"), Some(true));
        assert_eq!(fits("github.com", "www.github.com"), Some(true));
        assert_eq!(fits("https://accounts.google.com", "mail.google.com"), Some(false));
        assert_eq!(fits("https://google.com", "google.com.evil.io"), None);
        assert_eq!(fits("https://bank.co.uk", "evil.co.uk"), None);
        assert_eq!(fits("https://mybank.co.uk", "login.mybank.co.uk"), Some(false));
    }

    #[test]
    fn native_messages_round_trip() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json!({ "type": "status" })).unwrap();
        let mut reader = &buffer[..];
        assert_eq!(read_message(&mut reader).unwrap().unwrap()["type"], "status");
        assert!(read_message(&mut reader).unwrap().is_none());
        let too_big = (MAX_MESSAGE + 1).to_le_bytes();
        assert!(read_message(&mut &too_big[..]).is_err());
    }

    #[tokio::test]
    async fn browser_listener_takes_over_after_the_previous_version_exits() {
        let name = format!(r"\\.\pipe\myle-browser-update-test-{}", uuid::Uuid::new_v4());
        let previous = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name).unwrap();
        let listener = browser_listener(&name);
        tokio::pin!(listener);
        // Updates deliberately overlap the two app processes. The new one
        // must wait without joining the previous version's pipe.
        assert!(tokio::time::timeout(Duration::from_millis(50), &mut listener).await.is_err());
        drop(previous);
        let server = tokio::time::timeout(Duration::from_secs(2), listener).await.unwrap().unwrap();
        drop(server);
    }

    #[tokio::test]
    async fn a_busy_browser_pipe_is_retried_until_the_next_listener_is_ready() {
        let name = format!(r"\\.\pipe\myle-browser-busy-test-{}", uuid::Uuid::new_v4());
        let previous = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name).unwrap();
        let probe = std::fs::OpenOptions::new().read(true).write(true).open(&name).unwrap();
        previous.connect().await.unwrap();
        let next_name = name.clone();
        let asking = tokio::task::spawn_blocking(move || open_app_pipe(&next_name, Duration::from_secs(1)));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let next = ServerOptions::new().reject_remote_clients(true).create(&name).unwrap();
        assert!(asking.await.unwrap().is_ok(), "a connected probe or simultaneous request must not mean MYLE is not running");
        drop((probe, previous, next));
    }

    #[tokio::test]
    async fn waiting_for_a_busy_browser_pipe_has_a_deadline() {
        let name = format!(r"\\.\pipe\myle-browser-busy-deadline-{}", uuid::Uuid::new_v4());
        let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name).unwrap();
        let occupied = std::fs::OpenOptions::new().read(true).write(true).open(&name).unwrap();
        server.connect().await.unwrap();
        let asking = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let error = open_app_pipe(&name, Duration::from_millis(30)).unwrap_err();
            (started.elapsed(), error.raw_os_error())
        });
        let (elapsed, error) = asking.await.unwrap();
        assert_eq!(error, Some(ERROR_PIPE_BUSY as i32));
        assert!(elapsed >= Duration::from_millis(30), "busy instances should be given time to become available");
        assert!(elapsed < Duration::from_secs(1), "a busy pipe must not block the host indefinitely");
        drop((occupied, server));
    }

    #[test]
    fn a_missing_browser_pipe_is_reported_without_waiting() {
        let name = format!(r"\\.\pipe\myle-browser-missing-test-{}", uuid::Uuid::new_v4());
        let started = Instant::now();
        assert_eq!(open_app_pipe(&name, Duration::from_secs(2)).unwrap_err().kind(), std::io::ErrorKind::NotFound);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn a_lost_browser_reply_is_not_classified_as_safe_to_resend() {
        let name = format!(r"\\.\pipe\myle-browser-lost-reply-{}", uuid::Uuid::new_v4());
        let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name).unwrap();
        let asking = tokio::task::spawn_blocking(move || ask_app_on(&name, &json!({ "type": "passkeyCreate" })));
        server.connect().await.unwrap();
        let mut server = tokio::io::BufReader::new(server);
        let mut request = String::new();
        server.read_line(&mut request).await.unwrap();
        assert!(request.contains("passkeyCreate"));
        // The app could already have committed this request; an EOF before
        // its acknowledgement must not make the host dispatch it again.
        drop(server);
        assert_eq!(asking.await.unwrap(), Err(AppError::Failed));
    }

    #[test]
    fn a_page_changes_only_the_login_saved_for_its_own_host() {
        let urls = |u: &str| vec![u.to_string()];
        let (main, shop, other) = (urls("https://example.com"), urls("https://shop.example.com"), urls("https://other.org"));
        let entries = [("main", "Alice", main.as_slice()), ("shop", "alice", shop.as_slice()), ("other", "alice", other.as_slice())];
        // On shop.example.com: its own login may change; example.com's only counts as known.
        let found = same_login_in(entries, "shop.example.com", " alice ");
        assert_eq!(found.exact.as_deref(), Some("shop"));
        assert_eq!(found.same_site, ["main", "shop"]);
        // A subdomain with no login of its own changes nothing.
        let found = same_login_in(entries, "evil.example.com", "alice");
        assert_eq!(found.exact, None);
        assert_eq!(found.same_site, ["main", "shop"]);
        assert_eq!(same_login_in(entries, "example.com", "bob"), SameLogin::default());
        // A password-change form without a user name: the host's only login.
        assert_eq!(same_login_in(entries, "shop.example.com", "").exact.as_deref(), Some("shop"));
        let (a, b) = (urls("https://two.example.com"), urls("https://two.example.com/login"));
        let two = [("a", "x", a.as_slice()), ("b", "y", b.as_slice())];
        assert_eq!(same_login_in(two, "two.example.com", "").exact, None, "which of the two is unclear");
    }

    #[test]
    fn a_script_cannot_ask_for_more_than_a_minutes_share() {
        let bucket = Mutex::new(VecDeque::new());
        for _ in 0..5 {
            assert!(allow(&bucket, 5));
        }
        assert!(!allow(&bucket, 5), "the sixth in the same minute is refused");
    }

    #[test]
    fn only_our_extension_starts_the_host() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        for id in CHROME_EXTENSION_IDS {
            assert!(started_by_our_extension(&args(&[&format!("chrome-extension://{id}/")])));
            assert!(started_by_our_extension(&args(&[&format!("chrome-extension://{id}"), "--parent-window=0"])));
        }
        assert!(started_by_our_extension(&args(&[r"C:\x\firefox.json", FIREFOX_EXTENSION_ID])));
        assert!(!started_by_our_extension(&args(&["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"])));
        assert!(!started_by_our_extension(&args(&["chrome-extension://mifjffbnaeeljjfboiglbcoaokgdilca/x"])));
        assert!(!started_by_our_extension(&args(&[r"C:\x\firefox.json", "evil@example.com"])));
    }

    #[test]
    fn a_closed_app_is_started_for_a_passkey_and_left_alone_for_the_rest() {
        // A passkey request is the user reaching for MYLE: it is started, so
        // the click they already made carries on instead of hearing "not
        // running" and making MYLE look as if it had no passkey at all.
        for message in [
            json!({ "type": "passkeyList", "url": "https://accounts.google.com/", "wake": true }),
            json!({ "type": "passkeyGet", "url": "https://x.com/", "challenge": "Y2g", "credentialId": "a" }),
            json!({ "type": "passkeyCreate", "url": "https://x.com/", "rpName": "X" }),
        ] {
            assert!(wants_the_app(&message), "{message} starts the app");
        }
        // Filling in a login does not: the menu has its own Open MYLE button,
        // and a page must never bring the app up by itself. The extension's own
        // "open" starts it through the host's other path.
        for message in [
            // The menu on a sign-in field asking what MYLE has, by itself.
            json!({ "type": "passkeyList", "url": "https://accounts.google.com/" }),
            json!({ "type": "logins", "url": "https://x.com/" }),
            json!({ "type": "fill", "id": "1" }),
            json!({ "type": "open" }),
            json!({ "type": "status" }),
        ] {
            assert!(!wants_the_app(&message), "{message} leaves it closed");
        }
    }

    #[test]
    fn a_request_to_the_app_carries_the_browser_and_the_copy() {
        let envelope = request_from("Edge", "folder", &json!({ "type": "status" }));
        assert_eq!(envelope["browser"], "Edge");
        assert_eq!(envelope["copy"], "folder");
        assert_eq!(envelope["request"]["type"], "status");
    }

    #[test]
    fn a_registration_that_lost_its_manifest_or_points_elsewhere_is_found() {
        let id = std::process::id();
        let root = format!(r"Software\MYLE-tests-{id}");
        let dir = std::env::temp_dir().join(format!("myle-hosts-{id}"));
        let hosts = Hosts { root: format!(r"{root}\"), dir: dir.clone(), exe: PathBuf::from(r"C:\Apps\MYLE\MYLE.exe") };
        assert!(hosts.check().unwrap_err().contains("not been told"), "nothing yet");
        hosts.write().unwrap();
        hosts.check().unwrap();

        // What this PC had: the browsers' keys, but the manifests gone.
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(hosts.check().unwrap_err().contains("missing"));
        hosts.write().unwrap();
        hosts.check().unwrap();

        // The program moved (or another copy registered): written again.
        let moved = Hosts { root: hosts.root.clone(), dir: dir.clone(), exe: PathBuf::from(r"D:\MYLE\MYLE.exe") };
        assert!(moved.check().unwrap_err().contains("another copy"));
        moved.write().unwrap();
        moved.check().unwrap();

        moved.remove();
        assert!(!dir.exists());
        assert!(moved.check().is_err());
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&root);
    }

    #[test]
    fn the_host_names_the_browser_and_notes_whom_it_turned_away() {
        assert_eq!(browser_name("msedge.exe"), Some("Edge"));
        assert_eq!(browser_name("Chrome.exe"), Some("Chrome"));
        assert_eq!(browser_name("vivaldi.exe"), None);
        assert_eq!(
            parse_refusal("1700000000\tvivaldi.exe\tnot a supported browser"),
            Some(Refusal { program: "vivaldi.exe".into(), reason: "not a supported browser".into(), at: 1_700_000_000 })
        );
        assert_eq!(parse_refusal("garbage"), None);
        let envelope: Envelope = serde_json::from_value(json!({ "browser": "Edge", "request": { "type": "status" } })).unwrap();
        assert_eq!(envelope.browser, "Edge");
        assert!(matches!(envelope.request, Request::Status));
        assert!(serde_json::from_value::<Envelope>(json!({ "type": "status" })).is_err(), "a bare request is refused");
    }

    #[test]
    fn passkey_requests_are_read_with_the_extensions_names() {
        let create: Request = serde_json::from_value(json!({
            "type": "passkeyCreate", "url": "https://example.com/", "rpId": "example.com", "rpName": "Example",
            "userId": "dXNlcg", "userName": "me", "userDisplayName": "Me", "challenge": "Y2hhbGxlbmdl",
            "algorithms": [-7, -257], "exclude": [], "userVerification": "preferred"
        }))
        .unwrap();
        assert!(create.is_passkey());
        assert!(matches!(
            create,
            Request::PasskeyCreate { ref rp_id, ref user_verification, ref user_display_name, .. }
                if rp_id.as_deref() == Some("example.com") && user_verification == "preferred" && user_display_name == "Me"
        ));
        let get: Request = serde_json::from_value(json!({
            "type": "passkeyGet", "url": "https://example.com/", "challenge": "Y2g", "credentialId": "abc"
        }))
        .unwrap();
        assert!(matches!(get, Request::PasskeyGet { rp_id: None, ref credential_id, .. } if credential_id == "abc"));
        let fill: Request = serde_json::from_value(json!({ "type": "fill", "id": "1", "url": "https://x.com" })).unwrap();
        assert!(!fill.is_passkey());
    }

    #[test]
    fn the_host_tells_the_stores_copy_from_a_folders() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(which_copy(&args(&["chrome-extension://mifjffbnaeeljjfboiglbcoaokgdilca/"])), Some("store"));
        assert_eq!(which_copy(&args(&["chrome-extension://gaelkhdpkgnffkfmaaklknijinjmmopo", "--parent-window=0"])), Some("folder"));
        assert_eq!(which_copy(&args(&[r"C:\x\firefox.json", FIREFOX_EXTENSION_ID])), Some("firefox"));
        assert_eq!(which_copy(&args(&["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"])), None);
        let old: Envelope = serde_json::from_value(json!({ "browser": "Edge", "request": { "type": "status" } })).unwrap();
        assert_eq!(old.copy, "", "a host without the copy still works");
    }

    #[test]
    fn a_page_picture_must_be_whole_and_a_2fa_qr_code() {
        use base64::Engine;
        let b64 = |bytes: Vec<u8>| base64::engine::general_purpose::STANDARD.encode(bytes);
        assert_eq!(qr_in_pixels(0, 0, ""), None);
        assert_eq!(qr_in_pixels(10, 10, &b64(vec![0; 99])), None, "not the size it says");
        assert_eq!(qr_in_pixels(4000, 10, &b64(vec![0; 40_000])), None, "too large");
        assert_eq!(qr_in_pixels(100, 100, &b64(vec![255; 10_000])), None, "a blank picture");
        assert_eq!(qr_in_pixels(10, 10, "not base64!"), None);
    }

    #[test]
    fn the_pipe_is_per_user_and_ours() {
        assert!(pipe_name().starts_with(r"\\.\pipe\myle-passwords-"));
        assert_eq!(pipe_name(), pipe_name());
    }
}
