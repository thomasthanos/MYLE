//! Centers desktop windows in physical pixels on Windows.
use tauri::{AppHandle, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

pub fn place_on_monitor<R: Runtime>(
    window: &WebviewWindow<R>,
    monitor: &Monitor,
    size: PhysicalSize<u32>,
) -> tauri::Result<()> {
    let work = monitor.work_area();
    let position = centered_position(work.position, work.size, size);
    // Move first so Windows applies the target display's DPI before we set the size.
    window.set_position(position)?;
    window.set_size(size)?;
    // Windows includes an invisible frame around a borderless window when its shadow is enabled.
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

pub(crate) fn centered_position(
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
