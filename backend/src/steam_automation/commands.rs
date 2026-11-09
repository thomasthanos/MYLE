//! What the page asks for. Every command here is refused for anyone but the owner (`account::owner`).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State, Url, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Notify;
use super::community::HttpCommunity;
use super::engine::Status;
use super::library::{self, LibraryAnswer, Saved};
use super::runner::{GameRunner, HelperSpawner, ProcessRunner};
use super::session::{self, Session};
use super::settings::{self, Settings};
use crate::steam_helper::commands::SteamHelperState;
const LOGIN_LABEL: &str = "steam-login";
const REFRESH_LABEL: &str = "steam-refresh";
const LOGIN_PAGE: &str = "https://steamcommunity.com/login/home/?goto=";
const COMMUNITY: &str = "https://steamcommunity.com";
const STORE: &str = "https://store.steampowered.com";
/// How long the sign-in window waits: the owner may need their phone.
const SIGN_IN_SECONDS: u32 = 600;
/// How long the store is given to hand over its own cookie after signing in.
const STORE_SECONDS: u32 = 15;
/// How long a hidden page is given to renew an expired session.
const REFRESH_SECONDS: u32 = 20;
#[derive(Default)]
pub struct SteamAutomationState {
    session: Mutex<Option<Session>>,
    settings: Mutex<Option<Settings>>,
    status: Mutex<Status>,
    paused: AtomicBool,
    poke: Notify,
    runner: OnceLock<Arc<ProcessRunner<HelperSpawner>>>,
}
impl SteamAutomationState {
    pub(crate) fn session(&self) -> Option<Session> {
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if session.is_none() {
            *session = folder().ok().and_then(|folder| session::load(&folder.join("session.bin")));
        }
        session.clone()
    }
    fn set_session(&self, new: Session) -> Result<(), String> {
        let folder = folder()?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        new.save(&folder.join("session.bin"))?;
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = Some(new);
        self.poke.notify_one();
        Ok(())
    }
    pub(crate) fn settings(&self) -> Settings {
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings.get_or_insert_with(|| folder().map(|folder| settings::load(&folder.join("settings.json"))).unwrap_or_default()).clone()
    }
    pub(crate) fn paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    pub(crate) fn set_status(&self, status: Status) {
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = status;
    }
    pub(crate) fn set_runner(&self, runner: Arc<ProcessRunner<HelperSpawner>>) {
        let _ = self.runner.set(runner);
    }
    pub(crate) async fn poked(&self) {
        self.poke.notified().await;
    }
    /// Stops everything that acts on the account.
    pub(crate) fn stop_all(&self) {
        if let Some(runner) = self.runner.get() {
            runner.stop_all();
        }
    }
}
pub(crate) fn folder() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join("steam-automation"))
}
pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
#[tauri::command]
pub fn steam_automation_status(state: State<'_, SteamAutomationState>) -> Status {
    state.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
#[tauri::command]
pub fn steam_automation_settings(state: State<'_, SteamAutomationState>) -> Settings {
    state.settings()
}
#[tauri::command]
pub fn steam_automation_set_settings(state: State<'_, SteamAutomationState>, settings: Settings) -> Result<(), String> {
    settings::save(&folder()?.join("settings.json"), &settings)?;
    *state.settings.lock().unwrap_or_else(|p| p.into_inner()) = Some(settings);
    state.poke.notify_one();
    Ok(())
}
#[tauri::command]
pub fn steam_automation_pause(state: State<'_, SteamAutomationState>, paused: bool) {
    state.paused.store(paused, Ordering::SeqCst);
    state.poke.notify_one();
}
/// The owner's games, from a saved copy while it is a day old or less unless `force` asks again.
#[tauri::command]
pub async fn steam_automation_library(app: AppHandle, state: State<'_, SteamAutomationState>, force: bool) -> Result<LibraryAnswer, String> {
    let folder = folder()?;
    let file = folder.join("library.json");
    let saved: Option<Saved> = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if !force && let Some(saved) = saved.as_ref().filter(|saved| saved.is_fresh(now())) {
        return Ok(LibraryAnswer { games: saved.games.clone(), saved_at: saved.saved_at, problem: None });
    }
    let fetched = match state.session() {
        Some(session) => {
            let community = HttpCommunity::new()?;
            let steam_id = session.steam_id;
            community.set_session(Some(session));
            let store = app.state::<SteamHelperState>().store()?;
            library::fetch(&community, &*store, steam_id).await
        }
        None => Err("Sign in to Steam first.".to_string()),
    };
    let answer = library::settle(saved, fetched, now())?;
    if answer.problem.is_none() {
        let bytes = serde_json::to_vec(&Saved { saved_at: answer.saved_at, games: answer.games.clone() }).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let _ = crate::game_saves::atomic::write(&file, &bytes);
    }
    Ok(answer)
}
fn is_steam_site(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        ["steamcommunity.com", "steampowered.com"]
            .iter()
            .any(|site| host == *site || host.ends_with(&format!(".{site}")))
    })
}
fn cookies(window: &tauri::WebviewWindow, site: &Url) -> Vec<(String, String)> {
    window
        .cookies_for_url(site.clone())
        .unwrap_or_default()
        .iter()
        .map(|cookie| (cookie.name().to_string(), cookie.value().to_string()))
        .collect()
}
/// Opens Steam's own login page in a window with no access to MYLE, and waits for the owner to
/// finish. MYLE never sees what is typed there, only the cookies Steam sets afterwards.
#[tauri::command]
pub async fn steam_automation_sign_in(app: AppHandle, state: State<'_, SteamAutomationState>) -> Result<(), String> {
    if let Some(open) = app.get_webview_window(LOGIN_LABEL) {
        let _ = open.set_focus();
        return Ok(());
    }
    let page: Url = LOGIN_PAGE.parse().map_err(|_| "Bad sign-in address.".to_string())?;
    let community: Url = COMMUNITY.parse().map_err(|_| "Bad address.".to_string())?;
    let store: Url = STORE.parse().map_err(|_| "Bad address.".to_string())?;
    let window = WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(page))
        .title("Sign in to Steam")
        .inner_size(520.0, 760.0)
        .data_directory(folder()?.join("webview-steam"))
        .on_navigation(is_steam_site)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = crate::webview_policy::harden(&window);
    for _ in 0..SIGN_IN_SECONDS {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if app.get_webview_window(LOGIN_LABEL).is_none() {
            return Err("The sign-in window was closed.".to_string());
        }
        let Some(signed_in) = session::from_cookies(&cookies(&window, &community)) else {
            continue;
        };
        let _ = window.navigate(store.clone());
        let mut with_store = signed_in.clone();
        for _ in 0..STORE_SECONDS {
            tokio::time::sleep(Duration::from_secs(1)).await;
            with_store = signed_in.clone().with_store_cookies(&cookies(&window, &store));
            if with_store.store_cookie_header().is_some() {
                break;
            }
        }
        state.set_session(with_store)?;
        let _ = window.close();
        return Ok(());
    }
    let _ = window.close();
    Err("Signing in took too long.".to_string())
}
/// Loads Steam Community once, hidden, in the signed-in profile, which renews an expired session
/// if Steam still remembers the owner. `true` when a new session was saved.
pub(crate) async fn refresh_session(app: &AppHandle) -> bool {
    let (Ok(community), Ok(store), Ok(folder)) = (COMMUNITY.parse::<Url>(), STORE.parse::<Url>(), folder()) else {
        return false;
    };
    if app.get_webview_window(LOGIN_LABEL).is_some() || app.get_webview_window(REFRESH_LABEL).is_some() {
        return false;
    }
    let state = app.state::<SteamAutomationState>();
    let before = state.session();
    let Ok(window) = WebviewWindowBuilder::new(app, REFRESH_LABEL, WebviewUrl::External(community.clone()))
        .visible(false)
        .data_directory(folder.join("webview-steam"))
        .on_navigation(is_steam_site)
        .build()
    else {
        return false;
    };
    let mut refreshed = false;
    for _ in 0..REFRESH_SECONDS {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(found) = session::from_cookies(&cookies(&window, &community)) else {
            continue;
        };
        let found = found.with_store_cookies(&cookies(&window, &store));
        if Some(&found) != before.as_ref() {
            refreshed = state.set_session(found).is_ok();
            break;
        }
    }
    let _ = window.close();
    refreshed
}
