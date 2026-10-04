//! The account: sign-in with Discord or Google through Supabase Auth (PKCE),
//! its session and refreshing it, and signed-in requests to the account's
//! tables. Where the session is kept and how the browser comes back with
//! the sign-in's code is each app's own business.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::http::err;

pub const SUPABASE_URL: &str = "https://oofcywdbmhmqpowmwykz.supabase.co";
/// The project's public anon key. It is meant to ship inside apps: row-level
/// security limits every request to the signed-in user's own rows.
pub const SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9vZmN5d2RibWhtcXBvd213eWt6Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODMwMDU5NTIsImV4cCI6MjA5ODU4MTk1Mn0.lu8JE-CfgcfPc3TaeDBFFu1nuwbihwtEgCr9wK0P9ps";
/// Refresh the access token when it has less than this left.
const REFRESH_MARGIN_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Discord,
    Google,
}

impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Discord => "discord",
            Self::Google => "google",
        }
    }
}

/// Who is signed in, as the Settings page shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: u64,
    pub profile: Profile,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A signed-in connection to the account's tables (the Password Manager's),
/// with the session's token and row security.
pub struct Cloud {
    pub user_id: String,
    token: String,
    client: reqwest::Client,
}

impl Cloud {
    pub fn new(session: &Session, client: reqwest::Client) -> Self {
        Self {
            user_id: session.profile.id.clone(),
            token: session.access_token.clone(),
            client,
        }
    }

    /// `rest/v1/<path>` with `query` parameters.
    pub fn url(&self, path: &str, query: &[(&str, String)]) -> Result<reqwest::Url, String> {
        reqwest::Url::parse_with_params(&format!("{SUPABASE_URL}/rest/v1/{path}"), query).map_err(err)
    }

    pub fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .header("apikey", SUPABASE_ANON_KEY)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(20))
    }
}

// ---------------------------------------------------------------------------
// Sign-in

/// A random PKCE verifier and its S256 challenge.
pub fn pkce_pair() -> (String, String) {
    // Two v4 UUIDs: 64 characters and 244 random bits, within the 43-128
    // characters RFC 7636 allows.
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Supabase's sign-in page for `provider`, which comes back to `redirect`
/// with a code for `exchange_code`.
pub fn authorize_url(provider: Provider, redirect: &str, challenge: &str) -> Result<reqwest::Url, String> {
    reqwest::Url::parse_with_params(
        &format!("{SUPABASE_URL}/auth/v1/authorize"),
        &[
            ("provider", provider.id()),
            ("redirect_to", redirect),
            ("code_challenge", challenge),
            ("code_challenge_method", "s256"),
        ],
    )
    .map_err(err)
}

/// The session for the code the sign-in came back with.
pub async fn exchange_code(client: &reqwest::Client, code: &str, verifier: &str) -> Result<Session, String> {
    token_request(
        client,
        "pkce",
        serde_json::json!({ "auth_code": code, "code_verifier": verifier }),
    )
    .await
    .map_err(String::from)
}

/// A new session for a refresh token.
pub async fn refresh(client: &reqwest::Client, refresh_token: String) -> Result<Session, TokenError> {
    token_request(
        client,
        "refresh_token",
        serde_json::json!({ "refresh_token": refresh_token }),
    )
    .await
}

/// Revokes the session's refresh token. Best effort: signing out on the
/// device happens regardless.
pub async fn logout(client: &reqwest::Client, session: &Session) {
    let _ = client
        .post(format!("{SUPABASE_URL}/auth/v1/logout"))
        .header("apikey", SUPABASE_ANON_KEY)
        .bearer_auth(&session.access_token)
        .timeout(Duration::from_secs(8))
        .send()
        .await;
}

/// A session with at least a minute left, refreshed (and stored) if needed:
/// one refresh at a time (two at once would send the same refresh token
/// twice), and a refusal signs out only the session it was about, never one
/// renewed or signed in meanwhile.
pub async fn renew<Refresh, Refreshing>(
    gate: &tokio::sync::Mutex<()>,
    current: impl Fn() -> Option<Session>,
    store: impl Fn(Option<Session>) -> Result<(), String>,
    refresh: Refresh,
) -> Result<Session, String>
where
    Refresh: FnOnce(String) -> Refreshing,
    Refreshing: std::future::Future<Output = Result<Session, TokenError>>,
{
    let fresh_enough = |session: &Session| session.expires_at > now() + REFRESH_MARGIN_SECS;
    let session = current().ok_or("You are not signed in.")?;
    if fresh_enough(&session) {
        return Ok(session);
    }
    let _one_at_a_time = gate.lock().await;
    // Another request may have refreshed it while this one waited.
    let session = current().ok_or("You are not signed in.")?;
    if fresh_enough(&session) {
        return Ok(session);
    }
    let sent = session.refresh_token;
    match refresh(sent.clone()).await {
        Ok(fresh) => {
            store(Some(fresh.clone()))?;
            Ok(fresh)
        }
        Err(TokenError::Rejected(_)) => {
            // Revoked, or expired after long disuse: sign out cleanly.
            if current().is_some_and(|now| now.refresh_token == sent) {
                store(None)?;
            }
            Err("Your session has expired. Sign in again.".into())
        }
        Err(TokenError::Network(e)) => Err(e),
    }
}

pub enum TokenError {
    /// Supabase answered and refused (4xx): the token is no good.
    Rejected(String),
    Network(String),
}

impl From<TokenError> for String {
    fn from(error: TokenError) -> Self {
        match error {
            TokenError::Rejected(e) | TokenError::Network(e) => e,
        }
    }
}

pub async fn token_request(client: &reqwest::Client, grant: &str, body: Value) -> Result<Session, TokenError> {
    let response = client
        .post(format!("{SUPABASE_URL}/auth/v1/token?grant_type={grant}"))
        .header("apikey", SUPABASE_ANON_KEY)
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| TokenError::Network(e.to_string()))?;
    let status = response.status();
    // Busy or slow, not a verdict on the token: signing out over it would
    // throw away a perfectly good session.
    if matches!(
        status,
        reqwest::StatusCode::TOO_MANY_REQUESTS | reqwest::StatusCode::REQUEST_TIMEOUT
    ) {
        return Err(TokenError::Network(format!(
            "Supabase is busy ({status}). Try again in a moment."
        )));
    }
    let json: Value = response
        .json()
        .await
        .map_err(|e| TokenError::Network(e.to_string()))?;
    if status.is_client_error() {
        let message = json
            .get("error_description")
            .or_else(|| json.get("msg"))
            .or_else(|| json.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Supabase refused the sign-in.");
        return Err(TokenError::Rejected(message.to_string()));
    }
    if !status.is_success() {
        return Err(TokenError::Network(format!("Supabase answered {status}")));
    }
    session_from(&json).ok_or_else(|| TokenError::Network("Supabase sent an incomplete session.".into()))
}

fn session_from(json: &Value) -> Option<Session> {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_string);
    let user = json.get("user")?;
    let meta = user.get("user_metadata");
    let meta_text = |key: &str| text(meta.and_then(|m| m.get(key)));
    let expires_at = json
        .get("expires_at")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| now() + json.get("expires_in").and_then(Value::as_u64).unwrap_or(3600));
    Some(Session {
        access_token: text(json.get("access_token"))?,
        refresh_token: text(json.get("refresh_token"))?,
        expires_at,
        profile: Profile {
            id: text(user.get("id"))?,
            name: meta_text("full_name")
                .or_else(|| meta_text("name"))
                .or_else(|| meta_text("user_name")),
            email: text(user.get("email")).filter(|e| !e.is_empty()),
            avatar_url: meta_text("avatar_url").or_else(|| meta_text("picture")),
            provider: text(user.get("app_metadata").and_then(|m| m.get("provider"))),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn the_challenge_is_the_s256_of_the_verifier() {
        let (verifier, challenge) = pkce_pair();
        assert_eq!(verifier.len(), 64);
        assert!(verifier.chars().all(|c| c.is_ascii_hexdigit()));
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
        assert_ne!(pkce_pair().0, verifier);
    }

    #[test]
    fn the_sign_in_page_comes_back_to_the_redirect() {
        let url = authorize_url(Provider::Google, "uk.thomast.myle.passwords://auth-callback", "abc").unwrap();
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(url.as_str().starts_with(&format!("{SUPABASE_URL}/auth/v1/authorize?")));
        assert!(query.contains(&("provider".into(), "google".into())));
        assert!(query.contains(&("redirect_to".into(), "uk.thomast.myle.passwords://auth-callback".into())));
        assert!(query.contains(&("code_challenge".into(), "abc".into())));
    }

    #[test]
    fn a_discord_session_becomes_a_profile() {
        let json = serde_json::json!({
            "access_token": "a", "refresh_token": "r", "expires_in": 3600, "expires_at": 2_000_000_000u64,
            "user": {
                "id": "uuid-1", "email": "me@example.com",
                "app_metadata": { "provider": "discord" },
                "user_metadata": { "full_name": "Thomas", "avatar_url": "https://cdn.discordapp.com/a.png" }
            }
        });
        let session = session_from(&json).unwrap();
        assert_eq!(session.expires_at, 2_000_000_000);
        assert_eq!(session.profile.name.as_deref(), Some("Thomas"));
        assert_eq!(session.profile.provider.as_deref(), Some("discord"));
        assert!(session_from(&serde_json::json!({ "access_token": "a" })).is_none());
    }

    fn session(refresh_token: &str, expires_at: u64) -> Session {
        Session {
            access_token: format!("access-{refresh_token}"),
            refresh_token: refresh_token.into(),
            expires_at,
            profile: Profile {
                id: "uuid-1".into(),
                name: None,
                email: None,
                avatar_url: None,
                provider: None,
            },
        }
    }

    #[tokio::test]
    async fn requests_at_once_share_one_refresh() {
        let stored = &Mutex::new(Some(session("old", 0)));
        let gate = tokio::sync::Mutex::new(());
        let calls = &std::sync::atomic::AtomicUsize::new(0);
        let current = || stored.lock().unwrap().clone();
        let store = |value| {
            *stored.lock().unwrap() = value;
            Ok(())
        };
        let refresh = |token: String| async move {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(token, "old");
            Ok(session("new", now() + 3600))
        };
        let (a, b) = tokio::join!(
            renew(&gate, current, store, refresh),
            renew(&gate, current, store, refresh)
        );
        assert_eq!(a.unwrap().refresh_token, "new");
        assert_eq!(b.unwrap().refresh_token, "new");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_late_refusal_keeps_a_session_signed_in_meanwhile() {
        let stored = &Mutex::new(Some(session("old", 0)));
        let gate = tokio::sync::Mutex::new(());
        let current = || stored.lock().unwrap().clone();
        let store = |value| {
            *stored.lock().unwrap() = value;
            Ok(())
        };
        let refused = renew(&gate, current, store, |_| async move {
            // The user signs in again while the old token is being refused.
            *stored.lock().unwrap() = Some(session("signed-in-again", now() + 3600));
            Err(TokenError::Rejected("invalid refresh token".into()))
        })
        .await;
        assert!(refused.is_err());
        assert_eq!(current().unwrap().refresh_token, "signed-in-again");

        // Refused with nothing newer: signed out.
        *stored.lock().unwrap() = Some(session("old", 0));
        let refused = renew(&gate, current, store, |_| async move {
            Err(TokenError::Rejected("invalid refresh token".into()))
        })
        .await;
        assert!(refused.is_err());
        assert!(current().is_none());
    }

    #[test]
    fn providers_come_from_the_page_as_camel_case() {
        let provider: Provider = serde_json::from_str("\"google\"").unwrap();
        assert_eq!(provider.id(), "google");
        assert!(serde_json::from_str::<Provider>("\"github\"").is_err());
    }

    #[test]
    fn a_saved_session_still_reads() {
        // The Windows app keeps sessions saved before this crate existed.
        let saved = r#"{"access_token":"a","refresh_token":"r","expires_at":5,
            "profile":{"id":"u","name":null,"email":null,"avatarUrl":null,"provider":"google"}}"#;
        let session: Session = serde_json::from_str(saved).unwrap();
        assert_eq!(session.profile.provider.as_deref(), Some("google"));
    }
}
