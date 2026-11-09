//! What the Steam client on this PC is doing, from the registry keys it keeps up to date.
use std::collections::BTreeSet;
use std::path::PathBuf;
use winreg::RegKey;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use winreg::enums::HKEY_CURRENT_USER;
/// A SteamID64's low 32 bits are the account number the registry keeps.
const ACCOUNT_BITS: u64 = 0xFFFF_FFFF;
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SteamState {
    pub path: PathBuf,
    pub active_user: u32,
    pub running: Vec<u32>,
}
/// `None` when Steam is not running with somebody signed in.
pub(crate) fn read() -> Option<SteamState> {
    let steam = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let path: String = steam.get_value("SteamPath").ok()?;
    let active_user: u32 = steam.open_subkey("ActiveProcess").ok()?.get_value("ActiveUser").ok()?;
    if active_user == 0 {
        return None;
    }
    let mut running = Vec::new();
    if let Ok(apps) = steam.open_subkey("Apps") {
        for name in apps.enum_keys().flatten() {
            let is_running = apps.open_subkey(&name).ok().and_then(|app| app.get_value::<u32, _>("Running").ok());
            if is_running == Some(1)
                && let Ok(appid) = name.parse()
            {
                running.push(appid);
            }
        }
    }
    Some(SteamState { path: PathBuf::from(path), active_user, running })
}
/// The Steam client's process id, 0 when it has closed.
pub(crate) fn steam_pid() -> u32 {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam\ActiveProcess").ok().and_then(|key| key.get_value("pid").ok()).unwrap_or(0)
}
pub(crate) fn process_alive(pid: u32) -> bool {
    // Steam leaves its id behind if it crashes, so the process itself has to be looked at.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        CloseHandle(process);
        alive
    }
}
/// A game Steam is running that MYLE did not start.
pub(crate) fn real_game(running: &[u32], ours: &BTreeSet<u32>) -> Option<u32> {
    running.iter().copied().find(|appid| !ours.contains(appid))
}
pub(crate) fn same_account(active_user: u32, steam_id64: u64) -> bool {
    active_user != 0 && steam_id64 & ACCOUNT_BITS == u64::from(active_user)
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use super::*;
    #[test]
    fn our_own_games_are_not_a_real_game() {
        assert_eq!(real_game(&[620], &BTreeSet::from([620])), None);
    }
    #[test]
    fn a_game_we_did_not_start_is_real() {
        assert_eq!(real_game(&[620, 730], &BTreeSet::from([620])), Some(730));
    }
    #[test]
    fn nothing_running_is_no_game() {
        assert_eq!(real_game(&[], &BTreeSet::new()), None);
    }
    #[test]
    fn accounts_match_on_the_low_bits() {
        assert!(same_account(12345, 76561197960265728 + 12345));
        assert!(!same_account(12345, 76561197960265728 + 99));
    }
    #[test]
    fn no_active_user_matches_nobody() {
        assert!(!same_account(0, 76561197960265728));
    }
}
