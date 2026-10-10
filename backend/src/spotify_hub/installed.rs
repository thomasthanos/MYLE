//! Marketplace does not ship a version in manifest.json. Keep a receipt
//! inside the swapped payload so failed installs restore its old version too.
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::download::{err, to_hex};

pub fn applied(apps: &Path) -> bool {
    if !apps.join("xpui/spicetify-config.json").is_file()
        || !apps.join("xpui/spicetify-routes-marketplace.js").is_file()
    {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(apps) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        if entry.file_name().to_string_lossy().ends_with(".spa") {
            let Ok(kind) = entry.file_type() else {
                return false;
            };
            if !kind.is_dir() {
                return false;
            }
        }
    }
    true
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    version: String,
    index_sha256: String,
}

pub fn write_version(path: &Path, version: &str) -> Result<(), String> {
    let receipt = Receipt {
        version: version.into(),
        index_sha256: index_digest(path).ok_or("Marketplace index.js is unavailable.")?,
    };
    std::fs::write(
        path.join(".myle-release.json"),
        serde_json::to_vec(&receipt).map_err(err)?,
    )
    .map_err(err)
}

pub fn version(path: &Path) -> Option<String> {
    let receipt: Receipt =
        serde_json::from_slice(&std::fs::read(path.join(".myle-release.json")).ok()?).ok()?;
    semver::Version::parse(&receipt.version).ok()?;
    (index_digest(path)? == receipt.index_sha256).then_some(receipt.version)
}

fn index_digest(path: &Path) -> Option<String> {
    Some(to_hex(&Sha256::digest(
        std::fs::read(path.join("index.js")).ok()?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_update_or_missing_marketplace_requires_reapply_even_with_current_versions() {
        let root =
            std::env::temp_dir().join(format!("myle-spotify-applied-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("xpui")).unwrap();
        assert!(!applied(&root));
        std::fs::write(root.join("xpui/spicetify-config.json"), b"{}").unwrap();
        assert!(!applied(&root));
        std::fs::write(
            root.join("xpui/spicetify-routes-marketplace.js"),
            b"marketplace",
        )
        .unwrap();
        assert!(applied(&root));
        std::fs::write(root.join("xpui.spa"), b"new stock Spotify").unwrap();
        assert!(
            !applied(&root),
            "fresh stock packages invalidate the old extracted customization"
        );
        assert!(!applied(&root.join("missing")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_version_is_unknown_until_recorded_and_invalidated_by_external_changes() {
        let root = std::env::temp_dir().join(format!("myle-marketplace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.js"), b"original Marketplace").unwrap();
        assert_eq!(version(&root), None);
        write_version(&root, "1.0.11").unwrap();
        assert_eq!(version(&root).as_deref(), Some("1.0.11"));
        std::fs::write(root.join("index.js"), b"externally updated Marketplace").unwrap();
        assert_eq!(version(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
