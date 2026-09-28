use super::*;

pub(super) fn install_ow_add(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SettingsApi>().on_ow_add(move || {
        if let Some(w) = weak.upgrade() {
            open_ow_create(&w, &st, "", "");
        }
    });
}

pub(super) fn install_ow_recipe_add(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_recipe_add(move |index: i32| {
            let Some(w) = weak.upgrade() else { return };
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            let Some(recipe) = RECIPES.get(index) else {
                return;
            };
            let Some(program) = actions::resolve_program(recipe.programs) else {
                return; // the tool disappeared since the list was built
            };
            let lang = st.snapshot_config().language;
            open_ow_create(&w, &st, &program, &recipe.label(lang));
            w.global::<crate::SettingsApi>()
                .set_ow_popup_icon_kind(recipe.icon.as_i32());
            w.global::<crate::SettingsApi>()
                .set_ow_popup_args(recipe.args.into());
            w.global::<crate::SettingsApi>()
                .set_ow_popup_ctx_file(recipe.ctx & openers::CTX_FILE != 0);
            w.global::<crate::SettingsApi>()
                .set_ow_popup_ctx_ext(recipe.ctx_exts.join(", ").into());
            w.global::<crate::SettingsApi>()
                .set_ow_popup_ctx_dir(recipe.ctx & openers::CTX_DIR != 0);
            w.global::<crate::SettingsApi>()
                .set_ow_popup_ctx_bg(recipe.ctx & openers::CTX_BACKGROUND != 0);
            recompute_ow_preview(&w, &st);
        });
}

pub(super) fn install_ow_browse(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SettingsApi>().on_ow_browse(move || {
        let Some(w) = weak.upgrade() else { return };
        let lang = st.snapshot_config().language;
        if let Some(path) = openwith::browse_for_exe(lang) {
            w.global::<crate::SettingsApi>()
                .set_ow_popup_program(path.into());
            recompute_ow_preview(&w, &st);
        }
    });
}

pub(super) fn install_ow_open_picker(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_open_picker(move || {
            let Some(w) = weak.upgrade() else { return };
            let Some(path) = selected_paths(&st).into_iter().next() else {
                return;
            };
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            w.global::<crate::SettingsApi>()
                .set_ow_picker_show_other_apps(false);
            w.global::<crate::SettingsApi>()
                .set_ow_picker_search_text(SharedString::new());
            refresh_ow_picker_handlers(&w, &st, &ext, &path, false);
            w.global::<crate::SettingsApi>()
                .set_ow_picker_set_default(false); // unchecked on every opening
            w.global::<crate::SettingsApi>().set_ow_picker_open(true);
        });
}

pub(super) fn install_ow_picker_search(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_picker_search(move |text: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            push_filtered_ow_picker_ui(&w, &st, text.as_str());
        });
}

pub(super) fn install_ow_picker_show_other_apps_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_picker_show_other_apps_changed(move |show_other_apps| {
            let Some(w) = weak.upgrade() else { return };
            let context = st
                .ow_pick_ctx
                .borrow()
                .as_ref()
                .map(|ctx| (ctx.ext.clone(), ctx.path.clone()));
            let Some((ext, path)) = context else { return };
            refresh_ow_picker_handlers(&w, &st, &ext, &path, show_other_apps);
        });
}

pub(super) fn install_ow_pick(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_pick(move |key: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let key = key.to_string();
            // Extracted from the context (path/ext + chosen handler), then releases the borrow.
            let picked = {
                let ctx = st.ow_pick_ctx.borrow();
                let Some(ctx) = ctx.as_ref() else { return };
                let handler = ctx.handlers.iter().find(|h| h.key == key).cloned();
                (ctx.ext.clone(), ctx.path.clone(), handler)
            };
            let (ext, path, handler) = picked;
            match openwith::launch(&key, &ext, &path) {
                Ok(()) => {
                    // Capture: registers ANY chosen app (regular exe OR an OS
                    // app without an exe — Windows UWP/Store, Linux `.desktop`) and marks it
                    // as the MOST RECENTLY used → head of the suggestions.
                    if let Some(h) = handler {
                        // "Set as default" checked → the app becomes Favnyr's
                        // default for this extension (takes priority over the OS).
                        let set_default =
                            w.global::<crate::SettingsApi>().get_ow_picker_set_default();
                        {
                            let mut store = st.openers.borrow_mut();
                            let id = match h.exe {
                                Some(exe) => store
                                    .openers
                                    .iter()
                                    .find(|o| o.program == exe)
                                    .map(|o| o.id.clone())
                                    .unwrap_or_else(|| {
                                        store.add(&h.name, &exe, vec!["{file}".to_string()])
                                    }),
                                None => store
                                    .openers
                                    .iter()
                                    .find(|o| o.assoc.as_deref() == Some(h.key.as_str()))
                                    .map(|o| o.id.clone())
                                    .unwrap_or_else(|| store.add_assoc(&h.name, &h.key)),
                            };
                            store.record_use(&id, Some(&ext));
                            if set_default && !ext.is_empty() {
                                store.set_default_ext(&id, &ext, true);
                            }
                        }
                        save_openers(&st);
                        push_openers_ui(&w, &st);
                    }
                }
                Err(err) => {
                    error!(error = %err, "open-with launch failed");
                    let lang = st.config.borrow().language;
                    notice(
                        &w,
                        i18n::strings_for(lang).fav_toast_missing.clone(),
                        NoticeKind::FavMissing,
                    );
                }
            }
        });
}

pub(super) fn install_ow_pick_browse(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_pick_browse(move || {
            let Some(w) = weak.upgrade() else { return };
            // Picker context required (opened on a selection); we keep the
            // first path as a fallback if the selection emptied in the meantime.
            let ctx_path = {
                let ctx = st.ow_pick_ctx.borrow();
                let Some(ctx) = ctx.as_ref() else { return };
                ctx.path.clone()
            };
            // Cancelling the native dialog → we leave everything untouched (picker stays open).
            let lang = st.snapshot_config().language;
            let Some(exe) = openwith::browse_for_exe(lang) else {
                return;
            };
            let name = std::path::Path::new(&exe)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| exe.clone());
            // Registers the app (reuses the existing one if same program).
            let opener = {
                let mut store = st.openers.borrow_mut();
                match store.openers.iter().find(|o| o.program == exe).cloned() {
                    Some(o) => o,
                    None => {
                        let id = store.add(&name, &exe, vec!["{file}".to_string()]);
                        store.get(&id).cloned().expect("opener was just added")
                    }
                }
            };
            save_openers(&st);
            push_openers_ui(&w, &st);
            // Launches on the current selection (multi-file handled by run_opener).
            let mut paths = selected_paths(&st);
            if paths.is_empty() {
                paths.push(ctx_path);
            }
            let ext = paths.first().and_then(|p| {
                p.extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
            });
            match actions::run_opener(&opener, &paths) {
                Ok(()) => {
                    let set_default = w.global::<crate::SettingsApi>().get_ow_picker_set_default();
                    {
                        let mut store = st.openers.borrow_mut();
                        store.record_use(&opener.id, ext.as_deref());
                        if set_default && let Some(e) = ext.as_deref().filter(|e| !e.is_empty()) {
                            store.set_default_ext(&opener.id, e, true);
                        }
                    }
                    save_openers(&st);
                    push_openers_ui(&w, &st);
                }
                Err(err) => {
                    error!(error = %err, label = opener.label, "ow-pick-browse run_opener failed");
                    let lang = st.config.borrow().language;
                    notice(
                        &w,
                        i18n::tr(lang, "ow_run_failed").replace("{name}", &opener.label),
                        NoticeKind::Error,
                    );
                }
            }
            w.global::<crate::SettingsApi>().set_ow_picker_open(false);
        });
}

pub(super) fn install_ow_use_as_app(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_use_as_app(move || {
            let Some(w) = weak.upgrade() else { return };
            let sel = selected_paths(&st);
            if let Some(p) = sel.first() {
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                open_ow_create(&w, &st, &p.display().to_string(), &name);
            }
        });
}

pub(super) fn install_ow_promote_accept(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_promote_accept(move || {
            if let Some(w) = weak.upgrade() {
                open_ow_create(&w, &st, "", "");
            }
        });
}

pub(super) fn install_ow_edit(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_edit(move |id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            if let Some(op) = st.openers.borrow().get(&id) {
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_id(op.id.clone().into());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_name(op.label.clone().into());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_program(op.program.clone().into());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_icon_kind(effective_opener_icon(op).as_i32());
                // OS/Store app (assoc, without exe) → Program field frozen.
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_is_store(op.assoc.is_some());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_args(join_args(&op.args).into());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_default_ext(op.default_exts.join(", ").into());
                // Learned/manual extensions → offered in "Open with".
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_used_ext(op.used_exts.join(", ").into());
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_elevated(op.elevated); // "run as admin"
                // Pinning to the context menu.
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_ctx_file(op.ctx_menu & openers::CTX_FILE != 0);
                // Empty in an opener saved before this field existed → show the
                // wildcard rather than a blank that would read as "none".
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_ctx_ext(if op.ctx_exts.is_empty() {
                        openers::CTX_EXT_ALL.to_string().into()
                    } else {
                        op.ctx_exts.join(", ").into()
                    });
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_ctx_dir(op.ctx_menu & openers::CTX_DIR != 0);
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_ctx_bg(op.ctx_menu & openers::CTX_BACKGROUND != 0);
                w.global::<crate::SettingsApi>().set_ow_popup_add(true);
                w.global::<crate::SettingsApi>().set_ow_popup_open(true);
                w.global::<crate::SettingsApi>()
                    .set_ow_popup_focus_armed(true);
            }
            recompute_ow_preview(&w, &st);
        });
}

pub(super) fn install_ow_delete(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_delete(move |id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            st.openers.borrow_mut().remove(&id);
            save_openers(&st);
            push_openers_ui(&w, &st);
        });
}

pub(super) fn install_ow_duplicate(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_duplicate(move |id: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            if st.openers.borrow_mut().duplicate(&id).is_some() {
                save_openers(&st);
            }
            push_openers_ui(&w, &st);
        });
}

pub(super) fn install_ow_move(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_move(move |id: SharedString, dir: i32| {
            let Some(w) = weak.upgrade() else { return };
            {
                let mut s = st.openers.borrow_mut();
                let id = id.to_string();
                if let Some(pos) = s.openers.iter().position(|o| o.id == id) {
                    let n = s.openers.len();
                    let target = if dir < 0 {
                        pos.checked_sub(1)
                    } else if pos + 1 < n {
                        Some(pos + 1)
                    } else {
                        None
                    };
                    if let Some(t) = target {
                        s.openers.swap(pos, t);
                    }
                }
            }
            save_openers(&st);
            push_openers_ui(&w, &st);
        });
}

pub(super) fn install_ow_popup_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_popup_changed(move || {
            if let Some(w) = weak.upgrade() {
                recompute_ow_preview(&w, &st);
            }
        });
}

pub(super) fn install_ow_popup_commit(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SettingsApi>()
        .on_ow_popup_commit(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w
                .global::<crate::SettingsApi>()
                .get_ow_popup_id()
                .to_string();
            let label = w
                .global::<crate::SettingsApi>()
                .get_ow_popup_name()
                .to_string();
            let program = w
                .global::<crate::SettingsApi>()
                .get_ow_popup_program()
                .to_string();
            let args = split_args(&w.global::<crate::SettingsApi>().get_ow_popup_args());
            let exts = parse_exts(&w.global::<crate::SettingsApi>().get_ow_popup_default_ext());
            // "Compatible" extensions (used_exts) edited by hand → the opener
            // is listed in the "Open with" flyout for these.
            let used = parse_exts(&w.global::<crate::SettingsApi>().get_ow_popup_used_ext());
            // Run as administrator (Windows) — checkbox checked in the popup.
            let elevated = w.global::<crate::SettingsApi>().get_ow_popup_elevated();
            let icon = openers::OpenerIcon::from_i32(
                w.global::<crate::SettingsApi>().get_ow_popup_icon_kind(),
            );
            // Pinning to the context menu: 3 checkboxes → bitmask.
            // A lone "*" is stored as-is: `matches_ctx_ext` reads it as the
            // wildcard, and an empty field means the same thing.
            let ctx_exts = parse_exts(&w.global::<crate::SettingsApi>().get_ow_popup_ctx_ext());
            let ctx_mask = (if w.global::<crate::SettingsApi>().get_ow_popup_ctx_file() {
                openers::CTX_FILE
            } else {
                0
            }) | (if w.global::<crate::SettingsApi>().get_ow_popup_ctx_dir() {
                openers::CTX_DIR
            } else {
                0
            }) | (if w.global::<crate::SettingsApi>().get_ow_popup_ctx_bg() {
                openers::CTX_BACKGROUND
            } else {
                0
            });
            if !id.is_empty() {
                st.openers.borrow_mut().update(&id, &label, &program, args);
                st.openers.borrow_mut().set_icon(&id, icon);
                apply_default_exts(&st, &id, &exts);
                st.openers.borrow_mut().set_used_exts(&id, &used);
                st.openers.borrow_mut().set_elevated(&id, elevated);
                st.openers.borrow_mut().set_ctx_menu(&id, ctx_mask);
                st.openers.borrow_mut().set_ctx_exts(&id, &ctx_exts);
                save_openers(&st);
            } else if w.global::<crate::SettingsApi>().get_ow_popup_add() {
                let new_id = st.openers.borrow_mut().add(&label, &program, args);
                st.openers.borrow_mut().set_icon(&new_id, icon);
                apply_default_exts(&st, &new_id, &exts);
                st.openers.borrow_mut().set_used_exts(&new_id, &used);
                st.openers.borrow_mut().set_elevated(&new_id, elevated);
                st.openers.borrow_mut().set_ctx_menu(&new_id, ctx_mask);
                st.openers.borrow_mut().set_ctx_exts(&new_id, &ctx_exts);
                save_openers(&st);
            } else {
                // One-shot: temporary opener, launched on the selection, not saved.
                let temp = openers::Opener {
                    id: String::new(),
                    label,
                    program,
                    assoc: None,
                    icon: openers::OpenerIcon::from_i32(
                        w.global::<crate::SettingsApi>().get_ow_popup_icon_kind(),
                    ),
                    args,
                    default_exts: Vec::new(),
                    used_exts: Vec::new(),
                    use_count: 0,
                    last_used: 0,
                    elevated,
                    ctx_menu: 0,
                    ctx_exts: Vec::new(),
                };
                if let Err(err) = actions::run_opener(&temp, &selected_paths(&st)) {
                    error!(error = %err, "one-shot opener failed");
                }
            }
            push_openers_ui(&w, &st);
        });
}
