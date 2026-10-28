// ! What a scheduled backup leaves on the screen: a small window of MYLE's ! own at the bottom right, over every other window, with a chime. The ! headless backup starts `MYLE.exe --game-saves-notice=<data>`, a run of ! the app with only this window, which exits when it closes. Clicking it ! opens MYLE at Game Saves.
use std::os::windows::process::CommandExt;
use std::process::Command;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use tauri::{App, WebviewUrl, WebviewWindowBuilder};
const FLAG: &str = "--game-saves-notice=";
/// Starts MYLE at Game Saves; a running MYLE is told to go there.
pub const OPEN_FLAG: &str = "--open-game-saves";
pub const LABEL: &str = "game-saves-notice";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 112.0;
/// From the work area's edges (above the taskbar).
const MARGIN: f64 = 14.0;
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeKind {
    BackedUp,
    NothingNew,
    Failed,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub kind: NoticeKind,
    /// Games backed up.
    pub games: u64,
    /// Games that could not be.
    pub failed: Vec<String>,
    /// Why nothing could be backed up at all.
    pub error: Option<String>,
    /// Unix seconds.
    pub at: u64,
}
impl Notice {
    /// From a scheduled run's outcome: games backed up and games that failed,
    /// or why it could not run.
    pub fn from_result(result: &Result<(u64, Vec<String>), String>, at: u64) -> Self {
        match result {
            Ok((games, failed)) => Notice {
                kind: if !failed.is_empty() {
                    NoticeKind::Failed
                } else if *games > 0 {
                    NoticeKind::BackedUp
                } else {
                    NoticeKind::NothingNew
                },
                games: *games,
                failed: failed.clone(),
                error: None,
                at,
            },
            Err(error) => Notice {
                kind: NoticeKind::Failed,
                games: 0,
                failed: Vec::new(),
                error: Some(error.clone()),
                at,
            },
        }
    }
    fn encode(&self) -> Option<String> {
        serde_json::to_vec(self).ok().map(|json| URL_SAFE_NO_PAD.encode(json))
    }
    fn decode(data: &str) -> Option<Self> {
        let json = URL_SAFE_NO_PAD.decode(data).ok()?;
        serde_json::from_slice(&json).ok()
    }
}
/// Puts the notice on the screen from the headless backup. It never holds
/// the backup up, and a notice that cannot start is simply not shown.
pub fn show(notice: &Notice) {
    let (Ok(exe), Some(data)) = (std::env::current_exe(), notice.encode()) else {
        return;
    };
    let _ = Command::new(exe)
        .arg(format!("{FLAG}{data}"))
        .creation_flags(DETACHED_PROCESS)
        .spawn();
}
/// The notice this run was started to show, if any.
pub fn from_args() -> Option<Notice> {
    std::env::args().find_map(|arg| arg.strip_prefix(FLAG).and_then(Notice::decode))
}
/// The notice's window: bottom right of the main screen's work area, on
/// top, out of the taskbar, and without taking the focus from what the user
/// is doing. The page slides itself in once it has loaded.
pub fn create_window(app: &App) -> tauri::Result<()> {
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("notice.html".into()))
        .title("MYLE")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .devtools(cfg!(debug_assertions))
        .background_color(tauri::window::Color(0, 0, 0, 0));
    if let Ok(dir) = crate::storage::local_dir() {
        // Its own profile: the main window's may be open with other options.
        builder = builder.data_directory(dir.join("webview-notice"));
    }
    if let Some(monitor) = app.primary_monitor()? {
        let scale = monitor.scale_factor();
        let work = monitor.work_area();
        let right = f64::from(work.position.x) / scale + f64::from(work.size.width) / scale;
        let bottom = f64::from(work.position.y) / scale + f64::from(work.size.height) / scale;
        builder = builder.position(right - WIDTH - MARGIN, bottom - HEIGHT - MARGIN);
    }
    let window = builder.build()?;
    crate::webview_policy::harden(&window)?;
    Ok(())
}
/// Opens MYLE at Game Saves: a running MYLE is focused and goes there.
pub fn open_game_saves() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(&exe)
            .arg(OPEN_FLAG)
            .current_dir(exe.parent().unwrap_or(&exe))
            .creation_flags(DETACHED_PROCESS)
            .spawn();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_notice_survives_the_command_line() {
        let notice = Notice::from_result(&Ok((3, vec!["Hades \"II\"".into()])), 1_790_000_000);
        assert_eq!(notice.kind, NoticeKind::Failed);
        let data = notice.encode().unwrap();
        assert!(data.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(Notice::decode(&data), Some(notice));
        assert_eq!(Notice::decode("not base64!"), None);
    }
    #[test]
    fn what_a_scheduled_run_says() {
        assert_eq!(Notice::from_result(&Ok((2, vec![])), 0).kind, NoticeKind::BackedUp);
        assert_eq!(Notice::from_result(&Ok((0, vec![])), 0).kind, NoticeKind::NothingNew);
        let failed = Notice::from_result(&Err("G:\\ is not connected".into()), 0);
        assert_eq!((failed.kind, failed.error.as_deref()), (NoticeKind::Failed, Some("G:\\ is not connected")));
    }
}
