// ! Opens the main window at a size that matches the user's screen: ! 4K -> ~1440p, 2K -> ~1080p, 1080p -> ~720p (physical pixels), centered.
use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow,
};

pub const SPLASH_SIZE: LogicalSize<f64> = LogicalSize::new(300.0, 380.0);
/// Never cover more than this share of the work area (taskbar excluded).
const MAX_WORK_AREA_PERCENT: u32 = 95;
/// Screens below 1080p get this share of their work area.
const SMALL_SCREEN_PERCENT: u32 = 90;
/// The smallest the main window may be, in logical pixels (`min_inner_size`).
pub const MIN_WIDTH: f64 = 800.0;
pub const MIN_HEIGHT: f64 = 500.0;
/// The size the window really takes: `target` (already within the work
/// area), but never below its minimum in physical pixels, which Windows
/// would enlarge it to anyway, even past the work area.
pub fn fitted_size(target: (u32, u32), minimum: (u32, u32)) -> (u32, u32) {
    (target.0.max(minimum.0), target.1.max(minimum.1))
}
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
/// Places the hidden splash on the display under the pointer before it opens.
pub fn center_splash<R: Runtime>(
    window: &WebviewWindow<R>,
    monitor: Option<Monitor>,
) -> tauri::Result<()> {
    let Some(monitor) = monitor else {
        return window.center();
    };
    place_on_monitor(window, &monitor, SPLASH_SIZE.to_physical(monitor.scale_factor()))
}

/// Fits the main window to the splash's display, or to the cursor's display
/// when the splash is unavailable.
pub fn fit_to_screen<R: Runtime>(
    window: &WebviewWindow<R>,
    splash_monitor: Option<Monitor>,
) -> tauri::Result<()> {
    let Some(monitor) = splash_monitor.or_else(|| cursor_monitor(window.app_handle())) else {
        return window.center();
    };
    let screen = monitor.size();
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let minimum = ((MIN_WIDTH * scale).round() as u32, (MIN_HEIGHT * scale).round() as u32);
    let (width, height) = fitted_size(
        target_size((screen.width, screen.height), (work.size.width, work.size.height)),
        minimum,
    );
    place_on_monitor(window, &monitor, PhysicalSize::new(width, height))
}

fn place_on_monitor<R: Runtime>(
    window: &WebviewWindow<R>,
    monitor: &Monitor,
    size: PhysicalSize<u32>,
) -> tauri::Result<()> {
    let work = monitor.work_area();
    let position = centered_position(work.position, work.size, size);
    // Move first so Windows applies the target display's DPI before we set the size.
    window.set_position(position)?;
    window.set_size(size)?;
    // Windows includes an invisible frame around the splash when its shadow is enabled.
    // Center the actual content using the frame inset at the target DPI.
    let size = window.inner_size()?;
    let outer = window.outer_position()?;
    let inner = window.inner_position()?;
    let position = centered_position(work.position, work.size, size);
    window.set_position(PhysicalPosition::new(
        position.x + (outer.x - inner.x),
        position.y + (outer.y - inner.y),
    ))?;
    #[cfg(debug_assertions)]
    println!(
        "[window] {} on {}x{} (work area {}x{}, scale {}) -> {}x{} at {},{}",
        window.label(),
        monitor.size().width,
        monitor.size().height,
        work.size.width,
        work.size.height,
        monitor.scale_factor(),
        size.width,
        size.height,
        position.x,
        position.y,
    );
    Ok(())
}

fn centered_position(
    origin: PhysicalPosition<i32>,
    work_area: PhysicalSize<u32>,
    window: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    // For a window bigger than the work area, keep its top or left edge reachable.
    PhysicalPosition::new(
        origin.x + (work_area.width.saturating_sub(window.width) / 2) as i32,
        origin.y + (work_area.height.saturating_sub(window.height) / 2) as i32,
    )
}
pub fn cursor_monitor<R: Runtime>(app: &AppHandle<R>) -> Option<Monitor> {
    app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}
#[cfg(test)]
mod tests {
    use super::{SPLASH_SIZE, centered_position, fitted_size, target_size};
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn splash_centers_on_a_secondary_display_at_its_dpi() {
        let splash = SPLASH_SIZE.to_physical(1.5);
        assert_eq!(splash, PhysicalSize::new(450, 570));
        let position = centered_position(
            PhysicalPosition::new(-1920, -300),
            PhysicalSize::new(1920, 1032),
            splash,
        );
        assert_eq!(position, PhysicalPosition::new(-1185, -69));
    }

    #[test]
    fn an_oversized_window_keeps_its_edges_on_the_selected_display() {
        let origin = PhysicalPosition::new(3840, 260);
        let position = centered_position(
            origin,
            PhysicalSize::new(1024, 552),
            PhysicalSize::new(1200, 750),
        );
        assert_eq!(position, origin);
    }

    #[test]
    fn the_minimum_size_counts_at_high_scaling() {
        // 1080p at 200%: the 800x500 minimum is 1600x1000 physical pixels.
        let target = target_size((1920, 1080), (1920, 1032));
        assert_eq!(fitted_size(target, (1600, 1000)), (1600, 1000));
        // 720p at 150%: the minimum (1200x750) is larger than the screen; the window takes it all the same.
        assert_eq!(fitted_size((1094, 619), (1200, 750)), (1200, 750));
        // At 100% nothing changes.
        assert_eq!(fitted_size(target, (800, 500)), target);
    }
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
