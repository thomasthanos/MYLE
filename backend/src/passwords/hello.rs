// ! Opening the vault with Windows Hello (PIN, fingerprint or face). ! ! Turning it on makes a Windows Hello key for this user on this PC (kept in ! the TPM where there is one) and has it sign a random challenge. The ! signature (RSA PKCS#1 v1.5, the same every time for the same data) is ! hashed into a key that wraps the vault key. Only the wrapped key and the ! challenge are stored, on this PC only, sealed with DPAPI, never synced: ! opening the vault this way needs Windows Hello to sign again, which it ! does only after the user's PIN, fingerprint or face. ! ! The master password still opens the vault, and is the only way on a new ! PC or after Windows Hello was reset.
use std::future::IntoFuture;
use std::path::PathBuf;
use std::time::Duration;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use serde::{Deserialize, Serialize};
use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::core::HSTRING;
use zeroize::Zeroizing;
use super::crypto::{self, Key};
/// The Windows Hello key's name for this app.
const CREDENTIAL: &str = "MakeYourLifeEasier-PasswordVault";
const HELLO_AAD: &str = "myle-vault|hello";
const FILE: &str = "passwords-hello.bin";
#[derive(Deserialize, Serialize)]
struct Stored {
    /// Which vault the wrapped key belongs to.
    vault_id: String,
    challenge: String,
    wrapped_key: String,
}
fn path() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join(FILE))
}
fn load() -> Option<Stored> {
    let bytes = Zeroizing::new(crate::account::vault::load(&path().ok()?)?);
    serde_json::from_slice(&bytes).ok()
}
/// Whether this PC can use Windows Hello at all (a PIN or biometrics set up).
pub fn available() -> bool {
    KeyCredentialManager::IsSupportedAsync()
        .and_then(|op| op.get())
        .unwrap_or(false)
}
/// Whether Windows Hello is on for this vault on this PC.
pub fn enabled(vault_id: &str) -> bool {
    load().is_some_and(|stored| stored.vault_id == vault_id)
}
/// The Windows Hello prompt can open behind the app: bring it forward.
fn focus_prompt() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};
    std::thread::spawn(|| {
        let class: Vec<u16> = "Credential Dialog Xaml Host".encode_utf16().chain(Some(0)).collect();
        for _ in 0..40 {
            let window = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
            if !window.is_null() {
                unsafe { SetForegroundWindow(window) };
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}
fn status_error(status: KeyCredentialStatus) -> String {
    match status {
        KeyCredentialStatus::UserCanceled => "Windows Hello was cancelled.".into(),
        KeyCredentialStatus::NotFound => {
            "Windows Hello was reset on this PC. Open the vault with your master password and turn Windows Hello on again."
                .into()
        }
        _ => "Windows Hello could not confirm it is you. Use your master password.".into(),
    }
}
/// What came of asking the user to confirm it is them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consent {
    Verified,
    /// Cancelled, or not them.
    Refused,
    /// No PIN, fingerprint or face set up on this PC.
    Unavailable,
}
/// A Windows Hello prompt on screen that can be told to stop.
pub(crate) trait Prompt {
    /// Closes the prompt, if it has not answered yet.
    fn cancel(&self);
}
/// Closes the prompt when dropped before it answered: a sign-in cancelled in
/// the browser (or a tab closed) takes its Windows Hello dialog with it,
/// instead of leaving it on screen asking for a PIN nobody needs.
pub(crate) struct CancelOnDrop<P: Prompt> {
    prompt: Option<P>,
}
impl<P: Prompt> CancelOnDrop<P> {
    pub(crate) fn new(prompt: P) -> Self {
        Self { prompt: Some(prompt) }
    }
    /// The prompt answered: nothing to close any more.
    pub(crate) fn answered(&mut self) {
        self.prompt = None;
    }
}
impl<P: Prompt> Drop for CancelOnDrop<P> {
    fn drop(&mut self) {
        if let Some(prompt) = &self.prompt {
            prompt.cancel();
        }
    }
}
type Verification = windows_future::IAsyncOperation<windows::Security::Credentials::UI::UserConsentVerificationResult>;
impl Prompt for Verification {
    fn cancel(&self) {
        if self.Status() == Ok(windows_future::AsyncStatus::Started) {
            let _ = self.Cancel();
        }
    }
}
/// Opens the prompt (the availability check blocks a moment): the running
/// operation, or what already came of it.
fn start_verification(message: &str) -> Result<Verification, Consent> {
    use windows::Security::Credentials::UI::{UserConsentVerifier, UserConsentVerifierAvailability};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    let available = UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get());
    if available != Ok(UserConsentVerifierAvailability::Available) {
        return Err(Consent::Unavailable);
    }
    let message = HSTRING::from(message);
    let front = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    // Owned by the window in front, the prompt shows over it; else on its own.
    let operation = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
        .and_then(|interop| unsafe { interop.RequestVerificationForWindowAsync::<Verification>(HWND(front), &message) })
        .or_else(|_| UserConsentVerifier::RequestVerificationAsync(&message));
    // Windows Hello is there but its prompt could not open: that is no consent (taking it for "no Windows Hello" would skip the check).
    let operation = operation.map_err(|_| Consent::Refused)?;
    focus_prompt();
    Ok(operation)
}
/// What the prompt's answer means.
pub(crate) fn consent_of(result: windows::core::Result<windows::Security::Credentials::UI::UserConsentVerificationResult>) -> Consent {
    use windows::Security::Credentials::UI::UserConsentVerificationResult as Answer;
    match result {
        Ok(Answer::Verified) => Consent::Verified,
        Ok(Answer::DeviceNotPresent | Answer::NotConfiguredForUser | Answer::DisabledByPolicy) => Consent::Unavailable,
        // Cancelled, retries used up, busy, or an error once Windows Hello was known to be there: never "unavailable", which skips the check.
        _ => Consent::Refused,
    }
}
/// One Windows Hello prompt at a time (two sign-ins never stack dialogs).
static PROMPTS: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(Default::default);
/// Asks Windows Hello (PIN, fingerprint or face) to confirm it is the user,
/// over the window in front (the browser, for a passkey). Dropping the future
/// before it answers (the sign-in was cancelled) closes the prompt.
pub async fn verify(message: String) -> Consent {
    let _turn = PROMPTS.lock().await;
    let started = tauri::async_runtime::spawn_blocking(move || start_verification(&message)).await;
    let operation = match started {
        Ok(Ok(operation)) => operation,
        Ok(Err(consent)) => return consent,
        Err(_) => return Consent::Refused,
    };
    let mut prompt = CancelOnDrop::new(operation.clone());
    let result = operation.into_future().await;
    prompt.answered();
    consent_of(result)
}
/// Has the Windows Hello key sign `data` (the user confirms first).
fn sign(credential: &KeyCredential, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let buffer = CryptographicBuffer::CreateFromByteArray(data).map_err(|e| e.to_string())?;
    let operation = credential.RequestSignAsync(&buffer).map_err(|e| e.to_string())?;
    focus_prompt();
    let result = operation.get().map_err(|e| e.to_string())?;
    let status = result.Status().map_err(|e| e.to_string())?;
    if status != KeyCredentialStatus::Success {
        return Err(status_error(status));
    }
    let signed = result.Result().map_err(|e| e.to_string())?;
    let mut bytes = windows::core::Array::<u8>::new();
    CryptographicBuffer::CopyToByteArray(&signed, &mut bytes).map_err(|e| e.to_string())?;
    Ok(Zeroizing::new(bytes.to_vec()))
}
/// Turns Windows Hello on for this vault: one or two Windows Hello prompts.
pub fn enable(vault_key: &Key, vault_id: &str) -> Result<(), String> {
    if !available() {
        return Err("Windows Hello is not set up on this PC. Add a PIN in Windows Settings first.".into());
    }
    let operation = KeyCredentialManager::RequestCreateAsync(
        &HSTRING::from(CREDENTIAL),
        KeyCredentialCreationOption::ReplaceExisting,
    )
    .map_err(|e| e.to_string())?;
    focus_prompt();
    let created = operation.get().map_err(|e| e.to_string())?;
    let status = created.Status().map_err(|e| e.to_string())?;
    if status != KeyCredentialStatus::Success {
        return Err(status_error(status));
    }
    let credential = created.Credential().map_err(|e| e.to_string())?;
    let mut challenge = [0u8; 32];
    OsRng.fill_bytes(&mut challenge);
    let signature = sign(&credential, &challenge)?;
    let key = crypto::key_from_signature(&signature);
    let stored = Stored {
        vault_id: vault_id.to_string(),
        challenge: B64.encode(challenge),
        wrapped_key: crypto::wrap(&key, HELLO_AAD, vault_key)?,
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&stored).map_err(|e| e.to_string())?);
    crate::account::vault::save(&path()?, &bytes)
}
/// The vault key, after Windows Hello confirms it is the user.
pub fn unlock(vault_id: &str) -> Result<Key, String> {
    let stored = load()
        .filter(|stored| stored.vault_id == vault_id)
        .ok_or("Windows Hello is not on for this vault. Use your master password.")?;
    let opened = KeyCredentialManager::OpenAsync(&HSTRING::from(CREDENTIAL))
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;
    let status = opened.Status().map_err(|e| e.to_string())?;
    if status != KeyCredentialStatus::Success {
        if status == KeyCredentialStatus::NotFound {
            disable();
        }
        return Err(status_error(status));
    }
    let credential = opened.Credential().map_err(|e| e.to_string())?;
    let challenge = B64
        .decode(&stored.challenge)
        .map_err(|_| "The Windows Hello data on this PC is damaged.".to_string())?;
    let signature = sign(&credential, &challenge)?;
    let key = crypto::key_from_signature(&signature);
    crypto::unwrap(&key, HELLO_AAD, &stored.wrapped_key).map_err(|_| {
        "Windows Hello no longer matches this vault. Open it with your master password and turn Windows Hello on again."
            .to_string()
    })
}
/// Turns Windows Hello off: forgets the wrapped key and the Windows Hello key.
pub fn disable() {
    if let Ok(path) = path() {
        crate::account::vault::remove(&path);
    }
    let _ = KeyCredentialManager::DeleteAsync(&HSTRING::from(CREDENTIAL)).and_then(|op| op.get());
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use windows::Security::Credentials::UI::UserConsentVerificationResult as Answer;
    struct Fake(Arc<AtomicBool>);
    impl Prompt for Fake {
        fn cancel(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[test]
    fn a_prompt_left_unanswered_is_closed_and_an_answered_one_is_not() {
        let cancelled = Arc::new(AtomicBool::new(false));
        drop(CancelOnDrop::new(Fake(cancelled.clone())));
        assert!(cancelled.load(Ordering::SeqCst), "the sign-in went: its dialog closes");
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut prompt = CancelOnDrop::new(Fake(cancelled.clone()));
        prompt.answered();
        drop(prompt);
        assert!(!cancelled.load(Ordering::SeqCst));
    }
    #[test]
    fn only_windows_hello_saying_it_is_not_set_up_counts_as_unavailable() {
        assert_eq!(consent_of(Ok(Answer::Verified)), Consent::Verified);
        assert_eq!(consent_of(Ok(Answer::NotConfiguredForUser)), Consent::Unavailable);
        assert_eq!(consent_of(Ok(Answer::Canceled)), Consent::Refused);
        assert_eq!(consent_of(Ok(Answer::RetriesExhausted)), Consent::Refused);
        // An error once Windows Hello was there (aborted, say) is no consent.
        let aborted = windows::core::Error::from(windows::core::HRESULT(0x8000_4004_u32 as i32));
        assert_eq!(consent_of(Err(aborted)), Consent::Refused);
    }
}
