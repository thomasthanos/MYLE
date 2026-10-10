use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpotifyHubAction {
    InstallSpicetify,
    RestoreSpotify,
    PurgeAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpotifyHubStage {
    Preparing,
    Downloading,
    Verifying,
    Installing,
    Restoring,
    Uninstalling,
    Cleaning,
    Finalizing,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SpotifyHubEvent {
    Stage {
        job_id: String,
        stage: SpotifyHubStage,
    },
    Progress {
        job_id: String,
        fraction: f64,
    },
    Line {
        job_id: String,
        text: String,
        replace: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SpotifyHubResult {
    Done,
    Cancelled,
    Partial,
    NeedsAdmin,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyHubOutcome {
    pub result: SpotifyHubResult,
    pub job_id: String,
    pub action: SpotifyHubAction,
    pub note: Option<String>,
}

impl SpotifyHubOutcome {
    pub fn new(
        result: SpotifyHubResult,
        job_id: impl Into<String>,
        action: SpotifyHubAction,
        note: Option<String>,
    ) -> Self {
        Self {
            result,
            job_id: job_id.into(),
            action,
            note,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationState {
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpicetifyState {
    pub installed: bool,
    pub version: Option<String>,
    pub healthy: bool,
    pub applied: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceState {
    pub installed: bool,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyHubReleases {
    pub cli: String,
    pub marketplace: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prerequisites {
    pub desktop_spotify: bool,
    pub supported: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveJob {
    pub job_id: String,
    pub action: SpotifyHubAction,
    pub stage: SpotifyHubStage,
    pub progress: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyHubSnapshot {
    pub desktop: InstallationState,
    pub store: InstallationState,
    pub spicetify: SpicetifyState,
    pub marketplace: MarketplaceState,
    pub prerequisites: Prerequisites,
    pub last_outcome: Option<SpotifyHubOutcome>,
    pub active_job: Option<ActiveJob>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgePreview {
    pub token: String,
    pub detected_variants: Vec<String>,
    pub categories: Vec<String>,
    pub warnings: Vec<String>,
    /// Unix time in milliseconds. The token is one-use and is also invalidated
    /// whenever a fresh detection fingerprint differs.
    pub expires_at: u64,
}
