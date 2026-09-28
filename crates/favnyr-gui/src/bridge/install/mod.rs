use super::*;

pub fn install(window: &MainWindow, state: AppState) {
    // (The row model of the active panel no longer needs to be pushed as a
    // global `rows` — each panel carries its own model in
    // its PanelView via update_panels_ui.)

    // Initialization from config.
    window.on_filename_caret_offset(|name: SharedString, is_dir: bool| {
        filename_caret_offset(name.as_str(), is_dir)
    });
    let cfg = state.snapshot_config();
    apply_language(window, cfg.language);
    // One toast row per running operation, owned by the state so that the
    // worker threads can address a row by its operation id.
    window.set_ops(state.ops_model.clone().into());
    window.set_closed_tabs_available(state.has_closed_tabs());
    // The "Open with…" entry (native picker) only exists on Windows.
    window.set_platform_windows(cfg!(windows));
    window.set_platform_linux(cfg!(target_os = "linux"));
    window.set_theme_pref(match cfg.theme {
        Theme::Auto => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    });
    // Default tab bar position (settings).
    window.set_tabbar_default_pref(cfg.default_tab_bar_mode.min(2) as i32);
    // Tab path tooltip (settings) — unchecked by default.
    window.set_tab_tooltip_enabled(cfg.tab_path_tooltip);
    // "Unsaved changes" guard before loading, checked by
    // default in settings.
    window.set_ws_warn_unsaved(cfg.warn_unsaved_workspace);
    // Hybrid Previews mode: single-icon types stay compact.
    window.set_compact_preview_rows_enabled(cfg.compact_icon_rows_in_preview);
    // Timezone for the "Modified" column (settings): 0 = local time
    // (default), 1 = UTC. The effective offset is cached for the rows.
    window.set_clock_utc_pref(if cfg.clock_utc { 1 } else { 0 });
    refresh_mtime_offset(&state);
    // Windows shell context menu (shell extensions) — checked by default.
    window.set_shell_menu_enabled(cfg.shell_ctx_menu);
    // Left panel: any positive state opens the unified Places +
    // Favorites panel. This normalization also accepts the serialized value 2.
    window.set_left_panel(if cfg.left_panel >= 1 { 1 } else { 0 });
    window.set_sidebar_width(cfg.sidebar_width.max(140) as f32);
    push_sidebar_sections_ui(window, &state);
    refresh_sidebar(window, &state);
    // The Slint chevrons immediately update the live workspace state.
    // The title then re-reads the single dirty source; the Workspaces panel
    // already resynchronizes each time it opens via `ws-refresh`.
    install_sidebar_section_state_changed(window, state.clone());
    // The target is the FINAL index (0..3) among the four sections. The order is
    // a GLOBAL preference: a single config write on drop, no
    // workspace snapshot change or churn during the moves.
    install_sidebar_section_reordered(window, state.clone());
    // Initial drives signature, so the first poll doesn't trigger
    // an unnecessary rebuild.
    state.last_drives_sig.set(sidebar_drives_signature());
    // The Shell/WPD can block on a slow driver: the initial phone scan
    // only starts after the initial snapshot and off the UI thread.
    // Its revision will cause the sidebar to rebuild on the next poll if needed.
    #[cfg(windows)]
    crate::winportable::request_refresh();
    push_favorites_ui(window, &state);
    push_openers_ui(window, &state);
    push_recipes_ui(window, &state);
    install_sidebar_place_clicked(window, state.clone());
    // Middle-click on a sidebar shortcut / drive → opens the target
    // in a NEW tab of the active view (same pattern as `on_action_open_new_tab`).
    install_place_open_new_tab(window, state.clone());
    // Re-scan of places (mounted/unmounted volumes) when the sidebar opens.
    install_sidebar_refresh(window, state.clone());
    // Cross-instance IPC initialization: called by a Slint Timer shortly
    // after startup (the window exists → HWND available), RETRIED as long as
    // the window isn't found (returns `true` = initialized, the Timer
    // stops). Marks + subclasses the window to receive tabs
    // transferred from another Favnyr instance. No-op outside Windows.
    install_init_ipc(window, state.clone());
    // Scrolling of a panel's tab bar: the view reports
    // `tabs-flick.viewport-x` (plain) on every change → used for the hit-test of the
    // insertion gap for a tab received from another instance.
    install_panel_tabs_scrolled(window, state.clone());
    // LIGHTWEIGHT periodic poll of drives while the "Drives" sidebar is
    // open: external changes (subst, USB, `net use`, mount)
    // don't always emit a system event → we compare the signature and
    // only rebuild the model if it has moved. See the Timer on the Slint side.
    install_poll_drives(window, state.clone());
    // Re-check of unavailable tabs after a drive change.
    // `drives_signature` avoids a potentially slow network `is_dir` as long
    // as no mount has changed. The Slint timer stays active if needed.
    install_recheck_unavailable(window, state.clone());

    // A swatch was clicked in the "Colour & note" flyout.
    install_folder_color_picked(window, state.clone());

    // Opening the note editor: the menu row knows neither the item's name nor
    // the note already stored, so the bridge fills both before showing it.
    install_open_comment_editor(window, state.clone());
    install_comment_confirmed(window, state.clone());

    // Eject / network disconnect -----
    // These operations can block (unmounting, power loss, network
    // I/O) → background thread, then back to the UI (toast + sidebar re-scan).
    window.on_place_copy_path(move |path: SharedString| {
        if let Err(err) = crate::actions::copy_to_clipboard(&path) {
            warn!(error = %err, "copy path failed");
        }
    });
    install_drive_eject(window, state.clone());
    install_drive_disconnect(window, state.clone());

    // Tree favorites -----
    // Click on a row: container → collapses/expands; favorite → new tab.
    install_fav_row_activate(window, state.clone());
    // Drop a navigable sidebar item on a tab gap or panel zone. Exact gaps
    // insert at that position; a panel center appends a tab; an edge creates a
    // split. Favorite files keep the historical gap behavior (their parent is
    // opened) but only favorite directories may target a panel zone.
    install_sidebar_item_dropped(window, state.clone());
    // Middle click: opens a favorite in a new tab.
    install_fav_row_middle(window, state.clone());
    // Open a favorite (context menu).
    install_fav_open(window, state.clone());
    // Container: open ALL descendant favorites (one tab each).
    install_fav_open_all(window, state.clone());
    // Naming or renaming a container/favorite via a modal popup. Validation
    // reuses `ops::is_valid_entry_name` (empty, separators, "." and "..").
    install_fav_name_check(window);
    // Confirms the popup: creates a (sub-)container or renames a node, with the
    // same protections as the New folder / Rename popups.
    install_fav_name_confirm(window, state.clone());
    // Collapse all.
    install_fav_collapse_all(window, state.clone());
    // Expand all.
    install_fav_expand_all(window, state.clone());
    // Delete a node (+ descendants).
    install_fav_delete(window, state.clone());
    // Copy a favorite's path to the clipboard.
    install_fav_copy_path(window, state.clone());
    // Save the current tab as a favorite (popup).
    install_fav_save_tab(window, state.clone());
    // Save ALL tabs of the view as favorites (multi popup).
    install_fav_save_all_tabs(window, state.clone());
    // Dropping a tab by DRAG onto a favorites folder: a "direct"
    // duplicate of the right-click "save as favorite" (the target container is
    // chosen by the drop point → no popup).
    install_tab_to_favorite(window, state.clone());
    // Dropping the SELECTION of a view (files/folders) onto a favorites
    // folder via DRAG. The file-drag target is a favorites container
    // → the selection is saved there (no file operation). Same paths
    // as the native drag (`panel_selected_paths`).
    install_file_to_favorite(window, state.clone());
    // Add the current selection (files) to favorites (popup).
    install_fav_add_selection(window, state.clone());
    // Create a container "on the fly" from the popup and select it.
    install_fav_save_new_container(window, state.clone());
    // Confirm the save: creates the favorite(s) in the chosen container.
    install_fav_save_commit(window, state.clone());
    // Reorder drag: updates the insertion indicator.
    install_fav_drag_move(window, state.clone());
    // Reorder drag release: always clears transient state, and mutates the
    // tree only when the UI confirms that the cursor is in its visible frame.
    install_fav_drag_drop(window, state.clone());
    // Auto-expand during a drag: expands a container IN PLACE (inserting the
    // subtree into the existing VecModel → row components, including the
    // dragged row and its grab handle, survive; `push_favorites_ui` would destroy them).
    install_fav_expand(window, state.clone());
    // default columns (Settings) — initial state + toggle.
    push_settings_columns(window, cfg.language, &cfg.default_columns);
    // recursive mtime depth (initial state + handler).
    window.set_rmtime_depth(cfg.recursive_mtime_depth);
    install_rmtime_depth_changed(window, state.clone());
    // recursive size depth (initial state + handler), mirroring the mtime one.
    window.set_size_depth(cfg.recursive_size_depth);
    install_size_depth_changed(window, state.clone());
    install_default_column_toggle(window, state.clone());
    // configurable shortcuts (settings) — list + rebind/reset/search.
    push_shortcuts_ui(window, &state);
    install_shortcut_search(window, state.clone());
    install_shortcut_rebind(window, state.clone());
    install_shortcut_resolve_conflict(window, state.clone());
    install_shortcut_reset(window, state.clone());
    install_shortcut_unassign(window, state.clone());
    install_shortcut_reset_all(window, state.clone());

    // Empty the trash (permanent — already confirmed on the UI side in 2 steps).
    // Background thread: enumerating + shell-purging a full
    // trash can take seconds — the UI thread doesn't wait.
    install_empty_trash(window);
    // Sorting is stored per panel and pushed by `update_panels_ui`.

    // ----- Language -----
    install_language_changed(window, state.clone());

    // ----- Theme -----
    install_theme_changed(window, state.clone());

    // UI zoom (settings): applies live + persists -----
    install_ui_scale_changed(window, state.clone());
    // Zoom picker: labels (language-independent) + index from the
    // config, then DEFERRED application — the window only has its real OS scale
    // once realized, not during this setup (before `run()`).
    install_ui_scale_deferred(window, state.clone());
    // "Video thumbnails (ffmpeg)" section (Linux): initial detection + button
    // "Recheck" + "copy" button for an install command.
    // Which annotations point at something that is gone. A filesystem walk, so
    // it is taken when the settings panel opens rather than kept live — and
    // taken ONCE: the badge reads its length, and the cleanup list reads the
    // snapshot itself, so opening the list costs nothing.
    install_count_annotation_orphans(window, state.clone());
    // Sweeping them, on an explicit request only. An item in the trash looks
    // exactly like a deleted one from here, so this is never done on the user's
    // behalf — and an unplugged drive is protected by the rule itself, which
    // requires the parent folder to still be there.
    install_clean_annotations(window, state.clone());
    // Ticking one entry, or all of them. The set lives in the state, so
    // "Select all" covers every orphan rather than the rows on screen.
    install_orphan_toggled(window, state.clone());
    install_orphans_select_all(window, state.clone());
    // The answer. Each ticked entry is re-checked before it goes: the list was
    // a snapshot, and an item restored from the trash while the question was on
    // screen keeps its annotation. The figure reported is what really left.
    install_orphans_confirmed(window, state.clone());
    apply_ffmpeg_info(window);
    install_ffmpeg_recheck(window);
    install_settings_copy(window);

    // DEFAULT tab bar position (settings) -----
    // Only applies to views created "from scratch" (new workspace /
    // reset); existing views keep their PER-VIEW setting.
    install_tabbar_default_changed(window, state.clone());
    // Tab path tooltip (settings): opt-in, persisted.
    install_tab_tooltip_changed(window, state.clone());
    // Guard before loading another workspace (settings): persisted.
    install_ws_warn_unsaved_changed(window, state.clone());
    // Hybrid height of Previews mode: persists then only recomputes the
    // geometry of models already in memory (no relisting, no I/O).
    install_compact_preview_rows_changed(window, state.clone());
    // Timezone for the "Modified" column (settings): 0 = local, 1 = UTC.
    // Persists, recomputes the offset, then rebuilds the rows of ALL
    // panels to reflect the new timezone immediately.
    install_clock_mode_changed(window, state.clone());
    // Windows context menu (settings): persisted kill-switch.
    install_shell_menu_changed(window, state.clone());

    // ----- Navigation -----
    install_nav_callback(window, &state, NavAction::Back);
    install_nav_callback(window, &state, NavAction::Forward);
    install_nav_callback(window, &state, NavAction::Parent);
    install_nav_callback(window, &state, NavAction::Home);
    install_nav_callback(window, &state, NavAction::Refresh);

    // ----- Editable address bar -----
    install_navigate_to(window, state.clone());
    // The URL menu's "Copy" and "Paste" are handled entirely in the interface,
    // by the field itself. Driving them from here meant working on the whole
    // text: copying took the current path whatever was selected, and pasting
    // replaced the entire line instead of dropping the clipboard at the caret.
    // Only the field knows its selection and its caret.

    // ----- Row activation (double-click) -----
    install_row_activated(window, state.clone());

    // Selection: single / Ctrl / Shift-click -----
    install_row_clicked(window, state.clone());
    install_row_ctrl_clicked(window, state.clone());
    install_row_shift_clicked(window, state.clone());
    install_select_all(window, state.clone());
    install_deselect_all(window, state.clone());

    // Rubber-band: we receive (x1, y1, x2, y2) in **content coordinates**
    // (the Slint side has already subtracted the exact Flickable `viewport-y`).
    // We only use y1/y2 (band selection). The lookup relies on the rows'
    // variable geometry, so it stays exact in a list mixing large
    // thumbnails and small icons, regardless of scroll position.
    install_rubber_band_update(window, state.clone());
    // Start of a rubber-band: captures the base selection + the mode
    // (0 replace, 1 add [Shift], 2 subtract [Ctrl]).
    install_rubber_band_begin(window, state.clone());

    // Configurable shortcuts -----
    // Resolves a key combination → action id according to the effective map. Called
    // on every keystroke by the Slint `key-scope`; returns "" if no action matches.
    install_match_action(window, state.clone());
    // Keyboard cursor navigation (arrows / Shift+arrows / Home / End).
    install_move_cursor(window, state.clone());

    // : file operations -----

    // Row the Menu key aims its context menu at, in the active view. The
    // selection anchor is the entry the user last put the focus on, so it is
    // preferred; a selection built some other way (Ctrl+A, a rectangle) leaves
    // it stale, and the first selected row then stands for the whole set. With
    // nothing selected, `-1` asks for the background menu.
    install_keyboard_context_row(window, state.clone());

    // Right-click: selects the row if not already selected, then opens the menu.
    install_row_right_clicked(window, state.clone());
    install_ctx_close(window);
    // Pinned user command in the context menu: on the view BACKGROUND
    // it targets the current folder ({dir} = current), otherwise the selection.
    install_ctx_custom_run(window, state.clone());
    // Windows SHELL context menu entry: InvokeCommand on the live
    // session. The session is TAKEN OUT of the state during the call (some
    // handlers pump messages → re-entrancy must not find the RefCell
    // already borrowed), then put back.
    install_ctx_shell_run(window, state.clone());
    // Hovering a shell CASCADE: pushes the children into the flyout.
    install_ctx_shell_sub_hover(window, state.clone());
    // Opening the "Detected Windows entries" sub-tab: lazy probe.
    // Deferred by one event-loop tick → the tab appears BEFORE the (potentially
    // slow) COM scan, the list fills in right after.
    install_shell_ext_scan(window, state.clone());
    // Checkbox of the "detected Windows entries" panel: hides/shows
    // the entry (by label), persisted.
    install_shell_ext_toggle(window, state.clone());

    // Open: for a folder, navigate; for a file, xdg-open.
    install_action_open(window, state.clone());
    install_open_many_confirmed(window, state.clone());
    // Open in a new tab (of the ACTIVE panel). The selected folder
    // opens in a new tab; for a file, it's its parent folder.
    install_action_open_new_tab(window, state.clone());
    // "Create shortcut": opens the popup (kind 3) pre-filled to create,
    // in the current folder, a link (.lnk or symlink) to the selected file.
    install_action_create_link(window, state.clone());
    // "Open as administrator": launches the targeted file ELEVATED via
    // ShellExecuteW("runas") → UAC prompt. Windows only (the menu entry is
    // hidden elsewhere via `platform-windows`).
    install_action_open_admin(window, state.clone());
    // "Open with…": the OS's native picker (Windows only; the menu
    // entry is hidden elsewhere via `platform-windows`).
    install_action_open_with(window, state.clone());
    // Open with: openers -----
    // Settings filter: doesn't rebuild the items and therefore doesn't re-extract
    // any icons while typing.
    install_opener_search(window, state.clone());
    // Launches an opener on the current selection.
    install_run_opener(window, state.clone());
    // Blank "Custom command" popup (menu / Settings).
    install_ow_add(window, state.clone());
    // Ready-made archiving command: opens the editor prefilled rather than
    // saving straight away, so the template stays reviewable and editable.
    install_ow_recipe_add(window, state.clone());
    // "…" button of the Program field: native exe picker.
    install_ow_browse(window, state.clone());
    // "Choose an application…": enumerates the OS's apps → custom picker.
    install_ow_open_picker(window, state.clone());
    // The picker search filters its cached rows only; application discovery and
    // icon extraction remain tied to opening the picker or changing its scope.
    install_ow_picker_search(window, state.clone());
    // Linux-only option: include launchers that make no MIME declaration.
    install_ow_picker_show_other_apps_changed(window, state.clone());
    // Choice in the picker: launches + CAPTURES the app (persisted as an opener).
    install_ow_pick(window, state.clone());
    // Picker's "Browse…": choose a custom exe from the OS (like the
    // Custom Command's "…"), REGISTER it as a reusable opener, then
    // launch it on the current selection. Symmetric across Windows (IFileOpenDialog) /
    // Linux (zenity/kdialog).
    install_ow_pick_browse(window, state.clone());
    // "Use as application": pre-fills from the selected exe.
    install_ow_use_as_app(window, state.clone());
    // Promotion toast accepted → blank popup (the user points to the exe).
    install_ow_promote_accept(window, state.clone());
    // Editing an existing opener.
    install_ow_edit(window, state.clone());
    // Deleting an opener.
    install_ow_delete(window, state.clone());
    // Duplicating an entry: clone inserted right after the original.
    install_ow_duplicate(window, state.clone());
    // Reordering (up/down) in Settings.
    install_ow_move(window, state.clone());
    // Live preview (on every keystroke in the popup).
    install_ow_popup_changed(window, state.clone());
    // Popup submission: saves (checkbox checked) OR launches one-shot.
    install_ow_popup_commit(window, state.clone());

    // ext: Open parent folder / Open terminal here -----
    install_action_open_parent(window, state.clone());
    install_action_open_terminal(window, state.clone());

    // ext: Copy path / Copy name -----
    install_action_copy_path(window, state.clone());
    install_action_copy_name(window, state.clone());

    // Copy: snapshot of the selected paths into the internal clipboard.
    install_action_copy(window, state.clone());

    // Cut: same as Copy but with ClipOp::Cut + visual marking.
    install_action_cut(window, state.clone());

    // Paste: prepares a PasteJob. Items without a name conflict are
    // resolved directly; those whose name is already taken open the
    // conflict popup (one at a time). The actual execution happens once everything is resolved.
    install_action_paste(window, state.clone());
    // Live validation of the name typed in the conflict popup.
    install_paste_conflict_check(window, state.clone());
    // Confirms the name for the current conflict, then moves to the next one.
    install_paste_conflict_confirm(window, state.clone());
    // Skips the current conflicting item.
    install_paste_conflict_skip(window, state.clone());
    // Replaces (overwrites) the CURRENT conflicting item, then moves to the next.
    install_paste_conflict_replace(window, state.clone());
    // Replaces (overwrites) the current item AND ALL remaining conflicts, then
    // executes.
    install_paste_conflict_replace_all(window, state.clone());
    // Skips the current item AND ALL remaining conflicts, then executes.
    install_paste_conflict_skip_all(window, state.clone());
    // Cancels the whole paste operation (nothing has been executed yet).
    install_paste_conflict_cancel(window, state.clone());
    // Cancels the ongoing long operation (raises the cooperative flag).
    install_op_cancel(window, state.clone());
    // Creates or refreshes one operation's toast row (pushed from its worker).
    install_op_progress(window, state.clone());
    // Closes one operation's toast, by hand or once its auto-dismiss fires.
    install_op_dismiss(window, state.clone());
    // End of a long operation: resets the state (from the thread, via invoke).
    install_op_finished(window, state.clone());

    // Duplicate: copies each selected row to a unique sibling
    // (background thread + progress bar).
    install_action_duplicate(window, state.clone());

    // Rename opens a modal popup pre-filled with the name of the first selected
    // row. The exact source path is captured in AppState: the row
    // index can become stale if the watcher re-lists while the dialog is open.
    install_action_rename(window, state.clone());
    // Live check of the rename conflict (same spirit as paste).
    install_rename_check(window, state.clone());
    // "Type-ahead" filter of the active view — SHARED with the
    // by-extension filter: the same keystrokes feed one OR the other depending
    // on whether the active tab's "extension filter" mode is on. -----
    install_filter_type(window, state.clone());
    install_filter_backspace(window, state.clone());
    install_filter_clear(window, state.clone());
    // ----- Refreshes ALL panels (after a multi-view op: drag-drop…) -----
    install_refresh_all(window, state.clone());
    // File drag'n'drop between views -----
    // Drag start: ensures the grabbed row is selected in the SOURCE
    // view (if it wasn't, only it gets selected).
    install_file_drag_begin(window, state.clone());
    // Real-time validation of the internal target. In the source view, the
    // `selected` flag allows an O(1) check of the common case (folder dropped onto itself).
    // Between two views, we compare paths to cover the same folder
    // displayed on both sides and a drop into a descendant.
    install_file_drop_target_invalid(window, state.clone());
    // Switches to NATIVE DRAG (OLE) when the cursor leaves the window during a
    // file drag → external applications receive the files as CF_HDROP.
    // Blocking (modal loop) until the
    // drop. Windows only; no-op elsewhere.
    install_start_native_drag(window, state.clone());
    // Drop ONTO an executable: if the target row is a .exe/.cmd/.bat…,
    // we LAUNCH it with the dropped files as ARGUMENTS (instead of copying). The
    // Slint side sets file-drop-src/target/row BEFORE calling this callback; `true` =
    // launched (no menu). No-op → `false` (normal Copy/Move/Link menu).
    install_file_drop_onto_exec(window, state.clone());
    // Drop: executes the chosen action (0 move · 1 copy · 2 link) from the
    // source view to the target view's folder.
    install_file_drop_action(window, state.clone());
    install_rename_confirmed(window, state.clone());
    // Forced replace: offered only for a file↔file conflict.
    // The core primitive uses the OS's atomic replace; no prior
    // `remove` that could lose both files.
    install_rename_force_replace(window, state.clone());
    // Live name check for New folder / New file. It
    // reuses exactly the same source of truth as Rename; the
    // shortcut/link modes keep their specific rules (derived name allowed).
    install_create_check(window, state.clone());
    // Creation of a new folder / file / shortcut (empty-area popup) in
    // the active panel's current folder. `kind`: 0 = folder, 1 = file,
    // 2 = .lnk shortcut (Windows).
    install_create_confirmed(window, state.clone());
    // "Browse…" (shortcut popup): native picker → fills in the target and,
    // if the name is empty, pre-fills it from the target's name.
    install_create_browse_target(window, state.clone());

    // Right-click in a panel's empty area → "background" menu.
    install_empty_right_clicked(window, state.clone());

    // Deliveries from the trash workers. This single callback keeps the
    // native PathBufs until the UI thread and centralizes updating
    // the history, the notices, and the refresh.
    install_process_op_events(window, state.clone());

    // Delete: move to trash (background thread + progress bar).
    // Windows + NETWORK volume: no trash → the deletion will be PERMANENT
    // (`ops::trash` switches to `permanent_delete`) → we confirm BEFORE,
    // like Explorer. `is_network_path` is a LOCAL (instant) check.
    install_action_delete(window, state.clone());
    // Ctrl+Z (customizable) restores only the last deleted item,
    // and only if the active view still shows its original folder.
    install_action_restore_last_trashed(window, state.clone());
    // Shift+Delete: truly permanent deletion on both Windows AND Linux. The
    // combination is resolved by the configurable shortcut map; this
    // callback validates the selection before opening the same modal warning.
    install_action_delete_permanent(window, state.clone());
    // Confirmation of the shared popup. We consume the selection frozen at
    // opening time: an asynchronous re-listing can't change the target. The explicit mode calls
    // `permanent_delete`; the network mode keeps the `trash` route, which performs
    // its permanent switch only for those Windows volumes.
    install_delete_warning_confirmed(window, state.clone());

    // Properties: the system's **native** dialog, on both platforms —
    // Windows via the shell (Security/Details/Versions tabs…), Linux via
    // `org.freedesktop.FileManager1` (Dolphin, Nautilus, Nemo, Caja…). There is
    // no more internal panel: as with the other desktop integrations
    // (opening via `xdg-open`, `zenity`/`kdialog` picker…), a failure is
    // simply reported to the user.
    install_action_properties(window, state.clone());

    // ----- Sort by clicking a header -----
    install_sort_clicked(window, state.clone());

    // Grouping mode: header menu + Ctrl+G -----
    // The header menu and Ctrl+G first activate the target panel → `idx` is
    // the active panel. `refresh_listing` re-sorts (with the new group_mode read
    // from the active tab), applies the type-ahead filter, and preserves the selection.
    install_set_group_mode(window, state.clone());
    // Extension filter — toggled from the columns menu: shows /
    // hides panel `idx`'s input bar (turning it off clears the text).
    install_ext_filter_toggle(window, state.clone());

    // : Tabs -----
    install_tab_new(window, state.clone());
    // Duplicate a tab (tab context menu): copy inserted right
    // after the targeted tab, in the targeted panel (may not be the active one).
    install_duplicate_tab(window, state.clone());
    // Scroll target of the tab bar (overflow): exact
    // geometric computation on the Rust side (`TabInfo` widths), returns the `viewport-x` in px.
    install_panel_scroll_target(window, state.clone());
    install_tab_closed(window, state.clone());
    install_tab_clicked(window, state.clone());

    // : Panels (split / close of the layout tree) -----
    install_panel_split(window, state.clone());
    // A view's tab bar position: 0 top · 1 left ·
    // 2 right. Persisted per view in the workspace.
    install_set_tab_bar_mode(window, state.clone());
    // Vertical bar width set via the handle — persisted
    // per view. The view has already applied the LIVE resize; we store the
    // EFFECTIVE value (already clamped by the shared formula).
    install_panel_vbar_resized(window, state.clone());
    install_panel_closed(window, state.clone());
    install_panel_clicked(window, state.clone());

    // Previews / thumbnails -----
    // Starts the scheduler's worker POOL for this application state.
    // The scheduler is already concurrency-safe (queue + `in_flight` under a mutex,
    // `take_next` skips an already-taken path, per-path wait/completion); the
    // heavy decoding runs outside the lock and never touches `AppState`. Going from
    // 1 to N threads therefore parallelizes the decoding without changing the logic.
    if state.thumb_scheduler.start_once() {
        let workers = thumb_worker_count();
        for _ in 0..workers {
            spawn_thumb_worker(state.thumb_scheduler.clone(), window.as_weak());
        }
        debug!(workers, "thumbnail pool started");
    }
    // The view publishes its exact viewport after every scroll/layout. The bridge
    // adjusts the rendered sub-model with an overscan screen, then re-prioritizes the
    // next thumbnail. No listing or folder scan is triggered.
    install_panel_viewport_changed(window, state.clone());
    // All interactive uses (hover, click, drag, visible range) go
    // through this single hit-test on the rows' precomputed geometry.
    install_panel_row_at_content_y(window);
    // Background worker for the recursive mtime — same pattern.
    if let Some(rx) = state.rmtime_rx.borrow_mut().take() {
        spawn_rmtime_worker(rx, state.rmtime_gen.clone(), window.as_weak());
    }
    // Background worker for the "show subfolder contents" scans.
    if let Some(rx) = state.subscan_rx.borrow_mut().take() {
        spawn_subscan_worker(rx, state.subscans.clone(), window.as_weak());
    }
    // Background worker for image metadata — same pattern.
    if let Some(rx) = state.imgmeta_rx.borrow_mut().take() {
        spawn_imgmeta_worker(rx, state.imgmeta_gen.clone(), window.as_weak());
    }
    // Image metadata ready (pushed by the worker): cache + apply it to the
    // rows at the matching path.
    install_imgmeta_ready(window, state.clone());
    // Sets the display mode of panel `idx`'s active tab: "list" / "previews" /
    // "grid" (the 3-option menu of the view button).
    install_set_view_mode(window, state.clone());
    // Ctrl+P / the keyboard route of the view button: list → previews → grid.
    install_cycle_view_mode(window, state.clone());
    // Folds / unfolds a section by its key (click on a section header). Shape
    // only: the rows are rebuilt from the cached listing, no disk access.
    install_toggle_section(window, state.clone());
    // "Show subfolder contents" of panel `idx`'s active tab: one section per
    // direct subfolder, each listing read on a background thread.
    install_toggle_subfolder_contents(window, state.clone());
    // Width of a view's list area: the grid packs its tiles with it. Only a
    // grid re-packs — the other modes' rows span whatever width they are given.
    install_rows_area_width(window, state.clone());
    // Entry zoom (Ctrl+wheel): adjusts the level of panel `idx`'s
    // active tab, derives the thumbnail mode from it, and recomputes in memory the
    // useful geometry/resolution without re-reading the folder.
    install_zoom_view(window, state.clone());
    // Shows / hides hidden files for panel `idx`'s active tab
    //. The button calls `activate` beforehand → `idx` is the panel
    // active one, so `refresh_listing` (which honors the type-ahead filter and preserves
    // the selection) targets the right panel.
    install_toggle_show_hidden(window, state.clone());
    // Thumbnail ready (pushed by the worker): we cache it + apply it
    // to the row(s) at the matching path, in preview panels.
    install_thumb_ready(window, state.clone());

    // Recursive folder stats ready: cache the mtime and/or size and apply them
    // to the "modified"/"age" cells and the "size" cell of the matching FOLDER
    // row. An empty string for a metric means "not computed" for this job.
    install_folder_stats_ready(window, state.clone());

    // ----- Column resizing: PER PANEL -----
    // The panel has already applied the widths locally (Slint in-out
    // property); we remember them on the Rust side so they survive
    // model rebuilds (navigation, sort, split, panel switch).
    install_panel_column_resized(window, state.clone());
    // check/uncheck a column (right-click header → menu).
    install_column_toggle(window, state.clone());
    // Reordering a column by drag-and-drop.
    install_column_moved(window, state.clone());

    // App version (displayed in the Settings header).
    window.set_app_version(env!("CARGO_PKG_VERSION").into());

    // ----- XDG paths (informational in the Settings panel) -----
    window.set_config_dir_path(paths::config_path().display().to_string().into());
    window.set_data_dir_path(paths::data_dir().display().to_string().into());
    window.set_cache_dir_path(paths::cache_dir().display().to_string().into());
    // Windows: `config_dir() == data_dir()` (%APPDATA%\favnyr) → `config.toml`
    // lives in the Data dir, so the "Configuration file" row is
    // redundant and hidden. Linux: separate folders → row kept.
    window.set_config_path_redundant(paths::config_dir() == paths::data_dir());

    // Open an XDG folder in the OS's file manager (xdg-open). For config,
    // we open the **folder** (not the toml file) to stay consistent with
    // the other two entries.
    window.on_open_config_dir(|| {
        if let Err(err) = actions::open_path(&paths::config_dir()) {
            error!(error = %err, "xdg_open(config_dir) failed");
        }
    });
    window.on_open_data_dir(|| {
        if let Err(err) = actions::open_path(&paths::data_dir()) {
            error!(error = %err, "xdg_open(data_dir) failed");
        }
    });
    window.on_open_cache_dir(|| {
        if let Err(err) = actions::open_path(&paths::cache_dir()) {
            error!(error = %err, "xdg_open(cache_dir) failed");
        }
    });

    // ----- Tab drag'n'drop -----
    // The intra-instance target index computation (preview + drop) is done on the
    // SLINT SIDE (same formula → visual/logic consistency). The bridge handles multi-
    // instance: on every progress update, if ANOTHER Favnyr window is under the
    // cursor, we send it the hover point → it shows its insertion
    // preview. `hover_target` (shared with `tab-drag-completed`) remembers the
    // last hovered instance so it can be sent a "hover end" at the
    // right moment (target change OR end of drag).
    let hover_target = Rc::new(std::cell::Cell::new(0isize));
    install_tab_drag_progress(window, hover_target.clone());
    install_tab_drag_completed(window, state.clone(), hover_target.clone());

    // Resizing a splitter -----
    // `frac` = the pointer's absolute position as a fraction [0,1] of the container along
    // the split axis. We convert it to a ratio local to the split via the area computed
    // from the tree, then adjust the node. This absolute computation requires
    // no snapshot and doesn't accumulate drift.
    install_splitter_resized(window, state.clone());

    // Even out the views a separator governs — or the whole layout, when the
    // request comes from the menu or a shortcut, which aim at no separator.
    install_equalize_views(window, state.clone());

    // A view released OUTSIDE the window leaves for a window of its own, with
    // all its tabs.
    install_view_torn_off(window, state.clone());

    // Two views exchange their places. Only the layout tree is touched, and
    // only by two leaf indices: the views themselves, their tabs and their
    // history stay exactly where they are in `panels`.
    install_views_swapped(window, state.clone());

    // : Named workspaces -----
    //
    // IMPORTANT: all these callbacks are triggered from an element of the
    // `workspaces` list (click on a row, on an icon, or submitting the
    // rename TextInput). But they modify Slint models
    // (`set_workspaces` via refresh_workspaces_ui, `set_panels` via
    // refresh_listing) — recreating the model DESTROYS the element whose callback
    // is currently executing → "Recursion detected".
    //
    // So we defer the work via `slint::Timer::single_shot(0, …)`: it
    // runs on the next event-loop tick, outside the item's callback.
    // (`invoke_from_event_loop` isn't usable here since it requires `Send`,
    // incompatible with `AppState` = `Rc<RefCell<…>>`.)
    refresh_workspaces_ui(window, &state);
    // Live validation of the "Workspace name" field. Names are compared after
    // trimming and case-insensitively: "Project" and "project" would be
    // impossible to properly distinguish in the title and the dirty indicator.
    install_ws_name_check(window);
    // Reopens the last tab actually closed in the panel whose dead
    // zone opened the menu. Transfers/tear-offs never go through
    // the history and therefore can't be accidentally duplicated.
    install_reopen_last_closed_tab(window, state.clone());
    // Delivery of regular network listings. The worker never captures
    // the AppState (`Rc`): it pushes a Send value into the queue then simply
    // wakes Slint; all model mutation stays on the UI thread.
    install_async_listing_drain(window, state.clone());
    // Delivery of the subfolder scans. Same shape as the network listings:
    // the worker wakes Slint, every model mutation stays on the UI thread.
    install_subfolders_drain(window, state.clone());
    install_ws_save_new(window, state.clone());
    install_ws_load(window, state.clone());
    // Safeguard: Load goes through here. Clean/ad-hoc current → loads
    // directly; modified current → opens the warning dialog, UNLESS
    // the user has unchecked the safeguard in the settings.
    install_ws_load_guarded(window, state.clone());
    // "Save and…": overwrites the current workspace with the
    // live state, THEN executes the pending action — load `ws-dirty-target-id`, or
    // start over blank if `ws-dirty-reset`.
    install_ws_save_and_load(window, state.clone());
    install_ws_overwrite(window, state.clone());
    // Quick save of the current named workspace (customizable action,
    // Ctrl+S by default). An ad-hoc state opens the panel to ask for a name.
    install_ws_save_current(window, state.clone());
    install_ws_delete(window, state.clone());
    install_ws_rename(window, state.clone());
    // Resets the current layout to the blank state (1 panel, $HOME).
    // DIRECT path (no safeguard): used by the "load without
    // saving" dialog when the target is "new workspace".
    install_ws_reset(window, state.clone());
    // "Start a new workspace" from the menu: goes through the safeguard
    //. Clean/ad-hoc current or safeguard disabled → direct reset + closes
    // the overlay; modified current → dialog (target = "New workspace").
    install_ws_reset_guarded(window, state.clone());
    // Reverses the sort order of the workspace list.
    install_ws_toggle_sort(window, state.clone());
    // Recomputed when the menu opens (is-current/is-dirty flags from the
    // live state). Deferred: `changed workspaces-open` fires during
    // the processing of an event → we update the model on the next tick.
    install_ws_refresh(window, state.clone());

    // Initial async population of all panels: the window is displayed
    // immediately, then a background thread delivers each listing as it comes in.
    // Delays from various network paths thus never block the
    // interface's startup.
    initial_populate_async(window, &state);
    // The shell menu probe is lazy and only starts when the
    // "Detected Windows entries" sub-tab opens; see `scan_shell_ext`.
    // Window title: "{workspace} — Favnyr" (or "Favnyr") depending on the
    // named workspace the restored state comes from.
    update_window_title(window, &state);
}

mod cb_sidebar;
use cb_sidebar::*;

mod cb_favorites;
use cb_favorites::*;

mod cb_prefs;
use cb_prefs::*;

mod cb_nav;
use cb_nav::*;

mod cb_open;
use cb_open::*;

mod cb_openwith;
use cb_openwith::*;

mod cb_clipboard;
use cb_clipboard::*;

mod cb_files;
use cb_files::*;

mod cb_dnd;
use cb_dnd::*;

mod cb_view;
use cb_view::*;

mod cb_rows;
use cb_rows::*;

mod cb_workspace;
use cb_workspace::*;
