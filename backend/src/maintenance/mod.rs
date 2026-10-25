//! System Maintenance: Windows diagnostics and repairs, one at a time.
//!
//! The page sends an action id; the command, its arguments and the elevated
//! script all come from the fixed table in `tasks.rs`.

mod runner;
mod tasks;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::apps::Jobs;
use crate::apps::process::ERROR_CANCELLED;
use crate::console::Line;
use tasks::{Elevation, Run, Section};

/// DISM's "the work is done, now restart".
const REBOOT_REQUIRED: i32 = 3010;
/// winget: nothing had an update available.
const NOTHING_TO_UPGRADE: i32 = 0x8A15_002B_u32 as i32;

// ---------------------------------------------------------------------------
// The cards

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInfo {
    id: String,
    label: String,
    cancellable: bool,
    confirm: Option<String>,
    cancel_confirm: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInfo {
    id: String,
    section: Section,
    title: String,
    description: String,
    icon: String,
    admin: bool,
    caution: Option<String>,
    console: bool,
    actions: Vec<ActionInfo>,
}

/// Static card data, so the page can draw before anything runs.
#[tauri::command]
pub fn maintenance_cards() -> Vec<CardInfo> {
    tasks::CARDS
        .iter()
        .map(|card| CardInfo {
            id: card.id.into(),
            section: card.section,
            title: card.title.into(),
            description: card.description.into(),
            icon: card.icon.into(),
            admin: card.needs_admin(),
            caution: card.caution.map(Into::into),
            console: card.console,
            actions: card
                .actions
                .iter()
                .map(|action| ActionInfo {
                    id: action.id.into(),
                    label: action.label.into(),
                    cancellable: action.cancellable,
                    confirm: action.confirm.map(Into::into),
                    cancel_confirm: action.cancel_confirm.map(Into::into),
                })
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Events

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum TaskEvent {
    /// The UAC prompt is on screen; nothing is running yet.
    Waiting,
    Started,
    Line {
        text: String,
        replace: bool,
    },
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum TaskOutcome {
    Done {
        note: Option<String>,
    },
    RebootRequired,
    Cancelled,
    /// Either the action turned out to need elevation, or UAC was declined.
    NeedsAdmin,
}

// ---------------------------------------------------------------------------
// One at a time

struct Active {
    action: String,
    /// Touching this file asks the elevated half to stop.
    stop: Option<PathBuf>,
    cancelled: bool,
}

/// The single-task lock. The page disables its buttons too, but this is what
/// actually stops two repairs from running over each other.
#[derive(Clone, Default)]
pub struct Running(Arc<Mutex<Option<Active>>>);

impl Running {
    /// `None` when something else already holds the lock.
    fn claim(&self, id: &str, stop: Option<PathBuf>) -> Option<RunGuard> {
        let mut slot = self.lock();
        if slot.is_some() {
            return None;
        }
        *slot = Some(Active {
            action: id.to_string(),
            stop,
            cancelled: false,
        });
        Some(RunGuard(self.clone()))
    }

    fn cancel(&self, id: &str) {
        let mut slot = self.lock();
        let Some(active) = slot.as_mut() else { return };
        if active.action != id {
            return;
        }
        active.cancelled = true;
        touch(active.stop.as_deref());
    }

    fn is_cancelled(&self) -> bool {
        self.lock().as_ref().is_some_and(|active| active.cancelled)
    }

    pub fn current(&self) -> Option<String> {
        self.lock().as_ref().map(|active| active.action.clone())
    }

    /// Reserves the maintenance lane for a pending firmware restart. Keeping
    /// the returned guard alive prevents every maintenance action, including
    /// actions that do not participate in the Install Apps job registry, from
    /// starting while the UAC prompt is open.
    pub(crate) fn reserve_for_firmware_restart(&self) -> Result<RunGuard, String> {
        self.claim("restart-to-firmware", None)
            .ok_or_else(|| "Wait for the current Maintenance task to finish first.".into())
    }

    /// Called as the app exits: an elevated scan must not outlive its window.
    pub fn stop_all(&self) {
        if let Some(active) = self.lock().as_ref() {
            touch(active.stop.as_deref());
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Active>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn touch(path: Option<&std::path::Path>) {
    if let Some(path) = path {
        let _ = std::fs::write(path, b"");
    }
}

pub(crate) struct RunGuard(Running);

impl Drop for RunGuard {
    fn drop(&mut self) {
        *self.0.lock() = None;
    }
}

#[tauri::command]
pub fn maintenance_running(running: State<'_, Running>) -> Option<String> {
    running.current()
}

#[tauri::command(async)]
pub fn maintenance_cancel(running: State<'_, Running>, jobs: State<'_, Jobs>, id: String) {
    if !tasks::find(&id).is_some_and(|(_, action)| action.cancellable) {
        return;
    }
    running.cancel(&id);
    // winget runs unelevated, so it can simply be killed.
    jobs.cancel(&id);
}

// ---------------------------------------------------------------------------
// Running an action

/// Forwards each line to the page and remembers the last one, which is what a
/// failure is reported with: the tool's own words, in the user's own language.
struct Sink<'a> {
    channel: &'a Channel<TaskEvent>,
    last: String,
}

impl Sink<'_> {
    fn line(&mut self, line: Line) {
        if !line.text.trim().is_empty() {
            self.last = line.text.clone();
        }
        let _ = self.channel.send(TaskEvent::Line {
            text: line.text,
            replace: line.replace,
        });
    }
}

#[tauri::command]
pub async fn maintenance_run(
    jobs: State<'_, Jobs>,
    running: State<'_, Running>,
    id: String,
    elevated: bool,
    on_event: Channel<TaskEvent>,
) -> Result<TaskOutcome, String> {
    let (card, action) = tasks::find(&id).ok_or("Unknown maintenance action.")?;
    if matches!(action.run, Run::WingetUpgradeAll) && !jobs.is_idle() {
        return Err(
            "An install is still running on the Install Apps page. Wait for it to finish.".into(),
        );
    }

    let workspace = (!matches!(action.run, Run::WingetUpgradeAll))
        .then(|| runner::Workspace::new(action.id))
        .transpose()?;
    let stop = workspace.as_ref().map(|w| w.stop.clone());
    let _guard = running
        .claim(&id, stop)
        .ok_or("Another maintenance task is already running.")?;

    let mut sink = Sink {
        channel: &on_event,
        last: String::new(),
    };

    let code = match &action.run {
        Run::Batch(steps) => {
            let workspace = workspace.as_ref().expect("batch actions get a workspace");
            let with_uac = card.elevation == Elevation::Yes || elevated;
            if with_uac {
                // A batch gives no sign of life until it is over, so it stays
                // in "waiting" rather than claiming a start it cannot see.
                let _ = on_event.send(TaskEvent::Waiting);
                runner::run_batch_elevated(steps, workspace, &mut |line| sink.line(line)).await?
            } else {
                let code = runner::run_plain(steps, &mut |line| sink.line(line)).await?;
                // It turned out to need rights after all: offer the retry.
                if code != 0 && card.elevation == Elevation::IfNeeded {
                    return Ok(TaskOutcome::NeedsAdmin);
                }
                code
            }
        }
        Run::Stream(step) => {
            let workspace = workspace.as_ref().expect("stream actions get a workspace");
            runner::run_stream_elevated(
                step,
                workspace,
                &mut |phase| {
                    let _ = on_event.send(match phase {
                        runner::Phase::Waiting => TaskEvent::Waiting,
                        runner::Phase::Started => TaskEvent::Started,
                    });
                },
                &mut |line| sink.line(line),
            )
            .await?
        }
        Run::DiskScan => {
            let workspace = workspace.as_ref().expect("disk scans get a workspace");
            runner::run_disk_scan(
                workspace,
                &mut |phase| {
                    let _ = on_event.send(match phase {
                        runner::Phase::Waiting => TaskEvent::Waiting,
                        runner::Phase::Started => TaskEvent::Started,
                    });
                },
                &mut |line| sink.line(line),
            )
            .await?
        }
        Run::WingetUpgradeAll => {
            let job = jobs.start(&id)?;
            let _ = on_event.send(TaskEvent::Started);
            let code =
                crate::apps::winget::upgrade_all(&job, &mut |line| sink.line(line.clone())).await?;
            if job.is_cancelled() {
                return Ok(TaskOutcome::Cancelled);
            }
            code
        }
    };

    if running.is_cancelled() {
        return Ok(TaskOutcome::Cancelled);
    }
    classify(action, code, &sink.last)
}

fn classify(action: &tasks::Action, code: i32, last_line: &str) -> Result<TaskOutcome, String> {
    if code == ERROR_CANCELLED {
        // Declining a UAC prompt is a normal choice, not a failure.
        return Ok(TaskOutcome::NeedsAdmin);
    }
    if code == REBOOT_REQUIRED || (code == 0 && action.reboot) {
        return Ok(TaskOutcome::RebootRequired);
    }
    if code == 0 {
        return Ok(TaskOutcome::Done { note: None });
    }
    if matches!(action.run, Run::WingetUpgradeAll) {
        return Ok(TaskOutcome::Done {
            note: Some(if code == NOTHING_TO_UPGRADE {
                "Everything is already up to date.".into()
            } else {
                "Some programs could not be updated — see the output.".into()
            }),
        });
    }
    if matches!(action.run, Run::DiskScan) && code == 2 {
        // Our disk worker reserves 2 for a completed scan with findings.
        // Launch errors, incomplete scans and provider failures remain errors.
        return Ok(TaskOutcome::Done {
            note: Some("Problems were reported — see the output.".into()),
        });
    }
    Err(if last_line.is_empty() {
        format!("The command failed with code {code}.")
    } else {
        last_line.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str) -> &'static tasks::Action {
        tasks::find(id).unwrap().1
    }

    #[test]
    fn cards_describe_every_action_and_flag_the_admin_ones() {
        let cards = maintenance_cards();
        assert_eq!(cards.len(), tasks::CARDS.len());
        let by_id = |id: &str| cards.iter().find(|c| c.id == id).unwrap();
        assert!(by_id("net-reset").admin);
        assert!(by_id("ip-renew").admin, "ipconfig /release needs rights");
        assert!(!by_id("flush-dns").admin, "asks only if Windows refuses");
        assert!(!by_id("update-apps").admin, "winget runs unelevated");
        assert_eq!(
            by_id("system-repair").actions.len(),
            2,
            "SFC and DISM share a card"
        );
        assert!(by_id("net-reset").caution.is_some());
    }

    #[test]
    fn events_and_outcomes_serialize_for_the_page() {
        assert_eq!(
            serde_json::to_value(TaskEvent::Line {
                text: "x".into(),
                replace: true
            })
            .unwrap(),
            serde_json::json!({ "event": "line", "data": { "text": "x", "replace": true } })
        );
        assert_eq!(
            serde_json::to_value(TaskEvent::Waiting).unwrap(),
            serde_json::json!({ "event": "waiting" })
        );
        assert_eq!(
            serde_json::to_value(TaskOutcome::Cancelled).unwrap(),
            serde_json::json!({ "result": "cancelled" })
        );
        assert_eq!(
            serde_json::to_value(TaskOutcome::Done { note: None }).unwrap(),
            serde_json::json!({ "result": "done", "note": null })
        );
    }

    #[test]
    fn the_lock_admits_one_task_and_releases_it_on_drop() {
        let running = Running::default();
        let guard = running.claim("sfc", None).expect("first claim wins");
        assert!(running.claim("dism", None).is_none());
        assert_eq!(running.current().as_deref(), Some("sfc"));
        drop(guard);
        assert_eq!(running.current(), None);
        assert!(running.claim("dism", None).is_some());
    }

    #[test]
    fn firmware_restart_reservation_blocks_maintenance_until_drop() {
        let running = Running::default();
        let reservation = running.reserve_for_firmware_restart().unwrap();
        assert_eq!(running.current().as_deref(), Some("restart-to-firmware"));
        assert!(running.claim("sfc", None).is_none());
        assert!(running.reserve_for_firmware_restart().is_err());

        drop(reservation);
        assert!(running.claim("sfc", None).is_some());
    }

    #[test]
    fn cancelling_names_the_task_it_stops() {
        let running = Running::default();
        let _guard = running.claim("sfc", None).unwrap();
        running.cancel("dism");
        assert!(
            !running.is_cancelled(),
            "a stale id must not stop the running task"
        );
        running.cancel("sfc");
        assert!(running.is_cancelled());
    }

    #[test]
    fn cancelling_writes_the_file_the_elevated_half_watches_for() {
        let stop =
            std::env::temp_dir().join(format!("myle-maint-test-{}.stop", std::process::id()));
        let _ = std::fs::remove_file(&stop);
        let running = Running::default();
        let _guard = running.claim("sfc", Some(stop.clone())).unwrap();
        assert!(!stop.exists());
        running.cancel("sfc");
        assert!(stop.exists());
        let _ = std::fs::remove_file(&stop);
    }

    #[test]
    fn a_declined_uac_prompt_is_not_an_error() {
        assert_eq!(
            classify(action("sfc"), ERROR_CANCELLED, ""),
            Ok(TaskOutcome::NeedsAdmin)
        );
    }

    #[test]
    fn failures_are_reported_in_the_tools_own_words() {
        let last = "The service name is invalid.";
        assert_eq!(
            classify(action("bluetooth-fix"), 2, last),
            Err(last.to_string())
        );
        assert_eq!(
            classify(action("bluetooth-fix"), 2, ""),
            Err("The command failed with code 2.".to_string())
        );
    }

    #[test]
    fn check_disk_finding_problems_is_a_result_not_a_failure() {
        let outcome = classify(action("chkdsk"), 2, "Errors found").unwrap();
        assert!(matches!(outcome, TaskOutcome::Done { note: Some(_) }));
    }

    #[test]
    fn check_disk_cannot_claim_success_when_the_scan_did_not_run() {
        assert_eq!(
            classify(action("chkdsk"), 1, "Access is denied."),
            Err("Access is denied.".into())
        );
        assert_eq!(
            classify(action("chkdsk"), 3, "The disk could not be checked."),
            Err("The disk could not be checked.".into())
        );
    }

    #[test]
    fn reboot_is_reported_from_the_table_and_from_dism() {
        assert_eq!(
            classify(action("net-reset"), 0, ""),
            Ok(TaskOutcome::RebootRequired)
        );
        assert_eq!(
            classify(action("dism"), REBOOT_REQUIRED, ""),
            Ok(TaskOutcome::RebootRequired)
        );
        assert_eq!(
            classify(action("sfc"), 0, ""),
            Ok(TaskOutcome::Done { note: None })
        );
    }

    #[test]
    fn nothing_to_upgrade_reads_as_success() {
        let outcome = classify(action("update-apps"), NOTHING_TO_UPGRADE, "").unwrap();
        assert_eq!(
            outcome,
            TaskOutcome::Done {
                note: Some("Everything is already up to date.".into())
            }
        );
    }
}
