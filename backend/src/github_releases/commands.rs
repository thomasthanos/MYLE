//! The Tauri commands of the GitHub Releases page. Projects are addressed
//! by id (`<repo id>` or `<repo id>/<app folder>`), files by their path in
//! the repository's status, release files by a path the backend checks;
//! the token and AI keys never come back to the page.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::ai::{self, Answer, ProviderId};
use super::build::artifacts::{self, Artifact};
use super::build::detect::{self, BuildPlan};
use super::build::runner::{self, BuildEvent, BuildOutcome};
use super::entry::{self, Entry};
use super::git::{self, BranchInfo, Remote};
use super::github::{self, Release, Run};
use super::project::{self, BuildKind, LastTag, Workflow};
use super::release::{self, Check, Mode, ReleaseEvent, ReleaseOutcome, ReleaseRequest, Resume};
use super::state::{GithubReleasesState, Job};
use super::store::{self, Account, GbrImport, Settings};
use super::versions::{self, Bump, Versions};
use super::{Problem, blocking, gbr_import, ops, secrets};
use crate::project_backups::compare::{self, FileDiff};

const SCAN_KEY: &str = "__scan__";

// ─── Page state ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoBrief {
    pub id: String,
    pub path: String,
    pub name: String,
    pub exists: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub def: ai::ProviderDef,
    pub model: String,
    pub ready: bool,
    pub key_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiView {
    pub providers: Vec<ProviderView>,
    pub order: Vec<ProviderId>,
    pub ollama_url: Option<String>,
    pub ollama_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    pub repos: Vec<RepoBrief>,
    pub account: Option<Account>,
    pub git: Option<String>,
    pub gh: Option<String>,
    pub ai: AiView,
    /// Shown until dismissed: what the Github-Build-Release import did.
    pub gbr_import: Option<GbrImport>,
}

fn briefs(settings: &Settings) -> Vec<RepoBrief> {
    settings
        .repos
        .iter()
        .map(|repo| {
            let path = PathBuf::from(&repo.path);
            RepoBrief {
                id: repo.id.clone(),
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| repo.path.clone()),
                exists: path.is_dir(),
                path: repo.path.clone(),
            }
        })
        .collect()
}

fn ai_view(settings: &Settings) -> AiView {
    let providers = ai::PROVIDERS
        .iter()
        .map(|def| {
            let key = if def.needs_key {
                secrets::ai_key(def.id)
            } else {
                None
            };
            ProviderView {
                def: def.clone(),
                model: ai::model(&settings.ai, def.id),
                ready: if def.needs_key {
                    key.is_some()
                } else {
                    settings.ai.ollama_enabled
                },
                key_hint: key.map(|k| secrets::hint(&k)),
            }
        })
        .collect();
    AiView {
        providers,
        order: settings.ai.order.clone(),
        ollama_url: settings.ai.ollama_url.clone(),
        ollama_enabled: settings.ai.ollama_enabled,
    }
}

fn page_state(settings: &Settings) -> PageState {
    PageState {
        repos: briefs(settings),
        account: settings
            .account
            .clone()
            .filter(|_| secrets::token().is_some()),
        git: git::git_exe().map(|p| p.display().to_string()),
        gh: git::gh_exe().map(|p| p.display().to_string()),
        ai: ai_view(settings),
        gbr_import: settings.gbr_import.clone().filter(|i| !i.dismissed),
    }
}

/// The page's data. The first time, Github-Build-Release's last project and
/// DeepSeek key are brought over.
#[tauri::command]
pub async fn github_releases_get_state() -> Result<PageState, String> {
    blocking(|| {
        let settings = store::edit(|settings| {
            if settings.gbr_import.is_none() {
                let report = gbr_import::config_path().and_then(|config| {
                    let has_key = secrets::ai_key(ProviderId::DeepSeek).is_some();
                    gbr_import::import_from(&config, settings, has_key, |key| {
                        secrets::save_ai_key(ProviderId::DeepSeek, key)
                    })
                });
                settings.gbr_import = Some(report.unwrap_or(GbrImport {
                    at: super::unix_millis(),
                    dismissed: true,
                    ..GbrImport::default()
                }));
            }
            Ok(())
        })?;
        Ok(page_state(&settings))
    })
    .await?
}

#[tauri::command]
pub async fn github_releases_dismiss_import() -> Result<(), String> {
    blocking(|| {
        store::edit(|settings| {
            if let Some(import) = settings.gbr_import.as_mut() {
                import.dismissed = true;
            }
            Ok(())
        })
        .map(|_| ())
    })
    .await?
}

// ─── Projects ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryStatus {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub sub: Option<String>,
    pub dir: String,
    pub monorepo: bool,
    pub tag_prefix: String,
    pub versions: Versions,
    pub build_kinds: Vec<BuildKind>,
    pub last_tag: Option<LastTag>,
    /// Uncommitted changes inside the project's folder.
    pub changes: usize,
    /// The workflow a release tag of this project starts.
    pub release_workflow: Option<String>,
    /// `local` or `actions`: saved, or picked by the workflow.
    pub release_mode: String,
    pub skip_version_files: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub repo_id: String,
    pub path: String,
    pub exists: bool,
    pub is_repo: bool,
    pub remote: Option<Remote>,
    pub branch: BranchInfo,
    pub changes: usize,
    pub entries: Vec<EntryStatus>,
    pub workflows: Vec<Workflow>,
    pub problem: Option<String>,
}

fn entry_status(
    entry: &Entry,
    tags: &[git::Tag],
    status: &git::Status,
    workflows: &[Workflow],
) -> EntryStatus {
    let versions = versions::detect(&entry.dir, &entry.config.skip_version_files);
    let probe = project::tag_name(
        &entry.tag_prefix,
        versions.current.as_deref().unwrap_or("1.0.0"),
    );
    let workflow = project::workflow_for_tag(workflows, &probe)
        .map(|w| w.name.clone().unwrap_or_else(|| w.file.clone()));
    let release_mode = entry
        .config
        .release_mode
        .clone()
        .filter(|m| m == "local" || m == "actions")
        .unwrap_or_else(|| {
            if workflow.is_some() {
                "actions".into()
            } else {
                "local".into()
            }
        });
    EntryStatus {
        id: entry.id.clone(),
        repo_id: entry.repo_id.clone(),
        name: entry.name.clone(),
        sub: entry.sub.clone(),
        dir: entry.dir.display().to_string(),
        monorepo: entry.monorepo,
        tag_prefix: entry.tag_prefix.clone(),
        last_tag: project::last_tag(tags, &entry.tag_prefix, versions.current.as_deref()),
        build_kinds: project::build_kinds(&entry.dir),
        changes: status
            .changes
            .iter()
            .filter(|c| entry.contains(&c.path))
            .count(),
        release_workflow: workflow,
        release_mode,
        skip_version_files: entry.config.skip_version_files.clone(),
        versions,
    }
}

async fn repo_status(repo_id: &str) -> Result<RepoStatus, String> {
    let settings = blocking(store::load).await??;
    let repo = settings.repo(repo_id)?.clone();
    let path = PathBuf::from(&repo.path);
    let mut out = RepoStatus {
        repo_id: repo.id.clone(),
        path: repo.path.clone(),
        exists: path.is_dir(),
        is_repo: project::is_repo(&path),
        remote: None,
        branch: BranchInfo::default(),
        changes: 0,
        entries: Vec::new(),
        workflows: Vec::new(),
        problem: None,
    };
    if !out.exists {
        out.problem = Some(
            "The folder was not found (moved, renamed, or on a drive that isn't connected).".into(),
        );
        return Ok(out);
    }
    if !out.is_repo {
        out.problem = Some("This folder is no longer a git repository.".into());
        return Ok(out);
    }
    if git::git_exe().is_none() {
        out.problem = Some(Problem::new("NO_GIT", "Git is not installed on this PC.").into());
        return Ok(out);
    }
    let (status, remote, tags) =
        tokio::join!(git::status(&path), git::remote(&path), git::tags(&path));
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            out.problem = Some(error);
            return Ok(out);
        }
    };
    out.remote = remote.unwrap_or(None);
    let tags = tags.unwrap_or_default();
    let (entries, workflows) = blocking({
        let status = status.clone();
        move || {
            let workflows = project::release_workflows(&path);
            let entries: Vec<EntryStatus> = entry::entries_of(&settings, &repo)
                .iter()
                .map(|entry| entry_status(entry, &tags, &status, &workflows))
                .collect();
            (entries, workflows)
        }
    })
    .await?;
    out.changes = status.changes.len();
    out.branch = status.branch;
    out.entries = entries;
    out.workflows = workflows;
    Ok(out)
}

#[tauri::command]
pub async fn github_releases_status(repo_id: String) -> Result<RepoStatus, String> {
    repo_status(&repo_id).await
}

/// Brings the remote's branches and tags; then the fresh status.
#[tauri::command]
pub async fn github_releases_fetch(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
    quiet: bool,
) -> Result<RepoStatus, String> {
    let root = repo_root(&repo_id).await?;
    let running = state.begin(&repo_id, Job::Git)?;
    let outcome = ops::fetch(&root, quiet, running.cancel.clone()).await;
    drop(running);
    let mut status = repo_status(&repo_id).await?;
    match outcome {
        Ok(result) if !result.ok && !quiet => {
            return Err(result
                .problem
                .map(String::from)
                .unwrap_or_else(|| "The fetch failed.".into()));
        }
        Err(error) if !quiet => return Err(error),
        Ok(result) if !result.ok => status.problem = result.problem.map(|p| p.message),
        _ => {}
    }
    Ok(status)
}

async fn repo_root(repo_id: &str) -> Result<PathBuf, String> {
    let settings = blocking(store::load).await??;
    let repo = settings.repo(repo_id)?;
    let path = PathBuf::from(&repo.path);
    if !path.is_dir() {
        return Err("The project folder was not found.".into());
    }
    Ok(path)
}

async fn resolve(entry_id: &str) -> Result<Entry, String> {
    let id = entry_id.to_string();
    blocking(move || {
        let settings = store::load()?;
        let entry = entry::resolve(&settings, &id)?;
        if !entry.dir.is_dir() {
            return Err("The project folder was not found.".to_string());
        }
        Ok(entry)
    })
    .await?
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundRepo {
    pub path: String,
    pub name: String,
    pub known: bool,
    /// App folders, for a monorepo.
    pub apps: Vec<String>,
}

async fn pick_folder(app: AppHandle, title: &str) -> Result<Option<PathBuf>, String> {
    let title = title.to_string();
    blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
            .map(|path| path.into_path().map_err(|error| error.to_string()))
            .transpose()
    })
    .await?
}

fn found(settings: &Settings, paths: Vec<PathBuf>) -> Vec<FoundRepo> {
    paths
        .into_iter()
        .map(|path| {
            let subs = project::sub_projects(&path);
            FoundRepo {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                known: settings
                    .repos
                    .iter()
                    .any(|r| store::same_path(Path::new(&r.path), &path)),
                apps: if subs.len() > 1 { subs } else { Vec::new() },
                path: path.display().to_string(),
            }
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub state: PageState,
    /// The repository that was added (or was already there).
    pub added: Option<String>,
    /// Repositories found inside the chosen folder, when it isn't one.
    pub found: Vec<FoundRepo>,
}

/// Adds a repository: the one the chosen folder is in, or (when it holds
/// several) lists them to pick from.
#[tauri::command]
pub async fn github_releases_add_folder(app: AppHandle) -> Result<Option<AddResult>, String> {
    let Some(folder) = pick_folder(app, "Choose a project folder (a git repository)").await? else {
        return Ok(None);
    };
    blocking(move || {
        let mut current = Some(folder.as_path());
        while let Some(dir) = current {
            if project::is_repo(dir) {
                let dir = dir.to_path_buf();
                let mut added = None;
                let settings = store::edit(|settings| {
                    added = Some(settings.add_repo(&dir)?);
                    Ok(())
                })?;
                return Ok(Some(AddResult { state: page_state(&settings), added, found: Vec::new() }));
            }
            current = dir.parent();
        }
        let settings = store::load()?;
        let repos = project::find_repos(&folder, &|| false);
        if repos.is_empty() {
            return Err(Problem::new(
                "NOT_A_REPO",
                "That folder isn't a git repository and has none inside. Run \"git init\" in it, or pick the folder of a cloned repository.",
            )
            .into());
        }
        Ok(Some(AddResult { state: page_state(&settings), added: None, found: found(&settings, repos) }))
    })
    .await?
}

/// Looks for repositories in a folder (up to four levels down).
#[tauri::command]
pub async fn github_releases_scan_folder(
    app: AppHandle,
    state: State<'_, GithubReleasesState>,
) -> Result<Option<Vec<FoundRepo>>, String> {
    let Some(folder) = pick_folder(app, "Choose a folder to search for git repositories").await?
    else {
        return Ok(None);
    };
    let running = state.begin(SCAN_KEY, Job::Scan)?;
    let cancel = running.cancel.clone();
    let result = blocking(move || {
        let settings = store::load()?;
        let repos = project::find_repos(&folder, &|| {
            cancel.load(std::sync::atomic::Ordering::Relaxed)
        });
        Ok::<_, String>(found(&settings, repos))
    })
    .await?;
    drop(running);
    result.map(Some)
}

#[tauri::command]
pub async fn github_releases_add_repos(paths: Vec<String>) -> Result<PageState, String> {
    blocking(move || {
        let settings = store::edit(|settings| {
            for path in &paths {
                let path = PathBuf::from(path.trim());
                if !path.is_absolute() || !project::is_repo(&path) {
                    return Err(format!("{} is not a git repository.", path.display()));
                }
                settings.add_repo(&path)?;
            }
            Ok(())
        })?;
        Ok(page_state(&settings))
    })
    .await?
}

#[tauri::command]
pub async fn github_releases_remove_repo(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
) -> Result<PageState, String> {
    if state.job(&repo_id).is_some() {
        return Err("Wait for what runs on this project to finish.".into());
    }
    blocking(move || {
        let settings = store::edit(|settings| {
            settings.remove_repo(&repo_id);
            Ok(())
        })?;
        Ok(page_state(&settings))
    })
    .await?
}

/// A field that may be missing (left alone), `null` (cleared) or a value.
fn double<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySettings {
    pub skip_version_files: Option<Vec<String>>,
    pub release_mode: Option<String>,
    #[serde(default, deserialize_with = "double")]
    pub build_command: Option<Option<String>>,
    #[serde(default, deserialize_with = "double")]
    pub build_choice: Option<Option<String>>,
}

#[tauri::command]
pub async fn github_releases_set_entry(
    entry_id: String,
    changes: EntrySettings,
) -> Result<(), String> {
    let entry = resolve(&entry_id).await?;
    blocking(move || {
        store::edit(|settings| {
            let config = settings.entries.entry(entry.id.clone()).or_default();
            if let Some(skip) = changes.skip_version_files {
                config.skip_version_files = skip
                    .into_iter()
                    .filter(|s| !s.contains(".."))
                    .take(50)
                    .collect();
            }
            if let Some(mode) = changes.release_mode {
                config.release_mode = (mode == "local" || mode == "actions").then_some(mode);
            }
            if let Some(command) = changes.build_command {
                config.build_command = command.map(|c| detect::check_command(&c)).transpose()?;
            }
            if let Some(choice) = changes.build_choice {
                config.build_choice = choice;
            }
            Ok(())
        })
        .map(|_| ())
    })
    .await?
}

// ─── GitHub data ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub releases: Vec<Release>,
    /// The newest published release of each project (by id).
    pub last_release: BTreeMap<String, Release>,
    /// The newest Actions run of the checked-out branch.
    pub run: Option<Run>,
    pub problem: Option<Problem>,
}

fn problem_of(error: String) -> Problem {
    serde_json::from_str::<serde_json::Value>(&error)
        .ok()
        .and_then(|v| {
            Some(Problem::new(
                v.get("code")?.as_str()?,
                v.get("message")?.as_str()?,
            ))
        })
        .unwrap_or_else(|| Problem::new("FAILED", error))
}

/// The newest release whose tag belongs to `entry` (its own prefix, else a
/// shared `v…` tag of its major version not ahead of it).
fn last_release_of<'a>(
    releases: &'a [Release],
    prefix: &str,
    current: Option<&str>,
) -> Option<&'a Release> {
    let published: Vec<&Release> = releases.iter().filter(|r| !r.draft).collect();
    let tags: Vec<git::Tag> = published
        .iter()
        .map(|r| git::Tag {
            name: r.tag_name.clone(),
            date: 0,
        })
        .collect();
    let last = project::last_tag(&tags, prefix, current)?;
    published.into_iter().find(|r| r.tag_name == last.name)
}

#[tauri::command]
pub async fn github_releases_remote(repo_id: String) -> Result<RemoteInfo, String> {
    let status = repo_status(&repo_id).await?;
    let mut info = RemoteInfo {
        releases: Vec::new(),
        last_release: BTreeMap::new(),
        run: None,
        problem: None,
    };
    let Some(remote) = status.remote.clone() else {
        return Ok(info);
    };
    let (Some(owner), Some(repo)) = (remote.owner, remote.repo) else {
        return Ok(info);
    };
    let Some(token) = secrets::token() else {
        info.problem = Some(Problem::new(
            "AUTH",
            "Connect your GitHub account to see releases and Actions runs.",
        ));
        return Ok(info);
    };
    let branch = status.branch.branch.clone();
    let (releases, runs) = tokio::join!(
        github::releases(&token, &owner, &repo, 60),
        github::runs(&token, &owner, &repo, branch.as_deref(), 1)
    );
    match releases {
        Ok(releases) => info.releases = releases,
        Err(error) => info.problem = Some(problem_of(error)),
    }
    info.run = runs.ok().and_then(|runs| runs.into_iter().next());
    for entry in &status.entries {
        if let Some(release) = last_release_of(
            &info.releases,
            &entry.tag_prefix,
            entry.versions.current.as_deref(),
        ) {
            info.last_release.insert(entry.id.clone(), release.clone());
        }
    }
    Ok(info)
}

// ─── Changes ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum GitEvent {
    Line { text: String, replace: bool },
}

#[tauri::command]
pub async fn github_releases_changes(repo_id: String) -> Result<git::Status, String> {
    git::status(&repo_root(&repo_id).await?).await
}

/// Reads `spec` (`HEAD:path`) from git, up to `limit` bytes.
async fn read_blob(root: &Path, spec: &str, limit: u64) -> Option<Result<Vec<u8>, u64>> {
    let size = git::read(root, &["cat-file", "-s", spec])
        .await
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    if size > limit {
        return Some(Err(size));
    }
    let output = git::run(root, &["cat-file", "blob", spec]).await.ok()?;
    output.ok().then_some(Ok(output.stdout))
}

#[tauri::command]
pub async fn github_releases_file_diff(repo_id: String, path: String) -> Result<FileDiff, String> {
    let root = repo_root(&repo_id).await?;
    let status = git::status(&root).await?;
    let change = status
        .changes
        .iter()
        .find(|c| c.path == path)
        .cloned()
        .ok_or("This file has no changes any more.")?;
    let limit = compare::preview_limit(&path);
    let old = match change.kind {
        git::ChangeKind::Added | git::ChangeKind::Untracked => None,
        _ if status.branch.head.is_none() => None,
        _ => {
            read_blob(
                &root,
                &format!(
                    "HEAD:{}",
                    change.orig_path.as_deref().unwrap_or(&change.path)
                ),
                limit,
            )
            .await
        }
    };
    let new = if change.kind == git::ChangeKind::Deleted {
        None
    } else {
        let file = root.join(change.path.replace('/', "\\"));
        blocking(move || {
            let size = std::fs::metadata(&file).ok()?.len();
            if size > limit {
                return Some(Err(size));
            }
            std::fs::read(&file).ok().map(Ok)
        })
        .await?
    };
    let path_for = path.clone();
    blocking(move || compare::bytes_diff(&path_for, old, new)).await
}

/// Stages (or unstages) `paths`, which must be changed files.
#[tauri::command]
pub async fn github_releases_stage(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
    paths: Vec<String>,
    stage: bool,
) -> Result<git::Status, String> {
    let root = repo_root(&repo_id).await?;
    let _running = state.begin(&repo_id, Job::Git)?;
    let status = git::status(&root).await?;
    let mut targets: Vec<String> = Vec::new();
    for path in &paths {
        let change = status
            .changes
            .iter()
            .find(|c| &c.path == path)
            .ok_or_else(|| format!("{path} has no changes any more."))?;
        targets.push(change.path.clone());
        if let Some(orig) = &change.orig_path {
            targets.push(orig.clone());
        }
    }
    if targets.is_empty() {
        return Ok(status);
    }
    for chunk in targets.chunks(100) {
        let mut args: Vec<&str> = if stage {
            vec!["--literal-pathspecs", "add", "-A", "--"]
        } else if status.branch.head.is_some() {
            vec!["--literal-pathspecs", "reset", "-q", "--"]
        } else {
            vec![
                "--literal-pathspecs",
                "rm",
                "--cached",
                "-r",
                "-q",
                "--ignore-unmatch",
                "--",
            ]
        };
        args.extend(chunk.iter().map(String::as_str));
        let output = git::run(&root, &args).await?;
        if !output.ok() {
            return Err(git::first_error(&output.all()));
        }
    }
    git::status(&root).await
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutcome {
    pub commit: Option<String>,
    pub problem: Option<Problem>,
    pub push: Option<ops::NetOutcome>,
    pub status: git::Status,
}

fn line_sink(channel: &Channel<GitEvent>) -> impl Fn(&str, bool) + Send + Sync + '_ {
    move |text: &str, replace: bool| {
        let _ = channel.send(GitEvent::Line {
            text: text.to_string(),
            replace,
        });
    }
}

/// Commits what is staged; with `push`, pushes it right after.
#[tauri::command]
pub async fn github_releases_commit(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
    message: String,
    push: bool,
    on_event: Channel<GitEvent>,
) -> Result<CommitOutcome, String> {
    let root = repo_root(&repo_id).await?;
    let running = state.begin(&repo_id, Job::Git)?;
    let committed = ops::commit(&root, &message, None).await;
    let mut outcome = CommitOutcome {
        commit: None,
        problem: None,
        push: None,
        status: git::Status::default(),
    };
    match committed {
        Ok(sha) => {
            outcome.commit = Some(sha);
            if push {
                let sink = line_sink(&on_event);
                outcome.push = Some(
                    match ops::push(&root, running.cancel.clone(), &sink).await {
                        Ok(result) => result,
                        Err(error) => ops::NetOutcome {
                            problem: Some(problem_of(error)),
                            ..ops::NetOutcome::default()
                        },
                    },
                );
            }
        }
        Err(problem) => outcome.problem = Some(problem),
    }
    drop(running);
    outcome.status = git::status(&root).await.unwrap_or_default();
    Ok(outcome)
}

#[tauri::command]
pub async fn github_releases_push(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
    on_event: Channel<GitEvent>,
) -> Result<ops::NetOutcome, String> {
    let root = repo_root(&repo_id).await?;
    let running = state.begin(&repo_id, Job::Git)?;
    let sink = line_sink(&on_event);
    ops::push(&root, running.cancel.clone(), &sink).await
}

#[tauri::command]
pub async fn github_releases_pull(
    state: State<'_, GithubReleasesState>,
    repo_id: String,
    on_event: Channel<GitEvent>,
) -> Result<ops::NetOutcome, String> {
    let root = repo_root(&repo_id).await?;
    let running = state.begin(&repo_id, Job::Git)?;
    let sink = line_sink(&on_event);
    ops::pull(&root, running.cancel.clone(), &sink).await
}

#[tauri::command]
pub fn github_releases_cancel(state: State<'_, GithubReleasesState>, repo_id: String) -> bool {
    state.cancel(&repo_id)
}

#[tauri::command]
pub fn github_releases_cancel_scan(state: State<'_, GithubReleasesState>) -> bool {
    state.cancel(SCAN_KEY)
}

const LOCK_EXCLUDES: &[&str] = &[
    ":(exclude)package-lock.json",
    ":(exclude)**/package-lock.json",
    ":(exclude)*.lock",
    ":(exclude)**/*.lock",
    ":(exclude)pnpm-lock.yaml",
    ":(exclude)**/pnpm-lock.yaml",
    ":(exclude)*.min.js",
    ":(exclude)*.map",
];

/// A commit message for what is staged (everything changed when nothing is).
#[tauri::command]
pub async fn github_releases_ai_commit_message(
    repo_id: String,
    provider: Option<ProviderId>,
) -> Result<Answer, String> {
    let root = repo_root(&repo_id).await?;
    let status = git::status(&root).await?;
    let staged = status.changes.iter().any(|c| c.staged != git::Staged::None);
    let base: Vec<&str> = if staged {
        vec!["diff", "--cached"]
    } else if status.branch.head.is_some() {
        vec!["diff", "HEAD"]
    } else {
        vec!["diff", "--cached"]
    };
    let mut stat_args = base.clone();
    stat_args.push("--stat=120");
    let mut diff_args = base.clone();
    diff_args.extend(["--no-color", "-U2", "--", "."]);
    diff_args.extend(LOCK_EXCLUDES);
    let stat = git::read(&root, &stat_args).await.unwrap_or_default();
    let mut diff = git::read(&root, &diff_args).await.unwrap_or_default();
    if !staged {
        let untracked: Vec<&str> = status
            .changes
            .iter()
            .filter(|c| c.kind == git::ChangeKind::Untracked)
            .map(|c| c.path.as_str())
            .take(50)
            .collect();
        if !untracked.is_empty() {
            diff.push_str("\nNew files:\n");
            diff.push_str(&untracked.join("\n"));
        }
    }
    if stat.trim().is_empty() && diff.trim().is_empty() {
        return Err(Problem::new("NOTHING_TO_COMMIT", "There are no changes to describe.").into());
    }
    let recent: Vec<String> = git::log(&root, "HEAD", None, 10)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.subject)
        .collect();
    let settings = blocking(store::load).await??;
    let prompt = ai::commit_prompt(&stat, &diff, &recent);
    ai::complete(&settings.ai, provider, ai::COMMIT_SYSTEM, &prompt, 1500).await
}

// ─── Building ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub plan: BuildPlan,
    /// The command a build runs now: the typed one, the chosen one, or the
    /// first detected one.
    pub command: Option<String>,
    pub custom_command: Option<String>,
    pub choice: Option<String>,
    pub last_build: Option<store::BuildStats>,
    pub running: bool,
}

fn chosen_command(entry: &Entry, plan: &BuildPlan) -> Option<String> {
    entry.config.build_command.clone().or_else(|| {
        entry
            .config
            .build_choice
            .as_ref()
            .and_then(|id| plan.options.iter().find(|o| &o.id == id))
            .or(plan.options.first())
            .map(|o| o.command.clone())
    })
}

#[tauri::command]
pub async fn github_releases_build_info(
    state: State<'_, GithubReleasesState>,
    entry_id: String,
) -> Result<BuildInfo, String> {
    let entry = resolve(&entry_id).await?;
    let running = matches!(state.job(&entry.repo_id), Some(Job::Build | Job::Release));
    blocking(move || {
        let plan = detect::plan(&entry.dir, &entry.root);
        BuildInfo {
            command: chosen_command(&entry, &plan),
            custom_command: entry.config.build_command.clone(),
            choice: entry.config.build_choice.clone(),
            last_build: entry.config.last_build.clone(),
            running,
            plan,
        }
    })
    .await
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub outcome: BuildOutcome,
    pub artifacts: Vec<Artifact>,
    /// The artifacts to tick for a release.
    pub selected: Vec<String>,
    /// Unix milliseconds; artifacts newer than this belong to this build.
    pub started_at: i64,
}

/// Builds the project with `command` (`install`: installs its dependencies).
#[tauri::command]
pub async fn github_releases_build(
    state: State<'_, GithubReleasesState>,
    entry_id: String,
    command: String,
    install: bool,
    on_event: Channel<BuildEvent>,
) -> Result<BuildResult, String> {
    let entry = resolve(&entry_id).await?;
    let command = detect::check_command(&command)?;
    let running = state.begin(&entry.repo_id, Job::Build)?;
    let started = SystemTime::now();
    let outcome = runner::run(
        runner::Request {
            dir: &entry.dir,
            command: &command,
            last: if install {
                None
            } else {
                entry
                    .config
                    .last_build
                    .clone()
                    .filter(|b| b.command == command)
            },
            log_path: super::build_log_path(&entry.id),
        },
        running.cancel.clone(),
        |event| {
            let _ = on_event.send(event);
        },
    )
    .await;
    drop(running);
    if outcome.ok && !install {
        let (id, command, result) = (entry.id.clone(), command.clone(), outcome.clone());
        let _ = blocking(move || super::remember_build(&id, &command, &result)).await;
    }
    let dir = entry.dir.clone();
    let found = if install {
        Vec::new()
    } else {
        blocking(move || artifacts::find(&dir, started)).await?
    };
    Ok(BuildResult {
        selected: artifacts::preselect(&found),
        artifacts: found,
        outcome,
        started_at: started
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    })
}

/// The files the last build made (newer than `since`, Unix ms).
#[tauri::command]
pub async fn github_releases_artifacts(
    entry_id: String,
    since: i64,
) -> Result<Vec<Artifact>, String> {
    let entry = resolve(&entry_id).await?;
    let since = SystemTime::UNIX_EPOCH + Duration::from_millis(since.max(0) as u64);
    blocking(move || artifacts::find(&entry.dir, since)).await
}

/// The SHA-256 of a file the build made, to paste into release notes.
#[tauri::command]
pub async fn github_releases_checksum(entry_id: String, path: String) -> Result<String, String> {
    let entry = resolve(&entry_id).await?;
    blocking(move || {
        let canonical = PathBuf::from(&path)
            .canonicalize()
            .map_err(|_| "That file is no longer there.".to_string())?;
        let root = entry.root.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(root) || !canonical.is_file() {
            return Err("That file is not part of this project.".to_string());
        }
        artifacts::sha256(&canonical)
    })
    .await?
}

/// Opens `file` (as a build printed it) at `line`:`column` in VS Code (or
/// Cursor), or with the default app when neither is installed.
#[tauri::command]
pub async fn github_releases_open_in_editor(
    entry_id: String,
    file: String,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<(), String> {
    let entry = resolve(&entry_id).await?;
    let path = blocking(move || -> Result<PathBuf, String> {
        let file = file.trim().trim_matches('"');
        let candidate = PathBuf::from(file);
        let candidates: Vec<PathBuf> = if candidate.is_absolute() {
            vec![candidate]
        } else {
            let mut list = vec![entry.dir.join(file), entry.root.join(file)];
            if let Some(tauri) = project::tauri_dir(&entry.dir) {
                list.push(tauri.join(file));
            }
            list
        };
        let root = entry.root.canonicalize().map_err(|e| e.to_string())?;
        candidates
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .find(|p| p.is_file() && p.starts_with(&root))
            .ok_or_else(|| format!("{file} was not found in the project."))
    })
    .await??;
    let display = path.display().to_string();
    let display = display
        .strip_prefix(r"\\?\")
        .unwrap_or(&display)
        .replace('\\', "/");
    let position = format!("{}:{}", line.unwrap_or(1), column.unwrap_or(1));
    for scheme in editor_schemes() {
        let url = format!("{scheme}://file/{}:{position}", encode_path(&display));
        if tauri_plugin_opener::open_url(&url, None::<&str>).is_ok() {
            return Ok(());
        }
    }
    tauri_plugin_opener::open_path(&display, None::<&str>).map_err(|error| error.to_string())
}

/// `vscode`, `cursor`: the editors whose link handler is registered.
fn editor_schemes() -> Vec<&'static str> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    ["vscode", "cursor"]
        .into_iter()
        .filter(|scheme| {
            RegKey::predef(HKEY_CLASSES_ROOT)
                .open_subkey(scheme)
                .is_ok()
                || RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey(format!(r"Software\Classes\{scheme}"))
                    .is_ok()
        })
        .collect()
}

fn encode_path(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            '?' => "%3F".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Shows a file (an artifact, a log) in Explorer, or opens a folder.
#[tauri::command]
pub async fn github_releases_reveal(entry_id: String, path: Option<String>) -> Result<(), String> {
    let entry = resolve(&entry_id).await?;
    let target = match path {
        None => entry.dir.clone(),
        Some(path) => {
            let path = PathBuf::from(path);
            let logs = crate::storage::local_dir()?
                .join("github-releases")
                .join("logs");
            let canonical = path
                .canonicalize()
                .map_err(|_| "That file is no longer there.".to_string())?;
            let allowed = [entry.root.canonicalize().ok(), logs.canonicalize().ok()]
                .into_iter()
                .flatten()
                .any(|base| canonical.starts_with(base));
            if !allowed {
                return Err("That file is not part of this project.".into());
            }
            path
        }
    };
    if target.is_file() {
        tauri_plugin_opener::reveal_item_in_dir(target).map_err(|error| error.to_string())
    } else {
        tauri_plugin_opener::open_path(target.display().to_string(), None::<&str>)
            .map_err(|error| error.to_string())
    }
}

// ─── Releasing ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub versions: Versions,
    /// The next version for each bump.
    pub next: BTreeMap<String, String>,
    pub tag_prefix: String,
    pub last_tag: Option<LastTag>,
    pub commits: Vec<git::Commit>,
    pub workflows: Vec<Workflow>,
    pub mode: String,
    /// A folder the release workflow reads notes from (`docs/release-notes`).
    pub notes_dir: Option<String>,
    pub branch: Option<String>,
    pub build_command: Option<String>,
    /// The steps of that command, when it is a detected one.
    pub build_steps: Vec<String>,
    /// The project's whole build, when the chosen command is not it.
    pub full_build: Option<String>,
}

/// The folder a release workflow reads its notes from: the first
/// `…/release-notes/` path in it (`docs/release-notes`), in code or a comment.
fn notes_dir_of(workflow: &str) -> Option<String> {
    let path_char =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\');
    workflow
        .match_indices("release-notes/")
        .find_map(|(at, _)| {
            let start = workflow[..at]
                .char_indices()
                .rev()
                .find(|(_, c)| !path_char(*c))
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            let dir = workflow[start..at + "release-notes".len()]
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string();
            let ok = !dir.starts_with('/')
                && !dir.split('/').any(|part| part.is_empty() || part == "..")
                && workflow[at + "release-notes/".len()..].starts_with(|c: char| {
                    c == '$' || c == '<' || c == '{' || c.is_ascii_alphanumeric()
                });
            ok.then_some(dir)
        })
}

#[tauri::command]
pub async fn github_releases_release_info(entry_id: String) -> Result<ReleaseInfo, String> {
    let entry = resolve(&entry_id).await?;
    let tags = git::tags(&entry.root).await.unwrap_or_default();
    let status = git::status(&entry.root).await?;
    let entry_for = entry.clone();
    let (versions, workflows, plan) = blocking(move || {
        (
            versions::detect(&entry_for.dir, &entry_for.config.skip_version_files),
            project::release_workflows(&entry_for.root),
            detect::plan(&entry_for.dir, &entry_for.root),
        )
    })
    .await?;
    let last_tag = project::last_tag(&tags, &entry.tag_prefix, versions.current.as_deref());
    let range = match &last_tag {
        Some(tag) => format!("{}..HEAD", tag.name),
        None => "HEAD".into(),
    };
    let commits = if status.branch.head.is_some() {
        git::log(&entry.root, &range, entry.sub.as_deref(), 200)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut next = BTreeMap::new();
    let current = versions.current.clone().unwrap_or_else(|| "0.0.0".into());
    for (name, bump) in [
        ("patch", Bump::Patch),
        ("minor", Bump::Minor),
        ("major", Bump::Major),
    ] {
        if let Ok(version) = versions::bump(&current, bump) {
            next.insert(name.to_string(), version);
        }
    }
    let probe = project::tag_name(
        &entry.tag_prefix,
        next.get("patch").map(String::as_str).unwrap_or("1.0.0"),
    );
    let workflow = project::workflow_for_tag(&workflows, &probe);
    let notes_dir = workflow.and_then(|w| {
        let text =
            std::fs::read_to_string(entry.root.join(".github").join("workflows").join(&w.file))
                .ok()?;
        notes_dir_of(&text)
    });
    let mode = entry.config.release_mode.clone().unwrap_or_else(|| {
        if workflow.is_some() {
            "actions".into()
        } else {
            "local".into()
        }
    });
    let build_command = chosen_command(&entry, &plan);
    let build_steps = plan
        .options
        .iter()
        .find(|o| Some(&o.command) == build_command.as_ref())
        .map(|o| o.steps.iter().map(|s| s.title.clone()).collect())
        .unwrap_or_default();
    let full_build = plan
        .options
        .iter()
        .find(|o| o.full)
        .map(|o| o.command.clone())
        .filter(|full| Some(full) != build_command.as_ref());
    Ok(ReleaseInfo {
        build_command,
        build_steps,
        full_build,
        versions,
        next,
        tag_prefix: entry.tag_prefix.clone(),
        last_tag,
        commits,
        workflows,
        mode,
        notes_dir,
        branch: status.branch.branch,
    })
}

/// The newest commits of a project and how many of them came after its
/// last release (for a clean working tree's overview).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCommits {
    pub commits: Vec<git::Commit>,
    /// Commits since the last release (counted up to `UNRELEASED_LIMIT`).
    pub unreleased: usize,
    pub last_tag: Option<String>,
}

const RECENT_LIMIT: usize = 30;
const UNRELEASED_LIMIT: usize = 200;

#[tauri::command]
pub async fn github_releases_recent_commits(
    entry_id: String,
    max: Option<usize>,
) -> Result<RecentCommits, String> {
    let entry = resolve(&entry_id).await?;
    let status = git::status(&entry.root).await?;
    if status.branch.head.is_none() {
        return Ok(RecentCommits {
            commits: Vec::new(),
            unreleased: 0,
            last_tag: None,
        });
    }
    let tags = git::tags(&entry.root).await.unwrap_or_default();
    let entry_for = entry.clone();
    let versions =
        blocking(move || versions::detect(&entry_for.dir, &entry_for.config.skip_version_files))
            .await?;
    let last_tag = project::last_tag(&tags, &entry.tag_prefix, versions.current.as_deref());
    let max = max.unwrap_or(10).clamp(1, RECENT_LIMIT);
    let commits = git::log(&entry.root, "HEAD", entry.sub.as_deref(), max).await?;
    let unreleased = match &last_tag {
        Some(tag) => git::log(
            &entry.root,
            &format!("{}..HEAD", tag.name),
            entry.sub.as_deref(),
            UNRELEASED_LIMIT,
        )
        .await
        .map(|c| c.len())
        .unwrap_or(0),
        None => git::log(&entry.root, "HEAD", entry.sub.as_deref(), UNRELEASED_LIMIT)
            .await
            .map(|c| c.len())
            .unwrap_or(0),
    };
    Ok(RecentCommits {
        commits,
        unreleased,
        last_tag: last_tag.map(|tag| tag.name),
    })
}

#[tauri::command]
pub async fn github_releases_preflight(
    entry_id: String,
    version: String,
    mode: Mode,
    include_changes: bool,
    build: bool,
) -> Result<Vec<Check>, String> {
    let entry = resolve(&entry_id).await?;
    let entry_for = entry.clone();
    let plan = blocking(move || detect::plan(&entry_for.dir, &entry_for.root)).await?;
    let has_build = chosen_command(&entry, &plan).is_some();
    let context = release::context_for_checks(entry).await?;
    Ok(release::preflight(&context, &version, mode, include_changes, has_build, build).await)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notes {
    pub title: Option<String>,
    pub notes: String,
    pub provider: ProviderId,
    pub model: String,
}

/// Release notes from the commits since the last release (`polish`: the
/// text the user wrote, tidied).
#[tauri::command]
pub async fn github_releases_ai_notes(
    entry_id: String,
    version: String,
    provider: Option<ProviderId>,
    polish: Option<String>,
) -> Result<Notes, String> {
    let entry = resolve(&entry_id).await?;
    let settings = blocking(store::load).await??;
    let answer = if let Some(text) = polish.filter(|t| !t.trim().is_empty()) {
        ai::complete(&settings.ai, provider, ai::POLISH_SYSTEM, &text, 2500).await?
    } else {
        let tags = git::tags(&entry.root).await.unwrap_or_default();
        let current = versions::detect(&entry.dir, &entry.config.skip_version_files).current;
        let last = project::last_tag(&tags, &entry.tag_prefix, current.as_deref());
        let range = last
            .as_ref()
            .map(|t| format!("{}..HEAD", t.name))
            .unwrap_or_else(|| "HEAD".into());
        let commits: Vec<String> = git::log(&entry.root, &range, entry.sub.as_deref(), 120)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.subject)
            .collect();
        let (stat, diff) = match &last {
            Some(tag) => {
                let pathspec = entry.sub.clone().unwrap_or_else(|| ".".into());
                let range = format!("{}..HEAD", tag.name);
                let mut diff_args = vec![
                    "diff",
                    "--no-color",
                    "-U1",
                    range.as_str(),
                    "--",
                    pathspec.as_str(),
                ];
                diff_args.extend(LOCK_EXCLUDES);
                (
                    git::read(
                        &entry.root,
                        &["diff", "--stat=120", &range, "--", &pathspec],
                    )
                    .await
                    .unwrap_or_default(),
                    git::read(&entry.root, &diff_args).await.unwrap_or_default(),
                )
            }
            None => (String::new(), String::new()),
        };
        if commits.is_empty() && diff.trim().is_empty() {
            return Err(Problem::new(
                "NOTHING_NEW",
                "There are no commits since the last release to describe.",
            )
            .into());
        }
        let prompt = ai::notes_prompt(&entry.name, &version, &commits, &stat, &diff);
        ai::complete(&settings.ai, provider, ai::NOTES_SYSTEM, &prompt, 2500).await?
    };
    let (title, notes) = ai::split_title(&answer.text);
    Ok(Notes {
        title,
        notes,
        provider: answer.provider,
        model: answer.model,
    })
}

#[tauri::command]
pub async fn github_releases_release(
    state: State<'_, GithubReleasesState>,
    request: ReleaseRequest,
    on_event: Channel<ReleaseEvent>,
) -> Result<ReleaseOutcome, String> {
    let entry = resolve(&request.entry_id).await?;
    let running = state.begin(&entry.repo_id, Job::Release)?;
    let entry_for = entry.clone();
    let plan = blocking(move || detect::plan(&entry_for.dir, &entry_for.root)).await?;
    let command = chosen_command(&entry, &plan);
    let sink = |event: ReleaseEvent| {
        let _ = on_event.send(event);
    };
    Ok(release::run(request, entry, command, running.cancel.clone(), &sink).await)
}

#[tauri::command]
pub async fn github_releases_resume(
    state: State<'_, GithubReleasesState>,
    entry_id: String,
    resume: Resume,
    on_event: Channel<ReleaseEvent>,
) -> Result<ReleaseOutcome, String> {
    let entry = resolve(&entry_id).await?;
    let running = state.begin(&entry.repo_id, Job::Release)?;
    if let Some(paths) = Some(&resume.pending_assets) {
        for path in paths {
            let canonical = PathBuf::from(path)
                .canonicalize()
                .map_err(|_| format!("{path} is no longer there."))?;
            if !canonical.starts_with(entry.root.canonicalize().map_err(|e| e.to_string())?) {
                return Err(format!("{path} is not a file of this project."));
            }
        }
    }
    let sink = |event: ReleaseEvent| {
        let _ = on_event.send(event);
    };
    Ok(release::resume(entry, resume, running.cancel.clone(), &sink).await)
}

/// Follows the newest workflow run of `commit` (the page was reopened).
#[tauri::command]
pub async fn github_releases_watch(
    state: State<'_, GithubReleasesState>,
    entry_id: String,
    commit: String,
    tag: Option<String>,
    on_event: Channel<ReleaseEvent>,
) -> Result<Run, String> {
    let entry = resolve(&entry_id).await?;
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) || commit.len() < 7 {
        return Err("Not a commit id.".into());
    }
    let running = state.begin(&entry.repo_id, Job::Release)?;
    let sink = |event: ReleaseEvent| {
        let _ = on_event.send(event);
    };
    release::watch_commit(entry, commit, tag, running.cancel.clone(), &sink).await
}

// ─── Release history ───────────────────────────────────────────────────────

async fn github_of(
    repo_id: &str,
) -> Result<(zeroize::Zeroizing<String>, String, String, PathBuf), String> {
    let root = repo_root(repo_id).await?;
    let remote = ops::require_remote(&root).await?;
    let (Some(owner), Some(repo)) = (remote.owner, remote.repo) else {
        return Err(
            Problem::new("NOT_GITHUB", "This repository's remote is not on GitHub.").into(),
        );
    };
    let token = secrets::token()
        .ok_or_else(|| String::from(Problem::new("AUTH", "Connect your GitHub account first.")))?;
    Ok((token, owner, repo, root))
}

#[tauri::command]
pub async fn github_releases_list_releases(repo_id: String) -> Result<Vec<Release>, String> {
    let (token, owner, repo, _) = github_of(&repo_id).await?;
    github::releases(&token, &owner, &repo, 100).await
}

#[tauri::command]
pub async fn github_releases_update_release(
    repo_id: String,
    release_id: u64,
    title: String,
    notes: String,
    prerelease: bool,
) -> Result<Release, String> {
    let (token, owner, repo, _) = github_of(&repo_id).await?;
    github::update_release(
        &token,
        &owner,
        &repo,
        release_id,
        &json!({ "name": title, "body": notes, "prerelease": prerelease }),
    )
    .await
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub release_id: u64,
    pub tag: String,
    pub ok: bool,
    pub tag_deleted: bool,
    pub error: Option<String>,
}

/// Deletes releases (the page asked first); with `delete_tags`, their tags
/// too, on GitHub and here.
#[tauri::command]
pub async fn github_releases_delete_releases(
    repo_id: String,
    release_ids: Vec<u64>,
    delete_tags: bool,
) -> Result<Vec<Deleted>, String> {
    let (token, owner, repo, root) = github_of(&repo_id).await?;
    let releases = github::releases(&token, &owner, &repo, 100).await?;
    let mut results = Vec::new();
    for id in release_ids {
        let Some(release) = releases.iter().find(|r| r.id == id) else {
            results.push(Deleted {
                release_id: id,
                tag: String::new(),
                ok: false,
                tag_deleted: false,
                error: Some("Not found on GitHub.".into()),
            });
            continue;
        };
        let mut result = Deleted {
            release_id: id,
            tag: release.tag_name.clone(),
            ok: false,
            tag_deleted: false,
            error: None,
        };
        match github::delete_release(&token, &owner, &repo, id).await {
            Ok(()) => {
                result.ok = true;
                if delete_tags {
                    match github::delete_tag(&token, &owner, &repo, &release.tag_name).await {
                        Ok(()) => {
                            result.tag_deleted = true;
                            let _ = git::run(&root, &["tag", "-d", &release.tag_name]).await;
                        }
                        Err(error) => {
                            result.error = Some(format!(
                                "The release was deleted, its tag wasn't: {}",
                                problem_of(error).message
                            ))
                        }
                    }
                }
            }
            Err(error) => result.error = Some(problem_of(error).message),
        }
        results.push(result);
    }
    Ok(results)
}

/// One set of notes from several releases.
#[tauri::command]
pub async fn github_releases_ai_combine(
    repo_id: String,
    release_ids: Vec<u64>,
    provider: Option<ProviderId>,
) -> Result<Notes, String> {
    let (token, owner, repo, _) = github_of(&repo_id).await?;
    let releases = github::releases(&token, &owner, &repo, 100).await?;
    let mut chosen: Vec<&Release> = releases
        .iter()
        .filter(|r| release_ids.contains(&r.id))
        .collect();
    if chosen.len() < 2 {
        return Err("Choose at least two releases to combine.".into());
    }
    chosen.reverse();
    let mut prompt = String::new();
    for release in chosen {
        prompt.push_str(&format!(
            "## {} ({})\n{}\n\n",
            release
                .name
                .clone()
                .unwrap_or_else(|| release.tag_name.clone()),
            release.tag_name,
            release
                .body
                .clone()
                .unwrap_or_default()
                .chars()
                .take(4000)
                .collect::<String>()
        ));
    }
    let settings = blocking(store::load).await??;
    let answer = ai::complete(&settings.ai, provider, ai::COMBINE_SYSTEM, &prompt, 2500).await?;
    let (title, notes) = ai::split_title(&answer.text);
    Ok(Notes {
        title,
        notes,
        provider: answer.provider,
        model: answer.model,
    })
}

// ─── Account ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhStatus {
    pub installed: bool,
    pub signed_in: bool,
}

async fn gh_token() -> Option<zeroize::Zeroizing<String>> {
    let gh = git::gh_exe()?;
    let output = crate::apps::process::hidden(gh)
        .args(["auth", "token", "--hostname", "github.com"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let token = zeroize::Zeroizing::new(String::from_utf8_lossy(&output.stdout).trim().to_string());
    (output.status.success() && !token.is_empty()).then_some(token)
}

#[tauri::command]
pub async fn github_releases_gh_status() -> Result<GhStatus, String> {
    Ok(GhStatus {
        installed: git::gh_exe().is_some(),
        signed_in: gh_token().await.is_some(),
    })
}

async fn connect(token: &str, source: &str) -> Result<PageState, String> {
    let mut account = github::whoami(token).await?;
    account.source = source.into();
    secrets::save_token(token)?;
    blocking(move || {
        let settings = store::edit(|settings| {
            settings.account = Some(account);
            Ok(())
        })?;
        Ok(page_state(&settings))
    })
    .await?
}

/// Uses the GitHub CLI's sign-in: its token is checked and kept sealed.
#[tauri::command]
pub async fn github_releases_connect_gh() -> Result<PageState, String> {
    if git::gh_exe().is_none() {
        return Err(Problem::new("NO_GH", "The GitHub CLI is not installed.").into());
    }
    let token = gh_token().await.ok_or_else(|| {
        String::from(Problem::new(
            "GH_SIGNED_OUT",
            "The GitHub CLI is not signed in yet.",
        ))
    })?;
    connect(&token, "gh").await
}

/// Uses a pasted personal access token.
#[tauri::command]
pub async fn github_releases_connect_token(token: String) -> Result<PageState, String> {
    let token = zeroize::Zeroizing::new(secrets::check_secret(&token, "GitHub token")?);
    connect(&token, "token").await
}

#[tauri::command]
pub async fn github_releases_disconnect() -> Result<PageState, String> {
    secrets::remove_token();
    blocking(|| {
        let settings = store::edit(|settings| {
            settings.account = None;
            Ok(())
        })?;
        Ok(page_state(&settings))
    })
    .await?
}

const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Opens a terminal window where the GitHub CLI signs in (in the browser).
#[tauri::command]
pub async fn github_releases_gh_login() -> Result<(), String> {
    let gh = git::gh_exe()
        .ok_or_else(|| String::from(Problem::new("NO_GH", "The GitHub CLI is not installed.")))?;
    let mut cmd = tokio::process::Command::new(gh);
    cmd.creation_flags(CREATE_NEW_CONSOLE).args([
        "auth",
        "login",
        "--hostname",
        "github.com",
        "--git-protocol",
        "https",
        "--web",
        "--scopes",
        "workflow",
    ]);
    cmd.spawn()
        .map(|_| ())
        .map_err(|error| format!("Can't start the GitHub CLI: {error}"))
}

/// Installs the GitHub CLI or Git with winget, in a window of its own so
/// the user sees it happen (nothing is installed silently).
#[tauri::command]
pub async fn github_releases_install_tool(tool: String) -> Result<(), String> {
    let id = match tool.as_str() {
        "gh" => "GitHub.cli",
        "git" => "Git.Git",
        _ => return Err("Unknown tool.".into()),
    };
    let winget = git::find_program("winget.exe", &[])
        .ok_or("winget (App Installer) is not available on this PC.")?;
    let mut cmd = tokio::process::Command::new(winget);
    cmd.creation_flags(CREATE_NEW_CONSOLE).args([
        "install",
        "--id",
        id,
        "--exact",
        "--source",
        "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
    ]);
    cmd.spawn()
        .map(|_| ())
        .map_err(|error| format!("Can't start winget: {error}"))
}

// ─── AI settings ───────────────────────────────────────────────────────────

/// Checks a key with its provider and keeps it sealed.
#[tauri::command]
pub async fn github_releases_ai_set_key(
    provider: ProviderId,
    key: String,
) -> Result<AiView, String> {
    if !provider.def().needs_key {
        return Err("This provider needs no key.".into());
    }
    let key = zeroize::Zeroizing::new(secrets::check_secret(&key, "key")?);
    let settings = blocking(store::load).await??;
    ai::check_key(&settings.ai, provider, Some(&key)).await?;
    secrets::save_ai_key(provider, &key)?;
    Ok(ai_view(&settings))
}

#[tauri::command]
pub async fn github_releases_ai_remove_key(provider: ProviderId) -> Result<AiView, String> {
    secrets::remove_ai_key(provider);
    let settings = blocking(store::load).await??;
    Ok(ai_view(&settings))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChanges {
    pub order: Option<Vec<ProviderId>>,
    pub models: Option<BTreeMap<ProviderId, String>>,
    #[serde(default, deserialize_with = "double")]
    pub ollama_url: Option<Option<String>>,
    pub ollama_enabled: Option<bool>,
}

#[tauri::command]
pub async fn github_releases_ai_set_settings(changes: AiChanges) -> Result<AiView, String> {
    if let Some(Some(url)) = &changes.ollama_url
        && !url.trim().is_empty()
        && !(url.trim().starts_with("http://") || url.trim().starts_with("https://"))
    {
        return Err(
            "Ollama's address starts with http:// (for example http://localhost:11434).".into(),
        );
    }
    blocking(move || {
        let settings = store::edit(|settings| {
            if let Some(order) = changes.order {
                let mut clean: Vec<ProviderId> = Vec::new();
                for provider in order.into_iter().chain(ProviderId::ALL) {
                    if !clean.contains(&provider) {
                        clean.push(provider);
                    }
                }
                settings.ai.order = clean;
            }
            if let Some(models) = changes.models {
                settings.ai.models = models
                    .into_iter()
                    .map(|(k, v)| (k, v.trim().chars().take(120).collect::<String>()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect();
            }
            if let Some(url) = changes.ollama_url {
                settings.ai.ollama_url =
                    url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
            }
            if let Some(enabled) = changes.ollama_enabled {
                settings.ai.ollama_enabled = enabled;
            }
            Ok(())
        })?;
        Ok(ai_view(&settings))
    })
    .await?
}

#[tauri::command]
pub async fn github_releases_ai_ollama_models() -> Result<Vec<String>, String> {
    let settings = blocking(store::load).await??;
    ai::ollama_models(&settings.ai).await
}

/// Tries a provider with a tiny request.
#[tauri::command]
pub async fn github_releases_ai_test(provider: ProviderId) -> Result<Answer, String> {
    let settings = blocking(store::load).await??;
    ai::complete(
        &settings.ai,
        Some(provider),
        "Reply with the single word: ready",
        "Are you there?",
        300,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool) -> Release {
        serde_json::from_value(json!({
            "id": 1, "tag_name": tag, "name": tag, "body": "", "draft": draft, "prerelease": false,
            "html_url": "https://github.com/a/b/releases/tag/x", "assets": []
        }))
        .unwrap()
    }

    #[test]
    fn missing_null_and_set_fields_differ() {
        let missing: EntrySettings = serde_json::from_value(json!({})).unwrap();
        let cleared: EntrySettings = serde_json::from_value(json!({ "buildCommand": null })).unwrap();
        let set: EntrySettings = serde_json::from_value(json!({ "buildCommand": "npm run build" })).unwrap();
        assert_eq!(missing.build_command, None);
        assert_eq!(cleared.build_command, Some(None));
        assert_eq!(set.build_command, Some(Some("npm run build".into())));
        let ai: AiChanges = serde_json::from_value(json!({ "models": { "groq": "x" }, "order": ["openrouter", "groq"] })).unwrap();
        assert_eq!(ai.order.unwrap(), [ProviderId::OpenRouter, ProviderId::Groq]);
        assert_eq!(ai.models.unwrap()[&ProviderId::Groq], "x");
    }

    #[test]
    fn the_last_release_of_each_project() {
        let releases = vec![
            release("v3.7.3", false),
            release("backup_projects-v2.0.8", true),
            release("v2.0.6", false),
        ];
        assert_eq!(
            last_release_of(&releases, "v", Some("9.0.0"))
                .unwrap()
                .tag_name,
            "v3.7.3"
        );
        assert_eq!(
            last_release_of(&releases, "backup_projects-v", Some("2.0.7"))
                .unwrap()
                .tag_name,
            "v2.0.6"
        );
        assert!(last_release_of(&releases, "discord_bot-v", Some("1.0.1")).is_none());
        assert_eq!(encode_path("C:/My Code/a#b.ts"), "C:/My%20Code/a%23b.ts");
    }
}
