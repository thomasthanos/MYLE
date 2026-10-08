//! Project Backups: zips of project folders kept in Google Drive or Dropbox,
//! in the same folders and with the same names as backup_projects, so the two
//! apps add to one history:
//! `<cloud>\Projects Backup\<AppName>\<YYYY-MM Month>\<AppName>_D<day>_V<n>.zip`.
//!
//! Each backup is built and checked locally, compared once more with the
//! project folder, then copied, re-read until it matches and only then given
//! its name (`archive`). Backups can be compared with each other or with the
//! project folder (`compare`).

mod archive;
pub mod commands;
mod compare;
mod detect;
mod engine;
mod gitindex;
mod naming;
mod rules;
mod state;
mod store;
mod walk;

use std::path::PathBuf;

use serde::Serialize;

pub use state::ProjectBackupsState;

pub(crate) const CANCELLED: &str = "Cancelled.";

/// The steps of a backup, as shown on the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Preparing,
    StartingCloud,
    ClosingApp,
    Scanning,
    Zipping,
    Verifying,
    CheckingCompleteness,
    Copying,
    VerifyingCopy,
    Finishing,
}

/// Where a running backup reports to, and asks whether to stop.
pub(crate) trait Sink {
    fn stage(&self, stage: Stage);
    fn progress(&self, done_bytes: u64, total_bytes: u64, done_files: u64, total_files: u64);
    fn cancelled(&self) -> bool;
}

#[derive(Debug)]
pub(crate) enum Failure {
    Cancelled,
    /// A file of the project is an online-only file of a cloud app that is
    /// not running (os error 362).
    CloudOffline(PathBuf),
    /// The project folder is not there (moved, renamed, another drive).
    SourceMissing,
    Message(String),
    /// A problem the page offers an action for (`NO_FILES`, `NAME_TAKEN`,
    /// `CLOUD_NOT_INSTALLED`, …), with its message.
    Coded(&'static str, String),
}

impl Failure {
    pub(crate) fn context(self, prefix: &str) -> Failure {
        match self {
            Failure::Message(text) => Failure::Message(format!("{prefix}: {text}")),
            other => other,
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Failure::Cancelled => CANCELLED.into(),
            Failure::CloudOffline(path) => format!(
                "\"{}\" is an online-only file and the cloud app that keeps it is not running.",
                path.display()
            ),
            Failure::SourceMissing => "The project folder was not found.".into(),
            Failure::Message(text) | Failure::Coded(_, text) => text.clone(),
        }
    }
}

impl From<String> for Failure {
    fn from(text: String) -> Self {
        if text == CANCELLED { Failure::Cancelled } else { Failure::Message(text) }
    }
}
