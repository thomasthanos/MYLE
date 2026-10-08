//! The Project Backups settings (`project-backups.json` next to MYLE's other
//! settings), checking what the page saves, and the one-time import of the
//! projects backup_projects knows.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::naming::{self, BACKUP_ROOT_NAME, CONFIG_MIRROR_FILE};
use super::rules::{DEFAULT_PATTERNS, Pattern, RuleInput};
use super::walk::same_path;
use crate::cloud::CloudProvider;

const FILE_NAME: &str = "project-backups.json";
const MAX_PATTERNS: usize = 500;
/// 2: names such as `target`, `out` and `release` are decided by the files
/// around them, no longer by the pattern list.
pub(crate) const SETTINGS_VERSION: u32 = 2;

/// Held while the settings are read, changed and saved.
static EDIT: Mutex<()> = Mutex::new(());

pub(crate) fn edit_lock() -> MutexGuard<'static, ()> {
    EDIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LastBackup {
    pub name: String,
    /// Unix milliseconds.
    pub created_at: i64,
    pub file_count: u64,
    pub zip_size: u64,
}

/// How the last backup attempt of a project ended.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LastResult {
    /// Unix milliseconds.
    pub at: i64,
    pub ok: bool,
    pub cancelled: bool,
    pub code: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source_path: String,
    /// The folder name under `Projects Backup` and the start of every backup
    /// name: `<appName>_D<day>_V<n>.zip`.
    pub app_name: String,
    /// A program (`MyApp.exe`) to close before the backup, if any.
    pub close_app: Option<String>,
    pub extra_exclusions: Vec<String>,
    pub keep: Vec<String>,
    pub last_backup: Option<LastBackup>,
    pub last_result: Option<LastResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    /// `googleDrive` or `dropbox`.
    pub provider: Option<CloudProvider>,
    /// The provider's folder (`G:\My Drive`); backups go to its
    /// `Projects Backup` folder.
    pub cloud_folder: Option<String>,
    pub projects: Vec<Project>,
    pub exclusions: Vec<String>,
    /// Leave out `build` folders only when they are build output.
    pub smart_build: bool,
    pub follow_gitignore: bool,
    /// The backup_projects import ran (it runs once by itself).
    pub imported: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            provider: None,
            cloud_folder: None,
            projects: Vec::new(),
            exclusions: default_patterns(),
            smart_build: true,
            follow_gitignore: false,
            imported: false,
        }
    }
}

pub(crate) fn default_patterns() -> Vec<String> {
    DEFAULT_PATTERNS
        .iter()
        .map(|pattern| pattern.to_string())
        .collect()
}

impl Settings {
    pub(crate) fn project(&self, id: &str) -> Result<&Project, String> {
        self.projects
            .iter()
            .find(|project| project.id == id)
            .ok_or_else(|| "This project no longer exists.".to_string())
    }

    pub(crate) fn rule_input(&self, project: Option<&Project>) -> RuleInput {
        RuleInput {
            patterns: self.exclusions.clone(),
            extra: project
                .map(|project| project.extra_exclusions.clone())
                .unwrap_or_default(),
            keep: project
                .map(|project| project.keep.clone())
                .unwrap_or_default(),
            smart_build: self.smart_build,
            follow_gitignore: self.follow_gitignore,
        }
    }

    /// `<cloud folder>\Projects Backup`.
    pub(crate) fn backup_root(&self) -> Option<PathBuf> {
        self.cloud_folder
            .as_deref()
            .filter(|folder| !folder.trim().is_empty())
            .map(|folder| Path::new(folder).join(BACKUP_ROOT_NAME))
    }
}

fn settings_path() -> Result<PathBuf, String> {
    crate::storage::roaming_dir().map(|dir| dir.join(FILE_NAME))
}

pub(crate) fn load() -> Result<Settings, String> {
    load_from(&settings_path()?)
}

pub(crate) fn load_from(path: &Path) -> Result<Settings, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(format!("Can't read the Project Backups settings: {error}")),
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(mut settings) => {
            migrate(&mut settings);
            Ok(settings)
        }
        Err(_) => {
            // Keep a damaged file for a look rather than overwrite it.
            let aside = path.with_extension(format!(
                "corrupt-{}.json",
                naming::unix_millis(std::time::SystemTime::now())
            ));
            let _ = fs::rename(path, aside);
            Ok(Settings::default())
        }
    }
}

/// Brings settings of an older MYLE up to date. The global list loses the
/// 9.4.0 defaults that are now told apart by markers (`target/`, `out/`,
/// `release/`, `gen/`, `venv/`, `*.obj`) and gains the new defaults; what
/// the user added or removed stays as it was.
pub(crate) fn migrate(settings: &mut Settings) {
    if settings.version >= SETTINGS_VERSION {
        return;
    }
    let old: Vec<String> = super::rules::DEFAULT_PATTERNS_V1
        .iter()
        .map(|p| p.to_lowercase())
        .collect();
    let current: Vec<String> = DEFAULT_PATTERNS.iter().map(|p| p.to_lowercase()).collect();
    settings.exclusions.retain(|pattern| {
        let lower = pattern.to_lowercase();
        !old.contains(&lower) || current.contains(&lower)
    });
    for pattern in DEFAULT_PATTERNS {
        let lower = pattern.to_lowercase();
        if !old.contains(&lower)
            && !settings
                .exclusions
                .iter()
                .any(|known| known.to_lowercase() == lower)
        {
            settings.exclusions.push((*pattern).to_string());
        }
    }
    settings.version = SETTINGS_VERSION;
}

pub(crate) fn save(settings: &Settings) -> Result<(), String> {
    save_to(&settings_path()?, settings)
}

pub(crate) fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    crate::game_saves::atomic::write(path, &text)
}

/// Reads the settings afresh, changes and saves them under the edit lock.
pub(crate) fn edit(
    change: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> Result<Settings, String> {
    let _edit = edit_lock();
    let mut settings = load()?;
    change(&mut settings)?;
    save(&settings)?;
    Ok(settings)
}

// ─── Checks ────────────────────────────────────────────────────────────────

const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A name usable as a single Windows file or folder name.
pub(crate) fn valid_file_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    !name.is_empty()
        && name.chars().count() <= 100
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !name.ends_with(['.', ' '])
        && !name.starts_with(' ')
        && !RESERVED.contains(&stem.as_str())
}

/// The names MYLE runs under: never closed for a backup.
pub(crate) fn is_own_program(exe: &str) -> bool {
    let lower = exe.to_ascii_lowercase();
    let own = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
    });
    [
        "myle.exe",
        "makeyourlifeeasier.exe",
        "make-your-life-easier.exe",
    ]
    .contains(&lower.as_str())
        || own.as_deref() == Some(lower.as_str())
}

pub(crate) fn clean_patterns(patterns: Vec<String>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for pattern in patterns {
        let pattern = pattern.trim().to_string();
        if pattern.is_empty() || out.iter().any(|known| known.eq_ignore_ascii_case(&pattern)) {
            continue;
        }
        Pattern::parse(&pattern)?;
        out.push(pattern);
    }
    if out.len() > MAX_PATTERNS {
        return Err(format!("Use at most {MAX_PATTERNS} patterns."));
    }
    Ok(out)
}

/// Checks and tidies a project from the editor. `id` empty: a new project.
pub(crate) fn check_project(
    settings: &Settings,
    mut project: Project,
    require_source: bool,
) -> Result<Project, String> {
    project.name = project.name.trim().to_string();
    project.app_name = project.app_name.trim().to_string();
    project.source_path = project
        .source_path
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_string();
    if project.source_path.ends_with(':') {
        project.source_path.push('\\');
    }
    project.close_app = project
        .close_app
        .map(|exe| exe.trim().to_string())
        .filter(|exe| !exe.is_empty());
    if project.name.is_empty()
        || project.name.chars().count() > 100
        || project.name.chars().any(char::is_control)
    {
        return Err("Enter a project name of 1 to 100 characters.".into());
    }
    if !valid_file_name(&project.app_name) {
        return Err("The backup name must work as a folder name: no \\ / : * ? \" < > |, and no dot or space at the end.".into());
    }
    if project.source_path.is_empty() {
        return Err("Choose the project folder.".into());
    }
    let source = Path::new(&project.source_path);
    if !source.is_absolute() {
        return Err("The project folder must be a full path, such as D:\\Projects\\MyApp.".into());
    }
    if require_source && !source.is_dir() {
        return Err(
            "The project folder does not exist. Choose the folder where the project is now.".into(),
        );
    }
    if let Some(root) = settings.backup_root()
        && (same_path(source, &root) || inside(source, &root))
    {
        return Err("The project folder is inside the backups folder.".into());
    }
    if let Some(exe) = &project.close_app {
        let plain =
            valid_file_name(exe) && exe.to_ascii_lowercase().ends_with(".exe") && exe.len() > 4;
        if !plain {
            return Err("The program to close must be a file name such as MyApp.exe.".into());
        }
        if is_own_program(exe) {
            return Err("MYLE can't close itself for a backup. Leave that field empty.".into());
        }
    }
    project.extra_exclusions = clean_patterns(project.extra_exclusions)?;
    project.keep = clean_patterns(project.keep)?;
    let existing = settings
        .projects
        .iter()
        .position(|known| known.id == project.id);
    if project.id.trim().is_empty() {
        project.id = uuid::Uuid::new_v4().to_string();
        project.last_backup = None;
        project.last_result = None;
    } else if let Some(index) = existing {
        // The backup history is the app's to keep, not the editor's.
        project.last_backup = settings.projects[index].last_backup.clone();
        project.last_result = settings.projects[index].last_result.clone();
    } else {
        return Err("This project no longer exists.".into());
    }
    if settings.projects.iter().any(|known| {
        known.id != project.id && known.app_name.eq_ignore_ascii_case(&project.app_name)
    }) {
        return Err(format!(
            "Another project already uses the backup name \"{}\".",
            project.app_name
        ));
    }
    Ok(project)
}

/// `path` is strictly inside `folder` (Windows path rules).
pub(crate) fn inside(path: &Path, folder: &Path) -> bool {
    let text = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let (path, folder) = (text(path), text(folder));
    path.len() > folder.len() + 1
        && path.starts_with(&folder)
        && path.as_bytes()[folder.len()] == b'\\'
}

// ─── Import from backup_projects ──────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: Vec<String>,
    pub already_known: usize,
    /// Where the list came from, if one was found.
    pub from: Option<String>,
}

/// backup_projects' own list on this PC.
pub(crate) fn backup_projects_config() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ThomasThanos")
            .join("Backup-projects")
            .join("projects.json"),
    )
}

/// The projects in a backup_projects list (`projects.json` or its copy in
/// the backups folder).
pub(crate) fn read_backup_projects(text: &str) -> Vec<Project> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.get("projects").and_then(Value::as_array) else {
        return Vec::new();
    };
    let field = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    items
        .iter()
        .filter_map(|item| {
            let app_name = field(item, "appName");
            let name = field(item, "name");
            if !valid_file_name(&app_name) {
                return None;
            }
            let exe = field(item, "appExe");
            let close_app = (!exe.is_empty()
                && exe.to_ascii_lowercase().ends_with(".exe")
                && valid_file_name(&exe)
                && !is_own_program(&exe))
            .then_some(exe);
            Some(Project {
                id: String::new(),
                name: if name.is_empty() {
                    app_name.clone()
                } else {
                    name
                },
                source_path: field(item, "sourcePath"),
                app_name,
                close_app,
                ..Project::default()
            })
        })
        .collect()
}

/// Projects that have backups in `root` (`Projects Backup`), for a PC
/// where no list was kept: their folders are known, their sources not.
pub(crate) fn projects_in_backup_root(root: &Path) -> Vec<Project> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let app_name = entry.file_name().to_string_lossy().into_owned();
        if !valid_file_name(&app_name) {
            continue;
        }
        let has_backups = fs::read_dir(entry.path()).is_ok_and(|months| {
            months.flatten().any(|month| {
                naming::is_month_folder(&month.file_name().to_string_lossy())
                    && fs::read_dir(month.path()).is_ok_and(|mut items| {
                        items.any(|item| {
                            item.is_ok_and(|item| {
                                naming::belongs_to(&item.file_name().to_string_lossy(), &app_name)
                            })
                        })
                    })
            })
        });
        if has_backups {
            found.push(Project {
                name: app_name.clone(),
                app_name,
                ..Project::default()
            });
        }
    }
    found.sort_by_key(|project| project.app_name.to_lowercase());
    found
}

/// Adds the projects of backup_projects that are not here yet (same backup
/// name). Looks at its list on this PC, then at the copy it keeps in the
/// backups folder, then at the backups themselves.
pub(crate) fn import(
    settings: &mut Settings,
    local_config: Option<&Path>,
    backup_root: Option<&Path>,
) -> ImportReport {
    let mut report = ImportReport::default();
    let mut candidates: Vec<(String, Vec<Project>)> = Vec::new();
    if let Some(path) = local_config
        && let Ok(text) = fs::read_to_string(path)
    {
        candidates.push((path.display().to_string(), read_backup_projects(&text)));
    }
    if let Some(root) = backup_root {
        let mirror = root.join(CONFIG_MIRROR_FILE);
        if let Ok(text) = fs::read_to_string(&mirror) {
            candidates.push((mirror.display().to_string(), read_backup_projects(&text)));
        }
        candidates.push((root.display().to_string(), projects_in_backup_root(root)));
    }
    let Some((from, projects)) = candidates
        .into_iter()
        .find(|(_, projects)| !projects.is_empty())
    else {
        return report;
    };
    report.from = Some(from);
    for mut project in projects {
        if settings
            .projects
            .iter()
            .any(|known| known.app_name.eq_ignore_ascii_case(&project.app_name))
        {
            report.already_known += 1;
            continue;
        }
        project.id = uuid::Uuid::new_v4().to_string();
        report.added.push(project.name.clone());
        settings.projects.push(project);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    const BACKUP_PROJECTS_JSON: &str = r#"{
      "activeProjectId": "default",
      "projects": [
        { "id": "default", "name": "MakeYourLifeEasier", "sourcePath": "H:\\Projects\\ThomasThanos\\Make_Your_Life_Easier.A.E", "appExe": "MYLE.exe", "appName": "MakeYourLifeEasier" },
        { "id": "p2", "name": "Steam Idler", "sourcePath": "D:\\Code\\steam-idler", "appExe": "SteamIdler.exe", "appName": "steam_idler" },
        { "id": "p3", "name": "Bad", "sourcePath": "D:\\x", "appExe": "", "appName": "a/b" },
        { "id": "p4", "name": "", "sourcePath": "", "appExe": "C:\\x.exe", "appName": "Restored", "restoredFromBackup": true }
      ]
    }"#;

    #[test]
    fn backup_projects_lists_are_imported_without_closing_myle() {
        let projects = read_backup_projects(BACKUP_PROJECTS_JSON);
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].app_name, "MakeYourLifeEasier");
        assert_eq!(projects[0].close_app, None, "MYLE never closes itself");
        assert_eq!(projects[1].close_app.as_deref(), Some("SteamIdler.exe"));
        assert_eq!(projects[1].source_path, "D:\\Code\\steam-idler");
        assert_eq!(projects[2].name, "Restored");
        assert_eq!(projects[2].close_app, None, "a path is not a program name");
    }

    #[test]
    fn import_prefers_the_local_list_then_the_copy_then_the_backups() {
        let root = temp("import");
        let config = root.join("projects.json");
        let backups = root.join("Projects Backup");
        fs::create_dir_all(backups.join("Old").join("2025-01 Ιανουάριος")).unwrap();
        fs::write(
            backups
                .join("Old")
                .join("2025-01 Ιανουάριος")
                .join("Old_D3_V1.zip"),
            b"x",
        )
        .unwrap();
        fs::create_dir_all(backups.join("NoBackups")).unwrap();

        let mut settings = Settings::default();
        let report = import(&mut settings, Some(&config), Some(&backups));
        assert_eq!(report.added, ["Old"], "only folders that hold backups");
        assert_eq!(settings.projects[0].source_path, "");

        fs::write(backups.join(CONFIG_MIRROR_FILE), BACKUP_PROJECTS_JSON).unwrap();
        let mut settings = Settings::default();
        let report = import(&mut settings, Some(&config), Some(&backups));
        assert_eq!(report.added.len(), 3);
        assert!(report.from.unwrap().ends_with(CONFIG_MIRROR_FILE));

        fs::write(
            &config,
            r#"{"projects":[{"name":"Steam Idler","appName":"STEAM_IDLER","sourcePath":"E:\\s"}]}"#,
        )
        .unwrap();
        let report = import(&mut settings, Some(&config), Some(&backups));
        assert!(report.added.is_empty());
        assert_eq!(report.already_known, 1, "matched by backup name, any case");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn projects_are_checked() {
        let root = temp("check");
        let mut settings = Settings {
            cloud_folder: Some(root.join("My Drive").display().to_string()),
            ..Settings::default()
        };
        fs::create_dir_all(root.join("src")).unwrap();
        let good = Project {
            name: " App ".into(),
            app_name: "App".into(),
            source_path: format!("{}\\", root.join("src").display()),
            close_app: Some("App.exe".into()),
            extra_exclusions: vec!["  ".into(), "secrets/".into(), "SECRETS/".into()],
            ..Project::default()
        };
        let saved = check_project(&settings, good.clone(), true).unwrap();
        assert_eq!(saved.name, "App");
        assert!(!saved.id.is_empty());
        assert!(!saved.source_path.ends_with('\\'));
        assert_eq!(saved.extra_exclusions, ["secrets/"]);
        settings.projects.push(saved.clone());

        let mut other = good.clone();
        other.app_name = "Other".into();
        assert!(check_project(&settings, other.clone(), true).is_ok());
        let bad = |change: fn(&mut Project)| {
            let mut project = other.clone();
            change(&mut project);
            check_project(&settings, project, true).is_err()
        };
        assert!(bad(|p| p.app_name = "a:b".into()));
        assert!(bad(|p| p.app_name = "con".into()));
        assert!(bad(|p| p.app_name = "name.".into()));
        assert!(bad(|p| p.close_app = Some("MYLE.exe".into())));
        assert!(bad(|p| p.close_app = Some("C:\\x\\App.exe".into())));
        assert!(bad(|p| p.close_app = Some("App.bat".into())));
        assert!(bad(|p| p.source_path = "relative\\path".into()));
        assert!(bad(|p| p.extra_exclusions = vec!["../x".into()]));
        assert!(bad(|p| p.id = "missing".into()));
        assert!(bad(|p| p.name = String::new()));
        // Same backup name as another project.
        assert!(bad(|p| p.app_name = "APP".into()));
        // Inside the backups folder.
        let mut nested = other.clone();
        nested.source_path = root
            .join("My Drive")
            .join("Projects Backup")
            .join("x")
            .display()
            .to_string();
        assert!(check_project(&settings, nested, false).is_err());

        // A missing folder is allowed (fixed later) unless asked for.
        let mut missing = saved.clone();
        missing.source_path = root.join("gone").display().to_string();
        assert!(check_project(&settings, missing.clone(), false).is_ok());
        assert!(check_project(&settings, missing, true).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn settings_survive_a_round_trip_and_a_damaged_file() {
        let root = temp("roundtrip");
        let path = root.join(FILE_NAME);
        assert_eq!(load_from(&path).unwrap(), Settings::default());
        let mut settings = Settings {
            provider: Some(CloudProvider::GoogleDrive),
            ..Settings::default()
        };
        settings.projects.push(Project {
            id: "1".into(),
            name: "A".into(),
            app_name: "A".into(),
            ..Project::default()
        });
        save_to(&path, &settings).unwrap();
        assert_eq!(load_from(&path).unwrap(), settings);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path).unwrap(), Settings::default());
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            1,
            "the damaged file is kept aside"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn settings_of_9_4_0_are_brought_up_to_date() {
        let mut settings = Settings {
            version: 1,
            exclusions: super::super::rules::DEFAULT_PATTERNS_V1
                .iter()
                .map(|p| p.to_string())
                .filter(|p| p != "*.zip")
                .chain(["secrets/".to_string()])
                .collect(),
            ..Settings::default()
        };
        migrate(&mut settings);
        assert_eq!(settings.version, SETTINGS_VERSION);
        let has = |p: &str| settings.exclusions.iter().any(|known| known == p);
        // Decided by markers now.
        for gone in ["target/", "out/", "release/", "gen/", "venv/", "*.obj"] {
            assert!(!has(gone), "{gone}");
        }
        // New defaults come in; the user's own changes stay.
        assert!(has(".ruff_cache/") && has(".dart_tool/") && has("node_modules/"));
        assert!(has("secrets/"));
        assert!(!has("*.zip"), "a default the user removed stays removed");
        let again = settings.clone();
        migrate(&mut settings);
        assert_eq!(settings, again);
    }
}
