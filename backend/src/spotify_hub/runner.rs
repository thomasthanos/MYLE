use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tauri::ipc::Channel;
use tokio::io::AsyncReadExt;

use crate::apps::JobHandle;
use crate::apps::process::{self, hidden};
use crate::console::{Line, LineSplitter};
use crate::download::err;

use super::models::{SpotifyHubEvent, SpotifyHubStage};
use super::state::{Cancellation, SpotifyHubState};

pub struct Reporter<'a> {
    pub job_id: &'a str,
    pub channel: &'a Channel<SpotifyHubEvent>,
    pub state: &'a SpotifyHubState,
}

impl Reporter<'_> {
    pub fn stage(&self, stage: SpotifyHubStage) {
        self.state.update(self.job_id, stage, None);
        let _ = self.channel.send(SpotifyHubEvent::Stage {
            job_id: self.job_id.to_string(),
            stage,
        });
    }

    pub fn progress(&self, fraction: f64) {
        let fraction = fraction.clamp(0.0, 1.0);
        self.state
            .update(self.job_id, self.current_stage(), Some(fraction));
        let _ = self.channel.send(SpotifyHubEvent::Progress {
            job_id: self.job_id.to_string(),
            fraction,
        });
    }

    pub fn line(&self, text: impl Into<String>) {
        self.output(text, false);
    }

    fn output(&self, text: impl Into<String>, replace: bool) {
        let text = text.into();
        if !text.trim().is_empty() {
            let _ = self.channel.send(SpotifyHubEvent::Line {
                job_id: self.job_id.to_string(),
                text,
                replace,
            });
        }
    }

    fn current_stage(&self) -> SpotifyHubStage {
        self.state
            .active()
            .filter(|active| active.job_id == self.job_id)
            .map(|active| active.stage)
            .unwrap_or(SpotifyHubStage::Preparing)
    }
}

pub async fn run_program(
    program: &Path,
    args: &[OsString],
    job: &JobHandle,
    cancellation: &Cancellation,
    reporter: &Reporter<'_>,
) -> Result<i32, String> {
    let mut child = hidden(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(err)?;
    let pid = child.id().ok_or("The child process has no process id.")?;
    job.set_pid(Some(pid));

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<(bool, Vec<u8>)>();
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, false, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, true, sender.clone());
    }
    drop(sender);

    let mut stdout_splitter = LineSplitter::default();
    let mut stderr_splitter = LineSplitter::default();
    let mut stdout_display = StreamDisplay::default();
    let mut stderr_display = StreamDisplay::default();
    let mut feed = |is_stderr: bool, bytes: &[u8]| {
        let splitter = if is_stderr {
            &mut stderr_splitter
        } else {
            &mut stdout_splitter
        };
        for line in splitter.feed(bytes) {
            if is_stderr {
                emit_terminal_line(line, reporter, &mut stderr_display, &mut stdout_display);
            } else {
                emit_terminal_line(line, reporter, &mut stdout_display, &mut stderr_display);
            }
        }
    };

    let mut wait = Box::pin(child.wait());
    // An interval, not a sleep re-armed per loop: with steady output a fresh
    // sleep never fires, and Stop would be ignored until the program went quiet.
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    let mut open = true;
    let mut killed = false;
    let status = loop {
        tokio::select! {
            status = &mut wait => break status.map_err(err)?,
            // Disabled once both pipes close, or `recv` would spin on `None`.
            chunk = receiver.recv(), if open => match chunk {
                Some((is_stderr, bytes)) => feed(is_stderr, &bytes),
                None => open = false,
            },
            _ = ticker.tick() => {
                if !killed && cancellation.is_cancelled() {
                    killed = true;
                    process::kill_tree(pid);
                }
            }
        }
    };
    job.set_pid(None);

    // A program the child started (Spotify relaunched by Spicetify, an
    // uninstaller's helper) can inherit the pipes and keep them open long
    // after the child exits, so the remaining output gets a short grace period.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while open {
        match tokio::time::timeout_at(deadline, receiver.recv()).await {
            Ok(Some((is_stderr, bytes))) => feed(is_stderr, &bytes),
            Ok(None) | Err(_) => open = false,
        }
    }
    if let Some(line) = stdout_splitter.finish() {
        emit_terminal_line(line, reporter, &mut stdout_display, &mut stderr_display);
    }
    if let Some(line) = stderr_splitter.finish() {
        emit_terminal_line(line, reporter, &mut stderr_display, &mut stdout_display);
    }

    if cancellation.is_cancelled() {
        return Err(crate::download::CANCELLED.into());
    }
    Ok(status.code().unwrap_or(-1))
}

#[derive(Debug, PartialEq)]
struct CleanedOutput {
    text: Option<String>,
    progress: Option<f64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct StreamDisplay {
    /// This stream currently owns the final visible row in the shared console.
    row_visible: bool,
    last_text: Option<String>,
}

impl StreamDisplay {
    fn reset(&mut self) {
        self.row_visible = false;
        self.last_text = None;
    }
}

#[derive(Debug, PartialEq)]
struct TerminalUpdate {
    text: Option<(String, bool)>,
    progress: Option<f64>,
}

fn prepare_terminal_update(line: Line, display: &mut StreamDisplay) -> TerminalUpdate {
    // A non-replacement starts a fresh terminal row. If it is later filtered
    // (for example an ANSI-only progress frame), that row must not inherit
    // ownership of an unrelated visible console line.
    if !line.replace {
        display.reset();
    }

    let cleaned = clean_terminal_output(&line.text);
    let text = cleaned.text.and_then(|text| {
        // Suppress only identical redraws of the same provisional row. Normal
        // committed messages such as `OK\nOK\n` must both remain visible.
        if line.replace
            && display.row_visible
            && display.last_text.as_deref() == Some(text.as_str())
        {
            return None;
        }

        let replace = line.replace && display.row_visible;
        display.row_visible = true;
        display.last_text = Some(text.clone());
        Some((text, replace))
    });

    TerminalUpdate {
        text,
        progress: cleaned.progress,
    }
}

fn emit_terminal_line(
    line: Line,
    reporter: &Reporter<'_>,
    display: &mut StreamDisplay,
    other_display: &mut StreamDisplay,
) {
    let update = prepare_terminal_update(line, display);
    if let Some(progress) = update.progress {
        reporter.progress(progress);
    }
    if let Some((text, replace)) = update.text {
        reporter.output(text, replace);
        // stdout and stderr share one frontend list. Once this stream writes,
        // a provisional row from the other stream is no longer the last row
        // and therefore cannot be replaced safely.
        other_display.reset();
    }
}

fn clean_terminal_output(input: &str) -> CleanedOutput {
    let text = strip_terminal_sequences(input).trim().to_string();
    if text.is_empty() {
        return CleanedOutput {
            text: None,
            progress: None,
        };
    }
    let progress = percentage(&text).map(|value| value as f64 / 100.0);
    let text = (!is_decorative_bar(&text)
        && !is_progress_bar_frame(&text)
        && !is_progress_only(&text, progress.is_some()))
    .then_some(text);
    CleanedOutput { text, progress }
}

/// Removes CSI colours/cursor movement, OSC titles/hyperlinks and the other
/// small escape forms used by terminal renderers. This is intentionally local
/// to Spotify Hub: other features still receive the original decoded text.
fn strip_terminal_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\u{1b}' => match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']') => skip_string_escape(&mut chars),
                Some('P' | 'X' | '^' | '_') => skip_string_escape(&mut chars),
                // Two-character escape sequences (save/restore cursor,
                // character-set selection, and similar) carry no text.
                Some(value) => skip_short_escape(value, &mut chars),
                None => {}
            },
            '\u{009b}' => skip_csi(&mut chars),
            '\u{0008}' => {
                output.pop();
            }
            '\t' => output.push(' '),
            value if value.is_control() => {}
            value => output.push(value),
        }
    }
    output
}

fn skip_short_escape(first: char, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    // ISO-2022 style escapes (for example ESC ( B) begin with one or more
    // intermediate bytes followed by a final byte. Single-byte escapes are
    // already complete after `first`.
    if (' '..='/').contains(&first) {
        for value in chars.by_ref() {
            if ('0'..='~').contains(&value) {
                break;
            }
        }
    }
}

fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    for value in chars.by_ref() {
        if ('@'..='~').contains(&value) {
            break;
        }
    }
}

fn skip_string_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut escaped = false;
    for value in chars.by_ref() {
        if value == '\u{0007}' || (escaped && value == '\\') {
            break;
        }
        escaped = value == '\u{1b}';
    }
}

fn percentage(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    for percent in (0..bytes.len()).filter(|index| bytes[*index] == b'%') {
        let mut start = percent;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == percent || percent - start > 3 {
            continue;
        }
        if let Ok(value) = text[start..percent].parse::<u8>()
            && value <= 100
        {
            return Some(value);
        }
    }
    None
}

fn is_decorative_bar(text: &str) -> bool {
    const BAR: &str = "█▓▒░▉▊▋▌▍▎▏▐▀▄▁▂▃▅▆▇■□▪▫▬▭▮▯▰▱─━═|[](){}<>=-_~·•";
    text.chars()
        .all(|value| value.is_whitespace() || BAR.contains(value))
}

fn is_progress_bar_frame(text: &str) -> bool {
    const BLOCKS: &str = "█▓▒░▉▊▋▌▍▎▏▐▀▄▁▂▃▅▆▇■□▪▫▬▭▮▯▰▱";
    text.chars().any(|value| BLOCKS.contains(value)) && has_numeric_ratio(text)
}

fn has_numeric_ratio(text: &str) -> bool {
    let bytes = text.as_bytes();
    for slash in (0..bytes.len()).filter(|index| bytes[*index] == b'/') {
        let mut left = slash;
        while left > 0 && bytes[left - 1].is_ascii_digit() {
            left -= 1;
        }
        let mut right = slash + 1;
        while right < bytes.len() && bytes[right].is_ascii_digit() {
            right += 1;
        }
        if left < slash && right > slash + 1 {
            return true;
        }
    }
    false
}

fn is_progress_only(text: &str, has_percentage: bool) -> bool {
    if !has_percentage {
        return false;
    }
    text.split(|value: char| !value.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .all(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "s" | "ms" | "b" | "kb" | "mb" | "gb" | "eta" | "it"
            )
        })
}

fn spawn_reader<R>(
    mut reader: R,
    stderr: bool,
    sender: tokio::sync::mpsc::UnboundedSender<(bool, Vec<u8>)>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = vec![0u8; 8192];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if sender.send((stderr, buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

pub async fn stop_spotify_processes(reporter: &Reporter<'_>) {
    for image in ["Spotify.exe", "spicetify.exe"] {
        let output = hidden(crate::apps::process::system32("taskkill.exe"))
            .args(["/IM", image, "/T", "/F"])
            .stdin(Stdio::null())
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => reporter.line(format!("Stopped {image}.")),
            // taskkill returns nonzero when the process simply is not running.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_colours_cursor_codes_and_hyperlinks_are_removed() {
        let raw =
            "\u{1b}[90mDim\u{1b}[0m \u{1b}[2K\u{1b}]8;;https://example.com\u{7}link\u{1b}]8;;\u{7}";
        assert_eq!(strip_terminal_sequences(raw), "Dim link");
    }

    #[test]
    fn spicetify_progress_art_is_collapsed_into_numeric_progress() {
        let bar = clean_terminal_output("\u{1b}[90m████████████\u{1b}[0m");
        assert_eq!(
            bar,
            CleanedOutput {
                text: None,
                progress: None
            }
        );

        let progress = clean_terminal_output("\u{1b}[38;2;182;72;0m 28%\u{1b}[0m | 2s\u{1b}[0m");
        assert_eq!(progress.text, None);
        assert_eq!(progress.progress, Some(0.28));

        let message = clean_terminal_output("\u{1b}[1mPatching files\u{1b}[0m");
        assert_eq!(message.text.as_deref(), Some("Patching files"));
        assert_eq!(message.progress, None);
    }

    #[test]
    fn partial_ratio_progress_frame_is_hidden_before_percentage_arrives() {
        let mut display = StreamDisplay::default();
        let partial = prepare_terminal_update(
            Line {
                text: "[28/100] ███████░░░".into(),
                replace: false,
            },
            &mut display,
        );
        assert_eq!(partial.text, None);
        assert_eq!(partial.progress, None);

        let complete = prepare_terminal_update(
            Line {
                text: "[28/100] ███████░░░ 28% | 2s".into(),
                replace: true,
            },
            &mut display,
        );
        assert_eq!(complete.text, None);
        assert_eq!(complete.progress, Some(0.28));
        assert!(!display.row_visible);
    }

    #[test]
    fn meaningful_messages_with_a_percentage_are_kept() {
        let output = clean_terminal_output("Downloaded 75% of the archive");
        assert_eq!(
            output.text.as_deref(),
            Some("Downloaded 75% of the archive")
        );
        assert_eq!(output.progress, Some(0.75));
    }

    #[test]
    fn a_filtered_provisional_row_never_replaces_an_unrelated_message() {
        let mut display = StreamDisplay::default();
        let hidden = prepare_terminal_update(
            Line {
                text: "\u{1b}[90m████\u{1b}[0m".into(),
                replace: false,
            },
            &mut display,
        );
        assert_eq!(hidden.text, None);

        let visible = prepare_terminal_update(
            Line {
                text: "Patching files".into(),
                replace: true,
            },
            &mut display,
        );
        assert_eq!(visible.text, Some(("Patching files".into(), false)));
    }

    #[test]
    fn a_visible_provisional_row_is_safely_replaced() {
        let mut display = StreamDisplay::default();
        let first = prepare_terminal_update(
            Line {
                text: "Preparing".into(),
                replace: false,
            },
            &mut display,
        );
        let second = prepare_terminal_update(
            Line {
                text: "Patching files".into(),
                replace: true,
            },
            &mut display,
        );
        assert_eq!(first.text, Some(("Preparing".into(), false)));
        assert_eq!(second.text, Some(("Patching files".into(), true)));
    }

    #[test]
    fn repeated_committed_messages_are_not_deduplicated() {
        let mut display = StreamDisplay::default();
        for _ in 0..2 {
            let update = prepare_terminal_update(
                Line {
                    text: "OK".into(),
                    replace: false,
                },
                &mut display,
            );
            assert_eq!(update.text, Some(("OK".into(), false)));
        }
    }

    #[test]
    fn another_stream_invalidates_repaint_ownership() {
        let mut stdout = StreamDisplay {
            row_visible: true,
            last_text: Some("stdout progress".into()),
        };
        let stderr = StreamDisplay {
            row_visible: true,
            last_text: Some("stderr message".into()),
        };
        stdout.reset();

        let update = prepare_terminal_update(
            Line {
                text: "stdout done".into(),
                replace: true,
            },
            &mut stdout,
        );
        assert_eq!(update.text, Some(("stdout done".into(), false)));
        assert!(stderr.row_visible);
    }

    #[test]
    fn charset_selection_and_incomplete_escape_sequences_do_not_leak() {
        assert_eq!(strip_terminal_sequences("\u{1b}(Bplain"), "plain");
        assert_eq!(strip_terminal_sequences("\u{1b}[38;2"), "");
    }
}
