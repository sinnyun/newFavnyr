use super::*;

/// Connection target for the network credentials prompt: a
/// `\\HOST` server root authenticates via `\\HOST\IPC$`; otherwise the
/// resource is connected as-is (`\\HOST\share`).
pub(super) fn net_connect_target(path: &Path) -> String {
    let s = path.to_string_lossy();
    if rfs::unc_server_root(path).is_some() {
        format!(r"{}\IPC$", s.trim_end_matches('\\'))
    } else {
        s.into_owned()
    }
}

/// Is a listing result an "access denied"? (→ attempt a network login.)
pub(super) fn listing_denied(r: &Result<(Vec<Entry>, usize), favnyr_core::Error>) -> bool {
    matches!(
        r,
        Err(favnyr_core::Error::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied
    )
}

/// Routes network paths to a worker. Local listings keep the
/// proven synchronous path, limiting the behavior change to the only
/// I/O likely to block for a long time (SMB, mapped drive, UNC).
pub(super) fn refresh_listing(window: &MainWindow, state: &AppState, path: &Path) {
    if favnyr_core::places::is_network_path(path) || rfs::is_unc_path(path) {
        let panel_idx = *state.active_panel.borrow();
        let name_filter = state.filter.borrow().clone();
        request_async_panel_listing(window, state, panel_idx, path, name_filter);
    } else {
        refresh_listing_sync(window, state, path);
    }
}

pub(super) fn request_async_panel_listing(
    window: &MainWindow,
    state: &AppState,
    panel_idx: usize,
    path: &Path,
    name_filter: String,
) {
    let path = path.to_path_buf();
    let lang = state.snapshot_config().language;

    let (
        same_dir,
        show_hidden,
        sort,
        group,
        collapsed,
        ext_on,
        ext_text,
        preserved_selected,
        preserved_anchor,
    ) = {
        let panels = state.panels.borrow();
        let Some(panel) = panels.get(panel_idx) else {
            return;
        };
        let tab = &panel.tabs.tabs[panel.tabs.active];
        let same_dir = panel.displayed_path == path;
        let model = panel.rows_model.clone();
        let selected = if same_dir {
            selected_paths_of(&*model)
        } else {
            Vec::new()
        };
        let anchor = if same_dir {
            anchor_path_of(&*model, tab.selection_anchor)
        } else {
            None
        };
        (
            same_dir,
            tab.show_hidden,
            tab.sort,
            tab.group_mode,
            tab.collapsed.clone(),
            tab.ext_filter_on,
            tab.ext_filter.clone(),
            selected,
            anchor,
        )
    };

    let r#gen = state.listing_serial.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut panels = state.panels.borrow_mut();
        let Some(panel) = panels.get_mut(panel_idx) else {
            return;
        };
        panel.pending_initial = false;
        panel.pending_listing = true;
        panel.listing_gen = r#gen;
        panel.pending_select = None;
        panel.displayed_path = path.clone();
        panel.unavailable = false;
        let active = panel.tabs.active;
        panel.tabs.tabs[active].current_path = path.clone();
        if !same_dir {
            panel.reset_rows_viewport();
            panel.replace_rows(Vec::new());
            panel.hidden_count = 0;
            panel.tabs.tabs[active].selection_anchor = -1;
        }
    }
    // Only the active panel has a watcher. Refreshing a non-active
    // split must not invalidate the one used by the active view.
    if panel_idx == *state.active_panel.borrow() {
        state.watcher_gen.fetch_add(1, Ordering::SeqCst);
    }
    update_panels_ui(window, state);

    let queue = state.async_listings.clone();
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let mut listing = rfs::list_dir_counted(&path, show_hidden);
        if listing_denied(&listing)
            && rfs::is_unc_path(&path)
            && favnyr_core::places::net_connect_prompt(&net_connect_target(&path))
        {
            listing = rfs::list_dir_counted(&path, show_hidden);
        }
        let result = match listing {
            Ok((mut entries, hidden)) => {
                rfs::sort(&mut entries, sort.column, sort.order, group);
                apply_name_filter(&mut entries, &name_filter);
                if !ext_text.is_empty() || ext_on {
                    apply_ext_filter(&mut entries, ext_on, &ext_text);
                }
                Ok((entries, hidden))
            }
            Err(err) => {
                let denied = matches!(
                    &err,
                    favnyr_core::Error::Io(e)
                        if e.kind() == std::io::ErrorKind::PermissionDenied
                );
                error!(error = %err, path = %path.display(), "async network list_dir failed");
                Err(denied)
            }
        };
        let delivery = AsyncListingDelivery {
            panel: panel_idx,
            r#gen,
            path,
            result,
            lang,
            collapsed,
            preserved_selected,
            preserved_anchor,
        };
        let queued = queue
            .lock()
            .map(|mut pending| pending.push_back(delivery))
            .is_ok();
        if queued {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    w.invoke_async_listing_drain();
                }
            });
        }
    });
}

pub(super) fn apply_async_listing(
    window: &MainWindow,
    state: &AppState,
    delivery: AsyncListingDelivery,
) {
    let is_active = delivery.panel == *state.active_panel.borrow();
    let compact_icon_rows = state.config.borrow().compact_icon_rows_in_preview;
    let mut denied_notice = false;
    let mut count = 0usize;
    let pending_focus = {
        let mut panels = state.panels.borrow_mut();
        let Some(panel) = panels.get_mut(delivery.panel) else {
            return;
        };
        let active = panel.tabs.active;
        if !async_listing_is_current(panel, delivery.r#gen, &delivery.path) {
            return; // stale response: a more recent navigation has won
        }
        let style = panel_row_style(panel, compact_icon_rows);
        let group = panel.tabs.tabs[active].group_mode;
        let subfolders = panel.tabs.tabs[active].subfolders;
        panel.pending_listing = false;
        let pending_select = panel.pending_select.take();
        let pending_focus = pending_select.clone();
        match delivery.result {
            Ok((entries, hidden_count)) => {
                // A focus request (upward navigation) replaces the preserved
                // selection: exactly the child left behind stays selected. It
                // arrives as a name, relative to the folder being listed.
                let focus_path = pending_select
                    .as_deref()
                    .map(|name| delivery.path.join(name));
                let anchor = focus_path
                    .clone()
                    .or_else(|| delivery.preserved_anchor.clone());
                let preserved: &[PathBuf] = match &focus_path {
                    Some(path) => std::slice::from_ref(path),
                    None => &delivery.preserved_selected,
                };
                install_rows(
                    panel,
                    &delivery.path,
                    entries,
                    style,
                    group,
                    &delivery.collapsed,
                    subfolders,
                    delivery.lang,
                    &annotations_now(state),
                    &state.clipboard.borrow(),
                    preserved,
                    anchor.as_deref(),
                );
                count = panel.entry_count.get();
                panel.hidden_count = hidden_count;
                panel.unavailable = false;
            }
            Err(denied) => {
                *panel.source.borrow_mut() = None;
                panel.entry_count.set(0);
                panel.grid_cols.set(0);
                panel.replace_rows(Vec::new());
                panel.tabs.tabs[active].selection_anchor = -1;
                panel.tabs.tabs[active].cursor = -1;
                panel.hidden_count = 0;
                panel.unavailable = !denied;
                denied_notice = denied && is_active;
            }
        }
        panel.displayed_path = delivery.path.clone();
        pending_focus
    };
    if denied_notice {
        show_notice(window, i18n::access_denied(delivery.lang));
    }
    update_panels_ui(window, state);
    if let Some(name) = pending_focus {
        schedule_focus_entry_by_name(window, state, delivery.path.clone(), name);
    }
    // The panel may have become inactive during the network listing while
    // remaining visible in the split: its previews must still start.
    request_thumbnails(state);
    request_folder_stats(state);
    request_imgmeta(state);
    request_subfolder_scan(state, delivery.panel);
    if is_active {
        install_watcher(state, window, &delivery.path);
    }
    debug!(path = %delivery.path.display(), count, "async network listing applied");
}

pub(super) fn async_listing_is_current(panel: &Panel, r#gen: u64, path: &Path) -> bool {
    panel.pending_listing
        && panel.listing_gen == r#gen
        && panel
            .tabs
            .tabs
            .get(panel.tabs.active)
            .is_some_and(|tab| tab.current_path == path)
}

pub(super) fn refresh_listing_sync(window: &MainWindow, state: &AppState, path: &Path) {
    let cfg = state.snapshot_config();
    let lang = cfg.language;

    // Same-dir = the active panel's `displayed_path` hasn't changed.
    let active_idx = *state.active_panel.borrow();
    let same_dir = {
        let mut panels = state.panels.borrow_mut();
        // Synchronous listing is fresher than the deferred initial population.
        panels[active_idx].pending_initial = false;
        panels[active_idx].pending_listing = false;
        panels[active_idx].pending_select = None;
        let same_dir = panels[active_idx].displayed_path == path;
        if !same_dir {
            panels[active_idx].reset_rows_viewport();
        }
        same_dir
    };

    // Preservation: we read from the active panel's model if same_dir.
    let (preserved_selected, preserved_anchor) = if same_dir {
        let panels = state.panels.borrow();
        panels
            .get(active_idx)
            .map(preserved_selection_of)
            .unwrap_or_default()
    } else {
        (Vec::new(), None)
    };

    let show_hidden = state.with_tabs(|book| book.tabs[book.active].show_hidden);
    // Local listing only. `refresh_listing` routes every network path to the
    // asynchronous version, which is where the system credentials prompt and
    // its retry live — a copy of them here could never run, since the path is
    // local by construction, and a blocking prompt has no business on this
    // thread anyway.
    let (mut entries, hidden_count) = match rfs::list_dir_counted(path, show_hidden) {
        Ok(ec) => ec,
        Err(err) => {
            error!(error = %err, path = %path.display(), "list_dir failed");
            // An access denial on a protected folder produces a notification
            // instead of being presented as an empty list. Detected via io::Error.
            let denied = matches!(
                &err,
                favnyr_core::Error::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied
            );
            // Everything that ISN'T an access denial (missing path, network drive
            // not started, mount gone) → "unavailable" tab: we keep the
            // path + a persistent banner + auto re-check.
            let unavailable = !denied;
            if denied {
                show_notice(window, i18n::access_denied(lang));
            }
            {
                let panels = state.panels.borrow();
                let panel = &panels[active_idx];
                *panel.source.borrow_mut() = None;
                panel.entry_count.set(0);
                panel.grid_cols.set(0);
                panel.replace_rows(Vec::new());
            }
            state.with_tabs_mut(|book| {
                let a = book.active;
                book.tabs[a].selection_anchor = -1;
                book.tabs[a].current_path = path.to_path_buf();
            });
            {
                let mut panels = state.panels.borrow_mut();
                panels[active_idx].displayed_path = path.to_path_buf();
                panels[active_idx].hidden_count = 0;
                panels[active_idx].unavailable = unavailable;
            }
            install_watcher(state, window, path);
            update_panels_ui(window, state);
            return;
        }
    };

    let sort_state = state.with_tabs(|book| {
        let a = book.active;
        book.tabs[a].sort
    });
    let (group_mode, collapsed, subfolders) = state.with_tabs(|book| {
        let tab = &book.tabs[book.active];
        (tab.group_mode, tab.collapsed.clone(), tab.subfolders)
    });
    let style = {
        let panels = state.panels.borrow();
        panels
            .get(active_idx)
            .map(|panel| panel_row_style(panel, cfg.compact_icon_rows_in_preview))
            .unwrap_or(RowStyle {
                mode: ViewMode::List,
                zoom: LIST_DEFAULT_ZOOM,
                compact_icon_rows: cfg.compact_icon_rows_in_preview,
                width: 0.0,
            })
    };
    rfs::sort(
        &mut entries,
        sort_state.column,
        sort_state.order,
        group_mode,
    );

    // Active view's "type-ahead" filter: searches for the fragment anywhere
    // in the name (case-insensitive).
    apply_name_filter(&mut entries, &state.filter.borrow());
    // Extension filter — orthogonal to type-ahead; doesn't touch
    // folders. Read from the active tab.
    {
        let (on, txt) = state.with_tabs(|book| {
            let a = book.active;
            (book.tabs[a].ext_filter_on, book.tabs[a].ext_filter.clone())
        });
        apply_ext_filter(&mut entries, on, &txt);
    }

    let count = {
        let mut panels = state.panels.borrow_mut();
        let Some(panel) = panels.get_mut(active_idx) else {
            return;
        };
        install_rows(
            panel,
            path,
            entries,
            style,
            group_mode,
            &collapsed,
            subfolders,
            lang,
            &annotations_now(state),
            &state.clipboard.borrow(),
            &preserved_selected,
            preserved_anchor.as_deref(),
        )
    };

    state.with_tabs_mut(|book| {
        let a = book.active;
        book.tabs[a].current_path = path.to_path_buf();
    });
    {
        let mut panels = state.panels.borrow_mut();
        panels[active_idx].displayed_path = path.to_path_buf();
        panels[active_idx].hidden_count = hidden_count;
        panels[active_idx].unavailable = false; // listing OK → available again
    }

    install_watcher(state, window, path);
    update_panels_ui(window, state);
    request_thumbnails(state);
    request_folder_stats(state);
    request_imgmeta(state);
    request_subfolder_scan(state, active_idx);

    debug!(path = %path.display(), count, "listing refreshed");
}

/// How many entries went, in the reader's own grammar.
///
/// The project already tells a singular from a plural for the footer; this
/// message had been left saying "1 entries removed".
pub(super) fn annotations_cleaned_text(lang: favnyr_core::i18n::Lang, removed: usize) -> String {
    if removed == 1 {
        i18n::tr(lang, "settings_annotations_cleaned_one")
    } else {
        i18n::tr(lang, "settings_annotations_cleaned").replace("{count}", &removed.to_string())
    }
}

/// The badge beside the "Clean up" button, from the snapshot.
pub(super) fn push_orphan_count(window: &MainWindow, state: &AppState) {
    window.set_annotation_orphans(i32::try_from(state.orphans.borrow().len()).unwrap_or(i32::MAX));
}

/// Splits a path into the folder that still exists and the name that does not.
///
/// The folder being there is the very rule that made this an orphan, so it is
/// context rather than the subject: the view shows it dimmed, ahead of the name.
pub(super) fn orphan_parts(path: &str) -> (String, String) {
    let path = Path::new(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            let mut shown = parent.to_string_lossy().into_owned();
            if !shown.ends_with(std::path::MAIN_SEPARATOR) {
                shown.push(std::path::MAIN_SEPARATOR);
            }
            shown
        }
        _ => String::new(),
    };
    (parent, name)
}

/// Mirrors the snapshot and the tick set into the view, with the count already
/// interpolated into the confirming button — where the number sits in that
/// sentence differs from one language to the next.
pub(super) fn push_orphan_rows(window: &MainWindow, state: &AppState) {
    let chosen = state.orphan_selection.borrow();
    let rows: Vec<OrphanRow> = state
        .orphans
        .borrow()
        .iter()
        .map(|orphan| {
            let (parent, name) = orphan_parts(&orphan.path);
            OrphanRow {
                key: orphan.path.as_str().into(),
                parent: parent.into(),
                name: name.into(),
                note: orphan.note.as_str().into(),
                slot: i32::from(orphan.color),
                checked: chosen.contains(&orphan.path),
            }
        })
        .collect();
    let picked = chosen.len();
    drop(chosen);
    window.set_orphan_rows(ModelRc::new(VecModel::from(rows)));
    window.set_orphans_checked(i32::try_from(picked).unwrap_or(i32::MAX));
    let lang = state.config.borrow().language;
    window.set_orphans_confirm_label(
        i18n::tr(lang, "annotations_cleanup_confirm")
            .replace("{count}", &picked.to_string())
            .into(),
    );
}
