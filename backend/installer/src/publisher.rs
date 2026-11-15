//! MYLE's release certificate, "ThomasT" (`scripts/new-release-certificate.ps1`),
//! which signs the app, the uninstaller and the setup. It is MYLE's own: no
//! public authority issued it, so Windows names no publisher for MYLE until
//! the user trusts it. The setup's last page offers that, for this Windows
//! user only, with no administrator rights: Windows itself asks before it adds
//! a root certificate, and again before the uninstaller takes it away. The
//! certificate is not a certificate authority and signs programs only, so
//! trusting it vouches for nothing else.
//!
//! The app includes this same file (`backend/src/publisher_offer.rs`), so a
//! copy that came by update, which never shows the setup's last page, offers
//! it in Settings; it therefore uses nothing else of the setup.
//!
//! Windows' administrator prompt (UAC) reads only the machine's trusted roots,
//! so Settings also offers trusting it for every account on this PC: with
//! administrator rights, asked for by Windows, and taken back by the
//! uninstaller the same way.
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CRYPT_E_EXISTS, ERROR_CANCELLED, GetLastError};
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_FIND_EXISTING, CERT_STORE_ADD_NEW, CertAddCertificateContextToStore,
    CertCloseStore, CertCreateCertificateContext, CertDeleteCertificateFromStore, CertFindCertificateInStore,
    CertFreeCertificateContext, CertOpenSystemStoreW, HCERTSTORE, X509_ASN_ENCODING,
};

/// `text` NUL-terminated, as Windows takes it.
fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

/// The name Windows shows as MYLE's publisher once the user trusts it.
pub const NAME: &str = "ThomasT";

/// The public part only: the key stays in the release secret.
const CERTIFICATE: &[u8] = include_bytes!("../ThomasT.cer");

/// This user's trusted root certificates (a view that also holds the
/// machine's).
struct Roots(HCERTSTORE);

impl Roots {
    fn open() -> Option<Self> {
        let name = wide("ROOT");
        // SAFETY: `name` is NUL-terminated; the store is closed on drop.
        let store = unsafe { CertOpenSystemStoreW(0, name.as_ptr()) };
        (!store.is_null()).then_some(Self(store))
    }

    /// The machine's own trusted roots, read only (changing them takes an
    /// administrator: `trust_machine`).
    fn machine() -> Option<Self> {
        use windows_sys::Win32::Security::Cryptography::{
            CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG,
            CERT_SYSTEM_STORE_LOCAL_MACHINE, CertOpenStore,
        };
        let name = wide("ROOT");
        // SAFETY: `name` is NUL-terminated; the store is closed on drop.
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                0,
                0,
                CERT_SYSTEM_STORE_LOCAL_MACHINE | CERT_STORE_READONLY_FLAG | CERT_STORE_OPEN_EXISTING_FLAG,
                name.as_ptr().cast(),
            )
        };
        (!store.is_null()).then_some(Self(store))
    }

    /// This store's copy of `certificate`, when it has one.
    fn find(&self, certificate: &Certificate) -> Option<Certificate> {
        // SAFETY: both are open; the copy found is freed on drop.
        let found = unsafe {
            CertFindCertificateInStore(self.0, X509_ASN_ENCODING, 0, CERT_FIND_EXISTING, certificate.0.cast(), null())
        };
        (!found.is_null()).then_some(Certificate(found))
    }
}

impl Drop for Roots {
    fn drop(&mut self) {
        // SAFETY: opened by `open`.
        unsafe { CertCloseStore(self.0, 0) };
    }
}

struct Certificate(*const CERT_CONTEXT);

impl Certificate {
    /// The certificate this setup carries.
    fn ours() -> Option<Self> {
        // SAFETY: reads `CERTIFICATE` only; the context is freed on drop.
        let context =
            unsafe { CertCreateCertificateContext(X509_ASN_ENCODING, CERTIFICATE.as_ptr(), CERTIFICATE.len() as u32) };
        (!context.is_null()).then_some(Self(context))
    }
}

impl Drop for Certificate {
    fn drop(&mut self) {
        // SAFETY: a context this module created or found, freed once.
        unsafe { CertFreeCertificateContext(self.0) };
    }
}

/// Whether `path` carries an intact signature made with this certificate.
/// Only then does trusting it change what Windows shows, so a release still
/// signed with the earlier certificate, or a build that isn't signed, offers
/// nothing.
pub fn signs(path: &std::path::Path) -> bool {
    signer(path).is_some_and(|der| der == CERTIFICATE)
}

/// The certificate whose embedded signature covers `path` unaltered, whether
/// or not Windows trusts it yet; never fetched over the network.
fn signer(path: &std::path::Path) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
        WTD_UI_NONE, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
    };
    const CERT_E_UNTRUSTEDROOT: i32 = 0x800B_0109_u32 as i32;
    let wide = wide(path.as_os_str());
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data`, `file` and `wide` outlive both calls; the state the
    // first call opens is read only in between and closed by the second.
    let status = unsafe { WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) };
    let der = (status == 0 || status == CERT_E_UNTRUSTEDROOT)
        .then(|| unsafe {
            // SAFETY: provider data of the verification just made, still open.
            let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
            let signer = if provider.is_null() { null_mut() } else { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
            if signer.is_null() || (*signer).csCertChain == 0 || (*signer).pasCertChain.is_null() {
                return None;
            }
            let certificate = (*(*signer).pasCertChain).pCert;
            if certificate.is_null() {
                return None;
            }
            Some(std::slice::from_raw_parts((*certificate).pbCertEncoded, (*certificate).cbCertEncoded as usize).to_vec())
        })
        .flatten();
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state the first call opened.
    unsafe { WinVerifyTrust(null_mut(), &mut action, (&raw mut data).cast()) };
    der
}

/// Whether Windows already trusts the certificate on this PC, for this user
/// or for every user.
pub fn is_trusted() -> bool {
    let (Some(roots), Some(ours)) = (Roots::open(), Certificate::ours()) else {
        return false;
    };
    roots.find(&ours).is_some()
}

/// The visible dialogs (`#32770`) on the desktop, as numbers.
fn dialogs() -> Vec<isize> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> i32 {
        // SAFETY: `window` comes from EnumWindows; `found` is the Vec passed below.
        unsafe {
            let mut class = [0u16; 16];
            let length = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
            if IsWindowVisible(window) != 0 && class[..length] == *wide("#32770").split_last().unwrap().1 {
                (*(found as *mut Vec<isize>)).push(window as isize);
            }
        }
        1
    }
    let mut found = Vec::new();
    // SAFETY: `found` outlives the enumeration, which is synchronous.
    unsafe { EnumWindows(Some(visit), (&raw mut found) as LPARAM) };
    found
}

/// Runs `ask`, which makes Windows put a question about a root certificate on
/// screen. That dialog has no owner window, so it can open behind MYLE's
/// (or the setup's): while `ask` waits for the answer, the dialog that
/// appeared is put on top and given the focus.
fn question_on_top<T>(ask: impl FnOnce() -> T) -> T {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow,
        SetWindowPos,
    };
    let before = dialogs();
    let answered = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let until = Instant::now() + Duration::from_secs(30);
            while !answered.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
                if let Some(window) = dialogs().into_iter().find(|window| !before.contains(window)) {
                    let window = window as HWND;
                    // SAFETY: a window handle just listed; at worst it has closed since.
                    unsafe {
                        SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
                        SetForegroundWindow(window);
                    }
                    return;
                }
            }
        });
        // Should Windows show it from a process of its own, that one may take the focus too.
        // SAFETY: no pointers.
        unsafe { AllowSetForegroundWindow(ASFW_ANY) };
        let answer = ask();
        answered.store(true, Ordering::SeqCst);
        answer
    })
}

/// Trusts the certificate for this user. Windows asks the user first, with a
/// window of its own; `Ok(false)` when they said no. Blocks until they answer.
pub fn trust() -> Result<bool, String> {
    let roots = Roots::open().ok_or("Windows did not open the trusted certificates.")?;
    let ours = Certificate::ours().ok_or("The certificate in this setup is damaged.")?;
    let (added, code) = question_on_top(|| {
        // SAFETY: both are open; no copy is asked back.
        let added = unsafe { CertAddCertificateContextToStore(roots.0, ours.0, CERT_STORE_ADD_NEW, null_mut()) } != 0;
        // SAFETY: read right after the call it describes, on the same thread.
        (added, unsafe { GetLastError() })
    });
    if added || code == CRYPT_E_EXISTS as u32 {
        Ok(true)
    } else if code == ERROR_CANCELLED {
        Ok(false)
    } else {
        Err(format!("Windows did not add the certificate (0x{code:08X})."))
    }
}

/// Takes back the trust `trust` gave, when there is any; Windows asks the user
/// first. Trust an administrator gave every user stays: that is theirs.
pub fn forget() {
    let (Some(roots), Some(ours)) = (Roots::open(), Certificate::ours()) else {
        return;
    };
    if let Some(found) = roots.find(&ours) {
        // SAFETY: deleting frees the context, whether or not it succeeds, so
        // `found` must not be freed again.
        question_on_top(|| unsafe { CertDeleteCertificateFromStore(found.0) });
        std::mem::forget(found);
    }
}

/// Whether every account on this PC trusts the certificate: the machine's
/// roots, the only ones Windows' administrator prompt reads.
pub fn machine_trusted() -> bool {
    let (Some(roots), Some(ours)) = (Roots::machine(), Certificate::ours()) else {
        return false;
    };
    roots.find(&ours).is_some()
}

/// Trusts the certificate for every account on this PC. Windows asks for
/// administrator rights first; `Ok(false)` when the user said no.
#[allow(dead_code, reason = "offered by the app's Settings; the setup only takes it back")]
pub fn trust_machine() -> Result<bool, String> {
    match run_elevated_powershell(&machine_script("Add"))? {
        None => Ok(false),
        Some(0) if machine_trusted() => Ok(true),
        Some(code) => Err(format!("Windows did not trust the certificate for every account (code {code}).")),
    }
}

/// Takes back the trust `trust_machine` gave, when there is any; Windows asks
/// for administrator rights first.
pub fn forget_machine() {
    if machine_trusted() {
        let _ = run_elevated_powershell(&machine_script("Remove"));
    }
}

/// A PowerShell script that adds the certificate to, or removes it from
/// (`action`: `Add` or `Remove`), the machine's trusted roots. The certificate
/// is written into the script itself, so nothing on disk can be swapped for
/// another between the user's approval and the change.
fn machine_script(action: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new([Convert]::FromBase64String('{}'))\n\
         $roots = [Security.Cryptography.X509Certificates.X509Store]::new('Root', 'LocalMachine')\n\
         $roots.Open('ReadWrite')\n\
         try {{ $roots.{action}($certificate) }} finally {{ $roots.Close() }}\n",
        base64(CERTIFICATE)
    )
}

/// Runs `script` in Windows PowerShell as administrator, hidden, and waits for
/// it: its exit code, or `None` when the user declined the prompt.
fn run_elevated_powershell(script: &str) -> Result<Option<u32>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let parameters = wide(format!(
        "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {}",
        encoded_command(script)
    ));
    let program = wide(windows_powershell());
    let verb = wide("runas");
    // SAFETY: zero is a valid empty value for every field of this plain struct.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = program.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: every string pointed to outlives the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: read right after the call it describes.
        return match unsafe { GetLastError() } {
            ERROR_CANCELLED => Ok(None),
            code => Err(format!("Windows could not start it with administrator rights (0x{code:08X}).")),
        };
    }
    if info.hProcess.is_null() {
        return Err("Windows started nothing to wait for.".into());
    }
    // SAFETY: the process handle is ours (SEE_MASK_NOCLOSEPROCESS) and closed below.
    unsafe {
        let finished = WaitForSingleObject(info.hProcess, 120_000) == WAIT_OBJECT_0;
        let mut code = 0u32;
        let read = GetExitCodeProcess(info.hProcess, &mut code) != 0;
        CloseHandle(info.hProcess);
        if !finished || !read {
            return Err("Changing the trusted certificates took too long.".into());
        }
        Ok(Some(code))
    }
}

/// `script` as PowerShell's `-EncodedCommand` takes it: UTF-16 in base64, so
/// no quoting can change it on the way.
fn encoded_command(script: &str) -> String {
    base64(&script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>())
}

/// Windows PowerShell 5.1, by its full path in the `System32` Windows names.
/// Not from `%SystemRoot%`: it runs as administrator, and the environment is
/// the user's to change, which would have them approve another program.
fn windows_powershell() -> std::path::PathBuf {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer and its length match.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    let system = if length == 0 || length >= buffer.len() {
        std::path::PathBuf::from(r"C:\Windows\System32")
    } else {
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]))
    };
    system.join(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// Standard base64, with padding.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { DIGITS[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Cryptography::{
        CERT_BASIC_CONSTRAINTS2_INFO, CERT_NAME_SIMPLE_DISPLAY_TYPE, CTL_USAGE, CertFindExtension,
        CertGetEnhancedKeyUsage, CertGetNameStringW, CryptDecodeObject, X509_BASIC_CONSTRAINTS2,
        szOID_BASIC_CONSTRAINTS2,
    };

    // `trust` and `forget` are never called here: they would change this
    // PC's trusted certificates (after Windows' question).

    /// `trust_machine` and `forget_machine` are never called here either:
    /// they would ask for administrator rights and change the machine.
    #[test]
    fn the_machine_script_carries_this_certificate_and_nothing_else() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        let add = machine_script("Add");
        assert!(add.contains(&format!("FromBase64String('{}')", base64(CERTIFICATE))));
        assert!(add.contains("'Root', 'LocalMachine'") && add.contains("$roots.Add($certificate)"));
        assert!(machine_script("Remove").contains("$roots.Remove($certificate)"));
        // Read only: whatever the answer on this PC, it changes nothing.
        let _ = machine_trusted();
    }

    /// The script reaches PowerShell whole, and the certificate in it opens
    /// there as ThomasT's: the same first lines, run without administrator
    /// rights and touching no store.
    #[test]
    fn powershell_reads_the_certificate_the_script_carries() {
        let script = machine_script("Add");
        let reading = script.split("$roots").next().unwrap().to_string() + "[Console]::Out.Write($certificate.Subject)\n";
        let output = std::process::Command::new(windows_powershell())
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded_command(&reading)])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "CN=ThomasT");
    }

    /// Waiting for Windows' question hands back the answer and stops looking
    /// for the dialog as soon as it is answered.
    #[test]
    fn the_question_watch_ends_with_the_answer() {
        let started = std::time::Instant::now();
        assert_eq!(question_on_top(|| 7), 7);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(dialogs().iter().all(|window| *window != 0));
    }

    /// A program not signed with ThomasT's certificate offers nothing: this
    /// unsigned test, and Windows' own, catalog-signed programs.
    #[test]
    fn only_a_program_signed_with_it_counts() {
        assert!(!signs(&std::env::current_exe().unwrap()));
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        assert!(!signs(&std::path::Path::new(&windows).join(r"System32\cmd.exe")));
        assert!(!signs(std::path::Path::new(r"C:\no\such\file.exe")));
    }

    /// What the setup offers to trust is ThomasT's certificate, made for
    /// signing programs and nothing else, and not a certificate authority.
    #[test]
    fn the_certificate_is_thomast_s_and_signs_programs_only() {
        let ours = Certificate::ours().expect("backend/installer/ThomasT.cer is a certificate");
        // SAFETY: `ours` stays alive for every call below; buffers are sized
        // by the calls themselves.
        unsafe {
            let mut name = [0u16; 128];
            let length = CertGetNameStringW(ours.0, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, null(), name.as_mut_ptr(), 128);
            assert_eq!(String::from_utf16_lossy(&name[..length.saturating_sub(1) as usize]), NAME);

            let info = &*(*ours.0).pCertInfo;
            let extension = CertFindExtension(szOID_BASIC_CONSTRAINTS2, info.cExtension, info.rgExtension);
            assert!(!extension.is_null(), "it says whether it is a certificate authority");
            let mut constraints = CERT_BASIC_CONSTRAINTS2_INFO::default();
            let mut size = size_of::<CERT_BASIC_CONSTRAINTS2_INFO>() as u32;
            assert_ne!(
                CryptDecodeObject(
                    X509_ASN_ENCODING,
                    X509_BASIC_CONSTRAINTS2,
                    (*extension).Value.pbData,
                    (*extension).Value.cbData,
                    0,
                    (&raw mut constraints).cast(),
                    &mut size,
                ),
                0
            );
            assert_eq!(constraints.fCA, 0, "not a certificate authority");

            let mut size = 0u32;
            assert_ne!(CertGetEnhancedKeyUsage(ours.0, 0, null_mut(), &mut size), 0);
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let usage = buffer.as_mut_ptr().cast::<CTL_USAGE>();
            assert_ne!(CertGetEnhancedKeyUsage(ours.0, 0, usage, &mut size), 0);
            let purposes: Vec<String> = (0..(*usage).cUsageIdentifier as usize)
                .map(|i| {
                    std::ffi::CStr::from_ptr((*(*usage).rgpszUsageIdentifier.add(i)).cast())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(purposes, ["1.3.6.1.5.5.7.3.3"], "code signing only");
        }
    }
}
