use super::*;

// ---------- UI zoom (global setting) ----------

/// Proposed UI zoom factors (× the screen scale). `1.0` = the OS's
/// native scale. Startup default = the index of `1.0`.
pub(super) const UI_SCALE_PRESETS: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

/// Picker labels ("80%", "100%", …) — language-independent.
pub(super) fn ui_scale_labels() -> Vec<SharedString> {
    UI_SCALE_PRESETS
        .iter()
        .map(|f| format!("{}%", (f * 100.0).round() as i32).into())
        .collect()
}

/// Index of the preset closest to a stored factor (robust to a config
/// value outside the list).
pub(super) fn ui_scale_nearest_index(factor: f32) -> i32 {
    UI_SCALE_PRESETS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - factor).abs().total_cmp(&(**b - factor).abs()))
        .map(|(i, _)| i as i32)
        .unwrap_or(2)
}

/// Applies the UI zoom: (SCREEN scale captured once) × `factor`, clamped, via
/// the public `dispatch_event(ScaleFactorChanged)` API → Slint re-scales and
/// re-lays-out the whole UI (fonts, margins, icons). Doesn't redispatch if
/// the scale is already correct (avoids an unnecessary relayout). The raw OS scale is
/// remembered on the 1st call so it stays the reference even after our own zooms.
pub(super) fn apply_ui_scale(window: &MainWindow, state: &AppState, factor: f32) {
    let base = state.ui_base_scale.get();
    let base = if base > 0.0 {
        base
    } else {
        let os = window.window().scale_factor().max(0.1);
        state.ui_base_scale.set(os);
        os
    };
    let target = (base * factor).clamp(0.5, 4.0);
    let win = window.window();
    if (win.scale_factor() - target).abs() <= 0.001 {
        return;
    }
    win.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
        scale_factor: target,
    });
    // `ScaleFactorChanged` only SETS the factor: it realigns neither the
    // geometry nor the rendering. Outside fullscreen, a spontaneous `Resized` follows and
    // everything realigns; MAXIMIZED/fullscreen, the physical size is locked by
    // the OS → no `Resized` → the zoom used to only apply after un-maximizing.
    // So we force a `Resized` at the SAME physical size (logical = physical /
    // scale): Slint recomputes the layout and redraws at the new scale WITHOUT
    // resizing the OS window (the event only affects Slint's internal state).
    let phys = win.size();
    if phys.width > 0 && phys.height > 0 {
        win.dispatch_event(slint::platform::WindowEvent::Resized {
            size: slint::LogicalSize::new(
                (phys.width as f32 / target).max(1.0),
                (phys.height as f32 / target).max(1.0),
            ),
        });
    }
}

/// Detects ffmpeg (Linux) and pushes the state to the "Video thumbnails"
/// settings section. Called at startup and on every (re)opening of settings / click on
/// "Recheck". Effectively a no-op on Windows (section hidden, `ffmpeg` unused).
pub(super) fn apply_ffmpeg_info(window: &MainWindow) {
    let info = actions::ffmpeg_info();
    let api = window.global::<crate::ApplicationApi>();
    api.set_ffmpeg_found(info.available);
    api.set_ffmpeg_version(info.version.into());
    api.set_ffmpeg_flatpak(info.flatpak);
    api.set_ffmpeg_detected_index(info.detected_distro);
}
