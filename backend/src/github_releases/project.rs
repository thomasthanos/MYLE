//! What a repository holds: its projects (one, or one per app folder of a
//! monorepo), what each is built with, its release tags, and the GitHub
//! Actions workflows that release on a tag.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::git::Tag;
use super::versions;

/// Folders never searched for repositories or projects.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "release",
    "vendor",
    ".venv",
    "venv",
    "bin",
    "obj",
    "gen",
    "coverage",
    "__pycache__",
    "$recycle.bin",
    "system volume information",
    "windows",
    "program files",
    "program files (x86)",
    "appdata",
];
/// How deep "Scan a folder" looks for repositories.
const SCAN_DEPTH: usize = 4;
/// At most this many repositories from one scan.
const SCAN_MAX: usize = 400;

fn skipped(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with('.') || SKIP_DIRS.contains(&lower.as_str())
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|entry| !skipped(&entry.file_name().to_string_lossy()))
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort_by_key(|path| path.display().to_string().to_lowercase());
    dirs
}

pub(crate) fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Git repositories in `parent` (itself included), not looking inside one.
pub(crate) fn find_repos(parent: &Path, cancelled: &dyn Fn() -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![(parent.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = queue.pop() {
        if cancelled() || found.len() >= SCAN_MAX {
            break;
        }
        if is_repo(&dir) {
            found.push(dir);
            continue;
        }
        if depth < SCAN_DEPTH {
            for child in subdirs(&dir).into_iter().rev() {
                queue.push((child, depth + 1));
            }
        }
    }
    found.sort_by_key(|path| path.display().to_string().to_lowercase());
    found
}

/// A folder holds a project of its own: a manifest of some language.
fn has_manifest(dir: &Path) -> bool {
    const FILES: &[&str] = &[
        "package.json",
        "Cargo.toml",
        "pyproject.toml",
        "setup.py",
        "go.mod",
        "pubspec.yaml",
        "tauri.conf.json",
    ];
    FILES.iter().any(|name| dir.join(name).is_file())
        || has_extension(dir, &["csproj", "sln"])
        || std::fs::read_to_string(dir.join("manifest.json"))
            .is_ok_and(|text| text.contains("\"manifest_version\""))
}

fn has_extension(dir: &Path, extensions: &[&str]) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().extension().is_some_and(|ext| {
                extensions
                    .iter()
                    .any(|wanted| ext.eq_ignore_ascii_case(wanted))
            })
        })
    })
}

/// The app folders of a monorepo (from the repository's top folder, with
/// `/`); empty when the repository is one project. A repository is one
/// project when its top folder states a version (package.json without
/// workspaces, Cargo.toml, …).
pub(crate) fn sub_projects(root: &Path) -> Vec<String> {
    let workspaces = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|json| json.get("workspaces").is_some());
    let root_is_project = versions::detect(root, &[]).current.is_some() && !workspaces;
    if root_is_project {
        return Vec::new();
    }
    let mut found = Vec::new();
    for dir in subdirs(root) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if has_manifest(&dir) {
            found.push(name);
        } else if ["packages", "apps", "projects", "tools"].contains(&name.to_lowercase().as_str())
        {
            for inner in subdirs(&dir) {
                if has_manifest(&inner) {
                    let inner_name = inner
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    found.push(format!("{name}/{inner_name}"));
                }
            }
        }
    }
    found
}

// ─── Build types ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum BuildKind {
    Tauri,
    Electron,
    Vite,
    Node,
    Rust,
    Dotnet,
    Python,
    Go,
    Flutter,
    Extension,
}

pub(crate) fn package_json(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Every dependency name of a package.json.
pub(crate) fn dependencies(json: &serde_json::Value) -> Vec<String> {
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .filter_map(|key| json.get(key)?.as_object())
        .flat_map(|deps| deps.keys().cloned())
        .collect()
}

/// The folders where a Tauri app keeps its config: the project folder and
/// the folders right inside it.
pub(crate) fn tauri_dir(dir: &Path) -> Option<PathBuf> {
    if dir.join("tauri.conf.json").is_file() {
        return Some(dir.to_path_buf());
    }
    subdirs(dir)
        .into_iter()
        .find(|sub| sub.join("tauri.conf.json").is_file())
}

pub(crate) fn build_kinds(dir: &Path) -> Vec<BuildKind> {
    let mut kinds = Vec::new();
    if let Some(json) = package_json(dir) {
        let deps = dependencies(&json);
        let has = |name: &str| deps.iter().any(|dep| dep == name);
        if has("electron") || has("electron-builder") || has("@electron-forge/cli") {
            kinds.push(BuildKind::Electron);
        }
        if has("vite") {
            kinds.push(BuildKind::Vite);
        }
        kinds.push(BuildKind::Node);
    }
    if tauri_dir(dir).is_some() {
        kinds.push(BuildKind::Tauri);
    }
    if dir.join("Cargo.toml").is_file()
        || tauri_dir(dir).is_some_and(|tauri| tauri.join("Cargo.toml").is_file())
    {
        kinds.push(BuildKind::Rust);
    }
    if has_extension(dir, &["csproj", "sln", "fsproj", "vbproj"]) {
        kinds.push(BuildKind::Dotnet);
    }
    if ["pyproject.toml", "setup.py", "requirements.txt"]
        .iter()
        .any(|name| dir.join(name).is_file())
        || has_extension(dir, &["spec"])
    {
        kinds.push(BuildKind::Python);
    }
    if dir.join("go.mod").is_file() {
        kinds.push(BuildKind::Go);
    }
    if dir.join("pubspec.yaml").is_file() {
        kinds.push(BuildKind::Flutter);
    }
    if std::fs::read_to_string(dir.join("manifest.json"))
        .is_ok_and(|text| text.contains("\"manifest_version\""))
    {
        kinds.push(BuildKind::Extension);
    }
    kinds.sort();
    kinds.dedup();
    kinds
}

// ─── Tags ──────────────────────────────────────────────────────────────────

/// The tag of `version`: `v1.2.3`, or `backup_projects-v1.2.3` for an app
/// of a monorepo.
pub(crate) fn tag_name(prefix: &str, version: &str) -> String {
    format!("{prefix}{version}")
}

/// `v`, or `<folder>-v` for an app folder of a monorepo (its last part:
/// `apps/web` → `web-v`).
pub(crate) fn tag_prefix(sub: Option<&str>, monorepo: bool) -> String {
    match sub {
        Some(sub) if monorepo => {
            let name = sub.rsplit('/').next().unwrap_or(sub);
            format!("{name}-v")
        }
        _ => "v".into(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastTag {
    pub name: String,
    pub date: i64,
    /// A shared `v…` tag from before the app had a prefix of its own.
    pub legacy: bool,
}

/// The newest release tag of a project. A monorepo app without tags of its
/// own yet falls back to the newest shared `v…` tag of its major version
/// that is not ahead of it (desktop-utils tagged its apps `v2.0.6`, `v3.7.3`).
pub(crate) fn last_tag(tags: &[Tag], prefix: &str, current: Option<&str>) -> Option<LastTag> {
    let of_prefix = |tag: &&Tag| {
        tag.name
            .strip_prefix(prefix)
            .is_some_and(|rest| versions::parse(rest).is_ok())
    };
    if let Some(tag) = tags.iter().find(of_prefix) {
        return Some(LastTag {
            name: tag.name.clone(),
            date: tag.date,
            legacy: false,
        });
    }
    if prefix == "v" {
        return None;
    }
    let current = versions::parse(current?).ok()?;
    tags.iter()
        .filter_map(|tag| {
            let version = versions::parse(tag.name.strip_prefix('v')?).ok()?;
            (version.major == current.major && version <= current).then_some((version, tag))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag)| LastTag {
            name: tag.name.clone(),
            date: tag.date,
            legacy: true,
        })
}

// ─── Release workflows ─────────────────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// `release.yml`.
    pub file: String,
    pub name: Option<String>,
    /// The tag patterns that start it (`v*`).
    pub tags: Vec<String>,
}

/// The tag patterns under `on: push: tags:` of a workflow, read loosely
/// (block and inline lists, quoted or not).
pub(crate) fn workflow_tags(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut patterns = Vec::new();
    let mut in_push = None::<usize>;
    let indent = |line: &str| line.len() - line.trim_start().len();
    let clean = |value: &str| {
        value
            .split(" #")
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(['"', '\''])
            .to_string()
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            i += 1;
            continue;
        }
        if let Some(level) = in_push
            && indent(line) <= level
        {
            in_push = None;
        }
        if trimmed == "push:" || trimmed.starts_with("push:") && trimmed.ends_with(':') {
            in_push = Some(indent(line));
        } else if in_push.is_some()
            && let Some(rest) = trimmed.strip_prefix("tags:")
        {
            let rest = rest.trim();
            if let Some(list) = rest.strip_prefix('[') {
                patterns.extend(
                    list.trim_end_matches(']')
                        .split(',')
                        .map(clean)
                        .filter(|p| !p.is_empty()),
                );
            } else if !rest.is_empty() {
                patterns.push(clean(rest));
            } else {
                let level = indent(line);
                while i + 1 < lines.len() {
                    let next = lines[i + 1];
                    let next_trim = next.trim();
                    if next_trim.is_empty() || next_trim.starts_with('#') {
                        i += 1;
                        continue;
                    }
                    if indent(next) <= level && !next_trim.starts_with('-') {
                        break;
                    }
                    match next_trim.strip_prefix('-') {
                        Some(item) => patterns.push(clean(item)),
                        None => break,
                    }
                    i += 1;
                }
            }
        }
        i += 1;
    }
    patterns.retain(|p| !p.is_empty() && !p.starts_with('!'));
    patterns
}

fn workflow_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("name:"))
        .map(|name| name.trim().trim_matches(['"', '\'']).to_string())
        .filter(|name| !name.is_empty())
}

/// The workflows of the repository at `root` that run on a tag push.
pub(crate) fn release_workflows(root: &Path) -> Vec<Workflow> {
    let dir = root.join(".github").join("workflows");
    let mut found: Vec<Workflow> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|ext| ext == "yml" || ext == "yaml")
                })
                .filter_map(|path| {
                    let text = std::fs::read_to_string(&path).ok()?;
                    let tags = workflow_tags(&text);
                    let file = path.file_name()?.to_string_lossy().to_string();
                    (!tags.is_empty()).then(|| Workflow {
                        file,
                        name: workflow_name(&text),
                        tags,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by(|a, b| a.file.cmp(&b.file));
    found
}

/// GitHub's tag filter patterns: `*` (no `/`), `**`, `?`, `[…]` ranges are
/// treated loosely as one character.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => {
                let double = p.get(1) == Some(&'*');
                let rest = if double { &p[2..] } else { &p[1..] };
                (0..=t.len())
                    .any(|skip| (double || !t[..skip].contains(&'/')) && go(rest, &t[skip..]))
            }
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some('[') => match p.iter().position(|c| *c == ']') {
                Some(end) => !t.is_empty() && go(&p[end + 1..], &t[1..]),
                None => t.first() == Some(&'[') && go(&p[1..], &t[1..]),
            },
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

/// The workflow a push of `tag` starts, if any.
pub(crate) fn workflow_for_tag<'a>(workflows: &'a [Workflow], tag: &str) -> Option<&'a Workflow> {
    workflows
        .iter()
        .find(|workflow| workflow.tags.iter().any(|pattern| glob_match(pattern, tag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("myle-gr-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_monorepo_like_desktop_utils_and_a_single_project() {
        let dir = temp("mono");
        for (app, version) in [
            ("Github-Build-Release", "3.7.4"),
            ("backup_projects", "2.0.6"),
            ("discord_bot", "1.0.1"),
        ] {
            std::fs::create_dir_all(dir.join(app)).unwrap();
            std::fs::write(
                dir.join(app).join("package.json"),
                format!("{{\"version\":\"{version}\"}}"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("node_modules/x/package.json"), "{}").unwrap();
        std::fs::write(dir.join("README.md"), "#").unwrap();
        assert_eq!(
            sub_projects(&dir),
            ["backup_projects", "discord_bot", "Github-Build-Release"]
        );

        // npm workspaces under packages/.
        let ws = temp("ws");
        std::fs::write(
            ws.join("package.json"),
            "{\"private\":true,\"workspaces\":[\"packages/*\"],\"version\":\"1.0.0\"}",
        )
        .unwrap();
        std::fs::create_dir_all(ws.join("packages/web")).unwrap();
        std::fs::write(
            ws.join("packages/web/package.json"),
            "{\"version\":\"0.1.0\"}",
        )
        .unwrap();
        assert_eq!(sub_projects(&ws), ["packages/web"]);

        // MYLE: the top folder has the version, so it is one project.
        let single = temp("single");
        std::fs::write(single.join("package.json"), "{\"version\":\"9.5.0\"}").unwrap();
        std::fs::create_dir_all(single.join("extension")).unwrap();
        std::fs::write(
            single.join("extension/manifest.json"),
            "{\"manifest_version\":3,\"version\":\"1.0\"}",
        )
        .unwrap();
        assert!(sub_projects(&single).is_empty());
        for dir in [dir, ws, single] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn scanning_finds_repositories_but_not_inside_them() {
        let dir = temp("scan");
        for repo in ["a", "group/b", "a/nested"] {
            std::fs::create_dir_all(dir.join(repo).join(".git")).unwrap();
        }
        std::fs::create_dir_all(dir.join("node_modules/c/.git")).unwrap();
        let found = find_repos(&dir, &|| false);
        let names: Vec<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(&dir)
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["a", "group/b"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn build_kinds_of_common_projects() {
        let dir = temp("kinds");
        std::fs::write(
            dir.join("package.json"),
            "{\"devDependencies\":{\"vite\":\"1\",\"electron-builder\":\"1\"}}",
        )
        .unwrap();
        assert_eq!(
            build_kinds(&dir),
            [BuildKind::Electron, BuildKind::Vite, BuildKind::Node]
        );
        std::fs::create_dir_all(dir.join("src-tauri")).unwrap();
        std::fs::write(dir.join("src-tauri/tauri.conf.json"), "{}").unwrap();
        std::fs::write(dir.join("src-tauri/Cargo.toml"), "[package]").unwrap();
        std::fs::write(dir.join("App.csproj"), "<Project/>").unwrap();
        let kinds = build_kinds(&dir);
        assert!(
            kinds.contains(&BuildKind::Tauri)
                && kinds.contains(&BuildKind::Rust)
                && kinds.contains(&BuildKind::Dotnet)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tags_per_app_with_the_shared_fallback() {
        let tags = vec![
            Tag {
                name: "v3.7.3".into(),
                date: 30,
            },
            Tag {
                name: "v2.0.6".into(),
                date: 20,
            },
            Tag {
                name: "v2.0.5".into(),
                date: 10,
            },
            Tag {
                name: "nightly".into(),
                date: 40,
            },
        ];
        assert_eq!(
            tag_prefix(Some("backup_projects"), true),
            "backup_projects-v"
        );
        assert_eq!(tag_prefix(Some("apps/web"), true), "web-v");
        assert_eq!(tag_prefix(Some("app"), false), "v");
        assert_eq!(
            tag_name("backup_projects-v", "2.0.7"),
            "backup_projects-v2.0.7"
        );
        let shared = last_tag(&tags, "backup_projects-v", Some("2.0.6")).unwrap();
        assert_eq!((shared.name.as_str(), shared.legacy), ("v2.0.6", true));
        assert_eq!(
            last_tag(&tags, "Github-Build-Release-v", Some("3.7.4"))
                .unwrap()
                .name,
            "v3.7.3"
        );
        assert_eq!(last_tag(&tags, "discord_bot-v", Some("1.0.1")), None);
        assert_eq!(last_tag(&tags, "v", Some("9.0.0")).unwrap().name, "v3.7.3");
        let mut own = tags.clone();
        own.insert(
            0,
            Tag {
                name: "backup_projects-v2.0.7".into(),
                date: 50,
            },
        );
        let own = last_tag(&own, "backup_projects-v", Some("2.0.7")).unwrap();
        assert_eq!(
            (own.name.as_str(), own.legacy),
            ("backup_projects-v2.0.7", false)
        );
    }

    #[test]
    fn release_workflows_and_tag_patterns() {
        let block = "name: Release\non:\n  push:\n    tags:\n      - 'v*'   # releases\n      - \"!v*-rc\"\n  workflow_dispatch:\njobs: {}\n";
        assert_eq!(workflow_tags(block), ["v*"]);
        let inline = "on:\n  push:\n    branches: [main]\n    tags: [ 'app-v*', \"web-v*\" ]\n";
        assert_eq!(workflow_tags(inline), ["app-v*", "web-v*"]);
        let ci = "on:\n  push:\n    branches: [main]\n  pull_request:\n";
        assert!(workflow_tags(ci).is_empty());
        assert!(glob_match("v*", "v9.6.0"));
        assert!(!glob_match("v*", "backup_projects-v2.0.7"));
        assert!(glob_match("backup_projects-v*", "backup_projects-v2.0.7"));
        assert!(glob_match("v[0-9]*", "v1.0.0"));
        assert!(!glob_match("v*", "v1/x"));
        let dir = temp("wf");
        std::fs::create_dir_all(dir.join(".github/workflows")).unwrap();
        std::fs::write(dir.join(".github/workflows/release.yml"), block).unwrap();
        std::fs::write(dir.join(".github/workflows/ci.yml"), ci).unwrap();
        let found = release_workflows(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("Release"));
        assert!(workflow_for_tag(&found, "v9.6.0").is_some());
        assert!(workflow_for_tag(&found, "x-v1").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
