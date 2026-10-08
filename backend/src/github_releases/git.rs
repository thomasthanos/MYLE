//! Running git and reading what it says.
//!
//! git runs hidden, in English (`LC_ALL=C`, so its messages can be told
//! apart), never asks on a terminal (`GIT_TERMINAL_PROMPT=0`) and never
//! colors. The GitHub token, when one is needed for a push, goes in through
//! the environment as an HTTP header for github.com only: never on the
//! command line (visible in process lists) and never into `.git/config`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tokio::io::AsyncReadExt;

use super::{CANCELLED, Problem};
use crate::apps::process::hidden;
use crate::console::LineSplitter;

// ─── Finding the programs ──────────────────────────────────────────────────

/// `name` (`git.exe`) on the PATH, or in one of `fallbacks` (full paths).
pub(crate) fn find_program(name: &str, fallbacks: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    fallbacks.iter().find(|path| path.is_file()).cloned()
}

fn env_dir(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable).map(PathBuf::from)
}

/// git for Windows, also when it was installed after MYLE started (the
/// PATH MYLE got then doesn't have it yet).
pub(crate) fn git_exe() -> Option<PathBuf> {
    let mut fallbacks = Vec::new();
    for base in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = env_dir(base) {
            fallbacks.push(dir.join(r"Git\cmd\git.exe"));
        }
    }
    if let Some(dir) = env_dir("LOCALAPPDATA") {
        fallbacks.push(dir.join(r"Programs\Git\cmd\git.exe"));
    }
    find_program("git.exe", &fallbacks)
}

/// The GitHub CLI.
pub(crate) fn gh_exe() -> Option<PathBuf> {
    let mut fallbacks = Vec::new();
    for base in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = env_dir(base) {
            fallbacks.push(dir.join(r"GitHub CLI\gh.exe"));
        }
    }
    if let Some(dir) = env_dir("LOCALAPPDATA") {
        fallbacks.push(dir.join(r"Programs\GitHub CLI\gh.exe"));
        fallbacks.push(dir.join(r"Microsoft\WinGet\Links\gh.exe"));
    }
    find_program("gh.exe", &fallbacks)
}

pub(crate) fn require_git() -> Result<PathBuf, String> {
    git_exe().ok_or_else(|| {
        Problem::new(
            "NO_GIT",
            "Git is not installed on this PC. Install Git for Windows, then try again.",
        )
        .into()
    })
}

// ─── Running ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub(crate) struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub(crate) fn ok(&self) -> bool {
        self.code == 0
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// stderr then stdout, for "Show details".
    pub(crate) fn all(&self) -> String {
        let stdout = self.text();
        match (self.stderr.trim(), stdout.trim()) {
            ("", out) => out.to_string(),
            (err, "") => err.to_string(),
            (err, out) => format!("{out}\n{err}"),
        }
    }
}

/// Extra environment for one git run.
#[derive(Clone, Default)]
pub(crate) struct GitEnv {
    /// Lets this run authenticate to github.com with the token.
    pub token: Option<zeroize::Zeroizing<String>>,
    /// No sign-in window from Git Credential Manager (background fetches).
    pub quiet: bool,
}

/// The environment that hands git the token for github.com over HTTPS,
/// through `GIT_CONFIG_*` (git 2.31+): an `Authorization` header like the
/// one actions/checkout uses.
pub(crate) fn token_env(token: &str) -> Vec<(String, String)> {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        (
            "GIT_CONFIG_KEY_0".into(),
            "http.https://github.com/.extraheader".into(),
        ),
        (
            "GIT_CONFIG_VALUE_0".into(),
            format!("AUTHORIZATION: basic {basic}"),
        ),
    ]
}

fn command(git: &Path, repo: &Path, args: &[&str], env: &GitEnv) -> tokio::process::Command {
    let mut cmd = hidden(git);
    cmd.arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.quotepath=false",
            "-c",
            "color.ui=false",
            "-c",
            "advice.detachedHead=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = env.token.as_deref() {
        for (key, value) in token_env(token) {
            cmd.env(key, value);
        }
    }
    if env.quiet {
        cmd.env("GCM_INTERACTIVE", "never");
    }
    cmd
}

/// Runs `git -C repo args`, with `input` on stdin, up to `limit`.
pub(crate) async fn run_with(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    env: &GitEnv,
    limit: Duration,
) -> Result<Output, String> {
    let git = require_git()?;
    let mut cmd = command(&git, repo, args, env);
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Can't start git: {error}"))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        use tokio::io::AsyncWriteExt;
        let bytes = bytes.to_vec();
        tokio::spawn(async move {
            let _ = stdin.write_all(&bytes).await;
            let _ = stdin.shutdown().await;
        });
    }
    match tokio::time::timeout(limit, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(Output {
            code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
        Ok(Err(error)) => Err(format!("git failed: {error}")),
        Err(_) => Err(format!(
            "git {} took longer than {} seconds and was stopped.",
            args.first().unwrap_or(&""),
            limit.as_secs()
        )),
    }
}

pub(crate) async fn run(repo: &Path, args: &[&str]) -> Result<Output, String> {
    run_with(
        repo,
        args,
        None,
        &GitEnv::default(),
        Duration::from_secs(60),
    )
    .await
}

/// stdout of a git run that must succeed.
pub(crate) async fn read(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = run(repo, args).await?;
    if output.ok() {
        Ok(output.text())
    } else {
        Err(first_error(&output.all()))
    }
}

/// Runs a network command (push, pull, fetch), sending each line git prints
/// to `on_line` (progress lines repaint: `replace`), until it ends or
/// `cancel` is set.
pub(crate) async fn stream(
    repo: &Path,
    args: &[&str],
    env: &GitEnv,
    cancel: Arc<AtomicBool>,
    mut on_line: impl FnMut(&str, bool) + Send,
) -> Result<Output, String> {
    let git = require_git()?;
    let mut cmd = command(&git, repo, args, env);
    cmd.stdin(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Can't start git: {error}"))?;
    let mut stdout = child.stdout.take().ok_or("git has no output")?;
    let mut stderr = child.stderr.take().ok_or("git has no output")?;
    let out_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes).await;
        bytes
    });
    let mut splitter = LineSplitter::default();
    let mut err_text = String::new();
    let mut buffer = vec![0u8; 8192];
    let mut last_replace = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Err(CANCELLED.into());
        }
        let read = tokio::time::timeout(Duration::from_millis(250), stderr.read(&mut buffer)).await;
        match read {
            Err(_) => continue,
            Ok(Ok(0)) => break,
            Ok(Ok(count)) => {
                for line in splitter.feed(&buffer[..count]) {
                    on_line(&line.text, line.replace);
                    if !line.replace || !last_replace {
                        err_text.push_str(&line.text);
                        err_text.push('\n');
                    }
                    last_replace = line.replace;
                }
            }
            Ok(Err(error)) => return Err(format!("git failed: {error}")),
        }
    }
    if let Some(line) = splitter.finish() {
        on_line(&line.text, line.replace);
        err_text.push_str(&line.text);
    }
    let status = child
        .wait()
        .await
        .map_err(|error| format!("git failed: {error}"))?;
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout: out_task.await.unwrap_or_default(),
        stderr: err_text,
    })
}

/// The line of git's output that tells what went wrong.
pub(crate) fn first_error(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
        .or(lines.last())
        .map(|line| {
            line.trim_start_matches("fatal: ")
                .trim_start_matches("error: ")
                .to_string()
        })
        .unwrap_or_else(|| "git failed.".into())
}

// ─── Status ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

/// How much of a file's change is staged for the next commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Staged {
    All,
    Partial,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// From the repository's top folder, with `/`.
    pub path: String,
    /// The old name of a renamed or copied file.
    pub orig_path: Option<String>,
    pub kind: ChangeKind,
    pub staged: Staged,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    /// `None` when detached.
    pub branch: Option<String>,
    /// The commit checked out; `None` before the first commit.
    pub head: Option<String>,
    /// `origin/main`.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub branch: BranchInfo,
    pub changes: Vec<FileChange>,
}

fn kind_of(code: char) -> ChangeKind {
    match code {
        'A' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'T' => ChangeKind::TypeChanged,
        _ => ChangeKind::Modified,
    }
}

/// Reads `git status --porcelain=v2 --branch -z --untracked-files=all`.
pub(crate) fn parse_status(raw: &[u8]) -> Status {
    let text = String::from_utf8_lossy(raw);
    let mut records = text.split('\0').filter(|r| !r.is_empty());
    let mut status = Status::default();
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => {
                    status.branch.head = Some(value.to_string())
                }
                "branch.head" if value != "(detached)" => {
                    status.branch.branch = Some(value.to_string())
                }
                "branch.upstream" => status.branch.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            status.branch.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.branch.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut chars = record.chars();
        let tag = chars.next().unwrap_or(' ');
        match tag {
            '1' | '2' => {
                // `1 XY sub mH mI mW hH hI path`; `2` adds `Xscore` and the
                // original path comes as the next record.
                let fields = if tag == '1' { 9 } else { 10 };
                let parts: Vec<&str> = record.splitn(fields, ' ').collect();
                if parts.len() < fields {
                    continue;
                }
                let xy: Vec<char> = parts[1].chars().collect();
                let (x, y) = (
                    xy.first().copied().unwrap_or('.'),
                    xy.get(1).copied().unwrap_or('.'),
                );
                let path = parts[fields - 1].to_string();
                let orig_path = (tag == '2')
                    .then(|| records.next().map(str::to_string))
                    .flatten();
                let staged = match (x != '.', y != '.') {
                    (true, false) => Staged::All,
                    (true, true) => Staged::Partial,
                    _ => Staged::None,
                };
                let kind = if x != '.' { kind_of(x) } else { kind_of(y) };
                // Added to the index, then deleted from the folder: a
                // deletion is what is left to see.
                let kind = if x == 'A' && y == 'D' {
                    ChangeKind::Deleted
                } else {
                    kind
                };
                status.changes.push(FileChange {
                    path,
                    orig_path,
                    kind,
                    staged,
                });
            }
            'u' => {
                let parts: Vec<&str> = record.splitn(11, ' ').collect();
                if let Some(path) = parts.get(10) {
                    status.changes.push(FileChange {
                        path: path.to_string(),
                        orig_path: None,
                        kind: ChangeKind::Conflicted,
                        staged: Staged::None,
                    });
                }
            }
            '?' => status.changes.push(FileChange {
                path: record[2..].to_string(),
                orig_path: None,
                kind: ChangeKind::Untracked,
                staged: Staged::None,
            }),
            _ => {}
        }
    }
    status.changes.sort_by_key(|c| c.path.to_lowercase());
    status
}

pub(crate) async fn status(repo: &Path) -> Result<Status, String> {
    let output = run_with(
        repo,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
        None,
        &GitEnv::default(),
        Duration::from_secs(60),
    )
    .await?;
    if !output.ok() {
        return Err(first_error(&output.stderr));
    }
    Ok(parse_status(&output.stdout))
}

// ─── Remotes ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    pub url: String,
    /// `github.com`, or another host.
    pub host: Option<String>,
    /// Set for GitHub remotes.
    pub owner: Option<String>,
    pub repo: Option<String>,
    /// The remote is reached over SSH (the token can't help a push then).
    pub ssh: bool,
}

/// Owner and name of a GitHub repository from any of the URL forms git
/// accepts (`https://github.com/o/r(.git)`, `git@github.com:o/r.git`,
/// `ssh://git@github.com/o/r`, with a user name or a port).
pub(crate) fn parse_remote(name: &str, url: &str) -> Remote {
    let trimmed = url.trim();
    let mut remote = Remote {
        name: name.into(),
        url: strip_credentials(trimmed),
        ..Remote::default()
    };
    let (host_part, path, ssh) = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("git://"))
    {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        (host, path, false)
    } else if let Some(rest) = trimmed.strip_prefix("ssh://") {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        (host, path, true)
    } else if let Some((host, path)) = trimmed.split_once(':').filter(|(host, _)| {
        // scp-like `user@host:path`, but not a Windows path (`C:\…`).
        host.len() > 1 && !host.contains('/') && !host.contains('\\')
    }) {
        (host, path, true)
    } else {
        return remote;
    };
    let host = host_part
        .rsplit('@')
        .next()
        .unwrap_or(host_part)
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    remote.ssh = ssh;
    remote.host = (!host.is_empty()).then(|| host.clone());
    if host == "github.com" || host == "www.github.com" || host == "ssh.github.com" {
        let mut parts = path.trim_matches('/').split('/');
        if let (Some(owner), Some(repo)) = (parts.next(), parts.next()) {
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            if !owner.is_empty() && !repo.is_empty() {
                remote.owner = Some(owner.to_string());
                remote.repo = Some(repo.to_string());
            }
        }
    }
    remote
}

/// `https://user:secret@host/…` → `https://host/…`: a token someone put in
/// a remote URL is never shown or kept.
fn strip_credentials(url: &str) -> String {
    for scheme in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            if let Some((_, host)) = authority.rsplit_once('@') {
                return format!("{scheme}{host}/{path}");
            }
        }
    }
    url.to_string()
}

/// `origin`, or the only/first remote there is.
pub(crate) async fn remote(repo: &Path) -> Result<Option<Remote>, String> {
    let names = read(repo, &["remote"]).await?;
    let names: Vec<&str> = names
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let Some(name) = names
        .iter()
        .find(|name| **name == "origin")
        .or(names.first())
    else {
        return Ok(None);
    };
    let url = read(repo, &["remote", "get-url", name]).await?;
    Ok(Some(parse_remote(name, url.trim())))
}

// ─── Tags and commits ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: String,
    /// Unix seconds.
    pub date: i64,
}

pub(crate) fn parse_tags(text: &str) -> Vec<Tag> {
    text.lines()
        .filter_map(|line| {
            let (name, date) = line.split_once('\t')?;
            Some(Tag {
                name: name.trim().to_string(),
                date: date.trim().parse().unwrap_or(0),
            })
        })
        .filter(|tag| !tag.name.is_empty())
        .collect()
}

/// Every tag, newest first.
pub(crate) async fn tags(repo: &Path) -> Result<Vec<Tag>, String> {
    let text = read(
        repo,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:strip=2)%09%(creatordate:unix)",
            "refs/tags",
        ],
    )
    .await?;
    Ok(parse_tags(&text))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    /// Unix seconds.
    pub date: i64,
}

pub(crate) fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim_matches(['\n', '\r']).split('\x1f');
            let sha = fields.next()?.trim().to_string();
            if sha.is_empty() {
                return None;
            }
            Some(Commit {
                sha,
                subject: fields.next().unwrap_or("").to_string(),
                author: fields.next().unwrap_or("").to_string(),
                date: fields.next().unwrap_or("0").trim().parse().unwrap_or(0),
            })
        })
        .collect()
}

/// Commits in `range` (`v1.0.0..HEAD`, or `HEAD`), newest first, limited to
/// `path` (a monorepo app folder) when given.
pub(crate) async fn log(
    repo: &Path,
    range: &str,
    path: Option<&str>,
    max: usize,
) -> Result<Vec<Commit>, String> {
    let max = format!("-{max}");
    let mut args = vec![
        "log",
        max.as_str(),
        "--no-merges",
        "--format=%H%x1f%s%x1f%an%x1f%ct%x1e",
        range,
    ];
    if let Some(path) = path {
        args.push("--");
        args.push(path);
    }
    let output = run(repo, &args).await?;
    // A repository without commits has no log: not an error.
    if !output.ok() {
        if output.stderr.contains("does not have any commits") {
            return Ok(Vec::new());
        }
        return Err(first_error(&output.stderr));
    }
    Ok(parse_log(&output.text()))
}

// ─── What went wrong ───────────────────────────────────────────────────────

/// Gives a failed commit, push or pull the code the page has an action for.
pub(crate) fn classify(action: &str, output: &str) -> Problem {
    let lower = output.to_lowercase();
    let has = |needle: &str| lower.contains(needle);
    let (code, message): (&str, String) = if has("please tell me who you are")
        || has("unable to auto-detect email address")
        || has("empty ident name")
    {
        (
            "NO_IDENTITY",
            "Git doesn't know your name and email yet. Set them once in a terminal: git config --global user.name \"Your Name\" and git config --global user.email you@example.com.".into(),
        )
    } else if has("nothing to commit") || has("no changes added to commit") {
        (
            "NOTHING_TO_COMMIT",
            "There is nothing staged to commit.".into(),
        )
    } else if has("gh013") || has("push protection") || has("secret scanning") {
        (
            "SECRET_BLOCKED",
            "GitHub blocked the push: a commit contains something that looks like a secret (a key or token). Remove it from the commit, then push again.".into(),
        )
    } else if has("gh006") || has("protected branch") {
        (
            "PROTECTED_BRANCH",
            "This branch is protected on GitHub: push to another branch and open a pull request."
                .into(),
        )
    } else if has("refusing to allow") && has("workflow") {
        (
            "WORKFLOW_SCOPE",
            "GitHub refused the push because it changes a workflow file and your sign-in lacks the \"workflow\" permission. Sign in again with it (gh auth refresh -s workflow), then push.".into(),
        )
    } else if has("hook declined")
        || has("pre-push hook")
        || has("pre-commit hook")
        || has("hook failed")
        || has("husky")
        || has("lint-staged")
    {
        (
            "HOOK_FAILED",
            format!(
                "A git hook stopped the {action}. Fix what it reports below, or run the {action} from a terminal to see more."
            ),
        )
    } else if has("[rejected]")
        || has("non-fast-forward")
        || has("fetch first")
        || has("failed to push some refs") && has("rejected")
    {
        (
            "REJECTED",
            "GitHub has commits you don't have yet. Pull them first (rebase), then push again."
                .into(),
        )
    } else if has("has no upstream branch")
        || has("no upstream configured")
        || has("no tracking information")
    {
        (
            "NO_UPSTREAM",
            "This branch isn't on GitHub yet. Push it to create it there.".into(),
        )
    } else if has("authentication failed")
        || has("could not read username")
        || has("could not read password")
        || has("terminal prompts disabled")
        || has("invalid username or password")
        || has("permission to") && has("denied")
        || has("the requested url returned error: 403")
        || has("the requested url returned error: 401")
        || has("permission denied (publickey)")
    {
        (
            "AUTH",
            if has("publickey") {
                "GitHub didn't accept your SSH key. Check your SSH setup, or switch the remote to HTTPS.".into()
            } else {
                "GitHub didn't accept the sign-in for this push. Connect your GitHub account on this page (or sign in to Git), then try again.".into()
            },
        )
    } else if has("repository not found") || has("does not appear to be a git repository") {
        (
            "NO_REPO",
            "The remote repository was not found, or your account can't see it.".into(),
        )
    } else if has("could not resolve host")
        || has("unable to access")
        || has("timed out")
        || has("connection was reset")
        || has("failed to connect")
    {
        (
            "NETWORK",
            "Couldn't reach GitHub. Check the internet connection and try again.".into(),
        )
    } else if has("conflict") || has("could not apply") {
        (
            "CONFLICT",
            format!("The {action} ran into conflicting changes in the same lines."),
        )
    } else if has("your local changes") && has("would be overwritten") {
        (
            "LOCAL_CHANGES",
            "Your uncommitted changes are in the way. Commit them first.".into(),
        )
    } else {
        ("GIT_FAILED", first_error(output))
    };
    Problem::new(code, message).with_details(output.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_status_with_renames_partial_staging_and_untracked() {
        let raw = concat!(
            "# branch.oid 1234567890abcdef\0",
            "# branch.head main\0",
            "# branch.upstream origin/main\0",
            "# branch.ab +2 -1\0",
            "1 M. N... 100644 100644 100644 aaa bbb src/staged.rs\0",
            "1 MM N... 100644 100644 100644 aaa bbb src/both.rs\0",
            "1 .M N... 100644 100644 100644 aaa bbb src/with space.ts\0",
            "1 .D N... 100644 100644 000000 aaa aaa gone.txt\0",
            "1 A. N... 000000 100644 100644 000 bbb new.txt\0",
            "2 R. N... 100644 100644 100644 aaa bbb R100 docs/new name.md\0docs/old name.md\0",
            "u UU N... 100644 100644 100644 100644 a b c conflict.json\0",
            "? notes/todo.md\0",
            "! ignored.log\0",
        );
        let status = parse_status(raw.as_bytes());
        assert_eq!(status.branch.branch.as_deref(), Some("main"));
        assert_eq!(status.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.branch.ahead, status.branch.behind), (2, 1));
        assert_eq!(status.branch.head.as_deref(), Some("1234567890abcdef"));
        let find = |path: &str| status.changes.iter().find(|c| c.path == path).unwrap();
        assert_eq!(find("src/staged.rs").staged, Staged::All);
        assert_eq!(find("src/both.rs").staged, Staged::Partial);
        assert_eq!(find("src/with space.ts").staged, Staged::None);
        assert_eq!(find("src/with space.ts").kind, ChangeKind::Modified);
        assert_eq!(find("gone.txt").kind, ChangeKind::Deleted);
        assert_eq!(find("new.txt").kind, ChangeKind::Added);
        let renamed = find("docs/new name.md");
        assert_eq!(renamed.kind, ChangeKind::Renamed);
        assert_eq!(renamed.orig_path.as_deref(), Some("docs/old name.md"));
        assert_eq!(find("conflict.json").kind, ChangeKind::Conflicted);
        assert_eq!(find("notes/todo.md").kind, ChangeKind::Untracked);
        assert!(status.changes.iter().all(|c| c.path != "ignored.log"));
        assert_eq!(status.changes.len(), 8);
    }

    #[test]
    fn a_new_repository_and_a_detached_head() {
        let status = parse_status(b"# branch.oid (initial)\0# branch.head master\0? a.txt\0");
        assert_eq!(status.branch.head, None);
        assert_eq!(status.branch.branch.as_deref(), Some("master"));
        assert_eq!(status.branch.upstream, None);
        let detached = parse_status(b"# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!(detached.branch.branch, None);
    }

    #[test]
    fn github_remotes_in_every_form() {
        for url in [
            "https://github.com/thomasthanos/MYLE.git",
            "https://github.com/thomasthanos/MYLE",
            "https://thomas@github.com/thomasthanos/MYLE.git",
            "git@github.com:thomasthanos/MYLE.git",
            "ssh://git@github.com/thomasthanos/MYLE.git",
            "ssh://git@ssh.github.com:443/thomasthanos/MYLE.git",
            "https://github.com/thomasthanos/MYLE/",
        ] {
            let remote = parse_remote("origin", url);
            assert_eq!(remote.owner.as_deref(), Some("thomasthanos"), "{url}");
            assert_eq!(remote.repo.as_deref(), Some("MYLE"), "{url}");
        }
        assert!(parse_remote("origin", "git@github.com:a/b.git").ssh);
        assert!(!parse_remote("origin", "https://github.com/a/b").ssh);
        let gitlab = parse_remote("origin", "https://gitlab.com/a/b.git");
        assert_eq!(gitlab.host.as_deref(), Some("gitlab.com"));
        assert_eq!(gitlab.owner, None);
        let local = parse_remote("origin", r"C:\repos\thing.git");
        assert_eq!(local.host, None);
        let secret = parse_remote(
            "origin",
            "https://x-access-token:ghp_secret@github.com/a/b.git",
        );
        assert!(!secret.url.contains("ghp_secret"));
        assert_eq!(secret.owner.as_deref(), Some("a"));
    }

    #[test]
    fn tags_and_log_records() {
        let tags = parse_tags("v9.5.0\t1760000000\nbackup_projects-v2.0.7\t1759000000\n\n");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "v9.5.0");
        assert_eq!(tags[1].date, 1759000000);
        let log = parse_log(
            "abc\x1fFix: a | b\x1fThomas\x1f1760000000\x1e\ndef\x1fSecond\x1fT\x1f1\x1e\n",
        );
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "Fix: a | b");
        assert_eq!(log[1].date, 1);
    }

    #[test]
    fn push_and_commit_failures_get_their_codes() {
        let rejected = " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to 'https://github.com/a/b.git'\nhint: Updates were rejected because the remote contains work";
        assert_eq!(classify("push", rejected).code, "REJECTED");
        assert_eq!(
            classify(
                "push",
                "fatal: The current branch feature has no upstream branch."
            )
            .code,
            "NO_UPSTREAM"
        );
        assert_eq!(
            classify(
                "push",
                "remote: error: GH006: Protected branch update failed"
            )
            .code,
            "PROTECTED_BRANCH"
        );
        assert_eq!(
            classify("push", "remote: error: GH013: Repository rule violations found\nremote: - Push cannot contain secrets").code,
            "SECRET_BLOCKED"
        );
        assert_eq!(
            classify(
                "push",
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
            )
            .code,
            "AUTH"
        );
        assert_eq!(
            classify("push", "error: failed to push some refs to 'x'\nhusky - pre-push hook exited with code 1 (error)").code,
            "HOOK_FAILED"
        );
        assert_eq!(
            classify("push", "fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com").code,
            "NETWORK"
        );
        assert_eq!(
            classify(
                "pull",
                "CONFLICT (content): Merge conflict in a.txt\nerror: could not apply 123abc... x"
            )
            .code,
            "CONFLICT"
        );
        assert_eq!(
            classify(
                "commit",
                "Author identity unknown\n*** Please tell me who you are."
            )
            .code,
            "NO_IDENTITY"
        );
        let other = classify("push", "fatal: something odd\n");
        assert_eq!(other.code, "GIT_FAILED");
        assert_eq!(other.message, "something odd");
        assert!(other.details.is_some());
    }

    #[test]
    fn the_token_goes_in_as_a_header_for_github_only() {
        let env = token_env("ghp_example");
        assert_eq!(env[1].1, "http.https://github.com/.extraheader");
        assert!(env[2].1.starts_with("AUTHORIZATION: basic "));
        assert!(!env[2].1.contains("ghp_example"));
    }
}
