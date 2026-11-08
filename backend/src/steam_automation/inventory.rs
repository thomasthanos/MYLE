//! Listing duplicate trading cards on the Steam Market, always keeping one of each. Every listing
//! still has to be confirmed in Steam Guard by the owner; MYLE never does that step.
use std::collections::{HashMap, HashSet};
use serde_json::Value;
use super::community::{Community, CommunityError, encode};
use super::session::Session;
const COMMUNITY: &str = "https://steamcommunity.com";
const SELL_URL: &str = "https://steamcommunity.com/market/sellitem/";
const TRADING_CARD: &str = "item_class_2";
const MAX_LISTINGS: usize = 20;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Card {
    pub asset_id: String,
    pub hash_name: String,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SellReport {
    pub listed: u32,
    pub cents: u32,
    pub waiting_confirmation: u32,
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}
fn is_marketable_card(description: &Value) -> bool {
    let is_card = description
        .get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| tags.iter().any(|tag| text(tag, "internal_name") == Some(TRADING_CARD)));
    is_card && description.get("marketable").and_then(Value::as_u64) == Some(1)
}
/// The marketable trading cards on one page, and where the next page starts.
pub(crate) fn parse_inventory(json: &str) -> Result<(Vec<Card>, Option<String>), String> {
    let unexpected = || "Steam did not send the inventory.".to_string();
    let value: Value = serde_json::from_str(json).map_err(|_| unexpected())?;
    if value.get("success").and_then(Value::as_u64) != Some(1) {
        return Err(unexpected());
    }
    let list = |key: &str| value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let names: HashMap<(&str, &str), &str> = list("descriptions")
        .iter()
        .filter(|description| is_marketable_card(description))
        .filter_map(|d| Some(((text(d, "classid")?, text(d, "instanceid")?), text(d, "market_hash_name")?)))
        .collect();
    let cards = list("assets")
        .iter()
        .filter_map(|asset| {
            let name = names.get(&(text(asset, "classid")?, text(asset, "instanceid")?))?;
            Some(Card { asset_id: text(asset, "assetid")?.to_string(), hash_name: name.to_string() })
        })
        .collect();
    let more = value.get("more_items").and_then(Value::as_u64) == Some(1);
    Ok((cards, more.then(|| text(&value, "last_assetid").map(str::to_string)).flatten()))
}
/// Every copy of each card but the first, which is never sold.
pub(crate) fn duplicates(cards: &[Card]) -> Vec<Card> {
    let mut seen = HashSet::new();
    cards.iter().filter(|card| !seen.insert(card.hash_name.as_str())).cloned().collect()
}
/// A formatted price in cents: `0,31€`, `2,--€`, `1.234,56€`.
pub(crate) fn parse_price(text: &str) -> Option<u32> {
    let kept: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-')).collect();
    let (whole, cents) = kept.rsplit_once([',', '.'])?;
    let whole: u32 = whole.replace([',', '.'], "").parse().ok()?;
    let cents: u32 = if cents == "--" { 0 } else { cents.parse().ok()? };
    Some(whole * 100 + cents)
}
/// A cent under the cheapest listing, and never below the owner's minimum.
pub(crate) fn list_price(lowest: u32, minimum: u32) -> u32 {
    lowest.saturating_sub(1).max(minimum)
}
/// What the seller receives when the buyer pays this: Steam adds 5% and 10%, each at least a cent.
pub(crate) fn seller_gets(buyer_cents: u32) -> u32 {
    (1..=buyer_cents)
        .rev()
        .find(|seller| seller + (seller * 5 / 100).max(1) + (seller * 10 / 100).max(1) <= buyer_cents)
        .unwrap_or(0)
}
async fn read_all_cards<C: Community>(api: &C, steam_id: u64) -> Result<Vec<Card>, CommunityError> {
    let mut cards = Vec::new();
    let mut start = None;
    loop {
        let mut url = format!("{COMMUNITY}/inventory/{steam_id}/753/6?l=english&count=2000");
        if let Some(last) = &start {
            url.push_str(&format!("&start_assetid={last}"));
        }
        let (found, more) = parse_inventory(&api.get(&url).await?).map_err(CommunityError::Other)?;
        cards.extend(found);
        match more {
            Some(next) => start = Some(next),
            None => return Ok(cards),
        }
    }
}
async fn lowest_price<C: Community>(api: &C, hash_name: &str) -> Result<Option<u32>, CommunityError> {
    let url = format!(
        "{COMMUNITY}/market/priceoverview/?country=GR&currency=3&appid=753&market_hash_name={}",
        encode(hash_name)
    );
    let answer: Value = serde_json::from_str(&api.get(&url).await?).unwrap_or(Value::Null);
    Ok(text(&answer, "lowest_price").and_then(parse_price))
}
/// Lists up to twenty duplicates. An asset goes into `already_listed` before it is posted, so a
/// failure partway can never list the same one twice.
pub(crate) async fn sell_duplicates<C: Community>(
    api: &C,
    session: &Session,
    minimum: u32,
    already_listed: &mut HashSet<String>,
) -> Result<SellReport, CommunityError> {
    let cards = read_all_cards(api, session.steam_id).await?;
    let to_sell: Vec<Card> = duplicates(&cards)
        .into_iter()
        .filter(|card| !already_listed.contains(&card.asset_id))
        .take(MAX_LISTINGS)
        .collect();
    let referer = format!("{COMMUNITY}/profiles/{}/inventory/", session.steam_id);
    let mut prices: HashMap<String, Option<u32>> = HashMap::new();
    let mut report = SellReport::default();
    for card in to_sell {
        let price = match prices.get(&card.hash_name) {
            Some(known) => *known,
            None => {
                let found = lowest_price(api, &card.hash_name).await?;
                prices.insert(card.hash_name.clone(), found);
                found
            }
        };
        let Some(lowest) = price else {
            continue;
        };
        let seller = seller_gets(list_price(lowest, minimum));
        if seller == 0 {
            continue;
        }
        already_listed.insert(card.asset_id.clone());
        let form = [
            ("sessionid", session.session_id.clone()),
            ("appid", "753".to_string()),
            ("contextid", "6".to_string()),
            ("assetid", card.asset_id.clone()),
            ("amount", "1".to_string()),
            ("price", seller.to_string()),
        ];
        let answer: Value = serde_json::from_str(&api.post(SELL_URL, &form, &referer).await?).unwrap_or(Value::Null);
        if answer.get("success").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        report.listed += 1;
        report.cents += seller;
        let needs = |key: &str| answer.get(key).and_then(Value::as_bool) == Some(true);
        if needs("needs_mobile_confirmation") || needs("needs_email_confirmation") {
            report.waiting_confirmation += 1;
        }
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use serde_json::json;
    use super::*;
    use crate::steam_automation::community::FakeCommunity;
    use crate::steam_automation::session::from_cookies;
    const INVENTORY: &str = include_str!("fixtures/inventory.json");
    fn session() -> Session {
        from_cookies(&[("steamLoginSecure".to_string(), "76561198000000001%7C%7Ct".to_string()), ("sessionid".to_string(), "sid".to_string())]).unwrap()
    }
    fn card(asset_id: &str, hash_name: &str) -> Card {
        Card { asset_id: asset_id.to_string(), hash_name: hash_name.to_string() }
    }
    #[test]
    fn only_marketable_trading_cards_count() {
        let (cards, more) = parse_inventory(INVENTORY).unwrap();
        assert_eq!(cards.iter().map(|c| c.asset_id.as_str()).collect::<Vec<_>>(), vec!["1001", "1002", "1003", "1004", "1007"]);
        assert_eq!(more, None);
    }
    #[test]
    fn a_page_with_more_gives_where_to_go_on() {
        let json = r#"{"assets":[],"descriptions":[],"more_items":1,"last_assetid":"55","success":1}"#;
        assert_eq!(parse_inventory(json).unwrap().1, Some("55".to_string()));
    }
    #[test]
    fn an_empty_inventory_is_empty() {
        assert!(parse_inventory(r#"{"total_inventory_count":0,"success":1}"#).unwrap().0.is_empty());
    }
    #[test]
    fn an_answer_that_is_not_an_inventory_is_an_error() {
        assert!(parse_inventory("null").is_err());
        assert!(parse_inventory("<html>").is_err());
        assert!(parse_inventory(r#"{"success":false}"#).is_err());
    }
    #[test]
    fn the_last_copy_is_always_kept() {
        let cards = [card("1", "A"), card("2", "A"), card("3", "A"), card("4", "B")];
        assert_eq!(duplicates(&cards), vec![card("2", "A"), card("3", "A")]);
    }
    #[test]
    fn foils_are_different_cards() {
        assert!(duplicates(&[card("1", "620-Wheatley"), card("2", "620-Wheatley (Foil)")]).is_empty());
    }
    #[test]
    fn prices_read_in_euros() {
        assert_eq!(parse_price("0,31€"), Some(31));
        assert_eq!(parse_price("1,84€"), Some(184));
        assert_eq!(parse_price("2,--€"), Some(200));
        assert_eq!(parse_price("1.234,56€"), Some(123456));
        assert_eq!(parse_price(""), None);
    }
    #[test]
    fn the_price_is_a_cent_under_the_market() {
        assert_eq!(list_price(31, 5), 30);
    }
    #[test]
    fn the_price_never_goes_below_the_minimum() {
        assert_eq!(list_price(5, 5), 5);
        assert_eq!(list_price(3, 5), 5);
    }
    #[test]
    fn fees_are_taken_off_the_buyers_price() {
        assert_eq!(seller_gets(3), 1);
        assert_eq!(seller_gets(5), 3);
        assert_eq!(seller_gets(31), 28);
        assert_eq!(seller_gets(30), 27);
        assert_eq!(seller_gets(100), 88);
    }
    #[test]
    fn too_cheap_for_the_fees_gets_nothing() {
        assert_eq!(seller_gets(2), 0);
    }
    /// An inventory of `count` copies of one card, and a market price of 31 cents.
    fn market(count: u32) -> FakeCommunity {
        FakeCommunity::answering(move |url| {
            if url.contains("priceoverview") {
                return Ok(r#"{"success":true,"lowest_price":"0,31€"}"#.to_string());
            }
            let assets: Vec<_> = (1..=count).map(|i| json!({ "assetid": i.to_string(), "classid": "1", "instanceid": "0", "amount": "1" })).collect();
            Ok(json!({
                "assets": assets,
                "descriptions": [{ "classid": "1", "instanceid": "0", "marketable": 1, "market_hash_name": "620-Card", "tags": [{ "category": "item_class", "internal_name": "item_class_2" }] }],
                "success": 1
            }).to_string())
        })
        .answering_posts(|_| Ok(r#"{"success":true,"requires_confirmation":1,"needs_mobile_confirmation":true}"#.to_string()))
    }
    #[tokio::test]
    async fn a_run_stops_at_twenty_listings() {
        let api = market(30);
        let report = sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert_eq!((api.posts().len(), report.listed), (20, 20));
    }
    #[tokio::test]
    async fn one_price_lookup_serves_every_copy_of_a_card() {
        let api = market(4);
        sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert_eq!(api.gets().iter().filter(|u| u.contains("priceoverview")).count(), 1);
    }
    #[tokio::test]
    async fn a_card_with_no_price_is_left_alone() {
        let api = FakeCommunity::answering(|url| {
            if url.contains("priceoverview") {
                return Ok(r#"{"success":true}"#.to_string());
            }
            Ok(INVENTORY.to_string())
        });
        let report = sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert!(api.posts().is_empty());
        assert_eq!(report.listed, 0);
    }
    #[tokio::test]
    async fn an_asset_is_never_listed_twice() {
        let api = market(3);
        let mut listed = HashSet::new();
        sell_duplicates(&api, &session(), 5, &mut listed).await.unwrap();
        assert_eq!(api.posts().len(), 2);
        sell_duplicates(&api, &session(), 5, &mut listed).await.unwrap();
        assert_eq!(api.posts().len(), 2);
    }
    #[tokio::test]
    async fn being_signed_out_stops_the_run() {
        let posted = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter = posted.clone();
        let api = market(5).answering_posts(move |_| {
            if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok(r#"{"success":true}"#.to_string())
            } else {
                Err(CommunityError::SignedOut)
            }
        });
        assert_eq!(sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await, Err(CommunityError::SignedOut));
        assert_eq!(api.posts().len(), 2);
    }
    #[tokio::test]
    async fn a_refused_listing_is_not_counted_and_the_run_goes_on() {
        let api = market(3).answering_posts(|_| Ok(r#"{"success":false,"message":"no"}"#.to_string()));
        let report = sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert_eq!((api.posts().len(), report.listed), (2, 0));
    }
    #[tokio::test]
    async fn the_form_sends_what_the_seller_gets() {
        let api = market(2);
        sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        let (url, form, referer) = api.posts()[0].clone();
        assert_eq!(url, "https://steamcommunity.com/market/sellitem/");
        let value = |key: &str| form.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
        assert_eq!((value("price"), value("sessionid"), value("appid"), value("contextid"), value("amount"), value("assetid")), ("27".to_string(), "sid".to_string(), "753".to_string(), "6".to_string(), "1".to_string(), "2".to_string()));
        assert_eq!(referer, "https://steamcommunity.com/profiles/76561198000000001/inventory/");
    }
    #[tokio::test]
    async fn waiting_listings_are_counted() {
        let api = market(2);
        let report = sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert_eq!((report.waiting_confirmation, report.cents), (1, 27));
    }
    #[tokio::test]
    async fn every_page_of_the_inventory_is_read() {
        let api = FakeCommunity::answering(|url| {
            if url.contains("start_assetid=55") {
                return Ok(r#"{"assets":[],"descriptions":[],"success":1}"#.to_string());
            }
            Ok(r#"{"assets":[],"descriptions":[],"more_items":1,"last_assetid":"55","success":1}"#.to_string())
        });
        sell_duplicates(&api, &session(), 5, &mut HashSet::new()).await.unwrap();
        assert_eq!(api.gets().len(), 2);
    }
}
