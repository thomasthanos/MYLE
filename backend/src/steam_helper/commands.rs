//! What the page asks for. One command per section, so sections arrive as they are ready.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;
use super::cache::{DiskCache, Section, live_offers, through_cache};
use super::model::{Filters, FreeOffer, Game};
use super::picks::{week_key, weekly_picks};
use super::sections::{MorePage, RowKind, free_right_now, more, row};
use super::store::HttpStore;
/// One store for the whole app, so the gap between requests holds across sections.
#[derive(Default)]
pub struct SteamHelperState {
    store: Mutex<Option<Arc<HttpStore>>>,
}
impl SteamHelperState {
    pub(crate) fn store(&self) -> Result<Arc<HttpStore>, String> {
        let mut store = self.store.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(store) = store.as_ref() {
            return Ok(store.clone());
        }
        let created = Arc::new(HttpStore::new()?);
        *store = Some(created.clone());
        Ok(created)
    }
}
pub(crate) fn folder() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join("steam-helper"))
}
pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
fn this_week(now: u64) -> Result<i64, String> {
    let local = crate::game_saves::headless::local_seconds(now).ok_or("Could not read this PC's clock.")?;
    Ok(week_key((local / 86_400) as i64))
}
/// Two files per filter combination, taking turns week by week: a section's hour of caching
/// never carries last week's picks past Monday, and old weeks do not pile up.
fn picks_key(filters: Filters, week: i64) -> String {
    format!("picks-{}-{}", (week / 7) % 2, filters.key())
}
#[tauri::command]
pub async fn steam_helper_free(state: State<'_, SteamHelperState>, force: bool) -> Result<Section<FreeOffer>, String> {
    let api = state.store()?;
    let cache = DiskCache::new(folder()?);
    let now = now();
    let mut section = through_cache(&cache, "free", force, now, || free_right_now(&*api)).await;
    section.items = live_offers(section.items, now);
    Ok(section)
}
#[tauri::command]
pub async fn steam_helper_picks(
    state: State<'_, SteamHelperState>,
    filters: Filters,
    force: bool,
) -> Result<Section<Game>, String> {
    let api = state.store()?;
    let folder = folder()?;
    let now = now();
    let week = this_week(now)?;
    let file = folder.join("picks.json");
    let cache = DiskCache::new(folder);
    Ok(through_cache(&cache, &picks_key(filters, week), force, now, || weekly_picks(&*api, &file, filters, week)).await)
}
#[tauri::command]
pub async fn steam_helper_row(
    state: State<'_, SteamHelperState>,
    kind: RowKind,
    filters: Filters,
    force: bool,
) -> Result<Section<Game>, String> {
    let api = state.store()?;
    let cache = DiskCache::new(folder()?);
    let name = if kind == RowKind::Deals { "deals" } else { "free-to-play" };
    let key = format!("{name}-{}", filters.key());
    Ok(through_cache(&cache, &key, force, now(), || row(&*api, kind, filters)).await)
}
#[tauri::command]
pub async fn steam_helper_more(
    state: State<'_, SteamHelperState>,
    kind: RowKind,
    filters: Filters,
    cursor: u32,
) -> Result<MorePage, String> {
    more(&*state.store()?, kind, filters, cursor).await
}
#[cfg(test)]
mod tests {
    use super::*;
    const COOP: Filters = Filters { online_coop: true, single_player: false, story_only: false };
    // An hour's cache must not carry last week's picks past Monday midnight.
    #[test]
    fn a_new_week_never_reads_last_weeks_saved_picks() {
        assert_ne!(picks_key(COOP, 20759), picks_key(COOP, 20766));
    }
    // Two files per filter combination, however many weeks go by.
    #[test]
    fn saved_picks_do_not_pile_up() {
        assert_eq!(picks_key(COOP, 20759), picks_key(COOP, 20759 + 14));
    }
    #[test]
    fn filter_combinations_are_saved_apart() {
        assert_ne!(picks_key(COOP, 20759), picks_key(Filters::default(), 20759));
    }
}
