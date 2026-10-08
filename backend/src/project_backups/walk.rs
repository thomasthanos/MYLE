//! Reading a project folder: which files a backup holds, which it leaves
//! out, and which it can't hold.
//!
//! Windows marks many ordinary files with a reparse point: online-only and
//! "always keep" files of OneDrive and Dropbox, files compressed with
//! `compact`/CompactGUI (WOF), deduplicated files. Node.js (backup_projects
//! up to 2.0.5) reported them as links and skipped them without a word, which
//! made backups incomplete. Here a directory entry's type is trusted only for
//! plain files and folders; anything else is decided from its metadata, and a
//! second, independent walk (`audit`) checks the finished zip against the
//! folder.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use super::rules::{DirNames, Early, Exclusion, Rules, StackListing};

#[derive(Clone, Debug)]
pub(crate) struct SourceFile {
    /// Relative to the project folder, with `/`.
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkipReason {
    /// A link (or junction) to a folder: never followed.
    LinkToFolder,
    /// A link whose target is gone.
    BrokenLink,
    /// A device or other entry that is neither a file nor a folder.
    NotAFile,
}

impl SkipReason {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            SkipReason::LinkToFolder => "link to a folder",
            SkipReason::BrokenLink => "broken link",
            SkipReason::NotAFile => "not a regular file",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Skipped {
    pub rel: String,
    pub reason: SkipReason,
}

#[derive(Clone, Debug)]
pub(crate) struct Excluded {
    pub rel: String,
    pub is_dir: bool,
    pub why: Exclusion,
}

#[derive(Debug, Default)]
pub(crate) struct Walk {
    pub files: Vec<SourceFile>,
    pub skipped: Vec<Skipped>,
    /// Links to files: their content is backed up.
    pub links: Vec<String>,
    pub excluded: Vec<Excluded>,
    /// Folders with nothing to back up inside; kept as folder entries.
    pub empty_dirs: Vec<String>,
}

impl Walk {
    pub(crate) fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

enum Kind {
    Dir,
    File { link: bool },
    Skip(SkipReason),
}

/// The type of `path`, trusting the directory entry only when it says a
/// plain file or folder.
fn classify(entry_type: Option<fs::FileType>, path: &Path) -> Result<Kind, String> {
    if let Some(kind) = entry_type {
        if kind.is_dir() {
            return Ok(Kind::Dir);
        }
        if kind.is_file() {
            return Ok(Kind::File { link: false });
        }
    }
    let own = fs::symlink_metadata(path)
        .map_err(|error| format!("Can't read \"{}\": {error}", path.display()))?;
    if own.file_type().is_symlink() {
        return Ok(match fs::metadata(path) {
            Ok(target) if target.is_file() => Kind::File { link: true },
            Ok(target) if target.is_dir() => Kind::Skip(SkipReason::LinkToFolder),
            Ok(_) => Kind::Skip(SkipReason::NotAFile),
            Err(_) => Kind::Skip(SkipReason::BrokenLink),
        });
    }
    if own.is_dir() {
        return Ok(Kind::Dir);
    }
    if own.is_file() {
        return Ok(Kind::File { link: false });
    }
    Ok(Kind::Skip(SkipReason::NotAFile))
}

pub(crate) fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Same folder, the way Windows compares paths.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    let text = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    text(a) == text(b)
}

fn is_skipped_dir(path: &Path, skip: &[PathBuf]) -> bool {
    skip.iter().any(|dir| same_path(dir, path))
}

/// The entries of a folder, sorted by name.
fn read_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("Can't read the folder \"{}\": {error}", dir.display()))?;
    let mut entries: Vec<fs::DirEntry> = entries
        .collect::<Result<_, _>>()
        .map_err(|error| format!("Can't read the folder \"{}\": {error}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn names_of(entries: &[fs::DirEntry]) -> DirNames {
    let mut names = DirNames::default();
    for entry in entries {
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        names.add(&entry.file_name().to_string_lossy(), is_dir);
    }
    names
}

/// Every file of `root` a backup holds, in a stable order.
pub(crate) fn walk(
    root: &Path,
    rules: &Rules,
    skip_dirs: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
) -> Result<Walk, String> {
    let mut out = Walk::default();
    let listing = StackListing::default();
    let entries = read_sorted(root)?;
    listing.set("", names_of(&entries));
    walk_dir(
        &Walker {
            rules,
            skip_dirs,
            cancelled,
            listing: &listing,
        },
        "",
        entries,
        &mut out,
    )?;
    Ok(out)
}

struct Walker<'a> {
    rules: &'a Rules,
    skip_dirs: &'a [PathBuf],
    cancelled: &'a dyn Fn() -> bool,
    listing: &'a StackListing,
}

fn walk_dir(
    walker: &Walker,
    rel: &str,
    entries: Vec<fs::DirEntry>,
    out: &mut Walk,
) -> Result<(), String> {
    if (walker.cancelled)() {
        return Err(super::CANCELLED.into());
    }
    let rules = walker.rules;
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let child = join_rel(rel, &name);
        match classify(entry.file_type().ok(), &path)? {
            Kind::Dir => {
                let early = rules.early_dir(&child);
                if let Early::Excluded(why) = early {
                    out.excluded.push(Excluded {
                        rel: child,
                        is_dir: true,
                        why,
                    });
                    continue;
                }
                if is_skipped_dir(&path, walker.skip_dirs) {
                    continue;
                }
                // Read before deciding: markers inside (CACHEDIR.TAG,
                // pyvenv.cfg) tell output apart. A folder that can't be read
                // only fails the backup when it is not left out anyway.
                let inner = read_sorted(&path);
                walker
                    .listing
                    .set(&child, inner.as_deref().map(names_of).unwrap_or_default());
                if matches!(early, Early::Undecided)
                    && let Some(why) = rules.detected_dir(&child, walker.listing)
                {
                    walker.listing.forget(&child);
                    out.excluded.push(Excluded {
                        rel: child,
                        is_dir: true,
                        why,
                    });
                    continue;
                }
                let inner = match inner {
                    Ok(inner) => inner,
                    Err(error) => {
                        walker.listing.forget(&child);
                        return Err(error);
                    }
                };
                let before = (
                    out.files.len(),
                    out.empty_dirs.len(),
                    out.excluded.len(),
                    out.skipped.len(),
                );
                let result = walk_dir(walker, &child, inner, out);
                walker.listing.forget(&child);
                result?;
                if before
                    == (
                        out.files.len(),
                        out.empty_dirs.len(),
                        out.excluded.len(),
                        out.skipped.len(),
                    )
                {
                    out.empty_dirs.push(child);
                }
            }
            Kind::File { link } => {
                if let Some(why) = rules.excluded_file(&child, walker.listing) {
                    out.excluded.push(Excluded {
                        rel: child,
                        is_dir: false,
                        why,
                    });
                    continue;
                }
                let meta = fs::metadata(&path)
                    .map_err(|error| format!("Can't read \"{}\": {error}", path.display()))?;
                if link {
                    out.links.push(child.clone());
                }
                out.files.push(SourceFile {
                    rel: child,
                    abs: path,
                    size: meta.len(),
                    modified: meta.modified().ok(),
                });
            }
            Kind::Skip(reason) => {
                if !matches!(rules.early_dir(&child), Early::Undecided | Early::Keep)
                    || rules.excluded_file(&child, walker.listing).is_some()
                {
                    continue;
                }
                out.skipped.push(Skipped { rel: child, reason });
            }
        }
    }
    Ok(())
}

/// The files of `root` that should be in a backup but are not in `archived`
/// (zip entry names). Walks again without trusting directory entry types at
/// all; files written after `since` (the backup was already reading) don't
/// count.
pub(crate) fn audit(
    root: &Path,
    rules: &Rules,
    skip_dirs: &[PathBuf],
    archived: &HashSet<String>,
    since: SystemTime,
) -> Vec<String> {
    let mut missing = Vec::new();
    let since = since.checked_sub(Duration::from_secs(2)).unwrap_or(since);
    let listing = StackListing::default();
    let Some(entries) = audit_entries(root) else {
        return missing;
    };
    listing.set("", audit_names(&entries));
    let auditor = Auditor {
        rules,
        skip_dirs,
        archived,
        since,
        listing: &listing,
    };
    audit_dir(&auditor, "", entries, &mut missing);
    missing.sort();
    missing
}

struct Auditor<'a> {
    rules: &'a Rules,
    skip_dirs: &'a [PathBuf],
    archived: &'a HashSet<String>,
    since: SystemTime,
    listing: &'a StackListing,
}

/// A folder's entries with their metadata, read without trusting entry types.
struct AuditEntry {
    name: String,
    path: PathBuf,
    own: fs::Metadata,
    meta: fs::Metadata,
}

fn audit_entries(dir: &Path) -> Option<Vec<AuditEntry>> {
    let entries = fs::read_dir(dir).ok()?;
    Some(
        entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = dir.join(&name);
                // Gone meanwhile, or a broken link.
                let own = fs::symlink_metadata(&path).ok()?;
                let meta = fs::metadata(&path).ok()?;
                Some(AuditEntry {
                    name,
                    path,
                    own,
                    meta,
                })
            })
            .collect(),
    )
}

fn audit_names(entries: &[AuditEntry]) -> DirNames {
    let mut names = DirNames::default();
    for entry in entries {
        names.add(
            &entry.name,
            entry.meta.is_dir() && !entry.own.file_type().is_symlink(),
        );
    }
    names
}

fn audit_dir(auditor: &Auditor, rel: &str, entries: Vec<AuditEntry>, missing: &mut Vec<String>) {
    let rules = auditor.rules;
    for entry in entries {
        let child = join_rel(rel, &entry.name);
        if entry.meta.is_dir() {
            if entry.own.file_type().is_symlink() || is_skipped_dir(&entry.path, auditor.skip_dirs)
            {
                continue;
            }
            let early = rules.early_dir(&child);
            if matches!(early, Early::Excluded(_)) {
                continue;
            }
            let Some(inner) = audit_entries(&entry.path) else {
                continue;
            };
            auditor.listing.set(&child, audit_names(&inner));
            let excluded = matches!(early, Early::Undecided)
                && rules.detected_dir(&child, auditor.listing).is_some();
            if !excluded {
                audit_dir(auditor, &child, inner, missing);
            }
            auditor.listing.forget(&child);
        } else if entry.meta.is_file() {
            if rules.excluded_file(&child, auditor.listing).is_some() {
                continue;
            }
            let changed = [entry.meta.modified().ok(), entry.meta.created().ok()]
                .into_iter()
                .flatten()
                .any(|time| time >= auditor.since);
            if !changed && !auditor.archived.contains(&child) {
                missing.push(child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::rules::{DEFAULT_PATTERNS, RuleInput};

    pub(crate) fn rules() -> Rules {
        Rules::compile(
            &RuleInput {
                patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
                smart_build: true,
                ..RuleInput::default()
            },
            None,
        )
        .unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-walk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn excluded_folders_are_not_entered_and_files_come_in_order() {
        let root = temp("order");
        put(&root, "src/b.rs", "b");
        put(&root, "src/a.rs", "a");
        put(&root, "package.json", "{}");
        put(&root, ".env", "SECRET=1");
        put(&root, "node_modules/x/index.js", "x");
        put(&root, ".agents/skill.md", "s");
        put(&root, "debug.log", "l");
        fs::create_dir_all(root.join("assets").join("empty")).unwrap();
        let result = walk(&root, &rules(), &[], &|| false).unwrap();
        let names: Vec<&str> = result.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, [".env", "package.json", "src/a.rs", "src/b.rs"]);
        let excluded: Vec<&str> = result.excluded.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(excluded, [".agents", "debug.log", "node_modules"]);
        assert!(result.skipped.is_empty());
        assert_eq!(result.empty_dirs, ["assets/empty"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_backup_folder_inside_the_project_is_never_backed_up() {
        let root = temp("skip");
        put(&root, "src/a.rs", "a");
        put(&root, "My Drive/Projects Backup/App/x.zip.part", "zip");
        let skip = vec![root.join("My Drive").join("Projects Backup")];
        let result = walk(&root, &rules(), &skip, &|| false).unwrap();
        let names: Vec<&str> = result.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, ["src/a.rs"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_audit_finds_files_missing_from_an_archive() {
        let root = temp("audit");
        put(&root, "package.json", "{}");
        put(&root, "src/main.ts", "m");
        put(&root, "node_modules/x.js", "x");
        let since = SystemTime::now() + Duration::from_secs(60);
        let mut archived: HashSet<String> = ["package.json".to_string()].into();
        assert_eq!(
            audit(&root, &rules(), &[], &archived, since),
            ["src/main.ts"]
        );
        archived.insert("src/main.ts".into());
        assert!(audit(&root, &rules(), &[], &archived, since).is_empty());
        // Files written while the backup ran are not counted as missing.
        assert!(
            audit(
                &root,
                &rules(),
                &[],
                &HashSet::new(),
                SystemTime::now() - Duration::from_secs(60)
            )
            .is_empty()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelling_stops_the_walk() {
        let root = temp("cancel");
        put(&root, "a.txt", "a");
        assert!(walk(&root, &rules(), &[], &|| true).is_err());
        let _ = fs::remove_dir_all(root);
    }

    /// Links to files are followed, links to folders (and junctions) and
    /// broken links are reported, never entered.
    #[cfg(windows)]
    #[test]
    fn links_are_followed_to_files_only() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let root = temp("links");
        put(&root, "real/a.txt", "a");
        put(&root, "target.txt", "t");
        let outside = temp("links-outside");
        put(&outside, "secret.txt", "s");
        let made = symlink_file(root.join("target.txt"), root.join("link.txt")).is_ok()
            && fs::symlink_metadata(root.join("link.txt"))
                .is_ok_and(|meta| meta.file_type().is_symlink());
        if !made {
            eprintln!("symbolic links need Developer Mode or admin rights here: skipped");
            let _ = fs::remove_dir_all(root);
            return;
        }
        symlink_file(root.join("missing.txt"), root.join("broken.txt")).unwrap();
        symlink_dir(&outside, root.join("dirlink")).unwrap();
        let junction = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("junction"))
            .arg(&outside)
            .output()
            .is_ok_and(|out| out.status.success());
        let result = walk(&root, &rules(), &[], &|| false).unwrap();
        let names: Vec<&str> = result.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, ["link.txt", "real/a.txt", "target.txt"]);
        assert_eq!(result.links, ["link.txt"]);
        let mut skipped: Vec<(&str, SkipReason)> = result
            .skipped
            .iter()
            .map(|s| (s.rel.as_str(), s.reason))
            .collect();
        skipped.sort();
        let mut expected = vec![
            ("broken.txt", SkipReason::BrokenLink),
            ("dirlink", SkipReason::LinkToFolder),
        ];
        if junction {
            expected.push(("junction", SkipReason::LinkToFolder));
        }
        assert_eq!(skipped, expected);
        // The audit agrees: nothing behind the folder links is missing.
        let archived: HashSet<String> = names.iter().map(|n| n.to_string()).collect();
        assert!(
            audit(
                &root,
                &rules(),
                &[],
                &archived,
                SystemTime::now() + Duration::from_secs(60)
            )
            .is_empty()
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    /// Files Windows marks with a reparse point that is not a link (cloud
    /// placeholders, `compact` (WOF) files, deduplicated files) are ordinary
    /// files to std and are backed up. Node.js up to backup_projects 2.0.5
    /// skipped them.
    #[cfg(windows)]
    #[test]
    fn compressed_and_placeholder_files_are_backed_up() {
        let root = temp("wof");
        put(&root, "app.exe", &"MZ".repeat(50_000));
        put(&root, "plain.txt", "p");
        // WOF-compress one file, as CompactGUI does. Not every file system
        // (or test runner) allows it; the walk must hold it either way.
        let _ = std::process::Command::new("compact")
            .args(["/C", "/EXE:XPRESS4K"])
            .arg(root.join("app.exe"))
            .output();
        let result = walk(&root, &rules(), &[], &|| false).unwrap();
        let names: Vec<&str> = result.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, ["app.exe", "plain.txt"]);
        assert_eq!(result.files[0].size, 100_000);
        assert!(result.skipped.is_empty() && result.links.is_empty());
        // Without the entry's own type the metadata decides, the same way.
        assert!(matches!(
            classify(None, &root.join("app.exe")).unwrap(),
            Kind::File { link: false }
        ));
        assert!(matches!(classify(None, &root).unwrap(), Kind::Dir));
        let _ = fs::remove_dir_all(root);
    }
}
