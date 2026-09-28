use super::*;

/// Moves the `from` tab of panel `src` to panel `target` (at
/// position `insert_at`). If `src` becomes empty, it is **closed** (panel removed
/// and its area reclaimed by its sibling). Returns the new panel index to
/// activate, or `None` if the operation is invalid. `src` ≠ `target` required.
/// Removes view `idx` from the layout tree AND from `panels`, re-indexing the
/// active view.
/// Returns what was there, or `None` when there is nothing to take: the last
/// view, an index out of range, or a tree that refuses — and the `Vec` is then
/// left untouched, so the two never fall out of step.
///
/// Shared by closing a view and by tearing one off. What differs between the
/// two is what the caller does with the view it gets back, not how it leaves.
pub(super) fn take_view(state: &AppState, idx: usize) -> Option<Panel> {
    let mut panels = state.panels.borrow_mut();
    if panels.len() <= 1 || idx >= panels.len() {
        return None;
    }
    if !state.layout.borrow_mut().remove_panel(idx) {
        return None;
    }
    let taken = panels.remove(idx);
    let mut active = state.active_panel.borrow_mut();
    if *active >= panels.len() {
        *active = panels.len() - 1;
    } else if idx < *active {
        *active -= 1;
    }
    Some(taken)
}

/// Whether a drop at these WINDOW coordinates landed outside the window.
///
/// The 24px margin keeps a plain overshoot of an edge from counting as a
/// tear-off, and covers most of the title bar so going a little too far up is
/// not one either.
pub(super) fn dropped_outside(window: &MainWindow, x: f32, y: f32) -> bool {
    let size = window.window().size();
    let scale = window.window().scale_factor().max(0.1);
    const MARGIN: f32 = 24.0;
    x < -MARGIN
        || y < -MARGIN
        || x > size.width as f32 / scale + MARGIN
        || y > size.height as f32 / scale + MARGIN
}

/// VIEW tear-off: the whole view leaves for a window of its own, with ALL its
/// tabs.
///
/// The new instance is launched BEFORE anything is removed here, and it carries
/// every tab in a single launch: the operation therefore succeeds whole or
/// fails whole, and no tab is ever left stranded between two windows.
///
/// Works on BOTH systems, like the tab tear-off it is modelled on: starting an
/// instance needs no cross-instance messaging, which is the part still missing
/// outside Windows. Only the placement of the new window degrades where the
/// compositor decides it.
///
/// Refused for the only view of a window, which would empty itself just to
/// reopen identical — the same guard the tab tear-off already carries.
pub(super) fn tear_off_view(state: &AppState, src: usize, at: (i32, i32)) -> bool {
    let (dirs, mode) = {
        let panels = state.panels.borrow();
        if src >= panels.len() || panels.len() <= 1 {
            return false;
        }
        let view = &panels[src];
        let active = view.tabs.active.min(view.tabs.tabs.len().saturating_sub(1));
        // The active tab FIRST: it is the one the new window opens on.
        let mut dirs = vec![view.tabs.tabs[active].current_path.clone()];
        dirs.extend(
            view.tabs
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != active)
                .map(|(_, tab)| tab.current_path.clone()),
        );
        (dirs, view.tab_bar_mode)
    };
    if actions::spawn_detached_view(&dirs, at, mode).is_err() {
        return false;
    }
    // Taken, not "closed": the view did not disappear, it moved. Offering to
    // reopen it here would put a copy of it beside the window it just left.
    take_view(state, src).is_some()
}

/// Tab tear-off (browser-style tear-off): opens a NEW
/// Favnyr instance on the folder of panel `src`'s `from` tab, then
/// removes that tab from here. Refuses if it's the sole tab of the sole panel
/// (otherwise the window would end up empty for a simple duplicate — Firefox
/// behavior). The new instance is launched BEFORE the removal: on failure,
/// the tab isn't lost. If the source panel becomes empty, it is closed (like
/// a cross-view move). Returns `true` if the tab was torn off.
pub(super) fn tear_off_tab(
    state: &AppState,
    src: usize,
    from: usize,
    at: Option<(i32, i32)>,
) -> bool {
    let (path, mode) = {
        let panels = state.panels.borrow();
        if src >= panels.len() || from >= panels[src].tabs.tabs.len() {
            return false;
        }
        // Last tab of the only panel → don't empty the window.
        if panels.len() == 1 && panels[src].tabs.tabs.len() == 1 {
            return false;
        }
        // The new instance inherits the tab bar position of the
        // source view — the chrome follows the torn-off tab.
        (
            panels[src].tabs.tabs[from].current_path.clone(),
            panels[src].tab_bar_mode,
        )
    };
    if actions::spawn_new_instance(&path, at, mode).is_err() {
        return false;
    }
    // The removal and pruning shared with the cross-instance transfer also
    // handle re-indexing the active panel. The guard above forbids an
    // empty instance.
    let _ = remove_tab_pruning(state, src, from);
    state.persist_workspace();
    true
}

// Cross-INSTANCE tab transfer (Windows) ----------

static SUPPRESS_WORKSPACE_PERSIST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// True if the workspace persistence on close must be SKIPPED: instance
/// emptied by transferring ALL its tabs to another instance → otherwise we'd
/// overwrite the shared workspace with an empty state.
pub fn suppress_workspace_persist() -> bool {
    SUPPRESS_WORKSPACE_PERSIST.load(std::sync::atomic::Ordering::SeqCst)
}

/// Serializes a tab for cross-instance transfer: path + view state +
/// drop point (screen), separated by `\t` (invalid in a Windows
/// file name → unambiguous).
pub(super) fn serialize_tab(tab: &Tab, drop: (i32, i32)) -> String {
    format!(
        // The zoom is APPENDED, after the drop point, so the two directions
        // stay compatible between instances of different versions: an older
        // receiver reads the fields it knows and ignores the extra one, a
        // newer receiver finds nothing at that index and falls back.
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        tab.current_path.display(),
        tab.sort.column.code(),
        u8::from(matches!(tab.sort.order, SortOrder::Asc)),
        u8::from(tab.mode.thumbnails()),
        u8::from(tab.show_hidden),
        tab.group_mode.code(),
        drop.0,
        drop.1,
        tab.zoom,
        tab.mode.code(),
        u8::from(tab.subfolders),
    )
}

/// Rebuilds a tab from a serialized payload, with the (physical) SCREEN
/// drop point if present. `None` if malformed.
pub(super) fn deserialize_tab(payload: &str) -> Option<(Tab, Option<(i32, i32)>)> {
    let p: Vec<&str> = payload.split('\t').collect();
    if p.len() < 6 || p[0].is_empty() {
        return None;
    }
    let column = SortColumn::from_code(p[1]).unwrap_or(SortColumn::Name);
    let order = if p[2] == "1" {
        SortOrder::Asc
    } else {
        SortOrder::Desc
    };
    // The display mode is APPENDED after the zoom: a sender that predates the
    // grid (or the mode field) is read through its legacy `preview` flag.
    let mode = p
        .get(9)
        .and_then(|code| ViewMode::from_code(code))
        .unwrap_or(if p[3] == "1" {
            ViewMode::Previews
        } else {
            ViewMode::List
        });
    let tab = Tab::restored(
        PathBuf::from(p[0]),
        SortState { column, order },
        mode,
        p.get(8).and_then(|z| z.parse().ok()),
        p[4] == "1",
        GroupMode::from_code(p[5]).unwrap_or(GroupMode::FoldersFirst),
        p.get(10) == Some(&"1"),
        Vec::new(),
    );
    let drop = match (p.get(6), p.get(7)) {
        (Some(x), Some(y)) => x.parse().ok().zip(y.parse().ok()),
        _ => None,
    };
    Some((tab, drop))
}

/// Removes the `from` tab of panel `src` and prunes the panel if it empties out.
/// Returns `true` — WITHOUT REMOVING ANYTHING — if it was the last tab of the last
/// panel (empty instance): the caller quits. We NEVER leave an empty
/// `TabBook` in the state: the event loop still processes a few events
/// before stopping, and any `with_tabs` access would panic.
pub(super) fn remove_tab_pruning(state: &AppState, src: usize, from: usize) -> bool {
    let mut panels = state.panels.borrow_mut();
    if src >= panels.len() || from >= panels[src].tabs.tabs.len() {
        return false;
    }
    if panels.len() == 1 && panels[src].tabs.tabs.len() == 1 {
        return true; // empty instance → quit (the state stays consistent to the end)
    }
    panels[src].tabs.tabs.remove(from);
    {
        let b = &mut panels[src].tabs;
        if !b.tabs.is_empty() && b.active >= b.tabs.len() {
            b.active = b.tabs.len() - 1;
        }
    }
    if panels[src].tabs.tabs.is_empty() && state.layout.borrow_mut().remove_panel(src) {
        panels.remove(src);
        // Re-indexing the active panel (same rule as panel closing):
        // clamp if the active one pointed past the end, decrement if it was AFTER `src`
        // (the indices shifted by one).
        let mut a = state.active_panel.borrow_mut();
        if *a >= panels.len() {
            *a = panels.len().saturating_sub(1);
        } else if src < *a {
            *a -= 1;
        }
    }
    false
}

/// Outcome of a cross-instance tab transfer attempt.
pub(super) enum Transfer {
    /// Transferred; the instance stays open → the caller refreshes the UI.
    Moved,
    /// Transferred; the instance has EMPTIED and is closing → refresh nothing.
    Emptied,
    /// Failed (invalid target) → the caller falls back to tear-off.
    Failed,
}

/// Transfers the `from` tab of panel `src` to the Favnyr instance `target_hwnd`
/// (window under the cursor), then removes it from here. If the instance empties out,
/// it quits (without overwriting the shared workspace).
pub(super) fn transfer_tab_to(
    state: &AppState,
    src: usize,
    from: usize,
    target_hwnd: isize,
    drop: (i32, i32),
) -> Transfer {
    let payload = {
        let panels = state.panels.borrow();
        if src >= panels.len() || from >= panels[src].tabs.tabs.len() {
            return Transfer::Failed;
        }
        serialize_tab(&panels[src].tabs.tabs[from], drop)
    };
    if !crate::winmsg::send(target_hwnd, &payload) {
        return Transfer::Failed;
    }
    if remove_tab_pruning(state, src, from) {
        SUPPRESS_WORKSPACE_PERSIST.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = slint::quit_event_loop();
        Transfer::Emptied
    } else {
        // The target now owns the tab: persist the source immediately so
        // an unexpected shutdown doesn't restore it on the next launch.
        state.persist_workspace();
        Transfer::Moved
    }
}

/// Slint coordinates (logical client) → physical screen. On Windows we
/// must go through `ClientToScreen`: `Window::position()` describes the outer
/// window, and therefore introduces the border thickness + the title bar.
pub(super) fn window_logical_to_screen(window: &MainWindow, x: f32, y: f32) -> (i32, i32) {
    let scale = window.window().scale_factor().max(0.1);
    let client = ((x * scale).round() as i32, (y * scale).round() as i32);
    #[cfg(windows)]
    if let Some(screen) = crate::winmsg::client_to_screen(client.0, client.1) {
        return screen;
    }
    let outer = window.window().position();
    (outer.x + client.0, outer.y + client.1)
}

/// Physical screen → logical Slint client coordinates. The fallback is only used
/// before HWND initialization or outside Windows.
pub(super) fn screen_to_window_logical(window: &MainWindow, x: i32, y: i32) -> (f32, f32) {
    let scale = window.window().scale_factor().max(0.1);
    #[cfg(windows)]
    if let Some((client_x, client_y)) = crate::winmsg::screen_to_client(x, y) {
        return (client_x as f32 / scale, client_y as f32 / scale);
    }
    let outer = window.window().position();
    ((x - outer.x) as f32 / scale, (y - outer.y) as f32 / scale)
}

/// Insertion point (panel, gap) for a received tab, from the screen
/// drop point `(sx, sy)` in physical pixels. The geometry relies solely on the
/// container exported by Slint, the fractional rectangles (`panel_rects`),
/// the tab widths (`tab_layout`), and the scroll reported by
/// `panel-tabs-scrolled`. `gap = None` denotes an insertion at the end of the bar.
pub(super) fn locate_tab_drop(
    window: &MainWindow,
    state: &AppState,
    sx: i32,
    sy: i32,
) -> (usize, Option<usize>) {
    let active = *state.active_panel.borrow();
    // Physical screen → logical window → panel container frame of reference.
    let (wx, wy) = screen_to_window_logical(window, sx, sy);
    let cx = wx - window.get_panels_cont_x();
    let cy = wy - window.get_panels_cont_y();
    let (cw, ch) = (window.get_panels_cont_w(), window.get_panels_cont_h());
    if cw <= 0.0 || ch <= 0.0 || !(0.0..cw).contains(&cx) || !(0.0..ch).contains(&cy) {
        return (active, None); // outside the panels area → active panel, at the end
    }
    let panels = state.panels.borrow();
    let rects = panel_rects(&current_geom(state), panels.len());
    let Some(p) = rects.iter().position(|r| {
        cx >= r.x * cw && cx < (r.x + r.w) * cw && cy >= r.y * ch && cy < (r.y + r.h) * ch
    }) else {
        return (active, None);
    };
    // Position local to the panel's RECT (the PanelComponent is inset by
    // +panel-gap=4px within its rect; the bands below absorb that).
    let lx = cx - rects[p].x * cw;
    let ly = cy - rects[p].y * ch;
    let mode = panels[p].tab_bar_mode;
    // Does the drop target the panel's TAB BAR (→ precise gap) or its
    // body (→ predictable insertion at the end)? The band depends on the mode.
    let in_bar = match mode {
        // Horizontal: gap 4 + panel padding 7 + toolbar padding 6 + strip 26,
        // + tolerance → 44 px below the TOP of the panel.
        0 => ly <= 44.0,
        // Vertical: bar column — gap 4 + panel padding 7 + clamped
        // width (shared `vbar_width` formula), + 4 px tolerance.
        _ => {
            let rect_w = rects[p].w * cw;
            let bw = vbar_width(
                rect_w - 2.0 * 4.0, /* panel-gap */
                panels[p].vbar_user_w,
            );
            match mode {
                1 => lx <= 4.0 + 7.0 + bw + 4.0,
                _ => lx >= rect_w - 4.0 - 7.0 - bw - 4.0,
            }
        }
    };
    if !in_bar {
        return (p, None);
    }
    // Gap in the bar: coordinate along the MAIN AXIS, local to the
    // start of the tab list (insets below), converted to the Flickable's
    // CONTENT frame of reference (viewport ≤ 0, reported by the view). Same rule as
    // reordering: first half of a tab → before it, second half →
    // after. Insets: horizontal = 13 px (padding 7 + toolbar 6, see
    // `tabs-row-x`); vertical = gap 4 + padding 7 + bar padding 4 + "+"
    // head 24 + spacing 4 + rule 1 + spacing 4 = 48 px (MUST == `vtabs-col-y`
    // on the Slint side, 44 px excluding panel-gap).
    let strip_pos = match mode {
        0 => lx - 13.0 - panels[p].tabs_viewport_x,
        _ => ly - 48.0 - panels[p].tabs_viewport_x,
    };
    let titles: Vec<String> = panels[p]
        .tabs
        .tabs
        .iter()
        .map(|t| tab_title(&t.current_path))
        .collect();
    let geo = tab_layout_for(mode, &titles);
    let gap = geo
        .iter()
        .position(|(w, off)| strip_pos < off + w / 2.0)
        .unwrap_or(geo.len());
    (p, Some(gap))
}

/// Cross-instance hover: simulates a LOCAL tab drag at the SCREEN point
/// `(sx, sy)` received from the other instance → the existing insertion preview
/// machinery (identical to the intra-instance drag) lights up in the targeted panel.
/// `drag-source-panel` stays -1 (EXTERNAL drag, no source tab here).
pub(super) fn set_external_hover(window: &MainWindow, sx: i32, sy: i32) {
    // Physical screen → logical window (`drag-abs-x/y`'s frame of reference, see tab-drag-progress).
    let (wx, wy) = screen_to_window_logical(window, sx, sy);
    window.set_drag_source_panel(-1);
    window.set_drag_abs_x(wx);
    window.set_drag_abs_y(wy);
    window.set_drag_active(true);
}

/// End of cross-instance hover / drop: turns off the simulated drag → clears the
/// insertion preview.
pub(super) fn clear_external_hover(window: &MainWindow) {
    if window.get_drag_active() && window.get_drag_source_panel() < 0 {
        window.set_drag_active(false);
    }
}

/// OLE hover for files coming from another Windows instance/application.
/// OLE coordinates are in physical screen space; we convert them into the same
/// logical frame of reference as the internal Slint drag, so we can reuse without divergence
/// the panel/row hit-test, the hover, and the action ghost.
#[cfg(windows)]
pub(super) fn set_external_file_hover(
    window: &MainWindow,
    screen_x: i32,
    screen_y: i32,
    copy: bool,
) {
    let (wx, wy) = screen_to_window_logical(window, screen_x, screen_y);
    window.set_file_drag_source_panel(-1);
    window.set_file_drag_target_invalid(false);
    window.set_file_drag_copy(copy);
    window.set_file_drag_abs_x(wx);
    window.set_file_drag_abs_y(wy);
    window.set_file_drag_active(true);
}

#[cfg(windows)]
pub(super) fn clear_external_file_hover(window: &MainWindow) {
    if window.get_file_drag_active() && window.get_file_drag_source_panel() < 0 {
        window.set_file_drag_active(false);
        window.set_file_drag_target_panel(-1);
        window.set_file_drag_target_row(-1);
        window.set_file_drag_target_folder(false);
        window.set_file_drag_target_exec(false);
        window.set_file_drag_target_invalid(false);
        window.set_file_drag_copy(false);
    }
}

/// Finishes an OLE drop received by Favnyr. The final target is re-read after
/// replaying the authoritative point: executable → ShellExecute, Ctrl → direct copy,
/// otherwise the same Move/Copy/Link menu as for a drag between views.
#[cfg(windows)]
pub(super) fn on_external_file_drop(
    window: &MainWindow,
    state: &AppState,
    paths: Vec<PathBuf>,
    screen_x: i32,
    screen_y: i32,
    copy: bool,
    staging: Option<IncomingDropStaging>,
) {
    if paths.is_empty() {
        clear_external_file_hover(window);
        return;
    }
    set_external_file_hover(window, screen_x, screen_y, copy);
    let target_panel = window.get_file_drag_target_panel();
    if target_panel < 0 {
        clear_external_file_hover(window);
        return;
    }
    let target_row = window.get_file_drag_target_row();
    let target = target_panel as usize;
    let target_info = if target_row >= 0 {
        panel_path_at_row(state, target, target_row as usize).map(|path| {
            let is_dir = panel_folder_at_row(state, target, target_row as usize).is_some();
            (path, is_dir)
        })
    } else {
        Some((panel_dir(state, target), true))
    };
    if let Some(staging) = staging {
        // Favnyr already owns this staging tree before the OLE call returns.
        // Favnyr-owned data goes directly through the normal paste pipeline,
        // name conflicts included. No Move/Copy/Link menu is shown because
        // virtual attachments and temporary application paths are copy-only.
        let dest = match &target_info {
            Some((path, true)) => path.clone(),
            _ => panel_dir(state, target),
        };
        clear_external_file_hover(window);
        begin_paste_with_cleanup(
            window,
            state,
            staging.op,
            dest,
            paths,
            Some(staging.cleanup),
        );
        return;
    }
    if let Some((target_path, target_is_dir)) = target_info
        && paths_conflict_with_drop_target(&paths, &target_path, target_is_dir)
    {
        clear_external_file_hover(window);
        return;
    }
    *state.external_drop_paths.borrow_mut() = paths;
    window.set_file_drop_src_panel(-1);
    window.set_file_drop_target_panel(target_panel);
    window.set_file_drop_target_row(target_row);

    let (wx, wy) = screen_to_window_logical(window, screen_x, screen_y);
    window.set_file_drop_menu_open(false);
    if window.invoke_file_drop_onto_exec() {
        // The callback consumed `external_drop_paths`.
    } else if copy {
        window.invoke_file_drop_action(1);
    } else {
        window.set_file_drop_menu_x(wx);
        window.set_file_drop_menu_y(wy);
        window.set_file_drop_menu_open(true);
    }
    clear_external_file_hover(window);
}

/// Receives a tab transferred from ANOTHER instance: inserts it at the drop
/// point (panel + gap in the bar, see `locate_tab_drop`) and activates it. The
/// window has already been brought to the foreground by the sender.
pub(super) fn on_tab_received(window: &MainWindow, state: &AppState, payload: &str) {
    let Some((tab, drop)) = deserialize_tab(payload) else {
        clear_external_hover(window);
        return;
    };
    // Case B: drop onto a FAVORITES folder of THIS instance. The async
    // hover may LAG BEHIND the final position → we REPLAY the
    // drop point (authoritative, carried in the payload) via
    // `set_external_hover`, then READ `fav-hover-container` PULL-BASED (the binding
    // is re-evaluated on read → reflects that exact point). Non-empty = we save it
    // as a favorite (the tab was already removed from the source by the transfer: it
    // "merges into" the favorites folder instead of attaching to a panel).
    let fav_container = match drop {
        Some((sx, sy)) => {
            set_external_hover(window, sx, sy);
            window.get_fav_hover_container().to_string()
        }
        None => String::new(),
    };
    clear_external_hover(window); // clears the insertion preview in all cases
    if !fav_container.is_empty() {
        add_paths_to_favorite(
            window,
            state,
            std::slice::from_ref(&tab.current_path),
            &fav_container,
        );
        return;
    }
    let (panel, gap) = match drop {
        Some((sx, sy)) => locate_tab_drop(window, state, sx, sy),
        None => (*state.active_panel.borrow(), None),
    };
    let path = tab.current_path.clone();
    let panel = panel.min(state.panels.borrow().len().saturating_sub(1));
    {
        let mut panels = state.panels.borrow_mut();
        let book = &mut panels[panel].tabs;
        let at = gap.unwrap_or(book.tabs.len());
        book.insert_tab_at(at, tab);
    }
    *state.active_panel.borrow_mut() = panel;
    info!(path = %path.display(), panel, "tab received from another instance");
    load_directory(window, state, &path, false);
    state.persist_workspace();
}

pub(super) fn move_tab_between(
    state: &AppState,
    src: usize,
    from: usize,
    mut target: usize,
    insert_at: usize,
) -> Option<usize> {
    let mut panels = state.panels.borrow_mut();
    let n = panels.len();
    if src >= n || target >= n || src == target {
        return None;
    }
    if from >= panels[src].tabs.tabs.len() {
        return None;
    }

    // Extract the tab from the source.
    let tab = panels[src].tabs.tabs.remove(from);
    {
        let b = &mut panels[src].tabs;
        if !b.tabs.is_empty() && b.active >= b.tabs.len() {
            b.active = b.tabs.len() - 1;
        }
    }
    let src_empty = panels[src].tabs.tabs.is_empty();

    // Insert into the target.
    {
        panels[target].tabs.insert_tab_at(insert_at, tab);
    }

    // Source emptied (it was its last tab) → close the source panel.
    if src_empty && state.layout.borrow_mut().remove_panel(src) {
        panels.remove(src);
        if src < target {
            target -= 1;
        }
    }
    Some(target.min(panels.len().saturating_sub(1)))
}

/// Dropping a tab on the EDGE of a panel: splits
/// `target_panel` according to `dir` and places the `from_tab` tab (moved from
/// `source_panel`) into the newly created panel. `new_first` = the new
/// panel is the first child (west/north edge). If the source panel becomes
/// empty, it is removed (merge). Returns `true` if the operation took place.
pub(super) fn split_with_tab(
    state: &AppState,
    source_panel: usize,
    from_tab: usize,
    target_panel: usize,
    dir: SplitDir,
    new_first: bool,
) -> bool {
    let mut panels = state.panels.borrow_mut();
    let n = panels.len();
    if source_panel >= n || target_panel >= n {
        return false;
    }
    if from_tab >= panels[source_panel].tabs.tabs.len() {
        return false;
    }
    // Splitting your own panel with its only tab doesn't make sense.
    if target_panel == source_panel && panels[source_panel].tabs.tabs.len() <= 1 {
        return false;
    }
    // Cap: a split creates +1 panel. But if the source then empties out
    // (last tab → closes), the net change is zero → allowed even at 16.
    let source_will_close =
        target_panel != source_panel && panels[source_panel].tabs.tabs.len() == 1;
    if n >= MAX_PANELS && !source_will_close {
        return false;
    }

    // 1. Extract the tab from the source panel.
    let tab = panels[source_panel].tabs.tabs.remove(from_tab);
    {
        let b = &mut panels[source_panel].tabs;
        if !b.tabs.is_empty() && b.active >= b.tabs.len() {
            b.active = b.tabs.len() - 1;
        }
    }
    let source_empty = panels[source_panel].tabs.tabs.is_empty();

    // 2. New panel containing the moved tab. It inherits the columns
    //    and the tab bar position of the source view (visual
    // consistency during a cross-view drag).
    let new_idx = panels.len();
    let inherited_cols = panels[source_panel].columns.clone();
    let inherited_mode = panels[source_panel].tab_bar_mode;
    let inherited_vbar = panels[source_panel].vbar_user_w;
    panels.push(Panel::from_tab(
        tab,
        inherited_cols,
        inherited_mode,
        inherited_vbar,
    ));

    // 3. Split the target leaf in the tree.
    let ok = state
        .layout
        .borrow_mut()
        .split_leaf(target_panel, dir, new_idx, 0.5, new_first);
    if !ok {
        // Rollback: remove the created panel, put the tab back into the source.
        if let Some(p) = panels.pop()
            && let Some(t) = p.tabs.tabs.into_iter().next()
        {
            let dst = &mut panels[source_panel].tabs;
            let at = from_tab.min(dst.tabs.len());
            dst.tabs.insert(at, t);
        }
        return false;
    }

    // 4. Source emptied → remove the source panel (merge).
    let mut final_active = new_idx;
    if source_empty && state.layout.borrow_mut().remove_panel(source_panel) {
        panels.remove(source_panel);
        if source_panel < final_active {
            final_active -= 1;
        }
    }

    *state.active_panel.borrow_mut() = final_active.min(panels.len().saturating_sub(1));
    true
}

/// Splits a target view with a newly opened directory from the sidebar. Unlike
/// `split_with_tab`, no source view is consumed: the panel count always grows
/// by one, and the new view inherits the target chrome.
pub(super) fn split_with_path(
    state: &AppState,
    path: PathBuf,
    target_panel: usize,
    dir: SplitDir,
    new_first: bool,
) -> bool {
    let mut panels = state.panels.borrow_mut();
    if target_panel >= panels.len() || panels.len() >= MAX_PANELS {
        return false;
    }

    let new_idx = panels.len();
    let inherited_cols = panels[target_panel].columns.clone();
    let inherited_mode = panels[target_panel].tab_bar_mode;
    let inherited_vbar = panels[target_panel].vbar_user_w;
    let panel = Panel::from_tab(
        Tab::new(path),
        inherited_cols,
        inherited_mode,
        inherited_vbar,
    );
    if !state
        .layout
        .borrow_mut()
        .split_leaf(target_panel, dir, new_idx, 0.5, new_first)
    {
        return false;
    }
    panels.push(panel);
    *state.active_panel.borrow_mut() = new_idx;
    true
}
