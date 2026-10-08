//! Reads a build's output as it comes: errors and warnings with their file,
//! line and column (Rust, TypeScript, svelte-check, ESLint, Vite / Rollup /
//! esbuild, MSBuild and C#, electron-builder, NSIS, npm, Python, Go), the
//! stage the build is in, and a percentage where a tool prints one.

use std::sync::OnceLock;

use regex_lite::Regex;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// As the tool printed it (relative or full).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// `E0308`, `TS2322`, `CS1002`, an ESLint rule.
    pub code: Option<String>,
    /// `rust`, `typescript`, `eslint`, …
    pub tool: &'static str,
    /// The log line it was read from (0-based).
    pub log_line: usize,
}

/// What one line told.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineInfo {
    pub diagnostics: Vec<Diagnostic>,
    pub stage: Option<String>,
    pub percent: Option<f32>,
}

/// Removes ANSI color and cursor codes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() || next == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC … BEL or ST.
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

struct Patterns {
    rust_head: Regex,
    rust_at: Regex,
    paren_code: Regex,
    colon_ts: Regex,
    svelte_machine: Regex,
    location_only: Regex,
    error_word: Regex,
    eslint_row: Regex,
    path_line: Regex,
    esbuild_head: Regex,
    esbuild_at: Regex,
    vite_plugin: Regex,
    rollup_resolve: Regex,
    nsis_script: Regex,
    nsis_warning: Regex,
    python_at: Regex,
    python_error: Regex,
    go_error: Regex,
    generic: Regex,
    percent: Regex,
    modules: Regex,
    npm_script: Regex,
    cargo_compiling: Regex,
    eb_target: Regex,
    tauri_bundling: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let re = |pattern: &str| Regex::new(pattern).expect("a valid pattern");
        Patterns {
            rust_head: re(r"^(error|warning)(?:\[([A-Za-z0-9_:]+)\])?: (.+)$"),
            rust_at: re(r"^\s*--> (.+?):(\d+):(\d+)\s*$"),
            // tsc (--pretty false), MSBuild, csc: `file(12,5): error TS2322: msg`.
            paren_code: re(r"^\s*(.+?)\((\d+),(\d+)(?:,\d+,\d+)?\)\s*:\s*(error|warning)\s+([A-Za-z]+\d+)\s*:\s*(.+?)(?:\s+\[[^\]]+\])?$"),
            colon_ts: re(r"^\s*(.+?):(\d+):(\d+) - (error|warning) (TS\d+): (.+)$"),
            svelte_machine: re(r#"^\d+ (ERROR|WARNING) "(.+?)" (\d+):(\d+) "(.+)"$"#),
            location_only: re(r"^\s*((?:[A-Za-z]:)?[^\s:]*[\\/][^:]+\.[A-Za-z0-9]+|[^\s:]+\.(?:svelte|vue|ts|tsx|js|jsx|mjs|cjs|css|scss)):(\d+):(\d+)\s*$"),
            error_word: re(r"^\s*(Error|Warn|Warning|error|warning): (.+)$"),
            eslint_row: re(r"^\s+(\d+):(\d+)\s+(error|warning)\s+(.+?)\s{2,}(\S+)\s*$"),
            path_line: re(r"^((?:[A-Za-z]:\\|/)\S.*\.[A-Za-z0-9]+)\s*$"),
            esbuild_head: re(r"^\s*(✘|▲) \[(ERROR|WARNING)\] (.+)$"),
            esbuild_at: re(r"^\s+(.+?):(\d+):(\d+):\s*$"),
            vite_plugin: re(r"^\s*(?:\[vite\]:?|\[plugin:? ?[^\]]+\]|RollupError:|\[commonjs--resolver\])\s*(.+)$"),
            rollup_resolve: re(r#"failed to resolve import "(.+?)" from "(.+?)""#),
            nsis_script: re(r#"Error in script "(.+?)" on line (\d+)"#),
            nsis_warning: re(r"^warning (\d+): (.+)$"),
            python_at: re(r#"^\s*File "(.+?)", line (\d+)"#),
            python_error: re(r"^([A-Za-z_][\w.]*(?:Error|Exception)): (.+)$"),
            go_error: re(r"^(\S+\.go):(\d+):(\d+): (.+)$"),
            generic: re(r"^\s*(?:\[?(ERROR|Error|error|FATAL|Fatal|fatal(?: error)?)\]?[: ]\s*|(?:npm (?:ERR!|error) )|⨯ |\(!\) |(?:\[?(WARN|WARNING|Warning|warning)\]?[: ]\s*))(.+)$"),
            percent: re(r"(?:^|\s)(\d{1,3}(?:\.\d+)?)\s?%"),
            modules: re(r"(\d+) modules transformed"),
            npm_script: re(r"^> (\S+)@\S+ (\S+)"),
            cargo_compiling: re(r"^\s*Compiling (\S+) v"),
            eb_target: re(r"•\s+building\s+target=(\S+)"),
            tauri_bundling: re(r"^\s*Bundling (.+?)(?: \(|$)"),
        }
    })
}

/// npm prints these around a failed script; they say nothing of the error.
const NPM_NOISE: &[&str] = &[
    "a complete log of this run",
    "code elifecycle",
    "errno",
    "lifecycle script",
    "command failed",
    "this is probably not a problem with npm",
    "failed at the",
    "path ",
    "workspace ",
    "location ",
    "signal ",
    "command ",
];

fn num(text: &str) -> Option<u32> {
    text.parse().ok()
}

/// Reads one build's output, line by line.
#[derive(Default)]
pub struct Parser {
    /// A Rust or esbuild diagnostic waiting for its `-->` / location line.
    pending: Option<Diagnostic>,
    /// The file ESLint's stylish output is listing.
    eslint_file: Option<String>,
    /// A location seen alone on a line (svelte-check), for the next message.
    location: Option<(String, u32, u32)>,
    python_at: Option<(String, u32)>,
    crates: u32,
    /// Messages seen, so a repeated one (npm prints some twice) counts once.
    seen: std::collections::HashSet<String>,
}

impl Parser {
    /// Reads the line `index` of the output.
    pub fn line(&mut self, raw: &str, index: usize) -> LineInfo {
        let text = strip_ansi(raw);
        let p = patterns();
        let mut info = LineInfo::default();
        let trimmed = text.trim();

        // A pending Rust / esbuild diagnostic takes its location, or is
        // finished by the first line that is not part of it.
        if let Some(mut pending) = self.pending.take() {
            let at = p
                .rust_at
                .captures(&text)
                .or_else(|| p.esbuild_at.captures(&text));
            if let Some(at) = at {
                pending.file = Some(at[1].trim().to_string());
                pending.line = num(&at[2]);
                pending.column = num(&at[3]);
                self.push(&mut info, pending);
                return info;
            }
            let detail = trimmed.is_empty()
                || trimmed.starts_with('|')
                || trimmed.starts_with('=')
                || text.starts_with(' ');
            if detail && pending.tool == "esbuild" {
                self.pending = Some(pending);
                return info;
            }
            self.push(&mut info, pending);
        }
        if trimmed.is_empty() {
            return info;
        }

        // Stages and progress.
        if let Some(c) = p.npm_script.captures(trimmed) {
            info.stage = Some(format!("Running {} ({})", &c[2], &c[1]));
        } else if trimmed.contains("vite v") && trimmed.contains("building") {
            info.stage = Some("Bundling with Vite".into());
        } else if let Some(c) = p.modules.captures(trimmed) {
            info.stage = Some(format!("Bundling with Vite: {} modules", &c[1]));
        } else if trimmed.starts_with("rendering chunks") {
            info.stage = Some("Writing the bundle".into());
        } else if let Some(c) = p.cargo_compiling.captures(&text) {
            self.crates += 1;
            info.stage = Some(format!(
                "Compiling Rust: {} ({} crates)",
                &c[1], self.crates
            ));
        } else if trimmed.starts_with("Finished")
            && (trimmed.contains("profile") || trimmed.contains("target(s)"))
        {
            info.stage = Some("Rust build finished".into());
        } else if let Some(c) = p.tauri_bundling.captures(&text) {
            info.stage = Some(format!("Bundling {}", c[1].trim()));
        } else if let Some(c) = p.eb_target.captures(trimmed) {
            info.stage = Some(format!("Building the {} installer", &c[1]));
        } else if trimmed.starts_with("• packaging") {
            info.stage = Some("Packaging the app".into());
        } else if trimmed.starts_with("• signing") {
            info.stage = Some("Signing".into());
        } else if trimmed.starts_with("Processing script file") || trimmed.contains("makensis") {
            info.stage = Some("Building the installer (NSIS)".into());
        } else if trimmed.starts_with("Restore complete")
            || trimmed.contains("Determining projects to restore")
        {
            info.stage = Some("Restoring .NET packages".into());
        } else if trimmed.starts_with("svelte-check")
            || trimmed.contains("Getting Svelte diagnostics")
        {
            info.stage = Some("Checking types (svelte-check)".into());
        } else if trimmed.starts_with("Building wheel") || trimmed.contains("* Building") {
            info.stage = Some("Building the Python package".into());
        }
        if let Some(c) = p.percent.captures(trimmed)
            && let Ok(value) = c[1].parse::<f32>()
            && value <= 100.0
            && (trimmed.contains("Downloading")
                || trimmed.contains("downloading")
                || trimmed.starts_with('[')
                || trimmed.contains("Progress")
                || trimmed.contains("progress")
                || trimmed.ends_with('%'))
        {
            info.percent = Some(value);
        }

        // Diagnostics, most specific first.
        if let Some(c) = p.svelte_machine.captures(trimmed) {
            self.push(
                &mut info,
                diag(
                    &c[1],
                    &c[5],
                    Some(&c[2]),
                    num(&c[3]).map(|l| l + 1),
                    num(&c[4]).map(|c| c + 1),
                    None,
                    "svelte",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.paren_code.captures(&text) {
            let code = c[5].to_string();
            let tool = if code.starts_with("TS") {
                "typescript"
            } else if code.starts_with("CS") {
                "csharp"
            } else {
                "msbuild"
            };
            self.push(
                &mut info,
                diag(
                    &c[4],
                    &c[6],
                    Some(&c[1]),
                    num(&c[2]),
                    num(&c[3]),
                    Some(code),
                    tool,
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.colon_ts.captures(&text) {
            self.push(
                &mut info,
                diag(
                    &c[4],
                    &c[6],
                    Some(&c[1]),
                    num(&c[2]),
                    num(&c[3]),
                    Some(c[5].to_string()),
                    "typescript",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.rust_head.captures(trimmed) {
            let message = c[3].trim();
            let lower = message.to_lowercase();
            // Summaries that repeat what was already listed.
            if (lower.contains("generated") && lower.contains("warning"))
                || lower.starts_with("aborting due to")
                || lower.contains("could not compile")
                    && self.seen.iter().any(|s| s.starts_with("rust"))
                || lower.contains("build failed, waiting for other jobs")
            {
                return info;
            }
            self.pending = Some(diag(
                &c[1],
                message,
                None,
                None,
                None,
                c.get(2).map(|m| m.as_str().to_string()),
                "rust",
                index,
            ));
            return info;
        }
        if let Some(c) = p.esbuild_head.captures(&text) {
            self.pending = Some(diag(
                if &c[2] == "ERROR" { "error" } else { "warning" },
                &c[3],
                None,
                None,
                None,
                None,
                "esbuild",
                index,
            ));
            return info;
        }
        if let Some(c) = p.location_only.captures(&text) {
            self.location = Some((
                c[1].to_string(),
                num(&c[2]).unwrap_or(0),
                num(&c[3]).unwrap_or(0),
            ));
            return info;
        }
        if let Some(c) = p.error_word.captures(&text)
            && let Some((file, line, column)) = self.location.take()
        {
            self.push(
                &mut info,
                diag(
                    &c[1],
                    &c[2],
                    Some(&file),
                    Some(line),
                    Some(column),
                    None,
                    "svelte",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.eslint_row.captures(&text)
            && let Some(file) = self.eslint_file.clone()
        {
            self.push(
                &mut info,
                diag(
                    &c[3],
                    &c[4],
                    Some(&file),
                    num(&c[1]),
                    num(&c[2]),
                    Some(c[5].to_string()),
                    "eslint",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.path_line.captures(trimmed) {
            self.eslint_file = Some(c[1].to_string());
            return info;
        }
        if let Some(c) = p.nsis_script.captures(trimmed) {
            self.push(
                &mut info,
                diag(
                    "error",
                    trimmed,
                    Some(&c[1]),
                    num(&c[2]),
                    None,
                    None,
                    "nsis",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.nsis_warning.captures(trimmed) {
            self.push(
                &mut info,
                diag(
                    "warning",
                    &c[2],
                    None,
                    None,
                    None,
                    Some(c[1].to_string()),
                    "nsis",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.python_at.captures(&text) {
            self.python_at = Some((c[1].to_string(), num(&c[2]).unwrap_or(0)));
            return info;
        }
        if let Some(c) = p.python_error.captures(trimmed) {
            let at = self.python_at.take();
            self.push(
                &mut info,
                diag(
                    "error",
                    trimmed,
                    at.as_ref().map(|a| a.0.as_str()),
                    at.as_ref().map(|a| a.1),
                    None,
                    Some(c[1].to_string()),
                    "python",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.go_error.captures(trimmed) {
            self.push(
                &mut info,
                diag(
                    "error",
                    &c[4],
                    Some(&c[1]),
                    num(&c[2]),
                    num(&c[3]),
                    None,
                    "go",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.rollup_resolve.captures(trimmed) {
            self.push(
                &mut info,
                diag(
                    "error",
                    trimmed,
                    Some(&c[2]),
                    None,
                    None,
                    None,
                    "vite",
                    index,
                ),
            );
            return info;
        }
        if let Some(c) = p.vite_plugin.captures(trimmed) {
            self.push(
                &mut info,
                diag("error", &c[1], None, None, None, None, "vite", index),
            );
            return info;
        }
        if let Some(c) = p.generic.captures(trimmed) {
            let message = c[3].trim();
            let lower = message.to_lowercase();
            if message.len() < 3 || NPM_NOISE.iter().any(|noise| lower.starts_with(noise)) {
                return info;
            }
            let warning = c.get(2).is_some() || trimmed.starts_with("(!)");
            let tool = if trimmed.starts_with("npm ") {
                "npm"
            } else if trimmed.starts_with('⨯') {
                "electron-builder"
            } else {
                "build"
            };
            self.push(
                &mut info,
                diag(
                    if warning { "warning" } else { "error" },
                    message,
                    None,
                    None,
                    None,
                    None,
                    tool,
                    index,
                ),
            );
        }
        info
    }

    /// The output ended: a diagnostic still waiting for its location.
    pub fn finish(&mut self) -> Option<Diagnostic> {
        self.pending.take()
    }

    fn push(&mut self, info: &mut LineInfo, diagnostic: Diagnostic) {
        let key = format!(
            "{}|{}|{:?}|{:?}|{:?}",
            diagnostic.tool,
            diagnostic.message,
            diagnostic.file,
            diagnostic.line,
            diagnostic.severity
        );
        if self.seen.insert(key) {
            info.diagnostics.push(diagnostic);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn diag(
    severity: &str,
    message: &str,
    file: Option<&str>,
    line: Option<u32>,
    column: Option<u32>,
    code: Option<String>,
    tool: &'static str,
    log_line: usize,
) -> Diagnostic {
    let severity = if severity.to_lowercase().starts_with("warn") {
        Severity::Warning
    } else {
        Severity::Error
    };
    Diagnostic {
        severity,
        message: message.trim().to_string(),
        file: file.map(|f| f.trim().to_string()).filter(|f| !f.is_empty()),
        line,
        column,
        code,
        tool,
        log_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> (Vec<Diagnostic>, Vec<String>) {
        let mut parser = Parser::default();
        let mut diags = Vec::new();
        let mut stages = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            let info = parser.line(line, index);
            diags.extend(info.diagnostics);
            stages.extend(info.stage);
        }
        diags.extend(parser.finish());
        (diags, stages)
    }

    #[test]
    fn rust_errors_and_warnings_with_their_location() {
        let (diags, stages) = run(&[
            "   Compiling serde v1.0.200",
            "   Compiling myle v9.5.0 (C:\\Code\\MYLE\\backend)",
            "error[E0308]: mismatched types",
            "  --> src\\github_releases\\git.rs:42:9",
            "   |",
            "42 |     let x: u32 = \"a\";",
            "warning: unused variable: `y`",
            " --> src/lib.rs:10:5",
            "warning: `myle` (lib) generated 1 warning",
            "error: could not compile `myle` (lib) due to 1 previous error",
        ]);
        assert_eq!(diags.len(), 2, "{diags:#?}");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code.as_deref(), Some("E0308"));
        assert_eq!(
            diags[0].file.as_deref(),
            Some("src\\github_releases\\git.rs")
        );
        assert_eq!((diags[0].line, diags[0].column), (Some(42), Some(9)));
        assert_eq!(diags[1].severity, Severity::Warning);
        assert_eq!(diags[1].file.as_deref(), Some("src/lib.rs"));
        assert!(stages.last().unwrap().contains("2 crates"));
    }

    #[test]
    fn typescript_msbuild_and_svelte_check() {
        let (diags, _) = run(&[
            "src/app.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.",
            "src/main.ts:3:1 - error TS1005: ';' expected.",
            "C:\\src\\Program.cs(10,20): error CS1002: ; expected [C:\\src\\App.csproj]",
            "C:\\src\\App.csproj(5,3): warning MSB3245: Could not resolve this reference.",
            "1712345678 ERROR \"src/App.svelte\" 4:2 \"'foo' is not defined\"",
            "/home/x/src/Page.svelte:20:7",
            "Warn: A11y: <img> element should have an alt attribute (svelte)",
        ]);
        assert_eq!(diags.len(), 6, "{diags:#?}");
        assert_eq!(diags[0].code.as_deref(), Some("TS2322"));
        assert_eq!(diags[0].tool, "typescript");
        assert_eq!(diags[1].line, Some(3));
        assert_eq!(diags[2].tool, "csharp");
        assert_eq!(diags[2].message, "; expected");
        assert_eq!(diags[3].severity, Severity::Warning);
        assert_eq!(diags[3].tool, "msbuild");
        assert_eq!(
            (diags[4].file.as_deref(), diags[4].line, diags[4].column),
            (Some("src/App.svelte"), Some(5), Some(3))
        );
        assert_eq!(diags[5].severity, Severity::Warning);
        assert_eq!(diags[5].file.as_deref(), Some("/home/x/src/Page.svelte"));
    }

    #[test]
    fn eslint_vite_esbuild_and_electron_builder() {
        let (diags, stages) = run(&[
            "C:\\Code\\app\\src\\index.js",
            "  12:5  error    'x' is assigned a value but never used  no-unused-vars",
            "  20:1  warning  Unexpected console statement           no-console",
            "vite v7.1.0 building for production...",
            "✓ 120 modules transformed.",
            "✘ [ERROR] Expected \";\" but found \")\"",
            "",
            "    src/main.ts:4:10:",
            "[vite]: Rollup failed to resolve import \"lodash\" from \"src/util.ts\".",
            "  • building        target=nsis file=dist\\App Setup 1.0.0.exe archs=x64",
            "  ⨯ cannot find specified resource \"build/icon.ico\"",
            "(!) Some chunks are larger than 500 kB after minification.",
        ]);
        let messages: Vec<(&str, Option<&str>, Severity)> = diags
            .iter()
            .map(|d| (d.tool, d.file.as_deref(), d.severity))
            .collect();
        assert_eq!(
            messages,
            [
                (
                    "eslint",
                    Some("C:\\Code\\app\\src\\index.js"),
                    Severity::Error
                ),
                (
                    "eslint",
                    Some("C:\\Code\\app\\src\\index.js"),
                    Severity::Warning
                ),
                ("esbuild", Some("src/main.ts"), Severity::Error),
                ("vite", Some("src/util.ts"), Severity::Error),
                ("electron-builder", None, Severity::Error),
                ("build", None, Severity::Warning),
            ]
        );
        assert_eq!(diags[0].code.as_deref(), Some("no-unused-vars"));
        assert!(
            stages
                .iter()
                .any(|s| s == "Bundling with Vite: 120 modules")
        );
        assert!(stages.iter().any(
            |s| s == "Building the target=nsis installer" || s == "Building the nsis installer"
        ));
    }

    #[test]
    fn nsis_npm_python_go_and_colors() {
        let (diags, stages) = run(&[
            "\u{1b}[31merror\u{1b}[0m: something broke",
            "Error in script \"installer.nsi\" on line 42 -- aborting creation process",
            "warning 6010: install function \"x\" not referenced",
            "> myapp@1.0.0 build",
            "npm error Missing script: \"build:win\"",
            "npm error A complete log of this run can be found in: C:\\x.log",
            "npm error code ELIFECYCLE",
            "  File \"C:\\app\\main.py\", line 7, in <module>",
            "ModuleNotFoundError: No module named 'requests'",
            "cmd/tool/main.go:12:3: undefined: foo",
        ]);
        assert_eq!(diags.len(), 6, "{diags:#?}");
        assert_eq!(diags[0].message, "something broke");
        assert_eq!((diags[1].tool, diags[1].line), ("nsis", Some(42)));
        assert_eq!(diags[2].severity, Severity::Warning);
        assert_eq!(diags[3].message, "Missing script: \"build:win\"");
        assert_eq!(
            (diags[4].file.as_deref(), diags[4].line),
            (Some("C:\\app\\main.py"), Some(7))
        );
        assert_eq!(diags[5].tool, "go");
        assert_eq!(stages, ["Running build (myapp)"]);
        assert_eq!(
            strip_ansi("\u{1b}[1;32mok\u{1b}[0m \u{1b}]0;title\u{7}done"),
            "ok done"
        );
    }

    #[test]
    fn percentages_from_progress_lines() {
        let mut parser = Parser::default();
        assert_eq!(
            parser
                .line("  • downloading url=https://x size=5 MB parts=1 45%", 0)
                .percent,
            Some(45.0)
        );
        assert_eq!(parser.line("Progress: 73.5%", 1).percent, Some(73.5));
        assert_eq!(
            parser
                .line("Compressed to 30% of the original size", 2)
                .percent,
            None
        );
    }
}
