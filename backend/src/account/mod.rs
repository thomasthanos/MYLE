//! Account: sign in with Discord or Google through Supabase Auth, and keep
//! this app's settings in the Supabase project the old app used.
//!
//! - Sign-in: the browser opens Supabase's authorize page (PKCE); a loopback
//!   server on `localhost:5252`, the redirect that project already allows,
//!   receives the code, which is exchanged for a session here.
//! - The session is kept encrypted with DPAPI (`vault`) and refreshed on use.
//!   Supabase itself (sign-in, refresh, signed-in requests) is
//!   `myle_vault::account`, shared with MYLE Passwords for phones.
//! - Settings live in `user_settings.data` under their own key (`DATA_KEY`),
//!   next to whatever the old app stored there, which is left untouched.
//! - The owner-only pages (`owner`) open for a verdict this side gets from
//!   Supabase about the signed-in user, never for anything the page says.

mod oauth;
pub(crate) mod owner;
pub(crate) mod vault;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;

use crate::download::err;
pub use myle_vault::account::{Cloud, Profile, Provider};
use myle_vault::account::{SUPABASE_ANON_KEY, SUPABASE_URL, Session, now, renew};

/// Allowed as a redirect URL in the Supabase project (used by the old app).
const REDIRECT_PORT: u16 = 5252;
const REDIRECT_URL: &str = "http://localhost:5252";
const TABLE: &str = "user_settings";
/// This app's settings inside the row's `data` object.
const DATA_KEY: &str = "v7";
const VAULT_FILE: &str = "account.bin";
/// The owner verdict (`owner::Verdict`), sealed like the session.
const ACCESS_FILE: &str = "account-access.bin";
/// Tells the page that the owner-only pages opened or closed.
pub(crate) const ACCESS_EVENT: &str = "account-access";
const USER_AGENT: &str = "MakeYourLifeEasier-Account";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    loaded: bool,
    signing_in: Option<Arc<Notify>>,
    verdict: Option<owner::Verdict>,
    verdict_loaded: bool,
}

/// The session, and a gate that lets one refresh run at a time.
#[derive(Clone, Default)]
pub struct AccountState(Arc<Mutex<Inner>>, Arc<tokio::sync::Mutex<()>>);

impl AccountState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The session from disk, read once per run.
    fn session(&self, app: &AppHandle) -> Option<Session> {
        let mut inner = self.lock();
        if !inner.loaded {
            inner.loaded = true;
            inner.session = vault_path(app)
                .ok()
                .and_then(|path| vault::load(&path))
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        }
        inner.session.clone()
    }

    fn store(&self, app: &AppHandle, session: Option<Session>) -> Result<(), String> {
        let path = vault_path(app)?;
        match &session {
            Some(session) => vault::save(&path, &serde_json::to_vec(session).map_err(err)?)?,
            None => vault::remove(&path),
        }
        let was_owner = self.is_owner(app);
        {
            let mut inner = self.lock();
            // A verdict is only ever about the user it was checked for.
            let same_user = matches!(
                (&session, &inner.verdict),
                (Some(session), Some(verdict)) if session.profile.id == verdict.user_id
            );
            inner.session = session;
            inner.loaded = true;
            if !same_user {
                inner.verdict = None;
                inner.verdict_loaded = true;
                remove_verdict();
            }
        }
        self.announce(app, was_owner);
        Ok(())
    }

    /// The saved verdict, read once per run.
    fn verdict(&self) -> Option<owner::Verdict> {
        let mut inner = self.lock();
        if !inner.verdict_loaded {
            inner.verdict_loaded = true;
            inner.verdict = access_path()
                .ok()
                .and_then(|path| vault::load(&path))
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        }
        inner.verdict.clone()
    }

    fn set_verdict(&self, app: &AppHandle, verdict: owner::Verdict) {
        let was_owner = self.is_owner(app);
        if let Ok(path) = access_path()
            && let Ok(bytes) = serde_json::to_vec(&verdict)
        {
            let _ = vault::save(&path, &bytes);
        }
        {
            let mut inner = self.lock();
            inner.verdict = Some(verdict);
            inner.verdict_loaded = true;
        }
        self.announce(app, was_owner);
    }

    /// Whether the signed-in user is the owner, by the last verdict from
    /// Supabase. No network: the invoke handler asks this for every command.
    pub(crate) fn is_owner(&self, app: &AppHandle) -> bool {
        let Some(session) = self.session(app) else {
            return false;
        };
        self.verdict()
            .is_some_and(|verdict| verdict.admits(&session.profile.id, now()))
    }

    /// Tells the page when the owner-only pages open or close; on closing,
    /// stops whatever they still run.
    fn announce(&self, app: &AppHandle, was_owner: bool) {
        let owner = self.is_owner(app);
        if owner == was_owner {
            return;
        }
        if !owner {
            stop_owner_only_work(app);
        }
        let _ = app.emit(ACCESS_EVENT, owner);
    }
}

/// Stops the owner-only pages' builds, releases, backups and comparisons.
fn stop_owner_only_work(app: &AppHandle) {
    if let Some(state) = app.try_state::<crate::github_releases::GithubReleasesState>() {
        state.cancel_all();
    }
    if let Some(state) = app.try_state::<crate::project_backups::ProjectBackupsState>() {
        state.cancel(None);
        state.cancel_preview();
    }
}

fn access_path() -> Result<PathBuf, String> {
    Ok(crate::storage::roaming_dir()?.join(ACCESS_FILE))
}

fn remove_verdict() {
    if let Ok(path) = access_path() {
        vault::remove(&path);
    }
}

fn vault_path(app: &AppHandle) -> Result<PathBuf, String> {
    let _ = app;
    Ok(crate::storage::roaming_dir()?.join(VAULT_FILE))
}

fn client() -> Result<reqwest::Client, String> {
    crate::download::http_client(USER_AGENT)
}

/// `None` when nobody is signed in.
pub(crate) async fn cloud(app: &AppHandle) -> Result<Option<Cloud>, String> {
    let state = app.state::<AccountState>();
    if state.session(app).is_none() {
        return Ok(None);
    }
    let session = fresh_session(app, &state).await?;
    Ok(Some(Cloud::new(&session, client()?)))
}

/// Asks Supabase who the session's user is and keeps the verdict. When
/// Supabase can't be reached, the last verdict stands (within its grace).
async fn check_owner(app: &AppHandle, state: &AccountState, force: bool) -> bool {
    let Some(session) = state.session(app) else {
        return false;
    };
    if !force
        && state
            .verdict()
            .is_some_and(|verdict| verdict.is_recent(&session.profile.id, now()))
    {
        return state.is_owner(app);
    }
    let Ok(session) = fresh_session(app, state).await else {
        // Signed out by a refused refresh, or offline.
        return state.is_owner(app);
    };
    match fetch_user(&session).await {
        Ok(Some(user)) => {
            let same_user =
                user.get("id").and_then(Value::as_str) == Some(session.profile.id.as_str());
            state.set_verdict(
                app,
                owner::Verdict {
                    user_id: session.profile.id.clone(),
                    owner: same_user && owner::is_owner(&user),
                    checked_at: now(),
                },
            );
        }
        // Supabase refused the token: not the owner until a sign-in says so.
        Ok(None) => state.set_verdict(
            app,
            owner::Verdict {
                user_id: session.profile.id.clone(),
                owner: false,
                checked_at: now(),
            },
        ),
        Err(_) => {}
    }
    state.is_owner(app)
}

/// Supabase's user object for the session (`Ok(None)`: the token was
/// refused). Supabase checks the token; nothing here is taken on trust.
async fn fetch_user(session: &Session) -> Result<Option<Value>, String> {
    let response = client()?
        .get(format!("{SUPABASE_URL}/auth/v1/user"))
        .header("apikey", SUPABASE_ANON_KEY)
        .bearer_auth(&session.access_token)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(err)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("Supabase answered {status}"));
    }
    response.json().await.map(Some).map_err(err)
}

// ---------------------------------------------------------------------------
// Commands

/// Whether the owner-only pages are open. Checks with Supabase again when
/// the last check is more than a few minutes old (or `recheck`).
#[tauri::command]
pub async fn account_access(
    app: AppHandle,
    state: State<'_, AccountState>,
    recheck: Option<bool>,
) -> Result<bool, String> {
    Ok(check_owner(&app, &state, recheck.unwrap_or(false)).await)
}

/// The signed-in profile, if any. Reads the saved session; no network.
#[tauri::command(async)]
pub fn account_profile(app: AppHandle, state: State<'_, AccountState>) -> Option<Profile> {
    state.session(&app).map(|session| session.profile)
}

/// Signs in with the provider in the browser and returns who signed in.
#[tauri::command]
pub async fn account_sign_in(
    app: AppHandle,
    state: State<'_, AccountState>,
    provider: Provider,
) -> Result<Profile, String> {
    let cancel = Arc::new(Notify::new());
    {
        let mut inner = state.lock();
        if inner.signing_in.is_some() {
            return Err("A sign-in is already waiting for the browser.".into());
        }
        inner.signing_in = Some(cancel.clone());
    }
    let result = sign_in(provider, &cancel).await;
    state.lock().signing_in = None;
    let session = result?;
    let profile = session.profile.clone();
    state.store(&app, Some(session))?;
    check_owner(&app, &state, true).await;
    // The browser has the focus now; bring the app back.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(profile)
}

/// Stops waiting for the browser (the user closed the tab, changed their mind).
#[tauri::command]
pub fn account_cancel_sign_in(state: State<'_, AccountState>) {
    if let Some(cancel) = state.lock().signing_in.take() {
        cancel.notify_one();
    }
}

#[tauri::command]
pub async fn account_sign_out(app: AppHandle, state: State<'_, AccountState>) -> Result<(), String> {
    if let Some(session) = state.session(&app) {
        // Revokes the refresh token; the local sign-out happens regardless.
        myle_vault::account::logout(&client()?, &session).await;
    }
    state.store(&app, None)
}

/// This app's settings from the cloud, or `None` when none are saved yet.
#[tauri::command]
pub async fn account_pull(
    app: AppHandle,
    state: State<'_, AccountState>,
) -> Result<Option<Value>, String> {
    let session = fresh_session(&app, &state).await?;
    let row = read_row(&session).await?;
    Ok(row.and_then(|data| data.get(DATA_KEY).cloned()))
}

/// Saves this app's settings to the cloud, keeping the rest of the row.
#[tauri::command]
pub async fn account_push(
    app: AppHandle,
    state: State<'_, AccountState>,
    settings: Value,
) -> Result<(), String> {
    if !settings.is_object() {
        return Err("Settings must be an object.".into());
    }
    let session = fresh_session(&app, &state).await?;
    let mut data = match read_row(&session).await? {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    data.insert(DATA_KEY.into(), settings);
    let body = serde_json::json!({
        "user_id": session.profile.id,
        "data": data,
        "updated_at": iso8601(now()),
    });
    client()?
        .post(format!("{SUPABASE_URL}/rest/v1/{TABLE}?on_conflict=user_id"))
        .header("apikey", SUPABASE_ANON_KEY)
        .bearer_auth(&session.access_token)
        .header("Prefer", "resolution=merge-duplicates,return=minimal")
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(|e| format!("The cloud did not accept the settings: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sign-in

async fn sign_in(provider: Provider, cancel: &Notify) -> Result<Session, String> {
    // Listen before the browser opens, so the redirect cannot arrive first.
    let loopback = oauth::Loopback::bind(REDIRECT_PORT).await?;
    let (verifier, challenge) = myle_vault::account::pkce_pair();
    let authorize = myle_vault::account::authorize_url(provider, REDIRECT_URL, &challenge)?;
    tauri_plugin_opener::open_url(authorize.as_str(), None::<&str>).map_err(err)?;

    let callback = tokio::select! {
        callback = loopback.next_callback() => callback,
        _ = cancel.notified() => return Err("Sign-in was cancelled.".into()),
        _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => {
            return Err("Sign-in timed out. Try again.".into());
        }
    };
    let code = match callback {
        oauth::Callback::Code(code) => code,
        oauth::Callback::Error(message) => return Err(message),
    };
    myle_vault::account::exchange_code(&client()?, &code, &verifier).await
}

/// A session with at least a minute left, refreshed (and saved) if needed.
async fn fresh_session(app: &AppHandle, state: &AccountState) -> Result<Session, String> {
    renew(
        &state.1,
        || state.session(app),
        |session| state.store(app, session),
        |token| async move {
            let client = client().map_err(myle_vault::account::TokenError::Network)?;
            myle_vault::account::refresh(&client, token).await
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// user_settings

/// The row's whole `data` object, or `None` when the user has no row yet.
async fn read_row(session: &Session) -> Result<Option<Value>, String> {
    let url = reqwest::Url::parse_with_params(
        &format!("{SUPABASE_URL}/rest/v1/{TABLE}"),
        &[
            ("select", "data".to_string()),
            ("user_id", format!("eq.{}", session.profile.id)),
        ],
    )
    .map_err(err)?;
    let rows: Vec<Value> = client()?
        .get(url)
        .header("apikey", SUPABASE_ANON_KEY)
        .bearer_auth(&session.access_token)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(|e| format!("The cloud settings could not be read: {e}"))?
        .json()
        .await
        .map_err(err)?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|mut row| row.get_mut("data").map(Value::take)))
}

/// `2026-09-27T12:34:56Z` from Unix seconds (UTC), for `updated_at`.
fn iso8601(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_iso_8601_utc() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(1_790_462_096), "2026-09-26T22:34:56Z");
    }
}
