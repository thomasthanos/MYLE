// ! Opens the main window at a size that matches the user's screen: ! 4K -> ~1440p, 2K -> ~1080p, 1080p -> ~720p (physical pixels), centered.
use tauri::{Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};
/// Never cover more than this share of the work area (taskbar excluded).
const MAX_WORK_AREA_PERCENT: u32 = 95;
/// Screens below 1080p get this share of their work area.
const SMALL_SCREEN_PERCENT: u32 = 90;
/// Window size for a monitor, in physical pixels.
///
/// The tier comes from the monitor's short side, so ultrawide and portrait
/// screens are classified by their real pixel density class.
pub fn target_size(monitor: (u32, u32), work_area: (u32, u32)) -> (u32, u32) {
    let short_side = monitor.0.min(monitor.1);
    let (width, height) = match short_side {
        2160.. => (2560, 1440),
        1440.. => (1920, 1080),
        1080.. => (1280, 720),
        _ => (
            percent(work_area.0, SMALL_SCREEN_PERCENT),
            percent(work_area.1, SMALL_SCREEN_PERCENT),
        ),
    };
    (
        width.min(percent(work_area.0, MAX_WORK_AREA_PERCENT)),
        height.min(percent(work_area.1, MAX_WORK_AREA_PERCENT)),
    )
}
fn percent(value: u32, pct: u32) -> u32 {
    value * pct / 100
}
/// Resizes the window for the monitor under the cursor (falling back to the
/// primary one) and centers it in that monitor's work area.
pub fn fit_to_screen<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let Some(monitor) = pick_monitor(window) else {
        return window.center();
    };
    let screen = monitor.size();
    let work = monitor.work_area();
    let (width, height) = target_size(
        (screen.width, screen.height),
        (work.size.width, work.size.height),
    );
    let x = work.position.x + ((work.size.width - width) / 2) as i32;
    let y = work.position.y + ((work.size.height - height) / 2) as i32;
    // Move onto the target monitor first: crossing to a monitor with another DPI makes Windows rescale the window, so the size is set after that.
    window.set_position(PhysicalPosition::new(x, y))?;
    window.set_size(PhysicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(x, y))?;
    #[cfg(debug_assertions)]
    println!(
        "[window] monitor {}x{} (work area {}x{}, scale {}) -> window {width}x{height} at {x},{y}",
        screen.width,
        screen.height,
        work.size.width,
        work.size.height,
        monitor.scale_factor()
    );
    Ok(())
}
fn pick_monitor<R: Runtime>(window: &WebviewWindow<R>) -> Option<Monitor> {
    window
        .cursor_position()
        .ok()
        .and_then(|p| window.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
}
#[cfg(test)]
mod tests {
    use super::target_size;
    // Work areas assume a 48px taskbar at 100% scaling.
    #[test]
    fn uhd_4k_opens_at_1440p() {
        assert_eq!(target_size((3840, 2160), (3840, 2112)), (2560, 1440));
    }
    #[test]
    fn qhd_2k_opens_at_1080p() {
        assert_eq!(target_size((2560, 1440), (2560, 1392)), (1920, 1080));
    }
    #[test]
    fn full_hd_opens_at_720p() {
        assert_eq!(target_size((1920, 1080), (1920, 1032)), (1280, 720));
    }
    #[test]
    fn ultrawide_uses_its_height_class() {
        assert_eq!(target_size((3440, 1440), (3440, 1392)), (1920, 1080));
    }
    #[test]
    fn portrait_screen_is_clamped_to_its_width() {
        assert_eq!(target_size((1440, 2560), (1440, 2512)), (1368, 1080));
    }
    #[test]
    fn small_screens_use_most_of_the_work_area() {
        assert_eq!(target_size((1366, 768), (1366, 720)), (1229, 648));
        assert_eq!(target_size((1024, 600), (1024, 552)), (921, 496));
    }
    #[test]
    fn never_exceeds_the_work_area() {
        // 4K panel with a huge taskbar and 1440p would not fit vertically.
        let (w, h) = target_size((3840, 2160), (3840, 1400));
        assert_eq!((w, h), (2560, 1330));
    }
}
