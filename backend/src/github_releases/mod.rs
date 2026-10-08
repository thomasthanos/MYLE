//! GitHub Releases: commit and push, build, and release your projects to
//! GitHub from one page.
//!
//! - `store`: the projects (git repositories) and the page's settings, in
//!   `github-releases.json`; secrets (the GitHub token, AI keys) are sealed
//!   with DPAPI by `secrets` and never leave the backend.
//! - `git`: runs git (never with a token on its command line) and reads its
//!   status; `project` finds sub-projects, build types and version files.
//! - `github`: the REST API (account, releases, uploads, Actions runs).
//! - `ai`: one OpenAI-compatible client for every AI provider.
//! - `build`: runs a build in a Job Object, reads errors, warnings and
//!   progress from its output, and finds what it made.
//! - `release`: bump → build → commit → push → tag → release, in that order,
//!   undone up to the push when a step fails.

pub mod ai;
pub mod build;
pub mod commands;
mod entry;
mod gbr_import;
pub mod git;
pub mod github;
mod ops;
pub mod project;
pub mod release;
pub mod secrets;
mod state;
pub mod store;
pub mod versions;

pub use state::GithubReleasesState;

pub(crate) const CANCELLED: &str = "Cancelled.";

/// A failure the page offers an action for, with its message.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `NO_GIT`, `NO_UPSTREAM`, `REJECTED`, `HOOK_FAILED`, `AUTH`, …
    pub code: String,
    pub message: String,
    /// What the tool printed, for "Show details".
    pub details: Option<String>,
    /// More for the page (the AI provider to offer next, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Problem {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Problem {
            code: code.into(),
            message: message.into(),
            details: None,
            data: None,
        }
    }

    pub(crate) fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    pub(crate) fn with_details(mut self, details: impl Into<String>) -> Self {
        let details = details.into();
        self.details = (!details.trim().is_empty()).then_some(details);
        self
    }
}

/// Errors cross to the page as JSON text (`{"code":…,"message":…}`) so it
/// can offer the right action; plain messages stay plain.
impl From<Problem> for String {
    fn from(problem: Problem) -> String {
        serde_json::to_string(&problem).unwrap_or(problem.message)
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A new log file for a build of the project `entry_id`; the oldest logs of
/// that project beyond the last 10 are removed.
pub(crate) fn build_log_path(entry_id: &str) -> Option<std::path::PathBuf> {
    let dir = crate::storage::local_dir()
        .ok()?
        .join("github-releases")
        .join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let safe: String = entry_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut old: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&format!("{safe}-")))
        })
        .collect();
    old.sort();
    while old.len() >= 10 {
        let _ = std::fs::remove_file(old.remove(0));
    }
    Some(dir.join(format!("{safe}-{}.log", unix_millis())))
}

/// Keeps how a good build went, for the next one's progress estimate.
pub(crate) fn remember_build(entry_id: &str, command: &str, outcome: &build::runner::BuildOutcome) {
    let stats = release::stats_of(command, outcome);
    let id = entry_id.to_string();
    let _ = store::edit(move |settings| {
        settings.entries.entry(id).or_default().last_build = Some(stats);
        Ok(())
    });
}
