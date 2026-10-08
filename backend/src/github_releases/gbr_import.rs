//! The one-time import from Github-Build-Release (the Electron app this page
//! replaces): its last project joins the page and its DeepSeek key moves
//! into MYLE's sealed storage. The key is then removed from that app's
//! plain-text settings file, and the page says so.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::store::{GbrImport, Settings};

/// `%APPDATA%\ThomasThanos\GithubReleaseManager\grm-config.json`.
pub(crate) fn config_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ThomasThanos")
            .join("GithubReleaseManager")
            .join("grm-config.json"),
    )
}

/// The top folder of the git repository `path` is in.
fn repo_root(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Imports from the settings file at `config`. `has_key` tells whether
/// MYLE has a DeepSeek key already; `save_key` seals one.
pub(crate) fn import_from(
    config: &Path,
    settings: &mut Settings,
    has_key: bool,
    save_key: impl Fn(&str) -> Result<(), String>,
) -> Option<GbrImport> {
    let text = std::fs::read_to_string(config).ok()?;
    let mut report = GbrImport {
        at: super::unix_millis(),
        config_path: Some(config.display().to_string()),
        ..GbrImport::default()
    };
    let Ok(Value::Object(mut json)) = serde_json::from_str::<Value>(&text) else {
        report.error = Some("Github-Build-Release's settings could not be read.".into());
        return Some(report);
    };
    if let Some(project) = json.get("lastProjectPath").and_then(Value::as_str)
        && let Some(root) = repo_root(Path::new(project))
        && settings.add_repo(&root).is_ok()
    {
        report.project = Some(root.display().to_string());
    }
    let key = json
        .get("deepseekApiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string);
    if let Some(key) = key {
        let sealed = if has_key { Ok(()) } else { save_key(&key) };
        match sealed {
            Ok(()) => {
                report.deepseek_key = !has_key;
                json.remove("deepseekApiKey");
                let cleaned = serde_json::to_vec_pretty(&Value::Object(json)).unwrap_or_default();
                match crate::game_saves::atomic::write(config, &cleaned) {
                    Ok(()) => report.plaintext_removed = true,
                    Err(error) => {
                        report.error = Some(format!(
                            "The key is now kept safely by MYLE, but it could not be removed from Github-Build-Release's file: {error}"
                        ))
                    }
                }
            }
            Err(error) => {
                report.error = Some(format!("The DeepSeek key could not be stored: {error}"))
            }
        }
    }
    (report.project.is_some()
        || report.deepseek_key
        || report.plaintext_removed
        || report.error.is_some())
    .then_some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn the_last_project_and_key_come_over_and_the_plain_key_is_removed() {
        let dir = std::env::temp_dir().join(format!("myle-gr-gbr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("app")).unwrap();
        let config = dir.join("grm-config.json");
        let project = repo.join("app").display().to_string().replace('\\', "\\\\");
        std::fs::write(&config, format!("{{\"lastProjectPath\":\"{project}\",\"deepseekApiKey\":\"sk-test-123456\",\"theme\":\"dark\"}}")).unwrap();
        let mut settings = Settings::default();
        let saved = RefCell::new(None);
        let report = import_from(&config, &mut settings, false, |key| {
            *saved.borrow_mut() = Some(key.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(saved.borrow().as_deref(), Some("sk-test-123456"));
        assert!(report.deepseek_key && report.plaintext_removed);
        assert_eq!(settings.repos.len(), 1);
        assert!(super::super::store::same_path(
            Path::new(&settings.repos[0].path),
            &repo
        ));
        let left = std::fs::read_to_string(&config).unwrap();
        assert!(!left.contains("sk-test"));
        assert!(left.contains("\"theme\""));
        // Nothing more to bring the second time.
        assert!(
            import_from(&config, &mut settings, true, |_| Ok(())).is_some_and(|r| !r.deepseek_key)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
