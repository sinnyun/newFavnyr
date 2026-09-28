use super::*;

/// Requests the "show subfolder contents" scan of a panel: one listing per
/// direct subfolder, on a background thread. Called when the feature is turned
/// on and after every fresh listing of a tab that uses it; the sections already
/// exist (pending) by the time the results come back, so they only get filled.
pub(in crate::bridge) fn request_subfolder_scan(state: &AppState, panel_idx: usize) {
    let job = {
        let panels = state.panels.borrow();
        let Some(panel) = panels.get(panel_idx) else {
            return;
        };
        let tab = &panel.tabs.tabs[panel.tabs.active];
        if !tab.subfolders {
            return;
        }
        let source = panel.source.borrow();
        let Some(source) = source.as_ref() else {
            return;
        };
        // One level only: the direct subfolders of the listing.
        let dirs: Vec<(String, PathBuf)> = source
            .own
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| (entry.name.clone(), source.root.join(&entry.name)))
            .collect();
        if dirs.is_empty() {
            return;
        }
        SubScanJob {
            panel: panel_idx,
            sub_gen: panel.sub_gen.get(),
            root: source.root.clone(),
            dirs,
            sort: (tab.sort.column, tab.sort.order),
            group: subfolder_group(tab.group_mode),
            show_hidden: tab.show_hidden,
        }
    };
    let _ = state.subscan_tx.send(job);
}

/// Sort criterion used INSIDE a subfolder section. The category grouping has no
/// header of its own down there, so its rank order would read as noise: a
/// subfolder keeps the folders-first ordering instead.
pub(in crate::bridge) fn subfolder_group(group: GroupMode) -> GroupMode {
    if group == GroupMode::Category {
        GroupMode::FoldersFirst
    } else {
        group
    }
}

/// Reads every direct subfolder of one job, then hands the result to the UI
/// thread through the shared queue. A subfolder that cannot be read yields an
/// empty section rather than failing the whole scan.
pub(in crate::bridge) fn spawn_subscan_worker(
    rx: mpsc::Receiver<SubScanJob>,
    queue: Arc<std::sync::Mutex<VecDeque<SubScanDelivery>>>,
    weak: slint::Weak<MainWindow>,
) {
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let mut dirs = Vec::with_capacity(job.dirs.len());
            for (name, path) in &job.dirs {
                let mut entries = match rfs::list_dir_counted(path, job.show_hidden) {
                    Ok((entries, _hidden)) => entries,
                    Err(err) => {
                        debug!(error = %err, path = %path.display(), "subfolder scan failed");
                        Vec::new()
                    }
                };
                rfs::sort(&mut entries, job.sort.0, job.sort.1, job.group);
                dirs.push(SubFolder {
                    name: name.clone(),
                    path: path.to_path_buf(),
                    entries,
                    pending: false,
                });
            }
            let delivery = SubScanDelivery {
                panel: job.panel,
                sub_gen: job.sub_gen,
                root: job.root,
                dirs,
            };
            let queued = queue
                .lock()
                .map(|mut pending| pending.push_back(delivery))
                .is_ok();
            if queued {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_subfolders_drain();
                    }
                });
            }
        }
    });
}

/// Fills the pending subfolder sections of a panel with a finished scan. A
/// delivery whose view has navigated, re-listed, or turned the feature off is
/// dropped: it describes a listing that is no longer on screen.
pub(in crate::bridge) fn apply_subfolder_scan(
    window: &MainWindow,
    state: &AppState,
    delivery: SubScanDelivery,
) {
    let compact = state.config.borrow().compact_icon_rows_in_preview;
    let lang = state.config.borrow().language;
    {
        let mut panels = state.panels.borrow_mut();
        let Some(panel) = panels.get_mut(delivery.panel) else {
            return;
        };
        let active = panel.tabs.active;
        if panel.sub_gen.get() != delivery.sub_gen || !panel.tabs.tabs[active].subfolders {
            return;
        }
        {
            let mut source = panel.source.borrow_mut();
            let Some(source) = source.as_mut() else {
                return;
            };
            if source.root != delivery.root {
                return;
            }
            source.dirs = delivery.dirs;
        }
        // The rows are rebuilt from the filled source: selection and cut marks
        // come back by path, section folds are preserved as they are.
        rebuild_panel_rows(
            panel,
            lang,
            compact,
            &annotations_now(state),
            &state.clipboard.borrow(),
        );
    }
    debug!(panel = delivery.panel, "subfolder scan applied");
    update_panels_ui(window, state);
    request_thumbnails(state);
}
