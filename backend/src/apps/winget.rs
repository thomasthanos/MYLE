// ! winget CLI integration: output-table parsing, installed/search queries and ! installs with progress, retries and cancellation. ! ! winget has no JSON output for `list`/`search`, but when stdout is piped it ! prints full-width, untruncated tables whose columns start where the header ! words start (measured in display cells), so the tables parse reliably.
use std::process::Stdio;
use std::sync::LazyLock;
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tokio::io::AsyncReadExt;
use unicode_width::UnicodeWidthChar;
use super::jobs::{JobHandle, Jobs};
use super::process::{ERROR_CANCELLED, hidden, run_elevated};
use super::{JobEvent, JobOutcome, Stage};
use crate::console::{Line, LineSplitter};
use crate::download::err;
const COMMON_ARGS: [&str; 2] = ["--accept-source-agreements", "--disable-interactivity"];
// winget exit codes (HRESULTs, as the process exit code).
const DOWNLOAD_FAILED: i32 = 0x8A15_0008_u32 as i32;
const HASH_MISMATCH: i32 = 0x8A15_0011_u32 as i32;
const NO_PACKAGES_FOUND: i32 = 0x8A15_0014_u32 as i32;
const NO_APPLICABLE_UPDATE: i32 = 0x8A15_002B_u32 as i32;
const ALREADY_INSTALLED: i32 = 0x8A15_0061_u32 as i32;
const REBOOT_REQUIRED: i32 = 0x8A15_0109_u32 as i32;
// Installer results that another attempt with other options cannot change.
const PACKAGE_IN_USE: i32 = 0x8A15_0101_u32 as i32;
const INSTALL_IN_PROGRESS: i32 = 0x8A15_0102_u32 as i32;
const DISK_FULL: i32 = 0x8A15_0105_u32 as i32;
const NO_NETWORK: i32 = 0x8A15_0107_u32 as i32;
const CANCELLED_BY_USER: i32 = 0x8A15_010C_u32 as i32;
const BLOCKED_BY_POLICY: i32 = 0x8A15_010F_u32 as i32;
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Newer version offered by winget, if any.
    pub available: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub version: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Install,
    Upgrade,
}
impl Mode {
    fn verb(self) -> &'static str {
        match self {
            Mode::Install => "install",
            Mode::Upgrade => "upgrade",
        }
    }
}
// --------------------------------------------------------------------------- Commands
#[tauri::command]
pub async fn apps_search(query: String) -> Result<Vec<SearchHit>, String> {
    let query = query.trim().trim_start_matches('-');
    if query.chars().count() < 2 || query.len() > 100 {
        return Ok(Vec::new());
    }
    let (code, out) = capture(&[
        "search", "--query", query, "--source", "winget", "--count", "40",
    ])
    .await?;
    if code == NO_PACKAGES_FOUND {
        return Ok(Vec::new());
    }
    Ok(parse_search(&out))
}
/// Where a package lives on the web, for catalog results (which carry no
/// site): the page to open and the domain to take its favicon from.
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageLinks {
    pub homepage: Option<String>,
    pub icon_domain: Option<String>,
}
#[tauri::command]
pub async fn apps_package_links(id: String) -> Result<PackageLinks, String> {
    validate_id(&id)?;
    let (_, out) = capture(&["show", "--id", &id, "--exact", "--source", "winget"]).await?;
    Ok(parse_links(&out))
}
#[tauri::command]
pub async fn apps_install_winget(
    jobs: State<'_, Jobs>,
    id: String,
    mode: Mode,
    on_event: Channel<JobEvent>,
) -> Result<JobOutcome, String> {
    validate_id(&id)?;
    let job = jobs.start(&id)?;
    install_with_retries(&job, &id, mode, &on_event).await
}
/// Only called after the user confirmed it. Runs one elevated script (a single
/// UAC prompt) that temporarily enables winget's `InstallerHashOverride`
/// admin setting, installs with `--ignore-security-hash`, then turns the
/// setting off again.
#[tauri::command]
pub async fn apps_install_ignoring_hash(
    jobs: State<'_, Jobs>,
    id: String,
    mode: Mode,
    on_event: Channel<JobEvent>,
) -> Result<JobOutcome, String> {
    validate_id(&id)?;
    let job = jobs.start(&id)?;
    let _ = on_event.send(JobEvent::Stage {
        stage: Stage::Installing,
    });
    let _ = on_event.send(JobEvent::Note {
        text: "Waiting for administrator approval…".into(),
    });
    // This process cannot stop an elevated one: Cancel leaves this file, and the elevated script stops winget when it sees it.
    let stop = StopFile::new()?;
    let script = hash_override_script(mode.verb(), &id, &stop.path);
    let args: Vec<String> = [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
    ]
    .into_iter()
    .map(String::from)
    .chain([super::process::encode_command(&script)])
    .collect();
    let run = run_elevated("powershell.exe", &args, true);
    tokio::pin!(run);
    let code = loop {
        tokio::select! {
            code = &mut run => break code?,
            () = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                if job.is_cancelled() && !stop.path.exists() {
                    let _ = std::fs::write(&stop.path, b"");
                }
            }
        }
    };
    if code == ERROR_CANCELLED {
        return Err("Administrator approval was declined.".into());
    }
    if job.is_cancelled() {
        return Ok(JobOutcome::Cancelled);
    }
    match classify(code, mode) {
        Classified::Success(note) => Ok(JobOutcome::Done { note }),
        Classified::UpToDate => Ok(JobOutcome::UpToDate),
        Classified::Cancelled => Ok(JobOutcome::Cancelled),
        Classified::HashMismatch => Err(describe(code)),
        Classified::Fatal(msg) | Classified::Retry(msg) => Err(msg),
    }
}
/// The file that asks an elevated script to stop, in a folder of its own that
/// goes when this is dropped.
struct StopFile {
    folder: std::path::PathBuf,
    path: std::path::PathBuf,
}
impl StopFile {
    fn new() -> Result<Self, String> {
        let folder = std::env::temp_dir().join(format!("myle-winget-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&folder).map_err(err)?;
        Ok(Self { path: folder.join("stop"), folder })
    }
}
impl Drop for StopFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.folder);
    }
}
/// Installs with the hash check off, watching `stop`; the setting is turned
/// back off however winget ends, stopped or failed.
fn hash_override_script(verb: &str, id: &str, stop: &std::path::Path) -> String {
    let stop = format!("'{}'", stop.to_string_lossy().replace('\'', "''"));
    format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         $code = 1\n\
         winget settings --enable InstallerHashOverride | Out-Null\n\
         try {{\n\
           $p = Start-Process -FilePath 'winget' -NoNewWindow -PassThru -ArgumentList \
         '{verb} --id {id} --exact --source winget --ignore-security-hash --silent \
         --accept-package-agreements --accept-source-agreements --disable-interactivity'\n\
           if (-not $p) {{ exit $code }}\n\
           $null = $p.Handle\n\
           while (-not $p.HasExited) {{\n\
             if (Test-Path -LiteralPath {stop}) {{ & \"$env:SystemRoot\\System32\\taskkill.exe\" /PID $p.Id /T /F | Out-Null }}\n\
             Start-Sleep -Milliseconds 200\n\
           }}\n\
           $p.WaitForExit()\n\
           $code = $p.ExitCode\n\
         }} finally {{\n\
           winget settings --disable InstallerHashOverride | Out-Null\n\
         }}\n\
         exit $code\n"
    )
}
// --------------------------------------------------------------------------- Queries
pub async fn list_installed() -> Result<Vec<InstalledPackage>, String> {
    let (code, out) = capture(&["list", "--source", "winget"]).await?;
    if code != 0 && code != NO_PACKAGES_FOUND {
        return Err(format!("winget list failed: {}", describe(code)));
    }
    Ok(parse_installed(&out))
}
/// Runs winget hidden and returns (exit code, stdout).
async fn capture(args: &[&str]) -> Result<(i32, String), String> {
    let output = hidden("winget")
        .args(args)
        .args(COMMON_ARGS)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(spawn_error)?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}
fn spawn_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "winget is not available on this PC. Install \"App Installer\" from the Microsoft Store."
            .into()
    } else {
        e.to_string()
    }
}
/// Package IDs are passed as arguments, so only allow plain identifiers.
pub fn validate_id(id: &str) -> Result<(), String> {
    static ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$").unwrap());
    if ID.is_match(id) {
        Ok(())
    } else {
        Err(format!("invalid package id: {id:?}"))
    }
}
// --------------------------------------------------------------------------- Table parsing
#[derive(Debug, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
impl Table {
    /// Column index by English header name, else by position (localized winget).
    fn column(&self, name: &str, position: usize) -> Option<usize> {
        self.headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .or((position < self.headers.len()).then_some(position))
    }
}
/// Parses the first table in winget output: a header line, a line of dashes,
/// then rows. Columns are cut at the display-cell offsets of the header words.
pub fn parse_table(text: &str) -> Option<Table> {
    // A `\r` rewinds the line (spinners, progress), so keep what follows it.
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.rsplit('\r').find(|s| !s.is_empty()).unwrap_or(""))
        .collect();
    let dashes = lines
        .iter()
        .position(|l| l.len() >= 10 && l.trim_end().chars().all(|c| c == '-'))?;
    let header = *lines.get(dashes.checked_sub(1)?)?;
    let mut starts = Vec::new();
    let mut headers = Vec::new();
    let mut col = 0usize;
    let mut prev_space = true;
    for c in header.chars() {
        let is_space = c == ' ';
        if !is_space && prev_space {
            starts.push(col);
            headers.push(String::new());
        }
        if !is_space {
            headers.last_mut()?.push(c);
        }
        prev_space = is_space;
        col += c.width().unwrap_or(0);
    }
    if starts.len() < 2 {
        return None;
    }
    let mut rows = Vec::new();
    for line in &lines[dashes + 1..] {
        if line.trim().is_empty() {
            break; // end of the table
        }
        let mut cells = vec![String::new(); starts.len()];
        let mut col = 0usize;
        for c in line.chars() {
            let index = starts.iter().rposition(|&s| s <= col).unwrap_or(0);
            cells[index].push(c);
            col += c.width().unwrap_or(0);
        }
        rows.push(cells.into_iter().map(|c| c.trim().to_string()).collect());
    }
    Some(Table { headers, rows })
}
pub fn parse_installed(out: &str) -> Vec<InstalledPackage> {
    let Some(table) = parse_table(out) else {
        return Vec::new();
    };
    let (Some(name), Some(id), Some(version)) = (
        table.column("Name", 0),
        table.column("Id", 1),
        table.column("Version", 2),
    ) else {
        return Vec::new();
    };
    let available = table.column("Available", 3);
    let mut seen = std::collections::HashSet::new();
    table
        .rows
        .iter()
        .filter(|row| validate_id(&row[id]).is_ok() && seen.insert(row[id].to_ascii_lowercase()))
        .map(|row| InstalledPackage {
            id: row[id].clone(),
            name: row[name].clone(),
            version: row[version].clone(),
            // Guard against a localized "Source" column landing here.
            available: available
                .map(|a| row[a].clone())
                .filter(|v| v.chars().any(|c| c.is_ascii_digit())),
        })
        .collect()
}
pub fn parse_search(out: &str) -> Vec<SearchHit> {
    let Some(table) = parse_table(out) else {
        return Vec::new();
    };
    let (Some(name), Some(id), Some(version)) = (
        table.column("Name", 0),
        table.column("Id", 1),
        table.column("Version", 2),
    ) else {
        return Vec::new();
    };
    table
        .rows
        .iter()
        .filter(|row| validate_id(&row[id]).is_ok())
        .map(|row| SearchHit {
            id: row[id].clone(),
            name: row[name].clone(),
            version: row[version].clone(),
        })
        .collect()
}
/// Reads `winget show` output. Prefers the manifest's Homepage / Publisher
/// URLs; many manifests have neither, so it falls back to the site root of
/// any other URL (docs, then the installer: a GitHub repo page, if hosted there).
pub fn parse_links(out: &str) -> PackageLinks {
    let urls: Vec<(String, String)> = out
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(": ")?;
            let value = value.trim();
            (value.starts_with("https://") || value.starts_with("http://"))
                .then(|| (key.trim().to_ascii_lowercase(), value.to_string()))
        })
        .collect();
    let field = |name: &str| urls.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    if let Some(url) = field("homepage")
        .or_else(|| field("publisher url"))
        .or_else(|| field("publisher support url"))
    {
        return PackageLinks {
            icon_domain: host(&url),
            homepage: Some(url),
        };
    }
    const NOT_A_SITE: [&str; 5] = [
        "installer url",
        "license url",
        "privacy url",
        "copyright url",
        "release notes url",
    ];
    let other = urls
        .iter()
        .find(|(k, _)| !NOT_A_SITE.contains(&k.as_str()))
        .map(|(_, v)| v.clone());
    let fallback = other.or_else(|| field("installer url"));
    let Some(url) = fallback else {
        return PackageLinks::default();
    };
    let homepage = github_repo(&url).or_else(|| site_domain(&url).map(|d| format!("https://{d}")));
    PackageLinks {
        icon_domain: homepage.as_deref().and_then(host),
        homepage,
    }
}
fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}
/// The registrable domain: "docs.blender.org" -> "blender.org",
/// "shop.example.co.uk" -> "example.co.uk".
fn site_domain(url: &str) -> Option<String> {
    let host = host(url)?;
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return Some(host);
    }
    let country_second_level = labels[n - 1].len() == 2 && labels[n - 2].len() <= 3;
    let keep = if country_second_level { 3 } else { 2 };
    Some(labels[n.saturating_sub(keep)..].join("."))
}
/// "https://github.com/owner/repo/releases/…" -> "https://github.com/owner/repo"
fn github_repo(url: &str) -> Option<String> {
    let path = url.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("https://github.com/{owner}/{repo}"))
}
// --------------------------------------------------------------------------- Installing
struct Attempt {
    silent: bool,
    user_scope: bool,
    note: Option<&'static str>,
}
/// Tried in order until one succeeds.
const ATTEMPTS: [Attempt; 3] = [
    Attempt {
        silent: true,
        user_scope: false,
        note: None,
    },
    Attempt {
        silent: false,
        user_scope: false,
        note: Some("Retrying with the installer's own window…"),
    },
    Attempt {
        silent: true,
        user_scope: true,
        note: Some("Retrying for the current user only…"),
    },
];
fn install_args(id: &str, mode: Mode, attempt: &Attempt) -> Vec<String> {
    let mut args: Vec<String> = [
        mode.verb(),
        "--id",
        id,
        "--exact",
        "--source",
        "winget",
        "--accept-package-agreements",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if attempt.silent {
        args.push("--silent".into());
    }
    if attempt.user_scope {
        args.extend(["--scope".into(), "user".into()]);
    }
    args
}
async fn install_with_retries(
    job: &JobHandle,
    id: &str,
    mode: Mode,
    on_event: &Channel<JobEvent>,
) -> Result<JobOutcome, String> {
    let mut last_error = String::new();
    for attempt in &ATTEMPTS {
        if let Some(note) = attempt.note {
            let _ = on_event.send(JobEvent::Note { text: note.into() });
        }
        let mut parser = ProgressParser::default();
        let code = run_streaming(job, &install_args(id, mode, attempt), &mut |line| {
            for event in parser.feed(&line.text) {
                let _ = on_event.send(event);
            }
        })
        .await?;
        if job.is_cancelled() {
            return Ok(JobOutcome::Cancelled);
        }
        match classify(code, mode) {
            Classified::Success(note) => return Ok(JobOutcome::Done { note }),
            Classified::UpToDate => return Ok(JobOutcome::UpToDate),
            Classified::HashMismatch => return Ok(JobOutcome::HashMismatch),
            Classified::Cancelled => return Ok(JobOutcome::Cancelled),
            Classified::Fatal(msg) => return Err(msg),
            Classified::Retry(msg) => last_error = msg,
        }
    }
    Err(last_error)
}
#[derive(Debug, PartialEq)]
enum Classified {
    Success(Option<String>),
    UpToDate,
    HashMismatch,
    /// The user closed the installer: asking again would be rude.
    Cancelled,
    /// Retrying with other options cannot help.
    Fatal(String),
    Retry(String),
}
fn classify(code: i32, mode: Mode) -> Classified {
    match code {
        0 => Classified::Success(None),
        REBOOT_REQUIRED => Classified::Success(Some("Restart your PC to finish.".into())),
        ALREADY_INSTALLED if mode == Mode::Install => {
            Classified::Success(Some("Already installed.".into()))
        }
        ALREADY_INSTALLED | NO_APPLICABLE_UPDATE => Classified::UpToDate,
        HASH_MISMATCH => Classified::HashMismatch,
        CANCELLED_BY_USER => Classified::Cancelled,
        NO_PACKAGES_FOUND | PACKAGE_IN_USE | INSTALL_IN_PROGRESS | DISK_FULL | NO_NETWORK
        | BLOCKED_BY_POLICY => Classified::Fatal(describe(code)),
        _ => Classified::Retry(describe(code)),
    }
}
fn describe(code: i32) -> String {
    match code {
        DOWNLOAD_FAILED => "Downloading the installer failed.".into(),
        HASH_MISMATCH => "The installer's hash does not match the winget manifest.".into(),
        NO_PACKAGES_FOUND => "The package was not found in winget.".into(),
        PACKAGE_IN_USE => "The app is running. Close it and try again.".into(),
        INSTALL_IN_PROGRESS => {
            "Another installation is running. Wait for it to finish and try again.".into()
        }
        DISK_FULL => "There is not enough free disk space.".into(),
        NO_NETWORK => "The installer could not reach the internet.".into(),
        BLOCKED_BY_POLICY => "The installation is blocked by a policy on this PC.".into(),
        _ => format!("winget failed with code 0x{:08X}.", code as u32),
    }
}
/// Upgrades every package winget can, streaming its output line by line.
/// `--include-unknown` covers packages whose installed version it cannot read.
pub(crate) async fn upgrade_all(
    job: &JobHandle,
    on_line: &mut (dyn FnMut(&Line) + Send),
) -> Result<i32, String> {
    let args: Vec<String> = [
        "upgrade",
        "--all",
        "--include-unknown",
        "--silent",
        "--accept-package-agreements",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    run_streaming(job, &args, on_line).await
}
/// Runs winget, handing each finished line to `on_line` as it appears.
async fn run_streaming(
    job: &JobHandle,
    args: &[String],
    on_line: &mut (dyn FnMut(&Line) + Send),
) -> Result<i32, String> {
    let mut child = hidden("winget")
        .args(args)
        .args(COMMON_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(spawn_error)?;
    job.set_pid(child.id());
    let mut stdout = child
        .stdout
        .take()
        .ok_or("winget produced no output stream")?;
    let mut splitter = LineSplitter::default();
    let mut buf = [0u8; 4096];
    loop {
        let n = stdout.read(&mut buf).await.map_err(err)?;
        if n == 0 {
            break;
        }
        for line in splitter.feed(&buf[..n]) {
            on_line(&line);
        }
    }
    for line in splitter.finish() {
        on_line(&line);
    }
    let status = child.wait().await.map_err(err)?;
    job.set_pid(None);
    Ok(status.code().unwrap_or(-1))
}
/// Turns winget's console output into stage and progress events.
#[derive(Default)]
pub struct ProgressParser {
    stage: Option<Stage>,
    last_fraction: Option<f64>,
}
impl ProgressParser {
    pub fn feed(&mut self, line: &str) -> Vec<JobEvent> {
        let mut events = Vec::new();
        let lower = line.to_ascii_lowercase();
        let stage = if lower.contains("downloading") {
            Some(Stage::Downloading)
        } else if lower.contains("verified installer hash") {
            Some(Stage::Verifying)
        } else if lower.contains("starting package install") || lower.contains("installing") {
            Some(Stage::Installing)
        } else {
            None
        };
        if let Some(stage) = stage.filter(|s| self.stage != Some(*s)) {
            self.stage = Some(stage);
            self.last_fraction = None;
            events.push(JobEvent::Stage { stage });
        }
        if let Some(fraction) = parse_fraction(line) {
            let changed = self.last_fraction.is_none_or(|last| {
                (fraction - last).abs() >= 0.01 || fraction >= 1.0 && last < 1.0
            });
            if changed {
                self.last_fraction = Some(fraction);
                events.push(JobEvent::Progress {
                    fraction,
                    downloaded: None,
                    total: None,
                });
            }
        }
        events
    }
}
/// "12.5 MB / 50.0 MB" or "42%" -> 0.0..=1.0
pub fn parse_fraction(line: &str) -> Option<f64> {
    static SIZES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+(?:\.\d+)?)\s*(B|KB|MB|GB)\s*/\s*(\d+(?:\.\d+)?)\s*(B|KB|MB|GB)").unwrap()
    });
    static PERCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3})\s*%").unwrap());
    fn bytes(value: &str, unit: &str) -> Option<f64> {
        let scale = match unit {
            "B" => 1.0,
            "KB" => 1024.0,
            "MB" => 1024.0 * 1024.0,
            _ => 1024.0 * 1024.0 * 1024.0,
        };
        Some(value.parse::<f64>().ok()? * scale)
    }
    if let Some(c) = SIZES.captures(line) {
        let done = bytes(&c[1], &c[2])?;
        let total = bytes(&c[3], &c[4])?;
        return (total > 0.0).then(|| (done / total).clamp(0.0, 1.0));
    }
    let pct: f64 = PERCENT.captures(line)?[1].parse().ok()?;
    (pct <= 100.0).then_some(pct / 100.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_hash_override_is_always_turned_off_and_winget_can_be_stopped() {
        let script = hash_override_script("install", "Some.App", std::path::Path::new(r"C:\Temp\it's\stop"));
        let enable = script.find("--enable InstallerHashOverride").unwrap();
        let install = script.find("install --id Some.App --exact").unwrap();
        let finally = script.find("} finally {").unwrap();
        let disable = script.find("--disable InstallerHashOverride").unwrap();
        assert!(enable < install && install < finally && finally < disable);
        assert!(script.contains(r"Test-Path -LiteralPath 'C:\Temp\it''s\stop'"));
        assert!(script.contains(r"System32\taskkill.exe"));
        assert!(script.trim_end().ends_with("exit $code"));
        // Parsed by PowerShell itself, never run.
        let parse = "$e = $null; [void][System.Management.Automation.Language.Parser]::ParseInput(\
                     $env:MYLE_SCRIPT, [ref]$null, [ref]$e); exit $e.Count";
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", parse])
            .env("MYLE_SCRIPT", &script)
            .status()
            .unwrap();
        assert!(status.success(), "{script}");
    }
    // Real `winget search --query obs --count 6` output (CJK names are 2 cells wide).
    const SEARCH: &str = "Name           Id                        Version                 Match\r\n\
-----------------------------------------------------------------------------\r\n\
OBS Studio     OBSProject.OBSStudio      32.2.2                  Moniker: obs\r\n\
哔哩哔哩直播姬 Bilibili.Livehime         8.7.0.11081             Tag: obs\r\n\
N Air 実験版   DWANGO.NAIR.experimental  1.1.20260909-unstable.1 Tag: obs\r\n\
DistroAV       DistroAV.DistroAV         6.2.1                   Tag: obs\r\n\
<additional entries truncated due to result limit>\r\n";
    /// `winget list --source winget` layout (rows taken from real output):
    /// a spinner, then every column padded to its widest value.
    fn list_output() -> String {
        let row = |cells: [&str; 4]| {
            format!(
                "{:<31}{:<31}{:<15}{}\r\n",
                cells[0], cells[1], cells[2], cells[3]
            )
        };
        let mut out = String::from("   - \r   \\ \r");
        out += &row(["Name", "Id", "Version", "Available"]);
        out += &format!("{}\r\n", "-".repeat(90));
        out += &row(["7-Zip 26.03 (x64 edition)", "7zip.7zip", "26.03.00.0", ""]);
        out += &row(["AnyDesk", "AnyDesk.AnyDesk", "ad 9.7.15", "9.7.16"]);
        out += &row(["Discord", "Discord.Discord", "1.0.9258", "1.0.9259"]);
        out += &row(["Google Chrome", "Google.Chrome", "< 131.0", "140.0.1"]);
        out += &row([
            "WindowsAppRuntime.2",
            "Microsoft.WindowsAppRuntime.2",
            "2.5.1.0",
            "",
        ]);
        out += &row([
            "WindowsAppRuntime.2",
            "Microsoft.WindowsAppRuntime.2",
            "2.5.1.0",
            "",
        ]);
        out += "\r\n";
        out
    }
    #[test]
    fn search_table_handles_wide_characters_and_footer() {
        let hits = parse_search(SEARCH);
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            SearchHit {
                id: "OBSProject.OBSStudio".into(),
                name: "OBS Studio".into(),
                version: "32.2.2".into()
            }
        );
        assert_eq!(hits[1].id, "Bilibili.Livehime");
        assert_eq!(hits[1].name, "哔哩哔哩直播姬");
        assert_eq!(hits[2].id, "DWANGO.NAIR.experimental");
        assert_eq!(hits[2].version, "1.1.20260909-unstable.1");
    }
    #[test]
    fn installed_table_keeps_versions_with_spaces_and_dedupes() {
        let list = parse_installed(&list_output());
        assert_eq!(list.len(), 5);
        assert_eq!(list[0].available, None);
        assert_eq!(list[1].version, "ad 9.7.15");
        assert_eq!(list[1].available.as_deref(), Some("9.7.16"));
        assert_eq!(list[3].version, "< 131.0");
        assert_eq!(list[3].available.as_deref(), Some("140.0.1"));
        assert_eq!(list[4].id, "Microsoft.WindowsAppRuntime.2");
    }
    #[test]
    fn no_table_means_no_rows() {
        assert!(parse_installed("No installed package found matching input criteria.").is_empty());
        assert!(parse_search("").is_empty());
    }
    #[test]
    fn links_prefer_the_homepage() {
        let show = "Found Bforartists 4 [Bforartists.Bforartists]\nPublisher Url: https://www.bforartists.de/\nHomepage: https://www.bforartists.de/download/\n";
        assert_eq!(
            parse_links(show),
            PackageLinks {
                homepage: Some("https://www.bforartists.de/download/".into()),
                icon_domain: Some("www.bforartists.de".into())
            }
        );
        assert_eq!(
            parse_links("Publisher Url: https://x.org\n")
                .homepage
                .as_deref(),
            Some("https://x.org")
        );
        assert_eq!(
            parse_links("Homepage: javascript:alert(1)\n"),
            PackageLinks::default()
        );
    }
    #[test]
    fn links_fall_back_to_the_site_of_other_urls() {
        // Real `winget show BlenderFoundation.Blender`: no Homepage/Publisher Url.
        let blender = "Publisher: Blender Foundation\nDocumentation:\n  Manual: https://docs.blender.org/manual/en/latest/\n\
Installer:\n  Installer Url: https://download.blender.org/release/Blender5.2/blender-5.2.1-windows-x64.msi\n";
        assert_eq!(
            parse_links(blender),
            PackageLinks {
                homepage: Some("https://blender.org".into()),
                icon_domain: Some("blender.org".into())
            }
        );
        let github = "License Url: https://github.com/o/r/blob/main/LICENSE\nInstaller Url: https://github.com/o/r/releases/download/v1/setup.exe\n";
        assert_eq!(
            parse_links(github).homepage.as_deref(),
            Some("https://github.com/o/r")
        );
        assert_eq!(
            site_domain("https://shop.example.co.uk/x").as_deref(),
            Some("example.co.uk")
        );
        assert_eq!(
            site_domain("https://example.com").as_deref(),
            Some("example.com")
        );
    }
    #[test]
    fn ids_are_validated() {
        assert!(validate_id("Google.Chrome").is_ok());
        assert!(validate_id("Microsoft.VCRedist.2015+.x64").is_ok());
        assert!(validate_id("--silent").is_err());
        assert!(validate_id("a b").is_err());
        assert!(validate_id("x'; rm").is_err());
        assert!(validate_id("").is_err());
    }
    #[test]
    fn progress_fractions() {
        assert_eq!(parse_fraction("  ██████▒▒▒▒  25.0 MB / 100 MB"), Some(0.25));
        assert_eq!(parse_fraction("  512 KB / 1.00 MB"), Some(0.5));
        assert_eq!(parse_fraction("  ███▒▒▒  42%"), Some(0.42));
        assert_eq!(parse_fraction("Starting package install..."), None);
    }
    #[test]
    fn progress_parser_emits_stages_once_and_throttles() {
        let mut p = ProgressParser::default();
        assert_eq!(
            p.feed("Downloading https://example.com/setup.exe"),
            vec![JobEvent::Stage {
                stage: Stage::Downloading
            }]
        );
        assert_eq!(
            p.feed("  1.00 MB / 100 MB"),
            vec![JobEvent::Progress {
                fraction: 0.01,
                downloaded: None,
                total: None
            }]
        );
        assert!(p.feed("  1.00 MB / 100 MB").is_empty());
        assert_eq!(
            p.feed("  100 MB / 100 MB"),
            vec![JobEvent::Progress {
                fraction: 1.0,
                downloaded: None,
                total: None
            }]
        );
        assert_eq!(
            p.feed("Successfully verified installer hash"),
            vec![JobEvent::Stage {
                stage: Stage::Verifying
            }]
        );
        assert_eq!(
            p.feed("Starting package install..."),
            vec![JobEvent::Stage {
                stage: Stage::Installing
            }]
        );
        assert!(p.feed("Successfully installed").is_empty());
    }
    #[test]
    fn exit_codes_are_classified() {
        assert_eq!(classify(0, Mode::Install), Classified::Success(None));
        assert!(matches!(
            classify(REBOOT_REQUIRED, Mode::Install),
            Classified::Success(Some(_))
        ));
        assert!(matches!(
            classify(ALREADY_INSTALLED, Mode::Install),
            Classified::Success(Some(_))
        ));
        assert_eq!(
            classify(ALREADY_INSTALLED, Mode::Upgrade),
            Classified::UpToDate
        );
        assert_eq!(
            classify(NO_APPLICABLE_UPDATE, Mode::Upgrade),
            Classified::UpToDate
        );
        assert_eq!(
            classify(HASH_MISMATCH, Mode::Install),
            Classified::HashMismatch
        );
        assert!(matches!(
            classify(NO_PACKAGES_FOUND, Mode::Install),
            Classified::Fatal(_)
        ));
        assert!(matches!(
            classify(DOWNLOAD_FAILED, Mode::Install),
            Classified::Retry(_)
        ));
        // Closing the installer's window is an answer, not a failure to retry.
        assert_eq!(
            classify(CANCELLED_BY_USER, Mode::Install),
            Classified::Cancelled
        );
        for code in [
            PACKAGE_IN_USE,
            INSTALL_IN_PROGRESS,
            DISK_FULL,
            NO_NETWORK,
            BLOCKED_BY_POLICY,
        ] {
            assert!(matches!(
                classify(code, Mode::Upgrade),
                Classified::Fatal(_)
            ));
            assert!(
                !describe(code).contains("code 0x"),
                "{code:#x} has its own message"
            );
        }
        assert_eq!(describe(-1), "winget failed with code 0xFFFFFFFF.");
    }
    #[test]
    fn install_args_follow_the_attempt() {
        let args = install_args("Git.Git", Mode::Upgrade, &ATTEMPTS[2]);
        assert_eq!(args[0], "upgrade");
        assert!(args.contains(&"--silent".to_string()));
        assert!(args.windows(2).any(|w| w == ["--scope", "user"]));
        assert!(
            !install_args("Git.Git", Mode::Install, &ATTEMPTS[1]).contains(&"--silent".to_string())
        );
    }
}
