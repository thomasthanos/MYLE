//! This week's picks: twelve well-reviewed games, chosen once a week for each filter combination
//! and saved, so the list holds still all week and really changes the next.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use serde::{Deserialize, Serialize};
use super::model::{Filters, Game};
use super::parse::UNEXPECTED;
use super::store::{ListingKind, Mode, StoreApi, details, listing, modes};
const PICKS: usize = 12;
const STORY_SHARE: usize = 8;
const MIN_PERCENT: u8 = 85;
const MIN_REVIEWS: u32 = 5000;
const TIER_PAGE: u32 = 100;
const TIER_PAGES: u32 = 1;
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PickWeek {
    week: i64,
    appids: Vec<u32>,
}
type PicksFile = HashMap<String, PickWeek>;
/// The day number of the Monday that starts `local_day`'s week. Day 0 was a Thursday.
pub(crate) fn week_key(local_day: i64) -> i64 {
    local_day - (local_day + 3).rem_euclid(7)
}
/// FNV-1a over the week and the game: an order that is fixed for a week and differs the next.
fn week_rank(week: i64, appid: u32) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in week.to_le_bytes().into_iter().chain(appid.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
fn in_week_order(week: i64, ids: &[u32]) -> Vec<u32> {
    let mut ordered = ids.to_vec();
    ordered.sort_by_key(|id| week_rank(week, *id));
    ordered
}
fn assemble(story: &[u32], other: &[u32], story_only: bool) -> Vec<u32> {
    let story_first = if story_only { PICKS } else { STORY_SHARE };
    let mut chosen: Vec<u32> = story.iter().take(story_first).copied().collect();
    let room = PICKS - chosen.len();
    chosen.extend(other.iter().take(room));
    let room = PICKS - chosen.len();
    chosen.extend(story.iter().skip(story_first).take(room));
    chosen
}
/// Twelve games: story games first, the others after, a short tier filled by the other. Last
/// week's games come back only if there would otherwise be too few.
pub(crate) fn choose(week: i64, story: &[u32], other: &[u32], exclude: &[u32], story_only: bool) -> Vec<u32> {
    let story = in_week_order(week, story);
    let other = if story_only { Vec::new() } else { in_week_order(week, other) };
    let is_new = |id: &u32| !exclude.contains(id);
    let new_only = |list: &[u32]| list.iter().copied().filter(is_new).collect::<Vec<u32>>();
    let chosen = assemble(&new_only(&story), &new_only(&other), story_only);
    if chosen.len() >= PICKS {
        return chosen;
    }
    let new_then_old = |list: &[u32]| {
        let (new, old): (Vec<u32>, Vec<u32>) = list.iter().copied().partition(is_new);
        new.into_iter().chain(old).collect::<Vec<u32>>()
    };
    assemble(&new_then_old(&story), &new_then_old(&other), story_only)
}
/// The games of one tier that clear the floor.
async fn qualifying<S: StoreApi>(api: &S, chosen: &[Option<Mode>], story: bool) -> Result<Vec<u32>, String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for mode in chosen {
        for page in 0..TIER_PAGES {
            let found = listing(api, ListingKind::Ranked, *mode, story, page * TIER_PAGE, TIER_PAGE).await?;
            ids.extend(found.ids.into_iter().filter(|id| seen.insert(*id)));
            if found.rows < TIER_PAGE {
                break;
            }
        }
    }
    Ok(details(api, &ids)
        .await?
        .into_iter()
        .filter(|item| !item.adult && item.game.review_percent >= MIN_PERCENT && item.game.review_count >= MIN_REVIEWS)
        .map(|item| item.game.appid)
        .collect())
}
fn read(file: &Path) -> PicksFile {
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}
fn save(file: &Path, picks: &PicksFile) {
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    // Not saved only means choosing again; the order is the same within a week.
    if let Ok(bytes) = serde_json::to_vec(picks) {
        let _ = crate::game_saves::atomic::write(file, &bytes);
    }
}
async fn games_for<S: StoreApi>(api: &S, appids: &[u32]) -> Result<Vec<Game>, String> {
    Ok(details(api, appids).await?.into_iter().filter(|item| !item.adult).map(|item| item.game).collect())
}
/// This week's picks for these filters, with today's prices.
pub(crate) async fn weekly_picks<S: StoreApi>(
    api: &S,
    file: &Path,
    filters: Filters,
    week: i64,
) -> Result<Vec<Game>, String> {
    let mut saved = read(file);
    let key = filters.key();
    let last = saved.get(&key).cloned().unwrap_or_default();
    if last.week == week {
        return games_for(api, &last.appids).await;
    }
    let chosen_modes = modes(filters);
    let story = qualifying(api, &chosen_modes, true).await?;
    let other = if filters.story_only {
        Vec::new()
    } else {
        let others = qualifying(api, &chosen_modes, false).await?;
        others.into_iter().filter(|id| !story.contains(id)).collect()
    };
    let appids = choose(week, &story, &other, &last.appids, filters.story_only);
    // Saving an empty list would leave the section blank until next Monday.
    if appids.is_empty() {
        return Err(UNEXPECTED.to_string());
    }
    let games = games_for(api, &appids).await?;
    saved.insert(key, PickWeek { week, appids });
    save(file, &saved);
    Ok(games)
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use serde_json::{Value, json};
    use super::*;
    use crate::steam_helper::store::fake::{FakeStore, answer_with, input_of};
    fn ids(range: std::ops::Range<u32>) -> Vec<u32> {
        range.collect()
    }
    const WEEK: i64 = 20759;
    const NEXT_WEEK: i64 = 20766;
    #[test]
    fn a_week_starts_on_monday() {
        assert_eq!(week_key(20765), 20759); // Sunday 2026-11-08
        assert_eq!(week_key(20766), 20766); // Monday 2026-11-09
        assert_eq!(week_key(20759), 20759);
    }
    #[test]
    fn the_same_week_gives_the_same_picks_and_another_does_not() {
        let this_week = choose(WEEK, &ids(1..60), &ids(100..300), &[], false);
        assert_eq!(this_week, choose(WEEK, &ids(1..60), &ids(100..300), &[], false));
        assert_ne!(this_week, choose(NEXT_WEEK, &ids(1..60), &ids(100..300), &[], false));
    }
    #[test]
    fn eight_story_then_four_others() {
        let chosen = choose(WEEK, &ids(1..60), &ids(100..300), &[], false);
        assert_eq!(chosen.len(), 12);
        assert_eq!(chosen.iter().filter(|i| **i < 60).count(), 8);
        assert!(chosen[..8].iter().all(|i| *i < 60), "story games come first");
    }
    #[test]
    fn story_only_is_all_story() {
        let chosen = choose(WEEK, &ids(1..60), &ids(100..300), &[], true);
        assert_eq!(chosen.len(), 12);
        assert!(chosen.iter().all(|i| *i < 60));
    }
    #[test]
    fn a_short_story_tier_is_filled_by_the_others() {
        assert_eq!(choose(WEEK, &ids(1..4), &ids(100..300), &[], false).len(), 12);
    }
    #[test]
    fn a_short_other_tier_is_filled_by_story() {
        let chosen = choose(WEEK, &ids(1..60), &ids(100..102), &[], false);
        assert_eq!(chosen.len(), 12);
        assert_eq!(chosen.iter().filter(|i| **i >= 100).count(), 2);
    }
    #[test]
    fn last_week_is_left_out() {
        let last = ids(1..30);
        assert!(choose(NEXT_WEEK, &ids(1..60), &ids(100..300), &last, false).iter().all(|i| !last.contains(i)));
    }
    #[test]
    fn last_week_comes_back_only_when_there_is_nothing_else() {
        assert_eq!(choose(NEXT_WEEK, &ids(1..6), &[], &ids(1..6), true).len(), 5);
    }
    fn temp_file() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("myle-picks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("picks.json")
    }
    fn html_of(ids: &[u32]) -> String {
        let rows: String = ids.iter().map(|id| format!("<a data-ds-appid=\"{id}\"></a>")).collect();
        json!({ "total_count": 999, "results_html": rows }).to_string()
    }
    fn game_json(appid: u32, price: u32) -> Value {
        json!({
            "appid": appid, "name": format!("Game {appid}"),
            "reviews": { "summary_filtered": { "percent_positive": 90, "review_count": 6000 } },
            "best_purchase_option": { "formatted_final_price": format!("{price}€") },
        })
    }
    /// Story games 1..=30, other games 100..=140, each answering at `price`.
    fn store(price: Arc<AtomicU32>) -> FakeStore {
        FakeStore::responding(move |url| {
            if url.contains("search/results") {
                let list: Vec<u32> = if url.contains("tags=1742") { (1..=30).collect() } else { (1..=30).chain(100..=140).collect() };
                return Ok(html_of(&list));
            }
            let at = price.load(Ordering::SeqCst);
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| game_json(id["appid"].as_u64().unwrap() as u32, at)).collect();
            Ok(answer_with(games))
        })
    }
    const COOP: Filters = Filters { online_coop: true, single_player: false, story_only: false };
    fn listings(api: &FakeStore) -> usize {
        api.calls().iter().filter(|u| u.contains("search/results")).count()
    }
    #[tokio::test]
    async fn inside_a_week_the_list_is_kept_and_prices_refresh() {
        let price = Arc::new(AtomicU32::new(10));
        let api = store(price.clone());
        let file = temp_file();
        let first = weekly_picks(&api, &file, COOP, WEEK).await.unwrap();
        let asked = listings(&api);
        price.store(20, Ordering::SeqCst);
        let second = weekly_picks(&api, &file, COOP, WEEK).await.unwrap();
        assert_eq!(first.iter().map(|g| g.appid).collect::<Vec<_>>(), second.iter().map(|g| g.appid).collect::<Vec<_>>());
        assert_eq!(first[0].price.as_deref(), Some("10€"));
        assert_eq!(second[0].price.as_deref(), Some("20€"));
        assert_eq!(listings(&api), asked, "the choice is not made again");
    }
    #[tokio::test]
    async fn the_next_week_leaves_out_what_was_shown() {
        let api = store(Arc::new(AtomicU32::new(10)));
        let file = temp_file();
        let first = weekly_picks(&api, &file, COOP, WEEK).await.unwrap();
        let second = weekly_picks(&api, &file, COOP, NEXT_WEEK).await.unwrap();
        assert_eq!(second.len(), 12);
        assert!(second.iter().all(|g| first.iter().all(|f| f.appid != g.appid)));
    }
    #[tokio::test]
    async fn each_filter_combination_keeps_its_own_list() {
        let api = store(Arc::new(AtomicU32::new(10)));
        let file = temp_file();
        weekly_picks(&api, &file, COOP, WEEK).await.unwrap();
        let asked = listings(&api);
        weekly_picks(&api, &file, Filters { story_only: true, ..COOP }, WEEK).await.unwrap();
        assert!(listings(&api) > asked);
    }
    #[tokio::test]
    async fn a_corrupt_file_starts_fresh() {
        let api = store(Arc::new(AtomicU32::new(10)));
        let file = temp_file();
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(weekly_picks(&api, &file, COOP, WEEK).await.unwrap().len(), 12);
    }
    #[tokio::test]
    async fn games_below_the_floor_or_for_adults_are_not_picked() {
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                return Ok(html_of(&[1, 2, 3, 4]));
            }
            Ok(answer_with(vec![
                json!({ "appid": 1, "name": "Low", "reviews": { "summary_filtered": { "percent_positive": 84, "review_count": 9000 } } }),
                json!({ "appid": 2, "name": "Few", "reviews": { "summary_filtered": { "percent_positive": 99, "review_count": 4999 } } }),
                json!({ "appid": 3, "name": "Adult", "reviews": { "summary_filtered": { "percent_positive": 99, "review_count": 9000 } }, "content_descriptorids": [3] }),
                json!({ "appid": 4, "name": "Good", "reviews": { "summary_filtered": { "percent_positive": 85, "review_count": 5000 } } }),
            ]))
        });
        let picks = weekly_picks(&api, &temp_file(), COOP, WEEK).await.unwrap();
        assert_eq!(picks.iter().map(|g| g.appid).collect::<Vec<_>>(), vec![4]);
    }
    #[tokio::test]
    async fn a_tier_reads_one_page_of_results() {
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                return Ok(html_of(&(1..=100).collect::<Vec<u32>>()));
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| game_json(id["appid"].as_u64().unwrap() as u32, 10)).collect();
            Ok(answer_with(games))
        });
        weekly_picks(&api, &temp_file(), Filters { story_only: true, ..COOP }, WEEK).await.unwrap();
        let starts: Vec<bool> = [0, 100, 200].iter().map(|s| api.calls().iter().any(|u| u.contains(&format!("start={s}&")))).collect();
        assert_eq!(starts, vec![true, false, false]);
    }
    #[tokio::test]
    async fn an_empty_choice_is_an_error_and_is_not_saved() {
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                return Ok(html_of(&[1]));
            }
            Ok(answer_with(vec![json!({ "appid": 1, "name": "Low" })]))
        });
        let file = temp_file();
        assert!(weekly_picks(&api, &file, COOP, WEEK).await.is_err());
        assert!(!file.exists());
    }
    #[tokio::test]
    async fn a_failure_saves_nothing() {
        let api = FakeStore::new(vec![("search/results", Err("down".to_string()))]);
        let file = temp_file();
        assert_eq!(weekly_picks(&api, &file, COOP, WEEK).await, Err("down".to_string()));
        assert!(!file.exists());
    }
}
