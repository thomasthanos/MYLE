//! Command line, in the NSIS dialect the app's updater and scripts already
//! speak: `/S` silent, `/P` passive, `/UPDATE`, `/R` relaunch, `/NS` no
//! shortcuts, `/D=<folder>` (last, unquoted, may contain spaces), plus
//! `/LIVE` (with `/S /UPDATE`): the app's seamless update, run while the app
//! is still open.
//!
//! The uninstaller also takes `_?=<folder>` (NSIS's "uninstall this folder,
//! from where you are") and `--parent=<pid>`: how its copy in %TEMP% is
//! started (see `relocate`). Uninstall options are chosen in its window.

use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    /// No window at all.
    pub silent: bool,
    /// Progress only: starts at once and closes when done.
    pub passive: bool,
    /// An update over an existing install: shortcuts are left as they are.
    pub update: bool,
    /// Start the app once installed.
    pub relaunch: bool,
    /// Update in place while the app keeps running; it restarts itself.
    pub live: bool,
    pub no_shortcuts: bool,
    pub dir: Option<PathBuf>,
    /// Uninstall: the folder to remove, without copying to %TEMP% first.
    pub in_place: Option<PathBuf>,
    /// Uninstall: the uninstaller that started this copy and waits for it.
    pub parent: Option<u32>,
}

/// A path given as `/D=` or `_?=`: the value and every argument after it,
/// spaces included (NSIS convention: these come last and are not quoted).
fn rest_of_line(first: &str, rest: &[String]) -> Option<PathBuf> {
    let mut path = first.to_string();
    for more in rest {
        path.push(' ');
        path.push_str(more);
    }
    let path = path.trim().trim_matches('"');
    (!path.is_empty()).then(|| PathBuf::from(path))
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Cli {
    let args: Vec<String> = args.into_iter().collect();
    let mut cli = Cli::default();
    for (index, arg) in args.iter().enumerate() {
        let upper = arg.to_ascii_uppercase();
        if upper.starts_with("/D=") {
            cli.dir = rest_of_line(&arg[3..], &args[index + 1..]);
            break;
        }
        if upper.starts_with("_?=") {
            cli.in_place = rest_of_line(&arg[3..], &args[index + 1..]);
            break;
        }
        if let Some(pid) = upper.strip_prefix("--PARENT=") {
            cli.parent = pid.parse().ok();
            continue;
        }
        match upper.as_str() {
            "/S" | "--SILENT" => cli.silent = true,
            "/P" | "--PASSIVE" => cli.passive = true,
            "/UPDATE" => cli.update = true,
            "/R" => cli.relaunch = true,
            "/LIVE" => cli.live = true,
            "/NS" => cli.no_shortcuts = true,
            _ => {}
        }
    }
    cli
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn the_updater_command_line_is_understood() {
        let cli = parse(args("/S /UPDATE /R"));
        assert!(cli.silent && cli.update && cli.relaunch);
        assert!(!cli.passive && !cli.live && cli.dir.is_none());

        let live = parse(args("/S /UPDATE /LIVE"));
        assert!(live.silent && live.update && live.live && !live.relaunch);
    }

    #[test]
    fn a_folder_after_d_keeps_its_spaces() {
        let cli = parse(args(r"/s /D=C:\My Apps\Make Your Life Easier"));
        assert!(cli.silent);
        assert_eq!(
            cli.dir,
            Some(PathBuf::from(r"C:\My Apps\Make Your Life Easier"))
        );
    }

    #[test]
    fn the_uninstaller_copy_is_told_its_folder_and_parent() {
        let cli = parse(args(r"/S --parent=4242 _?=C:\My Apps\MYLE"));
        assert!(cli.silent);
        assert_eq!(cli.parent, Some(4242));
        assert_eq!(cli.in_place, Some(PathBuf::from(r"C:\My Apps\MYLE")));
        assert_eq!(parse(args("--parent=x")).parent, None);
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        assert_eq!(parse(args("--whatever /X")), Cli::default());
        assert_eq!(parse(args("/PURGE")), Cli::default());
    }
}
