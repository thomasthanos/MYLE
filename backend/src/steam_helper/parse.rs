//! Reads what Steam's store sends. None of it is a documented API, so a missing field costs that
//! field, and only an answer that is not Steam's at all is an error.
use std::sync::LazyLock;
use regex_lite::Regex;
use serde_json::Value;
use super::model::Game;
const IMAGES: &str = "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps";
const UNEXPECTED: &str = "Steam sent something unexpected.";
const CATEGORY_SINGLE_PLAYER: u64 = 2;
const CATEGORY_ONLINE_COOP: u64 = 38;
const TAG_STORY_RICH: u64 = 1742;
const ADULT_TAGS: [u64; 2] = [9130, 24904];
const ADULT_DESCRIPTORS: [u64; 2] = [3, 4];
static ROW_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-ds-appid="([^"]*)""#).expect("a valid pattern"));
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FreeWeekend {
    pub end_time: u64,
    pub text: Option<String>,
    pub appid: Option<u32>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub game: Game,
    pub adult: bool,
    pub free_weekend: Option<FreeWeekend>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPage {
    pub ids: Vec<u32>,
    /// Rows Steam returned, counting those without a single numeric id.
    pub rows: u32,
    pub total: u32,
}
/// The games in a `GetItems` or `Query` answer.
pub(crate) fn items(json: &str) -> Result<Vec<Item>, String> {
    let value: Value = serde_json::from_str(json).map_err(|_| UNEXPECTED.to_string())?;
    let response = value.get("response").ok_or(UNEXPECTED)?;
    Ok(response
        .get("store_items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(item)
        .collect())
}
/// The app ids of a store search page. The HTML is only searched, never shown.
pub(crate) fn search_page(json: &str) -> Result<SearchPage, String> {
    let value: Value = serde_json::from_str(json).map_err(|_| UNEXPECTED.to_string())?;
    let html = value.get("results_html").and_then(Value::as_str).ok_or(UNEXPECTED)?;
    let rows: Vec<&str> = ROW_ID.captures_iter(html).filter_map(|c| c.get(1)).map(|m| m.as_str()).collect();
    Ok(SearchPage {
        ids: rows.iter().filter_map(|id| id.parse().ok()).collect(),
        rows: rows.len() as u32,
        total: number(&value, "total_count").unwrap_or(0) as u32,
    })
}
fn item(value: &Value) -> Option<Item> {
    let appid = u32::try_from(number(value, "appid")?).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    let players = ids(value.get("categories").and_then(|c| c.get("supported_player_categoryids")), None);
    let tags = ids(value.get("tags"), Some("tagid"));
    let descriptors = ids(value.get("content_descriptorids"), None);
    let reviews = value.get("reviews").and_then(|r| r.get("summary_filtered"));
    let purchase = value.get("best_purchase_option");
    let header = value.get("assets").and_then(|a| a.get("header")).and_then(Value::as_str);
    let game = Game {
        appid,
        name,
        cover_url: header.map(|path| format!("{IMAGES}/{appid}/{path}")),
        store_url: format!("https://store.steampowered.com/app/{appid}"),
        review_percent: reviews.and_then(|r| number(r, "percent_positive")).unwrap_or(0).min(100) as u8,
        review_count: reviews.and_then(|r| number(r, "review_count")).unwrap_or(0) as u32,
        is_free: value.get("is_free").and_then(Value::as_bool).unwrap_or(false),
        price: text(purchase, "formatted_final_price"),
        original_price: text(purchase, "formatted_original_price"),
        discount_percent: purchase.and_then(|p| number(p, "discount_pct")).unwrap_or(0).min(100) as u8,
        deal_ends_at: purchase
            .and_then(|p| p.get("active_discounts"))
            .and_then(|d| d.get(0))
            .and_then(|d| number(d, "discount_end_date")),
        online_coop: players.contains(&CATEGORY_ONLINE_COOP),
        single_player: players.contains(&CATEGORY_SINGLE_PLAYER),
        story: tags.contains(&TAG_STORY_RICH),
    };
    Some(Item {
        game,
        adult: tags.iter().any(|t| ADULT_TAGS.contains(t))
            || descriptors.iter().any(|d| ADULT_DESCRIPTORS.contains(d)),
        free_weekend: value.get("free_weekend").and_then(free_weekend),
    })
}
fn free_weekend(value: &Value) -> Option<FreeWeekend> {
    Some(FreeWeekend {
        end_time: number(value, "end_time")?,
        text: text(Some(value), "text"),
        appid: number(value, "appid").and_then(|id| u32::try_from(id).ok()),
    })
}
/// Numbers, or the `key` of each object, from a JSON list.
fn ids(list: Option<&Value>, key: Option<&str>) -> Vec<u64> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| match key {
            Some(key) => entry.get(key)?.as_u64(),
            None => entry.as_u64(),
        })
        .collect()
}
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}
fn text(value: Option<&Value>, key: &str) -> Option<String> {
    value?.get(key)?.as_str().map(str::to_string)
}
#[cfg(test)]
mod tests {
    use super::*;
    const ITEMS: &str = include_str!("fixtures/items.json");
    fn by_id(appid: u32) -> Item {
        items(ITEMS).unwrap().into_iter().find(|i| i.game.appid == appid).unwrap()
    }
    #[test]
    fn a_game_is_read_whole() {
        let g = by_id(239140).game;
        assert_eq!((g.name.as_str(), g.review_percent, g.review_count), ("Dying Light", 95, 376585));
        assert_eq!(
            (g.discount_percent, g.price.as_deref(), g.original_price.as_deref(), g.deal_ends_at),
            (90, Some("2,49€"), Some("24,99€"), Some(1794243600))
        );
        assert!(g.online_coop && g.single_player && !g.story);
        assert_eq!(
            g.cover_url.as_deref(),
            Some("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/239140/9d1b810b85ae46a0acbdfddf583ccbb81271183e/header.jpg")
        );
        assert_eq!(g.store_url, "https://store.steampowered.com/app/239140");
    }
    #[test]
    fn story_comes_from_the_story_rich_tag() {
        assert!(by_id(1086940).game.story);
        assert!(!by_id(230410).game.story);
    }
    #[test]
    fn the_adult_rule_matches_what_steam_really_sends() {
        for id in [420110, 1, 2, 3] {
            assert!(by_id(id).adult, "{id}");
        }
        for id in [1086940, 1091500, 239140] {
            assert!(!by_id(id).adult, "{id}");
        }
    }
    #[test]
    fn a_free_game_has_no_price() {
        let g = by_id(230410).game;
        assert!(g.is_free);
        assert_eq!(g.price, None);
    }
    #[test]
    fn a_free_weekend_keeps_its_trial() {
        let w = by_id(4435490).free_weekend.unwrap();
        assert_eq!((w.end_time, w.appid), (1794420000, Some(4269880)));
        assert_eq!(w.text.as_deref(), Some("Call of Duty®: Modern Warfare® 4 - DMZ Free Trial"));
        let plain = by_id(1272080).free_weekend.unwrap();
        assert_eq!((plain.end_time, plain.text), (1794247200, None));
        assert!(by_id(239140).free_weekend.is_none());
    }
    #[test]
    fn missing_fields_do_not_lose_the_game() {
        let g = by_id(9).game;
        assert_eq!((g.review_percent, g.review_count, g.cover_url, g.price), (0, 0, None, None));
        assert!(!g.is_free && !g.online_coop && !g.single_player);
    }
    #[test]
    fn entries_without_a_name_or_id_are_skipped() {
        let list = items(ITEMS).unwrap();
        assert!(list.iter().all(|i| i.game.appid != 10 && i.game.appid != 11));
        assert_eq!(list.len(), 11);
    }
    #[test]
    fn what_is_not_steams_answer_is_an_error() {
        assert!(items("<html>").is_err());
        assert!(items("{}").is_err());
        assert!(search_page("<html>").is_err());
        assert!(search_page("{}").is_err());
    }
    #[test]
    fn a_response_with_no_list_is_empty() {
        assert!(items(r#"{"response":{}}"#).unwrap().is_empty());
    }
    #[test]
    fn search_counts_rows_and_skips_bundles() {
        let json = r#"{"total_count":498,"results_html":"<a data-ds-appid=\"1211020\"></a><a data-ds-appid=\"239140\"></a><a data-ds-appid=\"10,20\"></a>"}"#;
        let p = search_page(json).unwrap();
        assert_eq!((p.ids, p.rows, p.total), (vec![1211020, 239140], 3, 498));
    }
    #[test]
    fn a_filter_key_tells_combinations_apart() {
        use super::super::model::Filters;
        assert_eq!(Filters { online_coop: true, single_player: false, story_only: false }.key(), "c1s0t0");
        assert_eq!(Filters { online_coop: false, single_player: true, story_only: true }.key(), "c0s1t1");
    }
}
