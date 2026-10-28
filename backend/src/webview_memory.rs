// ! How much the main window's web view keeps in memory: as much as it needs ! while it is on screen, little (trimmed caches, less GPU memory) while it ! waits in the tray or minimized.
use std::sync::atomic::{AtomicBool, Ordering};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    ICoreWebView2_19,
};
use windows::core::Interface;
/// Whether the web view was last asked to keep little in memory.
static LOW: AtomicBool = AtomicBool::new(false);
/// Only a change is passed on: resizing calls this all the time.
pub(crate) fn set_low(window: &tauri::WebviewWindow, low: bool) {
    if LOW.swap(low, Ordering::SeqCst) == low {
        return;
    }
    let _ = window.with_webview(move |webview| {
        let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        // SAFETY: Tauri runs this callback on the web view's UI thread. An older WebView2 without the setting keeps its normal memory.
        let _ = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|view| view.cast::<ICoreWebView2_19>())
                .and_then(|view| view.SetMemoryUsageTargetLevel(level))
        };
    });
}
