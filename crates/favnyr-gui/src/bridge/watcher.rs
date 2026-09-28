use super::*;

// ---------- Watcher notify ----------

/// Debouncing of watcher-triggered refreshes on the UI thread.
/// A single-shot timer is restarted on every event so that a
/// burst, for example during a copy or a multi-delete, produces
/// only a single listing. This coalescing is especially important over SMB.
pub(super) const WATCH_DEBOUNCE_MS: u64 = 300;
thread_local! {
    pub(super) static WATCH_DEBOUNCE: slint::Timer = slint::Timer::default();
}

/// Re-arms the displayed folder's watcher. Its creation and the call to `watch()`
/// run in the background, since opening a network handle can involve an
/// SMB reconnection. The `watcher_gen` generation invalidates in-flight installs:
/// only the most recent navigation can install its watcher, and an eject
/// prevents any handle from reappearing on the removed volume.
pub(super) fn install_watcher(state: &AppState, window: &MainWindow, path: &Path) {
    let weak = window.as_weak();
    let path_owned = path.to_path_buf();
    let r#gen = state.watcher_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let gen_ref = state.watcher_gen.clone();
    let event_gen_ref = gen_ref.clone();
    let slot = state.watcher.clone();
    let thumb_invalidations = state.thumb_invalidations.clone();

    std::thread::spawn(move || {
        let result: notify::Result<RecommendedWatcher> = RecommendedWatcher::new(
            move |res: notify::Result<Event>| match res {
                Ok(ev) => {
                    if matches!(
                        ev.kind,
                        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
                    ) {
                        thumb_invalidations
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .extend(ev.paths);
                        let weak = weak.clone();
                        let event_gen_ref = event_gen_ref.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            WATCH_DEBOUNCE.with(|t| {
                                t.start(
                                    slint::TimerMode::SingleShot,
                                    Duration::from_millis(WATCH_DEBOUNCE_MS),
                                    move || {
                                        let Some(w) = weak.upgrade() else { return };
                                        if event_gen_ref.load(Ordering::SeqCst) != r#gen {
                                            return; // watcher for a folder already left
                                        }
                                        // Suspends refreshes during a tab
                                        // drag, since rebuilding the TabItems would cancel
                                        // their pointer capture. A long operation also
                                        // suspends them, to avoid re-listing on every write;
                                        // `run_heavy` calls `refresh_all` at its end.
                                        if w.global::<crate::DragDropApi>().get_file_drag_active() {
                                            w.global::<crate::DragDropApi>()
                                                .set_file_drag_refresh_pending(true);
                                            return;
                                        }
                                        let busy =
                                            w.global::<crate::DragDropApi>().get_drag_active()
                                                || w.global::<crate::OperationsApi>().get_op_busy();
                                        if !busy {
                                            w.global::<crate::PanelsApi>().invoke_refresh();
                                        }
                                    },
                                );
                            });
                        });
                    }
                }
                Err(err) => warn!(error = %err, "watcher error"),
            },
            notify::Config::default(),
        );
        match result {
            Ok(mut w) => {
                if let Err(err) = w.watch(&path_owned, RecursiveMode::NonRecursive) {
                    warn!(error = %err, path = %path_owned.display(), "watcher watch() failed");
                } else if let Ok(mut guard) = slot.lock()
                    && gen_ref.load(Ordering::SeqCst) == r#gen
                {
                    // Replaces (and drops) the previous one HERE, on the
                    // background thread — its possible network handle doesn't block the UI.
                    *guard = Some(w);
                }
            }
            Err(err) => warn!(error = %err, "watcher creation failed"),
        }
    });
}
