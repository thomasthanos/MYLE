//! Runs the four features on their own: each tick looks at the world, decides, and acts.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use super::achievements::{AchievementList, AchievementTool, HelperTool, UnlockQueue, next_delay_secs};
use super::badges::{BadgeGame, RefundCheck, read_badges, read_licences};
use super::commands::{SteamAutomationState, folder, now, refresh_session};
use super::community::{Community, CommunityError};
use super::decide::{FarmStep, Waiting, World, decide, farm_step, filter_cards, idle_allowed};
use super::inventory::{SellReport, sell_duplicates};
use super::library::LibraryGame;
use super::runner::{GameRunner, HelperSpawner, ProcessRunner};
use super::session::Session;
use super::settings::{Activity, ActivityEntry, Settings};
use super::steam_state::{self, SteamState, real_game, same_account};
const TICK: Duration = Duration::from_secs(60);
const FIRST_TICK: Duration = Duration::from_secs(5);
const CHECK_MIN_SECS: u64 = 20 * 60;
const CHECK_SPREAD_SECS: u64 = 10 * 60;
const INVENTORY_SECS: u64 = 6 * 3600;
const RETRY_SECS: u64 = 3600;
const FARM_RETRY_SECS: u64 = 300;
/// How long a game is left alone after its helper ended without MYLE asking. Doubles each time.
const HANDS_OFF_SECS: u64 = 300;
const HANDS_OFF_MAX_SECS: u64 = 3600;
const SESSION_RETRY_SECS: u64 = 30 * 60;
/// Achievement lists read in one tick, so a long list of games cannot hold everything up.
const LISTS_PER_TICK: usize = 3;
/// What is about to act on the account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Job {
    Selling,
    Unlocking,
}
pub(crate) type KeepGoing = Arc<dyn Fn(Job) -> bool + Send + Sync>;
/// Things the owner pressed since the last tick: a helper's waiting period does not outrank them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Pressed {
    pub all: bool,
    pub games: Vec<u32>,
}
/// What is running for now. Starts from the automatic switches when MYLE opens and is never saved.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub farm: bool,
    pub idle: bool,
    pub achievements: bool,
}
impl Started {
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self { farm: settings.farm_on, idle: settings.idle_on, achievements: settings.achievements_on }
    }
}
pub(crate) struct TickInput {
    pub steam: Option<SteamState>,
    pub session: Option<Session>,
    pub settings: Settings,
    pub started: Started,
    pub manual: Vec<u32>,
    pub library: Vec<LibraryGame>,
    /// Asked before each listing or unlock, so a long tick can be stopped.
    pub keep_going: KeepGoing,
    pub pressed: Pressed,
    pub user_paused: bool,
    pub is_owner: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingView {
    pub kind: &'static str,
    pub appid: Option<u32>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FarmView {
    pub phase: &'static str,
    pub games_left: u32,
    pub drops_left: u32,
    pub skipped_refundable: u32,
    pub next_check_at: Option<u64>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsView {
    pub total: u32,
    pub locked: u32,
    pub next_at: Option<u64>,
    pub waiting_for_farming: bool,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryView {
    pub listed: u32,
    pub cents: u32,
    pub waiting_confirmation: u32,
    pub next_at: Option<u64>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub ready: bool,
    pub steam_running: bool,
    pub community_signed_in: bool,
    pub community_ok: bool,
    pub waiting: Option<WaitingView>,
    pub purchase_dates_unknown: bool,
    pub started: Started,
    pub manual: Vec<u32>,
    pub idle_skipped: u32,
    pub store_stale: bool,
    pub unmatched_licence: Option<String>,
    pub running: Vec<u32>,
    pub farm: FarmView,
    pub achievements: AchievementsView,
    pub inventory: InventoryView,
    pub activity: Vec<ActivityEntry>,
}
/// What one tick found, for the page.
struct Seen<'a> {
    world: &'a World,
    farm: &'a FarmStep,
    candidates: &'a [BadgeGame],
    refunds: &'a RefundCheck,
    waiting: Option<Waiting>,
    settings: &'a Settings,
    started: Started,
    idle_skipped: u32,
    session_exists: bool,
    now: u64,
}
pub(crate) struct Engine<C, A> {
    runner: Arc<dyn GameRunner>,
    pub(crate) community: C,
    tool: A,
    random: Box<dyn FnMut() -> u64 + Send>,
    pub(crate) activity: Activity,
    community_ok: bool,
    farm_games: Vec<BadgeGame>,
    licences: Option<Vec<(String, u64)>>,
    purchases_read: bool,
    next_purchases_at: u64,
    store_stale: bool,
    farm_read: bool,
    next_badges_at: u64,
    last_wanted: BTreeSet<u32>,
    strikes: HashMap<u32, u32>,
    hands_off: HashMap<u32, u64>,
    queue: UnlockQueue,
    lists: HashMap<u32, AchievementList>,
    next_unlock_at: u64,
    listed: Arc<tokio::sync::Mutex<HashSet<String>>>,
    report: SellReport,
    next_inventory_at: u64,
}
impl<C: Community, A: AchievementTool> Engine<C, A> {
    pub(crate) fn new(runner: Arc<dyn GameRunner>, community: C, tool: A, listed: Arc<tokio::sync::Mutex<HashSet<String>>>, random: Box<dyn FnMut() -> u64 + Send>) -> Self {
        Self {
            runner,
            community,
            tool,
            random,
            activity: Activity::default(),
            community_ok: true,
            farm_games: Vec::new(),
            licences: None,
            purchases_read: false,
            next_purchases_at: 0,
            store_stale: false,
            farm_read: false,
            next_badges_at: 0,
            last_wanted: BTreeSet::new(),
            strikes: HashMap::new(),
            hands_off: HashMap::new(),
            queue: UnlockQueue::default(),
            lists: HashMap::new(),
            next_unlock_at: 0,
            listed,
            report: SellReport::default(),
            next_inventory_at: 0,
        }
    }
    /// A new session has arrived, so Steam gets another chance to accept it.
    /// Not the owner: nothing runs.
    pub(crate) fn rest(&mut self) {
        self.runner.stop_all();
        self.last_wanted.clear();
    }
    /// Lists the page has just read, or (`None`) one it made old by changing that game's
    /// achievements, which is then read again on the next tick.
    pub(crate) fn take_lists(&mut self, fresh: Vec<(u32, Option<AchievementList>)>) {
        for (appid, list) in fresh {
            match list {
                Some(list) => {
                    self.lists.insert(appid, list);
                }
                None => {
                    self.lists.remove(&appid);
                }
            }
        }
    }
    /// A page action found the session refused, and with nothing started the engine would not have.
    pub(crate) fn session_rejected(&mut self) {
        self.community_ok = false;
    }
    #[cfg(test)]
    pub(crate) fn community_lost_for_test(&mut self) {
        self.community_ok = false;
    }
    pub(crate) fn session_changed(&mut self) {
        self.community_ok = true;
        self.store_stale = false;
        self.next_badges_at = 0;
        self.next_purchases_at = 0;
        self.next_inventory_at = 0;
    }
    pub(crate) fn activity_to_save(&mut self) -> bool {
        self.activity.take_changed()
    }
    pub(crate) fn mark_session_refreshed(&mut self) {
        self.community_ok = true;
        self.store_stale = false;
    }
    fn note_community_error(&mut self, error: &CommunityError, now: u64) {
        match error {
            CommunityError::SignedOut => self.community_ok = false,
            CommunityError::Limited => self.activity.push(now, "Steam is limiting requests, trying again later".to_string()),
            CommunityError::Other(reason) => self.activity.push(now, reason.clone()),
        }
    }
    fn next_look(&mut self, now: u64) -> u64 {
        now + CHECK_MIN_SECS + (self.random)() % (CHECK_SPREAD_SECS + 1)
    }
    async fn read_farm(&mut self, steam_id: u64, now: u64) {
        let games = match read_badges(&self.community, steam_id).await {
            Ok(games) => games,
            Err(error) => {
                self.next_badges_at = now + FARM_RETRY_SECS;
                self.note_community_error(&error, now);
                return;
            }
        };
        self.next_badges_at = self.next_look(now);
        for before in &self.farm_games {
            let left = games.iter().find(|game| game.appid == before.appid).map_or(0, |game| game.drops);
            if left < before.drops {
                let left = if left == 0 { "none".to_string() } else { left.to_string() };
                self.activity.push(now, format!("A card dropped in {}, {left} left", before.name));
            }
        }
        self.farm_games = games;
        self.farm_read = true;
    }
    /// Licences live on the store, whose sign-in is separate, so losing it only means the dates
    /// are unknown for now. It says nothing about Steam Community.
    async fn read_purchases(&mut self, now: u64) {
        self.next_purchases_at = self.next_look(now);
        self.purchases_read = true;
        match read_licences(&self.community).await {
            Ok(licences) => {
                self.licences = Some(licences);
                self.store_stale = false;
            }
            Err(error) => {
                self.licences = None;
                self.store_stale = error == CommunityError::SignedOut;
            }
        }
    }
    /// What the owner pressed comes before any waiting period.
    fn forgive(&mut self, pressed: Pressed) {
        if pressed.all {
            self.hands_off.clear();
            self.strikes.clear();
        }
        for appid in pressed.games {
            self.hands_off.remove(&appid);
            self.strikes.remove(&appid);
        }
    }
    /// The games that should really run. A helper that ended without MYLE asking means the owner
    /// pressed Stop in Steam, or Steam would not run it, so that game is left alone for a while.
    fn supervise(&mut self, games: &BTreeSet<u32>, now: u64) -> BTreeSet<u32> {
        // Pressed while this tick was reading: what it decided is already out of date.
        if self.runner.take_stopped_all() {
            self.last_wanted.clear();
            return BTreeSet::new();
        }
        let alive = self.runner.running();
        let expected: Vec<u32> = self.last_wanted.intersection(games).copied().collect();
        for appid in expected {
            if alive.contains(&appid) {
                self.strikes.remove(&appid);
                continue;
            }
            let strikes = self.strikes.entry(appid).or_insert(0);
            *strikes += 1;
            let wait = HANDS_OFF_SECS.saturating_mul(1 << (*strikes - 1).min(8)).min(HANDS_OFF_MAX_SECS);
            self.hands_off.insert(appid, now + wait);
            self.activity.push(now, format!("Game {appid} stopped without MYLE asking, trying again in {} min", wait / 60));
        }
        self.hands_off.retain(|_, until| *until > now);
        let wanted: BTreeSet<u32> = games.iter().copied().filter(|appid| !self.hands_off.contains_key(appid)).collect();
        self.last_wanted = wanted.clone();
        wanted
    }
    async fn run_achievements(&mut self, picked: &[u32], keep_going: &KeepGoing, now: u64) {
        let missing: Vec<u32> = picked.iter().copied().filter(|appid| !self.lists.contains_key(appid)).take(LISTS_PER_TICK).collect();
        for appid in missing {
            let list = self.tool.list(appid).await.unwrap_or_else(|reason| {
                self.activity.push(now, format!("Could not read the achievements of game {appid}: {reason}"));
                AchievementList::default()
            });
            self.lists.insert(appid, list);
        }
        if now < self.next_unlock_at {
            return;
        }
        let random = (self.random)();
        let Some((appid, name)) = self.queue.next(picked, &self.lists, random) else {
            return;
        };
        // Reading the lists can take minutes, and the owner may have stopped this since.
        if !keep_going(Job::Unlocking) {
            return;
        }
        match self.tool.unlock(appid, &name).await {
            Ok(()) => {
                if let Some(list) = self.lists.get_mut(&appid) {
                    list.locked.retain(|locked| *locked != name);
                }
                self.activity.push(now, format!("Unlocked an achievement in game {appid}"));
                self.next_unlock_at = now + next_delay_secs((self.random)());
            }
            Err(_) => {
                self.queue.mark_refused(appid, &name);
                self.activity.push(now, format!("Steam refused an achievement in game {appid}"));
            }
        }
    }
    async fn run_inventory(&mut self, session: &Session, settings: &Settings, keep_going: KeepGoing, now: u64) {
        let locked: HashSet<String> = settings.locked_assets.iter().cloned().collect();
        let (report, error) = sell_duplicates(&self.community, session, settings.minimum_cents, &locked, &mut *self.listed.lock().await, &|| keep_going(Job::Selling)).await;
        if report.listed > 0 {
            let cards = if report.listed == 1 { "card" } else { "cards" };
            let confirm = match report.waiting_confirmation {
                0 => "",
                1 => " Confirm it in Steam Guard.",
                _ => " Confirm them in Steam Guard.",
            };
            self.activity.push(now, format!("Listed {} duplicate {cards}.{confirm}", report.listed));
        }
        self.report = SellReport { waiting_confirmation: self.report.waiting_confirmation + report.waiting_confirmation, ..report };
        self.next_inventory_at = now + if error.is_some() { RETRY_SECS } else { INVENTORY_SECS };
        if let Some(error) = error {
            self.note_community_error(&error, now);
        }
    }
    fn view(&self, seen: Seen<'_>) -> Status {
        let Seen { world, farm, candidates, refunds, waiting, settings, started, idle_skipped, session_exists, now } = seen;
        let skipped = candidates.iter().filter(|game| refunds.protects(&game.name, game.minutes_played)).count() as u32;
        let phase = match farm {
            _ if started.farm && !world.session_ok => "signedOut",
            FarmStep::Accumulate(_) => "building",
            FarmStep::Solo(_) => "solo",
            FarmStep::Reading => "reading",
            FarmStep::Done => "done",
        };
        Status {
            ready: true,
            steam_running: world.steam_running,
            community_signed_in: session_exists,
            community_ok: session_exists && self.community_ok,
            waiting: waiting.map(|waiting| match waiting {
                Waiting::NoSteam => WaitingView { kind: "noSteam", appid: None },
                Waiting::OtherAccount => WaitingView { kind: "otherAccount", appid: None },
                Waiting::RealGame(appid) => WaitingView { kind: "realGame", appid: Some(appid) },
                Waiting::Paused => WaitingView { kind: "paused", appid: None },
            }),
            purchase_dates_unknown: self.purchases_read && self.licences.is_none(),
            started,
            manual: world.manual.clone(),
            idle_skipped,
            store_stale: self.store_stale,
            unmatched_licence: refunds.unmatched().map(str::to_string),
            running: self.runner.running().into_iter().collect(),
            farm: FarmView {
                phase,
                games_left: candidates.len() as u32,
                drops_left: candidates.iter().map(|game| game.drops).sum(),
                skipped_refundable: skipped,
                next_check_at: (started.farm && self.farm_read).then_some(self.next_badges_at),
            },
            achievements: AchievementsView {
                total: self.lists.values().map(|list| list.names.len() as u32).sum(),
                locked: self.lists.values().map(|list| list.locked.len() as u32).sum(),
                next_at: (started.achievements && self.next_unlock_at > now).then_some(self.next_unlock_at),
                waiting_for_farming: started.achievements && started.farm && world.session_ok && *farm != FarmStep::Done,
            },
            inventory: InventoryView {
                listed: self.report.listed,
                cents: self.report.cents,
                waiting_confirmation: self.report.waiting_confirmation,
                next_at: (settings.inventory_on && self.next_inventory_at > now).then_some(self.next_inventory_at),
            },
            activity: self.activity.entries.clone(),
        }
    }
    pub(crate) async fn tick(&mut self, input: TickInput, now: u64) -> Status {
        if !input.is_owner {
            self.rest();
            return Status::default();
        }
        let TickInput { steam, session, settings, started, manual, library, keep_going, user_paused, pressed, .. } = input;
        self.forgive(pressed);
        self.hands_off.retain(|_, until| *until > now);
        let account_ok = match (&steam, &session) {
            (Some(steam), Some(session)) => same_account(steam.active_user, session.steam_id),
            _ => true,
        };
        let ours = self.runner.running();
        let real = steam.as_ref().and_then(|steam| real_game(&steam.running, &ours));
        let clear_to_run = steam.is_some() && account_ok && real.is_none() && !user_paused;
        if started.farm && clear_to_run && self.community_ok && now >= self.next_badges_at
            && let Some(session) = &session
        {
            self.read_farm(session.steam_id, now).await;
        }
        if (started.farm || started.idle) && clear_to_run && self.community_ok && session.is_some() && now >= self.next_purchases_at {
            self.read_purchases(now).await;
        }
        // A game being left alone must not hold up the games behind it.
        let candidates: Vec<BadgeGame> = filter_cards(&self.farm_games, &settings.card_blacklist).into_iter().filter(|game| !self.hands_off.contains_key(&game.appid)).collect();
        let names: Vec<String> = library.iter().map(|game| game.name.clone()).collect();
        let refunds = RefundCheck::new(self.licences.as_deref(), &names, now);
        let farm = match (started.farm, self.farm_read) {
            (false, _) => FarmStep::Done,
            (true, false) => FarmStep::Reading,
            (true, true) => farm_step(&candidates, &refunds),
        };
        let idle_games = idle_allowed(&settings.idle_games, &library, &refunds);
        let idle_skipped = if started.idle { (settings.idle_games.len() - idle_games.len()) as u32 } else { 0 };
        let world = World {
            steam_running: steam.is_some(),
            account_ok,
            real_game: real,
            user_paused,
            farm_on: started.farm,
            farm: farm.clone(),
            idle_on: started.idle,
            idle_games,
            manual,
            session_ok: session.is_some() && self.community_ok,
        };
        let decision = decide(&world);
        let wanted = self.supervise(&decision.games, now);
        if let Err(reason) = self.runner.apply(&wanted) {
            self.activity.push(now, reason);
        }
        if decision.achievements_may_run && started.achievements {
            self.run_achievements(&settings.achievement_games, &keep_going, now).await;
        }
        if decision.inventory_may_run && settings.inventory_on && now >= self.next_inventory_at
            && let Some(session) = &session
        {
            self.run_inventory(session, &settings, keep_going, now).await;
        }
        self.view(Seen { world: &world, farm: &farm, candidates: &candidates, refunds: &refunds, waiting: decision.waiting, settings: &settings, started, idle_skipped, session_exists: session.is_some(), now })
    }
}
/// Whether the hidden page should renew the session.
pub(crate) fn needs_refresh(status: &Status, signed_in: bool) -> bool {
    status.ready && signed_in && (!status.community_ok || status.store_stale)
}
/// Starts the loop that runs for as long as MYLE does.
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<SteamAutomationState>();
        let (Ok(spawner), Ok(community), Ok(folder)) = (HelperSpawner::new(), state.community(), folder()) else {
            return;
        };
        let spawner_job = spawner.job();
        let runner = Arc::new(ProcessRunner::new(spawner));
        state.set_runner(runner.clone());
        state.set_job(spawner_job.clone());
        let random = || uuid::Uuid::new_v4().as_u128() as u64;
        let job = spawner_job.clone();
        let mut engine = Engine::new(runner.clone(), community, HelperTool { job }, state.listed(), Box::new(random));
        let activity_file = folder.join("activity.json");
        engine.activity = Activity::load(&activity_file);
        let mut last_generation = 0;
        let mut next_refresh = 0;
        tokio::time::sleep(FIRST_TICK).await;
        loop {
            // Anyone but the owner: Steam's state, the session and the library are not even read.
            if !app.state::<crate::account::AccountState>().is_owner(&app) {
                engine.rest();
                state.set_status(Status::default());
                tokio::select! {
                    _ = tokio::time::sleep(TICK) => {}
                    _ = state.poked() => {}
                }
                continue;
            }
            engine.take_lists(state.take_fresh_lists());
            let session = state.session();
            if state.take_rejected() {
                engine.session_rejected();
            }
            let generation = state.session_generation();
            if generation != last_generation {
                engine.session_changed();
                last_generation = generation;
            }
            engine.community.set_session(session.clone());
            let settings = state.settings();
            let keep_going: KeepGoing = {
                let app = app.clone();
                let runner = runner.clone();
                Arc::new(move |job: Job| {
                    let state = app.state::<SteamAutomationState>();
                    let settings = state.settings();
                    let wanted = match job {
                        Job::Selling => settings.inventory_on,
                        Job::Unlocking => state.started(&settings).achievements,
                    };
                    let ours = runner.running();
                    let real = steam_state::read().and_then(|steam| real_game(&steam.running, &ours));
                    wanted && !settings.paused && real.is_none() && app.state::<crate::account::AccountState>().is_owner(&app)
                })
            };
            let input = TickInput {
                steam: steam_state::read(),
                session: session.clone(),
                started: state.started(&settings),
                manual: state.manual(),
                library: state.library_games(),
                keep_going,
                pressed: state.take_pressed(),
                settings,
                user_paused: state.paused(),
                is_owner: app.state::<crate::account::AccountState>().is_owner(&app),
            };
            let status = engine.tick(input, now()).await;
            if status.ready && engine.activity_to_save() {
                engine.activity.save(&activity_file);
            }
            let refresh_now = needs_refresh(&status, session.is_some()) && now() >= next_refresh;
            state.set_status(status);
            if refresh_now {
                next_refresh = now() + SESSION_RETRY_SECS;
                if refresh_session(&app).await {
                    engine.mark_session_refreshed();
                    continue;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = state.poked() => {}
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use super::*;
    use crate::steam_automation::achievements::AchievementList;
    use crate::steam_automation::community::{CommunityError, FakeCommunity};
    use crate::steam_automation::runner::FakeRunner;
    use crate::steam_automation::session::from_cookies;
    const ME: u32 = 12345;
    const MY_ID: u64 = 76561197960265728 + 12345;
    const GAME: u32 = 620;
    const HOUR: u64 = 3600;
    fn session(id: u64) -> Session {
        from_cookies(&[("steamLoginSecure".to_string(), format!("{id}%7C%7Ct")), ("sessionid".to_string(), "s".to_string())]).unwrap()
    }
    fn steam(running: &[u32]) -> SteamState {
        SteamState { path: PathBuf::from("x"), active_user: ME, running: running.to_vec() }
    }
    fn badges(drops: u32) -> String {
        format!(r#"<div class="badge_row is_link"><a href="x/gamecards/{GAME}/"></a> 9.0 hrs on record <span class="progress_info_bold">{drops} card drops remaining</span><div class="badge_title">Portal 2 &nbsp;<span></span></div></div>"#)
    }
    const NOTHING_LISTED: &str = r#"{"success":true,"total_count":0,"listings":[]}"#;
    const LICENCES: &str = r#"<td  class="license_date_col">1 Jan, 2020</td> <td>Portal 2</td>"#;
    #[derive(Default)]
    struct FakeTool {
        lists: Mutex<HashMap<u32, AchievementList>>,
        unlocked: Mutex<Vec<(u32, String)>>,
    }
    impl AchievementTool for Arc<FakeTool> {
        async fn list(&self, appid: u32) -> Result<AchievementList, String> {
            Ok(self.lists.lock().unwrap().get(&appid).cloned().unwrap_or_default())
        }
        async fn unlock(&self, appid: u32, name: &str) -> Result<(), String> {
            self.unlocked.lock().unwrap().push((appid, name.to_string()));
            Ok(())
        }
    }
    struct Rig {
        runner: Arc<FakeRunner>,
        tool: Arc<FakeTool>,
        drops: Arc<AtomicU32>,
        engine: Engine<FakeCommunity, Arc<FakeTool>>,
    }
    fn rig_with(community: impl Fn(Arc<AtomicU32>) -> FakeCommunity) -> Rig {
        let runner = Arc::new(FakeRunner::default());
        let tool = Arc::new(FakeTool::default());
        let drops = Arc::new(AtomicU32::new(3));
        let engine = Engine::new(runner.clone(), community(drops.clone()), tool.clone(), Arc::default(), Box::new(|| 0));
        Rig { runner, tool, drops, engine }
    }
    fn normal_community(drops: Arc<AtomicU32>) -> FakeCommunity {
        FakeCommunity::answering(move |url| {
            if url.contains("/badges/") {
                return Ok(badges(drops.load(Ordering::SeqCst)));
            }
            if url.contains("licenses") {
                return Ok(LICENCES.to_string());
            }
            if url.contains("mylistings") {
                return Ok(NOTHING_LISTED.to_string());
            }
            Ok(r#"{"assets":[],"descriptions":[],"success":1}"#.to_string())
        })
    }
    fn rig() -> Rig {
        rig_with(normal_community)
    }
    /// Everything the switches ask for has been started.
    fn input(settings: Settings) -> TickInput {
        let started = Started { farm: settings.farm_on, idle: settings.idle_on, achievements: settings.achievements_on };
        let library = [(7, "Seven", 500), (8, "Eight", 500), (GAME, "Portal 2", 540)].map(|(appid, name, minutes)| owned(appid, name, minutes)).to_vec();
        TickInput { steam: Some(steam(&[])), session: Some(session(MY_ID)), settings, started, manual: Vec::new(), library, keep_going: Arc::new(|_| true), pressed: Pressed::default(), user_paused: false, is_owner: true }
    }
    fn owned(appid: u32, name: &str, minutes: u32) -> LibraryGame {
        LibraryGame { appid, name: name.to_string(), cover_url: None, header_url: None, minutes, achievements_unlocked: 0, achievements_total: 0, achievements_ok: true, shared: false }
    }
    fn farming() -> Settings {
        Settings { farm_on: true, ..Settings::default() }
    }
    fn running(rig: &Rig) -> BTreeSet<u32> {
        rig.runner.current.lock().unwrap().clone()
    }
    fn badge_reads(rig: &Rig) -> usize {
        rig.engine.community.gets().iter().filter(|u| u.contains("/badges/")).count()
    }
    const T0: u64 = 1_800_000_000;
    #[tokio::test]
    async fn everything_off_touches_nothing() {
        let mut rig = rig();
        rig.engine.tick(input(Settings::default()), T0).await;
        assert!(running(&rig).is_empty());
        assert!(rig.engine.community.gets().is_empty());
    }
    #[test]
    fn resting_stops_every_game_and_asks_steam_nothing() {
        let mut rig = rig();
        rig.engine.rest();
        assert_eq!(*rig.runner.stops.lock().unwrap(), 1);
        assert!(rig.engine.community.gets().is_empty());
    }
    #[tokio::test]
    async fn not_the_owner_stops_everything() {
        let mut rig = rig();
        rig.engine.tick(TickInput { is_owner: false, ..input(farming()) }, T0).await;
        assert_eq!(*rig.runner.stops.lock().unwrap(), 1);
        assert!(rig.engine.community.gets().is_empty());
    }
    #[tokio::test]
    async fn a_game_with_drops_is_run() {
        let mut rig = rig();
        let status = rig.engine.tick(input(farming()), T0).await;
        assert_eq!(running(&rig), BTreeSet::from([GAME]));
        assert_eq!((status.farm.games_left, status.farm.drops_left), (1, 3));
        assert!(status.ready, "the page can tell a real answer from no answer yet");
    }
    #[tokio::test]
    async fn a_real_game_stops_and_closing_it_resumes() {
        let mut rig = rig();
        rig.engine.tick(input(farming()), T0).await;
        let status = rig.engine.tick(TickInput { steam: Some(steam(&[9, GAME])), ..input(farming()) }, T0 + 60).await;
        assert!(running(&rig).is_empty());
        assert_eq!(status.waiting.as_ref().map(|w| (w.kind, w.appid)), Some(("realGame", Some(9))));
        let status = rig.engine.tick(input(farming()), T0 + 120).await;
        assert_eq!(running(&rig), BTreeSet::from([GAME]));
        assert!(status.waiting.is_none());
    }
    #[tokio::test]
    async fn badges_are_read_again_after_the_wait_not_before() {
        let mut rig = rig();
        rig.engine.tick(input(farming()), T0).await;
        rig.engine.tick(input(farming()), T0 + 19 * 60).await;
        assert_eq!(badge_reads(&rig), 1);
        rig.engine.tick(input(farming()), T0 + 31 * 60).await;
        assert_eq!(badge_reads(&rig), 2);
    }
    #[tokio::test]
    async fn a_drop_is_written_to_activity() {
        let mut rig = rig();
        rig.engine.tick(input(farming()), T0).await;
        rig.drops.store(2, Ordering::SeqCst);
        let status = rig.engine.tick(input(farming()), T0 + 31 * 60).await;
        assert!(status.activity.iter().any(|entry| entry.text == "A card dropped in Portal 2, 2 left"), "{:?}", status.activity);
    }
    #[tokio::test]
    async fn a_lost_session_waits_and_says_so() {
        let mut rig = rig_with(|_| FakeCommunity::answering(|_| Err(CommunityError::SignedOut)));
        let settings = Settings { inventory_on: true, ..farming() };
        let status = rig.engine.tick(input(settings), T0).await;
        assert!(!status.community_ok);
        assert!(rig.engine.community.posts().is_empty());
    }
    #[tokio::test]
    async fn unreadable_purchase_dates_are_reported() {
        let mut rig = rig_with(|drops| {
            FakeCommunity::answering(move |url| {
                if url.contains("licenses") {
                    return Err(CommunityError::Other("no".to_string()));
                }
                Ok(badges(drops.load(Ordering::SeqCst)))
            })
        });
        assert!(rig.engine.tick(input(farming()), T0).await.purchase_dates_unknown);
    }
    #[tokio::test]
    async fn the_wrong_account_runs_nothing() {
        let mut rig = rig();
        let status = rig.engine.tick(TickInput { session: Some(session(MY_ID + 1)), ..input(farming()) }, T0).await;
        assert!(running(&rig).is_empty());
        assert_eq!(status.waiting.map(|w| w.kind), Some("otherAccount"));
        assert!(rig.engine.community.gets().is_empty());
    }
    fn achievements() -> Settings {
        Settings { achievements_on: true, achievement_games: vec![GAME], ..Settings::default() }
    }
    #[tokio::test]
    async fn achievements_wait_for_their_delay() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into(), "B".into()], locked: vec!["A".into(), "B".into()] });
        rig.engine.tick(input(achievements()), T0).await;
        assert_eq!(rig.tool.unlocked.lock().unwrap().len(), 1);
        rig.engine.tick(input(achievements()), T0 + 60).await;
        assert_eq!(rig.tool.unlocked.lock().unwrap().len(), 1);
        rig.engine.tick(input(achievements()), T0 + 301).await;
        assert_eq!(rig.tool.unlocked.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn the_inventory_runs_every_six_hours() {
        let mut rig = rig();
        let settings = Settings { inventory_on: true, ..Settings::default() };
        let inventory_reads = |rig: &Rig| rig.engine.community.gets().iter().filter(|u| u.contains("/inventory/")).count();
        rig.engine.tick(input(settings.clone()), T0).await;
        assert_eq!(inventory_reads(&rig), 1);
        rig.engine.tick(input(settings.clone()), T0 + 6 * HOUR - 1).await;
        assert_eq!(inventory_reads(&rig), 1);
        rig.engine.tick(input(settings), T0 + 6 * HOUR).await;
        assert_eq!(inventory_reads(&rig), 2);
    }
    #[test]
    fn the_automatic_switches_decide_what_starts_when_myle_opens() {
        let settings = Settings { farm_on: true, achievements_on: true, ..Settings::default() };
        assert_eq!(Started::from_settings(&settings), Started { farm: true, idle: false, achievements: true });
    }
    #[tokio::test]
    async fn an_automatic_switch_alone_does_not_keep_a_stopped_feature_running() {
        let mut rig = rig();
        let settings = farming();
        rig.engine.tick(TickInput { started: Started::default(), ..input(settings) }, T0).await;
        assert!(running(&rig).is_empty());
        assert!(rig.engine.community.gets().is_empty());
    }
    #[tokio::test]
    async fn start_works_with_the_automatic_switch_off() {
        let mut rig = rig();
        let started = Started { farm: true, ..Started::default() };
        rig.engine.tick(TickInput { started, ..input(Settings::default()) }, T0).await;
        assert_eq!(running(&rig), BTreeSet::from([GAME]));
    }
    #[tokio::test]
    async fn a_skipped_game_is_not_farmed() {
        let mut rig = rig();
        let status = rig.engine.tick(input(Settings { card_blacklist: vec![GAME], ..farming() }), T0).await;
        assert!(running(&rig).is_empty());
        assert_eq!((status.farm.games_left, status.farm.drops_left), (0, 0));
    }
    #[tokio::test]
    async fn a_game_idled_by_hand_is_run_and_reported() {
        let mut rig = rig();
        let started = Started { farm: true, ..Started::default() };
        let status = rig.engine.tick(TickInput { manual: vec![7], started, ..input(farming()) }, T0).await;
        assert_eq!(running(&rig), BTreeSet::from([7, GAME]));
        assert_eq!(status.manual, vec![7]);
        assert_eq!(status.started, started);
    }
    #[tokio::test]
    async fn a_list_read_on_the_page_is_used_from_the_next_tick() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into(), "B".into()], locked: vec!["A".into()] });
        rig.engine.take_lists(vec![(GAME, Some(AchievementList { names: vec!["A".into(), "B".into()], locked: vec!["B".into()] }))]);
        rig.engine.tick(input(achievements()), T0).await;
        assert_eq!(*rig.tool.unlocked.lock().unwrap(), [(GAME, "B".to_string())]);
    }
    #[tokio::test]
    async fn a_list_changed_on_the_page_is_read_again() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into()], locked: vec!["A".into()] });
        rig.engine.tick(input(achievements()), T0).await;
        // The page locked A again: the engine's copy says nothing is left.
        rig.engine.take_lists(vec![(GAME, None)]);
        rig.engine.tick(input(achievements()), T0 + 1801).await;
        assert_eq!(rig.tool.unlocked.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn stopping_achievements_stops_unlocking() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into()], locked: vec!["A".into()] });
        rig.engine.tick(TickInput { started: Started::default(), ..input(achievements()) }, T0).await;
        assert!(rig.tool.unlocked.lock().unwrap().is_empty());
    }
    fn idling(games: Vec<u32>) -> Settings {
        Settings { idle_on: true, idle_games: games, ..Settings::default() }
    }
    const BOUGHT_YESTERDAY: &str = r#"<td  class="license_date_col">14 Jan, 2027</td> <td>Portal 2</td>"#;
    #[tokio::test]
    async fn a_game_that_can_still_be_refunded_is_not_idled() {
        let mut rig = rig_with(|_| FakeCommunity::answering(|url| Ok(if url.contains("licenses") { BOUGHT_YESTERDAY.to_string() } else { String::new() })));
        let library = vec![owned(GAME, "Portal 2", 30), owned(7, "Seven", 500)];
        let status = rig.engine.tick(TickInput { library, ..input(idling(vec![GAME, 7])) }, T0).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
        assert_eq!(status.idle_skipped, 1);
    }
    #[tokio::test]
    async fn purchases_are_read_for_the_idler_alone() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        assert_eq!(rig.engine.community.gets().iter().filter(|url| url.contains("licenses")).count(), 1);
    }
    #[tokio::test]
    async fn a_failed_first_read_does_not_say_there_are_no_cards() {
        let mut rig = rig_with(|_| FakeCommunity::answering(|_| Err(CommunityError::Other("no".to_string()))));
        let settings = Settings { idle_on: true, idle_games: vec![7], ..farming() };
        let status = rig.engine.tick(input(settings), T0).await;
        assert_eq!(status.farm.phase, "reading");
        assert!(running(&rig).is_empty(), "the idler waits until the cards are known");
    }
    #[tokio::test]
    async fn a_failed_read_is_tried_again_soon() {
        let mut rig = rig_with(|_| FakeCommunity::answering(|_| Err(CommunityError::Other("no".to_string()))));
        rig.engine.tick(input(farming()), T0).await;
        rig.engine.tick(input(farming()), T0 + 299).await;
        assert_eq!(badge_reads(&rig), 1);
        rig.engine.tick(input(farming()), T0 + 301).await;
        assert_eq!(badge_reads(&rig), 2);
    }
    #[tokio::test]
    async fn a_lost_session_stops_the_game_being_farmed_and_says_so() {
        let mut rig = rig();
        rig.engine.tick(input(farming()), T0).await;
        assert_eq!(running(&rig), BTreeSet::from([GAME]));
        rig.engine.community_lost_for_test();
        let status = rig.engine.tick(input(farming()), T0 + 60).await;
        assert!(running(&rig).is_empty());
        assert_eq!(status.farm.phase, "signedOut");
    }
    #[tokio::test]
    async fn a_helper_that_stops_by_itself_is_left_alone_for_a_while() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        rig.runner.current.lock().unwrap().remove(&7);
        let status = rig.engine.tick(input(idling(vec![7])), T0 + 60).await;
        assert!(running(&rig).is_empty(), "it may be the owner pressing Stop in Steam");
        assert!(status.activity.iter().any(|entry| entry.text.contains("stopped without MYLE asking")), "{:?}", status.activity);
        rig.engine.tick(input(idling(vec![7])), T0 + 120).await;
        assert!(running(&rig).is_empty());
        rig.engine.tick(input(idling(vec![7])), T0 + 361).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn a_helper_that_keeps_stopping_is_left_longer_each_time() {
        let mut rig = rig();
        let settings = idling(vec![7]);
        rig.engine.tick(input(settings.clone()), T0).await;
        rig.runner.current.lock().unwrap().remove(&7);
        rig.engine.tick(input(settings.clone()), T0 + 60).await;
        rig.engine.tick(input(settings.clone()), T0 + 361).await;
        rig.runner.current.lock().unwrap().remove(&7);
        rig.engine.tick(input(settings.clone()), T0 + 421).await;
        rig.engine.tick(input(settings.clone()), T0 + 421 + 599).await;
        assert!(running(&rig).is_empty(), "ten minutes the second time");
        rig.engine.tick(input(settings), T0 + 421 + 601).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn stopping_a_game_on_purpose_is_not_mistaken_for_it_stopping_by_itself() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        rig.runner.stop_all();
        let status = rig.engine.tick(input(Settings::default()), T0 + 60).await;
        assert!(status.activity.is_empty(), "{:?}", status.activity);
        rig.engine.tick(input(idling(vec![7])), T0 + 120).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn a_licence_page_that_says_signed_out_only_affects_the_store() {
        let mut rig = rig_with(|drops| {
            FakeCommunity::answering(move |url| {
                if url.contains("licenses") {
                    return Err(CommunityError::SignedOut);
                }
                Ok(badges(drops.load(Ordering::SeqCst)))
            })
        });
        let status = rig.engine.tick(input(farming()), T0).await;
        assert!(status.community_ok);
        assert!(status.purchase_dates_unknown);
        assert!(status.store_stale);
    }
    #[tokio::test]
    async fn listings_from_a_run_that_ended_early_are_still_recorded() {
        let inventory = r#"{"assets":[{"assetid":"1","classid":"1","instanceid":"0"},{"assetid":"2","classid":"1","instanceid":"0"},{"assetid":"3","classid":"1","instanceid":"0"}],"descriptions":[{"classid":"1","instanceid":"0","marketable":1,"market_hash_name":"A","tags":[{"category":"item_class","internal_name":"item_class_2"}]}],"success":1}"#;
        let posted = Arc::new(AtomicU32::new(0));
        let mut rig = rig_with(|_| {
            let posted = posted.clone();
            FakeCommunity::answering(move |url| Ok(if url.contains("mylistings") { NOTHING_LISTED.to_string() } else if url.contains("priceoverview") { r#"{"lowest_price":"0,31€"}"#.to_string() } else { inventory.to_string() }))
                .answering_posts(move |_| {
                    if posted.fetch_add(1, Ordering::SeqCst) == 0 { Ok(r#"{"success":true,"needs_mobile_confirmation":true}"#.to_string()) } else { Err(CommunityError::SignedOut) }
                })
        });
        let status = rig.engine.tick(input(Settings { inventory_on: true, ..Settings::default() }), T0).await;
        assert_eq!((status.inventory.listed, status.inventory.waiting_confirmation), (1, 1));
        assert!(status.activity.iter().any(|entry| entry.text == "Listed 1 duplicate card. Confirm it in Steam Guard."), "{:?}", status.activity);
    }
    #[tokio::test]
    async fn pausing_during_the_list_reads_stops_the_unlock() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into()], locked: vec!["A".into()] });
        let keep_going: KeepGoing = Arc::new(|job| job != Job::Unlocking);
        rig.engine.tick(TickInput { keep_going, ..input(achievements()) }, T0).await;
        assert!(rig.tool.unlocked.lock().unwrap().is_empty());
    }
    fn two_games(drops: Arc<AtomicU32>) -> FakeCommunity {
        FakeCommunity::answering(move |url| {
            if url.contains("/badges/") {
                let _ = drops.load(Ordering::SeqCst);
                let row = |appid: u32, name: &str, left: u32| format!(r#"<div class="badge_row is_link"><a href="x/gamecards/{appid}/"></a> 9.0 hrs on record <span class="progress_info_bold">{left} card drops remaining</span><div class="badge_title">{name} &nbsp;<span></span></div></div>"#);
                return Ok(format!("{}{}", row(GAME, "Portal 2", 1), row(7, "Seven", 2)));
            }
            Ok(LICENCES.to_string())
        })
    }
    #[tokio::test]
    async fn a_game_left_alone_does_not_hold_up_the_next_one() {
        let mut rig = rig_with(two_games);
        rig.engine.tick(input(farming()), T0).await;
        assert_eq!(running(&rig), BTreeSet::from([GAME]));
        rig.runner.current.lock().unwrap().remove(&GAME);
        rig.engine.tick(input(farming()), T0 + 60).await;
        rig.engine.tick(input(farming()), T0 + 120).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn stop_all_during_a_read_is_not_counted_as_stopping_by_itself() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        rig.runner.stop_all();
        let status = rig.engine.tick(input(idling(vec![7])), T0 + 1).await;
        assert!(status.activity.is_empty(), "{:?}", status.activity);
        assert!(running(&rig).is_empty(), "this tick started before the press");
        rig.engine.tick(input(idling(vec![7])), T0 + 2).await;
        assert_eq!(running(&rig), BTreeSet::from([7]), "no waiting period was imposed");
    }
    #[tokio::test]
    async fn pressing_play_clears_the_wait() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        rig.runner.current.lock().unwrap().remove(&7);
        rig.engine.tick(input(idling(vec![7])), T0 + 60).await;
        assert!(running(&rig).is_empty());
        let pressed = Pressed { all: false, games: vec![7] };
        rig.engine.tick(TickInput { pressed, ..input(idling(vec![7])) }, T0 + 61).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn pressing_start_clears_every_wait() {
        let mut rig = rig();
        rig.engine.tick(input(idling(vec![7])), T0).await;
        rig.runner.current.lock().unwrap().remove(&7);
        rig.engine.tick(input(idling(vec![7])), T0 + 60).await;
        rig.engine.tick(TickInput { pressed: Pressed { all: true, games: vec![] }, ..input(idling(vec![7])) }, T0 + 61).await;
        assert_eq!(running(&rig), BTreeSet::from([7]));
    }
    #[tokio::test]
    async fn a_session_refused_elsewhere_shows_on_the_page() {
        let mut rig = rig();
        rig.engine.session_rejected();
        assert!(!rig.engine.tick(input(Settings::default()), T0).await.community_ok);
    }
    #[tokio::test]
    async fn the_page_is_told_which_purchase_could_not_be_matched() {
        const RECENT: &str = r#"<td  class="license_date_col">14 Jan, 2027</td> <td>Mystery Bundle</td>"#;
        let mut rig = rig_with(|_| FakeCommunity::answering(|url| Ok(if url.contains("licenses") { RECENT.to_string() } else { String::new() })));
        let status = rig.engine.tick(input(idling(vec![7])), T0).await;
        assert_eq!(status.unmatched_licence.as_deref(), Some("Mystery Bundle"));
    }
    #[test]
    fn someone_who_is_not_the_owner_never_opens_the_hidden_page() {
        assert!(!needs_refresh(&Status::default(), true));
    }
    #[test]
    fn a_rejected_session_or_store_cookie_asks_for_a_refresh() {
        let rejected = Status { ready: true, community_ok: false, ..Status::default() };
        let stale = Status { ready: true, community_ok: true, store_stale: true, ..Status::default() };
        let fine = Status { ready: true, community_ok: true, ..Status::default() };
        assert!(needs_refresh(&rejected, true));
        assert!(needs_refresh(&stale, true));
        assert!(!needs_refresh(&fine, true));
        assert!(!needs_refresh(&rejected, false), "nothing to renew");
    }
    #[tokio::test]
    async fn the_idler_runs_the_picked_games() {
        let mut rig = rig();
        rig.engine.tick(input(Settings { idle_on: true, idle_games: vec![7, 8], ..Settings::default() }), T0).await;
        assert_eq!(running(&rig), BTreeSet::from([7, 8]));
    }
}
