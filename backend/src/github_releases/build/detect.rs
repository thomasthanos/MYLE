//! The build commands a project offers: its package.json scripts (with the
//! package manager its lock file names), `tauri build`, `cargo build`,
//! `dotnet publish`, PyInstaller or `python -m build`, `go build`,
//! `flutter build`.

use std::path::Path;

use serde::Serialize;

use super::super::project;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildOption {
    /// `script:build`, `tauri`, `cargo`, …
    pub id: String,
    /// The command line, as run in the project folder.
    pub command: String,
    /// What it is, in a few words.
    pub label: String,
    /// The script's text, for a package.json script.
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlan {
    pub options: Vec<BuildOption>,
    /// `npm`, `pnpm`, `yarn`, `bun`, for a Node project.
    pub package_manager: Option<String>,
    /// node_modules is missing: dependencies need installing first.
    pub needs_install: bool,
    /// The command that installs them.
    pub install_command: Option<String>,
}

/// The package manager of the project in `dir`, from the lock file there or
/// in a folder above it up to `root` (a monorepo keeps one at the top).
pub(crate) fn package_manager(dir: &Path, root: &Path) -> &'static str {
    let mut current = Some(dir);
    while let Some(folder) = current {
        if folder.join("pnpm-lock.yaml").is_file() {
            return "pnpm";
        }
        if folder.join("yarn.lock").is_file() {
            return "yarn";
        }
        if folder.join("bun.lockb").is_file() || folder.join("bun.lock").is_file() {
            return "bun";
        }
        if folder.join("package-lock.json").is_file() {
            return "npm";
        }
        if folder == root {
            break;
        }
        current = folder.parent();
    }
    "npm"
}

/// Scripts that build something to ship, best first.
const SCRIPTS: &[&str] = &[
    "build-all",
    "build:all",
    "release",
    "dist",
    "build:win",
    "package",
    "make",
    "build:prod",
    "build",
];

pub(crate) fn plan(dir: &Path, root: &Path) -> BuildPlan {
    let mut plan = BuildPlan::default();
    let tauri = project::tauri_dir(dir);
    if let Some(json) = project::package_json(dir) {
        let pm = package_manager(dir, root);
        plan.package_manager = Some(pm.into());
        let run = |script: &str| match pm {
            "npm" => format!("npm run {script}"),
            other => format!("{other} run {script}"),
        };
        let scripts = json.get("scripts").and_then(|s| s.as_object());
        let mut has_tauri_build = false;
        if let Some(scripts) = scripts {
            for name in SCRIPTS {
                if let Some(text) = scripts.get(*name).and_then(|v| v.as_str()) {
                    has_tauri_build |= text.contains("tauri build");
                    plan.options.push(BuildOption {
                        id: format!("script:{name}"),
                        command: run(name),
                        label: format!("The \"{name}\" script"),
                        detail: Some(text.to_string()),
                    });
                }
            }
        }
        if tauri.is_some() && !has_tauri_build {
            let has_cli = project::dependencies(&json)
                .iter()
                .any(|d| d == "@tauri-apps/cli");
            let tauri_script = scripts.is_some_and(|s| s.contains_key("tauri"));
            let command = if tauri_script {
                format!("{} tauri build", if pm == "npm" { "npm run" } else { pm })
            } else if has_cli {
                "npx tauri build".to_string()
            } else {
                "cargo tauri build".to_string()
            };
            plan.options.push(BuildOption {
                id: "tauri".into(),
                command,
                label: "Tauri app and installer".into(),
                detail: None,
            });
        }
        let node_modules = [dir, root].iter().any(|d| d.join("node_modules").is_dir());
        plan.needs_install = !node_modules && !plan.options.is_empty();
        plan.install_command = Some(match pm {
            "npm" if dir.join("package-lock.json").is_file() => "npm ci".into(),
            other => format!("{other} install"),
        });
    } else if tauri.is_some() {
        plan.options.push(BuildOption {
            id: "tauri".into(),
            command: "cargo tauri build".into(),
            label: "Tauri app and installer".into(),
            detail: None,
        });
    }
    if dir.join("Cargo.toml").is_file() && tauri.as_deref() != Some(dir) {
        plan.options.push(BuildOption {
            id: "cargo".into(),
            command: "cargo build --release".into(),
            label: "Rust, release build".into(),
            detail: None,
        });
    }
    let files: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    let with_ext = |ext: &str| {
        files
            .iter()
            .find(|name| name.to_lowercase().ends_with(ext))
            .cloned()
    };
    if let Some(solution) = with_ext(".sln").or_else(|| with_ext(".csproj")) {
        plan.options.push(BuildOption {
            id: "dotnet".into(),
            command: format!("dotnet publish \"{solution}\" -c Release"),
            label: ".NET, published in Release".into(),
            detail: None,
        });
    }
    if let Some(spec) = with_ext(".spec") {
        plan.options.push(BuildOption {
            id: "pyinstaller".into(),
            command: format!("pyinstaller --noconfirm \"{spec}\""),
            label: "PyInstaller executable".into(),
            detail: None,
        });
    }
    if dir.join("pyproject.toml").is_file() {
        plan.options.push(BuildOption {
            id: "python".into(),
            command: "python -m build".into(),
            label: "Python wheel and source package".into(),
            detail: None,
        });
    }
    if dir.join("go.mod").is_file() {
        plan.options.push(BuildOption {
            id: "go".into(),
            command: "go build -o dist/ ./...".into(),
            label: "Go programs into dist".into(),
            detail: None,
        });
    }
    if dir.join("pubspec.yaml").is_file() {
        plan.options.push(BuildOption {
            id: "flutter".into(),
            command: "flutter build windows --release".into(),
            label: "Flutter for Windows".into(),
            detail: None,
        });
    }
    plan
}

/// A command the user typed: one line, nothing that would make it more
/// than one command by surprise.
pub(crate) fn check_command(command: &str) -> Result<String, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Type the command that builds the project.".into());
    }
    if command.len() > 1000 || command.contains(['\n', '\r', '\0']) {
        return Err("The build command must be a single line.".into());
    }
    Ok(command.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("myle-gr-detect-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scripts_package_managers_and_tauri() {
        let dir = temp("node");
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"dev":"vite","build":"vite build","build-all":"npm run build && electron-builder"},"devDependencies":{"@tauri-apps/cli":"2"}}"#).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(dir.join("src-tauri")).unwrap();
        std::fs::write(dir.join("src-tauri/tauri.conf.json"), "{}").unwrap();
        let plan = plan(&dir, &dir);
        let commands: Vec<&str> = plan.options.iter().map(|o| o.command.as_str()).collect();
        assert_eq!(
            commands,
            ["pnpm run build-all", "pnpm run build", "npx tauri build"]
        );
        assert_eq!(plan.package_manager.as_deref(), Some("pnpm"));
        assert!(plan.needs_install);
        assert_eq!(plan.install_command.as_deref(), Some("pnpm install"));
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        assert!(!super::plan(&dir, &dir).needs_install);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_monorepo_app_uses_the_lock_file_at_the_top_and_other_languages() {
        let root = temp("mono");
        std::fs::write(root.join("yarn.lock"), "").unwrap();
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        assert_eq!(package_manager(&app, &root), "yarn");
        std::fs::write(app.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(app.join("Tool.sln"), "").unwrap();
        std::fs::write(app.join("pyproject.toml"), "").unwrap();
        let plan = plan(&app, &root);
        let ids: Vec<&str> = plan.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["cargo", "dotnet", "python"]);
        assert_eq!(
            plan.options[1].command,
            "dotnet publish \"Tool.sln\" -c Release"
        );
        assert!(check_command("npm run build").is_ok());
        assert!(check_command("a\nb").is_err());
        assert!(check_command("  ").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
