use super::*;

// ---------- Window size persistence ----------

pub(super) fn should_persist_window_dimensions(
    maximized: bool,
    fullscreen: bool,
    minimized: bool,
) -> bool {
    !maximized && !fullscreen && !minimized
}

/// Converts the physical client size back to OS-logical units for persistence.
/// Favnyr's UI zoom changes Slint's effective scale factor, but it must not
/// change the window size saved in the global configuration.
pub(super) fn persisted_logical_window_size(
    physical: (u32, u32),
    captured_os_scale: f32,
    effective_scale: f32,
) -> (u32, u32) {
    let scale = if captured_os_scale > 0.0 {
        captured_os_scale
    } else {
        // The fallback only applies if the window closes before UI zoom has
        // captured the native DPI scale during startup.
        effective_scale.max(0.01)
    };
    (
        (physical.0 as f32 / scale).round() as u32,
        (physical.1 as f32 / scale).round() as u32,
    )
}

pub fn persist_window_size(window: &MainWindow, state: &AppState) {
    let native_window = window.window();
    let persist_dimensions = should_persist_window_dimensions(
        native_window.is_maximized(),
        native_window.is_fullscreen(),
        native_window.is_minimized(),
    );
    let logical_size = persist_dimensions.then(|| {
        let size = native_window.size();
        persisted_logical_window_size(
            (size.width, size.height),
            state.ui_base_scale.get(),
            native_window.scale_factor(),
        )
    });

    if !persist_dimensions {
        debug!(
            maximized = native_window.is_maximized(),
            fullscreen = native_window.is_fullscreen(),
            minimized = native_window.is_minimized(),
            "preserving last normal window size"
        );
    }

    // Left panel: open state + width.
    let left_panel = window.global::<crate::SidebarApi>().get_left_panel();
    let sidebar_width = window
        .global::<crate::SidebarApi>()
        .get_sidebar_width()
        .round()
        .max(0.0) as u32;

    let cfg = state.snapshot_config();
    let dimensions_changed = logical_size
        .map(|(w, h)| cfg.window_width != w || cfg.window_height != h)
        .unwrap_or(false);
    if dimensions_changed
        || cfg.left_panel != left_panel
        || (sidebar_width > 0 && cfg.sidebar_width != sidebar_width)
    {
        state.persist_config(|c| {
            if let Some((w, h)) = logical_size {
                c.window_width = w;
                c.window_height = h;
            }
            c.left_panel = left_panel;
            if sidebar_width > 0 {
                c.sidebar_width = sidebar_width;
            }
        });
    }
}
