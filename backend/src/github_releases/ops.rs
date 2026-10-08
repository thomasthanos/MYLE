//! The git steps the page runs: commit, push, pull and fetch, with the
//! failures they can have turned into problems the page can act on.
//!
//! A push or fetch first uses the user's own git sign-in (Git Credential
//! Manager, SSH). Only when GitHub refuses it, and the remote is GitHub over
//! HTTPS, it is tried once more with the connected account's token, handed
//! to git through its environment for that one run.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Serialize;

use super::git::{self, GitEnv, Output, Remote};
use super::{Problem, secrets};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetOutcome {
    pub ok: bool,
    pub problem: Option<Problem>,
    /// The branch was pushed for the first time and now tracks the remote.
    pub set_upstream: bool,
    /// The connected account's token was needed.
    pub used_token: bool,
    pub output: String,
}

fn https_github(remote: &Remote) -> bool {
    remote.owner.is_some() && !remote.ssh
}

/// Hooks git would run (`.git/hooks/<name>` or a `core.hooksPath` such as
/// husky's).
async fn has_hooks(repo: &Path) -> bool {
    if git::read(repo, &["config", "--get", "core.hooksPath"])
        .await
        .is_ok_and(|p| !p.trim().is_empty())
    {
        return true;
    }
    let Ok(dir) = git::read(repo, &["rev-parse", "--git-path", "hooks"]).await else {
        return false;
    };
    let dir = repo.join(dir.trim());
    ["pre-commit", "commit-msg", "pre-push", "prepare-commit-msg"]
        .iter()
        .any(|name| dir.join(name).is_file())
}

/// Runs a network command, retrying once with the token when the user's
/// own sign-in is refused.
pub(crate) async fn network(
    repo: &Path,
    args: &[&str],
    remote: &Remote,
    quiet: bool,
    action: &str,
    cancel: Arc<AtomicBool>,
    on_line: &(dyn Fn(&str, bool) + Send + Sync),
) -> Result<(Output, NetOutcome), String> {
    let token = if https_github(remote) {
        secrets::token()
    } else {
        None
    };
    // Background fetches use the token right away when there is one: they
    // must never pop up a sign-in window.
    let first_env = GitEnv {
        token: if quiet { token.clone() } else { None },
        quiet,
    };
    let mut output = git::stream(repo, args, &first_env, cancel.clone(), on_line).await?;
    let mut outcome = NetOutcome {
        used_token: first_env.token.is_some(),
        ..NetOutcome::default()
    };
    if !output.ok()
        && first_env.token.is_none()
        && let Some(token) = token
        && git::classify(action, &output.all()).code == "AUTH"
    {
        on_line(
            "Git's own sign-in was refused; trying again with your connected GitHub account…",
            false,
        );
        output = git::stream(
            repo,
            args,
            &GitEnv {
                token: Some(token),
                quiet,
            },
            cancel,
            on_line,
        )
        .await?;
        outcome.used_token = true;
    }
    outcome.ok = output.ok();
    outcome.output = output.all();
    if !outcome.ok {
        let mut problem = git::classify(action, &outcome.output);
        if problem.code == "GIT_FAILED" && has_hooks(repo).await && action == "push" {
            problem = git::classify(action, &format!("pre-push hook failed\n{}", outcome.output));
        }
        outcome.problem = Some(problem);
    }
    Ok((output, outcome))
}

pub(crate) async fn require_remote(repo: &Path) -> Result<Remote, String> {
    git::remote(repo).await?.ok_or_else(|| {
        Problem::new(
            "NO_REMOTE",
            "This repository has no remote. Create the repository on GitHub and add it as \"origin\" first.",
        )
        .into()
    })
}

/// Pushes the current branch: to its upstream, or (the first time) to a
/// branch of the same name that it then tracks.
pub(crate) async fn push(
    repo: &Path,
    cancel: Arc<AtomicBool>,
    on_line: &(dyn Fn(&str, bool) + Send + Sync),
) -> Result<NetOutcome, String> {
    let status = git::status(repo).await?;
    let Some(branch) = status.branch.branch.clone() else {
        return Err(Problem::new(
            "DETACHED",
            "No branch is checked out (a detached HEAD). Check out a branch first.",
        )
        .into());
    };
    if status.branch.head.is_none() {
        return Err(Problem::new(
            "NO_COMMITS",
            "There is nothing to push yet: make the first commit.",
        )
        .into());
    }
    let remote = require_remote(repo).await?;
    let (target_remote, target_branch, set_upstream) = match status.branch.upstream.as_deref() {
        Some(upstream) => {
            let (name, branch) = upstream.split_once('/').unwrap_or((upstream, &branch));
            (name.to_string(), branch.to_string(), false)
        }
        None => (remote.name.clone(), branch.clone(), true),
    };
    let refspec = format!("HEAD:refs/heads/{target_branch}");
    let mut args = vec!["push", "--progress"];
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push(&target_remote);
    args.push(&refspec);
    let (_, mut outcome) = network(repo, &args, &remote, false, "push", cancel, on_line).await?;
    outcome.set_upstream = set_upstream && outcome.ok;
    Ok(outcome)
}

/// Pushes one tag.
pub(crate) async fn push_tag(
    repo: &Path,
    tag: &str,
    cancel: Arc<AtomicBool>,
    on_line: &(dyn Fn(&str, bool) + Send + Sync),
) -> Result<NetOutcome, String> {
    let remote = require_remote(repo).await?;
    let refspec = format!("refs/tags/{tag}:refs/tags/{tag}");
    let (_, outcome) = network(
        repo,
        &["push", "--progress", &remote.name, &refspec],
        &remote,
        false,
        "push",
        cancel,
        on_line,
    )
    .await?;
    Ok(outcome)
}

/// Pulls the upstream's new commits, putting the local ones on top
/// (`--rebase`, uncommitted changes set aside and back). A conflict undoes
/// the whole pull, so the repository is left as it was.
pub(crate) async fn pull(
    repo: &Path,
    cancel: Arc<AtomicBool>,
    on_line: &(dyn Fn(&str, bool) + Send + Sync),
) -> Result<NetOutcome, String> {
    let status = git::status(repo).await?;
    let Some(upstream) = status.branch.upstream.clone() else {
        return Err(Problem::new(
            "NO_UPSTREAM",
            "This branch isn't on GitHub yet, so there is nothing to pull. Push it first.",
        )
        .into());
    };
    let remote = require_remote(repo).await?;
    let (name, branch) = upstream
        .split_once('/')
        .unwrap_or((&remote.name, &upstream));
    let (name, branch) = (name.to_string(), branch.to_string());
    let (_, mut outcome) = network(
        repo,
        &[
            "pull",
            "--rebase",
            "--autostash",
            "--progress",
            &name,
            &branch,
        ],
        &remote,
        false,
        "pull",
        cancel,
        on_line,
    )
    .await?;
    if !outcome.ok {
        let rebasing = git::read(repo, &["rev-parse", "--git-path", "rebase-merge"])
            .await
            .ok()
            .map(|p| repo.join(p.trim()));
        let applying = git::read(repo, &["rev-parse", "--git-path", "rebase-apply"])
            .await
            .ok()
            .map(|p| repo.join(p.trim()));
        if rebasing.is_some_and(|p| p.exists()) || applying.is_some_and(|p| p.exists()) {
            let _ = git::run(repo, &["rebase", "--abort"]).await;
            if let Some(problem) = outcome.problem.as_mut() {
                problem.code = "CONFLICT".into();
                problem.message = "GitHub's new commits change the same lines as yours, so the pull was undone and nothing changed. Merge them in your editor or a terminal (git pull --rebase), then come back.".into();
            }
        }
    }
    Ok(outcome)
}

/// Fetches the remote's branches and tags. `quiet`: in the background,
/// without a sign-in window.
pub(crate) async fn fetch(
    repo: &Path,
    quiet: bool,
    cancel: Arc<AtomicBool>,
) -> Result<NetOutcome, String> {
    let remote = require_remote(repo).await?;
    let (_, outcome) = network(
        repo,
        &["fetch", "--progress", "--tags", "--prune", &remote.name],
        &remote,
        quiet,
        "fetch",
        cancel,
        &|_, _| {},
    )
    .await?;
    Ok(outcome)
}

/// Commits what is staged (or, with `paths`, exactly those paths as they
/// are in the folder) with `message`; the new commit's id.
pub(crate) async fn commit(
    repo: &Path,
    message: &str,
    paths: Option<&[String]>,
) -> Result<String, Problem> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Problem::new("NO_MESSAGE", "Write a commit message first."));
    }
    let mut args: Vec<&str> = vec!["commit", "--file=-"];
    if let Some(paths) = paths {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    let output = git::run_with(
        repo,
        &args,
        Some(message.as_bytes()),
        &GitEnv::default(),
        Duration::from_secs(600),
    )
    .await
    .map_err(|error| Problem::new("GIT_FAILED", error))?;
    if !output.ok() {
        let mut problem = git::classify("commit", &output.all());
        if problem.code == "GIT_FAILED" && has_hooks(repo).await {
            problem = git::classify(
                "commit",
                &format!("pre-commit hook failed\n{}", output.all()),
            );
        }
        return Err(problem);
    }
    git::read(repo, &["rev-parse", "HEAD"])
        .await
        .map(|sha| sha.trim().to_string())
        .map_err(|error| Problem::new("GIT_FAILED", error))
}
