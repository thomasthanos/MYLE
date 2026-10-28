mod engine;
mod model;
mod native;
mod store;
#[cfg(test)]
mod tests;
use engine::{Cancel, Driver};
use model::{Config, Mode, Point, Profiles};
use serde::Serialize;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};
const EVENT: &str = "autoclicker-status";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Ready,
    Countdown,
    Running,
    Stopping,
    Picking,
    Completed,
    Stopped,
    Error,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    revision: u64,
    run_id: u64,
    phase: Phase,
    mode: Mode,
    #[serde(rename = "loop")]
    loop_number: u64,
    loops: Option<u32>,
    step: usize,
    steps: usize,
    click: u32,
    clicks: u32,
    completed: u64,
    total: Option<u64>,
    countdown: u32,
    hotkey: bool,
    hotkey_error: Option<String>,
    error: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            revision: 0,
            run_id: 0,
            phase: Phase::Ready,
            mode: Mode::Simple,
            loop_number: 0,
            loops: None,
            step: 0,
            steps: 0,
            click: 0,
            clicks: 0,
            completed: 0,
            total: None,
            countdown: 0,
            hotkey: false,
            hotkey_error: None,
            error: None,
        }
    }
}
impl Status {
    fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Countdown | Phase::Running | Phase::Stopping | Phase::Picking
        )
    }
}
#[derive(Default)]
struct Run {
    status: Status,
    prepared: Option<Config>,
    cancel: Arc<Cancel>,
    cancelled_picks: std::collections::VecDeque<String>,
}
#[derive(Default)]
struct Inner {
    run: Mutex<Run>,
    profile_io: Mutex<()>,
    notify: OnceLock<Box<dyn Fn(Status) + Send + Sync>>,
    initialized: AtomicBool,
    exiting: AtomicBool,
    hotkey_thread: AtomicU32,
}
#[derive(Clone, Default)]
pub struct AutoClickerState(Arc<Inner>);
impl AutoClickerState {
    fn emit(&self, status: Status) {
        if let Some(notify) = self.0.notify.get() {
            notify(status);
        }
    }
    fn record(&self, change: impl FnOnce(&mut Run)) -> Status {
        let mut run = self.0.run.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut run);
        run.status.revision += 1;
        run.status.clone()
    }
    fn update(&self, change: impl FnOnce(&mut Run)) -> Status {
        let status = self.record(change);
        self.emit(status.clone());
        status
    }
    fn status(&self) -> Status {
        self.0
            .run
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
    fn initialize(&self, app: AppHandle) {
        self.0.notify.get_or_init(|| {
            Box::new(move |status| {
                let _ = app.emit_to("main", EVENT, status);
            })
        });
        if !self.0.initialized.swap(true, Ordering::AcqRel) {
            let state = self.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("myle-clicker-hotkey".into())
                .spawn(move || native::hotkeys(state))
            {
                self.hotkey_result(Err(format!("Cannot register F6: {error}")));
            }
        }
    }
    fn configure(&self, config: Option<Config>) -> Result<(), String> {
        let checked = config.as_ref().map(Config::validate).transpose();
        let mut run = self.0.run.lock().unwrap_or_else(|e| e.into_inner());
        // A worker owns an immutable snapshot. Edits arriving after an F6 start
        // may update/disarm the next run without changing the current one.
        // Invalid edits explicitly disarm the old snapshot used by the global hotkey.
        run.prepared = if checked.is_ok() { config } else { None };
        checked.map(|_| ())
    }
    fn launch(
        &self,
        config: Config,
        mut driver: impl Driver + Send + 'static,
        countdown_ms: u64,
    ) -> Result<Status, String> {
        config.validate()?;
        let (id, cancel, initial) = {
            let mut run = self.0.run.lock().unwrap_or_else(|e| e.into_inner());
            if self.0.exiting.load(Ordering::Acquire) {
                return Err("MYLE is closing.".into());
            }
            if run.status.busy() {
                return Err("Stop the current operation before starting another.".into());
            }
            run.cancel = Arc::new(Cancel::default());
            let old = &run.status;
            run.status = Status {
                revision: old.revision + 1,
                run_id: old.run_id + 1,
                phase: Phase::Countdown,
                mode: config.mode,
                loops: config.loops,
                steps: if config.mode == Mode::Simple {
                    1
                } else {
                    config.steps.len()
                },
                total: config.total(),
                hotkey: old.hotkey,
                hotkey_error: old.hotkey_error.clone(),
                countdown: countdown_ms.div_ceil(1000) as u32,
                ..Status::default()
            };
            (run.status.run_id, run.cancel.clone(), run.status.clone())
        };
        self.emit(initial);
        let state = self.clone();
        let result = std::thread::Builder::new()
            .name("myle-clicker".into())
            .spawn(move || {
                let mut remaining = countdown_ms;
                while remaining > 0 && !cancel.stopped() {
                    let wait = remaining.min(1000);
                    if !cancel.wait(wait) {
                        break;
                    }
                    remaining -= wait;
                    state.update(|run| {
                        if run.status.run_id == id {
                            run.status.countdown = remaining.div_ceil(1000) as u32;
                        }
                    });
                }
                if cancel.stopped() {
                    state.finish(id, Phase::Stopped, None);
                    return;
                }
                state.update(|run| {
                    if run.status.run_id == id {
                        run.status.phase = Phase::Running;
                        run.status.countdown = 0;
                    }
                });
                let mut last = Instant::now() - Duration::from_secs(1);
                let result = engine::execute(&config, &cancel, &mut driver, |progress| {
                    // Keep the stored counters exact; only UI notifications are throttled.
                    let status = state.record(|run| {
                        if run.status.run_id != id {
                            return;
                        }
                        run.status.loop_number = progress.loop_number;
                        run.status.step = progress.step;
                        run.status.click = if config.mode == Mode::Simple {
                            progress.completed.min(u64::from(u32::MAX)) as u32
                        } else {
                            progress.click
                        };
                        run.status.clicks = if config.mode == Mode::Simple {
                            config.loops.unwrap_or(0)
                        } else {
                            config.steps[progress.step - 1].clicks
                        };
                        run.status.completed = progress.completed;
                    });
                    if last.elapsed() >= Duration::from_millis(60)
                        || Some(progress.completed) == config.total()
                    {
                        last = Instant::now();
                        state.emit(status);
                    }
                });
                let phase = if result.is_err() {
                    Phase::Error
                } else if cancel.stopped() {
                    Phase::Stopped
                } else {
                    Phase::Completed
                };
                state.finish(id, phase, result.err());
            });
        if let Err(error) = result {
            self.finish(
                id,
                Phase::Error,
                Some(format!("Cannot start the clicker: {error}")),
            );
            return Err(error.to_string());
        }
        Ok(self.status())
    }
    fn finish(&self, id: u64, phase: Phase, error: Option<String>) {
        self.update(|run| {
            if run.status.run_id == id {
                run.status.phase = phase;
                run.status.error = error;
                run.status.countdown = 0;
            }
        });
    }
    pub fn stop(&self) -> Status {
        self.stop_request(None)
    }
    fn stop_request(&self, pick_request: Option<&str>) -> Status {
        self.update(|run| {
            if let Some(request) = pick_request.filter(|id| uuid::Uuid::parse_str(id).is_ok()) {
                run.cancelled_picks.push_back(request.to_string());
                while run.cancelled_picks.len() > 64 {
                    run.cancelled_picks.pop_front();
                }
            }
            if run.status.busy() {
                run.cancel.stop();
                run.status.phase = Phase::Stopping;
            }
        })
    }
    fn begin_pick_request(
        &self,
        request: Option<&str>,
    ) -> Result<Option<(u64, Arc<Cancel>)>, String> {
        let mut run = self.0.run.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(request) = request {
            if uuid::Uuid::parse_str(request).is_err() {
                return Err("Invalid location request.".into());
            }
            if run.cancelled_picks.iter().any(|id| id == request) {
                return Ok(None);
            }
        }
        if run.status.busy() || self.0.exiting.load(Ordering::Acquire) {
            return Err("Stop the current operation before picking a location.".into());
        }
        run.cancel = Arc::new(Cancel::default());
        run.status.run_id += 1;
        run.status.revision += 1;
        run.status.phase = Phase::Picking;
        run.status.error = None;
        run.status.loop_number = 0;
        run.status.step = 0;
        run.status.steps = 0;
        run.status.click = 0;
        run.status.clicks = 0;
        run.status.countdown = 0;
        run.status.completed = 0;
        run.status.total = None;
        let result = (run.status.run_id, run.cancel.clone());
        let status = run.status.clone();
        drop(run);
        self.emit(status);
        Ok(Some(result))
    }
    fn toggle(&self) {
        if self.status().busy() {
            self.stop();
            return;
        }
        let config = self
            .0
            .run
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prepared
            .clone();
        let result = config
            .ok_or("Open Auto Clicker and finish its settings before using F6.".to_string())
            .and_then(|config| self.launch(config, native::WindowsDriver, 3000));
        if let Err(error) = result {
            self.update(|run| run.status.error = Some(error));
        }
    }
    fn hotkey_result(&self, result: Result<(), String>) {
        self.update(|run| {
            run.status.hotkey = result.is_ok();
            run.status.hotkey_error = result.err();
        });
    }
    fn set_hotkey_thread(&self, id: u32) {
        self.0.hotkey_thread.store(id, Ordering::Release);
    }
    pub fn shutdown(&self) {
        self.0.exiting.store(true, Ordering::Release);
        self.stop();
        let thread = self.0.hotkey_thread.load(Ordering::Acquire);
        if thread != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    thread,
                    windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                    0,
                    0,
                );
            }
        }
    }
}
#[tauri::command]
pub fn autoclicker_start(
    config: Config,
    state: State<'_, AutoClickerState>,
    app: AppHandle,
) -> Result<Status, String> {
    state.initialize(app);
    state.configure(Some(config.clone()))?;
    state.launch(config, native::WindowsDriver, 3000)
}
#[tauri::command]
pub fn autoclicker_stop(
    pick_request_id: Option<String>,
    state: State<'_, AutoClickerState>,
) -> Status {
    state.stop_request(pick_request_id.as_deref())
}
#[tauri::command]
pub fn autoclicker_configure(
    config: Option<Config>,
    state: State<'_, AutoClickerState>,
    app: AppHandle,
) -> Result<(), String> {
    state.initialize(app);
    state.configure(config)
}
#[tauri::command]
pub fn autoclicker_get_status(state: State<'_, AutoClickerState>, app: AppHandle) -> Status {
    state.initialize(app);
    state.status()
}
#[tauri::command]
pub async fn autoclicker_pick_location(
    request_id: Option<String>,
    state: State<'_, AutoClickerState>,
) -> Result<Option<Point>, String> {
    let state = state.inner().clone();
    let Some((id, cancel)) = state.begin_pick_request(request_id.as_deref())? else {
        return Ok(None);
    };
    let result = tauri::async_runtime::spawn_blocking(move || native::pick(&cancel))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    state.finish(
        id,
        if result.is_err() {
            Phase::Error
        } else {
            Phase::Ready
        },
        result.as_ref().err().cloned(),
    );
    result
}
#[tauri::command]
pub async fn autoclicker_load_profiles(
    state: State<'_, AutoClickerState>,
) -> Result<Profiles, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = state.0.profile_io.lock().map_err(|e| e.to_string())?;
        store::load(&store::path()?)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn autoclicker_save_profiles(
    profiles: Profiles,
    state: State<'_, AutoClickerState>,
) -> Result<(), String> {
    profiles.validate()?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = state.0.profile_io.lock().map_err(|e| e.to_string())?;
        store::save(&store::path()?, &profiles)
    })
    .await
    .map_err(|e| e.to_string())?
}
