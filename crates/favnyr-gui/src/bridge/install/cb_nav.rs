use super::*;

pub(super) fn install_navigate_to(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_navigate_to(move |path: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        // `~` for the home folder, and the environment variables written
        // the way this platform writes them. The rule lives in the core,
        // where it is testable without a window; an address that expands to
        // nothing reaches the check below unchanged and gets the usual
        // "not listable" answer.
        let p = rfs::expand_typed_path(&path);
        // NEVER pre-test a network path on the UI thread: `is_dir`
        // can block on SMB for several seconds. The listing worker
        // will decide and prompt for credentials if needed.
        let network = rfs::is_unc_path(&p) || favnyr_core::places::is_network_path(&p);
        // `%TEMP%` answers with an 8.3 spelling whenever the account name is
        // long, where the system's own file manager shows the full one.
        // Adopting it keeps ONE spelling per folder, which matters to
        // everything that keys on a path — a colour, a note. Asked of the
        // filesystem, so only when a component actually looks mangled, and
        // never over the network, whose round trip would land on this thread.
        #[cfg(windows)]
        let p = if !network && crate::winutil::has_short_component(&p) {
            crate::winutil::long_path(&p).unwrap_or(p)
        } else {
            p
        };
        if network || rfs::is_listable(&p) {
            load_directory(&w, &st, &p, true);
        } else {
            // An unreachable path used to be dropped in silence, leaving
            // the user staring at an unchanged view: say so with a toast.
            warn!(path = %p.display(), "URL bar: path not listable");
            let lang = st.config.borrow().language;
            notice(
                &w,
                i18n::strings_for(lang).fav_toast_missing.clone(),
                NoticeKind::FavMissing,
            );
        }
    });
}

pub(super) fn install_row_activated(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_row_activated(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let rows = st.active_rows_model();
        let Some(row) = slint::Model::row_data(&rows, idx as usize) else {
            return;
        };
        // `row.path` = parent folder path; we append the name to it.
        let target = PathBuf::from(row.path.to_string()).join(row.name.as_str());
        if row.is_dir {
            load_directory(&w, &st, &target, true);
        } else if !open_shortcut_as_tab(&w, &st, &target) {
            // File: SAME logic as Enter / the "Open" menu entry — Favnyr's
            // per-extension default if set, otherwise the OS default. (Double-click
            // used to ignore the Favnyr default, which made the Settings field
            // look like it did nothing.) A folder .lnk shortcut has already
            // been opened as a tab above.
            open_file_default(&st, &[target]);
        }
    });
}

pub(super) fn install_row_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    // Returns whether the row was ALREADY the whole selection. A click that
    // shrinks a multiple selection down to one entry must not also arm the
    // deferred rename: the user is dropping the other entries, not asking
    // to edit this one's name.
    window.on_row_clicked(move |idx: i32| -> bool {
        let (count, was_alone) = selection_set_only(&st.active_rows_model(), idx);
        st.set_selection_anchor(idx);
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
        was_alone
    });
}

pub(super) fn install_row_ctrl_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_row_ctrl_clicked(move |idx: i32| {
        let count = selection_toggle(&st.active_rows_model(), idx);
        st.set_selection_anchor(idx);
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
    });
}

pub(super) fn install_row_shift_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_row_shift_clicked(move |idx: i32| {
        let anchor = st.selection_anchor();
        let from = if anchor < 0 { idx } else { anchor };
        let count = selection_set_range(&st.active_rows_model(), from, idx);
        if anchor < 0 {
            st.set_selection_anchor(idx);
        }
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
    });
}

pub(super) fn install_select_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_select_all(move || {
        let count = selection_set_all(&st.active_rows_model(), true);
        st.set_selection_anchor(if count > 0 { 0 } else { -1 });
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
    });
}

pub(super) fn install_deselect_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_deselect_all(move || {
        let count = selection_set_all(&st.active_rows_model(), false);
        st.set_selection_anchor(-1);
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
    });
}

pub(super) fn install_rubber_band_update(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_rubber_band_update(move |_x1, y1, _x2, y2| {
        let model = st.active_rows_model();
        let (lo, hi) = row_band_for_content_range(&model, y1, y2);
        // Combine the band with the base captured at drag start, depending on
        // the mode (replace / add Shift / subtract Ctrl). Empty band (hi < lo):
        // lo > hi → no row is "in_band" → base kept (add/sub) or everything
        // deselected (replace).
        let (mode, count) = RB_STATE.with(|s| {
            let s = s.borrow();
            (s.0, selection_apply_band(&model, lo, hi, s.0, &s.1))
        });
        // The anchor follows the top edge of the band (for a future Shift+click) — only
        // in replace mode, where the band defines the whole selection.
        if mode == 0 && hi >= lo {
            st.set_selection_anchor(lo);
        }
        if let Some(w) = weak.upgrade() {
            push_active_footer(&w, &st, count);
        }
    });
}

pub(super) fn install_rubber_band_begin(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_rubber_band_begin(move |mode: i32| {
        let base = snapshot_selection(&st.active_rows_model());
        RB_STATE.with(|s| *s.borrow_mut() = (mode, base));
    });
}

pub(super) fn install_match_action(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_match_action(
        move |keyname: SharedString, ctrl: bool, alt: bool, shift: bool| -> SharedString {
            match Chord::from_event(&keyname, ctrl, alt, shift) {
                Some(chord) => st
                    .keymap
                    .borrow()
                    .action_for(&chord)
                    .map(SharedString::from)
                    .unwrap_or_default(),
                None => SharedString::new(),
            }
        },
    );
}

pub(super) fn install_move_cursor(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_move_cursor(move |kind: i32, extend: bool| {
        let Some(w) = weak.upgrade() else { return };
        let model = st.active_rows_model();
        let n = model.row_count() as i32;
        if n == 0 {
            return;
        }
        let (cur, anchor, grid) = {
            let panels = st.panels.borrow();
            let idx = *st.active_panel.borrow();
            let tab = &panels[idx].tabs.tabs[panels[idx].tabs.active];
            (tab.cursor, tab.selection_anchor, tab.mode.is_grid())
        };
        let start = if cur < 0 { 0 } else { cur.min(n - 1) };
        // The cursor never rests on a section header.
        let Some(base) = walk_entries(&model, start, 1).or_else(|| walk_entries(&model, start, -1))
        else {
            return;
        };
        let target = match kind {
            0 if grid => grid_neighbour(&model, base, 0, -1),
            1 if grid => grid_neighbour(&model, base, 0, 1),
            0 => walk_entries(&model, base - 1, -1), // up
            1 => walk_entries(&model, base + 1, 1),  // down
            2 => walk_entries(&model, 0, 1),         // first
            3 => walk_entries(&model, n - 1, -1),    // last
            4 if grid => grid_neighbour(&model, base, -1, 0), // left
            5 if grid => grid_neighbour(&model, base, 1, 0), // right
            // The single-column list has no tile beside: left/right are inert.
            _ => None,
        };
        let Some(new_cursor) = target else {
            return; // already at the edge of the listing
        };
        if extend {
            // Extend from the anchor (set if absent) to the new cursor.
            let anc = if anchor < 0 { base } else { anchor };
            let _ = selection_set_range(&model, anc, new_cursor);
            st.with_tabs_mut(|b| {
                let a = b.active;
                b.tabs[a].selection_anchor = anc;
            });
        } else {
            let _ = selection_set_only(&model, new_cursor);
            st.with_tabs_mut(|b| {
                let a = b.active;
                b.tabs[a].selection_anchor = new_cursor;
            });
        }
        st.with_tabs_mut(|b| {
            let a = b.active;
            b.tabs[a].cursor = new_cursor;
            b.tabs[a].scroll_gen += 1; // triggers the scroll-into-view on the Slint side
        });
        update_panels_ui(&w, &st);
    });
}

pub(super) fn install_keyboard_context_row(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_keyboard_context_row(move || -> i32 {
        let rows = st.active_rows_model();
        let anchor = st.selection_anchor();
        if anchor >= 0
            && rows
                .row_data(anchor as usize)
                .map(|r| r.selected)
                .unwrap_or(false)
        {
            return anchor;
        }
        (0..rows.row_count())
            .find(|i| rows.row_data(*i).map(|r| r.selected).unwrap_or(false))
            .map(|i| i as i32)
            .unwrap_or(-1)
    });
}

pub(super) fn install_row_right_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_row_right_clicked(move |idx: i32, x: f32, y: f32| {
        let Some(w) = weak.upgrade() else { return };
        let rows = st.active_rows_model();
        let already_selected = rows
            .row_data(idx as usize)
            .map(|r| r.selected)
            .unwrap_or(false);
        if !already_selected {
            let (count, _) = selection_set_only(&rows, idx);
            st.set_selection_anchor(idx);
            push_active_footer(&w, &st, count);
        }
        // "Use as application": visible if the selection = 1 executable.
        let sel = selected_paths(&st);
        // The PRIMARY item treated as a folder: a real directory, a folder
        // symlink/junction (already reflected by `is_dir`), or a Windows
        // `.lnk` pointing at a folder (resolved on demand — one COM call,
        // and only for an actual `.lnk`). Shared by every file/folder choice
        // below, so a folder shortcut behaves like the folder it targets:
        // no "Open with", "new tab" instead of "open as admin", and
        // folder-context pinned commands.
        let primary_dir = sel.first().map(|p| acts_as_dir(p)).unwrap_or(false);
        let is_file = sel.len() == 1 && !primary_dir;
        w.set_selection_is_executable(sel.len() == 1 && is_executable_path(&sel[0]));
        // "Open as administrator" (Windows): target = a SINGLE real file.
        w.set_ctx_selection_is_file(is_file);
        // A single item (file OR folder) → "Create a shortcut" is offered.
        w.set_ctx_selection_single(sel.len() == 1);
        // A folder (or folder shortcut) as the primary item → "Open with" is
        // hidden (it hands a file to an application; a folder is opened by
        // navigating into it).
        w.set_ctx_selection_is_dir(primary_dir);
        // Distinct from the line above, which describes only the PRIMARY
        // item: a selection whose first entry happens to be a file may
        // still hold folders worth colouring.
        let has_dir = sel.iter().any(|p| acts_as_dir(p));
        w.set_ctx_selection_has_dir(has_dir);
        // Slot already applied, so the strip can point at it. Taken from
        // the primary item: with a mixed selection the strip shows what the
        // first folder carries and assigns to all of them.
        w.set_mark_current_color(i32::from(
            sel.iter()
                .find(|p| acts_as_dir(p))
                .map_or(0, |p| annotations_now(&st).color_of(p)),
        ));
        // Pinned user commands, filtered by target: file (bit 1) or folder
        // (bit 2) — same folder rule as the built-in entries above.
        let bit = if primary_dir {
            openers::CTX_DIR
        } else {
            openers::CTX_FILE
        };
        let n_custom = push_ctx_custom_entries(&w, &st, bit, &sel);
        w.set_ctx_custom_bg(false);
        // Windows SHELL context menu for the selection.
        let n_shell = refresh_shell_menu(&w, &st, &sel);
        // Full menu height, needed to know whether the menu still fits
        // below the pointer. It repeats the Slint binding term for term:
        // a base holding every row that is always there, then one term per
        // row that can be hidden, on the very condition that renders it.
        // The two sides have to be changed together.
        let h = 376.0
            + if primary_dir { 0.0 } else { 26.0 }
            + if sel.len() == 1 { 26.0 } else { 0.0 }
            + if has_dir || sel.len() == 1 { 26.0 } else { 0.0 }
            + if n_custom > 0 {
                n_custom as f32 * 26.0 + 1.0
            } else {
                0.0
            }
            + if n_shell > 0 {
                n_shell as f32 * 26.0 + 1.0
            } else {
                0.0
            };
        let (x, y) = clamp_ctx_menu_pos(&w, x, y, h);
        // Refreshes the "Open with" flyout filtered by the TARGETED file's
        // extension → programs suited to this specific file.
        push_openers_ui(&w, &st);
        w.set_ctx_on_empty(false);
        w.set_ctx_menu_x(x);
        w.set_ctx_menu_y(y);
        arm_context_menu_navigation(&w);
        w.set_ctx_menu_open(true);
    });
}

pub(super) fn install_async_listing_drain(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_async_listing_drain(move || {
        let Some(w) = weak.upgrade() else { return };
        loop {
            let next = st
                .async_listings
                .lock()
                .ok()
                .and_then(|mut queue| queue.pop_front());
            let Some(delivery) = next else { break };
            apply_async_listing(&w, &st, delivery);
        }
    });
}

pub(super) fn install_subfolders_drain(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_subfolders_drain(move || {
        let Some(w) = weak.upgrade() else { return };
        loop {
            let next = st
                .subscans
                .lock()
                .ok()
                .and_then(|mut queue| queue.pop_front());
            let Some(delivery) = next else { break };
            apply_subfolder_scan(&w, &st, delivery);
        }
    });
}
