//! The GitHub Releases settings (`github-releases.json` next to MYLE's other
//! settings): the repositories on the page and what each project remembers.
//! Nothing secret is kept here: the token and AI keys are in `secrets`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::ai::ProviderId;

const FILE_NAME: &str = "github-releases.json";
pub(crate) const SETTINGS_VERSION: u32 = 1;
/// Projects on the page at most.
const MAX_REPOS: usize = 300;

static EDIT: Mutex<()> = Mutex::new(());

fn edit_lock() -> MutexGuard<'static, ()> {
    EDIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A git repository on the page. Its sub-projects (a monorepo) are found
/// every time it is read, so a new app folder shows up by itself.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Repo {
    pub id: String,
    /// The working tree's top folder.
    pub path: String,
    pub added_at: i64,
}

/// How the last good build of a project went: the next one's progress is
/// estimated from it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildStats {
    pub command: String,
    pub duration_ms: u64,
    pub lines: u64,
    pub at: i64,
}

/// What one project (a repository, or one app folder of a monorepo) keeps.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryConfig {
    /// A build command typed by the user; the detected one otherwise.
    pub build_command: Option<String>,
    /// The id of the detected build chosen last.
    pub build_choice: Option<String>,
    /// Version files left out of version bumps (paths from the project
    /// folder), e.g. a mobile app that has its own version.
    pub skip_version_files: Vec<String>,
    pub last_build: Option<BuildStats>,
    /// `local` or `actions`; picked by itself when not set.
    pub release_mode: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    /// `gh` (taken from the GitHub CLI) or `token` (pasted).
    pub source: String,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// The providers to use, first one first; the next is offered when one
    /// fails or hits its free limit.
    pub order: Vec<ProviderId>,
    /// A model per provider, when not the default.
    pub models: BTreeMap<ProviderId, String>,
    /// Ollama's address, when not `http://localhost:11434`.
    pub ollama_url: Option<String>,
    /// Ollama (on this PC, no key) is used.
    pub ollama_enabled: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            order: ProviderId::ALL.to_vec(),
            models: BTreeMap::new(),
            ollama_url: None,
            ollama_enabled: false,
        }
    }
}

/// What the one-time import from Github-Build-Release did.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GbrImport {
    pub at: i64,
    pub project: Option<String>,
    pub deepseek_key: bool,
    /// The key was removed from Github-Build-Release's settings file.
    pub plaintext_removed: bool,
    pub config_path: Option<String>,
    pub error: Option<String>,
    pub dismissed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub repos: Vec<Repo>,
    /// By project id (`<repo id>` or `<repo id>/<folder>`).
    pub entries: BTreeMap<String, EntryConfig>,
    pub account: Option<Account>,
    pub ai: AiSettings,
    /// Set once the Github-Build-Release import ran.
    pub gbr_import: Option<GbrImport>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            repos: Vec::new(),
            entries: BTreeMap::new(),
            account: None,
            ai: AiSettings::default(),
            gbr_import: None,
        }
    }
}

impl Settings {
    pub(crate) fn repo(&self, id: &str) -> Result<&Repo, String> {
        self.repos
            .iter()
            .find(|repo| repo.id == id)
            .ok_or_else(|| "This project is no longer on the page.".to_string())
    }

    pub(crate) fn entry(&self, id: &str) -> EntryConfig {
        self.entries.get(id).cloned().unwrap_or_default()
    }

    /// Adds `path` (a repository's top folder) unless it is there already;
    /// the repository's id either way.
    pub(crate) fn add_repo(&mut self, path: &Path) -> Result<String, String> {
        if let Some(known) = self
            .repos
            .iter()
            .find(|repo| same_path(Path::new(&repo.path), path))
        {
            return Ok(known.id.clone());
        }
        if self.repos.len() >= MAX_REPOS {
            return Err(format!("At most {MAX_REPOS} projects fit on the page."));
        }
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        self.repos.push(Repo {
            id: id.clone(),
            path: path.display().to_string(),
            added_at: super::unix_millis(),
        });
        Ok(id)
    }

    pub(crate) fn remove_repo(&mut self, id: &str) {
        self.repos.retain(|repo| repo.id != id);
        let prefix = format!("{id}/");
        self.entries
            .retain(|key, _| key != id && !key.starts_with(&prefix));
    }
}

/// Windows paths: case and slashes don't matter.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.display()
            .to_string()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn settings_path() -> Result<PathBuf, String> {
    crate::storage::roaming_dir().map(|dir| dir.join(FILE_NAME))
}

pub(crate) fn load() -> Result<Settings, String> {
    load_from(&settings_path()?)
}

pub(crate) fn load_from(path: &Path) -> Result<Settings, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(format!("Can't read the GitHub Releases settings: {error}")),
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            // Providers added by a newer MYLE join the end of the order.
            for provider in ProviderId::ALL {
                if !settings.ai.order.contains(&provider) {
                    settings.ai.order.push(provider);
                }
            }
            Ok(settings)
        }
        Err(_) => {
            let aside = path.with_extension(format!("corrupt-{}.json", super::unix_millis()));
            let _ = fs::rename(path, aside);
            Ok(Settings::default())
        }
    }
}

pub(crate) fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    crate::game_saves::atomic::write(path, &text)
}

/// Reads the settings afresh, changes and saves them under the edit lock.
pub(crate) fn edit(
    change: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> Result<Settings, String> {
    let _edit = edit_lock();
    let path = settings_path()?;
    let mut settings = load_from(&path)?;
    change(&mut settings)?;
    save_to(&path, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos_are_added_once_and_removed_with_their_entries() {
        let mut settings = Settings::default();
        let id = settings.add_repo(Path::new(r"C:\Code\MYLE")).unwrap();
        assert_eq!(settings.add_repo(Path::new(r"c:/code/myle/")).unwrap(), id);
        settings
            .entries
            .insert(format!("{id}/app"), EntryConfig::default());
        settings
            .entries
            .insert("other".into(), EntryConfig::default());
        settings.remove_repo(&id);
        assert!(settings.repos.is_empty());
        assert_eq!(settings.entries.len(), 1);
    }

    #[test]
    fn a_damaged_file_is_set_aside_and_new_providers_are_appended() {
        let dir = std::env::temp_dir().join(format!("myle-gr-store-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path).unwrap(), Settings::default());
        assert!(!path.exists());
        fs::write(&path, r#"{"version":1,"ai":{"order":["gemini"]}}"#).unwrap();
        let settings = load_from(&path).unwrap();
        assert_eq!(settings.ai.order[0], ProviderId::Gemini);
        assert_eq!(settings.ai.order.len(), ProviderId::ALL.len());
        let _ = fs::remove_dir_all(dir);
    }
}
