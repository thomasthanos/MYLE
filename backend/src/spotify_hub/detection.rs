use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::SystemTime;

use sha2::{Digest, Sha256};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

use crate::apps::process::hidden;
use crate::download::to_hex;

use super::filesystem::validate_no_reparse_points;
use super::models::{
    InstallationState, MarketplaceState, Prerequisites, SpicetifyState, SpotifyHubSnapshot,
};

const DESKTOP_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Spotify";
const STORE_FAMILY: &str = "SpotifyAB.SpotifyMusic_zpdnekdrzrea0";
/// Per-user AppX registrations; one subkey per installed package full name.
const APPX_PACKAGES_KEY: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

/// `spicetify -v` of the last CLI build seen, keyed by the exe's size and
/// modification time. Detection runs on every visit to the page and this is
/// the only part of it that starts a process.
static CLI_VERSION: Mutex<Option<CliVersion>> = Mutex::new(None);

/// Size and modification time of the exe, then `(version, healthy)`.
type CliVersion = (u64, SystemTime, Option<String>, bool);

#[derive(Clone, Debug)]
pub struct KnownPaths {
    pub local: PathBuf,
    pub desktop_spotify: PathBuf,
    pub local_spotify: PathBuf,
    pub cli: PathBuf,
    pub config: PathBuf,
    pub marketplace: PathBuf,
    pub placeholder_theme: PathBuf,
    pub store_data: PathBuf,
    pub start_shortcut: PathBuf,
    pub desktop_shortcut: PathBuf,
}

impl KnownPaths {
    pub fn discover() -> Result<Self, String> {
        let roaming = env_path("APPDATA")?;
        let local = env_path("LOCALAPPDATA")?;
        let profile = env_path("USERPROFILE")?;
        let config = roaming.join("spicetify");
        let desktop = user_desktop().unwrap_or_else(|| profile.join("Desktop"));
        Ok(Self {
            desktop_spotify: roaming.join("Spotify").join("Spotify.exe"),
            local_spotify: local.join("Spotify"),
            cli: local.join("spicetify"),
            marketplace: config.join("CustomApps").join("marketplace"),
            placeholder_theme: config.join("Themes").join("marketplace"),
            store_data: local.join("Packages").join(STORE_FAMILY),
            start_shortcut: roaming
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Spotify.lnk"),
            desktop_shortcut: desktop.join("Spotify.lnk"),
            local,
            config,
        })
    }

    pub fn cli_exe(&self) -> PathBuf {
        self.cli.join("spicetify.exe")
    }

    pub fn desktop_dir(&self) -> PathBuf {
        self.desktop_spotify
            .parent()
            .expect("known Spotify path has a parent")
            .to_path_buf()
    }

    pub fn purge_targets(&self) -> Vec<PathBuf> {
        vec![
            self.desktop_dir(),
            self.local_spotify.clone(),
            self.cli.clone(),
            self.config.clone(),
            self.store_data.clone(),
            self.start_shortcut.clone(),
            self.desktop_shortcut.clone(),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Detection {
    pub paths: KnownPaths,
    pub desktop: InstallationState,
    pub store: InstallationState,
    pub spicetify: SpicetifyState,
    pub marketplace: MarketplaceState,
    pub unknown_desktop_registration: bool,
}

impl Detection {
    pub fn snapshot(
        &self,
        last_outcome: Option<super::models::SpotifyHubOutcome>,
        active_job: Option<super::models::ActiveJob>,
    ) -> SpotifyHubSnapshot {
        let desktop = self.desktop.installed;
        let (supported, message) = if desktop {
            (true, None)
        } else if self.store.installed {
            (
                false,
                Some(
                    "The Microsoft Store build cannot be customized. Install Spotify Desktop first."
                        .into(),
                ),
            )
        } else {
            (false, Some("Install Spotify Desktop first.".into()))
        };
        SpotifyHubSnapshot {
            desktop: self.desktop.clone(),
            store: self.store.clone(),
            spicetify: self.spicetify.clone(),
            marketplace: self.marketplace.clone(),
            prerequisites: Prerequisites {
                desktop_spotify: desktop,
                supported,
                message,
            },
            last_outcome,
            active_job,
        }
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        for value in [
            self.desktop.installed.to_string(),
            self.desktop.version.clone().unwrap_or_default(),
            self.store.installed.to_string(),
            self.store.version.clone().unwrap_or_default(),
            self.spicetify.installed.to_string(),
            self.marketplace.installed.to_string(),
            self.unknown_desktop_registration.to_string(),
        ] {
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }
        // What exists and what kind of thing it is. Sizes and times are left
        // out on purpose: a running Spotify rewrites its cache and prefs every
        // few seconds, which made every purge fail as "state changed".
        for path in self.paths.purge_targets() {
            hasher.update(path.as_os_str().to_string_lossy().as_bytes());
            let kind = match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_symlink() => 3u8,
                Ok(meta) if meta.is_dir() => 2,
                Ok(_) => 1,
                Err(_) => 0,
            };
            hasher.update([kind]);
        }
        to_hex(&hasher.finalize())
    }
}

pub async fn detect() -> Result<Detection, String> {
    let paths = KnownPaths::discover()?;
    let (registered_version, registered_location, registry_present) = desktop_registry();
    let desktop_present = paths.desktop_spotify.is_file();
    let unknown_desktop_registration = registry_present
        && !desktop_present
        && registered_location.is_some_and(|location| {
            !same_path(&location, &paths.desktop_dir()) && location.join("Spotify.exe").is_file()
        });

    let store_folder = paths.store_data.is_dir();
    let store_version = store_version();
    let store_present = store_folder || store_version.is_some();

    let cli_exe = paths.cli_exe();
    let cli_present = cli_exe.is_file();
    let (spicetify_version, healthy) =
        if cli_present && validate_no_reparse_points(&paths.cli).is_ok() {
            cached_executable_version(&cli_exe).await
        } else {
            (None, false)
        };

    Ok(Detection {
        desktop: InstallationState {
            installed: desktop_present,
            version: registered_version,
        },
        store: InstallationState {
            installed: store_present,
            version: store_version,
        },
        spicetify: SpicetifyState {
            installed: cli_present,
            version: spicetify_version,
            healthy,
        },
        marketplace: MarketplaceState {
            installed: paths.marketplace.join("manifest.json").is_file()
                || paths.marketplace.join("index.js").is_file(),
            version: super::installed::version(&paths.marketplace),
        },
        paths,
        unknown_desktop_registration,
    })
}

async fn cached_executable_version(exe: &std::path::Path) -> (Option<String>, bool) {
    let stamp = std::fs::metadata(exe)
        .ok()
        .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
    if let Some((len, modified)) = stamp
        && let Some((cached_len, cached_modified, version, healthy)) =
            CLI_VERSION.lock().unwrap_or_else(|p| p.into_inner()).clone()
        && cached_len == len
        && cached_modified == modified
    {
        return (version, healthy);
    }
    let (version, healthy) = executable_version(exe).await;
    if let Some((len, modified)) = stamp {
        *CLI_VERSION.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((len, modified, version.clone(), healthy));
    }
    (version, healthy)
}

async fn executable_version(exe: &std::path::Path) -> (Option<String>, bool) {
    let output = hidden(exe).arg("-v").stdin(Stdio::null()).output().await;
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let version = text
                .split_whitespace()
                .find(|part| part.chars().any(|c| c.is_ascii_digit()))
                .map(|part| part.trim_start_matches('v').to_string());
            (version, true)
        }
        _ => (None, false),
    }
}

/// The Microsoft Store build's version, read from the per-user AppX
/// registrations instead of starting PowerShell for `Get-AppxPackage`
/// (half a second or more on every visit to the page).
fn store_version() -> Option<String> {
    let packages = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(APPX_PACKAGES_KEY, KEY_READ)
        .ok()?;
    packages
        .enum_keys()
        .flatten()
        .filter_map(|name| store_package_version(&name))
        .max_by_key(|version| version_key(version))
}

/// `SpotifyAB.SpotifyMusic_1.268.495.0_x64__zpdnekdrzrea0` -> `1.268.495.0`.
fn store_package_version(full_name: &str) -> Option<String> {
    let (name, publisher) = STORE_FAMILY.split_once('_')?;
    let rest = full_name.strip_prefix(name)?.strip_prefix('_')?;
    if !rest.ends_with(&format!("__{publisher}")) {
        return None;
    }
    let version = rest.split('_').next()?;
    (!version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .then(|| version.to_string())
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn desktop_registry() -> (Option<String>, Option<PathBuf>, bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey_with_flags(DESKTOP_UNINSTALL_KEY, KEY_READ) else {
        return (None, None, false);
    };
    let version = key.get_value::<String, _>("DisplayVersion").ok();
    let location = key
        .get_value::<String, _>("InstallLocation")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    (version, location, true)
}

fn user_desktop() -> Option<PathBuf> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
            KEY_READ,
        )
        .ok()?;
    let raw: String = key.get_value("Desktop").ok()?;
    Some(expand_known_env(&raw))
}

fn expand_known_env(value: &str) -> PathBuf {
    let mut expanded = value.to_string();
    for name in ["USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
        if let Ok(replacement) = std::env::var(name) {
            expanded = expanded.replace(&format!("%{name}%"), &replacement);
        }
    }
    PathBuf::from(expanded)
}

fn env_path(name: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(name).ok_or_else(|| format!("{name} is unavailable"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{name} is not an absolute path"));
    }
    Ok(path)
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_environment_expansion_does_not_expand_arbitrary_variables() {
        let value = expand_known_env(r"%NOT_ALLOWED%\Spotify");
        assert!(value.to_string_lossy().contains("%NOT_ALLOWED%"));
    }

    #[test]
    fn store_versions_come_from_the_package_full_name() {
        assert_eq!(
            store_package_version("SpotifyAB.SpotifyMusic_1.268.495.0_x64__zpdnekdrzrea0")
                .as_deref(),
            Some("1.268.495.0")
        );
        assert_eq!(
            store_package_version("SpotifyAB.SpotifyMusic_1.2.3.0_neutral_~_zpdnekdrzrea0"),
            None,
            "a resource or bundle entry is not the app itself"
        );
        assert_eq!(
            store_package_version("Microsoft.WindowsCalculator_11.2607.0.0_x64__8wekyb3d8bbwe"),
            None
        );
        assert!(version_key("1.10.0.0") > version_key("1.9.9.0"));
    }

    #[test]
    fn windows_path_comparison_is_case_insensitive() {
        assert!(same_path(
            std::path::Path::new(r"C:\Users\Me\Spotify"),
            std::path::Path::new(r"c:\users\me\spotify")
        ));
    }
}
