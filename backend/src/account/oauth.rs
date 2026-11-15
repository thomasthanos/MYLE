// ! Sign-in in the user's own browser (RFC 8252): a one-shot loopback ! server that receives Supabase's redirect (PKCE: `myle_vault::account`). ! ! The browser, not a webview, shows the provider's page: Google refuses ! embedded browsers, and the user is usually signed in there already.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const COMPLETE_URL: &str = "https://make-your-life-easier.pages.dev/sign-in/complete/";
const FAILED_URL: &str = "https://make-your-life-easier.pages.dev/sign-in/failed/";

/// What came back from the provider on the redirect.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum Callback {
    Code(String),
    Error(String),
}

pub struct BrowserCallback {
    callback: Callback,
    stream: TcpStream,
}

impl BrowserCallback {
    pub fn code(&self) -> Result<&str, String> {
        match &self.callback {
            Callback::Code(code) => Ok(code),
            Callback::Error(message) => Err(message.clone()),
        }
    }

    /// Only the outcome goes to the public website. Suppress the referrer
    /// because the incoming loopback URL contains the one-time code.
    pub async fn finish(mut self, signed_in: bool) {
        let location = if signed_in { COMPLETE_URL } else { FAILED_URL };
        let response = format!(
            "HTTP/1.1 303 See Other\r\nLocation: {location}\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = self.stream.write_all(response.as_bytes()).await;
        let _ = self.stream.shutdown().await;
    }
}
/// Listens on `localhost:<port>`, both IPv4 and IPv6: browsers try either.
pub struct Loopback {
    v4: TcpListener,
    v6: Option<TcpListener>,
}
impl Loopback {
    pub async fn bind(port: u16) -> Result<Self, String> {
        let v4 = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
            format!("Sign-in needs port {port} on this PC, but it is in use ({e}). Close the program using it and try again.")
        })?;
        let v6 = TcpListener::bind(("::1", port)).await.ok();
        Ok(Self { v4, v6 })
    }
    /// Answers requests until one carries a code or an error. Anything else
    /// (a favicon, a stray visit) gets a 404 and is ignored. Every connection
    /// is served on its own: browsers open spare connections that may never
    /// send a request, and one of those must not hold up the real redirect.
    pub async fn next_callback(&self) -> BrowserCallback {
        let (found, mut callbacks) = tokio::sync::mpsc::channel(1);
        // Request tasks belong to this attempt. Dropping the callback future
        // (cancel/timeout) or accepting its result also closes spare sockets.
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let accepted = tokio::select! {
                Some(callback) = callbacks.recv() => return callback,
                _ = connections.join_next(), if !connections.is_empty() => continue,
                accepted = self.v4.accept() => accepted,
                accepted = accept_optional(self.v6.as_ref()) => accepted,
            };
            let Ok((stream, _)) = accepted else {
                continue;
            };
            connections.spawn(serve(stream, found.clone()));
        }
    }
}
async fn accept_optional(
    listener: Option<&TcpListener>,
) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}
/// Gives the accepted browser connection to the sign-in attempt so its
/// response can reflect the result of exchanging the code and saving the session.
async fn serve(mut stream: TcpStream, found: tokio::sync::mpsc::Sender<BrowserCallback>) {
    let Some((target, opened_as_page)) = read_request(&mut stream).await else {
        return;
    };
    // The redirect opens a page in the tab. A web page elsewhere could load this address unseen (an image, a frame, a fetch), and that must not end the sign-in with a made-up error.
    if !opened_as_page {
        not_found(&mut stream).await;
        return;
    }
    if let Some(callback) = parse_callback(&target) {
        let _ = found.send(BrowserCallback { callback, stream }).await;
    } else {
        not_found(&mut stream).await;
    }
}
/// `/?code=…` or `/?error=…&error_description=…` from the request target.
fn parse_callback(target: &str) -> Option<Callback> {
    let url = reqwest::Url::parse(&format!("http://localhost{target}")).ok()?;
    if url.path() != "/" {
        return None;
    }
    let mut code = None;
    let mut error = None;
    let mut description = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if !value.is_empty() => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => description = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(message) = description.or(error) {
        return Some(Callback::Error(message));
    }
    code.map(Callback::Code)
}
/// The request's target, and whether the browser opened it as a page (by
/// `Sec-Fetch-Dest`; a browser too old to send it counts as a page).
async fn read_request(stream: &mut TcpStream) -> Option<(String, bool)> {
    let mut request = Vec::new();
    let mut buffer = [0u8; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .ok()?
            .ok()?;
        if read == 0 || request.len() > 16 * 1024 {
            return None;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.lines();
    let mut parts = lines.next()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?.to_string();
    Some((target, opened_as_page(lines)))
}
fn opened_as_page<'a>(headers: impl Iterator<Item = &'a str>) -> bool {
    headers
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("sec-fetch-dest"))
        .is_none_or(|(_, value)| value.trim().eq_ignore_ascii_case("document"))
}
async fn not_found(stream: &mut TcpStream) {
    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").await;
    let _ = stream.shutdown().await;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_are_read_for_a_code_or_an_error() {
        assert_eq!(
            parse_callback("/?code=abc-123"),
            Some(Callback::Code("abc-123".into()))
        );
        assert_eq!(
            parse_callback("/?error=access_denied&error_description=The+user+cancelled"),
            Some(Callback::Error("The user cancelled".into()))
        );
        assert_eq!(parse_callback("/favicon.ico"), None);
        assert_eq!(parse_callback("/"), None);
    }
    #[test]
    fn only_a_page_the_browser_opens_counts() {
        assert!(opened_as_page(["Host: localhost", "Sec-Fetch-Dest: document"].into_iter()));
        assert!(opened_as_page(["Host: localhost"].into_iter()), "an older browser");
        assert!(!opened_as_page(["sec-fetch-dest: image"].into_iter()));
        assert!(!opened_as_page(["Sec-Fetch-Dest: empty"].into_iter()), "fetch()");
        assert!(!opened_as_page(["Sec-Fetch-Dest: script"].into_iter()));
        assert!(!opened_as_page(["Sec-Fetch-Dest: iframe"].into_iter()), "a hidden frame");
    }
    /// Another website loading `localhost:5252/?error=…` in the background
    /// cannot cut a sign-in short; the real redirect still gets through.
    #[tokio::test]
    async fn a_background_request_cannot_end_the_sign_in() {
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        tokio::spawn(async move {
            for (target, dest) in [("/?error=access_denied", "image"), ("/?code=real", "document")] {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nSec-Fetch-Dest: {dest}\r\n\r\n");
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut reply = Vec::new();
                let _ = stream.read_to_end(&mut reply).await;
            }
        });
        let callback = tokio::time::timeout(std::time::Duration::from_secs(2), server.next_callback())
            .await
            .unwrap();
        assert_eq!(callback.code(), Ok("real"));
        callback.finish(true).await;
    }
    #[tokio::test]
    async fn an_idle_browser_connection_does_not_hold_up_the_redirect() {
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        // A speculative connection that never sends anything, opened first.
        let mut idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream
                .write_all(b"GET /?code=fast HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut reply = Vec::new();
            let _ = stream.read_to_end(&mut reply).await;
        });
        let callback =
            tokio::time::timeout(std::time::Duration::from_secs(2), server.next_callback())
                .await
                .expect("the idle connection must not delay the real one");
        assert_eq!(callback.code(), Ok("fast"));
        callback.finish(true).await;
        let mut byte = [0];
        let closed = tokio::time::timeout(std::time::Duration::from_secs(2), idle.read(&mut byte))
            .await.expect("finishing sign-in must close spare connections");
        assert!(matches!(closed, Ok(0) | Err(_)));
    }

    #[tokio::test]
    async fn cancelling_a_sign_in_closes_its_accepted_browser_connections() {
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        let mut idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let waiting = tokio::spawn(async move { server.next_callback().await });
        // A later, answered request proves the earlier connection was accepted.
        let mut probe = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        probe.write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut reply = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(2), probe.read_to_end(&mut reply)).await.unwrap().unwrap();
        assert!(reply.starts_with(b"HTTP/1.1 404"));
        waiting.abort();
        assert!(matches!(waiting.await, Err(error) if error.is_cancelled()));
        let mut byte = [0];
        let closed = tokio::time::timeout(std::time::Duration::from_secs(2), idle.read(&mut byte))
            .await.expect("cancelling sign-in must drop its request handlers");
        assert!(matches!(closed, Ok(0) | Err(_)));
    }
    #[tokio::test]
    async fn the_browser_waits_for_the_sign_in_result_before_opening_the_site() {
        // Port 0 picks a free port, but only the IPv4 socket knows it.
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        let mut browser = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        browser.write_all(b"GET /?code=one-time-test-code HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let callback = server.next_callback().await;
        assert_eq!(callback.code(), Ok("one-time-test-code"));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(25), browser.read(&mut [0]))
            .await.is_err());
        callback.finish(true).await;
        let mut reply = String::new();
        browser.read_to_string(&mut reply).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 303"));
        assert!(reply.contains("Location: https://make-your-life-easier.pages.dev/sign-in/complete/\r\n"));
        assert!(reply.contains("Referrer-Policy: no-referrer\r\n"));
        assert!(!reply.contains("one-time-test-code"));
    }

    #[tokio::test]
    async fn rejected_sign_in_opens_the_failure_page_without_provider_details() {
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        let mut browser = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        browser.write_all(b"GET /?error=access_denied&error_description=private-details HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let callback = server.next_callback().await;
        assert_eq!(callback.code(), Err("private-details".into()));
        callback.finish(false).await;
        let mut reply = String::new();
        browser.read_to_string(&mut reply).await.unwrap();
        assert!(reply.contains("Location: https://make-your-life-easier.pages.dev/sign-in/failed/\r\n"));
        assert!(!reply.contains("private-details"));
    }
}
