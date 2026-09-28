use super::*;

pub(super) fn install_action_copy_path(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_action_copy_path(move || {
        let paths = selected_paths(&st);
        // If several are selected, join them with a newline.
        let text = if paths.is_empty() {
            st.current_path().display().to_string()
        } else {
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };
        if let Err(err) = actions::copy_to_clipboard(&text) {
            error!(error = %err, "copy_to_clipboard (path) failed");
        } else {
            info!(len = text.len(), "copied path(s)");
        }
    });
}

pub(super) fn install_action_copy_name(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_action_copy_name(move || {
        let paths = selected_paths(&st);
        let text = if paths.is_empty() {
            // No selection → name of the current folder.
            st.current_path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
        } else {
            paths
                .iter()
                .filter_map(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        if let Err(err) = actions::copy_to_clipboard(&text) {
            error!(error = %err, "copy_to_clipboard (name) failed");
        } else {
            info!(len = text.len(), "copied name(s)");
        }
    });
}

pub(super) fn install_action_copy(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_action_copy(move || {
        if weak.upgrade().is_none() {
            return;
        }
        let paths = selected_paths(&st);
        if paths.is_empty() {
            return;
        }
        // OS clipboard (Explorer / Wayland compositor) — best-effort.
        if let Err(err) = clipboard::write_files(&paths, false) {
            debug!(error = %err, "clipboard write (copy) failed");
        }
        {
            let mut clip = st.clipboard.borrow_mut();
            clip.paths = paths;
            clip.op = Some(ClipOp::Copy);
        }
        clear_cut_marks(&st.active_rows_model());
        info!(count = ?st.clipboard.borrow_mut().paths.len(), "copy");
    });
}

pub(super) fn install_action_cut(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_action_cut(move || {
        if weak.upgrade().is_none() {
            return;
        }
        let paths = selected_paths(&st);
        if paths.is_empty() {
            return;
        }
        // OS clipboard (Explorer / Wayland) — "cut" = move.
        if let Err(err) = clipboard::write_files(&paths, true) {
            debug!(error = %err, "clipboard write (cut) failed");
        }
        {
            let mut clip = st.clipboard.borrow_mut();
            clip.paths = paths.clone();
            clip.op = Some(ClipOp::Cut);
        }
        // Visually marks the matching rows (by name).
        let cur_dir = st.current_path();
        let cut_names: std::collections::HashSet<String> = paths
            .iter()
            .filter_map(|p| {
                if p.parent() == Some(cur_dir.as_path()) {
                    p.file_name()
                        .and_then(|n| n.to_str().map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect();
        apply_cut_marks(&st.active_rows_model(), &cut_names);
        info!(count = paths.len(), "cut");
    });
}

pub(super) fn install_action_paste(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_action_paste(move || {
        let Some(w) = weak.upgrade() else { return };
        let cur = st.current_path();
        if cur.as_os_str().is_empty() {
            return;
        }
        // Priority to the OS clipboard (files copied/cut in
        // Explorer, Dolphin, or another app). Falls back to the
        // internal state if the clipboard doesn't contain files.
        let (paths, op) = match clipboard::read_files() {
            Some((paths, cut)) => (paths, Some(if cut { ClipOp::Cut } else { ClipOp::Copy })),
            None => {
                let clip = st.clipboard.borrow();
                (clip.paths.clone(), clip.op)
            }
        };
        let Some(op) = op else { return };
        if paths.is_empty() {
            return;
        }
        // A Cut pasted back into the folder its items already live in can
        // move nothing (each source would be skipped by `begin_paste`):
        // treat it as CANCELLING the cut rather than silently doing nothing.
        // Un-grey the rows now and drop the clipboard so the cancellation is
        // clean and no later paste keeps a stale move pending.
        if matches!(op, ClipOp::Cut)
            && paths.iter().all(|p| {
                p.parent()
                    .is_some_and(|parent| ops::paths_equal(parent, &cur))
            })
        {
            clear_cut_marks(&st.active_rows_model());
            let mut clip = st.clipboard.borrow_mut();
            clip.paths.clear();
            clip.op = None;
            return;
        }
        begin_paste(&w, &st, op, cur, paths);
    });
}

pub(super) fn install_paste_conflict_check(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_check(move |name: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        w.set_paste_conflict_name_taken(paste_name_invalid(&st, &name));
    });
}

pub(super) fn install_paste_conflict_confirm(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_confirm(move |name: SharedString| {
        let Some(w) = weak.upgrade() else { return };
        let name = name.to_string();
        if paste_name_invalid(&st, &name) {
            // Safeguard: name taken/invalid → we don't close, we flag it again.
            w.set_paste_conflict_name_taken(true);
            return;
        }
        {
            let mut guard = st.paste_job.borrow_mut();
            if let Some(job) = guard.as_mut()
                && let Some(src) = job.current.take()
            {
                // Rename → free target (no overwrite).
                job.resolved.push((src, job.dst_dir.join(&name), false));
            }
        }
        advance_paste(&w, &st);
    });
}

pub(super) fn install_paste_conflict_skip(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_skip(move || {
        let Some(w) = weak.upgrade() else { return };
        {
            let mut guard = st.paste_job.borrow_mut();
            if let Some(job) = guard.as_mut() {
                job.current = None;
            }
        }
        advance_paste(&w, &st);
    });
}

pub(super) fn install_paste_conflict_replace(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_replace(move || {
        let Some(w) = weak.upgrade() else { return };
        {
            let mut guard = st.paste_job.borrow_mut();
            if let Some(job) = guard.as_mut()
                && let Some(src) = job.current.take()
            {
                resolve_replace(job, src);
            }
        }
        advance_paste(&w, &st);
    });
}

pub(super) fn install_paste_conflict_replace_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_replace_all(move || {
        let Some(w) = weak.upgrade() else { return };
        {
            let mut guard = st.paste_job.borrow_mut();
            if let Some(job) = guard.as_mut() {
                if let Some(src) = job.current.take() {
                    resolve_replace(job, src);
                }
                while let Some(src) = job.pending.pop_front() {
                    resolve_replace(job, src);
                }
            }
        }
        advance_paste(&w, &st);
    });
}

pub(super) fn install_paste_conflict_skip_all(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.on_paste_conflict_skip_all(move || {
        let Some(w) = weak.upgrade() else { return };
        {
            let mut guard = st.paste_job.borrow_mut();
            if let Some(job) = guard.as_mut() {
                job.current = None;
                job.pending.clear();
            }
        }
        advance_paste(&w, &st);
    });
}

pub(super) fn install_paste_conflict_cancel(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window.on_paste_conflict_cancel(move || {
        *st.paste_job.borrow_mut() = None;
    });
}
