// ! Built-in Windows Auto-Logon support. ! ! The webview never supplies an account, registry path, command, or secret. ! The non-elevated process identifies its own token SID and launches this ! executable in a narrowly-scoped helper mode. The helper re-derives the SID ! from the original parent process, validates a password with `LogonUserW`, ! and writes the password only to the documented LSA `DefaultPassword` ! private-data slot.
use std::borrow::Cow;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use uuid::Uuid;
use winreg::enums::{
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_BINARY, REG_DWORD, REG_EXPAND_SZ,
    REG_SZ, RegType,
};
use winreg::{RegKey, RegValue};
use super::models::{
    AutoLogonAccountType, AutoLogonOperation, AutoLogonOutcome, AutoLogonSetResult,
    AutoLogonSnapshot, AutoLogonStatus,
};
const WINLOGON_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
const SYSTEM_POLICY_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
const OWNERSHIP_KEY: &str = r"SOFTWARE\MakeYourLifeEasier\AutoLogon";
const OWNERSHIP_VALUE: &str = "Ownership";
const REQUEST_FILE: &str = "request.json";
const HELPER_ARGUMENT: &str = "--windows-autologon-helper";
const REQUEST_ARGUMENT: &str = "--request";
const FORMAT_VERSION: u32 = 1;
const HELPER_OK: i32 = 0;
const HELPER_CANCELLED: i32 = 20;
const HELPER_CONFLICT: i32 = 21;
const HELPER_UNSUPPORTED: i32 = 22;
const HELPER_FAILED: i32 = 23;
const HELPER_INVALID_CREDENTIALS: i32 = 24;
const HELPER_SID_MISMATCH: i32 = 25;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HelperRequest {
    version: u32,
    action: AutoLogonOperation,
    parent_pid: u32,
    parent_started: u64,
    target_sid: String,
    state_fingerprint: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipMetadata {
    version: u32,
    sid: String,
    applied_user: String,
    applied_domain: String,
    previous_user: Option<StoredRegistryValue>,
    previous_domain: Option<StoredRegistryValue>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredRegistryValue {
    kind: StoredRegistryKind,
    bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum StoredRegistryKind {
    String,
    ExpandString,
}
struct SecretRegistryValue {
    kind: RegType,
    bytes: Vec<u8>,
}
impl Drop for SecretRegistryValue {
    fn drop(&mut self) {
        for byte in &mut self.bytes {
            // SAFETY: the byte reference is valid and the volatile write keeps plaintext rollback material from being optimized back in.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
#[derive(Clone, Debug)]
struct Identity {
    sid: String,
    name: String,
    domain: String,
    upn: Option<String>,
    display_name: String,
    account_type: AutoLogonAccountType,
}
impl Identity {
    fn account_name(&self) -> String {
        match self.account_type {
            AutoLogonAccountType::Microsoft | AutoLogonAccountType::Entra => self
                .upn
                .clone()
                .unwrap_or_else(|| qualified_name(&self.domain, &self.name)),
            AutoLogonAccountType::Domain => self
                .upn
                .clone()
                .unwrap_or_else(|| qualified_name(&self.domain, &self.name)),
            _ => qualified_name(&self.domain, &self.name),
        }
    }
    fn credential_name(&self) -> String {
        match (self.account_type, self.upn.as_deref()) {
            (AutoLogonAccountType::Microsoft, Some(upn)) => {
                format!("MicrosoftAccount\\{upn}")
            }
            (AutoLogonAccountType::Entra, Some(upn)) => format!("AzureAD\\{upn}"),
            (AutoLogonAccountType::Domain, Some(upn)) => upn.to_string(),
            _ => qualified_name(&self.domain, &self.name),
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistrySnapshot {
    auto_admin_logon: Option<String>,
    default_user_name: Option<String>,
    default_domain_name: Option<String>,
    plaintext_password_present: bool,
    force_auto_logon: Option<String>,
    auto_logon_count: Option<String>,
    legal_notice_caption: Option<String>,
    legal_notice_text: Option<String>,
}
impl RegistrySnapshot {
    fn enabled(&self) -> bool {
        self.auto_admin_logon
            .as_deref()
            .is_some_and(|value| value.trim() == "1")
    }
    fn configured_user(&self) -> Option<String> {
        self.default_user_name.as_deref().and_then(|user| {
            let user = user.trim();
            if user.is_empty() {
                None
            } else {
                Some(qualified_name(
                    self.default_domain_name.as_deref().unwrap_or_default(),
                    user,
                ))
            }
        })
    }
    fn blocker(&self) -> Option<String> {
        if self
            .force_auto_logon
            .as_deref()
            .is_some_and(|value| value.trim() == "1")
        {
            return Some(
                "Windows ForceAutoLogon is controlled by another tool or policy. Disable that policy first."
                    .into(),
            );
        }
        if self
            .auto_logon_count
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Some(
                "Windows AutoLogonCount is configured. Remove that external policy first.".into(),
            );
        }
        if self
            .legal_notice_caption
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .legal_notice_text
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        {
            return Some(
                "A Windows sign-in legal notice is configured and prevents automatic logon.".into(),
            );
        }
        None
    }
}
fn qualified_name(domain: &str, user: &str) -> String {
    if domain.trim().is_empty() {
        user.to_string()
    } else {
        format!("{}\\{}", domain.trim(), user.trim())
    }
}
fn same_text(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}
fn ownership_matches(
    ownership: Option<&OwnershipMetadata>,
    identity: &Identity,
    registry: &RegistrySnapshot,
) -> bool {
    ownership.is_some_and(|owner| {
        owner.version == FORMAT_VERSION
            && same_text(&owner.sid, &identity.sid)
            && registry
                .default_user_name
                .as_deref()
                .is_some_and(|value| same_text(value, &owner.applied_user))
            && registry
                .default_domain_name
                .as_deref()
                .unwrap_or_default()
                .eq_ignore_ascii_case(owner.applied_domain.trim())
    })
}
fn ownership_belongs_to_identity(
    ownership: Option<&OwnershipMetadata>,
    identity: &Identity,
) -> bool {
    ownership.is_some_and(|owner| {
        owner.version == FORMAT_VERSION && same_text(&owner.sid, &identity.sid)
    })
}
fn configured_matches_identity(
    identity: &Identity,
    registry: &RegistrySnapshot,
    ownership: Option<&OwnershipMetadata>,
) -> bool {
    if ownership_matches(ownership, identity, registry) {
        return true;
    }
    let Some(user) = registry.default_user_name.as_deref() else {
        return false;
    };
    let domain = registry.default_domain_name.as_deref().unwrap_or_default();
    if same_text(user, &identity.name) && same_text(domain, &identity.domain) {
        return true;
    }
    if identity
        .upn
        .as_deref()
        .is_some_and(|upn| same_text(user, upn))
        && (domain.trim().is_empty()
            || matches!(
                identity.account_type,
                AutoLogonAccountType::Microsoft | AutoLogonAccountType::Entra
            ) && same_text(
                domain,
                match identity.account_type {
                    AutoLogonAccountType::Microsoft => "MicrosoftAccount",
                    AutoLogonAccountType::Entra => "AzureAD",
                    _ => "",
                },
            ))
    {
        return true;
    }
    registry
        .configured_user()
        .and_then(|account| lookup_account_sid(&account).ok())
        .is_some_and(|sid| same_text(&sid, &identity.sid))
}
fn derive_snapshot(
    identity: &Identity,
    registry: &RegistrySnapshot,
    ownership: Option<&OwnershipMetadata>,
    operation: Option<AutoLogonOperation>,
) -> AutoLogonSnapshot {
    let blocker = registry.blocker();
    let belongs_to_current = ownership_belongs_to_identity(ownership, identity);
    let configured_for_current = configured_matches_identity(identity, registry, ownership);
    let (status, blocked_reason) = if operation.is_some() {
        (AutoLogonStatus::Working, None)
    } else if registry.enabled() && !configured_for_current {
        (
            AutoLogonStatus::Conflict,
            Some("Auto-Logon is configured for another or unrecognized Windows account.".into()),
        )
    } else if !registry.enabled() && ownership.is_some() && !belongs_to_current {
        (
            AutoLogonStatus::Conflict,
            Some(
                "An incomplete Auto-Logon configuration belongs to another Windows account. Disable it before enabling this account."
                    .into(),
            ),
        )
    } else if !registry.enabled() && belongs_to_current {
        (
            AutoLogonStatus::Unavailable,
            Some(
                "A previous Auto-Logon operation needs cleanup. Choose Disable, then try Enable again."
                    .into(),
            ),
        )
    } else if !registry.enabled() && registry.plaintext_password_present && !configured_for_current
    {
        (
            AutoLogonStatus::Conflict,
            Some(
                "A password residue for another or unrecognized Auto-Logon account must be disabled first."
                    .into(),
            ),
        )
    } else if let Some(reason) = blocker {
        (AutoLogonStatus::Unavailable, Some(reason))
    } else if registry.enabled() {
        (AutoLogonStatus::Enabled, None)
    } else {
        (AutoLogonStatus::Disabled, None)
    };
    AutoLogonSnapshot {
        status,
        display_name: identity.display_name.clone(),
        account_name: identity.account_name(),
        account_type: identity.account_type,
        configured_user: registry.configured_user(),
        blocked_reason,
        // Also true for a pending recovery marker whose identity writes were only partially completed. The frontend uses this to offer cleanup.
        owned_by_app: belongs_to_current,
        active_operation: operation,
    }
}
fn unavailable_snapshot(reason: impl Into<String>) -> AutoLogonSnapshot {
    AutoLogonSnapshot {
        status: AutoLogonStatus::Unavailable,
        display_name: "Current Windows user".into(),
        account_name: String::new(),
        account_type: AutoLogonAccountType::Unknown,
        configured_user: None,
        blocked_reason: Some(reason.into()),
        owned_by_app: false,
        active_operation: None,
    }
}
pub fn snapshot(app: &AppHandle, operation: Option<AutoLogonOperation>) -> AutoLogonSnapshot {
    match inspect(app) {
        Ok(inspected) => derive_snapshot(
            &inspected.identity,
            &inspected.registry,
            inspected.ownership.as_ref(),
            operation,
        ),
        Err(error) => {
            let mut unavailable = unavailable_snapshot(error);
            unavailable.active_operation = operation;
            if operation.is_some() {
                unavailable.status = AutoLogonStatus::Working;
                unavailable.blocked_reason = None;
            }
            unavailable
        }
    }
}
struct Inspected {
    identity: Identity,
    registry: RegistrySnapshot,
    ownership: Option<OwnershipMetadata>,
}
fn inspect(_app: &AppHandle) -> Result<Inspected, String> {
    let identity = current_identity()?;
    let registry = read_registry()?;
    let ownership = read_ownership();
    Ok(Inspected {
        identity,
        registry,
        ownership,
    })
}
pub async fn set_auto_logon(app: &AppHandle, enabled: bool) -> Result<AutoLogonOutcome, String> {
    let inspected = inspect(app)?;
    let current = derive_snapshot(
        &inspected.identity,
        &inspected.registry,
        inspected.ownership.as_ref(),
        None,
    );
    if enabled {
        match current.status {
            AutoLogonStatus::Enabled => {
                return Ok(outcome(
                    AutoLogonSetResult::Enabled,
                    "Auto-Logon is already enabled for this Windows account.",
                ));
            }
            AutoLogonStatus::Conflict => {
                return Ok(outcome(
                    AutoLogonSetResult::Conflict,
                    current.blocked_reason.as_deref().unwrap_or(
                        "Auto-Logon belongs to another or unrecognized Windows account.",
                    ),
                ));
            }
            AutoLogonStatus::Unavailable => {
                return Ok(outcome(
                    AutoLogonSetResult::Unsupported,
                    current
                        .blocked_reason
                        .as_deref()
                        .unwrap_or("Auto-Logon is unavailable on this system."),
                ));
            }
            AutoLogonStatus::Disabled => {}
            AutoLogonStatus::Working => {
                return Err("An Auto-Logon operation is already running.".into());
            }
        }
    }
    let workspace = HelperWorkspace::create()?;
    let request_path = workspace.request_path();
    let action = if enabled {
        AutoLogonOperation::Enable
    } else {
        AutoLogonOperation::Disable
    };
    let request = HelperRequest {
        version: FORMAT_VERSION,
        action,
        parent_pid: std::process::id(),
        parent_started: current_process_creation_time()?,
        target_sid: inspected.identity.sid.clone(),
        state_fingerprint: state_fingerprint(&inspected.identity.sid, &inspected.registry)?,
    };
    // Keep a no-share-write handle open until the helper exits. The elevated process can read the request, while same-user processes cannot race in and change the requested action after the UAC confirmation appears.
    let _request_lock = write_new_json(request_path, &request)?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate the application executable: {error}"))?;
    let args = vec![
        HELPER_ARGUMENT.to_string(),
        REQUEST_ARGUMENT.to_string(),
        request_path.to_string_lossy().into_owned(),
    ];
    let code = match super::elevated::run(executable, args).await? {
        super::elevated::ElevatedProcessOutcome::Exited(code) => code,
        super::elevated::ElevatedProcessOutcome::Cancelled
        | super::elevated::ElevatedProcessOutcome::NeedsAdmin => {
            return Ok(outcome(
                AutoLogonSetResult::NeedsAdmin,
                "Administrator approval was cancelled or unavailable.",
            ));
        }
    };
    match code {
        code if code == HELPER_OK as u32 && enabled => {
            Ok(AutoLogonOutcome {
                result: AutoLogonSetResult::Enabled,
                note: Some("Auto-Logon was enabled for the verified Windows account.".into()),
            })
        }
        code if code == HELPER_OK as u32 => Ok(AutoLogonOutcome {
            result: AutoLogonSetResult::Disabled,
            note: Some("Auto-Logon was disabled and its secret removed.".into()),
        }),
        code if code == HELPER_CANCELLED as u32 => Ok(outcome(
            AutoLogonSetResult::Cancelled,
            "The Windows credential prompt was cancelled.",
        )),
        code if code == HELPER_CONFLICT as u32 => Ok(outcome(
            AutoLogonSetResult::Conflict,
            "The Windows account or Auto-Logon state changed.",
        )),
        code if code == HELPER_UNSUPPORTED as u32 => Ok(outcome(
            AutoLogonSetResult::Unsupported,
            "Auto-Logon is unavailable on this system.",
        )),
        code if code == HELPER_INVALID_CREDENTIALS as u32 => {
            Err("The password was not accepted for the current Windows account. Use the account password, not a PIN or Windows Hello credential.".into())
        }
        code if code == HELPER_SID_MISMATCH as u32 => Err(
            "The verified credentials belong to a different Windows account. Auto-Logon was not changed."
                .into(),
        ),
        code if code == HELPER_FAILED as u32 => Err(
            "The Auto-Logon operation did not complete. Automatic sign-in was kept off where possible; use Disable again if cleanup is still shown."
                .into(),
        ),
        other => Err(format!(
            "The privileged Auto-Logon helper exited unexpectedly ({other})."
        )),
    }
}
fn outcome(result: AutoLogonSetResult, note: impl Into<String>) -> AutoLogonOutcome {
    AutoLogonOutcome {
        result,
        note: Some(note.into()),
    }
}
pub fn run_helper_from_args() -> Option<i32> {
    let mut arguments = std::env::args_os();
    let _ = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_ARGUMENT)) {
        return None;
    }
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(REQUEST_ARGUMENT)) {
        return Some(HELPER_FAILED);
    }
    let Some(request_path) = arguments.next().map(PathBuf::from) else {
        return Some(HELPER_FAILED);
    };
    if arguments.next().is_some() {
        return Some(HELPER_FAILED);
    }
    Some(run_helper(&request_path))
}
fn run_helper(request_path: &Path) -> i32 {
    let _helper_lock = match HelperMutex::acquire() {
        Ok(Some(lock)) => lock,
        Ok(None) => return HELPER_CONFLICT,
        Err(_) => return HELPER_FAILED,
    };
    match validate_request_path(request_path) {
        Ok(()) => {}
        Err(_) => return HELPER_FAILED,
    }
    let request = match read_bounded_json::<HelperRequest>(request_path, 64 * 1024) {
        Ok(request) if request.version == FORMAT_VERSION => request,
        Ok(_) => return HELPER_UNSUPPORTED,
        Err(_) => return HELPER_FAILED,
    };
    let parent_identity = match identity_from_process(request.parent_pid) {
        Ok(identity) => identity,
        Err(_) => return HELPER_CANCELLED,
    };
    if !parent_process_matches(
        request.parent_pid,
        request.parent_started,
        &request.target_sid,
    ) || !same_text(&parent_identity.sid, &request.target_sid)
    {
        return HELPER_CONFLICT;
    }
    let registry = match read_registry() {
        Ok(registry) => registry,
        Err(_) => return HELPER_FAILED,
    };
    let fingerprint = match state_fingerprint(&parent_identity.sid, &registry) {
        Ok(fingerprint) => fingerprint,
        Err(_) => return HELPER_FAILED,
    };
    if fingerprint != request.state_fingerprint {
        return HELPER_CONFLICT;
    }
    let operation = match request.action {
        AutoLogonOperation::Enable => helper_enable(
            request.parent_pid,
            request.parent_started,
            &parent_identity,
            &registry,
        ),
        AutoLogonOperation::Disable => helper_disable(
            request.parent_pid,
            request.parent_started,
            &parent_identity,
            &registry,
        ),
    };
    match operation {
        Ok(()) => HELPER_OK,
        Err(HelperError::Cancelled(note)) => {
            drop(note);
            HELPER_CANCELLED
        }
        Err(HelperError::Conflict(note)) => {
            drop(note);
            HELPER_CONFLICT
        }
        Err(HelperError::Unsupported(note)) => {
            drop(note);
            HELPER_UNSUPPORTED
        }
        Err(HelperError::InvalidCredentials) => HELPER_INVALID_CREDENTIALS,
        Err(HelperError::SidMismatch) => HELPER_SID_MISMATCH,
        Err(HelperError::Failed(note)) => {
            drop(note);
            HELPER_FAILED
        }
    }
}
struct HelperMutex(windows_sys::Win32::Foundation::HANDLE);
impl HelperMutex {
    fn acquire() -> Result<Option<Self>, String> {
        use windows_sys::Win32::Foundation::{
            CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let name = wide_null(std::ffi::OsStr::new(
            "Global\\MakeYourLifeEasier.AutoLogon.Helper.v1",
        ));
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("Windows could not create the Auto-Logon operation lock.".into());
        }
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Self(handle))),
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Ok(None)
            }
            _ => {
                unsafe { CloseHandle(handle) };
                Err("Windows could not acquire the Auto-Logon operation lock.".into())
            }
        }
    }
}
impl Drop for HelperMutex {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
enum HelperError {
    Cancelled(String),
    Conflict(String),
    Unsupported(String),
    InvalidCredentials,
    SidMismatch,
    Failed(String),
}
fn helper_enable(
    parent_pid: u32,
    parent_started: u64,
    identity: &Identity,
    registry: &RegistrySnapshot,
) -> Result<(), HelperError> {
    if let Some(blocker) = registry.blocker() {
        return Err(HelperError::Unsupported(blocker));
    }
    let ownership = read_ownership();
    if !registry.enabled() {
        if let Some(owner) = ownership.as_ref() {
            return if same_text(&owner.sid, &identity.sid) {
                Err(HelperError::Unsupported(
                    "A previous Auto-Logon operation needs cleanup. Disable it before enabling again."
                        .into(),
                ))
            } else {
                Err(HelperError::Conflict(
                    "An incomplete Auto-Logon configuration belongs to another Windows account."
                        .into(),
                ))
            };
        }
        let lsa_secret_present = LsaPolicy::open()
            .and_then(|policy| policy.retrieve_default_password())
            .map_err(HelperError::Failed)?
            .is_some();
        if (registry.plaintext_password_present || lsa_secret_present)
            && !configured_matches_identity(identity, registry, None)
        {
            return Err(HelperError::Conflict(
                "An Auto-Logon password for another or unrecognized Windows account must be disabled first."
                    .into(),
            ));
        }
    }
    if registry.enabled() {
        if configured_matches_identity(identity, registry, ownership.as_ref()) {
            return Ok(());
        }
        return Err(HelperError::Conflict(
            "Auto-Logon is configured for another Windows account.".into(),
        ));
    }
    let credential = match prompt_for_password(identity) {
        Ok(credential) => credential,
        Err(PromptError::Cancelled) => {
            return Err(HelperError::Cancelled(
                "The Windows credential prompt was cancelled.".into(),
            ));
        }
        Err(PromptError::InvalidCredentials) => return Err(HelperError::InvalidCredentials),
        Err(PromptError::SidMismatch) => return Err(HelperError::SidMismatch),
        Err(PromptError::Failed(error)) => return Err(HelperError::Failed(error)),
    };
    // The prompt may remain open while the app exits or external software changes Winlogon. Re-check both facts immediately before the first write.
    if !parent_process_matches(parent_pid, parent_started, &identity.sid) {
        return Err(HelperError::Cancelled(
            "The requesting application was closed.".into(),
        ));
    }
    let now = read_registry().map_err(HelperError::Failed)?;
    let expected = state_fingerprint(&identity.sid, registry).map_err(HelperError::Failed)?;
    let actual = state_fingerprint(&identity.sid, &now).map_err(HelperError::Failed)?;
    if expected != actual {
        return Err(HelperError::Conflict(
            "Auto-Logon settings changed while credentials were requested.".into(),
        ));
    }
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let winlogon = machine
        .open_subkey_with_flags(WINLOGON_KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| {
            HelperError::Failed(format!("Could not open Windows sign-in settings: {error}"))
        })?;
    let previous = PreviousState {
        auto_admin_logon: stored_value(&winlogon, "AutoAdminLogon").map_err(HelperError::Failed)?,
        default_user_name: stored_value(&winlogon, "DefaultUserName")
            .map_err(HelperError::Failed)?,
        default_domain_name: stored_value(&winlogon, "DefaultDomainName")
            .map_err(HelperError::Failed)?,
        ownership,
    };
    let mut mutator = SystemMutator::open(winlogon).map_err(HelperError::Failed)?;
    apply_enable(
        &mut mutator,
        &credential.password,
        &credential.user,
        &credential.domain,
        &identity.sid,
        &previous,
    )
    .map_err(HelperError::Failed)
}
fn helper_disable(
    parent_pid: u32,
    parent_started: u64,
    identity: &Identity,
    registry: &RegistrySnapshot,
) -> Result<(), HelperError> {
    if !parent_process_matches(parent_pid, parent_started, &identity.sid) {
        return Err(HelperError::Cancelled(
            "The requesting application was closed.".into(),
        ));
    }
    let now = read_registry().map_err(HelperError::Failed)?;
    let expected = state_fingerprint(&identity.sid, registry).map_err(HelperError::Failed)?;
    let actual = state_fingerprint(&identity.sid, &now).map_err(HelperError::Failed)?;
    if expected != actual {
        return Err(HelperError::Conflict(
            "Auto-Logon settings changed before it could be disabled.".into(),
        ));
    }
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let winlogon = machine
        .open_subkey_with_flags(WINLOGON_KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| {
            HelperError::Failed(format!("Could not open Windows sign-in settings: {error}"))
        })?;
    let mut mutator = SystemMutator::open(winlogon).map_err(HelperError::Failed)?;
    apply_disable(&mut mutator, identity, registry, read_ownership().as_ref())
        .map_err(HelperError::Failed)
}
struct VerifiedCredential {
    user: String,
    domain: String,
    password: SecretWide,
}
enum PromptError {
    Cancelled,
    InvalidCredentials,
    SidMismatch,
    Failed(String),
}
fn prompt_for_password(identity: &Identity) -> Result<VerifiedCredential, PromptError> {
    use windows_sys::Win32::Foundation::ERROR_CANCELLED;
    use windows_sys::Win32::Security::Credentials::{
        CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
        CREDUI_FLAGS_EXCLUDE_CERTIFICATES, CREDUI_FLAGS_GENERIC_CREDENTIALS,
        CREDUI_FLAGS_KEEP_USERNAME, CREDUI_FLAGS_PASSWORD_ONLY_OK, CREDUI_INFOW,
        CREDUI_MAX_USERNAME_LENGTH, CredUIParseUserNameW, CredUIPromptForCredentialsW,
    };
    let caption = wide_null(std::ffi::OsStr::new("Enable Windows Auto-Logon"));
    let message = wide_null(std::ffi::OsStr::new(
        "Enter the password for the current Windows account. PIN and Windows Hello credentials cannot be used.",
    ));
    let target = wide_null(std::ffi::OsStr::new("MakeYourLifeEasier.AutoLogon"));
    let mut user_name = vec![0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1];
    copy_wide(&identity.credential_name(), &mut user_name).map_err(PromptError::Failed)?;
    // CREDUI_MAX_PASSWORD_LENGTH is 256 characters; include the terminator.
    let mut password = SecretWide::with_capacity(257);
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        pszMessageText: message.as_ptr(),
        pszCaptionText: caption.as_ptr(),
        ..Default::default()
    };
    let flags = CREDUI_FLAGS_ALWAYS_SHOW_UI
        | CREDUI_FLAGS_DO_NOT_PERSIST
        | CREDUI_FLAGS_EXCLUDE_CERTIFICATES
        | CREDUI_FLAGS_GENERIC_CREDENTIALS
        | CREDUI_FLAGS_KEEP_USERNAME
        | CREDUI_FLAGS_PASSWORD_ONLY_OK;
    // SAFETY: buffers are writable, NUL-terminated, and their element counts are passed exactly. No save checkbox is requested and no credential is persisted by CredUI.
    let status = unsafe {
        CredUIPromptForCredentialsW(
            &info,
            target.as_ptr(),
            std::ptr::null(),
            0,
            user_name.as_mut_ptr(),
            user_name.len() as u32,
            password.as_mut_ptr(),
            password.capacity() as u32,
            std::ptr::null_mut(),
            flags,
        )
    };
    if status == ERROR_CANCELLED {
        return Err(PromptError::Cancelled);
    }
    if status != 0 {
        return Err(PromptError::Failed(format!(
            "Windows could not show the credential prompt (error {status})."
        )));
    }
    password.commit_from_nul();
    let mut parsed_user = vec![0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1];
    let mut parsed_domain = vec![0u16; 338];
    let parsed = unsafe {
        CredUIParseUserNameW(
            user_name.as_ptr(),
            parsed_user.as_mut_ptr(),
            parsed_user.len() as u32,
            parsed_domain.as_mut_ptr(),
            parsed_domain.len() as u32,
        )
    };
    let (user, domain) = if parsed == 0 {
        (
            wide_slice_to_string(&parsed_user),
            wide_slice_to_string(&parsed_domain),
        )
    } else {
        split_credential_name(&wide_slice_to_string(&user_name))
    };
    let validated = validate_credential_sid(&user, &domain, &password, &identity.sid)?;
    // Do not trust text returned by the dialog, even though KEEP_USERNAME and PASSWORD_ONLY_OK lock that field. Rebuild the Winlogon identity from the token returned by LogonUserW after its SID has been matched.
    let (user, domain) = split_credential_name(&validated.credential_name());
    Ok(VerifiedCredential {
        user,
        domain,
        password,
    })
}
fn split_credential_name(value: &str) -> (String, String) {
    if let Some((domain, user)) = value.split_once('\\') {
        (user.to_string(), domain.to_string())
    } else {
        (value.to_string(), String::new())
    }
}
fn validate_credential_sid(
    user: &str,
    domain: &str,
    password: &SecretWide,
    expected_sid: &str,
) -> Result<Identity, PromptError> {
    use windows_sys::Win32::Security::{
        LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW,
    };
    let user = wide_null(std::ffi::OsStr::new(user));
    let domain = (!domain.is_empty()).then(|| wide_null(std::ffi::OsStr::new(domain)));
    let mut token = std::ptr::null_mut();
    let logged_on = unsafe {
        LogonUserW(
            user.as_ptr(),
            domain
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            password.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    };
    if logged_on == 0 {
        return Err(PromptError::InvalidCredentials);
    }
    let identity = identity_from_token_handle(token).map_err(PromptError::Failed);
    unsafe { windows_sys::Win32::Foundation::CloseHandle(token) };
    let identity = identity?;
    if !same_text(&identity.sid, expected_sid) {
        return Err(PromptError::SidMismatch);
    }
    Ok(identity)
}
fn copy_wide(value: &str, destination: &mut [u16]) -> Result<(), String> {
    let encoded: Vec<u16> = value.encode_utf16().collect();
    if encoded.len() + 1 > destination.len() {
        return Err("The Windows account name is too long.".into());
    }
    destination[..encoded.len()].copy_from_slice(&encoded);
    destination[encoded.len()] = 0;
    Ok(())
}
struct SecretWide(Vec<u16>);
impl SecretWide {
    fn with_capacity(words: usize) -> Self {
        Self(vec![0; words])
    }
    fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
    fn as_mut_ptr(&mut self) -> *mut u16 {
        self.0.as_mut_ptr()
    }
    fn capacity(&self) -> usize {
        self.0.len()
    }
    fn commit_from_nul(&mut self) {
        let length = self
            .0
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(self.0.len());
        self.0.truncate(length);
    }
    fn words(&self) -> &[u16] {
        &self.0
    }
}
impl Drop for SecretWide {
    fn drop(&mut self) {
        for word in &mut self.0 {
            // SAFETY: each reference is valid and volatile writes prevent the compiler from optimizing away secret zeroization.
            unsafe { std::ptr::write_volatile(word, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
struct PreviousState {
    auto_admin_logon: Option<StoredRegistryValue>,
    default_user_name: Option<StoredRegistryValue>,
    default_domain_name: Option<StoredRegistryValue>,
    ownership: Option<OwnershipMetadata>,
}
trait Mutation {
    fn retrieve_secret(&mut self) -> Result<Option<SecretWide>, String>;
    fn set_secret(&mut self, value: Option<&[u16]>) -> Result<(), String>;
    fn retrieve_plaintext_password(&mut self) -> Result<Option<SecretRegistryValue>, String>;
    fn delete_plaintext_password(&mut self) -> Result<(), String>;
    fn restore_plaintext_password(
        &mut self,
        value: Option<&SecretRegistryValue>,
    ) -> Result<(), String>;
    fn set_string(&mut self, name: &'static str, value: &str) -> Result<(), String>;
    fn restore_value(
        &mut self,
        name: &'static str,
        value: Option<&StoredRegistryValue>,
    ) -> Result<(), String>;
    fn write_ownership(&mut self, value: &OwnershipMetadata) -> Result<(), String>;
    fn clear_ownership(&mut self) -> Result<(), String>;
}
struct SystemMutator {
    winlogon: RegKey,
    policy: LsaPolicy,
}
impl SystemMutator {
    fn open(winlogon: RegKey) -> Result<Self, String> {
        Ok(Self {
            winlogon,
            policy: LsaPolicy::open()?,
        })
    }
}
impl Mutation for SystemMutator {
    fn retrieve_secret(&mut self) -> Result<Option<SecretWide>, String> {
        self.policy.retrieve_default_password()
    }
    fn set_secret(&mut self, value: Option<&[u16]>) -> Result<(), String> {
        self.policy.store_default_password(value)
    }
    fn retrieve_plaintext_password(&mut self) -> Result<Option<SecretRegistryValue>, String> {
        secret_registry_value(&self.winlogon, "DefaultPassword")
    }
    fn delete_plaintext_password(&mut self) -> Result<(), String> {
        delete_value_if_present(&self.winlogon, "DefaultPassword")
    }
    fn restore_plaintext_password(
        &mut self,
        value: Option<&SecretRegistryValue>,
    ) -> Result<(), String> {
        restore_secret_registry_value(&self.winlogon, "DefaultPassword", value)
    }
    fn set_string(&mut self, name: &'static str, value: &str) -> Result<(), String> {
        self.winlogon
            .set_value(name, &value)
            .map_err(|error| format!("Could not set Windows sign-in value {name}: {error}"))
    }
    fn restore_value(
        &mut self,
        name: &'static str,
        value: Option<&StoredRegistryValue>,
    ) -> Result<(), String> {
        restore_value(&self.winlogon, name, value)
    }
    fn write_ownership(&mut self, value: &OwnershipMetadata) -> Result<(), String> {
        write_ownership(value)
    }
    fn clear_ownership(&mut self) -> Result<(), String> {
        clear_ownership()
    }
}
fn apply_enable<M: Mutation>(
    mutation: &mut M,
    password: &SecretWide,
    user: &str,
    domain: &str,
    sid: &str,
    previous: &PreviousState,
) -> Result<(), String> {
    let (previous_user, previous_domain) = previous
        .ownership
        .as_ref()
        .filter(|owner| same_text(&owner.sid, sid))
        .map(|owner| (owner.previous_user.clone(), owner.previous_domain.clone()))
        .unwrap_or_else(|| {
            (
                previous.default_user_name.clone(),
                previous.default_domain_name.clone(),
            )
        });
    let pending_ownership = OwnershipMetadata {
        version: FORMAT_VERSION,
        sid: sid.to_string(),
        applied_user: user.to_string(),
        applied_domain: domain.to_string(),
        previous_user,
        previous_domain,
    };
    let previous_secret = mutation.retrieve_secret()?;
    let previous_plaintext = mutation.retrieve_plaintext_password()?;
    // This machine-owned marker is deliberately the first mutation. If the process or PC stops at any later point, the next launch can offer a deterministic Disable/cleanup path.
    mutation.write_ownership(&pending_ownership)?;
    if let Err(error) = mutation.set_secret(Some(password.words())) {
        let auto_off = mutation.set_string("AutoAdminLogon", "0").is_ok();
        let secret_restored = mutation
            .set_secret(previous_secret.as_ref().map(SecretWide::words))
            .is_ok();
        let ownership_restored = if auto_off && secret_restored {
            match previous.ownership.as_ref() {
                Some(owner) => mutation.write_ownership(owner).is_ok(),
                None => mutation.clear_ownership().is_ok(),
            }
        } else {
            false
        };
        if !auto_off || !secret_restored || !ownership_restored {
            return Err(format!(
                "{error} Initial secret rollback was incomplete; automatic sign-in was forced off where possible."
            ));
        }
        return Err(error);
    }
    let result = (|| {
        mutation.delete_plaintext_password()?;
        mutation.set_string("DefaultUserName", user)?;
        mutation.set_string("DefaultDomainName", domain)?;
        // This is intentionally the final write. A crash before here cannot activate a partially configured Auto-Logon identity.
        mutation.set_string("AutoAdminLogon", "1")
    })();
    if let Err(error) = result {
        // Force the activation switch off before attempting any restoration. If a rollback step fails, leave it off instead of restoring an ambiguous prior representation.
        let mut rollback_ok = mutation.set_string("AutoAdminLogon", "0").is_ok();
        rollback_ok &= mutation
            .restore_value("DefaultUserName", previous.default_user_name.as_ref())
            .is_ok();
        rollback_ok &= mutation
            .restore_value("DefaultDomainName", previous.default_domain_name.as_ref())
            .is_ok();
        rollback_ok &= mutation
            .restore_plaintext_password(previous_plaintext.as_ref())
            .is_ok();
        rollback_ok &= mutation
            .set_secret(previous_secret.as_ref().map(SecretWide::words))
            .is_ok();
        // Keep the pending marker if any core rollback step failed so cleanup remains reachable. Restore the older marker only after recovery.
        if rollback_ok {
            rollback_ok &= match previous.ownership.as_ref() {
                Some(owner) => mutation.write_ownership(owner).is_ok(),
                None => mutation.clear_ownership().is_ok(),
            };
        }
        if rollback_ok {
            rollback_ok &= mutation
                .restore_value("AutoAdminLogon", previous.auto_admin_logon.as_ref())
                .is_ok();
        }
        if !rollback_ok {
            let _ = mutation.set_string("AutoAdminLogon", "0");
            return Err(format!(
                "{error} Rollback was incomplete; automatic sign-in was forced off."
            ));
        }
        return Err(error);
    }
    Ok(())
}
fn apply_disable<M: Mutation>(
    mutation: &mut M,
    identity: &Identity,
    registry: &RegistrySnapshot,
    ownership: Option<&OwnershipMetadata>,
) -> Result<(), String> {
    // Disabling first is fail-safe: any later partial cleanup cannot cause an automatic sign-in on the next boot.
    mutation.set_string("AutoAdminLogon", "0")?;
    mutation.set_secret(None)?;
    mutation.delete_plaintext_password()?;
    if let Some(owner) = ownership
        .filter(|owner| owner.version == FORMAT_VERSION && same_text(&owner.sid, &identity.sid))
    {
        // Each value is restored only while it still equals the value written by this app. This supports retry after a partial restore without overwriting a later external change.
        if registry
            .default_user_name
            .as_deref()
            .is_some_and(|user| same_text(user, &owner.applied_user))
        {
            mutation.restore_value("DefaultUserName", owner.previous_user.as_ref())?;
        }
        if same_text(
            registry.default_domain_name.as_deref().unwrap_or_default(),
            &owner.applied_domain,
        ) {
            mutation.restore_value("DefaultDomainName", owner.previous_domain.as_ref())?;
        }
    }
    mutation.clear_ownership()
}
struct LsaPolicy(windows_sys::Win32::Security::Authentication::Identity::LSA_HANDLE);
impl LsaPolicy {
    fn open() -> Result<Self, String> {
        use windows_sys::Win32::Security::Authentication::Identity::{
            LSA_OBJECT_ATTRIBUTES, LsaOpenPolicy, POLICY_CREATE_SECRET,
            POLICY_GET_PRIVATE_INFORMATION,
        };
        // Microsoft documents these attributes as reserved for LsaOpenPolicy and requires every member to be zero-initialized.
        let attributes = LSA_OBJECT_ATTRIBUTES::default();
        let mut handle = 0;
        let status = unsafe {
            LsaOpenPolicy(
                std::ptr::null(),
                &attributes,
                (POLICY_CREATE_SECRET | POLICY_GET_PRIVATE_INFORMATION) as u32,
                &mut handle,
            )
        };
        lsa_result(status, "open the Windows LSA policy")?;
        Ok(Self(handle))
    }
    fn retrieve_default_password(&self) -> Result<Option<SecretWide>, String> {
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::Security::Authentication::Identity::{
            LSA_UNICODE_STRING, LsaFreeMemory, LsaNtStatusToWinError, LsaRetrievePrivateData,
        };
        let key = LsaText::new("DefaultPassword")?;
        let mut data: *mut LSA_UNICODE_STRING = std::ptr::null_mut();
        let status = unsafe { LsaRetrievePrivateData(self.0, key.as_ptr(), &mut data) };
        if status != 0 {
            let windows_error = unsafe { LsaNtStatusToWinError(status) };
            if windows_error == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            return Err(format!(
                "Could not read the previous Auto-Logon secret (Windows error {windows_error})."
            ));
        }
        if data.is_null() {
            return Ok(None);
        }
        let (length, maximum, buffer) = unsafe {
            let data_ref = &*data;
            (data_ref.Length, data_ref.MaximumLength, data_ref.Buffer)
        };
        if length == 0 {
            unsafe { LsaFreeMemory(data.cast()) };
            return Ok(Some(SecretWide(Vec::new())));
        }
        if buffer.is_null() || length % 2 != 0 || length > maximum {
            unsafe { LsaFreeMemory(data.cast()) };
            return Err("Windows returned an invalid Auto-Logon secret buffer.".into());
        }
        let words = (length / 2) as usize;
        let value = unsafe { SecretWide(std::slice::from_raw_parts(buffer, words).to_vec()) };
        // LSA owns this allocation. Clear the secret bytes before releasing it in addition to keeping our Rust copy in a zeroizing wrapper.
        unsafe {
            for index in 0..words {
                std::ptr::write_volatile(buffer.add(index), 0);
            }
        }
        unsafe { LsaFreeMemory(data.cast()) };
        Ok(Some(value))
    }
    fn store_default_password(&self, value: Option<&[u16]>) -> Result<(), String> {
        use windows_sys::Win32::Security::Authentication::Identity::LsaStorePrivateData;
        let key = LsaText::new("DefaultPassword")?;
        let data = value.map(LsaText::from_words).transpose()?;
        let status = unsafe {
            LsaStorePrivateData(
                self.0,
                key.as_ptr(),
                data.as_ref().map_or(std::ptr::null(), LsaText::as_ptr),
            )
        };
        lsa_result(status, "update the Auto-Logon secret")
    }
}
impl Drop for LsaPolicy {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Security::Authentication::Identity::LsaClose(self.0);
        }
    }
}
struct LsaText {
    words: Vec<u16>,
    value: windows_sys::Win32::Security::Authentication::Identity::LSA_UNICODE_STRING,
}
impl Drop for LsaText {
    fn drop(&mut self) {
        for word in &mut self.words {
            unsafe { std::ptr::write_volatile(word, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
impl LsaText {
    fn new(value: &str) -> Result<Self, String> {
        Self::from_words(&value.encode_utf16().collect::<Vec<_>>())
    }
    fn from_words(words: &[u16]) -> Result<Self, String> {
        let bytes = words
            .len()
            .checked_mul(2)
            .and_then(|length| u16::try_from(length).ok())
            .ok_or_else(|| "The Auto-Logon secret is too long.".to_string())?;
        let maximum_bytes = words
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_mul(2))
            .and_then(|length| u16::try_from(length).ok())
            .ok_or_else(|| "The Auto-Logon secret is too long.".to_string())?;
        let mut owned = Vec::with_capacity(words.len() + 1);
        owned.extend_from_slice(words);
        owned.push(0);
        let value = windows_sys::Win32::Security::Authentication::Identity::LSA_UNICODE_STRING {
            Length: bytes,
            MaximumLength: maximum_bytes,
            Buffer: owned.as_mut_ptr(),
        };
        Ok(Self {
            words: owned,
            value,
        })
    }
    fn as_ptr(
        &self,
    ) -> *const windows_sys::Win32::Security::Authentication::Identity::LSA_UNICODE_STRING {
        let _keep_alive = &self.words;
        &self.value
    }
}
fn lsa_result(status: i32, action: &str) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    let error = unsafe {
        windows_sys::Win32::Security::Authentication::Identity::LsaNtStatusToWinError(status)
    };
    Err(format!("Could not {action} (Windows error {error})."))
}
struct HelperWorkspace {
    directory: PathBuf,
    request: PathBuf,
}
impl HelperWorkspace {
    fn create() -> Result<Self, String> {
        let root = std::env::temp_dir().join("myle-autologon");
        fs::create_dir_all(&root)
            .map_err(|error| format!("Could not prepare the Auto-Logon workspace: {error}"))?;
        let directory = root.join(Uuid::new_v4().simple().to_string());
        fs::create_dir(&directory)
            .map_err(|error| format!("Could not create the Auto-Logon workspace: {error}"))?;
        let request = directory.join(REQUEST_FILE);
        Ok(Self { directory, request })
    }
    fn request_path(&self) -> &Path {
        &self.request
    }
}
impl Drop for HelperWorkspace {
    fn drop(&mut self) {
        // The helper treats the request as read-only. Cleanup always happens in the unelevated parent, including setup and UAC failure paths.
        let _ = fs::remove_file(&self.request);
        let _ = fs::remove_dir(&self.directory);
    }
}
fn validate_request_path(path: &Path) -> Result<(), String> {
    if path.file_name().and_then(|name| name.to_str()) != Some(REQUEST_FILE) {
        return Err("Invalid Auto-Logon request name.".into());
    }
    let workspace = path
        .parent()
        .ok_or_else(|| "Invalid Auto-Logon workspace.".to_string())?;
    let id = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid Auto-Logon workspace identifier.".to_string())?;
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid Auto-Logon workspace identifier.".into());
    }
    if workspace
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        != Some("myle-autologon")
    {
        return Err("Invalid Auto-Logon workspace root.".into());
    }
    let root = workspace
        .parent()
        .ok_or_else(|| "Invalid Auto-Logon workspace root.".to_string())?;
    let expected_root = std::env::temp_dir().join("myle-autologon");
    let canonical_root = fs::canonicalize(root)
        .map_err(|error| format!("Could not resolve the Auto-Logon workspace: {error}"))?;
    let canonical_expected = fs::canonicalize(&expected_root)
        .map_err(|error| format!("Could not resolve the Auto-Logon workspace root: {error}"))?;
    if !canonical_root
        .to_string_lossy()
        .eq_ignore_ascii_case(&canonical_expected.to_string_lossy())
    {
        return Err("The Auto-Logon request is outside its temporary workspace.".into());
    }
    reject_reparse_point(root)?;
    reject_reparse_point(workspace)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect the Auto-Logon request: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("The Auto-Logon request is not a regular file.".into());
    }
    Ok(())
}
fn reject_reparse_point(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("Auto-Logon temporary paths cannot be reparse points.".into());
    }
    Ok(())
}
fn state_fingerprint(sid: &str, registry: &RegistrySnapshot) -> Result<String, String> {
    let encoded = serde_json::to_vec(&(FORMAT_VERSION, sid, registry))
        .map_err(|error| format!("Could not fingerprint Auto-Logon state: {error}"))?;
    Ok(Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<fs::File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("Could not encode Auto-Logon data: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    Ok(file)
}
fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path, maximum: u64) -> Result<T, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut bytes = Vec::with_capacity((maximum.min(64 * 1024) + 1) as usize);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if bytes.len() as u64 > maximum {
        return Err("Auto-Logon data exceeded the allowed size.".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}
fn read_registry() -> Result<RegistrySnapshot, String> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let winlogon = machine
        .open_subkey_with_flags(WINLOGON_KEY, KEY_READ | KEY_WOW64_64KEY)
        .map_err(|error| format!("Could not read Windows sign-in settings: {error}"))?;
    let policy = machine
        .open_subkey_with_flags(SYSTEM_POLICY_KEY, KEY_READ | KEY_WOW64_64KEY)
        .ok();
    Ok(RegistrySnapshot {
        auto_admin_logon: optional_scalar(&winlogon, "AutoAdminLogon"),
        default_user_name: optional_string(&winlogon, "DefaultUserName"),
        default_domain_name: optional_string(&winlogon, "DefaultDomainName"),
        plaintext_password_present: winlogon.get_raw_value("DefaultPassword").is_ok(),
        force_auto_logon: optional_scalar(&winlogon, "ForceAutoLogon"),
        auto_logon_count: optional_scalar(&winlogon, "AutoLogonCount"),
        legal_notice_caption: policy
            .as_ref()
            .and_then(|key| optional_string(key, "legalnoticecaption")),
        legal_notice_text: policy
            .as_ref()
            .and_then(|key| optional_string(key, "legalnoticetext")),
    })
}
fn optional_string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name).ok()
}
fn optional_scalar(key: &RegKey, name: &str) -> Option<String> {
    let raw = key.get_raw_value(name).ok()?;
    match raw.vtype {
        REG_SZ | REG_EXPAND_SZ => key.get_value::<String, _>(name).ok(),
        REG_DWORD if raw.bytes.len() >= 4 => {
            Some(u32::from_le_bytes(raw.bytes[..4].try_into().ok()?).to_string())
        }
        _ => None,
    }
}
fn stored_value(key: &RegKey, name: &str) -> Result<Option<StoredRegistryValue>, String> {
    match key.get_raw_value(name) {
        Ok(value) if value.bytes.len() <= 16 * 1024 => {
            let kind = match value.vtype {
                REG_SZ => StoredRegistryKind::String,
                REG_EXPAND_SZ => StoredRegistryKind::ExpandString,
                _ => return Ok(None),
            };
            Ok(Some(StoredRegistryValue {
                kind,
                bytes: value.bytes.into_owned(),
            }))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Could not read Windows sign-in value {name}: {error}"
        )),
    }
}
fn secret_registry_value(key: &RegKey, name: &str) -> Result<Option<SecretRegistryValue>, String> {
    match key.get_raw_value(name) {
        Ok(value) if value.bytes.len() <= 64 * 1024 => Ok(Some(SecretRegistryValue {
            kind: value.vtype,
            bytes: value.bytes.into_owned(),
        })),
        Ok(_) => Err(format!(
            "Windows sign-in value {name} is unexpectedly large."
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Could not read Windows sign-in value {name}: {error}"
        )),
    }
}
fn restore_secret_registry_value(
    key: &RegKey,
    name: &str,
    value: Option<&SecretRegistryValue>,
) -> Result<(), String> {
    if let Some(value) = value {
        key.set_raw_value(
            name,
            &RegValue {
                bytes: Cow::Borrowed(&value.bytes),
                vtype: value.kind.clone(),
            },
        )
        .map_err(|error| format!("Could not restore Windows sign-in value {name}: {error}"))
    } else {
        delete_value_if_present(key, name)
    }
}
fn restore_value(
    key: &RegKey,
    name: &str,
    value: Option<&StoredRegistryValue>,
) -> Result<(), String> {
    if let Some(value) = value {
        if value.bytes.len() > 16 * 1024 {
            return Err(format!("Stored Windows sign-in value {name} is invalid."));
        }
        let vtype = match value.kind {
            StoredRegistryKind::String => REG_SZ,
            StoredRegistryKind::ExpandString => REG_EXPAND_SZ,
        };
        key.set_raw_value(
            name,
            &RegValue {
                bytes: Cow::Borrowed(&value.bytes),
                vtype,
            },
        )
        .map_err(|error| format!("Could not restore Windows sign-in value {name}: {error}"))
    } else {
        delete_value_if_present(key, name)
    }
}
fn delete_value_if_present(key: &RegKey, name: &str) -> Result<(), String> {
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not remove Windows sign-in value {name}: {error}"
        )),
    }
}
fn read_ownership() -> Option<OwnershipMetadata> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = machine
        .open_subkey_with_flags(OWNERSHIP_KEY, KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    let raw = key.get_raw_value(OWNERSHIP_VALUE).ok()?;
    if raw.vtype != REG_BINARY || raw.bytes.len() > 64 * 1024 {
        return None;
    }
    let value: OwnershipMetadata = serde_json::from_slice(&raw.bytes).ok()?;
    (value.version == FORMAT_VERSION
        && value.applied_user.encode_utf16().count() <= 512
        && value.applied_domain.encode_utf16().count() <= 336)
        .then_some(value)
}
fn write_ownership(value: &OwnershipMetadata) -> Result<(), String> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| format!("Could not encode Auto-Logon ownership: {error}"))?;
    if encoded.len() > 64 * 1024 {
        return Err("Auto-Logon ownership data is too large.".into());
    }
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = machine
        .create_subkey_with_flags(OWNERSHIP_KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("Could not create Auto-Logon ownership storage: {error}"))?;
    key.set_raw_value(
        OWNERSHIP_VALUE,
        &RegValue {
            bytes: Cow::Owned(encoded),
            vtype: REG_BINARY,
        },
    )
    .map_err(|error| format!("Could not save Auto-Logon ownership: {error}"))
}
fn clear_ownership() -> Result<(), String> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = match machine
        .open_subkey_with_flags(OWNERSHIP_KEY, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
    {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Could not open Auto-Logon ownership storage: {error}"
            ));
        }
    };
    match key.delete_value(OWNERSHIP_VALUE) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not clear Auto-Logon ownership: {error}")),
    }
}
#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}
fn current_identity() -> Result<Identity, String> {
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    let mut token = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a process pseudo-handle valid in the current process; only the real token handle returned here is closed.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err("Could not read the Windows account token.".into());
    }
    let result = (|| {
        let mut identity = base_identity_from_token(token)?;
        if let Some(display) =
            extended_user_name(windows_sys::Win32::Security::Authentication::Identity::NameDisplay)
        {
            identity.display_name = display;
        }
        identity.upn = extended_user_name(
            windows_sys::Win32::Security::Authentication::Identity::NameUserPrincipal,
        )
        .filter(|value| !value.trim().is_empty());
        identity.account_type = classify_account(&identity.domain, identity.upn.as_deref());
        Ok(identity)
    })();
    unsafe { windows_sys::Win32::Foundation::CloseHandle(token) };
    result
}
fn identity_from_process(process_id: u32) -> Result<Identity, String> {
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: OpenProcess is called with a concrete PID and read/synchronize rights only. The returned handle is closed before returning.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            process_id,
        )
    };
    if process.is_null() {
        return Err("The requesting application process is no longer available.".into());
    }
    let identity = identity_from_process_handle(process);
    unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
    identity
}
fn identity_from_process_handle(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> Result<Identity, String> {
    use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    let mut token = std::ptr::null_mut();
    // SAFETY: process is a valid process handle or pseudo-handle; token is an out pointer and the resulting handle is closed below.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) } == 0 {
        return Err("Could not read the Windows account token.".into());
    }
    let result = identity_from_token_handle(token);
    unsafe { windows_sys::Win32::Foundation::CloseHandle(token) };
    result
}
fn identity_from_token_handle(
    token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<Identity, String> {
    let mut identity = base_identity_from_token(token)?;
    let (display_name, upn) = extended_names_for_token(token)?;
    identity.upn = upn.filter(|value| !value.trim().is_empty());
    identity.display_name = display_name.unwrap_or_else(|| identity.name.clone());
    identity.account_type = classify_account(&identity.domain, identity.upn.as_deref());
    Ok(identity)
}
fn base_identity_from_token(
    token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<Identity, String> {
    use windows_sys::Win32::Security::{TOKEN_USER, TokenUser};
    let buffer = token_information(token, TokenUser)?;
    // The Vec<usize> used by token_information is pointer-aligned.
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let sid = sid_to_string(user.User.Sid)?;
    let (name, domain) = lookup_sid_name(user.User.Sid)?;
    Ok(Identity {
        sid,
        name: name.clone(),
        domain,
        upn: None,
        display_name: name,
        account_type: AutoLogonAccountType::Unknown,
    })
}
fn extended_names_for_token(
    token: windows_sys::Win32::Foundation::HANDLE,
) -> Result<(Option<String>, Option<String>), String> {
    use windows_sys::Win32::Security::Authentication::Identity::{NameDisplay, NameUserPrincipal};
    use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
    // GetUserNameExW reads the current thread security context. Temporarily impersonating lets an elevated helper resolve the original caller's UPN instead of accidentally using the administrator account that approved UAC.
    if unsafe { ImpersonateLoggedOnUser(token) } == 0 {
        return Ok((None, None));
    }
    let display = extended_user_name(NameDisplay);
    let upn = extended_user_name(NameUserPrincipal);
    if unsafe { RevertToSelf() } == 0 {
        return Err("Windows could not restore the helper security context.".into());
    }
    Ok((display, upn))
}
fn token_information(
    token: windows_sys::Win32::Foundation::HANDLE,
    class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, String> {
    use windows_sys::Win32::Security::GetTokenInformation;
    let mut bytes = 0u32;
    unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut bytes) };
    if bytes == 0 {
        return Err("Windows returned an empty account token.".into());
    }
    let words = (bytes as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; words];
    if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), bytes, &mut bytes) }
        == 0
    {
        return Err("Could not read the Windows account SID.".into());
    }
    Ok(buffer)
}
fn sid_to_string(sid: windows_sys::Win32::Security::PSID) -> Result<String, String> {
    use windows_sys::Win32::Security::{Authorization::ConvertSidToStringSidW, IsValidSid};
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err("Windows returned an invalid account SID.".into());
    }
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err("Could not format the Windows account SID.".into());
    }
    let value = unsafe { wide_pointer_to_string(text) };
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(text.cast());
    }
    Ok(value)
}
fn lookup_sid_name(sid: windows_sys::Win32::Security::PSID) -> Result<(String, String), String> {
    use windows_sys::Win32::Security::{LookupAccountSidW, SID_NAME_USE};
    let mut name_length = 0u32;
    let mut domain_length = 0u32;
    let mut use_type: SID_NAME_USE = 0;
    unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            std::ptr::null_mut(),
            &mut name_length,
            std::ptr::null_mut(),
            &mut domain_length,
            &mut use_type,
        )
    };
    if name_length == 0 {
        return Err("Could not resolve the Windows account name.".into());
    }
    let mut name = vec![0u16; name_length as usize];
    let mut domain = vec![0u16; domain_length.max(1) as usize];
    if unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut use_type,
        )
    } == 0
    {
        return Err("Could not resolve the Windows account name.".into());
    }
    Ok((wide_slice_to_string(&name), wide_slice_to_string(&domain)))
}
fn lookup_account_sid(account: &str) -> Result<String, String> {
    use windows_sys::Win32::Security::{LookupAccountNameW, SID_NAME_USE};
    let account = wide_null(std::ffi::OsStr::new(account));
    let mut sid_length = 0u32;
    let mut domain_length = 0u32;
    let mut use_type: SID_NAME_USE = 0;
    unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            account.as_ptr(),
            std::ptr::null_mut(),
            &mut sid_length,
            std::ptr::null_mut(),
            &mut domain_length,
            &mut use_type,
        )
    };
    if sid_length == 0 {
        return Err("Windows could not resolve the configured Auto-Logon account.".into());
    }
    let words = (sid_length as usize).div_ceil(std::mem::size_of::<usize>());
    let mut sid = vec![0usize; words];
    let mut domain = vec![0u16; domain_length.max(1) as usize];
    if unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            account.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut use_type,
        )
    } == 0
    {
        return Err("Windows could not resolve the configured Auto-Logon account.".into());
    }
    sid_to_string(sid.as_mut_ptr().cast())
}
fn extended_user_name(format: i32) -> Option<String> {
    use windows_sys::Win32::Security::Authentication::Identity::GetUserNameExW;
    let mut length = 0u32;
    unsafe { GetUserNameExW(format, std::ptr::null_mut(), &mut length) };
    if length == 0 {
        return None;
    }
    let mut value = vec![0u16; length as usize];
    if unsafe { GetUserNameExW(format, value.as_mut_ptr(), &mut length) } {
        Some(wide_slice_to_string(&value))
    } else {
        None
    }
}
fn classify_account(domain: &str, upn: Option<&str>) -> AutoLogonAccountType {
    if same_text(domain, "MicrosoftAccount") {
        AutoLogonAccountType::Microsoft
    } else if same_text(domain, "AzureAD") {
        AutoLogonAccountType::Entra
    } else if domain.trim().is_empty()
        || computer_name().is_some_and(|name| same_text(domain, &name))
    {
        if upn.is_some_and(|value| value.contains('@')) {
            AutoLogonAccountType::Microsoft
        } else {
            AutoLogonAccountType::Local
        }
    } else if !domain.trim().is_empty() {
        AutoLogonAccountType::Domain
    } else {
        AutoLogonAccountType::Unknown
    }
}
fn computer_name() -> Option<String> {
    use windows_sys::Win32::System::WindowsProgramming::GetComputerNameW;
    let mut value = vec![0u16; 256];
    let mut length = value.len() as u32;
    if unsafe { GetComputerNameW(value.as_mut_ptr(), &mut length) } != 0 {
        value.truncate(length as usize);
        Some(String::from_utf16_lossy(&value))
    } else {
        None
    }
}
fn current_process_creation_time() -> Result<u64, String> {
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    process_creation_time(unsafe { GetCurrentProcess() })
}
fn process_creation_time(process: windows_sys::Win32::Foundation::HANDLE) -> Result<u64, String> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err("Could not identify the requesting application process.".into());
    }
    Ok(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
}
fn parent_process_matches(process_id: u32, expected_started: u64, expected_sid: &str) -> bool {
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        QueryFullProcessImageNameW, WaitForSingleObject,
    };
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            process_id,
        )
    };
    if process.is_null() {
        return false;
    }
    // A PID may be reused while the credential dialog is open. Use the same live process handle for the liveness, SID and image checks.
    if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
        return false;
    }
    let creation_matches =
        process_creation_time(process).is_ok_and(|time| time == expected_started);
    let sid_matches = identity_from_process_handle(process)
        .is_ok_and(|identity| same_text(&identity.sid, expected_sid));
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let read = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
    if !creation_matches || !sid_matches || read == 0 {
        return false;
    }
    buffer.truncate(length as usize);
    let parent = PathBuf::from(String::from_utf16_lossy(&buffer));
    match (
        fs::canonicalize(parent),
        std::env::current_exe().and_then(fs::canonicalize),
    ) {
        (Ok(parent), Ok(current)) => parent
            .to_string_lossy()
            .eq_ignore_ascii_case(&current.to_string_lossy()),
        _ => false,
    }
}
fn wide_slice_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}
unsafe fn wide_pointer_to_string(value: *const u16) -> String {
    let mut length = 0usize;
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) })
}
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;
    fn identity() -> Identity {
        Identity {
            sid: "S-1-5-21-1000".into(),
            name: "Thomas".into(),
            domain: "WORKSTATION".into(),
            upn: None,
            display_name: "Thomas".into(),
            account_type: AutoLogonAccountType::Local,
        }
    }
    fn registry(enabled: bool) -> RegistrySnapshot {
        RegistrySnapshot {
            auto_admin_logon: Some(if enabled { "1" } else { "0" }.into()),
            default_user_name: Some("Thomas".into()),
            default_domain_name: Some("WORKSTATION".into()),
            ..RegistrySnapshot::default()
        }
    }
    fn stored_text(value: &str) -> StoredRegistryValue {
        let bytes = value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        StoredRegistryValue {
            kind: StoredRegistryKind::String,
            bytes,
        }
    }
    fn ownership() -> OwnershipMetadata {
        OwnershipMetadata {
            version: FORMAT_VERSION,
            sid: identity().sid,
            applied_user: "Thomas".into(),
            applied_domain: "WORKSTATION".into(),
            previous_user: Some(stored_text("BeforeUser")),
            previous_domain: Some(stored_text("BeforeDomain")),
        }
    }
    #[test]
    fn status_precedence_keeps_enabled_configuration_removable() {
        let identity = identity();
        let clean = registry(false);
        assert_eq!(
            derive_snapshot(&identity, &clean, None, None).status,
            AutoLogonStatus::Disabled
        );
        let mut enabled = registry(true);
        enabled.force_auto_logon = Some("1".into());
        let snapshot = derive_snapshot(&identity, &enabled, None, None);
        assert_eq!(snapshot.status, AutoLogonStatus::Unavailable);
        assert!(snapshot.blocked_reason.is_some());
        enabled.default_user_name = Some("SomeoneElse".into());
        assert_eq!(
            derive_snapshot(&identity, &enabled, None, None).status,
            AutoLogonStatus::Conflict
        );
        let mut blocked = registry(false);
        blocked.auto_logon_count = Some("0".into());
        assert_eq!(
            derive_snapshot(&identity, &blocked, None, None).status,
            AutoLogonStatus::Unavailable
        );
        assert_eq!(
            derive_snapshot(&identity, &blocked, None, Some(AutoLogonOperation::Enable),).status,
            AutoLogonStatus::Working
        );
        let pending_owner = ownership();
        let pending = derive_snapshot(&identity, &clean, Some(&pending_owner), None);
        assert_eq!(pending.status, AutoLogonStatus::Unavailable);
        assert!(pending.owned_by_app);
        assert!(pending.blocked_reason.is_some());
        let mut foreign_owner = pending_owner;
        foreign_owner.sid = "S-1-5-21-OTHER".into();
        let foreign = derive_snapshot(&identity, &clean, Some(&foreign_owner), None);
        assert_eq!(foreign.status, AutoLogonStatus::Conflict);
        assert!(!foreign.owned_by_app);
    }
    #[test]
    fn account_classification_handles_cloud_and_domain_identities() {
        assert_eq!(
            classify_account("MicrosoftAccount", Some("user@example.com")),
            AutoLogonAccountType::Microsoft
        );
        assert_eq!(
            classify_account("AzureAD", Some("user@tenant.example")),
            AutoLogonAccountType::Entra
        );
        assert_eq!(
            classify_account("CONTOSO", Some("user@contoso.example")),
            AutoLogonAccountType::Domain
        );
        assert_eq!(
            classify_account("", Some("user@example.com")),
            AutoLogonAccountType::Microsoft
        );
        assert_eq!(classify_account("", None), AutoLogonAccountType::Local);
    }
    struct MockMutation {
        events: Vec<String>,
        fail_once: Option<String>,
        secret: Option<Vec<u16>>,
        plaintext: Option<(RegType, Vec<u8>)>,
        strings: HashMap<&'static str, String>,
        ownership: Option<OwnershipMetadata>,
    }
    impl Default for MockMutation {
        fn default() -> Self {
            Self {
                events: Vec::new(),
                fail_once: None,
                secret: Some("old-secret".encode_utf16().collect()),
                plaintext: Some((
                    REG_SZ,
                    "old-plain\0"
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                )),
                strings: HashMap::new(),
                ownership: None,
            }
        }
    }
    impl MockMutation {
        fn step(&mut self, event: impl Into<String>) -> Result<(), String> {
            let event = event.into();
            self.events.push(event.clone());
            if self.fail_once.as_deref() == Some(&event) {
                self.fail_once = None;
                Err(format!("injected failure at {event}"))
            } else {
                Ok(())
            }
        }
    }
    impl Mutation for MockMutation {
        fn retrieve_secret(&mut self) -> Result<Option<SecretWide>, String> {
            self.step("retrieve-secret")?;
            Ok(self.secret.take().map(SecretWide))
        }
        fn set_secret(&mut self, value: Option<&[u16]>) -> Result<(), String> {
            self.step(if value.is_some() {
                "set-secret"
            } else {
                "clear-secret"
            })?;
            self.secret = value.map(ToOwned::to_owned);
            Ok(())
        }
        fn retrieve_plaintext_password(&mut self) -> Result<Option<SecretRegistryValue>, String> {
            self.step("retrieve-plaintext")?;
            Ok(self
                .plaintext
                .take()
                .map(|(kind, bytes)| SecretRegistryValue { kind, bytes }))
        }
        fn delete_plaintext_password(&mut self) -> Result<(), String> {
            self.step("delete-plaintext")?;
            self.plaintext = None;
            Ok(())
        }
        fn restore_plaintext_password(
            &mut self,
            value: Option<&SecretRegistryValue>,
        ) -> Result<(), String> {
            self.step("restore-plaintext")?;
            self.plaintext = value.map(|value| (value.kind.clone(), value.bytes.clone()));
            Ok(())
        }
        fn set_string(&mut self, name: &'static str, value: &str) -> Result<(), String> {
            self.step(format!("set:{name}={value}"))?;
            self.strings.insert(name, value.into());
            Ok(())
        }
        fn restore_value(
            &mut self,
            name: &'static str,
            value: Option<&StoredRegistryValue>,
        ) -> Result<(), String> {
            self.step(format!("restore:{name}"))?;
            if value.is_some() {
                self.strings.insert(name, "restored".into());
            } else {
                self.strings.remove(name);
            }
            Ok(())
        }
        fn write_ownership(&mut self, value: &OwnershipMetadata) -> Result<(), String> {
            self.step("write-ownership")?;
            self.ownership = Some(value.clone());
            Ok(())
        }
        fn clear_ownership(&mut self) -> Result<(), String> {
            self.step("clear-ownership")?;
            self.ownership = None;
            Ok(())
        }
    }
    fn previous_state() -> PreviousState {
        PreviousState {
            auto_admin_logon: Some(stored_text("0")),
            default_user_name: Some(stored_text("BeforeUser")),
            default_domain_name: Some(stored_text("BeforeDomain")),
            ownership: None,
        }
    }
    #[test]
    fn enable_writes_activation_switch_last() {
        let mut mutation = MockMutation::default();
        apply_enable(
            &mut mutation,
            &SecretWide("new-secret".encode_utf16().collect()),
            "Thomas",
            "WORKSTATION",
            "S-1-5-21-1000",
            &previous_state(),
        )
        .unwrap();
        assert_eq!(
            mutation.events,
            [
                "retrieve-secret",
                "retrieve-plaintext",
                "write-ownership",
                "set-secret",
                "delete-plaintext",
                "set:DefaultUserName=Thomas",
                "set:DefaultDomainName=WORKSTATION",
                "set:AutoAdminLogon=1",
            ]
        );
    }
    #[test]
    fn failed_enable_forces_off_then_rolls_back_secrets_and_identity() {
        let mut mutation = MockMutation {
            fail_once: Some("set:AutoAdminLogon=1".into()),
            ..MockMutation::default()
        };
        let result = apply_enable(
            &mut mutation,
            &SecretWide("new-secret".encode_utf16().collect()),
            "Thomas",
            "WORKSTATION",
            "S-1-5-21-1000",
            &previous_state(),
        );
        assert!(result.is_err());
        let failed_at = mutation
            .events
            .iter()
            .position(|event| event == "set:AutoAdminLogon=1")
            .unwrap();
        assert_eq!(mutation.events[failed_at + 1], "set:AutoAdminLogon=0");
        assert!(mutation.events.contains(&"restore-plaintext".into()));
        assert!(mutation.events.contains(&"restore:AutoAdminLogon".into()));
        assert_eq!(
            mutation.secret.as_deref(),
            Some("old-secret".encode_utf16().collect::<Vec<_>>().as_slice())
        );
        assert!(mutation.plaintext.is_some());
    }
    #[test]
    fn failed_initial_secret_write_restores_old_secret_and_keeps_auto_logon_off() {
        let mut mutation = MockMutation {
            fail_once: Some("set-secret".into()),
            ..MockMutation::default()
        };
        let result = apply_enable(
            &mut mutation,
            &SecretWide("new-secret".encode_utf16().collect()),
            "Thomas",
            "WORKSTATION",
            "S-1-5-21-1000",
            &previous_state(),
        );
        assert!(result.is_err());
        assert_eq!(
            mutation.events,
            [
                "retrieve-secret",
                "retrieve-plaintext",
                "write-ownership",
                "set-secret",
                "set:AutoAdminLogon=0",
                "set-secret",
                "clear-ownership",
            ]
        );
        assert_eq!(
            mutation.secret.as_deref(),
            Some("old-secret".encode_utf16().collect::<Vec<_>>().as_slice())
        );
    }
    #[test]
    fn disable_is_fail_safe_and_restores_only_owned_names() {
        let mut mutation = MockMutation::default();
        let identity = identity();
        let registry = registry(true);
        apply_disable(&mut mutation, &identity, &registry, Some(&ownership())).unwrap();
        assert_eq!(
            mutation.events,
            [
                "set:AutoAdminLogon=0",
                "clear-secret",
                "delete-plaintext",
                "restore:DefaultUserName",
                "restore:DefaultDomainName",
                "clear-ownership",
            ]
        );
        let mut foreign = MockMutation::default();
        let mut foreign_owner = ownership();
        foreign_owner.sid = "S-1-5-21-OTHER".into();
        apply_disable(&mut foreign, &identity, &registry, Some(&foreign_owner)).unwrap();
        assert!(
            !foreign
                .events
                .iter()
                .any(|event| event.starts_with("restore:Default"))
        );
        assert_eq!(foreign.events.last().unwrap(), "clear-ownership");
    }
    #[test]
    fn partial_disable_keeps_recovery_marker_and_retry_converges() {
        let identity = identity();
        let owner = ownership();
        let mut first = MockMutation {
            fail_once: Some("restore:DefaultDomainName".into()),
            ownership: Some(owner.clone()),
            ..MockMutation::default()
        };
        let result = apply_disable(&mut first, &identity, &registry(true), Some(&owner));
        assert!(result.is_err());
        assert!(first.ownership.is_some());
        assert!(!first.events.iter().any(|event| event == "clear-ownership"));
        let mut retry_registry = registry(false);
        retry_registry.default_user_name = Some("BeforeUser".into());
        let mut retry = MockMutation {
            ownership: Some(owner.clone()),
            ..MockMutation::default()
        };
        apply_disable(&mut retry, &identity, &retry_registry, Some(&owner)).unwrap();
        assert!(
            !retry
                .events
                .iter()
                .any(|event| event == "restore:DefaultUserName")
        );
        assert!(
            retry
                .events
                .iter()
                .any(|event| event == "restore:DefaultDomainName")
        );
        assert_eq!(retry.events.last().unwrap(), "clear-ownership");
        assert!(retry.ownership.is_none());
    }
    #[test]
    fn helper_workspace_cleans_request_on_drop() {
        let request;
        let directory;
        {
            let workspace = HelperWorkspace::create().unwrap();
            request = workspace.request.clone();
            directory = workspace.directory.clone();
            let request_lock = write_new_json(
                &request,
                &HelperRequest {
                    version: FORMAT_VERSION,
                    action: AutoLogonOperation::Disable,
                    parent_pid: 1,
                    parent_started: 1,
                    target_sid: "S-1-0-0".into(),
                    state_fingerprint: "test".into(),
                },
            )
            .unwrap();
            assert!(request.exists());
            assert!(OpenOptions::new().write(true).open(&request).is_err());
            assert!(validate_request_path(&request).is_ok());
            let parsed: HelperRequest = read_bounded_json(&request, 64 * 1024).unwrap();
            assert_eq!(parsed.parent_started, 1);
            drop(request_lock);
        }
        assert!(!request.exists());
        assert!(!directory.exists());
    }
}
