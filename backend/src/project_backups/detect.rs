//! Build output, dependencies and caches told apart from sources by the files
//! around a folder, for names that can be either.
//!
//! `target` is Rust's build output next to a `Cargo.toml` and Maven's next to
//! a `pom.xml`, but a deployment target or a game's "target" sprites
//! elsewhere; `bin` and `obj` are .NET output next to a `.csproj`, but
//! scripts or 3D models elsewhere; `build` holds electron-builder's installer
//! script and icons in an Electron project. So these names are never left out
//! by name alone: a marker decides (`folder`), and without one a `.gitignore`
//! or git itself (`Rules::ambiguous_output`). A folder git tracks files in is
//! always backed up, whatever it is called.

use super::rules::{Listing, Rules, parent_of};

/// The reason shown for a folder a `.gitignore` names (no marker found).
pub(crate) const IGNORED_OUTPUT: &str = "build output (ignored by .gitignore)";
/// The reason shown for a folder git tracks nothing in (no marker found).
pub(crate) const UNTRACKED_OUTPUT: &str = "build output (git tracks nothing in it)";

/// Names that are build output, dependencies or caches in some projects and
/// sources in others: left out when a `.gitignore` ignores them.
pub(crate) const AMBIGUOUS: &[&str] = &[
    "build",
    "builds",
    "out",
    "output",
    "target",
    "bin",
    "obj",
    "debug",
    "release",
    "x64",
    "x86",
    "arm64",
    "win32",
    "gen",
    "generated",
    "packages",
    "pods",
    "library",
    "temp",
    "tmp",
    "logs",
    "env",
    "venv",
    "vendor",
];

/// The commonest of them: also left out in a git work tree that tracks
/// nothing inside them (they were never committed: generated).
pub(crate) const UNTRACKED_MEANS_OUTPUT: &[&str] = &[
    "build", "out", "target", "bin", "obj", "debug", "release", "gen",
];

const DOTNET_PROJECTS: &[&str] = &[".csproj", ".fsproj", ".vbproj"];
const VISUAL_STUDIO: &[&str] = &[".sln", ".slnx", ".vcxproj", ".vcproj"];
const GRADLE: &[&str] = &[
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];
const PYTHON_PACKAGE: &[&str] = &["pyproject.toml", "setup.py", "setup.cfg"];
const ELECTRON_BUILDER: &[&str] = &[
    "electron-builder.yml",
    "electron-builder.yaml",
    "electron-builder.json",
    "electron-builder.json5",
    "electron-builder.toml",
    "electron-builder.config.js",
    "electron-builder.config.cjs",
];
const ELECTRON_FORGE: &[&str] = &[
    "forge.config.js",
    "forge.config.cjs",
    "forge.config.mjs",
    "forge.config.ts",
];
const NEXT_CONFIG: &[&str] = &["next.config.js", "next.config.mjs", "next.config.ts"];
const TAURI_CONFIG: &[&str] = &[
    "tauri.conf.json",
    "tauri.conf.json5",
    "tauri.conf.toml",
    "tauri.toml",
];
const C_SOURCES: &[&str] = &["c", "cc", "cpp", "cxx", "asm", "s"];

/// Why the folder `rel` (its lower-case `name`) is build output,
/// dependencies or a cache, if the files around it say so.
pub(crate) fn folder(
    rel: &str,
    name: &str,
    listing: &dyn Listing,
    rules: &Rules,
) -> Option<&'static str> {
    if let Some(own) = listing.names(rel) {
        // Markers a tool leaves inside its own folders, whatever their name.
        if own.has_file("cachedir.tag") {
            return Some(if name == "target" {
                "Rust build output (Cargo)"
            } else {
                "cache folder (CACHEDIR.TAG)"
            });
        }
        if own.has_file(".rustc_info.json") {
            return Some("Rust build output (Cargo)");
        }
        if own.has_file("pyvenv.cfg") {
            return Some("Python virtual environment (pyvenv.cfg)");
        }
        if own.has_dir("conda-meta") {
            return Some("Conda environment");
        }
        if own.has_file("cmakecache.txt") {
            return Some("CMake build folder (CMakeCache.txt)");
        }
    }
    let parent_rel = parent_of(rel);
    let parent = listing.names(parent_rel)?;
    let unity = parent.has_dir("assets") && parent.has_dir("projectsettings");
    match name {
        "target" if parent.has_file("cargo.toml") => Some("Rust build output (Cargo.toml)"),
        "target" if parent.has_file("pom.xml") => Some("Maven build output (pom.xml)"),
        "target" if parent.has_file("build.sbt") => Some("sbt build output (build.sbt)"),
        "build" if parent.has_any_file(GRADLE) => Some("Gradle build output"),
        "build" if parent.has_file("pubspec.yaml") => Some("Flutter / Dart build output"),
        "build" if parent.has_any_file(PYTHON_PACKAGE) => Some("Python build output"),
        "build"
            if parent.has_dir_ending(&[".xcodeproj", ".xcworkspace"])
                || parent.has_file("package.swift") =>
        {
            Some("Xcode build output")
        }
        "build" if parent.has_file("cmakelists.txt") => Some("CMake build folder"),
        "library" | "temp" | "obj" | "logs" | "usersettings" | "memorycaptures" | "build"
        | "builds"
            if unity =>
        {
            Some("Unity generated folder")
        }
        "bin" | "obj" if parent.has_file_ending(DOTNET_PROJECTS) => Some(".NET build output"),
        "bin" if parent.has_file("go.mod") => Some("Go build output (go.mod)"),
        "debug" | "release" | "x64" | "x86" | "arm64" | "win32"
            if parent.has_file_ending(VISUAL_STUDIO) =>
        {
            Some("Visual Studio build output")
        }
        "release" | "dist_electron"
            if parent.has_any_file(ELECTRON_BUILDER)
                || (parent.has_file("package.json")
                    && rules.file_mentions(parent_rel, "package.json", "electron-builder")) =>
        {
            Some("electron-builder output")
        }
        "out" if parent.has_any_file(ELECTRON_FORGE) => Some("Electron Forge output"),
        "out" if parent.has_any_file(NEXT_CONFIG) => Some("Next.js static export"),
        "out" if parent.has_file_ending(&[".iml"]) || parent.has_dir(".idea") => {
            Some("IntelliJ build output")
        }
        "out"
            if parent.has_file("cmakelists.txt")
                && parent.has_any_file(&["cmakepresets.json", "cmakesettings.json"]) =>
        {
            Some("Visual Studio CMake output")
        }
        "packages"
            if parent.has_file_ending(&[".sln", ".slnx"]) && nuget_packages(rel, listing) =>
        {
            Some("NuGet packages")
        }
        "pods" if parent.has_file("podfile") => Some("CocoaPods dependencies (Podfile)"),
        ".import" if parent.has_file("project.godot") => Some("Godot import cache"),
        "schemas"
            if parent_rel
                .rsplit('/')
                .next()
                .unwrap_or(parent_rel)
                .eq_ignore_ascii_case("gen")
                && listing
                    .names(parent_of(parent_rel))
                    .is_some_and(|grand| grand.has_any_file(TAURI_CONFIG)) =>
        {
            Some("Tauri generated schemas")
        }
        _ => None,
    }
}

/// A solution's `packages` folder: `repositories.config`, or folders named
/// like `Newtonsoft.Json.13.0.3`.
fn nuget_packages(rel: &str, listing: &dyn Listing) -> bool {
    let Some(own) = listing.names(rel) else {
        return false;
    };
    own.has_file("repositories.config")
        || own.dirs.iter().any(|dir| {
            dir.split('.')
                .skip(1)
                .any(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        })
}

/// Why the file `rel` (its lower-case `name`) is build output, if the files
/// next to it say so. `.obj` is a compiler's object file next to C/C++
/// sources or a Visual Studio project, and a 3D model elsewhere.
pub(crate) fn file(rel: &str, name: &str, listing: &dyn Listing) -> Option<&'static str> {
    let stem = name.strip_suffix(".obj")?;
    let parent = listing.names(parent_of(rel))?;
    let compiled = parent.has_file_ending(VISUAL_STUDIO)
        || parent.has_any_file(&["makefile", "cmakelists.txt"])
        || parent.has_file_ending(&[".tlog", ".mak"])
        || C_SOURCES
            .iter()
            .any(|ext| parent.has_file(&format!("{stem}.{ext}")));
    compiled.then_some("C/C++ object file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_backups::rules::{DEFAULT_PATTERNS, RuleInput, Rules};
    use crate::project_backups::walk::walk;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myle-detect-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Writes the files of a project (a path ending with `/` is an empty folder).
    fn project(name: &str, paths: &[&str]) -> PathBuf {
        let root = temp(name);
        for rel in paths {
            let path = root.join(rel.trim_end_matches('/'));
            if rel.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, format!("content of {rel}")).unwrap();
            }
        }
        root
    }

    struct Outcome {
        files: Vec<String>,
        excluded: Vec<(String, String)>,
    }

    impl Outcome {
        fn left_out(&self, rel: &str) -> &str {
            self.excluded
                .iter()
                .find(|(path, _)| path == rel)
                .map(|(_, why)| why.as_str())
                .unwrap_or_else(|| {
                    panic!("{rel} should be left out; left out: {:?}", self.excluded)
                })
        }

        fn backed_up(&self, rel: &str) {
            assert!(
                self.files.iter().any(|file| file == rel),
                "{rel} should be backed up; files: {:?}, left out: {:?}",
                self.files,
                self.excluded
            );
        }
    }

    fn backup_of(root: &Path) -> Outcome {
        let rules = Rules::compile(
            &RuleInput {
                patterns: DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect(),
                smart_build: true,
                ..RuleInput::default()
            },
            Some(root),
        )
        .unwrap();
        let found = walk(root, &rules, &[], &|| false).unwrap();
        let _ = fs::remove_dir_all(root);
        Outcome {
            files: found.files.into_iter().map(|file| file.rel).collect(),
            excluded: found
                .excluded
                .into_iter()
                .map(|item| (item.rel, item.why.describe()))
                .collect(),
        }
    }

    #[test]
    fn rust_workspaces_and_tauri() {
        let out = backup_of(&project(
            "rust",
            &[
                "Cargo.toml",
                "src/main.rs",
                ".env",
                "target/CACHEDIR.TAG",
                "target/debug/app.exe",
                // A workspace member with its own (older) target, no tag.
                "crates/core/Cargo.toml",
                "crates/core/src/lib.rs",
                "crates/core/target/debug/libcore.rlib",
                // Tauri: target, generated schemas; the mobile projects stay,
                // their Gradle output does not.
                "app/src-tauri/Cargo.toml",
                "app/src-tauri/tauri.conf.json",
                "app/src-tauri/target/release/app.exe",
                "app/src-tauri/gen/schemas/desktop-schema.json",
                "app/src-tauri/gen/android/build.gradle.kts",
                "app/src-tauri/gen/android/app/build.gradle.kts",
                "app/src-tauri/gen/android/app/build/outputs/app.apk",
                "app/src-tauri/gen/android/.gradle/cache.bin",
                // A game's "target" sprites, no Cargo.toml: sources.
                "game/target/crosshair.png",
            ],
        ));
        assert_eq!(out.left_out("target"), "Rust build output (Cargo)");
        assert_eq!(
            out.left_out("crates/core/target"),
            "Rust build output (Cargo.toml)"
        );
        assert_eq!(
            out.left_out("app/src-tauri/target"),
            "Rust build output (Cargo.toml)"
        );
        assert_eq!(
            out.left_out("app/src-tauri/gen/schemas"),
            "Tauri generated schemas"
        );
        assert_eq!(
            out.left_out("app/src-tauri/gen/android/app/build"),
            "Gradle build output"
        );
        assert_eq!(
            out.left_out("app/src-tauri/gen/android/.gradle"),
            ".gradle/"
        );
        for kept in [
            ".env",
            "Cargo.toml",
            "crates/core/src/lib.rs",
            "app/src-tauri/gen/android/app/build.gradle.kts",
            "game/target/crosshair.png",
        ] {
            out.backed_up(kept);
        }
    }

    #[test]
    fn javascript_typescript_and_electron() {
        let mut paths = vec![
            "package.json",
            ".env.local",
            "src/index.ts",
            "node_modules/react/index.js",
            "web/.yarn/cache/x.zip",
            "web/.yarn/releases/yarn.cjs",
            "electron-builder.yml",
            "release/1.0.0/App Setup.exe",
            "dist_electron/x.js",
            // electron-builder's resources: sources (no git, no .gitignore).
            "build/installer.nsh",
            "build/icon.ico",
            // A monorepo's packages: sources.
            "packages/ui/index.ts",
            ".eslintcache",
            "tsconfig.tsbuildinfo",
        ];
        let caches = [
            "dist",
            ".next",
            ".nuxt",
            ".output",
            ".svelte-kit",
            ".vite",
            ".turbo",
            ".parcel-cache",
            ".angular",
            "storybook-static",
            ".expo",
            ".vercel",
            ".netlify",
            "coverage",
            ".nyc_output",
            ".docusaurus",
            ".wrangler",
        ];
        let files: Vec<String> = caches.iter().map(|dir| format!("{dir}/x.js")).collect();
        paths.extend(files.iter().map(String::as_str));
        let out = backup_of(&project("node", &paths));
        for dir in caches {
            assert_eq!(out.left_out(dir), format!("{dir}/"));
        }
        assert_eq!(out.left_out("node_modules"), "node_modules/");
        assert_eq!(out.left_out("web/.yarn/cache"), "**/.yarn/cache/");
        assert_eq!(out.left_out("release"), "electron-builder output");
        assert_eq!(out.left_out("dist_electron"), "dist_electron/");
        assert_eq!(out.left_out(".eslintcache"), ".eslintcache");
        assert_eq!(out.left_out("tsconfig.tsbuildinfo"), "*.tsbuildinfo");
        for kept in [
            ".env.local",
            "build/installer.nsh",
            "build/icon.ico",
            "packages/ui/index.ts",
            "web/.yarn/releases/yarn.cjs",
        ] {
            out.backed_up(kept);
        }

        // electron-builder named in package.json only.
        let out = backup_of(&project(
            "electron-json",
            &["release/1.0.0/app.exe", "src/main.js"],
        ));
        out.backed_up("release/1.0.0/app.exe");
        let root = project("electron-json2", &["release/app.exe"]);
        fs::write(
            root.join("package.json"),
            r#"{"devDependencies":{"electron-builder":"^26"}}"#,
        )
        .unwrap();
        assert_eq!(
            backup_of(&root).left_out("release"),
            "electron-builder output"
        );
    }

    #[test]
    fn python() {
        let out = backup_of(&project(
            "python",
            &[
                "pyproject.toml",
                ".env",
                "src/pkg/__init__.py",
                "src/pkg/__pycache__/x.cpython-312.pyc",
                "src/pkg/old.pyc",
                "venv/pyvenv.cfg",
                "venv/Lib/site-packages/x.py",
                "env/pyvenv.cfg",
                ".venv/Scripts/python.exe",
                "conda/conda-meta/history",
                ".pytest_cache/v/x",
                ".mypy_cache/x",
                ".ruff_cache/x",
                ".tox/py312/x",
                "src/pkg.egg-info/PKG-INFO",
                "build/lib/pkg/__init__.py",
                "dist/pkg-1.0.whl",
                "htmlcov/index.html",
                ".ipynb_checkpoints/a.ipynb",
                // A settings folder called env (no pyvenv.cfg): sources.
                "deploy/env/production.yaml",
            ],
        ));
        assert_eq!(
            out.left_out("venv"),
            "Python virtual environment (pyvenv.cfg)"
        );
        assert_eq!(
            out.left_out("env"),
            "Python virtual environment (pyvenv.cfg)"
        );
        assert_eq!(out.left_out(".venv"), ".venv/");
        assert_eq!(out.left_out("conda"), "Conda environment");
        assert_eq!(out.left_out("src/pkg/__pycache__"), "__pycache__/");
        assert_eq!(out.left_out("src/pkg/old.pyc"), "*.pyc");
        assert_eq!(out.left_out("src/pkg.egg-info"), "*.egg-info/");
        assert_eq!(out.left_out("build"), "Python build output");
        for dir in [
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            ".tox",
            "dist",
            "htmlcov",
            ".ipynb_checkpoints",
        ] {
            out.left_out(dir);
        }
        out.backed_up(".env");
        out.backed_up("deploy/env/production.yaml");
        out.backed_up("src/pkg/__init__.py");
    }

    #[test]
    fn dotnet_and_visual_studio_cpp() {
        let out = backup_of(&project(
            "dotnet",
            &[
                "App.sln",
                "App/App.csproj",
                "App/Program.cs",
                "App/bin/Debug/net8.0/App.dll",
                "App/obj/project.assets.json",
                "packages/Newtonsoft.Json.13.0.3/Newtonsoft.Json.13.0.3.nupkg",
                ".vs/App/v17/.suo",
                // C++ project of the same solution.
                "Engine/Engine.vcxproj",
                "Engine/engine.cpp",
                "Engine/x64/Debug/engine.obj",
                "Engine/Release/engine.exe",
                "Engine/engine.ilk",
                // Scripts in a bin folder without a project file: sources.
                "tools/bin/deploy.ps1",
                // A Wavefront model is not an object file.
                "assets/models/teapot.obj",
                "native/main.c",
                "native/main.obj",
                "native/Makefile",
            ],
        ));
        assert_eq!(out.left_out("App/bin"), ".NET build output");
        assert_eq!(out.left_out("App/obj"), ".NET build output");
        assert_eq!(out.left_out("packages"), "NuGet packages");
        assert_eq!(out.left_out(".vs"), ".vs/");
        assert_eq!(out.left_out("Engine/x64"), "Visual Studio build output");
        assert_eq!(out.left_out("Engine/Release"), "Visual Studio build output");
        assert_eq!(out.left_out("Engine/engine.ilk"), "*.ilk");
        assert_eq!(out.left_out("native/main.obj"), "C/C++ object file");
        for kept in [
            "App/Program.cs",
            "Engine/engine.cpp",
            "tools/bin/deploy.ps1",
            "assets/models/teapot.obj",
            "native/main.c",
        ] {
            out.backed_up(kept);
        }
    }

    #[test]
    fn java_kotlin_go_and_cmake() {
        let out = backup_of(&project(
            "jvm",
            &[
                "settings.gradle.kts",
                "build.gradle.kts",
                "build/classes/Main.class",
                ".gradle/8.5/x",
                ".kotlin/sessions/x",
                "server/pom.xml",
                "server/target/server.jar",
                "tool/tool.iml",
                "tool/out/production/Main.class",
                "go/go.mod",
                "go/main.go",
                "go/bin/app.exe",
                "go/vendor/github.com/x/y/y.go",
                "cpp/CMakeLists.txt",
                "cpp/main.cpp",
                "cpp/build-release/CMakeCache.txt",
                "cpp/build-release/app.exe",
                "cpp/cmake-build-debug/app.exe",
                "cpp/CMakeFiles/x",
                "cpp/main.o",
            ],
        ));
        assert_eq!(out.left_out("build"), "Gradle build output");
        assert_eq!(out.left_out(".gradle"), ".gradle/");
        assert_eq!(out.left_out(".kotlin"), ".kotlin/");
        assert_eq!(
            out.left_out("server/target"),
            "Maven build output (pom.xml)"
        );
        assert_eq!(out.left_out("tool/out"), "IntelliJ build output");
        assert_eq!(out.left_out("go/bin"), "Go build output (go.mod)");
        assert_eq!(
            out.left_out("cpp/build-release"),
            "CMake build folder (CMakeCache.txt)"
        );
        assert_eq!(out.left_out("cpp/cmake-build-debug"), "cmake-build-*/");
        assert_eq!(out.left_out("cpp/CMakeFiles"), "CMakeFiles/");
        assert_eq!(out.left_out("cpp/main.o"), "*.o");
        out.backed_up("go/vendor/github.com/x/y/y.go");
        out.backed_up("cpp/main.cpp");
        out.backed_up("settings.gradle.kts");
    }

    #[test]
    fn flutter_xcode_unity_and_godot() {
        let out = backup_of(&project(
            "mobile-games",
            &[
                "flutter/pubspec.yaml",
                "flutter/lib/main.dart",
                "flutter/.dart_tool/x",
                "flutter/build/app/outputs/app.apk",
                "flutter/ios/Podfile",
                "flutter/ios/Pods/x",
                "xcode/App.xcodeproj/project.pbxproj",
                "xcode/App.xcodeproj/xcuserdata/me.xcuserdatad/x",
                "xcode/build/Release/App.app",
                "xcode/DerivedData/x",
                "swift/Package.swift",
                "swift/.build/debug/x",
                "unity/Assets/Scripts/Player.cs",
                "unity/Assets/Logs/notes.txt",
                "unity/ProjectSettings/ProjectVersion.txt",
                "unity/Library/ArtifactDB",
                "unity/Temp/x",
                "unity/Obj/x",
                "unity/Logs/x.txt",
                "unity/UserSettings/x",
                "godot/project.godot",
                "godot/.godot/imported/x",
                "godot/.import/x",
                "godot/scenes/main.tscn",
            ],
        ));
        assert_eq!(out.left_out("flutter/.dart_tool"), ".dart_tool/");
        assert_eq!(out.left_out("flutter/build"), "Flutter / Dart build output");
        assert_eq!(
            out.left_out("flutter/ios/Pods"),
            "CocoaPods dependencies (Podfile)"
        );
        assert_eq!(out.left_out("xcode/build"), "Xcode build output");
        assert_eq!(out.left_out("xcode/DerivedData"), "DerivedData/");
        assert_eq!(
            out.left_out("xcode/App.xcodeproj/xcuserdata"),
            "xcuserdata/"
        );
        assert_eq!(out.left_out("swift/.build"), ".build/");
        for dir in ["Library", "Temp", "Obj", "Logs", "UserSettings"] {
            assert_eq!(
                out.left_out(&format!("unity/{dir}")),
                "Unity generated folder"
            );
        }
        assert_eq!(out.left_out("godot/.godot"), ".godot/");
        assert_eq!(out.left_out("godot/.import"), "Godot import cache");
        for kept in [
            "flutter/lib/main.dart",
            "flutter/ios/Podfile",
            "xcode/App.xcodeproj/project.pbxproj",
            "unity/Assets/Scripts/Player.cs",
            "unity/Assets/Logs/notes.txt",
            "godot/scenes/main.tscn",
        ] {
            out.backed_up(kept);
        }
    }

    #[test]
    fn system_files_logs_editors_and_agents() {
        let out = backup_of(&project(
            "junk",
            &[
                "README.md",
                "Thumbs.db",
                "assets/.DS_Store",
                "assets/desktop.ini",
                "debug.log",
                ".idea/workspace.xml",
                ".vscode/settings.json",
                ".vscode/ipch/x",
                ".agents/skills/x.md",
                ".claude/settings.json",
                ".codex/x",
                ".git/HEAD",
                "notes.swp",
            ],
        ));
        for (rel, rule) in [
            ("Thumbs.db", "Thumbs.db"),
            ("assets/.DS_Store", ".DS_Store"),
            ("assets/desktop.ini", "desktop.ini"),
            ("debug.log", "*.log"),
            (".idea", ".idea/"),
            (".vscode/ipch", "ipch/"),
            (".agents", ".agents/"),
            (".claude", ".claude/"),
            (".codex", ".codex/"),
            (".git", ".git/"),
            ("notes.swp", "*.swp"),
        ] {
            assert_eq!(out.left_out(rel), rule);
        }
        out.backed_up(".vscode/settings.json");
        out.backed_up("README.md");
    }

    #[test]
    fn git_decides_names_without_markers_and_tracked_folders_stay() {
        let root = project(
            "git",
            &[
                "Cargo.toml",
                "src/main.rs",
                // Tracked: kept even next to Cargo.toml.
                "target/deploy.yaml",
                // Not tracked, no marker: generated.
                "out/bundle.js",
                "build/installer.nsh",
                "gen/android/app.txt",
                // Not tracked but a name that is often a source: kept.
                "packages/ui/index.ts",
                "scripts/bin/run.sh",
            ],
        );
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(
            root.join(".git").join("index"),
            crate::project_backups::gitindex::tests::index_v2(&[
                "Cargo.toml",
                "src/main.rs",
                "target/deploy.yaml",
                "build/installer.nsh",
                "scripts/bin/run.sh",
            ]),
        )
        .unwrap();
        let out = backup_of(&root);
        assert_eq!(out.left_out("out"), UNTRACKED_OUTPUT);
        assert_eq!(out.left_out("gen"), UNTRACKED_OUTPUT);
        for kept in [
            "target/deploy.yaml",
            "build/installer.nsh",
            "packages/ui/index.ts",
            "scripts/bin/run.sh",
        ] {
            out.backed_up(kept);
        }

        // No git: a .gitignore that names the folder decides.
        let root = project("gitignore-only", &["web/out/x.js", "web/vendor/lib.js"]);
        fs::write(root.join("web").join(".gitignore"), "out/\n").unwrap();
        let out = backup_of(&root);
        assert_eq!(out.left_out("web/out"), IGNORED_OUTPUT);
        out.backed_up("web/vendor/lib.js");
    }
}
