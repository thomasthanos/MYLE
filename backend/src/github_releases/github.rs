//! The GitHub REST API: the signed-in account, releases and their files,
//! and Actions runs. The token only ever goes into an `Authorization`
//! header of a request to api.github.com / uploads.github.com.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CANCELLED, Problem};

const API: &str = "https://api.github.com";
const USER_AGENT: &str = "MYLE-GitHub-Releases";

pub(crate) fn client() -> Result<reqwest::Client, String> {
    crate::download::http_client(USER_AGENT)
}

fn request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    token: &str,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(token)
}

/// A failed request as a problem the page can act on.
pub(crate) fn api_problem(
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Problem {
    let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = json
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let details: Vec<String> = json
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(|e| {
                    e.get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            e.get("code").and_then(Value::as_str).map(|c| {
                                format!(
                                    "{} {c}",
                                    e.get("field").and_then(Value::as_str).unwrap_or("")
                                )
                            })
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let problem = match status {
        401 => Problem::new(
            "AUTH",
            "GitHub didn't accept the saved sign-in (it may have expired). Connect your account again.",
        ),
        403 | 429
            if remaining.as_deref() == Some("0")
                || message.to_lowercase().contains("rate limit") =>
        {
            Problem::new(
                "RATE_LIMITED",
                "GitHub's request limit was reached. Try again in a few minutes.",
            )
        }
        403 => Problem::new(
            "FORBIDDEN",
            format!(
                "GitHub refused this: {}. Your sign-in may lack the \"repo\" permission.",
                if message.is_empty() {
                    "forbidden"
                } else {
                    &message
                }
            ),
        ),
        404 => Problem::new(
            "NOT_FOUND",
            "GitHub didn't find it, or your account can't see this repository.",
        ),
        422 => Problem::new(
            "INVALID",
            if details.is_empty() {
                format!("GitHub didn't accept this: {message}")
            } else {
                format!("GitHub didn't accept this: {}", details.join("; "))
            },
        ),
        _ => Problem::new(
            "GITHUB_FAILED",
            format!(
                "GitHub answered {status}{}",
                if message.is_empty() {
                    String::new()
                } else {
                    format!(": {message}")
                }
            ),
        ),
    };
    problem.with_details(body.chars().take(2000).collect::<String>())
}

async fn send(
    builder: reqwest::RequestBuilder,
) -> Result<(reqwest::header::HeaderMap, String), String> {
    let response = builder.send().await.map_err(|error| {
        String::from(Problem::new(
            "NETWORK",
            format!("Couldn't reach GitHub: {}", crate::download::err(error)),
        ))
    })?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response
        .text()
        .await
        .map_err(|error| format!("GitHub's answer was cut off: {error}"))?;
    if (200..300).contains(&status) {
        Ok((headers, body))
    } else {
        Err(api_problem(status, &headers, &body).into())
    }
}

async fn get_json<T: for<'de> Deserialize<'de>>(token: &str, url: &str) -> Result<T, String> {
    let (_, body) = send(request(&client()?, reqwest::Method::GET, url, token)).await?;
    serde_json::from_str(&body)
        .map_err(|error| format!("GitHub's answer was not understood: {error}"))
}

// ─── Account ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
struct User {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

/// The account `token` belongs to, and its scopes (classic tokens and gh;
/// empty for fine-grained tokens, which have none to list).
pub(crate) async fn whoami(token: &str) -> Result<super::store::Account, String> {
    let (headers, body) = send(request(
        &client()?,
        reqwest::Method::GET,
        &format!("{API}/user"),
        token,
    ))
    .await?;
    let user: User = serde_json::from_str(&body)
        .map_err(|error| format!("GitHub's answer was not understood: {error}"))?;
    let scopes = headers
        .get("x-oauth-scopes")
        .and_then(|v| v.to_str().ok())
        .map(|text| {
            text.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(super::store::Account {
        login: user.login,
        name: user.name.filter(|n| !n.trim().is_empty()),
        avatar_url: user.avatar_url,
        source: String::new(),
        scopes,
    })
}

// ─── Releases ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    #[serde(alias = "download_count")]
    pub download_count: u64,
    #[serde(alias = "browser_download_url")]
    pub browser_download_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub id: u64,
    #[serde(alias = "tag_name")]
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    #[serde(alias = "created_at")]
    pub created_at: Option<String>,
    #[serde(alias = "published_at")]
    pub published_at: Option<String>,
    #[serde(alias = "html_url")]
    pub html_url: String,
    #[serde(alias = "upload_url", default)]
    pub upload_url: String,
    #[serde(alias = "target_commitish", default)]
    pub target_commitish: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

pub(crate) async fn releases(
    token: &str,
    owner: &str,
    repo: &str,
    count: usize,
) -> Result<Vec<Release>, String> {
    get_json(
        token,
        &format!(
            "{API}/repos/{owner}/{repo}/releases?per_page={}",
            count.clamp(1, 100)
        ),
    )
    .await
}

/// Every release of the repository, drafts too, newest first (all pages).
pub(crate) async fn all_releases(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<Release>, String> {
    let mut all = Vec::new();
    for page in 1..=50 {
        let list: Vec<Release> = get_json(
            token,
            &format!("{API}/repos/{owner}/{repo}/releases?per_page=100&page={page}"),
        )
        .await?;
        let done = list.len() < 100;
        all.extend(list);
        if done {
            return Ok(all);
        }
    }
    Err("The repository has too many releases to list.".into())
}

/// One release, as GitHub has it now.
pub(crate) async fn release(
    token: &str,
    owner: &str,
    repo: &str,
    id: u64,
) -> Result<Release, String> {
    get_json(token, &format!("{API}/repos/{owner}/{repo}/releases/{id}")).await
}

/// A failed tag delete that only found the tag gone already.
pub(crate) fn tag_was_gone(error: &str) -> bool {
    error.contains("\"NOT_FOUND\"") || error.contains("Reference does not exist")
}

/// The tag of every release of the repository, drafts too (all pages, so
/// a tag is never taken for one without a release because its release was
/// further down the list).
pub(crate) async fn release_tags(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<std::collections::HashSet<String>, String> {
    #[derive(Deserialize)]
    struct Brief {
        tag_name: String,
    }
    let mut tags = std::collections::HashSet::new();
    for page in 1..=50 {
        let list: Vec<Brief> = get_json(
            token,
            &format!("{API}/repos/{owner}/{repo}/releases?per_page=100&page={page}"),
        )
        .await?;
        let done = list.len() < 100;
        tags.extend(list.into_iter().map(|r| r.tag_name));
        if done {
            return Ok(tags);
        }
    }
    Err("The repository has too many releases to list.".into())
}

/// The repository's tags on GitHub (all pages).
pub(crate) async fn remote_tags(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Brief {
        name: String,
    }
    let mut tags = Vec::new();
    for page in 1..=50 {
        let list: Vec<Brief> = get_json(
            token,
            &format!("{API}/repos/{owner}/{repo}/tags?per_page=100&page={page}"),
        )
        .await?;
        let done = list.len() < 100;
        tags.extend(list.into_iter().map(|t| t.name));
        if done {
            return Ok(tags);
        }
    }
    Err("The repository has too many tags to list.".into())
}

pub(crate) async fn release_by_tag(
    token: &str,
    owner: &str,
    repo: &str,
    tag: &str,
) -> Result<Option<Release>, String> {
    let url = format!("{API}/repos/{owner}/{repo}/releases/tags/{}", encode(tag));
    match get_json::<Release>(token, &url).await {
        Ok(release) => Ok(Some(release)),
        Err(error) if error.contains("\"NOT_FOUND\"") => Ok(None),
        Err(error) => Err(error),
    }
}

/// Percent-encodes one path segment.
pub(crate) fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRelease {
    pub tag: String,
    /// The commit the tag is made on, when GitHub makes the tag.
    pub target: Option<String>,
    pub title: String,
    pub notes: String,
    pub draft: bool,
    pub prerelease: bool,
    pub make_latest: bool,
}

pub(crate) fn release_body(new: &NewRelease) -> Value {
    let mut body = json!({
        "tag_name": new.tag,
        "name": new.title,
        "body": new.notes,
        "draft": new.draft,
        "prerelease": new.prerelease,
        "make_latest": if new.make_latest && !new.prerelease { "true" } else { "false" },
    });
    if let Some(target) = &new.target {
        body["target_commitish"] = json!(target);
    }
    body
}

pub(crate) async fn create_release(
    token: &str,
    owner: &str,
    repo: &str,
    new: &NewRelease,
) -> Result<Release, String> {
    let (_, body) = send(
        request(
            &client()?,
            reqwest::Method::POST,
            &format!("{API}/repos/{owner}/{repo}/releases"),
            token,
        )
        .json(&release_body(new)),
    )
    .await?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

/// Changes a release: `patch` holds the fields to set (`name`, `body`,
/// `draft`, `prerelease`, `make_latest`).
pub(crate) async fn update_release(
    token: &str,
    owner: &str,
    repo: &str,
    id: u64,
    patch: &Value,
) -> Result<Release, String> {
    let (_, body) = send(
        request(
            &client()?,
            reqwest::Method::PATCH,
            &format!("{API}/repos/{owner}/{repo}/releases/{id}"),
            token,
        )
        .json(patch),
    )
    .await?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

pub(crate) async fn delete_release(
    token: &str,
    owner: &str,
    repo: &str,
    id: u64,
) -> Result<(), String> {
    send(request(
        &client()?,
        reqwest::Method::DELETE,
        &format!("{API}/repos/{owner}/{repo}/releases/{id}"),
        token,
    ))
    .await
    .map(|_| ())
}

/// Deletes the tag `tag` on GitHub.
pub(crate) async fn delete_tag(
    token: &str,
    owner: &str,
    repo: &str,
    tag: &str,
) -> Result<(), String> {
    send(request(
        &client()?,
        reqwest::Method::DELETE,
        &format!("{API}/repos/{owner}/{repo}/git/refs/tags/{}", encode(tag)),
        token,
    ))
    .await
    .map(|_| ())
}

pub(crate) async fn delete_asset(
    token: &str,
    owner: &str,
    repo: &str,
    id: u64,
) -> Result<(), String> {
    send(request(
        &client()?,
        reqwest::Method::DELETE,
        &format!("{API}/repos/{owner}/{repo}/releases/assets/{id}"),
        token,
    ))
    .await
    .map(|_| ())
}

/// The upload address of a release, without its `{?name,label}` template.
pub(crate) fn upload_base(upload_url: &str) -> &str {
    upload_url.split('{').next().unwrap_or(upload_url)
}

fn content_type(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".zip") {
        "application/zip"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".txt")
        || lower.ends_with(".sig")
    {
        "text/plain"
    } else if lower.ends_with(".gz") {
        "application/gzip"
    } else {
        "application/octet-stream"
    }
}

/// Uploads `path` as the file `name` of a release, reporting
/// `(sent, total)` bytes, until done or `cancel` is set.
pub(crate) async fn upload_asset(
    token: &str,
    upload_url: &str,
    path: &Path,
    name: &str,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<Asset, String> {
    use futures_util::StreamExt;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("Can't open {name}: {error}"))?;
    let total = file
        .metadata()
        .await
        .map_err(|error| error.to_string())?
        .len();
    let sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let progress = Arc::new(on_progress);
    let last = Arc::new(std::sync::Mutex::new(Instant::now()));
    let stream = file_stream(file).map({
        let sent = sent.clone();
        let cancel = cancel.clone();
        let progress = progress.clone();
        move |chunk: std::io::Result<Vec<u8>>| {
            if cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::other(CANCELLED));
            }
            let chunk = chunk?;
            let done = sent.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64;
            let mut last = last.lock().unwrap_or_else(|p| p.into_inner());
            if done == total || last.elapsed() > Duration::from_millis(150) {
                *last = Instant::now();
                progress(done, total);
            }
            Ok::<_, std::io::Error>(chunk)
        }
    });
    let url = format!("{}?name={}", upload_base(upload_url), encode(name));
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let result = send(
        request(&client, reqwest::Method::POST, &url, token)
            .header("Content-Type", content_type(name))
            .header("Content-Length", total)
            .body(reqwest::Body::wrap_stream(stream)),
    )
    .await;
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    let (_, body) = result?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

/// A file read in 256 KiB chunks, as a stream.
fn file_stream(
    file: tokio::fs::File,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send + 'static {
    use tokio::io::AsyncReadExt;
    futures_util::stream::unfold(Some(file), |state| async move {
        let mut file = state?;
        let mut buffer = vec![0u8; 256 * 1024];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(count) => {
                buffer.truncate(count);
                Some((Ok(buffer), Some(file)))
            }
            Err(error) => Some((Err(error), None)),
        }
    })
}

// ─── Actions ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: u64,
    pub name: Option<String>,
    #[serde(alias = "display_title", default)]
    pub display_title: Option<String>,
    /// `queued`, `in_progress`, `completed`, …
    pub status: Option<String>,
    /// `success`, `failure`, `cancelled`, … once completed.
    pub conclusion: Option<String>,
    #[serde(alias = "html_url")]
    pub html_url: String,
    #[serde(alias = "head_branch", default)]
    pub head_branch: Option<String>,
    #[serde(alias = "head_sha", default)]
    pub head_sha: String,
    #[serde(default)]
    pub event: String,
    #[serde(alias = "created_at", default)]
    pub created_at: Option<String>,
    #[serde(alias = "updated_at", default)]
    pub updated_at: Option<String>,
    /// The workflow file (`.github/workflows/release.yml`).
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize)]
struct Runs {
    workflow_runs: Vec<Run>,
}

/// The newest runs, of `branch` when given.
pub(crate) async fn runs(
    token: &str,
    owner: &str,
    repo: &str,
    branch: Option<&str>,
    count: usize,
) -> Result<Vec<Run>, String> {
    let mut url = format!("{API}/repos/{owner}/{repo}/actions/runs?per_page={count}");
    if let Some(branch) = branch {
        url.push_str(&format!("&branch={}", encode(branch)));
    }
    Ok(get_json::<Runs>(token, &url).await?.workflow_runs)
}

/// The runs a push of `sha` started.
pub(crate) async fn runs_for_commit(
    token: &str,
    owner: &str,
    repo: &str,
    sha: &str,
) -> Result<Vec<Run>, String> {
    let url = format!("{API}/repos/{owner}/{repo}/actions/runs?per_page=20&head_sha={sha}");
    Ok(get_json::<Runs>(token, &url).await?.workflow_runs)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub number: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    #[serde(alias = "html_url", default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Jobs {
    jobs: Vec<Job>,
}

pub(crate) async fn run_jobs(
    token: &str,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<Vec<Job>, String> {
    let url = format!("{API}/repos/{owner}/{repo}/actions/runs/{run_id}/jobs?per_page=50");
    Ok(get_json::<Jobs>(token, &url).await?.jobs)
}

pub(crate) async fn run(token: &str, owner: &str, repo: &str, run_id: u64) -> Result<Run, String> {
    get_json(
        token,
        &format!("{API}/repos/{owner}/{repo}/actions/runs/{run_id}"),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_get_codes_and_the_upload_url_its_base() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(api_problem(401, &headers, "{}").code, "AUTH");
        assert_eq!(
            api_problem(404, &headers, "{\"message\":\"Not Found\"}").code,
            "NOT_FOUND"
        );
        let invalid = api_problem(
            422,
            &headers,
            r#"{"message":"Validation Failed","errors":[{"resource":"Release","code":"already_exists","field":"tag_name"}]}"#,
        );
        assert_eq!(invalid.code, "INVALID");
        assert!(invalid.message.contains("tag_name already_exists"));
        let mut limited = reqwest::header::HeaderMap::new();
        limited.insert("x-ratelimit-remaining", "0".parse().unwrap());
        assert_eq!(
            api_problem(403, &limited, "{\"message\":\"API rate limit exceeded\"}").code,
            "RATE_LIMITED"
        );
        assert_eq!(
            upload_base("https://uploads.github.com/repos/o/r/releases/1/assets{?name,label}"),
            "https://uploads.github.com/repos/o/r/releases/1/assets"
        );
        assert_eq!(encode("backup_projects-v2.0.7"), "backup_projects-v2.0.7");
        assert_eq!(encode("My App 1.0.exe"), "My%20App%201.0.exe");
    }

    #[test]
    fn a_new_release_targets_its_commit() {
        let body = release_body(&NewRelease {
            tag: "v1.0.0".into(),
            target: Some("abc123".into()),
            title: "1.0.0".into(),
            notes: "n".into(),
            draft: true,
            prerelease: false,
            make_latest: true,
        });
        assert_eq!(body["target_commitish"], "abc123");
        assert_eq!(body["make_latest"], "true");
        assert_eq!(body["draft"], true);
    }
}
