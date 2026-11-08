//! Asks Steam's public store. No sign-in and no key.
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::Instant;
use super::model::Filters;
use super::parse::{self, Item, SearchPage};
const SEARCH: &str = "https://store.steampowered.com/search/results/?infinite=1&category1=998&cc=gr&l=english";
const GET_ITEMS: &str = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";
const QUERY: &str = "https://api.steampowered.com/IStoreQueryService/Query/v1/";
const GAP: Duration = Duration::from_millis(150);
const PER_REQUEST: usize = 50;
const SCAN_PAGE: u32 = 100;
const SCAN_PAGES: u32 = 10;
/// Steam's top-sellers order. Observed, not documented.
const SORT_TOP_SELLERS: u32 = 12;
const CATEGORY_ONLINE_COOP: u32 = 38;
const CATEGORY_SINGLE_PLAYER: u32 = 2;
const TAG_STORY_RICH: u32 = 1742;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ListingKind {
    Deals,
    FreeToPlay,
    FreeToKeep,
    Ranked,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    OnlineCoop,
    SinglePlayer,
}
/// The listings a filter combination asks for: one per chosen mode, or one for all games.
pub(crate) fn modes(filters: Filters) -> Vec<Option<Mode>> {
    let mut chosen = Vec::new();
    if filters.online_coop {
        chosen.push(Some(Mode::OnlineCoop));
    }
    if filters.single_player {
        chosen.push(Some(Mode::SinglePlayer));
    }
    if chosen.is_empty() {
        chosen.push(None);
    }
    chosen
}
pub(crate) fn search_url(kind: ListingKind, mode: Option<Mode>, story: bool, start: u32, count: u32) -> String {
    let mut url = SEARCH.to_string();
    // Sorting by reviews drops games with none, and a brand-new giveaway may have none.
    if kind != ListingKind::FreeToKeep {
        url.push_str("&sort_by=Reviews_DESC");
    }
    url.push_str(match kind {
        ListingKind::Deals => "&specials=1",
        ListingKind::FreeToPlay => "&maxprice=free",
        ListingKind::FreeToKeep => "&maxprice=free&specials=1",
        ListingKind::Ranked => "",
    });
    if let Some(mode) = mode {
        let category = match mode {
            Mode::OnlineCoop => CATEGORY_ONLINE_COOP,
            Mode::SinglePlayer => CATEGORY_SINGLE_PLAYER,
        };
        url.push_str(&format!("&category2={category}"));
    }
    if story {
        url.push_str(&format!("&tags={TAG_STORY_RICH}"));
    }
    url.push_str(&format!("&start={start}&count={count}"));
    url
}
fn with_input(base: &str, input: &Value) -> Result<String, String> {
    reqwest::Url::parse_with_params(base, &[("input_json", input.to_string())])
        .map(String::from)
        .map_err(|e| e.to_string())
}
fn context() -> Value {
    json!({ "language": "english", "country_code": "GR" })
}
fn data_request() -> Value {
    json!({
        "include_basic_info": true,
        "include_reviews": true,
        "include_assets": true,
        "include_all_purchase_options": true,
        "include_tag_count": 20
    })
}
pub(crate) trait StoreApi: Send + Sync {
    fn get(&self, url: &str) -> impl Future<Output = Result<String, String>> + Send;
}
/// The real store, one request about every 150 ms however many sections ask at once.
pub(crate) struct HttpStore {
    client: reqwest::Client,
    next: Mutex<Instant>,
}
impl HttpStore {
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self { client: crate::download::http_client("MYLE")?, next: Mutex::new(Instant::now()) })
    }
}
impl StoreApi for HttpStore {
    async fn get(&self, url: &str) -> Result<String, String> {
        let slot = {
            let mut next = self.next.lock().await;
            let slot = (*next).max(Instant::now());
            *next = slot + GAP;
            slot
        };
        tokio::time::sleep_until(slot).await;
        let response = self.client.get(url).send().await.map_err(|_| "Steam could not be reached.".to_string())?;
        if !response.status().is_success() {
            return Err(format!("Steam answered {}.", response.status().as_u16()));
        }
        response.text().await.map_err(|_| "Steam could not be reached.".to_string())
    }
}
pub(crate) async fn listing<S: StoreApi>(
    api: &S,
    kind: ListingKind,
    mode: Option<Mode>,
    story: bool,
    start: u32,
    count: u32,
) -> Result<SearchPage, String> {
    let page = parse::search_page(&api.get(&search_url(kind, mode, story, start, count)).await?)?;
    // Steam counts results, yet the page shows none: the page has changed. Past the first page
    // it only means the list has run out.
    if start == 0 && page.rows == 0 && page.total > 0 {
        return Err(parse::UNEXPECTED.to_string());
    }
    Ok(page)
}
/// Details for `ids`, in that order. A game Steam does not return is left out, but a batch
/// with nothing usable in it is an error, so a changed answer never looks like an empty one.
pub(crate) async fn details<S: StoreApi>(api: &S, ids: &[u32]) -> Result<Vec<Item>, String> {
    let mut found = HashMap::new();
    for batch in ids.chunks(PER_REQUEST) {
        let input = json!({
            "ids": batch.iter().map(|appid| json!({ "appid": appid })).collect::<Vec<_>>(),
            "context": context(),
            "data_request": data_request(),
        });
        let answer = parse::items(&api.get(&with_input(GET_ITEMS, &input)?).await?)?;
        if answer.items.is_empty() {
            return Err(parse::UNEXPECTED.to_string());
        }
        for item in answer.items {
            found.insert(item.game.appid, item);
        }
    }
    Ok(ids.iter().filter_map(|id| found.remove(id)).collect())
}
/// The games on a free weekend among Steam's top sellers.
pub(crate) async fn scan_free_weekends<S: StoreApi>(api: &S) -> Result<Vec<Item>, String> {
    let mut found = Vec::new();
    for page in 0..SCAN_PAGES {
        let input = json!({
            "query": {
                "start": page * SCAN_PAGE,
                "count": SCAN_PAGE,
                "sort": SORT_TOP_SELLERS,
                "filters": { "released_only": true, "type_filters": { "include_games": true } },
            },
            "context": context(),
            "data_request": data_request(),
        });
        let answer = parse::items(&api.get(&with_input(QUERY, &input)?).await?)?;
        // Steam always has top sellers, so none at all means the answer has changed.
        if page == 0 && answer.items.is_empty() {
            return Err(parse::UNEXPECTED.to_string());
        }
        let short = (answer.listed as u32) < SCAN_PAGE;
        found.extend(answer.items.into_iter().filter(|item| item.free_weekend.is_some()));
        if short {
            break;
        }
    }
    Ok(found)
}
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;
    use super::StoreApi;
    type Responder = Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
    /// Steam, answered from the test.
    pub(crate) struct FakeStore {
        responder: Responder,
        calls: Mutex<Vec<String>>,
    }
    impl FakeStore {
        /// The first route whose text the URL contains answers it.
        pub(crate) fn new(routes: Vec<(&str, Result<String, String>)>) -> Self {
            let routes: Vec<(String, Result<String, String>)> =
                routes.into_iter().map(|(text, answer)| (text.to_string(), answer)).collect();
            Self::responding(move |url| {
                routes
                    .iter()
                    .find(|(text, _)| url.contains(text.as_str()))
                    .map(|(_, answer)| answer.clone())
                    .unwrap_or_else(|| Err(format!("no route for {url}")))
            })
        }
        /// For answers that depend on what was asked.
        pub(crate) fn responding(answer: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Self {
            Self { responder: Box::new(answer), calls: Mutex::new(Vec::new()) }
        }
        pub(crate) fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }
    impl StoreApi for FakeStore {
        async fn get(&self, url: &str) -> Result<String, String> {
            self.calls.lock().unwrap().push(url.to_string());
            (self.responder)(url)
        }
    }
    /// The `input_json` a Steam API call carried.
    pub(crate) fn input_of(url: &str) -> serde_json::Value {
        let url = reqwest::Url::parse(url).unwrap();
        let (_, json) = url.query_pairs().find(|(key, _)| key == "input_json").unwrap();
        serde_json::from_str(&json).unwrap()
    }
    /// A Steam answer listing these games.
    pub(crate) fn answer_with(games: Vec<serde_json::Value>) -> String {
        serde_json::json!({ "response": { "store_items": games } }).to_string()
    }
}
#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::fake::{FakeStore, answer_with, input_of};
    use super::*;
    use crate::steam_helper::model::Filters;
    const BASE: &str = "https://store.steampowered.com/search/results/?infinite=1&category1=998&cc=gr&l=english";
    #[test]
    fn listing_urls() {
        assert_eq!(
            search_url(ListingKind::Deals, Some(Mode::OnlineCoop), false, 0, 100),
            format!("{BASE}&sort_by=Reviews_DESC&specials=1&category2=38&start=0&count=100")
        );
        assert_eq!(
            search_url(ListingKind::FreeToPlay, Some(Mode::SinglePlayer), true, 100, 50),
            format!("{BASE}&sort_by=Reviews_DESC&maxprice=free&category2=2&tags=1742&start=100&count=50")
        );
        assert_eq!(
            search_url(ListingKind::Ranked, None, true, 0, 100),
            format!("{BASE}&sort_by=Reviews_DESC&tags=1742&start=0&count=100")
        );
    }
    // Sorting by reviews drops games with none, and a brand-new 100%-off game may have none.
    #[test]
    fn the_free_to_keep_listing_is_not_sorted() {
        assert_eq!(
            search_url(ListingKind::FreeToKeep, None, false, 0, 100),
            format!("{BASE}&maxprice=free&specials=1&start=0&count=100")
        );
    }
    #[test]
    fn modes_follow_the_filters() {
        assert_eq!(modes(Filters::default()), vec![None]);
        assert_eq!(
            modes(Filters { online_coop: true, single_player: false, story_only: true }),
            vec![Some(Mode::OnlineCoop)]
        );
        assert_eq!(
            modes(Filters { online_coop: true, single_player: true, story_only: false }),
            vec![Some(Mode::OnlineCoop), Some(Mode::SinglePlayer)]
        );
    }
    /// Answers each `GetItems` with its games in reverse, as nothing promises Steam's order.
    fn reversing_store() -> FakeStore {
        FakeStore::responding(|url| {
            let input = input_of(url);
            let mut games: Vec<_> = input["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| json!({ "appid": id["appid"], "name": format!("Game {}", id["appid"]) }))
                .collect();
            games.reverse();
            Ok(answer_with(games))
        })
    }
    #[tokio::test]
    async fn details_go_out_in_fifties_in_order() {
        let api = reversing_store();
        let ids: Vec<u32> = (1..=120).collect();
        let out = details(&api, &ids).await.unwrap();
        assert_eq!(api.calls().len(), 3);
        assert_eq!(out.iter().map(|i| i.game.appid).collect::<Vec<_>>(), ids);
        let first = input_of(&api.calls()[0]);
        assert_eq!(first["ids"].as_array().unwrap().len(), 50);
        assert_eq!(first["context"]["country_code"], "GR");
        assert_eq!(first["data_request"]["include_tag_count"], 20);
    }
    #[tokio::test]
    async fn no_ids_cost_no_request() {
        let api = reversing_store();
        assert!(details(&api, &[]).await.unwrap().is_empty());
        assert!(api.calls().is_empty());
    }
    #[tokio::test]
    async fn a_batch_that_comes_back_empty_is_an_error() {
        let api = FakeStore::responding(|_| Ok(answer_with(vec![])));
        assert!(details(&api, &[1, 2]).await.is_err());
    }
    #[tokio::test]
    async fn a_batch_with_nothing_readable_is_an_error() {
        let api = FakeStore::responding(|_| Ok(answer_with(vec![json!({ "renamed": 1 })])));
        assert!(details(&api, &[1, 2]).await.is_err());
    }
    fn listing_answering(total: u32) -> FakeStore {
        FakeStore::new(vec![("search/results", Ok(json!({ "total_count": total, "results_html": "<a></a>" }).to_string()))])
    }
    #[tokio::test]
    async fn a_listing_that_counts_results_but_shows_none_is_an_error() {
        assert!(listing(&listing_answering(498), ListingKind::Deals, None, false, 0, 100).await.is_err());
    }
    #[tokio::test]
    async fn running_off_the_end_of_a_listing_is_fine() {
        let page = listing(&listing_answering(498), ListingKind::Deals, None, false, 100, 50).await.unwrap();
        assert!(page.ids.is_empty());
    }
    #[tokio::test]
    async fn a_listing_with_no_results_is_fine() {
        assert!(listing(&listing_answering(0), ListingKind::FreeToKeep, None, false, 0, 100).await.is_ok());
    }
    #[tokio::test]
    async fn an_empty_first_scan_page_is_an_error() {
        let api = FakeStore::responding(|_| Ok(answer_with(vec![])));
        assert!(scan_free_weekends(&api).await.is_err());
    }
    #[tokio::test]
    async fn an_unreadable_entry_does_not_end_the_scan() {
        let api = FakeStore::responding(|url| {
            let start = input_of(url)["query"]["start"].as_u64().unwrap();
            let games = if start == 0 {
                (0..100).map(|i| if i == 7 { json!({ "appid": 8 }) } else { json!({ "appid": i + 1, "name": "Paid" }) }).collect()
            } else {
                vec![json!({ "appid": 200, "name": "Paid" })]
            };
            Ok(answer_with(games))
        });
        scan_free_weekends(&api).await.unwrap();
        assert_eq!(api.calls().len(), 2);
    }
    #[tokio::test]
    async fn a_failed_batch_is_an_error() {
        let api = FakeStore::responding(|url| {
            if input_of(url)["ids"][0]["appid"] == 51 {
                Err("down".into())
            } else {
                Ok(answer_with(vec![json!({ "appid": 1, "name": "Game" })]))
            }
        });
        let ids: Vec<u32> = (1..=60).collect();
        assert_eq!(details(&api, &ids).await, Err("down".to_string()));
    }
    #[tokio::test]
    async fn the_scan_keeps_only_free_weekends_and_stops_at_a_short_page() {
        let api = FakeStore::responding(|url| {
            let start = input_of(url)["query"]["start"].as_u64().unwrap();
            let size = if start == 0 { 100 } else { 5 };
            let games = (0..size)
                .map(|i| {
                    let free = (start == 0 && i < 2) || (start == 100 && i == 0);
                    let appid = start + i + 1;
                    if free {
                        json!({ "appid": appid, "name": "Free", "free_weekend": { "end_time": 2000 } })
                    } else {
                        json!({ "appid": appid, "name": "Paid" })
                    }
                })
                .collect();
            Ok(answer_with(games))
        });
        let found = scan_free_weekends(&api).await.unwrap();
        assert_eq!(found.iter().map(|i| i.game.appid).collect::<Vec<_>>(), vec![1, 2, 101]);
        let calls = api.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(input_of(&calls[0])["query"]["sort"], 12);
        assert_eq!(input_of(&calls[1])["query"]["start"], 100);
    }
    #[tokio::test]
    async fn the_scan_covers_a_thousand_games_and_no_more() {
        let api = FakeStore::responding(|_| {
            let games = (0..100).map(|i| json!({ "appid": i + 1, "name": "Paid" })).collect();
            Ok(answer_with(games))
        });
        scan_free_weekends(&api).await.unwrap();
        assert_eq!(api.calls().len(), 10);
    }
    #[tokio::test]
    async fn a_listing_is_read_from_the_search_page() {
        let html = r#"{"total_count":2,"results_html":"<a data-ds-appid=\"5\"></a><a data-ds-appid=\"6\"></a>"}"#;
        let api = FakeStore::new(vec![("search/results", Ok(html.to_string()))]);
        let page = listing(&api, ListingKind::Deals, None, false, 0, 100).await.unwrap();
        assert_eq!(page.ids, vec![5, 6]);
    }
}
