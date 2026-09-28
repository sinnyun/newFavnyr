use super::*;

/// Resolves the id of the currently active named workspace by looking it up
/// by name in the on-disk list. `None` if no named workspace is active, or
/// it can no longer be found.
pub(super) fn current_workspace_id(state: &AppState) -> Option<String> {
    let name = state.current_workspace.borrow().clone()?;
    workspace::list_named_workspaces(&paths::workspaces_dir())
        .into_iter()
        .find(|meta| meta.name == name)
        .map(|meta| meta.id)
}

/// Overwrites a named workspace with the live state and synchronizes, in this
/// order, the current identity, the single dirty snapshot, and the session workspace.
/// Shared by the Update button, Ctrl+S, and the "save and…" safeguard.
pub(super) fn overwrite_named_from_live(state: &AppState, id: &str) -> favnyr_core::Result<()> {
    let ws = state.capture_workspace();
    workspace::overwrite_named_workspace(&paths::workspaces_dir(), id, &ws)?;
    if let Some(meta) = workspace::list_named_workspaces(&paths::workspaces_dir())
        .into_iter()
        .find(|meta| meta.id == id)
    {
        *state.current_workspace.borrow_mut() = Some(meta.name);
    }
    state.remember_workspace_saved();
    state.persist_workspace();
    Ok(())
}

/// Loads the named workspace `id` (replaces the layout, repopulates, refreshes,
/// persists, updates the title). Shared by `ws-load`, `ws-load-guarded`, and
/// `ws-save-and-load`.
pub(super) fn load_named_into(window: &MainWindow, state: &AppState, id: &str) {
    match workspace::load_named_workspace(&paths::workspaces_dir(), id) {
        Ok((name, ws)) => {
            info!(id = %id, name, "workspace loaded");
            state.replace_with_workspace(ws);
            *state.current_workspace.borrow_mut() = Some(name);
            state.remember_workspace_saved();
            push_sidebar_sections_ui(window, state);
            window.set_closed_tabs_available(state.has_closed_tabs());
            refresh_all_panels(window, state);
            state.persist_workspace();
            update_window_title(window, state);
        }
        Err(err) => error!(error = %err, id = %id, "load named workspace failed"),
    }
}

/// Starts over with a BLANK workspace (1 panel, `$HOME`, no longer attached to any name).
/// Shared by `ws-reset`, `ws-reset-guarded`, and the "reset" branch of
/// `ws-save-and-load`.
pub(super) fn reset_into(window: &MainWindow, state: &AppState) {
    state.reset_to_blank();
    push_sidebar_sections_ui(window, state);
    window.set_closed_tabs_available(false);
    refresh_all_panels(window, state);
    state.persist_workspace();
    update_window_title(window, state);
    info!("workspace reset to blank");
}

/// "Content" signature of a workspace for "modified" detection.
///
/// We compare ONLY what matters to the user: **the tabs open per
/// view** (path + sort + per-tab display settings), each view's **tab bar
/// position**, and the four **collapsible sections of the left
/// sidebar**. We deliberately IGNORE volatile fields or ones not faithful to a
/// capture∘load round-trip, which caused false positives: split
/// ratios (`stretch` — always 1.0 at capture time — and ratios carried by `layout`),
/// column widths, vertical bar width, as well as the ACTIVE
/// tab/panel (simple focus, not an open/close).
///
/// All the retained fields are captured DIRECTLY from the live state, written
/// as-is into the TOML, and faithfully reconstructed by `build_panels` → the
/// comparison is stable in both directions.
pub(super) type TabSig = (
    String,
    SortColumn,
    SortOrder,
    bool,
    bool,
    GroupMode,
    Option<i32>,
    String,
    bool,
    Vec<String>,
);
pub(super) type WorkspaceSig = (Vec<(Vec<TabSig>, u8)>, SidebarSectionsState);
pub(super) fn workspace_signature(ws: &WorkspaceState) -> WorkspaceSig {
    let panels = ws
        .panels
        .iter()
        .map(|p| {
            let tabs: Vec<TabSig> = p
                .tabs
                .iter()
                .map(|t| {
                    (
                        t.path.clone(),
                        t.sort_column,
                        t.sort_order,
                        t.preview,
                        t.show_hidden,
                        t.group_mode,
                        t.zoom,
                        t.view_mode.clone().unwrap_or_default(),
                        t.subfolders,
                        t.collapsed.clone(),
                    )
                })
                .collect();
            (tabs, p.tab_bar_mode)
        })
        .collect();
    (panels, ws.sidebar_sections)
}

/// Do two workspaces differ in their relevant CONTENT (tabs per view +
/// display settings)? We ignore `workspace_name`, the ratios, and the focus.
pub(super) fn workspaces_differ(a: &WorkspaceState, b: &WorkspaceState) -> bool {
    workspace_signature(a) != workspace_signature(b)
}

/// The SINGLE source of truth for the "modified workspace" state.
///
/// The saved snapshot is kept in memory; this function never touches
/// disk. The window title AND the `WorkspaceEntry.is_dirty` flag go
/// exclusively through here, so any future change to the criteria stays confined
/// to `workspace_signature` above.
pub(super) fn current_workspace_is_dirty(state: &AppState) -> bool {
    if state.current_workspace.borrow().is_none() {
        return false;
    }
    let saved = state.saved_workspace.borrow();
    let Some(saved) = saved.as_ref() else {
        return false;
    };
    let live = state.capture_workspace().sanitized();
    workspaces_differ(&live, saved)
}

/// Is the CURRENT named workspace "dirty" (live state ≠ state saved on
/// disk)? Returns `Some((id, name))` if YES, `None` otherwise — ad-hoc current
/// (never named), not found on disk, or identical to the saved one.
pub(super) fn dirty_current_workspace(state: &AppState) -> Option<(String, String)> {
    let name = state.current_workspace.borrow().clone()?;
    if !current_workspace_is_dirty(state) {
        return None;
    }
    let dir = paths::workspaces_dir();
    let meta = workspace::list_named_workspaces(&dir)
        .into_iter()
        .find(|m| m.name == name)?;
    Some((meta.id, name))
}

/// Reloads the list of named workspaces from disk and pushes it to the UI.
/// The core sorts by recency (most recent first); we reverse it here if
/// the user has switched the sort to "oldest first".
pub(super) fn refresh_workspaces_ui(window: &MainWindow, state: &AppState) {
    let dir = paths::workspaces_dir();
    let mut metas = workspace::list_named_workspaces(&dir);
    let cur_name = state.current_workspace.borrow().clone();
    // Same source of truth as the title's asterisk — no second comparison
    // logic should appear here.
    let dirty_id: Option<String> = current_workspace_is_dirty(state)
        .then(|| {
            cur_name
                .as_ref()
                .and_then(|name| metas.iter().find(|m| &m.name == name))
                .map(|m| m.id.clone())
        })
        .flatten();
    if !window.get_ws_sort_newest_first() {
        metas.reverse();
    }
    let entries: Vec<WorkspaceEntry> = metas
        .into_iter()
        .map(|m| {
            let is_current = cur_name.as_deref() == Some(m.name.as_str());
            let is_dirty = dirty_id.as_deref() == Some(m.id.as_str());
            WorkspaceEntry {
                id: m.id.into(),
                name: m.name.into(),
                panels: m.panels as i32,
                tabs: m.tabs as i32,
                is_current,
                is_dirty,
            }
        })
        .collect();
    window.set_workspaces(ModelRc::new(VecModel::from(entries)));
}

pub(super) fn normalized_workspace_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Does a workspace already have this name? File ids remain unique,
/// but the UI treats the name as the visible identity; rejecting duplicate names also
/// avoids any ambiguity when tracking the current workspace.
pub(super) fn workspace_name_exists(name: &str) -> bool {
    let wanted = normalized_workspace_name(name);
    !wanted.is_empty()
        && workspace::list_named_workspaces(&paths::workspaces_dir())
            .iter()
            .any(|meta| normalized_workspace_name(&meta.name) == wanted)
}
