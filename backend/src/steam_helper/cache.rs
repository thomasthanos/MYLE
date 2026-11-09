//! Each section's last answer, on disk: an hour old at most before asking Steam again, and what
//! the page falls back on, marked as such, when Steam cannot answer.
use std::future::Future;
use std::path::PathBuf;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use super::model::FreeOffer;
const FRESH_SECS: u64 = 3600;
const MIN_FORCE_SECS: u64 = 60;
/// Older than this and a list says more about last week than about now.
const SHOWN_FOR_SECS: u64 = 7 * 86_400;
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Section<T> {
    pub items: Vec<T>,
    pub updated_at: Option<u64>,
    /// These are older answers: asking again just failed.
    pub stale: bool,
    pub error: Option<String>,
    /// Recent enough that Steam need not be asked.
    pub fresh: bool,
}
#[derive(Deserialize, Serialize)]
struct Entry<T> {
    saved_at: u64,
    items: Vec<T>,
}
pub(crate) struct DiskCache {
    dir: PathBuf,
}
impl DiskCache {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    pub(crate) fn has(&self, key: &str) -> bool {
        self.path(key).exists()
    }
    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }
    fn read<T: DeserializeOwned>(&self, key: &str) -> Option<Entry<T>> {
        serde_json::from_slice(&std::fs::read(self.path(key)).ok()?).ok()
    }
    /// A failed save only means asking Steam again next time.
    fn write<T: Serialize>(&self, key: &str, entry: &Entry<T>) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(bytes) = serde_json::to_vec(entry) {
            let _ = crate::game_saves::atomic::write(&self.path(key), &bytes);
        }
    }
}
/// A section's items: saved ones while fresh, otherwise `fetch`'s, and the saved ones again,
/// marked stale, when `fetch` fails. `force` shortens fresh to a minute.
pub(crate) async fn through_cache<T, F, Fut>(cache: &DiskCache, key: &str, force: bool, now: u64, fetch: F) -> Section<T>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<T>, String>>,
{
    let limit = if force { MIN_FORCE_SECS } else { FRESH_SECS };
    let saved = match cache.read::<T>(key) {
        Some(entry) if now.saturating_sub(entry.saved_at) < limit => {
            return Section { items: entry.items, updated_at: Some(entry.saved_at), stale: false, error: None, fresh: true };
        }
        other => other,
    };
    match fetch().await {
        Ok(items) => {
            let entry = Entry { saved_at: now, items };
            cache.write(key, &entry);
            Section { items: entry.items, updated_at: Some(now), stale: false, error: None, fresh: true }
        }
        Err(error) => match saved {
            Some(entry) => Section { items: entry.items, updated_at: Some(entry.saved_at), stale: true, error: Some(error), fresh: false },
            None => Section { items: Vec::new(), updated_at: None, stale: false, error: Some(error), fresh: false },
        },
    }
}
/// What was saved for this section, without asking Steam, for the page to show at once.
pub(crate) fn saved_section<T: Serialize + DeserializeOwned>(cache: &DiskCache, key: &str, now: u64) -> Section<T> {
    match cache.read::<T>(key).filter(|entry| now.saturating_sub(entry.saved_at) < SHOWN_FOR_SECS) {
        Some(entry) => Section { fresh: now.saturating_sub(entry.saved_at) < FRESH_SECS, items: entry.items, updated_at: Some(entry.saved_at), stale: false, error: None },
        None => Section { items: Vec::new(), updated_at: None, stale: false, error: None, fresh: false },
    }
}
/// Keeps the offers that are running now, however recently they were saved.
pub(crate) fn live_offers(offers: Vec<FreeOffer>, now: u64) -> Vec<FreeOffer> {
    offers
        .into_iter()
        .filter(|offer| offer.starts_at.is_none_or(|start| start <= now) && offer.ends_at.is_none_or(|end| end >= now))
        .collect()
}
#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use super::*;
    use crate::steam_helper::model::{Game, OfferKind};
    fn cache() -> DiskCache {
        DiskCache::new(std::env::temp_dir().join(format!("myle-cache-{}", uuid::Uuid::new_v4())))
    }
    /// Saves `[1]` at second 1000.
    async fn seeded() -> DiskCache {
        let cache = cache();
        let section = through_cache(&cache, "deals", false, 1000, || async { Ok(vec![1u32]) }).await;
        assert_eq!(section.updated_at, Some(1000));
        cache
    }
    /// Runs the cache and says whether Steam would have been asked.
    async fn run(cache: &DiskCache, force: bool, now: u64, answer: Result<Vec<u32>, String>) -> (Section<u32>, bool) {
        let asked = Cell::new(false);
        let section = through_cache(cache, "deals", force, now, || async {
            asked.set(true);
            answer
        })
        .await;
        (section, asked.get())
    }
    #[tokio::test]
    async fn the_cache_knows_which_sections_were_saved() {
        let cache = seeded().await;
        assert!(cache.has("deals"));
        assert!(!cache.has("free"));
    }
    const WEEK: u64 = 7 * 86_400;
    #[tokio::test]
    async fn what_was_saved_comes_back_without_asking_steam() {
        let section: Section<u32> = saved_section(&seeded().await, "deals", 1001);
        assert_eq!((section.items, section.updated_at, section.fresh), (vec![1], Some(1000), true));
    }
    #[tokio::test]
    async fn a_saved_answer_past_the_hour_is_shown_and_says_it_should_be_refreshed() {
        let section: Section<u32> = saved_section(&seeded().await, "deals", 1000 + 3600);
        assert_eq!((section.items, section.fresh), (vec![1], false));
    }
    #[tokio::test]
    async fn an_answer_older_than_a_week_is_not_shown() {
        let section: Section<u32> = saved_section(&seeded().await, "deals", 1000 + WEEK);
        assert_eq!((section.items, section.updated_at, section.fresh), (vec![], None, false));
    }
    #[tokio::test]
    async fn nothing_saved_is_empty_and_not_fresh() {
        let section: Section<u32> = saved_section(&cache(), "deals", 1000);
        assert_eq!((section.items, section.fresh), (vec![], false));
    }
    #[tokio::test]
    async fn a_fresh_entry_costs_no_request() {
        let (section, asked) = run(&seeded().await, false, 4599, Ok(vec![2])).await;
        assert!(!asked);
        assert_eq!((section.items, section.updated_at, section.stale, section.error), (vec![1], Some(1000), false, None));
    }
    #[tokio::test]
    async fn an_old_entry_is_fetched_again() {
        let (section, asked) = run(&seeded().await, false, 4600, Ok(vec![2])).await;
        assert!(asked);
        assert_eq!((section.items, section.updated_at, section.stale), (vec![2], Some(4600), false));
    }
    #[tokio::test]
    async fn what_was_fetched_is_kept_for_next_time() {
        let cache = seeded().await;
        run(&cache, false, 4600, Ok(vec![2])).await;
        let (section, asked) = run(&cache, false, 4700, Err("unused".to_string())).await;
        assert!(!asked);
        assert_eq!(section.items, vec![2]);
    }
    #[tokio::test]
    async fn a_failure_keeps_what_was_saved_and_says_so() {
        let (section, _) = run(&seeded().await, false, 9000, Err("Steam is down".to_string())).await;
        assert_eq!((section.items, section.updated_at, section.stale), (vec![1], Some(1000), true));
        assert_eq!(section.error.as_deref(), Some("Steam is down"));
    }
    #[tokio::test]
    async fn a_failure_with_nothing_saved_is_empty_with_an_error() {
        let (section, _) = run(&cache(), false, 9000, Err("Steam is down".to_string())).await;
        assert_eq!((section.items, section.updated_at, section.stale), (vec![], None, false));
        assert_eq!(section.error.as_deref(), Some("Steam is down"));
    }
    #[tokio::test]
    async fn refresh_is_held_to_once_a_minute() {
        let cache = seeded().await;
        assert!(!run(&cache, true, 1059, Ok(vec![2])).await.1);
        assert!(run(&cache, true, 1060, Ok(vec![2])).await.1);
    }
    #[tokio::test]
    async fn a_corrupt_entry_counts_as_nothing() {
        let cache = seeded().await;
        std::fs::write(cache.path("deals"), "not json").unwrap();
        let (section, asked) = run(&cache, false, 1001, Ok(vec![2])).await;
        assert!(asked);
        assert_eq!(section.items, vec![2]);
    }
    fn offer(ends_at: Option<u64>) -> FreeOffer {
        FreeOffer {
            game: Game {
                appid: 1,
                name: "Game".into(),
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
            offer_id: "1:0".into(),
        }
    }
    #[test]
    fn an_offer_that_has_not_started_is_not_live() {
        let soon = FreeOffer { starts_at: Some(1001), ..offer(Some(5000)) };
        let started = FreeOffer { starts_at: Some(1000), ..offer(Some(5000)) };
        let live = live_offers(vec![soon, started], 1000);
        assert_eq!(live.iter().map(|o| o.starts_at).collect::<Vec<_>>(), vec![Some(1000)]);
    }
    #[test]
    fn an_offer_past_its_end_is_not_live() {
        let live = live_offers(vec![offer(Some(999)), offer(Some(1000)), offer(None)], 1000);
        assert_eq!(live.iter().map(|o| o.ends_at).collect::<Vec<_>>(), vec![Some(1000), None]);
    }
}
