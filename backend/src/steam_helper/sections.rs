//! What the page's sections show, built from what the store answers.
use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use super::model::{Filters, FreeOffer, Game, OfferKind};
use super::parse::{Item, UNEXPECTED};
use super::store::{ListingKind, StoreApi, details, listing, modes, scan_free_weekends};
const ROW_SIZE: usize = 8;
const ROW_MIN_REVIEWS: u32 = 1000;
const ROW_PAGE: u32 = 100;
const MORE_PAGE: u32 = 50;
const FREE_LISTING: u32 = 100;
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RowKind {
    Deals,
    FreeToPlay,
}
impl RowKind {
    fn listing(self) -> ListingKind {
        match self {
            RowKind::Deals => ListingKind::Deals,
            RowKind::FreeToPlay => ListingKind::FreeToPlay,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MorePage {
    pub games: Vec<Game>,
    pub next_cursor: Option<u32>,
}
fn offer_id(appid: u32, ends_at: Option<u64>) -> String {
    format!("{appid}:{}", ends_at.unwrap_or(0))
}
/// One offer per trial, soonest to end first, an offer with no end last.
pub(crate) fn offers_from(weekends: Vec<Item>, to_keep: Vec<Item>) -> Vec<FreeOffer> {
    let mut seen = HashSet::new();
    let mut offers = Vec::new();
    for item in weekends {
        let Some(weekend) = item.free_weekend.filter(|_| !item.adult) else {
            continue;
        };
        let trial = weekend.appid.unwrap_or(item.game.appid);
        if !seen.insert(trial) {
            continue;
        }
        let label = weekend.text.as_deref().and_then(|t| t.rsplit_once(" - ")).map(|(_, label)| label.trim());
        offers.push(FreeOffer {
            game: item.game,
            kind: OfferKind::FreeWeekend,
            label: label.unwrap_or("Free weekend").to_string(),
            starts_at: weekend.start_time,
            ends_at: Some(weekend.end_time),
            offer_id: offer_id(trial, Some(weekend.end_time)),
        });
    }
    for item in to_keep {
        // The search index can lag behind the price: only a game Steam still has at 100% off counts.
        if item.adult || item.game.discount_percent != 100 || !seen.insert(item.game.appid) {
            continue;
        }
        let ends_at = item.game.deal_ends_at;
        offers.push(FreeOffer {
            offer_id: offer_id(item.game.appid, ends_at),
            game: item.game,
            kind: OfferKind::FreeToKeep,
            label: "Free to keep".to_string(),
            starts_at: None,
            ends_at,
        });
    }
    offers.sort_by_key(|offer| offer.ends_at.unwrap_or(u64::MAX));
    offers
}
/// The union of several ranked lists, best reviewed first.
pub(crate) fn merge_ranked(lists: Vec<Vec<Game>>) -> Vec<Game> {
    let mut seen = HashSet::new();
    let mut games: Vec<Game> = lists.into_iter().flatten().filter(|game| seen.insert(game.appid)).collect();
    games.sort_by_key(|game| std::cmp::Reverse((game.review_percent, game.review_count)));
    games
}
pub(crate) async fn free_right_now<S: StoreApi>(api: &S) -> Result<Vec<FreeOffer>, String> {
    let weekends = scan_free_weekends(api).await?;
    let page = listing(api, ListingKind::FreeToKeep, None, false, 0, FREE_LISTING).await?;
    let to_keep = details(api, &page.ids).await?;
    Ok(offers_from(weekends, to_keep))
}
/// One listing per chosen mode, without adult games, and the most rows any of them returned.
async fn ranked_lists<S: StoreApi>(
    api: &S,
    kind: RowKind,
    filters: Filters,
    start: u32,
    count: u32,
) -> Result<(Vec<Vec<Game>>, u32), String> {
    let mut lists = Vec::new();
    let mut rows = 0;
    for mode in modes(filters) {
        let page = listing(api, kind.listing(), mode, filters.story_only, start, count).await?;
        rows = rows.max(page.rows);
        let games = details(api, &page.ids).await?;
        lists.push(games.into_iter().filter(|item| !item.adult).map(|item| item.game).collect());
    }
    Ok((lists, rows))
}
/// The row on the main page.
pub(crate) async fn row<S: StoreApi>(api: &S, kind: RowKind, filters: Filters) -> Result<Vec<Game>, String> {
    let (lists, _) = ranked_lists(api, kind, filters, 0, ROW_PAGE).await?;
    let mut games = merge_ranked(lists);
    // These listings are sorted by reviews and leave out games with none.
    if !games.is_empty() && games.iter().all(|game| game.review_count == 0) {
        return Err(UNEXPECTED.to_string());
    }
    games.retain(|game| game.review_count >= ROW_MIN_REVIEWS);
    games.truncate(ROW_SIZE);
    Ok(games)
}
/// The next part of a See all list. `cursor` moves by the rows Steam returned.
pub(crate) async fn more<S: StoreApi>(
    api: &S,
    kind: RowKind,
    filters: Filters,
    cursor: u32,
) -> Result<MorePage, String> {
    let (lists, rows) = ranked_lists(api, kind, filters, cursor, MORE_PAGE).await?;
    Ok(MorePage { games: merge_ranked(lists), next_cursor: (rows >= MORE_PAGE).then_some(cursor + rows) })
}
#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use super::*;
    use crate::steam_helper::model::{Game, OfferKind};
    use crate::steam_helper::parse::FreeWeekend;
    use crate::steam_helper::store::fake::{FakeStore, answer_with, input_of};
    fn g(appid: u32, percent: u8, count: u32) -> Game {
        Game {
            appid,
            name: format!("Game {appid}"),
            cover_url: None,
            store_url: format!("https://store.steampowered.com/app/{appid}"),
            review_percent: percent,
            review_count: count,
            is_free: false,
            price: None,
            original_price: None,
            discount_percent: 0,
            deal_ends_at: None,
            online_coop: false,
            single_player: false,
            story: false,
        }
    }
    fn weekend(appid: u32, end_time: u64, text: Option<&str>, trial: Option<u32>) -> Item {
        Item {
            game: g(appid, 80, 5000),
            adult: false,
            item_type: Some(0),
            portrait_url: None,
            features: Vec::new(),
            free_weekend: Some(FreeWeekend { start_time: None, end_time, text: text.map(str::to_string), appid: trial }),
        }
    }
    const TRIAL: &str = "Call of Duty®: Modern Warfare® 4 - DMZ Free Trial";
    fn mw4() -> Item {
        weekend(4435490, 1794420000, Some(TRIAL), Some(4269880))
    }
    fn cod_same_trial() -> Item {
        weekend(1938090, 1794420000, Some(TRIAL), Some(4269880))
    }
    fn payday() -> Item {
        weekend(1272080, 1794247200, None, None)
    }
    fn to_keep(appid: u32, ends: Option<u64>) -> Item {
        let mut game = g(appid, 0, 0);
        game.deal_ends_at = ends;
        game.discount_percent = 100;
        Item { game, adult: false, free_weekend: None, item_type: Some(0), portrait_url: None, features: Vec::new() }
    }
    #[test]
    fn offers_group_by_trial_and_end_soonest_first() {
        let out = offers_from(vec![mw4(), cod_same_trial(), payday()], vec![to_keep(7, None)]);
        assert_eq!(out.iter().map(|o| o.game.appid).collect::<Vec<_>>(), vec![1272080, 4435490, 7]);
        assert_eq!(out[0].label, "Free weekend");
        assert_eq!(out[0].offer_id, "1272080:1794247200");
        assert_eq!(out[1].label, "DMZ Free Trial");
        assert_eq!(out[1].offer_id, "4269880:1794420000");
        assert_eq!(out[2].label, "Free to keep");
        assert_eq!(out[2].offer_id, "7:0");
        assert_eq!((out[1].kind, out[2].kind), (OfferKind::FreeWeekend, OfferKind::FreeToKeep));
    }
    #[test]
    fn a_giveaway_keeps_its_end_time() {
        let out = offers_from(vec![], vec![to_keep(7, Some(1794300000))]);
        assert_eq!(out[0].ends_at, Some(1794300000));
        assert_eq!(out[0].offer_id, "7:1794300000");
    }
    #[test]
    fn a_game_no_longer_fully_discounted_is_not_free_to_keep() {
        let mut was_free = to_keep(7, None);
        was_free.game.discount_percent = 50;
        assert!(offers_from(vec![], vec![was_free]).is_empty());
    }
    #[test]
    fn a_free_weekend_carries_its_start() {
        let mut soon = payday();
        soon.free_weekend.as_mut().unwrap().start_time = Some(5000);
        assert_eq!(offers_from(vec![soon], vec![])[0].starts_at, Some(5000));
    }
    #[test]
    fn adult_offers_are_dropped() {
        let mut adult_weekend = payday();
        adult_weekend.adult = true;
        let mut adult_gift = to_keep(7, None);
        adult_gift.adult = true;
        assert!(offers_from(vec![adult_weekend], vec![adult_gift]).is_empty());
    }
    #[test]
    fn merging_is_a_union_ordered_by_percent_then_reviews() {
        let merged = merge_ranked(vec![vec![g(1, 90, 100), g(2, 95, 10)], vec![g(1, 90, 100), g(3, 95, 50)]]);
        assert_eq!(merged.iter().map(|x| x.appid).collect::<Vec<_>>(), vec![3, 2, 1]);
    }
    fn html_of(ids: impl Iterator<Item = u32>) -> String {
        let rows: String = ids.map(|id| format!("<a data-ds-appid=\"{id}\"></a>")).collect();
        json!({ "total_count": 999, "results_html": rows }).to_string()
    }
    fn game_json(appid: u32, reviews: u32, adult: bool) -> Value {
        let mut game = json!({
            "appid": appid, "name": format!("Game {appid}"),
            "reviews": { "summary_filtered": { "percent_positive": 90, "review_count": reviews } },
        });
        if adult {
            game["tags"] = json!([{ "tagid": 9130 }]);
        }
        game
    }
    /// A listing of `rows` games numbered from 1; 1 and 2 have few reviews and 3 is adult.
    fn store_with(rows: u32) -> FakeStore {
        FakeStore::responding(move |url| {
            if url.contains("search/results") {
                return Ok(html_of(1..=rows));
            }
            let games = input_of(url)["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    let appid = id["appid"].as_u64().unwrap() as u32;
                    game_json(appid, if appid <= 2 { 40 } else { 5000 }, appid == 3)
                })
                .collect();
            Ok(answer_with(games))
        })
    }
    const COOP: Filters = Filters { online_coop: true, single_player: false, story_only: false };
    #[tokio::test]
    async fn a_row_needs_a_thousand_reviews_and_stops_at_eight() {
        let api = store_with(20);
        let row = row(&api, RowKind::Deals, COOP).await.unwrap();
        assert_eq!(row.iter().map(|x| x.appid).collect::<Vec<_>>(), vec![4, 5, 6, 7, 8, 9, 10, 11]);
        let listing = &api.calls()[0];
        assert!(listing.contains("specials=1") && listing.contains("category2=38") && listing.contains("count=100"), "{listing}");
    }
    #[tokio::test]
    async fn the_free_to_play_row_asks_for_free_games() {
        let api = store_with(4);
        row(&api, RowKind::FreeToPlay, COOP).await.unwrap();
        assert!(api.calls()[0].contains("maxprice=free"));
    }
    #[tokio::test]
    async fn both_modes_ask_steam_twice_and_story_narrows_each() {
        let api = store_with(4);
        let filters = Filters { online_coop: true, single_player: true, story_only: true };
        let games = row(&api, RowKind::Deals, filters).await.unwrap();
        let listings: Vec<String> = api.calls().into_iter().filter(|u| u.contains("search/results")).collect();
        assert_eq!(listings.len(), 2);
        assert!(listings[0].contains("category2=38") && listings[0].contains("tags=1742"), "{}", listings[0]);
        assert!(listings[1].contains("category2=2&") && listings[1].contains("tags=1742"), "{}", listings[1]);
        assert_eq!(games.iter().map(|x| x.appid).collect::<Vec<_>>(), vec![4], "the two lists are one union");
    }
    #[tokio::test]
    async fn see_all_has_no_review_floor_and_advances_by_rows() {
        let api = store_with(50);
        let page = more(&api, RowKind::Deals, COOP, 100).await.unwrap();
        assert!(page.games.iter().any(|x| x.appid == 1), "few reviews are fine here");
        assert!(page.games.iter().all(|x| x.appid != 3), "adult games are never shown");
        assert_eq!(page.next_cursor, Some(150));
        assert!(api.calls()[0].contains("start=100&count=50"));
    }
    // Sorting by reviews leaves out games with none, so this can only mean Steam changed its answer.
    #[tokio::test]
    async fn a_row_where_no_game_has_reviews_is_an_error() {
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                return Ok(html_of(4..=6));
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| json!({ "appid": id["appid"], "name": "Game" })).collect();
            Ok(answer_with(games))
        });
        assert!(row(&api, RowKind::Deals, COOP).await.is_err());
    }
    #[tokio::test]
    async fn see_all_ends_on_a_short_page() {
        let api = store_with(30);
        assert_eq!(more(&api, RowKind::Deals, COOP, 0).await.unwrap().next_cursor, None);
    }
    #[tokio::test]
    async fn free_right_now_joins_weekends_and_giveaways() {
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                assert!(url.contains("maxprice=free&specials=1"), "{url}");
                return Ok(html_of(77..=77));
            }
            if url.contains("Query") {
                return Ok(answer_with(vec![json!({
                    "appid": 5, "name": "Weekend", "free_weekend": { "end_time": 2000 }
                })]));
            }
            let mut gift = game_json(77, 0, false);
            gift["best_purchase_option"] = json!({ "discount_pct": 100, "is_free_to_keep": true });
            Ok(answer_with(vec![gift]))
        });
        let offers = free_right_now(&api).await.unwrap();
        assert_eq!(offers.iter().map(|o| (o.game.appid, o.kind)).collect::<Vec<_>>(), vec![(5, OfferKind::FreeWeekend), (77, OfferKind::FreeToKeep)]);
    }
    #[tokio::test]
    async fn a_giveaway_listing_that_fails_is_an_error() {
        let weekend = json!({ "appid": 5, "name": "Weekend", "free_weekend": { "end_time": 2000 } });
        let api = FakeStore::new(vec![("Query", Ok(answer_with(vec![weekend]))), ("search/results", Err("down".to_string()))]);
        assert_eq!(free_right_now(&api).await, Err("down".to_string()));
    }
    /// Page open and then quick filter changes, against the real store, as the app sends them.
    #[tokio::test]
    #[ignore]
    async fn live_quick_filter_changes_are_not_refused() {
        use crate::steam_helper::picks::weekly_picks;
        let api = crate::steam_helper::store::HttpStore::new().unwrap();
        let file = std::env::temp_dir().join(format!("myle-live-{}", uuid::Uuid::new_v4())).join("picks.json");
        let on = |online_coop, single_player, story_only| Filters { online_coop, single_player, story_only };
        let (api, file) = (&api, &file);
        let mut asked = Vec::new();
        for filters in [on(true, false, false), on(true, false, true), on(false, true, false), on(false, true, true), on(false, false, false), on(true, true, false)] {
            asked.push(async move {
                let (deals, free, picks) = tokio::join!(row(api, RowKind::Deals, filters), row(api, RowKind::FreeToPlay, filters), weekly_picks(api, file, filters, 20759));
                (filters, deals.map(|g| g.len()), free.map(|g| g.len()), picks.map(|g| g.len()))
            });
        }
        for (filters, deals, free, picks) in futures_util::future::join_all(asked).await {
            println!("{}: deals {deals:?}, free {free:?}, picks {picks:?}", filters.key());
            assert!(deals.is_ok() && free.is_ok() && picks.is_ok(), "{}", filters.key());
        }
    }
    /// By hand when Steam changes something: `cargo test live_steam_still_answers -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_steam_still_answers() {
        let api = crate::steam_helper::store::HttpStore::new().unwrap();
        let deals = row(&api, RowKind::Deals, COOP).await.unwrap();
        assert!(!deals.is_empty());
        assert!(deals.iter().all(|g| g.online_coop && g.cover_url.is_some()));
        free_right_now(&api).await.unwrap();
    }
}
