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
    /// What it runs, in order: a script's chain of commands and scripts, and
    /// the named steps of a script file it starts (`build-setup.ps1`).
    pub steps: Vec<BuildStep>,
    /// The project's whole build (its own top-level build script), the one
    /// to run before a release.
    pub full: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep {
    pub title: String,
    pub command: Option<String>,
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

/// The script a command line starts through a package manager
/// (`npm run x`, `pnpm x`, `yarn run x`, `bun run x`), when it is one of
/// `scripts`.
fn script_called<'a>(
    part: &'a str,
    scripts: &serde_json::Map<String, serde_json::Value>,
) -> Option<&'a str> {
    let words: Vec<&str> = part.split_whitespace().collect();
    let name = match words.as_slice() {
        [
            "npm" | "pnpm" | "yarn" | "bun",
            "run" | "run-script",
            name,
            ..,
        ] => *name,
        ["pnpm" | "yarn", name, ..] => *name,
        _ => return None,
    };
    scripts.contains_key(name).then_some(name)
}

/// A script file a command line starts (`./scripts/build-setup.ps1`), inside
/// `dir`.
fn file_called(part: &str, dir: &Path) -> Option<std::path::PathBuf> {
    const KINDS: &[&str] = &[".ps1", ".mjs", ".cjs", ".js", ".sh", ".py", ".cmd", ".bat"];
    part.split_whitespace()
        .map(|word| word.trim_matches(|c| c == '"' || c == '\''))
        .filter(|word| !word.contains("..") && !word.starts_with('/') && !word.contains(':'))
        .find(|word| {
            KINDS
                .iter()
                .any(|kind| word.to_ascii_lowercase().ends_with(kind))
        })
        .map(|word| dir.join(word.trim_start_matches("./").replace('\\', "/")))
        .filter(|path| path.is_file())
}

fn short(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        text
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}

/// The steps of a script file: its named steps (`Invoke-Step "App" { … }`,
/// `echo "==> App"`), else the build tools it runs.
pub(crate) fn file_steps(text: &str) -> Vec<BuildStep> {
    static NAMED: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    static ARROW: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    static TOOL: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    let named = NAMED.get_or_init(|| {
        regex_lite::Regex::new(
            r#"(?m)^\s*[A-Z][A-Za-z]*-[A-Z][A-Za-z]*\s+["']([^"'\n]+)["']\s*\{([^}\n]*)\}?"#,
        )
        .unwrap()
    });
    let arrow = ARROW.get_or_init(|| {
        regex_lite::Regex::new(
            r#"(?m)(?:echo|Write-Host|console\.log\(|print\()\s*["'`]==>\s*([^"'`\n]+)"#,
        )
        .unwrap()
    });
    let tool = TOOL.get_or_init(|| {
        regex_lite::Regex::new(r"(?m)\b(tauri build[^\n;|}]*|vite build[^\n;|}]*|cargo build[^\n;|}]*|electron-builder[^\n;|}]*|makensis[^\n;|}]*|dotnet publish[^\n;|}]*|pyinstaller[^\n;|}]*|wix[^\n;|}]*|signtool[^\n;|}]*)").unwrap()
    });
    let title_of = |raw: &str| {
        // "App $version" reads "App".
        let words: Vec<&str> = raw
            .split_whitespace()
            .filter(|w| !w.starts_with('$'))
            .collect();
        short(&words.join(" "), 60)
    };
    let mut steps: Vec<BuildStep> = named
        .captures_iter(text)
        .filter(|c| !c[0].trim_start().starts_with("function"))
        .map(|c| BuildStep {
            title: title_of(&c[1]),
            command: c
                .get(2)
                .map(|m| short(m.as_str().trim(), 90))
                .filter(|m| !m.is_empty()),
        })
        .filter(|step| !step.title.is_empty())
        .collect();
    if steps.is_empty() {
        steps = arrow
            .captures_iter(text)
            .map(|c| BuildStep {
                title: title_of(&c[1]),
                command: None,
            })
            .filter(|step| !step.title.is_empty() && !step.title.starts_with("Done"))
            .collect();
    }
    if steps.is_empty() {
        for c in tool.captures_iter(text) {
            let command = short(c[1].trim(), 90);
            if !steps
                .iter()
                .any(|s| s.command.as_deref() == Some(command.as_str()))
            {
                steps.push(BuildStep {
                    title: command.split(' ').take(2).collect::<Vec<_>>().join(" "),
                    command: Some(command),
                });
            }
        }
    }
    steps.truncate(30);
    steps
}

/// What a package.json script runs, in order, with the scripts and script
/// files it calls opened up (a few levels deep).
fn script_steps(
    name: &str,
    scripts: &serde_json::Map<String, serde_json::Value>,
    dir: &Path,
    depth: usize,
) -> Vec<BuildStep> {
    let Some(text) = scripts.get(name).and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    for part in text.split("&&").map(str::trim).filter(|p| !p.is_empty()) {
        if let Some(inner) =
            script_called(part, scripts).filter(|inner| *inner != name && depth < 4)
        {
            let more = script_steps(inner, scripts, dir, depth + 1);
            if more.is_empty() {
                steps.push(BuildStep {
                    title: inner.to_string(),
                    command: Some(short(part, 90)),
                });
            } else {
                steps.extend(more);
            }
            continue;
        }
        if let Some(file) = file_called(part, dir) {
            let inner = std::fs::metadata(&file)
                .ok()
                .filter(|m| m.len() < 1024 * 1024)
                .and_then(|_| std::fs::read_to_string(&file).ok())
                .map(|text| file_steps(&text))
                .unwrap_or_default();
            if !inner.is_empty() {
                steps.extend(inner);
                continue;
            }
        }
        steps.push(BuildStep {
            title: short(
                part.split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .as_str(),
                40,
            ),
            command: Some(short(part, 90)),
        });
    }
    steps
}

/// Whether Tauri makes installers for the project (`bundle.active`).
fn tauri_bundles(tauri_dir: &Path) -> bool {
    std::fs::read_to_string(tauri_dir.join("tauri.conf.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("bundle")?.get("active")?.as_bool())
        .unwrap_or(true)
}

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
                    let steps = script_steps(name, scripts, dir, 0);
                    has_tauri_build |= text.contains("tauri build")
                        || steps.iter().any(|s| {
                            s.command
                                .as_deref()
                                .is_some_and(|c| c.contains("tauri build"))
                        });
                    let full = plan.options.is_empty();
                    plan.options.push(BuildOption {
                        id: format!("script:{name}"),
                        command: run(name),
                        label: if full {
                            format!(
                                "The project's build (\"{name}\" script, {} step{})",
                                steps.len(),
                                if steps.len() == 1 { "" } else { "s" }
                            )
                        } else {
                            format!("The \"{name}\" script")
                        },
                        detail: Some(text.to_string()),
                        steps,
                        full,
                    });
                }
            }
        }
        if let Some(tauri) = tauri.as_deref().filter(|_| !has_tauri_build) {
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
            plan.options
                .push(tauri_option(command, tauri, plan.options.is_empty()));
        }
        let node_modules = [dir, root].iter().any(|d| d.join("node_modules").is_dir());
        plan.needs_install = !node_modules && !plan.options.is_empty();
        plan.install_command = Some(match pm {
            "npm" if dir.join("package-lock.json").is_file() => "npm ci".into(),
            other => format!("{other} install"),
        });
    } else if let Some(tauri) = tauri.as_deref() {
        plan.options
            .push(tauri_option("cargo tauri build".into(), tauri, true));
    }
    if dir.join("Cargo.toml").is_file() && tauri.as_deref() != Some(dir) {
        plan.options.push(BuildOption {
            id: "cargo".into(),
            command: "cargo build --release".into(),
            label: "Rust, release build".into(),
            detail: None,
            steps: Vec::new(),
            full: false,
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
            steps: Vec::new(),
            full: false,
        });
    }
    if let Some(spec) = with_ext(".spec") {
        plan.options.push(BuildOption {
            id: "pyinstaller".into(),
            command: format!("pyinstaller --noconfirm \"{spec}\""),
            label: "PyInstaller executable".into(),
            detail: None,
            steps: Vec::new(),
            full: false,
        });
    }
    if dir.join("pyproject.toml").is_file() {
        plan.options.push(BuildOption {
            id: "python".into(),
            command: "python -m build".into(),
            label: "Python wheel and source package".into(),
            detail: None,
            steps: Vec::new(),
            full: false,
        });
    }
    if dir.join("go.mod").is_file() {
        plan.options.push(BuildOption {
            id: "go".into(),
            command: "go build -o dist/ ./...".into(),
            label: "Go programs into dist".into(),
            detail: None,
            steps: Vec::new(),
            full: false,
        });
    }
    if dir.join("pubspec.yaml").is_file() {
        plan.options.push(BuildOption {
            id: "flutter".into(),
            command: "flutter build windows --release".into(),
            label: "Flutter for Windows".into(),
            detail: None,
            steps: Vec::new(),
            full: false,
        });
    }
    // Without a build script, the first detected command is the build
    // (not Tauri when it makes the app alone).
    if !plan.options.iter().any(|o| o.full)
        && let Some(first) = plan
            .options
            .iter_mut()
            .find(|o| o.id != "tauri" || o.steps.first().is_some_and(|s| s.title != "App only"))
    {
        first.full = true;
    }
    plan
}

fn tauri_option(command: String, tauri_dir: &Path, full: bool) -> BuildOption {
    let bundles = tauri_bundles(tauri_dir);
    BuildOption {
        id: "tauri".into(),
        steps: vec![BuildStep {
            title: if bundles {
                "App and installers"
            } else {
                "App only"
            }
            .into(),
            command: Some(command.clone()),
        }],
        command,
        label: if bundles {
            "Tauri app and installer".into()
        } else {
            "Tauri app only (no installer: bundling is off in tauri.conf.json)".into()
        },
        detail: None,
        full: full && bundles,
    }
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

    #[test]
    fn the_projects_own_build_script_is_the_full_pipeline() {
        // MYLE: "build" runs scripts/build-setup.ps1, which runs tauri build
        // itself; Tauri's bundling is off, so `tauri build` alone makes only
        // the exe and must not be offered as "app and installer".
        let dir = temp("myle");
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"tauri":"tauri","dev":"tauri dev","build":"powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/build-setup.ps1"},"devDependencies":{"@tauri-apps/cli":"2"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("package-lock.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("backend")).unwrap();
        std::fs::write(
            dir.join("backend/tauri.conf.json"),
            r#"{"bundle":{"active":false}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("scripts/build-setup.ps1"),
            "function Invoke-Step([string]$Title, [scriptblock]$Command) {\n  Write-Host \"==> $Title\"\n}\n\
             Invoke-Step \"App $version\" { npx tauri build --no-bundle }\n\
             Invoke-Step \"Setup window\" { npx vite build --mode setup }\n\
             Invoke-Step \"Uninstaller\" {\n  cargo build --release -p myle-setup\n}\n\
             Invoke-Step \"Payload\" { & $pack $stage }\n\
             Invoke-Step \"Setup\" {\n  cargo build --release -p myle-setup --bin setup\n}\n",
        )
        .unwrap();
        let plan = plan(&dir, &dir);
        let ids: Vec<&str> = plan.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["script:build"]);
        let build = &plan.options[0];
        assert!(build.full);
        assert_eq!(build.command, "npm run build");
        let titles: Vec<&str> = build.steps.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["App", "Setup window", "Uninstaller", "Payload", "Setup"]
        );
        assert_eq!(
            build.steps[0].command.as_deref(),
            Some("npx tauri build --no-bundle")
        );
        assert!(build.label.contains("5 steps"));

        // Without such a script, tauri build says what it makes.
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"tauri":"tauri"},"devDependencies":{"@tauri-apps/cli":"2"}}"#,
        )
        .unwrap();
        let plan = super::plan(&dir, &dir);
        assert_eq!(plan.options[0].id, "tauri");
        assert!(
            plan.options[0].label.contains("App only")
                || plan.options[0].label.contains("app only")
        );
        assert!(!plan.options[0].full);
        std::fs::write(
            dir.join("backend/tauri.conf.json"),
            r#"{"bundle":{"active":true}}"#,
        )
        .unwrap();
        let plan = super::plan(&dir, &dir);
        assert_eq!(plan.options[0].label, "Tauri app and installer");
        assert!(plan.options[0].full);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_chain_of_scripts_is_opened_up() {
        let dir = temp("chain");
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"build:ext":"node scripts/pack-extension.mjs","build:web":"vite build","build-all":"npm run build:web && npm run build:ext && electron-builder --win nsis zip"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("scripts/pack-extension.mjs"),
            "console.log(\"==> Extension zip\");\n",
        )
        .unwrap();
        let plan = plan(&dir, &dir);
        let all = &plan.options[0];
        assert_eq!(all.id, "script:build-all");
        assert!(all.full);
        let titles: Vec<&str> = all.steps.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["vite build", "Extension zip", "electron-builder --win nsis"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
