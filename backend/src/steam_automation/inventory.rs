//! Listing duplicate trading cards on the Steam Market, always keeping one of each. Every listing
//! still has to be confirmed in Steam Guard by the owner; MYLE never does that step.
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use super::community::{Community, CommunityError, encode};
use super::session::Session;
const COMMUNITY: &str = "https://steamcommunity.com";
const SELL_URL: &str = "https://steamcommunity.com/market/sellitem/";
const TRADING_CARD: &str = "item_class_2";
const FOIL_BORDER: &str = "cardborder_1";
const MAX_LISTINGS: usize = 20;
const ICONS: &str = "https://community.fastly.steamstatic.com/economy/image";
const MARKET_PAGE: &str = "https://steamcommunity.com/market/";
const LISTINGS_PER_PAGE: u32 = 100;
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
/// One thing in the owner's Steam inventory, for the page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub asset_id: String,
    pub hash_name: String,
    pub name: String,
    pub game: String,
    pub kind: &'static str,
    pub icon_url: Option<String>,
    pub marketable: bool,
    pub amount: u32,
}
/// A request to list one item. With no price, the Market's own is used.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRequest {
    pub asset_id: String,
    pub hash_name: String,
    pub buyer_cents: Option<u32>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOutcome {
    pub listed: bool,
    pub buyer_cents: u32,
    pub waiting_confirmation: bool,
    /// Why it was not listed.
    pub reason: Option<String>,
}
/// Something the owner has up for sale.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub listing_id: String,
    pub asset_id: String,
    pub hash_name: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub buyer_cents: u32,
    pub waiting_confirmation: bool,
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}
fn tag_of<'a>(description: &'a Value, category: &str) -> Option<&'a Value> {
    description.get("tags")?.as_array()?.iter().find(|tag| text(tag, "category") == Some(category))
}
fn kind_of(description: &Value) -> &'static str {
    let foil = tag_of(description, "cardborder").and_then(|tag| text(tag, "internal_name")) == Some(FOIL_BORDER);
    match tag_of(description, "item_class").and_then(|tag| text(tag, "internal_name")) {
        Some(TRADING_CARD) if foil => "foil",
        Some(TRADING_CARD) => "card",
        Some("item_class_3") => "background",
        Some("item_class_4") => "emoticon",
        Some("item_class_5") => "booster",
        _ => "other",
    }
}
fn icon_url(description: &Value) -> Option<String> {
    text(description, "icon_url").filter(|icon| !icon.is_empty()).map(|icon| format!("{ICONS}/{icon}/184x184"))
}
/// Everything on one page of the inventory, and where the next page starts.
pub(crate) fn parse_items(json: &str) -> Result<(Vec<InventoryItem>, Option<String>), String> {
    let unexpected = || "Steam did not send the inventory.".to_string();
    let value: Value = serde_json::from_str(json).map_err(|_| unexpected())?;
    if value.get("success").and_then(Value::as_u64) != Some(1) {
        return Err(unexpected());
    }
    let list = |key: &str| value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let descriptions: HashMap<(&str, &str), &Value> =
        list("descriptions").iter().filter_map(|d| Some(((text(d, "classid")?, text(d, "instanceid")?), d))).collect();
    let items = list("assets")
        .iter()
        .filter_map(|asset| {
            let description = *descriptions.get(&(text(asset, "classid")?, text(asset, "instanceid")?))?;
            let label = |key: &str| text(description, key).unwrap_or_default().to_string();
            Some(InventoryItem {
                asset_id: text(asset, "assetid")?.to_string(),
                hash_name: label("market_hash_name"),
                name: label("name"),
                game: tag_of(description, "Game").and_then(|tag| text(tag, "localized_tag_name")).map_or_else(|| label("type"), str::to_string),
                kind: kind_of(description),
                icon_url: icon_url(description),
                marketable: description.get("marketable").and_then(Value::as_u64) == Some(1),
                amount: text(asset, "amount").and_then(|amount| amount.parse().ok()).unwrap_or(1),
            })
        })
        .collect();
    let more = value.get("more_items").and_then(Value::as_u64) == Some(1);
    Ok((items, more.then(|| text(&value, "last_assetid").map(str::to_string)).flatten()))
}
/// The marketable trading cards on one page, and where the next page starts.
pub(crate) fn parse_inventory(json: &str) -> Result<(Vec<Card>, Option<String>), String> {
    let (items, more) = parse_items(json)?;
    let cards = items
        .into_iter()
        .filter(|item| item.marketable && !item.hash_name.is_empty() && matches!(item.kind, "card" | "foil"))
        .map(|item| Card { asset_id: item.asset_id, hash_name: item.hash_name })
        .collect();
    Ok((cards, more))
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
type Page<T> = Result<(Vec<T>, Option<String>), String>;
async fn read_pages<C: Community, T>(api: &C, steam_id: u64, parse: fn(&str) -> Page<T>) -> Result<Vec<T>, CommunityError> {
    let mut all = Vec::new();
    let mut start = None;
    loop {
        let mut url = format!("{COMMUNITY}/inventory/{steam_id}/753/6?l=english&count=2000");
        if let Some(last) = &start {
            url.push_str(&format!("&start_assetid={last}"));
        }
        let (found, more) = parse(&api.get(&url).await?).map_err(CommunityError::Other)?;
        all.extend(found);
        match more {
            Some(next) => start = Some(next),
            None => return Ok(all),
        }
    }
}
pub(crate) async fn read_items<C: Community>(api: &C, steam_id: u64) -> Result<Vec<InventoryItem>, CommunityError> {
    read_pages(api, steam_id, parse_items).await
}
pub(crate) async fn lowest_price<C: Community>(api: &C, hash_name: &str) -> Result<Option<u32>, CommunityError> {
    let url = format!(
        "{COMMUNITY}/market/priceoverview/?country=GR&currency=3&appid=753&market_hash_name={}",
        encode(hash_name)
    );
    let answer: Value = serde_json::from_str(&api.get(&url).await?).unwrap_or(Value::Null);
    Ok(text(&answer, "lowest_price").and_then(parse_price))
}
async fn post_listing<C: Community>(api: &C, session: &Session, asset_id: &str, seller: u32) -> Result<Value, CommunityError> {
    let referer = format!("{COMMUNITY}/profiles/{}/inventory/", session.steam_id);
    let form = [
        ("sessionid", session.session_id.clone()),
        ("appid", "753".to_string()),
        ("contextid", "6".to_string()),
        ("assetid", asset_id.to_string()),
        ("amount", "1".to_string()),
        ("price", seller.to_string()),
    ];
    Ok(serde_json::from_str(&api.post(SELL_URL, &form, &referer).await?).unwrap_or(Value::Null))
}
fn was_listed(answer: &Value) -> bool {
    answer.get("success").and_then(Value::as_bool) == Some(true)
}
fn awaits_confirmation(answer: &Value) -> bool {
    ["needs_mobile_confirmation", "needs_email_confirmation"].iter().any(|key| answer.get(*key).and_then(Value::as_bool) == Some(true))
}
fn is_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}
fn not_listed(reason: &str) -> ListOutcome {
    ListOutcome { reason: Some(reason.to_string()), ..ListOutcome::default() }
}
/// Lists one item the owner chose, which may be their last copy. A locked item never is.
pub(crate) async fn list_item<C: Community>(
    api: &C,
    session: &Session,
    request: &ListRequest,
    minimum: u32,
    locked: &HashSet<String>,
    listed: &mut HashSet<String>,
) -> Result<ListOutcome, CommunityError> {
    if !is_id(&request.asset_id) {
        return Ok(not_listed("Not an item"));
    }
    if locked.contains(&request.asset_id) {
        return Ok(not_listed("Locked"));
    }
    if listed.contains(&request.asset_id) {
        return Ok(not_listed("Already listed"));
    }
    let buyer_cents = match request.buyer_cents {
        Some(typed) => typed,
        None => match lowest_price(api, &request.hash_name).await? {
            Some(lowest) => list_price(lowest, minimum),
            None => return Ok(not_listed("No market price")),
        },
    };
    let seller = seller_gets(buyer_cents);
    if seller == 0 {
        return Ok(not_listed("Price is too low"));
    }
    listed.insert(request.asset_id.clone());
    let answer = post_listing(api, session, &request.asset_id, seller).await?;
    if !was_listed(&answer) {
        listed.remove(&request.asset_id);
        return Ok(not_listed(text(&answer, "message").unwrap_or("Steam refused the listing")));
    }
    Ok(ListOutcome { listed: true, buyer_cents, waiting_confirmation: awaits_confirmation(&answer), reason: None })
}
fn listing(entry: &Value, root: &Value, waiting_confirmation: bool) -> Option<Listing> {
    let asset = entry.get("asset");
    let details = asset.and_then(|asset| {
        if text(asset, "market_hash_name").is_some() {
            return Some(asset);
        }
        root.get("assets")?.get(asset.get("appid")?.to_string())?.get(text(asset, "contextid")?)?.get(text(asset, "id")?)
    });
    let label = |key: &str| details.and_then(|details| text(details, key)).unwrap_or_default().to_string();
    let cents = |key: &str| entry.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
    Some(Listing {
        listing_id: text(entry, "listingid")?.to_string(),
        asset_id: asset.and_then(|asset| text(asset, "id")).unwrap_or_default().to_string(),
        hash_name: label("market_hash_name"),
        name: label("name"),
        icon_url: details.and_then(icon_url),
        buyer_cents: cents("price") + cents("fee"),
        waiting_confirmation,
    })
}
/// The listings on one page, and how many the owner has in all.
pub(crate) fn parse_listings(json: &str) -> Result<(Vec<Listing>, u32), String> {
    let unexpected = || "Steam did not send your listings.".to_string();
    let value: Value = serde_json::from_str(json).map_err(|_| unexpected())?;
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(unexpected());
    }
    let found = [("listings", false), ("listings_to_confirm", true), ("listings_on_hold", false)]
        .iter()
        .flat_map(|(key, waiting)| {
            let entries = value.get(*key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            entries.iter().filter_map(|entry| listing(entry, &value, *waiting))
        })
        .collect();
    Ok((found, value.get("total_count").and_then(Value::as_u64).unwrap_or(0) as u32))
}
/// Everything the owner has for sale. The lists awaiting confirmation come back on every page.
pub(crate) async fn read_listings<C: Community>(api: &C) -> Result<Vec<Listing>, CommunityError> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut start = 0;
    loop {
        let url = format!("{COMMUNITY}/market/mylistings/?norender=1&start={start}&count={LISTINGS_PER_PAGE}");
        let (page, total) = parse_listings(&api.get(&url).await?).map_err(CommunityError::Other)?;
        let before = found.len();
        found.extend(page.into_iter().filter(|listing| seen.insert(listing.listing_id.clone())));
        start += LISTINGS_PER_PAGE;
        if start >= total || found.len() == before {
            return Ok(found);
        }
    }
}
pub(crate) async fn remove_listing<C: Community>(api: &C, session: &Session, listing_id: &str) -> Result<(), CommunityError> {
    if !is_id(listing_id) {
        return Err(CommunityError::Other("Not a listing.".to_string()));
    }
    let form = [("sessionid", session.session_id.clone())];
    api.post(&format!("{COMMUNITY}/market/removelisting/{listing_id}"), &form, MARKET_PAGE).await?;
    Ok(())
}
/// Lists up to twenty duplicates, and says what it managed even when it ended early. An asset goes
/// into `already_listed` before it is posted, so a failure partway can never list it twice. One
/// card Steam answers badly about is skipped; only being signed out or limited ends the run.
pub(crate) async fn sell_duplicates<C: Community>(
    api: &C,
    session: &Session,
    minimum: u32,
    locked: &HashSet<String>,
    already_listed: &mut HashSet<String>,
    keep_going: &(dyn Fn() -> bool + Send + Sync),
) -> (SellReport, Option<CommunityError>) {
    let cards = match read_pages(api, session.steam_id, parse_inventory).await {
        Ok(cards) => cards,
        Err(error) => return (SellReport::default(), Some(error)),
    };
    let to_sell: Vec<Card> = duplicates(&cards)
        .into_iter()
        .filter(|card| !already_listed.contains(&card.asset_id) && !locked.contains(&card.asset_id))
        .take(MAX_LISTINGS)
        .collect();
    let mut prices: HashMap<String, Option<u32>> = HashMap::new();
    let mut report = SellReport::default();
    for card in to_sell {
        if !keep_going() {
            break;
        }
        let price = match prices.get(&card.hash_name) {
            Some(known) => *known,
            None => match lowest_price(api, &card.hash_name).await {
                Ok(found) => {
                    prices.insert(card.hash_name.clone(), found);
                    found
                }
                Err(CommunityError::Other(_)) => continue,
                Err(error) => return (report, Some(error)),
            },
        };
        let Some(lowest) = price else {
            continue;
        };
        let seller = seller_gets(list_price(lowest, minimum));
        if seller == 0 {
            continue;
        }
        already_listed.insert(card.asset_id.clone());
        let answer = match post_listing(api, session, &card.asset_id, seller).await {
            Ok(answer) => answer,
            Err(CommunityError::Other(_)) => continue,
            Err(error) => return (report, Some(error)),
        };
        if !was_listed(&answer) {
            continue;
        }
        report.listed += 1;
        report.cents += seller;
        if awaits_confirmation(&answer) {
            report.waiting_confirmation += 1;
        }
    }
    (report, None)
}
#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use serde_json::json;
    use super::*;
    use crate::steam_automation::community::FakeCommunity;
    use crate::steam_automation::session::from_cookies;
    const INVENTORY: &str = include_str!("fixtures/inventory.json");
    fn nothing_locked() -> HashSet<String> {
        HashSet::new()
    }
    async fn sold(api: &FakeCommunity, listed: &mut HashSet<String>) -> SellReport {
        let (report, error) = sell_duplicates(api, &session(), 5, &nothing_locked(), listed, &|| true).await;
        assert_eq!(error, None);
        report
    }
    fn described(tags: serde_json::Value) -> String {
        json!({
            "assets": [{ "assetid": "1", "classid": "1", "instanceid": "0", "amount": "3" }],
            "descriptions": [{ "classid": "1", "instanceid": "0", "marketable": 1, "name": "Wheatley", "type": "Portal 2 Trading Card", "market_hash_name": "620-Wheatley", "icon_url": "abc", "tags": tags }],
            "success": 1
        }).to_string()
    }
    fn tag(category: &str, internal: &str, shown: &str) -> serde_json::Value {
        json!({ "category": category, "internal_name": internal, "localized_tag_name": shown })
    }
    fn kind_with(class: &str) -> &'static str {
        parse_items(&described(json!([tag("item_class", class, "x")]))).unwrap().0[0].kind
    }
    #[test]
    fn an_item_says_what_it_is() {
        let tags = json!([tag("Game", "app_620", "Portal 2"), tag("item_class", "item_class_2", "Trading Card"), tag("cardborder", "cardborder_0", "Normal")]);
        let (items, more) = parse_items(&described(tags)).unwrap();
        assert_eq!(more, None);
        assert_eq!(items, vec![InventoryItem {
            asset_id: "1".to_string(),
            hash_name: "620-Wheatley".to_string(),
            name: "Wheatley".to_string(),
            game: "Portal 2".to_string(),
            kind: "card",
            icon_url: Some("https://community.fastly.steamstatic.com/economy/image/abc/184x184".to_string()),
            marketable: true,
            amount: 3,
        }]);
    }
    #[test]
    fn a_foil_card_is_told_apart() {
        let tags = json!([tag("item_class", "item_class_2", "Trading Card"), tag("cardborder", "cardborder_1", "Foil")]);
        assert_eq!(parse_items(&described(tags)).unwrap().0[0].kind, "foil");
    }
    #[test]
    fn each_kind_of_item_is_recognised() {
        assert_eq!(kind_with("item_class_3"), "background");
        assert_eq!(kind_with("item_class_4"), "emoticon");
        assert_eq!(kind_with("item_class_5"), "booster");
        assert_eq!(kind_with("item_class_7"), "other");
    }
    #[test]
    fn with_no_game_tag_the_type_is_shown() {
        assert_eq!(parse_items(&described(json!([]))).unwrap().0[0].game, "Portal 2 Trading Card");
    }
    #[test]
    fn items_that_cannot_be_sold_are_still_listed() {
        let (items, _) = parse_items(INVENTORY).unwrap();
        assert_eq!(items.len(), 10);
        assert!(!items.iter().find(|item| item.asset_id == "1008").unwrap().marketable);
    }
    #[tokio::test]
    async fn a_locked_card_is_never_sold_automatically() {
        let api = market(3);
        let locked = HashSet::from(["2".to_string()]);
        sell_duplicates(&api, &session(), 5, &locked, &mut HashSet::new(), &|| true).await;
        let sold: Vec<String> = api.posts().iter().map(|(_, form, _)| form.iter().find(|(k, _)| k == "assetid").unwrap().1.clone()).collect();
        assert_eq!(sold, vec!["3".to_string()]);
    }
    fn request(buyer_cents: Option<u32>) -> ListRequest {
        ListRequest { asset_id: "7".to_string(), hash_name: "620-Card".to_string(), buyer_cents }
    }
    #[tokio::test]
    async fn a_typed_price_is_listed_for_what_the_seller_gets() {
        let api = market(1);
        let outcome = list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut HashSet::new()).await.unwrap();
        assert_eq!((outcome.listed, outcome.buyer_cents, outcome.waiting_confirmation), (true, 31, true));
        let (url, form, referer) = api.posts()[0].clone();
        let value = |key: &str| form.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(url, "https://steamcommunity.com/market/sellitem/");
        assert_eq!((value("price"), value("assetid"), value("sessionid")), ("28".to_string(), "7".to_string(), "sid".to_string()));
        assert_eq!(referer, "https://steamcommunity.com/profiles/76561198000000001/inventory/");
        assert!(api.gets().is_empty(), "no lookup is needed");
    }
    #[tokio::test]
    async fn the_last_copy_can_be_listed_by_hand() {
        let api = market(1);
        assert!(list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut HashSet::new()).await.unwrap().listed);
    }
    #[tokio::test]
    async fn with_no_typed_price_the_market_sets_it() {
        let api = market(1);
        let outcome = list_item(&api, &session(), &request(None), 5, &nothing_locked(), &mut HashSet::new()).await.unwrap();
        assert_eq!((outcome.listed, outcome.buyer_cents), (true, 30));
    }
    #[tokio::test]
    async fn an_item_with_no_market_price_is_left_alone() {
        let api = FakeCommunity::answering(|_| Ok(r#"{"success":true}"#.to_string()));
        let outcome = list_item(&api, &session(), &request(None), 5, &nothing_locked(), &mut HashSet::new()).await.unwrap();
        assert_eq!((outcome.listed, outcome.reason.as_deref()), (false, Some("No market price")));
        assert!(api.posts().is_empty());
    }
    #[tokio::test]
    async fn a_locked_item_is_refused_by_hand_too() {
        let api = market(1);
        let locked = HashSet::from(["7".to_string()]);
        let outcome = list_item(&api, &session(), &request(Some(31)), 5, &locked, &mut HashSet::new()).await.unwrap();
        assert_eq!((outcome.listed, outcome.reason.as_deref()), (false, Some("Locked")));
        assert!(api.posts().is_empty());
    }
    #[tokio::test]
    async fn an_item_listed_this_session_is_not_listed_twice() {
        let api = market(1);
        let mut listed = HashSet::new();
        list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut listed).await.unwrap();
        let again = list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut listed).await.unwrap();
        assert_eq!((again.listed, again.reason.as_deref()), (false, Some("Already listed")));
        assert_eq!(api.posts().len(), 1);
    }
    #[tokio::test]
    async fn too_low_a_price_is_refused_before_asking_steam() {
        let api = market(1);
        let outcome = list_item(&api, &session(), &request(Some(2)), 5, &nothing_locked(), &mut HashSet::new()).await.unwrap();
        assert_eq!((outcome.listed, outcome.reason.as_deref()), (false, Some("Price is too low")));
        assert!(api.posts().is_empty());
    }
    #[tokio::test]
    async fn steams_refusal_is_passed_on_and_can_be_tried_again() {
        let api = market(1).answering_posts(|_| Ok(r#"{"success":false,"message":"Not today"}"#.to_string()));
        let mut listed = HashSet::new();
        let outcome = list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut listed).await.unwrap();
        assert_eq!((outcome.listed, outcome.reason.as_deref()), (false, Some("Not today")));
        assert!(listed.is_empty());
    }
    #[tokio::test]
    async fn an_answer_that_never_came_stays_marked_as_listed() {
        let api = market(1).answering_posts(|_| Err(CommunityError::SignedOut));
        let mut listed = HashSet::new();
        assert_eq!(list_item(&api, &session(), &request(Some(31)), 5, &nothing_locked(), &mut listed).await, Err(CommunityError::SignedOut));
        assert!(listed.contains("7"), "it may have gone through, so it must not be sent twice");
    }
    #[tokio::test]
    async fn a_strange_asset_id_never_reaches_steam() {
        let api = market(1);
        let bad = ListRequest { asset_id: "7&price=1".to_string(), hash_name: "x".to_string(), buyer_cents: Some(31) };
        let outcome = list_item(&api, &session(), &bad, 5, &nothing_locked(), &mut HashSet::new()).await.unwrap();
        assert!(!outcome.listed);
        assert!(api.posts().is_empty());
    }
    const LISTINGS: &str = r#"{"success":true,"total_count":2,"start":0,"pagesize":100,"num_active_listings":1,
        "assets":{"753":{"6":{"55":{"name":"From assets","market_hash_name":"620-A","icon_url":"def"}}}},
        "listings":[{"listingid":"900","price":28,"fee":3,"asset":{"appid":753,"contextid":"6","id":"55"}}],
        "listings_to_confirm":[{"listingid":"901","price":10,"fee":2,"asset":{"appid":753,"contextid":"6","id":"56","name":"On the listing","market_hash_name":"620-B"}}],
        "listings_on_hold":[]}"#;
    #[test]
    fn listings_are_read_with_their_details() {
        let (found, total) = parse_listings(LISTINGS).unwrap();
        assert_eq!(total, 2);
        assert_eq!(found[0], Listing { listing_id: "900".to_string(), asset_id: "55".to_string(), hash_name: "620-A".to_string(), name: "From assets".to_string(), icon_url: Some("https://community.fastly.steamstatic.com/economy/image/def/184x184".to_string()), buyer_cents: 31, waiting_confirmation: false });
        assert_eq!((found[1].name.as_str(), found[1].buyer_cents, found[1].waiting_confirmation), ("On the listing", 12, true));
    }
    #[test]
    fn an_answer_that_is_not_listings_is_an_error() {
        assert!(parse_listings("<html>").is_err());
        assert!(parse_listings(r#"{"success":false}"#).is_err());
    }
    #[tokio::test]
    async fn every_page_of_listings_is_read_once() {
        let api = FakeCommunity::answering(|url| {
            let start = if url.contains("start=100") { 100 } else { 0 };
            let listings: Vec<_> = (0..100).map(|i| json!({ "listingid": (start + i).to_string(), "price": 1, "fee": 2 })).take(if start == 0 { 100 } else { 20 }).collect();
            Ok(json!({ "success": true, "total_count": 120, "listings": listings, "listings_to_confirm": [{ "listingid": "5000", "price": 1, "fee": 2 }] }).to_string())
        });
        let found = read_listings(&api).await.unwrap();
        assert_eq!(found.len(), 121);
        assert_eq!(api.gets().len(), 2);
    }
    #[tokio::test]
    async fn removing_a_listing_sends_the_session() {
        let api = market(1);
        remove_listing(&api, &session(), "900").await.unwrap();
        let (url, form, referer) = api.posts()[0].clone();
        assert_eq!(url, "https://steamcommunity.com/market/removelisting/900");
        assert_eq!(form, vec![("sessionid".to_string(), "sid".to_string())]);
        assert_eq!(referer, "https://steamcommunity.com/market/");
    }
    #[tokio::test]
    async fn a_strange_listing_id_never_reaches_steam() {
        let api = market(1);
        assert!(remove_listing(&api, &session(), "../../x").await.is_err());
        assert!(api.posts().is_empty());
    }
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
        let report = sold(&api, &mut HashSet::new()).await;
        assert_eq!((api.posts().len(), report.listed), (20, 20));
    }
    #[tokio::test]
    async fn one_price_lookup_serves_every_copy_of_a_card() {
        let api = market(4);
        sold(&api, &mut HashSet::new()).await;
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
        let report = sold(&api, &mut HashSet::new()).await;
        assert!(api.posts().is_empty());
        assert_eq!(report.listed, 0);
    }
    #[tokio::test]
    async fn an_asset_is_never_listed_twice() {
        let api = market(3);
        let mut listed = HashSet::new();
        sold(&api, &mut listed).await;
        assert_eq!(api.posts().len(), 2);
        sold(&api, &mut listed).await;
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
        let (report, error) = sell_duplicates(&api, &session(), 5, &nothing_locked(), &mut HashSet::new(), &|| true).await;
        assert_eq!(error, Some(CommunityError::SignedOut));
        assert_eq!(report.listed, 1, "what was already listed is not forgotten");
        assert_eq!(api.posts().len(), 2);
    }
    /// One duplicate each of cards A and B.
    fn two_cards() -> String {
        json!({
            "assets": [{ "assetid": "1", "classid": "1", "instanceid": "0" }, { "assetid": "2", "classid": "1", "instanceid": "0" }, { "assetid": "3", "classid": "2", "instanceid": "0" }, { "assetid": "4", "classid": "2", "instanceid": "0" }],
            "descriptions": [
                { "classid": "1", "instanceid": "0", "marketable": 1, "market_hash_name": "A", "tags": [{ "category": "item_class", "internal_name": "item_class_2" }] },
                { "classid": "2", "instanceid": "0", "marketable": 1, "market_hash_name": "B", "tags": [{ "category": "item_class", "internal_name": "item_class_2" }] }
            ],
            "success": 1
        }).to_string()
    }
    #[tokio::test]
    async fn a_card_whose_price_cannot_be_read_does_not_end_the_run() {
        let inventory = two_cards();
        let api = FakeCommunity::answering(move |url| {
            if url.ends_with("market_hash_name=A") {
                return Err(CommunityError::Other("Steam answered 500.".to_string()));
            }
            if url.contains("priceoverview") {
                return Ok(r#"{"success":true,"lowest_price":"0,31€"}"#.to_string());
            }
            Ok(inventory.clone())
        })
        .answering_posts(|_| Ok(r#"{"success":true}"#.to_string()));
        let (report, error) = sell_duplicates(&api, &session(), 5, &nothing_locked(), &mut HashSet::new(), &|| true).await;
        assert_eq!((report.listed, error), (1, None));
    }
    #[tokio::test]
    async fn steam_limiting_requests_ends_the_run() {
        let inventory = two_cards();
        let api = FakeCommunity::answering(move |url| if url.contains("priceoverview") { Err(CommunityError::Limited) } else { Ok(inventory.clone()) });
        let (_, error) = sell_duplicates(&api, &session(), 5, &nothing_locked(), &mut HashSet::new(), &|| true).await;
        assert_eq!(error, Some(CommunityError::Limited));
        assert_eq!(api.gets().iter().filter(|url| url.contains("priceoverview")).count(), 1);
    }
    #[tokio::test]
    async fn a_listing_steam_answered_badly_does_not_end_the_run() {
        let api = market(3).answering_posts(|_| Err(CommunityError::Other("Steam answered 500.".to_string())));
        let (_, error) = sell_duplicates(&api, &session(), 5, &nothing_locked(), &mut HashSet::new(), &|| true).await;
        assert_eq!((api.posts().len(), error), (2, None));
    }
    #[tokio::test]
    async fn a_run_stops_before_the_next_listing_when_asked_to() {
        let api = market(4);
        let asked = std::sync::atomic::AtomicU32::new(0);
        let keep_going = || asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 1;
        let (report, error) = sell_duplicates(&api, &session(), 5, &nothing_locked(), &mut HashSet::new(), &keep_going).await;
        assert_eq!((report.listed, error, api.posts().len()), (1, None, 1));
    }
    #[tokio::test]
    async fn a_refused_listing_is_not_counted_and_the_run_goes_on() {
        let api = market(3).answering_posts(|_| Ok(r#"{"success":false,"message":"no"}"#.to_string()));
        let report = sold(&api, &mut HashSet::new()).await;
        assert_eq!((api.posts().len(), report.listed), (2, 0));
    }
    #[tokio::test]
    async fn the_form_sends_what_the_seller_gets() {
        let api = market(2);
        sold(&api, &mut HashSet::new()).await;
        let (url, form, referer) = api.posts()[0].clone();
        assert_eq!(url, "https://steamcommunity.com/market/sellitem/");
        let value = |key: &str| form.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
        assert_eq!((value("price"), value("sessionid"), value("appid"), value("contextid"), value("amount"), value("assetid")), ("27".to_string(), "sid".to_string(), "753".to_string(), "6".to_string(), "1".to_string(), "2".to_string()));
        assert_eq!(referer, "https://steamcommunity.com/profiles/76561198000000001/inventory/");
    }
    #[tokio::test]
    async fn waiting_listings_are_counted() {
        let api = market(2);
        let report = sold(&api, &mut HashSet::new()).await;
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
        sold(&api, &mut HashSet::new()).await;
        assert_eq!(api.gets().len(), 2);
    }
}
