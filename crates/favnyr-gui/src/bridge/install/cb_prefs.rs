use super::*;

pub(super) fn install_rmtime_depth_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_rmtime_depth_changed(move |v: i32| {
            let Some(w) = weak.upgrade() else { return };
            let v = v.clamp(0, 8);
            st.persist_config(|c| c.recursive_mtime_depth = v);
            w.global::<crate::SettingsApi>().set_rmtime_depth(v);
            // Depth changed → the mtime values are stale. Re-list (restoring the
            // folders' OWN mtime), then `request_folder_stats` re-applies them if
            // depth > 0.
            st.rmtime_cache.borrow_mut().clear();
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_size_depth_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_size_depth_changed(move |v: i32| {
            let Some(w) = weak.upgrade() else { return };
            let v = v.clamp(0, 8);
            st.persist_config(|c| c.recursive_size_depth = v);
            w.global::<crate::SettingsApi>().set_size_depth(v);
            // Depth changed → the recursive sizes are stale; re-list, then
            // `request_folder_stats` recomputes them if depth > 0.
            st.size_cache.borrow_mut().clear();
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_default_column_toggle(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_default_column_toggle(move |id: SharedString, visible: bool| {
            let id = id.to_string();
            if id == "name" {
                return; // anchor can't be unchecked
            }
            st.persist_config(|c| {
                c.default_columns = columns::sanitize(std::mem::take(&mut c.default_columns));
                if let Some(col) = c.default_columns.iter_mut().find(|x| x.id == id) {
                    col.visible = visible;
                }
            });
            let (lang, cols) = {
                let c = st.config.borrow();
                (c.language, c.default_columns.clone())
            };
            if let Some(w) = weak.upgrade() {
                push_settings_columns(&w, lang, &cols);
            }
        });
}

pub(super) fn install_shortcut_search(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shortcut_search(move |text: SharedString| {
            *st.shortcut_filter.borrow_mut() = text.to_string();
            if let Some(w) = weak.upgrade() {
                push_shortcuts_ui(&w, &st);
            }
        });
}

pub(super) fn install_shortcut_rebind(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SettingsApi>().on_shortcut_rebind(
        move |action_id: SharedString, chord_raw: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            w.global::<crate::SettingsApi>()
                .set_shortcut_capturing(SharedString::new());
            let action_id = action_id.to_string();
            // Normalizes the entered combination; ignores it if there's no real key.
            let Some(chord) = Chord::parse(chord_raw.as_str()).map(|c| c.serialize()) else {
                return;
            };
            // Conflict with another action?
            let other = st
                .keymap
                .borrow()
                .conflict(&action_id, &chord)
                .map(|s| s.to_string());
            if let Some(other_id) = other {
                let lang = st.config.borrow().language;
                let other_name = i18n::shortcut_action_name(lang, &other_id);
                let msg = i18n::shortcut_conflict_message(lang, &other_name);
                *st.pending_conflict.borrow_mut() = Some((action_id.clone(), other_id, chord));
                w.global::<crate::SettingsApi>()
                    .set_shortcut_conflict_action(action_id.into());
                w.global::<crate::SettingsApi>()
                    .set_shortcut_conflict_message(msg.into());
                return;
            }
            apply_shortcut_override(&st, &action_id, &chord);
            push_shortcuts_ui(&w, &st);
        },
    );
}

pub(super) fn install_shortcut_resolve_conflict(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shortcut_resolve_conflict(move |reassign: bool| {
            let Some(w) = weak.upgrade() else { return };
            if let Some((action_id, other_id, chord)) = st.pending_conflict.borrow_mut().take()
                && reassign
            {
                apply_shortcut_override(&st, &action_id, &chord);
                // Frees the other action (marks it "unassigned").
                st.persist_config(|c| {
                    c.shortcut_overrides.insert(other_id.clone(), String::new());
                });
                st.rebuild_keymap();
            }
            w.global::<crate::SettingsApi>()
                .set_shortcut_conflict_action(SharedString::new());
            w.global::<crate::SettingsApi>()
                .set_shortcut_conflict_message(SharedString::new());
            push_shortcuts_ui(&w, &st);
        });
}

pub(super) fn install_shortcut_reset(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shortcut_reset(move |action_id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let id = action_id.to_string();
            // Resetting = (re)assigning the action ITS factory default — via the
            // SAME assignment path as capture (single uniqueness rule:
            // no path can create a duplicate). If this default is already held
            // by ANOTHER action, we open the same conflict banner
            // ("Reassign here" / "Keep") instead of silently restoring it
            // (which would produce two actions sharing the same combination).
            let default = shortcuts::ACTIONS
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.default)
                .unwrap_or("");
            if !default.is_empty() {
                let other = st
                    .keymap
                    .borrow()
                    .conflict(&id, default)
                    .map(|s| s.to_string());
                if let Some(other_id) = other {
                    let lang = st.config.borrow().language;
                    let other_name = i18n::shortcut_action_name(lang, &other_id);
                    let msg = i18n::shortcut_conflict_message(lang, &other_name);
                    *st.pending_conflict.borrow_mut() =
                        Some((id.clone(), other_id, default.to_string()));
                    w.global::<crate::SettingsApi>()
                        .set_shortcut_conflict_action(id.into());
                    w.global::<crate::SettingsApi>()
                        .set_shortcut_conflict_message(msg.into());
                    return;
                }
            }
            // Free default (or "unassigned") → apply it (removes the override).
            apply_shortcut_override(&st, &id, default);
            push_shortcuts_ui(&w, &st);
        });
}

pub(super) fn install_shortcut_unassign(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shortcut_unassign(move |action_id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            // Unassigning = setting an EMPTY override (no conflict possible). If
            // the action has a factory default, `overridden` becomes true → the
            // "restore default" button (circular arrow) shows up in the list.
            apply_shortcut_override(&st, action_id.as_ref(), "");
            push_shortcuts_ui(&w, &st);
        });
}

pub(super) fn install_shortcut_reset_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shortcut_reset_all(move || {
            let Some(w) = weak.upgrade() else { return };
            st.persist_config(|c| c.shortcut_overrides.clear());
            st.rebuild_keymap();
            w.global::<crate::SettingsApi>()
                .set_shortcut_conflict_action(SharedString::new());
            push_shortcuts_ui(&w, &st);
        });
}

pub(super) fn install_empty_trash(window: &MainWindow) {
    window
        .global::<crate::SidebarApi>()
        .on_empty_trash(move || {
            std::thread::spawn(|| match favnyr_core::fs::ops::empty_trash() {
                Ok(()) => info!("trash emptied"),
                Err(err) => error!(error = %err, "empty_trash failed"),
            });
        });
}

pub(super) fn install_language_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::ApplicationApi>()
        .on_language_changed(move |idx: i32| {
            let Some(w) = weak.upgrade() else { return };
            let lang = Lang::all().get(idx as usize).copied().unwrap_or_default();
            info!(lang = lang.code(), "language changed");
            // Commit the language to config BEFORE any refresh: `update_panels_ui`
            // (via `refresh_listing`) reads the language back from CONFIG for the
            // VIEW texts (footer, columns, title). Otherwise they stayed one
            // language behind ("you have to change it twice").
            st.persist_config(|c| c.language = lang);
            apply_language(&w, lang);
            // Footer, sizes, ages and column labels all depend on the
            // language, and they are baked into every view's rows — not just
            // the active one. Refreshing a single panel left the others
            // reading in the previous language until they happened to be
            // re-listed. The image depth is cached as ALREADY FORMATTED text,
            // so that cache is dropped first; the re-listing below refills it.
            st.imgmeta_cache.borrow_mut().clear();
            refresh_all_panels(&w, &st);
            push_settings_columns(&w, lang, &st.config.borrow().default_columns);
            push_shortcuts_ui(&w, &st); // localized shortcut labels
            push_recipes_ui(&w, &st); // localized recipe labels
            // Surfaces that build localized text OUTSIDE the `Strings` struct,
            // so `apply_language` above does not reach them. Anything added
            // later that formats with `lang` or reads `strings_for` belongs in
            // this list too — the drive gauge was missing from it and stayed in
            // the previous language until the sidebar was toggled by hand.
            refresh_sidebar(&w, &st); // capacity gauges + their hover hint
            push_favorites_ui(&w, &st); // "Favorites (root)" in the container list
        });
}

pub(super) fn install_theme_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::ApplicationApi>()
        .on_theme_changed(move |idx: i32| {
            let theme = Theme::all().get(idx as usize).copied().unwrap_or_default();
            info!(theme = theme.code(), "theme changed");
            st.persist_config(|c| c.theme = theme);
        });
}

pub(super) fn install_ui_scale_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::ApplicationApi>()
        .on_ui_scale_changed(move |idx: i32| {
            let Some(w) = weak.upgrade() else { return };
            let factor = UI_SCALE_PRESETS
                .get(idx.max(0) as usize)
                .copied()
                .unwrap_or(1.0);
            info!(factor, "UI scale changed");
            st.persist_config(|c| c.ui_scale = factor);
            apply_ui_scale(&w, &st, factor);
        });
}

pub(super) fn install_ui_scale_deferred(window: &MainWindow, state: AppState) {
    let ui_scale = state.config.borrow().ui_scale;
    let api = window.global::<crate::ApplicationApi>();
    api.set_ui_scale_labels(ModelRc::new(VecModel::from(ui_scale_labels())));
    api.set_ui_scale_index(ui_scale_nearest_index(ui_scale));
    let st = state.clone();
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(w) = weak.upgrade() {
            apply_ui_scale(&w, &st, ui_scale);
        }
    });
}

pub(super) fn install_count_annotation_orphans(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_count_annotation_orphans(move || {
            let Some(w) = weak.upgrade() else { return };
            *st.orphans.borrow_mut() = annotations_now(&st).orphans();
            push_orphan_count(&w, &st);
        });
}

pub(super) fn install_clean_annotations(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_clean_annotations(move || {
            let Some(w) = weak.upgrade() else { return };
            // Everything arrives ticked: getting here already took opening the
            // settings and asking for a cleanup. What was missing was seeing
            // what goes — not one more step to click through.
            *st.orphan_selection.borrow_mut() = st
                .orphans
                .borrow()
                .iter()
                .map(|orphan| orphan.path.clone())
                .collect();
            push_orphan_rows(&w, &st);
            w.global::<crate::SettingsApi>().set_orphans_open(true);
        });
}

pub(super) fn install_orphan_toggled(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_orphan_toggled(move |key: SharedString, on: bool| {
            let Some(w) = weak.upgrade() else { return };
            {
                let mut chosen = st.orphan_selection.borrow_mut();
                if on {
                    chosen.insert(key.to_string());
                } else {
                    chosen.remove(key.as_str());
                }
            }
            push_orphan_rows(&w, &st);
        });
}

pub(super) fn install_orphans_select_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_orphans_select_all(move |on: bool| {
            let Some(w) = weak.upgrade() else { return };
            {
                let mut chosen = st.orphan_selection.borrow_mut();
                chosen.clear();
                if on {
                    chosen.extend(st.orphans.borrow().iter().map(|o| o.path.clone()));
                }
            }
            push_orphan_rows(&w, &st);
        });
}

pub(super) fn install_orphans_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_orphans_confirmed(move || {
            let Some(w) = weak.upgrade() else { return };
            let chosen: Vec<String> = st.orphan_selection.borrow().iter().cloned().collect();
            let removed = {
                let mut annotations = annotations_for_update(&st);
                let removed = annotations.remove_selected(&chosen);
                if removed > 0 {
                    save_annotations(&st, &annotations);
                }
                removed
            };
            st.orphan_selection.borrow_mut().clear();
            // Recounted from the store rather than assumed to be zero: a
            // partial answer leaves the rest, and the badge must say so.
            *st.orphans.borrow_mut() = annotations_now(&st).orphans();
            push_orphan_count(&w, &st);
            let lang = st.snapshot_config().language;
            show_notice_ok(&w, annotations_cleaned_text(lang, removed));
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_ffmpeg_recheck(window: &MainWindow) {
    let weak = window.as_weak();
    window
        .global::<crate::ApplicationApi>()
        .on_ffmpeg_recheck(move || {
            if let Some(w) = weak.upgrade() {
                apply_ffmpeg_info(&w);
            }
        });
}

pub(super) fn install_settings_copy(window: &MainWindow) {
    window
        .global::<crate::ApplicationApi>()
        .on_settings_copy(move |text: SharedString| {
            if let Err(err) = actions::copy_to_clipboard(&text) {
                error!(error = %err, "clipboard copy (setting) failed");
            }
        });
}

pub(super) fn install_tabbar_default_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::ApplicationApi>()
        .on_tabbar_default_changed(move |idx: i32| {
            let mode = idx.clamp(0, 2) as u8;
            info!(mode, "default tab bar position changed");
            st.persist_config(|c| c.default_tab_bar_mode = mode);
        });
}

pub(super) fn install_tab_tooltip_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::ApplicationApi>()
        .on_tab_tooltip_changed(move |on: bool| {
            info!(on, "tab path tooltip toggled");
            st.persist_config(|c| c.tab_path_tooltip = on);
        });
}

pub(super) fn install_ws_warn_unsaved_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::ApplicationApi>()
        .on_ws_warn_unsaved_changed(move |on: bool| {
            info!(on, "unsaved-workspace warning toggled");
            st.persist_config(|c| c.warn_unsaved_workspace = on);
        });
}

pub(super) fn install_compact_preview_rows_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::ApplicationApi>()
        .on_compact_preview_rows_changed(move |on: bool| {
            let Some(w) = weak.upgrade() else { return };
            info!(on, "compact icon rows in preview toggled");
            st.persist_config(|c| c.compact_icon_rows_in_preview = on);
            refresh_preview_panel_visuals(&st);
            update_panels_ui(&w, &st);
            request_thumbnails(&st);
        });
}

pub(super) fn install_clock_mode_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::ApplicationApi>()
        .on_clock_mode_changed(move |idx: i32| {
            let Some(w) = weak.upgrade() else { return };
            info!(idx, "modified-column time zone changed");
            st.persist_config(|c| c.clock_utc = idx == 1);
            refresh_mtime_offset(&st);
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_shell_menu_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::SettingsApi>()
        .on_shell_menu_changed(move |on: bool| {
            info!(on, "windows shell context menu toggled");
            st.persist_config(|c| c.shell_ctx_menu = on);
        });
}
