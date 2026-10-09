//! Runs one maintenance action and reports what it printed.
//!
//! Live output from an elevated program is the awkward part. `run_elevated`
//! only hands back an exit code, and `Start-Process -RedirectStandardOutput`
//! buffers through a .NET writer that never flushes until the process ends —
//! a ten-minute `sfc` scan would show nothing and then everything. So the
//! redirect is done by `cmd.exe` instead: the log file handle *is* the tool's
//! stdout, every write lands immediately, and cmd opens it shareable, so this
//! process can read the file while the tool is still writing to it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use super::tasks::{Step, system32};
use crate::apps::process::{encode_command, hidden, run_elevated};
use crate::console::{Line, LineSplitter};
use crate::download::err;

const POLL: Duration = Duration::from_millis(180);
/// How long to keep draining the log after the program exited, in case the
/// last write lands just after the process does.
const FINAL_DRAINS: usize = 6;

/// Where a run is told about a program starting, as opposed to UAC still being
/// on screen with nothing running yet.
pub enum Phase {
    Waiting,
    Started,
}

/// The temporary files of one run, in a folder of its own that is removed
/// with them when this is dropped.
pub struct Workspace {
    dir: PathBuf,
    pub log: PathBuf,
    pub stop: PathBuf,
}

impl Workspace {
    /// The folder is named at random and made here, never reused: nothing can
    /// be waiting in its place (such as a link planted to steer where the
    /// elevated program writes its output).
    pub fn new(action: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "myle-maint-{action}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir).map_err(err)?;
        Ok(Workspace {
            log: dir.join("output.log"),
            stop: dir.join("stop"),
            dir,
        })
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.log);
        let _ = std::fs::remove_file(&self.stop);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// Running

/// Runs the steps in order without elevation, reporting what they printed.
pub async fn run_plain(
    steps: &[Step],
    on_line: &mut (dyn FnMut(Line) + Send),
) -> Result<i32, String> {
    for step in steps {
        let output = hidden(system32(step.exe))
            .args(step.args.split_whitespace())
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(err)?;
        emit_all(&output.stdout, on_line);
        emit_all(&output.stderr, on_line);
        let code = output.status.code().unwrap_or(-1);
        if code != 0 && !step.allow_failure {
            return Ok(code);
        }
    }
    Ok(0)
}

/// Runs the steps behind a single UAC prompt, then reports everything they
/// printed. Used for the short actions, where live output buys nothing.
pub async fn run_batch_elevated(
    steps: &[Step],
    workspace: &Workspace,
    on_line: &mut (dyn FnMut(Line) + Send),
) -> Result<i32, String> {
    let code = elevate(batch_script(steps, &workspace.log)).await?;
    if let Ok(bytes) = std::fs::read(&workspace.log) {
        emit_all(&bytes, on_line);
    }
    Ok(code)
}

/// Runs one program behind a UAC prompt, streaming its output as it appears.
pub async fn run_stream_elevated(
    step: &Step,
    workspace: &Workspace,
    on_phase: &mut (dyn FnMut(Phase) + Send),
    on_line: &mut (dyn FnMut(Line) + Send),
) -> Result<i32, String> {
    stream(step, workspace, true, on_phase, on_line).await
}

/// The same watch loop with or without the UAC prompt. Only the tests run it
/// unelevated, which is how the redirect and the tailing are exercised without
/// asking for rights or touching the system.
async fn stream(
    step: &Step,
    workspace: &Workspace,
    with_uac: bool,
    on_phase: &mut (dyn FnMut(Phase) + Send),
    on_line: &mut (dyn FnMut(Line) + Send),
) -> Result<i32, String> {
    let script = stream_script(step, &workspace.log, &workspace.stop);

    let mut tail = Tail::new(&workspace.log);
    on_phase(Phase::Waiting);
    let mut started = false;

    let mut launched: BoxFuture = if with_uac {
        Box::pin(elevate(script))
    } else {
        Box::pin(run_hidden(script))
    };
    let code = loop {
        tokio::select! {
            code = &mut launched => break code?,
            _ = tokio::time::sleep(POLL) => {
                if tail.drain(on_line).await && !started {
                    started = true;
                    on_phase(Phase::Started);
                }
            }
        }
    };

    // Whatever the tool wrote just before exiting.
    for _ in 0..FINAL_DRAINS {
        tail.drain(on_line).await;
        tokio::time::sleep(POLL).await;
    }
    tail.finish(on_line);
    Ok(code)
}

type BoxFuture = std::pin::Pin<Box<dyn Future<Output = Result<i32, String>> + Send>>;

fn powershell_args(script: &str) -> Vec<String> {
    [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
    ]
    .into_iter()
    .map(String::from)
    .chain([encode_command(script)])
    .collect()
}

async fn elevate(script: String) -> Result<i32, String> {
    run_elevated(&crate::apps::process::powershell().to_string_lossy(), &powershell_args(&script), true).await
}

async fn run_hidden(script: String) -> Result<i32, String> {
    let status = hidden(crate::apps::process::powershell())
        .args(powershell_args(&script))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(err)?;
    Ok(status.code().unwrap_or(-1))
}

// ---------------------------------------------------------------------------
// Reading the log as it grows

struct Tail {
    path: PathBuf,
    file: Option<tokio::fs::File>,
    splitter: LineSplitter,
    buf: [u8; 8192],
}

impl Tail {
    fn new(path: &Path) -> Self {
        Tail {
            path: path.to_path_buf(),
            file: None,
            splitter: LineSplitter::default(),
            buf: [0; 8192],
        }
    }

    /// Emits everything written since the last call. Returns true once the log
    /// exists, which is the first moment the program is known to be running.
    async fn drain(&mut self, on_line: &mut (dyn FnMut(Line) + Send)) -> bool {
        if self.file.is_none() {
            // Missing until UAC is answered; briefly unreadable if another
            // handle is in the way. Both are worth retrying, never fatal.
            self.file = tokio::fs::File::open(&self.path).await.ok();
        }
        let Some(file) = self.file.as_mut() else {
            return false;
        };
        loop {
            match file.read(&mut self.buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for line in self.splitter.feed(&self.buf[..n]) {
                        on_line(line);
                    }
                }
            }
        }
        true
    }

    fn finish(&mut self, on_line: &mut (dyn FnMut(Line) + Send)) {
        if let Some(line) = self.splitter.finish() {
            on_line(line);
        }
    }
}

fn emit_all(bytes: &[u8], on_line: &mut (dyn FnMut(Line) + Send)) {
    let mut splitter = LineSplitter::default();
    for line in splitter.feed(bytes) {
        on_line(line);
    }
    if let Some(line) = splitter.finish() {
        on_line(line);
    }
}

// ---------------------------------------------------------------------------
// Script generation

/// The `cmd.exe` argument tail that runs one step and appends its output to
/// `log`. `/d` skips AutoRun scripts; `/s` makes cmd strip only the outer pair
/// of quotes and take the rest verbatim, which is the one quoting form that
/// behaves the same on every Windows version.
fn cmd_tail(exe: &Path, args: &str, log: &Path) -> String {
    let mut command = format!("\"{}\"", exe.display());
    if !args.is_empty() {
        command.push(' ');
        command.push_str(args);
    }
    format!("/d /s /c \"{command} >> \"{}\" 2>&1\"", log.display())
}

fn start_process(tail: &str, extra: &str) -> String {
    format!(
        "$p = Start-Process -FilePath {} -ArgumentList {} -NoNewWindow -PassThru{extra}\n",
        ps_quote(&system32("cmd.exe").to_string_lossy()),
        ps_quote(tail)
    )
}

/// The start of every script: runs nothing unless the run's folder is still
/// the plain, empty folder `Workspace::new` made (a link swapped in for it
/// would steer the elevated writes elsewhere).
fn script_start(log: &Path) -> String {
    let dir = log.parent().unwrap_or(log);
    format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $d = Get-Item -LiteralPath {} -Force\n\
         if (-not $d -or -not $d.PSIsContainer -or ($d.Attributes -band [IO.FileAttributes]::ReparsePoint) -or \
         (Test-Path -LiteralPath {})) {{ exit 5 }}\n",
        ps_quote(&dir.to_string_lossy()),
        ps_quote(&log.to_string_lossy()),
    )
}

/// Every step in order; the first hard failure ends the script with its code.
fn batch_script(steps: &[Step], log: &Path) -> String {
    let mut script = script_start(log);
    for step in steps {
        script.push_str(&start_process(
            &cmd_tail(&system32(step.exe), step.args, log),
            " -Wait",
        ));
        if !step.allow_failure {
            script.push_str("if ($p.ExitCode -ne 0) { exit $p.ExitCode }\n");
        }
    }
    script.push_str("exit 0\n");
    script
}

/// One program, watched so it can be stopped: this process cannot kill an
/// elevated child itself, so it drops a sentinel file and the elevated script
/// does the killing. Cancelling that way needs no second UAC prompt.
fn stream_script(step: &Step, log: &Path, stop: &Path) -> String {
    let mut script = script_start(log);
    script.push_str(&start_process(
        &cmd_tail(&system32(step.exe), step.args, log),
        "",
    ));
    script.push_str(&format!(
        "while (-not $p.HasExited) {{\n  \
           if (Test-Path -LiteralPath {}) {{ & {} /PID $p.Id /T /F | Out-Null; break }}\n  \
           Start-Sleep -Milliseconds 200\n\
         }}\n",
        ps_quote(&stop.to_string_lossy()),
        ps_quote(&system32("taskkill.exe").to_string_lossy()),
    ));
    script.push_str("$p.WaitForExit()\nexit $p.ExitCode\n");
    script
}

/// A PowerShell single-quoted string literal.
fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maintenance::tasks;

    #[test]
    fn the_cmd_tail_quotes_the_program_and_the_log_but_not_the_arguments() {
        let tail = cmd_tail(
            Path::new(r"C:\Windows\System32\sfc.exe"),
            "/scannow",
            Path::new(r"C:\Temp\out.log"),
        );
        assert_eq!(
            tail,
            r#"/d /s /c ""C:\Windows\System32\sfc.exe" /scannow >> "C:\Temp\out.log" 2>&1""#
        );
    }

    #[test]
    fn a_step_without_arguments_has_no_stray_space() {
        let tail = cmd_tail(Path::new(r"C:\x\a.exe"), "", Path::new(r"C:\t.log"));
        assert_eq!(tail, r#"/d /s /c ""C:\x\a.exe" >> "C:\t.log" 2>&1""#);
    }

    #[test]
    fn the_batch_script_keeps_the_steps_in_order_and_stops_at_the_first_failure() {
        let (_, action) = tasks::find("net-reset").unwrap();
        let tasks::Run::Batch(steps) = &action.run else {
            panic!("net-reset is a batch")
        };
        let script = batch_script(steps, Path::new(r"C:\Temp\out.log"));

        let winsock = script.find("winsock reset").expect("winsock step");
        let ip = script.find("int ip reset").expect("ip step");
        let ipv6 = script.find("int ipv6 reset").expect("ipv6 step");
        assert!(winsock < ip && ip < ipv6, "steps must run in table order");
        // Two hard steps guard their exit code; the optional third does not.
        assert_eq!(
            script
                .matches("if ($p.ExitCode -ne 0) { exit $p.ExitCode }")
                .count(),
            2
        );
        assert!(script.ends_with("exit 0\n"));
    }

    #[test]
    fn the_stream_script_can_be_stopped_and_reports_the_real_exit_code() {
        let (_, action) = tasks::find("sfc").unwrap();
        let tasks::Run::Stream(step) = &action.run else {
            panic!("sfc streams")
        };
        let script = stream_script(
            step,
            Path::new(r"C:\Temp\out.log"),
            Path::new(r"C:\Temp\out.stop"),
        );

        assert!(script.contains(r"'C:\Temp\out.stop'"));
        assert!(script.contains("Test-Path"));
        assert!(script.contains("taskkill.exe"));
        assert!(script.contains("$p.WaitForExit()"));
        assert!(script.contains("exit $p.ExitCode"));
        assert!(!script.contains("-Wait"), "it must poll, not block");
    }

    /// The whole point of the fixed table: nothing else can reach the shell.
    #[test]
    fn scripts_only_ever_name_programs_from_the_table() {
        let known: Vec<String> = tasks::CARDS
            .iter()
            .flat_map(|card| card.actions.iter())
            .flat_map(|action| match &action.run {
                tasks::Run::Batch(steps) => steps.iter().collect::<Vec<_>>(),
                tasks::Run::Stream(step) => vec![step],
                tasks::Run::WingetUpgradeAll => Vec::new(),
            })
            .map(|step| system32(step.exe).to_string_lossy().to_ascii_lowercase())
            .chain([
                system32("cmd.exe").to_string_lossy().to_ascii_lowercase(),
                system32("taskkill.exe")
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            ])
            .collect();

        for card in tasks::CARDS {
            for action in card.actions {
                let script = match &action.run {
                    tasks::Run::Batch(steps) => batch_script(steps, Path::new(r"C:\Temp\o.log")),
                    tasks::Run::Stream(step) => stream_script(
                        step,
                        Path::new(r"C:\Temp\o.log"),
                        Path::new(r"C:\Temp\o.stop"),
                    ),
                    tasks::Run::WingetUpgradeAll => continue,
                };
                for exe in script.match_indices(".exe").map(|(at, _)| at) {
                    let start = script[..exe].rfind(['\'', '"']).unwrap() + 1;
                    let named = script[start..exe + 4].to_ascii_lowercase();
                    assert!(
                        known.contains(&named),
                        "unexpected program in {}: {named}",
                        action.id
                    );
                }
            }
        }
    }

    #[test]
    fn quotes_are_escaped() {
        assert_eq!(ps_quote(r"C:\Users\it's\t.log"), r"'C:\Users\it''s\t.log'");
    }

    /// The end-to-end proof that the redirect works. `sfc` talks UTF-16LE and
    /// is one of the real streaming tools, and asking it to run without rights
    /// changes nothing, so this exercises the real script, the real `cmd`
    /// redirect and the real decoding without a prompt.
    #[tokio::test]
    async fn a_tools_output_reaches_us_through_the_redirect() {
        let step = Step {
            exe: "sfc.exe",
            args: "/?",
            allow_failure: true,
        };
        let workspace = Workspace::new("selftest-output").unwrap();
        let mut lines: Vec<String> = Vec::new();
        let mut phases = 0;
        stream(
            &step,
            &workspace,
            false,
            &mut |_| phases += 1,
            &mut |line| lines.push(line.text),
        )
        .await
        .expect("the script runs");

        assert!(phases >= 2, "it reports waiting and then started");
        // Asserted on shape, not on words: the message is localized.
        let raw = std::fs::read(&workspace.log).expect("the log was written");
        assert!(
            raw.contains(&0),
            "sfc writes UTF-16LE, so the raw log holds NUL bytes"
        );
        let text = lines.join("\n");
        assert!(
            !text.contains('\u{0}'),
            "…which must not survive decoding: {text:?}"
        );
        assert!(
            lines.len() >= 2,
            "the output is split into lines: {lines:?}"
        );
        assert!(
            text.chars().any(|c| c.is_ascii_alphabetic()),
            "got: {text:?}"
        );
    }

    /// Cancellation goes through the sentinel file, because this process is not
    /// allowed to kill an elevated child.
    #[tokio::test]
    async fn the_stop_file_ends_a_running_program() {
        // ping is the shortest harmless way to hold a process open.
        let step = Step {
            exe: "ping.exe",
            args: "127.0.0.1 -n 30",
            allow_failure: true,
        };
        let workspace = Workspace::new("selftest-stop").unwrap();
        let stop = workspace.stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let _ = std::fs::write(&stop, b"");
        });

        let started = std::time::Instant::now();
        stream(&step, &workspace, false, &mut |_| {}, &mut |_| {})
            .await
            .expect("the script runs");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "a 30-second ping must be stopped early, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_workspace_cleans_up_after_itself() {
        let workspace = Workspace::new("test").unwrap();
        std::fs::write(&workspace.log, b"x").unwrap();
        std::fs::write(&workspace.stop, b"").unwrap();
        let (dir, log, stop) = (workspace.dir.clone(), workspace.log.clone(), workspace.stop.clone());
        drop(workspace);
        assert!(!log.exists() && !stop.exists() && !dir.exists());
    }

    #[test]
    fn every_run_gets_a_new_folder_of_its_own() {
        let first = Workspace::new("test").unwrap();
        let second = Workspace::new("test").unwrap();
        assert_ne!(first.dir, second.dir);
        assert_eq!(first.log.parent(), Some(first.dir.as_path()));
        assert_eq!(first.stop.parent(), Some(first.dir.as_path()));
    }

    /// A log already there, or a folder that is gone (or was swapped for a
    /// link), stops the script before it runs anything.
    #[tokio::test]
    async fn nothing_runs_where_the_folder_is_not_ours() {
        let step = Step {
            exe: "sfc.exe",
            args: "/?",
            allow_failure: true,
        };
        let workspace = Workspace::new("selftest-check").unwrap();
        std::fs::write(&workspace.log, b"planted").unwrap();
        let code = stream(&step, &workspace, false, &mut |_| {}, &mut |_| {}).await.unwrap();
        assert_eq!(code, 5);
        assert_eq!(std::fs::read(&workspace.log).unwrap(), b"planted");
    }
}
