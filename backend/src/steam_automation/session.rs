//! The Steam Community session: only its cookies, in memory and sealed with DPAPI on disk.
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::account::vault;
const LOGIN_COOKIE: &str = "steamLoginSecure";
const SESSION_COOKIE: &str = "sessionid";
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Session {
    pub steam_login_secure: String,
    pub session_id: String,
    pub steam_id: u64,
    #[serde(default)]
    store_cookie: Option<String>,
}
/// Prints the account only, so a session can never reach a log.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("steam_id", &self.steam_id).finish_non_exhaustive()
    }
}
fn cookie<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}
/// The session in these Steam Community cookies, if they hold one.
pub(crate) fn from_cookies(pairs: &[(String, String)]) -> Option<Session> {
    let steam_login_secure = cookie(pairs, LOGIN_COOKIE)?.to_string();
    let session_id = cookie(pairs, SESSION_COOKIE)?.to_string();
    let digits = steam_login_secure.split(['%', '|']).next()?;
    let steam_id = digits.parse().ok()?;
    Some(Session { steam_login_secure, session_id, steam_id, store_cookie: None })
}
/// Reads a sealed session. `None` when there is none, or it was sealed by another user or PC.
pub(crate) fn load(path: &Path) -> Option<Session> {
    serde_json::from_slice(&vault::load(path)?).ok()
}
impl Session {
    /// Steam gives the store its own login cookie.
    pub(crate) fn with_store_cookies(mut self, pairs: &[(String, String)]) -> Self {
        self.store_cookie = cookie(pairs, LOGIN_COOKIE)
            .zip(cookie(pairs, SESSION_COOKIE))
            .map(|(login, session)| format!("{LOGIN_COOKIE}={login}; {SESSION_COOKIE}={session}"));
        self
    }
    /// Keeps the store cookie of an earlier session when this one has none.
    pub(crate) fn keeping_store_cookie_of(mut self, earlier: Option<&Session>) -> Self {
        if self.store_cookie.is_none() {
            self.store_cookie = earlier.and_then(|earlier| earlier.store_cookie.clone());
        }
        self
    }
    pub(crate) fn cookie_header(&self) -> String {
        format!("{LOGIN_COOKIE}={}; {SESSION_COOKIE}={}", self.steam_login_secure, self.session_id)
    }
    pub(crate) fn store_cookie_header(&self) -> Option<&str> {
        self.store_cookie.as_deref()
    }
    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        vault::save(path, &serde_json::to_vec(self).map_err(|e| e.to_string())?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }
    fn signed_in() -> Vec<(String, String)> {
        vec![pair("steamLoginSecure", "76561198000000001%7C%7Ctoken"), pair("sessionid", "abc"), pair("other", "x")]
    }
    #[test]
    fn cookies_give_a_session() {
        let session = from_cookies(&signed_in()).unwrap();
        assert_eq!((session.steam_id, session.session_id.as_str()), (76561198000000001, "abc"));
        assert_eq!(session.cookie_header(), "steamLoginSecure=76561198000000001%7C%7Ctoken; sessionid=abc");
    }
    #[test]
    fn a_plain_separator_also_gives_the_id() {
        let session = from_cookies(&[pair("steamLoginSecure", "76561198000000001||token"), pair("sessionid", "abc")]).unwrap();
        assert_eq!(session.steam_id, 76561198000000001);
    }
    #[test]
    fn a_missing_cookie_is_no_session() {
        assert!(from_cookies(&[pair("sessionid", "abc")]).is_none());
        assert!(from_cookies(&[pair("steamLoginSecure", "76561198000000001%7C%7Ctoken")]).is_none());
    }
    #[test]
    fn a_cookie_without_an_id_is_no_session() {
        assert!(from_cookies(&[pair("steamLoginSecure", "garbage"), pair("sessionid", "abc")]).is_none());
    }
    #[test]
    fn the_store_has_its_own_cookies() {
        let session = from_cookies(&signed_in()).unwrap().with_store_cookies(&[pair("steamLoginSecure", "76561198000000001%7C%7Cstore"), pair("sessionid", "def")]);
        assert_eq!(session.store_cookie_header(), Some("steamLoginSecure=76561198000000001%7C%7Cstore; sessionid=def"));
    }
    #[test]
    fn a_session_without_a_store_cookie_keeps_the_earlier_one() {
        let earlier = from_cookies(&signed_in()).unwrap().with_store_cookies(&[pair("steamLoginSecure", "76561198000000001%7C%7Cstore"), pair("sessionid", "def")]);
        let newer = from_cookies(&signed_in()).unwrap().keeping_store_cookie_of(Some(&earlier));
        assert_eq!(newer.store_cookie_header(), earlier.store_cookie_header());
    }
    #[test]
    fn a_newer_store_cookie_wins() {
        let earlier = from_cookies(&signed_in()).unwrap().with_store_cookies(&[pair("steamLoginSecure", "1%7C%7Cold"), pair("sessionid", "o")]);
        let newer = from_cookies(&signed_in()).unwrap().with_store_cookies(&[pair("steamLoginSecure", "1%7C%7Cnew"), pair("sessionid", "n")]).keeping_store_cookie_of(Some(&earlier));
        assert_eq!(newer.store_cookie_header(), Some("steamLoginSecure=1%7C%7Cnew; sessionid=n"));
    }
    #[test]
    fn no_store_cookies_means_no_store_header() {
        let session = from_cookies(&signed_in()).unwrap().with_store_cookies(&[pair("other", "x")]);
        assert_eq!(session.store_cookie_header(), None);
    }
    #[test]
    fn a_session_is_not_stored_in_plain_text() {
        let dir = std::env::temp_dir().join(format!("myle-session-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.bin");
        let session = from_cookies(&signed_in()).unwrap();
        session.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(5).any(|w| w == b"token"));
        assert_eq!(load(&path), Some(session));
    }
    #[test]
    fn nothing_saved_is_no_session() {
        assert_eq!(load(std::path::Path::new("Z:/does/not/exist.bin")), None);
    }
}
