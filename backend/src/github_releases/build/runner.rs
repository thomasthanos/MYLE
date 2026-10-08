//! Runs a build command and streams what it prints.
//!
//! The command runs through `cmd.exe /d /s /c` (so `npm run build && …`
//! works as typed) inside a Job Object that kills every process of the job
//! when it closes: Cancel ends npm, node, cargo, rustc and the installer
//! compiler together, not only the shell on top. Output is read from both
//! streams with the console line splitter (progress lines repaint, the OEM
//! code page is decoded), sent to the page in batches, kept in a log file,
//! and read by `parse` for errors, warnings and stages.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncReadExt;

use super::parse::{Diagnostic, Parser, Severity, strip_ansi};
use crate::apps::process::hidden;
use crate::console::LineSplitter;
use crate::github_releases::store::BuildStats;

/// Diagnostics kept for the page at most (the log has them all).
const MAX_DIAGNOSTICS: usize = 1000;
/// How often lines are sent to the page.
const BATCH: Duration = Duration::from_millis(120);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub index: usize,
    pub text: String,
    /// From stderr.
    pub err: bool,
    /// Replaces the line before it (a repainting progress line).
    pub replace: bool,
    pub severity: Option<Severity>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum BuildEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        command: String,
        dir: String,
        log_path: Option<String>,
    },
    Lines {
        lines: Vec<LogLine>,
    },
    Diagnostic {
        diagnostic: Diagnostic,
    },
    Stage {
        text: String,
    },
    /// `estimated`: from the last good build's time and output, not the tool.
    #[serde(rename_all = "camelCase")]
    Progress {
        percent: f32,
        estimated: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildOutcome {
    pub ok: bool,
    pub cancelled: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub lines: u64,
    pub errors: u32,
    pub warnings: u32,
    pub diagnostics: Vec<Diagnostic>,
    pub log_path: Option<String>,
    /// The build could not start (or another failure outside it).
    pub failure: Option<String>,
}

/// A Windows Job Object that kills its processes when closed.
struct JobObject(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a job handle may be used and closed from any thread.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    fn new() -> Option<JobObject> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: plain Win32 calls; the handle is owned by the returned
        // value and closed in Drop; `info` lives across the call.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let job = JobObject(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            (ok != 0).then_some(job)
        }
    }

    fn assign(&self, process: windows_sys::Win32::Foundation::HANDLE) -> bool {
        // SAFETY: both handles are valid for the call.
        unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.0, process) != 0
        }
    }

    fn kill(&self) {
        // SAFETY: the job handle is valid until Drop.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own; the job's processes end with it.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// An estimate of how far a build is, from the last good one: its time and
/// the lines it printed, never past 97 % before the end.
pub(crate) fn estimate(last: &BuildStats, elapsed_ms: u64, lines: u64) -> f32 {
    let by_time = if last.duration_ms > 0 {
        elapsed_ms as f32 / last.duration_ms as f32
    } else {
        0.0
    };
    let by_lines = if last.lines > 0 {
        lines as f32 / last.lines as f32
    } else {
        by_time
    };
    ((by_time * 0.55 + by_lines * 0.45) * 100.0).clamp(0.0, 97.0)
}

pub(crate) struct Request<'a> {
    pub dir: &'a Path,
    pub command: &'a str,
    pub last: Option<BuildStats>,
    pub log_path: Option<PathBuf>,
}

enum Chunk {
    Out(Vec<u8>),
    Err(Vec<u8>),
    OutDone,
    ErrDone,
}

/// Runs `request.command` in `request.dir` until it ends or `cancel` is set.
pub(crate) async fn run(
    request: Request<'_>,
    cancel: Arc<AtomicBool>,
    on_event: impl Fn(BuildEvent) + Send,
) -> BuildOutcome {
    let started = Instant::now();
    let mut outcome = BuildOutcome {
        log_path: request.log_path.as_ref().map(|p| p.display().to_string()),
        ..BuildOutcome::default()
    };
    on_event(BuildEvent::Started {
        command: request.command.to_string(),
        dir: request.dir.display().to_string(),
        log_path: outcome.log_path.clone(),
    });
    let mut log = request.log_path.as_ref().and_then(|path| {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::File::create(path).ok()
    });
    let mut write_log = |text: &str| {
        if let Some(file) = log.as_mut() {
            use std::io::Write;
            let _ = writeln!(file, "{text}");
        }
    };
    write_log(&format!(
        "> {}\n  in {}\n",
        request.command,
        request.dir.display()
    ));

    let mut cmd = hidden("cmd.exe");
    cmd.raw_arg(format!("/d /s /c \"{}\"", request.command))
        .current_dir(request.dir)
        .env("FORCE_COLOR", "0")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TERM_PROGRESS_WHEN", "never")
        .env("npm_config_color", "false")
        .env("npm_config_progress", "false")
        .env("DOTNET_CLI_UI_LANGUAGE", "en")
        .env("VSLANG", "1033")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let job = JobObject::new();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            outcome.failure = Some(format!("The build could not start: {error}"));
            return outcome;
        }
    };
    if let (Some(job), Some(handle)) = (job.as_ref(), child.raw_handle()) {
        job.assign(handle as _);
    }

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Chunk>();
    if let Some(mut stdout) = child.stdout.take() {
        let sender = sender.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 16 * 1024];
            while let Ok(count) = stdout.read(&mut buffer).await {
                if count == 0 || sender.send(Chunk::Out(buffer[..count].to_vec())).is_err() {
                    break;
                }
            }
            let _ = sender.send(Chunk::OutDone);
        });
    }
    if let Some(mut stderr) = child.stderr.take() {
        let sender = sender.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 16 * 1024];
            while let Ok(count) = stderr.read(&mut buffer).await {
                if count == 0 || sender.send(Chunk::Err(buffer[..count].to_vec())).is_err() {
                    break;
                }
            }
            let _ = sender.send(Chunk::ErrDone);
        });
    }
    drop(sender);

    let mut out_split = LineSplitter::default();
    let mut err_split = LineSplitter::default();
    let mut parser = Parser::default();
    let mut batch: Vec<LogLine> = Vec::new();
    let mut last_flush = Instant::now();
    let mut last_progress = Instant::now();
    let mut index = 0usize;
    let mut tool_percent = false;
    // The line of each stream still being drawn: replaced, not appended.
    let mut provisional = [false, false];
    let mut open = 2;

    let mut handle_line = |text: String,
                           err: bool,
                           replace: bool,
                           batch: &mut Vec<LogLine>,
                           index: &mut usize,
                           outcome: &mut BuildOutcome,
                           tool_percent: &mut bool,
                           provisional: &mut [bool; 2]| {
        let text = strip_ansi(&text);
        let slot = usize::from(err);
        let replacing = replace && provisional[slot];
        provisional[slot] = replace;
        let line_index = if replacing {
            index.saturating_sub(1)
        } else {
            *index
        };
        let info = parser.line(&text, line_index);
        let severity = info
            .diagnostics
            .iter()
            .map(|d| d.severity)
            .min_by_key(|s| if *s == Severity::Error { 0 } else { 1 });
        if !replacing {
            *index += 1;
            outcome.lines += 1;
            write_log(&text);
        }
        batch.push(LogLine {
            index: line_index,
            text,
            err,
            replace: replacing,
            severity,
        });
        if let Some(stage) = info.stage {
            on_event(BuildEvent::Stage { text: stage });
        }
        if let Some(percent) = info.percent {
            *tool_percent = true;
            on_event(BuildEvent::Progress {
                percent,
                estimated: false,
            });
        }
        for diagnostic in info.diagnostics {
            record(outcome, &diagnostic);
            on_event(BuildEvent::Diagnostic { diagnostic });
        }
    };

    let mut cancelled = false;
    while open > 0 {
        let chunk = tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await;
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            if let Some(job) = job.as_ref() {
                job.kill();
            }
            let _ = child.kill().await;
            break;
        }
        match chunk {
            Ok(Some(Chunk::Out(bytes))) => {
                for line in out_split.feed(&bytes) {
                    handle_line(
                        line.text,
                        false,
                        line.replace,
                        &mut batch,
                        &mut index,
                        &mut outcome,
                        &mut tool_percent,
                        &mut provisional,
                    );
                }
            }
            Ok(Some(Chunk::Err(bytes))) => {
                for line in err_split.feed(&bytes) {
                    handle_line(
                        line.text,
                        true,
                        line.replace,
                        &mut batch,
                        &mut index,
                        &mut outcome,
                        &mut tool_percent,
                        &mut provisional,
                    );
                }
            }
            Ok(Some(Chunk::OutDone)) => {
                if let Some(line) = out_split.finish() {
                    handle_line(
                        line.text,
                        false,
                        line.replace,
                        &mut batch,
                        &mut index,
                        &mut outcome,
                        &mut tool_percent,
                        &mut provisional,
                    );
                }
                open -= 1;
            }
            Ok(Some(Chunk::ErrDone)) => {
                if let Some(line) = err_split.finish() {
                    handle_line(
                        line.text,
                        true,
                        line.replace,
                        &mut batch,
                        &mut index,
                        &mut outcome,
                        &mut tool_percent,
                        &mut provisional,
                    );
                }
                open -= 1;
            }
            Ok(None) => break,
            Err(_) => {}
        }
        if !batch.is_empty() && last_flush.elapsed() >= BATCH {
            on_event(BuildEvent::Lines {
                lines: std::mem::take(&mut batch),
            });
            last_flush = Instant::now();
        }
        if !tool_percent
            && let Some(last) = request.last.as_ref()
            && last_progress.elapsed() >= Duration::from_millis(500)
        {
            last_progress = Instant::now();
            on_event(BuildEvent::Progress {
                percent: estimate(last, started.elapsed().as_millis() as u64, outcome.lines),
                estimated: true,
            });
        }
    }
    if let Some(diagnostic) = parser.finish() {
        record(&mut outcome, &diagnostic);
        on_event(BuildEvent::Diagnostic { diagnostic });
    }
    if !batch.is_empty() {
        on_event(BuildEvent::Lines { lines: batch });
    }
    let status = if cancelled {
        None
    } else {
        tokio::time::timeout(Duration::from_secs(30), child.wait())
            .await
            .ok()
            .and_then(Result::ok)
    };
    // Whatever the shell left running (a watcher, a stuck child) ends here.
    if let Some(job) = job.as_ref() {
        job.kill();
    }
    outcome.cancelled = cancelled;
    outcome.exit_code = status.and_then(|s| s.code());
    outcome.ok = !cancelled && outcome.exit_code == Some(0);
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    write_log(&format!(
        "\n{} after {:.1} s (exit code {})",
        if cancelled {
            "Cancelled"
        } else if outcome.ok {
            "Finished"
        } else {
            "Failed"
        },
        outcome.duration_ms as f64 / 1000.0,
        outcome
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    outcome
}

fn record(outcome: &mut BuildOutcome, diagnostic: &Diagnostic) {
    match diagnostic.severity {
        Severity::Error => outcome.errors += 1,
        Severity::Warning => outcome.warnings += 1,
    }
    if outcome.diagnostics.len() < MAX_DIAGNOSTICS {
        outcome.diagnostics.push(diagnostic.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_blends_time_and_lines_and_never_claims_done() {
        let last = BuildStats {
            command: "x".into(),
            duration_ms: 100_000,
            lines: 1000,
            at: 0,
        };
        assert_eq!(estimate(&last, 0, 0), 0.0);
        let half = estimate(&last, 50_000, 500);
        assert!((half - 50.0).abs() < 0.01);
        assert_eq!(estimate(&last, 500_000, 9000), 97.0);
    }

    #[test]
    fn a_command_runs_and_its_output_is_read() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let dir = std::env::temp_dir();
        let events = std::sync::Mutex::new(Vec::new());
        let outcome = runtime.block_on(run(
            Request {
                dir: &dir,
                command: "echo hello&& echo error: broken 1>&2 && exit /b 3",
                last: None,
                log_path: None,
            },
            Arc::new(AtomicBool::new(false)),
            |event| events.lock().unwrap().push(event),
        ));
        assert!(!outcome.ok);
        assert_eq!(outcome.exit_code, Some(3));
        assert_eq!(outcome.errors, 1);
        let events = events.into_inner().unwrap();
        let lines: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                BuildEvent::Lines { lines } => Some(lines.clone()),
                _ => None,
            })
            .flatten()
            .map(|l| l.text.trim().to_string())
            .collect();
        assert!(lines.contains(&"hello".to_string()), "{lines:?}");
    }
}
