use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use semver::Version;
use serde::Deserialize;
use crate::download::{self, parse_sha256_digest};
use super::state::Cancellation;
const USER_AGENT: &str = "MakeYourLifeEasier/SpotifyHub";
const CLI_RELEASES: &str = "https://api.github.com/repos/spicetify/cli/releases?per_page=30";
const MARKETPLACE_RELEASES: &str =
    "https://api.github.com/repos/spicetify/marketplace/releases?per_page=30";
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_UNPACKED_BYTES: u64 = 768 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
    pub digest: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct VerifiedRelease {
    pub version: String,
    pub asset: Asset,
    pub digest: String,
}
pub async fn resolve_cli() -> Result<VerifiedRelease, String> {
    let releases = fetch(CLI_RELEASES).await?;
    let release = newest_stable(&releases).ok_or("No stable Spicetify release was found.")?;
    let version = release.tag_name.trim_start_matches('v');
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "x86" => "x32",
        "aarch64" => "arm64",
        other => {
            return Err(format!(
                "Spicetify does not publish a Windows asset for {other}."
            ));
        }
    };
    let expected = format!("spicetify-{version}-windows-{architecture}.zip");
    selected(release, &expected, "spicetify/cli", version)
}
pub async fn resolve_marketplace() -> Result<VerifiedRelease, String> {
    let releases = fetch(MARKETPLACE_RELEASES).await?;
    let release = newest_stable(&releases).ok_or("No stable Marketplace release was found.")?;
    selected(
        release,
        "marketplace.zip",
        "spicetify/marketplace",
        release.tag_name.trim_start_matches('v'),
    )
}
async fn fetch(url: &str) -> Result<Vec<Release>, String> {
    let response = download::http_client(USER_AGENT)?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(download::err)?
        .error_for_status()
        .map_err(download::err)?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !content_type
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        return Err("GitHub returned a non-JSON release response.".into());
    }
    response.json().await.map_err(download::err)
}
fn newest_stable(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            version.pre.is_empty().then_some((version, release))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| release)
}
fn selected(
    release: &Release,
    expected_name: &str,
    repository: &str,
    version: &str,
) -> Result<VerifiedRelease, String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(expected_name))
        .cloned()
        .ok_or_else(|| format!("The stable release has no {expected_name} asset."))?;
    let expected_prefix = format!("https://github.com/{repository}/releases/download/");
    if !asset.browser_download_url.starts_with(&expected_prefix) {
        return Err("The release asset points outside the official GitHub repository.".into());
    }
    let digest = asset
        .digest
        .as_deref()
        .and_then(parse_sha256_digest)
        .ok_or(
            "The official GitHub asset has no valid SHA-256 digest; installation was stopped.",
        )?;
    Ok(VerifiedRelease {
        version: version.to_string(),
        asset,
        digest,
    })
}
pub async fn download_verified(
    release: &VerifiedRelease,
    destination: &Path,
    cancellation: &Cancellation,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let actual = download::download_to(
        &download::http_client(USER_AGENT)?,
        &release.asset.browser_download_url,
        destination,
        release.asset.size,
        on_progress,
        || cancellation.is_cancelled(),
    )
    .await?;
    if actual != release.digest {
        let _ = tokio::fs::remove_file(destination).await;
        return Err("The downloaded asset failed SHA-256 verification.".into());
    }
    Ok(())
}
/// Strict ZIP extraction for trusted release archives. Unlike a convenience
/// extractor, this treats traversal entries and symlinks as an error instead
/// of silently ignoring them.
pub fn extract_verified_zip(
    archive_path: &Path,
    destination: &Path,
    cancellation: &Cancellation,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(download::err)?;
    let mut archive = zip::ZipArchive::new(file).map_err(download::err)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("The release archive contains an unreasonable number of entries.".into());
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(download::err)?;
        if entry.enclosed_name().is_none() {
            return Err("The release archive contains an unsafe path.".into());
        }
        if is_symlink(&entry) {
            return Err("The release archive contains a symbolic link.".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("The release archive size is invalid.")?;
        if total > MAX_UNPACKED_BYTES {
            return Err("The release archive is unexpectedly large.".into());
        }
    }
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(download::err)?;
    }
    std::fs::create_dir_all(destination).map_err(download::err)?;
    let mut done = 0u64;
    let mut buffer = vec![0u8; 128 * 1024];
    for index in 0..archive.len() {
        if cancellation.is_cancelled() {
            return Err(download::CANCELLED.into());
        }
        let mut entry = archive.by_index(index).map_err(download::err)?;
        let relative: PathBuf = entry
            .enclosed_name()
            .ok_or("The release archive contains an unsafe path.")?
            .to_path_buf();
        let output = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output).map_err(download::err)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(download::err)?;
        }
        let mut target = std::fs::File::create(&output).map_err(download::err)?;
        loop {
            let read = entry.read(&mut buffer).map_err(download::err)?;
            if read == 0 {
                break;
            }
            target.write_all(&buffer[..read]).map_err(download::err)?;
            done += read as u64;
            on_progress(done, total.max(done));
            if cancellation.is_cancelled() {
                return Err(download::CANCELLED.into());
            }
        }
    }
    Ok(())
}
fn is_symlink<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> bool {
    entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify_hub::models::SpotifyHubAction;
    use crate::spotify_hub::state::SpotifyHubState;
    use zip::write::SimpleFileOptions;
    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("myle-spicetify-{name}-{}", std::process::id()))
    }
    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    fn release(tag: &str, prerelease: bool, digest: Option<&str>) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease,
            assets: vec![Asset {
                name: "marketplace.zip".into(),
                browser_download_url: format!(
                    "https://github.com/spicetify/marketplace/releases/download/{tag}/marketplace.zip"
                ),
                size: Some(10),
                digest: digest.map(str::to_string),
            }],
        }
    }
    #[test]
    fn stable_release_wins_over_newer_beta() {
        let list = vec![
            release("v3.0.0-beta.19", true, None),
            release("v2.45.0", false, None),
            release("v2.45.1", false, None),
        ];
        assert_eq!(newest_stable(&list).unwrap().tag_name, "v2.45.1");
    }
    #[test]
    fn digest_is_mandatory_and_well_formed() {
        let missing = release("v1.0.0", false, None);
        assert!(
            selected(
                &missing,
                "marketplace.zip",
                "spicetify/marketplace",
                "1.0.0"
            )
            .is_err()
        );
        let malformed = release("v1.0.0", false, Some("sha256:nope"));
        assert!(
            selected(
                &malformed,
                "marketplace.zip",
                "spicetify/marketplace",
                "1.0.0"
            )
            .is_err()
        );
    }
    #[test]
    fn unofficial_download_host_is_rejected() {
        let mut item = release(
            "v1.0.0",
            false,
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        item.assets[0].browser_download_url = "https://example.com/marketplace.zip".into();
        assert!(selected(&item, "marketplace.zip", "spicetify/marketplace", "1.0.0").is_err());
    }
    #[test]
    fn malformed_and_traversing_archives_are_rejected() {
        let root = temp_dir("unsafe-zip");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let malformed = root.join("malformed.zip");
        std::fs::write(&malformed, b"this is not a zip").unwrap();
        let state = SpotifyHubState::default();
        let cancellation = state
            .begin("malformed".into(), SpotifyHubAction::InstallSpicetify)
            .unwrap();
        assert!(
            extract_verified_zip(
                &malformed,
                &root.join("malformed-out"),
                &cancellation,
                |_, _| {}
            )
            .is_err()
        );
        let unsafe_zip = root.join("unsafe.zip");
        zip_with(&unsafe_zip, &[("../escape.txt", b"must not escape")]);
        assert!(
            extract_verified_zip(
                &unsafe_zip,
                &root.join("unsafe-out"),
                &cancellation,
                |_, _| {}
            )
            .is_err()
        );
        assert!(!root.join("escape.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn cancelled_extraction_writes_no_payload() {
        let root = temp_dir("cancelled-zip");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("release.zip");
        zip_with(&archive, &[("spicetify.exe", b"verified payload")]);
        let state = SpotifyHubState::default();
        let cancellation = state
            .begin("cancelled".into(), SpotifyHubAction::InstallSpicetify)
            .unwrap();
        cancellation.cancel();
        let destination = root.join("out");
        let error = extract_verified_zip(&archive, &destination, &cancellation, |_, _| {})
            .expect_err("cancelled extraction must stop");
        assert_eq!(error, download::CANCELLED);
        assert!(!destination.join("spicetify.exe").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
