use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

use super::models::GameSavesSettings;
use super::{atomic, settings};

const ENGINE_NAME: &str = "ludusavi.exe";

/// `ludusavi --version` per executable. The page asks for it on every visit,
/// and the bundled engine only changes with an app update.
static VERSION: Mutex<Option<(PathBuf, Option<String>)>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    executable: PathBuf,
    config_dir: PathBuf,
    bundled_manifest: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct EngineOutput {
    pub stdout: String,
}

/// Output of a run whatever its exit code: some failures (unknown game names)
/// still describe themselves in the JSON on stdout.
#[derive(Debug)]
pub(crate) struct RawOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Engine {
    pub(crate) fn for_app(app: &AppHandle) -> Result<Self, String> {
        let resource = app.path().resource_dir().ok();
        let executable = resolve_executable(resource.as_deref())?;
        let bundled_manifest = resolve_manifest(resource.as_deref());
        Ok(Self {
            executable,
            config_dir: settings::engine_config_dir(app)?,
            bundled_manifest,
        })
    }

    pub(crate) fn headless(config_root: PathBuf) -> Result<Self, String> {
        let executable = resolve_executable(None)?;
        let bundled_manifest = resolve_manifest(None);
        Ok(Self {
            executable,
            config_dir: config_root.join("ludusavi"),
            bundled_manifest,
        })
    }

    pub(crate) fn manifest_path(&self) -> PathBuf {
        self.config_dir.join("manifest.yaml")
    }

    pub(crate) fn prepare(&self, settings: &GameSavesSettings) -> Result<(), String> {
        fs::create_dir_all(&self.config_dir).map_err(|e| e.to_string())?;
        self.seed_manifest()?;
        write_atomic(
            &self.config_dir.join("config.yaml"),
            render_config(settings).as_bytes(),
        )
    }

    fn seed_manifest(&self) -> Result<(), String> {
        let destination = self.config_dir.join("manifest.yaml");
        if destination.exists() {
            return Ok(());
        }
        let Some(source) = &self.bundled_manifest else {
            return Ok(());
        };
        if source.is_file() {
            let bytes = fs::read(source).map_err(|e| e.to_string())?;
            atomic::write(&destination, &bytes)?;
        }
        Ok(())
    }

    pub(crate) async fn version(&self) -> Option<String> {
        if let Some((path, version)) = VERSION.lock().unwrap_or_else(|p| p.into_inner()).clone()
            && path == self.executable
        {
            return version;
        }
        let version = self.query_version().await;
        *VERSION.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((self.executable.clone(), version.clone()));
        version
    }

    async fn query_version(&self) -> Option<String> {
        let output = crate::apps::process::hidden(&self.executable)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;
        output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_start_matches("ludusavi ")
                .to_string()
        })
    }

    /// Runs the engine; a non-zero exit is an error carrying its last message.
    /// `on_spawn(pid, running)` is told when the process starts and ends.
    ///
    /// With `--api`, a non-zero exit that still printed its JSON report is a
    /// partial result, not a failure: Ludusavi exits with an error when *some*
    /// files could not be processed, after handling all the others. The
    /// report says which games failed and why, per file.
    pub(crate) async fn run(
        &self,
        args: &[String],
        stdin: Option<&str>,
        on_spawn: &mut (dyn FnMut(u32, bool) + Send),
    ) -> Result<EngineOutput, String> {
        let output = self.run_raw(args, stdin, on_spawn).await?;
        if !output.success {
            let partial = args.iter().any(|arg| arg == "--api")
                && crate::game_saves::parser::parse_api(&output.stdout)
                    .is_ok_and(|api| !api.games.is_empty());
            if !partial {
                return Err(failure_detail(&output.stderr, &output.stdout).to_string());
            }
        }
        Ok(EngineOutput {
            stdout: output.stdout,
        })
    }

    pub(crate) async fn run_raw(
        &self,
        args: &[String],
        stdin: Option<&str>,
        on_spawn: &mut (dyn FnMut(u32, bool) + Send),
    ) -> Result<RawOutput, String> {
        let mut command = crate::apps::process::hidden(&self.executable);
        command
            .arg("--config")
            .arg(&self.config_dir)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|e| {
            format!(
                "Could not start the Game Saves engine ({}): {e}",
                self.executable.display()
            )
        })?;
        let pid = child.id();
        if let Some(pid) = pid {
            on_spawn(pid, true);
        }
        let mut finished = |pid: Option<u32>| {
            if let Some(pid) = pid {
                on_spawn(pid, false);
            }
        };

        if let Some(input) = stdin
            && let Some(mut pipe) = child.stdin.take()
            && let Err(error) = pipe.write_all(input.as_bytes()).await
        {
            let _ = child.kill().await;
            finished(pid);
            return Err(format!(
                "Could not send games to the backup engine: {error}"
            ));
        }

        let output = child.wait_with_output().await;
        finished(pid);
        let output = output.map_err(|e| e.to_string())?;
        Ok(RawOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    pub(crate) fn backup_preview_args(backup_folder: &str) -> Vec<String> {
        vec![
            "--no-manifest-update".into(),
            "backup".into(),
            "--preview".into(),
            "--api".into(),
            "--path".into(),
            backup_folder.into(),
        ]
    }

    pub(crate) fn restore_preview_args(backup_folder: &str, snapshot: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--no-manifest-update".into(),
            "restore".into(),
            "--preview".into(),
            "--api".into(),
            "--path".into(),
            backup_folder.into(),
        ];
        if let Some(snapshot) = snapshot.filter(|id| !id.is_empty() && *id != "latest") {
            args.push("--backup".into());
            args.push(snapshot.into());
        }
        args
    }

    pub(crate) fn backups_args(backup_folder: &str) -> Vec<String> {
        vec![
            "--no-manifest-update".into(),
            "backups".into(),
            "--api".into(),
            "--path".into(),
            backup_folder.into(),
        ]
    }

    pub(crate) fn backup_args(backup_folder: &str) -> Vec<String> {
        vec![
            "--no-manifest-update".into(),
            "backup".into(),
            "--force".into(),
            "--api".into(),
            "--no-cloud-sync".into(),
            "--format".into(),
            "zip".into(),
            "--compression".into(),
            "deflate".into(),
            "--compression-level".into(),
            "6".into(),
            "--full-limit".into(),
            "3".into(),
            "--differential-limit".into(),
            "0".into(),
            "--path".into(),
            backup_folder.into(),
        ]
    }

    pub(crate) fn restore_args(backup_folder: &str, snapshot: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--no-manifest-update".into(),
            "restore".into(),
            "--force".into(),
            "--api".into(),
            "--no-cloud-sync".into(),
            "--path".into(),
            backup_folder.into(),
        ];
        if let Some(snapshot) = snapshot.filter(|id| !id.is_empty() && *id != "latest") {
            args.push("--backup".into());
            args.push(snapshot.into());
        }
        args
    }

    pub(crate) fn manifest_update_args() -> Vec<String> {
        vec!["manifest".into(), "update".into(), "--force".into()]
    }
}

pub(crate) fn failure_detail<'a>(stderr: &'a str, stdout: &'a str) -> &'a str {
    stderr
        .lines()
        .rev()
        .find(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("note: run with `RUST_BACKTRACE=")
        })
        .or_else(|| stdout.lines().rev().find(|line| !line.trim().is_empty()))
        .map(str::trim)
        .unwrap_or("The backup engine stopped unexpectedly.")
}

fn resolve_executable(resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("MYLE_LUDUSAVI") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    let mut candidates = Vec::new();
    if let Some(resource) = resource_dir {
        candidates.push(resource.join("ludusavi").join(ENGINE_NAME));
        candidates.push(resource.join(ENGINE_NAME));
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        candidates.push(dir.join("resources").join("ludusavi").join(ENGINE_NAME));
        candidates.push(dir.join("ludusavi").join(ENGINE_NAME));
        candidates.push(dir.join(ENGINE_NAME));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("ludusavi")
            .join(ENGINE_NAME),
    );
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "The bundled Game Saves engine is missing. Reinstall the application.".into()
        })
}

fn resolve_manifest(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(resource) = resource_dir {
        candidates.push(resource.join("ludusavi").join("manifest.yaml"));
    }
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        candidates.push(dir.join("resources").join("ludusavi").join("manifest.yaml"));
        candidates.push(dir.join("ludusavi").join("manifest.yaml"));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("ludusavi")
            .join("manifest.yaml"),
    );
    candidates.into_iter().find(|path| path.is_file())
}

fn render_config(settings: &GameSavesSettings) -> String {
    let backup = settings.backup_folder.as_deref().unwrap_or("");
    let mut out =
        "---\nlanguage: en-US\nrelease:\n  check: false\nmanifest:\n  enable: true\n".to_string();
    if settings.roots.is_empty() {
        out.push_str("roots: []\n");
    } else {
        out.push_str("roots:\n");
        for root in &settings.roots {
            out.push_str(&format!(
                "  - path: {}\n    store: {}\n",
                yaml_string(&glob_escape(&root.path)),
                root.store.ludusavi_name()
            ));
        }
    }
    out.push_str(&format!(
        "backup:\n  path: {}\n  format:\n    chosen: zip\n    zip:\n      compression: deflate\n    compression:\n      deflate:\n        level: 6\n  retention:\n    full: 3\n    differential: 0\nrestore:\n  path: {}\n",
        yaml_string(backup),
        yaml_string(backup)
    ));

    if settings.path_mappings.is_empty() {
        out.push_str("redirects: []\n");
    } else {
        out.push_str("redirects:\n");
        for mapping in &settings.path_mappings {
            out.push_str(&format!(
                "  - kind: restore\n    source: {}\n    target: {}\n",
                yaml_string(&mapping.source),
                yaml_string(&mapping.target)
            ));
        }
    }

    if settings.custom_games.is_empty() {
        out.push_str("customGames: []\n");
    } else {
        out.push_str("customGames:\n");
        for game in &settings.custom_games {
            out.push_str(&format!(
                "  - name: {}\n    integration: override\n    ignore: false\n    files:\n",
                yaml_string(&game.name)
            ));
            for path in &game.paths {
                out.push_str(&format!("      - {}\n", yaml_string(&glob_escape(path))));
            }
            out.push_str("    registry: []\n");
            if let Some(path) = game.install_path.as_deref() {
                let name = Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path);
                out.push_str(&format!("    installDir:\n      - {}\n", yaml_string(name)));
            } else {
                out.push_str("    installDir: []\n");
            }
        }
    }
    out
}

/// The engine reads root and custom-game paths as glob patterns: a folder
/// named `Games [SSD]` would be a character class and match nothing. Each
/// pattern character is written as a class of its own (`[[]`), matching
/// itself. Only the engine's config changes; the settings keep the path.
fn glob_escape(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for character in path.chars() {
        match character {
            '[' | ']' | '*' | '?' | '{' | '}' => {
                escaped.push('[');
                escaped.push(character);
                escaped.push(']');
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_saves::models::{CustomGame, RestorePathMapping};

    #[test]
    fn folders_with_brackets_are_matched_as_they_are_named() {
        assert_eq!(glob_escape(r"D:\Games [SSD]\SteamLibrary"), r"D:\Games [[]SSD[]]\SteamLibrary");
        assert_eq!(glob_escape(r"C:\Saves {x}"), r"C:\Saves [{]x[}]");
        assert_eq!(glob_escape(r"C:\Plain\Folder"), r"C:\Plain\Folder");
        let settings = GameSavesSettings {
            roots: vec![crate::game_saves::models::GameRoot {
                id: "r".into(),
                path: r"D:\Games [SSD]".into(),
                store: crate::game_saves::models::RootStore::Steam,
                source: crate::game_saves::models::RootSource::Manual,
            }],
            ..GameSavesSettings::default()
        };
        let config = render_config(&settings);
        assert!(config.contains(r"'D:\Games [[]SSD[]]'"), "{config}");
        assert_eq!(settings.roots[0].path, r"D:\Games [SSD]", "the settings keep the folder's real name");
    }

    #[test]
    fn yaml_quotes_apostrophes_and_keeps_windows_slashes_literal() {
        assert_eq!(yaml_string(r"C:\User's\Saves"), r"'C:\User''s\Saves'");
    }

    #[test]
    fn config_contains_roots_custom_games_redirects_and_three_snapshots() {
        let mut settings = GameSavesSettings {
            backup_folder: Some(r"D:\Backups".into()),
            ..Default::default()
        };
        settings.custom_games.push(CustomGame {
            id: "custom-x".into(),
            name: "My Game".into(),
            paths: vec![r"C:\Saves\My Game".into()],
            install_path: Some(r"D:\Games\My Game".into()),
            auto_backup: true,
        });
        settings.path_mappings.push(RestorePathMapping {
            game_id: "x".into(),
            source: r"C:\Old".into(),
            target: r"D:\New".into(),
        });
        let yaml = render_config(&settings);
        assert!(yaml.contains("full: 3"));
        assert!(yaml.contains("level: 6"));
        assert!(yaml.contains("name: 'My Game'"));
        assert!(yaml.contains("source: 'C:\\Old'"));
    }

    #[test]
    fn backup_cli_is_offline_zip_deflate_six_with_three_full_snapshots() {
        let args = Engine::backup_args(r"D:\Backups");
        let joined = args.join(" ");
        assert!(joined.contains("--no-manifest-update"));
        assert!(joined.contains("--format zip"));
        assert!(joined.contains("--compression deflate"));
        assert!(joined.contains("--compression-level 6"));
        assert!(joined.contains("--full-limit 3"));
        assert!(joined.contains("--differential-limit 0"));
        assert!(joined.contains("--no-cloud-sync"));
    }

    #[test]
    fn engine_errors_do_not_hide_the_cause_behind_a_backtrace_hint() {
        let stderr = "thread panicked\nactual failure\nnote: run with `RUST_BACKTRACE=1` environment variable";
        assert_eq!(failure_detail(stderr, ""), "actual failure");
    }
}
