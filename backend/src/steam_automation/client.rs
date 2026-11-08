//! Talks to the Steam client already installed on this PC, through its own `steamclient64.dll`, so
//! nothing of Valve's ships with MYLE. The method numbers below are the positions in the C++
//! interfaces of Valve's published Steamworks headers, fixed by the version strings asked for.
use std::ffi::{CStr, CString, c_char, c_void};
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW, SetDllDirectoryW,
};
const CLIENT_VERSION: &str = "SteamClient017";
const UNSUPPORTED: &str = "This Steam version is not supported.";
// ISteamClient.
const CREATE_STEAM_PIPE: usize = 0;
const CONNECT_TO_GLOBAL_USER: usize = 2;
const GET_ISTEAM_USER_STATS: usize = 13;
// ISteamUserStats, version 012, the last with RequestCurrentStats.
const STATS_VERSION: &str = "STEAMUSERSTATS_INTERFACE_VERSION012";
const REQUEST_CURRENT_STATS: usize = 0;
const GET_ACHIEVEMENT: usize = 6;
const SET_ACHIEVEMENT: usize = 7;
const STORE_STATS: usize = 10;
const GET_NUM_ACHIEVEMENTS: usize = 14;
const GET_ACHIEVEMENT_NAME: usize = 15;
const USER_STATS_RECEIVED: i32 = 1101;
const USER_STATS_STORED: i32 = 1102;
const RESULT_OK: i32 = 1;
const CALLBACK_LIMIT: Duration = Duration::from_secs(15);
type This = *mut c_void;
type CreateInterface = unsafe extern "C" fn(*const c_char, *mut i32) -> This;
type GetCallback = unsafe extern "C" fn(i32, *mut CallbackMsg, *mut i32) -> bool;
type FreeCallback = unsafe extern "C" fn(i32) -> bool;
#[repr(C)]
pub(super) struct CallbackMsg {
    pub user: i32,
    pub callback: i32,
    pub param: *mut u8,
    pub size: i32,
}
/// The method at `index` of a C++ object's table of methods.
///
/// # Safety
/// `object` must be a live object whose table has that index, and `T` its exact signature.
pub(super) unsafe fn method<T: Copy>(object: This, index: usize) -> T {
    // SAFETY: the caller promises the object and index; the first word of a C++ object with
    // virtual methods is its table.
    unsafe {
        let table = *(object as *const *const *const c_void);
        std::mem::transmute_copy(&*table.add(index))
    }
}
fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(once(0)).collect()
}
/// One connection to the Steam client, as the game `SteamAppId` names.
pub(crate) struct SteamClient {
    pub(super) client: This,
    pub(super) pipe: i32,
    pub(super) user: i32,
    get_callback: GetCallback,
    free_callback: FreeCallback,
}
impl SteamClient {
    /// Connects as `appid`: from here until this process ends, Steam shows that game as running.
    /// Only for a helper process, which is single-threaded at this point.
    pub(crate) fn open(steam_path: &Path, appid: u32) -> Result<Self, String> {
        // SAFETY: called before any other thread exists in the helper process.
        unsafe { std::env::set_var("SteamAppId", appid.to_string()) };
        let folder = wide(steam_path.as_os_str());
        let dll = wide(steam_path.join("steamclient64.dll").as_os_str());
        // SAFETY: the strings are null-terminated and live for these calls; the library stays
        // loaded for the life of the process; each export is cast to its documented signature.
        unsafe {
            SetDllDirectoryW(folder.as_ptr());
            let library = LoadLibraryExW(dll.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH);
            if library.is_null() {
                return Err("Could not load Steam's client library.".to_string());
            }
            let export = |name: &[u8]| GetProcAddress(library, name.as_ptr()).ok_or_else(|| UNSUPPORTED.to_string());
            let create: CreateInterface = std::mem::transmute(export(b"CreateInterface\0")?);
            let get_callback: GetCallback = std::mem::transmute(export(b"Steam_BGetCallback\0")?);
            let free_callback: FreeCallback = std::mem::transmute(export(b"Steam_FreeLastCallback\0")?);
            let version = CString::new(CLIENT_VERSION).map_err(|e| e.to_string())?;
            let client = create(version.as_ptr(), std::ptr::null_mut());
            if client.is_null() {
                return Err(UNSUPPORTED.to_string());
            }
            let create_pipe: unsafe extern "C" fn(This) -> i32 = method(client, CREATE_STEAM_PIPE);
            let pipe = create_pipe(client);
            if pipe == 0 {
                return Err("Steam is not running.".to_string());
            }
            let connect: unsafe extern "C" fn(This, i32) -> i32 = method(client, CONNECT_TO_GLOBAL_USER);
            let user = connect(client, pipe);
            if user == 0 {
                return Err("Nobody is signed in to Steam.".to_string());
            }
            Ok(Self { client, pipe, user, get_callback, free_callback })
        }
    }
    /// Takes one waiting message from Steam, if there is one.
    pub(super) fn next_callback(&self) -> Option<(i32, Vec<u8>)> {
        let mut message = CallbackMsg { user: 0, callback: 0, param: std::ptr::null_mut(), size: 0 };
        let mut call = 0;
        // SAFETY: `pipe` is open, the out-pointers are live for the call, and the message is
        // copied out before Steam is told it may free it.
        unsafe {
            if !(self.get_callback)(self.pipe, &mut message, &mut call) {
                return None;
            }
            let body = if message.param.is_null() || message.size <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(message.param, message.size as usize).to_vec()
            };
            (self.free_callback)(self.pipe);
            Some((message.callback, body))
        }
    }
    /// This game's achievements and stats.
    pub(crate) fn user_stats(&self) -> Result<UserStats<'_>, String> {
        let version = CString::new(STATS_VERSION).map_err(|e| e.to_string())?;
        // SAFETY: `client` is the live interface asked for by version, and the method has this signature.
        let stats = unsafe {
            let get: unsafe extern "C" fn(This, i32, i32, *const c_char) -> This = method(self.client, GET_ISTEAM_USER_STATS);
            get(self.client, self.user, self.pipe, version.as_ptr())
        };
        if stats.is_null() {
            return Err(UNSUPPORTED.to_string());
        }
        Ok(UserStats { client: self, stats })
    }
    /// Empties Steam's waiting messages, as a game does while it runs.
    pub(crate) fn pump(&self) {
        while self.next_callback().is_some() {}
    }
}
/// One game's achievements, through ISteamUserStats.
pub(crate) struct UserStats<'a> {
    client: &'a SteamClient,
    stats: This,
}
impl UserStats<'_> {
    /// Waits for Steam's answer of this kind and checks that it succeeded.
    fn wait_for(&self, callback: i32) -> Result<(), String> {
        let started = Instant::now();
        while started.elapsed() < CALLBACK_LIMIT {
            while let Some((kind, body)) = self.client.next_callback() {
                if kind == callback {
                    let result = body.get(8..12).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                    return if result == Some(RESULT_OK) { Ok(()) } else { Err("Steam refused.".to_string()) };
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("Steam took too long to answer.".to_string())
    }
    /// Asks Steam for this game's stats. Nothing below works until this has succeeded.
    pub(crate) fn load(&self) -> Result<(), String> {
        // SAFETY: `stats` is the live interface asked for by version, and the method has this signature.
        let asked = unsafe {
            let request: unsafe extern "C" fn(This) -> bool = method(self.stats, REQUEST_CURRENT_STATS);
            request(self.stats)
        };
        if !asked {
            return Err("Steam would not give the achievements.".to_string());
        }
        self.wait_for(USER_STATS_RECEIVED)
    }
    /// Each achievement's internal name and whether it is unlocked.
    pub(crate) fn achievements(&self) -> Vec<(String, bool)> {
        // SAFETY: as above; a returned name is Steam's own null-terminated string, copied at once.
        unsafe {
            let count: unsafe extern "C" fn(This) -> u32 = method(self.stats, GET_NUM_ACHIEVEMENTS);
            let name_of: unsafe extern "C" fn(This, u32) -> *const c_char = method(self.stats, GET_ACHIEVEMENT_NAME);
            let achieved: unsafe extern "C" fn(This, *const c_char, *mut bool) -> bool = method(self.stats, GET_ACHIEVEMENT);
            (0..count(self.stats))
                .filter_map(|index| {
                    let name = name_of(self.stats, index);
                    if name.is_null() {
                        return None;
                    }
                    let mut unlocked = false;
                    achieved(self.stats, name, &mut unlocked).then(|| (CStr::from_ptr(name).to_string_lossy().into_owned(), unlocked))
                })
                .collect()
        }
    }
    /// Unlocks one achievement and waits for Steam to confirm it was stored.
    pub(crate) fn unlock(&self, name: &str) -> Result<(), String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: as above; `name` lives for the call.
        let stored = unsafe {
            let set: unsafe extern "C" fn(This, *const c_char) -> bool = method(self.stats, SET_ACHIEVEMENT);
            let store: unsafe extern "C" fn(This) -> bool = method(self.stats, STORE_STATS);
            set(self.stats, name.as_ptr()) && store(self.stats)
        };
        if !stored {
            return Err("Steam refused.".to_string());
        }
        self.wait_for(USER_STATS_STORED)
    }
}
