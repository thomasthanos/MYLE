//! Streaming HTTP downloads with progress reporting and SHA-256 hashing,
//! shared by the app updater and the Install Apps page.
//!
//! Large files from servers that support byte ranges are fetched over a few
//! connections at once: hosts such as Google Drive cap the speed of each
//! connection, not of the download.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Below this, one connection is as fast as several.
const SEGMENTED_MIN: u64 = 32 * 1024 * 1024;
const SEGMENTS: u64 = 4;
/// A dropped connection resumes its part from where it stopped.
const SEGMENT_RETRIES: u32 = 3;
/// Network chunks are small (~16 KB); each write to a Tokio file is a trip
/// to a blocking thread, so they are gathered first.
const WRITE_BUFFER: usize = 1024 * 1024;

pub fn http_client(user_agent: &str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(err)
}

/// Downloads `url` to `dest`, calling `on_progress(downloaded, total)` at most
/// every 100 ms (and once at the end). `is_cancelled` is polled between chunks.
/// Returns the lowercase hex SHA-256 of the downloaded file.
pub async fn download_to(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size_hint: Option<u64>,
    on_progress: impl FnMut(u64, Option<u64>),
    is_cancelled: impl Fn() -> bool,
) -> Result<String, String> {
    download_file(
        client,
        url,
        dest,
        size_hint,
        on_progress,
        is_cancelled,
        true,
    )
    .await
    .map(Option::unwrap_or_default)
}

/// `download_to`, with the SHA-256 only when `want_hash` (a multi-gigabyte
/// file fetched in parts is hashed by reading it back, which is not free).
/// `total` is `None` while the size is unknown, including when a `size_hint`
/// turns out to be too small. A download that fails leaves no partial file.
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size_hint: Option<u64>,
    on_progress: impl FnMut(u64, Option<u64>),
    is_cancelled: impl Fn() -> bool,
    want_hash: bool,
) -> Result<Option<String>, String> {
    let result = fetch(
        client,
        url,
        dest,
        size_hint,
        on_progress,
        is_cancelled,
        want_hash,
    )
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

async fn fetch(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size_hint: Option<u64>,
    mut on_progress: impl FnMut(u64, Option<u64>),
    is_cancelled: impl Fn() -> bool,
    want_hash: bool,
) -> Result<Option<String>, String> {
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(err)?;
    }
    // Asking for the whole file as a range tells, in the same request,
    // whether the server can serve parts: 206 and a total, or a plain 200.
    let response = client
        .get(url)
        .header(RANGE, "bytes=0-")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if is_html(content_type) {
        return Err(html_instead_of_file(url));
    }

    let range = (response.status() == StatusCode::PARTIAL_CONTENT)
        .then(|| content_range_of(&response))
        .flatten();
    let ranged_total = range.and_then(|range| range.total);
    if let Some(total) = ranged_total.filter(|total| *total >= SEGMENTED_MIN) {
        // Redirects (Drive, GitHub) are resolved once; the parts go straight
        // to the final address.
        let final_url = response.url().to_string();
        drop(response);
        on_progress(0, Some(total));
        match segmented(
            client,
            &final_url,
            dest,
            total,
            &mut on_progress,
            &is_cancelled,
        )
        .await
        {
            Ok(()) => {
                on_progress(total, Some(total));
                return if want_hash {
                    hash_file(dest).await.map(Some)
                } else {
                    Ok(None)
                };
            }
            Err(e) if e == CANCELLED => return Err(e),
            // Some servers refuse parallel ranges part-way; one plain
            // connection still gets the file.
            Err(_) => {
                let response = client
                    .get(url)
                    .send()
                    .await
                    .map_err(err)?
                    .error_for_status()
                    .map_err(err)?;
                return stream(response, dest, None, on_progress, is_cancelled, want_hash).await;
            }
        }
    }
    // A server that answered with only the start of the file (or would not
    // say how much it sent): ask again for all of it, without a range.
    let response = match range {
        Some(range) if !range.is_whole_file() => {
            drop(response);
            client
                .get(url)
                .send()
                .await
                .map_err(err)?
                .error_for_status()
                .map_err(err)?
        }
        _ => response,
    };
    stream(
        response,
        dest,
        ranged_total.or(size_hint),
        on_progress,
        is_cancelled,
        want_hash,
    )
    .await
}

/// One connection, written as it arrives and hashed on the way.
async fn stream(
    response: reqwest::Response,
    dest: &Path,
    size_hint: Option<u64>,
    mut on_progress: impl FnMut(u64, Option<u64>),
    is_cancelled: impl Fn() -> bool,
    want_hash: bool,
) -> Result<Option<String>, String> {
    // Checked on every answer that is saved, not only the first: a server
    // may answer a second request (without a range) with a web page.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if is_html(content_type) {
        return Err(html_instead_of_file(response.url().as_str()));
    }
    // What the server itself says the file holds, as opposed to a guess.
    let exact = match response.status() {
        StatusCode::OK => response.content_length(),
        StatusCode::PARTIAL_CONTENT => content_range_of(&response)
            .filter(ContentRange::is_whole_file)
            .and_then(|range| range.total),
        _ => None,
    };
    let total = exact.or(size_hint.filter(|s| *s > 0));
    // A size from the catalog can be stale: once passed, the size is unknown
    // rather than a progress bar stuck past 100%.
    let shown = |downloaded: u64| total.filter(|total| downloaded <= *total);
    on_progress(0, total);

    let mut file = BufWriter::with_capacity(
        WRITE_BUFFER,
        tokio::fs::File::create(dest).await.map_err(err)?,
    );
    let mut hasher = want_hash.then(Sha256::new);
    let mut downloaded = 0u64;
    let mut body = response.bytes_stream();
    // Progress goes out on a clock, not only when a chunk lands: a connection
    // that delivers in bursts still moves the bar every 100 ms.
    let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let chunk = tokio::select! {
            biased;
            next = body.next() => match next {
                Some(chunk) => chunk,
                None => break,
            },
            _ = ticker.tick() => {
                on_progress(downloaded, shown(downloaded));
                continue;
            }
        };
        if is_cancelled() {
            return Err(CANCELLED.into());
        }
        let chunk = chunk.map_err(err)?;
        if let Some(hasher) = &mut hasher {
            hasher.update(&chunk);
        }
        file.write_all(&chunk).await.map_err(err)?;
        downloaded += chunk.len() as u64;
    }
    file.flush().await.map_err(err)?;
    if let Some(exact) = exact.filter(|exact| *exact != downloaded) {
        return Err(format!(
            "The download stopped early ({downloaded} of {exact} bytes)."
        ));
    }
    on_progress(downloaded, Some(downloaded));
    Ok(hasher.map(|hasher| to_hex(&hasher.finalize())))
}

/// A `Content-Range: bytes <start>-<end>/<total>` header (`total` is `None`
/// for `*`: the server does not know).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: Option<u64>,
}

impl ContentRange {
    fn parse(value: &str) -> Option<Self> {
        let (span, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
        let (start, end) = span.trim().split_once('-')?;
        let (start, end) = (start.trim().parse().ok()?, end.trim().parse().ok()?);
        let total = match total.trim() {
            "*" => None,
            known => Some(known.parse().ok()?),
        };
        (start <= end).then_some(Self { start, end, total })
    }

    fn is_whole_file(&self) -> bool {
        self.start == 0 && self.total == Some(self.end + 1)
    }
}

fn content_range_of(response: &reqwest::Response) -> Option<ContentRange> {
    response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(ContentRange::parse)
}

/// `total` bytes split into `count` inclusive ranges.
fn split_ranges(total: u64, count: u64) -> Vec<(u64, u64)> {
    let count = count.clamp(1, total.max(1));
    let size = total.div_ceil(count);
    (0..count)
        .map(|index| (index * size, ((index + 1) * size).min(total) - 1))
        .filter(|(start, end)| start <= end)
        .collect()
}

/// Fetches `total` bytes as `SEGMENTS` ranges at once, each written at its
/// own offset of a file sized up front.
async fn segmented(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    total: u64,
    on_progress: &mut impl FnMut(u64, Option<u64>),
    is_cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    let file = tokio::fs::File::create(dest).await.map_err(err)?;
    file.set_len(total).await.map_err(err)?;
    drop(file);

    let done = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let parts = split_ranges(total, SEGMENTS)
        .into_iter()
        .map(|(start, end)| {
            let part = fetch_range(
                client.clone(),
                url.to_string(),
                dest.to_path_buf(),
                start,
                end,
                done.clone(),
                stop.clone(),
            );
            let stop = stop.clone();
            async move {
                let result = part.await;
                // One part giving up dooms the whole attempt: stop the others
                // now rather than let them finish for a file that is refetched.
                if result.is_err() {
                    stop.store(true, Ordering::Relaxed);
                }
                result
            }
        });
    let mut all = std::pin::pin!(futures_util::future::join_all(parts));
    let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
    let results = loop {
        tokio::select! {
            results = &mut all => break results,
            _ = ticker.tick() => {
                if is_cancelled() {
                    stop.store(true, Ordering::Relaxed);
                }
                on_progress(done.load(Ordering::Relaxed), Some(total));
            }
        }
    };
    if is_cancelled() {
        return Err(CANCELLED.into());
    }
    // The parts stopped because of a failure report "cancelled"; the failure
    // itself is what counts (and is not the user cancelling).
    let mut errors = results.into_iter().filter_map(Result::err);
    match errors.find(|e| e != CANCELLED) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn fetch_range(
    client: reqwest::Client,
    url: String,
    path: PathBuf,
    start: u64,
    end: u64,
    done: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut file = BufWriter::with_capacity(
        WRITE_BUFFER,
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .map_err(err)?,
    );
    let mut position = start;
    let mut failures = 0;
    let mut last_error = String::new();
    while position <= end {
        if stop.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let resumed_at = position;
        let response = client
            .get(&url)
            .header(RANGE, format!("bytes={position}-{end}"))
            .send()
            .await;
        match response {
            Ok(response) if response.status() == StatusCode::PARTIAL_CONTENT => {
                // Written where it belongs only if it is the part asked for.
                if content_range_of(&response).map(|range| range.start) != Some(position) {
                    return Err("the server sent a different part of the file".into());
                }
                file.seek(SeekFrom::Start(position)).await.map_err(err)?;
                let mut body = response.bytes_stream();
                while let Some(chunk) = body.next().await {
                    if stop.load(Ordering::Relaxed) {
                        return Err(CANCELLED.into());
                    }
                    match chunk {
                        Ok(chunk) => {
                            let wanted = (end + 1 - position).min(chunk.len() as u64) as usize;
                            file.write_all(&chunk[..wanted]).await.map_err(err)?;
                            position += wanted as u64;
                            done.fetch_add(wanted as u64, Ordering::Relaxed);
                            if position > end {
                                break;
                            }
                        }
                        Err(e) => {
                            last_error = e.to_string();
                            break;
                        }
                    }
                }
            }
            // Anything but a part of the file: ranges are not really supported.
            Ok(response) => return Err(format!("the server answered {}", response.status())),
            Err(e) => last_error = e.to_string(),
        }
        if position <= end {
            // Only drops in a row count: a long download over a flaky
            // connection may lose its link many times and still get there.
            if position > resumed_at {
                failures = 0;
            }
            failures += 1;
            if failures > SEGMENT_RETRIES {
                return Err(last_error);
            }
            tokio::time::sleep(Duration::from_millis(500 * u64::from(failures))).await;
        }
    }
    file.flush().await.map_err(err)
}

/// SHA-256 of a file on disk, read off the async runtime.
async fn hash_file(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(err)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(err)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(to_hex(&hasher.finalize()))
    })
    .await
    .map_err(err)?
}

/// Error text used when a job was cancelled by the user.
pub const CANCELLED: &str = "cancelled";

fn is_html(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|t| t.trim_start().to_ascii_lowercase().starts_with("text/html"))
}

/// File hosts answer with a web page when a download is blocked. Google Drive
/// does it for its daily per-file quota and for files that are not shared.
fn html_instead_of_file(url: &str) -> String {
    if url.contains("drive.usercontent.google.com") || url.contains("drive.google.com") {
        "Google Drive returned a web page instead of the file. The file must be shared with \
         \"Anyone with the link\", and Drive blocks a file for a while once it hits its daily \
         download quota."
            .into()
    } else {
        "The server returned a web page instead of a file. The download link may have expired."
            .into()
    }
}

/// `"sha256:ABC…"` -> lowercase hex, if it is a well-formed SHA-256 digest.
pub fn parse_sha256_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A safe local file name for a download: the last URL/path segment with
/// anything unusual replaced.
pub fn file_name_from(name_or_url: &str, fallback: &str) -> String {
    let last = name_or_url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_parsing() {
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(
            parse_sha256_digest(&format!("sha256:{}", hex.to_uppercase())).as_deref(),
            Some(hex)
        );
        assert_eq!(parse_sha256_digest(hex), None);
        assert_eq!(parse_sha256_digest("sha256:1234"), None);
        assert_eq!(parse_sha256_digest("sha512:abcd"), None);
    }

    #[test]
    fn ranges_cover_the_file_exactly_once() {
        let ranges = split_ranges(10, 4);
        assert_eq!(ranges, vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
        for total in [1u64, 7, 32 * 1024 * 1024 + 3] {
            let ranges = split_ranges(total, SEGMENTS);
            assert_eq!(ranges.first().unwrap().0, 0);
            assert_eq!(ranges.last().unwrap().1, total - 1);
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].1 + 1, pair[1].0);
            }
        }
        let range = ContentRange::parse("bytes 0-1023/4096").unwrap();
        assert_eq!(range.total, Some(4096));
        assert!(!range.is_whole_file());
        assert!(
            ContentRange::parse("bytes 0-4095/4096")
                .unwrap()
                .is_whole_file()
        );
        assert_eq!(ContentRange::parse("bytes 0-1023/*").unwrap().total, None);
        assert_eq!(ContentRange::parse("bytes 9-3/10"), None);
        assert_eq!(ContentRange::parse("items 0-1/2"), None);
    }

    /// A local server that serves byte ranges, as Google Drive and GitHub do.
    async fn serve_ranges(body: Vec<u8>) -> (String, Arc<AtomicU64>) {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = Arc::new(body);
        let requests = Arc::new(AtomicU64::new(0));
        let counter = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    let total = body.len() as u64;
                    let (start, end) = text
                        .lines()
                        .find_map(|line| line.strip_prefix("range: bytes="))
                        .and_then(|range| {
                            let (start, end) = range.trim().split_once('-')?;
                            let start: u64 = start.parse().ok()?;
                            let end = end.parse().unwrap_or(total - 1).min(total - 1);
                            Some((start, end))
                        })
                        .unwrap_or((0, total - 1));
                    let part = &body[start as usize..=end as usize];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total}\r\nConnection: close\r\n\r\n",
                        part.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(part).await;
                });
            }
        });
        (format!("http://{address}/file.bin"), requests)
    }

    #[tokio::test]
    async fn a_large_file_is_fetched_in_parts_and_arrives_intact() {
        let body: Vec<u8> = (0..(SEGMENTED_MIN + 12_345))
            .map(|i| (i % 251) as u8)
            .collect();
        let expected = to_hex(&Sha256::digest(&body));
        let (url, requests) = serve_ranges(body.clone()).await;
        let dest = std::env::temp_dir().join(format!("myle-ranged-{}.bin", std::process::id()));
        let client = http_client("test").unwrap();
        let mut last = (0, None);
        let hash = download_file(
            &client,
            &url,
            &dest,
            None,
            |done, total| last = (done, total),
            || false,
            true,
        )
        .await
        .unwrap();
        assert_eq!(hash.as_deref(), Some(expected.as_str()));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(last, (body.len() as u64, Some(body.len() as u64)));
        // The probe, then one request per part: not the single-stream fallback.
        assert_eq!(requests.load(Ordering::Relaxed), 1 + SEGMENTS);
        let _ = std::fs::remove_file(&dest);
    }

    /// Answers the first range request (the probe) and plain requests, but
    /// refuses the parts that do not start at 0, like a host that stops
    /// honouring parallel ranges part-way.
    async fn serve_refusing_parts(body: Vec<u8>) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = Arc::new(body);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    let total = body.len();
                    let start = text
                        .lines()
                        .find_map(|line| line.strip_prefix("range: bytes="))
                        .and_then(|range| range.split('-').next()?.parse::<usize>().ok());
                    let (head, part): (String, &[u8]) = match start {
                        Some(0) => (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {total}\r\nContent-Range: bytes 0-{}/{total}\r\nConnection: close\r\n\r\n",
                                total - 1
                            ),
                            &body[..],
                        ),
                        Some(_) => (
                            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
                            &[],
                        ),
                        None => (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                            ),
                            &body[..],
                        ),
                    };
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(part).await;
                });
            }
        });
        format!("http://{address}/file.bin")
    }

    #[tokio::test]
    async fn a_refused_part_falls_back_to_one_connection_and_is_not_a_cancel() {
        let body: Vec<u8> = (0..(SEGMENTED_MIN + 4_321))
            .map(|i| (i % 241) as u8)
            .collect();
        let expected = to_hex(&Sha256::digest(&body));
        let url = serve_refusing_parts(body.clone()).await;
        let dest = std::env::temp_dir().join(format!("myle-refused-{}.bin", std::process::id()));
        let client = http_client("test").unwrap();
        let hash = download_file(&client, &url, &dest, None, |_, _| {}, || false, true)
            .await
            .expect("the plain fallback gets the whole file");
        assert_eq!(hash.as_deref(), Some(expected.as_str()));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = std::fs::remove_file(&dest);
    }

    /// Answers any range with just the first KB (as a capped CDN might),
    /// and a plain request with the whole file.
    async fn serve_capped_ranges(body: Vec<u8>) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = Arc::new(body);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    let total = body.len();
                    let (head, part) = if text.contains("range: bytes=") {
                        (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: 1024\r\nContent-Range: bytes 0-1023/{total}\r\nConnection: close\r\n\r\n"
                            ),
                            &body[..1024],
                        )
                    } else {
                        (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
                            ),
                            &body[..],
                        )
                    };
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(part).await;
                });
            }
        });
        format!("http://{address}/file.bin")
    }

    #[tokio::test]
    async fn a_partial_answer_to_the_probe_is_not_saved_as_the_file() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 239) as u8).collect();
        let url = serve_capped_ranges(body.clone()).await;
        let dest = std::env::temp_dir().join(format!("myle-capped-{}.bin", std::process::id()));
        let client = http_client("test").unwrap();
        download_file(&client, &url, &dest, None, |_, _| {}, || false, false)
            .await
            .expect("the whole file is asked for again");
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = std::fs::remove_file(&dest);
    }

    /// Answers a range with the first KB of the file, and a plain request
    /// with a web page (a host's "quota exceeded" page, say).
    async fn serve_page_on_the_second_try(total: usize) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let ranged = String::from_utf8_lossy(&request).to_ascii_lowercase().contains("range: bytes=");
                    let page = b"<!doctype html><html><body>Quota exceeded</body></html>";
                    let (head, body): (String, &[u8]) = if ranged {
                        (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Type: application/zip\r\nContent-Length: 1024\r\nContent-Range: bytes 0-1023/{total}\r\nConnection: close\r\n\r\n"
                            ),
                            &[b'P'; 1024],
                        )
                    } else {
                        (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                page.len()
                            ),
                            page,
                        )
                    };
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                });
            }
        });
        format!("http://{address}/app.zip")
    }

    #[tokio::test]
    async fn a_web_page_on_the_second_try_is_not_saved_as_the_file() {
        let url = serve_page_on_the_second_try(200_000).await;
        let dest = std::env::temp_dir().join(format!("myle-page-{}.zip", std::process::id()));
        let client = http_client("test").unwrap();
        let result = download_file(&client, &url, &dest, None, |_, _| {}, || false, false).await;
        let error = result.unwrap_err();
        assert!(error.contains("web page"), "{error}");
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn a_failed_download_leaves_no_partial_file() {
        let dest = std::env::temp_dir().join(format!("myle-failed-{}.bin", std::process::id()));
        std::fs::write(&dest, b"stale").unwrap();
        let client = http_client("test").unwrap();
        // Nothing listens on port 9 (discard) locally: the request fails.
        let result = download_file(
            &client,
            "http://127.0.0.1:9/x",
            &dest,
            None,
            |_, _| {},
            || false,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        let digest = Sha256::digest(b"abc");
        assert_eq!(
            to_hex(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn web_pages_are_not_accepted_as_downloads() {
        assert!(is_html(Some("text/html; charset=utf-8")));
        assert!(is_html(Some("TEXT/HTML")));
        assert!(!is_html(Some("application/zip")));
        assert!(!is_html(Some("application/octet-stream")));
        assert!(!is_html(None));
        assert!(
            html_instead_of_file("https://drive.usercontent.google.com/download?id=x")
                .contains("Google Drive")
        );
        assert!(html_instead_of_file("https://example.com/a.zip").contains("web page"));
    }

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(
            file_name_from(
                "https://us.download.nvidia.com/nvapp/client/1.2/NVIDIA_app_v1.2.exe?x=1",
                "setup.exe"
            ),
            "NVIDIA_app_v1.2.exe"
        );
        assert_eq!(
            file_name_from("..\\..\\evil name.exe", "setup.exe"),
            "evil_name.exe"
        );
        assert_eq!(
            file_name_from("https://example.com/", "setup.exe"),
            "setup.exe"
        );
    }
}
