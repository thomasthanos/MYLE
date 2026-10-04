//! Opening the vault with Face ID, Touch ID or a fingerprint: the phone's
//! own counterpart of Windows Hello, behind the same commands
//! (`passwords_hello_*`), so the page needs nothing new.
//!
//! Turning it on makes a random key and gives it to the phone's secure store
//! (the Keychain on iOS, the Keystore on Android), which hands it back only
//! after the user's face, fingerprint or (on iOS) device passcode. That key
//! wraps the vault key; only the wrapped copy is kept in the app's folder.
//! A changed master password keeps it working (the vault key is the same);
//! another vault turns it off, and the master password always works.

use std::path::{Path, PathBuf};

use myle_vault::crypto::{self, Key};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::passwords::PasswordsState;

const FILE: &str = "passwords-biometry.json";
const WRAP_AAD: &str = "myle-biometry-v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    vault_id: String,
    wrapped_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloStatus {
    /// The phone has a face or fingerprint set up.
    available: bool,
    /// It opens this vault here.
    enabled: bool,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn read(path: &Path) -> Option<Stored> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn vault_id(state: &PasswordsState) -> Result<String, String> {
    state
        .cell
        .with(|vault| Ok(vault.vault_id()))?
        .ok_or_else(|| "There is no vault yet.".to_string())
}

/// Off the async runtime: the phone's prompts wait for the user.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn passwords_hello_status(app: AppHandle, state: State<'_, PasswordsState>) -> Result<HelloStatus, String> {
    let id = state.cell.with(|vault| Ok(vault.vault_id()))?;
    let stored = read(&path(&app)?).filter(|stored| id.as_deref() == Some(stored.vault_id.as_str()));
    blocking(move || {
        let available = store::available(&app);
        Ok(HelloStatus {
            available,
            enabled: available && stored.is_some_and(|stored| store::has(&app, &stored.vault_id)),
        })
    })
    .await
}

/// Turns it on (the vault must be open), after the user shows their face or
/// finger once, so they know it works.
#[tauri::command]
pub async fn passwords_hello_enable(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let (vault_key, id) = state.cell.with(|vault| {
        let id = vault.vault_id().ok_or("There is no vault yet.")?;
        Ok((vault.key_copy()?, id))
    })?;
    let file = path(&app)?;
    blocking(move || {
        store::confirm(&app, "Turn on unlocking MYLE Passwords this way")?;
        let unlock_key = Key::random();
        let stored = Stored {
            vault_id: id.clone(),
            wrapped_key: crypto::wrap(&unlock_key, WRAP_AAD, &vault_key)?,
        };
        store::put(&app, &id, &unlock_key.to_text())?;
        crate::account::write_private(&file, &serde_json::to_vec(&stored).map_err(|e| e.to_string())?)
    })
    .await
}

#[tauri::command]
pub async fn passwords_hello_disable(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let file = path(&app)?;
    let id = read(&file).map(|stored| stored.vault_id).or_else(|| vault_id(&state).ok());
    let _ = std::fs::remove_file(&file);
    if let Some(id) = id {
        blocking(move || {
            store::remove(&app, &id);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn passwords_hello_unlock(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let id = vault_id(&state)?;
    let file = path(&app)?;
    let stored = read(&file)
        .filter(|stored| stored.vault_id == id)
        .ok_or("Unlocking this way is off. Use the master password.")?;
    let text = blocking(move || store::take(&app, &id, "Open your password vault")).await?;
    let unlock_key = Key::from_text(&text)?;
    let vault_key = crypto::unwrap(&unlock_key, WRAP_AAD, &stored.wrapped_key).map_err(|_| {
        // A key from another time: it can never open this vault again.
        let _ = std::fs::remove_file(&file);
        "Unlocking this way stopped working. Use the master password, then turn it on again.".to_string()
    })?;
    state.cell.with(|vault| vault.unlock_with_key(vault_key))
}

/// The phone's secure store, through `tauri-plugin-biometry`.
#[cfg(mobile)]
mod store {
    use tauri::AppHandle;
    use tauri_plugin_biometry::{AuthOptions, BiometryExt, DataOptions, GetDataOptions, SetDataOptions};
    use zeroize::Zeroizing;

    /// The Keychain service, and the Keystore key's name.
    const DOMAIN: &str = "uk.thomast.myle.passwords.unlock";

    pub fn available(app: &AppHandle) -> bool {
        app.biometry().status().is_ok_and(|status| status.is_available)
    }

    pub fn has(app: &AppHandle, name: &str) -> bool {
        app.biometry()
            .has_data(DataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
            })
            .unwrap_or(false)
    }

    pub fn confirm(app: &AppHandle, reason: &str) -> Result<(), String> {
        app.biometry()
            .authenticate(reason.into(), AuthOptions {
                title: Some("MYLE Passwords".into()),
                subtitle: None,
                allow_device_credential: Some(false),
                confirmation_required: Some(false),
                cancel_title: Some("Cancel".into()),
                fallback_title: None,
            })
            .map_err(|e| e.to_string())
    }

    pub fn put(app: &AppHandle, name: &str, data: &str) -> Result<(), String> {
        app.biometry()
            .set_data(SetDataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
                data: data.into(),
            })
            .map_err(|e| e.to_string())
    }

    /// The key, once the user shows their face or finger.
    pub fn take(app: &AppHandle, name: &str, reason: &str) -> Result<Zeroizing<String>, String> {
        app.biometry()
            .get_data(GetDataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
                reason: reason.into(),
                cancel_title: Some("Use the master password".into()),
            })
            .map(|answer| Zeroizing::new(answer.data))
            .map_err(|e| e.to_string())
    }

    pub fn remove(app: &AppHandle, name: &str) {
        let _ = app.biometry().remove_data(DataOptions {
            domain: DOMAIN.into(),
            name: name.into(),
        });
    }
}

/// On Windows (trying the app there) there is no such store.
#[cfg(not(mobile))]
mod store {
    use tauri::AppHandle;
    use zeroize::Zeroizing;

    const NONE: &str = "Face ID and fingerprints are for phones.";

    pub fn available(_: &AppHandle) -> bool {
        false
    }

    pub fn has(_: &AppHandle, _: &str) -> bool {
        false
    }

    pub fn confirm(_: &AppHandle, _: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn put(_: &AppHandle, _: &str, _: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn take(_: &AppHandle, _: &str, _: &str) -> Result<Zeroizing<String>, String> {
        Err(NONE.into())
    }

    pub fn remove(_: &AppHandle, _: &str) {}
}
