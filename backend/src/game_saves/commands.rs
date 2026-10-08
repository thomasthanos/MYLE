use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::atomic;
use crate::cloud::onedrive as cloud;
use super::detection::{self, same_title};
use super::engine::{Engine, EngineOutput, failure_detail};
use super::models::{
    BackupSchedule, CloudProvider, CustomGame, DatabaseUpdate, DetectedFolder, GameFailure, GameRoot,
    GameSaveEntry, GameSaveStatus, GameSavesEvent, GameSavesOperationResult, GameSavesPageState,
    GameSavesScan, GameSavesSettings, OperationKind, OperationStage, RestorePathMapping,
    RestoreSelection, RootSource, RootStore, ScanMode, ScheduleWeekday, SyncedGameSaves,
};
use super::parser::{self, ApiGame, ApiOutput, ManifestMetadata};
use super::scan_cache::{self, CachedScan};
use super::settings;
use super::state::{GameSavesState, OperationHandle};
use super::undo;

/// Must stay in sync with `GAME_SAVES_TASK` in installer/src/product.rs: the
/// uninstaller deletes this task.
const TASK_NAME: &str = "MakeYourLifeEasier Game Saves Backup";

/// The parsed game database, keyed by the manifest's size and modification
/// time: parsing the 17 MB file on every scan cost a noticeable pause.
static MANIFEST: Mutex<Option<(u64, SystemTime, ManifestMetadata)>> = Mutex::new(None);

/// Held by the quick settings commands while they read, change and save the
/// settings file. They run on the async pool, so two clicks in a row could
/// otherwise both read the old file and the second save would drop the first.
static SETTINGS_EDIT: Mutex<()> = Mutex::new(());

fn settings_edit() -> std::sync::MutexGuard<'static, ()> {
    SETTINGS_EDIT.lock().unwrap_or_else(|p| p.into_inner())
}

/// Reads the settings afresh, lets `change` change them and saves them, all
/// under the edit lock. For the commands that wait on something first (a
/// folder picker, a scan, the task scheduler): what was saved meanwhile is
/// kept rather than overwritten by the copy they read before waiting.
fn edit_settings(
    app: &AppHandle,
    change: impl FnOnce(&mut GameSavesSettings) -> Result<(), String>,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    let mut value = settings::initialize(app)?;
    change(&mut value)?;
    settings::save(app, &value)?;
    Ok(value)
}

#[tauri::command]
pub async fn game_saves_get_state(
    app: AppHandle,
    state: State<'_, GameSavesState>,
) -> Result<GameSavesPageState, String> {
    let settings = settings::initialize(&app)?;
    let engine = Engine::for_app(&app).ok();
    let version = match &engine {
        Some(engine) => engine.version().await,
        None => None,
    };
    let undo_restore = undo::load_valid(&settings::safety_root(&app)?);
    let cached_scan = scan_cache::load(&settings::config_root(&app)?).map(|cached| {
        let due = scan_cache::discovery_due(&cached, &settings, parser::now());
        GameSavesScan {
            discovery_due: due,
            ..cached.scan
        }
    });
    // Actions address games by id: let them work from the saved list too,
    // before this session's first scan finishes.
    if state.cached_scan().is_none()
        && let Some(scan) = &cached_scan
    {
        state.cache_scan(scan.clone());
    }
    Ok(GameSavesPageState {
        database_games: settings.database_games,
        database_updated_at: settings.database_updated_at,
        settings,
        cloud_folders: detection::detect_cloud_folders(),
        engine_available: engine.is_some(),
        engine_version: version,
        active_operation: state.active_kind(),
        undo_restore,
        cached_scan,
    })
}

#[tauri::command]
pub async fn game_saves_pick_backup_folder(
    app: AppHandle,
    state: State<'_, GameSavesState>,
) -> Result<Option<GameSavesSettings>, String> {
    state.ensure_idle()?;
    let Some(path) = pick_folder(app.clone(), "Choose a Game Saves backup folder".into()).await?
    else {
        return Ok(None);
    };
    set_backup_folder(&app, &path).map(Some)
}

/// Makes `<cloud folder>\Make Your Life Easier\Game Saves Backups` the
/// backup folder, creating it. `path` must be one of the folders this app
/// detected for `provider`; without one, the user is asked where that
/// provider's folder is. `None` when that question was cancelled.
#[tauri::command]
pub async fn game_saves_use_cloud_folder(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    provider: CloudProvider,
    path: Option<String>,
) -> Result<Option<GameSavesSettings>, String> {
    state.ensure_idle()?;
    let root = match path {
        Some(path) => {
            let candidate = PathBuf::from(path.trim());
            let detected = tauri::async_runtime::spawn_blocking(detection::detect_cloud_folders)
                .await
                .map_err(|e| e.to_string())?;
            if !detected.iter().any(|folder| {
                folder.provider == provider
                    && settings::paths_equal(&candidate, Path::new(&folder.path))
            }) {
                return Err(format!(
                    "That {} folder is no longer there. Detect the cloud folders again.",
                    provider.name()
                ));
            }
            candidate
        }
        None => {
            let title = format!("Where is your {} folder?", provider.name());
            let Some(picked) = pick_folder(app.clone(), title).await? else {
                return Ok(None);
            };
            match provider {
                CloudProvider::GoogleDrive => detection::google_my_drive(&picked).unwrap_or(picked),
                _ => picked,
            }
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let folder = detection::cloud_backup_folder(&root);
        let value = settings::initialize(&app)?;
        // Checked before anything is created on disk (and again as it is set).
        settings::validate_backup_folder(&app, &folder, &value)?;
        fs::create_dir_all(&folder)
            .map_err(|e| format!("{} could not be created: {e}", folder.display()))?;
        set_backup_folder(&app, &folder).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn game_saves_open_backup_folder(app: AppHandle) -> Result<(), String> {
    let settings = settings::initialize(&app)?;
    let path = backup_folder(&settings)?;
    settings::validate_backup_folder(&app, &path, &settings)?;
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn game_saves_open_game_folder(
    state: State<'_, GameSavesState>,
    game_id: String,
) -> Result<(), String> {
    let game = state
        .find_game(&game_id)
        .ok_or("Scan again before opening a game folder.")?;
    let path = game
        .paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
        .ok_or("No local save folder is currently available for this game.")?;
    let folder = if path.is_dir() {
        path
    } else {
        path.parent()
            .ok_or("The save path has no parent folder.")?
            .to_path_buf()
    };
    tauri_plugin_opener::open_path(folder, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn game_saves_detect_cloud_folders() -> Vec<DetectedFolder> {
    detection::detect_cloud_folders()
}

#[tauri::command(async)]
pub fn game_saves_refresh_roots(
    app: AppHandle,
    state: State<'_, GameSavesState>,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let mut value = settings::initialize(&app)?;
    settings::merge_detected_roots(&mut value);
    validate_current_backup(&app, &value)?;
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command]
pub async fn game_saves_add_root(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    store: RootStore,
) -> Result<Option<GameSavesSettings>, String> {
    state.ensure_idle()?;
    let Some(path) = pick_folder(app.clone(), "Choose a game installation folder".into()).await?
    else {
        return Ok(None);
    };
    if !path.is_dir() {
        return Err("Choose an existing game installation folder.".into());
    }
    edit_settings(&app, |value| {
        if let Some(backup) = &value.backup_folder
            && settings::overlaps(&path, Path::new(backup))
        {
            return Err("A game installation folder cannot overlap the backup folder.".into());
        }
        let display = path.to_string_lossy().into_owned();
        let root = GameRoot {
            id: detection::stable_id(store.ludusavi_name(), &display),
            path: display,
            store,
            source: RootSource::Manual,
        };
        if !value.roots.iter().any(|known| known.id == root.id) {
            value.roots.push(root);
        }
        validate_current_backup(&app, value)
    })
    .map(Some)
}

#[tauri::command(async)]
pub fn game_saves_remove_root(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    root_id: String,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let mut value = settings::initialize(&app)?;
    let before = value.roots.len();
    value.roots.retain(|root| root.id != root_id);
    if value.roots.len() == before {
        return Err("That game installation folder no longer exists in settings.".into());
    }
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command]
pub async fn game_saves_set_schedule(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    schedule: BackupSchedule,
    time: String,
    weekday: ScheduleWeekday,
) -> Result<GameSavesSettings, String> {
    state.ensure_idle()?;
    if !settings::valid_schedule_time(&time) {
        return Err("Use a valid 24-hour schedule time in HH:mm format.".into());
    }
    let mut previous = None;
    let value = edit_settings(&app, |value| {
        previous = Some((value.schedule, value.schedule_time.clone(), value.schedule_weekday));
        value.schedule = schedule;
        value.schedule_time = time.clone();
        value.schedule_weekday = weekday;
        Ok(())
    })?;
    if let Err(error) = configure_scheduled_task(schedule, &time, weekday).await {
        // Keep the persisted settings and the Windows task in agreement,
        // unless another change of the schedule came in meanwhile.
        if let Some((old_schedule, old_time, old_weekday)) = previous {
            let _ = edit_settings(&app, |value| {
                if value.schedule == schedule && value.schedule_time == time && value.schedule_weekday == weekday {
                    value.schedule = old_schedule;
                    value.schedule_time = old_time;
                    value.schedule_weekday = old_weekday;
                }
                Ok(())
            });
        }
        return Err(error);
    }
    Ok(value)
}

#[tauri::command(async)]
pub fn game_saves_set_game_auto_backup(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    game_id: String,
    enabled: bool,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let game = state
        .find_game(&game_id)
        .ok_or("Scan again before changing automatic backup for this game.")?;
    let mut value = settings::initialize(&app)?;
    value
        .auto_backup_excluded_game_ids
        .retain(|known| known != &game_id);
    if !enabled {
        value.auto_backup_excluded_game_ids.push(game_id);
    }
    if let Some(custom) = value
        .custom_games
        .iter_mut()
        .find(|item| same_title(&item.name, &game.title))
    {
        custom.auto_backup = enabled;
    }
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command]
pub async fn game_saves_pick_folder(
    app: AppHandle,
    title: String,
) -> Result<Option<String>, String> {
    pick_folder(app, title)
        .await
        .map(|picked| picked.map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command(async)]
pub fn game_saves_upsert_custom_game(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    mut game: CustomGame,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    game.name = game.name.trim().to_string();
    if game.name.is_empty()
        || game.name.chars().count() > 200
        || game.name.chars().any(char::is_control)
    {
        return Err("Enter a game name between 1 and 200 characters.".into());
    }
    game.paths = game
        .paths
        .into_iter()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .collect();
    game.paths.sort_by_key(|path| path.to_ascii_lowercase());
    game.paths.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if game.paths.is_empty() || game.paths.len() > 32 {
        return Err("Choose between 1 and 32 save folders.".into());
    }
    for path in &game.paths {
        if !Path::new(path).is_dir() {
            return Err(format!("The save folder does not exist: {path}"));
        }
    }
    if let Some(install) = game.install_path.as_mut() {
        *install = install.trim().to_string();
        if install.is_empty() {
            game.install_path = None;
        } else if !Path::new(install).is_dir() {
            return Err(format!("The install folder does not exist: {install}"));
        }
    }

    let mut value = settings::initialize(&app)?;
    if let Some(backup) = value.backup_folder.as_deref() {
        for path in &game.paths {
            if settings::overlaps(Path::new(path), Path::new(backup)) {
                return Err("A custom save folder cannot overlap the backup folder.".into());
            }
        }
    }

    let existing = (!game.id.trim().is_empty())
        .then(|| {
            value
                .custom_games
                .iter()
                .position(|known| known.id == game.id)
        })
        .flatten();
    if game.id.trim().is_empty() {
        let identity = format!("{}\0{}", game.name, game.paths.join("\0"));
        game.id = detection::stable_id("custom", &identity);
    } else if existing.is_none() {
        return Err("For a new custom game, leave its id empty.".into());
    }
    // The scan identifies the game by its name; keep the per-game exclusion
    // in step with the switch in this dialog, or turning it back on here
    // would have no effect.
    let scanned_id = detection::stable_id("game", &game.name);
    value
        .auto_backup_excluded_game_ids
        .retain(|known| known != &scanned_id);
    if !game.auto_backup {
        value.auto_backup_excluded_game_ids.push(scanned_id);
    }
    if value.custom_games.iter().enumerate().any(|(index, known)| {
        Some(index) != existing && same_title(&known.name, &game.name)
    }) {
        return Err("A custom game with this name already exists.".into());
    }
    if let Some(index) = existing {
        value.custom_games[index] = game;
    } else {
        value.custom_games.push(game);
    }
    validate_current_backup(&app, &value)?;
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command(async)]
pub fn game_saves_remove_custom_game(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    game_id: String,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let mut value = settings::initialize(&app)?;
    let before = value.custom_games.len();
    value.custom_games.retain(|game| game.id != game_id);
    if before == value.custom_games.len() {
        return Err("That custom game no longer exists.".into());
    }
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command(async)]
pub fn game_saves_set_path_mapping(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    mut mapping: RestorePathMapping,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let game = state
        .find_game(&mapping.game_id)
        .ok_or("Scan again before setting a restore location.")?;
    mapping.source = mapping.source.trim().to_string();
    mapping.target = mapping.target.trim().to_string();
    if mapping.source.is_empty() || mapping.target.is_empty() {
        return Err("Both the original and replacement restore paths are required.".into());
    }
    if !Path::new(&mapping.target).is_dir() {
        return Err("Choose an existing replacement restore folder.".into());
    }
    let mut value = settings::initialize(&app)?;
    let original = game
        .paths
        .iter()
        .find(|path| settings::paths_equal(Path::new(path), Path::new(&mapping.source)))
        .cloned()
        .ok_or("The original restore path is stale. Scan again.")?;
    mapping.source = original;
    settings::validate_restore_target(&app, Path::new(&mapping.target), &value)?;
    value.path_mappings.retain(|known| {
        known.game_id != mapping.game_id || !known.source.eq_ignore_ascii_case(&mapping.source)
    });
    value.path_mappings.push(mapping);
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command(async)]
pub fn game_saves_remove_path_mapping(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    game_id: String,
    source: String,
) -> Result<GameSavesSettings, String> {
    let _edit = settings_edit();
    state.ensure_idle()?;
    let mut value = settings::initialize(&app)?;
    value.path_mappings.retain(|mapping| {
        mapping.game_id != game_id || !mapping.source.eq_ignore_ascii_case(source.trim())
    });
    settings::save(&app, &value)?;
    Ok(value)
}

#[tauri::command]
pub async fn game_saves_scan(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    mode: Option<ScanMode>,
    on_event: Channel<GameSavesEvent>,
) -> Result<GameSavesScan, String> {
    let root = settings::config_root(&app)?;
    let operation = state.begin(&root, OperationKind::Scan)?;
    send_stage(&on_event, OperationStage::Preparing);
    let mut value = settings::initialize(&app)?;
    // Launchers gain library folders over time (a new drive for Steam). The
    // automatic roots were only detected once, so games installed there were
    // never scanned; they are re-detected here, manual roots are kept.
    let known_roots = value.roots.clone();
    settings::merge_detected_roots(&mut value);
    // A backup folder whose drive is not there (Google Drive not running) is
    // not a reason to show nothing: a scan only reads, so the saves on this
    // PC are listed, and the page says the backups are out of reach.
    let unreachable = value
        .backup_folder
        .clone()
        .filter(|folder| !settings::drive_connected(Path::new(folder)));
    if unreachable.is_none() {
        validate_current_backup(&app, &value)?;
    }
    if value.roots != known_roots {
        edit_settings(&app, |fresh| {
            settings::merge_detected_roots(fresh);
            Ok(())
        })?;
    }
    let engine = Engine::for_app(&app)?;
    engine.prepare(&value)?;

    // A quick refresh needs a list of games to re-check; without a saved
    // scan there is none, and the full scan is the only way to find them.
    let cached = scan_cache::load(&root);
    let titles = match (mode.unwrap_or_default(), &cached) {
        (ScanMode::Quick, Some(cached)) => {
            Some(scan_cache::known_titles(&cached.scan, &value)).filter(|titles| !titles.is_empty())
        }
        _ => None,
    };
    send_stage(&on_event, OperationStage::Scanning);
    let (mut scan, metadata) =
        scan_with_engine(&engine, &value, &operation, titles.as_deref()).await?;
    ensure_not_cancelled(&operation)?;
    if let Some(folder) = &unreachable {
        scan.without_backups(folder);
    }
    if metadata.supported_games > 0 && value.database_games != metadata.supported_games {
        value.database_games = metadata.supported_games;
        edit_settings(&app, |fresh| {
            fresh.database_games = metadata.supported_games;
            Ok(())
        })?;
    }

    // A quick refresh keeps the full scan's fingerprint and time: it did not
    // look for new games, so whether that is due has not changed.
    let now = parser::now();
    let (fingerprint, full_scan_at) = match (&titles, &cached) {
        (Some(_), Some(cached)) => (cached.fingerprint.clone(), cached.scan.full_scan_at),
        _ => (scan_cache::fingerprint(&value), now),
    };
    scan.quick = titles.is_some();
    scan.full_scan_at = full_scan_at;
    let cached = CachedScan {
        fingerprint,
        scan: scan.clone(),
    };
    scan_cache::save(&root, &cached);
    scan.discovery_due = scan.quick && scan_cache::discovery_due(&cached, &value, now);

    send_stage(&on_event, OperationStage::Finishing);
    state.cache_scan(scan.clone());
    Ok(scan)
}

#[tauri::command]
pub async fn game_saves_update_database(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    on_event: Channel<GameSavesEvent>,
) -> Result<DatabaseUpdate, String> {
    let root = settings::config_root(&app)?;
    let operation = state.begin(&root, OperationKind::UpdateDatabase)?;
    send_stage(&on_event, OperationStage::UpdatingDatabase);
    let value = settings::initialize(&app)?;
    let engine = Engine::for_app(&app)?;
    engine.prepare(&value)?;
    let manifest_path = engine.manifest_path();
    let previous_manifest = tokio::fs::read(&manifest_path)
        .await
        .map_err(|error| format!("The current game database could not be preserved: {error}"))?;
    if let Err(error) = run_engine(&engine, &Engine::manifest_update_args(), None, &operation).await
    {
        let _ = atomic::write(&manifest_path, &previous_manifest);
        return Err(error);
    }
    if let Err(error) = ensure_not_cancelled(&operation) {
        let _ = atomic::write(&manifest_path, &previous_manifest);
        return Err(error);
    }
    let metadata = match load_manifest_metadata(&engine).await {
        Ok(metadata) if metadata.supported_games > 0 => metadata,
        Ok(_) => {
            atomic::write(&manifest_path, &previous_manifest).map_err(|restore| {
                format!("The invalid database could not be rolled back: {restore}")
            })?;
            return Err("The updated game database contains no backup definitions; the previous database was restored.".into());
        }
        Err(error) => {
            atomic::write(&manifest_path, &previous_manifest).map_err(|restore| {
                format!("{error} The previous database could not be restored: {restore}")
            })?;
            return Err(format!("{error} The previous database was restored."));
        }
    };
    let updated_at = parser::now();
    let saved = edit_settings(&app, |fresh| {
        fresh.database_games = metadata.supported_games;
        fresh.database_updated_at = Some(updated_at);
        Ok(())
    });
    if let Err(error) = saved {
        let _ = atomic::write(&manifest_path, &previous_manifest);
        return Err(format!(
            "The updated database could not be committed: {error}"
        ));
    }
    send_stage(&on_event, OperationStage::Finishing);
    Ok(DatabaseUpdate {
        games: metadata.supported_games,
        updated_at,
    })
}

#[tauri::command]
pub async fn game_saves_backup(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    game_ids: Vec<String>,
    on_event: Channel<GameSavesEvent>,
) -> Result<GameSavesOperationResult, String> {
    let games = selected_games(&state, &game_ids, true)?;
    let root = settings::config_root(&app)?;
    let operation = state.begin(&root, OperationKind::Backup)?;
    let value = settings::initialize(&app)?;
    let backup = backup_folder(&value)?;
    settings::validate_backup_folder(&app, &backup, &value)?;
    fs::create_dir_all(&backup).map_err(|error| error.to_string())?;
    let engine = Engine::for_app(&app)?;
    engine.prepare(&value)?;
    send_stage(&on_event, OperationStage::BackingUp);
    let titles: Vec<String> = games.iter().map(|game| game.title.clone()).collect();
    let result = backup_titles(
        &engine,
        &backup.to_string_lossy(),
        &titles,
        &operation,
        &|stage| send_stage(&on_event, stage),
    )
    .await?;
    send_stage(&on_event, OperationStage::Finishing);
    Ok(result)
}

/// How long a backup waits for OneDrive to start and bring online-only saves
/// down to this PC.
const ONEDRIVE_WAIT: Duration = Duration::from_secs(90);

/// Backs up `titles`. Games that failed only because their saves are
/// online-only in OneDrive and OneDrive was not running get a second pass,
/// once OneDrive has been started and their files can be read.
pub(crate) async fn backup_titles(
    engine: &Engine,
    backup: &str,
    titles: &[String],
    operation: &OperationHandle,
    on_stage: &(dyn Fn(OperationStage) + Sync),
) -> Result<GameSavesOperationResult, String> {
    let api = backup_pass(engine, backup, titles, operation).await?;
    let first = operation_result(OperationKind::Backup, titles, &api, None);
    let (retry, files) = cloud_offline(&first.failed_games, &api);
    if retry.is_empty() {
        return Ok(first);
    }
    on_stage(OperationStage::WaitingForOneDrive);
    if !wait_for_onedrive(files, operation).await? {
        on_stage(OperationStage::BackingUp);
        return Ok(first);
    }
    on_stage(OperationStage::BackingUp);
    let api = backup_pass(engine, backup, &retry, operation).await?;
    let second = operation_result(OperationKind::Backup, &retry, &api, None);

    let still_failed = |title: &String| !retry.contains(title);
    let mut failed_games: Vec<String> =
        first.failed_games.iter().filter(|t| still_failed(t)).cloned().collect();
    failed_games.extend(second.failed_games);
    let mut failures: Vec<GameFailure> = first
        .failures
        .into_iter()
        .filter(|failure| still_failed(&failure.game))
        .collect();
    failures.extend(second.failures);
    Ok(GameSavesOperationResult {
        kind: OperationKind::Backup,
        processed_games: first.processed_games + second.processed_games,
        processed_bytes: first.processed_bytes + second.processed_bytes,
        failed_games,
        failures,
        safety_backup_path: None,
    })
}

async fn backup_pass(
    engine: &Engine,
    backup: &str,
    titles: &[String],
    operation: &OperationHandle,
) -> Result<ApiOutput, String> {
    let output = run_engine(
        engine,
        &Engine::backup_args(backup),
        Some(&stdin_titles(titles)),
        operation,
    )
    .await?;
    let api = parser::parse_api(&output.stdout)?;
    ensure_not_cancelled(operation)?;
    Ok(api)
}

/// The failed games whose saves could not be read because the cloud app
/// that keeps them online-only was not running, and those files.
fn cloud_offline(failed: &[String], api: &ApiOutput) -> (Vec<String>, Vec<PathBuf>) {
    let mut games = Vec::new();
    let mut files = Vec::new();
    for title in failed {
        let Some((_, game)) = api
            .games
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(title))
        else {
            continue;
        };
        let offline: Vec<PathBuf> = game
            .files
            .iter()
            .filter(|(_, file)| {
                file.error.as_ref().is_some_and(|error| {
                    os_error_code(&error.message) == Some(cloud::PROVIDER_NOT_RUNNING)
                })
            })
            .map(|(path, _)| PathBuf::from(path))
            .collect();
        if !offline.is_empty() {
            games.push(title.clone());
            files.extend(offline);
        }
    }
    (games, files)
}

/// Starts OneDrive for `files` and waits until they can be read. False when
/// they are not OneDrive's, OneDrive is not installed, or it took too long.
async fn wait_for_onedrive(
    files: Vec<PathBuf>,
    operation: &OperationHandle,
) -> Result<bool, String> {
    let started = tauri::async_runtime::spawn_blocking(move || cloud::start_onedrive(&files))
        .await
        .map_err(|error| error.to_string())?;
    let Some(probe) = started else {
        return Ok(false);
    };
    let since = Instant::now();
    while since.elapsed() < ONEDRIVE_WAIT {
        ensure_not_cancelled(operation)?;
        let file = probe.clone();
        if tauri::async_runtime::spawn_blocking(move || cloud::readable(&file))
            .await
            .unwrap_or(false)
        {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(false)
}

#[tauri::command]
pub async fn game_saves_restore(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    selections: Vec<RestoreSelection>,
    on_event: Channel<GameSavesEvent>,
) -> Result<GameSavesOperationResult, String> {
    let value = settings::initialize(&app)?;
    let games = selected_restore_games(&state, &selections, &value)?;
    let root = settings::config_root(&app)?;
    let operation = state.begin(&root, OperationKind::Restore)?;
    let backup = backup_folder(&value)?;
    settings::validate_backup_folder(&app, &backup, &value)?;
    let engine = Engine::for_app(&app)?;
    engine.prepare(&value)?;

    // Re-preview every selected restore point immediately before touching any
    // local files. This catches a deleted/corrupt/stale snapshot while it is
    // still safe to stop, and applies persisted path redirects in the preview.
    send_stage(&on_event, OperationStage::Preparing);
    let total = games.len();
    send_progress(&on_event, 0, total, None);
    for (index, (game, snapshot)) in games.iter().enumerate() {
        send_progress(&on_event, index, total, Some(game.title.clone()));
        let input = stdin_titles(std::slice::from_ref(&game.title));
        let output = run_engine(
            &engine,
            &Engine::restore_preview_args(&backup.to_string_lossy(), Some(snapshot)),
            Some(&input),
            &operation,
        )
        .await
        .map_err(|error| format!("Could not preview {}: {error}", game.title))?;
        let api = parser::parse_api(&output.stdout)?;
        if !failed_games(std::slice::from_ref(&game.title), &api).is_empty() {
            return Err(format!(
                "The selected snapshot for {} could not be verified. Scan again and choose another restore point.",
                game.title
            ));
        }
        send_progress(&on_event, index + 1, total, Some(game.title.clone()));
    }

    send_stage(&on_event, OperationStage::CreatingSafetyBackup);
    let safety_root = settings::safety_root(&app)?;
    let local_ids: HashSet<_> = state
        .cached_scan()
        .into_iter()
        .flat_map(|scan| scan.on_this_pc)
        .filter(|game| game.has_local_data)
        .map(|game| game.id)
        .collect();
    let safety_titles: Vec<String> = games
        .iter()
        .filter(|(game, _)| local_ids.contains(&game.id))
        .map(|(game, _)| game.title.clone())
        .collect();
    let mut safety_path = None;
    let mut safety_slot = None;
    if !safety_titles.is_empty() {
        let (undo_record, slot) = undo::create_slot(&safety_root)?;
        let safety_input = stdin_titles(&safety_titles);
        let safety_output = run_engine(
            &engine,
            &Engine::backup_args(&slot.to_string_lossy()),
            Some(&safety_input),
            &operation,
        )
        .await;
        let safety_output = match safety_output {
            Ok(output) => output,
            Err(error) => {
                undo::discard_slot(&safety_root, &undo_record.id);
                return Err(format!(
                    "The restore was not started because its safety copy failed: {error}"
                ));
            }
        };
        let safety_api = match parser::parse_api(&safety_output.stdout) {
            Ok(api) => api,
            Err(error) => {
                undo::discard_slot(&safety_root, &undo_record.id);
                return Err(format!(
                    "The restore was not started because the safety-copy result was invalid: {error}"
                ));
            }
        };
        let failures = failed_games(&safety_titles, &safety_api);
        if !failures.is_empty() {
            undo::discard_slot(&safety_root, &undo_record.id);
            return Err(format!(
                "The restore was not started because a safety copy failed for: {}",
                failures.join(", ")
            ));
        }
        undo::commit(&safety_root, undo_record.clone(), safety_titles)?;
        safety_path = Some(slot.to_string_lossy().into_owned());
        safety_slot = Some(undo_record.id);
    }

    send_stage(&on_event, OperationStage::Restoring);
    let mut processed_games = 0;
    let mut processed_bytes = 0;
    let mut failures = Vec::new();
    let mut reasons: Vec<GameFailure> = Vec::new();
    send_progress(&on_event, 0, total, None);
    for (index, (game, snapshot)) in games.iter().enumerate() {
        if operation.is_cancelled() {
            return Err("The restore was cancelled.".into());
        }
        send_progress(&on_event, index, total, Some(game.title.clone()));
        let input = stdin_titles(std::slice::from_ref(&game.title));
        match run_engine(
            &engine,
            &Engine::restore_args(&backup.to_string_lossy(), Some(snapshot)),
            Some(&input),
            &operation,
        )
        .await
        {
            Ok(output) => match parser::parse_api(&output.stdout) {
                Ok(api) => {
                    let result = operation_result(
                        OperationKind::Restore,
                        std::slice::from_ref(&game.title),
                        &api,
                        None,
                    );
                    processed_games += result.processed_games;
                    processed_bytes += result.processed_bytes;
                    failures.extend(result.failed_games);
                    reasons.extend(result.failures);
                }
                Err(error) => {
                    failures.push(game.title.clone());
                    reasons.push(GameFailure {
                        game: game.title.clone(),
                        reason: format!("The engine returned an invalid result: {error}"),
                        file: None,
                    });
                }
            },
            Err(error) => {
                if operation.is_cancelled() {
                    return Err("The restore was cancelled.".into());
                }
                failures.push(game.title.clone());
                reasons.push(GameFailure {
                    game: game.title.clone(),
                    reason: error.clone(),
                    file: None,
                });
                let _ = on_event.send(GameSavesEvent::Message { text: error });
            }
        }
        send_progress(&on_event, index + 1, total, Some(game.title.clone()));
    }
    send_progress(&on_event, total, total, None);
    send_stage(&on_event, OperationStage::Finishing);
    failures.sort();
    failures.dedup();
    // Keep the committed safety copy even if one restore failed: any games
    // that were already modified can still be put back.
    let _ = safety_slot;
    Ok(GameSavesOperationResult {
        kind: OperationKind::Restore,
        processed_games,
        processed_bytes,
        failed_games: failures,
        failures: reasons,
        safety_backup_path: safety_path,
    })
}

#[tauri::command]
pub async fn game_saves_undo_last_restore(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    on_event: Channel<GameSavesEvent>,
) -> Result<GameSavesOperationResult, String> {
    let safety_root = settings::safety_root(&app)?;
    let undo_record =
        undo::load_valid(&safety_root).ok_or("The restore safety copy is no longer available.")?;
    let slot = undo::slot_path(&safety_root, &undo_record.id)?;
    let root = settings::config_root(&app)?;
    let operation = state.begin(&root, OperationKind::Restore)?;
    let value = settings::initialize(&app)?;
    let engine = Engine::for_app(&app)?;
    engine.prepare(&value)?;
    send_stage(&on_event, OperationStage::Restoring);
    let input = stdin_titles(&undo_record.games);
    let output = run_engine(
        &engine,
        &Engine::restore_args(&slot.to_string_lossy(), None),
        Some(&input),
        &operation,
    )
    .await?;
    let api = parser::parse_api(&output.stdout)?;
    ensure_not_cancelled(&operation)?;
    let result = operation_result(OperationKind::Restore, &undo_record.games, &api, None);
    if result.failed_games.is_empty() {
        undo::consume(&safety_root, &undo_record.id)?;
    }
    send_stage(&on_event, OperationStage::Finishing);
    Ok(result)
}

/// What of the Game Saves setup the account syncs.
#[tauri::command(async)]
pub fn game_saves_sync_export(app: AppHandle) -> Result<SyncedGameSaves, String> {
    let value = settings::initialize(&app)?;
    Ok(SyncedGameSaves {
        schedule: value.schedule,
        schedule_time: value.schedule_time,
        schedule_weekday: value.schedule_weekday,
        auto_backup_excluded_game_ids: value.auto_backup_excluded_game_ids,
        custom_games: value.custom_games,
    })
}

/// Applies settings synced from another PC. Custom games keep only the save
/// folders that exist here (profile names and drives differ between PCs);
/// a game with none left is skipped. Local folders are never touched.
#[tauri::command]
pub async fn game_saves_sync_import(
    app: AppHandle,
    state: State<'_, GameSavesState>,
    synced: SyncedGameSaves,
) -> Result<GameSavesSettings, String> {
    state.ensure_idle()?;
    let current = settings::initialize(&app)?;
    let schedule_changed = (current.schedule != synced.schedule
        || current.schedule_time != synced.schedule_time
        || current.schedule_weekday != synced.schedule_weekday)
        && settings::valid_schedule_time(&synced.schedule_time);
    if schedule_changed {
        configure_scheduled_task(
            synced.schedule,
            &synced.schedule_time,
            synced.schedule_weekday,
        )
        .await?;
    }
    edit_settings(&app, |value| {
        value.auto_backup_excluded_game_ids = synced.auto_backup_excluded_game_ids;
        merge_synced_custom_games(&mut value.custom_games, synced.custom_games);
        if let Some(backup) = value.backup_folder.as_deref() {
            value.custom_games.retain(|game| {
                !game
                    .paths
                    .iter()
                    .any(|path| settings::overlaps(Path::new(path), Path::new(backup)))
            });
        }
        if schedule_changed {
            value.schedule = synced.schedule;
            value.schedule_time = synced.schedule_time;
            value.schedule_weekday = synced.schedule_weekday;
        }
        Ok(())
    })?;
    settings::initialize(&app)
}

fn merge_synced_custom_games(local: &mut Vec<CustomGame>, synced: Vec<CustomGame>) {
    for mut game in synced {
        game.paths.retain(|path| Path::new(path).is_dir());
        if game
            .install_path
            .as_deref()
            .is_some_and(|path| !Path::new(path).is_dir())
        {
            game.install_path = None;
        }
        if game.paths.is_empty() || game.name.trim().is_empty() {
            continue;
        }
        // The same game (by id, or by name from an older copy) is replaced.
        local.retain(|known| known.id != game.id && !same_title(&known.name, &game.name));
        local.push(game);
    }
}

#[tauri::command(async)]
pub fn game_saves_cancel(state: State<'_, GameSavesState>) {
    state.cancel();
}

/// Runs the three engine passes side by side: what is on this PC (all games,
/// or only `titles`), what a restore would change, and the backup inventory.
pub(crate) async fn scan_with_engine(
    engine: &Engine,
    value: &GameSavesSettings,
    operation: &OperationHandle,
    titles: Option<&[String]>,
) -> Result<(GameSavesScan, ManifestMetadata), String> {
    let backup = backup_folder(value)?;
    let backup_text = backup.to_string_lossy().into_owned();
    let local = async {
        match titles {
            Some(titles) => quick_local_preview(engine, &backup_text, titles, operation).await,
            None => {
                let output = run_engine(
                    engine,
                    &Engine::backup_preview_args(&backup_text),
                    None,
                    operation,
                )
                .await?;
                parser::parse_api(&output.stdout)
            }
        }
    };
    let history = async {
        if !backup.is_dir() {
            return Ok((ApiOutput::default(), ApiOutput::default()));
        }
        let restore_args = Engine::restore_preview_args(&backup_text, None);
        let inventory_args = Engine::backups_args(&backup_text);
        let (restore, inventory) = tokio::join!(
            run_engine(engine, &restore_args, None, operation),
            run_engine(engine, &inventory_args, None, operation),
        );
        Ok::<_, String>((
            parser::parse_api(&restore?.stdout)?,
            parser::parse_api(&inventory?.stdout)?,
        ))
    };
    let (local, history) = tokio::join!(local, history);
    ensure_not_cancelled(operation)?;
    let local = local?;
    let (restore, inventory) = history?;
    let metadata = load_manifest_metadata(engine).await?;
    Ok((
        parser::build_scan(local, restore, inventory, &metadata, value),
        metadata,
    ))
}

/// The "on this PC" pass limited to known games. A name the database no
/// longer has (a removed custom game, a renamed entry) makes the engine
/// process nothing at all, so those are dropped and the pass runs once more.
async fn quick_local_preview(
    engine: &Engine,
    backup: &str,
    titles: &[String],
    operation: &OperationHandle,
) -> Result<ApiOutput, String> {
    let mut titles = titles.to_vec();
    for _ in 0..2 {
        if titles.is_empty() {
            return Ok(ApiOutput::default());
        }
        let mut track = |pid, running| operation.track_pid(pid, running);
        let output = engine
            .run_raw(
                &Engine::backup_preview_args(backup),
                Some(&stdin_titles(&titles)),
                &mut track,
            )
            .await?;
        ensure_not_cancelled(operation)?;
        let api = parser::parse_api(&output.stdout);
        if output.success {
            return api;
        }
        let unknown = api
            .ok()
            .and_then(|api| api.errors)
            .map(|errors| errors.unknown_games)
            .unwrap_or_default();
        if unknown.is_empty() {
            return Err(failure_detail(&output.stderr, &output.stdout).to_string());
        }
        titles.retain(|title| !unknown.contains(title));
    }
    Err("The quick scan could not complete; run a full scan.".into())
}

pub(crate) async fn run_engine(
    engine: &Engine,
    args: &[String],
    stdin: Option<&str>,
    operation: &OperationHandle,
) -> Result<EngineOutput, String> {
    let mut track = |pid, running| operation.track_pid(pid, running);
    let result = engine.run(args, stdin, &mut track).await;
    if operation.is_cancelled() {
        Err("The Game Saves operation was cancelled.".into())
    } else {
        result
    }
}

pub(crate) fn operation_result(
    kind: OperationKind,
    requested: &[String],
    api: &ApiOutput,
    safety_backup_path: Option<String>,
) -> GameSavesOperationResult {
    let failed_games = failed_games(requested, api);
    // Never trust an aggregate count without matching per-game output. A
    // truncated but syntactically valid JSON object must not become success.
    let processed_games = requested.len().saturating_sub(failed_games.len()) as u64;
    let processed_bytes = api
        .overall
        .as_ref()
        .map(|overall| overall.processed_bytes)
        .unwrap_or_else(|| {
            api.games
                .values()
                .flat_map(|game| game.files.values())
                .filter(|file| !file.failed && !file.ignored)
                .map(|file| file.bytes)
                .sum()
        });
    let failures = failure_reasons(&failed_games, api);
    GameSavesOperationResult {
        kind,
        processed_games,
        processed_bytes,
        failed_games,
        failures,
        safety_backup_path,
    }
}

/// The first error the engine reported for each failed game, in words a
/// player can act on, with the file it happened to.
pub(crate) fn failure_reasons(failed: &[String], api: &ApiOutput) -> Vec<GameFailure> {
    failed
        .iter()
        .map(|title| {
            let game = api
                .games
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(title))
                .map(|(_, game)| game);
            let first = game.and_then(|game| {
                game.files
                    .iter()
                    .filter_map(|(path, file)| {
                        file.error.as_ref().map(|e| (path.as_str(), e.message.as_str()))
                    })
                    .chain(game.registry.iter().filter_map(|(path, entry)| {
                        entry.error.as_ref().map(|e| (path.as_str(), e.message.as_str()))
                    }))
                    .next()
            });
            let (reason, file) = match first {
                Some((path, message)) => (explain_error(message), Some(short_path(path))),
                None if game.is_none() => ("The engine did not report this game.".into(), None),
                None => ("The engine did not finish this game.".into(), None),
            };
            GameFailure {
                game: title.clone(),
                reason,
                file,
            }
        })
        .collect()
}

/// Ludusavi passes on the operating system's message; the common ones get a
/// hint about what to do.
fn explain_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let hint = match os_error_code(message) {
        // The backup already tried to start OneDrive; this is left when it
        // is not installed or signed in here, or another cloud app keeps it.
        Some(cloud::PROVIDER_NOT_RUNNING) => Some(
            "the file is online-only in OneDrive or another cloud folder, and that app is not running; start it and let the file download, or mark the folder \"Always keep on this device\"",
        ),
        // Other ERROR_CLOUD_FILE_* codes: an online-only file that could not come down.
        Some(389..=398 | 404) => Some(
            "the file is online-only in OneDrive or another cloud folder; open that app and let it download, or mark the folder \"Always keep on this device\"",
        ),
        Some(32 | 33) => Some("the file is in use; close the game and try again"),
        Some(5) => Some("access was denied; the game or an antivirus may be holding the file"),
        Some(206) => Some("the path is too long; choose a shorter backup folder"),
        Some(112) => Some("the disk is full"),
        _ if lower.contains("cloud") => {
            Some("the file is online-only in a cloud folder; let it download first")
        }
        _ => None,
    };
    match hint {
        Some(hint) => format!("{}: {hint}", message.trim().trim_end_matches('.')),
        None => message.trim().to_string(),
    }
}

/// The Windows error code in a message such as "… (os error 362)".
fn os_error_code(message: &str) -> Option<u32> {
    message
        .to_ascii_lowercase()
        .rsplit_once("os error ")
        .and_then(|(_, rest)| rest.trim_end_matches(')').trim().parse().ok())
}

/// The end of a long save path, enough to recognise it.
fn short_path(path: &str) -> String {
    const KEEP: usize = 60;
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= KEEP {
        path.to_string()
    } else {
        format!("…{}", chars[chars.len() - KEEP..].iter().collect::<String>())
    }
}

pub(crate) fn failed_games(requested: &[String], api: &ApiOutput) -> Vec<String> {
    let mut failed = Vec::new();
    for requested_title in requested {
        let game = api
            .games
            .iter()
            .find(|(title, _)| title.eq_ignore_ascii_case(requested_title));
        if game.is_none_or(|(_, game)| game_failed(game)) {
            failed.push(requested_title.clone());
        }
    }
    if api
        .errors
        .as_ref()
        .and_then(|errors| errors.some_games_failed)
        .unwrap_or(false)
        && failed.is_empty()
    {
        failed.extend(requested.iter().cloned());
    }
    failed.sort();
    failed.dedup();
    failed
}

fn game_failed(game: &ApiGame) -> bool {
    game.files
        .values()
        .any(|file| file.failed || file.error.is_some())
        || game
            .registry
            .values()
            .any(|entry| entry.failed || entry.error.is_some())
        // For backup/restore output, only an explicit `Processed` decision is
        // success. `Ignored`, `Cancelled`, or a truncated object with no
        // decision must never be presented as a completed backup.
        || !game
            .decision
            .as_deref()
            .is_some_and(|decision| decision.eq_ignore_ascii_case("processed"))
}

async fn load_manifest_metadata(engine: &Engine) -> Result<ManifestMetadata, String> {
    let path = engine.manifest_path();
    let stamp = fs::metadata(&path)
        .ok()
        .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
    if let Some((len, modified)) = stamp
        && let Some((cached_len, cached_modified, metadata)) =
            MANIFEST.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
        && *cached_len == len
        && *cached_modified == modified
    {
        return Ok(metadata.clone());
    }
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| format!("The Game Saves database could not be read: {error}"))?;
    let metadata =
        tauri::async_runtime::spawn_blocking(move || parser::parse_manifest_metadata(&text))
            .await
            .map_err(|error| error.to_string())?;
    if let Some((len, modified)) = stamp {
        *MANIFEST.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((len, modified, metadata.clone()));
    }
    Ok(metadata)
}

fn selected_games(
    state: &GameSavesState,
    ids: &[String],
    local: bool,
) -> Result<Vec<GameSaveEntry>, String> {
    if ids.is_empty() || ids.len() > 1000 {
        return Err("Select between 1 and 1,000 games.".into());
    }
    let scan = state
        .cached_scan()
        .ok_or("Scan again before starting this operation.")?;
    let source = if local {
        scan.on_this_pc
    } else {
        scan.in_backup
    };
    let by_id: HashMap<_, _> = source
        .into_iter()
        .map(|game| (game.id.clone(), game))
        .collect();
    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        games.push(
            by_id
                .get(id)
                .cloned()
                .ok_or("One of the selected games is stale. Scan again.")?,
        );
    }
    Ok(games)
}

fn selected_restore_games(
    state: &GameSavesState,
    selections: &[RestoreSelection],
    settings: &GameSavesSettings,
) -> Result<Vec<(GameSaveEntry, String)>, String> {
    let ids: Vec<_> = selections
        .iter()
        .map(|selection| selection.game_id.clone())
        .collect();
    let games = selected_games(state, &ids, false)?;
    let by_id: HashMap<_, _> = games
        .into_iter()
        .map(|game| (game.id.clone(), game))
        .collect();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for selection in selections {
        if !seen.insert(&selection.game_id) {
            return Err("Each game can only be restored once per operation.".into());
        }
        let game = by_id
            .get(&selection.game_id)
            .cloned()
            .ok_or("One of the selected games is stale. Scan again.")?;
        // The status comes from the last scan; a location chosen since then is
        // only in the settings, and must not be refused as missing.
        if game.status == GameSaveStatus::NeedsLocation
            && !settings
                .path_mappings
                .iter()
                .any(|mapping| mapping.game_id == game.id)
        {
            return Err(format!(
                "Choose a restore location for {} before restoring it.",
                game.title
            ));
        }
        if !game
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == selection.snapshot_id)
        {
            return Err(format!(
                "The selected snapshot for {} is no longer available. Scan again.",
                game.title
            ));
        }
        result.push((game, selection.snapshot_id.clone()));
    }
    Ok(result)
}

fn set_backup_folder(app: &AppHandle, path: &Path) -> Result<GameSavesSettings, String> {
    if !path.is_dir() {
        return Err("Choose an existing backup folder.".into());
    }
    edit_settings(app, |value| {
        settings::validate_backup_folder(app, path, value)?;
        value.backup_folder = Some(path.to_string_lossy().into_owned());
        Ok(())
    })
}

fn validate_current_backup(app: &AppHandle, value: &GameSavesSettings) -> Result<(), String> {
    if let Some(backup) = value.backup_folder.as_deref() {
        settings::validate_backup_folder(app, Path::new(backup), value)?;
    }
    Ok(())
}

fn backup_folder(value: &GameSavesSettings) -> Result<PathBuf, String> {
    value
        .backup_folder
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Choose a backup folder first.".into())
}

async fn pick_folder(app: AppHandle, title: String) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
            .map(|path| path.into_path().map_err(|error| error.to_string()))
            .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn configure_scheduled_task(
    schedule: BackupSchedule,
    time: &str,
    weekday: ScheduleWeekday,
) -> Result<(), String> {
    if schedule == BackupSchedule::Off {
        let deletion = crate::apps::process::hidden("schtasks.exe")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !deletion.status.success() {
            let query = crate::apps::process::hidden("schtasks.exe")
                .args(["/Query", "/TN", TASK_NAME])
                .stdin(Stdio::null())
                .output()
                .await
                .map_err(|error| error.to_string())?;
            if query.status.success() {
                return Err("Windows could not disable the scheduled Game Saves backup.".into());
            }
        }
        return Ok(());
    }

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let script = scheduled_task_script(schedule, time, weekday, &executable);
    let output = crate::apps::process::hidden("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ])
        .arg(crate::apps::process::encode_command(&script))
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Windows rejected the scheduled backup task.")
            .trim()
            .to_string();
        return Err(detail);
    }
    Ok(())
}

fn scheduled_task_script(
    schedule: BackupSchedule,
    time: &str,
    weekday: ScheduleWeekday,
    executable: &Path,
) -> String {
    let trigger = match schedule {
        BackupSchedule::Daily => format!(
            "New-ScheduledTaskTrigger -Daily -At ([datetime]::Today.Add([timespan]::ParseExact({}, 'hh\\:mm', $null)))",
            ps_literal(time)
        ),
        BackupSchedule::Weekly => format!(
            "New-ScheduledTaskTrigger -Weekly -DaysOfWeek {} -At ([datetime]::Today.Add([timespan]::ParseExact({}, 'hh\\:mm', $null)))",
            weekday_for_scheduled_tasks(weekday),
            ps_literal(time)
        ),
        BackupSchedule::Off => unreachable!(),
    };
    let working_directory = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy();
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ \
         $action = New-ScheduledTaskAction -Execute {} -Argument '--game-saves-auto-backup' -WorkingDirectory {}; \
         $trigger = {}; \
         $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 12); \
         $settings.WakeToRun = $false; \
         $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name; \
         $principal = New-ScheduledTaskPrincipal -UserId $identity -LogonType Interactive -RunLevel Limited; \
         $task = New-ScheduledTask -Action $action -Trigger $trigger -Settings $settings -Principal $principal; \
         Register-ScheduledTask -TaskName {} -InputObject $task -Force | Out-Null; \
         exit 0 \
         }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}",
        ps_literal(&executable.to_string_lossy()),
        ps_literal(&working_directory),
        trigger,
        ps_literal(TASK_NAME),
    )
}

fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn weekday_for_scheduled_tasks(day: ScheduleWeekday) -> &'static str {
    match day {
        ScheduleWeekday::Sunday => "Sunday",
        ScheduleWeekday::Monday => "Monday",
        ScheduleWeekday::Tuesday => "Tuesday",
        ScheduleWeekday::Wednesday => "Wednesday",
        ScheduleWeekday::Thursday => "Thursday",
        ScheduleWeekday::Friday => "Friday",
        ScheduleWeekday::Saturday => "Saturday",
    }
}

fn stdin_titles(titles: &[String]) -> String {
    let mut input = titles.join("\n");
    input.push('\n');
    input
}

fn send_stage(channel: &Channel<GameSavesEvent>, stage: OperationStage) {
    let _ = channel.send(GameSavesEvent::Stage { stage });
}

fn send_progress(
    channel: &Channel<GameSavesEvent>,
    done: usize,
    total: usize,
    current: Option<String>,
) {
    let _ = channel.send(GameSavesEvent::Progress {
        done,
        total,
        current,
    });
}

fn ensure_not_cancelled(operation: &OperationHandle) -> Result<(), String> {
    if operation.is_cancelled() {
        Err("The Game Saves operation was cancelled.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_api_failures_are_exposed_to_the_ui() {
        let api = parser::parse_api(
            r#"{"games":{"Good":{"decision":"Processed"},"Bad":{"decision":"Processed","files":{"x":{"failed":true,"error":{"message":"locked"}}}}},"overall":{"processedGames":1,"processedBytes":50},"errors":{"someGamesFailed":true}}"#,
        )
        .unwrap();
        let result = operation_result(
            OperationKind::Backup,
            &["Good".into(), "Bad".into()],
            &api,
            None,
        );
        assert_eq!(result.processed_games, 1);
        assert_eq!(result.failed_games, vec!["Bad"]);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].game, "Bad");
        assert_eq!(result.failures[0].reason, "locked");
        assert_eq!(result.failures[0].file.as_deref(), Some("x"));
    }

    #[test]
    fn common_os_errors_come_with_a_hint() {
        let cloud = explain_error("The cloud file provider is not running. (os error 362)");
        assert!(cloud.contains("online-only"), "{cloud}");
        let busy = explain_error(
            "The process cannot access the file because it is being used by another process. (os error 32)",
        );
        assert!(busy.contains("close the game"), "{busy}");
        assert_eq!(explain_error("something else"), "something else");
        assert!(short_path(&"a/".repeat(80)).starts_with('…'));
    }

    #[test]
    fn only_saves_waiting_for_a_cloud_app_get_a_second_pass() {
        let api = parser::parse_api(
            r#"{"games":{
                "Cloud":{"decision":"Processed","files":{
                    "C:/Users/Panos/OneDrive/Documents/My Games/a.sav":{"failed":true,"error":{"message":"The cloud file provider is not running. (os error 362)"}},
                    "C:/Users/Panos/OneDrive/Documents/My Games/b.sav":{"bytes":3}}},
                "Busy":{"decision":"Processed","files":{
                    "C:/Games/busy.sav":{"failed":true,"error":{"message":"in use (os error 32)"}}}}}}"#,
        )
        .unwrap();
        let failed = failed_games(&["Cloud".into(), "Busy".into()], &api);
        let (games, files) = cloud_offline(&failed, &api);
        assert_eq!(games, vec!["Cloud".to_string()]);
        assert_eq!(
            files,
            vec![PathBuf::from("C:/Users/Panos/OneDrive/Documents/My Games/a.sav")]
        );
        assert_eq!(os_error_code("x (os error 362)"), Some(362));
        assert_eq!(os_error_code("no code"), None);
    }

    #[test]
    fn missing_per_game_output_can_never_be_reported_as_success() {
        let api =
            parser::parse_api(r#"{"games":{},"overall":{"processedGames":1,"processedBytes":50}}"#)
                .unwrap();
        let result = operation_result(OperationKind::Backup, &["Missing".into()], &api, None);
        assert_eq!(result.processed_games, 0);
        assert_eq!(result.failed_games, vec!["Missing"]);
    }

    #[test]
    fn ignored_or_truncated_game_output_is_not_success() {
        let api =
            parser::parse_api(r#"{"games":{"Ignored":{"decision":"Ignored"},"Truncated":{}}}"#)
                .unwrap();
        let result = operation_result(
            OperationKind::Backup,
            &["Ignored".into(), "Truncated".into()],
            &api,
            None,
        );
        assert_eq!(result.processed_games, 0);
        assert_eq!(result.failed_games, vec!["Ignored", "Truncated"]);
    }

    #[test]
    fn a_restore_location_chosen_after_the_scan_is_accepted() {
        use super::super::models::GameSaveSnapshot;
        let state = GameSavesState::default();
        state.cache_scan(GameSavesScan {
            in_backup: vec![GameSaveEntry {
                id: "game-1".into(),
                title: "Old Game".into(),
                status: GameSaveStatus::NeedsLocation,
                platform_badges: Vec::new(),
                file_count: 1,
                total_bytes: 1,
                last_save_at: None,
                last_backup_at: None,
                paths: vec![r"E:\Saves\Old Game".into()],
                auto_backup: true,
                has_local_data: false,
                has_backup: true,
                error: None,
                snapshots: vec![GameSaveSnapshot {
                    id: "backup-1".into(),
                    timestamp: "2026-01-01T00:00:00Z".into(),
                    bytes: 1,
                    label: None,
                    is_safety: false,
                }],
                steam_id: None,
            }],
            ..Default::default()
        });
        let selections = [RestoreSelection {
            game_id: "game-1".into(),
            snapshot_id: "backup-1".into(),
        }];
        let mut value = GameSavesSettings::default();
        assert!(selected_restore_games(&state, &selections, &value).is_err());
        value.path_mappings.push(RestorePathMapping {
            game_id: "game-1".into(),
            source: r"E:\Saves\Old Game".into(),
            target: r"C:\Saves\Old Game".into(),
        });
        assert!(selected_restore_games(&state, &selections, &value).is_ok());
    }

    #[test]
    fn synced_custom_games_keep_only_folders_that_exist_here() {
        let here = std::env::temp_dir();
        let game = |id: &str, name: &str, paths: Vec<String>| CustomGame {
            id: id.into(),
            name: name.into(),
            paths,
            install_path: Some(r"Q:\Nowhere\Game".into()),
            auto_backup: true,
        };
        let mut local = vec![
            game(
                "custom-a",
                "Alpha",
                vec![here.to_string_lossy().into_owned()],
            ),
            game(
                "custom-keep",
                "Kept",
                vec![here.to_string_lossy().into_owned()],
            ),
        ];
        merge_synced_custom_games(
            &mut local,
            vec![
                // Same game from the other PC: replaces the local copy.
                game(
                    "custom-a",
                    "Alpha",
                    vec![
                        here.to_string_lossy().into_owned(),
                        r"Q:\Users\Other\Saves".into(),
                    ],
                ),
                // Nothing of it exists on this PC: skipped.
                game("custom-b", "Beta", vec![r"Q:\Users\Other\Beta".into()]),
            ],
        );
        let names: Vec<_> = local.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["Kept", "Alpha"]);
        let alpha = local.iter().find(|g| g.name == "Alpha").unwrap();
        assert_eq!(alpha.paths.len(), 1);
        assert_eq!(alpha.install_path, None);
    }

    #[test]
    fn scheduled_weekdays_use_scheduled_tasks_enum_names() {
        assert_eq!(
            weekday_for_scheduled_tasks(ScheduleWeekday::Sunday),
            "Sunday"
        );
        assert_eq!(
            weekday_for_scheduled_tasks(ScheduleWeekday::Wednesday),
            "Wednesday"
        );
    }

    #[test]
    fn scheduled_task_is_limited_interactive_catch_up_and_never_wakes_the_pc() {
        let script = scheduled_task_script(
            BackupSchedule::Weekly,
            "03:00",
            ScheduleWeekday::Sunday,
            Path::new(r"C:\Program Files\MYLE\MakeYourLifeEasier.exe"),
        );
        assert!(script.contains("-StartWhenAvailable"));
        assert!(script.contains("WakeToRun = $false"));
        assert!(script.contains("-LogonType Interactive -RunLevel Limited"));
        assert!(script.contains("-Weekly -DaysOfWeek Sunday"));
        assert!(script.contains("--game-saves-auto-backup"));
    }
}
