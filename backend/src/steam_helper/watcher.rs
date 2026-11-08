//! While MYLE runs, looks for what has newly become free, once the page has been opened: an
//! install that never uses the Helper never asks Steam for anything.
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use crate::game_saves::notice::{self, Notice};
use super::cache::{DiskCache, live_offers, through_cache};
use super::commands::{SteamHelperState, folder, now};
use super::model::FreeOffer;
use super::sections::free_right_now;
use super::store::StoreApi;
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(3 * 3600);
/// Offers already known, each with when it ends.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Seen {
    offers: HashMap<String, Option<u64>>,
}
/// What a notice needs to say.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeNews {
    pub game: String,
    pub ends_at: Option<u64>,
    pub more: u32,
}
/// What is known after this look, and which offers are new. With nothing known yet, nothing is
/// new: the first look only remembers, so the owner is not flooded.
pub(crate) fn diff(seen: Option<Seen>, offers: &[FreeOffer], now: u64) -> (Seen, Vec<FreeOffer>) {
    let first_look = seen.is_none();
    let mut seen = seen.unwrap_or_default();
    let new = offers.iter().filter(|o| !first_look && !seen.offers.contains_key(&o.offer_id)).cloned().collect();
    seen.offers.retain(|id, ends_at| match ends_at {
        Some(end) => *end >= now,
        None => offers.iter().any(|o| o.offer_id == *id),
    });
    seen.offers.extend(offers.iter().map(|o| (o.offer_id.clone(), o.ends_at)));
    (seen, new)
}
fn read_seen(file: &Path) -> Option<Seen> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}
fn save_seen(file: &Path, seen: &Seen) {
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(bytes) = serde_json::to_vec(seen) {
        let _ = crate::game_saves::atomic::write(file, &bytes);
    }
}
/// One look. `None` until the page has been opened, without touching the network or the disk.
pub(crate) async fn tick<S: StoreApi>(
    api: &S,
    cache: &DiskCache,
    seen_file: &Path,
    opened: bool,
    now: u64,
) -> Option<FreeNews> {
    if !opened {
        return None;
    }
    let section = through_cache(cache, "free", false, now, || free_right_now(api)).await;
    if section.stale || section.error.is_some() {
        return None;
    }
    let (seen, mut new) = diff(read_seen(seen_file), &live_offers(section.items, now), now);
    save_seen(seen_file, &seen);
    new.sort_by_key(|offer| offer.ends_at.unwrap_or(u64::MAX));
    let soonest = new.first()?;
    Some(FreeNews { game: soonest.game.name.clone(), ends_at: soonest.ends_at, more: new.len() as u32 - 1 })
}
async fn look(app: &AppHandle) -> Option<FreeNews> {
    let api = app.state::<SteamHelperState>().store().ok()?;
    let folder = folder().ok()?;
    let cache = DiskCache::new(folder.clone());
    let opened = cache.has("free");
    tick(&*api, &cache, &folder.join("seen.json"), opened, now()).await
}
/// Looks shortly after start-up and then every three hours, for as long as MYLE runs.
pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if let Some(news) = look(&app).await {
                notice::show(&Notice::free_on_steam(news, now()));
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use serde_json::json;
    use super::*;
    use crate::steam_helper::model::{Game, OfferKind};
    use crate::steam_helper::store::fake::{FakeStore, answer_with};
    fn o(id: &str, ends_at: Option<u64>) -> FreeOffer {
        FreeOffer {
            game: Game {
                appid: 1,
                name: id.to_string(),
                cover_url: None,
                store_url: String::new(),
                review_percent: 0,
                review_count: 0,
                is_free: false,
                price: None,
                original_price: None,
                discount_percent: 0,
                deal_ends_at: None,
                online_coop: false,
                single_player: false,
                story: false,
            },
            kind: OfferKind::FreeWeekend,
            label: "Free weekend".into(),
            starts_at: None,
            ends_at,
            offer_id: id.to_string(),
        }
    }
    fn seen_with(entries: &[(&str, Option<u64>)]) -> Seen {
        Seen { offers: entries.iter().map(|(id, end)| (id.to_string(), *end)).collect() }
    }
    fn names(offers: &[FreeOffer]) -> Vec<&str> {
        offers.iter().map(|o| o.offer_id.as_str()).collect()
    }
    #[test]
    fn the_first_run_only_remembers() {
        let (seen, new) = diff(None, &[o("a:10", Some(10))], 5);
        assert!(new.is_empty());
        assert!(seen.offers.contains_key("a:10"));
    }
    #[test]
    fn a_new_offer_is_reported_once() {
        let offers = [o("b:20", Some(20))];
        let (seen, new) = diff(Some(Seen::default()), &offers, 5);
        assert_eq!(names(&new), vec!["b:20"]);
        let (_, again) = diff(Some(seen), &offers, 6);
        assert!(again.is_empty());
    }
    #[test]
    fn the_same_game_returning_later_is_new() {
        let (_, new) = diff(Some(seen_with(&[("1272080:100", Some(100))])), &[o("1272080:900", Some(900))], 50);
        assert_eq!(names(&new), vec!["1272080:900"]);
    }
    #[test]
    fn expired_offers_are_forgotten() {
        let (seen, _) = diff(Some(seen_with(&[("a:10", Some(10))])), &[], 11);
        assert!(seen.offers.is_empty());
    }
    #[test]
    fn an_offer_that_has_not_ended_is_remembered_even_when_steam_stops_listing_it() {
        let (seen, _) = diff(Some(seen_with(&[("a:10", Some(10))])), &[], 9);
        assert!(seen.offers.contains_key("a:10"));
    }
    #[test]
    fn an_offer_with_no_end_stays_while_listed() {
        let listed = [o("a:0", None)];
        let (seen, _) = diff(Some(seen_with(&[("a:0", None)])), &listed, 50);
        assert!(seen.offers.contains_key("a:0"));
        let (seen, _) = diff(Some(seen), &[], 51);
        assert!(seen.offers.is_empty());
    }
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("myle-watcher-{}", uuid::Uuid::new_v4()));
        dir.join(name)
    }
    fn cache_for(seen_file: &std::path::Path) -> DiskCache {
        DiskCache::new(seen_file.parent().unwrap().to_path_buf())
    }
    /// Steam with these free weekends, as (appid, name, end).
    fn store_with(weekends: Vec<(u32, &'static str, u64)>) -> FakeStore {
        FakeStore::responding(move |url| {
            if url.contains("search/results") {
                return Ok(json!({ "total_count": 0, "results_html": "" }).to_string());
            }
            let games = weekends
                .iter()
                .map(|(appid, name, end)| json!({ "appid": appid, "name": name, "free_weekend": { "end_time": end } }))
                .collect();
            Ok(answer_with(games))
        })
    }
    fn write_seen(file: &std::path::Path, text: &str) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    #[tokio::test]
    async fn before_the_page_is_ever_opened_it_costs_no_network() {
        let api = store_with(vec![(5, "Game", 9000)]);
        let file = temp("seen.json");
        assert_eq!(tick(&api, &cache_for(&file), &file, false, 1000).await, None);
        assert!(api.calls().is_empty());
        assert!(!file.exists());
    }
    #[tokio::test]
    async fn several_new_offers_become_one_notice() {
        let api = store_with(vec![(5, "Later", 9000), (6, "Soonest", 3000), (7, "Middle", 5000)]);
        let file = temp("seen.json");
        write_seen(&file, r#"{"offers":{}}"#);
        let news = tick(&api, &cache_for(&file), &file, true, 1000).await.unwrap();
        assert_eq!(news, FreeNews { game: "Soonest".to_string(), ends_at: Some(3000), more: 2 });
    }
    #[tokio::test]
    async fn the_first_check_ever_is_silent_and_the_next_finds_what_is_new() {
        let file = temp("seen.json");
        let cache = cache_for(&file);
        assert_eq!(tick(&store_with(vec![(5, "Old", 90000)]), &cache, &file, true, 1000).await, None);
        // Past the hour's cache, so Steam is asked again.
        let news = tick(&store_with(vec![(5, "Old", 90000), (6, "New", 80000)]), &cache, &file, true, 9000).await.unwrap();
        assert_eq!((news.game.as_str(), news.more), ("New", 0));
    }
    #[tokio::test]
    async fn a_failed_scan_forgets_nothing_and_notifies_nobody() {
        let file = temp("seen.json");
        let cache = cache_for(&file);
        assert_eq!(tick(&store_with(vec![(5, "Old", 90000)]), &cache, &file, true, 1000).await, None);
        let before = std::fs::read(&file).unwrap();
        let down = FakeStore::responding(|_| Err("down".to_string()));
        assert_eq!(tick(&down, &cache, &file, true, 9000).await, None);
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
    #[tokio::test]
    async fn a_corrupt_seen_file_counts_as_a_first_run() {
        let file = temp("seen.json");
        write_seen(&file, "not json");
        let api = store_with(vec![(5, "Game", 9000)]);
        assert_eq!(tick(&api, &cache_for(&file), &file, true, 1000).await, None);
        assert!(serde_json::from_slice::<Seen>(&std::fs::read(&file).unwrap()).is_ok());
    }
    #[tokio::test]
    async fn an_offer_is_news_when_it_starts_and_not_before() {
        let file = temp("seen.json");
        write_seen(&file, r#"{"offers":{}}"#);
        let cache = cache_for(&file);
        let api = FakeStore::responding(|url| {
            if url.contains("search/results") {
                return Ok(json!({ "total_count": 0, "results_html": "" }).to_string());
            }
            Ok(answer_with(vec![json!({ "appid": 5, "name": "Soon", "free_weekend": { "start_time": 5000, "end_time": 90000 } })]))
        });
        assert_eq!(tick(&api, &cache, &file, true, 1000).await, None);
        let news = tick(&api, &cache, &file, true, 9000).await.unwrap();
        assert_eq!(news.game, "Soon");
    }
    #[tokio::test]
    async fn an_offer_that_has_ended_is_not_news() {
        let file = temp("seen.json");
        write_seen(&file, r#"{"offers":{}}"#);
        let api = store_with(vec![(5, "Over", 999)]);
        assert_eq!(tick(&api, &cache_for(&file), &file, true, 1000).await, None);
    }
}
