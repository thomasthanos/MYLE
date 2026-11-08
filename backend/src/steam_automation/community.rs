//! Signed-in requests to Steam Community and the store, one every few seconds.
use std::future::Future;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use super::session::Session;
const GAP: Duration = Duration::from_secs(3);
/// Price lookups are limited harder than pages, so they wait 4 seconds in all.
const PRICE_EXTRA: Duration = Duration::from_secs(1);
const COMMUNITY_ORIGIN: &str = "https://steamcommunity.com";
const STORE_HOST: &str = "store.steampowered.com";
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommunityError {
    SignedOut,
    Limited,
    Other(String),
}
pub(crate) trait Community: Send + Sync {
    fn get(&self, url: &str) -> impl Future<Output = Result<String, CommunityError>> + Send;
    fn post(&self, url: &str, form: &[(&str, String)], referer: &str) -> impl Future<Output = Result<String, CommunityError>> + Send;
}
/// Whether an answer can be used, and if not, why.
pub(crate) fn classify(status: u16, final_url: &str, body: &str) -> Result<(), CommunityError> {
    match status {
        401 | 403 => return Err(CommunityError::SignedOut),
        429 => return Err(CommunityError::Limited),
        200..=299 => {}
        other => return Err(CommunityError::Other(format!("Steam answered {other}."))),
    }
    if final_url.contains("/login") || body.contains("g_steamID = false") {
        return Err(CommunityError::SignedOut);
    }
    Ok(())
}
/// The cookies for this address: Steam gives the store its own.
pub(crate) fn cookie_for(url: &str, session: &Session) -> Option<String> {
    if url.contains(STORE_HOST) {
        return session.store_cookie_header().map(str::to_string);
    }
    Some(session.cookie_header())
}
pub(super) fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}
fn form_body(form: &[(&str, String)]) -> String {
    form.iter().map(|(key, value)| format!("{}={}", encode(key), encode(value))).collect::<Vec<_>>().join("&")
}
pub(crate) struct HttpCommunity {
    client: reqwest::Client,
    session: std::sync::Mutex<Option<Session>>,
    next: Mutex<Instant>,
}
impl HttpCommunity {
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self { client: crate::download::http_client("MYLE")?, session: std::sync::Mutex::new(None), next: Mutex::new(Instant::now()) })
    }
    pub(crate) fn set_session(&self, session: Option<Session>) {
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = session;
    }
    fn cookie(&self, url: &str) -> Result<String, CommunityError> {
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        session.as_ref().and_then(|session| cookie_for(url, session)).ok_or(CommunityError::SignedOut)
    }
    async fn wait_for_turn(&self) {
        let slot = {
            let mut next = self.next.lock().await;
            let slot = (*next).max(Instant::now());
            *next = slot + GAP;
            slot
        };
        tokio::time::sleep_until(slot).await;
    }
    async fn finish(&self, request: reqwest::RequestBuilder) -> Result<String, CommunityError> {
        let unreachable = |_| CommunityError::Other("Steam could not be reached.".to_string());
        let response = request.send().await.map_err(unreachable)?;
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let body = response.text().await.map_err(unreachable)?;
        classify(status, &final_url, &body)?;
        Ok(body)
    }
}
impl Community for HttpCommunity {
    async fn get(&self, url: &str) -> Result<String, CommunityError> {
        let cookie = self.cookie(url)?;
        self.wait_for_turn().await;
        if url.contains("priceoverview") {
            tokio::time::sleep(PRICE_EXTRA).await;
        }
        self.finish(self.client.get(url).header("Cookie", cookie)).await
    }
    async fn post(&self, url: &str, form: &[(&str, String)], referer: &str) -> Result<String, CommunityError> {
        let cookie = self.cookie(url)?;
        self.wait_for_turn().await;
        let request = self
            .client
            .post(url)
            .header("Cookie", cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Referer", referer)
            .header("Origin", COMMUNITY_ORIGIN)
            .body(form_body(form));
        self.finish(request).await
    }
}
#[cfg(test)]
pub(crate) use fake::FakeCommunity;
#[cfg(test)]
mod fake {
    use std::sync::Mutex;
    use super::*;
    type Answer = Box<dyn Fn(&str) -> Result<String, CommunityError> + Send + Sync>;
    /// A posted request: its address, its form and its referer.
    type Posted = (String, Vec<(String, String)>, String);
    pub(crate) struct FakeCommunity {
        answer: Answer,
        post_answer: Mutex<Option<Answer>>,
        gets: Mutex<Vec<String>>,
        posts: Mutex<Vec<Posted>>,
    }
    impl FakeCommunity {
        pub(crate) fn answering(answer: impl Fn(&str) -> Result<String, CommunityError> + Send + Sync + 'static) -> Self {
            Self { answer: Box::new(answer), post_answer: Mutex::new(None), gets: Mutex::default(), posts: Mutex::default() }
        }
        pub(crate) fn answering_posts(self, answer: impl Fn(&str) -> Result<String, CommunityError> + Send + Sync + 'static) -> Self {
            *self.post_answer.lock().unwrap() = Some(Box::new(answer));
            self
        }
        pub(crate) fn gets(&self) -> Vec<String> {
            self.gets.lock().unwrap().clone()
        }
        pub(crate) fn posts(&self) -> Vec<Posted> {
            self.posts.lock().unwrap().clone()
        }
    }
    impl Community for FakeCommunity {
        async fn get(&self, url: &str) -> Result<String, CommunityError> {
            self.gets.lock().unwrap().push(url.to_string());
            (self.answer)(url)
        }
        async fn post(&self, url: &str, form: &[(&str, String)], referer: &str) -> Result<String, CommunityError> {
            let form = form.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
            self.posts.lock().unwrap().push((url.to_string(), form, referer.to_string()));
            match self.post_answer.lock().unwrap().as_ref() {
                Some(answer) => answer(url),
                None => Ok(String::new()),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const PAGE: &str = "https://steamcommunity.com/profiles/1/badges/";
    #[test]
    fn form_values_are_encoded() {
        assert_eq!(form_body(&[("sessionid", "a b".to_string()), ("price", "27".to_string()), ("x", "é&".to_string())]), "sessionid=a%20b&price=27&x=%C3%A9%26");
    }
    #[test]
    fn a_normal_page_is_fine() {
        assert_eq!(classify(200, PAGE, "<html>badges</html>"), Ok(()));
    }
    #[test]
    fn being_sent_to_login_is_signed_out() {
        assert_eq!(classify(200, "https://steamcommunity.com/login/home/?goto=x", "<html>"), Err(CommunityError::SignedOut));
    }
    #[test]
    fn a_page_for_nobody_is_signed_out() {
        assert_eq!(classify(200, PAGE, "var g_steamID = false;"), Err(CommunityError::SignedOut));
    }
    #[test]
    fn unauthorised_is_signed_out() {
        assert_eq!(classify(401, PAGE, ""), Err(CommunityError::SignedOut));
        assert_eq!(classify(403, PAGE, ""), Err(CommunityError::SignedOut));
    }
    #[test]
    fn too_many_requests_is_limited() {
        assert_eq!(classify(429, PAGE, ""), Err(CommunityError::Limited));
    }
    #[test]
    fn other_failures_say_their_status() {
        assert_eq!(classify(500, PAGE, ""), Err(CommunityError::Other("Steam answered 500.".to_string())));
    }
    #[test]
    fn the_store_gets_its_own_cookies() {
        use crate::steam_automation::session::from_cookies;
        let pairs = [("steamLoginSecure".to_string(), "1%7C%7Ca".to_string()), ("sessionid".to_string(), "b".to_string())];
        let store = [("steamLoginSecure".to_string(), "1%7C%7Cc".to_string()), ("sessionid".to_string(), "d".to_string())];
        let session = from_cookies(&pairs).unwrap().with_store_cookies(&store);
        assert_eq!(cookie_for("https://steamcommunity.com/x", &session).as_deref(), Some("steamLoginSecure=1%7C%7Ca; sessionid=b"));
        assert_eq!(cookie_for("https://store.steampowered.com/x", &session).as_deref(), Some("steamLoginSecure=1%7C%7Cc; sessionid=d"));
    }
}
