//! What the page asks for. Every command here is refused for anyone but the owner (`account::owner`).
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State, Url, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Notify;
use super::badges::{BadgeGame, read_badges};
use super::community::{CommunityError, HttpCommunity};
use super::claim::{self, Claimed};
use super::engine::{Pressed, Started, Status};
use super::inventory::{self, InventoryItem, ListOutcome, ListRequest, Listing};
use super::library::{self, LibraryAnswer, LibraryGame, Saved};
use super::runner::{GameRunner, HelperSpawner, ProcessRunner};
use super::session::{self, Session};
use super::settings::{self, Settings};
use crate::steam_helper::commands::SteamHelperState;
/// How long the card and inventory lists are kept before Steam is asked again.
const DROPS_FRESH_SECS: u64 = 600;
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
    /// Empty until the first look at the saved switches.
    started: Mutex<Option<Started>>,
    manual: Mutex<BTreeSet<u32>>,
    drops: Mutex<Option<(u64, Vec<BadgeGame>)>>,
    inventory: Mutex<Option<(u64, Vec<InventoryItem>)>>,
    /// Assets listed by hand this session, so the same one is never sent twice.
    listed: Arc<tokio::sync::Mutex<HashSet<String>>>,
    /// One line to Steam for the engine and the page, so together they stay within its limits.
    community: OnceLock<Arc<HttpCommunity>>,
    generation: AtomicU64,
    rejected: AtomicBool,
    pressed: Mutex<Pressed>,
    library: Mutex<Option<Vec<LibraryGame>>>,
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
        self.note_new_session();
        self.poke.notify_one();
        Ok(())
    }
    pub(crate) fn community(&self) -> Result<Arc<HttpCommunity>, String> {
        if let Some(community) = self.community.get() {
            return Ok(community.clone());
        }
        let community = Arc::new(HttpCommunity::new()?);
        Ok(self.community.get_or_init(|| community).clone())
    }
    /// The line to Steam, signed in as the owner.
    fn signed_in(&self) -> Result<(Arc<HttpCommunity>, Session), String> {
        let session = self.session().ok_or("Sign in to Steam first.")?;
        let community = self.community()?;
        community.set_session(Some(session.clone()));
        Ok((community, session))
    }
    /// Listed by hand or automatically, so neither lists what the other just did.
    pub(crate) fn listed(&self) -> Arc<tokio::sync::Mutex<HashSet<String>>> {
        self.listed.clone()
    }
    /// A page action was told the session is no good. The engine hears of it on its next look.
    fn note_refused(&self, error: &CommunityError) {
        if *error == CommunityError::SignedOut {
            self.rejected.store(true, Ordering::SeqCst);
            self.poke.notify_one();
        }
    }
    pub(crate) fn take_rejected(&self) -> bool {
        self.rejected.swap(false, Ordering::SeqCst)
    }
    fn refused(&self, error: CommunityError) -> String {
        self.note_refused(&error);
        say(error)
    }
    pub(crate) fn take_pressed(&self) -> Pressed {
        std::mem::take(&mut *self.pressed.lock().unwrap_or_else(|p| p.into_inner()))
    }
    /// So a game just added shows up the next time the library is looked at.
    fn mark_library_old(&self) {
        let Ok(file) = folder().map(|folder| folder.join("library.json")) else {
            return;
        };
        if let Some(saved) = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            && let Ok(bytes) = serde_json::to_vec(&Saved { saved_at: 0, games: saved.games })
        {
            let _ = crate::game_saves::atomic::write(&file, &bytes);
        }
    }
    /// Goes up whenever a new session is saved, even one with the same cookies.
    pub(crate) fn session_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    fn note_new_session(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
    /// The saved library, for checking refunds without asking Steam.
    pub(crate) fn library_games(&self) -> Vec<LibraryGame> {
        let mut games = self.library.lock().unwrap_or_else(|p| p.into_inner());
        games
            .get_or_insert_with(|| {
                folder()
                    .ok()
                    .and_then(|folder| std::fs::read(folder.join("library.json")).ok())
                    .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
                    .map(|saved| saved.games)
                    .unwrap_or_default()
            })
            .clone()
    }
    pub(crate) fn settings(&self) -> Settings {
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings.get_or_insert_with(|| folder().map(|folder| settings::load(&folder.join("settings.json"))).unwrap_or_default()).clone()
    }
    pub(crate) fn started(&self, settings: &Settings) -> Started {
        *self.started.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(|| Started::from_settings(settings))
    }
    pub(crate) fn manual(&self) -> Vec<u32> {
        self.manual.lock().unwrap_or_else(|p| p.into_inner()).iter().copied().collect()
    }
    pub(crate) fn paused(&self) -> bool {
        self.settings().paused
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
fn say(error: CommunityError) -> String {
    match error {
        CommunityError::SignedOut => "Your Steam sign-in has expired. Sign in again.".to_string(),
        CommunityError::Limited => "Steam is limiting requests right now. Try again in a minute.".to_string(),
        CommunityError::Other(reason) => reason,
    }
}
pub(crate) fn folder() -> Result<PathBuf, String> {
    crate::storage::local_item("steam-automation")
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
pub fn steam_automation_pause(state: State<'_, SteamAutomationState>, paused: bool) -> Result<(), String> {
    let settings = Settings { paused, ..state.settings() };
    settings::save(&folder()?.join("settings.json"), &settings)?;
    *state.settings.lock().unwrap_or_else(|p| p.into_inner()) = Some(settings);
    state.poke.notify_one();
    Ok(())
}
/// Starts or stops one feature for now. The automatic switches only decide what happens when MYLE opens.
#[tauri::command]
pub fn steam_automation_set_started(state: State<'_, SteamAutomationState>, feature: String, on: bool) -> Result<(), String> {
    let settings = state.settings();
    let mut started = state.started(&settings);
    match feature.as_str() {
        "farm" => started.farm = on,
        "idle" => started.idle = on,
        "achievements" => started.achievements = on,
        _ => return Err("Unknown feature.".to_string()),
    }
    *state.started.lock().unwrap_or_else(|p| p.into_inner()) = Some(started);
    if on {
        state.pressed.lock().unwrap_or_else(|p| p.into_inner()).all = true;
    }
    state.poke.notify_one();
    Ok(())
}
/// Idles one game by hand, or stops it.
#[tauri::command]
pub fn steam_automation_idle_game(state: State<'_, SteamAutomationState>, appid: u32, on: bool) {
    let mut manual = state.manual.lock().unwrap_or_else(|p| p.into_inner());
    if on {
        manual.insert(appid);
        state.pressed.lock().unwrap_or_else(|p| p.into_inner()).games.push(appid);
    } else {
        manual.remove(&appid);
    }
    drop(manual);
    state.poke.notify_one();
}
/// Stops everything that is running, by feature and by hand. The automatic switches stay as they are.
#[tauri::command]
pub fn steam_automation_stop_all(state: State<'_, SteamAutomationState>) {
    *state.started.lock().unwrap_or_else(|p| p.into_inner()) = Some(Started::default());
    state.manual.lock().unwrap_or_else(|p| p.into_inner()).clear();
    state.stop_all();
    state.poke.notify_one();
}
/// The games with card drops left, kept for a few minutes because each page takes seconds to read.
#[tauri::command]
pub async fn steam_automation_drops(state: State<'_, SteamAutomationState>, force: bool) -> Result<Vec<BadgeGame>, String> {
    let kept = state.drops.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if !force && let Some((_, games)) = kept.filter(|(read_at, _)| now().saturating_sub(*read_at) < DROPS_FRESH_SECS) {
        return Ok(games);
    }
    let (community, session) = state.signed_in()?;
    let games = read_badges(&community, session.steam_id).await.map_err(|error| state.refused(error))?;
    *state.drops.lock().unwrap_or_else(|p| p.into_inner()) = Some((now(), games.clone()));
    Ok(games)
}
/// Everything in the Steam inventory, kept for a few minutes.
#[tauri::command]
pub async fn steam_automation_inventory(state: State<'_, SteamAutomationState>, force: bool) -> Result<Vec<InventoryItem>, String> {
    let kept = state.inventory.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if !force && let Some((_, items)) = kept.filter(|(read_at, _)| now().saturating_sub(*read_at) < DROPS_FRESH_SECS) {
        return Ok(items);
    }
    let (community, session) = state.signed_in()?;
    let items = inventory::read_items(&community, session.steam_id).await.map_err(|error| state.refused(error))?;
    *state.inventory.lock().unwrap_or_else(|p| p.into_inner()) = Some((now(), items.clone()));
    Ok(items)
}
/// The cheapest price on the Market right now, in cents.
#[tauri::command]
pub async fn steam_automation_price(state: State<'_, SteamAutomationState>, hash_name: String) -> Result<Option<u32>, String> {
    let (community, _) = state.signed_in()?;
    inventory::lowest_price(&community, &hash_name).await.map_err(|error| state.refused(error))
}
/// Lists one item. The page sends them one at a time so it can show how far it has got.
#[tauri::command]
pub async fn steam_automation_list_item(state: State<'_, SteamAutomationState>, item: ListRequest) -> Result<ListOutcome, String> {
    let (community, session) = state.signed_in()?;
    let settings = state.settings();
    let locked: HashSet<String> = settings.locked_assets.iter().cloned().collect();
    let mut listed = state.listed.lock().await;
    inventory::list_item(&community, &session, &item, settings.minimum_cents, &locked, &mut listed).await.map_err(|error| state.refused(error))
}
#[tauri::command]
pub async fn steam_automation_listings(state: State<'_, SteamAutomationState>) -> Result<Vec<Listing>, String> {
    let (community, _) = state.signed_in()?;
    inventory::read_listings(&community).await.map_err(|error| state.refused(error))
}
#[tauri::command]
pub async fn steam_automation_remove_listing(state: State<'_, SteamAutomationState>, listing_id: String, asset_id: String) -> Result<(), String> {
    let (community, session) = state.signed_in()?;
    let mut listed = state.listed.lock().await;
    inventory::remove_listing(&community, &session, &listing_id, &asset_id, &mut listed).await.map_err(|error| state.refused(error))
}
/// Adds a game that is free to keep to the owner's account.
#[tauri::command]
pub async fn steam_automation_claim(app: AppHandle, state: State<'_, SteamAutomationState>, appid: u32) -> Result<Claimed, String> {
    let (community, session) = state.signed_in()?;
    let store = app.state::<SteamHelperState>().store()?;
    let claimed = claim::claim(&community, &*store, &session, appid).await;
    match &claimed {
        Ok(_) => state.mark_library_old(),
        Err(reason) if reason == library::SIGNED_OUT => state.note_refused(&CommunityError::SignedOut),
        Err(_) => {}
    }
    claimed
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
    let fetched = match state.signed_in() {
        Ok((community, session)) => {
            let store = app.state::<SteamHelperState>().store()?;
            library::fetch(&community, &*store, session.steam_id).await
        }
        Err(reason) => Err(reason),
    };
    if fetched.as_ref().is_err_and(|reason| reason == library::SIGNED_OUT) {
        state.note_refused(&CommunityError::SignedOut);
    }
    let answer = library::settle(saved, fetched, now())?;
    if answer.problem.is_none() {
        *state.library.lock().unwrap_or_else(|p| p.into_inner()) = Some(answer.games.clone());
        let bytes = serde_json::to_vec(&Saved { saved_at: answer.saved_at, games: answer.games.clone() }).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let _ = crate::game_saves::atomic::write(&file, &bytes);
    }
    Ok(answer)
}
/// Whether the sign-in window can close. Cookies from before do not count until Steam has
/// shown it accepts them by moving off its login page.
fn sign_in_done(found: &Session, before: Option<&Session>, left_login_page: bool) -> bool {
    left_login_page || before.is_none_or(|before| before.steam_login_secure != found.steam_login_secure)
}
fn left_login_page(url: &Url) -> bool {
    is_steam_site(url) && !url.path().starts_with("/login")
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
    let before = state.session();
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
        let left = window.url().is_ok_and(|url| left_login_page(&url));
        let Some(signed_in) = session::from_cookies(&cookies(&window, &community)).filter(|found| sign_in_done(found, before.as_ref(), left)) else {
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
        state.set_session(with_store.keeping_store_cookie_of(before.as_ref()))?;
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
    let mut found = None;
    for _ in 0..REFRESH_SECONDS {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(cookies) = session::from_cookies(&cookies(&window, &community)) else {
            continue;
        };
        let renewed = before.as_ref().is_none_or(|before| before.steam_login_secure != cookies.steam_login_secure);
        found = Some(cookies);
        if renewed {
            break;
        }
    }
    let Some(found) = found else {
        let _ = window.close();
        return false;
    };
    // The store keeps its own sign-in, which only renews when the store itself is visited.
    let _ = window.navigate(store.clone());
    let mut renewed = found.clone();
    for _ in 0..STORE_SECONDS {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let candidate = found.clone().with_store_cookies(&cookies(&window, &store));
        if candidate.store_cookie_header().is_some() {
            let changed = before.as_ref().is_none_or(|before| before.store_cookie_header() != candidate.store_cookie_header());
            renewed = candidate;
            if changed {
                break;
            }
        }
    }
    let _ = window.close();
    let renewed = renewed.keeping_store_cookie_of(before.as_ref());
    Some(&renewed) != before.as_ref() && state.set_session(renewed).is_ok()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam_automation::session::from_cookies;
    fn session(token: &str) -> Session {
        from_cookies(&[("steamLoginSecure".to_string(), format!("76561198000000001%7C%7C{token}")), ("sessionid".to_string(), "s".to_string())]).unwrap()
    }
    #[test]
    fn the_cookies_that_were_already_there_do_not_finish_signing_in() {
        assert!(!sign_in_done(&session("old"), Some(&session("old")), false));
    }
    #[test]
    fn new_cookies_finish_signing_in() {
        assert!(sign_in_done(&session("new"), Some(&session("old")), false));
        assert!(sign_in_done(&session("new"), None, false));
    }
    #[test]
    fn steam_moving_off_its_login_page_accepts_the_same_cookies() {
        assert!(sign_in_done(&session("old"), Some(&session("old")), true));
    }
    #[test]
    fn only_a_steam_page_past_the_login_counts_as_having_left_it() {
        let page = |address: &str| left_login_page(&address.parse().unwrap());
        assert!(!page("about:blank"), "the window has not loaded anything yet");
        assert!(!page("https://steamcommunity.com/login/home/?goto="));
        assert!(page("https://steamcommunity.com/id/someone"));
        assert!(page("https://store.steampowered.com/"));
    }
    #[test]
    fn a_session_a_page_action_found_rejected_asks_for_sign_in() {
        let state = SteamAutomationState::default();
        assert!(!state.take_rejected());
        state.note_refused(&CommunityError::Limited);
        assert!(!state.take_rejected());
        state.note_refused(&CommunityError::SignedOut);
        assert!(state.take_rejected());
        assert!(!state.take_rejected(), "once told is enough");
    }
    #[test]
    fn every_saved_session_counts_as_new() {
        let state = SteamAutomationState::default();
        state.note_new_session();
        state.note_new_session();
        assert_eq!(state.session_generation(), 2);
    }
}
