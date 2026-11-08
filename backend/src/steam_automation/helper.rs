//! MYLE started as one running Steam game instead of as the app.
use std::path::PathBuf;
use std::time::Duration;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;
use super::client::SteamClient;
const PUMP_EVERY: Duration = Duration::from_secs(1);
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HelperMode {
    Run(u32),
    Achievements(u32),
    Unlock { appid: u32, name: String },
}
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().find_map(|arg| arg.strip_prefix(flag))
}
fn app_id(args: &[String], flag: &str) -> Option<u32> {
    value_of(args, flag)?.parse().ok().filter(|id| *id > 0)
}
pub(crate) fn mode_from_args(args: &[String]) -> Option<HelperMode> {
    if let Some(appid) = app_id(args, "--steam-run=") {
        return Some(HelperMode::Run(appid));
    }
    if let Some(appid) = app_id(args, "--steam-achievements=") {
        return Some(HelperMode::Achievements(appid));
    }
    let appid = app_id(args, "--steam-unlock=")?;
    let name = value_of(args, "--steam-achievement=")?.to_string();
    Some(HelperMode::Unlock { appid, name })
}
pub(super) fn steam_path() -> Result<PathBuf, String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .map_err(|_| "Steam is not installed.".to_string())?;
    let path: String = key.get_value("SteamPath").map_err(|_| "Steam is not installed.".to_string())?;
    Ok(PathBuf::from(path))
}
/// Stays connected as this game until the process is killed.
fn run_game(appid: u32) -> Result<(), String> {
    let client = SteamClient::open(&steam_path()?, appid)?;
    loop {
        client.pump();
        std::thread::sleep(PUMP_EVERY);
    }
}
/// Runs a helper if this process was started as one. `None` means a normal app launch.
pub fn run_helper_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let result = match mode_from_args(&args)? {
        HelperMode::Run(appid) => run_game(appid),
        HelperMode::Achievements(_) | HelperMode::Unlock { .. } => Err("Not available yet.".to_string()),
    };
    Some(match result {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("{reason}");
            1
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn a_normal_launch_is_not_a_helper() {
        assert_eq!(mode_from_args(&args(&["MYLE.exe"])), None);
    }
    #[test]
    fn run_takes_an_app_id() {
        assert_eq!(mode_from_args(&args(&["x", "--steam-run=620"])), Some(HelperMode::Run(620)));
    }
    #[test]
    fn a_bad_app_id_is_not_a_helper() {
        assert_eq!(mode_from_args(&args(&["x", "--steam-run=abc"])), None);
        assert_eq!(mode_from_args(&args(&["x", "--steam-run=0"])), None);
    }
    #[test]
    fn achievements_takes_an_app_id() {
        assert_eq!(mode_from_args(&args(&["x", "--steam-achievements=620"])), Some(HelperMode::Achievements(620)));
    }
    #[test]
    fn unlock_takes_a_name() {
        assert_eq!(
            mode_from_args(&args(&["x", "--steam-unlock=620", "--steam-achievement=ACH_WIN"])),
            Some(HelperMode::Unlock { appid: 620, name: "ACH_WIN".into() })
        );
    }
    #[test]
    fn unlock_without_a_name_is_not_a_helper() {
        assert_eq!(mode_from_args(&args(&["x", "--steam-unlock=620"])), None);
    }
}
