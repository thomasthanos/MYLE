//! Sign-in in the user's own browser (RFC 8252): a one-shot loopback
//! server that receives Supabase's redirect (PKCE: `myle_vault::account`).
//!
//! The browser, not a webview, shows the provider's page: Google refuses
//! embedded browsers, and the user is usually signed in there already.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What came back from the provider on the redirect.
#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Error(String),
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
    pub async fn next_callback(&self) -> Callback {
        let (found, mut callbacks) = tokio::sync::mpsc::channel::<Callback>(1);
        loop {
            let accepted = tokio::select! {
                Some(callback) = callbacks.recv() => return callback,
                accepted = self.v4.accept() => accepted,
                accepted = accept_optional(self.v6.as_ref()) => accepted,
            };
            let Ok((stream, _)) = accepted else {
                continue;
            };
            tokio::spawn(serve(stream, found.clone()));
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

/// Answers one request. A code or an error is handed on only after the
/// browser has its page, so the tab never hangs on a closed app.
async fn serve(mut stream: TcpStream, found: tokio::sync::mpsc::Sender<Callback>) {
    let Some((target, opened_as_page)) = read_request(&mut stream).await else {
        return;
    };
    // The redirect opens a page in the tab. A web page elsewhere could load
    // this address unseen (an image, a frame, a fetch), and that must not end
    // the sign-in with a made-up error.
    if !opened_as_page {
        respond(&mut stream, 404, "").await;
        return;
    }
    match parse_callback(&target) {
        Some(Callback::Code(code)) => {
            respond(&mut stream, 200, &page(true, "")).await;
            let _ = found.send(Callback::Code(code)).await;
        }
        Some(Callback::Error(message)) => {
            respond(&mut stream, 200, &page(false, &message)).await;
            let _ = found.send(Callback::Error(message)).await;
        }
        None => respond(&mut stream, 404, "").await,
    }
}

/// `/?code=…` or `/?error=…&error_description=…` from the request target.
pub fn parse_callback(target: &str) -> Option<Callback> {
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

async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// The page left in the browser tab after the redirect.
fn page(ok: bool, message: &str) -> String {
    let (title, text) = if ok {
        (
            "You're signed in",
            "You can close this tab and go back to Make Your Life Easier.".to_string(),
        )
    } else {
        ("Sign-in didn't finish", escape(message))
    };
    let accent = if ok { "#8b97ff" } else { "#ff8b8d" };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{title}</title>
<style>
html,body{{height:100%;margin:0}}
body{{display:grid;place-items:center;background:radial-gradient(circle at 30% 20%,#1b2040,#0a0c12 70%);color:#e8eaf6;font:15px/1.5 "Segoe UI",system-ui,sans-serif}}
.card{{max-width:420px;padding:32px 36px;border:1px solid rgba(255,255,255,.1);border-radius:18px;background:rgba(255,255,255,.05);box-shadow:0 24px 60px -24px rgba(0,0,0,.8);text-align:center}}
h1{{margin:0 0 8px;font-size:20px;color:{accent}}}
p{{margin:0;color:#aab0c8}}
</style></head><body><div class="card"><h1>{title}</h1><p>{text}</p></div></body></html>"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
        assert_eq!(callback, Callback::Code("real".into()));
    }

    #[test]
    fn error_text_cannot_inject_markup() {
        assert!(!page(false, "<script>x</script>").contains("<script>x"));
    }

    #[tokio::test]
    async fn an_idle_browser_connection_does_not_hold_up_the_redirect() {
        let server = Loopback::bind(0).await.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        // A speculative connection that never sends anything, opened first.
        let _idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
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
        assert_eq!(callback, Callback::Code("fast".into()));
    }

    #[tokio::test]
    async fn the_loopback_server_hands_over_the_code_and_answers_the_browser() {
        let server = Loopback::bind(0).await;
        // Port 0 picks a free port, but only the IPv4 socket knows it.
        let server = server.unwrap();
        let port = server.v4.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream
                .write_all(b"GET /?code=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).await.unwrap();
            reply
        });
        assert_eq!(server.next_callback().await, Callback::Code("xyz".into()));
        let reply = browser.await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(reply.contains("signed in"));
    }
}
