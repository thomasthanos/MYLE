//! What the page asks for. Every command here is refused for anyone but the owner (`account::owner`).
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, Url, WebviewUrl, WebviewWindowBuilder};
use super::session::{self, Session};
use super::steam_state;
const LOGIN_LABEL: &str = "steam-login";
const LOGIN_PAGE: &str = "https://steamcommunity.com/login/home/?goto=";
const COMMUNITY: &str = "https://steamcommunity.com";
const STORE: &str = "https://store.steampowered.com";
/// How long the sign-in window waits: the owner may need their phone.
const SIGN_IN_SECONDS: u32 = 600;
/// How long the store is given to hand over its own cookie after signing in.
const STORE_SECONDS: u32 = 15;
#[derive(Default)]
pub struct SteamAutomationState {
    session: Mutex<Option<Session>>,
}
impl SteamAutomationState {
    fn session(&self) -> Option<Session> {
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
        Ok(())
    }
    /// Stops everything that acts on the account.
    pub(crate) fn stop_all(&self) {}
}
fn folder() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join("steam-automation"))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    steam_running: bool,
    community_signed_in: bool,
    same_account: bool,
}
#[tauri::command]
pub fn steam_automation_status(state: State<'_, SteamAutomationState>) -> Status {
    let steam = steam_state::read();
    let session = state.session();
    Status {
        steam_running: steam.is_some(),
        community_signed_in: session.is_some(),
        same_account: match (&steam, &session) {
            (Some(steam), Some(session)) => steam_state::same_account(steam.active_user, session.steam_id),
            _ => false,
        },
    }
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
