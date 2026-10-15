//! In-process JNI access for private Android credential activities. No Binder,
//! socket or webview endpoint exposes these actions to a destination app.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use myle_vault::VaultCell;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use zeroize::Zeroizing;

use crate::provider::{Provider, erase};

struct Shared {
    dir: PathBuf,
    provider: Provider,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

fn provider(dir: &Path) -> Result<&'static Provider, String> {
    let dir = dir.canonicalize().map_err(|_| "The private app directory is unavailable.")?;
    if !dir.is_dir() { return Err("The private app directory is unavailable.".into()); }
    if SHARED.get().is_none() {
        let path = dir.join("passwords.vault");
        let _ = SHARED.set(Shared { dir: dir.clone(), provider: Provider::new(VaultCell::new(move || Ok(path.clone()))) });
    }
    let shared = SHARED.get().ok_or("The vault could not initialize.")?;
    if shared.dir != dir { return Err("The vault directory changed. Restart MYLE.".into()); }
    Ok(&shared.provider)
}

pub(crate) fn shared_cell(dir: &Path) -> Result<VaultCell, String> { Ok(provider(dir)?.cell.clone()) }

pub(crate) fn listen(app: AppHandle, dir: &Path) -> Result<(), String> {
    let _ = APP.set(app.clone());
    provider(dir)?.set_lock_listener(Arc::new(move || { let _ = app.emit("passwords-locked", ()); }));
    Ok(())
}

fn request(dir: &str, action: &str, body: &str) -> Result<Value, String> {
    if dir.len() > 4096 || action.len() > 32 || body.len() > 128 * 1024 { return Err("badRequest".into()); }
    let payload = serde_json::from_str(body).map_err(|_| "badRequest")?;
    let result = provider(Path::new(dir))?.call(action, payload)?;
    if matches!(action, "create" | "save") && let Some(app) = APP.get() {
        let _ = app.emit("passwords-changed", ());
    }
    Ok(result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_uk_thomast_myle_mobile_NativeVault_call(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    dir: JString<'_>,
    action: JString<'_>,
    body: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir: String = env.get_string(&dir).map_err(|_| "badRequest")?.into();
        let action: String = env.get_string(&action).map_err(|_| "badRequest")?.into();
        let body = Zeroizing::new(String::from(env.get_string(&body).map_err(|_| "badRequest")?));
        request(&dir, &action, &body)
    })).unwrap_or_else(|_| Err("The vault operation stopped. Verify again.".into()));
    let mut reply = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(error) => json!({"ok":false,"error":error}),
    };
    let encoded = Zeroizing::new(reply.to_string());
    erase(&mut reply);
    env.new_string(encoded.as_str()).map(JString::into_raw).unwrap_or(std::ptr::null_mut())
}
