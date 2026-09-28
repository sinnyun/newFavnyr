use super::*;

pub(super) fn install_fav_row_activate(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_row_activate(move |id: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let id = id.to_string();
        let info = st
            .favorites
            .borrow()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| (n.is_container(), n.expanded));
        match info {
            Some((true, expanded)) => {
                favorites_for_update(&st).set_expanded(&id, !expanded);
                save_favorites(&st);
                push_favorites_ui(&w, &st);
            }
            // LEFT click on a favorite (leaf) → navigates the ACTIVE tab
            // (overwrites it). MIDDLE click (on_fav_row_middle) keeps the new
            // tab.
            Some((false, _)) => fav_open_here(&w, &st, &id),
            None => {}
        }
    });
}

pub(super) fn install_sidebar_item_dropped(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_sidebar_item_dropped(
        move |source: SharedString, kind: i32, panel: i32, gap: i32, zone: i32| {
            let Some(w) = weak.upgrade() else { return };
            let panel_drop = gap < 0 && zone > 0;
            let target = if kind == -1 {
                let stored = {
                    let favorites = favorites_now(&st);
                    favorites.path_of(source.as_str()).map(PathBuf::from)
                };
                if panel_drop {
                    stored.filter(|path| path.is_dir())
                } else {
                    stored
                        .and_then(|path| resolve_fav_dir(&w, &path, st.snapshot_config().language))
                }
            } else if matches!(kind, 0 | 1 | 2 | 4) {
                resolve_sidebar_place_dir(source.as_str())
            } else {
                None
            };
            let Some(target) = target else { return };

            if gap >= 0 || zone == 1 {
                let insert_at = if gap >= 0 { gap } else { i32::MAX };
                open_path_in_tab_at(&w, &st, target, panel, insert_at);
            } else if panel >= 0 && (2..=5).contains(&zone) {
                let dir = if zone == 4 || zone == 5 {
                    SplitDir::Column
                } else {
                    SplitDir::Row
                };
                let new_first = zone == 2 || zone == 4;
                if split_with_path(&st, target, panel as usize, dir, new_first) {
                    refresh_all_panels(&w, &st);
                }
            }
        },
    );
}

pub(super) fn install_fav_row_middle(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_row_middle(move |id: SharedString| {
        if let Some(w) = weak.upgrade() {
            fav_open(&w, &st, &id);
        }
    });
}

pub(super) fn install_fav_open(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_open(move |id: SharedString| {
        if let Some(w) = weak.upgrade() {
            fav_open(&w, &st, &id);
        }
    });
}

pub(super) fn install_fav_open_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_open_all(move |id: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let paths = favorites_now(&st).descendant_paths(&id);
        for p in paths {
            fav_open_path_new_tab(&w, &st, PathBuf::from(p));
        }
    });
}

pub(super) fn install_fav_name_check(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_fav_name_check(move |name: SharedString| {
        if let Some(w) = weak.upgrade() {
            w.set_fav_name_invalid(!ops::is_valid_entry_name(name.trim()));
        }
    });
}

pub(super) fn install_fav_name_confirm(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_name_confirm(move || {
        let Some(w) = weak.upgrade() else { return };
        let raw = w.get_fav_name_value();
        let name = raw.trim();
        // Final guard (the button is already disabled on an invalid name).
        if !ops::is_valid_entry_name(name) {
            return;
        }
        let target = w.get_fav_name_target().to_string();
        if w.get_fav_name_rename() {
            favorites_for_update(&st).rename(&target, name);
        } else {
            favorites_for_update(&st).add_container(&target, name);
        }
        save_favorites(&st);
        push_favorites_ui(&w, &st);
        w.set_fav_name_open(false);
    });
}

pub(super) fn install_fav_collapse_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_collapse_all(move || {
        let Some(w) = weak.upgrade() else { return };
        favorites_for_update(&st).collapse_all();
        save_favorites(&st);
        push_favorites_ui(&w, &st);
    });
}

pub(super) fn install_fav_expand_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_expand_all(move || {
        let Some(w) = weak.upgrade() else { return };
        favorites_for_update(&st).expand_all();
        save_favorites(&st);
        push_favorites_ui(&w, &st);
    });
}

pub(super) fn install_fav_delete(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_delete(move |id: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        favorites_for_update(&st).delete(&id);
        save_favorites(&st);
        if w.get_fav_selected_id() == id {
            w.set_fav_selected_id(SharedString::new());
        }
        push_favorites_ui(&w, &st);
    });
}

pub(super) fn install_fav_copy_path(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_fav_copy_path(move |id: SharedString| {
        if let Some(p) = favorites_now(&st).path_of(&id)
            && let Err(err) = actions::copy_to_clipboard(&p)
        {
            error!(error = %err, "fav copy path failed");
        }
    });
}

pub(super) fn install_fav_save_tab(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_save_tab(move |panel: i32, tab: i32| {
        let Some(w) = weak.upgrade() else { return };
        let path = {
            let panels = st.panels.borrow();
            panels.get(panel as usize).and_then(|p| {
                p.tabs
                    .tabs
                    .get(tab as usize)
                    .map(|t| t.current_path.clone())
            })
        };
        if let Some(path) = path {
            open_fav_save_popup(&w, &st, vec![path]);
        }
    });
}

pub(super) fn install_fav_save_all_tabs(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_save_all_tabs(move |panel: i32| {
        let Some(w) = weak.upgrade() else { return };
        let paths: Vec<PathBuf> = {
            let panels = st.panels.borrow();
            panels
                .get(panel as usize)
                .map(|p| p.tabs.tabs.iter().map(|t| t.current_path.clone()).collect())
                .unwrap_or_default()
        };
        open_fav_save_popup(&w, &st, paths);
    });
}

pub(super) fn install_tab_to_favorite(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_tab_to_favorite(move |panel: i32, tab: i32, container: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let path = {
            let panels = st.panels.borrow();
            panels.get(panel.max(0) as usize).and_then(|p| {
                p.tabs
                    .tabs
                    .get(tab.max(0) as usize)
                    .map(|t| t.current_path.clone())
            })
        };
        let Some(path) = path else { return };
        add_paths_to_favorite(&w, &st, std::slice::from_ref(&path), &container);
    });
}

pub(super) fn install_file_to_favorite(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_file_to_favorite(move |panel: i32, container: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let paths = panel_selected_paths(&st, panel.max(0) as usize);
        add_paths_to_favorite(&w, &st, &paths, &container);
    });
}

pub(super) fn install_fav_add_selection(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_add_selection(move || {
        if let Some(w) = weak.upgrade() {
            open_fav_save_popup(&w, &st, selected_paths(&st));
        }
    });
}

pub(super) fn install_fav_save_new_container(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_save_new_container(move |name: SharedString, parent_index: i32| {
        let Some(w) = weak.upgrade() else { return };
        // Parent container chosen in the sub-popup (index 0 = root).
        let parent = st
            .fav_container_ids
            .borrow()
            .get(parent_index.max(0) as usize)
            .cloned()
            .unwrap_or_default();
        let id = favorites_for_update(&st).add_container(&parent, &name);
        save_favorites(&st);
        push_favorites_ui(&w, &st);
        // Selects the freshly created container as the destination.
        if let Some(id) = id {
            let idx = st
                .fav_container_ids
                .borrow()
                .iter()
                .position(|c| *c == id)
                .unwrap_or(0) as i32;
            w.set_fav_container_index(idx);
        }
    });
}

pub(super) fn install_fav_save_commit(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_save_commit(move |alias: SharedString, index: i32| {
        let Some(w) = weak.upgrade() else { return };
        let container = st
            .fav_container_ids
            .borrow()
            .get(index.max(0) as usize)
            .cloned()
            .unwrap_or_default();
        let paths = std::mem::take(&mut *st.fav_save_pending.borrow_mut());
        let multi = paths.len() > 1;
        // Deduplication: we do NOT add a path already present in the
        // target container (otherwise a silent duplicate). We count the additions
        // to choose the toast (added vs already present).
        let mut added = 0usize;
        {
            let mut fav = favorites_for_update(&st);
            for p in &paths {
                let path_str = p.display().to_string();
                if fav.container_has_path(&container, &path_str) {
                    continue; // already in this favorites folder → ignored
                }
                let alias = if !multi {
                    let a = alias.to_string();
                    if a.trim().is_empty() {
                        default_alias(p)
                    } else {
                        a
                    }
                } else {
                    default_alias(p)
                };
                fav.add_favorite(&container, &alias, &path_str);
                added += 1;
            }
        }
        let lang = st.config.borrow().language;
        if added > 0 {
            save_favorites(&st);
            push_favorites_ui(&w, &st);
            notice(
                &w,
                i18n::strings_for(lang).fav_toast_added.clone(),
                NoticeKind::FavAdded,
            );
        } else {
            // Nothing added = everything already existed → neutral info (bookmark accent),
            // not an error.
            notice(
                &w,
                i18n::strings_for(lang).fav_toast_exists.clone(),
                NoticeKind::FavExists,
            );
        }
    });
}

pub(super) fn install_fav_drag_move(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_drag_move(move |row_idx: i32, cur_y: f32, row_top: f32| {
        let Some(w) = weak.upgrade() else { return };
        let flat = favorites_now(&st).flatten();
        let count = flat.len() as i32;
        let (ti, zone) = fav_drag_target(cur_y, row_top, row_idx, count);
        w.set_fav_drag_active(true);
        w.set_fav_drag_target_index(ti);
        w.set_fav_drag_zone(zone);
        // Collapsed (non-empty) container targeted "inside" → auto-expand candidate.
        let hover = flat
            .get(ti.max(0) as usize)
            .filter(|n| n.is_container && n.has_children && !n.expanded && zone == 1)
            .map(|n| n.id.clone())
            .unwrap_or_default();
        w.set_fav_drag_hover_container(hover.into());
    });
}

pub(super) fn install_fav_drag_drop(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_drag_drop(
        move |row_idx: i32, cur_y: f32, row_top: f32, commit_reorder: bool| {
            let Some(w) = weak.upgrade() else { return };
            let (src_id, count) = {
                let fav = favorites_now(&st);
                let flat = fav.flatten();
                (
                    flat.get(row_idx.max(0) as usize).map(|n| n.id.clone()),
                    flat.len() as i32,
                )
            };
            w.set_fav_drag_active(false);
            w.set_fav_drag_target_index(-1);
            w.set_fav_drag_zone(0);
            w.set_fav_drag_hover_container(SharedString::new());
            if let (Some(src_id), Some((ti, zone))) = (
                src_id,
                fav_drop_target(commit_reorder, cur_y, row_top, row_idx, count),
            ) && fav_perform_move(&st, &src_id, ti, zone)
            {
                save_favorites(&st);
                push_favorites_ui(&w, &st);
            }
        },
    );
}

pub(super) fn install_fav_expand(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_fav_expand(move |id: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let id = id.to_string();
        let model = w.get_fav_nodes();
        // Graceful no-op if the model isn't a VecModel (the drag stays sane).
        let Some(vm) = model.as_any().downcast_ref::<VecModel<FavNode>>() else {
            return;
        };
        // Locates the container's row in the visible list.
        let mut ti = None;
        for i in 0..vm.row_count() {
            if vm.row_data(i).map(|r| r.id == id).unwrap_or(false) {
                ti = Some(i);
                break;
            }
        }
        let Some(ti) = ti else { return };
        let Some(mut crow) = vm.row_data(ti) else {
            return;
        };
        if !crow.is_container || crow.expanded {
            return;
        }
        let base_depth = crow.depth + 1;
        let children = {
            let mut fav = favorites_for_update(&st);
            fav.set_expanded(&id, true);
            fav.flatten_children(&id, base_depth)
        };
        save_favorites(&st);
        crow.expanded = true;
        vm.set_row_data(ti, crow);
        for (k, fc) in children.iter().enumerate() {
            vm.insert(ti + 1 + k, flat_to_favnode(fc));
        }
        w.set_fav_drag_hover_container(SharedString::new());
    });
}
