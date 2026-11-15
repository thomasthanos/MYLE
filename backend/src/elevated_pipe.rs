// ! An administrator helper that the app talks to over a named pipe: one UAC ! prompt, then every request of the session goes through it. ! ! The app creates `\\.\pipe\<prefix><32 hex digits>` as its first instance ! (so nothing can pose as the app), then starts this same program elevated ! with the helper's flag, the pipe's name and the app's process id. The ! helper serves only a pipe whose server is that process, running this same ! program; the app answers only a client that is this same program. Each ! request is one JSON line, and so is each answer. ! ! What a request may ask for is up to each helper: only ids of fixed, ! compiled tables, never paths or commands from the page.
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use crate::apps::process::{ERROR_CANCELLED, start_elevated};
use crate::download::err;
/// One kind of helper.
pub struct Helper {
    /// The command-line flag that starts this program as the helper.
    pub flag: &'static str,
    /// `\\.\pipe\myle-<name>-`.
    pub pipe_prefix: &'static str,
    /// For messages: "administrator cleaner".
    pub what: &'static str,
}
/// The helper waits this long for the next request, then exits. The next
/// request asks for approval again.
const HELPER_IDLE: Duration = Duration::from_secs(30 * 60);
/// From an approved prompt to the helper connecting.
const CONNECT_WAIT: Duration = Duration::from_secs(60);
pub const DECLINED: &str = "Administrator approval was declined.";
/// The app's side: a running helper.
pub struct Session {
    pipe: BufReader<NamedPipeServer>,
}
impl Session {
    /// Asks for administrator approval and starts the helper.
    pub async fn start(helper: &Helper) -> Result<Self, String> {
        let name = format!("{}{}", helper.pipe_prefix, uuid::Uuid::new_v4().simple());
        // First instance: the name is ours before the helper even starts, so nothing else can pose as the app to it.
        let mut options = ServerOptions::new();
        options.first_pipe_instance(true).reject_remote_clients(true);
        let server = create_private(&options, &name).map_err(err)?;
        let executable = std::env::current_exe().map_err(err)?;
        // After a live update the file here is the new MYLE while this one is the old: the two may not speak the same requests, so none is sent (and no approval asked) until MYLE restarts.
        if exe_version(&executable).is_some_and(|on_disk| Some(on_disk) != running_version()) {
            return Err("MYLE was updated. Close it and open it again to use this.".into());
        }
        let executable = executable
            .to_str()
            .ok_or_else(|| "The application path is not valid Unicode.".to_string())?;
        let args = [
            helper.flag.to_string(),
            "serve".into(),
            name,
            std::process::id().to_string(),
        ];
        match start_elevated(executable, &args).await? {
            0 => {}
            ERROR_CANCELLED => return Err(DECLINED.into()),
            code => {
                return Err(format!(
                    "The {} could not start (exit code {code}).",
                    helper.what
                ));
            }
        }
        tokio::time::timeout(CONNECT_WAIT, server.connect())
            .await
            .map_err(|_| format!("The {} did not start.", helper.what))?
            .map_err(err)?;
        // Only this very program may answer: not another process of the user's that connected first.
        if !client_is_this_program(&server) {
            return Err(format!("The {} could not be verified.", helper.what));
        }
        Ok(Self {
            pipe: BufReader::new(server),
        })
    }
    /// `Err` when the helper is gone (it exited after being idle, or was
    /// closed); its answer otherwise.
    pub async fn ask<Q: Serialize, R: DeserializeOwned>(&mut self, request: &Q) -> std::io::Result<R> {
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        self.pipe.get_mut().write_all(line.as_bytes()).await?;
        self.pipe.get_mut().flush().await?;
        let mut answer = String::new();
        if self.pipe.read_line(&mut answer).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        Ok(serde_json::from_str(answer.trim())?)
    }
    /// As `ask`, for a helper that sends bytes after its answer
    /// (`serve_with_payload`): the answer's line, then the bytes' length as
    /// 8 bytes (little-endian), then the bytes.
    pub async fn ask_with_payload<Q: Serialize, R: DeserializeOwned>(&mut self, request: &Q) -> std::io::Result<(R, Vec<u8>)> {
        use tokio::io::AsyncReadExt;
        let answer: R = self.ask(request).await?;
        let mut length = [0u8; 8];
        self.pipe.read_exact(&mut length).await?;
        let length = u64::from_le_bytes(length);
        if length > MAX_PAYLOAD {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let mut payload = vec![0u8; length as usize];
        self.pipe.read_exact(&mut payload).await?;
        Ok((answer, payload))
    }
}
/// Major, minor and patch of the program file at `path` (its version
/// resource); `None` when it has none.
pub fn exe_version(path: &Path) -> Option<(u16, u16, u16)> {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW};
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated; the buffer is as large as Windows asks; VerQueryValueW points into it, and its length is checked before reading.
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root: [u16; 2] = [u16::from(b'\\'), 0];
        let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut length = 0u32;
        if VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut info, &mut length) == 0
            || info.is_null()
            || (length as usize) < size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let fixed = std::ptr::read_unaligned(info.cast::<VS_FIXEDFILEINFO>());
        Some(((fixed.dwFileVersionMS >> 16) as u16, fixed.dwFileVersionMS as u16, (fixed.dwFileVersionLS >> 16) as u16))
    }
}
/// This build's version, as its version resource has it.
fn running_version() -> Option<(u16, u16, u16)> {
    let mut parts = env!("CARGO_PKG_VERSION").split(['.', '-', '+']).map(|part| part.parse::<u16>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}
/// A helper kept for the app session: started, with its approval prompt, on
/// the first request; asked again after that, until it exits after its idle
/// half hour (or after one request approved for another account), when the
/// next request starts a new one.
pub struct KeptHelper {
    helper: &'static Helper,
    session: tokio::sync::Mutex<Option<Session>>,
}
impl KeptHelper {
    pub const fn new(helper: &'static Helper) -> Self {
        Self { helper, session: tokio::sync::Mutex::const_new(None) }
    }
    /// `request`'s answer from the running helper, or from a new one when
    /// none runs.
    pub async fn ask<Q: Serialize, R: DeserializeOwned>(&self, request: &Q) -> Result<R, String> {
        let mut session = self.session.lock().await;
        if let Some(open) = session.as_mut() {
            match open.ask(request).await {
                Ok(answer) => return Ok(answer),
                // The helper has gone: start a new one below.
                Err(_) => *session = None,
            }
        }
        let mut fresh = Session::start(self.helper).await?;
        let answer = fresh
            .ask(request)
            .await
            .map_err(|error| format!("The {} stopped: {error}", self.helper.what))?;
        *session = Some(fresh);
        Ok(answer)
    }
    /// Whether a helper runs, so the next request will not ask for approval.
    pub async fn is_running(&self) -> bool {
        self.session.lock().await.is_some()
    }
    /// The same without waiting: `None` while a request has the helper.
    pub fn running_now(&self) -> Option<bool> {
        self.session.try_lock().ok().map(|session| session.is_some())
    }
}
/// The most bytes a helper may send after an answer.
const MAX_PAYLOAD: u64 = 4 << 30;
/// Creates the pipe open only to this app's account, administrators (the
/// helper may run as another administrator, approved over the shoulder) and
/// SYSTEM. The default security lets everyone, anonymous included, open a
/// pipe for reading: another account could take the connection first and
/// keep the helper out.
pub(crate) fn create_private(options: &ServerOptions, name: &str) -> std::io::Result<NamedPipeServer> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    let user = process_account(std::process::id())
        .ok_or_else(|| std::io::Error::other("The app's own account could not be read."))?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{user})(A;;GA;;;BA)(A;;GA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; the descriptor is freed below.
    let made = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut())
    };
    if made == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` and the descriptor it points at live across the call.
    let server = unsafe { options.create_with_security_attributes_raw(name, (&raw mut attributes).cast()) };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe { LocalFree(descriptor) };
    server
}
/// Whether the pipe's client runs this same program, elevated. A process of
/// the user's own could start this program unelevated as the helper and win
/// the connection: its requests would then quietly run without
/// administrator rights.
fn client_is_this_program(server: &NamedPipeServer) -> bool {
    use std::os::windows::io::AsRawHandle;
    let mut client = 0u32;
    // SAFETY: the handle is the live pipe; `client` receives the id.
    let ok = unsafe { GetNamedPipeClientProcessId(server.as_raw_handle() as HANDLE, &mut client) };
    ok != 0 && is_this_program(client) && is_elevated(client)
}
/// Whether `pid` runs with an elevated (administrator) token.
pub fn is_elevated(pid: u32) -> bool {
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    // SAFETY: both handles are closed below; `elevation` outlives the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut token = std::ptr::null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0;
        CloseHandle(process);
        if !opened {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0u32;
        let read = GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        ) != 0;
        CloseHandle(token);
        read && elevation.TokenIsElevated != 0
    }
}
/// The program file `pid` runs, if it can be asked (it can for elevated
/// processes too: only limited information is needed).
pub fn process_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: the handle is closed below; the buffer outlives the call.
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
/// The SID of the account `pid` runs as ("S-1-5-21-..."): whose hive is the
/// app's own when the helper runs as another administrator.
pub fn process_account(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    // SAFETY: both handles are closed below; the buffer is u64-aligned and
    // as large as Windows asks; the SID string is freed with LocalFree.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut token = std::ptr::null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0;
        CloseHandle(process);
        if !opened {
            return None;
        }
        let mut needed = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        let read = needed > 0 && GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed) != 0;
        CloseHandle(token);
        if !read {
            return None;
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 || text.is_null() {
            return None;
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        sid.starts_with("S-1-").then_some(sid)
    }
}
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}
/// Whether `pid` runs this very program.
pub fn is_this_program(pid: u32) -> bool {
    match (process_path(pid), std::env::current_exe()) {
        (Some(other), Ok(mine)) => same_file(&other, &mine),
        _ => false,
    }
}
/// Called before Tauri starts: the helper's arguments (`serve <pipe> <pid>`)
/// when this program was started as `helper`, else `None`.
pub fn helper_args(helper: &Helper) -> Option<Vec<OsString>> {
    let mut args = std::env::args_os();
    let _executable = args.next();
    (args.next().as_deref() == Some(std::ffi::OsStr::new(helper.flag))).then(|| args.collect())
}
/// Parses `serve <pipe> <app pid>`.
pub fn parse_serve_args(helper: &Helper, args: &[OsString]) -> Result<(String, u32), String> {
    let [mode, pipe, app_pid] = args else {
        return Err(format!("Invalid {} arguments.", helper.what));
    };
    if mode != "serve" {
        return Err(format!("Invalid {} mode.", helper.what));
    }
    let app_pid: u32 = app_pid
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("Invalid {} arguments.", helper.what))?;
    Ok((pipe.to_string_lossy().into_owned(), app_pid))
}
pub fn validate_pipe_name(helper: &Helper, pipe: &str) -> Result<(), String> {
    let valid = pipe.strip_prefix(helper.pipe_prefix).is_some_and(|id| {
        id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if valid {
        Ok(())
    } else {
        Err(format!("Invalid {} pipe.", helper.what))
    }
}
/// The helper's loop: connects to the app's pipe, checks that the app is its
/// server, then answers each request with `handle` until the app closes the
/// pipe (or exits), or none comes for half an hour. A line that is not a
/// request is answered with `invalid()`.
pub fn serve<Q, R>(
    helper: &Helper,
    pipe: &str,
    app_pid: u32,
    mut handle: impl FnMut(Q) -> R,
    invalid: impl Fn() -> R,
) -> Result<(), String>
where
    Q: DeserializeOwned,
    R: Serialize,
{
    serve_with(helper, pipe, app_pid, move |request| (handle(request), None), move || (invalid(), None))
}
/// As `serve`, for answers that come with bytes (read with
/// `Session::ask_with_payload`).
pub fn serve_with_payload<Q, R>(
    helper: &Helper,
    pipe: &str,
    app_pid: u32,
    mut handle: impl FnMut(Q) -> (R, Vec<u8>),
    invalid: impl Fn() -> (R, Vec<u8>),
) -> Result<(), String>
where
    Q: DeserializeOwned,
    R: Serialize,
{
    serve_with(
        helper,
        pipe,
        app_pid,
        move |request| {
            let (response, payload) = handle(request);
            (response, Some(payload))
        },
        move || {
            let (response, payload) = invalid();
            (response, Some(payload))
        },
    )
}
fn serve_with<Q, R>(
    helper: &Helper,
    pipe: &str,
    app_pid: u32,
    mut handle: impl FnMut(Q) -> (R, Option<Vec<u8>>),
    invalid: impl Fn() -> (R, Option<Vec<u8>>),
) -> Result<(), String>
where
    Q: DeserializeOwned,
    R: Serialize,
{
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::AsRawHandle;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    validate_pipe_name(helper, pipe)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(err)?;
    let mut server_pid = 0u32;
    // SAFETY: the handle is the open pipe; `server_pid` receives the id.
    let ok = unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut server_pid) };
    // The process named on the command line, and running this same program: whoever started the helper cannot point it at a pipe of their own.
    if ok == 0 || server_pid != app_pid || !is_this_program(server_pid) {
        return Err(format!("The {} pipe does not belong to the app.", helper.what));
    }
    // When the last request ended, and whether one is being handled. The
    // idle check and the exit happen under the same lock that a request
    // takes before it starts: a request that arrives at the half-hour mark
    // is never cut off half-way (removing Edge, say).
    let state = Arc::new(Mutex::new((Instant::now(), false)));
    {
        let state = Arc::clone(&state);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let guard = state.lock().unwrap_or_else(|p| p.into_inner());
            let (last, busy) = *guard;
            if !busy && last.elapsed() >= HELPER_IDLE {
                std::process::exit(0);
            }
        });
    }
    // An administrator typed their credentials for a standard user (the app
    // runs as another account): that approval was for this one request, not
    // half an hour of them. The app starts a new helper, and asks again,
    // for the next.
    let one_request = serves_once(process_account(app_pid), process_account(std::process::id()));
    let mut writer = file.try_clone().map_err(err)?;
    for line in BufReader::new(file).lines() {
        // A broken pipe: the app has closed it or exited.
        let Ok(line) = line else { break };
        state.lock().unwrap_or_else(|p| p.into_inner()).1 = true;
        let (response, payload) = match serde_json::from_str::<Q>(&line) {
            Ok(request) => handle(request),
            Err(_) => invalid(),
        };
        let mut out = serde_json::to_string(&response).map_err(err)?;
        out.push('\n');
        let sent = writer
            .write_all(out.as_bytes())
            .and_then(|()| match &payload {
                Some(bytes) => writer.write_all(&(bytes.len() as u64).to_le_bytes()).and_then(|()| writer.write_all(bytes)),
                None => Ok(()),
            })
            .and_then(|()| writer.flush());
        *state.lock().unwrap_or_else(|p| p.into_inner()) = (Instant::now(), false);
        if sent.is_err() || one_request {
            break;
        }
    }
    Ok(())
}
/// Whether the helper answers one request only: when the app runs as
/// another account than the helper (an over-the-shoulder approval), or
/// either account is unknown.
fn serves_once(app: Option<String>, helper: Option<String>) -> bool {
    match (app, helper) {
        (Some(app), Some(helper)) => app != helper,
        _ => true,
    }
}
#[cfg(test)]
pub mod tests {
    use super::*;
    #[test]
    fn a_program_files_version_is_read() {
        let explorer = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join("explorer.exe");
        // A program without a Windows 10 manifest (this test) is told 6.2 for Windows' own files; MYLE reads its own file, which Windows leaves alone.
        assert!(exe_version(&explorer).is_some_and(|(major, _, _)| major >= 6));
        assert_eq!(exe_version(&std::env::temp_dir().join("myle-no-such.exe")), None);
        let (major, minor, patch) = running_version().unwrap();
        assert_eq!(format!("{major}.{minor}.{patch}"), env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap());
    }
    #[test]
    fn an_approval_for_another_account_serves_one_request() {
        let me = || Some("S-1-5-21-1-1001".to_string());
        assert!(!serves_once(me(), me()), "the user's own approval keeps the helper for its half hour");
        assert!(serves_once(me(), Some("S-1-5-21-1-500".to_string())));
        assert!(serves_once(None, me()));
        assert!(process_account(std::process::id()).is_some_and(|sid| sid.starts_with("S-1-")));
    }
    pub const TEST_HELPER: Helper = Helper {
        flag: "--test-helper",
        pipe_prefix: r"\\.\pipe\myle-test-",
        what: "test helper",
    };
    /// A runtime for the app's side of the pipe.
    pub fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
    /// The app's side of a pipe served by a helper thread in this process.
    pub async fn connect_to(server: NamedPipeServer) -> Session {
        server.connect().await.unwrap();
        Session {
            pipe: BufReader::new(server),
        }
    }
    #[test]
    fn only_the_apps_random_pipe_names_are_accepted() {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let prefix = TEST_HELPER.pipe_prefix;
        assert!(validate_pipe_name(&TEST_HELPER, &format!("{prefix}{id}")).is_ok());
        assert!(validate_pipe_name(&TEST_HELPER, &format!("{prefix}{id}x")).is_err());
        assert!(validate_pipe_name(&TEST_HELPER, r"\\.\pipe\other").is_err());
        assert!(validate_pipe_name(&TEST_HELPER, &format!(r"\\server\pipe\myle-test-{id}")).is_err());
    }
    #[test]
    fn serve_arguments_are_checked() {
        let good: Vec<OsString> = vec!["serve".into(), "pipe".into(), "42".into()];
        assert_eq!(parse_serve_args(&TEST_HELPER, &good).unwrap(), ("pipe".into(), 42));
        let bad: Vec<OsString> = vec!["run".into(), "pipe".into(), "42".into()];
        assert!(parse_serve_args(&TEST_HELPER, &bad).is_err());
        assert!(parse_serve_args(&TEST_HELPER, &good[..2]).is_err());
    }
    #[test]
    fn bytes_follow_an_answer_when_the_helper_sends_them() {
        runtime().block_on(async {
            let name = format!("{}{}", TEST_HELPER.pipe_prefix, uuid::Uuid::new_v4().simple());
            let server = ServerOptions::new().first_pipe_instance(true).create(&name).unwrap();
            let helper = {
                let name = name.clone();
                std::thread::spawn(move || {
                    serve_with_payload(&TEST_HELPER, &name, std::process::id(), |n: u32| (n * 2, vec![7u8; n as usize]), || (0u32, Vec::new()))
                })
            };
            let mut session = connect_to(server).await;
            // More than a pipe buffer holds, then nothing at all.
            let (answer, bytes): (u32, Vec<u8>) = session.ask_with_payload(&300_000u32).await.unwrap();
            assert_eq!(answer, 600_000);
            assert_eq!(bytes, vec![7u8; 300_000]);
            let (answer, bytes): (u32, Vec<u8>) = session.ask_with_payload(&0u32).await.unwrap();
            assert_eq!((answer, bytes.len()), (0, 0));
            drop(session);
            let ended = tokio::task::spawn_blocking(move || helper.join().unwrap());
            assert!(tokio::time::timeout(Duration::from_secs(20), ended).await.unwrap().unwrap().is_ok());
        });
    }
    #[test]
    fn the_private_pipe_lets_this_account_in() {
        runtime().block_on(async {
            let name = format!("{}{}", TEST_HELPER.pipe_prefix, uuid::Uuid::new_v4().simple());
            let mut options = ServerOptions::new();
            options.first_pipe_instance(true).reject_remote_clients(true);
            let server = create_private(&options, &name).unwrap();
            let client = tokio::net::windows::named_pipe::ClientOptions::new().open(&name);
            assert!(client.is_ok(), "{client:?}");
            server.connect().await.unwrap();
            // Its security names this account, administrators and SYSTEM, and
            // nobody else: not Everyone (WD) or Anonymous (AN) as by default.
            let dacl = pipe_dacl(&server);
            let user = process_account(std::process::id()).unwrap();
            // The built-in Administrator (a CI runner's account) is written with its short form.
            let named = dacl.contains(&format!(";;;{user})")) || (user.ends_with("-500") && dacl.contains(";;;LA)"));
            assert!(named, "{user}: {dacl}");
            assert!(dacl.contains(";;;BA)") && dacl.contains(";;;SY)"), "{dacl}");
            assert!(!dacl.contains(";;;WD)") && !dacl.contains(";;;AN)"), "{dacl}");
            assert!(dacl.starts_with("D:P"), "protected: nothing inherited: {dacl}");
        });
    }
    /// The pipe's DACL as SDDL.
    fn pipe_dacl(server: &NamedPipeServer) -> String {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
        };
        use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
        // SAFETY: the pipe handle is live; both allocations are freed below.
        unsafe {
            let mut descriptor = std::ptr::null_mut();
            let status = GetSecurityInfo(
                server.as_raw_handle() as HANDLE,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            );
            assert_eq!(status, 0);
            let mut text = std::ptr::null_mut();
            let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                std::ptr::null_mut(),
            );
            assert_ne!(ok, 0);
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            LocalFree(descriptor);
            sddl
        }
    }
    #[test]
    fn an_unelevated_process_is_not_taken_for_the_helper() {
        // This test runs unelevated on a normal PC and on CI's runner it may
        // be elevated: either way, the answer matches the token.
        let elevated = is_elevated(std::process::id());
        let output = std::process::Command::new(crate::apps::process::system32("whoami.exe")).arg("/groups").output().unwrap();
        let high = String::from_utf8_lossy(&output.stdout).contains("S-1-16-12288");
        assert_eq!(elevated, high);
        assert!(!is_elevated(u32::MAX));
    }
    #[test]
    fn this_process_is_this_program() {
        assert!(is_this_program(std::process::id()));
        assert!(!is_this_program(4), "the System process is not");
    }
    #[test]
    fn this_process_has_the_users_account() {
        let sid = process_account(std::process::id()).unwrap();
        // The account whose hive is HKEY_CURRENT_USER here.
        let own = winreg::RegKey::predef(winreg::enums::HKEY_USERS).open_subkey(format!(r"{sid}\Software"));
        assert!(own.is_ok(), "{sid}");
        assert_eq!(process_account(u32::MAX), None);
    }
}
