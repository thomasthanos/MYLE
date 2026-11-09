//! Which games should be running right now. Nothing here touches Steam, so every rule can be tested.
use std::collections::BTreeSet;
use super::badges::{BadgeGame, RefundCheck};
use super::library::LibraryGame;
/// Steam starts dropping cards after this much playtime.
const FARM_THRESHOLD_MINUTES: u32 = 120;
const MAX_GAMES: usize = 32;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FarmStep {
    Accumulate(Vec<u32>),
    Solo(u32),
    /// Asked for, but Steam has not said yet which games have drops.
    Reading,
    Done,
}
/// The queued games the idler may run.
pub(crate) fn idle_allowed(queue: &[u32], library: &[LibraryGame], refunds: &RefundCheck) -> Vec<u32> {
    queue
        .iter()
        .copied()
        .filter(|appid| library.iter().find(|game| game.appid == *appid).is_some_and(|game| !refunds.protects(&game.name, game.minutes)))
        .collect()
}
/// What card farming should do next.
pub(crate) fn farm_step(games: &[BadgeGame], refunds: &RefundCheck) -> FarmStep {
    let allowed: Vec<&BadgeGame> = games.iter().filter(|game| !refunds.protects(&game.name, game.minutes_played)).collect();
    let mut building: Vec<&BadgeGame> = allowed.iter().copied().filter(|game| game.minutes_played < FARM_THRESHOLD_MINUTES).collect();
    if !building.is_empty() {
        building.sort_by_key(|game| (std::cmp::Reverse(game.minutes_played), game.appid));
        return FarmStep::Accumulate(building.iter().take(MAX_GAMES).map(|game| game.appid).collect());
    }
    match allowed.iter().min_by_key(|game| (game.drops, game.appid)) {
        Some(game) => FarmStep::Solo(game.appid),
        None => FarmStep::Done,
    }
}
/// The games card farming may touch: all of them but those the owner chose to skip.
pub(crate) fn filter_cards(games: &[BadgeGame], skipped: &[u32]) -> Vec<BadgeGame> {
    games.iter().filter(|game| !skipped.contains(&game.appid)).cloned().collect()
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct World {
    pub steam_running: bool,
    pub account_ok: bool,
    pub real_game: Option<u32>,
    pub user_paused: bool,
    pub farm_on: bool,
    pub farm: FarmStep,
    pub idle_on: bool,
    pub idle_games: Vec<u32>,
    pub manual: Vec<u32>,
    pub session_ok: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Waiting {
    NoSteam,
    OtherAccount,
    RealGame(u32),
    Paused,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Decision {
    pub games: BTreeSet<u32>,
    pub waiting: Option<Waiting>,
    pub achievements_may_run: bool,
    pub inventory_may_run: bool,
}
fn stopped(waiting: Waiting) -> Decision {
    Decision { games: BTreeSet::new(), waiting: Some(waiting), achievements_may_run: false, inventory_may_run: false }
}
pub(crate) fn decide(world: &World) -> Decision {
    if !world.steam_running {
        return stopped(Waiting::NoSteam);
    }
    if !world.account_ok {
        return stopped(Waiting::OtherAccount);
    }
    if let Some(game) = world.real_game {
        return stopped(Waiting::RealGame(game));
    }
    if world.user_paused {
        return stopped(Waiting::Paused);
    }
    // Without a session the drops cannot be checked, so a game must not idle on with no end.
    let farming = if world.farm_on && world.session_ok { &world.farm } else { &FarmStep::Done };
    let mut games: BTreeSet<u32> = match farming {
        FarmStep::Accumulate(list) => list.iter().copied().collect(),
        FarmStep::Solo(game) => BTreeSet::from([*game]),
        FarmStep::Done if world.idle_on => world.idle_games.iter().copied().collect(),
        FarmStep::Done | FarmStep::Reading => BTreeSet::new(),
    };
    let room = MAX_GAMES.saturating_sub(games.len());
    let by_hand: Vec<u32> = world.manual.iter().copied().filter(|game| !games.contains(game)).take(room).collect();
    games.extend(by_hand);
    Decision {
        games,
        waiting: None,
        achievements_may_run: *farming == FarmStep::Done,
        inventory_may_run: world.session_ok,
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use super::*;
    const NOW: u64 = 1_794_096_000;
    fn g(appid: u32, drops: u32, minutes_played: u32) -> BadgeGame {
        BadgeGame { appid, name: format!("Game {appid}"), drops, minutes_played }
    }
    fn none_recent() -> RefundCheck {
        RefundCheck::new(Some(&[]), &[], NOW)
    }
    fn refunds_with(licences: &[(String, u64)]) -> RefundCheck {
        RefundCheck::new(Some(licences), &["Game 1".to_string(), "Game 2".to_string()], NOW)
    }
    fn owned(appid: u32, name: &str, minutes: u32) -> LibraryGame {
        LibraryGame { appid, name: name.to_string(), cover_url: None, header_url: None, minutes, achievements_unlocked: 0, achievements_total: 0, achievements_ok: false, shared: false }
    }
    #[test]
    fn games_under_two_hours_build_up_together() {
        let step = farm_step(&[g(1, 1, 30), g(2, 1, 60), g(3, 1, 500)], &none_recent());
        assert_eq!(step, FarmStep::Accumulate(vec![2, 1]));
    }
    #[test]
    fn then_one_at_a_time_fewest_drops_first() {
        assert_eq!(farm_step(&[g(2, 3, 200), g(3, 1, 500)], &none_recent()), FarmStep::Solo(3));
    }
    #[test]
    fn exactly_two_hours_is_ready() {
        assert_eq!(farm_step(&[g(1, 1, 120)], &none_recent()), FarmStep::Solo(1));
    }
    #[test]
    fn refundable_games_are_skipped() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        assert_eq!(farm_step(&[g(1, 1, 30)], &refunds_with(&licences)), FarmStep::Done);
    }
    #[test]
    fn a_recent_game_past_two_hours_is_not_skipped() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        assert_eq!(farm_step(&[g(1, 1, 130)], &refunds_with(&licences)), FarmStep::Solo(1));
    }
    #[test]
    fn unknown_dates_skip_games_under_two_hours() {
        assert_eq!(farm_step(&[g(1, 1, 30), g(2, 1, 200)], &RefundCheck::new(None, &[], NOW)), FarmStep::Solo(2));
    }
    #[test]
    fn no_more_than_thirty_two_build_up_nearest_the_threshold_first() {
        let games: Vec<BadgeGame> = (1..=40).map(|i| g(i, 1, i)).collect();
        let FarmStep::Accumulate(ids) = farm_step(&games, &none_recent()) else { panic!() };
        assert_eq!(ids.len(), 32);
        assert_eq!(ids[0], 40);
        assert_eq!(*ids.last().unwrap(), 9);
    }
    #[test]
    fn nothing_left_is_done() {
        assert_eq!(farm_step(&[], &none_recent()), FarmStep::Done);
    }
    fn world() -> World {
        World {
            steam_running: true,
            account_ok: true,
            real_game: None,
            user_paused: false,
            farm_on: true,
            farm: FarmStep::Done,
            idle_on: true,
            idle_games: vec![7],
            manual: vec![],
            session_ok: true,
        }
    }
    fn games(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }
    #[test]
    fn a_skipped_game_is_not_farmed() {
        let found = filter_cards(&[g(1, 1, 500), g(2, 1, 500)], &[1]);
        assert_eq!(found.iter().map(|game| game.appid).collect::<Vec<_>>(), vec![2]);
    }
    #[test]
    fn with_nothing_skipped_every_game_is_farmed() {
        assert_eq!(filter_cards(&[g(1, 1, 500), g(2, 1, 500)], &[]).len(), 2);
    }
    #[test]
    fn a_game_idled_by_hand_runs_with_nothing_started() {
        let d = decide(&World { farm_on: false, idle_on: false, manual: vec![9], ..world() });
        assert_eq!(d.games, games(&[9]));
    }
    #[test]
    fn a_game_idled_by_hand_runs_alongside_farming() {
        assert_eq!(decide(&World { farm: FarmStep::Solo(5), manual: vec![9], ..world() }).games, games(&[5, 9]));
    }
    #[test]
    fn games_idled_by_hand_stop_when_everything_stops() {
        stopped(World { real_game: Some(3), manual: vec![9], ..world() }, Waiting::RealGame(3));
        stopped(World { user_paused: true, manual: vec![9], ..world() }, Waiting::Paused);
    }
    #[test]
    fn farming_keeps_its_places_when_there_are_too_many() {
        let many: Vec<u32> = (100..140).collect();
        let d = decide(&World { farm: FarmStep::Accumulate((1..=30).collect()), manual: many, ..world() });
        assert_eq!(d.games.len(), 32);
        assert!((1..=30).all(|id| d.games.contains(&id)));
    }
    #[test]
    fn a_game_that_can_still_be_refunded_is_not_idled() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        let library = [owned(1, "Game 1", 30), owned(2, "Game 2", 30)];
        assert_eq!(idle_allowed(&[1, 2], &library, &refunds_with(&licences)), vec![2]);
    }
    #[test]
    fn a_recent_game_past_two_hours_can_be_idled() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        assert_eq!(idle_allowed(&[1], &[owned(1, "Game 1", 200)], &refunds_with(&licences)), vec![1]);
    }
    #[test]
    fn a_queued_game_nothing_is_known_about_is_not_idled() {
        assert!(idle_allowed(&[9], &[], &none_recent()).is_empty());
    }
    #[test]
    fn a_lost_session_stops_farming_and_frees_the_rest() {
        let d = decide(&World { farm: FarmStep::Solo(5), session_ok: false, ..world() });
        assert_eq!(d.games, games(&[7]));
        assert!(d.achievements_may_run);
    }
    #[test]
    fn nothing_waiting_on_cards_runs_until_they_have_been_read() {
        let d = decide(&World { farm: FarmStep::Reading, ..world() });
        assert!(d.games.is_empty());
        assert!(!d.achievements_may_run);
    }
    #[test]
    fn farming_runs_alone() {
        let d = decide(&World { farm: FarmStep::Solo(5), ..world() });
        assert_eq!(d.games, games(&[5]));
        assert!(!d.achievements_may_run);
    }
    #[test]
    fn building_up_runs_all_of_them() {
        assert_eq!(decide(&World { farm: FarmStep::Accumulate(vec![5, 6]), ..world() }).games, games(&[5, 6]));
    }
    #[test]
    fn the_idler_runs_once_farming_is_done() {
        let d = decide(&World { idle_games: vec![7, 8], ..world() });
        assert_eq!(d.games, games(&[7, 8]));
        assert!(d.achievements_may_run && d.inventory_may_run);
    }
    #[test]
    fn farming_off_frees_the_idler() {
        assert_eq!(decide(&World { farm_on: false, farm: FarmStep::Solo(5), ..world() }).games, games(&[7]));
    }
    #[test]
    fn the_idler_off_runs_nothing_extra() {
        assert!(decide(&World { idle_on: false, ..world() }).games.is_empty());
    }
    #[test]
    fn the_inventory_may_run_while_farming() {
        assert!(decide(&World { farm: FarmStep::Solo(5), ..world() }).inventory_may_run);
    }
    fn stopped(world: World, waiting: Waiting) {
        let d = decide(&world);
        assert!(d.games.is_empty());
        assert_eq!(d.waiting, Some(waiting));
        assert!(!d.achievements_may_run && !d.inventory_may_run);
    }
    #[test]
    fn a_real_game_stops_everything() {
        stopped(World { real_game: Some(9), ..world() }, Waiting::RealGame(9));
    }
    #[test]
    fn no_steam_stops_everything() {
        stopped(World { steam_running: false, ..world() }, Waiting::NoSteam);
    }
    #[test]
    fn the_wrong_account_stops_everything() {
        stopped(World { account_ok: false, ..world() }, Waiting::OtherAccount);
    }
    #[test]
    fn pause_stops_everything() {
        stopped(World { user_paused: true, ..world() }, Waiting::Paused);
    }
    #[test]
    fn no_session_holds_back_only_what_needs_one() {
        let d = decide(&World { session_ok: false, ..world() });
        assert_eq!(d.games, games(&[7]));
        assert!(!d.inventory_may_run);
        assert_eq!(d.waiting, None);
    }
}
