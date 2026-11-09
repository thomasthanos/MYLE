//! Adding a game that is free to keep to the owner's account, the way the store's own button does.
use serde::Serialize;
use super::community::{Community, CommunityError};
use super::library::SIGNED_OUT;
use super::session::Session;
use crate::steam_helper::store::{StoreApi, details};
const ADD_URL: &str = "https://store.steampowered.com/freelicense/addfreelicense";
const SUCCESS_MARK: &str = "<h2>Success!</h2>";
const ERROR_OPEN: &str = r#"<span class="error">"#;
const NOT_FREE: &str = "This game is no longer free to keep.";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Claimed {
    Added,
    AlreadyOwned,
}
/// Steam answers with a page: a success heading, or a line saying what went wrong.
pub(crate) fn read_answer(html: &str) -> Result<Claimed, String> {
    if html.contains(SUCCESS_MARK) {
        return Ok(Claimed::Added);
    }
    let reason = html.split_once(ERROR_OPEN).and_then(|(_, rest)| rest.split_once("</span>")).map(|(reason, _)| reason.trim());
    match reason {
        Some(reason) if reason.to_lowercase().contains("already") => Ok(Claimed::AlreadyOwned),
        Some(reason) => Err(reason.to_string()),
        None => Err("Steam did not confirm that the game was added.".to_string()),
    }
}
/// The store is asked again first, so a game saved as free an hour ago is not assumed to still be.
pub(crate) async fn claim<C: Community, S: StoreApi>(community: &C, store: &S, session: &Session, appid: u32) -> Result<Claimed, String> {
    let item = details(store, &[appid]).await?.into_iter().next().ok_or(NOT_FREE)?;
    if item.game.discount_percent != 100 {
        return Err(NOT_FREE.to_string());
    }
    let package = item.package_id.ok_or(NOT_FREE)?;
    let session_id = session.store_session_id().ok_or(SIGNED_OUT)?.to_string();
    let form = [("action", "add_to_cart".to_string()), ("sessionid", session_id), ("subid", package.to_string())];
    let page = format!("https://store.steampowered.com/app/{appid}");
    let answer = community.post(ADD_URL, &form, &page).await.map_err(|error| match error {
        CommunityError::SignedOut => SIGNED_OUT.to_string(),
        CommunityError::Limited => "Steam is limiting requests right now. Try again in a minute.".to_string(),
        CommunityError::Other(reason) => reason,
    })?;
    read_answer(&answer)
}
#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use crate::steam_automation::community::FakeCommunity;
    use crate::steam_automation::session::from_cookies;
    use crate::steam_helper::store::fake::{FakeStore, answer_with};
    fn session() -> Session {
        let pairs = [("steamLoginSecure".to_string(), "76561198000000001%7C%7Ct".to_string()), ("sessionid".to_string(), "community".to_string())];
        let store = [("steamLoginSecure".to_string(), "76561198000000001%7C%7Cs".to_string()), ("sessionid".to_string(), "storeid".to_string())];
        from_cookies(&pairs).unwrap().with_store_cookies(&store)
    }
    fn store(discount: u32) -> FakeStore {
        FakeStore::responding(move |_| Ok(answer_with(vec![json!({ "appid": 620, "name": "Portal 2", "type": 0, "best_purchase_option": { "packageid": 7000, "discount_pct": discount } })])))
    }
    fn answering(html: &'static str) -> FakeCommunity {
        FakeCommunity::answering(|_| Ok(String::new())).answering_posts(move |_| Ok(html.to_string()))
    }
    const ADDED: &str = "<html><h2>Success!</h2></html>";
    #[test]
    fn steams_success_page_means_added() {
        assert_eq!(read_answer(ADDED), Ok(Claimed::Added));
    }
    #[test]
    fn already_owning_it_counts_as_done() {
        assert_eq!(read_answer(r#"<span class="error">You already own this product.</span>"#), Ok(Claimed::AlreadyOwned));
    }
    #[test]
    fn steams_reason_is_passed_on() {
        assert_eq!(read_answer(r#"<span class="error"> Not available in your country. </span>"#), Err("Not available in your country.".to_string()));
    }
    #[test]
    fn an_answer_with_neither_mark_is_not_taken_for_success() {
        assert!(read_answer("<html>something else</html>").is_err());
    }
    #[tokio::test]
    async fn a_free_game_is_added_by_its_package_with_the_stores_session() {
        let api = answering(ADDED);
        assert_eq!(claim(&api, &store(100), &session(), 620).await, Ok(Claimed::Added));
        let (url, form, referer) = api.posts()[0].clone();
        let value = |key: &str| form.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(url, "https://store.steampowered.com/freelicense/addfreelicense");
        assert_eq!((value("subid"), value("sessionid"), value("action")), ("7000".to_string(), "storeid".to_string(), "add_to_cart".to_string()));
        assert_eq!(referer, "https://store.steampowered.com/app/620");
    }
    #[tokio::test]
    async fn a_game_no_longer_free_is_refused_without_asking_for_it() {
        let api = answering(ADDED);
        assert!(claim(&api, &store(90), &session(), 620).await.is_err());
        assert!(api.posts().is_empty());
    }
    #[tokio::test]
    async fn no_store_sign_in_is_said_plainly() {
        let pairs = [("steamLoginSecure".to_string(), "76561198000000001%7C%7Ct".to_string()), ("sessionid".to_string(), "x".to_string())];
        let api = answering(ADDED);
        assert!(claim(&api, &store(100), &from_cookies(&pairs).unwrap(), 620).await.unwrap_err().contains("Sign in"));
        assert!(api.posts().is_empty());
    }
    #[tokio::test]
    async fn an_expired_session_says_to_sign_in() {
        let api = FakeCommunity::answering(|_| Ok(String::new())).answering_posts(|_| Err(CommunityError::SignedOut));
        assert!(claim(&api, &store(100), &session(), 620).await.unwrap_err().contains("Sign in"));
    }
}
