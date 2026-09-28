use super::*;

/// (Re)builds the Windows SHELL context menu for `targets` (selection, or
/// [current folder] for the dead zone — queried "as an item",
/// the item-context approach) and pushes the entries to the UI. The `IContextMenu`
/// session is kept in the state (the ids are only valid for it). Returns the
/// number of entries (menu height). Disabled / failed / non-Windows → 0.
pub(in crate::bridge) fn refresh_shell_menu(
    window: &MainWindow,
    state: &AppState,
    targets: &[PathBuf],
) -> usize {
    window
        .global::<crate::MenuApi>()
        .set_ctx_shell_sub_open(false); // resets the flyout from a previous opening
    let enabled = state.config.borrow().shell_ctx_menu;
    if !enabled || targets.is_empty() {
        *state.shell_menu.borrow_mut() = None;
        window
            .global::<crate::MenuApi>()
            .set_ctx_shell_entries(ModelRc::new(VecModel::from(Vec::<ShellCtxEntry>::new())));
        return 0;
    }
    let work_dir = state.current_path();
    match crate::shellmenu::build_for_paths(targets, &work_dir) {
        Some((session, entries)) => {
            // Feeds the settings' "detected entries" list.
            {
                let mut known = state.shell_known.borrow_mut();
                let mut grew = false;
                for e in &entries {
                    if !known.iter().any(|k| k == &e.label) {
                        known.push(e.label.clone());
                        grew = true;
                    }
                }
                drop(known);
                if grew {
                    refresh_shell_ext_rows(window, state);
                }
            }
            // Filters out entries HIDDEN by the user (by label).
            let disabled = state.config.borrow().shell_menu_disabled.clone();
            let mut items: Vec<ShellCtxEntry> = Vec::new();
            let mut subs: Vec<Vec<ShellCtxEntry>> = Vec::new();
            for e in &entries {
                if disabled.iter().any(|d| d == &e.label) {
                    continue;
                }
                let (has_sub, sub) = if e.children.is_empty() {
                    (false, -1)
                } else {
                    subs.push(
                        e.children
                            .iter()
                            .map(|c| ShellCtxEntry {
                                id: c.id as i32,
                                label: c.label.clone().into(),
                                icon: shell_icon(&c.icon),
                                monochrome: shell_icon_monochrome(&c.icon),
                                has_sub: false,
                                sub: -1,
                            })
                            .collect(),
                    );
                    (true, subs.len() as i32 - 1)
                };
                items.push(ShellCtxEntry {
                    id: e.id as i32,
                    label: e.label.clone().into(),
                    icon: shell_icon(&e.icon),
                    monochrome: shell_icon_monochrome(&e.icon),
                    has_sub,
                    sub,
                });
            }
            let n = items.len();
            *state.shell_menu.borrow_mut() = Some(session);
            *state.shell_subs.borrow_mut() = subs;
            window
                .global::<crate::MenuApi>()
                .set_ctx_shell_entries(ModelRc::new(VecModel::from(items)));
            n
        }
        None => {
            *state.shell_menu.borrow_mut() = None;
            *state.shell_subs.borrow_mut() = Vec::new();
            window
                .global::<crate::MenuApi>()
                .set_ctx_shell_entries(ModelRc::new(VecModel::from(Vec::<ShellCtxEntry>::new())));
            0
        }
    }
}

/// RGBA bitmap of a shell menu item → `slint::Image` (empty if absent).
pub(in crate::bridge) fn shell_icon(raw: &Option<(Vec<u8>, u32, u32)>) -> Image {
    match raw {
        Some((rgba, w, h)) => Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            rgba, *w, *h,
        )),
        None => Image::default(),
    }
}

/// A shell menu bitmap is "monochrome" when every OPAQUE pixel is effectively
/// grayscale (R≈G≈B). Windows system glyphs (e.g. the Win11 "Share" icon) are a
/// near-black single hue → unreadable on a dark menu; such icons are re-tinted to
/// the theme foreground via Slint `colorize`. COLOUR app icons (7-Zip, Git…) fail
/// the test and are left untouched.
pub(in crate::bridge) fn shell_icon_monochrome(raw: &Option<(Vec<u8>, u32, u32)>) -> bool {
    let Some((rgba, _, _)) = raw else {
        return false;
    };
    let mut seen_opaque = false;
    for px in rgba.chunks_exact(4) {
        if px[3] < 24 {
            continue; // near-transparent → ignored (antialiasing edges)
        }
        seen_opaque = true;
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        if r.max(g).max(b) - r.min(g).min(b) > 24 {
            return false; // a coloured pixel → not a monochrome glyph
        }
    }
    seen_opaque // only if the icon has at least one opaque pixel (not empty)
}

/// First FILE (non-folder) of the active view, from the listing ALREADY in
/// memory — no disk I/O. Serves as a probe target for the "file context".
pub(in crate::bridge) fn first_file_in_active_panel(state: &AppState) -> Option<PathBuf> {
    let active = *state.active_panel.borrow();
    let panels = state.panels.borrow();
    let p = panels.get(active)?;
    (0..p.rows_model.row_count())
        .filter_map(|i| p.rows_model.row_data(i))
        .filter(|r| !r.is_dir)
        .find_map(|r| row_path(&r))
}

/// LAZY probe of the shell menu — called when the "Detected Windows
/// entries" sub-tab opens, never at startup (the in-process COM scan
/// cost a wait cursor for a rarely-consulted list).
///
/// Probes BOTH registry contexts, which do NOT expose the same handlers
/// (`Directory\shell` vs `*\shell`, same on the COM side) — hence "file" entries
/// (Convert to PDF, Select left file…) missing from a "folder" probe:
///   - the current FOLDER;
///   - the first FILE of the active view, taken from the listing ALREADY loaded
///     (no I/O, and above all NO temporary file to create).
///
/// Only once per session; right-clicks keep enriching the list
/// (handlers specific to a file type only appear this way — see the
/// note displayed in the panel).
pub(in crate::bridge) fn scan_shell_ext(window: &MainWindow, state: &AppState) {
    if !cfg!(windows) || state.shell_scanned.get() || !state.config.borrow().shell_ctx_menu {
        return;
    }
    state.shell_scanned.set(true);
    let dir = state.current_path();
    let mut targets: Vec<PathBuf> = vec![dir.clone()];
    if let Some(f) = first_file_in_active_panel(state) {
        targets.push(f);
    }
    let mut grew = false;
    for t in &targets {
        // The session (IContextMenu + HMENU) is discarded immediately: here we only want
        // the LABELS, not something to invoke.
        if let Some((_session, entries)) =
            crate::shellmenu::build_for_paths(std::slice::from_ref(t), &dir)
        {
            let mut known = state.shell_known.borrow_mut();
            for e in entries {
                if !known.iter().any(|k| k == &e.label) {
                    known.push(e.label);
                    grew = true;
                }
            }
        }
    }
    if grew {
        refresh_shell_ext_rows(window, state);
    }
    info!(targets = targets.len(), "shell context menu probed (lazy)");
}

/// Pushes the "detected Windows entries" settings panel (labels seen, sorted,
/// checked unless hidden by the user).
pub(in crate::bridge) fn refresh_shell_ext_rows(window: &MainWindow, state: &AppState) {
    let disabled = state.config.borrow().shell_menu_disabled.clone();
    let mut labels = state.shell_known.borrow().clone();
    labels.sort_by_key(|l| l.to_lowercase());
    let rows: Vec<ShellExtRow> = labels
        .into_iter()
        .map(|l| ShellExtRow {
            enabled: !disabled.iter().any(|d| d == &l),
            label: l.into(),
        })
        .collect();
    window
        .global::<crate::SettingsApi>()
        .set_shell_ext_rows(ModelRc::new(VecModel::from(rows)));
}
