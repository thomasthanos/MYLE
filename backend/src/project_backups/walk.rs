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

use super::rules::{Exclusion, Rules};

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

/// Every file of `root` a backup holds, in a stable order.
pub(crate) fn walk(
    root: &Path,
    rules: &Rules,
    skip_dirs: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
) -> Result<Walk, String> {
    let mut out = Walk::default();
    walk_dir(root, "", rules, skip_dirs, cancelled, &mut out)?;
    Ok(out)
}

fn walk_dir(
    dir: &Path,
    rel: &str,
    rules: &Rules,
    skip_dirs: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
    out: &mut Walk,
) -> Result<(), String> {
    if cancelled() {
        return Err(super::CANCELLED.into());
    }
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("Can't read the folder \"{}\": {error}", dir.display()))?;
    let mut entries: Vec<fs::DirEntry> = entries
        .collect::<Result<_, _>>()
        .map_err(|error| format!("Can't read the folder \"{}\": {error}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let child = join_rel(rel, &name);
        match classify(entry.file_type().ok(), &path)? {
            Kind::Dir => {
                if let Some(why) = rules.excluded_dir(&child) {
                    out.excluded.push(Excluded {
                        rel: child,
                        is_dir: true,
                        why,
                    });
                    continue;
                }
                if is_skipped_dir(&path, skip_dirs) {
                    continue;
                }
                let before = (
                    out.files.len(),
                    out.empty_dirs.len(),
                    out.excluded.len(),
                    out.skipped.len(),
                );
                walk_dir(&path, &child, rules, skip_dirs, cancelled, out)?;
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
                if let Some(why) = rules.excluded_file(&child) {
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
                if rules.excluded_dir(&child).is_some() || rules.excluded_file(&child).is_some() {
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
    audit_dir(root, "", rules, skip_dirs, archived, since, &mut missing);
    missing.sort();
    missing
}

fn audit_dir(
    dir: &Path,
    rel: &str,
    rules: &Rules,
    skip_dirs: &[PathBuf],
    archived: &HashSet<String>,
    since: SystemTime,
    missing: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = dir.join(&name);
        let child = join_rel(rel, &name);
        let (Ok(own), Ok(meta)) = (fs::symlink_metadata(&path), fs::metadata(&path)) else {
            continue; // gone meanwhile, or a broken link
        };
        if meta.is_dir() {
            if own.file_type().is_symlink()
                || rules.excluded_dir(&child).is_some()
                || is_skipped_dir(&path, skip_dirs)
            {
                continue;
            }
            audit_dir(&path, &child, rules, skip_dirs, archived, since, missing);
        } else if meta.is_file() {
            if rules.excluded_file(&child).is_some() {
                continue;
            }
            let changed = [meta.modified().ok(), meta.created().ok()]
                .into_iter()
                .flatten()
                .any(|time| time >= since);
            if !changed && !archived.contains(&child) {
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
