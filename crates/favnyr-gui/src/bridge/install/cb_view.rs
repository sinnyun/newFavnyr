use super::*;

pub(super) fn install_sort_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_sort_clicked(move |col_id: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let Some(new_col) = SortColumn::from_code(&col_id) else {
            warn!(col = %col_id, "unknown sort column id");
            return;
        };
        st.with_tabs_mut(|book| {
            let a = book.active;
            let s = &mut book.tabs[a].sort;
            if s.column == new_col {
                s.order = s.order.flip();
            } else {
                s.column = new_col;
                s.order = SortOrder::Asc;
            }
            // sort-column / sort-asc will be pushed via update_panels_ui
            // in the refresh_listing that follows (see below).
            let _ = s;
        });
        let cur = st.current_path();
        if !cur.as_os_str().is_empty() {
            refresh_listing(&w, &st, &cur);
        }
    });
}

pub(super) fn install_set_group_mode(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_set_group_mode(move |idx: i32, mode: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let Some(gm) = GroupMode::from_code(&mode) else {
            warn!(mode = %mode, "unknown group mode");
            return;
        };
        {
            let mut panels = st.panels.borrow_mut();
            let Some(p) = panels.get_mut(idx as usize) else {
                return;
            };
            let a = p.tabs.active;
            p.tabs.tabs[a].group_mode = gm;
        }
        let cur = st.current_path();
        if !cur.as_os_str().is_empty() {
            refresh_listing(&w, &st, &cur);
        }
        st.persist_workspace();
    });
}

pub(super) fn install_ext_filter_toggle(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ext_filter_toggle(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let p = idx.max(0) as usize;
        let turned_on = {
            let mut panels = st.panels.borrow_mut();
            let Some(pn) = panels.get_mut(p) else { return };
            let a = pn.tabs.active;
            let t = &mut pn.tabs.tabs[a];
            t.ext_filter_on = !t.ext_filter_on;
            if !t.ext_filter_on {
                t.ext_filter.clear();
            }
            t.ext_filter_on
        };
        // The two filters (name / extension) are MUTUALLY EXCLUSIVE: turning one on clears
        // the other → a single bar, a single keyboard stream.
        if turned_on {
            st.filter.borrow_mut().clear();
        }
        *st.active_panel.borrow_mut() = p;
        switch_active_panel(&w, &st);
        w.set_active_filter(st.filter.borrow().clone().into());
    });
}

pub(super) fn install_tab_new(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_tab_new(move || {
        let Some(w) = weak.upgrade() else { return };
        let target = st.with_tabs_mut(|book| {
            // New UX: the new tab inherits the path of the
            // previously active tab (instead of always $HOME).
            let inherited = book.tabs[book.active].current_path.clone();
            book.open(inherited);
            let a = book.active;
            book.tabs[a].current_path.clone()
        });
        load_directory(&w, &st, &target, false);
    });
}

pub(super) fn install_duplicate_tab(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_duplicate_tab(move |panel: i32, tab: i32| {
        let Some(w) = weak.upgrade() else { return };
        let p = (panel.max(0) as usize).min(st.panels.borrow().len().saturating_sub(1));
        let done = {
            let mut panels = st.panels.borrow_mut();
            panels[p].tabs.duplicate(tab.max(0) as usize)
        };
        if done {
            *st.active_panel.borrow_mut() = p;
            switch_active_panel(&w, &st);
        }
    });
}

pub(super) fn install_panel_scroll_target(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_panel_scroll_target(
        move |panel: i32, idx: i32, forward: bool, viewport_x: f32, view_w: f32| {
            tab_scroll_target(&st, panel.max(0) as usize, idx, forward, viewport_x, view_w)
        },
    );
}

pub(super) fn install_tab_closed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_tab_closed(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        // Special case: if this is the active panel's last tab AND
        // there's > 1 panel, close the panel instead of refusing.
        let only_tab_left = st.with_tabs(|book| book.tabs.len() == 1);
        let multi_panel = st.panels.borrow().len() > 1;
        if only_tab_left && multi_panel {
            let active_panel = *st.active_panel.borrow();
            let closed_panel = {
                let mut panels = st.panels.borrow_mut();
                if active_panel >= panels.len()
                    || !st.layout.borrow_mut().remove_panel(active_panel)
                {
                    None
                } else {
                    let closed = panels.remove(active_panel);
                    let mut a = st.active_panel.borrow_mut();
                    if *a >= panels.len() {
                        *a = panels.len() - 1;
                    }
                    Some(closed)
                }
            };
            if let Some(panel) = closed_panel {
                remember_closed_panel(&st, panel);
                w.set_closed_tabs_available(true);
                switch_active_panel(&w, &st);
                st.persist_workspace();
            }
            return;
        }
        // Standard case: close a tab among several.
        let closed_and_target: Option<(Tab, PathBuf)> = st.with_tabs_mut(|book| {
            if let Some(closed) = book.close(idx as usize) {
                let a = book.active;
                Some((closed, book.tabs[a].current_path.clone()))
            } else {
                None
            }
        });
        if let Some((closed, p)) = closed_and_target {
            st.remember_closed_tab(closed);
            w.set_closed_tabs_available(true);
            load_directory(&w, &st, &p, false);
            st.persist_workspace();
        }
    });
}

pub(super) fn install_tab_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_tab_clicked(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let target: Option<PathBuf> = st.with_tabs_mut(|book| {
            if book.select(idx as usize) {
                let a = book.active;
                Some(book.tabs[a].current_path.clone())
            } else {
                None
            }
        });
        if let Some(p) = target {
            load_directory(&w, &st, &p, false);
        }
    });
}

pub(super) fn install_panel_split(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_panel_split(move |dir: i32| {
        let Some(w) = weak.upgrade() else { return };
        let inherited = st.current_path();
        let new_active = {
            let mut panels = st.panels.borrow_mut();
            if panels.len() >= MAX_PANELS {
                None
            } else {
                let active = *st.active_panel.borrow();
                let new_idx = panels.len();
                let dir = if dir == 1 {
                    SplitDir::Column
                } else {
                    SplitDir::Row
                };
                let split_ok = st
                    .layout
                    .borrow_mut()
                    .split_leaf(active, dir, new_idx, 0.5, false);
                if split_ok {
                    // The new view inherits the columns AND the tab bar
                    // position of the source view (the chrome
                    // is duplicated, as the user expects on split).
                    let cols = panels[active].columns.clone();
                    let mode = panels[active].tab_bar_mode;
                    panels.push(Panel::with_mode(inherited, cols, mode));
                    Some(new_idx)
                } else {
                    None
                }
            }
        };
        if let Some(idx) = new_active {
            *st.active_panel.borrow_mut() = idx;
            switch_active_panel(&w, &st);
        }
    });
}

pub(super) fn install_set_tab_bar_mode(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_set_tab_bar_mode(move |panel: i32, mode: i32| {
        let Some(w) = weak.upgrade() else { return };
        {
            let mut panels = st.panels.borrow_mut();
            let Some(p) = panels.get_mut(panel.max(0) as usize) else {
                return;
            };
            let m = mode.clamp(0, 2) as u8;
            if p.tab_bar_mode == m {
                return;
            }
            p.tab_bar_mode = m;
            // The scroll axis changes → the reported offset is stale (the
            // view resets to 0; auto-reveal re-centers the active tab).
            p.tabs_viewport_x = 0.0;
        }
        st.persist_workspace();
        update_panels_ui(&w, &st);
    });
}

pub(super) fn install_panel_vbar_resized(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_panel_vbar_resized(move |panel: i32, w: f32| {
        {
            let mut panels = st.panels.borrow_mut();
            let Some(p) = panels.get_mut(panel.max(0) as usize) else {
                return;
            };
            p.vbar_user_w = w.clamp(110.0, 800.0);
        }
        st.persist_workspace();
    });
}

pub(super) fn install_panel_closed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_panel_closed(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        if let Some(panel) = take_view(&st, idx.max(0) as usize) {
            remember_closed_panel(&st, panel);
            w.set_closed_tabs_available(true);
            switch_active_panel(&w, &st);
            st.persist_workspace();
        }
    });
}

pub(super) fn install_panel_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_panel_clicked(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let idx = idx as usize;
        let changed = {
            let panels = st.panels.borrow();
            let mut active = st.active_panel.borrow_mut();
            if idx < panels.len() && idx != *active {
                *active = idx;
                true
            } else {
                false
            }
        };
        if changed {
            // The type-ahead filter is specific to the active view → reset.
            st.filter.borrow_mut().clear();
            w.set_active_filter(SharedString::new());
            switch_active_panel(&w, &st);
        }
    });
}

pub(super) fn install_panel_viewport_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let scheduler = state.thumb_scheduler.clone();
    window.on_panel_viewport_changed(move |panel: i32, top: f32, height: f32| {
        if panel < 0 {
            return;
        }
        let (first, last) = update_panel_render_window(&st, panel as usize, top, height);
        scheduler.update_viewport(panel as usize, first, last);
    });
}

pub(super) fn install_panel_row_at_content_y(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_panel_row_at_content_y(move |panel: i32, y: f32| -> i32 {
        if panel < 0 {
            return -1;
        }
        // Reading the model already exposed to Slint avoids borrowing `AppState`
        // during a synchronous notification from `VecModel::set_vec` (listings
        // apply their rows under a borrow_mut of the panels).
        let Some(window) = weak.upgrade() else {
            return -1;
        };
        window
            .get_panels()
            .row_data(panel as usize)
            .map(|view| row_index_at_content_y(&view.rows, y))
            .unwrap_or(-1)
    });
}

pub(super) fn install_set_view_mode(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_set_view_mode(move |idx: i32, code: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let Some(mode) = ViewMode::from_code(code.as_str()) else {
            return; // unknown code: keep the current mode
        };
        let idx = idx.max(0) as usize;
        if idx >= st.panels.borrow().len() {
            return;
        }
        apply_view_mode(&w, &st, idx, mode);
    });
}

pub(super) fn install_cycle_view_mode(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_cycle_view_mode(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let idx = idx.max(0) as usize;
        // Read the current mode under the same borrow that checks the panel
        // exists: the cycle is derived from it, never from a stale copy.
        let next = {
            let panels = st.panels.borrow();
            let Some(panel) = panels.get(idx) else {
                return;
            };
            match panel.tabs.tabs[panel.tabs.active].mode {
                ViewMode::List => ViewMode::Previews,
                ViewMode::Previews => ViewMode::Grid,
                ViewMode::Grid => ViewMode::List,
            }
        };
        apply_view_mode(&w, &st, idx, next);
    });
}

pub(super) fn install_toggle_section(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_toggle_section(move |idx: i32, key: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        if key.is_empty() {
            return; // the unlabelled section has nothing to fold
        }
        let idx = idx.max(0) as usize;
        let compact = st.config.borrow().compact_icon_rows_in_preview;
        {
            let mut panels = st.panels.borrow_mut();
            let Some(panel) = panels.get_mut(idx) else {
                return;
            };
            let active = panel.tabs.active;
            let t = &mut panel.tabs.tabs[active];
            match t.collapsed.iter().position(|k| k == key.as_str()) {
                Some(pos) => {
                    t.collapsed.remove(pos);
                }
                None => t.collapsed.push(key.to_string()),
            }
            rebuild_panel_rows(
                panel,
                st.config.borrow().language,
                compact,
                &annotations_now(&st),
                &st.clipboard.borrow(),
            );
        }
        update_panels_ui(&w, &st);
        request_thumbnails(&st);
    });
}

pub(super) fn install_toggle_subfolder_contents(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_toggle_subfolder_contents(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let idx = idx.max(0) as usize;
        let compact = st.config.borrow().compact_icon_rows_in_preview;
        let on = {
            let mut panels = st.panels.borrow_mut();
            let Some(panel) = panels.get_mut(idx) else {
                return;
            };
            let active = panel.tabs.active;
            let t = &mut panel.tabs.tabs[active];
            t.subfolders = !t.subfolders;
            let on = t.subfolders;
            // Turning the feature on primes one pending section per direct
            // subfolder, so the view shows them at once and the scan only
            // has to fill them in. Turning it off just hides them: the
            // cached source keeps them, so flipping back is instant.
            if on {
                let mut source = panel.source.borrow_mut();
                if let Some(source) = source.as_mut() {
                    source.dirs = pending_subfolders(&source.root, &source.own);
                }
            }
            // Whatever is in flight describes the state we just left.
            panel.sub_gen.set(panel.sub_gen.get() + 1);
            rebuild_panel_rows(
                panel,
                st.config.borrow().language,
                compact,
                &annotations_now(&st),
                &st.clipboard.borrow(),
            );
            on
        };
        update_panels_ui(&w, &st);
        request_thumbnails(&st);
        if on {
            request_subfolder_scan(&st, idx);
        }
    });
}

pub(super) fn install_splitter_resized(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_splitter_resized(move |idx: i32, frac: f32| {
        let Some(w) = weak.upgrade() else { return };
        let geom = current_geom(&st);
        let Some(sp) = geom.splitters.get(idx as usize) else {
            return;
        };
        let (area_start, area_len) = match sp.dir {
            SplitDir::Row => (sp.area.x, sp.area.w),
            SplitDir::Column => (sp.area.y, sp.area.h),
        };
        if area_len <= f32::EPSILON {
            return;
        }
        let ratio = ((frac - area_start) / area_len).clamp(MIN_SPLIT_RATIO, 1.0 - MIN_SPLIT_RATIO);
        let path = sp.path.clone();
        let changed = st
            .layout
            .borrow_mut()
            .set_ratio(&path, ratio, MIN_SPLIT_RATIO);
        if changed {
            // Resizing by hand writes over the very sizes the way back
            // would restore, so it stops being offered.
            let was_armed = st.equalize_undo.borrow().is_some();
            *st.equalize_undo.borrow_mut() = None;
            push_geometry_inplace(&w, &st);
            if was_armed {
                w.set_equalize_undone(false);
            }
        }
    });
}

pub(super) fn install_equalize_views(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_equalize_views(move |idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        let geom = current_geom(&st);
        // The tree is flattened over the unit square, so the root governs
        // it whole; any other separator governs the area recorded on it.
        let (path, area) = if idx < 0 {
            (NodePath::new(), UNIT_AREA)
        } else {
            match geom.splitters.get(idx as usize) {
                Some(sp) => (sp.path.clone(), sp.area),
                None => return,
            }
        };
        // Read the way back BEFORE holding the tree: looking it up needs
        // the tree too, and the edit below holds it mutably.
        let back = equalize_undo_for(&st, &path);
        let restored;
        let taken = {
            // ONE borrow for the whole edit. Evening out takes the tree
            // mutably and the ratios it wrote are read back through the
            // same guard: a second `borrow()` while the first is alive is a
            // run-time panic, which the compiler does not catch.
            let mut layout = st.layout.borrow_mut();
            restored = back.is_some_and(|before| layout.restore_ratios(&path, &before));
            if restored {
                None
            } else {
                layout.equalize(&path, area, LAYOUT_GAP, MIN_SPLIT_RATIO)
            }
        };
        if let Some((before, after)) = taken {
            // What a second double-click here will put back.
            *st.equalize_undo.borrow_mut() = Some((path, before, after));
        } else if restored {
            *st.equalize_undo.borrow_mut() = None;
        }
        // Neither: the views were already even. An older way back, taken
        // somewhere else in the tree, is left standing.
        push_geometry_inplace(&w, &st);
        push_equalize_state(&w, &st);
    });
}

pub(super) fn install_view_torn_off(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_view_torn_off(move |src: i32, abs_x: f32, abs_y: f32| {
        let Some(w) = weak.upgrade() else { return };
        if !dropped_outside(&w, abs_x, abs_y) {
            return;
        }
        let at = window_logical_to_screen(&w, abs_x, abs_y);
        if tear_off_view(&st, src.max(0) as usize, at) {
            switch_active_panel(&w, &st);
            st.persist_workspace();
        }
    });
}

pub(super) fn install_views_swapped(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_views_swapped(move |a: i32, b: i32| {
        let Some(w) = weak.upgrade() else { return };
        let (a, b) = (a.max(0) as usize, b.max(0) as usize);
        // No proportion changes, so the way back from an "even out the
        // views" stays valid and is deliberately left on offer.
        if st.layout.borrow_mut().swap_panels(a, b) {
            push_geometry_inplace(&w, &st);
        }
    });
}
