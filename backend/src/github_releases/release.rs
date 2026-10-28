// ! Releasing a project, in an order that never leaves GitHub half done: ! ! 1. checks (branch, not behind GitHub, the tag is free, a clean folder or ! the changes go along, a token, a release workflow in Actions mode); ! 2. the version in every version file; ! 3. the build (local mode), then the files to upload; ! 4. a commit of the version files (and the chosen changes); ! 5. the push; 6. the tag on that exact commit, pushed; ! 7. local mode: a draft release, its files uploaded, then published. ! Actions mode: the workflow the tag started is watched to its end. ! ! A failure before the push puts every file and the branch back as they ! were. After the push nothing is ever rewritten: the outcome says what is ! left (upload the rest, publish the draft) so it can be finished.
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
    /// A branch of the remote to merge into the current one before the
    /// release (its name without the remote: `feature/x`).
    #[serde(default)]
    pub merge_branch: Option<String>,
    /// Delete the merged branch from GitHub and locally after release push.
    #[serde(default)]
    pub delete_merged_branch: bool,
    /// Actions mode: what the workflows build, written into the tag's
    /// message as `Build: windows` or `Build: full` for them to read.
    #[serde(default)]
    pub target: Option<String>,
}
/// Branches that must never be deleted after a release merge.
pub(crate) fn is_protected_branch(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    matches!(lower.as_str(), "main" | "master" | "old")
}
/// A branch name safe to hand to git: no option, no range, no oddities.
pub(crate) fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.ends_with(".lock")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}
/// The tag message's line for the workflows, from the chosen target.
pub(crate) fn target_line(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("windows") => Some("Build: windows"),
        Some("full") => Some("Build: full"),
        _ => None,
    }
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
    /// A workflow run failed (or one of its jobs did): what and where.
    Failure {
        jobs: Vec<FailedJob>,
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
    /// The release as it was asked for, so resuming it is the same release:
    /// built by GitHub Actions or here, its title and notes, and what it builds.
    #[serde(default)]
    pub mode: Option<Mode>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub notes_in_file: bool,
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
                // A tag only on GitHub (a failed release, another PC) would stop the tag push after the release commit is pushed.
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
            // The tag starts the repository's own release workflow anyway, which publishes the same release from GitHub.
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
    // 0. The chosen branch, merged into the default branch first. Until the release commit is on GitHub, any failure puts the branch back where it was (`reset --keep`, which never touches uncommitted work).
    let merging = request.merge_branch.clone().filter(|b| !b.is_empty());
    let mut before_merge = None;
    if let Some(branch) = merging.as_deref() {
        match merge_branch(&run, &entry, branch).await {
            Ok(head) => before_merge = Some(head),
            Err(problem) => {
                outcome.problem = Some(problem);
                return outcome;
            }
        }
    }
    let pushed = AtomicBool::new(false);
    let mut outcome = release_merged(&run, &request, &entry, build_command, &version, &tag, outcome, &pushed).await;
    if let (Some(head), false) = (before_merge.as_deref(), pushed.load(Ordering::Relaxed)) {
        outcome.rolled_back |= undo_merge(&run, &entry, head).await;
    }
    // The merged branch goes only once the release is out: never while its workflow may still fail, and never with commits that are not merged.
    if outcome.ok
        && request.delete_merged_branch
        && let Some(branch) = merging.as_deref()
    {
        delete_merged_branch(&run, &entry, branch).await;
    }
    outcome
}
/// The release after the merge: checks, version files, build, commit, push,
/// tag, release. `pushed` says when the release commit reached GitHub.
#[allow(clippy::too_many_arguments)]
async fn release_merged(
    run: &Session<'_>,
    request: &ReleaseRequest,
    entry: &Entry,
    build_command: Option<String>,
    version: &str,
    tag: &str,
    mut outcome: ReleaseOutcome,
    release_pushed: &AtomicBool,
) -> ReleaseOutcome {
    let on_event = run.on_event;
    let run = Session {
        on_event,
        cancel: run.cancel.clone(),
    };
    let cancel = run.cancel.clone();
    let (request, entry, version, tag) = (request.clone(), entry.clone(), version.to_string(), tag.to_string());
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
        let created = !path.exists();
        let before = std::fs::read(&path).unwrap_or_default();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        edits.push(Edit {
            path,
            rel: rel.clone(),
            before,
            after: format!("{}\n", request.notes.trim()).into_bytes(),
            created,
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
    release_pushed.store(true, Ordering::Relaxed);
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
        mode: Some(request.mode),
        title: request.title.clone(),
        notes: request.notes.clone(),
        prerelease: request.prerelease,
        target: request.target.clone(),
        notes_in_file,
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
        let mut message = if request.title.trim().is_empty() {
            tag.to_string()
        } else {
            request.title.trim().to_string()
        };
        if request.mode == Mode::Actions
            && let Some(line) = target_line(request.target.as_deref())
        {
            message.push_str("\n\n");
            message.push_str(line);
        }
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
    // Runs this commit already has (an earlier attempt of this version on the same commit): never taken for the ones this tag push starts.
    let earlier: Vec<u64> = if request.mode == Mode::Actions {
        github::runs_for_commit(token, owner, repo, commit)
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| r.id)
            .collect()
    } else {
        Vec::new()
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
                    outcome.resume = Some(resume_tag(&assets));
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
                ..resume_tag(&assets)
            };
            outcome.release = Some(release);
            finish_release(run, token, owner, repo, resume, outcome).await
        }
        Mode::Actions => {
            run.step("workflow", StepState::Running, None);
            let workflows = tag_workflows(entry, tag).await;
            let result = watch(run, token, owner, repo, commit, Some(tag), &workflows, &earlier).await;
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
                    // The workflow made the release; give it the notes when they were not handed over in a file.
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
                    // Stopped or lost from sight: the release is not known to be out, so it is never reported as published. The tag is pushed, so "Follow the run" can pick it up again.
                    outcome.ok = false;
                    outcome.cancelled = stopped;
                    outcome.problem = Some(problem);
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
    earlier: &[u64],
) -> Option<Run> {
    // `earlier`: runs this commit had before the tag was pushed (an earlier attempt of the same version), never the ones this push started.
    let mut candidates = runs
        .into_iter()
        .filter(|r| r.event == "push" && !earlier.contains(&r.id))
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
/// The repository's workflows a push of `tag` starts, the release one first.
async fn tag_workflows(entry: &Entry, tag: &str) -> Vec<Workflow> {
    let root = entry.root.clone();
    let Ok(workflows) = super::blocking(move || project::release_workflows(&root)).await else {
        return Vec::new();
    };
    project::workflows_for_tag(&workflows, tag).into_iter().cloned().collect()
}
/// Merges `origin/<branch>` into the checked-out branch, which must be the
/// repository's default branch (main). A conflict is undone (`merge --abort`)
/// and reported with the files in conflict. Answers the commit the branch was
/// on before, for `undo_merge`.
async fn merge_branch(run: &Session<'_>, entry: &Entry, branch: &str) -> Result<String, Problem> {
    run.step("merge", StepState::Running, Some(branch.to_string()));
    let fail = |message: String| {
        run.step("merge", StepState::Failed, Some(message.clone()));
        Problem::new("MERGE_FAILED", message)
    };
    if !valid_branch(branch) {
        return Err(fail(format!("{branch:?} is not a branch name.")));
    }
    let remote = ops::require_remote(&entry.root).await.map_err(fail)?;
    // Into the default branch only: merging a feature into whatever happens to be checked out is never what "merge before release" means.
    let current = git::read(&entry.root, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_default();
    let current = current.trim();
    let default = default_branch(&entry.root, &remote.name).await;
    if current == branch {
        return Err(fail(format!("{branch} is the branch being released; choose the feature branch to merge into it.")));
    }
    if let Some(default) = default.as_deref()
        && current != default
    {
        return Err(fail(format!(
            "The release merges {branch} into {default}, but {current} is checked out. Switch to {default}, then release again."
        )));
    }
    let before = git::read(&entry.root, &["rev-parse", "HEAD"])
        .await
        .map(|head| head.trim().to_string())
        .map_err(|error| fail(format!("The current commit could not be read: {error}")))?;
    match ops::fetch(&entry.root, false, run.cancel.clone()).await {
        Ok(result) if result.ok => {}
        Ok(result) => {
            return Err(fail(
                result.problem.map(|p| p.message).unwrap_or_else(|| "The fetch failed.".into()),
            ))
        }
        Err(error) => return Err(fail(error)),
    }
    let theirs = format!("{}/{branch}", remote.name);
    let message = format!("Merge {branch}");
    let merged = git::run_with(
        &entry.root,
        &["merge", "--no-ff", "--no-edit", "-m", &message, &theirs],
        None,
        &git::GitEnv::default(),
        Duration::from_secs(120),
    )
    .await
    .map_err(fail)?;
    if merged.ok() {
        run.log(merged.all());
        run.step("merge", StepState::Done, Some(format!("{branch} merged")));
        return Ok(before);
    }
    let conflicts = git::read(&entry.root, &["diff", "--name-only", "--diff-filter=U"])
        .await
        .unwrap_or_default();
    let files: Vec<&str> = conflicts.lines().filter(|l| !l.trim().is_empty()).collect();
    let _ = git::run(&entry.root, &["merge", "--abort"]).await;
    run.log(merged.all());
    Err(fail(if files.is_empty() {
        format!("Merging {branch} failed: {}", git::first_error(&merged.all()))
    } else {
        format!(
            "Merging {branch} has conflicts in {}: {}. Nothing was changed; resolve them in the branch and try again.",
            if files.len() == 1 { "1 file".to_string() } else { format!("{} files", files.len()) },
            files.join(", ")
        )
    }))
}
/// The remote's default branch (`main` mostly): what `<remote>/HEAD` names,
/// else `main` or `master` when the repository has it.
async fn default_branch(root: &Path, remote: &str) -> Option<String> {
    if let Ok(head) = git::read(root, &["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")]).await
        && let Some(name) = head.trim().strip_prefix(&format!("{remote}/"))
        && !name.is_empty()
    {
        return Some(name.to_string());
    }
    for name in ["main", "master"] {
        if git::run(root, &["rev-parse", "-q", "--verify", &format!("refs/heads/{name}")]).await.is_ok_and(|o| o.ok()) {
            return Some(name.to_string());
        }
    }
    None
}
/// Takes back a merge the release made, when the release did not reach
/// GitHub: the branch goes back to `before`. `--keep` keeps uncommitted work
/// and refuses rather than overwrite it. Answers whether the branch is back.
async fn undo_merge(run: &Session<'_>, entry: &Entry, before: &str) -> bool {
    let head = git::read(&entry.root, &["rev-parse", "HEAD"]).await.unwrap_or_default();
    if head.trim() == before {
        return true;
    }
    // Only when the merge is still part of the branch (nothing else was done).
    let contains = git::run(&entry.root, &["merge-base", "--is-ancestor", before, "HEAD"]).await.is_ok_and(|o| o.ok());
    if !contains {
        run.log("The merge was not taken back: the branch changed meanwhile.");
        return false;
    }
    match git::run(&entry.root, &["reset", "--keep", before]).await {
        Ok(output) if output.ok() => {
            run.log("The merge was taken back: the branch is as it was before the release.");
            true
        }
        Ok(output) => {
            run.log(format!("The merge could not be taken back ({}). Undo it with: git reset --keep {before}", git::first_error(&output.all())));
            false
        }
        Err(error) => {
            run.log(format!("The merge could not be taken back ({error}). Undo it with: git reset --keep {before}"));
            false
        }
    }
}
/// Deletes a merged feature branch after a release that is out: on GitHub,
/// then here with `branch -d`, which refuses a branch with commits that were
/// never merged (they would be lost).
async fn delete_merged_branch(run: &Session<'_>, entry: &Entry, branch: &str) {
    if is_protected_branch(branch) || !valid_branch(branch) {
        run.log(format!("Branch '{branch}' is protected and will not be deleted."));
        return;
    }
    let Ok(remote) = ops::require_remote(&entry.root).await else {
        return;
    };
    run.log(format!("Cleaning up merged branch '{branch}'…"));
    let on_line = |text: &str, replace: bool| {
        if !replace {
            (run.on_event)(ReleaseEvent::Log { text: text.to_string() });
        }
    };
    match ops::network(&entry.root, &["push", &remote.name, "--delete", branch], &remote, false, "push", run.cancel.clone(), &on_line).await {
        Ok((_, outcome)) if outcome.ok => run.log(format!("Deleted remote branch '{branch}' from {}.", remote.name)),
        Ok((_, outcome)) => {
            if let Some(problem) = outcome.problem {
                run.log(format!("Notice: Could not delete remote branch '{branch}': {}", problem.message));
            }
        }
        Err(error) => run.log(format!("Notice: Could not delete remote branch '{branch}': {error}")),
    }
    let exists = git::run(&entry.root, &["rev-parse", "-q", "--verify", &format!("refs/heads/{branch}")]).await.is_ok_and(|o| o.ok());
    if exists {
        match git::run(&entry.root, &["branch", "-d", branch]).await {
            Ok(output) if output.ok() => run.log(format!("Deleted local branch '{branch}'.")),
            _ => run.log(format!("Kept local branch '{branch}': it has commits that are not merged.")),
        }
    }
    let _ = ops::fetch(&entry.root, true, run.cancel.clone()).await;
}
/// What went wrong in a failed run: per failed job, the failed step, the
/// error annotations and the part of the job's log that shows the error.
async fn failure_report(token: &str, owner: &str, repo: &str, jobs: &[github::Job]) -> Vec<FailedJob> {
    let mut out = Vec::new();
    for job in jobs.iter().filter(|j| matches!(j.conclusion.as_deref(), Some("failure" | "timed_out" | "startup_failure"))) {
        let step = job
            .steps
            .iter()
            .find(|s| s.conclusion.as_deref() == Some("failure"))
            .map(|s| s.name.clone());
        let errors = github::job_annotations(token, owner, repo, job.id).await.unwrap_or_default();
        // A log asked for right as the job ends may not be there yet.
        let mut log = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            if let Ok(text) = github::job_log(token, owner, repo, job.id).await
                && !text.trim().is_empty()
            {
                log = text;
                break;
            }
        }
        out.push(FailedJob {
            job: job.name.clone(),
            step,
            errors,
            log_tail: log_excerpt(&log),
            url: job.html_url.clone(),
        });
        if out.len() >= 4 {
            break;
        }
    }
    out
}
/// The part of a job log that shows why it failed: from the start of the
/// step that failed (or 60 lines before) to the last error, without GitHub's
/// timestamps and group markers, and without the clean-up that follows.
pub(crate) fn log_excerpt(log: &str) -> String {
    let lines: Vec<&str> = log.lines().map(without_stamp).collect();
    let Some(error) = lines.iter().rposition(|line| line.starts_with("##[error]")) else {
        // No error marker: the end of the log, before GitHub's clean-up.
        let end = lines.iter().position(|line| line.trim() == "Post job cleanup.").unwrap_or(lines.len());
        return lines[end.saturating_sub(40)..end].join("\n");
    };
    let floor = error.saturating_sub(60);
    let start = lines[floor..=error]
        .iter()
        .rposition(|line| line.starts_with("##[group]Run "))
        .map_or(floor, |at| floor + at);
    let kept: Vec<&str> = lines[start..=error]
        .iter()
        .filter(|line| !line.starts_with("##[endgroup]"))
        .map(|line| line.strip_prefix("##[group]").unwrap_or(line))
        .collect();
    kept[kept.len().saturating_sub(120)..].join("\n")
}
/// A log line without GitHub's leading timestamp.
fn without_stamp(line: &str) -> &str {
    match line.split_once(' ') {
        Some((stamp, rest)) if stamp.len() >= 20 && stamp.as_bytes()[4] == b'-' && stamp.ends_with('Z') => rest,
        _ => line,
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedJob {
    job: String,
    step: Option<String>,
    errors: Vec<String>,
    log_tail: String,
    url: Option<String>,
}
/// How many GitHub requests in a row may fail before watching gives up and
/// says why (a revoked token, the rate limit, no network).
const WATCH_FAILURES: u32 = 8;
/// How long a release's workflows are followed at most.
const WATCH_LIMIT: Duration = Duration::from_secs(6 * 60 * 60);
/// Keeps count of GitHub requests that failed in a row, waiting longer after
/// each (and as long as GitHub asks for a rate limit).
struct Backoff {
    failures: u32,
    started: Instant,
}
impl Backoff {
    fn new() -> Self {
        Self { failures: 0, started: Instant::now() }
    }
    /// The value, or (after a pause) `None` to try again; the error itself
    /// once too many requests failed in a row.
    async fn check<T>(&mut self, run: &Session<'_>, result: Result<T, String>) -> Result<Option<T>, String> {
        match result {
            Ok(value) => {
                self.failures = 0;
                Ok(Some(value))
            }
            Err(error) => {
                self.failures += 1;
                if error_is_final(&error) || self.failures >= WATCH_FAILURES {
                    return Err(error);
                }
                let wait = if error.contains("RATE_LIMITED") { 60 } else { 5u64 << self.failures.min(4) };
                run.log(format!("GitHub did not answer ({}); trying again in {wait} s.", problem_of(error.clone()).message));
                sleep_unless_cancelled(&run.cancel, Duration::from_secs(wait)).await;
                Ok(None)
            }
        }
    }
    fn out_of_time(&self) -> bool {
        self.started.elapsed() > WATCH_LIMIT
    }
}
fn stopped() -> String {
    Problem::new(
        "CANCELLED",
        "Stopped following the workflow. It goes on on GitHub unless it was cancelled there; Follow the run picks it up again.",
    )
    .into()
}
/// Follows the workflow runs a push of `commit` (and `tag`) started, to their
/// end: the release workflow first (`workflows[0]`), then every other the tag
/// started, so a release is never called done while one of them still runs.
/// Answers the first run that did not succeed, or the release run. Cancel stops
/// watching; it never cancels a run on GitHub (closing MYLE must not stop a
/// release): that is a separate, explicit action.
#[allow(clippy::too_many_arguments)]
async fn watch(
    run: &Session<'_>,
    token: &str,
    owner: &str,
    repo: &str,
    commit: &str,
    tag: Option<&str>,
    workflows: &[Workflow],
    earlier: &[u64],
) -> Result<Run, String> {
    let mut backoff = Backoff::new();
    let first = discover(run, token, owner, repo, commit, tag, workflows.first(), earlier, &mut backoff, true).await?;
    let Some(first) = first else {
        return Err(Problem::new("NO_RUN", "GitHub didn't start a workflow for the tag within 3 minutes. Check the repository's Actions tab.").into());
    };
    let main = follow(run, token, owner, repo, first, &mut backoff).await?;
    if !succeeded(&main) {
        return Ok(main);
    }
    for other in workflows.iter().skip(1) {
        let Some(found) = discover(run, token, owner, repo, commit, tag, Some(other), earlier, &mut backoff, false).await? else {
            run.log(format!("{} did not start for this tag.", other.name.as_deref().unwrap_or(&other.file)));
            continue;
        };
        let done = follow(run, token, owner, repo, found, &mut backoff).await?;
        if !succeeded(&done) {
            return Ok(done);
        }
    }
    Ok(main)
}
fn succeeded(run: &Run) -> bool {
    matches!(run.conclusion.as_deref(), Some("success" | "skipped" | "neutral"))
}
/// Waits for the run of `workflow` that the push started (3 minutes for the
/// release workflow, `main`; less for the others, which start with it).
#[allow(clippy::too_many_arguments)]
async fn discover(
    run: &Session<'_>,
    token: &str,
    owner: &str,
    repo: &str,
    commit: &str,
    tag: Option<&str>,
    workflow: Option<&Workflow>,
    earlier: &[u64],
    backoff: &mut Backoff,
    main: bool,
) -> Result<Option<Run>, String> {
    let started = Instant::now();
    let limit = Duration::from_secs(if main { 180 } else { 60 });
    loop {
        if run.cancelled() {
            return Err(stopped());
        }
        if let Some(runs) = backoff.check(run, github::runs_for_commit(token, owner, repo, commit).await).await?
            && let Some(found) = pick_run(runs, tag, workflow, earlier)
        {
            return Ok(Some(found));
        }
        if started.elapsed() > limit {
            return Ok(None);
        }
        if main {
            (run.on_event)(ReleaseEvent::Workflow { run: None, jobs: Vec::new() });
        }
        sleep_unless_cancelled(&run.cancel, Duration::from_secs(5)).await;
    }
}
/// Follows one run to its end, telling the page each job and step, and what
/// failed as soon as a job fails.
async fn follow(
    run: &Session<'_>,
    token: &str,
    owner: &str,
    repo: &str,
    mut current: Run,
    backoff: &mut Backoff,
) -> Result<Run, String> {
    let mut reported: Vec<u64> = Vec::new();
    let mut waiting_said = false;
    loop {
        let jobs = backoff.check(run, github::run_jobs(token, owner, repo, current.id).await).await?.unwrap_or_default();
        // A job that failed while others still run: told right away.
        let failed_now: Vec<u64> = jobs
            .iter()
            .filter(|j| j.conclusion.as_deref() == Some("failure"))
            .map(|j| j.id)
            .collect();
        if failed_now.iter().any(|id| !reported.contains(id)) && current.status.as_deref() != Some("completed") {
            reported.extend(failed_now.iter().copied());
            let failed = failure_report(token, owner, repo, &jobs).await;
            if !failed.is_empty() {
                (run.on_event)(ReleaseEvent::Failure { jobs: failed });
            }
        }
        if !waiting_said && matches!(current.status.as_deref(), Some("waiting")) {
            waiting_said = true;
            run.log("The workflow waits for an approval on GitHub (a protected environment).");
        }
        (run.on_event)(ReleaseEvent::Workflow {
            run: Some(current.clone()),
            jobs,
        });
        if current.status.as_deref() == Some("completed") {
            if !succeeded(&current) {
                // The jobs once more, so the report is complete; an empty report is never sent over one already shown.
                let fresh = github::run_jobs(token, owner, repo, current.id).await.unwrap_or_default();
                let failed = failure_report(token, owner, repo, &fresh).await;
                if !failed.is_empty() {
                    (run.on_event)(ReleaseEvent::Failure { jobs: failed });
                }
            }
            return Ok(current);
        }
        if run.cancelled() {
            return Err(stopped());
        }
        if backoff.out_of_time() {
            return Err(Problem::new("WATCH_TIMEOUT", "The workflow has been running for 6 hours; MYLE stopped following it. Open it on GitHub.").into());
        }
        sleep_unless_cancelled(&run.cancel, Duration::from_secs(6)).await;
        if let Some(fresh) = backoff.check(run, github::run(token, owner, repo, current.id).await).await? {
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
        let request = resume_request(&entry, &resume, &tag);
        outcome.tag = tag.clone();
        let assets = resume.pending_assets.iter().map(PathBuf::from).collect();
        return finish_from_tag(
            &run, &entry, &token, &owner, &repo, &request, &tag, &commit, assets, resume.notes_in_file, outcome,
        )
        .await;
    }
    finish_release(&run, &token, &owner, &repo, resume, outcome).await
}
/// The release a resume finishes: the one that was asked for (an older page
/// sends no mode: a local release then, as before).
pub(crate) fn resume_request(entry: &Entry, resume: &Resume, tag: &str) -> ReleaseRequest {
    ReleaseRequest {
        entry_id: entry.id.clone(),
        version: tag.trim_start_matches(&entry.tag_prefix).to_string(),
        mode: resume.mode.unwrap_or(Mode::Local),
        build: false,
        command: None,
        include_changes: false,
        commit_message: None,
        title: if resume.title.trim().is_empty() { tag.to_string() } else { resume.title.clone() },
        notes: resume.notes.clone(),
        assets: None,
        draft: !resume.publish,
        prerelease: resume.prerelease,
        make_latest: resume.make_latest,
        notes_file: None,
        merge_branch: None,
        delete_merged_branch: false,
        target: resume.target.clone(),
    }
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
    let workflows = match tag.as_deref() {
        Some(tag) => tag_workflows(&entry, tag).await,
        None => Vec::new(),
    };
    watch(
        &run,
        &token,
        &remote.owner.unwrap_or_default(),
        &remote.repo.unwrap_or_default(),
        &commit,
        tag.as_deref(),
        &workflows,
        &[],
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
    fn branch_names_targets_and_log_tails() {
        assert!(valid_branch("feature/x-1.2"));
        assert!(!valid_branch("--upload-pack=x"));
        assert!(!valid_branch("a..b"));
        assert!(!valid_branch("a b"));
        assert!(is_protected_branch("main"));
        assert!(is_protected_branch("Main"));
        assert!(is_protected_branch("master"));
        assert!(is_protected_branch("old"));
        assert!(is_protected_branch("OLD"));
        assert!(is_protected_branch("Old"));
        assert!(!is_protected_branch("codex/something"));
        assert!(!is_protected_branch("feature/branch"));
        assert_eq!(target_line(Some("windows")), Some("Build: windows"));
        assert_eq!(target_line(Some("nope")), None);
        let log = "2026-10-10T09:00:00.1234567Z one\n2026-10-10T09:00:01.1234567Z two\nthree";
        assert_eq!(log_excerpt(log), "one\ntwo\nthree");
    }
    #[test]
    fn the_release_workflows_run_is_followed_not_another_on_the_same_tag() {
        // MYLE's tag starts MYLE Passwords (done in seconds) and Release; the API lists the newest first.
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
            pick_run(runs(), Some("v9.7.2"), Some(&release), &[]).map(|r| r.id),
            Some(1)
        );
        // Without the workflow, the first run of the tag, as before.
        assert_eq!(
            pick_run(runs(), Some("v9.7.2"), None, &[]).map(|r| r.id),
            Some(2)
        );
        // Not started yet: keep waiting.
        assert!(pick_run(runs()[..2].to_vec(), Some("v9.7.2"), Some(&release), &[]).is_none());
        // Runs without a path are matched by name.
        let unnamed = vec![
            run_of(5, "MYLE Passwords", None, "v1.0.0"),
            run_of(4, "Release", None, "v1.0.0"),
        ];
        assert_eq!(
            pick_run(unnamed, Some("v1.0.0"), Some(&release), &[]).map(|r| r.id),
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
            pick_run(with_ref, Some("v2.0.0"), Some(&release), &[]).map(|r| r.id),
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
    #[test]
    fn the_failure_excerpt_shows_the_failing_step_up_to_its_error() {
        let log = [
            "2026-10-10T09:00:00.0000000Z ##[group]Run npm ci",
            "2026-10-10T09:00:01.0000000Z added 300 packages",
            "2026-10-10T09:00:02.0000000Z ##[endgroup]",
            "2026-10-10T09:00:03.0000000Z ##[group]Run npm run check",
            "2026-10-10T09:00:04.0000000Z src/a.ts(3,1): error TS2304: Cannot find name 'x'.",
            "2026-10-10T09:00:05.0000000Z ##[error]Process completed with exit code 1.",
            "2026-10-10T09:00:06.0000000Z Post job cleanup.",
            "2026-10-10T09:00:07.0000000Z [command]git config --unset-all http.extraheader",
        ]
        .join("\n");
        let excerpt = log_excerpt(&log);
        assert_eq!(
            excerpt,
            "Run npm run check\nsrc/a.ts(3,1): error TS2304: Cannot find name 'x'.\n##[error]Process completed with exit code 1."
        );
        assert!(!excerpt.contains("Post job cleanup"));
        assert!(!excerpt.contains("added 300 packages"));
    }
    #[test]
    fn a_log_without_an_error_marker_ends_before_the_clean_up() {
        let log = "step output\nlast line\nPost job cleanup.\nCleaning up orphan processes";
        assert_eq!(log_excerpt(log), "step output\nlast line");
    }
    #[test]
    fn a_run_from_an_earlier_attempt_on_the_same_commit_is_never_followed() {
        let run = |id: u64| run_of(id, "Release", Some(".github/workflows/release.yml"), "v9.17.1");
        assert_eq!(pick_run(vec![run(1)], Some("v9.17.1"), None, &[1]).map(|r| r.id), None);
        assert_eq!(pick_run(vec![run(2), run(1)], Some("v9.17.1"), None, &[1]).map(|r| r.id), Some(2));
    }
    #[test]
    fn this_repositorys_tag_is_followed_through_release_yml_first() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let workflows = project::release_workflows(&root);
        let started: Vec<&str> = project::workflows_for_tag(&workflows, "v9.17.1").iter().map(|w| w.file.as_str()).collect();
        assert_eq!(started.first(), Some(&"release.yml"), "the Windows release, not MYLE Passwords");
        assert!(started.contains(&"mobile.yml"), "the phone builds are followed too");
        assert_eq!(project::workflow_for_tag(&workflows, "v9.17.1").map(|w| w.file.as_str()), Some("release.yml"));
    }
    #[test]
    fn a_resume_is_the_release_that_was_asked_for() {
        let entry = entry(None, false);
        let resume = Resume {
            tag: Some("v2.0.0".into()),
            commit: Some("abc".into()),
            publish: true,
            mode: Some(Mode::Actions),
            title: "Big release".into(),
            notes: "Notes".into(),
            prerelease: true,
            target: Some("windows".into()),
            ..Resume::default()
        };
        let request = resume_request(&entry, &resume, "v2.0.0");
        assert_eq!(request.mode, Mode::Actions);
        assert_eq!(request.title, "Big release");
        assert_eq!(request.notes, "Notes");
        assert!(request.prerelease);
        assert_eq!(request.target.as_deref(), Some("windows"));
        // What an older page sends back still works (a local release, as before).
        let old: Resume = serde_json::from_str(r#"{"releaseId":null,"pendingAssets":[],"publish":true,"makeLatest":true,"tag":"v2.0.0","commit":"abc"}"#).unwrap();
        assert_eq!(resume_request(&entry, &old, "v2.0.0").mode, Mode::Local);
    }
}
