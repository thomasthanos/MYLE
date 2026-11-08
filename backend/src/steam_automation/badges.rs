//! What Steam says about card drops and purchases, read from the signed-in pages.
use std::collections::HashSet;
use std::sync::LazyLock;
use regex_lite::Regex;
use super::community::{Community, CommunityError};
const ROW: &str = r#"class="badge_row is_link""#;
const REFUND_SECONDS: u64 = 14 * 86_400;
const REFUND_MINUTES: u32 = 120;
const COMMUNITY: &str = "https://steamcommunity.com";
const LICENCES_URL: &str = "https://store.steampowered.com/account/licenses/?l=english";
fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}
static APP: LazyLock<Regex> = LazyLock::new(|| regex(r"gamecards/(\d+)"));
static PLAYTIME: LazyLock<Regex> = LazyLock::new(|| regex(r"([\d,]+(?:\.\d+)?) hrs on record"));
static DROPS: LazyLock<Regex> = LazyLock::new(|| regex(r#"progress_info_bold">(\d+) card drops? remaining"#));
static TITLE: LazyLock<Regex> = LazyLock::new(|| regex(r#"class="badge_title">\s*([^<]*?)\s*(?:&nbsp;|<)"#));
static PAGE: LazyLock<Regex> = LazyLock::new(|| regex(r#"class="pagelink"[^>]*>(\d+)<"#));
static LICENCE: LazyLock<Regex> = LazyLock::new(|| regex(r#"(?s)<td class="license_date_col">([^<]+)</td>\s*<td>(.*?)</td>"#));
static BLOCK: LazyLock<Regex> = LazyLock::new(|| regex(r"(?s)<div.*?</div>"));
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BadgeGame {
    pub appid: u32,
    pub name: String,
    pub drops: u32,
    pub minutes_played: u32,
}
fn number(pattern: &Regex, text: &str) -> Option<u32> {
    pattern.captures(text)?.get(1)?.as_str().parse().ok()
}
fn minutes_played(row: &str) -> u32 {
    let Some(found) = PLAYTIME.captures(row).and_then(|c| c.get(1)) else {
        return 0;
    };
    let hours: f64 = found.as_str().replace(',', "").parse().unwrap_or(0.0);
    (hours * 60.0).round() as u32
}
/// The games on one badges page that still have drops, and the number of the last page.
pub(crate) fn parse_badges(html: &str) -> Result<(Vec<BadgeGame>, u32), String> {
    if !html.contains(ROW) {
        return Err("Steam did not send the badges page.".to_string());
    }
    let games = html
        .split(ROW)
        .skip(1)
        .filter_map(|row| {
            Some(BadgeGame {
                appid: number(&APP, row)?,
                name: TITLE.captures(row)?.get(1)?.as_str().to_string(),
                drops: number(&DROPS, row)?,
                minutes_played: minutes_played(row),
            })
        })
        .collect();
    let last = PAGE.captures_iter(html).filter_map(|c| c.get(1)?.as_str().parse().ok()).max().unwrap_or(1);
    Ok((games, last))
}
/// Days since 1970 for a calendar date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
/// Reads a date like `8 Nov, 2026` as Unix seconds.
fn parse_date(text: &str) -> Option<u64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut parts = text.split([' ', ',']).filter(|part| !part.is_empty());
    let day: i64 = parts.next()?.parse().ok()?;
    let month_name = parts.next()?;
    let month = MONTHS.iter().position(|name| *name == month_name)? as i64 + 1;
    let year: i64 = parts.next()?.parse().ok()?;
    u64::try_from(days_from_civil(year, month, day) * 86_400).ok()
}
/// Each licence's name and when it was added.
pub(crate) fn parse_licences(html: &str) -> Vec<(String, u64)> {
    LICENCE
        .captures_iter(html)
        .filter_map(|row| {
            let date = parse_date(row.get(1)?.as_str().trim())?;
            let cell = BLOCK.replace_all(row.get(2)?.as_str(), "");
            let name = cell.trim().replace("&amp;", "&");
            (!name.is_empty()).then_some((name, date))
        })
        .collect()
}
/// Whether a licence bought in the refund window names this game. Editions count for their game.
pub(crate) fn bought_recently(game: &str, licences: &[(String, u64)], now: u64) -> bool {
    let game = game.to_lowercase();
    licences
        .iter()
        .any(|(name, date)| now.saturating_sub(*date) <= REFUND_SECONDS && name.to_lowercase().contains(&game))
}
/// `recent` is `None` when purchase dates could not be read, which counts against idling.
pub(crate) fn is_refundable(recent: Option<bool>, minutes_played: u32) -> bool {
    minutes_played < REFUND_MINUTES && recent != Some(false)
}
/// Every game with drops left, across all the badges pages.
pub(crate) async fn read_badges<C: Community>(api: &C, steam_id: u64) -> Result<Vec<BadgeGame>, CommunityError> {
    let mut games = Vec::new();
    let mut seen = HashSet::new();
    let mut page = 1;
    loop {
        let url = format!("{COMMUNITY}/profiles/{steam_id}/badges/?l=english&p={page}");
        let (found, last) = parse_badges(&api.get(&url).await?).map_err(CommunityError::Other)?;
        games.extend(found.into_iter().filter(|game| seen.insert(game.appid)));
        if page >= last {
            return Ok(games);
        }
        page += 1;
    }
}
/// The account's licences. An empty answer counts as not having been able to read them.
pub(crate) async fn read_licences<C: Community>(api: &C) -> Result<Vec<(String, u64)>, CommunityError> {
    let licences = parse_licences(&api.get(LICENCES_URL).await?);
    if licences.is_empty() {
        return Err(CommunityError::Other("Steam did not send the licences page.".to_string()));
    }
    Ok(licences)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam_automation::community::{CommunityError, FakeCommunity};
    const BADGES: &str = include_str!("fixtures/badges.html");
    const LICENCES: &str = include_str!("fixtures/licences.html");
    fn game(appid: u32) -> Option<BadgeGame> {
        parse_badges(BADGES).unwrap().0.into_iter().find(|g| g.appid == appid)
    }
    const NOV_8_2026: u64 = 1_794_096_000;
    #[test]
    fn the_fixture_gives_games_with_drops() {
        assert_eq!(game(620), Some(BadgeGame { appid: 620, name: "Portal 2".into(), drops: 3, minutes_played: 90 }));
    }
    #[test]
    fn one_drop_and_large_playtimes_read_correctly() {
        assert_eq!(game(239140), Some(BadgeGame { appid: 239140, name: "Dying Light".into(), drops: 1, minutes_played: 72270 }));
    }
    #[test]
    fn a_game_never_played_has_no_minutes() {
        assert_eq!(game(730).unwrap().minutes_played, 0);
    }
    #[test]
    fn a_game_with_no_drops_is_left_out() {
        assert_eq!(game(960910), None);
    }
    #[test]
    fn a_badge_that_is_not_a_game_is_left_out() {
        assert_eq!(parse_badges(BADGES).unwrap().0.len(), 3);
    }
    #[test]
    fn the_last_page_comes_from_the_page_links() {
        assert_eq!(parse_badges(BADGES).unwrap().1, 3);
    }
    #[test]
    fn a_page_that_is_not_badges_is_an_error() {
        assert!(parse_badges("<html></html>").is_err());
    }
    #[test]
    fn licences_read_their_names_and_dates() {
        assert_eq!(
            parse_licences(LICENCES),
            vec![("Portal 2".to_string(), NOV_8_2026), ("Dying Light Deluxe Edition".to_string(), 1_577_836_800)]
        );
    }
    #[test]
    fn a_recent_licence_counts_by_its_name() {
        let licences = parse_licences(LICENCES);
        assert!(bought_recently("Portal 2", &licences, NOV_8_2026 + 86400));
        assert!(!bought_recently("Dying Light", &licences, NOV_8_2026 + 86400), "that licence is years old");
        assert!(!bought_recently("Portal 2", &licences, NOV_8_2026 + 15 * 86400));
    }
    #[test]
    fn an_edition_counts_for_its_game() {
        let licences = vec![("Dying Light Deluxe Edition".to_string(), NOV_8_2026)];
        assert!(bought_recently("Dying Light", &licences, NOV_8_2026));
    }
    #[test]
    fn under_two_hours_and_bought_recently_is_refundable() {
        assert!(is_refundable(Some(true), 119));
    }
    #[test]
    fn two_hours_played_is_not_refundable() {
        assert!(!is_refundable(Some(true), 120));
    }
    #[test]
    fn an_old_purchase_is_not_refundable() {
        assert!(!is_refundable(Some(false), 10));
    }
    #[test]
    fn unknown_dates_count_as_refundable_under_two_hours() {
        assert!(is_refundable(None, 119));
        assert!(!is_refundable(None, 120));
    }
    #[tokio::test]
    async fn every_badge_page_is_read() {
        let api = FakeCommunity::answering(|_| Ok(BADGES.to_string()));
        let games = read_badges(&api, 76561198000000001).await.unwrap();
        assert_eq!(api.gets().len(), 3);
        assert!(api.gets()[1].contains("/profiles/76561198000000001/badges/?l=english&p=2"));
        assert_eq!(games.len(), 3, "the same game on several pages counts once");
    }
    #[tokio::test]
    async fn being_signed_out_stops_the_reading() {
        let api = FakeCommunity::answering(|_| Err(CommunityError::SignedOut));
        assert_eq!(read_badges(&api, 1).await, Err(CommunityError::SignedOut));
    }
}
