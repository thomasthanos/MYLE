//! The version of a project, in every file that states it: package.json and
//! its lock file, Cargo.toml (and Cargo.lock), tauri.conf.json, pyproject.toml,
//! a .csproj, a browser extension's manifest.json.
//!
//! Files are changed by replacing the version text alone, so their layout,
//! comments and key order stay exactly as they were.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    PackageJson,
    PackageLock,
    CargoToml,
    TauriConf,
    Pyproject,
    Csproj,
    ExtensionManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    /// From the project folder, with `/`.
    pub path: String,
    pub kind: FileKind,
    pub version: String,
    /// Left out of bumps by the user.
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Versions {
    pub files: Vec<VersionFile>,
    /// The project's version: the one most files agree on (package.json's
    /// on a tie).
    pub current: Option<String>,
    /// Files (not skipped) that disagree with `current`.
    pub mismatched: Vec<String>,
}

// ─── Reading ───────────────────────────────────────────────────────────────

/// The byte range of the string value at `path` (`["packages", "", "version"]`)
/// in JSON `text`, without its quotes.
pub(crate) fn json_string_range(text: &str, path: &[&str]) -> Option<(usize, usize)> {
    enum Frame {
        /// The key whose value comes (or is being read) next.
        Object {
            key: Option<String>,
            expect_key: bool,
        },
        Array,
    }
    let bytes = text.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => stack.push(Frame::Object {
                key: None,
                expect_key: true,
            }),
            b'[' => stack.push(Frame::Array),
            b'}' | b']' => {
                stack.pop();
            }
            b',' => {
                if let Some(Frame::Object { key, expect_key }) = stack.last_mut() {
                    *key = None;
                    *expect_key = true;
                }
            }
            b':' => {
                if let Some(Frame::Object { expect_key, .. }) = stack.last_mut() {
                    *expect_key = false;
                }
            }
            b'"' => {
                // The string's end, stepping over escapes.
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if end >= bytes.len() {
                    return None;
                }
                let raw = &text[i + 1..end];
                match stack.last_mut() {
                    Some(Frame::Object {
                        key,
                        expect_key: true,
                    }) => *key = Some(raw.replace("\\\"", "\"")),
                    _ => {
                        let keys: Vec<&str> = stack
                            .iter()
                            .map(|frame| match frame {
                                Frame::Object { key, .. } => key.as_deref().unwrap_or("\u{0}"),
                                Frame::Array => "\u{0}",
                            })
                            .collect();
                        if keys == path {
                            return Some((i + 1, end));
                        }
                    }
                }
                i = end;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn json_version(text: &str, path: &[&str]) -> Option<String> {
    json_string_range(text, path).map(|(a, b)| text[a..b].to_string())
}

/// A TOML `version = "…"` in `[section]` (`package`, `workspace.package`,
/// `project`, `tool.poetry`): the byte range of the version text.
pub(crate) fn toml_version_range(text: &str, section: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let name = trimmed
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim();
            inside = !trimmed.starts_with("[[") && name == section;
        } else if inside
            && let Some(rest) = trimmed.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote != '"' && quote != '\'' {
                // `version.workspace = true` and the like.
                return None;
            }
            let start_in_line = line.find(value)? + 1;
            let len = value[1..].find(quote)?;
            let start = offset + start_in_line;
            return Some((start, start + len));
        }
        offset += line.len();
    }
    None
}

fn toml_version(text: &str, section: &str) -> Option<String> {
    toml_version_range(text, section).map(|(a, b)| text[a..b].to_string())
}

fn toml_name(text: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[package]";
        } else if inside
            && let Some(rest) = trimmed.strip_prefix("name")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            return Some(value.trim().trim_matches(['"', '\'']).to_string());
        }
    }
    None
}

pub(crate) fn csproj_version_range(text: &str) -> Option<(usize, usize)> {
    let start = text.find("<Version>")? + "<Version>".len();
    let len = text[start..].find("</Version>")?;
    Some((start, start + len))
}

/// What looks like a version (`1.2.3`, `1.2.3-beta.1`, `1.2`) rather than a
/// path (`../package.json`) or a placeholder.
fn looks_like_version(value: &str) -> bool {
    let value = value.trim();
    value.chars().next().is_some_and(|c| c.is_ascii_digit())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
}

fn read_text(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.strip_prefix('\u{feff}')
            .map(str::to_string)
            .unwrap_or(text),
    )
}

/// The version stated by `file` of kind `kind`.
pub(crate) fn read_version(path: &Path, kind: FileKind) -> Option<String> {
    let text = read_text(path)?;
    let version = match kind {
        FileKind::PackageJson | FileKind::TauriConf | FileKind::ExtensionManifest => {
            json_version(&text, &["version"])
        }
        FileKind::PackageLock => json_version(&text, &["version"])
            .or_else(|| json_version(&text, &["packages", "", "version"])),
        FileKind::CargoToml => {
            toml_version(&text, "package").or_else(|| toml_version(&text, "workspace.package"))
        }
        FileKind::Pyproject => {
            toml_version(&text, "project").or_else(|| toml_version(&text, "tool.poetry"))
        }
        FileKind::Csproj => csproj_version_range(&text).map(|(a, b)| text[a..b].to_string()),
    }?;
    looks_like_version(&version).then_some(version)
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "release",
    ".git",
    "vendor",
    ".venv",
    "venv",
    "bin",
    "obj",
    "gen",
    "coverage",
];

/// The folders where an app's own version files live: the project folder
/// and the folders right inside it that hold a tauri.conf.json (Tauri's
/// `src-tauri`, MYLE's `backend`). Other folders (a mobile app, a helper
/// crate) usually have their own version and are left alone.
fn version_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![dir.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str())
            })
            .map(|entry| entry.path())
            .filter(|path| path.join("tauri.conf.json").is_file())
            .collect();
        found.sort();
        dirs.extend(found);
    }
    dirs
}

fn rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Every version file of the project in `dir`.
pub(crate) fn detect(dir: &Path, skip: &[String]) -> Versions {
    let mut files = Vec::new();
    for folder in version_dirs(dir) {
        let mut candidates: Vec<(PathBuf, FileKind)> = vec![
            (folder.join("package.json"), FileKind::PackageJson),
            (folder.join("package-lock.json"), FileKind::PackageLock),
            (folder.join("Cargo.toml"), FileKind::CargoToml),
            (folder.join("tauri.conf.json"), FileKind::TauriConf),
            (folder.join("pyproject.toml"), FileKind::Pyproject),
        ];
        if folder == dir {
            candidates.push((folder.join("manifest.json"), FileKind::ExtensionManifest));
            if let Ok(entries) = std::fs::read_dir(&folder) {
                let mut projects: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
                    })
                    .collect();
                projects.sort();
                candidates.extend(projects.into_iter().map(|p| (p, FileKind::Csproj)));
            }
        }
        for (path, kind) in candidates {
            if !path.is_file() {
                continue;
            }
            if kind == FileKind::ExtensionManifest
                && !read_text(&path).is_some_and(|text| text.contains("\"manifest_version\""))
            {
                continue;
            }
            if let Some(version) = read_version(&path, kind) {
                let path = rel(dir, &path);
                files.push(VersionFile {
                    skipped: skip.iter().any(|s| s.eq_ignore_ascii_case(&path)),
                    path,
                    kind,
                    version,
                });
            }
        }
    }
    let current = main_version(&files);
    let mismatched = files
        .iter()
        .filter(|file| !file.skipped && Some(&file.version) != current.as_ref())
        .map(|file| file.path.clone())
        .collect();
    Versions {
        files,
        current,
        mismatched,
    }
}

fn main_version(files: &[VersionFile]) -> Option<String> {
    let counted: Vec<&VersionFile> = files.iter().filter(|f| !f.skipped).collect();
    let mut best: Option<(&str, usize, bool)> = None;
    for file in &counted {
        let count = counted.iter().filter(|f| f.version == file.version).count();
        let primary = file.kind == FileKind::PackageJson;
        let better = match best {
            None => true,
            Some((_, n, p)) => count > n || (count == n && primary && !p),
        };
        if better {
            best = Some((&file.version, count, primary));
        }
    }
    best.map(|(version, _, _)| version.to_string())
}

// ─── Bumping ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

/// The next version: `1.2.3` → `1.2.4` / `1.3.0` / `2.0.0`. A pre-release
/// (`1.3.0-beta.2`) becomes its release on a patch bump (`1.3.0`).
pub(crate) fn bump(current: &str, kind: Bump) -> Result<String, String> {
    let mut version = parse(current)?;
    let pre = !version.pre.is_empty();
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    match kind {
        Bump::Patch if pre => {}
        Bump::Patch => version.patch += 1,
        Bump::Minor => {
            version.minor += 1;
            version.patch = 0;
        }
        Bump::Major => {
            version.major += 1;
            version.minor = 0;
            version.patch = 0;
        }
    }
    Ok(version.to_string())
}

/// A version, also a short one (`1.2` = `1.2.0`).
pub(crate) fn parse(text: &str) -> Result<semver::Version, String> {
    let text = text.trim().trim_start_matches('v');
    semver::Version::parse(text)
        .or_else(|_| semver::Version::parse(&format!("{text}.0")))
        .map_err(|_| format!("\"{text}\" is not a version like 1.2.3."))
}

/// A version the user typed: `1.2.3`, `1.2.3-beta.1`.
pub(crate) fn check_new_version(text: &str) -> Result<String, String> {
    let text = text.trim().trim_start_matches('v');
    semver::Version::parse(text)
        .map(|v| v.to_string())
        .map_err(|_| format!("\"{text}\" is not a version like 1.2.3 or 1.2.3-beta.1."))
}

fn replace_range(text: &str, (a, b): (usize, usize), with: &str) -> String {
    format!("{}{}{}", &text[..a], with, &text[b..])
}

/// `text` (a file of `kind`) with its version set to `new`.
pub(crate) fn set_version(text: &str, kind: FileKind, new: &str) -> Result<String, String> {
    let missing = || "The version was not found in the file.".to_string();
    Ok(match kind {
        FileKind::PackageJson | FileKind::TauriConf | FileKind::ExtensionManifest => {
            let range = json_string_range(text, &["version"]).ok_or_else(missing)?;
            replace_range(text, range, new)
        }
        FileKind::PackageLock => {
            let mut out = text.to_string();
            let mut found = false;
            // The inner one first: changing it doesn't move the outer one.
            for path in [&["packages", "", "version"][..], &["version"][..]] {
                if let Some(range) = json_string_range(&out, path) {
                    out = replace_range(&out, range, new);
                    found = true;
                }
            }
            if !found {
                return Err(missing());
            }
            out
        }
        FileKind::CargoToml => {
            let range = toml_version_range(text, "package")
                .or_else(|| toml_version_range(text, "workspace.package"))
                .ok_or_else(missing)?;
            replace_range(text, range, new)
        }
        FileKind::Pyproject => {
            let range = toml_version_range(text, "project")
                .or_else(|| toml_version_range(text, "tool.poetry"))
                .ok_or_else(missing)?;
            replace_range(text, range, new)
        }
        FileKind::Csproj => {
            let range = csproj_version_range(text).ok_or_else(missing)?;
            replace_range(text, range, new)
        }
    })
}

/// Cargo.lock with the version of the crate `name` set to `new`.
pub(crate) fn set_lock_version(text: &str, name: &str, old: &str, new: &str) -> Option<String> {
    let needle = format!("name = \"{name}\"\nversion = \"{old}\"");
    let crlf = needle.replace('\n', "\r\n");
    if text.contains(&needle) {
        Some(text.replacen(
            &needle,
            &format!("name = \"{name}\"\nversion = \"{new}\""),
            1,
        ))
    } else if text.contains(&crlf) {
        Some(text.replacen(
            &crlf,
            &format!("name = \"{name}\"\r\nversion = \"{new}\""),
            1,
        ))
    } else {
        None
    }
}

/// One file a bump changes, with its old and new text.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    pub path: PathBuf,
    /// From the project folder, with `/`.
    pub rel: String,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

/// The edits that set every (not skipped) version file of the project in
/// `dir` to `new`, Cargo.lock entries included.
pub(crate) fn plan(dir: &Path, versions: &Versions, new: &str) -> Result<Vec<Edit>, String> {
    let mut edits: Vec<Edit> = Vec::new();
    for file in versions.files.iter().filter(|f| !f.skipped) {
        let path = dir.join(&file.path);
        let before =
            std::fs::read(&path).map_err(|error| format!("Can't read {}: {error}", file.path))?;
        let (bom, text) = match before.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(rest) => (true, String::from_utf8_lossy(rest).into_owned()),
            None => (false, String::from_utf8_lossy(&before).into_owned()),
        };
        let changed = set_version(&text, file.kind, new)
            .map_err(|error| format!("{}: {error}", file.path))?;
        let mut after = if bom {
            vec![0xEF, 0xBB, 0xBF]
        } else {
            Vec::new()
        };
        after.extend_from_slice(changed.as_bytes());
        if file.kind == FileKind::CargoToml
            && let Some(name) = toml_name(&text)
        {
            // The lock file sits next to the manifest or above it (a workspace).
            let folder = path.parent().unwrap_or(dir);
            for lock in [
                folder.join("Cargo.lock"),
                folder.parent().unwrap_or(folder).join("Cargo.lock"),
            ] {
                if !lock.is_file() {
                    continue;
                }
                let lock_before = match edits.iter().find(|e| e.path == lock) {
                    Some(edit) => edit.after.clone(),
                    None => std::fs::read(&lock).map_err(|error| error.to_string())?,
                };
                let lock_text = String::from_utf8_lossy(&lock_before);
                if let Some(updated) = set_lock_version(&lock_text, &name, &file.version, new) {
                    match edits.iter_mut().find(|e| e.path == lock) {
                        Some(edit) => edit.after = updated.into_bytes(),
                        None => edits.push(Edit {
                            rel: rel(dir, &lock),
                            path: lock.clone(),
                            before: lock_before.clone(),
                            after: updated.into_bytes(),
                        }),
                    }
                    break;
                }
            }
        }
        if before != after {
            edits.push(Edit {
                rel: file.path.clone(),
                path,
                before,
                after,
            });
        }
    }
    Ok(edits)
}

/// Writes `edits`; on a failure the files already written are put back.
pub(crate) fn apply(edits: &[Edit]) -> Result<(), String> {
    for (index, edit) in edits.iter().enumerate() {
        if let Err(error) = std::fs::write(&edit.path, &edit.after) {
            undo(&edits[..index]);
            return Err(format!("Can't write {}: {error}", edit.rel));
        }
    }
    Ok(())
}

/// Puts the files of `edits` back as they were.
pub(crate) fn undo(edits: &[Edit]) {
    for edit in edits {
        let _ = std::fs::write(&edit.path, &edit.before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("myle-gr-versions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn json_versions_are_found_at_their_path_only() {
        let lock = r#"{
  "name": "app",
  "version": "1.2.3",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "app", "version": "1.2.3", "dependencies": { "x": "^1" } },
    "node_modules/x": { "version": "9.9.9", "list": ["a", {"version": "0.0.1"}] }
  }
}"#;
        assert_eq!(json_version(lock, &["version"]).as_deref(), Some("1.2.3"));
        assert_eq!(
            json_version(lock, &["packages", "", "version"]).as_deref(),
            Some("1.2.3")
        );
        let changed = set_version(lock, FileKind::PackageLock, "1.3.0").unwrap();
        assert_eq!(changed.matches("1.3.0").count(), 2);
        assert!(changed.contains("\"version\": \"9.9.9\""));
        assert!(changed.contains("\"version\": \"0.0.1\""));
        // A nested "version" before the top-level one is not taken.
        let nested = r#"{"engines": {"version": "0.1.0"}, "deps": [1, 2, {"version": "5"}], "version": "2.0.0"}"#;
        assert_eq!(json_version(nested, &["version"]).as_deref(), Some("2.0.0"));
        let escaped = r#"{"description": "say \"hi\" {", "version": "3.1.4"}"#;
        assert_eq!(
            json_version(escaped, &["version"]).as_deref(),
            Some("3.1.4")
        );
    }

    #[test]
    fn toml_csproj_and_pyproject() {
        let cargo = "[package]\nname = \"my-app\"\nversion = \"0.4.1\" # the app\n\n[dependencies]\nserde = { version = \"1\" }\n";
        assert_eq!(toml_version(cargo, "package").as_deref(), Some("0.4.1"));
        let changed = set_version(cargo, FileKind::CargoToml, "0.5.0").unwrap();
        assert!(changed.contains("version = \"0.5.0\" # the app"));
        assert!(changed.contains("serde = { version = \"1\" }"));
        assert_eq!(
            toml_version("[package]\nversion.workspace = true\n", "package"),
            None
        );
        let workspace = "[workspace]\nmembers = []\n[workspace.package]\nversion = '2.0.0'\n";
        assert_eq!(
            toml_version(workspace, "workspace.package").as_deref(),
            Some("2.0.0")
        );
        let py = "[tool.poetry]\nname = \"x\"\nversion = \"1.0.0\"\n";
        assert_eq!(toml_version(py, "tool.poetry").as_deref(), Some("1.0.0"));
        let csproj = "<Project><PropertyGroup><Version>3.2.1</Version></PropertyGroup></Project>";
        assert_eq!(
            set_version(csproj, FileKind::Csproj, "3.3.0").unwrap(),
            csproj.replace("3.2.1", "3.3.0")
        );
        let lock = "[[package]]\nname = \"my-app\"\nversion = \"0.4.1\"\n\n[[package]]\nname = \"other\"\nversion = \"0.4.1\"\n";
        let lock2 = set_lock_version(lock, "my-app", "0.4.1", "0.5.0").unwrap();
        assert!(lock2.contains("name = \"my-app\"\nversion = \"0.5.0\""));
        assert!(lock2.contains("name = \"other\"\nversion = \"0.4.1\""));
    }

    #[test]
    fn a_tauri_app_like_myle_and_a_mismatch() {
        let dir = temp("tauri");
        std::fs::write(
            dir.join("package.json"),
            "{\n  \"name\": \"app\",\n  \"version\": \"9.5.0\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("package-lock.json"),
            "{\"name\":\"app\",\"version\":\"9.5.0\",\"packages\":{\"\":{\"version\":\"9.5.0\"}}}",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("backend/mobile")).unwrap();
        std::fs::write(
            dir.join("backend/tauri.conf.json"),
            "{\"version\": \"../package.json\"}",
        )
        .unwrap();
        std::fs::write(
            dir.join("backend/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"9.4.0\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("backend/Cargo.lock"),
            "[[package]]\nname = \"app\"\nversion = \"9.4.0\"\n",
        )
        .unwrap();
        // A mobile app inside has its own version and is not looked at.
        std::fs::write(
            dir.join("backend/mobile/Cargo.toml"),
            "[package]\nname = \"m\"\nversion = \"9.3.0\"\n",
        )
        .unwrap();
        let versions = detect(&dir, &[]);
        let paths: Vec<&str> = versions.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["package.json", "package-lock.json", "backend/Cargo.toml"]
        );
        assert_eq!(versions.current.as_deref(), Some("9.5.0"));
        assert_eq!(versions.mismatched, ["backend/Cargo.toml"]);

        let edits = plan(&dir, &versions, "9.6.0").unwrap();
        let rels: Vec<&str> = edits.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "package.json",
                "package-lock.json",
                "backend/Cargo.lock",
                "backend/Cargo.toml"
            ]
        );
        apply(&edits).unwrap();
        let after = detect(&dir, &[]);
        assert!(after.files.iter().all(|f| f.version == "9.6.0"));
        assert!(after.mismatched.is_empty());
        assert!(
            std::fs::read_to_string(dir.join("backend/Cargo.lock"))
                .unwrap()
                .contains("9.6.0")
        );
        undo(&edits);
        assert_eq!(detect(&dir, &[]), versions);

        // Skipped files neither count nor change.
        let skipped = detect(&dir, &["backend/Cargo.toml".into()]);
        assert!(skipped.mismatched.is_empty());
        assert_eq!(plan(&dir, &skipped, "9.6.0").unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn extensions_csproj_and_bumps() {
        let dir = temp("ext");
        std::fs::write(
            dir.join("manifest.json"),
            "{\"manifest_version\": 3, \"version\": \"1.4\"}",
        )
        .unwrap();
        std::fs::write(
            dir.join("Tool.csproj"),
            "<Project><PropertyGroup><Version>1.4</Version></PropertyGroup></Project>",
        )
        .unwrap();
        let versions = detect(&dir, &[]);
        assert_eq!(versions.files.len(), 2);
        assert_eq!(versions.current.as_deref(), Some("1.4"));
        // A web app manifest (no manifest_version) is not a version file.
        std::fs::write(
            dir.join("manifest.json"),
            "{\"name\": \"x\", \"version\": \"1.0.0\"}",
        )
        .unwrap();
        assert_eq!(detect(&dir, &[]).files.len(), 1);
        let _ = std::fs::remove_dir_all(dir);

        assert_eq!(bump("1.2.3", Bump::Patch).unwrap(), "1.2.4");
        assert_eq!(bump("1.2.3", Bump::Minor).unwrap(), "1.3.0");
        assert_eq!(bump("1.2.3", Bump::Major).unwrap(), "2.0.0");
        assert_eq!(bump("1.3.0-beta.2", Bump::Patch).unwrap(), "1.3.0");
        assert_eq!(bump("1.4", Bump::Patch).unwrap(), "1.4.1");
        assert!(bump("abc", Bump::Patch).is_err());
        assert_eq!(check_new_version("v2.0.0-rc.1").unwrap(), "2.0.0-rc.1");
        assert!(check_new_version("2.0").is_err());
    }
}
