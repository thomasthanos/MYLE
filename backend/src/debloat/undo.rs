// ! What a tweak changed, kept so Undo puts back exactly what was there ! before: the value, or its absence (and the keys made for it), the ! service's start type, the task's state, the user's time format. ! ! Only the first application of a tweak is kept: applying it again must ! not replace the original with the tweak's own value.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use super::catalog::Start;
use super::system::Value;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Before {
    /// A registry value, and the first key applying created (if any).
    Reg { value: Value, created: Option<String> },
    Service { start: Start },
    Task { enabled: bool },
    Clock { short: String, long: String },
    /// Edge was installed; Undo installs it again.
    Edge,
    /// A part of Windows was installed; Undo adds it again.
    Capability,
}
/// One operation of a tweak (its index in the tweak's `ops`) and what it
/// found there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub op: usize,
    pub before: Before,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    #[serde(default)]
    pub tweaks: BTreeMap<String, Vec<Saved>>,
    /// Apps MYLE removed (their catalog ids), to offer them again.
    #[serde(default)]
    pub removed_apps: BTreeSet<String>,
}
impl Store {
    fn path() -> Result<PathBuf, String> {
        Ok(crate::storage::local_dir()?.join("debloat-undo.json"))
    }
    pub fn load() -> Store {
        Self::path()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let temp = path.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &path).map_err(|e| e.to_string())
    }
    /// Keeps `saved` unless this operation's original is already kept.
    pub fn record(&mut self, tweak: &str, saved: Saved) {
        let list = self.tweaks.entry(tweak.to_string()).or_default();
        if !list.iter().any(|kept| kept.op == saved.op) {
            list.push(saved);
        }
    }
    pub fn saved(&self, tweak: &str) -> Vec<Saved> {
        self.tweaks.get(tweak).cloned().unwrap_or_default()
    }
    /// After Undo: the operations put back are forgotten.
    pub fn forget(&mut self, tweak: &str, ops: &[usize]) {
        if let Some(list) = self.tweaks.get_mut(tweak) {
            list.retain(|kept| !ops.contains(&kept.op));
            if list.is_empty() {
                self.tweaks.remove(tweak);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_first_original_is_the_one_kept() {
        let mut store = Store::default();
        let first = Saved { op: 0, before: Before::Reg { value: Value::Dword { value: 1 }, created: None } };
        let again = Saved { op: 0, before: Before::Reg { value: Value::Dword { value: 0 }, created: None } };
        store.record("telemetry", first.clone());
        store.record("telemetry", again);
        assert_eq!(store.saved("telemetry"), vec![first]);
        store.record("telemetry", Saved { op: 3, before: Before::Task { enabled: true } });
        store.forget("telemetry", &[0]);
        assert_eq!(store.saved("telemetry").len(), 1);
        store.forget("telemetry", &[3]);
        assert!(store.tweaks.is_empty());
    }
    #[test]
    fn it_reads_back_what_it_wrote() {
        let mut store = Store::default();
        store.record("location", Saved { op: 1, before: Before::Reg { value: Value::Sz { value: "Allow".into() }, created: Some(r"SOFTWARE\x".into()) } });
        store.record("services", Saved { op: 0, before: Before::Service { start: Start::AutoDelayed } });
        store.removed_apps.insert("bing-news".into());
        let json = serde_json::to_string(&store).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tweaks, store.tweaks);
        assert_eq!(back.removed_apps, store.removed_apps);
    }
}
