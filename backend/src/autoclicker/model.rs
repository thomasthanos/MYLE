use serde::{Deserialize, Serialize};
use std::collections::HashSet;
pub const MAX_STEPS: usize = 128;
pub const MAX_PROFILES: usize = 100;
pub const MAX_COUNT: u32 = 1_000_000;
pub const MAX_DELAY: u64 = 86_400_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Simple,
    Sequence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClickType {
    Single,
    Double,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub clicks: u32,
    pub button: MouseButton,
    pub click_type: ClickType,
    pub interval_ms: u64,
    pub delay_ms: u64,
}
impl Default for Step {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            x: 0,
            y: 0,
            clicks: 50,
            button: MouseButton::Left,
            click_type: ClickType::Single,
            interval_ms: 100,
            delay_ms: 500,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    pub button: MouseButton,
    pub click_type: ClickType,
    pub interval_ms: u64,
    pub steps: Vec<Step>,
    /// Simple: number of click actions. Sequence: number of complete loops.
    pub loops: Option<u32>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Simple,
            button: MouseButton::Left,
            click_type: ClickType::Single,
            interval_ms: 100,
            steps: vec![Step::default()],
            loops: Some(50),
        }
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !(10..=MAX_DELAY).contains(&self.interval_ms) {
            return Err("Set the click interval between 10 ms and 24 hours.".into());
        }
        if self.loops.is_some_and(|n| n == 0 || n > MAX_COUNT) {
            return Err(
                "Set a repeat count between 1 and 1,000,000, or choose Until stopped.".into(),
            );
        }
        if self.steps.len() > MAX_STEPS || (self.mode == Mode::Sequence && self.steps.is_empty()) {
            return Err("A sequence needs between 1 and 128 steps.".into());
        }
        let mut ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if !valid_id(&step.id) || !ids.insert(&step.id) {
                return Err("Each step needs a unique ID.".into());
            }
            if step.clicks == 0
                || step.clicks > MAX_COUNT
                || !(10..=MAX_DELAY).contains(&step.interval_ms)
                || step.delay_ms > MAX_DELAY
            {
                return Err(format!(
                    "Step {}: use 1–1,000,000 clicks, an interval of 10 ms–24 hours, and a delay of 0–24 hours.",
                    index + 1
                ));
            }
        }
        Ok(())
    }
    pub fn total(&self) -> Option<u64> {
        self.loops.map(|loops| {
            u64::from(loops)
                * match self.mode {
                    Mode::Simple => 1,
                    Mode::Sequence => self.steps.iter().map(|s| u64::from(s.clicks)).sum(),
                }
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub config: Config,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profiles {
    pub version: u32,
    pub active_id: String,
    pub items: Vec<Profile>,
}
impl Default for Profiles {
    fn default() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            version: 1,
            active_id: id.clone(),
            items: vec![Profile {
                id,
                name: "My clicks".into(),
                config: Config::default(),
            }],
        }
    }
}
impl Profiles {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("This profile file uses an unsupported version.".into());
        }
        if self.items.is_empty() || self.items.len() > MAX_PROFILES {
            return Err("Keep between 1 and 100 profiles.".into());
        }
        let mut ids = HashSet::new();
        for profile in &self.items {
            if !valid_id(&profile.id) || !ids.insert(&profile.id) {
                return Err("Each profile needs a unique ID.".into());
            }
            if profile.name.trim().is_empty()
                || profile.name.chars().count() > 80
                || profile.name.chars().any(char::is_control)
            {
                return Err("Give each profile a name of 1–80 characters.".into());
            }
            profile.config.validate()?;
        }
        if !ids.contains(&self.active_id) {
            return Err("The selected profile is missing.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_zero_or_excessive_intervals_and_repeat_counts() {
        let mut config = Config::default();
        for interval in [0, 9, 86_400_001] {
            config.interval_ms = interval;
            assert!(config.validate().is_err());
        }
        config.interval_ms = 10;
        assert!(config.validate().is_ok());
        config.loops = Some(0);
        assert!(config.validate().is_err());
        config.loops = None;
        assert!(config.validate().is_ok());
    }
    #[test]
    fn sequence_requires_steps_positive_clicks_and_unique_ids() {
        let mut config = Config {
            mode: Mode::Sequence,
            ..Config::default()
        };
        config.steps.clear();
        assert!(config.validate().is_err());
        config.steps.push(Step::default());
        config.steps[0].x = -1920;
        config.steps[0].y = -240;
        assert!(config.validate().is_ok());
        config.steps[0].clicks = 0;
        assert!(config.validate().is_err());
        config.steps[0].clicks = 50;
        config.steps.push(config.steps[0].clone());
        assert!(config.validate().is_err());
    }
    #[test]
    fn profiles_validate_active_selection_names_and_version() {
        let mut profiles = Profiles::default();
        assert!(profiles.validate().is_ok());
        profiles.active_id = "missing".into();
        assert!(profiles.validate().is_err());
        profiles.active_id = profiles.items[0].id.clone();
        profiles.items[0].name = "\n".into();
        assert!(profiles.validate().is_err());
        profiles.items[0].name = "Work".into();
        profiles.version = 2;
        assert!(profiles.validate().is_err());
    }
    #[test]
    fn config_roundtrips_without_losing_double_click_or_negative_coordinates() {
        let mut config = Config {
            mode: Mode::Sequence,
            loops: Some(3),
            ..Config::default()
        };
        config.steps[0].x = -1200;
        config.steps[0].button = MouseButton::Middle;
        config.steps[0].click_type = ClickType::Double;
        let text = serde_json::to_string(&config).unwrap();
        assert!(text.contains("\"clickType\":\"double\""));
        assert_eq!(serde_json::from_str::<Config>(&text).unwrap(), config);
    }
}
