//! What a project backup leaves out.
//!
//! Patterns, one per line, in the spirit of `.gitignore`:
//! - `node_modules/` – a folder with that name, anywhere
//! - `*.log`, `Thumbs.db` – a file (or folder) with that name, anywhere
//! - `docs/drafts/`, `/notes.txt`, `assets/**/*.psd` – a path from the
//!   project's folder
//!
//! The default list skips version control, tool and editor folders,
//! dependencies, build output and caches whose names are never sources.
//! Folders whose names can be either (`build`, `target`, `bin`, `obj`, `out`,
//! `Debug`, `packages`, …) are left out only when the files around them say
//! they are output (`detect`: a `Cargo.toml` next to `target`, a `.csproj`
//! next to `bin`), a `.gitignore` ignores them, or git tracks nothing inside.
//! A folder git tracks files in is always kept. Following `.gitignore` for
//! everything else is optional, and never drops `.env` files.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use regex_lite::Regex;

use super::{detect, gitindex};

/// Written by every backup; never compared or treated as project content.
pub(crate) const BACKUP_INFO_FILE: &str = ".backup-info.json";
/// Half-written files of a backup in progress (also backup_projects').
pub(crate) const PARTIAL_PREFIX: &str = "__partial__";

/// Left out wherever they are: names that are never project sources. Folder
/// names that can be either (`build`, `target`, `bin`, `out`, …) are decided
/// by `detect` from the files around them.
pub(crate) const DEFAULT_PATTERNS: &[&str] = &[
    // version control, AI agents, editors
    ".git/",
    ".hg/",
    ".svn/",
    ".agents/",
    ".claude/",
    ".codex/",
    ".idea/",
    ".vs/",
    ".history/",
    "ipch/",
    // JavaScript / TypeScript
    "node_modules/",
    "bower_components/",
    "jspm_packages/",
    ".pnpm-store/",
    "**/.yarn/cache/",
    "**/.yarn/unplugged/",
    "dist/",
    "dist-ssr/",
    "dist_electron/",
    ".next/",
    ".nuxt/",
    ".output/",
    ".svelte-kit/",
    ".vite/",
    ".turbo/",
    ".parcel-cache/",
    ".angular/",
    ".expo/",
    ".vercel/",
    ".netlify/",
    ".wrangler/",
    ".docusaurus/",
    "storybook-static/",
    ".cache/",
    ".eslintcache",
    ".stylelintcache",
    "*.tsbuildinfo",
    "coverage/",
    ".nyc_output/",
    // Python
    "__pycache__/",
    "*.pyc",
    "*.pyo",
    ".venv/",
    ".pytest_cache/",
    ".mypy_cache/",
    ".ruff_cache/",
    ".tox/",
    ".nox/",
    ".hypothesis/",
    ".ipynb_checkpoints/",
    "*.egg-info/",
    ".eggs/",
    "htmlcov/",
    ".coverage",
    // Java / Kotlin, Dart / Flutter, Swift / Xcode, Godot, others
    ".gradle/",
    ".kotlin/",
    ".dart_tool/",
    "DerivedData/",
    ".build/",
    "xcuserdata/",
    ".godot/",
    ".terraform/",
    ".zig-cache/",
    "zig-out/",
    ".stack-work/",
    "dist-newstyle/",
    "_build/",
    // C / C++
    "cmake-build-*/",
    "CMakeFiles/",
    "*.o",
    "*.ilk",
    "*.idb",
    "*.ipch",
    "*.pdb",
    "*.VC.db",
    "*.VC.opendb",
    // logs, temporary files, archives
    "*.log",
    "*.tmp",
    "*.temp",
    "*.bak",
    "*.swp",
    "*.swo",
    "*.zip",
    "*.rar",
    "*.7z",
    // system files
    ".DS_Store",
    "Thumbs.db",
    "ehthumbs.db",
    "desktop.ini",
    "$RECYCLE.BIN/",
];

/// The 9.4.0 defaults, to bring saved lists up to date (`store::migrate`).
pub(crate) const DEFAULT_PATTERNS_V1: &[&str] = &[
    ".git/",
    ".agents/",
    ".claude/",
    ".codex/",
    ".idea/",
    ".vs/",
    "node_modules/",
    "venv/",
    ".venv/",
    "__pycache__/",
    "dist/",
    "dist-ssr/",
    "release/",
    "out/",
    "target/",
    "gen/",
    "coverage/",
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
    /// Tell build output from sources by the files around a folder (`detect`).
    pub smart_build: bool,
    pub follow_gitignore: bool,
}

/// The lower-case names directly inside one folder.
#[derive(Clone, Debug, Default)]
pub(crate) struct DirNames {
    pub files: HashSet<String>,
    pub dirs: HashSet<String>,
}

impl DirNames {
    pub(crate) fn add(&mut self, name: &str, is_dir: bool) {
        let lower = name.to_lowercase();
        if is_dir {
            self.dirs.insert(lower);
        } else {
            self.files.insert(lower);
        }
    }

    pub(crate) fn has_file(&self, name: &str) -> bool {
        self.files.contains(name)
    }

    pub(crate) fn has_dir(&self, name: &str) -> bool {
        self.dirs.contains(name)
    }

    pub(crate) fn has_any_file(&self, names: &[&str]) -> bool {
        names.iter().any(|name| self.files.contains(*name))
    }

    /// A file ending with one of `suffixes` (`.csproj`).
    pub(crate) fn has_file_ending(&self, suffixes: &[&str]) -> bool {
        self.files
            .iter()
            .any(|name| suffixes.iter().any(|suffix| name.ends_with(suffix)))
    }

    /// A folder ending with one of `suffixes` (`.xcodeproj`).
    pub(crate) fn has_dir_ending(&self, suffixes: &[&str]) -> bool {
        self.dirs
            .iter()
            .any(|name| suffixes.iter().any(|suffix| name.ends_with(suffix)))
    }
}

/// What is in the folders around a path: read from the disk while walking,
/// or from the file list of a backup.
pub(crate) trait Listing {
    /// The names in the folder `rel` (`""`: the project folder), when known.
    fn names(&self, rel: &str) -> Option<Arc<DirNames>>;
}

/// Nothing known: only patterns decide.
#[cfg(test)]
pub(crate) struct NoListing;

#[cfg(test)]
impl Listing for NoListing {
    fn names(&self, _rel: &str) -> Option<Arc<DirNames>> {
        None
    }
}

/// The folders of a list of file paths (a backup's content).
#[derive(Default)]
pub(crate) struct TreeListing {
    dirs: HashMap<String, Arc<DirNames>>,
}

impl TreeListing {
    pub(crate) fn from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> TreeListing {
        let mut dirs: HashMap<String, DirNames> = HashMap::new();
        for path in paths {
            let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            for (index, part) in parts.iter().enumerate() {
                let parent = parts[..index].join("/").to_lowercase();
                dirs.entry(parent)
                    .or_default()
                    .add(part, index + 1 < parts.len());
            }
        }
        TreeListing {
            dirs: dirs
                .into_iter()
                .map(|(rel, names)| (rel, Arc::new(names)))
                .collect(),
        }
    }
}

impl Listing for TreeListing {
    fn names(&self, rel: &str) -> Option<Arc<DirNames>> {
        self.dirs.get(&rel.to_lowercase()).cloned()
    }
}

/// The rules for one project folder: patterns, `.gitignore` files and what
/// git tracks, decided once per backup so the backup, its completeness check,
/// the preview and a comparison agree.
#[derive(Debug, Default)]
pub(crate) struct Rules {
    exclude: Vec<Pattern>,
    keep: Vec<Pattern>,
    smart_build: bool,
    follow_gitignore: bool,
    gitignore: Vec<GitignoreFile>,
    /// Sorted lower-case relative paths of tracked files, when the folder is
    /// a git work tree.
    tracked: Option<Vec<String>>,
    /// The project folder, to read a marker file's content.
    root: Option<std::path::PathBuf>,
    /// Decisions about ambiguous folders and marker contents, by key.
    cache: std::sync::Mutex<HashMap<String, bool>>,
}

/// Why something was left out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Exclusion {
    /// A pattern of the list (its text).
    Pattern(String),
    /// Build output, dependencies or a cache told by the files around it.
    Detected(&'static str),
    Gitignore,
    Internal,
}

impl Exclusion {
    pub(crate) fn describe(&self) -> String {
        match self {
            Exclusion::Pattern(source) => source.clone(),
            Exclusion::Detected(rule) => (*rule).into(),
            Exclusion::Gitignore => "listed in .gitignore".into(),
            Exclusion::Internal => "backup file".into(),
        }
    }

    /// `pattern`, `detected`, `gitignore` or `internal`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Exclusion::Pattern(_) => "pattern",
            Exclusion::Detected(_) => "detected",
            Exclusion::Gitignore => "gitignore",
            Exclusion::Internal => "internal",
        }
    }
}

/// The first, cheap decision about a folder (before its content is read).
pub(crate) enum Early {
    /// A keep pattern: backed up whatever else says.
    Keep,
    Excluded(Exclusion),
    /// Decided by `detected_dir` once the folder's names are known.
    Undecided,
}

/// The parent folder of `rel` (`""` for a top-level name).
pub(crate) fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("")
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
            rules.root = Some(root.to_path_buf());
            if rules.smart_build {
                rules.tracked = gitindex::tracked_paths(root).map(|paths| {
                    let mut lower: Vec<String> =
                        paths.into_iter().map(|path| path.to_lowercase()).collect();
                    lower.sort();
                    lower
                });
            }
            if rules.smart_build || rules.follow_gitignore {
                rules.gitignore = load_gitignores(root, &rules);
            }
        }
        Ok(rules)
    }

    /// Internal files, keep patterns and the pattern list.
    pub(crate) fn early_dir(&self, rel: &str) -> Early {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        if name.to_lowercase().starts_with(PARTIAL_PREFIX) {
            return Early::Excluded(Exclusion::Internal);
        }
        if self.keep.iter().any(|pattern| pattern.matches(rel, true)) {
            return Early::Keep;
        }
        if let Some(pattern) = self
            .exclude
            .iter()
            .find(|pattern| pattern.matches(rel, true))
        {
            return Early::Excluded(Exclusion::Pattern(pattern.source.clone()));
        }
        Early::Undecided
    }

    /// What the files around the folder `rel` say, and `.gitignore`.
    pub(crate) fn detected_dir(&self, rel: &str, listing: &dyn Listing) -> Option<Exclusion> {
        // Markers name the tool; a followed .gitignore comes next; the
        // name-only guess (ambiguous names git leaves alone) comes last.
        let smart = self.smart_build && !self.tracks_inside(rel);
        let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
        if smart && let Some(rule) = detect::folder(rel, &name, listing, self) {
            return Some(Exclusion::Detected(rule));
        }
        if self.follow_gitignore && self.gitignored(rel, true) {
            return Some(Exclusion::Gitignore);
        }
        if smart && let Some(rule) = self.ambiguous_output(rel, &name) {
            return Some(Exclusion::Detected(rule));
        }
        None
    }

    /// Whether the folder at `rel` (relative, `/`) is left out, and why.
    pub(crate) fn excluded_dir(&self, rel: &str, listing: &dyn Listing) -> Option<Exclusion> {
        match self.early_dir(rel) {
            Early::Keep => None,
            Early::Excluded(why) => Some(why),
            Early::Undecided => self.detected_dir(rel, listing),
        }
    }

    /// Whether the file at `rel` is left out, and why.
    pub(crate) fn excluded_file(&self, rel: &str, listing: &dyn Listing) -> Option<Exclusion> {
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
        if self.smart_build
            && !self.is_tracked(rel)
            && let Some(rule) = detect::file(rel, &lower, listing)
        {
            return Some(Exclusion::Detected(rule));
        }
        if self.follow_gitignore && !is_env_file(&lower) && self.gitignored(rel, false) {
            return Some(Exclusion::Gitignore);
        }
        None
    }

    /// A file path read back from a backup: left out when any folder on its
    /// way, or the file itself, is.
    pub(crate) fn excluded_path(&self, rel: &str, listing: &dyn Listing) -> bool {
        let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
        for end in 1..parts.len() {
            if self
                .excluded_dir(&parts[..end].join("/"), listing)
                .is_some()
            {
                return true;
            }
        }
        self.excluded_file(&parts.join("/"), listing).is_some()
    }

    /// Git tracks a file inside the folder `rel`: it holds sources, whatever
    /// its name.
    fn tracks_inside(&self, rel: &str) -> bool {
        let Some(tracked) = &self.tracked else {
            return false;
        };
        let prefix = format!("{}/", rel.to_lowercase());
        let start = tracked.partition_point(|path| path.as_str() < prefix.as_str());
        tracked
            .get(start)
            .is_some_and(|path| path.starts_with(&prefix))
    }

    fn is_tracked(&self, rel: &str) -> bool {
        let Some(tracked) = &self.tracked else {
            return false;
        };
        tracked.binary_search(&rel.to_lowercase()).is_ok()
    }

    /// A folder with a name build tools use (`build`, `out`, `bin`, …) and no
    /// marker that says which tool: build output when a `.gitignore` ignores
    /// it, or, for the commonest names, when the project is a git work tree
    /// and git tracks nothing inside. Otherwise it is kept: it may hold
    /// sources (an installer script, icons).
    fn ambiguous_output(&self, rel: &str, name: &str) -> Option<&'static str> {
        if !detect::AMBIGUOUS.contains(&name) {
            return None;
        }
        let key = format!("ambiguous:{}", rel.to_lowercase());
        if let Some(&known) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return known.then_some(detect::IGNORED_OUTPUT);
        }
        let ignored = self.gitignored(rel, true);
        let untracked =
            !ignored && detect::UNTRACKED_MEANS_OUTPUT.contains(&name) && self.tracked.is_some();
        let output = ignored || untracked;
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, output);
        if ignored {
            Some(detect::IGNORED_OUTPUT)
        } else if untracked {
            Some(detect::UNTRACKED_OUTPUT)
        } else {
            None
        }
    }

    /// Whether the lower-case file `name` in the folder `dir` mentions
    /// `needle` (cached; small files only).
    pub(crate) fn file_mentions(&self, dir: &str, name: &str, needle: &str) -> bool {
        let Some(root) = &self.root else {
            return false;
        };
        let key = format!("mentions:{}/{name}:{needle}", dir.to_lowercase());
        if let Some(&known) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return known;
        }
        let mut path = root.clone();
        for part in dir.split('/').filter(|part| !part.is_empty()) {
            path.push(part);
        }
        path.push(name);
        let found = fs::metadata(&path).is_ok_and(|meta| meta.len() <= 1024 * 1024)
            && fs::read_to_string(&path).is_ok_and(|text| text.contains(needle));
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, found);
        found
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
pub(crate) fn is_env_file(lower_name: &str) -> bool {
    lower_name == ".env" || lower_name.starts_with(".env.")
}

/// `rel` below the folder `base` (both relative, `/`), compared the way
/// Windows compares names.
fn strip_base<'a>(rel: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(rel);
    }
    let mut rest = rel;
    for part in base.split('/') {
        let (head, tail) = rest.split_once('/')?;
        if head.to_lowercase() != part.to_lowercase() {
            return None;
        }
        rest = tail;
    }
    Some(rest)
}

#[derive(Debug)]
struct GitignoreFile {
    /// The folder it is in, relative to the project (`""` for the root).
    base: String,
    patterns: Vec<(Pattern, bool)>,
}

/// Every `.gitignore` in the project, outside the folders the patterns or
/// the markers already leave out.
fn load_gitignores(root: &Path, rules: &Rules) -> Vec<GitignoreFile> {
    let mut found = Vec::new();
    let listing = StackListing::default();
    let mut pending: Vec<String> = vec![String::new()];
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
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let entries: Vec<(String, bool)> = entries
            .flatten()
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.file_type().is_ok_and(|kind| kind.is_dir()),
                )
            })
            .collect();
        let mut names = DirNames::default();
        for (name, is_dir) in &entries {
            names.add(name, *is_dir);
        }
        listing.set(&rel, names);
        if !rel.is_empty()
            && rules.smart_build
            && !rules.tracks_inside(&rel)
            && detect::folder(
                &rel,
                &rel.rsplit('/').next().unwrap_or(&rel).to_lowercase(),
                &listing,
                rules,
            )
            .is_some()
        {
            continue;
        }
        if let Ok(text) = fs::read_to_string(dir.join(".gitignore")) {
            let patterns = parse_gitignore(&text);
            if !patterns.is_empty() {
                found.push(GitignoreFile {
                    base: rel.clone(),
                    patterns,
                });
            }
        }
        let mut children: Vec<String> = entries
            .into_iter()
            .filter(|(_, is_dir)| *is_dir)
            .map(|(name, _)| crate::project_backups::walk::join_rel(&rel, &name))
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

/// The folders read so far by a walk, by lower-case relative path.
#[derive(Default)]
pub(crate) struct StackListing {
    dirs: std::cell::RefCell<HashMap<String, Arc<DirNames>>>,
}

impl StackListing {
    pub(crate) fn set(&self, rel: &str, names: DirNames) {
        self.dirs
            .borrow_mut()
            .insert(rel.to_lowercase(), Arc::new(names));
    }

    /// Forgets the folder `rel` once the walk is done with it.
    pub(crate) fn forget(&self, rel: &str) {
        self.dirs.borrow_mut().remove(&rel.to_lowercase());
    }
}

impl Listing for StackListing {
    fn names(&self, rel: &str) -> Option<Arc<DirNames>> {
        self.dirs.borrow().get(&rel.to_lowercase()).cloned()
    }
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
            "src/__pycache__",
            ".next",
        ] {
            assert!(rules.excluded_dir(dir, &NoListing).is_some(), "{dir}");
        }
        for file in ["debug.log", "a/b/Thumbs.db", "pack.zip", "app.pdb"] {
            assert!(rules.excluded_file(file, &NoListing).is_some(), "{file}");
        }
        for file in [
            ".env",
            ".env.local",
            "src/main.rs",
            ".agentsrc",
            "agents/readme.md",
        ] {
            assert_eq!(rules.excluded_file(file, &NoListing), None, "{file}");
        }
        assert_eq!(rules.excluded_dir("src", &NoListing), None);
        assert_eq!(rules.excluded_dir("agents", &NoListing), None);
        assert!(rules.excluded_path(".agents/skills/x.md", &NoListing));
        assert!(rules.excluded_path("web/node_modules/pkg/index.js", &NoListing));
        assert!(!rules.excluded_path("web/src/index.js", &NoListing));
    }

    #[test]
    fn keep_patterns_win_over_the_global_list() {
        let input = RuleInput {
            keep: vec!["dist/".into(), "*.zip".into()],
            extra: vec!["secrets/".into()],
            ..defaults()
        };
        let rules = Rules::compile(&input, None).unwrap();
        assert_eq!(rules.excluded_dir("dist", &NoListing), None);
        assert_eq!(rules.excluded_file("assets/pack.zip", &NoListing), None);
        assert!(rules.excluded_dir("secrets", &NoListing).is_some());
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
        assert_eq!(
            rules.excluded_dir("build", &NoListing),
            None,
            "tracked sources stay"
        );
        assert_eq!(
            rules.excluded_dir("web/build", &NoListing),
            Some(Exclusion::Detected(detect::UNTRACKED_OUTPUT))
        );

        // No git: only a .gitignore says it is output.
        let plain = temp("build-plain");
        fs::create_dir_all(plain.join("build")).unwrap();
        fs::create_dir_all(plain.join("app").join("build")).unwrap();
        fs::write(plain.join("app").join(".gitignore"), "build/\n").unwrap();
        let rules = Rules::compile(&defaults(), Some(&plain)).unwrap();
        assert_eq!(rules.excluded_dir("build", &NoListing), None);
        assert_eq!(
            rules.excluded_dir("app/build", &NoListing),
            Some(Exclusion::Detected(detect::IGNORED_OUTPUT))
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
        assert_eq!(off.excluded_dir("app/build", &NoListing), None);
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
        assert_eq!(off.excluded_file("a.secret", &NoListing), None);

        let on = Rules::compile(
            &RuleInput {
                follow_gitignore: true,
                ..defaults()
            },
            Some(&root),
        )
        .unwrap();
        assert_eq!(
            on.excluded_file("a.secret", &NoListing),
            Some(Exclusion::Gitignore)
        );
        assert_eq!(
            on.excluded_dir("generated", &NoListing),
            Some(Exclusion::Gitignore)
        );
        assert_eq!(on.excluded_dir("src/generated", &NoListing), None);
        assert_eq!(on.excluded_file(".env", &NoListing), None);
        assert_eq!(on.excluded_file("config/.env.production", &NoListing), None);
        assert_eq!(
            on.excluded_file("keep/other.txt", &NoListing),
            Some(Exclusion::Gitignore)
        );
        assert_eq!(on.excluded_file("keep/this.txt", &NoListing), None);
        let _ = fs::remove_dir_all(root);
    }
}
