use super::*;

pub(super) fn install_rows_area_width(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_rows_area_width(move |idx: i32, width: f32| {
            let Some(w) = weak.upgrade() else { return };
            let idx = idx.max(0) as usize;
            let width = width.max(0.0);
            let compact = st.config.borrow().compact_icon_rows_in_preview;
            {
                let mut panels = st.panels.borrow_mut();
                let Some(panel) = panels.get_mut(idx) else {
                    return;
                };
                // Sub-pixel churn (fractional scaling, scrollbar toggling) must
                // not re-pack the grid.
                if (panel.grid_width.get() - width).abs() < 0.5 {
                    return;
                }
                let tab = &panel.tabs.tabs[panel.tabs.active];
                let before = grid_metrics(tab.zoom, panel.grid_width.get());
                let after = grid_metrics(tab.zoom, width);
                panel.grid_width.set(width);
                if !tab.mode.is_grid() {
                    return; // remembered; the next rebuild will use it
                }
                // Two widths of the same "packing bucket" yield the exact same
                // geometry: resizing across one must not rebuild the rows.
                if before == after {
                    return;
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
        });
}

pub(super) fn install_zoom_view(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::PanelsApi>().on_zoom_view(
        move |idx: i32, delta: i32, top: f32, height: f32, pointer_y: f32| {
            let Some(w) = weak.upgrade() else {
                return top.max(0.0);
            };
            let idx = idx as usize;
            let crossed_mode;
            {
                let mut panels = st.panels.borrow_mut();
                if idx >= panels.len() {
                    return top.max(0.0);
                }
                let active = panels[idx].tabs.active;
                let t = &mut panels[idx].tabs.tabs[active];
                let new_zoom = (t.zoom + delta).clamp(MIN_ZOOM, MAX_ZOOM);
                if new_zoom == t.zoom {
                    return top.max(0.0); // already at the bounds → nothing to do
                }
                // The grid is a LAYOUT: the wheel only resizes its tiles.
                // The two other modes follow the level (list below the
                // thumbnail floor, previews at or above it).
                let was_thumbnails = t.mode.thumbnails();
                t.zoom = new_zoom;
                if t.mode != ViewMode::Grid {
                    t.mode = if new_zoom >= THUMB_ZOOM {
                        ViewMode::Previews
                    } else {
                        ViewMode::List
                    };
                }
                crossed_mode = t.mode.thumbnails() != was_thumbnails;
            }
            let compact = st.config.borrow().compact_icon_rows_in_preview;
            {
                let panels = st.panels.borrow();
                if let Some(panel) = panels.get(idx) {
                    let style = panel_row_style(panel, compact);
                    // Captures the anchor on the old geometry and performs the
                    // relayout in the SAME pass over the rows. A single visible
                    // selection takes priority; otherwise the row under the pointer, then
                    // the viewport's center. No listing or disk access.
                    let anchored_top = zoom_panel_visuals(
                        panel,
                        style,
                        crossed_mode,
                        ZoomViewport {
                            top,
                            height,
                            pointer_y,
                        },
                    );
                    // The final geometry is already available: publishing its
                    // visible range before even returning control prevents a
                    // worker from picking up a stale priority job during the
                    // very short interval preceding the Slint callback.
                    let (first, end) = row_range_for_content_span(
                        &*panel.rows_model,
                        anchored_top,
                        anchored_top + height.max(0.0),
                    );
                    if first < end {
                        st.thumb_scheduler.update_viewport(
                            idx,
                            first as i32,
                            end.saturating_sub(1) as i32,
                        );
                    } else {
                        st.thumb_scheduler.update_viewport(idx, -1, -1);
                    }
                    drop(panels);
                    update_panels_ui(&w, &st);
                    if crossed_mode {
                        request_thumbnails(&st);
                    }
                    return anchored_top;
                }
            }
            top.max(0.0)
        },
    );
}

pub(super) fn install_toggle_show_hidden(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_toggle_show_hidden(move |idx: i32| {
            let Some(w) = weak.upgrade() else { return };
            let idx = idx as usize;
            let path = {
                let mut panels = st.panels.borrow_mut();
                if idx >= panels.len() {
                    return;
                }
                let active = panels[idx].tabs.active;
                let t = &mut panels[idx].tabs.tabs[active];
                t.show_hidden = !t.show_hidden;
                t.current_path.clone()
            };
            refresh_listing(&w, &st, &path);
        });
}

pub(super) fn install_imgmeta_ready(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.global::<crate::PanelsApi>().on_imgmeta_ready(
        move |panel_idx: i32,
              row_idx: i32,
              path: SharedString,
              resolution: SharedString,
              depth: SharedString,
              mtime: SharedString| {
            let key = path.to_string();
            let mtime = mtime.parse::<i64>().unwrap_or(0);
            st.imgmeta_cache.borrow_mut().insert(
                key.clone(),
                (mtime, resolution.to_string(), depth.to_string()),
            );
            let target = PathBuf::from(&key);
            let panels = st.panels.borrow();
            let Some(panel) = usize::try_from(panel_idx)
                .ok()
                .and_then(|index| panels.get(index))
            else {
                return;
            };
            let Some(row_index) = usize::try_from(row_idx).ok() else {
                return;
            };
            let model = &panel.rows_model;
            let Some(mut row) = model.row_data(row_index) else {
                return;
            };
            // A watcher/re-sort can recycle the index while reading:
            // the full path remains authoritative before any mutation.
            if row.resolution.is_empty() && row_path(&row).as_deref() == Some(target.as_path()) {
                row.resolution = resolution.clone();
                row.depth = depth.clone();
                model.set_row_data(row_index, row);
            }
        },
    );
}

pub(super) fn install_thumb_ready(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.global::<crate::PanelsApi>().on_thumb_ready(
        move |path: SharedString, serial: i32, img: Image| {
            let key = path.to_string();
            let target = PathBuf::from(&key);
            let locations = st.thumb_scheduler.in_flight_locations(&target, serial);
            if locations.is_empty() {
                // The path was invalidated or superseded while decoding. Never
                // let an old generation repopulate the cache under a reused name.
                st.thumb_scheduler.complete(&target, serial);
                return;
            }
            st.thumb_cache.borrow_mut().put(key.clone(), img.clone());
            let panels = st.panels.borrow();
            for location in locations {
                let Some(panel) = panels.get(location.panel) else {
                    continue;
                };
                let tab = &panel.tabs.tabs[panel.tabs.active];
                if !tab.mode.thumbnails() {
                    continue;
                }
                let model = &panel.rows_model;
                let Some(mut row) = model.row_data(location.row) else {
                    continue;
                };
                // The index may have been recycled by a watcher: the path remains
                // authoritative. A row outside the window keeps only the LRU.
                if row.rendered
                    && row.thumbnail.size().width == 0
                    && row_path(&row).as_deref() == Some(target.as_path())
                {
                    row.thumbnail = img.clone();
                    model.set_row_data(location.row, row);
                }
            }
            // The worker waits for this acknowledgment before choosing the next job:
            // a scroll that happened during the decoding can therefore re-prioritize the
            // queue before the next thumbnail is started.
            st.thumb_scheduler.complete(Path::new(&key), serial);
        },
    );
}

pub(super) fn install_folder_stats_ready(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.global::<crate::PanelsApi>().on_folder_stats_ready(
        move |panel_idx: i32,
              row_idx: i32,
              path: SharedString,
              mtime_str: SharedString,
              size_str: SharedString| {
            let m = mtime_str.parse::<i64>().ok();
            let s = size_str.parse::<u64>().ok();
            if m.is_none() && s.is_none() {
                return;
            }
            let key = path.to_string();
            if let Some(m) = m {
                st.rmtime_cache.borrow_mut().insert(key.clone(), m);
            }
            if let Some(s) = s {
                st.size_cache.borrow_mut().insert(key.clone(), s);
            }
            let lang = st.config.borrow().language;
            let now = now_unix();
            let target = PathBuf::from(&key);
            let panels = st.panels.borrow();
            let Some(panel) = usize::try_from(panel_idx)
                .ok()
                .and_then(|index| panels.get(index))
            else {
                return;
            };
            let Some(row_index) = usize::try_from(row_idx).ok() else {
                return;
            };
            let model = &panel.rows_model;
            let Some(mut row) = model.row_data(row_index) else {
                return;
            };
            if row.is_dir && row_path(&row).as_deref() == Some(target.as_path()) {
                if let Some(m) = m {
                    apply_rmtime_to_row(&mut row, m, now, lang);
                }
                if let Some(s) = s {
                    row.size = rfs::format_size(s, i18n::size_units(lang)).into();
                }
                model.set_row_data(row_index, row);
            }
        },
    );
}

pub(super) fn install_panel_column_resized(window: &MainWindow, state: AppState) {
    let st = state.clone();
    // End of a column resize: stores its width
    // per panel (survives model rebuilds). Uniform for any
    // resizable column (name/path/size/modified/ext/resolution/depth).
    window.global::<crate::PanelsApi>().on_panel_column_resized(
        move |idx: i32, id: SharedString, width: f32| {
            let mut panels = st.panels.borrow_mut();
            if let Some(p) = panels.get_mut(idx as usize) {
                set_col_width(&mut p.columns, &id, width);
            }
        },
    );
}

pub(super) fn install_column_toggle(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::PanelsApi>().on_column_toggle(
        move |idx: i32, id: SharedString, visible: bool| {
            let Some(w) = weak.upgrade() else { return };
            let id = id.to_string();
            {
                let mut panels = st.panels.borrow_mut();
                if let Some(p) = panels.get_mut(idx as usize) {
                    // The "name" anchor column is never hideable.
                    if id != "name"
                        && let Some(c) = p.columns.iter_mut().find(|c| c.id == id)
                    {
                        c.visible = visible;
                    }
                }
            }
            update_panels_ui(&w, &st);
            // resolution/depth column enabled → computes the missing image
            // metadata (no-op if no relevant column is visible).
            request_imgmeta(&st);
            st.persist_workspace();
        },
    );
}

pub(super) fn install_column_moved(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::PanelsApi>().on_column_moved(
        move |idx: i32, id: SharedString, delta: i32| {
            let Some(w) = weak.upgrade() else { return };
            let id = id.to_string();
            {
                let mut panels = st.panels.borrow_mut();
                if let Some(p) = panels.get_mut(idx as usize) {
                    reorder_column_by_delta(&mut p.columns, &id, delta as f32);
                }
            }
            update_panels_ui(&w, &st);
            st.persist_workspace();
        },
    );
}
