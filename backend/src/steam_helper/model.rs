//! The shapes the page receives.
use serde::{Deserialize, Serialize};
/// The page's filter chips.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Filters {
    pub online_coop: bool,
    pub single_player: bool,
    pub story_only: bool,
}
impl Filters {
    /// Names this combination in cache keys and saved picks.
    pub(crate) fn key(&self) -> String {
        format!(
            "c{}s{}t{}",
            u8::from(self.online_coop),
            u8::from(self.single_player),
            u8::from(self.story_only)
        )
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub appid: u32,
    pub name: String,
    pub cover_url: Option<String>,
    pub store_url: String,
    pub review_percent: u8,
    pub review_count: u32,
    pub is_free: bool,
    pub price: Option<String>,
    pub original_price: Option<String>,
    pub discount_percent: u8,
    pub deal_ends_at: Option<u64>,
    pub online_coop: bool,
    pub single_player: bool,
    pub story: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferKind {
    FreeWeekend,
    FreeToKeep,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeOffer {
    pub game: Game,
    pub kind: OfferKind,
    pub label: String,
    #[serde(default)]
    pub starts_at: Option<u64>,
    pub ends_at: Option<u64>,
    pub offer_id: String,
}
