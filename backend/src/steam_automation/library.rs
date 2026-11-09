//! The owner's games, with playtime and achievement progress. Asks with a token Steam gives the
//! signed-in store session, so no API key is involved.
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use super::achievements::eligible;
use super::community::{Community, CommunityError};
use crate::steam_helper::store::{StoreApi, details};
const FRESH_SECS: u64 = 86_400;
const STEAM_GAME: u64 = 0;
const TOKEN_URL: &str = "https://store.steampowered.com/pointssummary/ajaxgetasyncconfig";
const USERDATA_URL: &str = "https://store.steampowered.com/dynamicstore/userdata/";
const OWNED_GAMES: &str = "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/";
const TOP_ACHIEVEMENTS: &str = "https://api.steampowered.com/IPlayerService/GetTopAchievementsForGames/v1/";
const PLAYED_TIMES: &str = "https://api.steampowered.com/IPlayerService/ClientGetLastPlayedTimes/v1/";
const FAMILY_GROUP: &str = "https://api.steampowered.com/IFamilyGroupsService/GetFamilyGroupForUser/v1/";
const FAMILY_APPS: &str = "https://api.steampowered.com/IFamilyGroupsService/GetSharedLibraryApps/v1/";
const PROGRESS_PER_REQUEST: usize = 200;
const NO_LIBRARY: &str = "Steam did not send your library.";
pub(crate) const SIGNED_OUT: &str = "Your Steam sign-in has expired. Sign in again.";
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGame {
    pub appid: u32,
    pub name: String,
    /// Portrait art where the game has some, otherwise its wide header.
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub minutes: u32,
    pub achievements_unlocked: u32,
    pub achievements_total: u32,
    pub achievements_ok: bool,
    /// Reached through the owner's Steam Family, so it drops no cards.
    pub shared: bool,
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
/// What the page shows: a problem means these games are the last ones that could be read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAnswer {
    pub games: Vec<LibraryGame>,
    pub saved_at: u64,
    pub problem: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnedGame {
    pub appid: u32,
    pub name: String,
    pub minutes: u32,
}
fn json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|_| NO_LIBRARY.to_string())
}
/// The token Steam hands the signed-in store session. It is only ever sent back to Steam.
pub(crate) fn parse_token(text: &str) -> Result<String, String> {
    json(text)?
        .get("data")
        .and_then(|data| data.get("webapi_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| NO_LIBRARY.to_string())
}
/// Everything the account holds a licence for, DLC and tools included.
pub(crate) fn parse_owned(text: &str) -> Result<Vec<u32>, String> {
    let owned: Vec<u32> = json(text)?
        .get("rgOwnedApps")
        .and_then(Value::as_array)
        .ok_or(NO_LIBRARY)?
        .iter()
        .filter_map(|id| u32::try_from(id.as_u64()?).ok())
        .collect();
    if owned.is_empty() {
        return Err(NO_LIBRARY.to_string());
    }
    Ok(owned)
}
pub(crate) fn parse_owned_games(text: &str) -> Result<Vec<OwnedGame>, String> {
    let value = json(text)?;
    let games = value.get("response").and_then(|r| r.get("games")).and_then(Value::as_array).ok_or(NO_LIBRARY)?;
    Ok(games
        .iter()
        .filter_map(|game| {
            Some(OwnedGame {
                appid: u32::try_from(game.get("appid")?.as_u64()?).ok()?,
                name: game.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                minutes: game.get("playtime_forever").and_then(Value::as_u64).unwrap_or(0) as u32,
            })
        })
        .collect())
}
/// Steam leaves out the list of a game with nothing unlocked, and the entry of a game with no
/// achievements at all, so both read as none.
pub(crate) fn apply_progress(games: &mut [LibraryGame], text: &str) -> Result<(), String> {
    let value = json(text)?;
    let entries = value.get("response").ok_or(NO_LIBRARY)?.get("games").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let progress: HashMap<u64, &Value> = entries.iter().filter_map(|entry| Some((entry.get("appid")?.as_u64()?, entry))).collect();
    for game in games {
        if let Some(entry) = progress.get(&u64::from(game.appid)) {
            game.achievements_total = entry.get("total_achievements").and_then(Value::as_u64).unwrap_or(0) as u32;
            game.achievements_unlocked = entry.get("achievements").and_then(Value::as_array).map_or(0, Vec::len) as u32;
        }
    }
    Ok(())
}
/// What the owner's Steam Family shares. A family is an extra, so any trouble here means none.
async fn family_apps<S: StoreApi>(store: &S, token: &str, steam_id: u64) -> Vec<OwnedGame> {
    let group = store
        .get(&format!("{FAMILY_GROUP}?access_token={token}&steamid={steam_id}&include_family_group_response=false"))
        .await
        .ok()
        .and_then(|text| json(&text).ok())
        .and_then(|value| value.get("response")?.get("family_groupid")?.as_str().map(str::to_string));
    let Some(group) = group else {
        return Vec::new();
    };
    let url = format!("{FAMILY_APPS}?access_token={token}&family_groupid={group}&include_own=true&include_excluded=false&include_free=true&include_non_games=false&language=english&steamid={steam_id}");
    let Some(answer) = store.get(&url).await.ok().and_then(|text| json(&text).ok()) else {
        return Vec::new();
    };
    let apps = answer.get("response").and_then(|r| r.get("apps")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    apps.iter()
        .filter_map(|app| {
            Some(OwnedGame {
                appid: u32::try_from(app.get("appid")?.as_u64()?).ok()?,
                name: app.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                minutes: 0,
            })
        })
        .collect()
}
/// Minutes on everything this account has run. Unlike the owned list, it covers family games.
async fn played_times<S: StoreApi>(store: &S, token: &str) -> HashMap<u32, u32> {
    let Some(answer) = store.get(&format!("{PLAYED_TIMES}?access_token={token}&min_last_played=0")).await.ok().and_then(|text| json(&text).ok()) else {
        return HashMap::new();
    };
    let games = answer.get("response").and_then(|r| r.get("games")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    games
        .iter()
        .filter_map(|game| Some((u32::try_from(game.get("appid")?.as_u64()?).ok()?, game.get("playtime_forever")?.as_u64()? as u32)))
        .collect()
}
/// The token is refused when the session behind it has ended.
fn plainly(reason: String) -> String {
    if reason.ends_with("401.") || reason.ends_with("403.") { SIGNED_OUT.to_string() } else { reason }
}
async fn read<C: Community>(community: &C, url: &str) -> Result<String, String> {
    community.get(url).await.map_err(|error| match error {
        CommunityError::SignedOut => SIGNED_OUT.to_string(),
        _ => NO_LIBRARY.to_string(),
    })
}
/// The games among the licences and the Steam Family, joined with what Steam knows of the owner's
/// play. Licences also cover DLC, so only games count there, while anything Steam itself lists as
/// owned or shared is kept. Such a game keeps its name when the store no longer knows it.
pub(crate) async fn fetch<C: Community, S: StoreApi>(community: &C, store: &S, steam_id: u64) -> Result<Vec<LibraryGame>, String> {
    let token = parse_token(&read(community, TOKEN_URL).await?)?;
    let mut licensed = parse_owned(&read(community, USERDATA_URL).await?)?;
    let url = format!("{OWNED_GAMES}?access_token={token}&steamid={steam_id}&include_appinfo=true&include_played_free_games=true&skip_unvetted_apps=false");
    let played = parse_owned_games(&store.get(&url).await.map_err(plainly)?)?;
    let family = family_apps(store, &token, steam_id).await;
    let times = played_times(store, &token).await;
    let own: HashSet<u32> = licensed.iter().copied().chain(played.iter().map(|game| game.appid)).collect();
    let shared: Vec<OwnedGame> = family.into_iter().filter(|game| !own.contains(&game.appid)).collect();
    let listed: HashSet<u32> = played.iter().chain(&shared).map(|game| game.appid).collect();
    licensed.extend(listed.iter().filter(|appid| !licensed.contains(appid)).collect::<Vec<_>>());
    let shared_ids: HashSet<u32> = shared.iter().map(|game| game.appid).collect();
    let minutes: HashMap<u32, u32> = played.iter().map(|game| (game.appid, game.minutes)).collect();
    let minutes_of = |appid: u32| minutes.get(&appid).copied().unwrap_or(0).max(times.get(&appid).copied().unwrap_or(0));
    let items = details(store, &licensed).await?;
    let known: HashSet<u32> = items.iter().map(|item| item.game.appid).collect();
    let mut games: Vec<LibraryGame> = items
        .into_iter()
        .filter(|item| item.item_type == Some(STEAM_GAME) || listed.contains(&item.game.appid))
        .map(|item| {
            let features: Vec<u32> = item.features.iter().filter_map(|id| u32::try_from(*id).ok()).collect();
            LibraryGame {
                appid: item.game.appid,
                cover_url: item.portrait_url.or_else(|| item.game.cover_url.clone()),
                header_url: item.game.cover_url,
                minutes: minutes_of(item.game.appid),
                name: item.game.name,
                achievements_unlocked: 0,
                achievements_total: 0,
                achievements_ok: eligible(&features),
                shared: shared_ids.contains(&item.game.appid),
            }
        })
        .collect();
    games.extend(played.into_iter().chain(shared).filter(|game| !known.contains(&game.appid)).map(|game| LibraryGame {
        minutes: minutes_of(game.appid),
        shared: shared_ids.contains(&game.appid),
        appid: game.appid,
        name: game.name,
        cover_url: None,
        header_url: None,
        achievements_unlocked: 0,
        achievements_total: 0,
        achievements_ok: false,
    }));
    let ids: Vec<u32> = games.iter().map(|game| game.appid).collect();
    for batch in ids.chunks(PROGRESS_PER_REQUEST) {
        let mut url = format!("{TOP_ACHIEVEMENTS}?access_token={token}&steamid={steam_id}&language=english&max_achievements=10000");
        for (index, appid) in batch.iter().enumerate() {
            url.push_str(&format!("&appids[{index}]={appid}"));
        }
        apply_progress(&mut games, &store.get(&url).await.map_err(plainly)?)?;
    }
    games.sort_by_cached_key(|game| game.name.to_lowercase());
    Ok(games)
}
/// What a refresh leaves the page with. One that failed keeps the saved copy and says why.
pub(crate) fn settle(saved: Option<Saved>, fetched: Result<Vec<LibraryGame>, String>, now: u64) -> Result<LibraryAnswer, String> {
    match (fetched, saved) {
        (Ok(games), _) => Ok(LibraryAnswer { games, saved_at: now, problem: None }),
        (Err(problem), Some(saved)) => Ok(LibraryAnswer { games: saved.games, saved_at: saved.saved_at, problem: Some(problem) }),
        (Err(problem), None) => Err(problem),
    }
}
#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use crate::steam_automation::community::{CommunityError, FakeCommunity};
    use crate::steam_helper::store::fake::{FakeStore, answer_with, input_of};
    const ME: u64 = 76561198000000001;
    fn game(appid: u32) -> LibraryGame {
        LibraryGame { appid, name: format!("App {appid}"), cover_url: None, header_url: None, minutes: 0, achievements_unlocked: 0, achievements_total: 0, achievements_ok: false, shared: false }
    }
    #[test]
    fn owned_apps_are_read() {
        assert_eq!(parse_owned(r#"{"rgOwnedApps":[620,730],"rgOwnedPackages":[1]}"#).unwrap(), vec![620, 730]);
    }
    #[test]
    fn an_answer_with_no_apps_is_an_error() {
        assert!(parse_owned(r#"{"rgOwnedApps":[]}"#).is_err());
        assert!(parse_owned("<html>").is_err());
    }
    #[test]
    fn the_web_token_is_read() {
        assert_eq!(parse_token(r#"{"success":1,"data":{"webapi_token":"abc"}}"#).unwrap(), "abc");
    }
    #[test]
    fn no_token_is_an_error() {
        assert!(parse_token(r#"{"success":1,"data":{"webapi_token":""}}"#).is_err());
        assert!(parse_token(r#"{"success":1,"data":{}}"#).is_err());
        assert!(parse_token("<html>").is_err());
    }
    #[test]
    fn owned_games_give_their_name_and_minutes() {
        let json = r#"{"response":{"game_count":2,"games":[{"appid":620,"name":"Portal 2","playtime_forever":90},{"appid":730,"name":"CS2"}]}}"#;
        assert_eq!(
            parse_owned_games(json).unwrap(),
            vec![OwnedGame { appid: 620, name: "Portal 2".to_string(), minutes: 90 }, OwnedGame { appid: 730, name: "CS2".to_string(), minutes: 0 }]
        );
    }
    #[test]
    fn an_answer_with_no_games_is_an_error() {
        assert!(parse_owned_games(r#"{"response":{}}"#).is_err());
        assert!(parse_owned_games("<html>").is_err());
    }
    #[test]
    fn progress_fills_unlocked_and_total() {
        let mut games = [game(620), game(730)];
        let json = r#"{"response":{"games":[{"appid":620,"total_achievements":51,"achievements":[{"statid":1},{"statid":2}]}]}}"#;
        apply_progress(&mut games, json).unwrap();
        assert_eq!((games[0].achievements_unlocked, games[0].achievements_total), (2, 51));
        assert_eq!((games[1].achievements_unlocked, games[1].achievements_total), (0, 0));
    }
    #[test]
    fn a_game_with_none_unlocked_still_has_its_total() {
        let mut games = [game(620)];
        apply_progress(&mut games, r#"{"response":{"games":[{"appid":620,"total_achievements":51}]}}"#).unwrap();
        assert_eq!((games[0].achievements_unlocked, games[0].achievements_total), (0, 51));
    }
    #[test]
    fn progress_that_is_not_steams_is_an_error() {
        assert!(apply_progress(&mut [game(620)], "<html>").is_err());
    }
    fn community() -> FakeCommunity {
        FakeCommunity::answering(|url| {
            if url.contains("ajaxgetasyncconfig") {
                return Ok(r#"{"data":{"webapi_token":"tok"}}"#.to_string());
            }
            Ok(r#"{"rgOwnedApps":[620,999,239140]}"#.to_string())
        })
    }
    /// 620 is a game with cards, 239140 has none played, 999 is a DLC, 5 is played but gone from the store.
    fn store() -> FakeStore {
        FakeStore::responding(|url| {
            if url.contains("IFamilyGroupsService") || url.contains("ClientGetLastPlayedTimes") {
                return Err("not in a family".to_string());
            }
            if url.contains("GetOwnedGames") {
                return Ok(json!({ "response": { "games": [{ "appid": 620, "name": "Owned name", "playtime_forever": 90 }, { "appid": 5, "name": "Delisted", "playtime_forever": 7 }] } }).to_string());
            }
            if url.contains("GetTopAchievementsForGames") {
                return Ok(json!({ "response": { "games": [{ "appid": 620, "total_achievements": 51, "achievements": [{ "statid": 1 }] }] } }).to_string());
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().filter(|id| id["appid"] != 5).map(|id| {
                let appid = id["appid"].as_u64().unwrap();
                let (kind, features, name) = match appid {
                    620 => (0, vec![22, 29], "Portal 2"),
                    239140 => (0, vec![22, 8], "Dying Light"),
                    _ => (4, vec![], "A DLC"),
                };
                json!({ "appid": appid, "name": name, "type": kind, "categories": { "feature_categoryids": features }, "assets": { "header": "header.jpg", "library_capsule": "library_600x900.jpg" } })
            }).collect();
            Ok(answer_with(games))
        })
    }
    #[tokio::test]
    async fn the_library_joins_licences_playtime_and_progress_by_name() {
        let games = fetch(&community(), &store(), ME).await.unwrap();
        assert_eq!(games.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Delisted", "Dying Light", "Portal 2"]);
        let portal = &games[2];
        assert_eq!((portal.minutes, portal.achievements_unlocked, portal.achievements_total, portal.achievements_ok), (90, 1, 51, true));
        assert_eq!(games[1].minutes, 0);
        assert!(!games[1].achievements_ok, "a VAC game is not offered");
    }
    #[tokio::test]
    async fn portrait_art_is_preferred_with_the_header_kept() {
        let games = fetch(&community(), &store(), ME).await.unwrap();
        assert!(games[2].cover_url.as_deref().unwrap().ends_with("/620/library_600x900.jpg"));
        assert!(games[2].header_url.as_deref().unwrap().ends_with("/620/header.jpg"));
    }
    #[tokio::test]
    async fn a_game_gone_from_the_store_keeps_its_name_and_playtime() {
        let games = fetch(&community(), &store(), ME).await.unwrap();
        assert_eq!((games[0].appid, games[0].minutes, games[0].cover_url.clone()), (5, 7, None));
    }
    #[tokio::test]
    async fn the_token_goes_only_to_the_player_api() {
        let store = store();
        fetch(&community(), &store, ME).await.unwrap();
        let calls = store.calls();
        let owned = calls.iter().find(|url| url.contains("GetOwnedGames")).unwrap();
        assert!(owned.contains("access_token=tok") && owned.contains(&format!("steamid={ME}")) && owned.contains("skip_unvetted_apps=false"));
        assert!(calls.iter().filter(|url| url.contains("GetItems")).all(|url| !url.contains("tok")));
    }
    #[tokio::test]
    async fn progress_is_asked_for_two_hundred_games_at_a_time() {
        let apps: Vec<u32> = (10_000..10_250).collect();
        let owned = json!({ "rgOwnedApps": apps }).to_string();
        let community = FakeCommunity::answering(move |url| {
            if url.contains("ajaxgetasyncconfig") {
                return Ok(r#"{"data":{"webapi_token":"tok"}}"#.to_string());
            }
            Ok(owned.clone())
        });
        let store = FakeStore::responding(|url| {
            if url.contains("IFamilyGroupsService") || url.contains("ClientGetLastPlayedTimes") {
                return Err("not in a family".to_string());
            }
            if url.contains("GetOwnedGames") {
                return Ok(json!({ "response": { "games": [] } }).to_string());
            }
            if url.contains("GetTopAchievementsForGames") {
                return Ok(json!({ "response": { "games": [] } }).to_string());
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| json!({ "appid": id["appid"], "name": "G", "type": 0 })).collect();
            Ok(answer_with(games))
        });
        assert_eq!(fetch(&community, &store, ME).await.unwrap().len(), 250);
        let progress: Vec<String> = store.calls().into_iter().filter(|url| url.contains("GetTopAchievementsForGames")).collect();
        assert_eq!(progress.len(), 2);
        assert!(progress[0].contains("appids[199]=") && !progress[0].contains("appids[200]="));
    }
    /// The owner has 620, and the family adds 77 and shares 620 back.
    fn family_store() -> FakeStore {
        FakeStore::responding(|url| {
            if url.contains("GetFamilyGroupForUser") {
                return Ok(json!({ "response": { "family_groupid": "42", "is_not_member_of_any_group": false } }).to_string());
            }
            if url.contains("GetSharedLibraryApps") {
                return Ok(json!({ "response": { "apps": [{ "appid": 620, "name": "Portal 2" }, { "appid": 77, "name": "Family game" }] } }).to_string());
            }
            if url.contains("ClientGetLastPlayedTimes") {
                return Ok(json!({ "response": { "games": [{ "appid": 77, "playtime_forever": 33 }, { "appid": 620, "playtime_forever": 1 }] } }).to_string());
            }
            if url.contains("GetOwnedGames") {
                return Ok(json!({ "response": { "games": [{ "appid": 620, "name": "Portal 2", "playtime_forever": 90 }] } }).to_string());
            }
            if url.contains("GetTopAchievementsForGames") {
                return Ok(json!({ "response": { "games": [] } }).to_string());
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| {
                let appid = id["appid"].as_u64().unwrap();
                json!({ "appid": appid, "name": if appid == 77 { "Family game" } else { "Portal 2" }, "type": if appid == 999 { 4 } else { 0 } })
            }).collect();
            Ok(answer_with(games))
        })
    }
    #[tokio::test]
    async fn family_games_are_listed_and_marked() {
        let games = fetch(&community(), &family_store(), ME).await.unwrap();
        let family = games.iter().find(|g| g.appid == 77).unwrap();
        assert!(family.shared);
        assert!(!games.iter().find(|g| g.appid == 620).unwrap().shared, "the owner's own copy is not a shared one");
    }
    #[tokio::test]
    async fn playtime_on_family_games_is_found_without_lowering_the_rest() {
        let games = fetch(&community(), &family_store(), ME).await.unwrap();
        assert_eq!(games.iter().find(|g| g.appid == 77).unwrap().minutes, 33);
        assert_eq!(games.iter().find(|g| g.appid == 620).unwrap().minutes, 90);
    }
    #[tokio::test]
    async fn a_game_steam_counts_as_owned_is_kept_whatever_its_type() {
        let store = FakeStore::responding(|url| {
            if url.contains("IFamilyGroupsService") || url.contains("ClientGetLastPlayedTimes") {
                return Err("no".to_string());
            }
            if url.contains("GetOwnedGames") {
                return Ok(json!({ "response": { "games": [{ "appid": 620, "name": "A tool", "playtime_forever": 5 }] } }).to_string());
            }
            if url.contains("GetTopAchievementsForGames") {
                return Ok(json!({ "response": { "games": [] } }).to_string());
            }
            let games = input_of(url)["ids"].as_array().unwrap().iter().map(|id| json!({ "appid": id["appid"], "name": "An app", "type": 6 })).collect();
            Ok(answer_with(games))
        });
        let ids: Vec<u32> = fetch(&community(), &store, ME).await.unwrap().iter().map(|g| g.appid).collect();
        assert_eq!(ids, vec![620]);
    }
    #[tokio::test]
    async fn an_expired_session_says_to_sign_in() {
        let community = FakeCommunity::answering(|_| Err(CommunityError::SignedOut));
        assert_eq!(fetch(&community, &store(), ME).await.unwrap_err(), "Your Steam sign-in has expired. Sign in again.");
    }
    #[tokio::test]
    async fn a_refused_token_says_to_sign_in() {
        let store = FakeStore::responding(|_| Err("Steam answered 401.".to_string()));
        assert_eq!(fetch(&community(), &store, ME).await.unwrap_err(), SIGNED_OUT);
    }
    #[test]
    fn a_refresh_replaces_what_was_saved() {
        let saved = Saved { saved_at: 1, games: vec![game(1)] };
        let answer = settle(Some(saved), Ok(vec![game(2)]), 50).unwrap();
        assert_eq!(answer, LibraryAnswer { games: vec![game(2)], saved_at: 50, problem: None });
    }
    #[test]
    fn a_failed_refresh_keeps_the_saved_copy_and_says_why() {
        let saved = Saved { saved_at: 1, games: vec![game(1)] };
        let answer = settle(Some(saved), Err("Steam said no.".to_string()), 50).unwrap();
        assert_eq!(answer, LibraryAnswer { games: vec![game(1)], saved_at: 1, problem: Some("Steam said no.".to_string()) });
    }
    #[test]
    fn a_failed_first_read_is_an_error() {
        assert_eq!(settle(None, Err("Steam said no.".to_string()), 50), Err("Steam said no.".to_string()));
    }
    #[test]
    fn a_saved_library_is_good_for_a_day() {
        let saved = Saved { saved_at: 1000, games: vec![] };
        assert!(saved.is_fresh(1000 + 86_399));
        assert!(!saved.is_fresh(1000 + 86_400));
    }
    #[test]
    fn a_copy_saved_by_the_older_version_is_not_used() {
        let old = r#"{"saved_at":1,"games":[{"appid":620,"name":"Portal 2","coverUrl":null,"achievementsOk":true}]}"#;
        assert!(serde_json::from_str::<Saved>(old).is_err());
    }
}
