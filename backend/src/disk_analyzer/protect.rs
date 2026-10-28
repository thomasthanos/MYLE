// ! What the Disk Analyzer never moves to the Recycle Bin, whatever the page ! sends: drives, the scanned folder, Windows, user profiles and their main ! folders, Program Files and ProgramData themselves, the file system's own ! files at a drive's root, MYLE itself, the folders installed programs live ! in, and programs that are running. A folder that holds any of them is ! refused too, since recycling it would take them along. ! ! Some things may go, but not without a word: what is inside an installed ! program or Program Files, the folder a running program came from, browser ! data and the save games MYLE found. The warning names what it is. ! ! Paths are compared by their parts, without regard to case, after taking ! off `\\?\` and working out `.` and `..`: never as plain text prefixes.
use std::borrow::Cow;
use std::path::{Path, PathBuf};
/// Why something is refused or needs a warning.
pub type Why = Cow<'static, str>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// Allowed, with a warning that says what it is.
    Caution(Why),
    /// Refused, and why.
    Protected(Why),
}
const DRIVE: &str = "A whole drive is never moved to the Recycle Bin.";
const SCANNED: &str = "This is the folder you scanned.";
const WINDOWS: &str = "Windows needs this.";
const PROFILE: &str = "A user's folder. What is inside it can go.";
const PROGRAMS: &str = "Windows keeps programs here. What is inside can go.";
const MYLE: &str = "MYLE needs this.";
const HOLDS: &str = "It holds something Windows, a user or MYLE needs.";
const IN_PROGRAMS: &str = "Inside Program Files or ProgramData: a program may stop working without it. Uninstalling the program is safer.";
/// Directly under a drive's root: refused themselves.
const ROOT_NAMES: &[&str] = &[
    "system volume information",
    "$recycle.bin",
    "$winreagent",
    "recovery",
    "boot",
    "efi",
    "bootmgr",
    "bootnxt",
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
];
/// Directly under a drive's root: refused with everything inside.
const ROOT_TREES: &[&str] = &["system volume information", "$recycle.bin", "$winreagent", "recovery", "boot", "efi"];
/// Directly under a drive's root: refused themselves, and what is inside
/// goes with a warning.
const CAUTION_NAMES: &[&str] = &["program files", "program files (x86)", "programdata"];
/// A profile's own folders, refused themselves in every profile.
const PROFILE_FOLDERS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Videos",
    "AppData",
    r"AppData\Local",
    r"AppData\LocalLow",
    r"AppData\Roaming",
];
/// Where browsers (and a mail program) keep what their user would miss, in
/// every profile: the folder, the program, and what is in it.
const BROWSER_DATA: &[(&str, &str, &str)] = &[
    (r"AppData\Local\Google\Chrome\User Data", "Chrome", BROWSER_WHAT),
    (r"AppData\Local\Google\Chrome Beta\User Data", "Chrome Beta", BROWSER_WHAT),
    (r"AppData\Local\Microsoft\Edge\User Data", "Edge", BROWSER_WHAT),
    (r"AppData\Local\BraveSoftware\Brave-Browser\User Data", "Brave", BROWSER_WHAT),
    (r"AppData\Local\Vivaldi\User Data", "Vivaldi", BROWSER_WHAT),
    (r"AppData\Local\Chromium\User Data", "Chromium", BROWSER_WHAT),
    (r"AppData\Local\Yandex\YandexBrowser\User Data", "Yandex Browser", BROWSER_WHAT),
    (r"AppData\Roaming\Opera Software\Opera Stable", "Opera", BROWSER_WHAT),
    (r"AppData\Roaming\Opera Software\Opera GX Stable", "Opera GX", BROWSER_WHAT),
    (r"AppData\Roaming\Mozilla\Firefox\Profiles", "Firefox", BROWSER_WHAT),
    (r"AppData\Roaming\LibreWolf\Profiles", "LibreWolf", BROWSER_WHAT),
    (r"AppData\Roaming\Waterfox\Profiles", "Waterfox", BROWSER_WHAT),
    (r"AppData\Roaming\Thunderbird\Profiles", "Thunderbird", "your mail, accounts and settings"),
];
const BROWSER_WHAT: &str = "saved passwords, history, bookmarks and extensions";
/// The places of this PC the rules are about.
#[derive(Default)]
pub struct Places {
    pub windows: Vec<PathBuf>,
    /// `C:\Users`.
    pub users: Vec<PathBuf>,
    /// `C:\Users\<name>`, Public, Default…
    pub profiles: Vec<PathBuf>,
    /// The current user's known folders, wherever they were moved.
    pub known: Vec<PathBuf>,
    /// Program Files, Program Files (x86), ProgramData.
    pub programs: Vec<PathBuf>,
    /// MYLE's install folder and data folders.
    pub myle: Vec<PathBuf>,
    /// Installed programs: where each one lives, and its name.
    pub installed: Vec<(PathBuf, String)>,
    /// The program files of the processes running now.
    pub running: Vec<PathBuf>,
    /// The save folders Game Saves found, and each game's title.
    pub saves: Vec<(PathBuf, String)>,
}
impl Places {
    pub fn of_this_pc() -> Self {
        use windows::Win32::UI::Shell::{
            FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
            FOLDERID_LocalAppDataLow, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_ProgramData,
            FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Public, FOLDERID_RoamingAppData,
            FOLDERID_UserProfiles, FOLDERID_Videos, FOLDERID_Windows,
        };
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let mut places = Places {
            windows: [known_folder(&FOLDERID_Windows), env("SystemRoot"), env("WINDIR")].into_iter().flatten().collect(),
            users: [known_folder(&FOLDERID_UserProfiles)].into_iter().flatten().collect(),
            profiles: [known_folder(&FOLDERID_Profile), known_folder(&FOLDERID_Public)].into_iter().flatten().collect(),
            known: [
                &FOLDERID_Desktop,
                &FOLDERID_Documents,
                &FOLDERID_Downloads,
                &FOLDERID_Pictures,
                &FOLDERID_Music,
                &FOLDERID_Videos,
                &FOLDERID_RoamingAppData,
                &FOLDERID_LocalAppData,
                &FOLDERID_LocalAppDataLow,
            ]
            .into_iter()
            .filter_map(known_folder)
            .collect(),
            programs: [
                known_folder(&FOLDERID_ProgramFiles),
                known_folder(&FOLDERID_ProgramFilesX86),
                known_folder(&FOLDERID_ProgramData),
                env("ProgramW6432"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            myle: [
                std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)),
                crate::storage::roaming_dir().ok(),
                crate::storage::local_dir().ok(),
                crate::storage::webview_dir().ok(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            installed: crate::cleaner::health::install_locations().into_iter().map(|(name, path)| (path, name)).collect(),
            running: running_programs(),
            saves: crate::game_saves::known_save_folders(),
        };
        if let Some(profile) = places.profiles.first().cloned() {
            places.known.push(profile.join("AppData"));
            if let Some(users) = profile.parent() {
                places.users.push(users.to_path_buf());
            }
        }
        // Every profile on this PC, other users' too.
        for users in places.users.clone() {
            if let Ok(entries) = std::fs::read_dir(&users) {
                places.profiles.extend(entries.flatten().map(|entry| entry.path()));
            }
        }
        places
    }
}
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned string is copied, then freed once.
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0 as *const _));
        text.ok().map(PathBuf::from)
    }
}
/// The program files of the processes running now that can be asked (most
/// of the user's own and many of the system's).
fn running_programs() -> Vec<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let mut pids = Vec::new();
    // SAFETY: plain calls; the snapshot handle is closed below.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            pids.push(entry.th32ProcessID);
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    let mut paths: Vec<PathBuf> = pids.into_iter().filter(|&pid| pid > 4).filter_map(crate::elevated_pipe::process_path).collect();
    paths.sort();
    paths.dedup();
    paths
}
type Parts = Vec<String>;
/// A place that is refused itself and makes a folder holding it refused too.
struct Held {
    parts: Parts,
    why: String,
    holds: String,
    /// What is inside it goes with this warning, when there is one.
    inside: Option<String>,
}
/// A place that, with everything inside, goes only with a warning, and makes
/// a folder holding it need one too.
struct Careful {
    parts: Parts,
    why: String,
    holds: String,
}
pub struct Guard {
    root: Parts,
    /// Refused with everything inside.
    whole: Vec<(Parts, &'static str)>,
    /// Refused themselves.
    exact: Vec<(Parts, &'static str)>,
    /// Installed programs' folders, running programs and their folders.
    held: Vec<Held>,
    /// Browser data and save games.
    careful: Vec<Careful>,
    /// Inside them: allowed with a warning.
    caution: Vec<Parts>,
}
impl Guard {
    pub fn new(scan_root: &Path, places: &Places) -> Self {
        let mut whole: Vec<(Parts, &'static str)> = Vec::new();
        let mut exact: Vec<(Parts, &'static str)> = Vec::new();
        whole.extend(places.windows.iter().map(|p| (parts(p), WINDOWS)));
        whole.extend(places.myle.iter().map(|p| (parts(p), MYLE)));
        exact.extend(places.users.iter().map(|p| (parts(p), PROFILE)));
        exact.extend(places.known.iter().map(|p| (parts(p), PROFILE)));
        for profile in &places.profiles {
            exact.push((parts(profile), PROFILE));
            exact.extend(PROFILE_FOLDERS.iter().map(|sub| (parts(&profile.join(sub)), PROFILE)));
        }
        exact.extend(places.programs.iter().map(|p| (parts(p), PROGRAMS)));
        // A program or a game that names a drive, a profile or Documents as its folder would make all of it its own: such a place (or one that holds it) keeps its usual rule only.
        let general = |p: &Parts| p.len() <= 1 || whole.iter().chain(&exact).any(|(q, _)| starts_with(q, p));
        let mut held = Vec::new();
        for (path, name) in &places.installed {
            let p = parts(path);
            if !general(&p) && !held.iter().any(|h: &Held| h.parts == p) {
                held.push(Held {
                    parts: p,
                    why: format!("{name} is installed here. Uninstall it from Apps instead."),
                    holds: format!("It holds {name}, an installed program. Uninstall it from Apps first."),
                    inside: Some(format!("Part of {name}, an installed program: it may stop working. Uninstalling it from Apps is safer.")),
                });
            }
        }
        for exe in &places.running {
            let p = parts(exe);
            if whole.iter().any(|(q, _)| starts_with(&p, q)) {
                continue;
            }
            let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            held.push(Held {
                parts: p.clone(),
                why: format!("{name} is running. Close it first."),
                holds: format!("{name} is running from inside it. Close it first."),
                inside: None,
            });
            let folder = p[..p.len() - 1].to_vec();
            if !general(&folder) {
                held.push(Held {
                    parts: folder,
                    why: format!("{name} is running from this folder. Close it first."),
                    holds: format!("{name} is running from inside it. Close it first."),
                    inside: Some(format!("{name} is running from this folder and may need it.")),
                });
            }
        }
        let mut careful = Vec::new();
        for profile in &places.profiles {
            for (sub, browser, what) in BROWSER_DATA {
                careful.push(Careful {
                    parts: parts(&profile.join(sub)),
                    why: format!("{browser} data: {what}."),
                    holds: format!("It holds {browser} data: {what}."),
                });
            }
        }
        for (path, title) in &places.saves {
            let p = parts(path);
            if !general(&p) {
                careful.push(Careful {
                    parts: p,
                    why: format!("Save games of {title}. Back them up in Game Saves first."),
                    holds: format!("It holds save games of {title}. Back them up in Game Saves first."),
                });
            }
        }
        Self {
            root: parts(scan_root),
            whole,
            exact,
            held,
            careful,
            caution: places.programs.iter().map(|p| parts(p)).collect(),
        }
    }
    pub fn for_this_pc(scan_root: &Path) -> Self {
        Self::new(scan_root, &Places::of_this_pc())
    }
    /// Whether `path` may go to the Recycle Bin. `system`: it has
    /// FILE_ATTRIBUTE_SYSTEM.
    pub fn check(&self, path: &Path, system: bool) -> Verdict {
        let path = parts(path);
        if path.len() <= 1 {
            return Verdict::Protected(DRIVE.into());
        }
        if path == self.root {
            return Verdict::Protected(SCANNED.into());
        }
        let top = path[1].as_str();
        let file_system = top.starts_with('$');
        if path.len() == 2 && CAUTION_NAMES.contains(&top) {
            return Verdict::Protected(PROGRAMS.into());
        }
        if path.len() == 2
            && (file_system || system || ROOT_NAMES.contains(&top) || top.starts_with("dumpstack.log"))
        {
            return Verdict::Protected(WINDOWS.into());
        }
        if path.len() > 2 && (file_system || ROOT_TREES.contains(&top)) {
            return Verdict::Protected(WINDOWS.into());
        }
        if let Some((_, why)) = self.whole.iter().find(|(p, _)| starts_with(&path, p)) {
            return Verdict::Protected((*why).into());
        }
        if let Some((_, why)) = self.exact.iter().find(|(p, _)| *p == path) {
            return Verdict::Protected((*why).into());
        }
        // Recycling a folder takes everything inside along.
        if self.whole.iter().chain(&self.exact).any(|(p, _)| p.len() > path.len() && starts_with(p, &path)) {
            return Verdict::Protected(HOLDS.into());
        }
        if let Some(held) = self.held.iter().find(|h| h.parts == path) {
            return Verdict::Protected(held.why.clone().into());
        }
        if let Some(held) = self.held.iter().find(|h| h.parts.len() > path.len() && starts_with(&h.parts, &path)) {
            return Verdict::Protected(held.holds.clone().into());
        }
        // The closest warning wins: a game's saves inside a launcher's folder are the game's saves.
        let mut warning: Option<(usize, &String)> = None;
        for held in &self.held {
            if let Some(inside) = &held.inside
                && path.len() > held.parts.len()
                && starts_with(&path, &held.parts)
                && warning.is_none_or(|(d, _)| held.parts.len() > d)
            {
                warning = Some((held.parts.len(), inside));
            }
        }
        for place in &self.careful {
            if starts_with(&path, &place.parts) && warning.is_none_or(|(d, _)| place.parts.len() > d) {
                warning = Some((place.parts.len(), &place.why));
            }
        }
        if let Some((_, why)) = warning {
            return Verdict::Caution(why.clone().into());
        }
        if let Some(place) = self.careful.iter().find(|c| c.parts.len() > path.len() && starts_with(&c.parts, &path)) {
            return Verdict::Caution(place.holds.clone().into());
        }
        if (path.len() > 2 && CAUTION_NAMES.contains(&top)) || self.caution.iter().any(|p| path.len() > p.len() && starts_with(&path, p)) {
            return Verdict::Caution(IN_PROGRAMS.into());
        }
        Verdict::Allowed
    }
}
fn starts_with(path: &[String], prefix: &[String]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}
/// The parts of a path, lower case: `["c:", "users", "ann"]`, or
/// `[r"\\server\share", …]` for a network path. `.` and `..` are worked out,
/// and Windows' trailing dots and spaces are dropped as Windows does.
pub fn parts(path: &Path) -> Parts {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\").or_else(|| text.strip_prefix(r"\\.\UNC\")) {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\").or_else(|| text.strip_prefix(r"\\.\")) {
        rest.to_string()
    } else {
        text
    };
    let mut out: Parts = Vec::new();
    let rest = if let Some(unc) = text.strip_prefix(r"\\") {
        let mut pieces = unc.splitn(3, '\\');
        let server = pieces.next().unwrap_or_default();
        let share = pieces.next().unwrap_or_default();
        out.push(format!(r"\\{server}\{share}").to_lowercase());
        pieces.next().unwrap_or_default().to_string()
    } else {
        let mut pieces = text.splitn(2, '\\');
        out.push(pieces.next().unwrap_or_default().to_lowercase());
        pieces.next().unwrap_or_default().to_string()
    };
    for piece in rest.split('\\') {
        match piece {
            "" | "." => {}
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
            }
            _ => {
                let piece = piece.trim_end_matches(['.', ' ']);
                if !piece.is_empty() {
                    out.push(piece.to_lowercase());
                }
            }
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    fn places() -> Places {
        Places {
            windows: vec![r"C:\Windows".into()],
            users: vec![r"C:\Users".into()],
            profiles: vec![r"C:\Users\Ann".into(), r"C:\Users\Public".into(), r"C:\Users\Default".into(), r"C:\Users\All Users".into()],
            known: vec![r"D:\OneDrive\Documents".into()],
            programs: vec![r"C:\Program Files".into(), r"C:\Program Files (x86)".into(), r"C:\ProgramData".into()],
            myle: vec![r"C:\Users\Ann\AppData\Local\Programs\MYLE".into(), r"C:\Users\Ann\AppData\Roaming\ThomasThanos\MakeYourLifeEasier".into()],
            ..Places::default()
        }
    }
    fn check(root: &str, path: &str) -> Verdict {
        Guard::new(Path::new(root), &places()).check(Path::new(path), false)
    }
    fn refused(root: &str, path: &str) -> bool {
        matches!(check(root, path), Verdict::Protected(_))
    }
    fn protected(why: &'static str) -> Verdict {
        Verdict::Protected(why.into())
    }
    /// The warning, or the reason it is refused, or "" when it may go freely.
    fn says(guard: &Guard, path: &str) -> String {
        match guard.check(Path::new(path), false) {
            Verdict::Allowed => String::new(),
            Verdict::Caution(why) | Verdict::Protected(why) => why.into_owned(),
        }
    }
    #[test]
    fn drives_and_the_scanned_folder_are_refused() {
        assert_eq!(check(r"C:\", r"C:\"), protected(DRIVE));
        assert_eq!(check(r"C:\", r"D:\"), protected(DRIVE));
        assert_eq!(check(r"D:\Data", r"d:\data\"), protected(SCANNED));
        assert_eq!(check(r"D:\Data", r"\\?\D:\DATA"), protected(SCANNED));
        assert_eq!(check(r"\\srv\share\x", r"\\srv\share"), protected(DRIVE));
        assert_eq!(check(r"\\srv\share\x", r"\\?\UNC\srv\share\x"), protected(SCANNED));
        assert_eq!(check(r"D:\Data", r"D:\Data\old.zip"), Verdict::Allowed);
    }
    #[test]
    fn windows_and_everything_inside_it_are_refused() {
        assert!(refused(r"C:\", r"C:\Windows"));
        assert!(refused(r"C:\", r"c:\windows\system32\drivers\etc\hosts"));
        assert!(refused(r"C:\", r"\\?\C:\WINDOWS\Temp\x.tmp"));
        assert!(refused(r"C:\", r"C:\Windows\..\Windows\notepad.exe"));
        assert!(refused(r"C:\", r"C:\Windows.\explorer.exe"), "Windows drops trailing dots");
        assert!(!refused(r"C:\", r"C:\Windows.old\x"));
    }
    #[test]
    fn program_folders_are_refused_and_their_contents_need_a_warning() {
        assert_eq!(check(r"C:\", r"C:\Program Files"), protected(PROGRAMS));
        assert_eq!(check(r"C:\", r"C:\PROGRAMDATA"), protected(PROGRAMS));
        assert_eq!(check(r"C:\", r"C:\Program Files (x86)\Old App"), Verdict::Caution(IN_PROGRAMS.into()));
        assert_eq!(check(r"C:\", r"C:\ProgramData\Package Cache\x.msi"), Verdict::Caution(IN_PROGRAMS.into()));
        // The same names on another drive.
        assert!(refused(r"D:\", r"D:\Program Files"));
        assert_eq!(check(r"D:\", r"D:\Program Files\Game"), Verdict::Caution(IN_PROGRAMS.into()));
    }
    #[test]
    fn the_file_systems_own_files_at_a_root_are_refused() {
        for name in [
            "System Volume Information",
            "$Recycle.Bin",
            "$WinREAgent",
            "Recovery",
            "Boot",
            "EFI",
            "pagefile.sys",
            "hiberfil.sys",
            "swapfile.sys",
            "DumpStack.log",
            "DumpStack.log.tmp",
            "$MFT",
            "$Extend",
        ] {
            assert!(refused(r"D:\", &format!(r"D:\{name}")), "{name}");
        }
        assert!(refused(r"D:\", r"D:\$Recycle.Bin\S-1-5-21\$R1.txt"), "inside the bin too");
        assert!(refused(r"D:\", r"D:\System Volume Information\x"));
        // Only at the root: a project's own "Boot" folder can go.
        assert!(!refused(r"D:\", r"D:\Code\Boot"));
        assert!(!refused(r"D:\", r"D:\Code\pagefile.sys"));
        // Anything Windows marks as a system file directly under a root.
        let guard = Guard::new(Path::new(r"D:\"), &places());
        assert!(matches!(guard.check(Path::new(r"D:\odd.bin"), true), Verdict::Protected(_)));
        assert_eq!(guard.check(Path::new(r"D:\Data\odd.bin"), true), Verdict::Allowed);
    }
    #[test]
    fn profiles_and_their_main_folders_are_refused_but_not_what_is_inside() {
        assert!(refused(r"C:\", r"C:\Users"));
        for profile in ["Ann", "public", "Default", "All Users"] {
            assert!(refused(r"C:\", &format!(r"C:\Users\{profile}")), "{profile}");
        }
        for folder in PROFILE_FOLDERS {
            assert!(refused(r"C:\", &format!(r"C:\Users\Ann\{folder}")), "{folder}");
        }
        assert_eq!(check(r"C:\", r"C:\Users\Ann\Downloads\setup.exe"), Verdict::Allowed);
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Local\Temp"), Verdict::Allowed);
        assert_eq!(check(r"C:\", r"C:\Users\Ann\Videos\Trip"), Verdict::Allowed);
        // A known folder moved elsewhere.
        assert!(refused(r"D:\", r"D:\OneDrive\Documents"));
        assert_eq!(check(r"D:\", r"D:\OneDrive\Documents\old.docx"), Verdict::Allowed);
    }
    #[test]
    fn myle_and_folders_that_hold_protected_ones_are_refused() {
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Local\Programs\MYLE\MYLE.exe"), protected(MYLE));
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Local\Programs"), protected(HOLDS));
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Roaming\ThomasThanos"), protected(HOLDS));
        assert_eq!(check(r"D:\", r"D:\OneDrive"), protected(HOLDS), "it holds a known folder");
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Roaming\Other App"), Verdict::Allowed);
    }
    #[test]
    fn installed_programs_are_refused_with_what_holds_them_and_their_files_need_a_warning() {
        let places = Places {
            installed: vec![
                (r"D:\Games\Cyberpunk 2077".into(), "Cyberpunk 2077".into()),
                (r"C:\Program Files (x86)\Steam".into(), "Steam".into()),
                (r"C:\Program Files (x86)\Steam\steamapps\common\Portal".into(), "Portal".into()),
                // Bad entries: whole places are left to their usual rule.
                (r"C:\".into(), "Odd".into()),
                (r"C:\Users\Ann\Desktop".into(), "Odd".into()),
                (r"C:\Program Files".into(), "Odd".into()),
            ],
            ..places()
        };
        let guard = Guard::new(Path::new(r"C:\"), &places);
        let d = Guard::new(Path::new(r"D:\"), &places);
        assert_eq!(says(&d, r"D:\Games\Cyberpunk 2077"), "Cyberpunk 2077 is installed here. Uninstall it from Apps instead.");
        assert_eq!(says(&d, r"D:\Games"), "It holds Cyberpunk 2077, an installed program. Uninstall it from Apps first.");
        assert!(says(&d, r"D:\Games\Cyberpunk 2077\bin\x64\game.exe").starts_with("Part of Cyberpunk 2077,"));
        assert!(matches!(d.check(Path::new(r"D:\Games\Cyberpunk 2077\bin"), false), Verdict::Caution(_)));
        assert_eq!(d.check(Path::new(r"D:\Games\Old Game"), false), Verdict::Allowed);
        // The closest program names the files; a launcher's library is held.
        assert!(says(&guard, r"C:\Program Files (x86)\Steam\steamapps\common\Portal\hl2.exe").starts_with("Part of Portal,"));
        assert!(says(&guard, r"C:\Program Files (x86)\Steam\steam.exe").starts_with("Part of Steam,"));
        assert!(says(&guard, r"C:\Program Files (x86)\Steam\steamapps").starts_with("It holds Portal,"));
        assert!(!says(&guard, r"C:\Users\Ann\Desktop\notes.txt").contains("Odd"));
        assert_eq!(guard.check(Path::new(r"C:\Data\x"), false), Verdict::Allowed);
        assert_eq!(says(&guard, r"C:\Program Files\Other\x.dll"), IN_PROGRAMS);
    }
    #[test]
    fn running_programs_and_their_folders_are_refused() {
        let places = Places {
            running: vec![
                r"D:\Tools\Portable Editor\editor.exe".into(),
                r"C:\Users\Ann\Downloads\setup.exe".into(),
                r"C:\Windows\explorer.exe".into(),
            ],
            ..places()
        };
        let guard = Guard::new(Path::new(r"C:\"), &places);
        let d = Guard::new(Path::new(r"D:\"), &places);
        assert_eq!(says(&d, r"D:\Tools\Portable Editor\editor.exe"), "editor.exe is running. Close it first.");
        assert_eq!(says(&d, r"D:\Tools\Portable Editor"), "editor.exe is running from this folder. Close it first.");
        assert_eq!(says(&d, r"D:\Tools"), "editor.exe is running from inside it. Close it first.");
        assert_eq!(says(&d, r"D:\Tools\Portable Editor\plugins"), "editor.exe is running from this folder and may need it.");
        // From Downloads: the program itself only, not all of Downloads.
        assert_eq!(says(&guard, r"C:\Users\Ann\Downloads\setup.exe"), "setup.exe is running. Close it first.");
        assert_eq!(guard.check(Path::new(r"C:\Users\Ann\Downloads\movie.mkv"), false), Verdict::Allowed);
        assert_eq!(says(&guard, r"C:\Windows\explorer.exe"), WINDOWS);
    }
    #[test]
    fn browser_data_and_save_games_go_only_with_a_warning() {
        let places = Places {
            saves: vec![
                (r"C:\Users\Ann\Documents\My Games\Skyrim\Saves".into(), "Skyrim".into()),
                (r"C:\Program Files (x86)\Steam\userdata\1\620\remote".into(), "Portal 2".into()),
                // Saves straight in Documents would warn about all of it.
                (r"C:\Users\Ann\Documents".into(), "Loose Game".into()),
            ],
            installed: vec![(r"C:\Program Files (x86)\Steam".into(), "Steam".into())],
            ..places()
        };
        let guard = Guard::new(Path::new(r"C:\"), &places);
        let chrome = r"C:\Users\Ann\AppData\Local\Google\Chrome\User Data";
        assert_eq!(says(&guard, chrome), "Chrome data: saved passwords, history, bookmarks and extensions.");
        assert_eq!(says(&guard, &format!(r"{chrome}\Default\Login Data")), "Chrome data: saved passwords, history, bookmarks and extensions.");
        assert_eq!(says(&guard, r"C:\Users\Ann\AppData\Local\Google"), "It holds Chrome data: saved passwords, history, bookmarks and extensions.");
        assert!(says(&guard, r"C:\Users\Public\AppData\Roaming\Mozilla\Firefox\Profiles\x.default").starts_with("Firefox data"));
        assert!(matches!(guard.check(Path::new(chrome), false), Verdict::Caution(_)), "a warning, not a refusal");
        assert_eq!(says(&guard, r"C:\Users\Ann\Documents\My Games\Skyrim\Saves\quick.ess"), "Save games of Skyrim. Back them up in Game Saves first.");
        assert_eq!(says(&guard, r"C:\Users\Ann\Documents\My Games"), "It holds save games of Skyrim. Back them up in Game Saves first.");
        assert!(says(&guard, r"C:\Program Files (x86)\Steam\userdata\1\620\remote\save1.sav").starts_with("Save games of Portal 2."), "the game's saves, not Steam's files");
        assert_eq!(guard.check(Path::new(r"C:\Users\Ann\Documents\letter.docx"), false), Verdict::Allowed);
    }
    #[test]
    fn paths_are_compared_by_their_parts() {
        assert_eq!(parts(Path::new(r"\\?\C:\Users\.\Ann\..\Bob\")), ["c:", "users", "bob"]);
        assert_eq!(parts(Path::new(r"C:/Users/Ann")), ["c:", "users", "ann"]);
        assert_eq!(parts(Path::new(r"\\?\UNC\Srv\Share\A")), [r"\\srv\share", "a"]);
        assert_eq!(parts(Path::new(r"C:\..\..")), ["c:"]);
        assert_eq!(parts(Path::new("C:\\Ελληνικά")), ["c:", "ελληνικά"]);
        // Not a text prefix: C:\Users2 is not inside C:\Users.
        assert_eq!(check(r"C:\", r"C:\Users2"), Verdict::Allowed);
        assert_eq!(check(r"C:\", r"C:\Windows2\x"), Verdict::Allowed);
    }
    #[test]
    fn this_pcs_own_places_are_found() {
        let temp = std::env::temp_dir();
        let guard = Guard::for_this_pc(Path::new(r"C:\"));
        let windows = std::env::var_os("WINDIR").map(PathBuf::from).unwrap();
        assert!(matches!(guard.check(&windows.join("System32"), false), Verdict::Protected(_)));
        let profile = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap();
        assert!(matches!(guard.check(&profile, false), Verdict::Protected(_)));
        assert!(matches!(guard.check(&profile.join("Documents"), false), Verdict::Protected(_)));
        let exe = std::env::current_exe().unwrap();
        assert_eq!(guard.check(&exe, false), protected(MYLE));
        // A real Temp folder is not a protected Windows/profile folder, but a running installer or the CI runner itself may legitimately give its contents a warning. Deterministic fixtures above check Allowed and the running-program warnings separately.
        assert!(matches!(guard.check(&temp.join("some-file.tmp"), false), Verdict::Allowed | Verdict::Caution(_)));
        // This test runner is running, and found as such.
        assert!(Places::of_this_pc().running.iter().any(|p| parts(p) == parts(&exe)));
    }
}
