// ! Removing scan items, using the Recycle Bin by default. ! ! The shell's IFileOperation does the move, one item per operation. A ! progress sink watches each one: when the shell is about to delete an item ! without the "recycle if possible" flag (too big for the bin, or a drive ! without one), the sink aborts it, so it is skipped and reported instead ! of silently deleted for good. A separate, explicitly confirmed command ! can request permanent deletion. Every item is checked again right before it ! goes: still there, still inside the scanned folder, not a link, not ! protected, and still what the scan saw.
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use windows::Win32::Foundation::{E_ABORT, E_ACCESSDENIED};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx,
};
use windows::Win32::UI::Shell::{
    COPYENGINE_E_ACCESS_DENIED_SRC, COPYENGINE_E_RECYCLE_BIN_NOT_FOUND, COPYENGINE_E_RECYCLE_FORCE_NUKE,
    COPYENGINE_E_RECYCLE_PATH_TOO_LONG, COPYENGINE_E_RECYCLE_SIZE_TOO_BIG, COPYENGINE_E_REQUIRES_ELEVATION,
    COPYENGINE_E_SHARING_VIOLATION_SRC, FILEOPERATION_FLAGS, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOFX_RECYCLEONDELETE,
    FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName,
    TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows::core::{HRESULT, PCWSTR, Ref, implement};
use super::protect::{self, Guard, Verdict};
use super::tree::{display_path, long_path};
pub const OUTSIDE: &str = "It is not inside the scanned folder.";
pub const GONE: &str = "It is no longer there.";
pub const LINK: &str = "A link: MYLE never removes links.";
pub const CHANGED: &str = "It changed since the scan. Scan again.";
pub const NO_BIN: &str =
    "Windows cannot put this item in the Recycle Bin. It may exceed the bin's size limit, or this drive may not support recycling.";
pub const ADMIN: &str = "Needs administrator.";
pub const IN_USE: &str = "Open in another program.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Recycle,
    Permanent,
}
/// The shell reports estimated work points: total first, completed second.
pub type OnProgress = Arc<dyn Fn(u32, u32) + Send + Sync>;
/// COM for this thread, apartment-threaded, as the shell wants.
pub struct Com(bool);
impl Com {
    pub fn init() -> Self {
        // SAFETY: paired with CoUninitialize in Drop when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Self(hr.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread's own successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}
#[derive(Default)]
struct Watch {
    /// The shell wanted to delete something for good.
    refused: AtomicBool,
    /// The failure the shell reported for the item, if any.
    failed: AtomicI32,
}
/// Normal deletion cannot fall back to permanent deletion. Only the separate
/// permanently-delete command, after its own confirmation, selects that mode.
#[implement(IFileOperationProgressSink)]
struct RemovalSink {
    watch: Arc<Watch>,
    mode: Mode,
    progress: Option<OnProgress>,
}
impl IFileOperationProgressSink_Impl for RemovalSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, result: HRESULT) -> windows::core::Result<()> {
        if result.is_err() {
            let _ = self.watch.failed.compare_exchange(0, result.0, Ordering::SeqCst, Ordering::SeqCst);
        }
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR, _: HRESULT, _: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(&self, _: u32, _: Ref<'_, IShellItem>, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(&self, _: u32, _: Ref<'_, IShellItem>, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, flags: u32, _: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if self.mode == Mode::Recycle && flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            // It would be gone for good: stop it.
            self.watch.refused.store(true, Ordering::SeqCst);
            return Err(E_ABORT.into());
        }
        Ok(())
    }
    fn PostDeleteItem(&self, _: u32, _: Ref<'_, IShellItem>, result: HRESULT, _: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        if result.is_err() {
            let _ = self.watch.failed.compare_exchange(0, result.0, Ordering::SeqCst, Ordering::SeqCst);
        }
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, total: u32, done: u32) -> windows::core::Result<()> {
        if let Some(progress) = &self.progress {
            progress(total, done);
        }
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}
fn describe(hr: HRESULT) -> String {
    match hr {
        h if h == E_ACCESSDENIED || h == COPYENGINE_E_ACCESS_DENIED_SRC || h == COPYENGINE_E_REQUIRES_ELEVATION => ADMIN.into(),
        h if h == COPYENGINE_E_SHARING_VIOLATION_SRC || h.0 as u32 == 0x8007_0020 || h.0 as u32 == 0x8007_0021 => IN_USE.into(),
        h if h == COPYENGINE_E_RECYCLE_BIN_NOT_FOUND
            || h == COPYENGINE_E_RECYCLE_SIZE_TOO_BIG
            || h == COPYENGINE_E_RECYCLE_FORCE_NUKE
            || h == COPYENGINE_E_RECYCLE_PATH_TOO_LONG =>
        {
            NO_BIN.into()
        }
        h => format!("Windows could not remove this item (0x{:08X}).", h.0 as u32),
    }
}
/// Checks an item again right before it goes. `was_dir`: what the scan saw.
pub fn recheck(path: &Path, was_dir: bool, root: &Path, guard: &Guard) -> Result<(), String> {
    let (item, root) = (protect::parts(path), protect::parts(root));
    if item.len() <= root.len() || item[..root.len()] != root[..] {
        return Err(OUTSIDE.into());
    }
    let meta = std::fs::symlink_metadata(long_path(path)).map_err(|_| GONE.to_string())?;
    // Junctions and symbolic links; a cloud placeholder is not one.
    if meta.file_type().is_symlink() {
        return Err(LINK.into());
    }
    if meta.is_dir() != was_dir {
        return Err(CHANGED.into());
    }
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    match guard.check(path, meta.file_attributes() & FILE_ATTRIBUTE_SYSTEM != 0) {
        Verdict::Protected(why) => Err(why.into_owned()),
        Verdict::Allowed | Verdict::Caution(_) => Ok(()),
    }
}
/// Moves one item to the Recycle Bin. COM must be initialised on this thread
/// (`Com::init`). An item the bin cannot take stays where it is.
pub fn recycle(path: &Path, progress: Option<OnProgress>) -> Result<(), String> {
    remove(path, Mode::Recycle, progress)
}
/// Only called by the distinct permanently-delete command. Its caller applies
/// exactly the same scanned-path and protection checks as the Recycle Bin flow.
pub fn permanently_delete(path: &Path, progress: Option<OnProgress>) -> Result<(), String> {
    remove(path, Mode::Permanent, progress)
}
fn remove(path: &Path, mode: Mode, progress: Option<OnProgress>) -> Result<(), String> {
    let shown = display_path(path);
    // The shell takes the usual form; `\\?\` only past MAX_PATH.
    let parse = if shown.as_os_str().len() < 260 { shown } else { long_path(path) };
    let mut wide: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(parse.as_os_str()).collect();
    wide.push(0);
    let watch = Arc::new(Watch::default());
    // SAFETY: COM calls on this thread's apartment; `wide` outlives them.
    let performed = unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER).map_err(|e| describe(e.code()))?;
        let mut flags = FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0;
        if mode == Mode::Recycle {
            flags |= FOFX_RECYCLEONDELETE.0;
        }
        operation.SetOperationFlags(FILEOPERATION_FLAGS(flags)).map_err(|e| describe(e.code()))?;
        let sink: IFileOperationProgressSink = RemovalSink { watch: Arc::clone(&watch), mode, progress }.into();
        let cookie = operation.Advise(&sink).map_err(|e| describe(e.code()))?;
        let item: windows::core::Result<IShellItem> = SHCreateItemFromParsingName(PCWSTR::from_raw(wide.as_ptr()), None::<&IBindCtx>);
        let result = match item {
            Ok(item) => operation
                .DeleteItem(&item, None::<&IFileOperationProgressSink>)
                .and_then(|()| operation.PerformOperations())
                .and_then(|()| operation.GetAnyOperationsAborted().map(|aborted| aborted.as_bool())),
            Err(err) => Err(err),
        };
        let _ = operation.Unadvise(cookie);
        result
    };
    if watch.refused.load(Ordering::SeqCst) {
        return Err(NO_BIN.into());
    }
    let failed = watch.failed.load(Ordering::SeqCst);
    if failed != 0 {
        return Err(describe(HRESULT(failed)));
    }
    match performed {
        Ok(false) => {}
        Ok(true) => return Err("Windows stopped it.".into()),
        Err(e) if e.code() == windows::Win32::Foundation::E_INVALIDARG => {
            return Err(if std::fs::symlink_metadata(long_path(path)).is_ok() {
                "Windows could not handle this path (0x80070057).".into()
            } else {
                GONE.into()
            });
        }
        Err(e) => return Err(describe(e.code())),
    }
    if std::fs::symlink_metadata(long_path(path)).is_ok() {
        return Err("It is still there.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::super::protect::Places;
    use super::*;
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Temp {
        /// A test folder by its full, long name (the Recycle Bin records it so).
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("myle-recycle-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(display_path(&std::fs::canonicalize(&path).unwrap()))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn guard(root: &Path) -> Guard {
        Guard::new(root, &Places::default())
    }
    /// Removes what a test put in the Recycle Bin: `$I…` names the original
    /// path, `$R…` (same suffix) is the item itself.
    fn purge_from_bin(original: &Path) {
        let text = original.to_string_lossy().to_string();
        let wanted: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let bin = PathBuf::from(&text[..3]).join("$Recycle.Bin");
        let Ok(users) = std::fs::read_dir(&bin) else {
            return;
        };
        for user in users.flatten() {
            let Ok(entries) = std::fs::read_dir(user.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(suffix) = name.strip_prefix("$I") else {
                    continue;
                };
                let Ok(info) = std::fs::read(entry.path()) else {
                    continue;
                };
                if info.windows(wanted.len()).any(|w| w == wanted.as_slice()) {
                    let item = user.path().join(format!("$R{suffix}"));
                    let _ = std::fs::remove_dir_all(&item).or_else(|_| std::fs::remove_file(&item));
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    /// Needs the Windows shell and a Recycle Bin on the temp folder's drive.
    /// Either the item went to the bin (and is cleaned out of it here), or
    /// the bin could not take it and it is still on disk: never gone for good.
    #[test]
    fn a_file_and_a_folder_go_to_the_recycle_bin_with_the_shell() {
        let temp = Temp::new("shell");
        let file = temp.0.join("old.log");
        std::fs::write(&file, b"bytes").unwrap();
        let folder = temp.0.join("cache");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("inner").join("a.bin"), b"123").unwrap();
        let _com = Com::init();
        for (path, is_dir) in [(&file, false), (&folder, true)] {
            recheck(path, is_dir, &temp.0, &guard(&temp.0)).unwrap();
            match recycle(path, None) {
                Ok(()) => {
                    assert!(!path.exists());
                    purge_from_bin(path);
                }
                Err(reason) => {
                    eprintln!("not recycled here: {reason}");
                    assert!(path.exists(), "an item the bin did not take is left in place");
                }
            }
        }
    }
    /// Run by hand (`cargo test … -- --ignored`): it maps a drive letter for a
    /// moment. A `subst` drive has no Recycle Bin, so the shell would delete
    /// the file for good: the guard must stop it and leave the file.
    #[test]
    #[ignore]
    fn the_guard_keeps_what_the_bin_cannot_take() {
        use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
        let temp = Temp::new("no-bin");
        std::fs::write(temp.0.join("keep.me"), b"precious").unwrap();
        // SAFETY: no arguments.
        let used = unsafe { GetLogicalDrives() };
        let Some(letter) = (b'M'..=b'Y').rev().find(|l| used & (1 << (l - b'A')) == 0).map(char::from) else {
            eprintln!("skipped: no free drive letter");
            return;
        };
        let subst = crate::apps::process::system32("subst.exe");
        let drive = format!("{letter}:");
        if !std::process::Command::new(&subst).arg(&drive).arg(&temp.0).status().is_ok_and(|s| s.success()) {
            eprintln!("skipped: subst failed");
            return;
        }
        let _com = Com::init();
        let result = recycle(&PathBuf::from(format!(r"{drive}\keep.me")), None);
        let _ = std::process::Command::new(&subst).arg(&drive).arg("/D").status();
        eprintln!("recycling from {drive}: {result:?}");
        assert_eq!(result.unwrap_err(), NO_BIN);
        assert_eq!(std::fs::read(temp.0.join("keep.me")).unwrap(), b"precious");
    }
    #[test]
    fn a_path_outside_the_scanned_folder_is_refused() {
        let temp = Temp::new("outside");
        let other = Temp::new("other");
        let file = other.0.join("x.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(recheck(&file, false, &temp.0, &guard(&temp.0)).unwrap_err(), OUTSIDE);
        // `..` does not get out either.
        let sneaky = temp.0.join("..").join(other.0.file_name().unwrap()).join("x.txt");
        assert_eq!(recheck(&sneaky, false, &temp.0, &guard(&temp.0)).unwrap_err(), OUTSIDE);
        assert_eq!(recheck(&temp.0, true, &temp.0, &guard(&temp.0)).unwrap_err(), OUTSIDE, "the root itself");
    }
    #[test]
    fn a_protected_path_is_refused() {
        let temp = Temp::new("protected");
        let keep = temp.0.join("keep");
        std::fs::create_dir_all(keep.join("inside")).unwrap();
        let places = Places { myle: vec![keep.clone()], ..Places::default() };
        let guard = Guard::new(&temp.0, &places);
        assert!(recheck(&keep, true, &temp.0, &guard).is_err());
        assert!(recheck(&keep.join("inside"), true, &temp.0, &guard).is_err());
    }
    #[test]
    fn a_link_is_refused_and_what_it_points_at_stays() {
        let temp = Temp::new("link");
        let target = Temp::new("link-target");
        std::fs::write(target.0.join("precious.txt"), b"x").unwrap();
        let link = temp.0.join("jump");
        let made = std::process::Command::new(crate::apps::process::system32("cmd.exe"))
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target.0)
            .output()
            .is_ok_and(|out| out.status.success());
        if !made {
            eprintln!("skipped: mklink /J is not available here");
            return;
        }
        assert_eq!(recheck(&link, true, &temp.0, &guard(&temp.0)).unwrap_err(), LINK);
        assert!(target.0.join("precious.txt").exists());
    }
    #[test]
    fn a_path_that_changed_since_the_scan_is_refused() {
        let temp = Temp::new("changed");
        let item = temp.0.join("item");
        std::fs::write(&item, b"x").unwrap();
        // Scanned as a folder, a file now.
        assert_eq!(recheck(&item, true, &temp.0, &guard(&temp.0)).unwrap_err(), CHANGED);
        std::fs::remove_file(&item).unwrap();
        assert_eq!(recheck(&item, false, &temp.0, &guard(&temp.0)).unwrap_err(), GONE);
    }
    #[test]
    fn shell_failures_read_plainly() {
        assert_eq!(describe(E_ACCESSDENIED), ADMIN);
        assert_eq!(describe(COPYENGINE_E_SHARING_VIOLATION_SRC), IN_USE);
        assert_eq!(describe(COPYENGINE_E_RECYCLE_BIN_NOT_FOUND), NO_BIN);
        assert!(describe(HRESULT(0x8000_4005u32 as i32)).contains("0x80004005"));
    }
    #[test]
    fn items_too_large_for_the_bin_have_an_actionable_reason() {
        use windows::Win32::UI::Shell::{COPYENGINE_E_RECYCLE_FORCE_NUKE, COPYENGINE_E_RECYCLE_SIZE_TOO_BIG};
        assert_eq!(describe(COPYENGINE_E_RECYCLE_SIZE_TOO_BIG), NO_BIN);
        assert_eq!(describe(COPYENGINE_E_RECYCLE_FORCE_NUKE), NO_BIN);
    }
    #[test]
    fn explicit_permanent_deletion_removes_its_test_file_and_folder() {
        let temp = Temp::new("permanent");
        let file = temp.0.join("test-only.txt");
        let folder = temp.0.join("test-only-folder");
        std::fs::write(&file, b"temporary test data").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("child.txt"), b"temporary test data").unwrap();
        let _com = Com::init();
        let guard = guard(&temp.0);
        for (item, is_dir) in [(&file, false), (&folder, true)] {
            recheck(item, is_dir, &temp.0, &guard).unwrap();
            permanently_delete(item, None).unwrap();
            assert!(!item.exists(), "the explicitly deleted test item must be removed");
        }
        assert!(temp.0.exists(), "the scanned root stays in place");
    }
    #[test]
    fn progress_sink_forwards_windows_work_estimates_in_the_correct_order() {
        let updates = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = Arc::clone(&updates);
        let sink: IFileOperationProgressSink = RemovalSink {
            watch: Arc::new(Watch::default()),
            mode: Mode::Recycle,
            progress: Some(Arc::new(move |total, done| received.lock().unwrap().push((total, done)))),
        }
        .into();
        // SAFETY: a call to our own progress sink; no file operation is started.
        unsafe {
            sink.UpdateProgress(100, 37).unwrap();
            sink.UpdateProgress(0, 0).unwrap();
        }
        assert_eq!(*updates.lock().unwrap(), [(100, 37), (0, 0)]);
    }
}
