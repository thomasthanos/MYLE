//! The vault's commands, with the names and answers of the Windows app's
//! (`backend/src/passwords/mod.rs`), so the same page works on both: only
//! what a phone does (no browser extension, no Windows programs, no import
//! or export) is left out.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use myle_vault::VaultCell;
use myle_vault::crypto::KdfParams;
use myle_vault::vault::{EntryInput, OldPassword, Status, StatusInfo, Summary, Vault, check_master};
use myle_vault::{generator, icons, sync, totp};
use tauri::{AppHandle, Emitter, State};
use zeroize::Zeroizing;

/// Sent to the page when the vault locks itself.
const LOCKED_EVENT: &str = "passwords-locked";
/// Sent when an entry changed outside the page.
const CHANGED_EVENT: &str = "passwords-changed";
/// Sent after a background sync, so an open page shows what came in.
const SYNCED_EVENT: &str = "passwords-synced";
/// How often the vault syncs by itself while the app is open.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// Away from the app this long, and the vault is locked when the user comes
/// back (unless they chose never to lock it).
const AWAY_LOCK: Duration = Duration::from_secs(60);

/// One sync at a time: a background pass and "Sync now" must not interleave.
static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
pub struct PasswordsState {
    pub(crate) cell: VaultCell,
    /// Since when the app has been out of sight.
    hidden_at: Arc<Mutex<Option<Instant>>>,
}

impl PasswordsState {
    /// `dir`: the app's own folder, where the vault lives.
    pub fn new(dir: PathBuf) -> Self {
        let path = dir.join("passwords.vault");
        Self {
            cell: VaultCell::new(move || Ok(path.clone())),
            hidden_at: Arc::default(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        self.cell.with(f)
    }

    fn with_quiet<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        self.cell.with_quiet(f)
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

    fn hidden_since(&self) -> std::sync::MutexGuard<'_, Option<Instant>> {
        self.hidden_at.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Back in sight: locks the vault if the app was away a while (and the
    /// user did not choose never to lock it). Whether it locked it.
    fn lock_now_if_away(&self) -> bool {
        let Some(left) = self.hidden_since().take() else { return false };
        if left.elapsed() < AWAY_LOCK {
            return false;
        }
        let mut slot = self.cell.lock();
        let Some(vault) = slot.as_mut() else { return false };
        if vault.status() != Status::Unlocked || vault.prefs().auto_lock_minutes == 0 {
            return false;
        }
        vault.lock();
        true
    }
}

async fn sync_once(app: &AppHandle, state: &PasswordsState) -> Result<sync::SyncResult, String> {
    let _one = SYNCING.lock().await;
    sync::run(crate::account::cloud(app).await?, &state.cell).await
}

/// Locks the vault after the chosen idle time, and syncs it every few
/// minutes. A phone pauses the app in the background: both catch up when it
/// comes back.
pub fn watch(app: AppHandle, state: PasswordsState) {
    let (sync_app, sync_state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(SYNC_EVERY).await;
            if sync_state.cell.has_vault()
                && let Ok(result) = sync_once(&sync_app, &sync_state).await
            {
                let _ = sync_app.emit(SYNCED_EVENT, result);
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let locked_now = {
                let mut slot = state.cell.lock();
                let Some(vault) = slot.as_mut() else { continue };
                if vault.status() != Status::Unlocked {
                    continue;
                }
                let minutes = vault.prefs().auto_lock_minutes;
                let idle = minutes > 0
                    && vault.last_used.elapsed() >= Duration::from_secs(u64::from(minutes) * 60);
                if idle {
                    vault.lock();
                }
                idle
            };
            if locked_now {
                let _ = app.emit(LOCKED_EVENT, ());
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Commands

#[tauri::command(async)]
pub fn passwords_status(state: State<'_, PasswordsState>) -> Result<StatusInfo, String> {
    state.with(|vault| Ok(vault.status_info()))
}

/// The page went out of sight (`hidden`) or came back. Back after a while
/// away, the vault is locked first; returns whether it was.
#[tauri::command(async)]
pub fn passwords_app_hidden(state: State<'_, PasswordsState>, hidden: bool) -> bool {
    if hidden {
        state.hidden_since().get_or_insert_with(Instant::now);
        return false;
    }
    state.lock_now_if_away()
}

/// A new vault. Returns the recovery code, shown to the user once.
#[tauri::command]
pub async fn passwords_create(state: State<'_, PasswordsState>, master: String) -> Result<String, String> {
    check_master(&master)?;
    state
        .with_blocking(move |vault| Ok(vault.create(&master, KdfParams::new())?.to_string()))
        .await
}

#[tauri::command]
pub async fn passwords_unlock(state: State<'_, PasswordsState>, master: String) -> Result<(), String> {
    state.with_blocking(move |vault| vault.unlock(&master)).await
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
        .await
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

/// Syncs now: after a change, on opening the app, and from "Sync now".
#[tauri::command]
pub async fn passwords_sync(app: AppHandle, state: State<'_, PasswordsState>) -> Result<sync::SyncResult, String> {
    sync_once(&app, &state).await
}

/// The account holds a different vault than this phone: replace this
/// phone's with it (the page asks first).
#[tauri::command]
pub async fn passwords_use_account_vault(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let _one = SYNCING.lock().await;
    sync::use_account_vault(crate::account::cloud(&app).await?, &state.cell).await
}

#[tauri::command(async)]
pub fn passwords_lock(state: State<'_, PasswordsState>) -> Result<(), String> {
    state.with(|vault| {
        vault.lock();
        Ok(())
    })
}

#[tauri::command(async)]
pub fn passwords_set_auto_lock(state: State<'_, PasswordsState>, minutes: u32) -> Result<(), String> {
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

/// The icons already known for the vault's websites, but those the page has
/// (`skip`); the missing and old ones come later as `passwords-icon`.
#[tauri::command]
pub async fn passwords_icons(
    app: AppHandle,
    state: State<'_, PasswordsState>,
    skip: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let (known, pending) = icons::lookup(&state.cell, skip)?;
    if let Some(pending) = pending {
        let cell = state.cell.clone();
        tauri::async_runtime::spawn(async move {
            pending
                .fetch(&cell, |fetched| {
                    let _ = app.emit(icons::ICON_EVENT, fetched);
                })
                .await;
        });
    }
    Ok(known)
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
pub fn passwords_history(state: State<'_, PasswordsState>, id: String) -> Result<Vec<OldPassword>, String> {
    state.with(|vault| vault.history(&id))
}

/// Copies an entry's password, user name or 2FA code straight from the vault.
#[tauri::command(async)]
pub fn passwords_copy(
    app: AppHandle,
    state: State<'_, PasswordsState>,
    id: String,
    field: String,
) -> Result<(), String> {
    let text = state.with(|vault| match field.as_str() {
        "password" => Ok(vault.password(&id)?),
        "username" => Ok(Zeroizing::new(vault.username(&id)?)),
        "totp" => Ok(Zeroizing::new(vault.totp(&id)?.now().code)),
        _ => Err("Unknown field.".into()),
    })?;
    crate::clipboard::copy_secret(&app, &text)
}

/// Copies text the page already has (a generated password), the same way.
#[tauri::command(async)]
pub fn passwords_copy_text(app: AppHandle, text: String) -> Result<(), String> {
    crate::clipboard::copy_secret(&app, &Zeroizing::new(text))
}

/// The entry's 2FA code now; the page asks again when it runs out. Showing
/// it does not keep the vault open: only the user's taps do.
#[tauri::command(async)]
pub fn passwords_totp(state: State<'_, PasswordsState>, id: String) -> Result<totp::Code, String> {
    state.with_quiet(|vault| Ok(vault.totp(&id)?.now()))
}

/// Whose key it is and how its codes are made, for the editor to show
/// before saving; never the key.
#[tauri::command(async)]
pub fn passwords_totp_check(text: String) -> Result<totp::Info, String> {
    Ok(totp::Totp::parse(&Zeroizing::new(text))?.info())
}

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

#[tauri::command(async)]
pub fn passwords_save(state: State<'_, PasswordsState>, entry: EntryInput) -> Result<String, String> {
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
    generator::strength(&Zeroizing::new(password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coming_back_after_a_while_locks_the_vault() {
        let dir = std::env::temp_dir().join(format!("myle-passwords-away-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = PasswordsState::new(dir.clone());
        state
            .with(|vault| {
                vault.create("correct horse battery staple 9", KdfParams::new())?;
                let mut prefs = vault.prefs();
                prefs.auto_lock_minutes = 15;
                vault.set_prefs(prefs)
            })
            .unwrap();
        let unlocked = || state.with_quiet(|vault| Ok(vault.status() == Status::Unlocked)).unwrap();
        assert!(unlocked());

        // A quick look at another app keeps it open.
        *state.hidden_since() = Some(Instant::now());
        assert!(!state.lock_now_if_away());
        assert!(unlocked());

        // A while away locks it.
        *state.hidden_since() = Some(Instant::now() - AWAY_LOCK);
        assert!(state.lock_now_if_away());
        assert!(!unlocked());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
