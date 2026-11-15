//! The setup window: one frameless WebView2 window (`installer.html`) and
//! the commands its page calls. The page decides nothing about files; it
//! passes the user's choices here and shows the progress it is sent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::cleanup::AfterExit;
use crate::cli::Cli;
use crate::engine::{self, InstallOptions, Progress, ShortcutChoice, ShortcutMode};
use crate::{DECLINED, FAILED, payload, processes, product, publisher, registry, shell};

pub enum Mode {
    Install {
        payload: &'static [u8],
    },
    Uninstall {
        dir: PathBuf,
    },
    Notice {
        title: String,
        message: String,
        kind: &'static str,
        action_label: Option<String>,
    },
}

struct Context {
    mode: Mode,
    cli: Cli,
    /// Set while files are being changed: the window cannot be closed then.
    busy: AtomicBool,
    installed_exe: Mutex<Option<PathBuf>>,
    uninstall_exit: Arc<AtomicI32>,
    /// Deleted once the window has closed.
    after_exit: Arc<Mutex<AfterExit>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    version: Option<String>,
    dir: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shortcuts {
    desktop: bool,
    start_menu: bool,
    startup: bool,
    #[serde(default)]
    taskbar: bool,
    /// Started by Windows at sign-in: minimized to the taskbar, or on screen.
    #[serde(default = "minimized_by_default")]
    start_minimized: bool,
}

fn minimized_by_default() -> bool {
    true
}

impl From<ShortcutChoice> for Shortcuts {
    fn from(choice: ShortcutChoice) -> Self {
        Self {
            desktop: choice.desktop,
            start_menu: choice.start_menu,
            startup: choice.startup,
            taskbar: registry::taskbar_preference().enabled,
            start_minimized: registry::start_minimized(),
        }
    }
}

impl From<Shortcuts> for ShortcutChoice {
    fn from(shortcuts: Shortcuts) -> Self {
        Self {
            desktop: shortcuts.desktop,
            start_menu: shortcuts.start_menu,
            startup: shortcuts.startup,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NoticeInfo {
    title: String,
    message: String,
    kind: String,
    action_label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupState {
    mode: &'static str,
    product: &'static str,
    /// What this setup installs, or the installed version when uninstalling.
    version: String,
    installed: Option<Installed>,
    dir: String,
    /// Bytes on disk once installed.
    size: u64,
    shortcuts: Shortcuts,
    /// `/P`: install at once, then close by itself.
    passive: bool,
    /// Whether this setup carries the app at all (a dev build may not).
    ready: bool,
    /// The browser extension's download page, offered when there is one.
    extension_url: Option<&'static str>,
    notice: Option<NoticeInfo>,
    /// MYLE's release certificate, which the last page offers to trust
    /// (`publisher.rs`); installing only, with a setup signed with it.
    publisher: Option<Publisher>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Publisher {
    name: &'static str,
    /// Windows already trusts it here, for this user or every user.
    trusted: bool,
}

#[tauri::command(async)]
fn setup_state(context: State<'_, Context>) -> SetupState {
    let existing = registry::install_dir().filter(|dir| {
            dir.join(product::exe_name()).is_file() || dir.join(product::legacy_exe_name()).is_file()
        });
    let installed = existing.as_ref().map(|dir| Installed {
        version: registry::installed_version(),
        dir: dir.display().to_string(),
    });
    match &context.mode {
        Mode::Install { payload: bytes } => {
            let header = payload::open(bytes).ok().map(|(header, _)| header);
            let dir = install_dir(&context);
            // The user's last choices; a fresh install offers the Desktop and
            // Start menu shortcuts, and leaves starting with Windows off.
            let shortcuts: Shortcuts = engine::default_shortcuts(existing.as_deref()).into();
            SetupState {
                mode: "install",
                product: product::NAME,
                version: header
                    .as_ref()
                    .map_or_else(|| product::VERSION.to_string(), |h| h.version.clone()),
                installed,
                dir: dir.display().to_string(),
                size: header.as_ref().map_or(0, payload::Header::total_size),
                shortcuts,
                passive: context.cli.passive,
                ready: header.is_some(),
                extension_url: product::extension_url(),
                notice: None,
                // Offered only by a setup signed with the certificate itself:
                // for an older release or an unsigned build it changes nothing.
                publisher: std::env::current_exe()
                    .is_ok_and(|exe| publisher::signs(&exe))
                    .then(|| Publisher {
                        name: publisher::NAME,
                        trusted: publisher::is_trusted(),
                    }),
            }
        }
        Mode::Uninstall { dir } => SetupState {
            mode: "uninstall",
            product: product::NAME,
            version: registry::installed_version().unwrap_or_else(|| product::VERSION.to_string()),
            installed,
            dir: dir.display().to_string(),
            size: 0,
            shortcuts: ShortcutChoice::from_array([false; 3]).into(),
            passive: false,
            ready: true,
            extension_url: None,
            notice: None,
            publisher: None,
        },
        Mode::Notice {
            title,
            message,
            kind,
            action_label,
        } => SetupState {
            mode: "notice",
            product: product::NAME,
            version: product::VERSION.to_string(),
            installed: None,
            dir: String::new(),
            size: 0,
            shortcuts: ShortcutChoice::from_array([false; 3]).into(),
            passive: false,
            ready: true,
            extension_url: None,
            notice: Some(NoticeInfo {
                title: title.clone(),
                message: message.clone(),
                kind: kind.to_string(),
                action_label: action_label.clone(),
            }),
            publisher: None,
        },
    }
}

/// Names of the programs running from the folder, for the "close it?" prompt.
#[tauri::command(async)]
fn setup_running(dir: String) -> Vec<String> {
    let mut names: Vec<String> = processes::running_in(Path::new(&dir))
        .iter()
        .filter_map(|p| p.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Trusts MYLE's release certificate for this Windows user (`publisher.rs`),
/// when the user asks on the last page. Windows shows its own question, so
/// this waits for their answer; `false` when they said no.
#[tauri::command]
async fn setup_trust_publisher() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(publisher::trust)
        .await
        .map_err(|e| e.to_string())?
}

/// Rejects a folder before anything happens, with a sentence for the page.
#[tauri::command(async)]
fn setup_check_folder(dir: String) -> Result<(), String> {
    engine::check_dir(Path::new(dir.trim()))
}

/// Clears `busy` however the operation ends.
struct Busy<'a>(&'a AtomicBool);

impl<'a> Busy<'a> {
    fn start(flag: &'a AtomicBool) -> Result<Self, String> {
        if flag.swap(true, Ordering::SeqCst) {
            return Err("Setup is already working.".into());
        }
        Ok(Self(flag))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Where the app goes. It is not the user's choice: always the app's own
/// folder (%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier), or wherever an
/// earlier install already is. Only `/D=` on the command line (tests) moves it.
fn install_dir(context: &Context) -> PathBuf {
    context
        .cli
        .dir
        .clone()
        .or_else(|| registry::install_dir().filter(|dir| {
            dir.join(product::exe_name()).is_file() || dir.join(product::legacy_exe_name()).is_file()
        }))
        .unwrap_or_else(engine::default_dir)
}

/// What the window shows once the install is done.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallDone {
    dir: String,
    /// Shortcuts that could not be created; the app itself is installed.
    warnings: Vec<String>,
}

#[tauri::command]
async fn setup_install(
    context: State<'_, Context>,
    request: Shortcuts,
    on_event: Channel<Progress>,
) -> Result<InstallDone, String> {
    let Mode::Install { payload: bytes } = context.mode else {
        return Err("This is the uninstaller.".into());
    };
    let _busy = Busy::start(&context.busy)?;
    // The in-app updater quits by itself right after starting us.
    let grace = if context.cli.update {
        Duration::from_secs(20)
    } else {
        Duration::ZERO
    };
    let start_minimized = request.start_minimized;
    let offer_taskbar = request.taskbar && request.start_menu;
    let options = InstallOptions {
        dir: install_dir(&context),
        shortcuts: ShortcutMode::Choose(request.into()),
        live: false,
    };
    let mut installed = tauri::async_runtime::spawn_blocking(move || {
        let mut report = |event: Progress| {
            let _ = on_event.send(event);
        };
        report(Progress::Stage {
            stage: engine::Stage::ClosingApp,
        });
        engine::close_running(&options.dir, grace)?;
        engine::install(bytes, &options, &mut report)
    })
    .await
    .map_err(|e| e.to_string())??;
    registry::set_start_minimized(start_minimized);
    // Passive updates never reset a suggestion the user already dismissed.
    if (!context.cli.update || !context.cli.passive)
        && let Err(error) = registry::remember_taskbar_choice(offer_taskbar)
    {
        installed.warnings.push(format!("The taskbar choice could not be saved: {error}"));
    }
    let dir = installed
        .exe
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    *context
        .installed_exe
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(installed.exe);
    Ok(InstallDone {
        dir,
        warnings: installed.warnings,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UninstallRequest {
    remove_data: bool,
}

#[tauri::command]
async fn setup_uninstall(
    context: State<'_, Context>,
    request: UninstallRequest,
    on_event: Channel<Progress>,
) -> Result<(), String> {
    let Mode::Uninstall { dir } = &context.mode else {
        return Err("This is the installer.".into());
    };
    let _busy = Busy::start(&context.busy)?;
    if context.uninstall_exit.load(Ordering::SeqCst) == 0 {
        return Err("MYLE was already removed.".into());
    }
    context.uninstall_exit.store(FAILED, Ordering::SeqCst);
    let dir = dir.clone();
    let after_exit = tauri::async_runtime::spawn_blocking(move || {
        let mut report = |event: Progress| {
            let _ = on_event.send(event);
        };
        report(Progress::Stage {
            stage: engine::Stage::ClosingApp,
        });
        engine::close_running(&dir, Duration::ZERO)?;
        engine::uninstall(&dir, request.remove_data, &mut report)
    })
    .await
    .map_err(|e| e.to_string())??;
    context
        .after_exit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .append(after_exit);
    context.uninstall_exit.store(0, Ordering::SeqCst);
    Ok(())
}

/// Opens the installed app and returns once its first window is on screen
/// (or after a while regardless), so the setup never closes onto an empty
/// desktop.
#[tauri::command]
async fn setup_launch(context: State<'_, Context>) -> Result<(), String> {
    let exe = context
        .installed_exe
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or("Nothing was installed yet.")?;
    let preference = registry::taskbar_preference();
    let request_pin = !context.cli.passive && preference.pending;
    let pid = shell::launch_with_pin_request(&exe, context.cli.update, request_pin)?;
    let _ = tauri::async_runtime::spawn_blocking(move || {
        processes::wait_for_window(pid, Duration::from_secs(12))
    })
    .await;
    Ok(())
}

/// Opens the browser extension's download page in the default browser.
#[tauri::command(async)]
fn setup_open_extension() -> Result<(), String> {
    shell::open_url(product::extension_url().ok_or("The extension has no page yet.")?)
}

#[tauri::command]
fn setup_exit(app: tauri::AppHandle, context: State<'_, Context>) {
    if !context.busy.load(Ordering::SeqCst) {
        app.exit(0);
    }
}

#[tauri::command]
fn setup_focus_existing(app: tauri::AppHandle) {
    shell::focus_existing_setup();
    app.exit(0);
}

/// Opens the window and runs until it closes. Returns the exit code and
/// what is left to delete now that the window is gone.
pub fn run(mode: Mode, cli: Cli) -> (i32, AfterExit) {
    // The window's own browser profile lives in %TEMP% and goes with it: the
    // setup never creates the app's profile, and the uninstaller never holds
    // files in the folders it is removing.
    let webview_dir = std::env::temp_dir().join(format!(
        "{}-setup-{}",
        product::BINARY,
        uuid::Uuid::new_v4().simple()
    ));
    let mut after_exit = AfterExit::default();
    after_exit.remove.push(webview_dir.clone());
    let after_exit = Arc::new(Mutex::new(after_exit));
    let uninstalling = matches!(mode, Mode::Uninstall { .. });
    let is_notice = matches!(mode, Mode::Notice { .. });
    let uninstall_exit = Arc::new(AtomicI32::new(DECLINED));
    let context = Context {
        mode,
        cli,
        busy: AtomicBool::new(false),
        installed_exe: Mutex::new(None),
        uninstall_exit: uninstall_exit.clone(),
        after_exit: after_exit.clone(),
    };

    let app = tauri::Builder::default()
        .manage(context)
        .setup(move |app| {
            let title = if uninstalling {
                format!("Uninstall {}", product::NAME)
            } else if let Mode::Notice { title, .. } = &app.state::<Context>().mode {
                title.clone()
            } else {
                format!("{} Setup", product::NAME)
            };
            let window = WebviewWindowBuilder::new(app, "setup", WebviewUrl::App("installer.html".into()))
                .title(title)
                .resizable(false)
                .maximizable(false)
                .decorations(false)
                .shadow(true)
                .visible(false)
                .devtools(cfg!(debug_assertions))
                .background_color(tauri::window::Color(10, 12, 18, 255))
                .data_directory(webview_dir.clone());
            let window = if uninstalling {
                let size = tauri::LogicalSize::new(420.0, 550.0);
                let monitor = crate::window_placement::cursor_monitor(app.handle());
                let window = window.inner_size(size.width, size.height).build()?;
                if let Some(monitor) = monitor {
                    let work = monitor.work_area().size;
                    let size = size.to_physical::<u32>(monitor.scale_factor());
                    crate::window_placement::place_on_monitor(
                        &window,
                        &monitor,
                        tauri::PhysicalSize::new(size.width.min(work.width), size.height.min(work.height)),
                    )?;
                } else {
                    window.center()?;
                }
                window
            } else if is_notice {
                let size = tauri::LogicalSize::new(460.0, 210.0);
                let monitor = crate::window_placement::cursor_monitor(app.handle());
                let window = window.inner_size(size.width, size.height).build()?;
                if let Some(monitor) = monitor {
                    let work = monitor.work_area().size;
                    let size = size.to_physical::<u32>(monitor.scale_factor());
                    crate::window_placement::place_on_monitor(
                        &window,
                        &monitor,
                        tauri::PhysicalSize::new(size.width.min(work.width), size.height.min(work.height)),
                    )?;
                } else {
                    window.center()?;
                }
                window
            } else {
                window.inner_size(720.0, 540.0).center().build()?
            };
            crate::webview_policy::harden(&window)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing mid-copy would leave half an app behind.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<Context>().busy.load(Ordering::SeqCst)
            {
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            setup_state,
            setup_running,
            setup_check_folder,
            setup_trust_publisher,
            setup_install,
            setup_uninstall,
            setup_launch,
            setup_open_extension,
            setup_exit,
            setup_focus_existing
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            shell::message(
                product::NAME,
                &format!("Setup could not open its window: {error}"),
                true,
            );
            return (
                1,
                std::mem::take(&mut *after_exit.lock().unwrap_or_else(|p| p.into_inner())),
            );
        }
    };
    let code = app.run_return(|_, _| {});
    let code = if uninstalling && code == 0 {
        uninstall_exit.load(Ordering::SeqCst)
    } else {
        code
    };
    let pending = std::mem::take(&mut *after_exit.lock().unwrap_or_else(|p| p.into_inner()));
    (code, pending)
}
