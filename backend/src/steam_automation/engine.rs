//! Runs the four features on their own: each tick looks at the world, decides, and acts.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use super::achievements::{AchievementList, AchievementTool, HelperTool, UnlockQueue, next_delay_secs};
use super::badges::{BadgeGame, bought_recently, is_refundable, read_badges, read_licences};
use super::commands::{SteamAutomationState, folder, now, refresh_session};
use super::community::{Community, CommunityError};
use super::decide::{FarmStep, Waiting, World, decide, farm_step, filter_cards};
use super::inventory::{SellReport, sell_duplicates};
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
const SESSION_RETRY_SECS: u64 = 30 * 60;
/// Achievement lists read in one tick, so a long list of games cannot hold everything up.
const LISTS_PER_TICK: usize = 3;
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
    waiting: Option<Waiting>,
    settings: &'a Settings,
    started: Started,
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
    farm_read: bool,
    next_badges_at: u64,
    queue: UnlockQueue,
    lists: HashMap<u32, AchievementList>,
    next_unlock_at: u64,
    listed: HashSet<String>,
    report: SellReport,
    next_inventory_at: u64,
}
impl<C: Community, A: AchievementTool> Engine<C, A> {
    pub(crate) fn new(runner: Arc<dyn GameRunner>, community: C, tool: A, random: Box<dyn FnMut() -> u64 + Send>) -> Self {
        Self {
            runner,
            community,
            tool,
            random,
            activity: Activity::default(),
            community_ok: true,
            farm_games: Vec::new(),
            licences: None,
            farm_read: false,
            next_badges_at: 0,
            queue: UnlockQueue::default(),
            lists: HashMap::new(),
            next_unlock_at: 0,
            listed: HashSet::new(),
            report: SellReport::default(),
            next_inventory_at: 0,
        }
    }
    /// A new session has arrived, so Steam gets another chance to accept it.
    pub(crate) fn session_changed(&mut self) {
        self.community_ok = true;
        self.next_badges_at = 0;
        self.next_inventory_at = 0;
    }
    pub(crate) fn mark_session_refreshed(&mut self) {
        self.community_ok = true;
    }
    fn note_community_error(&mut self, error: &CommunityError, now: u64) {
        match error {
            CommunityError::SignedOut => self.community_ok = false,
            CommunityError::Limited => self.activity.push(now, "Steam is limiting requests, trying again later".to_string()),
            CommunityError::Other(reason) => self.activity.push(now, reason.clone()),
        }
    }
    async fn read_farm(&mut self, steam_id: u64, now: u64) {
        let random = (self.random)();
        self.next_badges_at = now + CHECK_MIN_SECS + random % (CHECK_SPREAD_SECS + 1);
        let games = match read_badges(&self.community, steam_id).await {
            Ok(games) => games,
            Err(error) => {
                self.note_community_error(&error, now);
                return;
            }
        };
        for before in &self.farm_games {
            let left = games.iter().find(|game| game.appid == before.appid).map_or(0, |game| game.drops);
            if left < before.drops {
                let left = if left == 0 { "none".to_string() } else { left.to_string() };
                self.activity.push(now, format!("A card dropped in {}, {left} left", before.name));
            }
        }
        self.farm_games = games;
        self.farm_read = true;
        self.licences = match read_licences(&self.community).await {
            Ok(licences) => Some(licences),
            Err(error) => {
                if error == CommunityError::SignedOut {
                    self.community_ok = false;
                }
                None
            }
        };
    }
    async fn run_achievements(&mut self, picked: &[u32], now: u64) {
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
    async fn run_inventory(&mut self, session: &Session, settings: &Settings, now: u64) {
        let locked: HashSet<String> = settings.locked_assets.iter().cloned().collect();
        match sell_duplicates(&self.community, session, settings.minimum_cents, &locked, &mut self.listed).await {
            Ok(report) => {
                self.next_inventory_at = now + INVENTORY_SECS;
                if report.listed > 0 {
                    self.activity.push(now, format!("Listed {} duplicate cards", report.listed));
                }
                self.report = report;
            }
            Err(error) => {
                self.next_inventory_at = now + RETRY_SECS;
                self.note_community_error(&error, now);
            }
        }
    }
    fn view(&self, seen: Seen<'_>) -> Status {
        let Seen { world, farm, candidates, waiting, settings, started, session_exists, now } = seen;
        let skipped = candidates
            .iter()
            .filter(|game| {
                let recent = self.licences.as_deref().map(|list| bought_recently(&game.name, list, now));
                is_refundable(recent, game.minutes_played)
            })
            .count() as u32;
        let phase = match farm {
            FarmStep::Accumulate(_) => "building",
            FarmStep::Solo(_) => "solo",
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
            purchase_dates_unknown: self.farm_read && self.licences.is_none(),
            started,
            manual: world.manual.clone(),
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
                waiting_for_farming: started.achievements && started.farm && *farm != FarmStep::Done,
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
            self.runner.stop_all();
            return Status::default();
        }
        let TickInput { steam, session, settings, started, manual, user_paused, .. } = input;
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
        let candidates = filter_cards(&self.farm_games, &settings.card_whitelist, &settings.card_blacklist);
        let farm = if started.farm {
            farm_step(&candidates, self.licences.as_deref(), now)
        } else {
            FarmStep::Done
        };
        let world = World {
            steam_running: steam.is_some(),
            account_ok,
            real_game: real,
            user_paused,
            farm_on: started.farm,
            farm: farm.clone(),
            idle_on: started.idle,
            idle_games: settings.idle_games.clone(),
            manual,
            session_ok: session.is_some() && self.community_ok,
        };
        let decision = decide(&world);
        if let Err(reason) = self.runner.apply(&decision.games) {
            self.activity.push(now, reason);
        }
        if decision.achievements_may_run && started.achievements {
            self.run_achievements(&settings.achievement_games, now).await;
        }
        if decision.inventory_may_run && settings.inventory_on && now >= self.next_inventory_at
            && let Some(session) = &session
        {
            self.run_inventory(session, &settings, now).await;
        }
        self.view(Seen { world: &world, farm: &farm, candidates: &candidates, waiting: decision.waiting, settings: &settings, started, session_exists: session.is_some(), now })
    }
}
/// Starts the loop that runs for as long as MYLE does.
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<SteamAutomationState>();
        let (Ok(spawner), Ok(community), Ok(folder)) = (HelperSpawner::new(), state.community(), folder()) else {
            return;
        };
        let runner = Arc::new(ProcessRunner::new(spawner));
        state.set_runner(runner.clone());
        let random = || uuid::Uuid::new_v4().as_u128() as u64;
        let mut engine = Engine::new(runner, community, HelperTool, Box::new(random));
        let activity_file = folder.join("activity.json");
        engine.activity = Activity::load(&activity_file);
        let mut last_session = None;
        let mut next_refresh = 0;
        tokio::time::sleep(FIRST_TICK).await;
        loop {
            let session = state.session();
            if session != last_session {
                engine.session_changed();
                last_session = session.clone();
            }
            engine.community.set_session(session.clone());
            let settings = state.settings();
            let input = TickInput {
                steam: steam_state::read(),
                session: session.clone(),
                started: state.started(&settings),
                manual: state.manual(),
                settings,
                user_paused: state.paused(),
                is_owner: app.state::<crate::account::AccountState>().is_owner(&app),
            };
            let status = engine.tick(input, now()).await;
            engine.activity.save(&activity_file);
            let needs_refresh = session.is_some() && !status.community_ok && now() >= next_refresh;
            state.set_status(status);
            if needs_refresh {
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
        let engine = Engine::new(runner.clone(), community(drops.clone()), tool.clone(), Box::new(|| 0));
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
            Ok(r#"{"assets":[],"descriptions":[],"success":1}"#.to_string())
        })
    }
    fn rig() -> Rig {
        rig_with(normal_community)
    }
    /// Everything the switches ask for has been started.
    fn input(settings: Settings) -> TickInput {
        let started = Started { farm: settings.farm_on, idle: settings.idle_on, achievements: settings.achievements_on };
        TickInput { steam: Some(steam(&[])), session: Some(session(MY_ID)), settings, started, manual: Vec::new(), user_paused: false, is_owner: true }
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
    async fn a_game_off_the_whitelist_is_not_farmed() {
        let mut rig = rig();
        let status = rig.engine.tick(input(Settings { card_whitelist: vec![1], ..farming() }), T0).await;
        assert!(running(&rig).is_empty());
        assert_eq!((status.farm.games_left, status.farm.drops_left), (0, 0));
    }
    #[tokio::test]
    async fn a_blacklisted_game_is_not_farmed() {
        let mut rig = rig();
        rig.engine.tick(input(Settings { card_blacklist: vec![GAME], ..farming() }), T0).await;
        assert!(running(&rig).is_empty());
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
    async fn stopping_achievements_stops_unlocking() {
        let mut rig = rig();
        rig.tool.lists.lock().unwrap().insert(GAME, AchievementList { names: vec!["A".into()], locked: vec!["A".into()] });
        rig.engine.tick(TickInput { started: Started::default(), ..input(achievements()) }, T0).await;
        assert!(rig.tool.unlocked.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn the_idler_runs_the_picked_games() {
        let mut rig = rig();
        rig.engine.tick(input(Settings { idle_on: true, idle_games: vec![7, 8], ..Settings::default() }), T0).await;
        assert_eq!(running(&rig), BTreeSet::from([7, 8]));
    }
}
