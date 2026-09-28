use super::*;

/// Pushes the flattened tree + the container dropdown to the GUI.
pub(super) fn push_favorites_ui(window: &MainWindow, state: &AppState) {
    let fav = favorites_now(state);
    let rows: Vec<FavNode> = fav.flatten().iter().map(flat_to_favnode).collect();
    window
        .global::<crate::SidebarApi>()
        .set_fav_nodes(ModelRc::new(VecModel::from(rows)));
    // Toggles "Collapse all" (if ≥1 container is expanded) / "Expand all".
    window
        .global::<crate::SidebarApi>()
        .set_fav_can_collapse(fav.any_expanded());

    // Popup dropdown: "Root" + all indented containers.
    let s = i18n::strings_for(state.config.borrow().language);
    let mut labels: Vec<SharedString> = vec![s.fav_popup_root.clone()];
    let mut ids: Vec<String> = vec![String::new()];
    for (id, name, depth) in fav.containers() {
        let indent = "   ".repeat(depth.max(0) as usize);
        labels.push(format!("{indent}{name}").into());
        ids.push(id);
    }
    window
        .global::<crate::SidebarApi>()
        .set_fav_container_labels(ModelRc::new(VecModel::from(labels)));
    *state.fav_container_ids.borrow_mut() = ids;
}

/// Resolves the target FOLDER of a favorite `p`: the folder itself, or the parent
/// of a file. `None` + "not found" toast if absent/inaccessible.
pub(super) fn resolve_fav_dir(window: &MainWindow, p: &Path, lang: Lang) -> Option<PathBuf> {
    let target = if p.is_dir() {
        p.to_path_buf()
    } else if p.is_file() {
        p.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| p.to_path_buf())
    } else {
        notice(
            window,
            i18n::strings_for(lang).fav_toast_missing.clone(),
            NoticeKind::FavMissing,
        );
        return None;
    };
    if !target.is_dir() {
        notice(
            window,
            i18n::strings_for(lang).fav_toast_missing.clone(),
            NoticeKind::FavMissing,
        );
        return None;
    }
    Some(target)
}

/// Returns a sidebar place only when Favnyr can navigate it without a
/// potentially blocking local probe. Network paths are validated by listing.
pub(super) fn resolve_sidebar_place_dir(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return None;
    }
    (favnyr_core::places::is_network_path(&path) || path.is_dir()).then_some(path)
}

/// Opens a new default tab at an exact gap in any existing view.
pub(super) fn open_path_in_tab_at(
    window: &MainWindow,
    state: &AppState,
    path: PathBuf,
    panel: i32,
    gap: i32,
) -> bool {
    let Some(panel) = usize::try_from(panel).ok() else {
        return false;
    };
    {
        let mut panels = state.panels.borrow_mut();
        let Some(target) = panels.get_mut(panel) else {
            return false;
        };
        target
            .tabs
            .insert_tab_at(gap.max(0) as usize, Tab::new(path.clone()));
    }
    *state.active_panel.borrow_mut() = panel;
    load_directory(window, state, &path, false);
    true
}

/// Opens a favorite path in a NEW tab of the active view: folder →
/// tab on the folder; file → tab on its parent (we locate it, we
/// don't execute it). Path not found → "notice" toast.
pub(super) fn fav_open_path_new_tab(window: &MainWindow, state: &AppState, p: PathBuf) {
    let lang = state.config.borrow().language;
    let Some(target) = resolve_fav_dir(window, &p, lang) else {
        return;
    };
    let opened = state.with_tabs_mut(|book| {
        let a = book.open_after_active(target);
        book.tabs[a].current_path.clone()
    });
    load_directory(window, state, &opened, false);
}

/// Opens a favorite IN the active tab (overwrites the current view) — LEFT
/// click in the sidebar (MIDDLE click keeps opening in a new tab).
pub(super) fn fav_open_here(window: &MainWindow, state: &AppState, id: &str) {
    let lang = state.config.borrow().language;
    let Some(p) = favorites_now(state).path_of(id).map(PathBuf::from) else {
        return;
    };
    if let Some(target) = resolve_fav_dir(window, &p, lang) {
        load_directory(window, state, &target, true);
    }
}

/// Opens a favorite by its `id` in a NEW tab (no-op if container / unknown).
pub(super) fn fav_open(window: &MainWindow, state: &AppState, id: &str) {
    let path = favorites_now(state).path_of(id);
    if let Some(p) = path {
        fav_open_path_new_tab(window, state, PathBuf::from(p));
    }
}

/// Default alias of a path = its last component (otherwise the path).
pub(super) fn default_alias(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Saves `paths` into the `container` favorites folder: deduplication
/// (already-present path ignored) + default alias, then saves + refreshes
/// the UI + appropriate toast (added / already present). SHARED core for drag
/// drops onto a favorites folder — intra/inter-instance tab and
/// a view's file selection — and aligned with `on_fav_save_commit`.
pub(super) fn add_paths_to_favorite(
    window: &MainWindow,
    state: &AppState,
    paths: &[PathBuf],
    container: &str,
) {
    // EMPTY `container` = root (""): drop on the "no favorites" zone / favorites
    // dead zone. The model handles "" as the root container; we only
    // reject the absence of paths now.
    if paths.is_empty() {
        return;
    }
    let mut added = 0usize;
    {
        let mut fav = favorites_for_update(state);
        for p in paths {
            let path_str = p.display().to_string();
            if fav.container_has_path(container, &path_str) {
                continue; // already in this favorites folder → ignored
            }
            if fav
                .add_favorite(container, &default_alias(p), &path_str)
                .is_some()
            {
                added += 1;
            }
        }
    }
    let lang = state.config.borrow().language;
    if added > 0 {
        save_favorites(state);
        push_favorites_ui(window, state);
        notice(
            window,
            i18n::strings_for(lang).fav_toast_added.clone(),
            NoticeKind::FavAdded,
        );
    } else {
        // Nothing added = everything already existed (the container is guaranteed valid on
        // hover) → neutral info, not an error.
        notice(
            window,
            i18n::strings_for(lang).fav_toast_exists.clone(),
            NoticeKind::FavExists,
        );
    }
}

/// Opens the "Save as favorite" popup for a set of paths.
pub(super) fn open_fav_save_popup(window: &MainWindow, state: &AppState, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let multi = paths.len() > 1;
    let target = if multi {
        i18n::footer_items_text(state.snapshot_config().language, paths.len())
    } else {
        paths[0].display().to_string()
    };
    let alias = if multi {
        String::new()
    } else {
        default_alias(&paths[0])
    };
    *state.fav_save_pending.borrow_mut() = paths;
    push_favorites_ui(window, state); // refreshes the container dropdown
    window
        .global::<crate::SidebarApi>()
        .set_fav_save_multi(multi);
    window
        .global::<crate::SidebarApi>()
        .set_fav_save_target(target.into());
    window
        .global::<crate::SidebarApi>()
        .set_fav_save_alias(alias.into());
    window
        .global::<crate::SidebarApi>()
        .set_fav_container_index(0);
    window.global::<crate::SidebarApi>().set_fav_save_open(true);
    window
        .global::<crate::SidebarApi>()
        .set_fav_save_focus_armed(true);
}

/// Height of a favorites tree row (30px) + spacing (1px) = the vertical
/// pitch, MUST match the rendering (FavPanel: height 30 + spacing 1).
pub(super) const FAV_ROW_PITCH: f32 = 31.0;

/// From the drag geometry (cursor y, source row top y, source
/// index), computes `(target index, zone)` — zone 0 = before · 1 = inside · 2 = after.
pub(super) fn fav_drag_target(cur_y: f32, row_top: f32, row_idx: i32, count: i32) -> (i32, i32) {
    if count <= 0 {
        return (0, 1);
    }
    let list_origin = row_top - row_idx as f32 * FAV_ROW_PITCH;
    let hovered = (cur_y - list_origin) / FAV_ROW_PITCH;
    let ti = (hovered.floor() as i32).clamp(0, count - 1);
    let frac = hovered - hovered.floor();
    let zone = if frac < 0.33 {
        0
    } else if frac > 0.66 {
        2
    } else {
        1
    };
    (ti, zone)
}
/// A release outside the visible favorites tree has no reorder target. The
/// geometric helper clamps by design for edge scrolling, so this semantic gate
/// must run before its result is allowed to mutate the tree.
pub(super) fn fav_drop_target(
    commit_reorder: bool,
    cur_y: f32,
    row_top: f32,
    row_idx: i32,
    count: i32,
) -> Option<(i32, i32)> {
    commit_reorder.then(|| fav_drag_target(cur_y, row_top, row_idx, count))
}

/// Applies a node move based on the computed target/zone. Returns
/// `true` if the tree changed.
pub(super) fn fav_perform_move(
    state: &AppState,
    src_id: &str,
    target_index: i32,
    zone: i32,
) -> bool {
    let mut fav = favorites_for_update(state);
    let (target_id, target_is_container) = {
        let flat = fav.flatten();
        match flat.get(target_index.max(0) as usize) {
            Some(t) => (t.id.clone(), t.is_container),
            None => return false,
        }
    };
    if target_id == src_id {
        return false;
    }
    let target_parent = fav
        .nodes
        .iter()
        .find(|n| n.id == target_id)
        .map(|n| n.parent.clone())
        .unwrap_or_default();
    if zone == 1 && target_is_container {
        // Drop INSIDE the container (at the end) + expand it to see the result.
        let ok = fav.move_node(src_id, &target_id, None);
        if ok {
            fav.set_expanded(&target_id, true);
        }
        ok
    } else if zone == 0 {
        // Insert BEFORE the target (same parent).
        fav.move_node(src_id, &target_parent, Some(&target_id))
    } else {
        // Insert AFTER the target = before its next sibling (or at the end).
        let siblings: Vec<String> = fav
            .nodes
            .iter()
            .filter(|n| n.parent == target_parent)
            .map(|n| n.id.clone())
            .collect();
        let before = siblings
            .iter()
            .position(|id| *id == target_id)
            .and_then(|p| siblings.get(p + 1))
            .cloned();
        fav.move_node(src_id, &target_parent, before.as_deref())
    }
}
