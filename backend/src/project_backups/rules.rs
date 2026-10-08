//! What a project backup leaves out.
//!
//! Patterns, one per line, in the spirit of `.gitignore`:
//! - `node_modules/` – a folder with that name, anywhere
//! - `*.log`, `Thumbs.db` – a file (or folder) with that name, anywhere
//! - `docs/drafts/`, `/notes.txt`, `assets/**/*.psd` – a path from the
//!   project's folder
//!
//! The default list skips version control, tool and editor folders,
//! dependencies, build output and caches. A folder named `build` is skipped
//! only when it is build output: ignored by a `.gitignore`, or holding no file
//! git tracks. Following `.gitignore` for everything else is optional, and
//! never drops `.env` files.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex_lite::Regex;

use super::gitindex;

/// Written by every backup; never compared or treated as project content.
pub(crate) const BACKUP_INFO_FILE: &str = ".backup-info.json";
/// Half-written files of a backup in progress (also backup_projects').
pub(crate) const PARTIAL_PREFIX: &str = "__partial__";

pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    // version control, AI agents, editors
    ".git/",
    ".agents/",
    ".claude/",
    ".codex/",
    ".idea/",
    ".vs/",
    // dependencies and environments
    "node_modules/",
    "venv/",
    ".venv/",
    "__pycache__/",
    // build output
    "dist/",
    "dist-ssr/",
    "release/",
    "out/",
    "target/",
    "gen/",
    "coverage/",
    // caches
    ".cache/",
    ".next/",
    ".nuxt/",
    ".svelte-kit/",
    ".vite/",
    ".turbo/",
    ".parcel-cache/",
    ".wrangler/",
    ".pytest_cache/",
    ".mypy_cache/",
    // logs, temporary files, archives, debug symbols
    "*.log",
    "*.tmp",
    "*.temp",
    "*.bak",
    "*.swp",
    "*.swo",
    "*.zip",
    "*.rar",
    "*.7z",
    "*.pdb",
    "*.obj",
    "*.ilk",
    "*.idb",
    "*.tsbuildinfo",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
];

/// One compiled pattern.
#[derive(Clone, Debug)]
pub(crate) struct Pattern {
    pub source: String,
    regex: Regex,
    /// Matched against the whole relative path (it has a `/` before its end).
    anchored: bool,
    /// Only folders (it ends with `/`).
    dir_only: bool,
}

impl Pattern {
    pub(crate) fn parse(raw: &str) -> Result<Pattern, String> {
        let source = raw.trim().to_string();
        let mut text = source.replace('\\', "/");
        if text.is_empty() || text == "/" {
            return Err("An empty pattern matches nothing.".into());
        }
        if text.split('/').any(|part| part == "..") {
            return Err(format!("\"{source}\": \"..\" can't be used in a pattern."));
        }
        let dir_only = text.ends_with('/');
        while text.ends_with('/') {
            text.pop();
        }
        let anchored = text.contains('/');
        let text = text.trim_start_matches('/');
        if text.is_empty() {
            return Err(format!("\"{source}\" matches nothing."));
        }
        let regex = Regex::new(&format!("(?i)^{}$", glob_to_regex(text)))
            .map_err(|error| format!("\"{source}\" is not a valid pattern: {error}"))?;
        Ok(Pattern {
            source,
            regex,
            anchored,
            dir_only,
        })
    }

    /// `rel` uses `/`; `name` is its last part.
    pub(crate) fn matches(&self, rel: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        if self.anchored {
            self.regex.is_match(rel)
        } else {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            self.regex.is_match(name)
        }
    }
}

fn glob_to_regex(glob: &str) -> String {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                let slash_after = chars.get(i + 2) == Some(&'/');
                let at_segment_start = i == 0 || chars[i - 1] == '/';
                if at_segment_start && slash_after {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == ']') {
                    let body: String = chars[i + 1..i + 1 + end].iter().collect();
                    let body = body
                        .strip_prefix('!')
                        .map(|rest| format!("^{rest}"))
                        .unwrap_or(body);
                    let body = body.replace('\\', "\\\\");
                    out.push('[');
                    out.push_str(&body);
                    out.push(']');
                    i += end + 2;
                    continue;
                }
                out.push_str("\\[");
            }
            c => out.push_str(&regex_lite::escape(&c.to_string())),
        }
        i += 1;
    }
    out
}

/// The exclusion settings a backup runs with.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuleInput {
    /// The global list (the defaults unless the user changed it).
    pub patterns: Vec<String>,
    /// A project's own additions.
    pub extra: Vec<String>,
    /// A project's patterns to back up even though a global one matches.
    pub keep: Vec<String>,
    pub smart_build: bool,
    pub follow_gitignore: bool,
}

/// The rules for one project folder: patterns, `.gitignore` files and the
/// `build` folders that are build output, decided once per backup so the
/// backup, its completeness check and a comparison agree.
#[derive(Debug, Default)]
pub(crate) struct Rules {
    exclude: Vec<Pattern>,
    keep: Vec<Pattern>,
    smart_build: bool,
    follow_gitignore: bool,
    gitignore: Vec<GitignoreFile>,
    /// Lower-case relative paths of tracked files, when the folder is a git work tree.
    tracked: Option<Vec<String>>,
    /// Decisions about `build` folders, by lower-case relative path.
    build_cache: std::sync::Mutex<HashMap<String, bool>>,
}

/// Why something was left out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Exclusion {
    Pattern(String),
    BuildOutput,
    Gitignore,
    Internal,
}

impl Exclusion {
    pub(crate) fn describe(&self) -> String {
        match self {
            Exclusion::Pattern(source) => source.clone(),
            Exclusion::BuildOutput => "build output".into(),
            Exclusion::Gitignore => ".gitignore".into(),
            Exclusion::Internal => "backup file".into(),
        }
    }
}

impl Rules {
    /// No exclusions at all (reading an old folder backup as it is).
    pub(crate) fn none() -> Rules {
        Rules::default()
    }

    pub(crate) fn compile(input: &RuleInput, root: Option<&Path>) -> Result<Rules, String> {
        let exclude = input
            .patterns
            .iter()
            .chain(&input.extra)
            .filter(|raw| !raw.trim().is_empty())
            .map(|raw| Pattern::parse(raw))
            .collect::<Result<Vec<_>, _>>()?;
        let keep = input
            .keep
            .iter()
            .filter(|raw| !raw.trim().is_empty())
            .map(|raw| Pattern::parse(raw))
            .collect::<Result<Vec<_>, _>>()?;
        let mut rules = Rules {
            exclude,
            keep,
            smart_build: input.smart_build,
            follow_gitignore: input.follow_gitignore,
            ..Rules::default()
        };
        if let Some(root) = root.filter(|root| root.is_dir()) {
            if rules.smart_build || rules.follow_gitignore {
                rules.gitignore = load_gitignores(root, &rules);
            }
            if rules.smart_build {
                rules.tracked = gitindex::tracked_paths(root)
                    .map(|paths| paths.into_iter().map(|path| path.to_lowercase()).collect());
            }
        }
        Ok(rules)
    }

    /// Whether the folder at `rel` (relative, `/`) is left out, and why.
    pub(crate) fn excluded_dir(&self, rel: &str) -> Option<Exclusion> {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        if name.to_lowercase().starts_with(PARTIAL_PREFIX) {
            return Some(Exclusion::Internal);
        }
        if self.keep.iter().any(|pattern| pattern.matches(rel, true)) {
            return None;
        }
        if let Some(pattern) = self
            .exclude
            .iter()
            .find(|pattern| pattern.matches(rel, true))
        {
            return Some(Exclusion::Pattern(pattern.source.clone()));
        }
        if self.smart_build && name.eq_ignore_ascii_case("build") && self.is_build_output(rel) {
            return Some(Exclusion::BuildOutput);
        }
        if self.follow_gitignore && self.gitignored(rel, true) {
            return Some(Exclusion::Gitignore);
        }
        None
    }

    /// Whether the file at `rel` is left out, and why.
    pub(crate) fn excluded_file(&self, rel: &str) -> Option<Exclusion> {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let lower = name.to_lowercase();
        if lower.starts_with(PARTIAL_PREFIX) || (lower == BACKUP_INFO_FILE && !rel.contains('/')) {
            return Some(Exclusion::Internal);
        }
        if self.keep.iter().any(|pattern| pattern.matches(rel, false)) {
            return None;
        }
        if let Some(pattern) = self
            .exclude
            .iter()
            .find(|pattern| pattern.matches(rel, false))
        {
            return Some(Exclusion::Pattern(pattern.source.clone()));
        }
        if self.follow_gitignore && !is_env_file(&lower) && self.gitignored(rel, false) {
            return Some(Exclusion::Gitignore);
        }
        None
    }

    /// A file path read back from a backup: left out when any folder on its
    /// way, or the file itself, is.
    pub(crate) fn excluded_path(&self, rel: &str) -> bool {
        let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
        for end in 1..parts.len() {
            if self.excluded_dir(&parts[..end].join("/")).is_some() {
                return true;
            }
        }
        self.excluded_file(&parts.join("/")).is_some()
    }

    /// A `build` folder is build output when a `.gitignore` ignores it, or the
    /// project is a git work tree and git tracks nothing inside it. Without
    /// git and without a matching `.gitignore` it is kept: it may hold sources
    /// (an installer script, icons).
    pub(crate) fn is_build_output(&self, rel: &str) -> bool {
        let key = rel.to_lowercase();
        if let Some(&known) = self
            .build_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return known;
        }
        let tracked_inside = self.tracked.as_ref().map(|tracked| {
            let prefix = format!("{key}/");
            tracked.iter().any(|path| path.starts_with(&prefix))
        });
        let output = match tracked_inside {
            Some(true) => false,
            Some(false) => true,
            None => self.gitignored(rel, true),
        };
        self.build_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, output);
        output
    }

    /// `.gitignore` files apply to their own folder and below; later lines win
    /// and `!` lines take a path back.
    fn gitignored(&self, rel: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for file in &self.gitignore {
            let Some(inner) = strip_base(rel, &file.base) else {
                continue;
            };
            if inner.is_empty() {
                continue;
            }
            for (pattern, negated) in &file.patterns {
                if pattern.matches(inner, is_dir) {
                    ignored = !negated;
                }
            }
        }
        ignored
    }
}

/// `.env`, `.env.local`, `.env.production`, …: always backed up.
fn is_env_file(lower_name: &str) -> bool {
    lower_name == ".env" || lower_name.starts_with(".env.")
}

fn strip_base<'a>(rel: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(rel);
    }
    let lower = rel.to_lowercase();
    let base_lower = base.to_lowercase();
    if lower.len() > base_lower.len()
        && lower.starts_with(&base_lower)
        && rel.as_bytes()[base.len()] == b'/'
    {
        Some(&rel[base.len() + 1..])
    } else {
        None
    }
}

#[derive(Debug)]
struct GitignoreFile {
    /// The folder it is in, relative to the project (`""` for the root).
    base: String,
    patterns: Vec<(Pattern, bool)>,
}

/// Every `.gitignore` in the project, outside the folders the patterns
/// already leave out.
fn load_gitignores(root: &Path, rules: &Rules) -> Vec<GitignoreFile> {
    let mut found = Vec::new();
    let mut pending = vec![String::new()];
    let mut visited = 0usize;
    while let Some(rel) = pending.pop() {
        visited += 1;
        if visited > 20_000 {
            break;
        }
        let dir = if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&rel)
        };
        if let Ok(text) = fs::read_to_string(dir.join(".gitignore")) {
            let patterns = parse_gitignore(&text);
            if !patterns.is_empty() {
                found.push(GitignoreFile {
                    base: rel.clone(),
                    patterns,
                });
            }
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .map(|name| {
                if rel.is_empty() {
                    name
                } else {
                    format!("{rel}/{name}")
                }
            })
            .filter(|child| {
                let name = child.rsplit('/').next().unwrap_or(child);
                !rules
                    .exclude
                    .iter()
                    .any(|pattern| pattern.matches(child, true))
                    && !name.eq_ignore_ascii_case(".git")
            })
            .collect();
        children.sort();
        pending.extend(children.into_iter().rev());
    }
    // Parents before children, so a deeper file's lines win.
    found.sort_by_key(|file| file.base.matches('/').count() + usize::from(!file.base.is_empty()));
    found
}

fn parse_gitignore(text: &str) -> Vec<(Pattern, bool)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches(['\r', ' ']);
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (negated, body) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line.strip_prefix('\\').unwrap_or(line)),
            };
            Pattern::parse(body).ok().map(|pattern| (pattern, negated))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn defaults() -> RuleInput {
        RuleInput {
            patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
            smart_build: true,
            ..RuleInput::default()
        }
    }

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-rules-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn patterns_match_names_anywhere_and_paths_from_the_root() {
        let folder = Pattern::parse("node_modules/").unwrap();
        assert!(folder.matches("node_modules", true));
        assert!(folder.matches("app/NODE_MODULES", true));
        assert!(!folder.matches("node_modules", false));

        let ext = Pattern::parse("*.log").unwrap();
        assert!(ext.matches("logs/debug.LOG", false));
        assert!(!ext.matches("logs/debug.log.txt", false));

        let anchored = Pattern::parse("backend/resources/ludusavi/*").unwrap();
        assert!(anchored.matches("backend/resources/ludusavi/ludusavi.exe", false));
        assert!(!anchored.matches("other/backend/resources/ludusavi/x", false));

        let deep = Pattern::parse("assets/**/*.psd").unwrap();
        assert!(deep.matches("assets/a.psd", false));
        assert!(deep.matches("assets/x/y/a.psd", false));
        assert!(!deep.matches("art/a.psd", false));

        let rooted = Pattern::parse("/notes.txt").unwrap();
        assert!(rooted.matches("notes.txt", false));
        assert!(!rooted.matches("docs/notes.txt", false));

        assert!(Pattern::parse("  ").is_err());
        assert!(Pattern::parse("../outside").is_err());
        assert!(Pattern::parse("[ab].txt").unwrap().matches("a.txt", false));
    }

    #[test]
    fn the_defaults_skip_agent_and_tool_folders_but_keep_env_files() {
        let rules = Rules::compile(&defaults(), None).unwrap();
        for dir in [
            ".git",
            ".agents",
            ".claude",
            "node_modules",
            "dist",
            "release",
            "target",
            "src/__pycache__",
            ".next",
        ] {
            assert!(rules.excluded_dir(dir).is_some(), "{dir}");
        }
        for file in ["debug.log", "a/b/Thumbs.db", "pack.zip", "app.pdb"] {
            assert!(rules.excluded_file(file).is_some(), "{file}");
        }
        for file in [
            ".env",
            ".env.local",
            "src/main.rs",
            ".agentsrc",
            "agents/readme.md",
        ] {
            assert_eq!(rules.excluded_file(file), None, "{file}");
        }
        assert_eq!(rules.excluded_dir("src"), None);
        assert_eq!(rules.excluded_dir("agents"), None);
        assert!(rules.excluded_path(".agents/skills/x.md"));
        assert!(rules.excluded_path("web/node_modules/pkg/index.js"));
        assert!(!rules.excluded_path("web/src/index.js"));
    }

    #[test]
    fn keep_patterns_win_over_the_global_list() {
        let input = RuleInput {
            keep: vec!["dist/".into(), "*.zip".into()],
            extra: vec!["secrets/".into()],
            ..defaults()
        };
        let rules = Rules::compile(&input, None).unwrap();
        assert_eq!(rules.excluded_dir("dist"), None);
        assert_eq!(rules.excluded_file("assets/pack.zip"), None);
        assert!(rules.excluded_dir("secrets").is_some());
        assert!(
            Rules::compile(
                &RuleInput {
                    extra: vec!["a/../b".into()],
                    ..defaults()
                },
                None
            )
            .is_err()
        );
    }

    #[test]
    fn build_is_skipped_only_when_it_is_build_output() {
        // A git work tree that tracks build/installer.nsh but not out/build.
        let root = temp("build");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(
            root.join(".git").join("index"),
            gitindex::tests::index_v2(&["build/installer.nsh", "src/main.rs"]),
        )
        .unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::create_dir_all(root.join("web").join("build")).unwrap();
        let rules = Rules::compile(&defaults(), Some(&root)).unwrap();
        assert_eq!(rules.excluded_dir("build"), None, "tracked sources stay");
        assert_eq!(
            rules.excluded_dir("web/build"),
            Some(Exclusion::BuildOutput)
        );

        // No git: only a .gitignore says it is output.
        let plain = temp("build-plain");
        fs::create_dir_all(plain.join("build")).unwrap();
        fs::create_dir_all(plain.join("app").join("build")).unwrap();
        fs::write(plain.join("app").join(".gitignore"), "build/\n").unwrap();
        let rules = Rules::compile(&defaults(), Some(&plain)).unwrap();
        assert_eq!(rules.excluded_dir("build"), None);
        assert_eq!(
            rules.excluded_dir("app/build"),
            Some(Exclusion::BuildOutput)
        );

        // Turned off, every build folder is backed up.
        let off = Rules::compile(
            &RuleInput {
                smart_build: false,
                ..defaults()
            },
            Some(&plain),
        )
        .unwrap();
        assert_eq!(off.excluded_dir("app/build"), None);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(plain);
    }

    #[test]
    fn following_gitignore_is_optional_and_never_drops_env_files() {
        let root = temp("gitignore");
        fs::write(
            root.join(".gitignore"),
            "*.secret\n.env\n.env.*\n/generated/\nkeep/*\n!keep/this.txt\n",
        )
        .unwrap();
        let off = Rules::compile(&defaults(), Some(&root)).unwrap();
        assert_eq!(off.excluded_file("a.secret"), None);

        let on = Rules::compile(
            &RuleInput {
                follow_gitignore: true,
                ..defaults()
            },
            Some(&root),
        )
        .unwrap();
        assert_eq!(on.excluded_file("a.secret"), Some(Exclusion::Gitignore));
        assert_eq!(on.excluded_dir("generated"), Some(Exclusion::Gitignore));
        assert_eq!(on.excluded_dir("src/generated"), None);
        assert_eq!(on.excluded_file(".env"), None);
        assert_eq!(on.excluded_file("config/.env.production"), None);
        assert_eq!(
            on.excluded_file("keep/other.txt"),
            Some(Exclusion::Gitignore)
        );
        assert_eq!(on.excluded_file("keep/this.txt"), None);
        let _ = fs::remove_dir_all(root);
    }
}
