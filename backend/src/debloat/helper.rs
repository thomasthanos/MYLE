// ! The administrator half of the debloater: machine-wide registry values, ! services, scheduled tasks, Store apps for every user, Edge, and restore ! points. It acts only on what the catalog names: a request carries tweak ! ids and package names, and an undo record is checked against the ! catalog before anything is written back (the record is the app's file, ! which another program of the user's could have edited).
use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use super::catalog::{self, Data, Hive, Op};
use super::system::{self, Value};
use super::undo::{Before, Saved};
use crate::elevated_pipe::{self, Helper};
pub const HELPER: Helper = Helper {
    flag: "--debloat-elevated-helper",
    pipe_prefix: r"\\.\pipe\myle-debloat-",
    what: "administrator debloater",
};
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Request {
    /// The machine-wide part of a tweak.
    Apply { tweak: String },
    /// Puts back the machine-wide part, from what applying kept.
    Undo { tweak: String, saved: Vec<Saved> },
    /// Removes Store apps for every user, and for users still to come.
    RemoveApps { packages: Vec<String> },
    /// A restore point before changes; `turn_on` first turns on System
    /// Protection for the Windows drive.
    RestorePoint { turn_on: bool },
    /// Hides or shows the Start Menu Recommended section machine-wide.
    SetHideRecommended { hide: bool },
    /// Applies or clears the Start Menu `ConfigureStartPins` policy.
    SetStartPins { json: Option<String> },
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    /// What applying changed, for undo.
    #[serde(default)]
    pub saved: Vec<Saved>,
    /// Operations put back by undo.
    #[serde(default)]
    pub undone: Vec<usize>,
    #[serde(default)]
    pub errors: Vec<String>,
    /// Packages no longer there.
    #[serde(default)]
    pub removed: Vec<String>,
    /// Restore point: created, or why not.
    #[serde(default)]
    pub restore_point: Option<RestorePoint>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum RestorePoint {
    Created,
    /// System Protection is off for the Windows drive.
    ProtectionOff,
    Failed { message: String },
}
impl Reply {
    fn error(message: impl Into<String>) -> Self {
        Reply { errors: vec![message.into()], ..Reply::default() }
    }
}
// --------------------------------------------------------------------------- Entry
pub fn run_helper_from_args() -> Option<i32> {
    let args: Vec<OsString> = elevated_pipe::helper_args(&HELPER)?;
    let served = elevated_pipe::parse_serve_args(&HELPER, &args)
        .and_then(|(pipe, app_pid)| elevated_pipe::serve(&HELPER, &pipe, app_pid, handle, || Reply::error("Invalid debloat request.")));
    Some(match served {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Elevated debloater: {error}");
            2
        }
    })
}
pub fn handle(request: Request) -> Reply {
    match request {
        Request::Apply { tweak } => match catalog::find(&tweak) {
            Some(tweak) => apply(tweak),
            None => Reply::error("Unknown tweak."),
        },
        Request::Undo { tweak, saved } => match catalog::find(&tweak) {
            Some(tweak) => undo(tweak, &saved),
            None => Reply::error("Unknown tweak."),
        },
        Request::RemoveApps { packages } => remove_apps(&packages),
        Request::RestorePoint { turn_on } => Reply { restore_point: Some(restore_point(turn_on)), ..Reply::default() },
        Request::SetHideRecommended { hide } => match super::start_menu::apply_machine_hide_recommended(hide) {
            Ok(()) => Reply::default(),
            Err(e) => Reply::error(e),
        },
        Request::SetStartPins { json } => match super::start_menu::apply_machine_start_pins(json.as_deref()) {
            Ok(()) => Reply::default(),
            Err(e) => Reply::error(e),
        },
    }
}
// --------------------------------------------------------------------------- Tweaks
fn apply(tweak: &catalog::Tweak) -> Reply {
    let mut reply = Reply::default();
    for (index, op) in tweak.ops.iter().enumerate() {
        let result = match *op {
            Op::Reg { hive: Hive::Machine, path, name, data, best_effort } => {
                let before = system::read(Hive::Machine, path, name);
                if before.matches(data) {
                    Ok(None)
                } else {
                    let created = system::first_missing(Hive::Machine, path);
                    match system::write(Hive::Machine, path, name, data) {
                        Ok(()) => Ok(Some(Before::Reg { value: before, created })),
                        Err(_) if best_effort => Ok(None),
                        Err(e) => Err(e),
                    }
                }
            }
            Op::Service { name, to, from, .. } => match system::start_type(name) {
                Ok(Some(current)) if from.contains(&current) => {
                    system::set_start_type(name, to).map(|()| Some(Before::Service { start: current }))
                }
                Ok(_) => Ok(None),
                Err(e) => Err(e),
            },
            Op::Task { folder, name } => match system::task_enabled(folder, name) {
                Ok(Some(true)) => system::set_task_enabled(folder, name, false).map(|()| Some(Before::Task { enabled: true })),
                Ok(_) => Ok(None),
                Err(e) => Err(e),
            },
            Op::Appx { app } => {
                let gone = remove_apps(&[app.to_string()]);
                reply.removed.extend(gone.removed);
                if gone.errors.is_empty() { Ok(None) } else { Err(gone.errors.join(" ")) }
            }
            Op::RemoveEdge => edge::remove().map(|removed| removed.then_some(Before::Edge)),
            Op::Capability { name, .. } => capability(name, false).map(|changed| changed.then_some(Before::Capability)),
            // The user's own part runs in the app, as the user.
            Op::Reg { hive: Hive::User, .. } | Op::Clock24 => Ok(None),
        };
        match result {
            Ok(Some(before)) => reply.saved.push(Saved { op: index, before }),
            Ok(None) => {}
            Err(e) => reply.errors.push(e),
        }
    }
    reply
}
/// Whether `saved` is something this tweak's operation `index` could have
/// kept, and safe to write back.
fn valid_undo(tweak: &catalog::Tweak, saved: &Saved) -> Option<&'static Op> {
    let op = tweak.ops.get(saved.op)?;
    let ok = match (op, &saved.before) {
        (Op::Reg { hive: Hive::Machine, path, name, .. }, Before::Reg { value, created }) => {
            let value_ok = match value {
                Value::Absent | Value::Dword { .. } | Value::Other => true,
                Value::Sz { value } => catalog::allowed_machine_strings(path, name).contains(&value.as_str()),
            };
            // Only keys on the way to the value, which applying may have made.
            let created_ok = created.as_deref().is_none_or(|top| system::key_on_path(path, top));
            value_ok && created_ok
        }
        (Op::Service { .. }, Before::Service { .. })
        | (Op::Task { .. }, Before::Task { .. })
        | (Op::Capability { .. }, Before::Capability) => true,
        _ => false,
    };
    ok.then_some(op)
}
fn undo(tweak: &catalog::Tweak, saved: &[Saved]) -> Reply {
    let mut reply = Reply::default();
    for record in saved {
        let Some(op) = valid_undo(tweak, record) else {
            continue;
        };
        let result = match (*op, &record.before) {
            (Op::Reg { path, name, .. }, Before::Reg { value, created }) => {
                system::restore(Hive::Machine, path, name, value, created.as_deref())
            }
            (Op::Service { name, .. }, Before::Service { start }) => system::set_start_type(name, *start),
            (Op::Task { folder, name }, Before::Task { enabled }) => system::set_task_enabled(folder, name, *enabled),
            (Op::Capability { name, .. }, Before::Capability) => capability(name, true).map(|_| ()),
            _ => continue,
        };
        match result {
            Ok(()) => reply.undone.push(record.op),
            Err(e) => reply.errors.push(e),
        }
    }
    reply
}
// --------------------------------------------------------------------------- Store apps
fn powershell() -> PathBuf {
    system::windows_dir().join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}
/// Runs a script hidden and returns what it printed.
fn run_script(script: &str, timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(powershell())
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(crate::apps::process::encode_command(script))
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("PowerShell could not start: {e}"))?;
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err("PowerShell took too long.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
/// Removes (or adds back) the parts of Windows whose capability name starts
/// with `name`, one of the catalog's. Adding back downloads it from Windows
/// Update. True when something changed.
fn capability(name: &str, add: bool) -> Result<bool, String> {
    let (wanted, verb) = if add { ("-ne 'Installed'", "Add") } else { ("-eq 'Installed'", "Remove") };
    let pattern = crate::apps::process::ps_quote(&format!("{name}*"));
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {{
    $found = @(Get-WindowsCapability -Online | Where-Object {{ $_.Name -like {pattern} -and $_.State {wanted} }})
    foreach ($item in $found) {{ {verb}-WindowsCapability -Online -Name $item.Name | Out-Null }}
    "MYLE-DONE:$($found.Count)"
}} catch {{
    "MYLE-ERROR:$($_.Exception.Message)"
}}"#
    );
    let output = run_script(&script, Duration::from_secs(15 * 60))?;
    for line in output.lines().map(str::trim) {
        if let Some(count) = line.strip_prefix("MYLE-DONE:") {
            return Ok(count.trim() != "0");
        }
        if let Some(error) = line.strip_prefix("MYLE-ERROR:") {
            return Err(error.trim().to_string());
        }
    }
    Err("Windows did not say whether the feature changed.".into())
}
fn remove_apps(packages: &[String]) -> Reply {
    let mut reply = Reply::default();
    // Only names of the catalog, never what Windows or MYLE needs.
    let names: Vec<&String> = packages.iter().filter(|name| catalog::removable(name).is_some()).collect();
    if names.len() != packages.len() {
        reply.errors.push("Some apps are not on MYLE's list and were left alone.".into());
    }
    if names.is_empty() {
        return reply;
    }
    let list = names
        .iter()
        .map(|name| crate::apps::process::ps_quote(name))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
$provisioned = @(Get-AppxProvisionedPackage -Online)
foreach ($name in @({list})) {{
    Get-AppxPackage -AllUsers -Name $name | ForEach-Object {{ Remove-AppxPackage -Package $_.PackageFullName -AllUsers }}
    $provisioned | Where-Object {{ $_.DisplayName -eq $name }} | ForEach-Object {{ Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName | Out-Null }}
    if (Get-AppxPackage -AllUsers -Name $name) {{ "MYLE-LEFT:$name" }} else {{ "MYLE-GONE:$name" }}
}}"#
    );
    match run_script(&script, Duration::from_secs(60 + 60 * names.len() as u64)) {
        Ok(output) => {
            for line in output.lines().map(str::trim) {
                if let Some(name) = line.strip_prefix("MYLE-GONE:") {
                    reply.removed.push(name.to_string());
                } else if let Some(name) = line.strip_prefix("MYLE-LEFT:") {
                    reply.errors.push(format!("{name} could not be removed."));
                }
            }
        }
        Err(e) => reply.errors.push(e),
    }
    reply
}
// --------------------------------------------------------------------------- Restore point
const SYSTEM_RESTORE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore";
/// Whether System Protection covers the Windows drive: the volumes it
/// protects are listed under SPP\Clients as "...:(C%3A)".
fn protection_on() -> bool {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    let drive = system::windows_dir().to_string_lossy().chars().next().unwrap_or('C').to_ascii_uppercase();
    let marker = format!("({drive}%3A)");
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SPP\Clients", KEY_READ | KEY_WOW64_64KEY)
    else {
        return false;
    };
    key.enum_values().flatten().any(|(name, _)| {
        key.get_value::<Vec<String>, _>(&name)
            .is_ok_and(|volumes| volumes.iter().any(|volume| volume.to_ascii_uppercase().contains(&marker)))
    })
}
fn restore_point(turn_on: bool) -> RestorePoint {
    if !protection_on() {
        if !turn_on {
            return RestorePoint::ProtectionOff;
        }
        let drive = system::windows_dir().to_string_lossy().chars().take(2).collect::<String>();
        let script = format!("Enable-ComputerRestore -Drive '{drive}\\'");
        if let Err(message) = run_script(&script, Duration::from_secs(120)) {
            return RestorePoint::Failed { message };
        }
    }
    // Windows makes at most one a day unless told otherwise; put its own setting back afterwards.
    let before = system::read(Hive::Machine, SYSTEM_RESTORE, "SystemRestorePointCreationFrequency");
    let _ = system::write(Hive::Machine, SYSTEM_RESTORE, "SystemRestorePointCreationFrequency", Data::Dword(0));
    let result = run_script(
        "try { Checkpoint-Computer -Description 'MYLE Debloat' -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; 'MYLE-CREATED' } catch { 'MYLE-FAILED:' + $_.Exception.Message }",
        Duration::from_secs(600),
    );
    let _ = system::restore(Hive::Machine, SYSTEM_RESTORE, "SystemRestorePointCreationFrequency", &before, None);
    match result {
        Ok(output) if output.contains("MYLE-CREATED") => RestorePoint::Created,
        Ok(output) => RestorePoint::Failed {
            message: output
                .lines()
                .find_map(|line| line.trim().strip_prefix("MYLE-FAILED:"))
                .unwrap_or("Windows did not create it.")
                .trim()
                .to_string(),
        },
        Err(message) => RestorePoint::Failed { message },
    }
}
// --------------------------------------------------------------------------- Edge
mod edge {
    use super::*;
    const UNINSTALL: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft Edge";
    const EDGE_UPDATE_DEV: &str = r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdateDev";
    /// Splits `"C:\...\setup.exe" --a --b` into the program and its switches.
    pub(super) fn parse_uninstall(command: &str) -> Option<(PathBuf, Vec<String>)> {
        let command = command.trim();
        let (program, rest) = if let Some(quoted) = command.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = command.find(" --").unwrap_or(command.len());
            (&command[..end], &command[end..])
        };
        let switches = rest
            .split_whitespace()
            .filter(|arg| arg.starts_with("--") && !arg.contains('"'))
            .map(str::to_string)
            .collect();
        Some((PathBuf::from(program), switches))
    }
    /// Edge's own uninstaller, and only from where Edge lives.
    pub(super) fn trusted_setup(program: &Path, edge_root: &Path) -> bool {
        let Ok(program) = program.canonicalize() else { return false };
        let Ok(root) = edge_root.canonicalize() else { return false };
        program.starts_with(&root)
            && program.file_name().is_some_and(|name| name.eq_ignore_ascii_case("setup.exe"))
    }
    /// `Ok(true)`: Edge was there and is gone now.
    pub fn remove() -> Result<bool, String> {
        if !system::edge_installed() {
            return Ok(false);
        }
        let Value::Sz { value: command } = system::read(Hive::Machine, UNINSTALL, "UninstallString") else {
            return Err("Edge's uninstaller was not found.".into());
        };
        let (setup, mut switches) = parse_uninstall(&command).ok_or("Edge's uninstaller was not understood.")?;
        let edge_root = system::program_files_x86().join(r"Microsoft\Edge\Application");
        if !trusted_setup(&setup, &edge_root) {
            return Err("Edge's uninstaller is not where Edge is installed.".into());
        }
        switches.retain(|switch| switch != "--force-uninstall");
        switches.push("--force-uninstall".into());
        // Edge refuses to uninstall outside the EEA unless allowed, and unless it believes the old Edge is still present.
        let allow_before = system::read(Hive::Machine, EDGE_UPDATE_DEV, "AllowUninstall");
        let allow_created = system::first_missing(Hive::Machine, EDGE_UPDATE_DEV);
        system::write(Hive::Machine, EDGE_UPDATE_DEV, "AllowUninstall", Data::Sz(""))?;
        let stub_dir = system::windows_dir().join(r"SystemApps\Microsoft.MicrosoftEdge_8wekyb3d8bbwe");
        let stub = stub_dir.join("MicrosoftEdge.exe");
        let made_dir = !stub_dir.exists() && std::fs::create_dir_all(&stub_dir).is_ok();
        let made_stub = !stub.exists() && std::fs::write(&stub, b"").is_ok();
        let ran = Command::new(&setup)
            .args(&switches)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Edge's uninstaller could not start: {e}"))
            .and_then(|mut child| {
                let started = Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) if started.elapsed() > Duration::from_secs(300) => {
                            let _ = child.kill();
                            return Err("Edge's uninstaller took too long.".to_string());
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(500)),
                        Err(e) => return Err(e.to_string()),
                    }
                }
            });
        if made_stub {
            let _ = std::fs::remove_file(&stub);
        }
        if made_dir {
            let _ = std::fs::remove_dir(&stub_dir);
        }
        let _ = system::restore(Hive::Machine, EDGE_UPDATE_DEV, "AllowUninstall", &allow_before, allow_created.as_deref());
        ran?;
        remove_common_shortcuts();
        if system::edge_installed() {
            Err("Edge is still installed; Windows refused to remove it.".into())
        } else {
            Ok(true)
        }
    }
    /// The Start menu and public desktop shortcuts every user shares.
    fn remove_common_shortcuts() {
        let program_data = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let public = std::env::var_os("PUBLIC").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
        for shortcut in [
            program_data.join(r"Microsoft\Windows\Start Menu\Programs\Microsoft Edge.lnk"),
            public.join(r"Desktop\Microsoft Edge.lnk"),
        ] {
            let _ = std::fs::remove_file(shortcut);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::debloat::catalog::Start;
    #[test]
    fn requests_carry_only_ids() {
        let json = serde_json::to_string(&Request::Apply { tweak: "telemetry".into() }).unwrap();
        assert_eq!(json, r#"{"do":"apply","tweak":"telemetry"}"#);
        let json = serde_json::to_string(&Request::RestorePoint { turn_on: true }).unwrap();
        assert_eq!(json, r#"{"do":"restorePoint","turnOn":true}"#);
        let reply = handle(Request::Apply { tweak: "../../evil".into() });
        assert!(!reply.errors.is_empty());
    }
    #[test]
    fn an_edited_undo_record_cannot_write_elsewhere() {
        let location = catalog::find("location").unwrap();
        // op 1: HKLM ConsentStore\location Value (a string).
        let honest = Saved { op: 1, before: Before::Reg { value: Value::Sz { value: "Allow".into() }, created: None } };
        assert!(valid_undo(location, &honest).is_some());
        let forged_value = Saved { op: 1, before: Before::Reg { value: Value::Sz { value: r"C:\evil.exe".into() }, created: None } };
        assert!(valid_undo(location, &forged_value).is_none());
        let forged_key = Saved { op: 1, before: Before::Reg { value: Value::Absent, created: Some("SOFTWARE".into()) } };
        assert!(valid_undo(location, &forged_key).is_none(), "never delete SOFTWARE");
        let wrong_kind = Saved { op: 1, before: Before::Service { start: Start::Auto } };
        assert!(valid_undo(location, &wrong_kind).is_none());
        let no_such_op = Saved { op: 99, before: Before::Task { enabled: true } };
        assert!(valid_undo(location, &no_such_op).is_none());
        // op 2 is the user's own value: not the helper's to write.
        let user_op = Saved { op: 2, before: Before::Reg { value: Value::Absent, created: None } };
        assert!(valid_undo(location, &user_op).is_none());
    }
    #[test]
    fn only_catalog_apps_reach_powershell() {
        let reply = remove_apps(&["Microsoft.WindowsStore".into(), "x'; Remove-Item C:\\ -Recurse; '".into()]);
        assert!(reply.removed.is_empty());
        assert!(!reply.errors.is_empty());
    }
    #[test]
    fn edges_uninstaller_is_parsed_and_checked() {
        let (program, switches) = edge::parse_uninstall(
            r#""C:\Program Files (x86)\Microsoft\Edge\Application\131.0.2903.70\Installer\setup.exe" --uninstall --msedge --channel=stable --system-level --verbose-logging"#,
        )
        .unwrap();
        assert!(program.ends_with(r"Installer\setup.exe"));
        assert_eq!(switches, ["--uninstall", "--msedge", "--channel=stable", "--system-level", "--verbose-logging"]);
        let root = std::env::temp_dir();
        assert!(!edge::trusted_setup(&std::env::current_exe().unwrap(), &root));
    }
}
