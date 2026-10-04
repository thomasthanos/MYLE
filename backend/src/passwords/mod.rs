//! The Password Manager: an end-to-end encrypted vault (`vault.rs`,
//! `crypto.rs`). The page never receives the vault key; a password reaches
//! it only when the user reveals it, and copying goes through
//! `clipboard.rs` without passing through the page at all.

pub mod browser;
mod clipboard;
mod hello;
pub mod icons;
mod qr;
mod session;
pub mod windows_fill;

// The vault itself, shared with MYLE Passwords for phones.
use myle_vault::{VaultCell, crypto, generator, import, passkeys, sync, totp, vault};

pub use windows_fill::WindowsFillState;

pub fn start_windows_fill(app: AppHandle, target: WindowsFillState) {
    windows_fill::start(app, target);
}

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::storage;
use crypto::KdfParams;
use vault::{EntryInput, OldPassword, Status, StatusInfo, Summary, Vault, check_master};

/// Sent to the page when the vault locks itself.
const LOCKED_EVENT: &str = "passwords-locked";
/// Sent when an entry changed outside the page (saved from the browser).
const CHANGED_EVENT: &str = "passwords-changed";
/// Sent after a background sync, so an open page shows what came in.
const SYNCED_EVENT: &str = "passwords-synced";
/// Asks the page for the master password, for a passkey (see `ask_master`).
const VERIFY_EVENT: &str = "passwords-verify";
/// How often the vault syncs by itself while the app runs.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);

/// One sync at a time: a background pass and "Sync now" must not interleave.
static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn sync_once(app: &AppHandle, state: &PasswordsState) -> Result<sync::SyncResult, String> {
    let _one = SYNCING.lock().await;
    sync::run(crate::account::cloud(app).await?, &state.cell).await
}
/// Larger than any real export; a guard against picking the wrong file.
const MAX_IMPORT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone)]
pub struct PasswordsState {
    cell: VaultCell,
    /// The file picked in "Import passwords": the only one an import reads.
    picked_import: Arc<Mutex<Option<String>>>,
}

impl Default for PasswordsState {
    fn default() -> Self {
        Self {
            cell: VaultCell::new(vault_path),
            picked_import: Arc::default(),
        }
    }
}

impl PasswordsState {
    fn lock(&self) -> MutexGuard<'_, Option<Vault>> {
        self.cell.lock()
    }

    /// Runs `f` on the vault for something the user did on the vault page:
    /// the vault counts as in use and stays open longer.
    fn with<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        self.cell.with(f)
    }

    /// Runs `f` on the vault, opening its file the first time. For the
    /// browser extension, the sync and other background work, which must
    /// never keep an unattended vault from locking itself.
    fn with_quiet<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        self.cell.with_quiet(f)
    }

    /// Whether this PC has a vault (made here or taken from the account).
    pub fn has_vault(&self) -> bool {
        self.cell.has_vault()
    }

    /// The same, off the async runtime: deriving a key takes a moment.
    async fn with_blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Vault) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let state = self.clone();
        tauri::async_runtime::spawn_blocking(move || state.with(f))
            .await
            .map_err(|e| e.to_string())?
    }
}

fn vault_path() -> Result<PathBuf, String> {
    Ok(storage::roaming_dir()?.join("passwords.vault"))
}

/// Locks the vault after the chosen idle time and whenever Windows locks,
/// and syncs it every few minutes.
pub fn watch(app: AppHandle, state: PasswordsState) {
    let (sync_app, sync_state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(SYNC_EVERY).await;
            let exists = sync_state.has_vault();
            if exists && let Ok(result) = sync_once(&sync_app, &sync_state).await {
                let _ = sync_app.emit(SYNCED_EVENT, result);
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut last_tick = SystemTime::now();
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            // A tick that took far longer than 10 seconds: the PC slept.
            let now = SystemTime::now();
            let woke = now
                .duration_since(last_tick)
                .is_ok_and(|gap| gap > Duration::from_secs(60));
            last_tick = now;
            let locked_now = {
                let mut slot = state.lock();
                let Some(vault) = slot.as_mut() else { continue };
                if vault.status() != Status::Unlocked {
                    continue;
                }
                let minutes = vault.prefs().auto_lock_minutes;
                let idle = minutes > 0
                    && vault.last_used.elapsed() >= Duration::from_secs(u64::from(minutes) * 60);
                if idle || woke || session::windows_is_locked() {
                    vault.lock();
                    true
                } else {
                    false
                }
            };
            if locked_now {
                let _ = app.emit(LOCKED_EVENT, ());
            }
        }
    });
}

/// Locks the vault now (the tray's "Lock the vault"), and tells the page.
pub fn lock_now(app: &AppHandle) {
    use tauri::Manager;
    let state = app.state::<PasswordsState>();
    let was_open = state
        .with_quiet(|vault| {
            let open = vault.status() == Status::Unlocked;
            vault.lock();
            Ok(open)
        })
        .unwrap_or(false);
    if was_open {
        let _ = app.emit(LOCKED_EVENT, ());
    }
}

#[tauri::command(async)]
pub fn passwords_status(state: State<'_, PasswordsState>) -> Result<StatusInfo, String> {
    state.with(|vault| Ok(vault.status_info()))
}

/// A new vault. Returns the recovery code, shown to the user once.
#[tauri::command]
pub async fn passwords_create(
    state: State<'_, PasswordsState>,
    master: String,
) -> Result<String, String> {
    check_master(&master)?;
    let code = state
        .with_blocking(move |vault| {
            let code = vault.create(&master, KdfParams::new())?;
            Ok(code.to_string())
        })
        .await?;
    mend_browser_registration(&state);
    Ok(code)
}

#[tauri::command]
pub async fn passwords_unlock(
    state: State<'_, PasswordsState>,
    master: String,
) -> Result<(), String> {
    state
        .with_blocking(move |vault| vault.unlock(&master))
        .await?;
    mend_browser_registration(&state);
    Ok(())
}

#[tauri::command]
pub async fn passwords_recover(
    state: State<'_, PasswordsState>,
    code: String,
    master: String,
) -> Result<(), String> {
    check_master(&master)?;
    state
        .with_blocking(move |vault| vault.recover(&code, &master, KdfParams::new()))
        .await?;
    mend_browser_registration(&state);
    Ok(())
}

#[tauri::command]
pub async fn passwords_change_master(
    state: State<'_, PasswordsState>,
    current: String,
    master: String,
) -> Result<(), String> {
    check_master(&master)?;
    state
        .with_blocking(move |vault| vault.change_master(&current, &master, KdfParams::new()))
        .await
}

/// Syncs now: after a change, on opening the page, and from "Sync now".
#[tauri::command]
pub async fn passwords_sync(app: AppHandle, state: State<'_, PasswordsState>) -> Result<sync::SyncResult, String> {
    sync_once(&app, &state).await
}

/// The account holds a different vault than this PC: replace this PC's with
/// it (the page asks first and suggests a backup).
#[tauri::command]
pub async fn passwords_use_account_vault(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let _one = SYNCING.lock().await;
    sync::use_account_vault(crate::account::cloud(&app).await?, &state.cell).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloStatus {
    /// Windows Hello is set up on this PC.
    available: bool,
    /// It opens this vault here.
    enabled: bool,
}

#[tauri::command]
pub async fn passwords_hello_status(state: State<'_, PasswordsState>) -> Result<HelloStatus, String> {
    let vault_id = state.with(|vault| Ok(vault.vault_id()))?;
    tauri::async_runtime::spawn_blocking(move || HelloStatus {
        available: hello::available(),
        enabled: vault_id.is_some_and(|id| hello::enabled(&id)),
    })
    .await
    .map_err(|e| e.to_string())
}

/// Turns Windows Hello on (the vault must be open). The prompts run
/// without holding the vault, so the rest of the app keeps answering.
#[tauri::command]
pub async fn passwords_hello_enable(state: State<'_, PasswordsState>) -> Result<(), String> {
    let (key, vault_id) = state.with(|vault| {
        let id = vault.vault_id().ok_or("There is no vault yet.")?;
        Ok((vault.key_copy()?, id))
    })?;
    tauri::async_runtime::spawn_blocking(move || hello::enable(&key, &vault_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn passwords_hello_disable() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(hello::disable)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn passwords_hello_unlock(state: State<'_, PasswordsState>) -> Result<(), String> {
    let vault_id = state
        .with(|vault| Ok(vault.vault_id()))?
        .ok_or("There is no vault yet.")?;
    let key = tauri::async_runtime::spawn_blocking(move || hello::unlock(&vault_id))
        .await
        .map_err(|e| e.to_string())??;
    state.with(|vault| vault.unlock_with_key(key))?;
    mend_browser_registration(&state);
    Ok(())
}

#[tauri::command(async)]
pub fn passwords_lock(state: State<'_, PasswordsState>) -> Result<(), String> {
    state.with(|vault| {
        vault.lock();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn passwords_set_auto_lock(
    state: State<'_, PasswordsState>,
    minutes: u32,
) -> Result<(), String> {
    state.with(|vault| {
        let mut prefs = vault.prefs();
        prefs.auto_lock_minutes = minutes.min(24 * 60);
        vault.set_prefs(prefs)
    })
}

/// Shows each website's icon in the list, or stops and forgets them all.
#[tauri::command(async)]
pub fn passwords_set_website_icons(state: State<'_, PasswordsState>, on: bool) -> Result<(), String> {
    state.with(|vault| {
        let mut prefs = vault.prefs();
        prefs.website_icons = on;
        vault.set_prefs(prefs)?;
        if !on {
            vault.forget_icons();
        }
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSetup {
    enabled: bool,
    /// The extension's folder, for "Load unpacked".
    extension_dir: Option<String>,
    /// Why a browser would not find MYLE, if one would not.
    registration_error: Option<String>,
    /// The browser that asked last, and when.
    last_contact: Option<browser::Contact>,
    /// The last start of the host it turned away (another browser, say).
    last_refusal: Option<browser::Refusal>,
    vault: Status,
    /// Seconds since 1970, to tell how long ago those were.
    now: u64,
}

fn extension_dir(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let clean = |dir: PathBuf| {
        let text = dir.to_string_lossy();
        match text.strip_prefix(r"\\?\") {
            Some(stripped) => PathBuf::from(stripped),
            None => dir,
        }
    };
    let dir = app.path().resource_dir().ok()?.join("extension");
    if dir.join("manifest.json").is_file() {
        return Some(clean(dir));
    }
    // A dev build runs from backend/target: the source folder.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("extension");
    source
        .join("manifest.json")
        .is_file()
        .then(|| clean(source.canonicalize().unwrap_or(source)))
}

#[tauri::command(async)]
pub fn passwords_browser_get(app: AppHandle, state: State<'_, PasswordsState>) -> Result<BrowserSetup, String> {
    let (enabled, status) = state.with(|vault| Ok((vault.prefs().browser_filling, vault.status())))?;
    // Mends a missing or stale registration (the program moved, a manifest
    // was deleted) and says what is still wrong.
    let registration_error = if enabled { browser::ensure_registered().err() } else { None };
    Ok(BrowserSetup {
        enabled,
        extension_dir: extension_dir(&app).map(|d| d.to_string_lossy().into_owned()),
        registration_error,
        last_contact: browser::last_contact(),
        last_refusal: browser::last_refusal(),
        vault: status,
        now: vault::now(),
    })
}

/// Browser filling on or off: registers the native messaging host with the
/// browsers, or removes it.
#[tauri::command(async)]
pub fn passwords_browser_set(state: State<'_, PasswordsState>, enabled: bool) -> Result<(), String> {
    if enabled {
        browser::register_hosts()?;
    }
    let saved = state.with(|vault| {
        let mut prefs = vault.prefs();
        prefs.browser_filling = enabled;
        vault.set_prefs(prefs)
    });
    if saved.is_err() && enabled {
        browser::unregister_hosts();
    }
    saved?;
    if !enabled {
        browser::unregister_hosts();
    }
    Ok(())
}

/// Shows the extension's folder in File Explorer, for "Load unpacked".
#[tauri::command(async)]
pub fn passwords_open_extension_dir(app: AppHandle) -> Result<(), String> {
    let dir = extension_dir(&app).ok_or("The extension's folder was not found.")?;
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Starts answering the browser extension, and keeps the browsers pointed
/// at this program while filling is on.
pub fn serve_browser(app: AppHandle, state: PasswordsState) {
    // On by default now: a vault made before that gets it once.
    let _ = state.with_quiet(|vault| vault.apply_filling_default());
    let enabled = state.with_quiet(|vault| Ok(vault.prefs().browser_filling)).unwrap_or(false);
    browser::serve(app, state, enabled);
}

/// After an unlock: makes sure the browsers can still reach this program
/// (a manifest deleted meanwhile, say), off the caller's thread.
fn mend_browser_registration(state: &PasswordsState) {
    if state.with_quiet(|vault| Ok(vault.prefs().browser_filling)).unwrap_or(false) {
        std::thread::spawn(|| {
            let _ = browser::ensure_registered();
        });
    }
}

/// A site that insists on user verification for a passkey, on a PC without
/// Windows Hello: the master password, typed in MYLE, stands in.
struct Verifying {
    id: u64,
    site: String,
    reply: tokio::sync::oneshot::Sender<bool>,
}

static VERIFYING: Mutex<Option<Verifying>> = Mutex::new(None);
static NEXT_VERIFY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const VERIFY_WAIT: Duration = Duration::from_secs(120);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    id: u64,
    site: String,
}

/// Brings MYLE forward to ask for the master password; true once the right
/// one was typed, false when cancelled or after two minutes.
pub(crate) async fn ask_master(app: &AppHandle, site: &str) -> bool {
    let (reply, answer) = tokio::sync::oneshot::channel();
    let id = NEXT_VERIFY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // A newer request replaces an older one, which is refused.
    *VERIFYING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Verifying { id, site: site.to_string(), reply });
    browser::open_vault(app);
    let _ = app.emit(VERIFY_EVENT, VerifyRequest { id, site: site.to_string() });
    let verified = matches!(tokio::time::timeout(VERIFY_WAIT, answer).await, Ok(Ok(true)));
    let mut slot = VERIFYING.lock().unwrap_or_else(|p| p.into_inner());
    if slot.as_ref().is_some_and(|pending| pending.id == id) {
        *slot = None;
    }
    verified
}

/// The master password asked for, if a passkey waits for it (the page may
/// open after the request).
#[tauri::command(async)]
pub fn passwords_verify_pending() -> Option<VerifyRequest> {
    VERIFYING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|pending| VerifyRequest { id: pending.id, site: pending.site.clone() })
}

/// The page's answer: the master password, or `None` to cancel. A wrong
/// password is an error and the request stays.
#[tauri::command]
pub async fn passwords_verify_answer(
    state: State<'_, PasswordsState>,
    id: u64,
    master: Option<String>,
) -> Result<(), String> {
    let take = || {
        let mut slot = VERIFYING.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|pending| pending.id == id) { slot.take() } else { None }
    };
    let Some(master) = master else {
        if let Some(pending) = take() {
            let _ = pending.reply.send(false);
        }
        return Ok(());
    };
    if passwords_verify_pending().is_none_or(|pending| pending.id != id) {
        return Err("That request has expired. Try the passkey again.".into());
    }
    let master = zeroize::Zeroizing::new(master);
    state.with_blocking(move |vault| vault.verify_master(&master)).await?;
    if let Some(pending) = take() {
        let _ = pending.reply.send(true);
    }
    Ok(())
}

/// Removes one passkey from a login.
#[tauri::command(async)]
pub fn passwords_passkey_delete(
    app: AppHandle,
    state: State<'_, PasswordsState>,
    id: String,
    credential_id: String,
) -> Result<(), String> {
    state.with(|vault| vault.delete_passkey(&id, &credential_id))?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

/// A program's .exe, picked in a dialog, for linking a login to it.
#[tauri::command]
pub async fn passwords_pick_program(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("The program this login is for")
            .add_filter("Programs", &["exe"])
            .blocking_pick_file()
            .map(|path| path.into_path().map(|path| path.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn passwords_list(state: State<'_, PasswordsState>) -> Result<Vec<Summary>, String> {
    state.with(Vault::summaries)
}

/// The password itself, for the page to show while "Show" is on.
#[tauri::command(async)]
pub fn passwords_reveal(state: State<'_, PasswordsState>, id: String) -> Result<String, String> {
    state.with(|vault| Ok(vault.password(&id)?.to_string()))
}

#[tauri::command(async)]
pub fn passwords_history(
    state: State<'_, PasswordsState>,
    id: String,
) -> Result<Vec<OldPassword>, String> {
    state.with(|vault| vault.history(&id))
}

/// Copies an entry's password or user name straight from the vault.
#[tauri::command(async)]
pub fn passwords_copy(
    state: State<'_, PasswordsState>,
    id: String,
    field: String,
) -> Result<(), String> {
    let text = state.with(|vault| match field.as_str() {
        "password" => Ok(vault.password(&id)?),
        "username" => Ok(zeroize::Zeroizing::new(vault.username(&id)?)),
        "totp" => Ok(zeroize::Zeroizing::new(vault.totp(&id)?.now().code)),
        _ => Err("Unknown field.".into()),
    })?;
    clipboard::copy_secret(&text)
}

/// The entry's 2FA code now; the page asks again when it runs out. Showing
/// it does not keep the vault open: only the user's clicks do.
#[tauri::command(async)]
pub fn passwords_totp(state: State<'_, PasswordsState>, id: String) -> Result<totp::Code, String> {
    state.with_quiet(|vault| Ok(vault.totp(&id)?.now()))
}

/// Whose key it is and how its codes are made, for the editor to show
/// before saving; never the key.
#[tauri::command(async)]
pub fn passwords_totp_check(text: String) -> Result<totp::Info, String> {
    Ok(totp::Totp::parse(&zeroize::Zeroizing::new(text))?.info())
}

/// The 2FA link in a QR code snipped or copied to the clipboard.
#[tauri::command]
pub async fn passwords_totp_scan_clipboard() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let picture = clipboard::read_image()?.ok_or(
            "There is no picture on the clipboard. Snip the QR code with Win+Shift+S, then try again.",
        )?;
        qr::otpauth_in(&picture)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The 2FA link in a QR code saved as a picture; `None` when the user
/// closed the dialog.
#[tauri::command]
pub async fn passwords_totp_scan_file(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("A picture of the QR code")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff"])
            .blocking_pick_file()
            .map(|path| path.into_path().map_err(|e| e.to_string()))
            .transpose()?;
        let Some(path) = picked else { return Ok(None) };
        qr::otpauth_in(&qr::grey_from_file(&path)?).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copies text the page already has (a generated password), the same way.
#[tauri::command(async)]
pub fn passwords_copy_text(text: String) -> Result<(), String> {
    clipboard::copy_secret(&zeroize::Zeroizing::new(text))
}

#[tauri::command(async)]
pub fn passwords_save(
    state: State<'_, PasswordsState>,
    entry: EntryInput,
) -> Result<String, String> {
    state.with(|vault| vault.save(&entry))
}

#[tauri::command(async)]
pub fn passwords_delete(state: State<'_, PasswordsState>, id: String) -> Result<(), String> {
    state.with(|vault| vault.delete(&id))
}

#[tauri::command(async)]
pub fn passwords_generate(options: generator::Options) -> Result<String, String> {
    Ok(generator::generate(&options)?.to_string())
}

#[tauri::command(async)]
pub fn passwords_strength(password: String) -> generator::Strength {
    generator::strength(&zeroize::Zeroizing::new(password))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    path: String,
    /// The app's own backup: it needs its password before anything shows.
    backup: bool,
    count: usize,
    /// A few titles and user names, to check it is the right file.
    sample: Vec<(String, String)>,
}

fn read_import(path: &str) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_IMPORT_BYTES {
        return Err("That file is too large to be a password export.".into());
    }
    std::fs::read_to_string(path).map_err(|_| "That file could not be read as text.".to_string())
}

/// Asks for an export file and shows what it holds, without importing.
#[tauri::command]
pub async fn passwords_import_pick(
    app: AppHandle,
    state: State<'_, PasswordsState>,
) -> Result<Option<ImportPreview>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Import passwords")
            .add_filter("Password exports", &["csv", "json", "myle-vault"])
            .blocking_pick_file()
            .map(|path| path.into_path().map_err(|e| e.to_string()))
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(path) = picked else { return Ok(None) };
    let path = path.to_string_lossy().into_owned();
    *state.picked_import.lock().unwrap_or_else(|p| p.into_inner()) = Some(path.clone());
    let text = zeroize::Zeroizing::new(read_import(&path)?);
    if import::is_backup(&text) {
        return Ok(Some(ImportPreview {
            path,
            backup: true,
            count: 0,
            sample: Vec::new(),
        }));
    }
    let entries = import::parse(&text)?;
    Ok(Some(ImportPreview {
        path,
        backup: false,
        count: entries.len(),
        sample: entries
            .iter()
            .take(5)
            .map(|e| (e.title.clone(), e.username.clone()))
            .collect(),
    }))
}

/// Imports the picked file. A backup needs `backup_password`.
#[tauri::command]
pub async fn passwords_import(
    state: State<'_, PasswordsState>,
    path: String,
    backup_password: Option<String>,
) -> Result<usize, String> {
    // Only the file the user just picked, never a path the page names.
    let picked = state
        .picked_import
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if picked.as_deref() != Some(path.as_str()) {
        return Err("Pick the file to import again.".into());
    }
    state
        .with_blocking(move |vault| {
            let text = zeroize::Zeroizing::new(read_import(&path)?);
            let entries = if import::is_backup(&text) {
                import::restore(&text, backup_password.as_deref().unwrap_or_default())?
            } else {
                import::parse(&text)?
            };
            vault.add_all(entries)
        })
        .await
}

/// Writes an encrypted backup of every entry, under its own password. Every
/// password leaves the vault this way, so the master password is asked
/// again first, even while the vault is open.
#[tauri::command]
pub async fn passwords_export(
    app: AppHandle,
    state: State<'_, PasswordsState>,
    password: String,
    master: String,
) -> Result<Option<String>, String> {
    check_master(&password)?;
    state
        .with_blocking(move |vault| vault.verify_master(&master))
        .await?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Save a password backup")
            .set_file_name("passwords.myle-vault")
            .add_filter("MYLE backup", &["myle-vault"])
            .blocking_save_file()
            .map(|path| path.into_path().map_err(|e| e.to_string()))
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(path) = picked else { return Ok(None) };
    let target = path.clone();
    state
        .with_blocking(move |vault| {
            let entries = vault.all_entries()?;
            let bytes = import::backup(&entries, &password, KdfParams::new())?;
            std::fs::write(&target, bytes).map_err(|e| e.to_string())
        })
        .await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
