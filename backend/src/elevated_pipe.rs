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
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)
            .map_err(err)?;
        let executable = std::env::current_exe().map_err(err)?;
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
/// The most bytes a helper may send after an answer.
const MAX_PAYLOAD: u64 = 4 << 30;
/// Whether the pipe's client runs this same program.
fn client_is_this_program(server: &NamedPipeServer) -> bool {
    use std::os::windows::io::AsRawHandle;
    let mut client = 0u32;
    // SAFETY: the handle is the live pipe; `client` receives the id.
    let ok = unsafe { GetNamedPipeClientProcessId(server.as_raw_handle() as HANDLE, &mut client) };
    ok != 0 && is_this_program(client)
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
    use std::sync::atomic::{AtomicBool, Ordering};
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
    let last = Arc::new(Mutex::new(Instant::now()));
    let busy = Arc::new(AtomicBool::new(false));
    {
        let (last, busy) = (Arc::clone(&last), Arc::clone(&busy));
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let idle = last.lock().map(|at| at.elapsed()).unwrap_or_default();
            if !busy.load(Ordering::SeqCst) && idle >= HELPER_IDLE {
                std::process::exit(0);
            }
        });
    }
    let mut writer = file.try_clone().map_err(err)?;
    for line in BufReader::new(file).lines() {
        // A broken pipe: the app has closed it or exited.
        let Ok(line) = line else { break };
        busy.store(true, Ordering::SeqCst);
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
        if let Ok(mut at) = last.lock() {
            *at = Instant::now();
        }
        busy.store(false, Ordering::SeqCst);
        if sent.is_err() {
            break;
        }
    }
    Ok(())
}
#[cfg(test)]
pub mod tests {
    use super::*;
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
    fn this_process_is_this_program() {
        assert!(is_this_program(std::process::id()));
        assert!(!is_this_program(4), "the System process is not");
    }
}
