use super::*;

pub(super) fn install_ctx_close(window: &MainWindow) {
    let weak = window.as_weak();
    window.global::<crate::MenuApi>().on_ctx_close(move || {
        if let Some(w) = weak.upgrade() {
            w.global::<crate::MenuApi>().set_ctx_menu_open(false);
            w.global::<CtxNav>().invoke_disarm();
        }
    });
}

pub(super) fn install_ctx_custom_run(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_ctx_custom_run(move |id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let Some(op) = st.openers.borrow().get(&id).cloned() else {
                return;
            };
            let result = if w.global::<crate::MenuApi>().get_ctx_custom_bg() {
                actions::run_opener_dir(&op, &st.current_path())
            } else {
                actions::run_opener(&op, &selected_paths(&st))
            };
            if let Err(err) = result {
                error!(error = %err, label = op.label, "pinned context command failed");
                let lang = st.config.borrow().language;
                notice(
                    &w,
                    i18n::tr(lang, "ow_run_failed").replace("{name}", &op.label),
                    NoticeKind::Error,
                );
            }
        });
}

pub(super) fn install_ctx_shell_run(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::MenuApi>()
        .on_ctx_shell_run(move |offset: i32| {
            let session = st.shell_menu.borrow_mut().take();
            if let Some(s) = session {
                let offset = offset.max(0) as u32;
                // The modern "Share" verb fails when invoked via the shell from an
                // unpackaged host (ERROR_INVALID_WINDOW_HANDLE): the share flyout
                // needs a DataTransferManager registered for the window first. We
                // drive it natively instead. "share" is the only standard verb
                // containing that token, so the match is language-independent.
                if crate::shellmenu::verb_for(&s, offset).contains("share") {
                    #[cfg(windows)]
                    {
                        let paths = selected_paths(&st);
                        if let Some(hwnd) = crate::winmsg::self_hwnd()
                            && !paths.is_empty()
                            && let Err(err) = crate::winshare::share_files(hwnd, &paths)
                        {
                            error!(error = %err, "native share failed");
                        }
                    }
                } else if let Err(err) = crate::shellmenu::invoke(&s, offset) {
                    error!(error = %err, "shell context command failed");
                }
                *st.shell_menu.borrow_mut() = Some(s);
            }
        });
}

pub(super) fn install_ctx_shell_sub_hover(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_ctx_shell_sub_hover(move |sub: i32| {
            let Some(w) = weak.upgrade() else { return };
            let subs = st.shell_subs.borrow();
            if let Some(children) = subs.get(sub.max(0) as usize) {
                w.global::<crate::MenuApi>()
                    .set_ctx_shell_sub_entries(ModelRc::new(VecModel::from(children.clone())));
            }
        });
}

pub(super) fn install_shell_ext_scan(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_shell_ext_scan(move || {
            let st = st.clone();
            let weak = weak.clone();
            defer(move || {
                if let Some(w) = weak.upgrade() {
                    scan_shell_ext(&w, &st);
                }
            });
        });
}

pub(super) fn install_shell_ext_toggle(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SettingsApi>().on_shell_ext_toggle(
        move |label: SharedString, on: bool| {
            let Some(w) = weak.upgrade() else { return };
            let label = label.to_string();
            info!(label, on, "shell context entry toggled");
            st.persist_config(|c| {
                if on {
                    c.shell_menu_disabled.retain(|x| x != &label);
                } else if !c.shell_menu_disabled.contains(&label) {
                    c.shell_menu_disabled.push(label.clone());
                }
            });
            refresh_shell_ext_rows(&w, &st);
        },
    );
}

pub(super) fn install_action_open(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_open(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            let Some(first) = paths.first() else { return };
            // Files chosen together open together, the way the desktop file
            // manager does it — but ONLY when no folder is in the lot.
            // Navigating is not something several items can share, and a folder
            // handed to the system would open a window outside Favnyr. This is
            // the one action that spreads over a selection: renaming and the
            // rest still act on a single item.
            if paths.len() > 1 && paths.iter().all(|p| !acts_as_dir(p)) {
                open_selected_files(&w, &st, paths);
                return;
            }
            let is_dir = first.is_dir();
            if is_dir {
                load_directory(&w, &st, first, true);
            } else if !open_shortcut_as_tab(&w, &st, first) {
                // FAVNYR's per-extension default if set, otherwise the OS default.
                // Logic shared with double-click (`open_file_default`).
                // A folder .lnk shortcut has already been opened as a tab.
                //
                // The FIRST item only: getting here with several selected means
                // a folder is among them, and handing that folder to the system
                // would open a window outside Favnyr.
                open_file_default(&st, std::slice::from_ref(first));
            }
        });
}

pub(super) fn install_open_many_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::OperationsApi>()
        .on_open_many_confirmed(move || {
            let paths = std::mem::take(&mut *st.pending_open.borrow_mut());
            open_file_default(&st, &paths);
        });
}

pub(super) fn install_action_open_new_tab(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_open_new_tab(move || {
            let Some(w) = weak.upgrade() else { return };
            let paths = selected_paths(&st);
            let target = match paths.first() {
                Some(p) if p.is_dir() => p.clone(),
                Some(p) => p
                    .parent()
                    .map(|pp| pp.to_path_buf())
                    .unwrap_or_else(|| st.current_path()),
                None => st.current_path(),
            };
            let opened = st.with_tabs_mut(|book| {
                let a = book.open_after_active(target);
                book.tabs[a].current_path.clone()
            });
            load_directory(&w, &st, &opened, false);
        });
}

pub(super) fn install_action_create_link(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_create_link(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(file) = selected_paths(&st).into_iter().next() else {
                return;
            };
            // Default name = the file's stem (avoids colliding with the
            // file itself for a symlink in the same folder).
            let default_name = file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            w.global::<crate::OperationsApi>().set_create_kind(3);
            w.global::<crate::OperationsApi>()
                .set_create_name(default_name.into());
            w.global::<crate::OperationsApi>()
                .set_create_target(file.display().to_string().into());
            // Linux: no `.lnk` → Symlink tab forced.
            w.global::<crate::OperationsApi>()
                .set_create_link_symlink(!cfg!(windows));
            w.global::<crate::OperationsApi>()
                .set_create_popup_open(true); // arms focus via `changed create-popup-open`
        });
}

pub(super) fn install_action_open_admin(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::OperationsApi>()
        .on_action_open_admin(move || {
            let paths = selected_paths(&st);
            let Some(first) = paths.first() else { return };
            if let Err(err) = actions::open_elevated(first) {
                error!(error = %err, path = %first.display(), "open as admin failed");
            }
        });
}

pub(super) fn install_action_open_with(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_action_open_with(move || {
            let paths = selected_paths(&st);
            let Some(first) = paths.first() else { return };
            if let Err(err) = actions::open_with(first) {
                error!(error = %err, path = %first.display(), "open with failed");
            } else if let Some(w) = weak.upgrade() {
                // The native dialog doesn't reveal the chosen app → we offer to
                // register it afterwards via a non-blocking toast.
                w.global::<crate::SettingsApi>()
                    .set_ow_promote_visible(true);
            }
        });
}

pub(super) fn install_opener_search(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_opener_search(move |text: SharedString| {
            *st.opener_filter.borrow_mut() = text.to_string();
            if let Some(w) = weak.upgrade() {
                push_filtered_openers_ui(&w, &st);
            }
        });
}

pub(super) fn install_run_opener(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::OperationsApi>()
        .on_run_opener(move |id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let opener = st.openers.borrow().get(&id).cloned();
            if let Some(op) = opener {
                let paths = selected_paths(&st);
                // Extension of the opened file → learned for the opener.
                let ext = paths.first().and_then(|p| {
                    p.extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                });
                match actions::run_opener(&op, &paths) {
                    Ok(()) => {
                        st.openers.borrow_mut().record_use(&op.id, ext.as_deref());
                        save_openers(&st);
                        push_openers_ui(&w, &st);
                    }
                    Err(err) => {
                        error!(error = %err, label = op.label, "run_opener failed");
                        let lang = st.config.borrow().language;
                        notice(
                            &w,
                            i18n::tr(lang, "ow_run_failed").replace("{name}", &op.label),
                            NoticeKind::Error,
                        );
                    }
                }
            }
        });
}

pub(super) fn install_action_open_parent(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::OperationsApi>()
        .on_action_open_parent(move || {
            let paths = selected_paths(&st);
            // If there's a selection: open the parent of the first item.
            // Otherwise: open the parent of the current folder.
            let target = paths.first().cloned().unwrap_or_else(|| st.current_path());
            if let Err(err) = actions::open_parent(&target) {
                error!(error = %err, "open_parent failed");
            }
        });
}

pub(super) fn install_action_open_terminal(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::OperationsApi>()
        .on_action_open_terminal(move || {
            let paths = selected_paths(&st);
            // Selection present → take the first path; otherwise → cwd.
            // `open_terminal` handles the path-or-parent mapping internally.
            let target = paths.first().cloned().unwrap_or_else(|| st.current_path());
            if let Err(err) = actions::open_terminal(&target) {
                error!(error = %err, "open_terminal failed");
            }
        });
}
