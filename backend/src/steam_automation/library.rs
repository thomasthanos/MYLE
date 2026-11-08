//! The owner's games, for choosing which to idle and which to unlock.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use super::achievements::eligible;
use crate::steam_helper::store::{StoreApi, details};
const FRESH_SECS: u64 = 86_400;
const STEAM_GAME: u64 = 0;
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGame {
    pub appid: u32,
    pub name: String,
    pub cover_url: Option<String>,
    pub achievements_ok: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Saved {
    pub saved_at: u64,
    pub games: Vec<LibraryGame>,
}
impl Saved {
    pub(crate) fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.saved_at) < FRESH_SECS
    }
}
pub(crate) fn parse_owned(json: &str) -> Result<Vec<u32>, String> {
    let unexpected = || "Steam did not send your library.".to_string();
    let value: Value = serde_json::from_str(json).map_err(|_| unexpected())?;
    let owned: Vec<u32> = value
        .get("rgOwnedApps")
        .and_then(Value::as_array)
        .ok_or_else(unexpected)?
        .iter()
        .filter_map(|id| u32::try_from(id.as_u64()?).ok())
        .collect();
    if owned.is_empty() {
        return Err(unexpected());
    }
    Ok(owned)
}
/// The games among these apps, by name. Steam also lists DLC, tools and the like.
pub(crate) async fn from_owned<S: StoreApi>(store: &S, owned: &[u32]) -> Result<Vec<LibraryGame>, String> {
    let mut games: Vec<LibraryGame> = details(store, owned)
        .await?
        .into_iter()
        .filter(|item| item.item_type == Some(STEAM_GAME))
        .map(|item| {
            let features: Vec<u32> = item.features.iter().filter_map(|id| u32::try_from(*id).ok()).collect();
            LibraryGame { appid: item.game.appid, name: item.game.name, cover_url: item.game.cover_url, achievements_ok: eligible(&features) }
        })
        .collect();
    games.sort_by_cached_key(|game| game.name.to_lowercase());
    Ok(games)
}
#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use crate::steam_helper::store::fake::{FakeStore, answer_with, input_of};
    #[test]
    fn owned_apps_are_read() {
        assert_eq!(parse_owned(r#"{"rgOwnedApps":[620,730],"rgOwnedPackages":[1]}"#).unwrap(), vec![620, 730]);
    }
    #[test]
    fn an_answer_with_no_apps_is_an_error() {
        assert!(parse_owned(r#"{"rgOwnedApps":[]}"#).is_err());
        assert!(parse_owned("<html>").is_err());
    }
    fn store() -> FakeStore {
        FakeStore::responding(|url| {
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| {
                let appid = id["appid"].as_u64().unwrap();
                let (kind, features) = match appid {
                    620 => (0, vec![22, 29]),
                    239140 => (0, vec![22, 8]),
                    999 => (4, vec![]),
                    _ => (0, vec![29]),
                };
                json!({ "appid": appid, "name": format!("App {appid}"), "type": kind, "categories": { "feature_categoryids": features } })
            }).collect();
            Ok(answer_with(games))
        })
    }
    #[tokio::test]
    async fn only_games_are_listed_with_what_they_can_do() {
        let games = from_owned(&store(), &[620, 239140, 999, 5]).await.unwrap();
        assert_eq!(games.iter().map(|g| (g.appid, g.achievements_ok)).collect::<Vec<_>>(), vec![(239140, false), (5, false), (620, true)]);
    }
    #[test]
    fn a_saved_library_is_good_for_a_day() {
        let saved = Saved { saved_at: 1000, games: vec![] };
        assert!(saved.is_fresh(1000 + 86_399));
        assert!(!saved.is_fresh(1000 + 86_400));
    }
}
