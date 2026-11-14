//! Opening the vault with Face ID, Touch ID or a fingerprint: the phone's
//! own counterpart of Windows Hello, behind the same commands
//! (`passwords_hello_*`), so the page needs nothing new.
//!
//! Turning it on makes a random key and gives it to the phone's secure store,
//! which hands it back only after the user's face or fingerprint. On iOS that
//! is the app's own Keychain item (`tauri-plugin-myle-mobile`): Face ID or
//! Touch ID as enrolled when it was turned on, never the passcode, never in a
//! backup. On Android it is the Keystore (`tauri-plugin-biometry`). That key
//! wraps the vault key; only the wrapped copy is kept in the app's folder.
//! A changed master password keeps it working (the vault key is the same);
//! another vault turns it off, and the master password always works.
//!
//! When the key can never open the vault again (Face ID changed, the passcode
//! was turned off, or it was kept the old way, which the passcode opened),
//! it turns itself off and the page is told why (`OFF_EVENT`).

use std::path::{Path, PathBuf};

use myle_vault::crypto::{self, Key};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_myle_mobile::{KeyError, KeyGone};

use crate::passwords::PasswordsState;

const FILE: &str = "passwords-biometry.json";
const WRAP_AAD: &str = "myle-biometry-v1";
/// Quick unlock turned itself off; the payload says why.
const OFF_EVENT: &str = "passwords-quick-unlock-off";

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

/// What the user reads when the key can never open the vault again.
fn gone_reason(gone: KeyGone) -> &'static str {
    match gone {
        KeyGone::Changed => {
            "Face ID or Touch ID changed on this iPhone (a face or finger was added or removed), so it no longer opens your vault. Unlock with your master password, then turn it on again in Settings › Security & vault."
        }
        KeyGone::Missing => {
            "This iPhone no longer has the key that let Face ID or Touch ID open your vault (the passcode was turned off, or Face ID changed). Unlock with your master password, then turn it on again in Settings › Security & vault."
        }
        KeyGone::Legacy => {
            "Face ID and Touch ID now open your vault only with your face or finger, never the iPhone passcode. Unlock with your master password, then turn it on again in Settings › Security & vault."
        }
    }
}

/// The key can never open this vault again: forgets it and its wrapped copy,
/// and tells the page why. Answers that reason.
fn turn_off(app: &AppHandle, file: &Path, name: &str, gone: KeyGone) -> String {
    let _ = key::remove(app, name);
    let _ = crate::account::remove_private(file);
    let why = gone_reason(gone);
    let _ = app.emit(OFF_EVENT, why);
    why.to_string()
}

#[tauri::command]
pub async fn passwords_hello_status(app: AppHandle, state: State<'_, PasswordsState>) -> Result<HelloStatus, String> {
    let id = state.cell.with(|vault| Ok(vault.vault_id()))?;
    let file = path(&app)?;
    let stored = read(&file).filter(|stored| id.as_deref() == Some(stored.vault_id.as_str()));
    blocking(move || {
        let available = biometry::available(&app);
        let enabled = match stored {
            Some(stored) if available => match key::has(&app, &stored.vault_id) {
                Ok(has) => has,
                Err(gone) => {
                    turn_off(&app, &file, &stored.vault_id, gone);
                    false
                }
            },
            _ => false,
        };
        Ok(HelloStatus { available, enabled })
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
        biometry::confirm(&app, "Turn on unlocking MYLE Passwords this way")?;
        let unlock_key = Key::random();
        let stored = Stored {
            vault_id: id.clone(),
            wrapped_key: crypto::wrap(&unlock_key, WRAP_AAD, &vault_key)?,
        };
        key::put(&app, &id, &unlock_key.to_text())?;
        crate::account::write_private(&file, &serde_json::to_vec(&stored).map_err(|e| e.to_string())?)
    })
    .await
}

#[tauri::command]
pub async fn passwords_hello_disable(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let file = path(&app)?;
    let id = read(&file).map(|stored| stored.vault_id).or_else(|| vault_id(&state).ok());
    if let Some(id) = id {
        blocking(move || key::remove(&app, &id)).await?;
    }
    crate::account::remove_private(&file)?;
    Ok(())
}

#[tauri::command]
pub async fn passwords_hello_unlock(app: AppHandle, state: State<'_, PasswordsState>) -> Result<(), String> {
    let id = vault_id(&state)?;
    let file = path(&app)?;
    let stored = read(&file)
        .filter(|stored| stored.vault_id == id)
        .ok_or("Unlocking this way is off. Use the master password.")?;
    let key_file = file.clone();
    let text = blocking(move || match key::take(&app, &id, "Open your password vault") {
        Ok(text) => Ok(text),
        Err(KeyError::Gone(gone)) => Err(turn_off(&app, &key_file, &id, gone)),
        Err(KeyError::Cancelled) => Err("Cancelled. Use your master password, or try again.".into()),
        Err(KeyError::Failed(error)) => Err(error),
    })
    .await?;
    let unlock_key = Key::from_text(&text)?;
    let vault_key = crypto::unwrap(&unlock_key, WRAP_AAD, &stored.wrapped_key).map_err(|_| {
        // A key from another time: it can never open this vault again.
        let _ = std::fs::remove_file(&file);
        "Unlocking this way stopped working. Use the master password, then turn it on again.".to_string()
    })?;
    state.cell.with(|vault| vault.unlock_with_key(vault_key))
}

/// Whether the phone has a face or fingerprint set up, and asking for it
/// (`tauri-plugin-biometry`).
#[cfg(mobile)]
mod biometry {
    use tauri::AppHandle;
    use tauri_plugin_biometry::{AuthOptions, BiometryExt};

    pub fn available(app: &AppHandle) -> bool {
        app.biometry().status().is_ok_and(|status| status.is_available)
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
}

/// On Windows (trying the app there) there is no face or fingerprint.
#[cfg(not(mobile))]
mod biometry {
    use tauri::AppHandle;

    pub fn available(_: &AppHandle) -> bool {
        false
    }

    pub fn confirm(_: &AppHandle, _: &str) -> Result<(), String> {
        Err("Face ID and fingerprints are for phones.".into())
    }
}

/// The key on Android: the Keystore, through `tauri-plugin-biometry`.
#[cfg(target_os = "android")]
mod key {
    use tauri::AppHandle;
    use tauri_plugin_biometry::{BiometryExt, DataOptions, GetDataOptions, SetDataOptions};
    use tauri_plugin_myle_mobile::{KeyError, KeyGone};
    use zeroize::Zeroizing;

    /// The Keystore key's name.
    const DOMAIN: &str = "uk.thomast.myle.passwords.unlock";

    /// Whether the key is there.
    pub fn has(app: &AppHandle, name: &str) -> Result<bool, KeyGone> {
        Ok(app
            .biometry()
            .has_data(DataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
            })
            .unwrap_or(false))
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
    pub fn take(app: &AppHandle, name: &str, reason: &str) -> Result<Zeroizing<String>, KeyError> {
        app.biometry()
            .get_data(GetDataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
                reason: reason.into(),
                cancel_title: Some("Use the master password".into()),
            })
            .map(|answer| Zeroizing::new(answer.data))
            .map_err(|e| KeyError::Failed(e.to_string()))
    }

    pub fn remove(app: &AppHandle, name: &str) -> Result<(), String> {
        let present = app
            .biometry()
            .has_data(DataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
            })
            .map_err(|e| e.to_string())?;
        if !present {
            return Ok(());
        }
        app.biometry()
            .remove_data(DataOptions {
                domain: DOMAIN.into(),
                name: name.into(),
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The key on iOS: the app's own Keychain item (`tauri-plugin-myle-mobile`),
/// for Face ID or Touch ID as enrolled when it was made, never the passcode.
/// On Windows the plugin answers that this is for phones.
#[cfg(not(target_os = "android"))]
mod key {
    use tauri::AppHandle;
    use tauri_plugin_myle_mobile::{KeyError, KeyGone, MobileExt};
    use zeroize::Zeroizing;

    /// Whether the key is there; `Err` when it can never open the vault again.
    pub fn has(app: &AppHandle, name: &str) -> Result<bool, KeyGone> {
        match app.myle_mobile().check_unlock_key(name) {
            Ok(()) => Ok(true),
            Err(KeyError::Gone(gone)) => Err(gone),
            Err(_) => Ok(false),
        }
    }

    pub fn put(app: &AppHandle, name: &str, data: &str) -> Result<(), String> {
        app.myle_mobile().put_unlock_key(name, data)
    }

    /// The key, once the user shows their face or finger.
    pub fn take(app: &AppHandle, name: &str, reason: &str) -> Result<Zeroizing<String>, KeyError> {
        app.myle_mobile()
            .take_unlock_key(name, reason, "Use the master password")
            .map(Zeroizing::new)
    }

    pub fn remove(app: &AppHandle, name: &str) -> Result<(), String> {
        app.myle_mobile().remove_unlock_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_gone_for_good_says_why_and_what_to_do() {
        for gone in [KeyGone::Changed, KeyGone::Missing, KeyGone::Legacy] {
            let why = gone_reason(gone);
            assert!(why.contains("master password"), "{why}");
            assert!(why.contains("Settings › Security & vault"), "{why}");
        }
        assert_ne!(gone_reason(KeyGone::Changed), gone_reason(KeyGone::Missing));
        assert!(gone_reason(KeyGone::Legacy).contains("never the iPhone passcode"));
    }
}
