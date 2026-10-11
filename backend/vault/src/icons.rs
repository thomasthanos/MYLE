//! Website icons for the vault's list.
//!
//! The app asks each website itself, so no icon service learns which sites
//! the vault holds, and it asks only public names over https, never an
//! address of this device or the local network. What comes back is shown only
//! if its first bytes are an image's (never trusting what the server says it
//! is), and it is kept in the vault's icon cache, sealed with the vault key
//! (`vault.rs`), so the cache on disk does not give the sites away either.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use reqwest::Url;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use serde::Serialize;

use crate::VaultCell;
use crate::vault::CachedIcon;

/// Sent to the page for each icon fetched in the background.
pub const ICON_EVENT: &str = "passwords-icon";
/// A found icon is fetched again after a month; a failed request, five minutes.
const KEEP_FOUND: u64 = 30 * 24 * 3600;
const KEEP_MISSING: u64 = 5 * 60;
const DISCOVERY_REVISION: u8 = 2;
/// Read of a home page: enough for its `<head>`.
const MAX_PAGE: usize = 512 * 1024;
const MAX_ICON: usize = 200 * 1024;
/// Websites asked at the same time.
const AT_ONCE: usize = 6;
static FETCHING: AtomicBool = AtomicBool::new(false);

/// An icon fetched in the background, sent to the page as `ICON_EVENT`.
#[derive(Clone, Serialize)]
pub struct Fetched {
    pub host: String,
    pub icon: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The host whose icon an entry's address shows: a public name (no IP
/// address, no `localhost`, no name only the local network or Tor knows,
/// which asking for would only leak the name).
pub fn icon_host(url: &str) -> Option<String> {
    let host = crate::sites::saved_host(url)?;
    public_name(&host).then_some(host)
}

fn public_name(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() || host.starts_with('[') || !host.contains('.') {
        return false;
    }
    let top = host.trim_end_matches('.').rsplit('.').next().unwrap_or_default();
    if matches!(top, "arpa" | "onion" | "localhost" | "local" | "internal") {
        return false;
    }
    psl::suffix(host.as_bytes()).is_some_and(|suffix| suffix.is_known())
}

/// Whether the app may ask `url`: https, to a public name. Checked for every
/// address asked, the site's own and those its answers point to (redirects,
/// its page's links), so none of them can send the app to this PC or the
/// local network.
fn may_ask(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(public_name)
}

/// Whether `ip` is on the internet: not this PC, not the local network, not
/// an address set aside for anything else.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                // Carrier-grade NAT, the IETF's own block, benchmarking, reserved.
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            // NAT64 (`64:ff9b::/96`) reaches the IPv4 address it carries.
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
                let v4 = (u32::from(s[6]) << 16) | u32::from(s[7]);
                return is_public(IpAddr::V4(Ipv4Addr::from(v4)));
            }
            !(s[..6] == [0; 6] // unspecified, loopback, the old IPv4-compatible ones
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
                || (s[0] & 0xffc0) == 0xfec0 // site-local
                || (s[0] == 0x2001 && s[1] == 0x0db8)) // documentation
        }
    }
}

/// Resolves only to public addresses: a public name that points at this PC
/// or the local network (a rebinding trick) is never asked.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect();
            if found.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            Ok(Box::new(found.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The websites whose icons are missing or old, to fetch in the background
/// with `fetch`. One fetch runs at a time: while it does, `lookup` hands out
/// no other.
pub struct Pending {
    hosts: Vec<String>,
}

impl Pending {
    /// Fetches the icons, keeps them in the vault's icon cache and passes
    /// each one found to `on_icon`.
    pub async fn fetch(self, state: &VaultCell, on_icon: impl Fn(Fetched)) {
        let mut hosts = self.hosts.clone();
        let mut attempted = HashSet::new();
        loop {
            attempted.extend(hosts.iter().cloned());
            if !fetch_all(state, &hosts, &on_icon).await { break; }
            // A login added while this batch was running is picked up too.
            // Each site is attempted once, even if a very large batch outlasts
            // the negative cache's lifetime.
            match cached_icons(state, &HashSet::new()) {
                Ok((_, wanted)) => {
                    hosts = wanted.into_iter().filter(|host| !attempted.contains(host)).collect();
                    if hosts.is_empty() { break; }
                }
                _ => break,
            }
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        FETCHING.store(false, Ordering::SeqCst);
    }
}

/// The icons already known for the vault's websites, but those the page has
/// (`skip`), and the missing and old ones to fetch (see `Pending`).
pub fn lookup(state: &VaultCell, skip: Vec<String>) -> Result<(HashMap<String, String>, Option<Pending>), String> {
    let skip: HashSet<String> = skip.into_iter().collect();
    let (known, wanted) = cached_icons(state, &skip)?;
    // Construct the owner only when the gate was acquired. An eagerly made
    // Pending would release somebody else's gate in Drop on a skipped lookup.
    let pending = (!wanted.is_empty() && !FETCHING.swap(true, Ordering::SeqCst))
        .then(|| Pending { hosts: wanted });
    Ok((known, pending))
}

fn cached_icons(state: &VaultCell, skip: &HashSet<String>) -> Result<(HashMap<String, String>, Vec<String>), String> {
    state.with_quiet(|vault| {
        if !vault.prefs().website_icons {
            return Ok((HashMap::new(), Vec::new()));
        }
        let hosts: BTreeSet<String> = vault
            .summaries()?
            .iter()
            .flat_map(|entry| entry.urls.iter().filter_map(|url| icon_host(url)))
            .collect();
        let now = now();
        let cache = vault.icons()?;
        let mut known = HashMap::new();
        let mut wanted = Vec::new();
        for host in hosts {
            if let Some(icon) = bundled_icon(&host) {
                if cache.icons.get(&host).and_then(|cached| cached.data.as_ref()) != Some(&icon) {
                    wanted.push(host.clone());
                }
                if !skip.contains(&host) {
                    known.insert(host, icon);
                }
                continue;
            }
            match cache.icons.get(&host) {
                Some(cached) => {
                    if let Some(data) = &cached.data
                        && !skip.contains(&host)
                    {
                        known.insert(host.clone(), data.clone());
                    }
                    if needs_refresh(cached, now) {
                        wanted.push(host);
                    }
                }
                None => wanted.push(host),
            }
        }
        Ok((known, wanted))
    })
}

fn needs_refresh(cached: &CachedIcon, now: u64) -> bool {
    let missing = cached.data.is_none();
    (missing && cached.revision < DISCOVERY_REVISION)
        || now.saturating_sub(cached.at) >= if missing { KEEP_MISSING } else { KEEP_FOUND }
}

async fn fetch_all(state: &VaultCell, hosts: &[String], on_icon: impl Fn(Fetched)) -> bool {
    let Ok(client) = client() else { return false };
    let mut results = futures_util::stream::iter(hosts.iter().cloned())
        .map(|host| {
            let client = client.clone();
            async move {
                let icon = fetch_icon(&client, &host).await;
                (host, icon)
            }
        })
        .buffer_unordered(AT_ONCE);
    let mut stored_since_save = 0;
    while let Some((host, icon)) = results.next().await {
        let stored = state.with_quiet(|vault| {
            if !vault.prefs().website_icons {
                return Err("Website icons are off.".to_string());
            }
            let cache = vault.icons()?;
            // A website that is down just now keeps the icon it had.
            let data = icon
                .clone()
                .or_else(|| cache.icons.get(&host).and_then(|old| old.data.clone()));
            cache.icons.insert(host.clone(), CachedIcon { data, at: now(), revision: DISCOVERY_REVISION });
            Ok(())
        });
        // Locked, or the icons were turned off: stop, and keep nothing.
        if stored.is_err() {
            return false;
        }
        if let Some(icon) = icon {
            on_icon(Fetched { host, icon });
        }
        // Kept as it goes: closing the app halfway loses little.
        stored_since_save += 1;
        if stored_since_save == 20 {
            stored_since_save = 0;
            let _ = state.with_quiet(|vault| vault.save_icons());
        }
    }
    let _ = state.with_quiet(|vault| vault.save_icons());
    true
}

fn client() -> Result<reqwest::Client, reqwest::Error> {
    crate::http::builder()
        // Some sites answer an unknown program with an error page.
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36",
        )
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        // Followed by `get`, which checks where each one goes.
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(PublicOnly)
        .build()
}

/// A GET that follows up to five redirects itself, turning an `http://` one
/// into `https://` (as a browser does for a site that asks for https), so
/// nothing ever goes out unencrypted, and never to a place `may_ask` turns
/// down.
async fn get(client: &reqwest::Client, url: &Url, accept: &str) -> Option<reqwest::Response> {
    let mut url = url.clone();
    for _ in 0..=5 {
        if !may_ask(&url) {
            return None;
        }
        let response = client.get(url.clone()).header(ACCEPT, accept).send().await.ok()?;
        if !response.status().is_redirection() {
            return Some(response);
        }
        let location = response.headers().get(LOCATION)?.to_str().ok()?;
        let mut next = url.join(location).ok()?;
        if next.scheme() == "http" {
            next.set_scheme("https").ok()?;
            if next.port() == Some(80) {
                next.set_port(None).ok()?;
            }
        }
        url = next;
    }
    None
}

/// Official icons for sites that reject background clients. Domain matching
/// uses the public suffix list, so lookalike hosts never inherit a brand.
fn bundled_icon(host: &str) -> Option<String> {
    let bytes: &[u8] = match psl::domain_str(host)? {
        "epicgames.com" => include_bytes!("site-icons/epic-games.ico"),
        "sony.com" => include_bytes!("site-icons/sony.ico"),
        "nordaccount.com" => include_bytes!("site-icons/nord-account.png"),
        "stoiximan.gr" => include_bytes!("site-icons/stoiximan.png"),
        "trovo.live" => include_bytes!("site-icons/trovo.ico"),
        "samsung.com" | "samsung.gr" => include_bytes!("site-icons/samsung.png"),
        _ => return None,
    };
    as_data_url(bytes)
}

fn site_hosts(host: &str) -> Vec<String> {
    let mut hosts = vec![host.to_string()];
    if let Some(site) = psl::domain_str(host) {
        for candidate in [site.to_string(), format!("www.{site}")] {
            if !hosts.contains(&candidate) {
                hosts.push(candidate);
            }
        }
    }
    hosts
}

/// From its own host, then the parent website and its www host. Bound the
/// whole search so an unresponsive website cannot hold up the icon queue.
async fn fetch_icon(client: &reqwest::Client, host: &str) -> Option<String> {
    if let Some(icon) = bundled_icon(host) {
        return Some(icon);
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        for candidate in site_hosts(host) {
            if let Ok(Some(icon)) = tokio::time::timeout(Duration::from_secs(6), icon_of(client, &candidate)).await {
                return Some(icon);
            }
        }
        None
    }).await.ok().flatten()
}

async fn icon_of(client: &reqwest::Client, host: &str) -> Option<String> {
    let home = Url::parse(&format!("https://{host}/")).ok()?;
    first_icon(discover_page_icon(client, &home), async {
        // Give the page's declared, higher-resolution icon a head start. A
        // stalled page must not stop a working favicon at its usual address.
        tokio::time::sleep(Duration::from_millis(800)).await;
        for link in default_icons(&home) {
            if let Some(icon) = image(client, &link).await {
                return Some(icon);
            }
        }
        None
    }).await
}

async fn first_icon(
    first: impl std::future::Future<Output = Option<String>>,
    second: impl std::future::Future<Output = Option<String>>,
) -> Option<String> {
    tokio::pin!(first, second);
    tokio::select! {
        icon = &mut first => match icon { Some(_) => icon, None => second.await },
        icon = &mut second => match icon { Some(_) => icon, None => first.await },
    }
}

async fn discover_page_icon(client: &reqwest::Client, home: &Url) -> Option<String> {
    let mut tried = Vec::new();
    let mut found = page(client, home).await;
    // A page that only sends the browser on (`<meta http-equiv="refresh">`).
    if let Some((base, html)) = &found
        && icon_links(html, base).is_empty()
        && let Some(next) = meta_refresh(html, base)
        && let Some(next_page) = page(client, &next).await
    {
        found = Some(next_page);
    }
    if let Some((base, html)) = found {
        for link in icon_links(&html, &base) {
            if tried.contains(&link) {
                continue;
            }
            if let Some(icon) = image(client, &link).await {
                return Some(icon);
            }
            tried.push(link);
        }
        for link in manifest_links(&html, &base) {
            if let Some((manifest_base, json)) = manifest(client, &link).await {
                for link in manifest_icons(&json, &manifest_base) {
                    if !tried.contains(&link) {
                        if let Some(icon) = image(client, &link).await {
                            return Some(icon);
                        }
                        tried.push(link);
                    }
                }
            }
        }
        for link in default_icons(&base) {
            if !tried.contains(&link) {
                if let Some(icon) = image(client, &link).await {
                    return Some(icon);
                }
                tried.push(link);
            }
        }
    }
    None
}

fn default_icons(base: &Url) -> impl Iterator<Item = Url> + '_ {
    ["/favicon.ico", "/favicon.png", "/apple-touch-icon.png"].into_iter().filter_map(|path| base.join(path).ok())
}

async fn manifest(client: &reqwest::Client, url: &Url) -> Option<(Url, String)> {
    let mut response = get(client, url, "application/manifest+json,application/json").await?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > MAX_PAGE as u64) {
        return None;
    }
    let base = response.url().clone();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_PAGE {
            return None;
        }
    }
    Some((base, String::from_utf8_lossy(&body).into_owned()))
}

fn manifest_links(html: &str, base: &Url) -> Vec<Url> {
    let base = document_base(html, base);
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    let mut links = Vec::new();
    while let Some(at) = lower[from..].find("<link") {
        let start = from + at;
        let Some(length) = lower[start..].find('>') else { break };
        from = start + length;
        let attrs = attributes(&html[start..start + length]);
        if attrs.get("rel").is_some_and(|rel| rel.split_ascii_whitespace().any(|word| word.eq_ignore_ascii_case("manifest")))
            && let Some(href) = attrs.get("href")
            && let Ok(url) = base.join(&href.trim().replace("&amp;", "&"))
            && may_ask(&url)
            && !links.contains(&url)
        {
            links.push(url);
            if links.len() == 2 { break; }
        }
    }
    links
}

fn manifest_icons(json: &str, base: &Url) -> Vec<Url> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(icons) = value.get("icons").and_then(|icons| icons.as_array()) else { return Vec::new() };
    let mut found = Vec::new();
    for icon in icons {
        let purpose = icon.get("purpose").and_then(|value| value.as_str()).unwrap_or("any");
        if !purpose.split_ascii_whitespace().any(|word| matches!(word, "any" | "maskable")) {
            continue;
        }
        let Some(src) = icon.get("src").and_then(|value| value.as_str()) else { continue };
        let Ok(url) = base.join(src) else { continue };
        if !may_ask(&url) && url.scheme() != "data" { continue; }
        let size = icon.get("sizes").and_then(|value| value.as_str()).and_then(largest).unwrap_or(0);
        let score = match size {
            48..=256 => 100 - size.abs_diff(96) / 8,
            32..=47 => 80,
            257.. => 60,
            0 => 50,
            _ => 40,
        };
        found.push((score, url));
    }
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let mut seen = HashSet::new();
    found.into_iter().filter_map(|(_, url)| seen.insert(url.clone()).then_some(url)).take(4).collect()
}

/// The start of a website's home page (up to its `</head>`), and where it
/// ended up after redirects.
async fn page(client: &reqwest::Client, url: &Url) -> Option<(Url, String)> {
    let mut response = get(client, url, "text/html").await?;
    if !response.status().is_success() {
        return None;
    }
    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("html"));
    if !is_html {
        return None;
    }
    let base = response.url().clone();
    let mut body: Vec<u8> = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        let searched = body.len().saturating_sub(6);
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_PAGE || contains_ignoring_case(&body[searched..], b"</head>") {
            break;
        }
    }
    body.truncate(MAX_PAGE);
    Some((base, String::from_utf8_lossy(&body).into_owned()))
}

fn contains_ignoring_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// The icons a page names in `<link rel="icon">` (and Apple's touch icons),
/// best first: a scalable or a larger one over a tiny one.
fn icon_links(html: &str, base: &Url) -> Vec<Url> {
    let base = document_base(html, base);
    // ASCII lowercasing keeps every byte where it was.
    let lower = html.to_ascii_lowercase();
    let mut found: Vec<(u32, Url)> = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<link") {
        let start = from + at;
        let Some(length) = lower[start..].find('>') else { break };
        let tag = &html[start..start + length];
        from = start + length;
        let attrs = attributes(tag);
        let rel = attrs.get("rel").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
        let touch = rel.split_ascii_whitespace().any(|r| r.starts_with("apple-touch-icon"));
        if !touch && !rel.split_ascii_whitespace().any(|r| r == "icon") {
            continue;
        }
        let Some(href) = attrs.get("href").map(|h| h.trim().replace("&amp;", "&")) else {
            continue;
        };
        let Ok(url) = base.join(&href) else { continue };
        if !matches!(url.scheme(), "https" | "data") {
            continue;
        }
        let kind = attrs.get("type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
        let sizes = attrs.get("sizes").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let scalable = kind.contains("svg") || sizes == "any" || url.path().to_ascii_lowercase().ends_with(".svg");
        let score = match largest(&sizes) {
            _ if scalable => 90,
            Some(size) if (48..=256).contains(&size) => 100 - size.abs_diff(96) / 8,
            Some(size) if size > 256 => 60,
            Some(size) if size >= 32 => 80,
            Some(_) => 40,
            None if touch => 85,
            None => 50,
        };
        found.push((score, url));
    }
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let mut seen = HashSet::new();
    found
        .into_iter()
        .filter_map(|(_, url)| seen.insert(url.clone()).then_some(url))
        .take(4)
        .collect()
}

/// Browsers resolve relative icon/manifest links against the first base href.
/// Keep the same public-HTTPS rules as all other discovered addresses.
fn document_base(html: &str, fallback: &Url) -> Url {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<base") {
        let start = from + at;
        let Some(length) = lower[start..].find('>') else { break };
        from = start + length;
        let attrs = attributes(&html[start..start + length]);
        if let Some(href) = attrs.get("href") {
            return fallback.join(&href.trim().replace("&amp;", "&"))
                .ok().filter(may_ask).unwrap_or_else(|| fallback.clone());
        }
    }
    fallback.clone()
}

/// Where a `<meta http-equiv="refresh" content="0; url=…">` sends the browser.
fn meta_refresh(html: &str, base: &Url) -> Option<Url> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<meta") {
        let start = from + at;
        let length = lower[start..].find('>')?;
        from = start + length;
        let attrs = attributes(&html[start..start + length]);
        if !attrs.get("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh")) {
            continue;
        }
        let content = attrs.get("content")?;
        let at = content.to_ascii_lowercase().find("url=")?;
        let target = content[at + 4..].trim().trim_matches(['\'', '"']);
        return base.join(target).ok().filter(|url| url.scheme() == "https");
    }
    None
}

/// The largest width in a `sizes` attribute ("16x16 32x32" → 32).
fn largest(sizes: &str) -> Option<u32> {
    sizes
        .split_ascii_whitespace()
        .filter_map(|size| size.split_once('x')?.0.parse().ok())
        .max()
}

/// A tag's attributes, names lowercased; the first of a repeated name wins.
fn attributes(tag: &str) -> HashMap<String, String> {
    let bytes = tag.as_bytes();
    let mut attrs = HashMap::new();
    // Past the tag's name.
    let mut i = bytes.iter().position(|b| b.is_ascii_whitespace()).unwrap_or(bytes.len());
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let name = tag[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                let value_start = i + 1;
                i = value_start;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = &tag[value_start..i];
                i += 1;
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = &tag[value_start..i];
            }
        }
        attrs.entry(name).or_insert_with(|| value.to_string());
    }
    attrs
}

async fn image(client: &reqwest::Client, url: &Url) -> Option<String> {
    if url.scheme() == "data" {
        return data_url_image(url.as_str());
    }
    let mut response = get(client, url, "image/*,*/*;q=0.5").await?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > MAX_ICON as u64) {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_ICON {
            return None;
        }
    }
    as_data_url(&body)
}

/// An icon written into the page itself (`data:image/png;base64,…`).
fn data_url_image(url: &str) -> Option<String> {
    let (meta, payload) = url.strip_prefix("data:")?.split_once(',')?;
    let bytes = if meta.to_ascii_lowercase().ends_with(";base64") {
        STANDARD.decode(payload.trim()).ok()?
    } else {
        percent_decode(payload)
    };
    as_data_url(&bytes)
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(high * 16 + low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn as_data_url(bytes: &[u8]) -> Option<String> {
    let kind = sniff(bytes)?;
    Some(format!("data:{kind};base64,{}", STANDARD.encode(bytes)))
}

/// What image `bytes` are, from their first bytes (never from what the
/// server claims), or none when they are not an image worth showing.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 24 || bytes.len() > MAX_ICON {
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        // A 1x1 placeholder is not an icon.
        return (width >= 16 && height >= 16).then_some("image/png");
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let width = u16::from_le_bytes([bytes[6], bytes[7]]);
        let height = u16::from_le_bytes([bytes[8], bytes[9]]);
        return (width >= 16 && height >= 16).then_some("image/gif");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    // Drawn by <img>, where an SVG's scripts never run.
    (head.starts_with('<') && head.contains("<svg")).then_some("image/svg+xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_names_are_asked() {
        assert_eq!(icon_host("https://www.GitHub.com/login").as_deref(), Some("github.com"));
        assert_eq!(icon_host("accounts.google.com").as_deref(), Some("accounts.google.com"));
        assert_eq!(icon_host("http://192.168.1.1"), None);
        assert_eq!(icon_host("http://localhost:8080"), None);
        assert_eq!(icon_host("https://router.local"), None);
        assert_eq!(icon_host("https://nas.home.arpa"), None);
        assert_eq!(icon_host("https://duckduckgogg42xjoc72x3sjasowoarfbgcmvfimaftt6twagswzczad.onion"), None);
        assert_eq!(icon_host("https://[::1]/"), None);
    }

    #[test]
    fn nothing_on_this_pc_or_the_local_network_is_asked() {
        for url in ["https://github.com/favicon.ico", "https://cdn.example.com/icon.png"] {
            assert!(may_ask(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "http://github.com/",
            "https://127.0.0.1/",
            "https://10.0.0.8/icon.png",
            "https://[fd00::1]/icon.png",
            "https://[::ffff:192.168.1.1]/",
            "https://localhost/",
            "https://printer/",
            "https://router.local/",
            "https://intranet.internal/",
            "https://nas.home.arpa/",
        ] {
            assert!(!may_ask(&Url::parse(url).unwrap()), "{url}");
        }
        for ip in ["8.8.8.8", "140.82.112.3", "2606:4700::1111", "::ffff:8.8.8.8", "64:ff9b::808:808"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "fe80::1",
            "fd12:3456::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn a_name_that_points_at_this_pc_is_not_asked() {
        use reqwest::dns::Resolve;
        let name: reqwest::dns::Name = "localhost".parse().unwrap();
        assert!(PublicOnly.resolve(name).await.is_err());
    }

    #[test]
    fn the_best_icon_a_page_names_comes_first() {
        let base = Url::parse("https://www.example.com/start").unwrap();
        let html = r##"<html><head>
            <link rel="stylesheet" href="/site.css">
            <LINK REL="shortcut icon" href="/favicon.ico">
            <link rel=icon sizes="16x16" href="small.png">
            <link rel="icon" type="image/png" sizes="96x96" href="https://cdn.example.com/icon-96.png?v=1&amp;x=2">
            <link rel="mask-icon" href="/mask.svg" color="#000">
            <link rel="apple-touch-icon" href="/touch.png">
            <link rel="icon" href="http://example.com/insecure.png">
        </head>"##;
        let links: Vec<String> = icon_links(html, &base).iter().map(Url::to_string).collect();
        assert_eq!(
            links,
            [
                "https://cdn.example.com/icon-96.png?v=1&x=2",
                "https://www.example.com/touch.png",
                "https://www.example.com/favicon.ico",
                "https://www.example.com/small.png",
            ]
        );
    }

    #[test]
    fn a_page_that_sends_the_browser_on_is_followed() {
        let base = Url::parse("https://www.example.gr/").unwrap();
        let html = r#"<HTML><HEAD><meta http-equiv="refresh" content="0; url=/hub" /></HEAD></HTML>"#;
        assert_eq!(meta_refresh(html, &base).unwrap().as_str(), "https://www.example.gr/hub");
        let insecure = r#"<meta http-equiv="Refresh" content="0;URL='http://example.gr/'">"#;
        assert_eq!(meta_refresh(insecure, &base), None);
    }

    #[test]
    fn only_real_images_are_kept() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&32u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        png.extend_from_slice(&[0; 20]);
        assert_eq!(sniff(&png), Some("image/png"));
        png[16..24].copy_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1]);
        assert_eq!(sniff(&png), None, "a 1x1 placeholder");
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>"#;
        assert_eq!(sniff(svg), Some("image/svg+xml"));
        assert_eq!(sniff(b"<!doctype html><html><body>Not found</body></html>"), None);
        assert_eq!(sniff(&[7; 64]), None);
    }

    /// Asks real websites: `cargo test --lib icons -- --ignored`.
    #[tokio::test]
    #[ignore = "uses the network"]
    async fn real_websites_give_their_icons() {
        let client = client().unwrap();
        let mut missing = Vec::new();
        for host in ["nordaccount.com", "stoiximan.gr", "samsung.gr", "epicgames.com", "my.account.sony.com", "github.com", "www.wikipedia.org", "accounts.google.com", "cosmote.gr", "paypal.com", "trovo.live", "vodafonecu.gr", "www.nerdom.gr"] {
            let icon = fetch_icon(&client, host).await;
            println!("{host}: {}", icon.as_deref().map_or("none".into(), |i| i[..40.min(i.len())].to_string()));
            if !icon.is_some_and(|i| i.starts_with("data:image/")) {
                missing.push(host);
            }
        }
        assert!(missing.is_empty(), "missing icons: {missing:?}");
    }

    #[test]
    fn icons_written_into_the_page_are_read() {
        let svg = "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2016%2016'%3E%3C/svg%3E";
        assert!(data_url_image(svg).unwrap().starts_with("data:image/svg+xml;base64,"));
        let html = format!("data:text/html;base64,{}", STANDARD.encode("<html><script>alert(1)</script></html>"));
        assert_eq!(data_url_image(&html), None);
    }

    #[test]
    fn blocked_sites_have_official_icons_without_network_requests() {
        for host in ["epicgames.com", "store.epicgames.com", "my.account.sony.com", "nordaccount.com", "my.nordaccount.com", "stoiximan.gr", "www.samsung.gr", "account.samsung.com", "trovo.live"] {
            assert!(bundled_icon(host).is_some_and(|icon| icon.starts_with("data:image/")), "{host}");
        }
        for host in ["epicgames.com.example.org", "sony.com.example.org", "nordaccount.com.example.org", "stoiximan.gr.example.org", "samsung.gr.example.org", "payzy.com"] {
            assert!(bundled_icon(host).is_none(), "{host}");
        }
    }

    #[test]
    fn discovery_also_checks_the_sites_www_host() {
        assert_eq!(site_hosts("account.example.co.uk"), ["account.example.co.uk", "example.co.uk", "www.example.co.uk"]);
        assert_eq!(site_hosts("example.com"), ["example.com", "www.example.com"]);
        assert_eq!(site_hosts("www.example.com"), ["www.example.com", "example.com"]);
    }

    #[test]
    fn manifest_icons_resolve_from_the_manifest_and_prefer_displayable_sizes() {
        let base = Url::parse("https://cdn.example.com/assets/site.webmanifest").unwrap();
        let json = r#"{"icons":[
            {"src":"tiny.png","sizes":"16x16"},
            {"src":"app.png","sizes":"192x192","purpose":"any maskable"},
            {"src":"mono.svg","sizes":"any","purpose":"monochrome"},
            {"src":"http://example.com/icon.png","sizes":"96x96"}
        ]}"#;
        let urls: Vec<_> = manifest_icons(json, &base).iter().map(Url::as_str).map(str::to_string).collect();
        assert_eq!(urls, ["https://cdn.example.com/assets/app.png", "https://cdn.example.com/assets/tiny.png"]);
        assert!(manifest_icons("not json", &base).is_empty());
        let home = Url::parse("https://example.com/").unwrap();
        assert_eq!(manifest_links(r#"<link rel="manifest" href="/app.webmanifest">"#, &home)[0].as_str(), "https://example.com/app.webmanifest");
    }

    #[test]
    fn old_missing_icons_are_retried_without_discarding_successful_ones() {
        let old: CachedIcon = serde_json::from_str(r#"{"data":null,"at":1000}"#).unwrap();
        assert!(needs_refresh(&old, 1001));
        let current = CachedIcon { revision: DISCOVERY_REVISION, ..old };
        assert!(!needs_refresh(&current, 1001));
        assert!(needs_refresh(&current, 1000 + KEEP_MISSING));
        let found = CachedIcon { data: Some("data:image/png;base64,AAAA".into()), revision: 0, at: 1000 };
        assert!(!needs_refresh(&found, 1001));
        assert!(needs_refresh(&found, 1000 + KEEP_FOUND));
    }

    #[tokio::test]
    async fn bundled_icons_replace_fresh_cached_misses_and_respect_the_preference() {
        let path = std::env::temp_dir().join(format!("myle-icon-test-{}.vault", uuid::Uuid::new_v4()));
        let cell_path = path.clone();
        let state = VaultCell::new(move || Ok(cell_path.clone()));
        state.with_quiet(|vault| {
            vault.create("test", crate::crypto::KdfParams::cheap_for_tests())?;
            let entry = serde_json::from_str(r#"{"title":"Epic Games","urls":["https://epicgames.com/"]}"#).unwrap();
            vault.save(&entry)?;
            vault.icons()?.icons.insert("epicgames.com".into(), CachedIcon {
                data: None, at: now(), revision: DISCOVERY_REVISION,
            });
            Ok(())
        }).unwrap();
        let (known, pending) = lookup(&state, Vec::new()).unwrap();
        assert_eq!(known.get("epicgames.com"), bundled_icon("epicgames.com").as_ref());
        assert!(pending.is_some());
        let (_, overlap) = lookup(&state, Vec::new()).unwrap();
        assert!(overlap.is_none());
        assert!(FETCHING.load(Ordering::SeqCst), "an overlapping lookup must not release the active fetch");
        let (_, overlap) = lookup(&state, Vec::new()).unwrap();
        assert!(overlap.is_none(), "the first fetch still owns the queue");
        drop(pending);
        let (known, pending) = lookup(&state, vec!["epicgames.com".into()]).unwrap();
        assert!(known.is_empty(), "do not resend icons already on the page");
        drop(pending);
        state.with_quiet(|vault| {
            let mut prefs = vault.prefs();
            prefs.website_icons = false;
            vault.set_prefs(prefs)
        }).unwrap();
        let (known, pending) = lookup(&state, Vec::new()).unwrap();
        assert!(known.is_empty() && pending.is_none(), "icons off applies to bundled icons too");
        state.with_quiet(|vault| {
            let mut prefs = vault.prefs();
            prefs.website_icons = true;
            vault.set_prefs(prefs)
        }).unwrap();
        let (_, pending) = lookup(&state, Vec::new()).unwrap();
        let found = std::sync::Mutex::new(Vec::new());
        pending.unwrap().fetch(&state, |icon| {
            found.lock().unwrap().push(icon.host.clone());
            if icon.host == "epicgames.com" {
                state.with_quiet(|vault| {
                    let entry = serde_json::from_str(r#"{"title":"Nord Account","urls":["https://nordaccount.com/"]}"#).unwrap();
                    vault.save(&entry)?;
                    Ok(())
                }).unwrap();
                assert!(lookup(&state, Vec::new()).unwrap().1.is_none());
            }
        }).await;
        assert_eq!(*found.lock().unwrap(), ["epicgames.com", "nordaccount.com"], "new sites join the running queue");
        assert!(!FETCHING.load(Ordering::SeqCst));
        state.with_quiet(|vault| { vault.forget_icons(); Ok(()) }).unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn relative_icons_and_manifests_follow_the_pages_base_href() {
        let home = Url::parse("https://example.com/login/").unwrap();
        let html = r#"<base href="https://cdn.example.com/assets/"><link rel="icon" href="brand.png"><link rel="manifest" href="app.json">"#;
        assert_eq!(icon_links(html, &home)[0].as_str(), "https://cdn.example.com/assets/brand.png");
        assert_eq!(manifest_links(html, &home)[0].as_str(), "https://cdn.example.com/assets/app.json");
        let html = r#"<base href="http://127.0.0.1/"><link rel="icon" href="brand.png">"#;
        assert_eq!(icon_links(html, &home)[0].as_str(), "https://example.com/login/brand.png");
    }

    #[tokio::test]
    async fn a_slow_or_failed_discovery_does_not_hide_a_working_fallback() {
        let icon = first_icon(std::future::pending(), async { Some("fallback".into()) }).await;
        assert_eq!(icon.as_deref(), Some("fallback"));
        let icon = first_icon(async { None }, async { Some("fallback".into()) }).await;
        assert_eq!(icon.as_deref(), Some("fallback"));
        let icon = first_icon(async { Some("declared".into()) }, std::future::pending()).await;
        assert_eq!(icon.as_deref(), Some("declared"));
        assert_eq!(first_icon(async { None }, async { None }).await, None);
    }
}
