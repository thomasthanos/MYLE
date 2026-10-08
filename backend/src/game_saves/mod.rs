pub(crate) mod atomic;
pub(crate) mod commands;
pub(crate) mod covers;
mod detection;
mod engine;
mod headless;
mod models;
pub(crate) mod notice;
mod parser;
mod scan_cache;
mod settings;
mod state;
mod undo;

pub use headless::run_headless_auto_backup;
pub(crate) use state::GameSavesReservation;
pub use state::GameSavesState;

pub(crate) fn reserve_operations(
    app: &tauri::AppHandle,
    state: &GameSavesState,
) -> Result<GameSavesReservation, String> {
    state.reserve_idle(&settings::config_root(app)?)
}
