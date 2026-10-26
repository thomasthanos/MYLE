pub(crate) mod atomic;
pub(crate) mod commands;
pub(crate) mod covers;
mod detection;
mod engine;
mod headless;
mod models;
pub(crate) mod notice;
mod parser;
mod safety;
mod scan_cache;
mod settings;
mod state;
mod undo;

pub use headless::run_headless_auto_backup;
pub(crate) use state::GameSavesReservation;
pub use state::GameSavesState;

/// The save folders the last scan found on this PC, with each game's title,
/// for the Disk Analyzer's warnings. Empty before the first scan.
pub(crate) fn known_save_folders() -> Vec<(std::path::PathBuf, String)> {
    // As `settings::config_root`, which needs no app handle either.
    let Ok(root) = crate::storage::roaming_dir().map(|path| path.join("game-saves")) else {
        return Vec::new();
    };
    let Some(cached) = scan_cache::load(&root) else {
        return Vec::new();
    };
    cached
        .scan
        .on_this_pc
        .into_iter()
        .flat_map(|game| {
            let title = game.title;
            game.paths.into_iter().map(move |path| (std::path::PathBuf::from(path), title.clone()))
        })
        .collect()
}

pub(crate) fn reserve_operations(
    app: &tauri::AppHandle,
    state: &GameSavesState,
) -> Result<GameSavesReservation, String> {
    state.reserve_idle(&settings::config_root(app)?)
}
