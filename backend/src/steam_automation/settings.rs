//! The owner's switches and picks, and the record of what was done.
use std::path::Path;
use serde::{Deserialize, Serialize};
const ACTIVITY_KEPT: usize = 200;
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub farm_on: bool,
    pub idle_on: bool,
    pub achievements_on: bool,
    pub inventory_on: bool,
    pub idle_games: Vec<u32>,
    pub achievement_games: Vec<u32>,
    pub minimum_cents: u32,
    pub favorites: Vec<u32>,
    pub card_blacklist: Vec<u32>,
    pub locked_assets: Vec<String>,
    pub paused: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            farm_on: false,
            idle_on: false,
            achievements_on: false,
            inventory_on: false,
            idle_games: Vec::new(),
            achievement_games: Vec::new(),
            minimum_cents: 5,
            favorites: Vec::new(),
            card_blacklist: Vec::new(),
            locked_assets: Vec::new(),
            paused: false,
        }
    }
}
/// Anything unreadable means everything off, the safe state.
pub(crate) fn load(path: &Path) -> Settings {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}
pub(crate) fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    crate::game_saves::atomic::write(path, &serde_json::to_vec(settings).map_err(|e| e.to_string())?)
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivityEntry {
    pub at: u64,
    pub text: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Activity {
    pub entries: Vec<ActivityEntry>,
    #[serde(skip)]
    changed: bool,
}
impl Activity {
    pub(crate) fn push(&mut self, at: u64, text: String) {
        self.entries.push(ActivityEntry { at, text });
        self.changed = true;
        if self.entries.len() > ACTIVITY_KEPT {
            let extra = self.entries.len() - ACTIVITY_KEPT;
            self.entries.drain(..extra);
        }
    }
    /// Whether anything was added since the last time this was asked.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }
    pub(crate) fn save(&self, path: &Path) {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = crate::game_saves::atomic::write(path, &bytes);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> std::path::PathBuf {
        let dir = crate::test_dirs::temp_dir("automation");
        dir.join(name)
    }
    #[test]
    fn everything_starts_off() {
        let settings = Settings::default();
        assert!(!settings.farm_on && !settings.idle_on && !settings.achievements_on && !settings.inventory_on);
        assert_eq!(settings.minimum_cents, 5);
    }
    #[test]
    fn settings_come_back_as_saved() {
        let path = temp("settings.json");
        let settings = Settings { farm_on: true, idle_games: vec![620], minimum_cents: 9, ..Settings::default() };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), settings);
    }
    #[test]
    fn corrupt_settings_mean_everything_off() {
        let path = temp("settings.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load(&path), Settings::default());
    }
    #[test]
    fn a_setting_from_an_older_version_is_not_lost() {
        let path = temp("settings.json");
        std::fs::write(&path, r#"{"farmOn":true}"#).unwrap();
        let settings = load(&path);
        assert!(settings.farm_on);
        assert_eq!(settings.minimum_cents, 5);
    }
    #[test]
    fn older_settings_have_empty_lists() {
        let path = temp("settings.json");
        std::fs::write(&path, r#"{"farmOn":true,"idleGames":[7]}"#).unwrap();
        let settings = load(&path);
        assert_eq!(settings.idle_games, vec![7]);
        assert!(settings.favorites.is_empty() && settings.card_blacklist.is_empty() && settings.locked_assets.is_empty());
    }
    #[test]
    fn the_new_lists_come_back_as_saved() {
        let path = temp("settings.json");
        let settings = Settings { favorites: vec![1], card_blacklist: vec![3], locked_assets: vec!["9".to_string()], ..Settings::default() };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), settings);
    }
    #[test]
    fn pause_is_remembered_and_starts_off() {
        assert!(!Settings::default().paused);
        let path = temp("settings.json");
        let settings = Settings { paused: true, ..Settings::default() };
        save(&path, &settings).unwrap();
        assert!(load(&path).paused);
    }
    #[test]
    fn activity_knows_when_it_has_something_new() {
        let mut activity = Activity::default();
        assert!(!activity.take_changed());
        activity.push(1, "one".to_string());
        assert!(activity.take_changed());
        assert!(!activity.take_changed(), "once told is enough");
    }
    #[test]
    fn activity_keeps_the_last_two_hundred() {
        let mut activity = Activity::default();
        for at in 0..205 {
            activity.push(at, format!("entry {at}"));
        }
        assert_eq!(activity.entries.len(), 200);
        assert_eq!(activity.entries[0].text, "entry 5");
        assert_eq!(activity.entries[199].text, "entry 204");
    }
    #[test]
    fn activity_comes_back_after_a_restart() {
        let path = temp("activity.json");
        let mut activity = Activity::default();
        activity.push(1, "one".to_string());
        activity.save(&path);
        assert_eq!(Activity::load(&path).entries.len(), 1);
    }
}
