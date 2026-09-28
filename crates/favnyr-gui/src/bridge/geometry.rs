use super::*;

/// Where an equalize was applied, the ratios it replaced, and the ones it
/// wrote in their place.
pub(super) type EqualizeUndo = (NodePath, Vec<f32>, Vec<f32>);

/// The whole container: the tree is flattened into fractions of it, so the
/// root of the layout governs exactly this.
pub(super) const UNIT_AREA: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1.0,
    h: 1.0,
};

/// The layout tree reserves nothing for its separators: the panels touch and
/// the grips are overlays on the seams. The visible gutter is an inset drawn
/// by the view, not a hole in the tree.
pub(super) const LAYOUT_GAP: f32 = 0.0;

/// Flattens the current tree into [0,1] fractions of the container.
pub(super) fn current_geom(state: &AppState) -> Layout {
    state.layout.borrow().compute(UNIT_AREA, LAYOUT_GAP)
}

/// The ratios to put back if the way out of the last equalize still applies at
/// `path`: it was taken there, and nothing has moved those ratios since.
///
/// Comparing against what the equalize WROTE is what makes the check
/// self-contained — a hand resize, a new view, a closed one or a workspace
/// swap all show up as a mismatch, with nothing to remember to call.
pub(super) fn equalize_undo_for(
    state: &AppState,
    path: &[favnyr_core::layout::Side],
) -> Option<Vec<f32>> {
    let slot = state.equalize_undo.borrow();
    let (taken_at, before, after) = slot.as_ref()?;
    if taken_at.as_slice() != path {
        return None;
    }
    let now = state.layout.borrow().ratios(path);
    let untouched = now.len() == after.len()
        && now
            .iter()
            .zip(after)
            .all(|(a, b)| (a - b).abs() < RATIO_MATCH);
    untouched.then(|| before.clone())
}

/// Two ratios closer than this came from the same write.
pub(super) const RATIO_MATCH: f32 = 1e-4;

/// Tells the view whether there is anything to even out, and whether the way
/// back is currently on offer (the menu row then reads "restore" instead).
pub(super) fn push_equalize_state(window: &MainWindow, state: &AppState) {
    window
        .global::<crate::PanelsApi>()
        .set_equalize_available(state.panels.borrow().len() > 1);
    window
        .global::<crate::PanelsApi>()
        .set_equalize_undone(equalize_undo_for(state, &[]).is_some());
}

/// The fractional rectangle of a view, as the GUI reads it.
pub(super) fn panel_box(r: Rect) -> PanelBox {
    PanelBox {
        fx: r.x,
        fy: r.y,
        fw: r.w,
        fh: r.h,
    }
}

/// Fractional rectangle of each panel, indexed by panel index.
pub(super) fn panel_rects(geom: &Layout, n: usize) -> Vec<Rect> {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    let mut rects = vec![full; n];
    for pg in &geom.panels {
        if pg.panel < n {
            rects[pg.panel] = pg.rect;
        }
    }
    rects
}

/// Converts the flattened splitters into `SplitterView` for Slint. The `idx`
/// (position in `geom.splitters`) is used to find the node again on the resize side.
pub(super) fn splitter_views(geom: &Layout) -> Vec<SplitterView> {
    geom.splitters
        .iter()
        .enumerate()
        .map(|(i, sp)| {
            let vertical = matches!(sp.dir, SplitDir::Row);
            let (pos, cross_start, cross_len) = if vertical {
                (sp.rect.x, sp.rect.y, sp.rect.h)
            } else {
                (sp.rect.y, sp.rect.x, sp.rect.w)
            };
            // Views under this separator: every separator sitting below it in
            // the tree, plus one. A subtree with n views holds n - 1 splits.
            let governs = geom
                .splitters
                .iter()
                .filter(|other| other.path.starts_with(&sp.path))
                .count()
                + 1;
            SplitterView {
                idx: i as i32,
                vertical,
                pos,
                cross_start,
                cross_len,
                area_x: sp.area.x,
                area_y: sp.area.y,
                area_w: sp.area.w,
                area_h: sp.area.h,
                governs: governs as i32,
            }
        })
        .collect()
}

/// Mutates the geometry **in place** (the views' boxes + the splitters'
/// pos/cross) without replacing any model. Crucial during a resize: a
/// `set_splitters(new model)` would recreate the `PanelSplitterAbs` element
/// currently being dragged (losing the `pressed` state → drag interrupted after a few
/// pixels). We therefore only replace the model if the **structure** changes
/// (different number of splitters) — which never happens mid-resize.
///
/// The boxes live in their own model, apart from `PanelView`: writing a
/// `PanelView` invalidates every binding that reads it, `rendered-rows`
/// included, so each frame of a drag would put the whole row repeater of the
/// two views concerned back to work. Same separation as the footers.
pub(super) fn push_geometry_inplace(window: &MainWindow, state: &AppState) {
    let geom = current_geom(state);
    let boxes_model = window.global::<crate::PanelsApi>().get_panel_boxes();
    let rects = panel_rects(&geom, boxes_model.row_count());
    for (i, r) in rects.iter().enumerate() {
        if let Some(cur) = boxes_model.row_data(i)
            && ((cur.fx - r.x).abs() > 1e-5
                || (cur.fy - r.y).abs() > 1e-5
                || (cur.fw - r.w).abs() > 1e-5
                || (cur.fh - r.h).abs() > 1e-5)
        {
            boxes_model.set_row_data(i, panel_box(*r));
        }
    }

    let new_sv = splitter_views(&geom);
    let sp_model = window.global::<crate::PanelsApi>().get_splitters();
    if sp_model.row_count() == new_sv.len() {
        // In-place update: doesn't recreate the elements → the drag survives.
        for (i, v) in new_sv.iter().enumerate() {
            if let Some(cur) = sp_model.row_data(i)
                && ((cur.pos - v.pos).abs() > 1e-5
                    || (cur.cross_start - v.cross_start).abs() > 1e-5
                    || (cur.cross_len - v.cross_len).abs() > 1e-5)
            {
                sp_model.set_row_data(i, v.clone());
            }
        }
    } else {
        window
            .global::<crate::PanelsApi>()
            .set_splitters(ModelRc::new(VecModel::from(new_sv)));
    }
}

/// Pushes the full list of panels to Slint as `[PanelView]`
/// (each panel carries its row model, its tabs, its current
/// path, its pre-formatted footer, etc.).
pub(super) fn update_panels_ui(window: &MainWindow, state: &AppState) {
    let panels = state.panels.borrow();
    let lang = state.config.borrow().language;
    let strings = i18n::strings_for(lang);
    let prefix = strings.panel_prefix.to_string();
    let geom = current_geom(state);
    let rects = panel_rects(&geom, panels.len());
    // View footers DECOUPLED from `PanelView`: accumulated separately and pushed into
    // the parallel `panel-footers` model. Writing the selection counter
    // there (rather than into the panel struct) avoids rewriting the whole
    // `PanelView` on every rubber-band step, which would invalidate
    // `rendered-rows` and break the selection's incremental rendering.
    let (views, footers): (Vec<PanelView>, Vec<SharedString>) = panels
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let tab = &p.tabs.tabs[p.tabs.active];
            let leaf = tab_title(&tab.current_path);
            let total = p.rows_model.row_count();
            let selected = count_selected(&*p.rows_model) as usize;
            // "Hidden" reminder only when hidden items are NOT shown.
            let hidden = if tab.show_hidden { 0 } else { p.hidden_count };
            // Initial listing still in flight (async startup):
            // "…" rather than a misleading "Empty folder".
            let footer: SharedString = if p.pending_initial || p.pending_listing {
                "…".into()
            } else {
                i18n::footer_text(lang, total, selected, hidden).into()
            };
            // Tabs: titles + FLAT geometry along the bar's main
            // axis (imposed width + cumulative offset — not uniform in Y
            // for a vertical bar). Source of truth for the drag on the Slint side.
            let tabs_infos: Vec<TabInfo> = {
                let titles: Vec<String> = p
                    .tabs
                    .tabs
                    .iter()
                    .map(|t| tab_title(&t.current_path))
                    .collect();
                let geo = tab_layout_for(p.tab_bar_mode, &titles);
                titles
                    .into_iter()
                    .zip(geo)
                    .zip(&p.tabs.tabs)
                    .map(|((title, (width, offset)), t)| TabInfo {
                        title: title.into(),
                        width,
                        offset,
                        // Full path → tooltip on hover.
                        path: t.current_path.display().to_string().into(),
                    })
                    .collect()
            };
            // Total width of VISIBLE columns (px) — precise (accounts for
            // visibility AND the fixed resolution/depth columns). Drives the
            // horizontal scroll on the Slint side (`content-width`).
            let content_w: f32 = {
                const COL_GAP: f32 = 8.0; // MUST == Tokens.col-gap (slint)
                // Widths of visible columns and the gaps between them (n - 1),
                // with no gap after the last column.
                let widths: Vec<f32> = p
                    .columns
                    .iter()
                    .filter(|c| c.visible)
                    .map(|c| col_width(&p.columns, &c.id))
                    .collect();
                let gaps = COL_GAP * widths.len().saturating_sub(1) as f32;
                widths.iter().sum::<f32>() + gaps
            };
            let view = PanelView {
                rows: ModelRc::from(p.rows_model.clone()),
                rendered_rows: p.rendered_rows_model.clone(),
                rows_revision: p.rows_revision.get(),
                viewport_reset_gen: p.viewport_reset_gen.get(),
                tabs: ModelRc::new(VecModel::from(tabs_infos)),
                current_path: tab.current_path.display().to_string().into(),
                total_count: total as i32,
                can_go_back: tab.history.can_back(),
                can_go_forward: tab.history.can_forward(),
                sort_column: tab.sort.column.code().into(),
                sort_asc: matches!(tab.sort.order, SortOrder::Asc),
                active_tab_idx: p.tabs.active as i32,
                title: format!("{prefix}{}{}{leaf}", i + 1, i18n::tr(lang, "separator_dot")).into(),
                crumbs: ModelRc::new(VecModel::from(breadcrumbs(&tab.current_path))),
                col_name_w: col_width(&p.columns, "name"),
                col_path_w: col_width(&p.columns, "path"),
                col_size_w: col_width(&p.columns, "size"),
                col_modified_w: col_width(&p.columns, "modified"),
                col_ext_w: col_width(&p.columns, "ext"),
                col_resolution_w: col_width(&p.columns, "resolution"),
                col_depth_w: col_width(&p.columns, "depth"),
                col_age_w: col_width(&p.columns, "age"),
                columns: ModelRc::new(VecModel::from({
                    // Cumulative offsets on VISIBLE columns (px), for the
                    // GUI-side drag target computation (without absolute-position).
                    // Includes the inter-column gap (= `Tokens.col-gap`) so that
                    // offset-space matches screen-space (otherwise the
                    // preview drifts by ~8px per column crossed).
                    const COL_GAP: f32 = 8.0; // MUST == Tokens.col-gap (slint)
                    let mut off = 0.0_f32;
                    let mut v = Vec::new();
                    for c in p.columns.iter().filter(|c| c.visible) {
                        v.push(column_info(&strings, c, off));
                        off += col_width(&p.columns, &c.id) + COL_GAP;
                    }
                    v
                })),
                // The check/uncheck menu names the image-only columns in full,
                // like the Settings list: read as a bare list, "Depth" does not
                // say what it measures. The visible headers just above keep the
                // short form.
                columns_menu: ModelRc::new(VecModel::from(
                    p.columns
                        .iter()
                        .map(|c| column_info_explicit(&strings, c, 0.0))
                        .collect::<Vec<_>>(),
                )),
                preview_mode: tab.mode.thumbnails(),
                view_mode: tab.mode.code().into(),
                grid_mode: tab.mode.is_grid(),
                grid_cols: p.grid_cols.get(),
                show_subfolders: tab.subfolders,
                content_width: content_w,
                show_hidden: tab.show_hidden,
                group_mode: tab.group_mode.code().into(),
                cursor_row: tab.cursor,
                scroll_gen: tab.scroll_gen,
                unavailable: p.unavailable, // folder unreachable → banner
                ext_filter_on: tab.ext_filter_on,
                ext_filter: tab.ext_filter.as_str().into(),
                tab_bar_mode: p.tab_bar_mode as i32, // 0 top · 1 left · 2 right
                vbar_user_w: p.vbar_user_w,          // handle width, 0 = auto
            };
            (view, footer)
        })
        .unzip();
    let active = *state.active_panel.borrow() as i32;
    let can_add = panels.len() < MAX_PANELS;
    // Active panel's "extension filter" mode → drives the keyboard
    // routing (Backspace/Escape) on the Slint side, shared with type-ahead.
    let active_ext_on = panels
        .get(active as usize)
        .map(|p| p.tabs.tabs[p.tabs.active].ext_filter_on)
        .unwrap_or(false);
    drop(panels);
    // IN-PLACE update if the number of panels is unchanged (active view
    // switch, refresh, sort, navigation…): each `PanelView` is mutated via
    // `set_row_data` instead of replacing the whole model. Otherwise `set_panels`
    // recreates ALL the `PanelComponent`s → an in-progress gesture (file drag,
    // middle-scroll, rubber-band) started in a NON-active view would die the
    // instant it becomes active. The model is only replaced for a
    // STRUCTURAL change (split/close → the number of panels changes).
    let existing = window.global::<crate::PanelsApi>().get_panels();
    if existing.row_count() == views.len() {
        for (i, v) in views.into_iter().enumerate() {
            existing.set_row_data(i, v);
        }
    } else {
        window
            .global::<crate::PanelsApi>()
            .set_panels(ModelRc::new(VecModel::from(views)));
    }
    // View footers in their PARALLEL model. In-place update when the
    // panel count is unchanged → the model instance is preserved, so
    // `push_active_footer` keeps writing into the same live model.
    let existing_footers = window.global::<crate::PanelsApi>().get_panel_footers();
    if existing_footers.row_count() == footers.len() {
        for (i, f) in footers.into_iter().enumerate() {
            existing_footers.set_row_data(i, f);
        }
    } else {
        window
            .global::<crate::PanelsApi>()
            .set_panel_footers(ModelRc::new(VecModel::from(footers)));
    }
    // Where each view sits, in its own parallel model for the same reason.
    let boxes: Vec<PanelBox> = rects.iter().map(|r| panel_box(*r)).collect();
    let existing_boxes = window.global::<crate::PanelsApi>().get_panel_boxes();
    if existing_boxes.row_count() == boxes.len() {
        for (i, b) in boxes.into_iter().enumerate() {
            existing_boxes.set_row_data(i, b);
        }
    } else {
        window
            .global::<crate::PanelsApi>()
            .set_panel_boxes(ModelRc::new(VecModel::from(boxes)));
    }
    window
        .global::<crate::PanelsApi>()
        .set_splitters(ModelRc::new(VecModel::from(splitter_views(&geom))));
    push_equalize_state(window, state);
    window
        .global::<crate::PanelsApi>()
        .set_active_panel_idx(active);
    window
        .global::<crate::PanelsApi>()
        .set_active_ext_filter_on(active_ext_on);
    window
        .global::<crate::PanelsApi>()
        .set_can_add_panel(can_add);
    // Is there at least one "unavailable" panel? Drives the auto
    // re-check Timer on the Slint side.
    let any_unavail = state.panels.borrow().iter().any(|p| p.unavailable);
    window
        .global::<crate::SidebarApi>()
        .set_any_unavailable(any_unavail);
    // This central path is taken by structural mutations and
    // tab state changes. The comparison is purely in-memory, so the
    // title stays reactive without hooking any I/O into UI interactions.
    update_window_title(window, state);
}

/// Updates the window's OS title (taskbar / Alt-Tab):.
/// "{workspace} — Favnyr[*]" (em dash U+2014) if the current state comes from
/// a named workspace, otherwise "Favnyr". The asterisk is fed by THE SAME
/// source of truth as the golden Update button (`current_workspace_is_dirty`).
/// The workspace name comes first so it survives taskbar label
/// truncation (from the end).
pub(super) fn update_window_title(window: &MainWindow, state: &AppState) {
    let brand = i18n::strings_for(state.config.borrow().language).app_title;
    let current = state.current_workspace.borrow().clone();
    let title = match current.as_deref() {
        Some(name) if !name.trim().is_empty() => {
            let marker = if current_workspace_is_dirty(state) {
                "*"
            } else {
                ""
            };
            format!("{name} — {brand}{marker}")
        }
        _ => brand.to_string(),
    };
    window.set_window_title(title.into());
}
