//! Windows shell pieces: known folders, shortcuts, message boxes, the
//! one-setup-at-a-time lock and starting the installed app.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs, FOLDERID_RoamingAppData,
    FOLDERID_Startup, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW, SW_SHOWNORMAL,
};
use windows_sys::core::GUID;

use crate::product;

pub fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: `raw` receives a CoTaskMemAlloc'd string, freed below.
    let result =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut raw) };
    if raw.is_null() {
        return None;
    }
    let path = (result >= 0).then(|| {
        // SAFETY: a successful call returns a NUL-terminated string.
        let length = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
        let units = unsafe { std::slice::from_raw_parts(raw, length) };
        PathBuf::from(OsString::from_wide(units))
    });
    // SAFETY: allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}

/// `%LOCALAPPDATA%`, where per-user apps live.
pub fn local_app_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_LocalAppData)
}

/// `%APPDATA%`.
pub fn roaming_app_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_RoamingAppData)
}

/// The three shortcuts the setup offers, by where they go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Desktop,
    StartMenu,
    /// Starts the app minimized when the user signs in.
    Startup,
}

impl Shortcut {
    pub const ALL: [Shortcut; 3] = [Shortcut::Desktop, Shortcut::StartMenu, Shortcut::Startup];

    /// `<folder>\MYLE.lnk`. The desktop follows OneDrive
    /// or any other redirection, which `%USERPROFILE%\Desktop` would miss.
    pub fn path(self) -> Option<PathBuf> {
        let folder = match self {
            Shortcut::Desktop => known_folder(&FOLDERID_Desktop),
            Shortcut::StartMenu => known_folder(&FOLDERID_Programs),
            Shortcut::Startup => known_folder(&FOLDERID_Startup),
        }?;
        Some(folder.join(format!("{}.lnk", product::NAME)))
    }

    fn fallback_path(self) -> Option<PathBuf> {
        let primary = self.path()?;
        Some(primary.with_file_name(format!("{} - {}.lnk", product::NAME, product::PUBLISHER)))
    }

    fn legacy_paths(self) -> Option<[PathBuf; 2]> {
        let primary = self.path()?;
        Some([
            primary.with_file_name(format!("{}.lnk", product::LEGACY_NAME)),
            primary.with_file_name(format!("{} - {}.lnk", product::LEGACY_NAME, product::PUBLISHER)),
        ])
    }

    fn arguments(self) -> &'static str {
        match self {
            // The app minimizes itself when it sees this (backend/src/lib.rs).
            Shortcut::Startup => "--autostart",
            _ => "",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Shortcut::Desktop => "Desktop",
            Shortcut::StartMenu => "Start Menu",
            Shortcut::Startup => "Startup",
        }
    }
}

/// Runs `work` with COM initialized on this thread (shortcuts need it).
fn with_com<T>(work: impl FnOnce() -> windows::core::Result<T>) -> Result<T, String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    // SAFETY: balanced below when initialization succeeded (S_OK or S_FALSE).
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = work().map_err(|e| e.message());
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

/// The details Windows has saved for one shortcut.
struct ShortcutDetails {
    target: PathBuf,
    working_directory: PathBuf,
    description: String,
    icon: PathBuf,
    arguments: String,
}

fn text(buffer: &[u16]) -> String {
    let length = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn path(buffer: &[u16]) -> PathBuf {
    let length = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    PathBuf::from(OsString::from_wide(&buffer[..length]))
}

fn icon_file(icon: &str) -> PathBuf {
    let path = icon
        .rsplit_once(',')
        .filter(|(_, index)| index.trim().parse::<i32>().is_ok())
        .map_or(icon, |(path, _)| path);
    PathBuf::from(path)
}

fn managed_link(link: &ShortcutDetails, exe: &Path) -> bool {
    same_file(&link.target, exe)
        || (link.description.eq_ignore_ascii_case(product::NAME)
            && exe
                .parent()
                .is_some_and(|folder| same_file(&link.working_directory, folder))
            && same_file(&link.icon, exe))
}

fn legacy_managed_link(link: &ShortcutDetails, exe: &Path) -> bool {
    let old = exe.with_file_name(product::legacy_exe_name());
    same_file(&link.target, &old)
        || (link.description.eq_ignore_ascii_case(product::LEGACY_NAME)
            && exe.parent().is_some_and(|folder| same_file(&link.working_directory, folder))
            && same_file(&link.icon, &old))
}

fn valid_link(link: &ShortcutDetails, kind: Shortcut, exe: &Path) -> bool {
    same_file(&link.target, exe) && link.arguments == kind.arguments()
}

/// A shortcut with our name that is not ours but may be taken over: one an
/// earlier install of the app left elsewhere (same program name), or a dead
/// link whose program is gone. Without this a new install put its shortcut
/// under the fallback name, next to the old one, which the Start menu then
/// listed (or hid, when dead) instead of ours.
fn replaceable_link(link: &ShortcutDetails, exe: &Path) -> bool {
    let same_program = match (link.target.file_name(), exe.file_name()) {
        (Some(target), Some(ours)) => target.eq_ignore_ascii_case(ours),
        _ => false,
    };
    let dead = link.target.as_os_str().is_empty() || !link.target.is_file();
    same_program || dead
}

fn details_at(link_path: &Path) -> Option<ShortcutDetails> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH, ShellLink};
    use windows::core::{HSTRING, Interface};

    if !link_path.is_file() {
        return None;
    }
    with_com(|| unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.cast::<IPersistFile>()?
            .Load(&HSTRING::from(link_path.as_os_str()), STGM_READ)?;

        // On the heap: together these are 260 KB, and the silent install
        // runs on the main thread, which has a 1 MB stack.
        let mut target = vec![0u16; 32_768];
        let mut working_directory = vec![0u16; 32_768];
        let mut description = vec![0u16; 1_024];
        let mut icon = vec![0u16; 32_768];
        let mut arguments = vec![0u16; 32_768];
        let mut icon_index = 0;
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)?;
        link.GetWorkingDirectory(&mut working_directory)?;
        link.GetDescription(&mut description)?;
        link.GetIconLocation(&mut icon, &mut icon_index)?;
        link.GetArguments(&mut arguments)?;

        Ok(ShortcutDetails {
            target: path(&target),
            working_directory: PathBuf::from(text(&working_directory)),
            description: text(&description),
            icon: icon_file(&text(&icon)),
            arguments: text(&arguments),
        })
    })
    .ok()
}

/// Where a new shortcut of this kind goes: the usual name when it is free,
/// ours, or replaceable; else the fallback name when that is free or ours.
fn link_path_for(kind: Shortcut, exe: &Path) -> Result<PathBuf, String> {
    let main = kind
        .path()
        .ok_or("the shortcut folder could not be found")?;
    let main_usable = match details_patiently(&main) {
        Some(link) => managed_link(&link, exe) || replaceable_link(&link, exe),
        // Not a shortcut Windows can read: only usable if nothing is there.
        None => !main.exists(),
    };
    if main_usable {
        return Ok(main);
    }
    kind.fallback_path()
        .filter(|fallback| {
            details_patiently(fallback).map_or(!fallback.exists(), |link| managed_link(&link, exe))
        })
        .ok_or_else(|| {
            format!(
                "A different shortcut already uses the {} shortcut name. Rename it and try again.",
                kind.label()
            )
        })
}

/// A shortcut that is there but could not be read yet (the shell or a
/// scanner has it open) is read again before it counts as someone else's.
fn details_patiently(link_path: &Path) -> Option<ShortcutDetails> {
    for attempt in 0..SHORTCUT_ATTEMPTS {
        if !link_path.is_file() {
            return None;
        }
        if let Some(details) = details_at(link_path) {
            return Some(details);
        }
        std::thread::sleep(SHORTCUT_RETRY_WAIT * (attempt as u32 + 1));
    }
    None
}

/// One `IShellLinkW` write.
fn write_link(kind: Shortcut, exe: &Path, link_path: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    let folder = exe.parent().unwrap_or(exe);
    with_com(|| unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()))?;
        link.SetArguments(&HSTRING::from(kind.arguments()))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from(product::NAME))?;
        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(link_path.as_os_str()), true)
    })
}

/// Tries per shortcut. The Start menu, the Startup folder and virus scanners
/// all open a new `.lnk` the moment it appears, which can make the save or
/// the read-back fail for a short while.
const SHORTCUT_ATTEMPTS: u64 = 4;
const SHORTCUT_RETRY_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

/// Writes (or rewrites) a shortcut and checks what Windows persisted.
pub fn create_shortcut(kind: Shortcut, exe: &Path) -> Result<(), String> {
    let link_path = link_path_for(kind, exe)?;
    if let Some(folder) = link_path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("The {} folder could not be created: {e}", kind.label()))?;
    }

    let mut last_error = String::new();
    for attempt in 0..SHORTCUT_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(SHORTCUT_RETRY_WAIT * attempt as u32);
        }
        match write_link(kind, exe, &link_path) {
            Ok(()) if details_at(&link_path).is_some_and(|link| valid_link(&link, kind, exe)) => {
                // Written under the usual name: an older copy of ours under
                // the fallback name would now be a second entry.
                if let Some(fallback) = kind.fallback_path()
                    && fallback != link_path
                    && details_at(&fallback).is_some_and(|link| managed_link(&link, exe))
                {
                    let _ = std::fs::remove_file(fallback);
                }
                // A renamed shortcut must not leave the old app name beside it.
                for legacy in kind.legacy_paths().into_iter().flatten() {
                    if details_at(&legacy).is_some_and(|link| legacy_managed_link(&link, exe)) {
                        let _ = std::fs::remove_file(legacy);
                    }
                }
                return Ok(());
            }
            Ok(()) => {
                last_error = "Windows saved it without a working target".into();
            }
            Err(error) => last_error = error,
        }
    }
    Err(format!(
        "The {} shortcut could not be created: {last_error}.",
        kind.label()
    ))
}

/// The shortcuts of this kind that belong to the app: ours under either
/// name, including old broken links which still have our description, icon
/// and working directory but no target, and a replaceable one under the
/// usual name (see `replaceable_link`).
fn our_links(kind: Shortcut, exe: &Path) -> Vec<PathBuf> {
    let main = kind.path().filter(|main| {
        details_at(main)
            .is_some_and(|link| managed_link(&link, exe) || replaceable_link(&link, exe))
    });
    let fallback = kind
        .fallback_path()
        .filter(|fallback| details_at(fallback).is_some_and(|link| managed_link(&link, exe)));
    let legacy = kind.legacy_paths().into_iter().flatten().filter(|path| {
        details_at(path).is_some_and(|link| legacy_managed_link(&link, exe))
    });
    main.into_iter().chain(fallback).chain(legacy).collect()
}

/// Whether the app has this kind of shortcut, working or not.
pub fn shortcut_present(kind: Shortcut, exe: &Path) -> bool {
    !our_links(kind, exe).is_empty()
}

pub fn remove_shortcut(kind: Shortcut, exe: &Path) {
    for path in our_links(kind, exe) {
        let _ = std::fs::remove_file(path);
    }
}

/// The path with 8.3 short names expanded and links resolved, without the
/// `\\?\` prefix `canonicalize` adds; unchanged when it does not exist.
pub fn real_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(real) => {
            let text = real.to_string_lossy();
            match text.strip_prefix(r"\\?\UNC\") {
                Some(unc) => PathBuf::from(format!(r"\\{unc}")),
                None => PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)),
            }
        }
        Err(_) => path.to_path_buf(),
    }
}

/// Same file, however either path is spelled (short names, case, slashes).
pub fn same_file(a: &Path, b: &Path) -> bool {
    same_path(a, b) || same_path(&real_path(a), &real_path(b))
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let normal = |p: &Path| {
        let text = p.to_string_lossy().replace('/', "\\");
        text.trim_end_matches('\\').to_lowercase()
    };
    normal(a) == normal(b)
}

/// Whether `path` is `dir` or inside it.
pub fn is_within(path: &Path, dir: &Path) -> bool {
    let normal = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    let (path, dir) = (normal(path), normal(dir));
    let dir = dir.trim_end_matches('\\');
    path == dir || path.starts_with(&format!("{dir}\\"))
}

pub fn message(title: &str, text: &str, error: bool) {
    let style = if error { MB_ICONERROR } else { MB_ICONWARNING };
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | style,
        )
    };
}

pub fn ask(title: &str, text: &str) -> bool {
    let (title, text) = (wide(title), wide(text));
    // SAFETY: as above.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING,
        )
    };
    answer == IDYES
}

/// Held for the whole run: one setup or uninstaller at a time, per user.
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    pub fn acquire() -> Option<Self> {
        let name = wide(format!("Local\\{}.Setup", product::LEGACY_BINARY));
        // SAFETY: a named mutex; the handle is closed on drop.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            // Could not even ask: better to run than to refuse.
            return Some(Self(handle));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self(handle))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Opens an https page in the user's default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https pages are opened.".into());
    }
    let (verb, file) = (wide("open"), wide(url));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean it worked.
    if result as isize > 32 {
        Ok(())
    } else {
        Err("The browser could not be opened.".into())
    }
}

/// The argument the app reads as "an update just put me here": its splash
/// then skips asking the network for a newer version, and opens sooner
/// (`JUST_UPDATED_ARG` in the app's `updater.rs`).
pub const JUST_UPDATED_ARG: &str = "--just-updated";

/// Starts the installed app, detached from this process, and returns its
/// process ID. `updated`: this run was an update (`/UPDATE`).
pub fn launch(exe: &Path, updated: bool) -> Result<u32, String> {
    std::process::Command::new(exe)
        .args(updated.then_some(JUST_UPDATED_ARG))
        .current_dir(exe.parent().unwrap_or(exe))
        .spawn()
        .map(|child| child.id())
        .map_err(|e| format!("The app could not be started: {e}"))
}

/// A console program run without a window, for its exit code only.
pub fn run_hidden(program: &Path, args: &[&str]) -> Option<i32> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .and_then(|status| status.code())
}

pub fn system32(program: &str) -> PathBuf {
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(windows).join("System32").join(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(
        target: &str,
        folder: &str,
        description: &str,
        icon: &str,
        arguments: &str,
    ) -> ShortcutDetails {
        ShortcutDetails {
            target: PathBuf::from(target),
            working_directory: PathBuf::from(folder),
            description: description.to_string(),
            icon: icon_file(icon),
            arguments: arguments.to_string(),
        }
    }

    #[test]
    fn recognizes_our_old_targetless_shortcut_for_repair() {
        let exe = Path::new(
            r"C:\Users\Thomas\AppData\Local\ThomasThanos\MakeYourLifeEasier\MakeYourLifeEasier.exe",
        );
        let folder = exe.parent().unwrap().to_string_lossy();
        let broken = link(
            "",
            &folder,
            product::NAME,
            &format!("{},0", exe.display()),
            "--autostart",
        );
        assert!(managed_link(&broken, exe));
        assert!(!valid_link(&broken, Shortcut::Startup, exe));
    }

    #[test]
    fn recognizes_the_previous_name_only_for_our_install() {
        let exe = Path::new(r"C:\Apps\MakeYourLifeEasier\MYLE.exe");
        let old = exe.with_file_name(product::legacy_exe_name());
        let same_install = link(
            old.to_str().unwrap(),
            exe.parent().unwrap().to_str().unwrap(),
            product::LEGACY_NAME,
            &format!("{},0", old.display()),
            "",
        );
        assert!(legacy_managed_link(&same_install, exe));
        let unrelated = link(
            r"C:\Other\MakeYourLifeEasier.exe",
            r"C:\Other",
            product::LEGACY_NAME,
            r"C:\Other\MakeYourLifeEasier.exe,0",
            "",
        );
        assert!(!legacy_managed_link(&unrelated, exe));
    }

    #[test]
    fn does_not_claim_a_different_shortcut_with_the_same_name() {
        let exe = Path::new(
            r"C:\Users\Thomas\AppData\Local\ThomasThanos\MakeYourLifeEasier\MakeYourLifeEasier.exe",
        );
        let other = link(
            r"C:\Tools\Other.exe",
            r"C:\Tools",
            "Other app",
            r"C:\Tools\Other.exe,0",
            "",
        );
        assert!(!managed_link(&other, exe));
    }

    #[test]
    fn validates_startup_arguments_and_strips_the_icon_index() {
        let exe = Path::new(r"C:\Apps\MakeYourLifeEasier.exe");
        let folder = exe.parent().unwrap().to_string_lossy();
        let startup = link(
            exe.to_str().unwrap(),
            &folder,
            product::NAME,
            &format!("{},0", exe.display()),
            "--autostart",
        );
        let start_menu = link(
            exe.to_str().unwrap(),
            &folder,
            product::NAME,
            &format!("{},0", exe.display()),
            "",
        );
        assert!(valid_link(&startup, Shortcut::Startup, exe));
        assert!(valid_link(&start_menu, Shortcut::StartMenu, exe));
        assert_eq!(icon_file(r"C:\Apps\MakeYourLifeEasier.exe,0"), exe);
    }

    #[test]
    fn takes_over_old_or_dead_links_but_not_a_working_other_app() {
        let exe = Path::new(
            r"C:\Users\Thomas\AppData\Local\ThomasThanos\MakeYourLifeEasier\MakeYourLifeEasier.exe",
        );
        // An earlier install of the app in another folder.
        let older = link(
            r"C:\Users\Thomas\AppData\Local\Make Your Life Easier\MakeYourLifeEasier.exe",
            r"C:\Users\Thomas\AppData\Local\Make Your Life Easier",
            "",
            "",
            "",
        );
        assert!(!managed_link(&older, exe));
        assert!(replaceable_link(&older, exe));

        // A dead link: its program is gone.
        let dead = link(r"C:\Nowhere\Gone\Missing.exe", r"C:\Nowhere", "", "", "");
        assert!(replaceable_link(&dead, exe));
        assert!(replaceable_link(&link("", "", "", "", ""), exe));

        // Someone else's working shortcut that happens to share the name.
        let cmd = system32("cmd.exe");
        let other = link(
            cmd.to_str().unwrap(),
            cmd.parent().unwrap().to_str().unwrap(),
            "Notes",
            "",
            "",
        );
        assert!(!replaceable_link(&other, exe));
    }

    #[test]
    fn paths_compare_like_windows_does() {
        assert!(same_path(
            Path::new(r"C:\Apps\X.exe"),
            Path::new(r"c:/apps/x.EXE")
        ));
        assert!(is_within(
            Path::new(r"C:\Apps\MYLE\ludusavi\l.exe"),
            Path::new(r"C:\apps\myle\")
        ));
        assert!(is_within(
            Path::new(r"C:\Apps\MYLE"),
            Path::new(r"C:\Apps\MYLE")
        ));
        assert!(!is_within(
            Path::new(r"C:\Apps\MYLE2\x.exe"),
            Path::new(r"C:\Apps\MYLE")
        ));
    }
}
