//! The account on the phone: sign-in with Discord or Google inside the app
//! (iOS's sign-in sheet, Android's Custom Tab: `tauri-plugin-myle-mobile`),
//! which comes back to the app's own address (`REDIRECT_URL`), and the
//! session kept in the app's own folder, which only this app can read.
//!
//! On Android the page comes back as a link to the app. The sign-in waiting
//! for it is kept on disk too: Android may end the app while the page is in
//! front, and the link then starts it again.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use myle_vault::account::{self as supabase, Cloud, Profile, Provider, Session, TokenError};
use myle_vault::http::err;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, Url};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_myle_mobile::{CANCELLED, MobileExt};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

/// Must be among the Supabase project's Redirect URLs.
pub const REDIRECT_URL: &str = "uk.thomast.myle.passwords://auth-callback";
const SCHEME: &str = "uk.thomast.myle.passwords";
const SESSION_FILE: &str = "account.json";
const PENDING_FILE: &str = "sign-in.json";
/// A sign-in older than this is not finished by a late return.
const PENDING_LIFETIME_SECS: u64 = 10 * 60;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const USER_AGENT: &str = "MYLE-Passwords";
/// A sign-in finished with no page waiting for it (the browser's return
/// started the app again).
const SIGNED_IN_EVENT: &str = "account-signed-in";

type Waiter = oneshot::Sender<Result<Profile, String>>;

#[derive(Serialize, Deserialize)]
struct Pending {
    verifier: String,
    at: u64,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    loaded: bool,
    waiting: Option<Waiter>,
    /// The page came back and the sign-in is being finished: closing the
    /// sign-in page now is no reason to cancel it.
    returning: bool,
}

#[derive(Clone)]
pub struct AccountState {
    dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
    /// One refresh of the session at a time.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl AccountState {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            inner: Arc::default(),
            gate: Arc::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The session from disk, read once per run.
    fn session(&self) -> Option<Session> {
        let mut inner = self.lock();
        if !inner.loaded {
            inner.loaded = true;
            inner.session = std::fs::read(self.dir.join(SESSION_FILE))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        }
        inner.session.clone()
    }

    fn store(&self, session: Option<Session>) -> Result<(), String> {
        let path = self.dir.join(SESSION_FILE);
        match &session {
            Some(session) => write_private(&path, &serde_json::to_vec(session).map_err(err)?)?,
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        let mut inner = self.lock();
        inner.session = session;
        inner.loaded = true;
        Ok(())
    }

    fn save_pending(&self, verifier: &str) -> Result<(), String> {
        let pending = Pending {
            verifier: verifier.to_string(),
            at: supabase::now(),
        };
        write_private(&self.dir.join(PENDING_FILE), &serde_json::to_vec(&pending).map_err(err)?)
    }

    /// The sign-in waiting for the browser, once: a second return of the
    /// same link finds none.
    fn take_pending(&self) -> Option<Pending> {
        let path = self.dir.join(PENDING_FILE);
        let pending: Pending = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        let _ = std::fs::remove_file(&path);
        (supabase::now().saturating_sub(pending.at) < PENDING_LIFETIME_SECS).then_some(pending)
    }

    /// A session with at least a minute left, refreshed (and saved) if needed.
    async fn fresh(&self) -> Result<Session, String> {
        supabase::renew(
            &self.gate,
            || self.session(),
            |session| self.store(session),
            |token| async move {
                let client = client().map_err(TokenError::Network)?;
                supabase::refresh(&client, token).await
            },
        )
        .await
    }
}

/// Replaces `path` whole, so a crash never leaves half a file.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(err)?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

fn client() -> Result<reqwest::Client, String> {
    myle_vault::http::client(USER_AGENT)
}

/// `None` when nobody is signed in.
pub async fn cloud(app: &AppHandle) -> Result<Option<Cloud>, String> {
    let state = app.state::<AccountState>();
    if state.session().is_none() {
        return Ok(None);
    }
    let session = state.fresh().await?;
    Ok(Some(Cloud::new(&session, client()?)))
}

/// Takes the browser's return: while the app runs, and the one that started it.
pub fn listen(app: &AppHandle) {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            returned(&handle, url);
        }
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            returned(app, url);
        }
    }
}

fn returned(app: &AppHandle, url: Url) {
    if !url.as_str().starts_with(REDIRECT_URL) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AccountState>();
        state.lock().returning = true;
        let result = finish(&state, &url).await;
        let waiting = {
            let mut inner = state.lock();
            inner.returning = false;
            inner.waiting.take()
        };
        let unheard = match waiting {
            Some(waiter) => waiter.send(result).err(),
            None => Some(result),
        };
        // Nobody asked any more (the app started again, or the page gave
        // up): the page still learns of a sign-in that worked.
        if let Some(Ok(profile)) = unheard {
            let _ = app.emit(SIGNED_IN_EVENT, profile);
        }
    });
}

async fn finish(state: &AccountState, url: &Url) -> Result<Profile, String> {
    let pending = state
        .take_pending()
        .ok_or("That sign-in is over. Tap Sign in again.")?;
    let code = match callback(url) {
        Callback::Code(code) => code,
        Callback::Error(message) => return Err(message),
    };
    let session = supabase::exchange_code(&client()?, &code, &pending.verifier).await?;
    let profile = session.profile.clone();
    state.store(Some(session))?;
    Ok(profile)
}

#[derive(Debug, PartialEq)]
enum Callback {
    Code(String),
    Error(String),
}

/// What the browser came back with: Supabase puts it in the query, or (for
/// some errors) after the `#`.
fn callback(url: &Url) -> Callback {
    let mut pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    if let Some(fragment) = url.fragment()
        && let Ok(parsed) = Url::parse(&format!("x:?{fragment}"))
    {
        pairs.extend(parsed.query_pairs().into_owned());
    }
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    if let Some(code) = get("code").filter(|code| !code.is_empty()) {
        return Callback::Code(code);
    }
    let message = get("error_description")
        .or_else(|| get("error"))
        .unwrap_or_else(|| "The sign-in came back without a code. Try again.".into());
    Callback::Error(message)
}

// ---------------------------------------------------------------------------
// Commands

/// The signed-in profile, if any. Reads the saved session; no network.
#[tauri::command(async)]
pub fn account_profile(state: State<'_, AccountState>) -> Option<Profile> {
    state.session().map(|session| session.profile)
}

/// Ends the sign-in waiting for its page, unless the page came back and it
/// is being finished.
fn cancel(state: &AccountState) {
    let waiter = {
        let mut inner = state.lock();
        if inner.returning {
            return;
        }
        inner.waiting.take()
    };
    if let Some(waiter) = waiter {
        let _ = waiter.send(Err("Sign-in was cancelled.".into()));
    }
    let _ = state.take_pending();
}

/// Signs in with the provider inside the app and returns who signed in.
#[tauri::command]
pub async fn account_sign_in(
    app: AppHandle,
    state: State<'_, AccountState>,
    provider: Provider,
) -> Result<Profile, String> {
    let (verifier, challenge) = supabase::pkce_pair();
    let authorize = supabase::authorize_url(provider, REDIRECT_URL, &challenge)?;
    let (waiter, answer) = oneshot::channel();
    {
        // A new sign-in replaces one still waiting (the user closed its page
        // without finishing it): that one ends as cancelled.
        let mut inner = state.lock();
        inner.waiting = Some(waiter);
        inner.returning = false;
    }
    state.save_pending(&verifier)?;

    // iOS answers when the sheet closes; Android at once (its page comes
    // back as a link, `listen`).
    let handle = app.clone();
    let address = authorize.to_string();
    let shown = tauri::async_runtime::spawn_blocking(move || handle.myle_mobile().sign_in(&address, SCHEME))
        .await
        .map_err(err)?;
    match shown {
        Ok(Some(back)) => returned(&app, Url::parse(&back).map_err(err)?),
        Ok(None) => {}
        Err(e) if e.contains(CANCELLED) => cancel(&state),
        // No sign-in page in the app here (Windows): the browser.
        Err(_) => app
            .opener()
            .open_url(authorize.as_str(), None::<&str>)
            .map_err(err)?,
    }
    tokio::select! {
        answer = answer => answer.unwrap_or_else(|_| Err("Sign-in was cancelled.".into())),
        _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => {
            cancel(&state);
            Err("Sign-in timed out. Try again.".into())
        }
    }
}

/// Stops waiting for the sign-in page (the user closed it, or gave up).
#[tauri::command]
pub async fn account_cancel_sign_in(app: AppHandle, state: State<'_, AccountState>) -> Result<(), String> {
    if state.lock().returning {
        return Ok(());
    }
    cancel(&state);
    tauri::async_runtime::spawn_blocking(move || app.myle_mobile().cancel_sign_in())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn account_sign_out(state: State<'_, AccountState>) -> Result<(), String> {
    if let Some(session) = state.session() {
        // Revokes the refresh token; the sign-out here happens regardless.
        supabase::logout(&client()?, &session).await;
    }
    state.store(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_comes_back_with_a_code_or_an_error() {
        let url = |text: &str| Url::parse(text).unwrap();
        assert_eq!(
            callback(&url("uk.thomast.myle.passwords://auth-callback?code=abc-123")),
            Callback::Code("abc-123".into())
        );
        assert_eq!(
            callback(&url(
                "uk.thomast.myle.passwords://auth-callback?error=access_denied&error_description=The+user+cancelled"
            )),
            Callback::Error("The user cancelled".into())
        );
        assert_eq!(
            callback(&url("uk.thomast.myle.passwords://auth-callback#error=server_error&error_description=Oops")),
            Callback::Error("Oops".into())
        );
        assert!(matches!(
            callback(&url("uk.thomast.myle.passwords://auth-callback")),
            Callback::Error(_)
        ));
    }

    #[test]
    fn a_sign_in_is_finished_once_and_not_too_late() {
        let dir = std::env::temp_dir().join(format!("myle-passwords-account-{}", std::process::id()));
        let state = AccountState::new(dir.clone());
        state.save_pending("verifier").unwrap();
        assert_eq!(state.take_pending().unwrap().verifier, "verifier");
        assert!(state.take_pending().is_none(), "the same return twice");

        let old = Pending {
            verifier: "old".into(),
            at: supabase::now() - PENDING_LIFETIME_SECS - 1,
        };
        write_private(&dir.join(PENDING_FILE), &serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(state.take_pending().is_none(), "a sign-in left long ago");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
