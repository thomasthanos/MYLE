//! The owner-only pages (GitHub Releases, Project Backups): open only to the
//! app's owner, signed in with Discord under the owner's verified email.
//!
//! Who that is comes from Supabase, never from the page: after a sign-in, and
//! again on each start, the backend asks Supabase's `auth/v1/user` with the
//! session's access token (which Supabase checks) and keeps the verdict for
//! that user id. The verdict is kept encrypted (DPAPI) next to the session so
//! an offline start still knows it. No command lets the page set it: the
//! page can only ask for it to be checked again.
//!
//! Every command of those pages is checked by [`is_owner_only_command`] in the app's invoke
//! handler, so a command added later is covered without remembering to.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The one account the owner-only pages open for.
pub(crate) const OWNER_EMAIL: &str = "thomasthanos28@gmail.com";
/// The sign-in provider the owner has to use.
const OWNER_PROVIDER: &str = "discord";
/// The commands of the owner-only pages, by name prefix.
const OWNER_ONLY_COMMANDS: &[&str] = &["github_releases_", "project_backups_"];
/// How long a verdict counts when Supabase can't be reached to check again.
pub(crate) const OFFLINE_GRACE_SECS: u64 = 30 * 24 * 3600;
/// A verdict newer than this is not checked again when the page asks.
pub(crate) const RECHECK_AFTER_SECS: u64 = 10 * 60;

/// What Supabase said about a user, kept for that user only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Verdict {
    pub user_id: String,
    pub owner: bool,
    /// Unix seconds of the check.
    pub checked_at: u64,
}

impl Verdict {
    /// Whether this verdict lets `user_id` in at `now`.
    pub(crate) fn admits(&self, user_id: &str, now: u64) -> bool {
        self.owner
            && self.user_id == user_id
            && now.saturating_sub(self.checked_at) <= OFFLINE_GRACE_SECS
    }

    pub(crate) fn is_recent(&self, user_id: &str, now: u64) -> bool {
        self.user_id == user_id && now.saturating_sub(self.checked_at) < RECHECK_AFTER_SECS
    }
}

/// Whether the command is one of the owner-only pages'.
pub(crate) fn is_owner_only_command(command: &str) -> bool {
    OWNER_ONLY_COMMANDS
        .iter()
        .any(|prefix| command.starts_with(prefix))
}

/// Whether Supabase's user object (`GET auth/v1/user`) is the owner's: the
/// owner's email (any case), confirmed by Supabase, signed up with Discord,
/// and a Discord identity carrying that same email, verified by Discord.
pub(crate) fn is_owner(user: &Value) -> bool {
    fn text(value: Option<&Value>) -> Option<&str> {
        value.and_then(Value::as_str).map(str::trim)
    }
    let is_owner_email =
        |value: Option<&Value>| text(value).is_some_and(|e| e.eq_ignore_ascii_case(OWNER_EMAIL));

    let email_confirmed = text(user.get("email_confirmed_at")).is_some_and(|at| !at.is_empty());
    let app = user.get("app_metadata");
    let signed_up_with_discord = text(app.and_then(|m| m.get("provider"))) == Some(OWNER_PROVIDER);
    let discord_identity = user
        .get("identities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|identity| text(identity.get("provider")) == Some(OWNER_PROVIDER))
        .any(|identity| {
            let data = identity.get("identity_data");
            is_owner_email(data.and_then(|d| d.get("email")))
                && data
                    .and_then(|d| d.get("email_verified"))
                    .and_then(Value::as_bool)
                    == Some(true)
        });
    is_owner_email(user.get("email"))
        && email_confirmed
        && signed_up_with_discord
        && discord_identity
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn owner_user() -> Value {
        json!({
            "id": "uuid-owner",
            "email": "thomasthanos28@gmail.com",
            "email_confirmed_at": "2025-01-02T03:04:05Z",
            "app_metadata": { "provider": "discord", "providers": ["discord"] },
            "user_metadata": { "email": "thomasthanos28@gmail.com", "email_verified": true },
            "identities": [{
                "provider": "discord",
                "identity_data": { "email": "thomasthanos28@gmail.com", "email_verified": true }
            }]
        })
    }

    #[test]
    fn the_owner_signed_in_with_discord_gets_in() {
        assert!(is_owner(&owner_user()));
    }

    #[test]
    fn the_email_is_compared_without_case() {
        let mut user = owner_user();
        user["email"] = json!("ThomasThanos28@Gmail.com");
        user["identities"][0]["identity_data"]["email"] = json!(" THOMASTHANOS28@GMAIL.COM ");
        assert!(is_owner(&user));
    }

    #[test]
    fn someone_else_does_not() {
        let mut user = owner_user();
        user["email"] = json!("someone@example.com");
        user["identities"][0]["identity_data"]["email"] = json!("someone@example.com");
        assert!(!is_owner(&user));
        // A look-alike address.
        let mut user = owner_user();
        user["email"] = json!("thomasthanos28@gmail.com.evil.example");
        assert!(!is_owner(&user));
        assert!(!is_owner(&json!({})));
        assert!(!is_owner(&json!(null)));
    }

    #[test]
    fn an_unconfirmed_email_does_not() {
        let mut user = owner_user();
        user["email_confirmed_at"] = Value::Null;
        assert!(!is_owner(&user));
        let mut user = owner_user();
        user["identities"][0]["identity_data"]["email_verified"] = json!(false);
        assert!(!is_owner(&user));
        let mut user = owner_user();
        user["identities"][0]["identity_data"]
            .as_object_mut()
            .unwrap()
            .remove("email_verified");
        assert!(!is_owner(&user));
    }

    #[test]
    fn the_same_email_through_google_does_not() {
        let mut user = owner_user();
        user["app_metadata"]["provider"] = json!("google");
        user["identities"][0]["provider"] = json!("google");
        assert!(!is_owner(&user));
        // Signed up with Google, Discord linked later: still not a Discord sign-up.
        let mut user = owner_user();
        user["app_metadata"]["provider"] = json!("google");
        assert!(!is_owner(&user));
        // A Discord sign-up without a Discord identity carrying the email.
        let mut user = owner_user();
        user["identities"] = json!([]);
        assert!(!is_owner(&user));
    }

    #[test]
    fn only_the_owner_only_pages_commands_are_gated() {
        assert!(is_owner_only_command("github_releases_get_state"));
        assert!(is_owner_only_command("project_backups_backup"));
        assert!(!is_owner_only_command("account_profile"));
        assert!(!is_owner_only_command("game_saves_restore"));
        assert!(!is_owner_only_command("github_release"));
        assert!(!is_owner_only_command("account_access"));
        assert!(!is_owner_only_command("Github_releases_commit"));
    }

    /// Every command those pages register in the app is under the gate.
    #[test]
    fn every_command_of_the_owner_only_pages_is_gated() {
        let registered: Vec<&str> = include_str!("../lib.rs")
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with("github_releases::") || line.starts_with("project_backups::")
            })
            .filter_map(|line| line.trim_end_matches(',').rsplit("::").next())
            .collect();
        assert!(registered.len() > 50, "{registered:?}");
        for command in registered {
            assert!(is_owner_only_command(command), "{command} is not gated");
        }
    }

    #[test]
    fn a_verdict_is_for_its_user_and_lasts_a_while_offline() {
        let verdict = Verdict {
            user_id: "uuid-owner".into(),
            owner: true,
            checked_at: 1_000_000,
        };
        assert!(verdict.admits("uuid-owner", 1_000_000));
        assert!(!verdict.admits("uuid-other", 1_000_000));
        assert!(verdict.admits("uuid-owner", 1_000_000 + OFFLINE_GRACE_SECS));
        assert!(!verdict.admits("uuid-owner", 1_000_000 + OFFLINE_GRACE_SECS + 1));
        let refused = Verdict {
            owner: false,
            ..verdict.clone()
        };
        assert!(!refused.admits("uuid-owner", 1_000_000));
        assert!(verdict.is_recent("uuid-owner", 1_000_000 + RECHECK_AFTER_SECS - 1));
        assert!(!verdict.is_recent("uuid-owner", 1_000_000 + RECHECK_AFTER_SECS));
        assert!(!verdict.is_recent("uuid-other", 1_000_000));
    }
}
