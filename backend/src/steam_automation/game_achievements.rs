//! One game's achievements for the page: the game's own list from the helper (every achievement in
//! the game's order, and which are unlocked) with Steam's public descriptions, icons and the share
//! of players who unlocked each. The helper is what counts: Steam's public list only adds words and
//! pictures, and an achievement it does not describe still shows, under its own name.
use std::collections::{HashMap, HashSet};
use serde::Serialize;
use serde_json::Value;
use super::achievements::AchievementList;
const DETAILS: &str = "https://api.steampowered.com/IPlayerService/GetGameAchievements/v1/";
const ICONS: &str = "https://cdn.fastly.steamstatic.com/steamcommunity/public/images/apps";
/// What Steam's public list says about one achievement, in English.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Detail {
    pub name: String,
    pub title: String,
    pub description: String,
    /// A picture's file name in the game's folder.
    pub icon: Option<String>,
    pub icon_gray: Option<String>,
    pub hidden: bool,
    /// The share of players who have it, 0 to 100.
    pub percent: Option<f32>,
}
/// One achievement as the page shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Achievement {
    pub name: String,
    pub title: String,
    pub description: String,
    pub icon: Option<String>,
    pub icon_gray: Option<String>,
    pub hidden: bool,
    pub percent: Option<f32>,
    pub unlocked: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameAchievements {
    pub appid: u32,
    pub achievements: Vec<Achievement>,
    /// False when Steam's descriptions could not be read: the names alone.
    pub described: bool,
}
/// Steam's public list of a game's achievements; it needs no sign-in.
pub(crate) fn details_url(appid: u32) -> String {
    format!("{DETAILS}?appid={appid}&language=english")
}
/// Steam's answer, or nothing when it is not the list it should be.
pub(crate) fn parse_details(body: &str) -> Vec<Detail> {
    let Ok(answer) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(items) = answer["response"]["achievements"].as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item["internal_name"].as_str().filter(|name| !name.is_empty())?.to_string();
            let text = |key: &str| item[key].as_str().unwrap_or_default().trim().to_string();
            let title = Some(text("localized_name")).filter(|title| !title.is_empty()).unwrap_or_else(|| name.clone());
            Some(Detail {
                title,
                description: text("localized_desc"),
                icon: picture(&item["icon"]),
                icon_gray: picture(&item["icon_gray"]),
                hidden: item["hidden"].as_bool().unwrap_or(false),
                percent: rate(&item["player_percent_unlocked"]),
                name,
            })
        })
        .collect()
}
/// A picture's plain file name. A path, an address or anything but a picture is none, so an icon
/// can only ever come from the game's folder on Steam's image server.
fn picture(value: &Value) -> Option<String> {
    let name = value.as_str()?;
    let plain = !name.is_empty() && name.len() <= 128 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    let lower = name.to_ascii_lowercase();
    let image = [".jpg", ".jpeg", ".png", ".gif"].iter().any(|kind| lower.ends_with(kind));
    (plain && image).then(|| name.to_string())
}
/// A share of players, given as text or as a number; none outside 0 to 100.
fn rate(value: &Value) -> Option<f32> {
    let rate = value.as_f64().or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    (0.0..=100.0).contains(&rate).then_some(rate as f32)
}
/// The game's own list, in its order, with what Steam's public list adds to each.
pub(crate) fn merge(appid: u32, list: &AchievementList, details: &[Detail]) -> Vec<Achievement> {
    let locked: HashSet<&str> = list.locked.iter().map(String::as_str).collect();
    let described: HashMap<&str, &Detail> = details.iter().map(|detail| (detail.name.as_str(), detail)).collect();
    let address = |file: &Option<String>| file.as_ref().map(|file| format!("{ICONS}/{appid}/{file}"));
    list.names
        .iter()
        .map(|name| {
            let detail = described.get(name.as_str()).copied();
            Achievement {
                name: name.clone(),
                title: detail.map_or_else(|| name.clone(), |detail| detail.title.clone()),
                description: detail.map(|detail| detail.description.clone()).unwrap_or_default(),
                icon: detail.and_then(|detail| address(&detail.icon)),
                icon_gray: detail.and_then(|detail| address(&detail.icon_gray)),
                hidden: detail.is_some_and(|detail| detail.hidden),
                percent: detail.and_then(|detail| detail.percent),
                unlocked: !locked.contains(name.as_str()),
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam_automation::achievements::AchievementList;
    const GAME: u32 = 271590;
    const ANSWER: &str = r#"{"response":{"achievements":[
        {"internal_name":"ACH00","localized_name":"Welcome to Los Santos","localized_desc":"You repo'd a car.","icon":"348a.jpg","icon_gray":"dd1d.jpg","hidden":true,"player_percent_unlocked":"81.9"},
        {"internal_name":"ACH01","localized_name":"A Friendship Resurrected","localized_desc":"Complete the mission.","icon":"aaa.jpg","icon_gray":"bbb.jpg","hidden":false,"player_percent_unlocked":43.7}
    ]}}"#;
    fn list(names: &[&str], locked: &[&str]) -> AchievementList {
        AchievementList { names: names.iter().map(|s| s.to_string()).collect(), locked: locked.iter().map(|s| s.to_string()).collect() }
    }
    #[test]
    fn steams_public_details_are_read() {
        let details = parse_details(ANSWER);
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].name, "ACH00");
        assert_eq!(details[0].title, "Welcome to Los Santos");
        assert_eq!(details[0].description, "You repo'd a car.");
        assert!(details[0].hidden && !details[1].hidden);
        assert_eq!(details[0].percent, Some(81.9), "a rate given as text");
        assert_eq!(details[1].percent, Some(43.7), "or as a number");
    }
    #[test]
    fn the_games_own_list_decides_the_order_and_what_is_unlocked() {
        let merged = merge(GAME, &list(&["ACH01", "ACH00", "SECRET"], &["ACH01", "SECRET"]), &parse_details(ANSWER));
        assert_eq!(merged.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["ACH01", "ACH00", "SECRET"]);
        assert_eq!(merged.iter().map(|a| a.unlocked).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(merged[1].icon.as_deref(), Some("https://cdn.fastly.steamstatic.com/steamcommunity/public/images/apps/271590/348a.jpg"));
        assert_eq!(merged[1].icon_gray.as_deref(), Some("https://cdn.fastly.steamstatic.com/steamcommunity/public/images/apps/271590/dd1d.jpg"));
        assert_eq!((merged[2].title.as_str(), merged[2].description.as_str(), merged[2].percent), ("SECRET", "", None), "not described: its own name");
    }
    #[test]
    fn an_achievement_only_steams_list_knows_is_left_out() {
        let merged = merge(GAME, &list(&["ACH00"], &[]), &parse_details(ANSWER));
        assert_eq!(merged.len(), 1);
    }
    #[test]
    fn a_broken_answer_gives_no_details() {
        assert!(parse_details("<html>busy</html>").is_empty());
        assert!(parse_details(r#"{"response":{}}"#).is_empty());
        assert!(parse_details(r#"{"response":{"achievements":[{"localized_name":"no internal name"}]}}"#).is_empty());
    }
    #[test]
    fn an_icon_is_only_ever_a_picture_in_the_games_folder() {
        let answer = r#"{"response":{"achievements":[{"internal_name":"A","icon":"../../x.jpg","icon_gray":"https://elsewhere.example/x.jpg"},{"internal_name":"B","icon":"page.html","icon_gray":"b_gray.PNG"}]}}"#;
        let merged = merge(1, &list(&["A", "B"], &[]), &parse_details(answer));
        assert_eq!((merged[0].icon.as_deref(), merged[0].icon_gray.as_deref()), (None, None));
        assert_eq!(merged[1].icon, None);
        assert_eq!(merged[1].icon_gray.as_deref(), Some("https://cdn.fastly.steamstatic.com/steamcommunity/public/images/apps/1/b_gray.PNG"));
    }
    #[test]
    fn a_rate_outside_zero_to_a_hundred_is_dropped() {
        let answer = r#"{"response":{"achievements":[{"internal_name":"A","player_percent_unlocked":"140"},{"internal_name":"B","player_percent_unlocked":"-1"},{"internal_name":"C","player_percent_unlocked":"n/a"}]}}"#;
        assert!(parse_details(answer).iter().all(|detail| detail.percent.is_none()));
    }
    #[test]
    fn the_details_address_asks_in_english_for_one_game() {
        assert_eq!(details_url(620), "https://api.steampowered.com/IPlayerService/GetGameAchievements/v1/?appid=620&language=english");
    }
}
