//! Which games should be running right now. Nothing here touches Steam, so every rule can be tested.
use std::collections::BTreeSet;
use super::badges::{BadgeGame, bought_recently, is_refundable};
/// Steam starts dropping cards after this much playtime.
const FARM_THRESHOLD_MINUTES: u32 = 120;
const MAX_GAMES: usize = 32;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FarmStep {
    Accumulate(Vec<u32>),
    Solo(u32),
    Done,
}
/// What card farming should do next. `licences` is `None` when purchase dates could not be read.
pub(crate) fn farm_step(games: &[BadgeGame], licences: Option<&[(String, u64)]>, now: u64) -> FarmStep {
    let allowed: Vec<&BadgeGame> = games
        .iter()
        .filter(|game| !is_refundable(licences.map(|list| bought_recently(&game.name, list, now)), game.minutes_played))
        .collect();
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
    let farming = if world.farm_on { &world.farm } else { &FarmStep::Done };
    let games: BTreeSet<u32> = match farming {
        FarmStep::Accumulate(list) => list.iter().copied().collect(),
        FarmStep::Solo(game) => BTreeSet::from([*game]),
        FarmStep::Done if world.idle_on => world.idle_games.iter().copied().collect(),
        FarmStep::Done => BTreeSet::new(),
    };
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
    fn none_recent() -> Option<&'static [(String, u64)]> {
        Some(&[])
    }
    #[test]
    fn games_under_two_hours_build_up_together() {
        let step = farm_step(&[g(1, 1, 30), g(2, 1, 60), g(3, 1, 500)], none_recent(), NOW);
        assert_eq!(step, FarmStep::Accumulate(vec![2, 1]));
    }
    #[test]
    fn then_one_at_a_time_fewest_drops_first() {
        assert_eq!(farm_step(&[g(2, 3, 200), g(3, 1, 500)], none_recent(), NOW), FarmStep::Solo(3));
    }
    #[test]
    fn exactly_two_hours_is_ready() {
        assert_eq!(farm_step(&[g(1, 1, 120)], none_recent(), NOW), FarmStep::Solo(1));
    }
    #[test]
    fn refundable_games_are_skipped() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        assert_eq!(farm_step(&[g(1, 1, 30)], Some(&licences), NOW), FarmStep::Done);
    }
    #[test]
    fn a_recent_game_past_two_hours_is_not_skipped() {
        let licences = vec![("Game 1".to_string(), NOW - 86_400)];
        assert_eq!(farm_step(&[g(1, 1, 130)], Some(&licences), NOW), FarmStep::Solo(1));
    }
    #[test]
    fn unknown_dates_skip_games_under_two_hours() {
        assert_eq!(farm_step(&[g(1, 1, 30), g(2, 1, 200)], None, NOW), FarmStep::Solo(2));
    }
    #[test]
    fn no_more_than_thirty_two_build_up_nearest_the_threshold_first() {
        let games: Vec<BadgeGame> = (1..=40).map(|i| g(i, 1, i)).collect();
        let FarmStep::Accumulate(ids) = farm_step(&games, none_recent(), NOW) else { panic!() };
        assert_eq!(ids.len(), 32);
        assert_eq!(ids[0], 40);
        assert_eq!(*ids.last().unwrap(), 9);
    }
    #[test]
    fn nothing_left_is_done() {
        assert_eq!(farm_step(&[], none_recent(), NOW), FarmStep::Done);
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
            session_ok: true,
        }
    }
    fn games(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
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
