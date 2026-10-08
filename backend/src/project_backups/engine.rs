//! One project's backup from start to finish, the list of its backups, and
//! the preview of what a backup would hold.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::json;

use super::archive::{self, MIN_ZIP_SIZE, ZipJob};
use super::naming;
use super::rules::{BACKUP_INFO_FILE, RuleInput, Rules};
use super::store::{Project, inside, valid_file_name};
use super::walk;
use super::{Failure, Sink, Stage};

/// `__partial__` files younger than this may belong to a backup running in
/// backup_projects right now.
const STALE_PARTIAL: Duration = Duration::from_secs(60 * 60);
/// Missing files named in an error message.
const NAMED_MISSING: usize = 8;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOutcome {
    pub name: String,
    pub zip_path: String,
    pub month_folder: String,
    pub version: u32,
    pub file_count: u64,
    pub total_bytes: u64,
    pub zip_size: u64,
    pub excluded_count: usize,
    pub skipped: Vec<SkippedItem>,
    pub linked_files: Vec<String>,
    /// The renamed zip showed its full size in time; when not, the cloud app
    /// is still catching up and the backup itself is fine.
    pub final_check_ok: bool,
    pub created_at: i64,
}

pub(crate) struct BackupRequest<'a> {
    pub project: &'a Project,
    pub rules: RuleInput,
    pub backup_root: &'a Path,
    pub staging: Option<&'a Path>,
    pub app_version: &'a str,
    pub delays: &'a [u64],
}

/// `<backup root>\<AppName>`, refusing names that would leave the root.
pub(crate) fn app_dir(backup_root: &Path, app_name: &str) -> Result<PathBuf, Failure> {
    let dir = backup_root.join(app_name);
    if !valid_file_name(app_name) || !inside(&dir, backup_root) {
        return Err(Failure::Message(format!(
            "\"{app_name}\" can't be used as a backup folder name."
        )));
    }
    Ok(dir)
}

pub(crate) fn backup_project(
    request: &BackupRequest,
    sink: &dyn Sink,
) -> Result<BackupOutcome, Failure> {
    let project = request.project;
    let source = PathBuf::from(&project.source_path);
    if project.source_path.trim().is_empty() || !source.is_dir() {
        return Err(Failure::SourceMissing);
    }
    sink.stage(Stage::Preparing);
    let app_dir = app_dir(request.backup_root, &project.app_name)?;
    fs::create_dir_all(&app_dir).map_err(|error| {
        Failure::Message(format!("Can't create \"{}\": {error}", app_dir.display()))
    })?;
    naming::remove_stale_partials(&app_dir, STALE_PARTIAL);
    if let Some(staging) = request.staging {
        archive::clean_staging(staging);
    }

    let now = SystemTime::now();
    let local = naming::local(now);
    let version = naming::next_version(&app_dir, &project.app_name);
    let name = naming::backup_name(&project.app_name, local.day, version);
    let month_folder = naming::month_folder(local.year, local.month);
    let final_path = app_dir.join(&month_folder).join(format!("{name}.zip"));

    sink.stage(Stage::Scanning);
    let rules = Rules::compile(&request.rules, Some(&source)).map_err(Failure::Message)?;
    let mut skip_dirs = vec![request.backup_root.to_path_buf()];
    skip_dirs.extend(request.staging.map(Path::to_path_buf));
    let cancelled = || sink.cancelled();
    let found = walk::walk(&source, &rules, &skip_dirs, &cancelled)?;
    if found.files.is_empty() {
        return Err(Failure::Message(
            "There are no files to back up in the project folder.".into(),
        ));
    }
    let skipped: Vec<SkippedItem> = found
        .skipped
        .iter()
        .map(|item| SkippedItem {
            path: item.rel.clone(),
            reason: item.reason.describe().into(),
        })
        .collect();
    let total_bytes = found.total_bytes();
    let info = json!({
        "name": name,
        "version": version,
        "day": local.day,
        "monthFolder": month_folder,
        "format": "zip",
        "appVersion": request.app_version,
        "createdBy": "MYLE",
        "sourcePath": project.source_path,
        "createdAt": naming::iso_utc(now),
        "timestamp": naming::unix_millis(now),
        "displayDate": naming::greek_display(local),
        "fileCount": found.files.len(),
        "totalBytes": total_bytes,
        "emptyFolders": found.empty_dirs.len(),
        "excludedCount": found.excluded.len(),
        "exclusions": request.rules.patterns.iter().chain(&request.rules.extra).collect::<Vec<_>>(),
        "kept": request.rules.keep,
        "skipped": skipped.iter().map(|item| &item.path).collect::<Vec<_>>(),
        "skippedDetails": skipped,
        "linkedFiles": found.links,
    });
    let info =
        serde_json::to_vec_pretty(&info).map_err(|error| Failure::Message(error.to_string()))?;
    let buffers = vec![(BACKUP_INFO_FILE.to_string(), info)];

    let mut completeness = |written: &archive::Written| -> Result<(), Failure> {
        sink.stage(Stage::CheckingCompleteness);
        let archived: HashSet<String> = written.manifest.keys().cloned().collect();
        let missing = walk::audit(&source, &rules, &skip_dirs, &archived, now);
        if missing.is_empty() {
            return Ok(());
        }
        let mut named = missing
            .iter()
            .take(NAMED_MISSING)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if missing.len() > NAMED_MISSING {
            named.push_str(&format!(" and {} more", missing.len() - NAMED_MISSING));
        }
        Err(Failure::Message(format!(
            "{} file(s) of the project are not in the zip: {named}. The backup was not saved.",
            missing.len()
        )))
    };
    let verified = archive::write_verified(
        &final_path,
        ZipJob {
            files: &found.files,
            empty_dirs: &found.empty_dirs,
            buffers: &buffers,
        },
        request.staging,
        request.delays,
        &mut completeness,
        sink,
        None,
    )?;
    Ok(BackupOutcome {
        name,
        zip_path: final_path.display().to_string(),
        month_folder,
        version,
        file_count: found.files.len() as u64,
        total_bytes,
        zip_size: verified.zip_size,
        excluded_count: found.excluded.len(),
        skipped,
        linked_files: found.links,
        final_check_ok: verified.final_check_ok,
        created_at: naming::unix_millis(now),
    })
}

// ─── Listing ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// `<month folder>/<file or folder name>`.
    pub id: String,
    pub name: String,
    pub month_folder: String,
    pub year: i64,
    pub month: u32,
    pub version: u32,
    pub day: u32,
    /// `zip`, or `folder` for the folder backups of older backup_projects.
    pub kind: &'static str,
    pub size: Option<u64>,
    /// Unix milliseconds.
    pub modified: Option<i64>,
    /// Too small to be a zip: a copy that never finished.
    pub broken: bool,
}

/// Every backup of `app_name`, newest first.
pub(crate) fn list_backups(app_dir: &Path, app_name: &str) -> Vec<BackupEntry> {
    let mut entries = Vec::new();
    let Ok(months) = fs::read_dir(app_dir) else {
        return entries;
    };
    for month in months.flatten() {
        let month_name = month.file_name().to_string_lossy().into_owned();
        let Some((year, month_number, _)) = naming::parse_month_folder(&month_name) else {
            continue;
        };
        if !naming::is_month_folder(&month_name)
            || !month.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let Ok(items) = fs::read_dir(month.path()) else {
            continue;
        };
        let mut zips = Vec::new();
        let mut folders = Vec::new();
        for item in items.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if !naming::belongs_to(&name, app_name) {
                continue;
            }
            let Ok(meta) = item.metadata() else {
                continue;
            };
            if meta.is_file() && name.to_lowercase().ends_with(".zip") {
                zips.push((name, meta));
            } else if meta.is_dir() {
                folders.push((name, meta));
            }
        }
        let zip_stems: HashSet<String> = zips
            .iter()
            .map(|(name, _)| naming::strip_zip(name).to_lowercase())
            .collect();
        for (name, meta) in zips.into_iter().chain(
            folders
                .into_iter()
                .filter(|(name, _)| !zip_stems.contains(&name.to_lowercase())),
        ) {
            let (base, day, version) = naming::parse_backup_name(&name);
            let is_zip = meta.is_file();
            entries.push(BackupEntry {
                id: format!("{month_name}/{name}"),
                name: base,
                month_folder: month_name.clone(),
                year,
                month: month_number,
                version,
                day,
                kind: if is_zip { "zip" } else { "folder" },
                size: is_zip.then_some(meta.len()),
                modified: meta.modified().ok().map(naming::unix_millis),
                broken: is_zip && meta.len() < MIN_ZIP_SIZE,
            });
        }
    }
    entries.sort_by(|a, b| b.version.cmp(&a.version).then(b.modified.cmp(&a.modified)));
    entries
}

/// The path of the backup `id` of `app_name`, refusing anything else.
pub(crate) fn resolve_backup(app_dir: &Path, app_name: &str, id: &str) -> Result<PathBuf, String> {
    let invalid = || format!("\"{id}\" is not a backup of this project.");
    let (month, name) = id.split_once('/').ok_or_else(invalid)?;
    if !naming::is_month_folder(month)
        || !valid_file_name(month)
        || !valid_file_name(name)
        || !naming::belongs_to(name, app_name)
    {
        return Err(invalid());
    }
    let path = app_dir.join(month).join(name);
    if !path.exists() {
        return Err(format!("The backup \"{name}\" is no longer there."));
    }
    Ok(path)
}

/// Which of two backups is the older one, by version and then date.
pub(crate) fn older_first<'a>(
    a: &'a BackupEntry,
    b: &'a BackupEntry,
) -> (&'a BackupEntry, &'a BackupEntry) {
    if (a.version, a.modified) <= (b.version, b.modified) {
        (a, b)
    } else {
        (b, a)
    }
}

// ─── Preview ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedItem {
    pub path: String,
    pub is_dir: bool,
    pub rule: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub file_count: usize,
    pub total_bytes: u64,
    pub excluded: Vec<ExcludedItem>,
    pub excluded_total: usize,
    pub skipped: Vec<SkippedItem>,
    pub env_files: Vec<String>,
}

const PREVIEW_LIMIT: usize = 1000;

pub(crate) fn preview(
    source: &Path,
    input: &RuleInput,
    skip_dirs: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
) -> Result<Preview, String> {
    if !source.is_dir() {
        return Err("The project folder was not found.".into());
    }
    let rules = Rules::compile(input, Some(source))?;
    let found = walk::walk(source, &rules, skip_dirs, cancelled)?;
    let env_files = found
        .files
        .iter()
        .filter(|file| {
            let name = file
                .rel
                .rsplit('/')
                .next()
                .unwrap_or(&file.rel)
                .to_lowercase();
            name == ".env" || name.starts_with(".env.")
        })
        .map(|file| file.rel.clone())
        .collect();
    Ok(Preview {
        file_count: found.files.len(),
        total_bytes: found.total_bytes(),
        excluded_total: found.excluded.len(),
        excluded: found
            .excluded
            .iter()
            .take(PREVIEW_LIMIT)
            .map(|item| ExcludedItem {
                path: item.rel.clone(),
                is_dir: item.is_dir,
                rule: item.why.describe(),
            })
            .collect(),
        skipped: found
            .skipped
            .iter()
            .map(|item| SkippedItem {
                path: item.rel.clone(),
                reason: item.reason.describe().into(),
            })
            .collect(),
        env_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::archive::tests::{Quiet, temp};
    use crate::project_backups::rules::DEFAULT_PATTERNS;
    use std::io::Read;

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn input() -> RuleInput {
        RuleInput {
            patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
            smart_build: true,
            ..RuleInput::default()
        }
    }

    fn zip_names(path: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn a_backup_lands_where_backup_projects_puts_it_and_numbering_continues() {
        let root = temp("engine");
        let source = root.join("steam-idler");
        put(&source, "package.json", "{}");
        put(&source, "src/main.js", "console.log(1)");
        put(&source, ".env", "TOKEN=1");
        put(&source, "node_modules/x/index.js", "x");
        put(&source, ".agents/skills/a.md", "a");
        put(&source, "build/installer.nsh", "!define X");
        put(&source, "dist/app.exe", "exe");
        let backup_root = root.join("My Drive").join("Projects Backup");
        // backup_projects made V7 last month (Greek month folder) and a
        // folder backup V3 earlier.
        let app_dir = backup_root.join("steam_idler");
        put(&app_dir, "2025-12 Δεκέμβριος/steam_idler_D30_V7.zip", "PK");
        fs::create_dir_all(app_dir.join("2025-11 Νοέμβριος").join("steam_idler_D2_V3")).unwrap();
        let project = Project {
            id: "1".into(),
            name: "Steam Idler".into(),
            app_name: "steam_idler".into(),
            source_path: source.display().to_string(),
            ..Project::default()
        };
        let staging = root.join("staging");
        let request = BackupRequest {
            project: &project,
            rules: input(),
            backup_root: &backup_root,
            staging: Some(&staging),
            app_version: "9.4.0",
            delays: &[1, 1],
        };
        let outcome = backup_project(&request, &Quiet).unwrap();
        assert_eq!(outcome.version, 8);
        let local = naming::local(SystemTime::now());
        assert_eq!(outcome.name, format!("steam_idler_D{}_V8", local.day));
        assert_eq!(
            outcome.month_folder,
            naming::month_folder(local.year, local.month)
        );
        let zip = PathBuf::from(&outcome.zip_path);
        assert!(zip.starts_with(app_dir.join(&outcome.month_folder)));
        let mut names = zip_names(&zip);
        names.sort();
        // build/ holds a source file (no git, no .gitignore): kept.
        assert_eq!(
            names,
            [
                ".backup-info.json",
                ".env",
                "build/installer.nsh",
                "package.json",
                "src/main.js"
            ]
        );
        assert_eq!(outcome.file_count, 4);

        let mut archive = zip::ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
        let mut text = String::new();
        archive
            .by_name(BACKUP_INFO_FILE)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let info: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(info["name"], outcome.name);
        assert_eq!(info["version"], 8);
        assert_eq!(info["format"], "zip");
        assert_eq!(info["fileCount"], 4);
        assert_eq!(info["monthFolder"], outcome.month_folder);
        assert!(info["createdAt"].as_str().unwrap().ends_with('Z'));
        assert!(info["skipped"].as_array().unwrap().is_empty());

        // The list shows both apps' backups, newest first; the next one is V9.
        let list = list_backups(&app_dir, "steam_idler");
        let versions: Vec<u32> = list.iter().map(|entry| entry.version).collect();
        assert_eq!(versions, [8, 7, 3]);
        assert!(list[1].broken, "a 2-byte zip is broken");
        assert_eq!(list[2].kind, "folder");
        assert_eq!(naming::next_version(&app_dir, "steam_idler"), 9);
        assert!(resolve_backup(&app_dir, "steam_idler", &list[0].id).is_ok());
        assert!(resolve_backup(&app_dir, "steam_idler", "../x/steam_idler_D1_V1.zip").is_err());
        assert!(
            resolve_backup(
                &app_dir,
                "steam_idler",
                "2025-12 Δεκέμβριος/other_D1_V1.zip"
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_project_folder_is_reported_as_such() {
        let root = temp("missing");
        let project = Project {
            id: "1".into(),
            name: "A".into(),
            app_name: "A".into(),
            source_path: root.join("gone").display().to_string(),
            ..Project::default()
        };
        let request = BackupRequest {
            project: &project,
            rules: input(),
            backup_root: &root.join("backups"),
            staging: None,
            app_version: "9.4.0",
            delays: &[1],
        };
        assert!(matches!(
            backup_project(&request, &Quiet),
            Err(Failure::SourceMissing)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn backups_inside_the_project_are_not_backed_up_again() {
        let root = temp("nested");
        let source = root.join("project");
        put(&source, "a.txt", "a");
        let backup_root = source.join("Projects Backup");
        let project = Project {
            id: "1".into(),
            name: "P".into(),
            app_name: "P".into(),
            source_path: source.display().to_string(),
            ..Project::default()
        };
        let request = BackupRequest {
            project: &project,
            rules: input(),
            backup_root: &backup_root,
            staging: None,
            app_version: "9.4.0",
            delays: &[1],
        };
        let first = backup_project(&request, &Quiet).unwrap();
        let second = backup_project(&request, &Quiet).unwrap();
        assert_eq!((first.version, second.version), (1, 2));
        let mut names = zip_names(Path::new(&second.zip_path));
        names.sort();
        assert_eq!(names, [".backup-info.json", "a.txt"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_preview_lists_what_is_left_out() {
        let root = temp("preview");
        put(&root, "src/a.rs", "a");
        put(&root, ".env.local", "X=1");
        put(&root, "target/debug/app.exe", "x");
        let preview = preview(&root, &input(), &[], &|| false).unwrap();
        assert_eq!(preview.file_count, 2);
        assert_eq!(preview.env_files, [".env.local"]);
        assert_eq!(preview.excluded.len(), 1);
        assert_eq!(preview.excluded[0].path, "target");
        assert_eq!(preview.excluded[0].rule, "target/");
        let _ = fs::remove_dir_all(root);
    }

    /// A zip made by backup_projects 2.0.6 (`createBackupZip`) from the
    /// project written below.
    const BACKUP_PROJECTS_ZIP: &[u8] = include_bytes!("testdata/backup_projects_2.0.6.zip");

    fn demo_project(root: &Path) {
        put(
            root,
            "package.json",
            "{\"name\":\"demo\",\"version\":\"1.0.0\"}\n",
        );
        put(root, "src/main.js", "const a = 1;\nconsole.log(a);\n");
        put(root, ".env", "TOKEN=abc\n");
        put(root, "Σημειώσεις/σημείωση.txt", "γεια σου\n");
        put(root, "node_modules/x/index.js", "x");
        fs::write(
            root.join("src").join("blob.bin"),
            (0..12).flat_map(|_| 0..=255u8).collect::<Vec<u8>>(),
        )
        .unwrap();
        fs::create_dir_all(root.join("assets").join("empty")).unwrap();
    }

    #[test]
    fn backups_made_by_backup_projects_are_read_compared_and_continued() {
        use crate::project_backups::compare::{self, Side};
        let root = temp("compat");
        let source = root.join("demo");
        demo_project(&source);
        let backup_root = root.join("Projects Backup");
        let month = backup_root.join("demo").join("2026-10 Οκτώβριος");
        fs::create_dir_all(&month).unwrap();
        let old_zip = month.join("demo_D5_V12.zip");
        fs::write(&old_zip, BACKUP_PROJECTS_ZIP).unwrap();

        // The zip reads back whole and holds the files and the info file.
        let check = archive::verify_zip(&old_zip, None, &Quiet).unwrap();
        assert_eq!(check.files, 6);
        let mut archive = zip::ZipArchive::new(fs::File::open(&old_zip).unwrap()).unwrap();
        let mut text = String::new();
        archive
            .by_name(BACKUP_INFO_FILE)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let info: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(info["fileCount"], 5);
        assert_eq!(
            info["displayDate"],
            naming::greek_display(naming::utc(
                std::time::UNIX_EPOCH + Duration::from_millis(1_791_194_400_000 + 3 * 3_600_000),
            ))
        );

        // Same content as the project folder.
        let rules = Rules::compile(&input(), Some(&source)).unwrap();
        let same = compare::compare(
            &Side::Zip(old_zip.clone()),
            &Side::Folder(source.clone()),
            &rules,
            Some(&source),
            &|| false,
        )
        .unwrap();
        assert!(same.changes.is_empty(), "{:?}", same.changes);
        assert_eq!(same.unchanged, 5);

        // MYLE's next backup is V13 in the same place, and matches it.
        let project = Project {
            id: "1".into(),
            name: "Demo".into(),
            app_name: "demo".into(),
            source_path: source.display().to_string(),
            ..Project::default()
        };
        let request = BackupRequest {
            project: &project,
            rules: input(),
            backup_root: &backup_root,
            staging: Some(&root.join("staging")),
            app_version: "9.4.0",
            delays: &[1],
        };
        let outcome = backup_project(&request, &Quiet).unwrap();
        assert_eq!(outcome.version, 13);
        let new_zip = PathBuf::from(&outcome.zip_path);
        let diff = compare::compare(
            &Side::Zip(old_zip.clone()),
            &Side::Zip(new_zip.clone()),
            &rules,
            Some(&source),
            &|| false,
        )
        .unwrap();
        assert!(diff.changes.is_empty(), "{:?}", diff.changes);

        // A change in the project shows up against both.
        fs::write(
            source.join("src").join("main.js"),
            "const a = 2;\nconsole.log(a);\n",
        )
        .unwrap();
        let changed = compare::compare(
            &Side::Zip(old_zip.clone()),
            &Side::Folder(source.clone()),
            &rules,
            Some(&source),
            &|| false,
        )
        .unwrap();
        assert_eq!(changed.modified, 1);
        let lines = compare::file_diff(
            &Side::Zip(old_zip),
            Some("src/main.js"),
            &Side::Folder(source.clone()),
            Some("src/main.js"),
        )
        .unwrap();
        let compare::FileDiff::Text { rows, .. } = lines else {
            panic!("a text diff")
        };
        assert!(
            rows.iter()
                .any(|row| row.old_text.as_deref() == Some("const a = 1;")
                    && row.new_text.as_deref() == Some("const a = 2;"))
        );
        let _ = fs::remove_dir_all(root);
    }
}
