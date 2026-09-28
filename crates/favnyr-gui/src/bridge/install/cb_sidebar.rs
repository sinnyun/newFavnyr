use super::*;

pub(super) fn install_sidebar_section_state_changed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SidebarApi>()
        .on_sidebar_section_state_changed(move |section: i32, collapsed: bool| {
            let mut sections = st.sidebar_sections.get();
            let target = match SidebarSection::from_index(section as usize) {
                Some(SidebarSection::Drives) => &mut sections.drives_collapsed,
                Some(SidebarSection::Shortcuts) => &mut sections.shortcuts_collapsed,
                Some(SidebarSection::Favorites) => &mut sections.favorites_collapsed,
                Some(SidebarSection::Network) => &mut sections.network_collapsed,
                _ => return,
            };
            if *target != collapsed {
                *target = collapsed;
                st.sidebar_sections.set(sections);
                if let Some(w) = weak.upgrade() {
                    update_window_title(&w, &st);
                }
            }
        });
}

pub(super) fn install_sidebar_section_reordered(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SidebarApi>()
        .on_sidebar_section_reordered(move |section: i32, target_index: i32| {
            let Some(section) = usize::try_from(section)
                .ok()
                .and_then(SidebarSection::from_index)
            else {
                return;
            };
            let Ok(target_index) = usize::try_from(target_index) else {
                return;
            };
            if target_index >= SidebarSection::COUNT {
                return;
            }

            let mut order = st.snapshot_config().sidebar_section_order;
            if SidebarSection::reorder(&mut order, section, target_index) {
                st.persist_config(|cfg| cfg.sidebar_section_order = order);
                if let Some(w) = weak.upgrade() {
                    push_sidebar_sections_ui(&w, &st);
                }
            }
        });
}

pub(super) fn install_sidebar_place_clicked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SidebarApi>()
        .on_sidebar_place_clicked(move |path: SharedString, kind: i32| {
            let Some(w) = weak.upgrade() else { return };
            // kind 5 = MTP portable device. This is NOT a file path: the
            // handle stays opaque and goes back to the platform backend that
            // produced it, which opens the device in the desktop's own file
            // manager.
            // kind 7 = an encrypted volume, still locked. Unlocking it is
            // deliberately not offered: it would mean holding a passphrase,
            // which Favnyr never does. Saying so beats a click that does
            // nothing.
            if kind == 7 {
                let lang = st.snapshot_config().language;
                show_notice_unavailable(&w, i18n::tr(lang, "volume_locked"));
                return;
            }
            // kind 6 = a volume the machine sees but has not mounted, with
            // the block device travelling in `path`. Mounting blocks for as
            // long as an authorisation agent keeps its prompt open, so it runs
            // off the UI thread and its outcome comes back through the event
            // loop. Favnyr never sees the password: polkit runs its own agent.
            if kind == 6 {
                let device = path.to_string();
                let lang = st.snapshot_config().language;
                let weak_window = w.as_weak();
                std::thread::spawn(move || {
                    let outcome = favnyr_core::mount::mount(&device);
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(w) = weak_window.upgrade() else {
                            return;
                        };
                        match outcome {
                            Ok(mount_point) => {
                                // The volume just left the unmounted list for
                                // the drives: the panel is rebuilt before the
                                // view moves into it.
                                w.global::<crate::SidebarApi>().invoke_sidebar_refresh();
                                w.global::<crate::OperationsApi>()
                                    .invoke_navigate_to(mount_point.display().to_string().into());
                            }
                            Err(err) => {
                                show_notice_unavailable(&w, i18n::mount_error_message(lang, &err))
                            }
                        }
                    });
                });
                return;
            }
            #[cfg(any(windows, target_os = "linux"))]
            if kind == 5 {
                if let Err(err) = open_portable_device(path.as_str()) {
                    error!(error = %err, "open portable device");
                }
                return;
            }
            // kind 3 = trash. On Windows it's virtual (empty path)
            // → open it in Explorer; elsewhere, navigate into it.
            #[cfg(windows)]
            if kind == 3 {
                if let Err(err) = actions::open_trash() {
                    error!(error = %err, "open recycle bin");
                }
                return;
            }
            let _ = kind;
            let p = PathBuf::from(path.to_string());
            if p.as_os_str().is_empty() {
                return;
            }
            // `is_dir()` on a NETWORK path = a full SMB round trip (possibly
            // even a session reconnect: seconds) ON THE UI THREAD, before the
            // listing even happens. We only pre-check LOCAL paths (instant); the network
            // case is resolved by the listing itself (failure → "unavailable" banner,
            // same safety net as Explorer).
            if favnyr_core::places::is_network_path(&p) || p.is_dir() {
                load_directory(&w, &st, &p, true);
            } else {
                warn!(path = %p.display(), "sidebar: target no longer exists, ignoring");
            }
        });
}

pub(super) fn install_place_open_new_tab(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SidebarApi>().on_place_open_new_tab(
        move |path: SharedString, kind: i32| {
            let Some(w) = weak.upgrade() else { return };
            // Trash (3), portable device (5) and volumes that are not mounted
            // (6, 7) have no path Favnyr can navigate: middle-click "new tab"
            // does not apply. The last two carry a block device in `path`,
            // which the guard below would reject anyway — saying so here states
            // the intent instead of relying on that.
            if kind == 3 || kind == 5 || kind == 6 || kind == 7 {
                return;
            }
            // Same resolver as a drag from the sidebar: network paths are
            // handed to the asynchronous listing without a blocking probe.
            let Some(p) = resolve_sidebar_place_dir(path.as_str()) else {
                return;
            };
            let opened = st.with_tabs_mut(|book| {
                let a = book.open_after_active(p);
                book.tabs[a].current_path.clone()
            });
            load_directory(&w, &st, &opened, false);
        },
    );
}

pub(super) fn install_sidebar_refresh(window: &MainWindow, state: AppState) {
    let weak = window.as_weak();
    let st = state.clone();
    window
        .global::<crate::SidebarApi>()
        .on_sidebar_refresh(move || {
            if let Some(w) = weak.upgrade() {
                #[cfg(windows)]
                crate::winportable::request_refresh();
                refresh_sidebar(&w, &st);
            }
        });
}

pub(super) fn install_init_ipc(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SidebarApi>()
        .on_init_ipc(move || -> bool {
            let tab_state = st.clone();
            let tab_weak = weak.clone();
            let ipc_ready = crate::winmsg::init(move |incoming| {
                let Some(w) = tab_weak.upgrade() else { return };
                match incoming {
                    crate::winmsg::Incoming::Transfer(payload) => {
                        // `on_tab_received` handles the favorites hover itself (replays
                        // the drop point then clears the preview).
                        on_tab_received(&w, &tab_state, &payload);
                    }
                    // Hover from another instance: simulates a local drag at
                    // this point → the insertion preview lights up in the targeted panel.
                    crate::winmsg::Incoming::Hover(sx, sy) => set_external_hover(&w, sx, sy),
                    crate::winmsg::Incoming::HoverEnd => clear_external_hover(&w),
                    // A device was plugged in or removed. The scan runs off the
                    // UI thread and publishes a new revision, which the drives
                    // poll below turns into a sidebar rebuild.
                    crate::winmsg::Incoming::DevicesChanged => {
                        #[cfg(windows)]
                        crate::winportable::request_refresh();
                    }
                }
            });
            if !ipc_ready {
                return false;
            }

            // OLE file target: replaces the winit target (which doesn't surface
            // DragOver) to get a real-time folder/executable hover
            // and reuse the Move/Copy/Link menu between Favnyr instances.
            #[cfg(windows)]
            {
                let Some(hwnd) = crate::winmsg::self_hwnd() else {
                    return false;
                };
                let file_state = st.clone();
                let file_weak = weak.clone();
                let registered = crate::winddrag::init_drop_target(hwnd, move |incoming| {
                    match incoming {
                        crate::winddrag::IncomingFileDrag::Hover {
                            screen_x,
                            screen_y,
                            copy,
                        } => {
                            if let Some(w) = file_weak.upgrade() {
                                set_external_file_hover(&w, screen_x, screen_y, copy);
                            }
                        }
                        crate::winddrag::IncomingFileDrag::Leave => {
                            if let Some(w) = file_weak.upgrade() {
                                clear_external_file_hover(&w);
                            }
                        }
                        crate::winddrag::IncomingFileDrag::Drop {
                            paths,
                            screen_x,
                            screen_y,
                            copy,
                            staging,
                        } => {
                            // Own the staging directory before deferring. If
                            // the window closes before the next UI tick, RAII
                            // still removes it instead of leaking temp data.
                            let staging = staging.map(|staging| IncomingDropStaging {
                                cleanup: TransientDropGuard::new(staging.temp_dir),
                                op: if staging.copy_from_staging {
                                    ClipOp::Copy
                                } else {
                                    ClipOp::Cut
                                },
                            });
                            // The OLE source still holds the capture until
                            // IDropTarget::Drop returns control. Focus and open
                            // the menu on the next tick, after OLE has released
                            // capture, so its first hover/click is usable.
                            let drop_state = file_state.clone();
                            let drop_weak = file_weak.clone();
                            defer(move || {
                                let Some(w) = drop_weak.upgrade() else { return };
                                crate::winmsg::focus_self();
                                on_external_file_drop(
                                    &w,
                                    &drop_state,
                                    paths,
                                    screen_x,
                                    screen_y,
                                    copy,
                                    staging,
                                );
                            });
                        }
                        crate::winddrag::IncomingFileDrag::ExternalDropFailed => {
                            let drop_state = file_state.clone();
                            let drop_weak = file_weak.clone();
                            defer(move || {
                                let Some(w) = drop_weak.upgrade() else { return };
                                crate::winmsg::focus_self();
                                clear_external_file_hover(&w);
                                let lang = drop_state.snapshot_config().language;
                                notice(
                                    &w,
                                    i18n::tr(lang, "virtual_drop_failed"),
                                    NoticeKind::Error,
                                );
                            });
                        }
                    }
                });
                if registered {
                    // This delay starts while the event loop is already
                    // running, unlike the declarative 250 ms startup timer.
                    // Rebinding once avoids winit reclaiming the HWND's minimal
                    // CF_HDROP target while the native window is finalized.
                    slint::Timer::single_shot(std::time::Duration::from_millis(500), move || {
                        crate::winddrag::rebind_drop_target(hwnd);
                    });
                }
                registered
            }
            #[cfg(not(windows))]
            true
        });
}

pub(super) fn install_panel_tabs_scrolled(window: &MainWindow, state: AppState) {
    let st = state.clone();
    window
        .global::<crate::PanelsApi>()
        .on_panel_tabs_scrolled(move |idx: i32, vx: f32| {
            let mut panels = st.panels.borrow_mut();
            if let Some(p) = panels.get_mut(idx.max(0) as usize) {
                p.tabs_viewport_x = vx;
            }
        });
}

pub(super) fn install_poll_drives(window: &MainWindow, state: AppState) {
    let weak = window.as_weak();
    let sig = state.last_drives_sig.clone();
    let st = state.clone();
    // Free space moves without any mount changing, so the mount signature
    // alone would leave every capacity gauge frozen until the next plug or
    // unplug. It gets its own check, on a slower beat: a capacity is worth
    // re-reading every few seconds, not every one and a half, and the
    // reading costs a `statvfs` per volume.
    let space_beat = Cell::new(0u32);
    let space_sig = Cell::new(drives_space_signature(state.snapshot_config().language));
    window
        .global::<crate::SidebarApi>()
        .on_poll_drives(move || {
            let Some(w) = weak.upgrade() else { return };
            // Plug and unplug now arrive as a device-change message, so the
            // Shell is no longer enumerated on every beat: walking "This PC"
            // instantiates each namespace extension registered there (cloud
            // clients, vendor drivers) inside this process, which is far too
            // much to repeat every second and a half for an event the system
            // already announces. The one case the message cannot cover is a
            // driver still initializing when it fired — the scan is then
            // retried until it resolves, and only until then.
            #[cfg(windows)]
            if crate::winportable::has_unresolved() {
                crate::winportable::request_refresh();
            }
            let lang = st.snapshot_config().language;
            let mut stale = false;
            let cur = sidebar_drives_signature();
            if cur != sig.get() {
                sig.set(cur);
                stale = true;
            }
            // One beat in eight of the 1.5 s poll, so roughly every 12 s.
            let beat = space_beat.get().wrapping_add(1);
            space_beat.set(beat);
            if beat.is_multiple_of(8) {
                let cur_space = drives_space_signature(lang);
                if cur_space != space_sig.get() {
                    space_sig.set(cur_space);
                    stale = true;
                }
            }
            if stale {
                refresh_sidebar(&w, &st);
            }
        });
}

pub(super) fn install_recheck_unavailable(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    let last_sig = std::cell::Cell::new(favnyr_core::places::drives_signature());
    window
        .global::<crate::SidebarApi>()
        .on_recheck_unavailable(move || {
            let Some(w) = weak.upgrade() else { return };
            let cur = favnyr_core::places::drives_signature();
            if cur == last_sig.get() {
                return; // nothing changed on the drives side → no blocking test
            }
            last_sig.set(cur);
            recheck_unavailable_panels(&w, &st);
        });
}

pub(super) fn install_folder_color_picked(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_folder_color_picked(move |slot: i32| {
            let Some(w) = weak.upgrade() else { return };
            let targets = selected_paths(&st);
            {
                let mut annotations = annotations_for_update(&st);
                for path in targets.iter().filter(|p| acts_as_dir(p)) {
                    // Slot 0 clears the entry rather than storing a default,
                    // which is what makes the first swatch a reset.
                    annotations.set_color(path, u8::try_from(slot).unwrap_or(0));
                }
                // A colour is cosmetic: a failure to persist it is logged and
                // the view updates anyway.
                save_annotations(&st, &annotations);
            }
            // The colour is baked into every view's rows, not just the active
            // one — the same folder may be open in several panels.
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_open_comment_editor(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_open_comment_editor(move || {
            let Some(w) = weak.upgrade() else { return };
            // Offered on a single selection only, so the first item IS the
            // target. The popup is modal, so it cannot drift afterwards.
            let Some(path) = selected_paths(&st).into_iter().next() else {
                return;
            };
            w.global::<crate::PanelsApi>()
                .set_comment_target_name(path_notice_name(&path).into());
            w.global::<crate::PanelsApi>()
                .set_comment_text(annotations_now(&st).note_of(&path).into());
            w.global::<crate::MenuApi>().set_comment_popup_open(true);
        });
}

pub(super) fn install_comment_confirmed(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::MenuApi>()
        .on_comment_confirmed(move |text: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            let Some(path) = selected_paths(&st).into_iter().next() else {
                return;
            };
            {
                let mut annotations = annotations_for_update(&st);
                // A blank note clears the entry rather than storing an empty
                // string, which is what makes "Clear" then "Save" a removal.
                annotations.set_note(&path, text.as_str());
                save_annotations(&st, &annotations);
            }
            refresh_all_panels(&w, &st);
        });
}

pub(super) fn install_drive_eject(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window.global::<crate::SidebarApi>().on_drive_eject(
        move |device: SharedString, hotplug: bool| {
            let Some(w) = weak.upgrade() else { return };
            spawn_eject(&w, &st, device.to_string(), EjectOp::SafeRemove, hotplug);
        },
    );
}

pub(super) fn install_drive_disconnect(window: &MainWindow, state: AppState) {
    let st = state.clone();
    let weak = window.as_weak();
    window
        .global::<crate::SidebarApi>()
        .on_drive_disconnect(move |device: SharedString| {
            let Some(w) = weak.upgrade() else { return };
            // A mapped network drive is disconnected, never unplugged.
            spawn_eject(&w, &st, device.to_string(), EjectOp::Disconnect, false);
        });
}
