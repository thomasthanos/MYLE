//! Child-process helpers: hidden console processes, UAC elevation, killing.
use std::ffi::OsStr;
use base64::Engine;
/// Keeps console programs (winget, taskkill, powershell) from flashing a window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Windows ERROR_ELEVATION_REQUIRED: the program's manifest demands admin.
pub const ERROR_ELEVATION_REQUIRED: i32 = 740;
/// Windows ERROR_CANCELLED: returned by `run_elevated` when UAC is declined.
pub const ERROR_CANCELLED: i32 = 1223;
/// `%SystemRoot%\System32\<relative>`. Windows' own tools are started by
/// their full path: a bare name is looked for in the app's own folder first,
/// which is per user and writable, so a planted `powershell.exe` there would
/// run in place of the real one.
pub fn system32(relative: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    std::path::PathBuf::from(root).join("System32").join(relative)
}
/// Windows PowerShell 5.1, by its full path (see `system32`).
pub fn powershell() -> std::path::PathBuf {
    system32(r"WindowsPowerShell\v1.0\powershell.exe")
}
/// The same, as text, for the helpers that take a program name.
pub fn system32_str(relative: &str) -> String {
    system32(relative).to_string_lossy().into_owned()
}
/// Windows PowerShell 5.1's full path as text (see `system32`).
pub fn powershell_str() -> String {
    powershell().to_string_lossy().into_owned()
}
/// winget (App Installer): the per-user alias App Installer puts in
/// WindowsApps, else one on PATH, never a `winget.exe` in the app's own
/// folder (see `system32`). An alias is a reparse point that only opens when
/// run, so it is looked for with `symlink_metadata`.
pub fn winget() -> std::path::PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(|dir| std::path::PathBuf::from(dir).join(r"Microsoft\WindowsApps"));
    let on_path = std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>()).unwrap_or_default();
    local
        .into_iter()
        .chain(on_path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("winget.exe"))
        .find(|path| std::fs::symlink_metadata(path).is_ok())
        .unwrap_or_else(|| "winget.exe".into())
}
/// File Explorer, by its full path (see `system32`).
pub fn explorer() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    std::path::PathBuf::from(root).join("explorer.exe")
}
pub fn hidden(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
/// Runs `program args` through a UAC prompt, waits for it and returns its exit
/// code, or `ERROR_CANCELLED` if the user declined the prompt.
pub async fn run_elevated(
    program: &str,
    args: &[String],
    hide_window: bool,
) -> Result<i32, String> {
    let mut script = format!(
        "$p = Start-Process -FilePath {} -Verb RunAs -Wait -PassThru",
        ps_quote(program)
    );
    if !args.is_empty() {
        let list: Vec<String> = args.iter().map(|a| ps_quote(a)).collect();
        script.push_str(&format!(" -ArgumentList {}", list.join(",")));
    }
    if hide_window {
        script.push_str(" -WindowStyle Hidden");
    }
    let script = format!("try {{ {script}; exit $p.ExitCode }} catch {{ exit {ERROR_CANCELLED} }}");
    powershell_via_stdin(&script).await
}
/// Starts `program args` through a UAC prompt and returns once it is running
/// (0), or `ERROR_CANCELLED` if the user declined the prompt. Does not wait
/// for the program: for a helper that keeps running and is talked to.
pub async fn start_elevated(program: &str, args: &[String]) -> Result<i32, String> {
    let list: Vec<String> = args.iter().map(|a| ps_quote(a)).collect();
    let mut script = format!(
        "Start-Process -FilePath {} -Verb RunAs -WindowStyle Hidden",
        ps_quote(program)
    );
    if !list.is_empty() {
        script.push_str(&format!(" -ArgumentList {}", list.join(",")));
    }
    let script = format!("try {{ {script}; exit 0 }} catch {{ exit {ERROR_CANCELLED} }}");
    powershell_via_stdin(&script).await
}
/// The line `powershell_via_stdin` sends: ASCII only (base64 of the UTF-16
/// script), so no console code page can mangle a non-English path.
fn stdin_line(script: &str) -> String {
    format!(
        "iex ([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{}')))\n",
        encode_command(script)
    )
}
/// Runs a PowerShell script given on standard input rather than on the
/// command line. A command line holds at most 32,767 characters, and an
/// elevated command nested inside an encoded script outgrew that (Check Disk's
/// came to about 36,500, so it never started). Standard input has no such limit.
async fn powershell_via_stdin(script: &str) -> Result<i32, String> {
    use tokio::io::AsyncWriteExt;
    let mut child = hidden(powershell())
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or("PowerShell has no input.")?;
    let written = input.write_all(stdin_line(script).as_bytes()).await;
    drop(input); // end of input: PowerShell runs the line and exits
    let status = child.wait().await.map_err(|e| e.to_string())?;
    written.map_err(|e| format!("Could not hand the script to PowerShell: {e}"))?;
    Ok(status.code().unwrap_or(-1))
}
/// PowerShell `-EncodedCommand` payload: base64 of the UTF-16LE script.
/// Sidesteps every command-line quoting rule.
pub fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
/// A PowerShell single-quoted string literal. PowerShell also ends such a
/// string at the typographic quotes ‘ ’ ‚ ‛ (U+2018 to U+201B), so those are
/// doubled too: a path like `C:\Users\O’Brien\…` broke every script, and a
/// path someone else chose could have been script.
pub(crate) fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}
/// Runs a blocking command to the end and returns its output. Both pipes are
/// read on threads of their own, so a program that writes more than a pipe
/// holds never stalls (reading one after the other, or only after it exits,
/// left it waiting until the timeout). The program and anything it starts
/// are in a Job Object and are killed together after `timeout`.
pub fn output_within(
    mut command: std::process::Command,
    timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
    use std::os::windows::io::AsRawHandle;
    use std::process::Stdio;
    fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    }
    let program = command.get_program().to_string_lossy().into_owned();
    let name = std::path::Path::new(&program)
        .file_name()
        .map_or(program.clone(), |name| name.to_string_lossy().into_owned());
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name} could not start: {e}"))?;
    let job = JobObject::new().filter(|job| job.assign(child.as_raw_handle() as _));
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            match &job {
                Some(job) => job.kill(),
                None => {
                    let _ = child.kill();
                }
            }
            let _ = child.wait();
            return Err(format!("{name} took too long and was stopped."));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    // Whatever the program started has its copy of the pipes: end it too,
    // so the readers finish.
    drop(job);
    Ok(std::process::Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}
/// Kills a process and everything it started (e.g. winget and its installer).
pub fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new(system32("taskkill.exe"))
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
/// A Windows Job Object that kills its processes when closed.
pub(crate) struct JobObject(windows_sys::Win32::Foundation::HANDLE);
// SAFETY: a job handle may be used and closed from any thread.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}
impl JobObject {
    pub(crate) fn new() -> Option<JobObject> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: plain Win32 calls; the handle is owned by the returned value and closed in Drop; `info` lives across the call.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let job = JobObject(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            (ok != 0).then_some(job)
        }
    }
    pub(crate) fn assign(&self, process: windows_sys::Win32::Foundation::HANDLE) -> bool {
        // SAFETY: both handles are valid for the call.
        unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.0, process) != 0
        }
    }
    pub(crate) fn kill(&self) {
        // SAFETY: the job handle is valid until Drop.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own; the job's processes end with it.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_are_escaped_for_powershell() {
        assert_eq!(ps_quote("it's"), "'it''s'");
        // PowerShell ends a single-quoted string at these too.
        assert_eq!(ps_quote("O\u{2019}Brien"), "'O\u{2019}\u{2019}Brien'");
        assert_eq!(ps_quote("\u{2018}\u{201A}\u{201B}"), "'\u{2018}\u{2018}\u{201A}\u{201A}\u{201B}\u{201B}'");
    }
    #[test]
    fn a_path_with_a_typographic_quote_reaches_powershell_whole() {
        let path = r"C:\Users\O’Brien\it's ‘odd’.txt";
        let mut command = std::process::Command::new(powershell());
        command.args(["-NoProfile", "-NonInteractive", "-Command", &format!("[Console]::Out.Write({})", ps_quote(path))]);
        let output = output_within(command, std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), path);
    }
    #[test]
    fn a_program_that_writes_more_than_a_pipe_holds_still_finishes() {
        // Far more than the 4 KB a pipe buffers, on both streams.
        let mut command = std::process::Command::new(powershell());
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "1..2000 | % { [Console]::Out.WriteLine('o' * 60); [Console]::Error.WriteLine('e' * 60) }; exit 3",
        ]);
        let output = output_within(command, std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(output.stdout.len() > 100_000 && output.stderr.len() > 100_000);
    }
    #[test]
    fn a_program_past_its_time_is_stopped_with_what_it_started() {
        let mut command = std::process::Command::new(powershell());
        command.args(["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 60"]);
        let started = std::time::Instant::now();
        let error = output_within(command, std::time::Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("took too long"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }
    #[test]
    fn the_script_on_standard_input_is_one_ascii_line() {
        let line = stdin_line("try { $p = 'C:\\Users\\Θωμάς\\x.exe'; exit 0 } catch { exit 1223 }\nexit 1");
        assert!(line.is_ascii());
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with("')))\n"));
    }
    #[tokio::test]
    async fn a_script_on_standard_input_keeps_its_exit_code_and_non_english_text() {
        let script = "try { if ('Θωμάς'.Length -eq 5) { exit 9 } else { exit 3 } } catch { exit 1223 }";
        assert_eq!(powershell_via_stdin(script).await, Ok(9));
        // Far past the 32,767-character command-line limit.
        let long = format!("try {{ $s = '{}'; exit ($s.Length % 241) }} catch {{ exit 1223 }}", "y".repeat(40_000));
        assert_eq!(powershell_via_stdin(&long).await, Ok(40_000 % 241));
    }
    #[test]
    fn encoded_command_is_utf16le_base64() {
        // "hi" -> 68 00 69 00
        assert_eq!(encode_command("hi"), "aABpAA==");
    }
}
