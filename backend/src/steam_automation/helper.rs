//! MYLE started as one running Steam game instead of as the app.
use std::path::PathBuf;
use std::time::Duration;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;
use super::achievements::{AchievementList, SetOutcome};
use super::client::SteamClient;
use super::steam_state;
const PUMP_EVERY: Duration = Duration::from_secs(1);
const CHECK_STEAM_SECS: u32 = 20;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HelperMode {
    Run(u32),
    Achievements(u32),
    Unlock { appid: u32, name: String },
    /// Unlocks (or locks) every one of `names`, stored at once.
    Set { appid: u32, names: Vec<String>, unlock: bool },
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
    if let Some(appid) = app_id(args, "--steam-set=") {
        let names: Vec<String> = serde_json::from_str(value_of(args, "--steam-names=")?).ok()?;
        let unlock = match value_of(args, "--steam-to=")? {
            "unlocked" => true,
            "locked" => false,
            _ => return None,
        };
        return (!names.is_empty()).then_some(HelperMode::Set { appid, names, unlock });
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
/// Whether the Steam this helper connected to has ended. `current` is 0 once Steam has closed.
fn steam_went_away(at_start: u32, current: u32, still_alive: bool) -> bool {
    at_start != 0 && (current != at_start || !still_alive)
}
/// Stays connected as this game until the process is killed or that Steam ends, so a helper is
/// never left running but connected to nothing.
fn run_game(appid: u32) -> Result<(), String> {
    let steam = steam_state::steam_pid();
    let client = SteamClient::open(&steam_path()?, appid)?;
    let mut seconds = 0_u32;
    loop {
        client.pump();
        std::thread::sleep(PUMP_EVERY);
        seconds = seconds.wrapping_add(1);
        if seconds.is_multiple_of(CHECK_STEAM_SECS) {
            let current = steam_state::steam_pid();
            if steam_went_away(steam, current, steam_state::process_alive(current)) {
                return Err("Steam has closed.".to_string());
            }
        }
    }
}
/// Prints this game's achievements as JSON, for the app to read.
fn list_achievements(appid: u32) -> Result<(), String> {
    let client = SteamClient::open(&steam_path()?, appid)?;
    let stats = client.user_stats()?;
    stats.load()?;
    let all = stats.achievements();
    let list = AchievementList {
        names: all.iter().map(|(name, _)| name.clone()).collect(),
        locked: all.into_iter().filter(|(_, unlocked)| !unlocked).map(|(name, _)| name).collect(),
    };
    println!("{}", serde_json::to_string(&list).map_err(|e| e.to_string())?);
    Ok(())
}
/// Prints what changed as JSON, for the app to read.
fn set_achievements(appid: u32, names: &[String], unlock: bool) -> Result<(), String> {
    let client = SteamClient::open(&steam_path()?, appid)?;
    let stats = client.user_stats()?;
    stats.load()?;
    let outcome: SetOutcome = stats.set_many(names, unlock)?;
    println!("{}", serde_json::to_string(&outcome).map_err(|e| e.to_string())?);
    Ok(())
}
fn unlock_achievement(appid: u32, name: &str) -> Result<(), String> {
    let client = SteamClient::open(&steam_path()?, appid)?;
    let stats = client.user_stats()?;
    stats.load()?;
    stats.unlock(name)
}
/// Runs a helper if this process was started as one. `None` means a normal app launch.
pub fn run_helper_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let result = match mode_from_args(&args)? {
        HelperMode::Run(appid) => run_game(appid),
        HelperMode::Achievements(appid) => list_achievements(appid),
        HelperMode::Unlock { appid, name } => unlock_achievement(appid, &name),
        HelperMode::Set { appid, names, unlock } => set_achievements(appid, &names, unlock),
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
    fn the_same_steam_still_running_is_fine() {
        assert!(!steam_went_away(100, 100, true));
    }
    #[test]
    fn a_closed_steam_is_gone() {
        assert!(steam_went_away(100, 0, false));
    }
    #[test]
    fn a_restarted_steam_is_gone() {
        assert!(steam_went_away(100, 200, true));
    }
    #[test]
    fn a_crashed_steam_is_gone_though_the_registry_still_names_it() {
        assert!(steam_went_away(100, 100, false));
    }
    #[test]
    fn with_no_steam_known_at_the_start_nothing_can_be_said() {
        assert!(!steam_went_away(0, 200, true));
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
    #[test]
    fn set_takes_names_and_where_they_go() {
        assert_eq!(
            mode_from_args(&args(&["x", "--steam-set=620", r#"--steam-names=["A","B C"]"#, "--steam-to=locked"])),
            Some(HelperMode::Set { appid: 620, names: vec!["A".into(), "B C".into()], unlock: false })
        );
        assert_eq!(
            mode_from_args(&args(&["x", "--steam-set=620", r#"--steam-names=["A"]"#, "--steam-to=unlocked"])),
            Some(HelperMode::Set { appid: 620, names: vec!["A".into()], unlock: true })
        );
    }
    #[test]
    fn set_needs_names_and_a_known_state() {
        assert_eq!(mode_from_args(&args(&["x", "--steam-set=620", "--steam-to=locked"])), None);
        assert_eq!(mode_from_args(&args(&["x", "--steam-set=620", "--steam-names=[]", "--steam-to=locked"])), None);
        assert_eq!(mode_from_args(&args(&["x", "--steam-set=620", r#"--steam-names=["A"]"#, "--steam-to=maybe"])), None);
        assert_eq!(mode_from_args(&args(&["x", "--steam-set=620", r#"--steam-names=["A"]"#])), None);
    }
    #[test]
    fn what_the_app_sends_is_what_the_helper_reads() {
        let names = vec!["ACH_1".to_string(), "with \"quotes\" and spaces".to_string()];
        let mut sent = vec!["MYLE.exe".to_string()];
        sent.extend(crate::steam_automation::achievements::set_args(620, &names, false));
        assert_eq!(mode_from_args(&sent), Some(HelperMode::Set { appid: 620, names, unlock: false }));
    }
}
