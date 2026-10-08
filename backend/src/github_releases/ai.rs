//! One client for every AI provider: they all speak the OpenAI chat
//! completions API, so a provider is only a base URL, a default model and
//! how its key is checked. Keys are the user's own (`secrets`); a provider
//! that is out of free requests (HTTP 429) or fails points the page at the
//! next one in the user's order.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Problem;
use super::store::AiSettings;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Groq,
    Gemini,
    OpenRouter,
    DeepSeek,
    Ollama,
}

impl ProviderId {
    pub(crate) const ALL: [ProviderId; 5] = [
        ProviderId::Groq,
        ProviderId::Gemini,
        ProviderId::OpenRouter,
        ProviderId::DeepSeek,
        ProviderId::Ollama,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ProviderId::Groq => "groq",
            ProviderId::Gemini => "gemini",
            ProviderId::OpenRouter => "openrouter",
            ProviderId::DeepSeek => "deepseek",
            ProviderId::Ollama => "ollama",
        }
    }

    pub(crate) fn def(self) -> &'static ProviderDef {
        PROVIDERS
            .iter()
            .find(|def| def.id == self)
            .unwrap_or(&PROVIDERS[0])
    }
}

/// What the page shows about a provider and how it is reached.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDef {
    pub id: ProviderId,
    pub name: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    /// Where to get a key.
    pub key_url: &'static str,
    pub needs_key: bool,
    /// The free tier, in a line.
    pub free: &'static str,
    /// What happens to what is sent.
    pub privacy: &'static str,
}

pub(crate) const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: ProviderId::Groq,
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        default_model: "openai/gpt-oss-120b",
        key_url: "https://console.groq.com/keys",
        needs_key: true,
        free: "Free, no card: about 30 requests a minute and 1,000 a day.",
        privacy: "Groq says it doesn't train on what you send.",
    },
    ProviderDef {
        id: ProviderId::Gemini,
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-flash-latest",
        key_url: "https://aistudio.google.com/apikey",
        needs_key: true,
        free: "Free tier with daily limits shown in AI Studio; a very large context.",
        privacy: "On the free tier Google may use what you send to improve its products.",
    },
    ProviderDef {
        id: ProviderId::OpenRouter,
        name: "OpenRouter (free models)",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openrouter/free",
        key_url: "https://openrouter.ai/keys",
        needs_key: true,
        free: "Free models: 20 requests a minute, 50 a day.",
        privacy: "Free models run at changing providers; some may log prompts.",
    },
    ProviderDef {
        id: ProviderId::DeepSeek,
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-v4-flash",
        key_url: "https://platform.deepseek.com/api_keys",
        needs_key: true,
        free: "Paid, but very cheap.",
        privacy: "Runs on DeepSeek's servers.",
    },
    ProviderDef {
        id: ProviderId::Ollama,
        name: "Ollama (this PC)",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3.2",
        key_url: "https://ollama.com/download",
        needs_key: false,
        free: "Free and offline; as good as the model your PC can run.",
        privacy: "Nothing leaves this PC.",
    },
];

/// One chat request, ready to send.
#[derive(Clone, Debug)]
pub(crate) struct ChatRequest {
    pub url: String,
    pub body: Value,
}

pub(crate) fn base_url(settings: &AiSettings, provider: ProviderId) -> String {
    match (provider, settings.ollama_url.as_deref()) {
        (ProviderId::Ollama, Some(url)) if !url.trim().is_empty() => {
            let url = url.trim().trim_end_matches('/');
            if url.ends_with("/v1") {
                url.to_string()
            } else {
                format!("{url}/v1")
            }
        }
        _ => provider.def().base_url.to_string(),
    }
}

pub(crate) fn model(settings: &AiSettings, provider: ProviderId) -> String {
    settings
        .models
        .get(&provider)
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| provider.def().default_model.to_string())
}

pub(crate) fn build_request(
    base: &str,
    provider: ProviderId,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> ChatRequest {
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "temperature": 0.3,
        "max_tokens": max_tokens,
        "stream": false,
    });
    // Reasoning models think less for a commit message or release notes.
    let reasoning = match provider {
        ProviderId::Groq => model.contains("gpt-oss") || model.contains("qwen"),
        ProviderId::Gemini => true,
        _ => false,
    };
    if reasoning {
        body["reasoning_effort"] = json!("low");
    }
    ChatRequest {
        url: format!("{}/chat/completions", base.trim_end_matches('/')),
        body,
    }
}

/// A failed AI request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiError {
    /// `RATE_LIMITED`, `BAD_KEY`, `NO_CREDIT`, `BAD_MODEL`, `UNAVAILABLE`,
    /// `NETWORK`, `AI_FAILED`.
    pub code: &'static str,
    pub message: String,
    pub retry_after: Option<u64>,
}

fn error_message(body: &str) -> String {
    let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = json.get("error").unwrap_or(&json);
    let first = if error.is_array() { &error[0] } else { error };
    let inner = first.get("error").unwrap_or(first);
    inner
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| inner.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// Drops a reasoning model's `<think>…</think>` and a Markdown fence around
/// the whole answer.
pub(crate) fn clean_answer(text: &str) -> String {
    let mut text = text.to_string();
    while let (Some(start), Some(end)) = (text.find("<think>"), text.find("</think>")) {
        if end < start {
            break;
        }
        text.replace_range(start..end + "</think>".len(), "");
    }
    let trimmed = text.trim();
    let unfenced = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
        .map(|inner| {
            // The fence's language name (```markdown).
            match inner.split_once('\n') {
                Some((first, rest)) if !first.contains(' ') => rest,
                _ => inner,
            }
        })
        .unwrap_or(trimmed);
    unfenced.trim().to_string()
}

pub(crate) fn parse_response(
    provider: ProviderId,
    status: u16,
    retry_after: Option<&str>,
    body: &str,
) -> Result<String, AiError> {
    let name = provider.def().name;
    let retry_after = retry_after
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map(|s| s.ceil() as u64);
    let fail = |code, message: String| AiError {
        code,
        message,
        retry_after,
    };
    match status {
        200..=299 => {
            let json: Value = serde_json::from_str(body)
                .map_err(|_| fail("AI_FAILED", format!("{name}'s answer was not understood.")))?;
            let content = json["choices"][0]["message"]["content"]
                .as_str()
                .map(clean_answer)
                .unwrap_or_default();
            if content.is_empty() {
                let reason = json["choices"][0]["finish_reason"].as_str().unwrap_or("");
                return Err(fail(
                    "AI_FAILED",
                    if reason == "length" {
                        format!(
                            "{name} ran out of room before answering. Try again, or pick another model."
                        )
                    } else {
                        format!("{name} sent an empty answer. Try again.")
                    },
                ));
            }
            Ok(content)
        }
        429 => Err(fail(
            "RATE_LIMITED",
            match retry_after {
                Some(seconds) if seconds <= 120 => format!(
                    "{name}'s free limit was reached for now; it frees up in about {seconds} s."
                ),
                _ => format!("{name}'s free limit was reached for now."),
            },
        )),
        401 | 403 => Err(fail(
            "BAD_KEY",
            format!("{name} didn't accept the key: {}", error_message(body)),
        )),
        402 => Err(fail(
            "NO_CREDIT",
            format!(
                "{name} needs credit on the account: {}",
                error_message(body)
            ),
        )),
        400 | 404 if error_message(body).to_lowercase().contains("model") => Err(fail(
            "BAD_MODEL",
            format!("{name} doesn't know this model: {}", error_message(body)),
        )),
        500..=599 => Err(fail(
            "UNAVAILABLE",
            format!("{name} is having trouble right now ({status}). Try again in a moment."),
        )),
        _ => Err(fail(
            "AI_FAILED",
            format!("{name} answered {status}: {}", error_message(body)),
        )),
    }
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("MYLE-GitHub-Releases")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())
}

pub(crate) async fn send(
    client: &reqwest::Client,
    provider: ProviderId,
    key: Option<&str>,
    request: &ChatRequest,
) -> Result<String, AiError> {
    let mut builder = client.post(&request.url).json(&request.body);
    if let Some(key) = key {
        builder = builder.bearer_auth(key);
    }
    if provider == ProviderId::OpenRouter {
        builder = builder
            .header("HTTP-Referer", "https://github.com/thomasthanos/MYLE")
            .header("X-Title", "MYLE");
    }
    let response = builder.send().await.map_err(|error| AiError {
        code: "NETWORK",
        message: if provider == ProviderId::Ollama {
            "Ollama is not running on this PC (or not at that address). Start it, then try again."
                .into()
        } else {
            format!("Couldn't reach {}: {error}", provider.def().name)
        },
        retry_after: None,
    })?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.unwrap_or_default();
    parse_response(provider, status, retry_after.as_deref(), &body)
}

/// Whether `provider` can be used: it has a key (Ollama: it is switched on).
pub(crate) fn ready(settings: &AiSettings, provider: ProviderId) -> bool {
    match provider {
        ProviderId::Ollama => settings.ollama_enabled,
        _ => super::secrets::ai_key(provider).is_some(),
    }
}

/// The providers that can be used, in the user's order.
pub(crate) fn ready_order(
    settings: &AiSettings,
    is_ready: impl Fn(ProviderId) -> bool,
) -> Vec<ProviderId> {
    settings
        .order
        .iter()
        .copied()
        .filter(|p| is_ready(*p))
        .collect()
}

/// The provider after `provider` in `order`.
pub(crate) fn next_after(order: &[ProviderId], provider: ProviderId) -> Option<ProviderId> {
    let index = order.iter().position(|p| *p == provider)?;
    order.get(index + 1).copied()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub text: String,
    pub provider: ProviderId,
    pub model: String,
}

/// Asks `provider` (the first ready one when `None`). A failure carries the
/// next ready provider in `data.next`, for the page to offer.
pub(crate) async fn complete(
    settings: &AiSettings,
    provider: Option<ProviderId>,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<Answer, String> {
    let order = ready_order(settings, |p| ready(settings, p));
    let Some(provider) = provider.or_else(|| order.first().copied()) else {
        return Err(Problem::new(
            "NO_AI",
            "Add a free AI key first (Groq takes a minute: console.groq.com/keys), in this page's settings.",
        )
        .into());
    };
    let key = super::secrets::ai_key(provider);
    if provider.def().needs_key && key.is_none() {
        return Err(Problem::new(
            "NO_AI",
            format!(
                "Add your {} key in this page's settings first.",
                provider.def().name
            ),
        )
        .into());
    }
    let model = model(settings, provider);
    let request = build_request(
        &base_url(settings, provider),
        provider,
        &model,
        system,
        user,
        max_tokens,
    );
    match send(
        &http()?,
        provider,
        key.as_deref().map(|k| k.as_str()),
        &request,
    )
    .await
    {
        Ok(text) => Ok(Answer {
            text,
            provider,
            model,
        }),
        Err(error) => {
            let next = next_after(&order, provider);
            Err(Problem::new(error.code, error.message)
                .with_data(json!({
                    "provider": provider,
                    "next": next,
                    "nextName": next.map(|n| n.def().name),
                    "retryAfter": error.retry_after,
                }))
                .into())
        }
    }
}

/// Checks a key by listing the models it may use.
pub(crate) async fn check_key(
    settings: &AiSettings,
    provider: ProviderId,
    key: Option<&str>,
) -> Result<(), String> {
    let base = base_url(settings, provider);
    let url = if provider == ProviderId::OpenRouter {
        format!("{base}/key")
    } else {
        format!("{base}/models")
    };
    let mut builder = http()?.get(&url);
    if let Some(key) = key {
        builder = builder.bearer_auth(key);
    }
    let response = builder.send().await.map_err(|error| {
        if provider == ProviderId::Ollama {
            "Ollama is not running on this PC (or not at that address).".to_string()
        } else {
            format!("Couldn't reach {}: {error}", provider.def().name)
        }
    })?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(format!("{} didn't accept this key.", provider.def().name)),
        _ => Err(format!(
            "{} answered {status}: {}",
            provider.def().name,
            error_message(&body)
        )),
    }
}

/// The models Ollama has (for the settings' model list).
pub(crate) async fn ollama_models(settings: &AiSettings) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base_url(settings, ProviderId::Ollama));
    let response =
        http()?.get(url).send().await.map_err(|_| {
            "Ollama is not running on this PC (or not at that address).".to_string()
        })?;
    let json: Value = response.json().await.map_err(|error| error.to_string())?;
    Ok(json["data"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

// ─── Prompts ───────────────────────────────────────────────────────────────

/// Most of a diff a request carries (Groq's free tier allows ~8,000 tokens a
/// minute, so the whole prompt stays well under it).
pub(crate) const MAX_DIFF_CHARS: usize = 14_000;

/// `diff` cut to `max` characters, each file keeping its share so one huge
/// file doesn't push the others out.
pub(crate) fn trim_diff(diff: &str, max: usize) -> String {
    if diff.len() <= max {
        return diff.to_string();
    }
    let files: Vec<&str> = diff.split("\ndiff --git ").collect();
    let share = (max / files.len().max(1)).max(400);
    let mut out = String::new();
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            out.push_str("\ndiff --git ");
        }
        if file.len() > share {
            let mut cut = share;
            while !file.is_char_boundary(cut) {
                cut -= 1;
            }
            out.push_str(&file[..cut]);
            out.push_str("\n… (rest of this file's changes left out)\n");
        } else {
            out.push_str(file);
        }
        if out.len() > max {
            out.push_str("\n… (more files left out)\n");
            break;
        }
    }
    out
}

pub(crate) const COMMIT_SYSTEM: &str = "You write git commit messages. Reply with the message only: a subject line of at most 72 characters in the imperative mood, then, only if it helps, a blank line and a few short bullet points. Follow the style of the project's recent commit subjects when they are given. No Markdown headings, no code fences, no quotes around the message.";

pub(crate) fn commit_prompt(stat: &str, diff: &str, recent: &[String]) -> String {
    let mut prompt = String::new();
    if !recent.is_empty() {
        prompt.push_str("Recent commit subjects of this project:\n");
        for subject in recent.iter().take(10) {
            prompt.push_str(&format!("- {subject}\n"));
        }
        prompt.push('\n');
    }
    prompt.push_str("Files changed:\n");
    prompt.push_str(stat.trim());
    prompt.push_str("\n\nThe changes:\n");
    prompt.push_str(&trim_diff(diff, MAX_DIFF_CHARS));
    prompt
}

pub(crate) const NOTES_SYSTEM: &str = "You write release notes for a GitHub release, for the people who use the app. Reply in exactly this form:\nTITLE: <a short title for the release, without the version number>\n---\n<the notes in Markdown: short sections such as ### New, ### Improved, ### Fixed, with one bullet per user-visible change, plain words, no commit hashes>\nLeave out sections that would be empty and changes users can't notice (refactors, CI, tests, version bumps).";

pub(crate) fn notes_prompt(
    project: &str,
    version: &str,
    commits: &[String],
    stat: &str,
    diff: &str,
) -> String {
    let mut prompt =
        format!("Project: {project}\nNew version: {version}\n\nCommits since the last release:\n");
    if commits.is_empty() {
        prompt.push_str("(none listed)\n");
    }
    for commit in commits.iter().take(80) {
        prompt.push_str(&format!("- {commit}\n"));
    }
    if !stat.trim().is_empty() {
        prompt.push_str("\nFiles changed:\n");
        prompt.push_str(stat.trim());
    }
    if !diff.trim().is_empty() {
        prompt.push_str("\n\nPart of the changes:\n");
        prompt.push_str(&trim_diff(diff, MAX_DIFF_CHARS / 2 + 2000));
    }
    prompt
}

pub(crate) const POLISH_SYSTEM: &str = "You tidy up release notes written by a developer: fix spelling and grammar, group the points under short Markdown sections (### New, ### Improved, ### Fixed) where that fits, and keep every fact. Reply in exactly this form:\nTITLE: <a short title without the version number>\n---\n<the notes in Markdown>";

pub(crate) const COMBINE_SYSTEM: &str = "You merge the notes of several GitHub releases into the notes of one release that replaces them. Keep every user-visible change once (drop duplicates and changes later reverted), group them under short Markdown sections (### New, ### Improved, ### Fixed). Reply in exactly this form:\nTITLE: <a short title without the version number>\n---\n<the notes in Markdown>";

/// Splits `TITLE: …\n---\n<notes>`; an answer without the form is all notes.
pub(crate) fn split_title(answer: &str) -> (Option<String>, String) {
    let text = answer.trim();
    let Some(rest) = text
        .strip_prefix("TITLE:")
        .or_else(|| text.strip_prefix("Title:"))
        .or_else(|| text.strip_prefix("**TITLE:**"))
    else {
        return (None, text.to_string());
    };
    let (title, notes) = rest.split_once('\n').unwrap_or((rest, ""));
    let notes = notes.trim_start();
    let notes = notes.strip_prefix("---").unwrap_or(notes);
    let title = title.trim().trim_matches(['*', '"']).trim().to_string();
    (
        (!title.is_empty()).then_some(title),
        notes.trim().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn requests_are_built_alike_for_every_provider() {
        let settings = AiSettings::default();
        let groq = build_request(
            &base_url(&settings, ProviderId::Groq),
            ProviderId::Groq,
            &model(&settings, ProviderId::Groq),
            "sys",
            "user",
            800,
        );
        assert_eq!(groq.url, "https://api.groq.com/openai/v1/chat/completions");
        assert_eq!(groq.body["model"], "openai/gpt-oss-120b");
        assert_eq!(groq.body["messages"][0]["role"], "system");
        assert_eq!(groq.body["messages"][1]["content"], "user");
        assert_eq!(groq.body["reasoning_effort"], "low");
        assert_eq!(groq.body["max_tokens"], 800);
        let deepseek = build_request(
            &base_url(&settings, ProviderId::DeepSeek),
            ProviderId::DeepSeek,
            "deepseek-v4-flash",
            "s",
            "u",
            10,
        );
        assert!(deepseek.body.get("reasoning_effort").is_none());
        let mut custom = AiSettings {
            ollama_url: Some("http://192.168.1.5:11434/".into()),
            ..AiSettings::default()
        };
        custom.models.insert(ProviderId::Ollama, "qwen3".into());
        assert_eq!(
            base_url(&custom, ProviderId::Ollama),
            "http://192.168.1.5:11434/v1"
        );
        assert_eq!(model(&custom, ProviderId::Ollama), "qwen3");
        assert_eq!(model(&custom, ProviderId::Gemini), "gemini-flash-latest");
    }

    #[test]
    fn answers_and_failures_are_read() {
        let ok = r#"{"choices":[{"message":{"content":"<think>hmm</think>\n```text\nFix the login button\n```"},"finish_reason":"stop"}]}"#;
        assert_eq!(
            parse_response(ProviderId::Groq, 200, None, ok).unwrap(),
            "Fix the login button"
        );
        let limited = parse_response(
            ProviderId::Groq,
            429,
            Some("7.2"),
            r#"{"error":{"message":"Rate limit reached"}}"#,
        )
        .unwrap_err();
        assert_eq!(limited.code, "RATE_LIMITED");
        assert_eq!(limited.retry_after, Some(8));
        assert!(limited.message.contains("8 s"));
        assert_eq!(
            parse_response(
                ProviderId::Gemini,
                400,
                None,
                r#"[{"error":{"message":"API key not valid"}}]"#
            )
            .unwrap_err()
            .code,
            "AI_FAILED"
        );
        assert_eq!(
            parse_response(
                ProviderId::Gemini,
                403,
                None,
                r#"[{"error":{"message":"API key not valid"}}]"#
            )
            .unwrap_err()
            .message,
            "Google Gemini didn't accept the key: API key not valid"
        );
        assert_eq!(
            parse_response(
                ProviderId::DeepSeek,
                402,
                None,
                r#"{"error":{"message":"Insufficient Balance"}}"#
            )
            .unwrap_err()
            .code,
            "NO_CREDIT"
        );
        assert_eq!(
            parse_response(
                ProviderId::Groq,
                404,
                None,
                r#"{"error":{"message":"The model `x` does not exist"}}"#
            )
            .unwrap_err()
            .code,
            "BAD_MODEL"
        );
        assert_eq!(
            parse_response(ProviderId::Groq, 503, None, "")
                .unwrap_err()
                .code,
            "UNAVAILABLE"
        );
        let empty = r#"{"choices":[{"message":{"content":""},"finish_reason":"length"}]}"#;
        assert!(
            parse_response(ProviderId::Groq, 200, None, empty)
                .unwrap_err()
                .message
                .contains("ran out of room")
        );
    }

    #[test]
    fn the_next_provider_follows_the_users_order() {
        let settings = AiSettings {
            order: vec![ProviderId::Gemini, ProviderId::Groq, ProviderId::Ollama],
            ..AiSettings::default()
        };
        let order = ready_order(&settings, |p| p != ProviderId::Ollama);
        assert_eq!(order, [ProviderId::Gemini, ProviderId::Groq]);
        assert_eq!(
            next_after(&order, ProviderId::Gemini),
            Some(ProviderId::Groq)
        );
        assert_eq!(next_after(&order, ProviderId::Groq), None);
    }

    /// A one-shot HTTP server: answers `response` and hands back the request.
    fn mock(
        status: &'static str,
        headers: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let response = format!(
            "HTTP/1.1 {status}\r\n{headers}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap_or(0);
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, handle)
    }

    #[test]
    fn a_mock_server_sees_the_key_once_and_a_429_is_reported() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (url, server) = mock(
            "429 Too Many Requests",
            "retry-after: 30\r\n",
            r#"{"error":{"message":"Rate limit hit"}}"#,
        );
        let request = build_request(
            &url,
            ProviderId::Groq,
            "openai/gpt-oss-120b",
            "sys",
            "the diff",
            100,
        );
        let error = runtime
            .block_on(send(
                &http().unwrap(),
                ProviderId::Groq,
                Some("gsk_test_key_123"),
                &request,
            ))
            .unwrap_err();
        assert_eq!(error.code, "RATE_LIMITED");
        assert_eq!(error.retry_after, Some(30));
        let seen = server.join().unwrap();
        assert!(seen.starts_with("POST /chat/completions"));
        assert!(
            seen.to_lowercase()
                .contains("authorization: bearer gsk_test_key_123")
        );
        assert!(seen.contains("\"model\":\"openai/gpt-oss-120b\""));

        let (url, server) = mock(
            "200 OK",
            "",
            r#"{"choices":[{"message":{"content":"Add the page"}}]}"#,
        );
        let request = build_request(&url, ProviderId::Ollama, "llama3.2", "s", "u", 50);
        let text = runtime
            .block_on(send(&http().unwrap(), ProviderId::Ollama, None, &request))
            .unwrap();
        assert_eq!(text, "Add the page");
        assert!(
            !server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("authorization")
        );
    }

    #[test]
    fn diffs_are_trimmed_fairly_and_titles_split() {
        let big = format!(
            "diff --git a/a b/a\n{}\ndiff --git a/b b/b\nsmall change\n",
            "x".repeat(50_000)
        );
        let trimmed = trim_diff(&big, 4000);
        assert!(trimmed.len() < 6000);
        assert!(trimmed.contains("small change"));
        assert_eq!(trim_diff("short", 100), "short");
        let (title, notes) = split_title("TITLE: Faster backups\n---\n### Fixed\n- A crash");
        assert_eq!(title.as_deref(), Some("Faster backups"));
        assert_eq!(notes, "### Fixed\n- A crash");
        assert_eq!(split_title("Just notes").0, None);
    }
}
