use super::*;

/// ASYNCHRONOUS initial population of the panels. The window is displayed
/// immediately (empty panels); ONE background thread lists the current folder
/// of each panel (the ACTIVE one first — perceived priority) and delivers the
/// results to the UI thread via a channel + `invoke_from_event_loop` (the drain
/// callback has captured the non-Send `AppState`). Each delivery is applied
/// only if it's still FRESH (`Panel.pending_initial`, turned off by any
/// synchronous listing that occurred in the meantime: navigation, F5, watcher…).
pub(super) fn initial_populate_async(window: &MainWindow, state: &AppState) {
    // Jobs frozen on the UI thread (path + view settings of the active tab).
    let active = *state.active_panel.borrow();
    let mut jobs: Vec<(usize, PathBuf, bool, SortState, GroupMode)> = Vec::new();
    {
        let mut panels = state.panels.borrow_mut();
        for (i, p) in panels.iter_mut().enumerate() {
            p.pending_initial = true;
            let t = &p.tabs.tabs[p.tabs.active];
            jobs.push((
                i,
                t.current_path.clone(),
                t.show_hidden,
                t.sort,
                t.group_mode,
            ));
        }
    }
    if active < jobs.len() {
        let a = jobs.remove(active);
        jobs.insert(0, a);
    }
    // Empty panels pushed right away → the display no longer waits on the network.
    update_panels_ui(window, state);

    // Delivery: the worker PUSHES (idx, path, sorted result, denied?) then
    // wakes the UI thread. The (pure) sort is done on the worker; the conversion
    // to rows (`entry_to_row`: Slint images + icon cache) stays on the UI side.
    type Delivery = (
        usize,
        PathBuf,
        std::result::Result<(Vec<Entry>, usize), bool>,
    );
    let (tx, rx) = mpsc::channel::<Delivery>();
    {
        let st = state.clone();
        let weak = window.as_weak();
        let rx = Rc::new(RefCell::new(rx));
        window
            .global::<crate::PanelsApi>()
            .on_initial_listing_drain(move || {
                let Some(w) = weak.upgrade() else { return };
                while let Ok((idx, path, res)) = rx.borrow().try_recv() {
                    apply_initial_listing(&w, &st, idx, &path, res);
                }
            });
    }
    let weak = window.as_weak();
    std::thread::spawn(move || {
        for (idx, path, show_hidden, sort, group) in jobs {
            let res = match rfs::list_dir_counted(&path, show_hidden) {
                Ok((mut entries, hidden)) => {
                    rfs::sort(&mut entries, sort.column, sort.order, group);
                    Ok((entries, hidden))
                }
                Err(err) => {
                    error!(error = %err, path = %path.display(), "initial list_dir failed");
                    // Same distinction as refresh_listing: access denied → toast;
                    // otherwise → "unavailable" tab (banner + re-check).
                    Err(matches!(
                        &err,
                        favnyr_core::Error::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied
                    ))
                }
            };
            if tx.send((idx, path, res)).is_err() {
                return; // receiver gone (window closed)
            }
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<crate::PanelsApi>()
                        .invoke_initial_listing_drain();
                }
            });
        }
    });
}

/// Applies (UI thread) an initial listing delivered by the startup thread —
/// ONLY if the panel is still waiting for it (`pending_initial`) and its
/// active tab still points to the listed path.
pub(super) fn apply_initial_listing(
    window: &MainWindow,
    state: &AppState,
    idx: usize,
    path: &Path,
    res: std::result::Result<(Vec<Entry>, usize), bool>,
) {
    let (lang, compact_icon_rows) = {
        let config = state.config.borrow();
        (config.language, config.compact_icon_rows_in_preview)
    };
    let is_active = idx == *state.active_panel.borrow();
    {
        let mut panels = state.panels.borrow_mut();
        let Some(p) = panels.get_mut(idx) else { return };
        let tab = &p.tabs.tabs[p.tabs.active];
        if !p.pending_initial || tab.current_path != path {
            return; // stale delivery (the user has already navigated / listed)
        }
        let style = panel_row_style(p, compact_icon_rows);
        let group = tab.group_mode;
        let collapsed = tab.collapsed.clone();
        let subfolders = tab.subfolders;
        p.pending_initial = false;
        match res {
            Ok((entries, hidden_count)) => {
                install_rows(
                    p,
                    path,
                    entries,
                    style,
                    group,
                    &collapsed,
                    subfolders,
                    lang,
                    &annotations_now(state),
                    &state.clipboard.borrow(),
                    &[],
                    None,
                );
                p.hidden_count = hidden_count;
                p.unavailable = false;
            }
            Err(denied) => {
                *p.source.borrow_mut() = None;
                p.entry_count.set(0);
                p.grid_cols.set(0);
                p.replace_rows(Vec::new());
                p.hidden_count = 0;
                p.unavailable = !denied;
                if denied && is_active {
                    show_notice(window, i18n::access_denied(lang));
                }
            }
        }
        p.displayed_path = path.to_path_buf();
    }
    update_panels_ui(window, state);
    // Any delivery may belong to a simultaneously visible view: the
    // global preview request must therefore be rebuilt even if this panel
    // isn't active (its listing may have arrived after the active panel's).
    request_thumbnails(state);
    // A tab that shows subfolder contents restarts its scan on every fresh
    // listing: the previous one described the previous listing.
    request_subfolder_scan(state, idx);
    // ACTIVE panel: watcher + other background work tied to navigation.
    if is_active {
        install_watcher(state, window, path);
        request_folder_stats(state);
        request_imgmeta(state);
    }
}

/// Refreshes each panel after a potentially multi-view operation.
/// Local folders are re-read directly; any network path, even in
/// a non-active split, goes through the async worker. The active panel keeps the
/// full route (`refresh_listing`) for its filter and its watcher.
pub(super) fn refresh_all_panels(window: &MainWindow, state: &AppState) {
    let active_idx = *state.active_panel.borrow();
    let paths: Vec<PathBuf> = state
        .panels
        .borrow()
        .iter()
        .map(|panel| panel.tabs.tabs[panel.tabs.active].current_path.clone())
        .collect();

    // First re-read the local panels without publishing the UI between each one.
    for (index, path) in paths.iter().enumerate() {
        if index != active_idx
            && !favnyr_core::places::is_network_path(path)
            && !rfs::is_unc_path(path)
        {
            relist_panel(state, index);
        }
    }

    // The network panels then start independently. The function publishes
    // their "in progress" state but performs no I/O on the UI thread.
    for (index, path) in paths.iter().enumerate() {
        if index != active_idx
            && (favnyr_core::places::is_network_path(path) || rfs::is_unc_path(path))
        {
            request_async_panel_listing(window, state, index, path, String::new());
        }
    }

    let active = paths.get(active_idx).cloned().unwrap_or_default();
    if active.as_os_str().is_empty() {
        update_panels_ui(window, state);
        request_thumbnails(state);
        request_folder_stats(state);
        request_imgmeta(state);
    } else {
        let active_is_network =
            favnyr_core::places::is_network_path(&active) || rfs::is_unc_path(&active);
        refresh_listing(window, state, &active);
        if active_is_network {
            // The active listing will arrive later; the local panels already
            // re-read shouldn't wait on this network share for their visual work.
            request_thumbnails(state);
            request_folder_stats(state);
            request_imgmeta(state);
        }
    }
}

/// Re-lists ONE panel (its active tab) — used when an "unavailable"
/// folder becomes accessible again. Does NOT touch the watcher or the
/// thumbnails (the caller handles that for the active panel). Leaves the panel
/// unavailable if the listing still fails.
pub(super) fn relist_panel(state: &AppState, i: usize) {
    let (lang, compact_icon_rows) = {
        let config = state.config.borrow();
        (config.language, config.compact_icon_rows_in_preview)
    };
    let mut panels = state.panels.borrow_mut();
    let Some(panel) = panels.get_mut(i) else {
        return;
    };
    panel.pending_initial = false; // fresher than the initial population
    panel.pending_listing = false;
    panel.pending_select = None;
    let path = panel.tabs.tabs[panel.tabs.active].current_path.clone();
    let sort = panel.tabs.tabs[panel.tabs.active].sort;
    let show_hidden = panel.tabs.tabs[panel.tabs.active].show_hidden;
    let group_mode = panel.tabs.tabs[panel.tabs.active].group_mode;
    let collapsed = panel.tabs.tabs[panel.tabs.active].collapsed.clone();
    let subfolders = panel.tabs.tabs[panel.tabs.active].subfolders;
    let style = panel_row_style(panel, compact_icon_rows);
    let ext_on = panel.tabs.tabs[panel.tabs.active].ext_filter_on;
    let ext_txt = panel.tabs.tabs[panel.tabs.active].ext_filter.clone();
    let same_dir = panel.displayed_path == path;
    if !same_dir {
        panel.reset_rows_viewport();
    }
    match rfs::list_dir_counted(&path, show_hidden) {
        Ok((mut entries, hidden_count)) => {
            rfs::sort(&mut entries, sort.column, sort.order, group_mode);
            apply_ext_filter(&mut entries, ext_on, &ext_txt);
            let (selected, anchor) = if same_dir {
                preserved_selection_of(panel)
            } else {
                (Vec::new(), None)
            };
            install_rows(
                panel,
                &path,
                entries,
                style,
                group_mode,
                &collapsed,
                subfolders,
                lang,
                &annotations_now(state),
                &state.clipboard.borrow(),
                &selected,
                anchor.as_deref(),
            );
            panel.hidden_count = hidden_count;
            panel.unavailable = false;
            panel.displayed_path = path;
        }
        Err(err) => {
            let denied = matches!(
                &err,
                favnyr_core::Error::Io(io)
                    if io.kind() == std::io::ErrorKind::PermissionDenied
            );
            error!(error = %err, path = %path.display(), "panel relist failed");
            *panel.source.borrow_mut() = None;
            panel.entry_count.set(0);
            panel.grid_cols.set(0);
            panel.replace_rows(Vec::new());
            let active = panel.tabs.active;
            panel.tabs.tabs[active].selection_anchor = -1;
            panel.tabs.tabs[active].cursor = -1;
            panel.hidden_count = 0;
            panel.unavailable = !denied;
            panel.displayed_path = path;
        }
    }
    drop(panels);
    // A tab showing subfolder contents re-reads them along with the folder.
    request_subfolder_scan(state, i);
}

/// Re-checks "unavailable" panels (network/missing folder): those whose
/// path has BECOME accessible again are re-listed and the banner disappears.
/// Called from the Slint poll, gated by a drive change (non-blocking:
/// `is_dir` is only tested if a mount changed, see `on_recheck_unavailable`).
pub(super) fn recheck_unavailable_panels(window: &MainWindow, state: &AppState) {
    let recovered: Vec<usize> = {
        let panels = state.panels.borrow();
        panels
            .iter()
            .enumerate()
            .filter(|(_, p)| p.unavailable)
            .filter(|(_, p)| p.tabs.tabs[p.tabs.active].current_path.is_dir())
            .map(|(i, _)| i)
            .collect()
    };
    if recovered.is_empty() {
        return;
    }
    for &i in &recovered {
        relist_panel(state, i);
    }
    update_panels_ui(window, state);
    // A recovered panel stays visible even if it doesn't have focus.
    request_thumbnails(state);
    // The active panel has recovered → re-arms its watcher and its other work.
    let active = *state.active_panel.borrow();
    if recovered.contains(&active) {
        let path = state.with_tabs(|book| book.tabs[book.active].current_path.clone());
        install_watcher(state, window, &path);
        request_folder_stats(state);
        request_imgmeta(state);
    }
}

/// Called when the active panel changes: refreshes the view on the new
/// active panel's current path (the model rebinding is implicit,
/// via update_panels_ui which pushes the updated PanelViews).
pub(super) fn switch_active_panel(window: &MainWindow, state: &AppState) {
    let path = state.with_tabs(|book| book.tabs[book.active].current_path.clone());
    refresh_listing(window, state, &path);
}

#[derive(Clone, Copy)]
pub(super) enum NavAction {
    Back,
    Forward,
    Parent,
    Home,
    Refresh,
}

pub(super) fn install_nav_callback(window: &MainWindow, state: &AppState, action: NavAction) {
    let st = state.clone();
    let weak = window.as_weak();
    let cb = move || {
        let Some(w) = weak.upgrade() else { return };
        match action {
            NavAction::Back => {
                // Folder we're LEAVING: if it's a direct child of the target, we
                // re-select it there ("where we came from", like Explorer).
                let left = st.current_path();
                let target = st.with_tabs_mut(|book| {
                    let a = book.active;
                    book.tabs[a].history.back()
                });
                if let Some(p) = target {
                    load_directory(&w, &st, &p, false);
                    select_child_from(&w, &st, &p, &left);
                }
            }
            NavAction::Forward => {
                let target = st.with_tabs_mut(|book| {
                    let a = book.active;
                    book.tabs[a].history.forward()
                });
                if let Some(p) = target {
                    load_directory(&w, &st, &p, false);
                }
            }
            NavAction::Parent => {
                let cur = st.current_path();
                // `Path::parent()`, or server root for a share root
                // `\\HOST\share`, for which `Path::parent()` returns `None`.
                let target = cur
                    .parent()
                    .map(Path::to_path_buf)
                    .or_else(|| rfs::unc_share_parent(&cur));
                if let Some(target) = target {
                    load_directory(&w, &st, &target, true);
                    // We select there the folder we just left.
                    select_child_from(&w, &st, &target, &cur);
                }
            }
            NavAction::Home => {
                let home = home_dir();
                load_directory(&w, &st, &home, true);
            }
            NavAction::Refresh => {
                // F5 = force a refresh: clears the recursive mtime + size caches
                // → recompute. Navigation, on the other hand, keeps them (no flicker).
                if !drain_thumbnail_invalidations(&st) {
                    // A manual F5 has no watcher paths to target. Treat it as
                    // an explicit request to refresh content textures in the
                    // active view while leaving every other cached folder hot.
                    let paths = active_thumbnail_paths(&st);
                    invalidate_thumbnail_paths(&st, &paths);
                }
                st.rmtime_cache.borrow_mut().clear();
                st.size_cache.borrow_mut().clear();
                let cur = st.current_path();
                if !cur.as_os_str().is_empty() {
                    refresh_listing(&w, &st, &cur);
                }
            }
        }
    };
    match action {
        NavAction::Back => window.global::<crate::PanelsApi>().on_go_back(cb),
        NavAction::Forward => window.global::<crate::PanelsApi>().on_go_forward(cb),
        NavAction::Parent => window.global::<crate::PanelsApi>().on_go_parent(cb),
        NavAction::Home => window.global::<crate::PanelsApi>().on_go_home(cb),
        NavAction::Refresh => window.global::<crate::PanelsApi>().on_refresh(cb),
    }
}

// ---------- Navigation and listing ----------

pub(super) fn load_directory(
    window: &MainWindow,
    state: &AppState,
    target: &Path,
    push_history: bool,
) {
    // Any navigation resets the active view's "type-ahead" filter.
    state.filter.borrow_mut().clear();
    window
        .global::<crate::PanelsApi>()
        .set_active_filter(SharedString::new());
    // The exact Flickable isn't recreated on every listing: we therefore explicitly
    // request scrolling back to top for any new navigation context,
    // including two distinct tabs pointing to the same folder.
    let active_panel = *state.active_panel.borrow();
    if let Some(panel) = state.panels.borrow().get(active_panel) {
        panel.reset_rows_viewport();
    }
    if push_history {
        state.with_tabs_mut(|book| {
            let a = book.active;
            book.tabs[a].history.push(target.to_path_buf());
        });
    }
    refresh_listing(window, state, target);
}
