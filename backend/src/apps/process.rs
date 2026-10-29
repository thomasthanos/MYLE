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
    let status = hidden(powershell())
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ])
        .arg(encode_command(&script))
        .status()
        .await
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(-1))
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
    let status = hidden(powershell())
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ])
        .arg(encode_command(&script))
        .status()
        .await
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(-1))
}
/// PowerShell `-EncodedCommand` payload: base64 of the UTF-16LE script.
/// Sidesteps every command-line quoting rule.
pub fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
/// A PowerShell single-quoted string literal.
pub(crate) fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
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
    }
    #[test]
    fn encoded_command_is_utf16le_base64() {
        // "hi" -> 68 00 69 00
        assert_eq!(encode_command("hi"), "aABpAA==");
    }
}
