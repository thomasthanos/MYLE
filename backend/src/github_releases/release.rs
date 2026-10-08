//! Releasing a project, in an order that never leaves GitHub half done:
//!
//! 1. checks (branch, not behind GitHub, the tag is free, a clean folder or
//!    the changes go along, a token, a release workflow in Actions mode);
//! 2. the version in every version file;
//! 3. the build (local mode), then the files to upload;
//! 4. a commit of the version files (and the chosen changes);
//! 5. the push; 6. the tag on that exact commit, pushed;
//! 7. local mode: a draft release, its files uploaded, then published.
//!    Actions mode: the workflow the tag started is watched to its end.
//!
//! A failure before the push puts every file and the branch back as they
//! were. After the push nothing is ever rewritten: the outcome says what is
//! left (upload the rest, publish the draft) so it can be finished.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::build::artifacts::{self, Artifact};
use super::build::runner::{self, BuildEvent, BuildOutcome};
use super::entry::Entry;
use super::git::{self, Remote};
use super::github::{self, NewRelease, Release, Run};
use super::project::{self, Workflow};
use super::versions::{self, Edit};
use super::{CANCELLED, Problem, ops, secrets, store};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Build here and upload the files.
    Local,
    /// Push the tag; the repository's workflow builds and releases.
    Actions,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRequest {
    pub entry_id: String,
    pub version: String,
    pub mode: Mode,
    /// Local mode: build before releasing.
    pub build: bool,
    /// The build command (the saved / detected one otherwise).
    pub command: Option<String>,
    /// Commit the project's other uncommitted changes with the release.
    pub include_changes: bool,
    pub commit_message: Option<String>,
    pub title: String,
    pub notes: String,
    /// Files to upload; `None`: the ones the build made, picked by kind.
    pub assets: Option<Vec<String>>,
    /// Leave the release as a draft.
    pub draft: bool,
    pub prerelease: bool,
    pub make_latest: bool,
    /// Actions mode: write the notes to this file (from the repository's
    /// top) and commit it, for a workflow that reads it.
    pub notes_file: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum ReleaseEvent {
    Step {
        id: &'static str,
        state: StepState,
        message: Option<String>,
    },
    Build {
        event: BuildEvent,
    },
    #[serde(rename_all = "camelCase")]
    Upload {
        name: String,
        sent: u64,
        total: u64,
        index: usize,
        count: usize,
    },
    #[serde(rename_all = "camelCase")]
    Workflow {
        run: Option<Run>,
        jobs: Vec<github::Job>,
    },
    Log {
        text: String,
    },
}

/// What is left to finish a release whose files or publishing failed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub release_id: Option<u64>,
    /// Files still to upload (full paths).
    pub pending_assets: Vec<String>,
    pub publish: bool,
    pub make_latest: bool,
    /// The tag still has to be made and pushed (on `commit`).
    pub tag: Option<String>,
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseOutcome {
    pub ok: bool,
    pub cancelled: bool,
    pub version: String,
    pub tag: String,
    pub commit: Option<String>,
    pub release: Option<Release>,
    pub run: Option<Run>,
    pub problem: Option<Problem>,
    /// The files and the branch were put back as they were.
    pub rolled_back: bool,
    pub resume: Option<Resume>,
    pub build: Option<BuildOutcome>,
    pub artifacts: Vec<Artifact>,
}

// ─── Checks ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: &'static str,
    pub state: CheckState,
    pub message: String,
    /// What the page can do about it: `pull`, `connect`, `commit`, `build`.
    pub fix: Option<&'static str>,
}

fn check(
    id: &'static str,
    state: CheckState,
    message: impl Into<String>,
    fix: Option<&'static str>,
) -> Check {
    Check {
        id,
        state,
        message: message.into(),
        fix,
    }
}

/// Everything a release needs, gathered once.
pub(crate) struct Context {
    pub entry: Entry,
    pub remote: Option<Remote>,
    pub status: git::Status,
    pub versions: versions::Versions,
    pub workflows: Vec<Workflow>,
}

/// When each repository was last fetched for the checks.
static CHECK_FETCHES: std::sync::Mutex<Vec<(PathBuf, Instant)>> = std::sync::Mutex::new(Vec::new());
/// The checks follow the form as it is typed; they fetch at most this often.
const CHECK_FETCH_EVERY: Duration = Duration::from_secs(90);

/// Whether the checks should fetch `root` now (and notes that they do).
fn check_fetch_due(root: &Path, now: Instant) -> bool {
    let mut fetches = CHECK_FETCHES.lock().unwrap_or_else(|e| e.into_inner());
    fetches.retain(|(_, at)| now.duration_since(*at) < CHECK_FETCH_EVERY);
    if fetches.iter().any(|(path, _)| path == root) {
        return false;
    }
    fetches.push((root.to_path_buf(), now));
    true
}

/// The context for the live checks: fetched now and then, not on every
/// keystroke (a fetch takes seconds, and the checks ran again on each).
pub(crate) async fn context_for_checks(entry: Entry) -> Result<Context, String> {
    let fetch = check_fetch_due(&entry.root, Instant::now());
    context(entry, fetch).await
}

pub(crate) async fn context(entry: Entry, fetch: bool) -> Result<Context, String> {
    git::require_git()?;
    if fetch {
        let _ = ops::fetch(&entry.root, true, Arc::new(AtomicBool::new(false))).await;
    }
    let remote = git::remote(&entry.root).await?;
    let status = git::status(&entry.root).await?;
    let skip = entry.config.skip_version_files.clone();
    let dir = entry.dir.clone();
    let root = entry.root.clone();
    let (versions, workflows) = super::blocking(move || {
        (
            versions::detect(&dir, &skip),
            project::release_workflows(&root),
        )
    })
    .await?;
    Ok(Context {
        entry,
        remote,
        status,
        versions,
        workflows,
    })
}

/// Changes inside the project that are not version files.
fn own_changes(context: &Context) -> Vec<&git::FileChange> {
    let version_paths: Vec<String> = context
        .versions
        .files
        .iter()
        .map(|f| context.entry.repo_path(&f.path))
        .collect();
    context
        .status
        .changes
        .iter()
        .filter(|c| context.entry.contains(&c.path))
        .filter(|c| !version_paths.contains(&c.path))
        .collect()
}

pub(crate) async fn preflight(
    context: &Context,
    version: &str,
    mode: Mode,
    include_changes: bool,
    has_build: bool,
    build: bool,
) -> Vec<Check> {
    let mut checks = Vec::new();
    let entry = &context.entry;
    let tag = project::tag_name(&entry.tag_prefix, version);
    match versions::check_new_version(version) {
        Err(error) => checks.push(check("version", CheckState::Fail, error, None)),
        Ok(_) => match context.versions.current.as_deref().map(versions::parse) {
            Some(Ok(current)) => {
                let new = versions::parse(version).unwrap_or(current.clone());
                if new < current {
                    checks.push(check(
                        "version",
                        CheckState::Fail,
                        format!("{version} is older than the current {current}."),
                        None,
                    ));
                } else if new == current {
                    checks.push(check(
                        "version",
                        CheckState::Warn,
                        format!("The files already say {version}: they are released as they are."),
                        None,
                    ));
                } else {
                    checks.push(check(
                        "version",
                        CheckState::Ok,
                        format!("{current} → {version}"),
                        None,
                    ));
                }
            }
            _ if context.versions.files.is_empty() => checks.push(check(
                "version",
                CheckState::Warn,
                format!("No version file was found: only the tag {tag} says {version}."),
                None,
            )),
            _ => checks.push(check("version", CheckState::Ok, version.to_string(), None)),
        },
    }
    if !context.versions.mismatched.is_empty() {
        checks.push(check(
            "versions",
            CheckState::Warn,
            format!(
                "These files disagree and will all be set to {version}: {}",
                context.versions.mismatched.join(", ")
            ),
            None,
        ));
    }
    match &context.remote {
        Some(remote) if remote.owner.is_some() => checks.push(check(
            "remote",
            CheckState::Ok,
            format!(
                "github.com/{}/{}",
                remote.owner.as_deref().unwrap_or(""),
                remote.repo.as_deref().unwrap_or("")
            ),
            None,
        )),
        Some(remote) => checks.push(check(
            "remote",
            CheckState::Fail,
            format!("The remote ({}) is not a GitHub repository.", remote.url),
            None,
        )),
        None => checks.push(check(
            "remote",
            CheckState::Fail,
            "This repository has no remote on GitHub.",
            None,
        )),
    }
    match &context.status.branch.branch {
        None => checks.push(check(
            "branch",
            CheckState::Fail,
            "No branch is checked out (a detached HEAD).",
            None,
        )),
        Some(branch) if context.status.branch.upstream.is_none() => checks.push(check(
            "branch",
            CheckState::Warn,
            format!("{branch} isn't on GitHub yet: the release pushes it there."),
            None,
        )),
        Some(branch) if context.status.branch.behind > 0 => checks.push(check(
            "branch",
            CheckState::Fail,
            format!(
                "{branch} is {} commit(s) behind GitHub. Pull first.",
                context.status.branch.behind
            ),
            Some("pull"),
        )),
        Some(branch) => checks.push(check(
            "branch",
            CheckState::Ok,
            format!("On {branch}, up to date with GitHub"),
            None,
        )),
    }
    let own = own_changes(context);
    let conflicted = own.iter().any(|c| c.kind == git::ChangeKind::Conflicted);
    if conflicted {
        checks.push(check(
            "changes",
            CheckState::Fail,
            "There are unresolved merge conflicts.",
            None,
        ));
    } else if own.is_empty() {
        checks.push(check(
            "changes",
            CheckState::Ok,
            "No uncommitted changes",
            None,
        ));
    } else if include_changes {
        checks.push(check(
            "changes",
            CheckState::Warn,
            format!(
                "{} uncommitted change(s) go into the release commit.",
                own.len()
            ),
            None,
        ));
    } else {
        checks.push(check(
            "changes",
            CheckState::Fail,
            format!("{} uncommitted change(s) would be built but not committed. Commit them, or include them in the release.", own.len()),
            Some("commit"),
        ));
    }
    let local_tag = git::run(
        &entry.root,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )
    .await
    .is_ok_and(|o| o.ok());
    if local_tag {
        checks.push(check(
            "tag",
            CheckState::Fail,
            format!("The tag {tag} already exists. Pick another version."),
            None,
        ));
    } else {
        checks.push(check(
            "tag",
            CheckState::Ok,
            format!("The tag {tag} is free"),
            None,
        ));
    }
    match secrets::token() {
        None => checks.push(check(
            "account",
            CheckState::Fail,
            "Connect your GitHub account first.",
            Some("connect"),
        )),
        Some(token) => {
            if let Some(remote) = &context.remote
                && let (Some(owner), Some(repo)) = (&remote.owner, &remote.repo)
            {
                // A tag only on GitHub (a failed release, another PC) would
                // stop the tag push after the release commit is pushed.
                if !local_tag && let Ok(true) = github::tag_exists(&token, owner, repo, &tag).await {
                    checks.push(check(
                        "tag",
                        CheckState::Fail,
                        format!("The tag {tag} is already on GitHub. Pick another version, or delete the tag under Releases → Tags without a release."),
                        None,
                    ));
                    checks.retain(|c| !(c.id == "tag" && c.state == CheckState::Ok));
                }
                match github::release_by_tag(&token, owner, repo, &tag).await {
                    Ok(Some(_)) => checks.push(check(
                        "account",
                        CheckState::Fail,
                        format!("GitHub already has a release for {tag}."),
                        None,
                    )),
                    Ok(None) => checks.push(check(
                        "account",
                        CheckState::Ok,
                        "Your GitHub account can see the repository",
                        None,
                    )),
                    Err(error) => {
                        let problem: Option<Problem> =
                            serde_json::from_str::<serde_json::Value>(&error)
                                .ok()
                                .and_then(|v| {
                                    Some(Problem::new(
                                        v.get("code")?.as_str()?,
                                        v.get("message")?.as_str()?,
                                    ))
                                });
                        let (message, fix) = match problem {
                            Some(p) if p.code == "AUTH" => (p.message, Some("connect")),
                            Some(p) => (p.message, None),
                            None => (error, None),
                        };
                        checks.push(check("account", CheckState::Fail, message, fix));
                    }
                }
            }
        }
    }
    match mode {
        Mode::Actions => match project::workflow_for_tag(&context.workflows, &tag) {
            Some(workflow) => checks.push(check(
                "workflow",
                CheckState::Ok,
                format!(
                    "Pushing {tag} starts {}",
                    workflow
                        .name
                        .clone()
                        .unwrap_or_else(|| workflow.file.clone())
                ),
                None,
            )),
            None => checks.push(check(
                "workflow",
                CheckState::Fail,
                format!("No workflow runs on a push of {tag}. Release from here instead."),
                None,
            )),
        },
        Mode::Local => {
            if build && !has_build {
                checks.push(check(
                    "build",
                    CheckState::Fail,
                    "No build command was found. Type one in the Build tab, or release without building.",
                    Some("build"),
                ));
            }
            // The tag starts the repository's own release workflow anyway,
            // which publishes the same release from GitHub.
            if let Some(workflow) = project::workflow_for_tag(&context.workflows, &tag) {
                checks.push(check(
                    "workflow",
                    CheckState::Warn,
                    format!(
                        "Pushing {tag} also starts {} on GitHub, which releases it too. Pick GitHub Actions to let it do the work.",
                        workflow.name.clone().unwrap_or_else(|| workflow.file.clone())
                    ),
                    None,
                ));
            }
        }
    }
    checks
}

// ─── Running ───────────────────────────────────────────────────────────────

pub(crate) struct Session<'a> {
    on_event: &'a (dyn Fn(ReleaseEvent) + Send + Sync),
    cancel: Arc<AtomicBool>,
}

impl Session<'_> {
    fn step(&self, id: &'static str, state: StepState, message: Option<String>) {
        (self.on_event)(ReleaseEvent::Step { id, state, message });
    }

    fn log(&self, text: impl Into<String>) {
        (self.on_event)(ReleaseEvent::Log { text: text.into() });
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn problem_of(error: String) -> Problem {
    serde_json::from_str::<serde_json::Value>(&error)
        .ok()
        .and_then(|v| {
            let mut problem = Problem::new(v.get("code")?.as_str()?, v.get("message")?.as_str()?);
            problem.details = v
                .get("details")
                .and_then(|d| d.as_str())
                .map(str::to_string);
            Some(problem)
        })
        .unwrap_or_else(|| {
            Problem::new(
                if error == CANCELLED {
                    "CANCELLED"
                } else {
                    "FAILED"
                },
                error,
            )
        })
}

/// A file to upload: inside the project's repository, and there.
fn check_asset(entry: &Entry, path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    let canonical = path
        .canonicalize()
        .map_err(|_| format!("{} is no longer there.", path.display()))?;
    let root = entry.root.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(format!("{} is not a file of this project.", path.display()));
    }
    Ok(path)
}

pub(crate) async fn run(
    request: ReleaseRequest,
    entry: Entry,
    build_command: Option<String>,
    cancel: Arc<AtomicBool>,
    on_event: &(dyn Fn(ReleaseEvent) + Send + Sync),
) -> ReleaseOutcome {
    let run = Session {
        on_event,
        cancel: cancel.clone(),
    };
    let version = match versions::check_new_version(&request.version) {
        Ok(version) => version,
        Err(error) => {
            return ReleaseOutcome {
                problem: Some(Problem::new("BAD_VERSION", error)),
                ..ReleaseOutcome::default()
            };
        }
    };
    let tag = project::tag_name(&entry.tag_prefix, &version);
    let mut outcome = ReleaseOutcome {
        version: version.clone(),
        tag: tag.clone(),
        ..ReleaseOutcome::default()
    };

    // 1. Checks.
    run.step("check", StepState::Running, None);
    let context = match context(entry.clone(), true).await {
        Ok(context) => context,
        Err(error) => {
            run.step("check", StepState::Failed, None);
            outcome.problem = Some(problem_of(error));
            return outcome;
        }
    };
    let has_build = build_command.is_some();
    let checks = preflight(
        &context,
        &version,
        request.mode,
        request.include_changes,
        has_build,
        request.build,
    )
    .await;
    if let Some(failed) = checks.iter().find(|c| c.state == CheckState::Fail) {
        run.step("check", StepState::Failed, Some(failed.message.clone()));
        outcome.problem = Some(
            Problem::new("CHECK_FAILED", failed.message.clone())
                .with_data(json!({ "check": failed.id, "fix": failed.fix })),
        );
        return outcome;
    }
    run.step("check", StepState::Done, None);
    let remote = context.remote.clone().unwrap_or_default();
    let (owner, repo) = (
        remote.owner.clone().unwrap_or_default(),
        remote.repo.clone().unwrap_or_default(),
    );
    let Some(token) = secrets::token() else {
        outcome.problem = Some(Problem::new("AUTH", "Connect your GitHub account first."));
        return outcome;
    };

    // 2. Version files (and the notes file).
    run.step("version", StepState::Running, None);
    let mut edits: Vec<Edit> = {
        let dir = entry.dir.clone();
        let versions = context.versions.clone();
        let version = version.clone();
        match super::blocking(move || versions::plan(&dir, &versions, &version))
            .await
            .and_then(|r| r)
        {
            Ok(edits) => edits,
            Err(error) => {
                run.step("version", StepState::Failed, Some(error.clone()));
                outcome.problem = Some(Problem::new("VERSION_FAILED", error));
                return outcome;
            }
        }
    };
    if let (Mode::Actions, Some(notes_file)) = (request.mode, request.notes_file.as_deref())
        && !request.notes.trim().is_empty()
    {
        let rel = notes_file.replace('\\', "/");
        if rel.contains("..") || Path::new(&rel).is_absolute() {
            outcome.problem = Some(Problem::new(
                "BAD_PATH",
                "The notes file must be inside the repository.",
            ));
            return outcome;
        }
        let path = entry.root.join(&rel);
        let before = std::fs::read(&path).unwrap_or_default();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        edits.push(Edit {
            path,
            rel: rel.clone(),
            before,
            after: format!("{}\n", request.notes.trim()).into_bytes(),
        });
    }
    let notes_rel: Option<String> = request
        .notes_file
        .as_deref()
        .map(|file| file.replace('\\', "/"))
        .filter(|_| request.mode == Mode::Actions && !request.notes.trim().is_empty());
    if let Err(error) = versions::apply(&edits) {
        run.step("version", StepState::Failed, Some(error.clone()));
        outcome.problem = Some(Problem::new("VERSION_FAILED", error));
        return outcome;
    }
    run.step(
        "version",
        StepState::Done,
        Some(if edits.is_empty() {
            "Already up to date".into()
        } else {
            edits
                .iter()
                .map(|e| e.rel.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }),
    );
    let rollback = |run: &Session, edits: &[Edit]| {
        versions::undo(edits);
        run.log("The version files were put back as they were.");
    };

    // 3. Build and files.
    let mut assets: Vec<PathBuf> = Vec::new();
    if request.mode == Mode::Local {
        if request.build {
            let command = request
                .command
                .clone()
                .or(build_command.clone())
                .unwrap_or_default();
            run.step("build", StepState::Running, Some(command.clone()));
            let started = SystemTime::now();
            let log_path = super::build_log_path(&entry.id);
            let result = runner::run(
                runner::Request {
                    dir: &entry.dir,
                    command: &command,
                    last: entry.config.last_build.clone(),
                    log_path,
                },
                cancel.clone(),
                |event| on_event(ReleaseEvent::Build { event }),
            )
            .await;
            let ok = result.ok;
            outcome.build = Some(result.clone());
            if ok {
                super::remember_build(&entry.id, &command, &result);
            }
            if !ok {
                run.step("build", StepState::Failed, None);
                rollback(&run, &edits);
                outcome.rolled_back = true;
                outcome.cancelled = result.cancelled;
                outcome.problem = Some(if result.cancelled {
                    Problem::new(
                        "CANCELLED",
                        "The release was cancelled during the build. Nothing was changed.",
                    )
                } else {
                    Problem::new(
                        "BUILD_FAILED",
                        result.failure.clone().unwrap_or_else(|| {
                            format!(
                                "The build failed ({} error(s)). Nothing was committed or pushed.",
                                result.errors
                            )
                        }),
                    )
                });
                return outcome;
            }
            run.step("build", StepState::Done, None);
            let dir = entry.dir.clone();
            outcome.artifacts = super::blocking(move || artifacts::find(&dir, started))
                .await
                .unwrap_or_default();
        } else {
            run.step("build", StepState::Skipped, None);
        }
        let chosen: Vec<String> = request
            .assets
            .clone()
            .unwrap_or_else(|| artifacts::preselect(&outcome.artifacts));
        for path in &chosen {
            match check_asset(&entry, path) {
                Ok(path) => assets.push(path),
                Err(error) => {
                    rollback(&run, &edits);
                    outcome.rolled_back = true;
                    outcome.problem = Some(Problem::new("ASSET_MISSING", error));
                    return outcome;
                }
            }
        }
    } else {
        run.step(
            "build",
            StepState::Skipped,
            Some("GitHub Actions builds it".into()),
        );
    }
    if run.cancelled() {
        rollback(&run, &edits);
        outcome.rolled_back = true;
        outcome.cancelled = true;
        outcome.problem = Some(Problem::new(
            "CANCELLED",
            "The release was cancelled. Nothing was changed.",
        ));
        return outcome;
    }

    // 4. Commit.
    run.step("commit", StepState::Running, None);
    let mut paths: Vec<String> = edits
        .iter()
        .map(|e| {
            e.path
                .strip_prefix(&entry.root)
                .map(|p| p.display().to_string().replace('\\', "/"))
                .unwrap_or_else(|_| e.rel.clone())
        })
        .collect();
    if let Some(rel) = &notes_rel
        && !paths.contains(rel)
    {
        paths.push(rel.clone());
    }
    if request.include_changes {
        for change in own_changes(&context) {
            paths.push(change.path.clone());
            if let Some(orig) = &change.orig_path {
                paths.push(orig.clone());
            }
        }
    }
    paths.sort();
    paths.dedup();
    let head_before = context.status.branch.head.clone();
    let mut committed = false;
    if !paths.is_empty() {
        let mut add: Vec<&str> = vec!["add", "-A", "--"];
        add.extend(paths.iter().map(String::as_str));
        match git::run(&entry.root, &add).await {
            Ok(output) if output.ok() => {}
            result => {
                let error = match result {
                    Ok(output) => output.all(),
                    Err(error) => error,
                };
                run.step("commit", StepState::Failed, Some(git::first_error(&error)));
                rollback(&run, &edits);
                outcome.rolled_back = true;
                outcome.problem = Some(Problem::new("GIT_FAILED", git::first_error(&error)));
                return outcome;
            }
        }
        let message = request
            .commit_message
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| default_commit_message(&entry, &version));
        match ops::commit(&entry.root, &message, Some(&paths)).await {
            Ok(sha) => {
                committed = true;
                outcome.commit = Some(sha);
            }
            Err(problem) if problem.code == "NOTHING_TO_COMMIT" => {}
            Err(problem) => {
                run.step("commit", StepState::Failed, Some(problem.message.clone()));
                let mut reset: Vec<&str> = vec!["reset", "-q", "--"];
                let version_paths: Vec<String> = edits
                    .iter()
                    .map(|e| {
                        e.path
                            .strip_prefix(&entry.root)
                            .map(|p| p.display().to_string().replace('\\', "/"))
                            .unwrap_or_default()
                    })
                    .collect();
                reset.extend(version_paths.iter().map(String::as_str));
                let _ = git::run(&entry.root, &reset).await;
                rollback(&run, &edits);
                outcome.rolled_back = true;
                outcome.problem = Some(problem);
                return outcome;
            }
        }
    }
    let commit = match outcome.commit.clone().or(head_before.clone()) {
        Some(sha) => sha,
        None => {
            run.step("commit", StepState::Failed, None);
            outcome.problem = Some(Problem::new(
                "NO_COMMITS",
                "The repository has no commits to release.",
            ));
            return outcome;
        }
    };
    outcome.commit = Some(commit.clone());
    run.step(
        "commit",
        if committed {
            StepState::Done
        } else {
            StepState::Skipped
        },
        Some(commit[..commit.len().min(7)].to_string()),
    );

    // 5. Push.
    run.step("push", StepState::Running, None);
    let on_line = |text: &str, replace: bool| {
        if !replace {
            on_event(ReleaseEvent::Log {
                text: text.to_string(),
            });
        }
    };
    let pushed = ops::push(&entry.root, cancel.clone(), &on_line).await;
    let push_problem = match pushed {
        Ok(result) if result.ok => None,
        Ok(result) => Some(
            result
                .problem
                .unwrap_or_else(|| Problem::new("PUSH_FAILED", "The push failed.")),
        ),
        Err(error) => Some(problem_of(error)),
    };
    if let Some(problem) = push_problem {
        run.step("push", StepState::Failed, Some(problem.message.clone()));
        // Nothing reached GitHub: the release commit is taken back.
        if committed {
            let still_head = git::read(&entry.root, &["rev-parse", "HEAD"])
                .await
                .is_ok_and(|h| h.trim() == commit);
            if still_head {
                let _ = git::run(&entry.root, &["reset", "--soft", "-q", "HEAD~1"]).await;
                let mut reset: Vec<&str> = vec!["reset", "-q", "--"];
                let version_paths: Vec<String> = edits
                    .iter()
                    .map(|e| {
                        e.path
                            .strip_prefix(&entry.root)
                            .map(|p| p.display().to_string().replace('\\', "/"))
                            .unwrap_or_default()
                    })
                    .collect();
                reset.extend(version_paths.iter().map(String::as_str));
                let _ = git::run(&entry.root, &reset).await;
            }
        }
        rollback(&run, &edits);
        outcome.rolled_back = true;
        outcome.commit = None;
        outcome.cancelled = problem.code == "CANCELLED";
        outcome.problem = Some(problem);
        return outcome;
    }
    run.step("push", StepState::Done, None);

    // 6. Tag.
    finish_from_tag(
        &run,
        &entry,
        &token,
        &owner,
        &repo,
        &request,
        &tag,
        &commit,
        assets,
        notes_rel.is_some(),
        outcome,
    )
    .await
}

fn default_commit_message(entry: &Entry, version: &str) -> String {
    match &entry.sub {
        Some(_) if entry.monorepo => format!("{} {version}", entry.name),
        _ => format!("Release {version}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn finish_from_tag(
    run: &Session<'_>,
    entry: &Entry,
    token: &str,
    owner: &str,
    repo: &str,
    request: &ReleaseRequest,
    tag: &str,
    commit: &str,
    assets: Vec<PathBuf>,
    notes_in_file: bool,
    mut outcome: ReleaseOutcome,
) -> ReleaseOutcome {
    let resume_tag = |assets: &[PathBuf]| Resume {
        tag: Some(tag.to_string()),
        commit: Some(commit.to_string()),
        pending_assets: assets.iter().map(|p| p.display().to_string()).collect(),
        publish: !request.draft,
        make_latest: request.make_latest,
        release_id: None,
    };
    run.step("tag", StepState::Running, Some(tag.to_string()));
    let exists = git::run(
        &entry.root,
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )
    .await
    .is_ok_and(|o| o.ok());
    if exists {
        // Left by an earlier attempt: only pushed when it is this commit's.
        let points = git::read(&entry.root, &["rev-parse", &format!("refs/tags/{tag}^{{commit}}")])
            .await
            .map(|sha| sha.trim().to_string())
            .unwrap_or_default();
        if points.is_empty() || !(points.starts_with(commit) || commit.starts_with(&points)) {
            run.step("tag", StepState::Failed, Some(format!("{tag} is on another commit")));
            outcome.problem = Some(Problem::new(
                "TAG_FAILED",
                format!(
                    "The commit is on GitHub, but the tag {tag} on this PC points at another commit. Delete that tag (Releases → Tags without a release) and resume."
                ),
            ));
            outcome.resume = Some(resume_tag(&assets));
            return outcome;
        }
    }
    if !exists {
        let message = if request.title.trim().is_empty() {
            tag.to_string()
        } else {
            request.title.trim().to_string()
        };
        let made = git::run_with(
            &entry.root,
            &["tag", "-a", tag, "--file=-", commit],
            Some(message.as_bytes()),
            &git::GitEnv::default(),
            Duration::from_secs(60),
        )
        .await;
        if !made.as_ref().is_ok_and(|o| o.ok()) {
            let error = made.map(|o| o.all()).unwrap_or_else(|e| e);
            run.step("tag", StepState::Failed, Some(git::first_error(&error)));
            outcome.problem = Some(Problem::new(
                "TAG_FAILED",
                format!(
                    "The commit is on GitHub, but the tag could not be made: {}",
                    git::first_error(&error)
                ),
            ));
            outcome.resume = Some(resume_tag(&assets));
            return outcome;
        }
    }
    let on_line = |text: &str, replace: bool| {
        if !replace {
            (run.on_event)(ReleaseEvent::Log {
                text: text.to_string(),
            });
        }
    };
    let pushed = ops::push_tag(&entry.root, tag, run.cancel.clone(), &on_line).await;
    let problem = match pushed {
        Ok(result) if result.ok => None,
        Ok(result) => result.problem,
        Err(error) => Some(problem_of(error)),
    };
    if let Some(problem) = problem {
        run.step("tag", StepState::Failed, Some(problem.message.clone()));
        // Only a tag made just now goes; one that was here before stays.
        if !exists {
            let _ = git::run(&entry.root, &["tag", "-d", tag]).await;
        }
        outcome.problem = Some(
            Problem::new(
                &problem.code,
                format!(
                    "The commit is on GitHub, but its tag could not be pushed: {}",
                    problem.message
                ),
            )
            .with_details(problem.details.unwrap_or_default()),
        );
        outcome.resume = Some(resume_tag(&assets));
        return outcome;
    }
    run.step("tag", StepState::Done, Some(tag.to_string()));

    match request.mode {
        Mode::Local => {
            run.step("release", StepState::Running, None);
            let new = NewRelease {
                tag: tag.to_string(),
                target: Some(commit.to_string()),
                title: if request.title.trim().is_empty() {
                    tag.to_string()
                } else {
                    request.title.trim().to_string()
                },
                notes: request.notes.clone(),
                draft: true,
                prerelease: request.prerelease,
                make_latest: false,
            };
            let release = match github::create_release(token, owner, repo, &new).await {
                Ok(release) => release,
                Err(error) => {
                    let problem = problem_of(error);
                    run.step("release", StepState::Failed, Some(problem.message.clone()));
                    outcome.problem = Some(problem);
                    let mut resume = resume_tag(&assets);
                    resume.tag = None;
                    outcome.resume = Some(resume);
                    return outcome;
                }
            };
            run.step("release", StepState::Done, Some("Draft made".into()));
            let resume = Resume {
                release_id: Some(release.id),
                pending_assets: assets.iter().map(|p| p.display().to_string()).collect(),
                publish: !request.draft,
                make_latest: request.make_latest,
                tag: None,
                commit: None,
            };
            outcome.release = Some(release);
            finish_release(run, token, owner, repo, resume, outcome).await
        }
        Mode::Actions => {
            run.step("workflow", StepState::Running, None);
            let workflow = tag_workflow(entry, tag).await;
            let result = watch(
                run,
                token,
                owner,
                repo,
                commit,
                Some(tag),
                workflow.as_ref(),
            )
            .await;
            match result {
                Ok(done) => {
                    let success = done.conclusion.as_deref() == Some("success");
                    outcome.run = Some(done.clone());
                    if !success {
                        run.step("workflow", StepState::Failed, done.conclusion.clone());
                        outcome.problem = Some(Problem::new(
                            "WORKFLOW_FAILED",
                            format!(
                                "The release workflow ended: {}. Open it on GitHub to see why.",
                                done.conclusion.unwrap_or_else(|| "unknown".into())
                            ),
                        ));
                        return outcome;
                    }
                    run.step("workflow", StepState::Done, None);
                    // The workflow made the release; give it the notes when
                    // they were not handed over in a file.
                    if let Ok(Some(release)) = github::release_by_tag(token, owner, repo, tag).await
                    {
                        let release = if !notes_in_file && !request.notes.trim().is_empty() {
                            github::update_release(
                                token,
                                owner,
                                repo,
                                release.id,
                                &json!({ "body": request.notes }),
                            )
                            .await
                            .unwrap_or(release)
                        } else {
                            release
                        };
                        outcome.release = Some(release);
                    }
                    outcome.ok = true;
                    outcome
                }
                Err(error) => {
                    let problem = problem_of(error);
                    let stopped = problem.code == "CANCELLED";
                    run.step(
                        "workflow",
                        if stopped {
                            StepState::Skipped
                        } else {
                            StepState::Failed
                        },
                        Some(problem.message.clone()),
                    );
                    // The tag is pushed: the release goes on on GitHub.
                    outcome.ok = stopped;
                    if !stopped {
                        outcome.problem = Some(problem);
                    }
                    outcome
                }
            }
        }
    }
}

/// Uploads the pending files of a draft (or any) release, then publishes it.
async fn finish_release(
    run: &Session<'_>,
    token: &str,
    owner: &str,
    repo: &str,
    mut resume: Resume,
    mut outcome: ReleaseOutcome,
) -> ReleaseOutcome {
    let Some(release_id) = resume.release_id else {
        outcome.problem = Some(Problem::new("FAILED", "There is no release to finish."));
        return outcome;
    };
    let mut release = match outcome.release.clone() {
        Some(release) => release,
        // By its id: a list could miss it behind newer releases.
        None => match github::release(token, owner, repo, release_id).await {
            Ok(release) => release,
            Err(error) if error.contains("\"NOT_FOUND\"") => {
                outcome.problem = Some(Problem::new(
                    "NOT_FOUND",
                    "The draft release is no longer on GitHub.",
                ));
                return outcome;
            }
            Err(error) => {
                outcome.problem = Some(problem_of(error));
                outcome.resume = Some(resume);
                return outcome;
            }
        },
    };
    let count = resume.pending_assets.len();
    if count > 0 {
        run.step(
            "upload",
            StepState::Running,
            Some(format!("{count} file(s)")),
        );
    } else {
        run.step("upload", StepState::Skipped, None);
    }
    let pending = resume.pending_assets.clone();
    for (index, path) in pending.iter().enumerate() {
        let path_buf = PathBuf::from(path);
        let name = path_buf
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut last_error = None;
        for attempt in 0..3 {
            if run.cancelled() {
                last_error = Some(CANCELLED.to_string());
                break;
            }
            // A failed upload can leave a broken file of that name behind.
            if let Some(stale) = release.assets.iter().find(|a| a.name == name).cloned() {
                let _ = github::delete_asset(token, owner, repo, stale.id).await;
                release.assets.retain(|a| a.id != stale.id);
            }
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
            let upload = github::upload_asset(
                token,
                &release.upload_url,
                &path_buf,
                &name,
                run.cancel.clone(),
                move |sent, total| {
                    let _ = sender.send((sent, total));
                },
            );
            tokio::pin!(upload);
            let result = loop {
                tokio::select! {
                    result = &mut upload => break result,
                    Some((sent, total)) = receiver.recv() => {
                        (run.on_event)(ReleaseEvent::Upload { name: name.clone(), sent, total, index, count });
                    }
                }
            };
            match result {
                Ok(asset) => {
                    release.assets.push(asset);
                    last_error = None;
                    break;
                }
                Err(error) => {
                    run.log(format!(
                        "Uploading {name} failed (attempt {}): {}",
                        attempt + 1,
                        problem_of(error.clone()).message
                    ));
                    last_error = Some(error);
                    if error_is_final(last_error.as_deref().unwrap_or("")) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2 * (attempt + 1))).await;
                }
            }
        }
        if let Some(error) = last_error {
            run.step("upload", StepState::Failed, Some(name.clone()));
            resume.pending_assets = pending[index..].to_vec();
            outcome.cancelled = error == CANCELLED;
            outcome.problem = Some(problem_of(error));
            outcome.release = Some(release);
            outcome.resume = Some(resume);
            return outcome;
        }
    }
    resume.pending_assets.clear();
    if count > 0 {
        run.step("upload", StepState::Done, None);
    }
    if resume.publish {
        run.step("publish", StepState::Running, None);
        let patch = json!({
            "draft": false,
            "make_latest": if resume.make_latest && !release.prerelease { "true" } else { "false" },
        });
        match github::update_release(token, owner, repo, release_id, &patch).await {
            Ok(published) => {
                run.step("publish", StepState::Done, None);
                release = published;
            }
            Err(error) => {
                run.step("publish", StepState::Failed, None);
                outcome.problem = Some(problem_of(error));
                outcome.release = Some(release);
                outcome.resume = Some(resume);
                return outcome;
            }
        }
    } else {
        run.step(
            "publish",
            StepState::Skipped,
            Some("Kept as a draft".into()),
        );
    }
    outcome.release = Some(release);
    outcome.ok = true;
    outcome
}

fn error_is_final(error: &str) -> bool {
    error == CANCELLED
        || ["\"AUTH\"", "\"FORBIDDEN\"", "\"NOT_FOUND\""]
            .iter()
            .any(|code| error.contains(code))
}

/// The run to follow among those a push of `commit` started: a push of
/// `tag`, and of the release `workflow` when it is known. A tag can start
/// several workflows (MYLE's starts Release and MYLE Passwords, which ends
/// in seconds); following another one would call the release done early.
pub(crate) fn pick_run(
    runs: Vec<Run>,
    tag: Option<&str>,
    workflow: Option<&Workflow>,
) -> Option<Run> {
    let mut candidates = runs
        .into_iter()
        .filter(|r| r.event == "push")
        .filter(|r| tag.is_none_or(|tag| r.head_branch.as_deref() == Some(tag)));
    match workflow {
        None => candidates.next(),
        Some(workflow) => candidates.find(|r| match &r.path {
            Some(path) => {
                let path = path.split('@').next().unwrap_or(path).replace('\\', "/");
                path.rsplit('/').next() == Some(workflow.file.as_str())
            }
            None => r.name.is_some() && r.name == workflow.name,
        }),
    }
}

/// The repository's workflow a push of `tag` starts.
async fn tag_workflow(entry: &Entry, tag: &str) -> Option<Workflow> {
    let root = entry.root.clone();
    let workflows = super::blocking(move || project::release_workflows(&root))
        .await
        .ok()?;
    project::workflow_for_tag(&workflows, tag).cloned()
}

/// Follows the workflow run a push of `commit` (and `tag`) started, to its
/// end. Cancel stops watching, not the run.
async fn watch(
    run: &Session<'_>,
    token: &str,
    owner: &str,
    repo: &str,
    commit: &str,
    tag: Option<&str>,
    workflow: Option<&Workflow>,
) -> Result<Run, String> {
    let started = Instant::now();
    let found = loop {
        if run.cancelled() {
            return Err(Problem::new(
                "CANCELLED",
                "Stopped watching. The workflow goes on on GitHub.",
            )
            .into());
        }
        let runs = github::runs_for_commit(token, owner, repo, commit)
            .await
            .unwrap_or_default();
        if let Some(found) = pick_run(runs, tag, workflow) {
            break found;
        }
        if started.elapsed() > Duration::from_secs(180) {
            return Err(Problem::new("NO_RUN", "GitHub didn't start a workflow for the tag within 3 minutes. Check the repository's Actions tab.").into());
        }
        (run.on_event)(ReleaseEvent::Workflow {
            run: None,
            jobs: Vec::new(),
        });
        sleep_unless_cancelled(&run.cancel, Duration::from_secs(5)).await;
    };
    let mut current = found;
    loop {
        let jobs = github::run_jobs(token, owner, repo, current.id)
            .await
            .unwrap_or_default();
        (run.on_event)(ReleaseEvent::Workflow {
            run: Some(current.clone()),
            jobs,
        });
        if current.status.as_deref() == Some("completed") {
            return Ok(current);
        }
        if run.cancelled() {
            return Err(Problem::new(
                "CANCELLED",
                "Stopped watching. The workflow goes on on GitHub.",
            )
            .into());
        }
        sleep_unless_cancelled(&run.cancel, Duration::from_secs(6)).await;
        if let Ok(fresh) = github::run(token, owner, repo, current.id).await {
            current = fresh;
        }
    }
}

async fn sleep_unless_cancelled(cancel: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(250);
    let mut waited = Duration::ZERO;
    while waited < total && !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(step).await;
        waited += step;
    }
}

/// Finishes a release from what `resume` says is left.
pub(crate) async fn resume(
    entry: Entry,
    resume: Resume,
    cancel: Arc<AtomicBool>,
    on_event: &(dyn Fn(ReleaseEvent) + Send + Sync),
) -> ReleaseOutcome {
    let run = Session { on_event, cancel };
    let mut outcome = ReleaseOutcome::default();
    let Some(token) = secrets::token() else {
        outcome.problem = Some(Problem::new("AUTH", "Connect your GitHub account first."));
        return outcome;
    };
    let remote = match ops::require_remote(&entry.root).await {
        Ok(remote) => remote,
        Err(error) => {
            outcome.problem = Some(problem_of(error));
            return outcome;
        }
    };
    let (owner, repo) = (
        remote.owner.unwrap_or_default(),
        remote.repo.unwrap_or_default(),
    );
    if let (Some(tag), Some(commit)) = (resume.tag.clone(), resume.commit.clone()) {
        let request = ReleaseRequest {
            entry_id: entry.id.clone(),
            version: tag.trim_start_matches(&entry.tag_prefix).to_string(),
            mode: Mode::Local,
            build: false,
            command: None,
            include_changes: false,
            commit_message: None,
            title: tag.clone(),
            notes: String::new(),
            assets: None,
            draft: !resume.publish,
            prerelease: false,
            make_latest: resume.make_latest,
            notes_file: None,
        };
        outcome.tag = tag.clone();
        let assets = resume.pending_assets.iter().map(PathBuf::from).collect();
        return finish_from_tag(
            &run, &entry, &token, &owner, &repo, &request, &tag, &commit, assets, false, outcome,
        )
        .await;
    }
    finish_release(&run, &token, &owner, &repo, resume, outcome).await
}

/// Watches the newest workflow run of `commit` on its own (the page asks
/// again after it was closed).
pub(crate) async fn watch_commit(
    entry: Entry,
    commit: String,
    tag: Option<String>,
    cancel: Arc<AtomicBool>,
    on_event: &(dyn Fn(ReleaseEvent) + Send + Sync),
) -> Result<Run, String> {
    let run = Session { on_event, cancel };
    let token = secrets::token()
        .ok_or_else(|| String::from(Problem::new("AUTH", "Connect your GitHub account first.")))?;
    let remote = ops::require_remote(&entry.root).await?;
    let workflow = match tag.as_deref() {
        Some(tag) => tag_workflow(&entry, tag).await,
        None => None,
    };
    watch(
        &run,
        &token,
        &remote.owner.unwrap_or_default(),
        &remote.repo.unwrap_or_default(),
        &commit,
        tag.as_deref(),
        workflow.as_ref(),
    )
    .await
}

pub(crate) fn stats_of(command: &str, outcome: &BuildOutcome) -> store::BuildStats {
    store::BuildStats {
        command: command.to_string(),
        duration_ms: outcome.duration_ms,
        lines: outcome.lines,
        at: super::unix_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_releases::store::EntryConfig;

    fn entry(sub: Option<&str>, monorepo: bool) -> Entry {
        Entry {
            id: "r".into(),
            repo_id: "r".into(),
            root: PathBuf::from(r"C:\code\desktop-utils"),
            sub: sub.map(String::from),
            dir: PathBuf::from(r"C:\code\desktop-utils"),
            name: sub.unwrap_or("MYLE").into(),
            monorepo,
            tag_prefix: "v".into(),
            config: EntryConfig::default(),
        }
    }

    fn change(path: &str) -> git::FileChange {
        git::FileChange {
            path: path.into(),
            orig_path: None,
            kind: git::ChangeKind::Modified,
            staged: git::Staged::None,
        }
    }

    #[test]
    fn commit_messages_and_the_projects_own_changes() {
        assert_eq!(
            default_commit_message(&entry(None, false), "9.6.0"),
            "Release 9.6.0"
        );
        assert_eq!(
            default_commit_message(&entry(Some("backup_projects"), true), "2.0.7"),
            "backup_projects 2.0.7"
        );

        let context = Context {
            entry: entry(Some("backup_projects"), true),
            remote: None,
            status: git::Status {
                changes: vec![
                    change("backup_projects/package.json"),
                    change("backup_projects/src/a.ts"),
                    change("discord_bot/b.js"),
                ],
                ..git::Status::default()
            },
            versions: versions::Versions {
                files: vec![versions::VersionFile {
                    path: "package.json".into(),
                    kind: versions::FileKind::PackageJson,
                    version: "2.0.6".into(),
                    skipped: false,
                }],
                ..versions::Versions::default()
            },
            workflows: Vec::new(),
        };
        let own: Vec<&str> = own_changes(&context)
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(own, ["backup_projects/src/a.ts"]);
    }

    fn run_of(id: u64, name: &str, path: Option<&str>, branch: &str) -> Run {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "html_url": format!("https://github.com/a/b/actions/runs/{id}"),
            "head_branch": branch,
            "head_sha": "abc",
            "event": "push",
            "path": path,
        }))
        .unwrap()
    }

    #[test]
    fn the_release_workflows_run_is_followed_not_another_on_the_same_tag() {
        // MYLE's tag starts MYLE Passwords (done in seconds) and Release; the
        // API lists the newest first.
        let runs = || {
            vec![
                run_of(3, "CI", Some(".github/workflows/ci.yml"), "main"),
                run_of(
                    2,
                    "MYLE Passwords",
                    Some(".github/workflows/mobile.yml"),
                    "v9.7.2",
                ),
                run_of(
                    1,
                    "Release",
                    Some(".github/workflows/release.yml"),
                    "v9.7.2",
                ),
            ]
        };
        let release = Workflow {
            file: "release.yml".into(),
            name: Some("Release".into()),
            tags: vec!["v*".into()],
        };
        assert_eq!(
            pick_run(runs(), Some("v9.7.2"), Some(&release)).map(|r| r.id),
            Some(1)
        );
        // Without the workflow, the first run of the tag, as before.
        assert_eq!(
            pick_run(runs(), Some("v9.7.2"), None).map(|r| r.id),
            Some(2)
        );
        // Not started yet: keep waiting.
        assert!(pick_run(runs()[..2].to_vec(), Some("v9.7.2"), Some(&release)).is_none());
        // Runs without a path are matched by name.
        let unnamed = vec![
            run_of(5, "MYLE Passwords", None, "v1.0.0"),
            run_of(4, "Release", None, "v1.0.0"),
        ];
        assert_eq!(
            pick_run(unnamed, Some("v1.0.0"), Some(&release)).map(|r| r.id),
            Some(4)
        );
        // A path with a ref.
        let with_ref = vec![run_of(
            6,
            "Release",
            Some(".github/workflows/release.yml@refs/tags/v2.0.0"),
            "v2.0.0",
        )];
        assert_eq!(
            pick_run(with_ref, Some("v2.0.0"), Some(&release)).map(|r| r.id),
            Some(6)
        );
    }

    #[test]
    fn the_live_checks_fetch_now_and_then() {
        let root = std::env::temp_dir().join("myle-gr-check-fetch");
        let other = std::env::temp_dir().join("myle-gr-check-fetch-other");
        let now = Instant::now();
        assert!(check_fetch_due(&root, now));
        assert!(!check_fetch_due(&root, now + Duration::from_secs(5)));
        assert!(check_fetch_due(&other, now + Duration::from_secs(5)));
        assert!(check_fetch_due(
            &root,
            now + CHECK_FETCH_EVERY + Duration::from_secs(1)
        ));
    }
}
