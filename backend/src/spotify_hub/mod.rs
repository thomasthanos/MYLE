//! Spotify Desktop and Spicetify lifecycle management.
//!
//! The webview can select only one of three action identifiers. Release URLs,
//! process arguments, registry keys and filesystem targets are all resolved
//! here from compile-time allowlists.

mod actions;
mod detection;
mod filesystem;
mod installed;
mod models;
mod release;
mod runner;
mod state;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::apps::{Cleanup, Jobs};
use crate::download::CANCELLED;

pub use models::{
    PurgePreview, SpotifyHubAction, SpotifyHubEvent, SpotifyHubOutcome, SpotifyHubResult,
    SpotifyHubSnapshot,
};
use state::PreviewGrant;
pub use state::SpotifyHubState;

const PREVIEW_LIFETIME: Duration = Duration::from_secs(5 * 60);

#[tauri::command]
pub async fn spotify_hub_check_updates() -> Result<models::SpotifyHubReleases, String> {
    let (cli, marketplace) =
        tokio::try_join!(release::resolve_cli(), release::resolve_marketplace())?;
    Ok(models::SpotifyHubReleases {
        cli: cli.version,
        marketplace: marketplace.version,
    })
}

#[tauri::command]
pub async fn spotify_hub_get_state(
    state: State<'_, SpotifyHubState>,
) -> Result<SpotifyHubSnapshot, String> {
    let detection = detection::detect().await?;
    Ok(detection.snapshot(state.last_outcome(), state.active()))
}

#[tauri::command]
pub async fn spotify_hub_preview_purge(
    state: State<'_, SpotifyHubState>,
) -> Result<PurgePreview, String> {
    let detection = detection::detect().await?;
    let token = Uuid::new_v4().simple().to_string();
    let expires_at = unix_millis(SystemTime::now() + PREVIEW_LIFETIME);
    state.put_preview(PreviewGrant {
        token: token.clone(),
        fingerprint: detection.fingerprint(),
        expires_at,
    });

    let mut detected_variants = Vec::new();
    if detection.desktop.installed {
        detected_variants.push(match &detection.desktop.version {
            Some(version) => format!("Spotify Desktop {version}"),
            None => "Spotify Desktop".into(),
        });
    }
    if detection.store.installed {
        detected_variants.push(match &detection.store.version {
            Some(version) => format!("Spotify (Microsoft Store) {version}"),
            None => "Spotify (Microsoft Store)".into(),
        });
    }
    if detection.spicetify.installed {
        detected_variants.push(match &detection.spicetify.version {
            Some(version) => format!("Spicetify {version}"),
            None => "Spicetify".into(),
        });
    }

    let mut categories = vec![
        "Spotify application files".into(),
        "Offline downloads and caches".into(),
        "Session and local settings".into(),
        "Spicetify, Marketplace, themes and extensions".into(),
        "Spotify shortcuts and allowlisted registry entries".into(),
    ];
    if detection.store.installed {
        categories.push("Current user's Microsoft Store package".into());
    }
    let mut warnings = vec![
        "This removes local downloads, sessions and settings. Completed deletions cannot be undone."
            .into(),
        "Your Spotify account, playlists and cloud library are not deleted.".into(),
        "Only this Windows profile and the detected application are in scope.".into(),
    ];
    if detection.unknown_desktop_registration {
        warnings.push(
            "An unrecognized Spotify binary was detected outside known paths and will be left untouched."
                .into(),
        );
    }
    Ok(PurgePreview {
        token,
        detected_variants,
        categories,
        warnings,
        expires_at,
    })
}

#[tauri::command]
pub async fn spotify_hub_run(
    app: AppHandle,
    state: State<'_, SpotifyHubState>,
    jobs: State<'_, Jobs>,
    cleanup: State<'_, Cleanup>,
    action: SpotifyHubAction,
    purge_token: Option<String>,
    on_event: Channel<SpotifyHubEvent>,
) -> Result<SpotifyHubOutcome, String> {
    let detection = detection::detect().await?;
    let job_id = format!("spotify-hub-{}", Uuid::new_v4().simple());
    let app_job = jobs.start_exclusive(&job_id)?;

    if action == SpotifyHubAction::PurgeAll {
        let token = purge_token.as_deref().ok_or(
            "A fresh purge preview and the exact REMOVE SPOTIFY confirmation are required.",
        )?;
        let grant = state.take_preview(token).ok_or(
            "The purge preview is invalid or was already used. Review the removal list again.",
        )?;
        if unix_millis(SystemTime::now()) > grant.expires_at {
            return Err("The purge preview expired. Review the removal list again.".into());
        }
        if grant.fingerprint != detection.fingerprint() {
            return Err(
                "Spotify's detected state changed after the preview. Review the removal list again."
                    .into(),
            );
        }
    } else if purge_token.is_some() {
        return Err("A purge token is accepted only for Full Uninstall.".into());
    }

    let cancellation = state.begin(job_id.clone(), action)?;
    let reporter = runner::Reporter {
        job_id: &job_id,
        channel: &on_event,
        state: &state,
    };
    reporter.stage(models::SpotifyHubStage::Preparing);

    let result = match action {
        SpotifyHubAction::InstallSpicetify => {
            actions::install(
                &app,
                &detection,
                &app_job,
                &cancellation,
                &reporter,
                &cleanup,
            )
            .await
        }
        SpotifyHubAction::RestoreSpotify => {
            actions::restore(&detection, &app_job, &cancellation, &reporter).await
        }
        SpotifyHubAction::PurgeAll => {
            actions::purge(&detection, &app_job, &cancellation, &reporter).await
        }
    };

    match result {
        Ok(outcome) => {
            state.finish(outcome.clone());
            Ok(outcome)
        }
        Err(error) if error == CANCELLED || cancellation.is_cancelled() => {
            let outcome = SpotifyHubOutcome::new(SpotifyHubResult::Cancelled, job_id, action, None);
            state.finish(outcome.clone());
            Ok(outcome)
        }
        Err(error) => {
            state.fail(&job_id);
            Err(error)
        }
    }
}

#[tauri::command(async)]
pub fn spotify_hub_cancel(
    state: State<'_, SpotifyHubState>,
    jobs: State<'_, Jobs>,
    job_id: String,
) {
    state.cancel(&job_id);
    jobs.cancel(&job_id);
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_event_fields_are_camel_case() {
        let value = serde_json::to_value(SpotifyHubEvent::Stage {
            job_id: "job".into(),
            stage: models::SpotifyHubStage::Preparing,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "event": "stage",
                "data": { "jobId": "job", "stage": "preparing" }
            })
        );

        let line = serde_json::to_value(SpotifyHubEvent::Line {
            job_id: "job".into(),
            text: "Patching files".into(),
            replace: true,
        })
        .unwrap();
        assert_eq!(
            line,
            serde_json::json!({
                "event": "line",
                "data": {
                    "jobId": "job",
                    "text": "Patching files",
                    "replace": true
                }
            })
        );
    }
}
