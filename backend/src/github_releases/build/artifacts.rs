//! The files a build made that are worth shipping: installers, archives,
//! packages, update manifests and signatures, written since the build
//! started. Inside a Rust `target` folder only `bundle` output and the
//! programs right in the profile folder count; unpacked app folders,
//! dependencies and intermediate files never do.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

const EXTENSIONS: &[&str] = &[
    "exe",
    "msi",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "zip",
    "7z",
    "nupkg",
    "snupkg",
    "whl",
    "gz",
    "tgz",
    "xz",
    "dmg",
    "appimage",
    "deb",
    "rpm",
    "apk",
    "aab",
    "sig",
    "blockmap",
    "crx",
    "xpi",
    "vsix",
    "jar",
];
/// Folders never looked into.
const SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "src",
    "deps",
    "incremental",
    ".fingerprint",
    "examples",
    "build",
    "win-unpacked",
    "win-ia32-unpacked",
    "win-arm64-unpacked",
    "linux-unpacked",
    "mac",
    "mac-arm64",
    ".cache",
    "obj",
    "__pycache__",
    ".venv",
    "venv",
    "public",
    "static",
    "assets",
];
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: String,
    /// From the project folder, with `/`.
    pub rel: String,
    pub name: String,
    pub size: u64,
    /// Unix milliseconds.
    pub modified: i64,
    /// `installer`, `archive`, `package`, `update`, `signature`, `program`.
    pub kind: &'static str,
}

fn kind_of(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if (lower.starts_with("latest")
        && (lower.ends_with(".yml") || lower.ends_with(".yaml") || lower.ends_with(".json")))
        || lower.ends_with(".blockmap")
    {
        return Some("update");
    }
    let ext = lower.rsplit('.').next()?;
    if !EXTENSIONS.contains(&ext) {
        return None;
    }
    Some(match ext {
        "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" | "dmg" | "deb" | "rpm" | "apk"
        | "aab" | "appimage" => "installer",
        "exe" if lower.contains("setup") || lower.contains("install") => "installer",
        "exe" => "program",
        "sig" => "signature",
        "nupkg" | "snupkg" | "whl" | "crx" | "xpi" | "vsix" | "jar" => "package",
        _ => "archive",
    })
}

/// Inside a `target` folder: `target/<profile>/<file>` or anything under a
/// `bundle` folder.
fn allowed_in_target(rel_parts: &[String]) -> bool {
    let Some(target) = rel_parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("target"))
    else {
        return true;
    };
    let after = &rel_parts[target + 1..];
    // `target/release/app.exe`, `target/<triple>/release/app.exe`, or bundles.
    after.len() == 2
        || (after.len() == 3 && after[1].eq_ignore_ascii_case("release"))
        || after.iter().any(|p| p.eq_ignore_ascii_case("bundle"))
}

/// Files in `dir` written at or after `since` (a little slack for clocks).
pub(crate) fn find(dir: &Path, since: SystemTime) -> Vec<Artifact> {
    let since = since.checked_sub(Duration::from_secs(2)).unwrap_or(since);
    let mut found = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    while let Some((folder, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                let lower = name.to_lowercase();
                let in_target = path.strip_prefix(dir).ok().is_some_and(|rel| {
                    rel.components()
                        .any(|c| c.as_os_str().eq_ignore_ascii_case("target"))
                });
                // `build` is skipped only inside `target` (build scripts):
                // elsewhere it can be where a tool puts what it made.
                let skip = SKIP.contains(&lower.as_str()) && (lower != "build" || in_target);
                if depth < MAX_DEPTH
                    && !skip
                    && !lower.ends_with("-unpacked")
                    && !(lower.starts_with('.') && lower != ".")
                {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Some(file_kind) = kind_of(&name) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if modified < since {
                continue;
            }
            let rel_path = path.strip_prefix(dir).unwrap_or(&path);
            let parts: Vec<String> = rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            if !allowed_in_target(&parts) {
                continue;
            }
            found.push(Artifact {
                rel: parts.join("/"),
                path: path.display().to_string(),
                name,
                size: meta.len(),
                modified: modified
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0),
                kind: file_kind,
            });
            if found.len() >= MAX_FILES {
                return sorted(found);
            }
        }
    }
    sorted(found)
}

fn sorted(mut found: Vec<Artifact>) -> Vec<Artifact> {
    let rank = |a: &Artifact| match a.kind {
        "installer" => 0,
        "program" => 1,
        "package" => 2,
        "archive" => 3,
        "update" => 4,
        _ => 5,
    };
    found.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()))
    });
    found
}

/// Which artifacts to tick by themselves: installers, packages, update
/// files and signatures; a bare program only when nothing else was made.
pub(crate) fn preselect(artifacts: &[Artifact]) -> Vec<String> {
    let shippable: Vec<&Artifact> = artifacts.iter().filter(|a| a.kind != "program").collect();
    let chosen: Vec<&Artifact> = if shippable
        .iter()
        .any(|a| a.kind != "update" && a.kind != "signature")
    {
        shippable
    } else {
        artifacts.iter().collect()
    };
    chosen.into_iter().map(|a| a.path.clone()).collect()
}

/// The SHA-256 of a file, in hex.
pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fresh_shippable_files_are_found() {
        let dir = std::env::temp_dir().join(format!("myle-gr-artifacts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = [
            "dist/App Setup 1.0.0.exe",
            "dist/App Setup 1.0.0.exe.blockmap",
            "dist/latest.yml",
            "dist/builder-debug.yml",
            "dist/win-unpacked/App.exe",
            "src-tauri/target/release/bundle/nsis/App_1.0.0_x64-setup.exe",
            "src-tauri/target/release/bundle/nsis/App_1.0.0_x64-setup.exe.sig",
            "src-tauri/target/release/app.exe",
            "src-tauri/target/release/nsis/x64/plugin.exe",
            "src-tauri/target/release/build/x/out/helper.exe",
            "src-tauri/target/release/deps/app.exe",
            "node_modules/electron/dist/electron.exe",
            "src/icon.zip",
            "README.md",
        ];
        let before = SystemTime::now();
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        let found = find(&dir, before);
        let rels: Vec<&str> = found.iter().map(|a| a.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "dist/App Setup 1.0.0.exe",
                "src-tauri/target/release/bundle/nsis/App_1.0.0_x64-setup.exe",
                "src-tauri/target/release/app.exe",
                "dist/App Setup 1.0.0.exe.blockmap",
                "dist/latest.yml",
                "src-tauri/target/release/bundle/nsis/App_1.0.0_x64-setup.exe.sig",
            ]
        );
        let ticked = preselect(&found);
        assert_eq!(ticked.len(), 5);
        assert!(!ticked.iter().any(|p| p.ends_with("app.exe")));
        // Nothing new since a later moment.
        assert!(find(&dir, SystemTime::now() + Duration::from_secs(60)).is_empty());
        assert_eq!(
            sha256(&dir.join("README.md")).unwrap(),
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
