mod account;
mod apps;
pub mod cleaner;
mod cloud;
mod console;
mod debloat;
mod download;
mod elevated_pipe;
pub mod game_saves;
pub mod github_releases;
pub mod project_backups;
mod maintenance;
mod passwords;
mod spotify_hub;
mod startup;
mod storage;
mod tray;
mod updater;
mod webview_policy;
mod window_sizing;
mod windows_optimization;

/// Handles a browser starting this program as the password extension's
/// native messaging host. `None` means this is not such a start.
pub fn run_passwords_native_host() -> Option<i32> {
    passwords::browser::run_native_host()
}

/// Handles the privileged Auto-Logon helper before Tauri and the
/// single-instance plugin start. `None` means this is a normal app launch.
pub fn run_windows_auto_logon_helper() -> Option<i32> {
    windows_optimization::run_auto_logon_helper_from_args()
}

/// Handles the debloater's administrator helper before Tauri starts.
pub fn run_debloat_helper() -> Option<i32> {
    debloat::run_elevated_helper_from_args()
}

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

fn create_windows(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::window::Color;

    let data_dir = storage::webview_dir().map_err(std::io::Error::other)?;
    let splash = WebviewWindowBuilder::new(
        app,
        "splash",
        WebviewUrl::App("splash.html".into()),
    )
    .title("MYLE")
    .inner_size(300.0, 380.0)
    .center()
    .resizable(false)
    .maximizable(false)
    .decorations(false)
    .shadow(true)
    .visible(false)
    .devtools(cfg!(debug_assertions))
    .background_color(Color(10, 12, 18, 255))
    .data_directory(data_dir.clone())
    .build()?;
    webview_policy::harden(&splash)?;
    let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("MYLE")
        .inner_size(1280.0, 720.0)
        .min_inner_size(800.0, 500.0)
        .center()
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .visible(false)
        .devtools(cfg!(debug_assertions))
        .background_color(Color(0, 0, 0, 0))
        .data_directory(data_dir)
        .build()?;
    webview_policy::harden(&main)?;
    Ok(())
}

/// Called by the splash once the update check is done: sizes and shows the
/// main window (preloaded while hidden), then removes the splash.
#[tauri::command]
async fn finish_startup(app: AppHandle) -> Result<(), String> {
    show_main(&app).map_err(|e| e.to_string())
}

/// The page to open on start: the Password Manager when the browser
/// extension started the app (`--open-passwords`), Game Saves from a
/// scheduled backup's notice.
#[tauri::command]
fn start_page() -> Option<&'static str> {
    page_asked(&std::env::args().collect::<Vec<_>>())
}

fn page_asked(args: &[String]) -> Option<&'static str> {
    if args.iter().any(|a| a == "--open-passwords") {
        Some("password-manager")
    } else if args.iter().any(|a| a == game_saves::notice::OPEN_FLAG) {
        Some("game-saves")
    } else {
        None
    }
}

/// Refuses the owner-only pages' commands (GitHub Releases, Project
/// Backups) unless the signed-in account is the owner's, by the verdict the
/// backend got from Supabase (`account::owner`). One check for every command,
/// so a command added to those pages later is covered too.
fn owner_gate(
    handler: impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        if account::owner::is_owner_only_command(invoke.message.command()) {
            let app = invoke.message.webview().app_handle().clone();
            if !app.state::<account::AccountState>().is_owner(&app) {
                invoke
                    .resolver
                    .reject("This page is only for the app's owner. Sign in with the owner's Discord account.");
                return true;
            }
        }
        handler(invoke)
    }
}

/// Everything the app is built from, for its normal run and for a notice.
fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

/// What the notice page shows.
#[tauri::command]
fn notice_data(notice: tauri::State<'_, game_saves::notice::Notice>) -> game_saves::notice::Notice {
    notice.inner().clone()
}

/// The notice closes: by itself, by its ×, or clicked to open Game Saves.
#[tauri::command]
fn notice_done(app: AppHandle, open: bool) {
    if open {
        game_saves::notice::open_game_saves();
    }
    app.exit(0);
}

/// `Some(exit code)` when this run is a scheduled backup's notice: only its
/// window, which is gone after a few seconds.
pub fn run_game_saves_notice() -> Option<i32> {
    let notice = game_saves::notice::from_args()?;
    let built = tauri::Builder::default()
        .manage(notice)
        .setup(|app| {
            game_saves::notice::create_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![notice_data, notice_done])
        .build(context());
    Some(match built {
        Ok(app) => {
            app.run(|_, _| {});
            0
        }
        Err(_) => 1,
    })
}

fn show_main(app: &AppHandle) -> tauri::Result<()> {
    let Some(main) = app.get_webview_window("main") else {
        return Ok(());
    };
    if !main.is_visible()? {
        window_sizing::fit_to_screen(&main)?;
        // Launched by the Startup shortcut: stay out of the way, unless the
        // user chose to have it open at sign-in. With the tray icon, out of
        // the way means there; otherwise minimized in the taskbar.
        let quietly = startup::launched_at_sign_in() && startup::start_minimized();
        if quietly && tray::keep_running() {
            let _ = tray::ensure(app);
        } else {
            main.show()?;
            if quietly {
                main.minimize()?;
            } else {
                main.set_focus()?;
            }
        }
    }
    if let Some(splash) = app.get_webview_window("splash") {
        // destroy() skips CloseRequested, which would otherwise quit the app.
        splash.destroy()?;
    }
    Ok(())
}

pub fn run() {
    // Started at sign-in although starting with Windows is switched off.
    if startup::respect_choice() {
        return;
    }
    if let Err(error) = storage::prepare() {
        eprintln!("Application-data migration failed: {error}");
    }
    let account_state = account::AccountState::default();
    let cleanup = apps::Cleanup::default();
    let jobs = apps::Jobs::default();
    let running = maintenance::Running::default();
    let game_saves_state = game_saves::GameSavesState::default();
    let spotify_hub_state = spotify_hub::SpotifyHubState::default();
    let github_releases_state = github_releases::GithubReleasesState::default();
    let windows_optimization_state = windows_optimization::WindowsOptimizationState::default();
    let passwords_state = passwords::PasswordsState::default();
    let passwords_windows_state = passwords::WindowsFillState::default();
    let app = tauri::Builder::default()
        // Must be registered first. A second launch focuses the running app.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Still starting: the splash. Otherwise the main window, also
            // when it waits in the tray.
            let splash = app
                .get_webview_window("splash")
                .filter(|w| w.is_visible().unwrap_or(false));
            match splash {
                Some(splash) => {
                    let _ = splash.set_focus();
                }
                None => tray::show_window(app),
            }
            // A scheduled backup's notice was clicked: go to Game Saves.
            if args.iter().any(|a| a == game_saves::notice::OPEN_FLAG) {
                let _ = app.emit("myle-navigate", "game-saves");
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(account_state)
        .manage(jobs.clone())
        .manage(cleanup.clone())
        .manage(running.clone())
        .manage(game_saves_state.clone())
        .manage(project_backups::ProjectBackupsState::default())
        .manage(github_releases_state.clone())
        .manage(spotify_hub_state.clone())
        .manage(windows_optimization_state.clone())
        .manage(passwords_state.clone())
        .manage(passwords_windows_state.clone())
        .setup({
            let cleanup = cleanup.clone();
            move |app| {
                create_windows(app)?;
                passwords::watch(app.handle().clone(), passwords_state.clone());
                passwords::serve_browser(app.handle().clone(), passwords_state.clone());
                passwords::start_windows_fill(app.handle().clone(), passwords_windows_state.clone());
                tray::init(app.handle());
                tray::watch_quit(app.handle().clone());
                // Sweeps anything a previous run could not delete. Off the main
                // thread: a locked file costs a retry delay.
                if let Ok(dir) = storage::roaming_dir() {
                    std::thread::spawn(move || cleanup.init(dir.join("pending-cleanup.json")));
                }
                // The installer of the last update, if any. This runs before
                // the splash can start a new download into the same folder.
                // An installer still exiting after relaunching us stays
                // locked and goes on a later start.
                let _ = std::fs::remove_dir_all(updater::update_dir());
                updater::sweep_leftovers_later();
                // If the splash never gets to call finish_startup (a failed
                // update check, a broken page), show the window anyway.
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
                        // A slow update check (feed, then GitHub) or an update
                        // download legitimately keeps the splash up.
                        if updater::is_checking() || updater::is_updating() {
                            continue;
                        }
                        let hidden = handle
                            .get_webview_window("main")
                            .is_some_and(|w| !w.is_visible().unwrap_or(false));
                        if hidden {
                            let _ = show_main(&handle);
                        }
                        return;
                    }
                });
                Ok(())
            }
        })
        .on_window_event(|window, event| {
            let WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            match window.label() {
                // Closing the splash (Alt+F4) before the main window is shown
                // would leave a hidden, running app behind, so quit instead.
                "splash" => window.app_handle().exit(0),
                // "Keep running in the tray" on: the app stays next to the
                // clock, so Ctrl+Shift+L and browser filling keep working.
                "main" if tray::keep_running() => {
                    api.prevent_close();
                    let _ = tray::ensure(window.app_handle());
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .invoke_handler(owner_gate(tauri::generate_handler![
            finish_startup,
            start_page,
            passwords::passwords_browser_get,
            passwords::passwords_browser_set,
            passwords::passwords_open_extension_dir,
            passwords::windows_fill::passwords_windows_hotkey_status,
            passwords::windows_fill::passwords_windows_fill,
            passwords::windows_fill::passwords_windows_matches,
            passwords::windows_fill::passwords_windows_link,
            passwords::passwords_pick_program,
            passwords::passwords_status,
            passwords::passwords_create,
            passwords::passwords_unlock,
            passwords::passwords_hello_status,
            passwords::passwords_hello_enable,
            passwords::passwords_hello_disable,
            passwords::passwords_hello_unlock,
            passwords::passwords_recover,
            passwords::passwords_change_master,
            passwords::passwords_sync,
            passwords::passwords_use_account_vault,
            passwords::passwords_lock,
            passwords::passwords_set_auto_lock,
            passwords::passwords_set_website_icons,
            passwords::icons::passwords_icons,
            passwords::passwords_list,
            passwords::passwords_reveal,
            passwords::passwords_history,
            passwords::passwords_copy,
            passwords::passwords_copy_text,
            passwords::passwords_totp,
            passwords::passwords_totp_check,
            passwords::passwords_totp_scan_clipboard,
            passwords::passwords_totp_scan_file,
            passwords::passwords_verify_pending,
            passwords::passwords_verify_answer,
            passwords::passwords_passkey_delete,
            passwords::passwords_save,
            passwords::passwords_delete,
            passwords::passwords_generate,
            passwords::passwords_strength,
            passwords::passwords_import_pick,
            passwords::passwords_import,
            passwords::passwords_export,
            tray::tray_get,
            tray::tray_set,
            startup::startup_get,
            startup::startup_set_enabled,
            startup::startup_set_minimized,
            updater::check_for_update,
            updater::install_update,
            account::account_profile,
            account::account_access,
            account::account_sign_in,
            account::account_cancel_sign_in,
            account::account_sign_out,
            account::account_pull,
            account::account_push,
            apps::apps_installed,
            apps::apps_cancel,
            apps::winget::apps_search,
            apps::winget::apps_package_links,
            apps::winget::apps_install_winget,
            apps::winget::apps_install_ignoring_hash,
            apps::custom::apps_custom_catalog,
            apps::custom::apps_install_custom,
            apps::custom::apps_activate,
            apps::io::apps_export_list,
            apps::io::apps_import_list,
            apps::creative::creative_catalog,
            apps::creative::creative_install,
            apps::creative::creative_clip_studio_restore_available,
            apps::creative::creative_clip_studio_swap_exe,
            cleaner::cleaner_categories,
            cleaner::cleaner_scan,
            cleaner::cleaner_scan_elevated,
            cleaner::cleaner_admin_ready,
            cleaner::cleaner_clean,
            cleaner::cleaner_clean_elevated,
            debloat::debloat_status,
            debloat::debloat_restore_point,
            debloat::debloat_run,
            debloat::debloat_undo,
            debloat::debloat_open_store,
            debloat::debloat_start_menu_set,
            debloat::debloat_start_menu_hide_recommended,
            debloat::debloat_start_menu_apply_pins,
            debloat::debloat_start_menu_restore_pins,
            maintenance::maintenance_cards,
            maintenance::maintenance_run,
            maintenance::maintenance_cancel,
            maintenance::maintenance_running,
            spotify_hub::spotify_hub_get_state,
            spotify_hub::spotify_hub_preview_purge,
            spotify_hub::spotify_hub_run,
            spotify_hub::spotify_hub_cancel,
            windows_optimization::windows_optimization_get_state,
            windows_optimization::windows_optimization_set_auto_logon,
            windows_optimization::windows_optimization_restart_to_firmware,
            game_saves::commands::game_saves_get_state,
            game_saves::commands::game_saves_pick_backup_folder,
            game_saves::commands::game_saves_use_cloud_folder,
            game_saves::commands::game_saves_open_backup_folder,
            game_saves::commands::game_saves_open_game_folder,
            game_saves::commands::game_saves_detect_cloud_folders,
            game_saves::commands::game_saves_refresh_roots,
            game_saves::commands::game_saves_add_root,
            game_saves::commands::game_saves_remove_root,
            game_saves::commands::game_saves_set_schedule,
            game_saves::commands::game_saves_set_game_auto_backup,
            game_saves::commands::game_saves_pick_folder,
            game_saves::commands::game_saves_upsert_custom_game,
            game_saves::commands::game_saves_remove_custom_game,
            game_saves::commands::game_saves_set_path_mapping,
            game_saves::commands::game_saves_remove_path_mapping,
            game_saves::commands::game_saves_scan,
            game_saves::commands::game_saves_update_database,
            game_saves::commands::game_saves_backup,
            project_backups::commands::project_backups_get_state,
            project_backups::commands::project_backups_import,
            project_backups::commands::project_backups_set_provider,
            project_backups::commands::project_backups_pick_folder,
            project_backups::commands::project_backups_save_project,
            project_backups::commands::project_backups_remove_project,
            project_backups::commands::project_backups_set_exclusions,
            project_backups::commands::project_backups_preview,
            project_backups::commands::project_backups_backup,
            project_backups::commands::project_backups_cancel,
            project_backups::commands::project_backups_cancel_preview,
            project_backups::commands::project_backups_start_cloud,
            project_backups::commands::project_backups_list,
            project_backups::commands::project_backups_compare,
            project_backups::commands::project_backups_file_diff,
            project_backups::commands::project_backups_open,
            github_releases::commands::github_releases_get_state,
            github_releases::commands::github_releases_dismiss_import,
            github_releases::commands::github_releases_status,
            github_releases::commands::github_releases_fetch,
            github_releases::commands::github_releases_add_folder,
            github_releases::commands::github_releases_scan_folder,
            github_releases::commands::github_releases_add_repos,
            github_releases::commands::github_releases_remove_repo,
            github_releases::commands::github_releases_set_entry,
            github_releases::commands::github_releases_remote,
            github_releases::commands::github_releases_changes,
            github_releases::commands::github_releases_file_diff,
            github_releases::commands::github_releases_stage,
            github_releases::commands::github_releases_commit,
            github_releases::commands::github_releases_push,
            github_releases::commands::github_releases_pull,
            github_releases::commands::github_releases_cancel,
            github_releases::commands::github_releases_cancel_scan,
            github_releases::commands::github_releases_ai_commit_message,
            github_releases::commands::github_releases_build_info,
            github_releases::commands::github_releases_build,
            github_releases::commands::github_releases_artifacts,
            github_releases::commands::github_releases_checksum,
            github_releases::commands::github_releases_open_in_editor,
            github_releases::commands::github_releases_reveal,
            github_releases::commands::github_releases_release_info,
            github_releases::commands::github_releases_recent_commits,
            github_releases::commands::github_releases_preflight,
            github_releases::commands::github_releases_ai_notes,
            github_releases::commands::github_releases_release,
            github_releases::commands::github_releases_resume,
            github_releases::commands::github_releases_watch,
            github_releases::commands::github_releases_list_releases,
            github_releases::commands::github_releases_update_release,
            github_releases::commands::github_releases_delete_releases,
            github_releases::commands::github_releases_lone_tags,
            github_releases::commands::github_releases_delete_tags,
            github_releases::commands::github_releases_ai_combine,
            github_releases::commands::github_releases_gh_status,
            github_releases::commands::github_releases_connect_gh,
            github_releases::commands::github_releases_connect_token,
            github_releases::commands::github_releases_disconnect,
            github_releases::commands::github_releases_gh_login,
            github_releases::commands::github_releases_install_tool,
            github_releases::commands::github_releases_ai_set_key,
            github_releases::commands::github_releases_ai_remove_key,
            github_releases::commands::github_releases_ai_set_settings,
            github_releases::commands::github_releases_ai_ollama_models,
            github_releases::commands::github_releases_ai_test,
            game_saves::commands::game_saves_restore,
            game_saves::commands::game_saves_undo_last_restore,
            game_saves::commands::game_saves_sync_export,
            game_saves::commands::game_saves_sync_import,
            game_saves::commands::game_saves_cancel,
            game_saves::covers::game_saves_covers
        ]))
        .build(context())
        .expect("error while building the application");

    app.run(move |_app, event| {
        // Downloaded packages are removed once the app is closed.
        if matches!(event, tauri::RunEvent::Exit) {
            // An elevated scan runs in its own process, so it has to be told
            // to stop rather than dying with the window.
            running.stop_all();
            game_saves_state.cancel();
            spotify_hub_state.cancel_all();
            github_releases_state.cancel_all();
            jobs.cancel_all();
            cleanup.run();
        }
    });
}
