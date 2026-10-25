//! What the Disk Analyzer never moves to the Recycle Bin, whatever the page
//! sends: drives, the scanned folder, Windows, user profiles and their main
//! folders, Program Files and ProgramData themselves, the file system's own
//! files at a drive's root, and MYLE itself. A folder that holds any of them
//! is refused too, since recycling it would take them along.
//!
//! Paths are compared by their parts, without regard to case, after taking
//! off `\\?\` and working out `.` and `..`: never as plain text prefixes.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// Allowed, with a warning: inside Program Files or ProgramData.
    Caution,
    /// Refused, and why.
    Protected(&'static str),
}

const DRIVE: &str = "A whole drive is never moved to the Recycle Bin.";
const SCANNED: &str = "This is the folder you scanned.";
const WINDOWS: &str = "Windows needs this.";
const PROFILE: &str = "A user's folder. What is inside it can go.";
const PROGRAMS: &str = "Windows keeps programs here. What is inside can go.";
const MYLE: &str = "MYLE needs this.";
const HOLDS: &str = "It holds something Windows, a user or MYLE needs.";

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

type Parts = Vec<String>;

pub struct Guard {
    root: Parts,
    /// Refused with everything inside.
    whole: Vec<(Parts, &'static str)>,
    /// Refused themselves.
    exact: Vec<(Parts, &'static str)>,
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
        Self {
            root: parts(scan_root),
            whole,
            exact,
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
            return Verdict::Protected(DRIVE);
        }
        if path == self.root {
            return Verdict::Protected(SCANNED);
        }
        let top = path[1].as_str();
        let file_system = top.starts_with('$');
        if path.len() == 2 && CAUTION_NAMES.contains(&top) {
            return Verdict::Protected(PROGRAMS);
        }
        if path.len() == 2
            && (file_system || system || ROOT_NAMES.contains(&top) || top.starts_with("dumpstack.log"))
        {
            return Verdict::Protected(WINDOWS);
        }
        if path.len() > 2 && (file_system || ROOT_TREES.contains(&top)) {
            return Verdict::Protected(WINDOWS);
        }
        if let Some((_, why)) = self.whole.iter().find(|(p, _)| starts_with(&path, p)) {
            return Verdict::Protected(why);
        }
        if let Some((_, why)) = self.exact.iter().find(|(p, _)| *p == path) {
            return Verdict::Protected(why);
        }
        // Recycling a folder takes everything inside along.
        if self.whole.iter().chain(&self.exact).any(|(p, _)| p.len() > path.len() && starts_with(p, &path)) {
            return Verdict::Protected(HOLDS);
        }
        if (path.len() > 2 && CAUTION_NAMES.contains(&top)) || self.caution.iter().any(|p| path.len() > p.len() && starts_with(&path, p)) {
            return Verdict::Caution;
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
        }
    }

    fn check(root: &str, path: &str) -> Verdict {
        Guard::new(Path::new(root), &places()).check(Path::new(path), false)
    }

    fn refused(root: &str, path: &str) -> bool {
        matches!(check(root, path), Verdict::Protected(_))
    }

    #[test]
    fn drives_and_the_scanned_folder_are_refused() {
        assert_eq!(check(r"C:\", r"C:\"), Verdict::Protected(DRIVE));
        assert_eq!(check(r"C:\", r"D:\"), Verdict::Protected(DRIVE));
        assert_eq!(check(r"D:\Data", r"d:\data\"), Verdict::Protected(SCANNED));
        assert_eq!(check(r"D:\Data", r"\\?\D:\DATA"), Verdict::Protected(SCANNED));
        assert_eq!(check(r"\\srv\share\x", r"\\srv\share"), Verdict::Protected(DRIVE));
        assert_eq!(check(r"\\srv\share\x", r"\\?\UNC\srv\share\x"), Verdict::Protected(SCANNED));
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
        assert_eq!(check(r"C:\", r"C:\Program Files"), Verdict::Protected(PROGRAMS));
        assert_eq!(check(r"C:\", r"C:\PROGRAMDATA"), Verdict::Protected(PROGRAMS));
        assert_eq!(check(r"C:\", r"C:\Program Files (x86)\Old App"), Verdict::Caution);
        assert_eq!(check(r"C:\", r"C:\ProgramData\Package Cache\x.msi"), Verdict::Caution);
        // The same names on another drive.
        assert!(refused(r"D:\", r"D:\Program Files"));
        assert_eq!(check(r"D:\", r"D:\Program Files\Game"), Verdict::Caution);
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
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Local\Programs\MYLE\MYLE.exe"), Verdict::Protected(MYLE));
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Local\Programs"), Verdict::Protected(HOLDS));
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Roaming\ThomasThanos"), Verdict::Protected(HOLDS));
        assert_eq!(check(r"D:\", r"D:\OneDrive"), Verdict::Protected(HOLDS), "it holds a known folder");
        assert_eq!(check(r"C:\", r"C:\Users\Ann\AppData\Roaming\Other App"), Verdict::Allowed);
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
        assert!(matches!(guard.check(&exe, false), Verdict::Protected(MYLE)));
        assert_eq!(guard.check(&temp.join("some-file.tmp"), false), Verdict::Allowed);
    }
}
