use super::*;

pub(super) fn install_file_drag_begin(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_file_drag_begin(move |src_panel: i32, row_idx: i32| {
        let Some(w) = weak.upgrade() else { return };
        // A previous drop menu may have been closed without choosing an action.
        // No new gesture should keep its stale frozen paths.
        *st.pending_file_drop.borrow_mut() = None;
        let src = src_panel.max(0) as usize;
        let changed_count = {
            let mut panels = st.panels.borrow_mut();
            let Some(p) = panels.get_mut(src) else {
                return;
            };
            let already = p
                .rows_model
                .row_data(row_idx.max(0) as usize)
                .map(|r| r.selected)
                .unwrap_or(false);
            if already {
                None
            } else {
                let (count, _) = selection_set_only(&p.rows_model, row_idx);
                let active = p.tabs.active;
                p.tabs.tabs[active].selection_anchor = row_idx;
                p.tabs.tabs[active].cursor = row_idx;
                Some(count)
            }
        };
        if let Some(count) = changed_count {
            // `sel-touch` activates the panel before starting the drag. The
            // footer and the Shift anchor must follow the implicit selection.
            if *st.active_panel.borrow() == src {
                push_active_footer(&w, &st, count);
            }
        }
    });
}

pub(super) fn install_file_drop_target_invalid(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_file_drop_target_invalid(move |src_panel, target_panel, row| {
        file_drop_target_invalid(&st, src_panel, target_panel, row)
    });
}

pub(super) fn install_start_native_drag(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_start_native_drag(move |src_panel: i32| {
        let src = src_panel.max(0) as usize;
        let paths = panel_selected_paths(&st, src);
        if paths.is_empty() {
            return;
        }
        #[cfg(windows)]
        {
            let dropped = crate::winddrag::drag_files(&paths);
            debug!(count = paths.len(), dropped, "native OLE drag finished");
        }
        #[cfg(not(windows))]
        {
            let _ = paths; // no native drag outside Windows (v1)
        }
    });
}

pub(super) fn install_file_drop_onto_exec(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_file_drop_onto_exec(move || -> bool {
        let Some(w) = weak.upgrade() else { return false };
        let target = w.get_file_drop_target_panel().max(0) as usize;
        let row = w.get_file_drop_target_row();
        let src = w.get_file_drop_src_panel();
        let sources = if src < 0 {
            std::mem::take(&mut *st.external_drop_paths.borrow_mut())
        } else {
            panel_selected_paths(&st, src as usize)
        };
        if sources.is_empty() {
            *st.pending_file_drop.borrow_mut() = None;
            return false;
        }

        if row >= 0
            && let Some(exe) = panel_path_at_row(&st, target, row as usize)
            && is_drop_runnable(&exe)
        {
            // Don't launch on itself (the exe is part of the selection).
            let args: Vec<PathBuf> = sources
                .iter()
                .filter(|path| *path != &exe)
                .cloned()
                .collect();
            if !args.is_empty() {
                *st.pending_file_drop.borrow_mut() = None;
                match actions::run_with_files(&exe, &args) {
                    Ok(()) => info!(exe = %exe.display(), n = args.len(), "drop onto executable: run with args"),
                    Err(err) => error!(error = %err, "drop-onto-exe run failed"),
                }
                return true;
            }
        }

        let destination = if row >= 0 {
            panel_folder_at_row(&st, target, row as usize)
                .unwrap_or_else(|| panel_dir(&st, target))
        } else {
            panel_dir(&st, target)
        };
        *st.pending_file_drop.borrow_mut() = if destination.as_os_str().is_empty() {
            None
        } else {
            Some(PendingFileDrop {
                sources,
                destination,
            })
        };
        false
    });
}

pub(super) fn install_file_drop_action(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_file_drop_action(move |action: i32| {
        let Some(w) = weak.upgrade() else { return };
        let Some(PendingFileDrop {
            sources,
            destination: dst,
        }) = st.pending_file_drop.borrow_mut().take()
        else {
            return;
        };
        if sources.is_empty() || dst.as_os_str().is_empty() {
            return;
        }
        // Never drop a folder INTO itself or one of its
        // descendants (recursion), nor onto itself.
        let sources: Vec<PathBuf> = sources
            .into_iter()
            .filter(|s| !ops::is_within(&dst, s))
            .collect();
        if sources.is_empty() {
            return;
        }
        match action {
            0 => begin_paste(&w, &st, ClipOp::Cut, dst, sources), // move
            1 => begin_paste(&w, &st, ClipOp::Copy, dst, sources), // copy
            2 => {
                // Link: symbolic link where allowed (both OSes), else a
                // hard link / junction on the same volume (Windows). Sync
                // (instant) → we refresh all views at the end. A failure
                // (e.g. a cross-volume drop without Developer Mode, or a
                // network share) is surfaced, not just logged.
                let mut created = Vec::new();
                let mut any_failed = false;
                for src in &sources {
                    match ops::link_into(src, &dst) {
                        Ok(path) => created.push(path),
                        Err(err) => {
                            // Full reason (cross-volume, privilege, filesystem)
                            // goes to the log; the toast stays concise.
                            error!(error = %err, src = %src.display(), "link failed");
                            any_failed = true;
                        }
                    }
                }
                invalidate_thumbnail_paths(&st, &created);
                w.invoke_refresh_all();
                if any_failed {
                    let lang = st.snapshot_config().language;
                    show_notice(&w, i18n::tr(lang, "link_failed"));
                }
            }
            _ => {}
        }
    });
}

pub(super) fn install_tab_drag_progress(
    window: &MainWindow,
    hover_target: Rc<std::cell::Cell<isize>>,
) {
    let weak = window.as_weak();
    let last = hover_target.clone();
    window.on_tab_drag_progress(move |_from_idx: i32, ax: f32, ay: f32| {
        let Some(w) = weak.upgrade() else { return };
        // Logical client → physical screen (`WindowFromPoint`'s frame of reference).
        let (sx, sy) = window_logical_to_screen(&w, ax, ay);
        let target = crate::winmsg::target_at(sx, sy).unwrap_or(0);
        let prev = last.get();
        if prev != target && prev != 0 {
            crate::winmsg::hover_end(prev); // left the previous instance
        }
        if target != 0 {
            crate::winmsg::hover(target, sx, sy);
        }
        last.set(target);
    });
}

pub(super) fn install_tab_drag_completed(
    window: &MainWindow,
    state: AppState,
    hover_target: Rc<std::cell::Cell<isize>>,
) {
    let last_hover = hover_target.clone();
    let st = state.clone();
    let weak = window.as_weak();
    window.on_tab_drag_completed(
        move |src_panel: i32, from_idx: i32, target_intra: i32, abs_x: f32, abs_y: f32| {
            let Some(w) = weak.upgrade() else { return };
            // End of drag: clears any hover preview still displayed in
            // another instance. For a TRANSFER, the target instance clears it
            // too upon receiving it; this hover_end covers the other cases (tear-off,
            // cancellation, or moving the cursor out of the hovered instance).
            let hovered = last_hover.replace(0);
            if hovered != 0 {
                crate::winmsg::hover_end(hovered);
            }

            // The SOURCE panel of the drag is passed explicitly (it may
            // NOT be the active panel — we no longer activate mid-drag since that
            // would rebuild the model and kill the drag).
            let source_panel =
                (src_panel.max(0) as usize).min(st.panels.borrow().len().saturating_sub(1));

            // --- Drop OUTSIDE this window: transfer to another Favnyr
            // instance OR tear-off. `abs_x/abs_y` are in
            // (logical) WINDOW coordinates; we compare against the client dimensions.
            // 24px margin: avoids a tear-off on a simple edge overshoot
            // (and covers most of the title bar at the top → no false
            // positive when going a bit too far up).
            {
                let outside = dropped_outside(&w, abs_x, abs_y);
                // Physical SCREEN position of the cursor at drop, converted from
                // the real Win32 CLIENT frame of reference (no borders/title bar).
                let at = window_logical_to_screen(&w, abs_x, abs_y);
                let from = from_idx.max(0) as usize;
                // 1) ANOTHER Favnyr window under the cursor → TRANSFER the tab
                // into that instance. Tested BEFORE `outside`: `WindowFromPoint`
                // returns the window at the TOP of the z-order — if instance B overlaps
                // A and the user sees B under the cursor, we do transfer
                // to B even if the point is still within A's bounds.
                if let Some(target) = crate::winmsg::target_at(at.0, at.1) {
                    w.set_drag_target_panel(-1);
                    w.set_drag_target_zone(0);
                    w.set_drag_target_gap(-1);
                    w.set_drag_target_gap_panel(-1);
                    match transfer_tab_to(&st, source_panel, from, target, at) {
                        Transfer::Moved => {
                            refresh_all_panels(&w, &st);
                            return;
                        }
                        // Instance emptied → in the process of closing: don't touch anything.
                        Transfer::Emptied => return,
                        // Target frozen/gone → tear off if outside the window, otherwise abandon.
                        Transfer::Failed => {
                            if !outside {
                                return;
                            }
                        }
                    }
                }
                // 2) Outside the window, no target instance → tear-off: new
                // Favnyr instance on the tab's folder.
                if outside {
                    w.set_drag_target_panel(-1);
                    w.set_drag_target_zone(0);
                    w.set_drag_target_gap(-1);
                    w.set_drag_target_gap_panel(-1);
                    if tear_off_tab(&st, source_panel, from, Some(at)) {
                        refresh_all_panels(&w, &st);
                    }
                    return;
                }
            }
            // Target + zone computed by the hovered panel (Slint).
            let dtp = w.get_drag_target_panel();
            let zone = w.get_drag_target_zone();
            w.set_drag_target_panel(-1);
            w.set_drag_target_zone(0);
            // Last claimed insertion gap (header) + panel claiming
            // full geometry on the Slint side, exact for any width.
            let gap = w.get_drag_target_gap();
            let gap_panel = w.get_drag_target_gap_panel();
            w.set_drag_target_gap(-1);
            w.set_drag_target_gap_panel(-1);

            // --- Drop on an EDGE → split of the target panel + adoption of the tab.
            if dtp >= 0 && (2..=5).contains(&zone) {
                let target_panel = dtp as usize;
                let dir = if zone == 4 || zone == 5 {
                    SplitDir::Column
                } else {
                    SplitDir::Row
                };
                // West/North → the new panel goes first (left/top).
                let new_first = zone == 2 || zone == 4;
                let from = from_idx.max(0) as usize;
                if split_with_tab(&st, source_panel, from, target_panel, dir, new_first) {
                    refresh_all_panels(&w, &st);
                }
                return;
            }

            // Drop outside any panel zone (gap, splitter, outside the window):
            // the drag didn't hover any panel → we cancel (the tab stays put).
            if dtp < 0 {
                return;
            }
            let target_panel = (dtp as usize).min(st.panels.borrow().len().saturating_sub(1));
            let from = from_idx.max(0) as usize;

            if target_panel == source_panel {
                // Same panel: reorder if an insertion position is
                // known (drop on the tab bar), otherwise no-op.
                if target_intra >= 0 {
                    let moved = {
                        let mut panels = st.panels.borrow_mut();
                        source_panel < panels.len()
                            && panels[source_panel]
                                .tabs
                                .move_tab(from, target_intra as usize)
                    };
                    if moved {
                        update_panels_ui(&w, &st);
                    }
                }
                // The drag (even a no-op one) activates the source panel.
                *st.active_panel.borrow_mut() = source_panel;
                update_panels_ui(&w, &st);
            } else {
                // Cross-view: move the tab into the target panel. If the
                // source empties out (last tab), it gets closed (merge).
                // Insertion position: EXACT gap claimed by the TARGET's
                // header (drop on its tab bar); otherwise — drop in the body
                // ("move" zone) — at the END of the bar, predictably.
                let insert_at = if gap >= 0 && gap_panel == dtp {
                    gap as usize
                } else {
                    st.panels.borrow()[target_panel].tabs.tabs.len()
                };
                if let Some(active) =
                    move_tab_between(&st, source_panel, from, target_panel, insert_at)
                {
                    *st.active_panel.borrow_mut() = active;
                    refresh_all_panels(&w, &st);
                }
            }
        },
    );
}
