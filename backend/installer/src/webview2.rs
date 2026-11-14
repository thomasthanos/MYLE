//! The setup's window is a WebView2 page, like the app itself. Windows 11
//! always has the runtime; an older Windows 10 may not, so it is installed
//! first with Microsoft's own bootstrapper (as the NSIS setup did).

use std::fs::File;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};

use crate::shell;

const CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const BOOTSTRAPPER: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

pub fn installed() -> bool {
    let keys = [
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\WOW6432Node\{CLIENT}"),
        ),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{CLIENT}")),
        (HKEY_CURRENT_USER, format!(r"Software\{CLIENT}")),
    ];
    keys.iter().any(|(hive, path)| {
        RegKey::predef(*hive)
            .open_subkey_with_flags(path, KEY_READ)
            .and_then(|key| key.get_value::<String, _>("pv"))
            .is_ok_and(|version| !version.is_empty() && version != "0.0.0.0")
    })
}

/// Downloads and runs the bootstrapper. `quiet` hides Microsoft's window.
pub fn install(quiet: bool) -> Result<(), String> {
    // A folder of its own, made here under a random name: nothing can be
    // waiting in its place.
    let folder = std::env::temp_dir().join(format!("myle-webview2-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&folder)
        .map_err(|e| format!("The Microsoft Edge WebView2 Runtime could not be downloaded: {e}"))?;
    let ran = download_and_run(&folder.join("MicrosoftEdgeWebview2Setup.exe"), quiet);
    let _ = std::fs::remove_dir_all(&folder);
    ran?;
    if installed() {
        Ok(())
    } else {
        Err("The Microsoft Edge WebView2 Runtime was not installed.".into())
    }
}

fn download_and_run(target: &Path, quiet: bool) -> Result<(), String> {
    let target_text = target.to_string_lossy().into_owned();
    // curl ships with Windows 10 1803 and later; PowerShell covers the rest.
    let downloaded = shell::run_hidden(
        &shell::system32("curl.exe"),
        &["-fsSL", "--retry", "2", "-o", &target_text, BOOTSTRAPPER],
    ) == Some(0)
        || shell::run_hidden(
            &shell::system32(r"WindowsPowerShell\v1.0\powershell.exe"),
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Invoke-WebRequest -UseBasicParsing -Uri '{BOOTSTRAPPER}' -OutFile {}",
                    ps_quote(&target_text)
                ),
            ],
        ) == Some(0);
    if !downloaded || !target.is_file() {
        return Err("The Microsoft Edge WebView2 Runtime could not be downloaded.".into());
    }
    // Held open from the check to the run, so it cannot be changed in between.
    let _held = hold(target)
        .map_err(|e| format!("The Microsoft Edge WebView2 Runtime could not be downloaded: {e}"))?;
    if !signed_by_microsoft(target) {
        return Err("The downloaded WebView2 setup is not signed by Microsoft, so it was not run.".into());
    }
    let mut command = std::process::Command::new(target);
    if quiet {
        command.args(["/silent", "/install"]);
    } else {
        command.arg("/install");
    }
    command.status().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens `path` so that others may read (and run) it but not change,
/// replace or delete it while the handle lives.
fn hold(path: &Path) -> std::io::Result<File> {
    const FILE_SHARE_READ: u32 = 1;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)
}

/// A PowerShell single-quoted string literal. PowerShell also ends one at
/// the typographic quotes ‘ ’ ‚ ‛, so those are doubled as well as `'`
/// (a profile such as `C:\Users\O’Brien` broke the script).
fn ps_quote(text: &str) -> String {
    let mut out = String::from('\'');
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Whether `path` carries a valid signature of Microsoft's.
fn signed_by_microsoft(path: &Path) -> bool {
    // Use Windows PowerShell's own module. A parent running another PowerShell
    // version may pass a PSModulePath whose Security module cannot load here.
    let script = format!(
        "Import-Module (Join-Path $PSHOME 'Modules\\Microsoft.PowerShell.Security\\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop; \
         $s = Get-AuthenticodeSignature -LiteralPath {}; \
         if ($s.Status -eq 'Valid' -and $s.SignerCertificate.Subject -match '(^|, )O=Microsoft Corporation(,|$)') {{ exit 0 }}; exit 1",
        ps_quote(&path.to_string_lossy())
    );
    shell::run_hidden(
        &shell::system32(r"WindowsPowerShell\v1.0\powershell.exe"),
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    ) == Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("myle-webview2-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&folder).unwrap();
        folder
    }

    #[test]
    fn only_a_setup_signed_by_microsoft_is_trusted() {
        let folder = scratch();
        let signed = folder.join("signed.exe");
        std::fs::copy(shell::system32("whoami.exe"), &signed).unwrap();
        let unsigned = folder.join("unsigned.exe");
        std::fs::write(&unsigned, b"MZ not a real program").unwrap();
        assert!(signed_by_microsoft(&signed));
        assert!(!signed_by_microsoft(&unsigned));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_held_setup_still_runs_but_cannot_be_swapped() {
        let folder = scratch();
        let exe = folder.join("held.exe");
        std::fs::copy(shell::system32("whoami.exe"), &exe).unwrap();
        let held = hold(&exe).unwrap();
        assert!(std::fs::write(&exe, b"swapped").is_err());
        assert!(std::fs::remove_file(&exe).is_err());
        assert!(std::fs::rename(&exe, folder.join("moved.exe")).is_err());
        assert!(signed_by_microsoft(&exe), "the check can read it");
        let status = std::process::Command::new(&exe)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "and it runs");
        drop(held);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
