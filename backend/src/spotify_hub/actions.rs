use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::apps::process::{self, ERROR_CANCELLED, encode_command};
use crate::apps::{Cleanup, JobHandle};
use crate::download::{CANCELLED, err};

use super::detection::{Detection, KnownPaths};
use super::filesystem::{
    DirectorySwap, add_user_path_entry, remove_allowlisted, remove_user_path_entry,
    validate_no_reparse_points,
};
use super::models::{SpotifyHubAction, SpotifyHubOutcome, SpotifyHubResult, SpotifyHubStage};
use super::release;
use super::runner::{Reporter, run_program, stop_spotify_processes};
use super::state::Cancellation;

fn replace_release(installed: Option<&str>, latest: &str) -> bool {
    match (
        installed.and_then(|version| semver::Version::parse(version).ok()),
        semver::Version::parse(latest).ok(),
    ) {
        (Some(installed), Some(latest)) => installed <= latest,
        _ => true,
    }
}

pub async fn install(
    app: &AppHandle,
    detection: &Detection,
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
    cleanup: &Cleanup,
) -> Result<SpotifyHubOutcome, String> {
    if !detection.desktop.installed {
        return Err("Install Spotify Desktop before installing Spicetify.".into());
    }
    reporter.stage(SpotifyHubStage::Preparing);
    reporter.line("Resolving official stable Spicetify and Marketplace releases…");
    let (cli_release, marketplace_release) =
        tokio::try_join!(release::resolve_cli(), release::resolve_marketplace())?;
    if cancellation.is_cancelled() {
        return Ok(cancelled(
            reporter.job_id,
            SpotifyHubAction::InstallSpicetify,
        ));
    }
    reporter.line(format!(
        "Selected stable Spicetify {}.",
        cli_release.version
    ));
    reporter.line(format!(
        "Selected stable Marketplace {}.",
        marketplace_release.version
    ));

    // Staging lives below LOCALAPPDATA and is registered for next-startup
    // cleanup before the first network byte is written.
    let work = detection
        .paths
        .local
        .join("MakeYourLifeEasier")
        .join("spotify-hub")
        .join(reporter.job_id);
    cleanup.add(work.clone());
    tokio::fs::create_dir_all(&work).await.map_err(err)?;
    let cli_zip = work.join("spicetify.zip");
    let marketplace_zip = work.join("marketplace.zip");

    reporter.stage(SpotifyHubStage::Downloading);
    reporter.line("Downloading Spicetify from its official GitHub release…");
    release::download_verified(&cli_release, &cli_zip, cancellation, |done, total| {
        if let Some(total) = total.filter(|value| *value > 0) {
            reporter.progress((done as f64 / total as f64) * 0.45);
        }
    })
    .await?;
    reporter.line("Downloading Marketplace from its official GitHub release…");
    release::download_verified(
        &marketplace_release,
        &marketplace_zip,
        cancellation,
        |done, total| {
            if let Some(total) = total.filter(|value| *value > 0) {
                reporter.progress(0.45 + (done as f64 / total as f64) * 0.30);
            }
        },
    )
    .await?;
    reporter.stage(SpotifyHubStage::Verifying);
    reporter.line("Both GitHub SHA-256 digests are valid.");

    let unpacked_cli = work.join("cli-unpacked");
    let unpacked_marketplace = work.join("marketplace-unpacked");
    extract(
        &cli_zip,
        &unpacked_cli,
        cancellation.clone(),
        reporter,
        0.75,
        0.10,
    )
    .await?;
    extract(
        &marketplace_zip,
        &unpacked_marketplace,
        cancellation.clone(),
        reporter,
        0.85,
        0.10,
    )
    .await?;
    if !unpacked_cli.join("spicetify.exe").is_file() {
        return Err(
            "The verified CLI archive does not contain spicetify.exe at its expected path.".into(),
        );
    }
    let marketplace_payload = unpacked_marketplace.join("marketplace-dist");
    if !marketplace_payload.is_dir()
        || !(marketplace_payload.join("manifest.json").is_file()
            || marketplace_payload.join("index.js").is_file())
    {
        return Err("The verified Marketplace archive has an unexpected layout.".into());
    }
    if cancellation.is_cancelled() {
        return Ok(cancelled(
            reporter.job_id,
            SpotifyHubAction::InstallSpicetify,
        ));
    }

    reporter.stage(SpotifyHubStage::Installing);
    let cli_stage = detection
        .paths
        .local
        .join(format!(".spicetify.myle-stage-{}", reporter.job_id));
    // Staged folders are renamed into place on success; if a later step fails
    // they would otherwise be left behind next to the real installation.
    cleanup.add(cli_stage.clone());
    move_directory(&unpacked_cli, &cli_stage)?;
    let marketplace_parent = detection
        .paths
        .marketplace
        .parent()
        .ok_or("Marketplace target has no parent.")?;
    std::fs::create_dir_all(marketplace_parent).map_err(err)?;
    let marketplace_stage =
        marketplace_parent.join(format!(".marketplace.myle-stage-{}", reporter.job_id));
    cleanup.add(marketplace_stage.clone());
    move_directory(&marketplace_payload, &marketplace_stage)?;
    super::installed::write_version(&marketplace_stage, &marketplace_release.version)?;

    let config_file = detection.paths.config.join("config-xpui.ini");
    let previous_config = std::fs::read(&config_file).ok();
    let active_theme = previous_config
        .as_deref()
        .and_then(current_theme)
        .unwrap_or_default();
    let preserve_custom_theme =
        !active_theme.is_empty() && !active_theme.eq_ignore_ascii_case("marketplace");

    let installed_cli = detection
        .spicetify
        .healthy
        .then_some(detection.spicetify.version.as_deref())
        .flatten();
    let cli_version = if replace_release(installed_cli, &cli_release.version) {
        cli_release.version.as_str()
    } else {
        installed_cli.expect("only a known newer version is preserved")
    };
    let marketplace_version = if replace_release(
        detection.marketplace.version.as_deref(),
        &marketplace_release.version,
    ) {
        marketplace_release.version.as_str()
    } else {
        detection
            .marketplace
            .version
            .as_deref()
            .expect("only a known newer version is preserved")
    };
    let mut cli_swap = if cli_version == cli_release.version {
        Some(DirectorySwap::apply(
            &cli_stage,
            &detection.paths.cli,
            reporter.job_id,
        )?)
    } else {
        reporter.line(format!(
            "Preserving newer installed Spicetify {cli_version}."
        ));
        None
    };
    let mut marketplace_swap = if marketplace_version == marketplace_release.version {
        match DirectorySwap::apply(
            &marketplace_stage,
            &detection.paths.marketplace,
            reporter.job_id,
        ) {
            Ok(swap) => Some(swap),
            Err(error) => {
                if let Some(swap) = &mut cli_swap {
                    let _ = swap.rollback();
                }
                return Err(error);
            }
        }
    } else {
        reporter.line(format!(
            "Preserving newer installed Marketplace {marketplace_version}."
        ));
        None
    };

    let mut theme_swap = if preserve_custom_theme {
        reporter.line(format!("Preserving active custom theme “{active_theme}”."));
        None
    } else {
        let bundled = bundled_theme(app)?;
        let parent = detection
            .paths
            .placeholder_theme
            .parent()
            .ok_or("Theme target has no parent.")?;
        std::fs::create_dir_all(parent).map_err(err)?;
        let stage = parent.join(format!(".marketplace-theme.myle-stage-{}", reporter.job_id));
        cleanup.add(stage.clone());
        copy_directory(&bundled, &stage)?;
        Some(DirectorySwap::apply(
            &stage,
            &detection.paths.placeholder_theme,
            reporter.job_id,
        )?)
    };

    let configured = configure_spicetify(
        detection,
        job,
        cancellation,
        reporter,
        preserve_custom_theme,
    )
    .await;
    if let Err(error) = configured {
        best_effort_restore_after_failed_apply(detection, reporter).await;
        restore_file(&config_file, previous_config.as_deref());
        if let Some(swap) = &mut theme_swap {
            let _ = swap.rollback();
        }
        if let Some(swap) = &mut marketplace_swap {
            let _ = swap.rollback();
        }
        if let Some(swap) = &mut cli_swap {
            let _ = swap.rollback();
        }
        if error == CANCELLED || cancellation.is_cancelled() {
            return Ok(cancelled(
                reporter.job_id,
                SpotifyHubAction::InstallSpicetify,
            ));
        }
        return Err(format!(
            "Spicetify configuration failed and the previous installation was restored: {error}"
        ));
    }

    if let Err(error) = add_user_path_entry(&detection.paths.cli) {
        best_effort_restore_after_failed_apply(detection, reporter).await;
        restore_file(&config_file, previous_config.as_deref());
        if let Some(swap) = &mut theme_swap {
            let _ = swap.rollback();
        }
        if let Some(swap) = &mut marketplace_swap {
            let _ = swap.rollback();
        }
        if let Some(swap) = &mut cli_swap {
            let _ = swap.rollback();
        }
        return Err(format!(
            "Could not safely add Spicetify to the user PATH: {error}"
        ));
    }
    let mut stale_backups = Vec::new();
    if let Some(swap) = theme_swap
        && let Some(path) = swap.commit()
    {
        stale_backups.push(path);
    }
    if let Some(swap) = marketplace_swap
        && let Some(path) = swap.commit()
    {
        stale_backups.push(path);
    }
    if let Some(swap) = cli_swap
        && let Some(path) = swap.commit()
    {
        stale_backups.push(path);
    }
    for path in stale_backups {
        reporter.line(format!(
            "A locked previous-version backup will be removed on app exit: {}",
            path.display()
        ));
        cleanup.add(path);
    }
    reporter.stage(SpotifyHubStage::Finalizing);
    reporter.progress(1.0);
    reporter.line("Spicetify and Marketplace are ready.");
    Ok(SpotifyHubOutcome::new(
        SpotifyHubResult::Done,
        reporter.job_id,
        SpotifyHubAction::InstallSpicetify,
        Some(format!(
            "Installed Spicetify {} and Marketplace {}.",
            cli_version, marketplace_version
        )),
    ))
}

async fn configure_spicetify(
    detection: &Detection,
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
    preserve_custom_theme: bool,
) -> Result<(), String> {
    let exe = detection.paths.cli_exe();
    for args in [
        vec!["config", "custom_apps", "marketplace"],
        vec!["config", "inject_css", "1", "replace_colors", "1"],
    ] {
        ensure_success(&exe, &args, job, cancellation, reporter).await?;
    }
    if !preserve_custom_theme {
        ensure_success(
            &exe,
            &["config", "current_theme", "marketplace"],
            job,
            cancellation,
            reporter,
        )
        .await?;
    }
    // This is the verified local executable and a fixed argument list. It is
    // intentionally unelevated and never invokes a downloaded script.
    let config = std::fs::read(detection.paths.config.join("config-xpui.ini")).map_err(err)?;
    let apps_dir = spotify_apps_dir(&config, &detection.paths.desktop_dir());
    let args = backup_apply_args(&apps_dir)?;
    if args.first() == Some(&"restore") {
        reporter.line("Spotify has no stock app packages; restoring the existing backup before rebuilding it with the new CLI…");
    }
    ensure_success(&exe, args, job, cancellation, reporter).await
}

/// Match the official CLI updater's IsBackupable check. A Spotify update may
/// leave both .spa packages and extracted directories: back up those new stock
/// packages without restoring an older Spotify version over them. A fully
/// customized installation needs restore first, then preprocessing by the new
/// CLI. The CLI itself checks backup presence/version before restoring.
fn backup_apply_args(apps_dir: &Path) -> Result<&'static [&'static str], String> {
    for entry in std::fs::read_dir(apps_dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_name().to_string_lossy().ends_with(".spa")
            && !entry.file_type().map_err(err)?.is_dir()
        {
            return Ok(&["backup", "apply"]);
        }
    }
    Ok(&["restore", "backup", "apply"])
}

/// Honor the path the CLI will use, including %VAR% references. Like its
/// InitPaths, fall back to detected Desktop Spotify when the configured Apps
/// directory does not exist. Do not inspect a different installation's state.
fn spotify_apps_dir(config: &[u8], default_spotify: &Path) -> PathBuf {
    let mut setting = false;
    for line in String::from_utf8_lossy(config).lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            setting = line.eq_ignore_ascii_case("[Setting]");
            continue;
        }
        if !setting {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("spotify_path")
        {
            let value = value.trim().trim_matches(['"', '\'']);
            if !value.is_empty() {
                let configured = PathBuf::from(crate::apps::custom::expand_env(value)).join("Apps");
                if configured.is_dir() {
                    return configured;
                }
            }
            break;
        }
    }
    default_spotify.join("Apps")
}

/// If applying the new build changed Spotify and a later install step fails,
/// restore vanilla Spotify while the verified new CLI and its recovery files
/// are still present. This is deliberately best-effort: rollback of the old
/// CLI/config must continue even when Spotify itself is locked.
async fn best_effort_restore_after_failed_apply(detection: &Detection, reporter: &Reporter<'_>) {
    let exe = detection.paths.cli_exe();
    if !exe.is_file() {
        return;
    }
    reporter.line("Install did not complete; attempting a safety restore before rollback…");
    let result = crate::apps::process::hidden(&exe)
        .arg("restore")
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match result {
        Ok(output) if output.status.success() => {
            reporter.line("Safety restore completed; rolling back the previous CLI files.")
        }
        _ => reporter.line(
            "Safety restore could not complete; recovery data is being preserved where possible.",
        ),
    }
}

pub async fn restore(
    detection: &Detection,
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
) -> Result<SpotifyHubOutcome, String> {
    let exe = detection.paths.cli_exe();
    if !exe.is_file() {
        return Err(
            "Spicetify CLI is missing, so Spotify cannot be safely restored. Recovery files were left untouched."
                .into(),
        );
    }
    validate_no_reparse_points(&detection.paths.cli)
        .map_err(|error| format!("Spicetify safety validation failed: {error}"))?;
    reporter.stage(SpotifyHubStage::Restoring);
    reporter.line("Running the official unelevated `spicetify restore` first…");
    let code = run_program(
        &exe,
        &[OsString::from("restore")],
        job,
        cancellation,
        reporter,
    )
    .await?;
    if code != 0 {
        return Err(format!(
            "Spicetify restore exited with code {code}. Recovery files were kept."
        ));
    }
    if cancellation.is_cancelled() {
        return Ok(cancelled(reporter.job_id, SpotifyHubAction::RestoreSpotify));
    }

    reporter.stage(SpotifyHubStage::Cleaning);
    let allowlist = vec![detection.paths.cli.clone(), detection.paths.config.clone()];
    let mut failures = Vec::new();
    for target in &allowlist {
        if let Err(error) = remove_allowlisted(target, &allowlist) {
            failures.push(format!("{}: {error}", target.display()));
        }
    }
    if let Err(error) = remove_user_path_entry(&detection.paths.cli) {
        failures.push(format!("PATH: {error}"));
    }
    reporter.stage(SpotifyHubStage::Finalizing);
    reporter.progress(1.0);
    if failures.is_empty() {
        reporter.line("Spotify is restored to its stock state.");
        Ok(SpotifyHubOutcome::new(
            SpotifyHubResult::Done,
            reporter.job_id,
            SpotifyHubAction::RestoreSpotify,
            Some("Spotify was restored and Spicetify data was removed.".into()),
        ))
    } else {
        for failure in &failures {
            reporter.line(format!("Cleanup warning: {failure}"));
        }
        Ok(SpotifyHubOutcome::new(
            SpotifyHubResult::Partial,
            reporter.job_id,
            SpotifyHubAction::RestoreSpotify,
            Some("Spotify was restored, but some Spicetify files could not be removed.".into()),
        ))
    }
}

pub async fn purge(
    detection: &Detection,
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
) -> Result<SpotifyHubOutcome, String> {
    reporter.stage(SpotifyHubStage::Preparing);
    stop_spotify_processes(reporter).await;
    let mut changed = false;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    reporter.stage(SpotifyHubStage::Uninstalling);
    if detection.desktop.installed && detection.paths.desktop_spotify.is_file() {
        match validate_no_reparse_points(&detection.paths.desktop_dir()) {
            Ok(()) => {
                reporter.line("Requesting the Spotify Desktop uninstaller…");
                let code = run_program(
                    &detection.paths.desktop_spotify,
                    &[OsString::from("/UNINSTALL"), OsString::from("/SILENT")],
                    job,
                    cancellation,
                    reporter,
                )
                .await;
                match code {
                    Ok(0) => changed = true,
                    Ok(code) => reporter.line(format!(
                        "Spotify's uninstaller exited with code {code}; allowlisted cleanup will continue."
                    )),
                    Err(error) if error == CANCELLED => {
                        return Ok(partial_or_cancelled(reporter.job_id, changed));
                    }
                    Err(error) => {
                        reporter.line(format!("Spotify's uninstaller could not run: {error}"))
                    }
                }
            }
            Err(error) => {
                let error = format!("Safety validation failed: {error}");
                reporter.line(format!(
                    "Spotify's uninstaller was not run because its folder is unsafe: {error}"
                ));
                failures.push((detection.paths.desktop_dir(), error));
            }
        }
    }
    if cancellation.is_cancelled() {
        return Ok(partial_or_cancelled(reporter.job_id, changed));
    }
    if detection.store.installed {
        reporter.line("Removing the current user's Microsoft Store Spotify package…");
        let args = [
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-AppxPackage -Name 'SpotifyAB.SpotifyMusic' | Remove-AppxPackage",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        match run_program(
            Path::new("powershell.exe"),
            &args,
            job,
            cancellation,
            reporter,
        )
        .await
        {
            Ok(0) => changed = true,
            Ok(code) => reporter.line(format!("Store package removal exited with code {code}.")),
            Err(error) if error == CANCELLED => {
                return Ok(partial_or_cancelled(reporter.job_id, changed));
            }
            Err(error) => reporter.line(format!("Store package removal failed: {error}")),
        }
    }
    if cancellation.is_cancelled() {
        return Ok(partial_or_cancelled(reporter.job_id, changed));
    }

    reporter.stage(SpotifyHubStage::Cleaning);
    reset_update_acl(&detection.paths, reporter).await;
    let allowlist = detection.paths.purge_targets();
    let total = allowlist.len().max(1);
    for (index, target) in allowlist.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Ok(partial_or_cancelled(reporter.job_id, changed));
        }
        let existed = target.exists() || std::fs::symlink_metadata(target).is_ok();
        match remove_allowlisted(target, &allowlist) {
            Ok(()) => {
                if existed {
                    changed = true;
                    reporter.line(format!("Removed {}.", target.display()));
                }
            }
            Err(error) => {
                reporter.line(format!("Could not remove {}: {error}", target.display()));
                failures.push((target.clone(), error));
            }
        }
        reporter.progress((index + 1) as f64 / total as f64 * 0.9);
    }

    if remove_user_registry_entries(reporter) {
        changed = true;
    }
    if let Err(error) = remove_user_path_entry(&detection.paths.cli) {
        reporter.line(format!("Could not update PATH: {error}"));
    }

    let admin_paths: Vec<PathBuf> = failures
        .iter()
        .filter(|(_, error)| admin_retry_worthwhile(error))
        .map(|(path, _)| path.clone())
        .collect();
    let machine_residue = machine_spotify_registration_exists();
    if !admin_paths.is_empty() || machine_residue {
        reporter.line("Administrator approval is required for protected Spotify remnants.");
        match elevated_cleanup(&admin_paths, machine_residue).await {
            Ok(ERROR_CANCELLED) => {
                return Ok(SpotifyHubOutcome::new(
                    SpotifyHubResult::NeedsAdmin,
                    reporter.job_id,
                    SpotifyHubAction::PurgeAll,
                    Some("Administrator approval was declined; protected remnants remain.".into()),
                ));
            }
            Ok(0) => {
                failures
                    .retain(|(path, _)| path.exists() || std::fs::symlink_metadata(path).is_ok());
            }
            Ok(code) => reporter.line(format!("Elevated cleanup exited with code {code}.")),
            Err(error) => reporter.line(format!("Elevated cleanup failed: {error}")),
        }
    }

    reporter.stage(SpotifyHubStage::Finalizing);
    reporter.progress(1.0);
    if failures.is_empty() && !machine_spotify_registration_exists() {
        reporter.line("Spotify and its local data were removed from this Windows profile.");
        Ok(SpotifyHubOutcome::new(
            SpotifyHubResult::Done,
            reporter.job_id,
            SpotifyHubAction::PurgeAll,
            Some("Local Spotify installations and data were removed. Your account and cloud library were not affected.".into()),
        ))
    } else {
        Ok(SpotifyHubOutcome::new(
            SpotifyHubResult::Partial,
            reporter.job_id,
            SpotifyHubAction::PurgeAll,
            Some(if changed {
                "Spotify was partially removed; locked, protected, or unrecognized items were left in place."
                    .into()
            } else {
                "No allowlisted Spotify data could be removed.".into()
            }),
        ))
    }
}

async fn extract(
    archive: &Path,
    destination: &Path,
    cancellation: Cancellation,
    reporter: &Reporter<'_>,
    base: f64,
    span: f64,
) -> Result<(), String> {
    let archive = archive.to_path_buf();
    let destination = destination.to_path_buf();
    let channel = reporter.channel.clone();
    let state = reporter.state.clone();
    let job_id = reporter.job_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        release::extract_verified_zip(&archive, &destination, &cancellation, |done, total| {
            if total > 0 {
                let fraction = base + (done as f64 / total as f64) * span;
                state.update(&job_id, SpotifyHubStage::Installing, Some(fraction));
                let _ = channel.send(super::models::SpotifyHubEvent::Progress {
                    job_id: job_id.clone(),
                    fraction,
                });
            }
        })
    })
    .await
    .map_err(err)?
}

async fn ensure_success(
    exe: &Path,
    args: &[&str],
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
) -> Result<(), String> {
    let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
    let code = run_program(exe, &owned, job, cancellation, reporter).await?;
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "spicetify {} exited with code {code}",
            args.join(" ")
        ))
    }
}

fn current_theme(config: &[u8]) -> Option<String> {
    String::from_utf8_lossy(config).lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("current_theme")
            .then(|| value.trim().to_string())
    })
}

fn bundled_theme(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app
        .path()
        .resource_dir()
        .map_err(err)?
        .join("spicetify")
        .join("placeholder-theme");
    if !path.join("color.ini").is_file() || !path.join("user.css").is_file() {
        return Err("The bundled Marketplace placeholder theme is missing.".into());
    }
    Ok(path)
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(err)?;
    }
    std::fs::create_dir_all(destination).map_err(err)?;
    for entry in std::fs::read_dir(source).map_err(err)? {
        let entry = entry.map_err(err)?;
        let target = destination.join(entry.file_name());
        if entry.file_type().map_err(err)?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(err)?;
        }
    }
    Ok(())
}

fn move_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(err)?;
    }
    std::fs::rename(source, destination).or_else(|_| {
        copy_directory(source, destination)?;
        std::fs::remove_dir_all(source).map_err(err)
    })
}

fn restore_file(path: &Path, previous: Option<&[u8]>) {
    match previous {
        Some(bytes) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, bytes);
        }
        None => drop(std::fs::remove_file(path)),
    }
}

fn cancelled(job_id: &str, action: SpotifyHubAction) -> SpotifyHubOutcome {
    SpotifyHubOutcome::new(SpotifyHubResult::Cancelled, job_id, action, None)
}

fn partial_or_cancelled(job_id: &str, changed: bool) -> SpotifyHubOutcome {
    if changed {
        SpotifyHubOutcome::new(
            SpotifyHubResult::Partial,
            job_id,
            SpotifyHubAction::PurgeAll,
            Some("Stopped between purge stages. Completed removals cannot be undone.".into()),
        )
    } else {
        cancelled(job_id, SpotifyHubAction::PurgeAll)
    }
}

async fn reset_update_acl(paths: &KnownPaths, reporter: &Reporter<'_>) {
    let update = paths.local_spotify.join("Update");
    if !update.exists() {
        return;
    }
    if let Err(error) = validate_no_reparse_points(&paths.local_spotify) {
        reporter.line(format!(
            "Spotify update-folder ACL reset was skipped by safety validation: {error}"
        ));
        return;
    }
    let output = crate::apps::process::hidden(crate::apps::process::system32("icacls.exe"))
        .arg(&update)
        .args(["/reset", "/T", "/C", "/Q"])
        .output()
        .await;
    if !matches!(output, Ok(ref value) if value.status.success()) {
        reporter.line("Spotify update-folder restrictions could not be reset in the user pass.");
    }
}

fn remove_user_registry_entries(reporter: &Reporter<'_>) -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut changed = false;
    for key in [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Spotify",
        r"Software\Spotify",
        r"Software\Classes\spotify",
    ] {
        match hkcu.delete_subkey_all(key) {
            Ok(()) => changed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => reporter.line(format!("Registry cleanup warning for {key}: {error}")),
        }
    }
    if let Ok(run) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_READ | winreg::enums::KEY_WRITE,
    ) {
        match run.delete_value("Spotify") {
            Ok(()) => changed = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => reporter.line(format!(
                "Registry cleanup warning for Run\\Spotify: {error}"
            )),
        }
    }
    changed
}

fn machine_spotify_registration_exists() -> bool {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Spotify",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Spotify",
    ]
    .iter()
    .any(|key| hklm.open_subkey_with_flags(key, KEY_READ).is_ok())
}

fn admin_retry_worthwhile(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    !lower.contains("safety validation failed")
        && (lower.contains("access is denied")
            || lower.contains("permission denied")
            || lower.contains("os error 5"))
}

async fn elevated_cleanup(paths: &[PathBuf], machine_registry: bool) -> Result<i32, String> {
    // The paths originate solely from KnownPaths::purge_targets and have
    // already passed exact allowlist validation. They are encoded as literal
    // PowerShell strings; no frontend-provided command/path reaches this pass.
    let literals = paths
        .iter()
        .map(|path| ps_quote(&path.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join(",");
    let mut script = format!(
        "$ErrorActionPreference='SilentlyContinue'; foreach($p in @({literals})) {{ if(Test-Path -LiteralPath $p) {{ try {{ $root=Get-Item -LiteralPath $p -Force -ErrorAction Stop; $hasLink=$false; $cursor=$root; while($null -ne $cursor) {{ if($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) {{ $hasLink=$true; break }}; $cursor=$cursor.Parent }}; $all=@(Get-ChildItem -LiteralPath $p -Force -Recurse -ErrorAction Stop); $links=@($all | Where-Object {{ $_.Attributes -band [IO.FileAttributes]::ReparsePoint }}); if(-not ($hasLink -or $links.Count)) {{ & takeown.exe /F $p /R /D Y | Out-Null; & icacls.exe $p /reset /T /C /Q | Out-Null; Remove-Item -LiteralPath $p -Recurse -Force }} }} catch {{ }} }} }};"
    );
    if machine_registry {
        script.push_str(
            "Remove-Item -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Spotify' -Recurse -Force; Remove-Item -LiteralPath 'Registry::HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Spotify' -Recurse -Force;",
        );
    }
    script.push_str("exit 0");
    let args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-EncodedCommand".to_string(),
        encode_command(&script),
    ];
    process::run_elevated("powershell.exe", &args, true).await
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_installed_component_is_preserved_when_the_other_component_updates() {
        assert!(!replace_release(Some("2.46.0"), "2.45.3"));
        assert!(!replace_release(Some("1.0.12"), "1.0.11"));
        assert!(replace_release(Some("2.45.1"), "2.45.3"));
        assert!(replace_release(Some("1.0.10"), "1.0.11"));
        assert!(replace_release(Some("2.45.3"), "2.45.3"));
        assert!(replace_release(None, "2.45.3"));
    }

    struct SpotifyAppsFixture(PathBuf);

    impl SpotifyAppsFixture {
        fn new(stock: bool, modified: bool) -> Self {
            let root =
                std::env::temp_dir().join(format!("myle-spicetify-apps-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            if stock {
                std::fs::write(root.join("xpui.spa"), b"original Spotify package").unwrap();
            }
            if modified {
                std::fs::create_dir(root.join("xpui")).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for SpotifyAppsFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn updating_modified_spotify_restores_before_rebuilding_backup() {
        let apps = SpotifyAppsFixture::new(false, true);
        assert_eq!(
            backup_apply_args(&apps.0).unwrap(),
            ["restore", "backup", "apply"]
        );
    }

    #[test]
    fn fresh_or_updated_spotify_backs_up_without_restoring_old_files() {
        for modified in [false, true] {
            let apps = SpotifyAppsFixture::new(true, modified);
            assert_eq!(backup_apply_args(&apps.0).unwrap(), ["backup", "apply"]);
        }
    }

    #[test]
    fn a_directory_named_spa_is_not_an_original_package() {
        let apps = SpotifyAppsFixture::new(false, true);
        std::fs::create_dir(apps.0.join("not-a-package.spa")).unwrap();
        assert_eq!(
            backup_apply_args(&apps.0).unwrap(),
            ["restore", "backup", "apply"]
        );
    }

    #[test]
    fn unreadable_spotify_apps_are_not_assumed_safe_to_back_up() {
        let apps = SpotifyAppsFixture::new(false, false);
        assert!(backup_apply_args(&apps.0.join("missing")).is_err());
    }

    #[test]
    fn backup_policy_uses_the_configured_spotify_installation() {
        let root = SpotifyAppsFixture::new(false, false);
        let apps = root.0.join("Apps");
        std::fs::create_dir(&apps).unwrap();
        let default = root.0.join("DefaultSpotify");
        let config = format!(
            "[Backup]\nspotify_path=ignored\n[Setting]\nspotify_path = {}\n",
            root.0.display()
        );
        assert_eq!(spotify_apps_dir(config.as_bytes(), &default), apps);
    }

    #[test]
    fn missing_or_invalid_configured_path_uses_detected_desktop_spotify() {
        let root = SpotifyAppsFixture::new(false, false);
        for config in [
            "[Setting]\nspotify_path=\n".to_string(),
            format!(
                "[Setting]\nspotify_path={}\n",
                root.0.join("missing").display()
            ),
            format!("[Backup]\nspotify_path={}\n", root.0.display()),
        ] {
            assert_eq!(
                spotify_apps_dir(config.as_bytes(), &root.0),
                root.0.join("Apps")
            );
        }
    }

    #[test]
    fn existing_custom_theme_is_detected_and_preserved() {
        let config = b"[Setting]\ncurrent_theme = Dribbblish\ncolor_scheme = base\n";
        assert_eq!(current_theme(config).as_deref(), Some("Dribbblish"));
        assert_eq!(current_theme(b"[Setting]\ncolor_scheme=base"), None);
    }

    #[test]
    fn powershell_literals_cannot_break_out() {
        assert_eq!(ps_quote("C:\\it's\\Spotify"), "'C:\\it''s\\Spotify'");
    }

    #[test]
    fn only_acl_style_failures_request_elevation() {
        assert!(admin_retry_worthwhile("Access is denied. (os error 5)"));
        assert!(!admin_retry_worthwhile(
            "Refusing to follow a reparse point"
        ));
        assert!(!admin_retry_worthwhile("file is in use (os error 32)"));
    }
}
