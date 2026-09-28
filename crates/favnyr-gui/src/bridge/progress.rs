use super::*;

// ---------- Long background operations (progress bar) ----------

pub(super) const OP_RUNNING: i32 = 1;
pub(super) const OP_SCAN: i32 = 2;
pub(super) const OP_SUCCESS: i32 = 3;
pub(super) const OP_ERROR: i32 = 4;
pub(super) const OP_CANCELLED: i32 = 5;

/// Work handed to the background thread. For Copy/Move, each item is
/// `(source, target, overwrite)`: `overwrite = true` → the existing target is
/// removed before the operation ("Replace" resolution).
pub(super) enum Heavy {
    Copy(Vec<(PathBuf, PathBuf, bool)>),
    Move(Vec<(PathBuf, PathBuf, bool)>),
    Trash(Vec<PathBuf>),
    PermanentDelete(Vec<PathBuf>),
}

/// i18n labels captured (owned → Send) for the thread.
#[derive(Clone)]
pub(super) struct OpLabels {
    running: String,
    scanning: String,
    done: String,
    cancelled: String,
    errors: String,
    items: String,
}

/// Toasts drawn in the stack; the rest are summarised by the "+N others" chip.
/// Mirrors the bound used by the `for` loop over `ops` in the .slint.
pub(super) const MAX_VISIBLE_TOASTS: usize = 3;

/// Number of operation toasts the stack cannot display.
pub(super) fn hidden_toast_count(total: usize) -> usize {
    total.saturating_sub(MAX_VISIBLE_TOASTS)
}

/// Pushes a toast update from a worker thread to the UI thread.
///
/// `visible` carries the deferred-appearance threshold: below it the operation
/// gets no row at all, so a quick copy never flashes a toast.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_op(
    weak: &slint::Weak<MainWindow>,
    op_id: i32,
    state_i: i32,
    visible: bool,
    progress: f32,
    title: String,
    detail: String,
    percent: String,
) {
    if !visible {
        return;
    }
    let row = OpProgress {
        id: op_id,
        state: state_i,
        progress,
        indeterminate: state_i == OP_SCAN,
        title: title.into(),
        detail: detail.into(),
        percent: percent.into(),
    };
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.invoke_op_progress(row);
        }
    });
}

/// Inserts or refreshes one operation's toast row, keyed by its id.
pub(super) fn upsert_op_row(state: &AppState, row: OpProgress) {
    let model = &state.ops_model;
    let existing =
        (0..model.row_count()).find(|&i| model.row_data(i).map(|r| r.id) == Some(row.id));
    match existing {
        Some(i) => model.set_row_data(i, row),
        None => model.push(row),
    }
}

/// Removes one operation's toast row, if it still has one.
pub(super) fn remove_op_row(state: &AppState, op_id: i32) {
    let model = &state.ops_model;
    if let Some(i) =
        (0..model.row_count()).find(|&i| model.row_data(i).map(|r| r.id) == Some(op_id))
    {
        model.remove(i);
    }
}

/// Refreshes what depends on the toast set as a whole: the "+N others" chip.
pub(super) fn refresh_ops_ui(window: &MainWindow, state: &AppState) {
    let hidden = hidden_toast_count(state.ops_model.row_count());
    let text = if hidden == 0 {
        String::new()
    } else {
        i18n::op_more_text(state.snapshot_config().language, hidden)
    };
    window.set_ops_more_text(text.into());
}

/// Publishes whether anything heavy is in flight. Drives the deferred
/// refreshes, which must not re-list while an operation is writing.
pub(super) fn sync_op_busy(window: &MainWindow, state: &AppState) {
    window.set_op_busy(state.ops.in_flight());
}

/// Settling delay before re-listing once operations complete. Short enough to
/// read as instant, long enough to absorb a burst of completions.
pub(super) const OP_REFRESH_DEBOUNCE_MS: u64 = 150;

thread_local! {
    static OP_REFRESH_DEBOUNCE: slint::Timer = slint::Timer::default();
}

/// Requests the re-listing that follows a completed operation.
///
/// `refresh_all_panels` re-reads every local panel synchronously, so running it
/// once per completion would be wasteful now that operations finish
/// independently. Restarting a single-shot timer collapses a burst into one
/// pass. A stream of completions closer together than the delay keeps pushing
/// it back, which is the intended trade: the views stay untouched while things
/// are still moving, then catch up in a single re-listing.
pub(super) fn request_op_refresh(window: &MainWindow) {
    let weak = window.as_weak();
    OP_REFRESH_DEBOUNCE.with(|timer| {
        timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(OP_REFRESH_DEBOUNCE_MS),
            move || {
                if let Some(w) = weak.upgrade() {
                    w.invoke_refresh_all();
                }
            },
        );
    });
}

/// Selects and scrolls to the item left behind by the last operation to finish,
/// once the views have been re-listed. Only acts if the ACTIVE view is really
/// showing its folder: the user may have navigated elsewhere during a long
/// copy, and the selection of an unrelated folder is never moved.
pub(super) fn apply_focus_after_refresh(window: &MainWindow, state: &AppState) {
    let Some(target) = state.focus_after_refresh.borrow_mut().take() else {
        return;
    };
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    if state.current_path() == dir {
        focus_entry_by_name(window, state, &name.to_string_lossy());
    }
}

/// Deposits a trash result into the Send queue then wakes its single
/// Slint consumer. Recovering from a poisoned mutex avoids permanently
/// losing the ability to cancel a deletion.
pub(super) fn deliver_op_event(
    weak: &slint::Weak<MainWindow>,
    deliveries: &Arc<std::sync::Mutex<VecDeque<OpDelivery>>>,
    event: OpDelivery,
) {
    deliveries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push_back(event);
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.invoke_process_op_events();
        }
    });
}

/// Starts a long operation: registers it, prepares the cancellation flag + the
/// toast, then launches the background thread. Operations run concurrently —
/// each gets its own thread, its own toast and its own cancel button — so this
/// never turns work away.
pub(super) fn start_heavy_op(
    window: &MainWindow,
    state: &AppState,
    work: Heavy,
    lang: Lang,
    pending_focus: Option<PathBuf>,
    transient_cleanup: Option<TransientDropGuard>,
) {
    // Destinations are claimed for the whole run: a paste started meanwhile
    // must see these names as taken even though nothing exists on disk yet.
    // Deletions write nowhere, so they claim nothing.
    let targets = match &work {
        Heavy::Copy(items) | Heavy::Move(items) => {
            items.iter().map(|(_, dst, _)| dst.clone()).collect()
        }
        Heavy::Trash(_) | Heavy::PermanentDelete(_) => Vec::new(),
    };
    let thumbnail_invalidations = match &work {
        Heavy::Copy(items) => items.iter().map(|(_, dst, _)| dst.clone()).collect(),
        Heavy::Move(items) => items
            .iter()
            .flat_map(|(src, dst, _)| [src.clone(), dst.clone()])
            .collect(),
        Heavy::Trash(paths) | Heavy::PermanentDelete(paths) => paths.clone(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let op_id = state.ops.register(OpHandle {
        cancel: Some(cancel.clone()),
        pending_focus,
        targets,
        thumbnail_invalidations,
        transient_cleanup,
    });

    let s = i18n::strings_for(lang);
    let running = match work {
        Heavy::Copy(_) => s.op_copying,
        Heavy::Move(_) => s.op_moving,
        Heavy::Trash(_) => s.op_deleting,
        Heavy::PermanentDelete(_) => s.op_deleting_permanently,
    }
    .to_string();
    let labels = OpLabels {
        running,
        scanning: s.op_scanning.to_string(),
        done: s.op_done.to_string(),
        cancelled: s.op_cancelled.to_string(),
        errors: s.op_errors.to_string(),
        items: s.op_items.to_string(),
    };

    // No row until the deferred appearance threshold (400 ms) — `push_op`
    // creates it on the first update that passes it.
    sync_op_busy(window, state);

    let weak = window.as_weak();
    let op_deliveries = state.op_deliveries.clone();
    std::thread::spawn(move || run_heavy(op_id, work, weak, cancel, lang, labels, op_deliveries));
}

pub(super) fn run_heavy(
    op_id: i32,
    work: Heavy,
    weak: slint::Weak<MainWindow>,
    cancel: Arc<AtomicBool>,
    lang: Lang,
    labels: OpLabels,
    op_deliveries: Arc<std::sync::Mutex<VecDeque<OpDelivery>>>,
) {
    let start = Instant::now();
    let shown = || start.elapsed() >= Duration::from_millis(400);
    let is_cancelled = || cancel.load(Ordering::Relaxed);
    let mut last = Instant::now() - Duration::from_millis(500);
    let mut had_error = false;
    let mut cancelled = false;
    let mut lock_notice: Option<String> = None;
    // First entry the copy had to step over, kept with its system error. Only
    // the first: a toast cannot list a folder's worth of refusals, and the file
    // that blocked first is the one the user has to act on.
    let mut first_skipped: Option<(PathBuf, String)> = None;

    // Breaks down into (kind, copy/move items, delete paths).
    // kind 2 = trash; kind 3 = explicit permanent deletion.
    let (kind, items, delete_paths): (u8, Vec<(PathBuf, PathBuf, bool)>, Vec<PathBuf>) = match work
    {
        Heavy::Copy(v) => (0, v, Vec::new()),
        Heavy::Move(v) => (1, v, Vec::new()),
        Heavy::Trash(v) => (2, Vec::new(), v),
        Heavy::PermanentDelete(v) => (3, Vec::new(), v),
    };

    if kind >= 2 {
        // Trash / permanent deletion — progress in items.
        let total = delete_paths.len();
        let mut done = 0usize;
        for p in &delete_paths {
            if is_cancelled() {
                cancelled = true;
                break;
            }
            let result = if kind == 3 {
                let outcome = ops::permanent_delete(p);
                if outcome.is_ok() {
                    deliver_op_event(
                        &weak,
                        &op_deliveries,
                        OpDelivery::PermanentlyDeleted(p.clone()),
                    );
                }
                outcome
            } else {
                match ops::trash_with_disposition(p) {
                    Ok(ops::TrashDisposition::Trashed) => {
                        deliver_op_event(&weak, &op_deliveries, OpDelivery::Trashed(p.clone()));
                        Ok(())
                    }
                    Ok(ops::TrashDisposition::PermanentlyDeleted) => {
                        // The trash was unavailable and the item went outright.
                        // Nothing brings it back, so its annotation goes too.
                        deliver_op_event(
                            &weak,
                            &op_deliveries,
                            OpDelivery::PermanentlyDeleted(p.clone()),
                        );
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            };
            if let Err(err) = result {
                had_error = true;
                error!(error = %err, path = %p.display(), permanent = kind == 3, "delete failed");
                // Diagnostic triggered ONLY after the failure: no probe,
                // enumeration, or Restart Manager session on the normal path.
                // A single toast is enough even for a multi-selection.
                if lock_notice.is_none() {
                    lock_notice = locked_item_notice(p, lang);
                }
            }
            done += 1;
            if last.elapsed() >= Duration::from_millis(80) || done == total {
                last = Instant::now();
                let pct = (done * 100).checked_div(total).unwrap_or(100);
                push_op(
                    &weak,
                    op_id,
                    OP_RUNNING,
                    shown(),
                    if total > 0 {
                        done as f32 / total as f32
                    } else {
                        1.0
                    },
                    labels.running.clone(),
                    format!("{done} / {total} {}", labels.items),
                    format!("{pct} %"),
                );
            }
        }
    } else {
        // Copy / move — progress in bytes after a pre-scan.
        push_op(
            &weak,
            op_id,
            OP_SCAN,
            shown(),
            0.0,
            labels.scanning.clone(),
            String::new(),
            String::new(),
        );
        let mut sizes = Vec::with_capacity(items.len());
        let mut total = 0u64;
        for (src, _, _) in &items {
            if is_cancelled() {
                cancelled = true;
                break;
            }
            let sz = ops::path_size(src);
            sizes.push(sz);
            total += sz;
            if last.elapsed() >= Duration::from_millis(80) {
                last = Instant::now();
                push_op(
                    &weak,
                    op_id,
                    OP_SCAN,
                    shown(),
                    0.0,
                    labels.scanning.clone(),
                    String::new(),
                    String::new(),
                );
            }
        }

        let mut done: u64 = 0;
        if !cancelled {
            for (i, (src, target, overwrite)) in items.iter().enumerate() {
                if is_cancelled() {
                    cancelled = true;
                    break;
                }
                // Overwrite ("Replace" resolution) → remove the existing
                // target BEFORE the operation, otherwise `rename`/`create_dir`
                // fail on an occupied target. Trash (recoverable) with a
                // fallback to permanent deletion. Both failing → skip
                // the item (never copy over a target that wasn't removed).
                if *overwrite && target.exists() {
                    if let Err(err) = ops::trash(target).or_else(|_| ops::permanent_delete(target))
                    {
                        had_error = true;
                        error!(error = %err, target = %target.display(), "overwrite: remove existing failed");
                        continue;
                    }
                    deliver_op_event(&weak, &op_deliveries, OpDelivery::Replaced(target.clone()));
                }
                let mut on_bytes = |delta: u64| {
                    done += delta;
                    if last.elapsed() >= Duration::from_millis(80) {
                        last = Instant::now();
                        let pct = (done.min(total) * 100).checked_div(total).unwrap_or(0) as u32;
                        push_op(
                            &weak,
                            op_id,
                            OP_RUNNING,
                            shown(),
                            if total > 0 {
                                (done as f32 / total as f32).min(1.0)
                            } else {
                                0.0
                            },
                            labels.running.clone(),
                            format!(
                                "{} / {}",
                                rfs::format_size(done, i18n::size_units(lang)),
                                rfs::format_size(total, i18n::size_units(lang))
                            ),
                            format!("{pct} %"),
                        );
                    }
                };
                if kind == 1 {
                    // Move: fast rename, otherwise copy+delete.
                    match std::fs::rename(src, target) {
                        Ok(()) => {
                            done += sizes.get(i).copied().unwrap_or(0);
                            deliver_op_event(
                                &weak,
                                &op_deliveries,
                                OpDelivery::Moved {
                                    from: src.clone(),
                                    to: target.clone(),
                                },
                            );
                        }
                        Err(_) => {
                            let mut skipped_here = 0_usize;
                            let copied = ops::copy_tree_progress(
                                src,
                                target,
                                &mut on_bytes,
                                &mut |path, err| {
                                    skipped_here += 1;
                                    error!(
                                        error = %err,
                                        path = %path.display(),
                                        "move: entry skipped"
                                    );
                                    if first_skipped.is_none() {
                                        first_skipped = Some((path.to_path_buf(), err.to_string()));
                                    }
                                },
                                &is_cancelled,
                            );
                            match copied {
                                Ok(ops::OpStatus::Cancelled) => cancelled = true,
                                // Part of the tree stayed behind: removing the
                                // source would destroy exactly what could not be
                                // copied. The move then stops at a copy, which
                                // the final message says.
                                Ok(ops::OpStatus::Done) if skipped_here > 0 => {
                                    had_error = true;
                                    // The source is deliberately kept, so the
                                    // item now exists in both places. Saying so
                                    // matters: the generic "skipped" message
                                    // would let the user believe the rest of the
                                    // move went through, when nothing moved at
                                    // all — the target only holds a copy.
                                    if lock_notice.is_none()
                                        && let Some((path, error)) = first_skipped.as_ref()
                                    {
                                        lock_notice = Some(i18n::move_source_kept(
                                            lang,
                                            &path_notice_name(src),
                                            &lock_reason(path, lang, error),
                                        ));
                                    }
                                }
                                Ok(ops::OpStatus::Done) => {
                                    if let Err(err) = ops::permanent_delete(src) {
                                        // The copy landed but the original stayed
                                        // behind: the move degenerated into a copy,
                                        // so the operation did NOT succeed. Reporting
                                        // it as done would leave two identical items
                                        // with no hint which one is authoritative.
                                        had_error = true;
                                        error!(
                                            error = %err,
                                            src = %src.display(),
                                            "move: delete source failed"
                                        );
                                        if lock_notice.is_none() {
                                            lock_notice = Some(move_source_kept_notice(
                                                src,
                                                lang,
                                                &err.to_string(),
                                            ));
                                        }
                                    } else {
                                        // Copied, then the original removed: the
                                        // move really happened.
                                        deliver_op_event(
                                            &weak,
                                            &op_deliveries,
                                            OpDelivery::Moved {
                                                from: src.clone(),
                                                to: target.clone(),
                                            },
                                        );
                                    }
                                }
                                Err(err) => {
                                    had_error = true;
                                    error!(error = %err, src = %src.display(), "move failed");
                                    if first_skipped.is_none() {
                                        first_skipped = Some((src.clone(), err.to_string()));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Copy.
                    let mut skipped_here = 0_usize;
                    let copied = ops::copy_tree_progress(
                        src,
                        target,
                        &mut on_bytes,
                        &mut |path, err| {
                            skipped_here += 1;
                            error!(
                                error = %err,
                                path = %path.display(),
                                "copy: entry skipped"
                            );
                            if first_skipped.is_none() {
                                first_skipped = Some((path.to_path_buf(), err.to_string()));
                            }
                        },
                        &is_cancelled,
                    );
                    match copied {
                        Ok(ops::OpStatus::Cancelled) => cancelled = true,
                        Ok(ops::OpStatus::Done) => {}
                        Err(err) => {
                            had_error = true;
                            error!(error = %err, src = %src.display(), "copy failed");
                            if first_skipped.is_none() {
                                first_skipped = Some((src.clone(), err.to_string()));
                            }
                        }
                    }
                    // Entries stepped over are failures too: the operation
                    // finishes, but it did not do everything it was asked.
                    if skipped_here > 0 {
                        had_error = true;
                    }
                }
                if cancelled {
                    break;
                }
            }
        }
    }

    // An entry stepped over is worth a message: the operation looks finished,
    // and only this says what did not make it through. A notice already set
    // (a move that kept its source) describes the same failure more precisely,
    // so it keeps the floor.
    if lock_notice.is_none()
        && let Some((path, error)) = first_skipped.as_ref()
    {
        lock_notice = Some(skipped_entry_notice(path, lang, error));
    }

    // Final state.
    let final_state = if cancelled {
        OP_CANCELLED
    } else if had_error {
        OP_ERROR
    } else {
        OP_SUCCESS
    };
    let final_visible = final_state == OP_ERROR || shown();
    let title = match final_state {
        OP_CANCELLED => labels.cancelled.clone(),
        OP_ERROR => labels.errors.clone(),
        _ => labels.done.clone(),
    };
    let weak2 = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak2.upgrade() {
            if final_visible {
                w.invoke_op_progress(OpProgress {
                    id: op_id,
                    state: final_state,
                    progress: 1.0,
                    indeterminate: false,
                    title: title.into(),
                    detail: SharedString::default(),
                    percent: if final_state == OP_SUCCESS {
                        SharedString::from("100 %")
                    } else {
                        SharedString::default()
                    },
                });
            } else {
                // Finished below the appearance threshold: make sure no row lingers.
                w.invoke_op_dismiss(op_id);
            }
            if let Some(message) = lock_notice {
                show_notice(&w, message);
            }
            // Releases the operation, which then schedules the re-listing of ALL
            // views (an op can involve 2 panels: drag-drop source+target, or the
            // same folder open in 2 views).
            w.invoke_op_finished(op_id);
        }
    });
}
