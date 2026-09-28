use super::*;

pub(super) fn install_op_cancel(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::OperationsApi>()
        .on_op_cancel(move |op_id: i32| {
            st.ops.cancel(op_id);
        });
}

pub(super) fn install_op_progress(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_op_progress(move |row: OpProgress| {
            let Some(w) = weak.upgrade() else { return };
            upsert_op_row(&st, row);
            refresh_ops_ui(&w, &st);
        });
}

pub(super) fn install_op_dismiss(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_op_dismiss(move |op_id: i32| {
            let Some(w) = weak.upgrade() else { return };
            remove_op_row(&st, op_id);
            refresh_ops_ui(&w, &st);
        });
}

pub(super) fn install_op_finished(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_op_finished(move |op_id: i32| {
            let Some(handle) = st.ops.finish(op_id) else {
                return; // unknown id: already released
            };
            let Some(w) = weak.upgrade() else { return };
            sync_op_busy(&w, &st);
            // The item to highlight is handed to the upcoming re-listing: the row
            // only exists once the views have been re-read. Operations complete
            // independently, so the last one to finish is the one highlighted.
            if let Some(target) = handle.pending_focus {
                *st.focus_after_refresh.borrow_mut() = Some(target);
            }
            invalidate_thumbnail_paths(&st, &handle.thumbnail_invalidations);
            // The worker no longer needs its sources. Remove any virtual-file
            // staging tree before views are listed again.
            drop(handle.transient_cleanup);
            request_op_refresh(&w);
        });
}

pub(super) fn install_action_duplicate(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_duplicate(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            if paths.is_empty() {
                return;
            }
            let pairs: Vec<(PathBuf, PathBuf, bool)> = paths
                .iter()
                .cloned()
                .zip(plan_unique_targets(&paths, |p| target_taken(&st, p)))
                .map(|(src, dst)| (src, dst, false))
                .collect();
            let lang = st.snapshot_config().language;
            start_heavy_op(&w, &st, Heavy::Copy(pairs), lang, None, None);
        });
}

pub(super) fn install_action_rename(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_rename(move || {
            let Some(w) = weak.upgrade() else { return };
            let rows = st.active_rows_model();
            let target = (0..rows.row_count())
                .find(|&i| rows.row_data(i).map(|r| r.selected).unwrap_or(false));
            let Some(idx) = target else { return };
            let Some(row) = rows.row_data(idx) else {
                return;
            };
            // A section header is not an entry: nothing to rename.
            let Some(source) = row_path(&row) else { return };
            *st.rename_source.borrow_mut() = Some(source);
            w.global::<crate::OperationsApi>()
                .set_rename_current_name(row.name.clone());
            w.global::<crate::OperationsApi>()
                .set_rename_current_is_dir(row.is_dir);
            w.global::<crate::OperationsApi>()
                .set_rename_conflict(false); // current name == itself → no conflict
            w.global::<crate::OperationsApi>()
                .set_rename_replace_available(false);
            w.global::<crate::OperationsApi>()
                .set_rename_name_error(SharedString::new());
            w.global::<crate::OperationsApi>()
                .set_rename_new_name(row.name);
            w.global::<crate::OperationsApi>()
                .set_rename_popup_open(true);
        });
}

pub(super) fn install_rename_check(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_rename_check(move |name: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let status = st
                .rename_source
                .borrow()
                .as_deref()
                .map(|source| rename_name_status_now(&st, source, name.as_ref()))
                .unwrap_or(RenameNameStatus::Invalid);
            w.global::<crate::OperationsApi>()
                .set_rename_conflict(status != RenameNameStatus::Valid);
            w.global::<crate::OperationsApi>()
                .set_rename_replace_available(status == RenameNameStatus::ReplaceableFile);
            // Says WHICH problem blocks the rename: a name the filesystem
            // refuses is not a name somebody else already holds.
            let lang = st.snapshot_config().language;
            let availability = match status {
                RenameNameStatus::Valid => EntryNameAvailability::Available,
                RenameNameStatus::Invalid => EntryNameAvailability::Invalid,
                RenameNameStatus::Conflict | RenameNameStatus::ReplaceableFile => {
                    EntryNameAvailability::ExistingNonDirectory
                }
            };
            w.global::<crate::OperationsApi>()
                .set_rename_name_error(name_error_text(lang, name.as_ref(), availability).into());
        });
}

pub(super) fn install_filter_type(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_filter_type(move |ch: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            if st.active_ext_filter_on() {
                st.with_tabs_mut(|b| {
                    let a = b.active;
                    b.tabs[a].ext_filter.push_str(ch.as_ref());
                });
            } else {
                st.filter.borrow_mut().push_str(ch.as_ref());
            }
            let cur = st.current_path();
            refresh_listing(&w, &st, &cur);
            w.global::<crate::PanelsApi>()
                .set_active_filter(st.filter.borrow().clone().into());
        });
}

pub(super) fn install_filter_backspace(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_filter_backspace(move || {
            let Some(w) = weak.upgrade() else { return };
            if st.active_ext_filter_on() {
                st.with_tabs_mut(|b| {
                    let a = b.active;
                    b.tabs[a].ext_filter.pop();
                });
            } else {
                st.filter.borrow_mut().pop();
            }
            let cur = st.current_path();
            refresh_listing(&w, &st, &cur);
            w.global::<crate::PanelsApi>()
                .set_active_filter(st.filter.borrow().clone().into());
        });
}

pub(super) fn install_filter_clear(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::PanelsApi>()
        .on_filter_clear(move || {
            let Some(w) = weak.upgrade() else { return };
            // In extension mode: "close" = turn off the mode (hides the bar)
            // + clear it; otherwise clear the by-name type-ahead filter.
            if st.active_ext_filter_on() {
                st.with_tabs_mut(|b| {
                    let a = b.active;
                    b.tabs[a].ext_filter_on = false;
                    b.tabs[a].ext_filter.clear();
                });
            } else {
                st.filter.borrow_mut().clear();
            }
            let cur = st.current_path();
            refresh_listing(&w, &st, &cur);
            w.global::<crate::PanelsApi>()
                .set_active_filter(st.filter.borrow().clone().into());
        });
}

pub(super) fn install_refresh_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_refresh_all(move || {
            let Some(w) = weak.upgrade() else { return };
            let _ = drain_thumbnail_invalidations(&st);
            // A file operation (copy/move/delete/trash/
            // duplicate/link/restore, or drop onto a folder) has modified the
            // CONTENTS of folders → their cached RECURSIVE mtime is stale. We
            // clear it, like F5, so the "folder modified date" column
            // recomputes on its own (otherwise F5 had to be pressed). Invisible: the
            // recompute is a separate background worker and thumbnails of
            // untouched paths still come from the LRU. No-op if the features
            // are off (empty caches).
            st.rmtime_cache.borrow_mut().clear();
            st.size_cache.borrow_mut().clear();
            refresh_all_panels(&w, &st);
            // The rows now exist, so a completed operation can point at its result.
            apply_focus_after_refresh(&w, &st);
        });
}

pub(super) fn install_rename_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_rename_confirmed(move |new_name: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let new = new_name.to_string();
            let Some(from) = st.rename_source.borrow().clone() else {
                return;
            };
            let old = from
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if new.is_empty() || new == old {
                return;
            }
            // Final safeguard: never overwrite an existing item (the UI already
            // blocks this; this protects against a race / a programmatic call).
            if rename_name_status_now(&st, &from, &new) != RenameNameStatus::Valid {
                warn!(name = %new, "rename: name conflicts, ignoring");
                return;
            }
            // The name ACTUALLY obtained (authoritative: the path returned by the OS), to
            // find the row again after re-sorting.
            let renamed_to = match ops::rename_in_place(&from, &new) {
                Ok(to) => {
                    info!(from = %from.display(), to = %to.display(), "renamed");
                    Some(to)
                }
                Err(err) => {
                    error!(error = %err, "rename failed");
                    report_rename_failure(
                        weak.clone(),
                        vec![from.clone()],
                        st.snapshot_config().language,
                        err.to_string(),
                    );
                    None
                }
            };
            *st.rename_source.borrow_mut() = None;
            if let Some(to) = renamed_to.as_ref() {
                invalidate_thumbnail_paths(&st, &[from.clone(), to.clone()]);
                // The colour and the note are keyed by path, so they have to
                // follow the item rather than be left behind under a name that
                // no longer exists — where a later item of the same name would
                // inherit them. Done only once the rename actually succeeded,
                // and with the path the OS returned rather than the one asked
                // for. Renaming a folder carries what was annotated inside it.
                let mut annotations = annotations_for_update(&st);
                annotations.rename(&from, to);
                save_annotations(&st, &annotations);
            }
            // Refresh EVERY panel, not just the active one, so other views on the
            // same folder show the rename immediately — harmonized with
            // delete/copy/move. Also clears the recursive-mtime cache.
            w.global::<crate::OperationsApi>().invoke_refresh_all();
            // Sorting may have moved the entry (sometimes off-screen): we
            // re-select it and bring it into view.
            if let Some(name) = renamed_to.and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }) {
                focus_entry_by_name(&w, &st, &name);
            }
        });
}

pub(super) fn install_rename_force_replace(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_rename_force_replace(move |new_name: SharedString| -> bool {
            let Some(w) = weak.upgrade() else {
                return false;
            };
            let new = new_name.to_string();
            let Some(from) = st.rename_source.borrow().clone() else {
                return false;
            };
            if rename_name_status_now(&st, &from, &new) != RenameNameStatus::ReplaceableFile {
                warn!(name = %new, "rename force replace: target cannot be replaced");
                return false;
            }
            let replaced_to = match ops::rename_in_place_replacing_file(&from, &new) {
                Ok(to) => {
                    info!(from = %from.display(), to = %to.display(), "renamed with replacement");
                    Some(to)
                }
                Err(err) => {
                    error!(error = %err, "rename force replace failed");
                    let mut candidates = vec![from.clone()];
                    if let Some(parent) = from.parent() {
                        candidates.push(parent.join(&new));
                    }
                    report_rename_failure(
                        weak.clone(),
                        candidates,
                        st.snapshot_config().language,
                        err.to_string(),
                    );
                    None
                }
            };
            let Some(replaced_to) = replaced_to else {
                return false;
            };
            // Same reasoning as a plain rename, with one more thing to settle:
            // the item that was overwritten is gone, so its colour and note
            // must not stay on that path and end up describing the newcomer.
            // The store's `rename` does exactly that — it carries the source's
            // annotation over and drops whatever the destination held.
            {
                let mut annotations = annotations_for_update(&st);
                annotations.rename(&from, &replaced_to);
                save_annotations(&st, &annotations);
            }
            *st.rename_source.borrow_mut() = None;
            invalidate_thumbnail_paths(&st, &[from, replaced_to]);
            // Refresh every panel (see `on_rename_confirmed`).
            w.global::<crate::OperationsApi>().invoke_refresh_all();
            true
        });
}

pub(super) fn install_create_check(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::OperationsApi>().on_create_check(
        move |kind: i32, name: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            if kind != 0 && kind != 1 {
                w.global::<crate::OperationsApi>()
                    .set_create_name_valid(true);
                w.global::<crate::OperationsApi>()
                    .set_create_conflict(false);
                // Clears whatever the folder/file mode had reported: these
                // modes judge the name by their own rules, further down.
                w.global::<crate::OperationsApi>()
                    .set_create_name_error(SharedString::new());
                return;
            }
            let cur = st.current_path();
            let trimmed = name.trim();
            // A reserved name is syntactically fine but already spoken for, so
            // it reads as a conflict just like an existing entry.
            let availability = entry_name_availability_now(&st, &cur, trimmed);
            let lang = st.snapshot_config().language;
            push_create_name_status(&w, lang, trimmed, availability);
        },
    );
}

pub(super) fn install_create_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::OperationsApi>().on_create_confirmed(
        move |kind: i32, name: SharedString, target: SharedString| -> bool {
            let Some(w) = weak.upgrade() else {
                return false;
            };
            let name = name.trim().to_string();
            let cur = st.current_path();
            let created = if kind == 2 {
                create_shortcut_entry(&cur, &name, target.trim())
            } else if kind == 3 {
                create_link_entry(
                    &w,
                    st.snapshot_config().language,
                    &cur,
                    &name,
                    target.trim(),
                )
            } else if name.is_empty() {
                return false;
            } else {
                // Final safeguard against an external navigation/change between the
                // keystroke and the click. A creation never offers to replace.
                let availability = entry_name_availability_now(&st, &cur, &name);
                if availability != EntryNameAvailability::Available {
                    let lang = st.snapshot_config().language;
                    push_create_name_status(&w, lang, &name, availability);
                    return false;
                }
                match ops::create_entry(&cur, &name, kind == 0) {
                    Ok(p) => {
                        info!(path = %p.display(), kind, "created entry");
                        p.file_name().map(|n| n.to_string_lossy().into_owned())
                    }
                    Err(err) => {
                        error!(error = %err, "create entry failed");
                        // An entry may have appeared after the guard check. The popup
                        // stays open and then reflects the newly
                        // observed conflict; `create_entry` guarantees it is left intact.
                        let late = entry_name_availability_now(&st, &cur, &name);
                        if late != EntryNameAvailability::Available {
                            let lang = st.snapshot_config().language;
                            push_create_name_status(&w, lang, &name, late);
                        }
                        None
                    }
                }
            };
            // A newly created HIDDEN entry (dotfile) would land out of sight
            // when the view isn't showing hidden files — a user might create
            // `.myconfig` and never find it. So, ONLY when the created
            // name is a dotfile, turn on "show hidden" for the active view
            // before the refresh below reveals it.
            if created.as_deref().is_some_and(|n| n.starts_with('.')) {
                let active = *st.active_panel.borrow();
                if let Some(panel) = st.panels.borrow_mut().get_mut(active) {
                    let t = &mut panel.tabs.tabs[panel.tabs.active];
                    t.show_hidden = true;
                }
            }
            if let Some(created_name) = created.as_ref() {
                invalidate_thumbnail_paths(&st, &[cur.join(created_name)]);
            }
            // Refresh every panel: other views on the same folder must show
            // the new entry immediately (like delete/copy/move).
            w.global::<crate::OperationsApi>().invoke_refresh_all();
            // Targets the new entry (cursor + selection) → a simple Enter
            // opens it right after (especially a folder just created).
            if let Some(created_name) = created {
                focus_entry_by_name(&w, &st, &created_name);
                true
            } else {
                false
            }
        },
    );
}

pub(super) fn install_create_browse_target(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_create_browse_target(move || {
            let Some(w) = weak.upgrade() else { return };
            // Folder tab → folder picker; File tab → file
            // picker ("All files").
            let picked = if w
                .global::<crate::OperationsApi>()
                .get_create_shortcut_folder()
            {
                openwith::browse_for_folder()
            } else {
                openwith::browse_for_target(st.snapshot_config().language)
            };
            if let Some(path) = picked {
                if w.global::<crate::OperationsApi>()
                    .get_create_name()
                    .trim()
                    .is_empty()
                {
                    // Pre-fills the name: folder name OR file name without extension.
                    let p = Path::new(&path);
                    let derived = if w
                        .global::<crate::OperationsApi>()
                        .get_create_shortcut_folder()
                    {
                        p.file_name()
                    } else {
                        p.file_stem()
                    };
                    if let Some(n) = derived {
                        w.global::<crate::OperationsApi>()
                            .set_create_name(n.to_string_lossy().into_owned().into());
                    }
                }
                w.global::<crate::OperationsApi>()
                    .set_create_target(path.into());
            }
        });
}

pub(super) fn install_empty_right_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_empty_right_clicked(move |x: f32, y: f32| {
            let Some(w) = weak.upgrade() else { return };
            // After deselection, targetless actions (Paste, copy path,
            // etc.) apply to the current folder.
            selection_set_all(&st.active_rows_model(), false);
            st.set_selection_anchor(-1);
            // Commands pinned to the view BACKGROUND (bit 4) — target the
            // current folder (e.g. "Git Bash here").
            let n_custom = push_ctx_custom_entries(&w, &st, openers::CTX_BACKGROUND, &[]);
            w.global::<crate::MenuApi>().set_ctx_custom_bg(true);
            // Shell menu of the CURRENT FOLDER "as an item":
            // exposes the full PeaZip menu, Git Bash here, Select left folder…
            // where Windows's "background" menu is much sparser.
            let cur = st.current_path();
            let n_shell = refresh_shell_menu(&w, &st, std::slice::from_ref(&cur));
            // Reduced menu (empty area): 194 px + pinned + shell.
            let h = 194.0
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
            w.global::<crate::MenuApi>().set_ctx_on_empty(true);
            w.global::<crate::MenuApi>().set_ctx_menu_x(x);
            w.global::<crate::MenuApi>().set_ctx_menu_y(y);
            arm_context_menu_navigation(&w);
            w.global::<crate::MenuApi>().set_ctx_menu_open(true);
        });
}

pub(super) fn install_process_op_events(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_process_op_events(move || {
            let Some(w) = weak.upgrade() else { return };
            let events: Vec<OpDelivery> = {
                let mut queue = st
                    .op_deliveries
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                queue.drain(..).collect()
            };
            // Annotation follow-ups are COLLECTED here and applied below, in a
            // single borrow. Two reasons, both real: the other arms re-enter
            // the interface — a restore asks every view to re-list — and a
            // listing needs the same store, so a borrow held across this loop
            // would meet itself. And borrowing per event would re-read the file
            // between two events of one batch, discarding what the previous
            // ones had just changed in memory.
            let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
            let mut gone: Vec<PathBuf> = Vec::new();
            for event in events {
                match event {
                    // The colour and the note are keyed by path, so they follow
                    // the item. Moving a folder carries what was annotated
                    // inside it.
                    OpDelivery::Moved { from, to } => moved.push((from, to)),
                    // Nothing brings this one back, so its annotation goes with
                    // it — and everything that was annotated under it.
                    OpDelivery::PermanentlyDeleted(path) => gone.push(path),
                    OpDelivery::Replaced(path) => gone.push(path),
                    OpDelivery::Trashed(path) => {
                        *st.last_trashed.borrow_mut() = Some(path);
                    }
                    OpDelivery::RestoreFinished {
                        op_id,
                        original_path,
                        error,
                    } => {
                        if let Some(handle) = st.ops.finish(op_id) {
                            invalidate_thumbnail_paths(&st, &handle.thumbnail_invalidations);
                        }
                        sync_op_busy(&w, &st);
                        let lang = st.snapshot_config().language;
                        let name = original_path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| original_path.display().to_string());
                        if let Some(err) = error {
                            error!(
                                path = %original_path.display(),
                                error = ?err,
                                "restore from trash failed"
                            );
                            let reason = i18n::trash_error_message(lang, &err);
                            let message =
                                i18n::tr(lang, "trash_restore_failed").replace("{name}", &name);
                            show_notice(&w, format!("{message}: {reason}"));
                        } else {
                            if st.last_trashed.borrow().as_ref() == Some(&original_path) {
                                st.last_trashed.borrow_mut().take();
                            }
                            let message = i18n::tr(lang, "trash_restored").replace("{name}", &name);
                            notice(&w, message, NoticeKind::Success);
                            // Several views may be displaying the restored folder.
                            w.global::<crate::OperationsApi>().invoke_refresh_all();
                        }
                    }
                }
            }
            if !moved.is_empty() || !gone.is_empty() {
                let mut annotations = annotations_for_update(&st);
                // Clearing comes FIRST, and the order is load-bearing. A move
                // onto an existing item reports both: the destination is
                // replaced, then the source arrives there. Renaming before
                // clearing would install the source's annotation and wipe it a
                // line later, losing what the user had written.
                for path in &gone {
                    annotations.forget(path);
                }
                for (from, to) in &moved {
                    annotations.rename(from, to);
                }
                // One operation reports item by item; the file is written once,
                // not per path. Cosmetic data: a failure is logged and never
                // interrupts the operation that just succeeded.
                save_annotations(&st, &annotations);
            }
        });
}

pub(super) fn install_action_delete(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_delete(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            if paths.is_empty() {
                return;
            }
            if cfg!(windows)
                && paths
                    .iter()
                    .any(|p| favnyr_core::places::is_network_path(p))
            {
                *st.delete_pending.borrow_mut() = paths;
                w.global::<crate::OperationsApi>()
                    .set_delete_warning_permanent(false);
                w.global::<crate::OperationsApi>()
                    .set_delete_warning_open(true);
                return;
            }
            let lang = st.snapshot_config().language;
            start_heavy_op(&w, &st, Heavy::Trash(paths), lang, None, None);
        });
}

pub(super) fn install_action_restore_last_trashed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_restore_last_trashed(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(original_path) = st.last_trashed.borrow().clone() else {
                return;
            };
            let current_path = st.current_path();
            let Some(original_parent) = original_path.parent() else {
                return;
            };
            if !ops::paths_equal(original_parent, &current_path) {
                return;
            }

            // Registered like any long operation, even though it shows no toast:
            // that is what keeps it mutually exclusive with the others.
            // Restoring is a single atomic move, so it offers no cancellation.
            // Restoring writes the item back to where it came from: that path is
            // claimed like any other destination. Moving it back is a single
            // atomic operation, so it offers no cancellation.
            let op_id = st.ops.register(OpHandle {
                cancel: None,
                pending_focus: None,
                targets: vec![original_path.clone()],
                thumbnail_invalidations: vec![original_path.clone()],
                transient_cleanup: None,
            });
            sync_op_busy(&w, &st);
            let deliveries = st.op_deliveries.clone();
            let weak = w.as_weak();
            std::thread::spawn(move || {
                let error = ops::restore_from_trash(&original_path).err();
                deliver_op_event(
                    &weak,
                    &deliveries,
                    OpDelivery::RestoreFinished {
                        op_id,
                        original_path,
                        error,
                    },
                );
            });
        });
}

pub(super) fn install_action_delete_permanent(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_delete_permanent(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            if paths.is_empty() {
                return;
            }
            *st.delete_pending.borrow_mut() = paths;
            w.global::<crate::OperationsApi>()
                .set_delete_warning_permanent(true);
            w.global::<crate::OperationsApi>()
                .set_delete_warning_open(true);
        });
}

pub(super) fn install_delete_warning_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_delete_warning_confirmed(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = std::mem::take(&mut *st.delete_pending.borrow_mut());
            if paths.is_empty() {
                return;
            }
            let lang = st.snapshot_config().language;
            let work = if w
                .global::<crate::OperationsApi>()
                .get_delete_warning_permanent()
            {
                Heavy::PermanentDelete(paths)
            } else {
                Heavy::Trash(paths)
            };
            start_heavy_op(&w, &st, work, lang, None, None);
        });
}

pub(super) fn install_action_properties(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_properties(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            let Some(first) = paths.first().cloned() else {
                return;
            };
            if let Err(err) = actions::show_native_properties(&first) {
                error!(error = %err, path = %first.display(), "native properties failed");
                let lang = st.snapshot_config().language;
                notice(&w, i18n::tr(lang, "prop_native_failed"), NoticeKind::Error);
            }
        });
}
