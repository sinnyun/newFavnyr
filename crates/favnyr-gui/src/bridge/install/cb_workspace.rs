use super::*;

pub(super) fn install_ws_name_check(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_ws_name_check(move |name: SharedString| {
        if let Some(w) = weak.upgrade() {
            w.set_ws_name_taken(workspace_name_exists(name.as_str()));
        }
    });
}

pub(super) fn install_reopen_last_closed_tab(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_reopen_last_closed_tab(move |panel: i32| {
        let Some(w) = weak.upgrade() else { return };
        let Some(saved) = st.pop_closed_tab() else {
            w.set_closed_tabs_available(false);
            return;
        };
        let restored = tab_from_state(saved);
        let target = restored.current_path.clone();
        let panel = {
            let mut panels = st.panels.borrow_mut();
            let panel = (panel.max(0) as usize).min(panels.len().saturating_sub(1));
            let tabs = &mut panels[panel].tabs;
            tabs.tabs.push(restored);
            tabs.active = tabs.tabs.len() - 1;
            panel
        };
        *st.active_panel.borrow_mut() = panel;
        w.set_closed_tabs_available(st.has_closed_tabs());
        load_directory(&w, &st, &target, false);
        st.persist_workspace();
    });
}

pub(super) fn install_ws_save_new(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_save_new(move |name: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let name = name.trim().to_string();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            if name.is_empty() {
                return;
            }
            // Final safeguard against a programmatic call or a concurrent
            // creation after the live validation.
            if workspace_name_exists(&name) {
                w.set_ws_name_taken(true);
                return;
            }
            let ws = st.capture_workspace();
            match workspace::save_named_workspace(&paths::workspaces_dir(), &name, &ws) {
                Ok(id) => {
                    info!(id, name, "workspace saved");
                    // The saved workspace becomes the current reference.
                    *st.current_workspace.borrow_mut() = Some(name.clone());
                    st.remember_workspace_saved();
                    st.persist_workspace();
                    update_window_title(&w, &st);
                    w.set_ws_toast(w.get_strings().ws_toast_saved);
                    w.set_ws_name_taken(false);
                }
                Err(err) => error!(error = %err, "save named workspace failed"),
            }
            refresh_workspaces_ui(&w, &st);
        });
    });
}

pub(super) fn install_ws_load(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_load(move |id: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let id = id.to_string();
        defer(move || {
            if let Some(w) = weak.upgrade() {
                load_named_into(&w, &st, &id);
            }
        });
    });
}

pub(super) fn install_ws_load_guarded(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_load_guarded(move |target_id: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let target_id = target_id.to_string();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            // Safeguard disabled (settings) → direct load, as if
            // the current workspace were clean.
            let dirty = if w.get_ws_warn_unsaved() {
                dirty_current_workspace(&st)
            } else {
                None
            };
            match dirty {
                Some((_cur_id, cur_name)) => {
                    let target_name = workspace::list_named_workspaces(&paths::workspaces_dir())
                        .into_iter()
                        .find(|m| m.id == target_id)
                        .map(|m| m.name)
                        .unwrap_or_default();
                    let s = w.get_strings();
                    w.set_ws_dirty_title_text(s.ws_dirty_title.replace("{name}", &cur_name).into());
                    w.set_ws_dirty_body_text(
                        s.ws_dirty_body
                            .replace("{name}", &cur_name)
                            .replace("{target}", &target_name)
                            .into(),
                    );
                    w.set_ws_dirty_target_id(target_id.into());
                    w.set_ws_dirty_reset(false); // pending action = load `target_id`
                    w.set_ws_dirty_warn_open(true);
                }
                None => {
                    load_named_into(&w, &st, &target_id);
                    w.set_workspaces_open(false);
                }
            }
        });
    });
}

pub(super) fn install_ws_save_and_load(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_save_and_load(move || {
        let st = st.clone();
        let weak = weak.clone();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            if let Some((cur_id, _)) = dirty_current_workspace(&st) {
                match overwrite_named_from_live(&st, &cur_id) {
                    Ok(()) => info!(id = %cur_id, "workspace saved before switching"),
                    Err(err) => error!(error = %err, "save-and-switch: overwrite failed"),
                }
            }
            if w.get_ws_dirty_reset() {
                reset_into(&w, &st);
            } else {
                let target_id = w.get_ws_dirty_target_id();
                load_named_into(&w, &st, target_id.as_ref());
            }
            w.set_ws_dirty_warn_open(false);
            w.set_workspaces_open(false);
        });
    });
}

pub(super) fn install_ws_overwrite(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_overwrite(move |id: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let id = id.to_string();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            match overwrite_named_from_live(&st, &id) {
                Ok(()) => {
                    info!(id = %id, "workspace overwritten");
                    update_window_title(&w, &st);
                    w.set_ws_toast(w.get_strings().ws_toast_updated);
                    refresh_workspaces_ui(&w, &st); // counters up to date
                }
                Err(err) => error!(error = %err, id = %id, "overwrite workspace failed"),
            }
        });
    });
}

pub(super) fn install_ws_save_current(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_save_current(move || {
        let st = st.clone();
        let weak = weak.clone();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(name) = st.current_workspace.borrow().clone() else {
                // Workspace still unnamed: we open the panel AND arm the
                // "name" field's focus — the user wanted to SAVE,
                // so the cursor awaits them directly in the field.
                w.set_workspaces_open(true);
                w.set_ws_name_focus_armed(true);
                return;
            };
            let Some(id) = current_workspace_id(&st) else {
                let msg = w
                    .get_strings()
                    .ws_notice_save_failed
                    .replace("{name}", &name);
                notice(&w, msg, NoticeKind::Error);
                w.set_workspaces_open(true);
                return;
            };
            match overwrite_named_from_live(&st, &id) {
                Ok(()) => {
                    info!(id = %id, name, "workspace saved from shortcut");
                    update_window_title(&w, &st);
                    let msg = w.get_strings().ws_notice_saved.replace("{name}", &name);
                    notice_for(&w, msg, NoticeKind::Success, 2_000);
                }
                Err(err) => {
                    error!(error = %err, id = %id, "workspace shortcut save failed");
                    let msg = w
                        .get_strings()
                        .ws_notice_save_failed
                        .replace("{name}", &name);
                    notice(&w, msg, NoticeKind::Error);
                }
            }
        });
    });
}

pub(super) fn install_ws_delete(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_delete(move |id: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let id = id.to_string();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            let deleting_current = current_workspace_id(&st).as_deref() == Some(id.as_str());
            match workspace::delete_named_workspace(&paths::workspaces_dir(), &id) {
                Ok(()) => {
                    info!(id = %id, "workspace deleted");
                    if deleting_current {
                        *st.current_workspace.borrow_mut() = None;
                        *st.saved_workspace.borrow_mut() = None;
                        st.persist_workspace();
                        update_window_title(&w, &st);
                    }
                    w.set_ws_toast(w.get_strings().ws_toast_deleted);
                }
                Err(err) => error!(error = %err, id = %id, "delete workspace failed"),
            }
            refresh_workspaces_ui(&w, &st);
        });
    });
}

pub(super) fn install_ws_rename(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_rename(move |id: SharedString, new_name: SharedString| {
        let st = st.clone();
        let weak = weak.clone();
        let id = id.to_string();
        let new_name = new_name.trim().to_string();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            if new_name.is_empty() {
                return;
            }
            let renaming_current = current_workspace_id(&st).as_deref() == Some(id.as_str());
            match workspace::rename_named_workspace(&paths::workspaces_dir(), &id, &new_name) {
                Ok(()) => {
                    info!(id = %id, name = new_name, "workspace renamed");
                    if renaming_current {
                        *st.current_workspace.borrow_mut() = Some(new_name.clone());
                        st.persist_workspace();
                        update_window_title(&w, &st);
                    }
                    w.set_ws_toast(w.get_strings().ws_toast_renamed);
                }
                Err(err) => error!(error = %err, id = %id, "rename workspace failed"),
            }
            refresh_workspaces_ui(&w, &st);
        });
    });
}

pub(super) fn install_ws_reset(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_reset(move || {
        let st = st.clone();
        let weak = weak.clone();
        defer(move || {
            if let Some(w) = weak.upgrade() {
                reset_into(&w, &st);
            }
        });
    });
}

pub(super) fn install_ws_reset_guarded(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_reset_guarded(move || {
        let st = st.clone();
        let weak = weak.clone();
        defer(move || {
            let Some(w) = weak.upgrade() else { return };
            let dirty = if w.get_ws_warn_unsaved() {
                dirty_current_workspace(&st)
            } else {
                None
            };
            match dirty {
                Some((_cur_id, cur_name)) => {
                    let s = w.get_strings();
                    let target_name = s.ws_new_name;
                    w.set_ws_dirty_title_text(s.ws_dirty_title.replace("{name}", &cur_name).into());
                    w.set_ws_dirty_body_text(
                        s.ws_dirty_body
                            .replace("{name}", &cur_name)
                            .replace("{target}", &target_name)
                            .into(),
                    );
                    w.set_ws_dirty_reset(true); // the pending action is a reset
                    w.set_ws_dirty_warn_open(true);
                }
                None => {
                    reset_into(&w, &st);
                    w.set_workspaces_open(false);
                }
            }
        });
    });
}

pub(super) fn install_ws_toggle_sort(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_toggle_sort(move || {
        let Some(w) = weak.upgrade() else { return };
        w.set_ws_sort_newest_first(!w.get_ws_sort_newest_first());
        refresh_workspaces_ui(&w, &st);
    });
}

pub(super) fn install_ws_refresh(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_ws_refresh(move || {
        let st = st.clone();
        let weak = weak.clone();
        defer(move || {
            if let Some(w) = weak.upgrade() {
                refresh_workspaces_ui(&w, &st);
            }
        });
    });
}
