// ! Narrow wrapper around `ShellExecuteExW("runas")` for backend-owned ! executables and arguments. Nothing from the webview is passed here.
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElevatedProcessOutcome {
    Exited(u32),
    Cancelled,
    NeedsAdmin,
}
struct ProcessHandle(windows_sys::Win32::Foundation::HANDLE);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns the process handle.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
}
pub(crate) async fn run(
    executable: PathBuf,
    arguments: Vec<String>,
) -> Result<ElevatedProcessOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || run_sync(&executable, &arguments))
        .await
        .map_err(|error| format!("The administrator process task failed: {error}"))?
}
pub(crate) fn run_sync(
    executable: &Path,
    arguments: &[String],
) -> Result<ElevatedProcessOutcome, String> {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_ELEVATION_REQUIRED, GetLastError, WAIT_FAILED,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    let executable = wide_null(executable.as_os_str());
    let verb = wide_null(std::ffi::OsStr::new("runas"));
    let parameters = wide_null(std::ffi::OsStr::new(
        &arguments
            .iter()
            .map(|argument| quote_windows_argument(argument))
            .collect::<Vec<_>>()
            .join(" "),
    ));
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: 0,
        ..Default::default()
    };
    // SAFETY: all pointers reference live NUL-terminated buffers for the call; the returned process handle is closed below on every successful launch.
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        let error = unsafe { GetLastError() };
        return match error {
            ERROR_CANCELLED => Ok(ElevatedProcessOutcome::Cancelled),
            ERROR_ACCESS_DENIED | ERROR_ELEVATION_REQUIRED => {
                Ok(ElevatedProcessOutcome::NeedsAdmin)
            }
            _ => Err(format!(
                "Could not start the administrator process (Windows error {error})."
            )),
        };
    }
    if execute.hProcess.is_null() {
        return Err("Windows did not return an administrator process handle.".into());
    }
    let process = ProcessHandle(execute.hProcess);
    // SAFETY: the handle remains live until `process` is dropped.
    let wait = unsafe { WaitForSingleObject(process.0, INFINITE) };
    if wait == WAIT_FAILED {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Could not wait for the administrator process (Windows error {error})."
        ));
    }
    if wait != WAIT_OBJECT_0 {
        return Err(format!(
            "The administrator process returned an unexpected wait result ({wait})."
        ));
    }
    let mut code = 0u32;
    // SAFETY: the process handle is still valid and `code` is writable.
    if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Could not read the administrator process result (Windows error {error})."
        ));
    }
    Ok(ElevatedProcessOutcome::Exited(code))
}
fn quote_windows_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .bytes()
            .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\x0B' | b'"'))
    {
        return argument.to_string();
    }
    let mut quoted = String::from('"');
    let mut slashes = 0usize;
    for character in argument.chars() {
        if character == '\\' {
            slashes += 1;
        } else if character == '"' {
            quoted.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            quoted.push('"');
            slashes = 0;
        } else {
            quoted.extend(std::iter::repeat_n('\\', slashes));
            slashes = 0;
            quoted.push(character);
        }
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_arguments_are_quoted_without_command_interpretation() {
        assert_eq!(quote_windows_argument("plain"), "plain");
        assert_eq!(quote_windows_argument("two words"), "\"two words\"");
        assert_eq!(quote_windows_argument("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_windows_argument("trailing \\"), "\"trailing \\\\\"");
        assert_ne!(
            ElevatedProcessOutcome::Exited(windows_sys::Win32::Foundation::ERROR_CANCELLED),
            ElevatedProcessOutcome::Cancelled
        );
    }
}
