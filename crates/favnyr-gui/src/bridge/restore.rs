use super::*;

pub(super) fn home_dir() -> PathBuf {
    // Cross-platform: `$HOME` (Linux) / `%USERPROFILE%` (Windows).
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

/// Builds `(panels, stretches, active_panel)` from a `WorkspaceState`.
/// Shared by `from_workspace` (creation) and `replace_with_workspace`
/// (in-place loading). Validates the paths (fallback `$HOME`).
pub(super) fn build_panels(ws: WorkspaceState) -> (Vec<Panel>, LayoutNode, usize) {
    let ws = ws.sanitized();
    // `sanitized()` guarantees a valid tree covering exactly the panels.
    let layout = ws.layout_tree();
    let mut panels = Vec::with_capacity(ws.panels.len());
    for ps in &ws.panels {
        let mut tabs = Vec::with_capacity(ps.tabs.len());
        for ts in &ps.tabs {
            let path = resolve_restored_path(&ts.path);
            let sort = SortState {
                column: ts.sort_column,
                order: ts.sort_order,
            };
            tabs.push(Tab::restored(
                path,
                sort,
                tab_mode_of(ts),
                ts.zoom,
                ts.show_hidden,
                ts.group_mode,
                ts.subfolders,
                ts.collapsed.clone(),
            ));
        }
        // `tabs` guaranteed non-empty (sanitized removes empty panels).
        let active = ps.active_tab.min(tabs.len() - 1);
        let initial_display = tabs[active].current_path.clone();
        let (rows_model, rendered_rows_model) = new_row_models();
        panels.push(Panel {
            tabs: TabBook { tabs, active },
            rows_model,
            rendered_rows_model,
            rows_revision: Cell::new(0),
            viewport_reset_gen: Cell::new(0),
            viewport_top: Cell::new(0.0),
            viewport_height: Cell::new(DEFAULT_RENDER_VIEWPORT_HEIGHT),
            rendered_first: Cell::new(0),
            rendered_end: Cell::new(0),
            displayed_path: initial_display,
            columns: columns::sanitize(ps.columns.clone()),
            hidden_count: 0,
            unavailable: false,
            tabs_viewport_x: 0.0,
            tab_bar_mode: ps.tab_bar_mode.min(2),
            vbar_user_w: ps.vbar_width.max(0.0),
            pending_initial: false,
            pending_listing: false,
            listing_gen: 0,
            pending_select: None,
            source: RefCell::new(None),
            entry_count: Cell::new(0),
            grid_width: Cell::new(0.0),
            grid_cols: Cell::new(0),
            sub_gen: Cell::new(0),
        });
    }
    let active_panel = ws.active_panel.min(panels.len() - 1);
    (panels, layout, active_panel)
}

/// Resolves a path restored from the workspace: we KEEP the path as-is
/// (even if it isn't accessible — network drive not started, mount missing).
/// Only an empty path falls back to `$HOME`. The inaccessible tab shows a
/// "not found" banner via `refresh_listing` and refreshes automatically
/// as soon as the folder becomes accessible again.
pub(super) fn resolve_restored_path(raw: &str) -> PathBuf {
    if raw.is_empty() {
        home_dir()
    } else {
        PathBuf::from(raw)
    }
}
