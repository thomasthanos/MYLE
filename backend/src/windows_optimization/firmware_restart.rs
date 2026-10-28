// ! Restart-to-firmware support. The executable and every argument are fixed ! by this module; the webview can only ask for the operation to begin.
use std::path::{Path, PathBuf};
use super::elevated::{self, ElevatedProcessOutcome};
use super::models::{
    FirmwareRestartOutcome, FirmwareRestartResult, FirmwareRestartSnapshot, FirmwareType,
};
const SHUTDOWN_ARGUMENTS: [&str; 6] = ["/r", "/fw", "/t", "0", "/d", "p:0:0"];
trait RestartPlatform {
    fn detect_firmware(&self) -> Result<FirmwareType, String>;
    fn system_directory(&self) -> Result<PathBuf, String>;
    fn run_elevated(
        &self,
        executable: &Path,
        arguments: &[String],
    ) -> Result<ElevatedProcessOutcome, String>;
}
struct NativePlatform;
impl RestartPlatform for NativePlatform {
    fn detect_firmware(&self) -> Result<FirmwareType, String> {
        native_firmware_type()
    }
    fn system_directory(&self) -> Result<PathBuf, String> {
        native_system_directory()
    }
    fn run_elevated(
        &self,
        executable: &Path,
        arguments: &[String],
    ) -> Result<ElevatedProcessOutcome, String> {
        elevated::run_sync(executable, arguments)
    }
}
pub(crate) fn snapshot(active: bool) -> FirmwareRestartSnapshot {
    snapshot_with(&NativePlatform, active)
}
pub(crate) async fn restart() -> Result<FirmwareRestartOutcome, String> {
    tauri::async_runtime::spawn_blocking(|| restart_with(&NativePlatform))
        .await
        .map_err(|error| format!("The firmware restart task failed: {error}"))?
}
fn snapshot_with(platform: &impl RestartPlatform, active: bool) -> FirmwareRestartSnapshot {
    match platform.detect_firmware() {
        Ok(FirmwareType::Uefi) => FirmwareRestartSnapshot {
            firmware_type: FirmwareType::Uefi,
            available: true,
            blocked_reason: None,
            active,
        },
        Ok(FirmwareType::LegacyBios) => FirmwareRestartSnapshot {
            firmware_type: FirmwareType::LegacyBios,
            available: false,
            blocked_reason: Some(
                "Windows is running in Legacy BIOS/CSM mode. Direct firmware restart requires UEFI."
                    .into(),
            ),
            active,
        },
        Ok(FirmwareType::Unknown) | Err(_) => FirmwareRestartSnapshot {
            firmware_type: FirmwareType::Unknown,
            available: false,
            blocked_reason: Some(
                "Windows could not confirm that this PC supports direct UEFI firmware restart."
                    .into(),
            ),
            active,
        },
    }
}
fn restart_with(platform: &impl RestartPlatform) -> Result<FirmwareRestartOutcome, String> {
    match platform.detect_firmware() {
        Ok(FirmwareType::Uefi) => {}
        Ok(FirmwareType::LegacyBios) => {
            return Ok(outcome(
                FirmwareRestartResult::Unsupported,
                "This Windows session is using Legacy BIOS/CSM mode, not UEFI.",
            ));
        }
        Ok(FirmwareType::Unknown) | Err(_) => {
            return Ok(outcome(
                FirmwareRestartResult::Unsupported,
                "Windows could not verify UEFI firmware restart support.",
            ));
        }
    }
    let system_directory = platform.system_directory()?;
    if !system_directory.is_absolute() {
        return Err("Windows returned an invalid system directory.".into());
    }
    let executable = system_directory.join("shutdown.exe");
    let arguments = SHUTDOWN_ARGUMENTS.map(str::to_owned).to_vec();
    match platform.run_elevated(&executable, &arguments)? {
        ElevatedProcessOutcome::Cancelled => Ok(outcome(
            FirmwareRestartResult::Cancelled,
            "Administrator approval was cancelled. The PC was not restarted.",
        )),
        ElevatedProcessOutcome::NeedsAdmin => Ok(outcome(
            FirmwareRestartResult::NeedsAdmin,
            "Windows did not allow the administrator restart request.",
        )),
        ElevatedProcessOutcome::Exited(0) => Ok(outcome(
            FirmwareRestartResult::Scheduled,
            "Windows accepted the request to restart into UEFI firmware settings.",
        )),
        ElevatedProcessOutcome::Exited(code) => classify_shutdown_error(code),
    }
}
fn classify_shutdown_error(code: u32) -> Result<FirmwareRestartOutcome, String> {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ELEVATION_REQUIRED, ERROR_ENVVAR_NOT_FOUND,
        ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_PRIVILEGE_NOT_HELD,
        ERROR_SHUTDOWN_IN_PROGRESS, ERROR_SHUTDOWN_IS_SCHEDULED,
    };
    match code {
        ERROR_ACCESS_DENIED | ERROR_ELEVATION_REQUIRED | ERROR_PRIVILEGE_NOT_HELD => Ok(outcome(
            FirmwareRestartResult::NeedsAdmin,
            "Windows did not grant the privileges required to restart this PC.",
        )),
        ERROR_NOT_SUPPORTED | ERROR_INVALID_PARAMETER | ERROR_ENVVAR_NOT_FOUND => Ok(outcome(
            FirmwareRestartResult::Unsupported,
            "This firmware does not expose a supported direct restart target to Windows.",
        )),
        ERROR_SHUTDOWN_IN_PROGRESS | ERROR_SHUTDOWN_IS_SCHEDULED => {
            Err("A Windows shutdown or restart is already in progress.".into())
        }
        other => Err(format!(
            "Windows rejected the firmware restart request (exit code {other})."
        )),
    }
}
fn outcome(result: FirmwareRestartResult, note: impl Into<String>) -> FirmwareRestartOutcome {
    FirmwareRestartOutcome {
        result,
        note: Some(note.into()),
    }
}
fn native_firmware_type() -> Result<FirmwareType, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::SystemInformation::{
        FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType,
    };
    let mut firmware: FIRMWARE_TYPE = 0;
    // SAFETY: `firmware` is a valid writable FIRMWARE_TYPE for the duration of the call.
    if unsafe { GetFirmwareType(&mut firmware) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Windows could not determine the firmware type (error {error})."
        ));
    }
    Ok(if firmware == FirmwareTypeUefi {
        FirmwareType::Uefi
    } else if firmware == FirmwareTypeBios {
        FirmwareType::LegacyBios
    } else {
        FirmwareType::Unknown
    })
}
fn native_system_directory() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: the buffer is writable for the advertised capacity.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "Windows could not locate its system directory (error {error})."
        ));
    }
    if length as usize >= buffer.len() {
        return Err("The Windows system directory path is unexpectedly long.".into());
    }
    buffer.truncate(length as usize);
    Ok(std::ffi::OsString::from_wide(&buffer).into())
}
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use super::*;
    struct MockPlatform {
        firmware: Result<FirmwareType, String>,
        system_directory: PathBuf,
        launch: Result<ElevatedProcessOutcome, String>,
        observed: RefCell<Option<(PathBuf, Vec<String>)>>,
    }
    impl MockPlatform {
        fn new(
            firmware: Result<FirmwareType, String>,
            launch: Result<ElevatedProcessOutcome, String>,
        ) -> Self {
            Self {
                firmware,
                system_directory: PathBuf::from(r"C:\Windows\System32"),
                launch,
                observed: RefCell::new(None),
            }
        }
    }
    impl RestartPlatform for MockPlatform {
        fn detect_firmware(&self) -> Result<FirmwareType, String> {
            self.firmware.clone()
        }
        fn system_directory(&self) -> Result<PathBuf, String> {
            Ok(self.system_directory.clone())
        }
        fn run_elevated(
            &self,
            executable: &Path,
            arguments: &[String],
        ) -> Result<ElevatedProcessOutcome, String> {
            *self.observed.borrow_mut() = Some((executable.to_path_buf(), arguments.to_vec()));
            self.launch.clone()
        }
    }
    #[test]
    fn capability_exposes_only_confirmed_uefi() {
        let uefi = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(0)),
        );
        assert_eq!(
            snapshot_with(&uefi, false),
            FirmwareRestartSnapshot {
                firmware_type: FirmwareType::Uefi,
                available: true,
                blocked_reason: None,
                active: false,
            }
        );
        let bios = MockPlatform::new(
            Ok(FirmwareType::LegacyBios),
            Ok(ElevatedProcessOutcome::Exited(0)),
        );
        let bios = snapshot_with(&bios, true);
        assert_eq!(bios.firmware_type, FirmwareType::LegacyBios);
        assert!(!bios.available);
        assert!(bios.active);
        let unknown = MockPlatform::new(Err("api failed".into()), Err("must not run".into()));
        let unknown = snapshot_with(&unknown, false);
        assert_eq!(unknown.firmware_type, FirmwareType::Unknown);
        assert!(!unknown.available);
    }
    #[test]
    fn uefi_restart_uses_only_system_shutdown_and_fixed_safe_arguments() {
        let platform = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(0)),
        );
        let result = restart_with(&platform).unwrap();
        assert_eq!(result.result, FirmwareRestartResult::Scheduled);
        let observed = platform.observed.borrow();
        let (executable, arguments) = observed.as_ref().unwrap();
        assert_eq!(
            executable,
            &PathBuf::from(r"C:\Windows\System32\shutdown.exe")
        );
        assert_eq!(arguments, &SHUTDOWN_ARGUMENTS.map(str::to_owned));
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.eq_ignore_ascii_case("/f"))
        );
    }
    #[test]
    fn unsupported_firmware_never_launches_a_process() {
        for firmware in [FirmwareType::LegacyBios, FirmwareType::Unknown] {
            let platform = MockPlatform::new(
                Ok(firmware),
                Err("the process runner must not be called".into()),
            );
            let result = restart_with(&platform).unwrap();
            assert_eq!(result.result, FirmwareRestartResult::Unsupported);
            assert!(platform.observed.borrow().is_none());
        }
    }
    #[test]
    fn elevation_outcomes_remain_distinct() {
        let cancelled = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Cancelled),
        );
        assert_eq!(
            restart_with(&cancelled).unwrap().result,
            FirmwareRestartResult::Cancelled
        );
        let denied = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::NeedsAdmin),
        );
        assert_eq!(
            restart_with(&denied).unwrap().result,
            FirmwareRestartResult::NeedsAdmin
        );
    }
    #[test]
    fn known_nonzero_shutdown_results_are_classified() {
        use windows_sys::Win32::Foundation::{
            ERROR_ENVVAR_NOT_FOUND, ERROR_NOT_SUPPORTED, ERROR_PRIVILEGE_NOT_HELD,
            ERROR_SHUTDOWN_IN_PROGRESS,
        };
        let unsupported = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(ERROR_NOT_SUPPORTED)),
        );
        assert_eq!(
            restart_with(&unsupported).unwrap().result,
            FirmwareRestartResult::Unsupported
        );
        let unavailable_target = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(ERROR_ENVVAR_NOT_FOUND)),
        );
        assert_eq!(
            restart_with(&unavailable_target).unwrap().result,
            FirmwareRestartResult::Unsupported
        );
        let denied = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(ERROR_PRIVILEGE_NOT_HELD)),
        );
        assert_eq!(
            restart_with(&denied).unwrap().result,
            FirmwareRestartResult::NeedsAdmin
        );
        let in_progress = MockPlatform::new(
            Ok(FirmwareType::Uefi),
            Ok(ElevatedProcessOutcome::Exited(ERROR_SHUTDOWN_IN_PROGRESS)),
        );
        assert!(
            restart_with(&in_progress)
                .unwrap_err()
                .contains("already in progress")
        );
    }
}
