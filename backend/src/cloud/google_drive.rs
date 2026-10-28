// ! Google Drive for desktop keeps its drive (`G:`) in a file system of its ! own, outside the Windows cloud files API, and offers one way to make files ! online-only: its Explorer menu, "Offline access" > "Online only". This ! asks that menu, as a right-click would. Online-only files still fill ! Drive's cache, and only Drive's own "Clear cache" button empties it: that ! is pressed through UI Automation while Drive's storage window is open. ! Google documents none of it, so when either changes, the page says so and ! shows Drive's own steps.
use std::path::Path;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    IDataObject,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    TreeScope_Children, TreeScope_Descendants, UIA_ButtonControlTypeId, UIA_InvokePatternId,
    UIA_TextControlTypeId,
};
use windows::Win32::UI::Shell::{
    BHID_DataObject, CMF_NORMAL, CMINVOKECOMMANDINFO, IContextMenu, IShellExtInit, IShellItem,
    SHCreateItemFromParsingName, SHGetIDListFromObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW,
    MFS_CHECKED, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU,
    SW_SHOWNORMAL,
};
use windows::core::{GUID, HSTRING, Interface, PCSTR, PWSTR};
use winreg::RegKey;
use winreg::enums::HKEY_CLASSES_ROOT;
/// The first command id given to Drive's menu.
const FIRST: u32 = 1;
/// Drive's own number for "Online only" (counted from its first command),
/// for when the menu is not in English. "Available offline" is 7.
const ONLINE_ONLY: u32 = 8;
/// One command of a submenu.
#[derive(Debug)]
pub(super) struct Choice {
    /// Counted from Drive's first command, as `InvokeCommand` wants it.
    pub id: u32,
    pub text: String,
    pub checked: bool,
}
/// "Online only" among the menu's submenus: the second of a submenu of
/// exactly two ("Available offline", "Online only"), named so or numbered as
/// Drive numbers it. None when the menu no longer looks like that.
pub(super) fn online_only_choice(submenus: &[Vec<Choice>]) -> Option<&Choice> {
    let named = |choice: &Choice, name: &str| choice.text.replace('&', "").eq_ignore_ascii_case(name);
    let mut found = submenus
        .iter()
        .filter(|choices| choices.len() == 2 && !named(&choices[0], "Online only"))
        .map(|choices| &choices[1])
        .filter(|choice| {
            !named(choice, "Available offline") && (named(choice, "Online only") || choice.id == ONLINE_ONLY)
        });
    let choice = found.next()?;
    found.next().is_none().then_some(choice)
}
/// Drive's "Storage on your computer" window shows this page, in any language.
const STORAGE_PAGE: &str = "file_stats_dialog.html";
/// What pressing "Clear cache" found.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "size")]
pub(super) enum CacheClear {
    /// Drive's storage window isn't open.
    NotOpen,
    /// The button is greyed out: nothing to clear.
    Empty,
    /// Pressed. The cache's size as the window showed it before ("94.4 GB").
    Cleared(Option<String>),
}
/// "Clear cache" among the storage window's buttons (name and class): by
/// name, or, in another language, the second of its four dialog buttons
/// ("Clear offline files", "Clear cache", "Go to cache settings", "Done").
pub(super) fn clear_cache_button(buttons: &[(String, String)]) -> Option<usize> {
    let named: Vec<_> = (0..buttons.len()).filter(|&i| buttons[i].0.eq_ignore_ascii_case("Clear cache")).collect();
    if let [index] = named[..] {
        return Some(index);
    }
    let dialog: Vec<_> = (0..buttons.len()).filter(|&i| buttons[i].1.split(' ').any(|class| class == "dfs-dialog-button")).collect();
    match dialog[..] {
        [_, index, _, _] if named.is_empty() => Some(index),
        _ => None,
    }
}
/// Presses "Clear cache" in Drive's storage window, when it is open. Drive
/// then removes what it may (files still uploading and files kept offline stay).
pub(super) fn clear_cache() -> Result<CacheClear, String> {
    on_com_thread(COINIT_MULTITHREADED, || {
        let failed = |error: windows::core::Error| error.message();
        // SAFETY: COM is set up on this thread; UI Automation's own objects.
        unsafe {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(failed)?;
            let Some(page) = storage_page(&automation).map_err(failed)? else {
                return Ok(CacheClear::NotOpen);
            };
            let all = page
                .FindAll(TreeScope_Descendants, &automation.CreateTrueCondition().map_err(failed)?)
                .map_err(failed)?;
            let mut buttons = Vec::new();
            let mut sizes = Vec::new();
            for index in 0..all.Length().map_err(failed)? {
                let element = all.GetElement(index).map_err(failed)?;
                let name = element.CurrentName().map(|name| name.to_string()).unwrap_or_default();
                let kind = element.CurrentControlType().map_err(failed)?;
                if kind == UIA_ButtonControlTypeId {
                    let class = element.CurrentClassName().map(|class| class.to_string()).unwrap_or_default();
                    buttons.push((element, name, class));
                } else if kind == UIA_TextControlTypeId && name.starts_with(|c: char| c.is_ascii_digit()) {
                    // "94.4 GB", "0 bytes": the last one is the cached files'.
                    sizes.push(name);
                }
            }
            let labels: Vec<_> = buttons.iter().map(|(_, name, class)| (name.clone(), class.clone())).collect();
            let index = clear_cache_button(&labels)
                .ok_or("Google Drive's storage window has no Clear cache button. Drive may have changed it.")?;
            let button = &buttons[index].0;
            if !button.CurrentIsEnabled().map_err(failed)?.as_bool() {
                return Ok(CacheClear::Empty);
            }
            button
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                .and_then(|invoke| invoke.Invoke())
                .map_err(failed)?;
            Ok(CacheClear::Cleared(sizes.pop()))
        }
    })
}
/// The page of Drive's storage window, among Drive's top-level windows.
unsafe fn storage_page(automation: &IUIAutomation) -> windows::core::Result<Option<IUIAutomationElement>> {
    // SAFETY: (for the whole body) calls on live UI Automation objects.
    unsafe {
        let everything = automation.CreateTrueCondition()?;
        let windows = automation.GetRootElement()?.FindAll(TreeScope_Children, &everything)?;
        for index in 0..windows.Length()? {
            let window = windows.GetElement(index)?;
            if !window.CurrentProcessId().is_ok_and(|id| is_google_drive(id as u32)) {
                continue;
            }
            let panes = window.FindAll(TreeScope_Children, &everything)?;
            for pane in 0..panes.Length()? {
                let pane = panes.GetElement(pane)?;
                if pane.CurrentName().is_ok_and(|name| name == STORAGE_PAGE) {
                    return Ok(Some(pane));
                }
            }
        }
        Ok(None)
    }
}
/// Whether the process runs `GoogleDriveFS.exe`.
fn is_google_drive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    // SAFETY: the handle is closed below; the buffer's length is passed with it.
    let ok = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) != 0;
        CloseHandle(process);
        ok
    };
    ok && String::from_utf16_lossy(&path[..length as usize])
        .to_lowercase()
        .ends_with(r"\googledrivefs.exe")
}
/// Whether "Online only" is on for a folder of Google Drive.
pub(super) fn is_online_only(path: &Path) -> Result<bool, String> {
    on_shell_thread(|| {
        let menu = Menu::open(path)?;
        Ok(menu.online_only()?.checked)
    })
}
/// Turns "Online only" on for a folder of Google Drive. False when it was
/// on already, and nothing was asked.
pub(super) fn make_online_only(path: &Path) -> Result<bool, String> {
    on_shell_thread(|| {
        let menu = Menu::open(path)?;
        let choice = menu.online_only()?;
        if choice.checked {
            return Ok(false);
        }
        let command = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            // The command by number, as Explorer passes it (MAKEINTRESOURCE).
            lpVerb: PCSTR(choice.id as usize as *const u8),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: a live menu from this handler; the structure outlives the call.
        unsafe { menu.menu.InvokeCommand(&command) }.map_err(|error| error.message())?;
        Ok(true)
    })
}
/// Runs `work` on a thread of its own with COM set up as the shell wants,
/// so no other thread's COM state is touched.
fn on_shell_thread<T: Send>(work: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    on_com_thread(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE, work)
}
fn on_com_thread<T: Send>(apartment: COINIT, work: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                // SAFETY: paired with CoUninitialize below when it succeeded.
                let com = unsafe { CoInitializeEx(None, apartment) };
                let result = work();
                if com.is_ok() {
                    // SAFETY: this thread's own successful CoInitializeEx.
                    unsafe { CoUninitialize() };
                }
                result
            })
            .join()
            .unwrap_or_else(|_| Err("Google Drive's menu stopped unexpectedly.".into()))
    })
}
/// Drive's Explorer menu handler (`drivefsext.dll`), under whatever key name
/// the installed version gave it.
fn handler() -> Option<GUID> {
    let root = RegKey::predef(HKEY_CLASSES_ROOT);
    let handlers = root.open_subkey(r"Directory\shellex\ContextMenuHandlers").ok()?;
    handlers.enum_keys().flatten().find_map(|name| {
        let clsid = handlers
            .open_subkey(&name)
            .ok()?
            .get_value::<String, _>("")
            .ok()
            .filter(|value| value.starts_with('{'))
            .unwrap_or(name);
        let server: String = root
            .open_subkey(format!(r"CLSID\{clsid}\InprocServer32"))
            .ok()?
            .get_value("")
            .ok()?;
        let server = server.trim_matches('"').to_lowercase();
        if !server.ends_with("drivefsext.dll") {
            return None;
        }
        GUID::try_from(clsid.trim_matches(['{', '}'])).ok()
    })
}
/// Drive's menu for one folder, built on this thread.
struct Menu {
    menu: IContextMenu,
    handle: HMENU,
}
impl Menu {
    fn open(path: &Path) -> Result<Self, String> {
        let handler = handler().ok_or("Google Drive's Explorer menu isn't installed.")?;
        let failed = |error: windows::core::Error| error.message();
        // SAFETY: COM is set up on this thread (on_shell_thread); every pointer passed lives for its call, and the folder's ID list is freed.
        unsafe {
            let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None).map_err(failed)?;
            let parent = item.GetParent().map_err(failed)?;
            let data: IDataObject = item.BindToHandler(None, &BHID_DataObject).map_err(failed)?;
            let folder = SHGetIDListFromObject(&parent).map_err(failed)?;
            let menu = CoCreateInstance::<_, IShellExtInit>(&handler, None, CLSCTX_INPROC_SERVER).and_then(|init| {
                init.Initialize(Some(folder), &data, None)?;
                init.cast::<IContextMenu>()
            });
            CoTaskMemFree(Some(folder as *const _));
            let menu = Self {
                menu: menu.map_err(failed)?,
                handle: CreatePopupMenu().map_err(failed)?,
            };
            menu.menu
                .QueryContextMenu(menu.handle, 0, FIRST, 0x7fff, CMF_NORMAL)
                .ok()
                .map_err(failed)?;
            Ok(menu)
        }
    }
    fn online_only(&self) -> Result<Choice, String> {
        let submenus: Vec<Vec<Choice>> = entries(self.handle)
            .into_iter()
            .filter_map(|entry| match entry {
                Entry::Submenu(menu) => Some(
                    entries(menu)
                        .into_iter()
                        .filter_map(|entry| match entry {
                            Entry::Command(choice) => Some(choice),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        let choice = online_only_choice(&submenus)
            .ok_or("Google Drive's menu has no Online only option for this folder. Drive may have changed it.")?;
        Ok(Choice {
            id: choice.id,
            text: choice.text.clone(),
            checked: choice.checked,
        })
    }
}
impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: created by CreatePopupMenu and destroyed only here.
        let _ = unsafe { DestroyMenu(self.handle) };
    }
}
enum Entry {
    Command(Choice),
    Submenu(HMENU),
    Separator,
}
fn entries(menu: HMENU) -> Vec<Entry> {
    // SAFETY: a live menu handle.
    let count = unsafe { GetMenuItemCount(Some(menu)) };
    (0..count.max(0) as u32)
        .filter_map(|index| {
            let mut text = [0u16; 256];
            let mut info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_ID | MIIM_STRING | MIIM_SUBMENU | MIIM_FTYPE | MIIM_STATE,
                dwTypeData: PWSTR(text.as_mut_ptr()),
                cch: text.len() as u32,
                ..Default::default()
            };
            // SAFETY: the text buffer outlives the call and its length is in `cch`.
            unsafe { GetMenuItemInfoW(menu, index, true, &mut info) }.ok()?;
            Some(if info.fType.0 & MFT_SEPARATOR.0 != 0 {
                Entry::Separator
            } else if !info.hSubMenu.is_invalid() {
                Entry::Submenu(info.hSubMenu)
            } else {
                Entry::Command(Choice {
                    id: info.wID.wrapping_sub(FIRST),
                    text: String::from_utf16_lossy(&text[..(info.cch as usize).min(text.len())]),
                    checked: info.fState.0 & MFS_CHECKED.0 != 0,
                })
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "presses Clear cache in Google Drive's storage window when it is open"]
    fn this_pc_s_drive_storage_window() {
        println!("{:?}", clear_cache());
    }
    #[test]
    fn clear_cache_is_found_by_name_or_as_the_second_dialog_button_never_guessed() {
        let dialog = "mdc-button dfs-button dfs-dialog-button";
        let buttons = |names: [&str; 4]| -> Vec<(String, String)> {
            let mut all = vec![("Learn more".to_string(), "link-style".to_string())];
            all.extend(names.map(|name| (name.to_string(), dialog.to_string())));
            all.push(("Offline files 0 bytes".into(), "segmentable-progress-bar__legend-item".into()));
            all
        };
        assert_eq!(clear_cache_button(&buttons(["Clear offline files", "Clear cache", "Go to cache settings", "Done"])), Some(2));
        // Another language: the same four dialog buttons, by position.
        assert_eq!(clear_cache_button(&buttons(["Διαγραφή αρχείων εκτός σύνδεσης", "Εκκαθάριση κρυφής μνήμης", "Ρυθμίσεις", "Τέλος"])), Some(2));
        // A window that changed shape: nothing is pressed.
        let mut three = buttons(["A", "B", "C", "D"]);
        three.remove(4);
        assert_eq!(clear_cache_button(&three), None);
        let mut twice = buttons(["Clear offline files", "Clear cache", "Go to cache settings", "Done"]);
        twice.push(("Clear cache".into(), dialog.into()));
        assert_eq!(clear_cache_button(&twice), None);
    }
    fn choice(id: u32, text: &str) -> Choice {
        Choice { id, text: text.into(), checked: false }
    }
    #[test]
    fn online_only_is_found_by_name_or_by_drive_s_number_and_never_guessed() {
        let english = [vec![choice(7, "Available offline"), choice(8, "Online only")]];
        assert_eq!(online_only_choice(&english).unwrap().id, 8);
        // Another language keeps Drive's numbers.
        let greek = [vec![choice(7, "Διαθέσιμο εκτός σύνδεσης"), choice(8, "Μόνο στο διαδίκτυο")]];
        assert_eq!(online_only_choice(&greek).unwrap().id, 8);
        // A later Drive that renumbered its commands, still in English.
        let renumbered = [vec![choice(11, "Available offline"), choice(12, "Online &only")]];
        assert_eq!(online_only_choice(&renumbered).unwrap().id, 12);
        // Reordered, or a submenu of another size: nothing is picked.
        assert!(online_only_choice(&[vec![choice(7, "Online only"), choice(8, "Available offline")]]).is_none());
        assert!(online_only_choice(&[vec![choice(7, "Available offline"), choice(8, "Online only"), choice(9, "More")]]).is_none());
        assert!(online_only_choice(&[vec![choice(3, "Copy"), choice(4, "Share")]]).is_none());
        // Two submenus that both look right: too unsure to pick either.
        let twice = [english[0].iter().map(|c| choice(c.id, &c.text)).collect(), vec![choice(1, "A"), choice(8, "B")]];
        assert!(online_only_choice(&twice).is_none());
    }
}
