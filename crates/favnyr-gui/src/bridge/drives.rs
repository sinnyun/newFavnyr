use super::*;

/// Eject operation requested from the drive menu.
#[derive(Clone, Copy)]
pub(super) enum EjectOp {
    /// Safe removal of a removable drive (USB/CD).
    SafeRemove,
    /// Disconnection of a mapped network drive.
    Disconnect,
}

/// Runs the eject/disconnect on a background thread (may block) then
/// comes back to the UI: result toast + sidebar re-scan on
/// success.
///
/// Before the removal, Favnyr releases its own handles on the volumes. The active
/// panel's `notify` watcher holds a directory handle that can block
/// the eject. It is therefore dropped unconditionally, then `invoke_refresh()`
/// re-lists the active panel and re-arms the watcher on return.
/// `hotplug` says whether the hardware can actually be unplugged. It is NOT
/// recomputed here: the sidebar already established it per platform — from the
/// sysfs bus on Linux, from a storage IOCTL on Windows — and deriving it a
/// second time from the device string alone would silently lose the Windows
/// answer, leaving the menu and the toast contradicting each other.
pub(super) fn spawn_eject(
    window: &MainWindow,
    state: &AppState,
    device: String,
    op: EjectOp,
    hotplug: bool,
) {
    // First invalidate any watcher installation still in flight — otherwise
    // it could resurrect a handle on the volume being ejected.
    state.watcher_gen.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut w) = state.watcher.lock() {
        *w = None; // releases the OS handle (otherwise it self-vetoes)
    }
    let weak = window.as_weak();
    let lang = state.snapshot_config().language;
    std::thread::spawn(move || {
        let res = match op {
            EjectOp::SafeRemove => favnyr_core::eject::safe_remove(&device, hotplug),
            EjectOp::Disconnect => favnyr_core::eject::disconnect(&device),
        };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(w) = weak.upgrade() else { return };
            let s = w.get_strings();
            match res {
                Ok(()) => {
                    show_notice_ok(
                        &w,
                        match op {
                            EjectOp::SafeRemove if hotplug => s.net_ejected,
                            EjectOp::SafeRemove => s.net_released,
                            EjectOp::Disconnect => s.net_disconnected,
                        },
                    );
                    // This runs back on the UI thread but outside any state
                    // handle: the closure crossed a thread boundary, so it
                    // cannot carry one. The window's own refresh entry point
                    // is invoked instead — one implementation of the re-scan,
                    // not a second one that could drift.
                    w.invoke_sidebar_refresh(); // the drive is gone
                }
                Err(err) => {
                    let reason = i18n::eject_error_message(lang, &err);
                    show_notice(&w, format!("{}: {reason}", s.net_eject_failed));
                }
            }
            w.invoke_refresh(); // re-lists the active panel + re-arms the watcher
        });
    });
}

/// (Re)builds the "Places" sidebar model (drives, shortcuts,
/// network, trash) from `favnyr-core::places` and pushes it to the UI.
/// Fingerprint of what the capacity gauges currently DISPLAY.
///
/// Hashes the rendered figures and warning levels, never the raw byte counts:
/// a volume ticking over by a few kilobytes changes its bytes constantly while
/// the text stays "38.0 / 64.0 GB". Comparing what is drawn means the sidebar is
/// rebuilt only when the user would actually see a difference, which keeps the
/// existing "rebuild only if changed" contract intact — a rebuild replaces the
/// row models and would otherwise churn under an idle disk.
pub(super) fn drives_space_signature(lang: Lang) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for place in favnyr_core::places::drives() {
        let space = drive_space_ui(&place, lang);
        place.path.hash(&mut hasher);
        space.level.hash(&mut hasher);
        space.text.hash(&mut hasher);
        space.text_short.hash(&mut hasher);
        space.hint.hash(&mut hasher);
    }
    hasher.finish()
}

/// What the capacity gauge shows for one place. Level `-1` means no gauge.
///
/// Only a local volume that answered gets one. A share is never measured (the
/// call leaves the machine and the answer would describe the server), and
/// neither is a shortcut or the trash, which live on a volume already listed on
/// its own line — stating the same capacity twice under two names reads as a
/// contradiction, not as agreement.
pub(super) struct DriveSpaceUi {
    /// `-1` no gauge · `0` roomy · `1` low · `2` critical.
    level: i32,
    /// Share of the volume in use, 0..1 — the bar grows with it.
    used_ratio: f32,
    /// "26.0 / 57.3 GB", and the used figure alone for a narrow panel.
    text: String,
    text_short: String,
    /// One-line breakdown for the hover hint, where there is room to name
    /// each figure instead of leaving the reader to infer which is which.
    hint: String,
}

impl DriveSpaceUi {
    /// No capacity to show: not a local volume, or one that did not answer.
    fn none() -> Self {
        DriveSpaceUi {
            level: -1,
            used_ratio: 0.0,
            text: String::new(),
            text_short: String::new(),
            hint: String::new(),
        }
    }
}

pub(super) fn drive_space_ui(place: &favnyr_core::places::Place, lang: Lang) -> DriveSpaceUi {
    use favnyr_core::places::PlaceKind;
    // A volume nobody has mounted has a size but no occupancy: no filesystem is
    // open to report one. It shows the figure and no gauge — an empty bar would
    // claim the volume is empty, which is a different statement.
    if matches!(place.kind, PlaceKind::Volume | PlaceKind::LockedVolume) {
        if place.total_bytes == 0 {
            return DriveSpaceUi::none();
        }
        let size = rfs::format_size(place.total_bytes, i18n::size_units(lang));
        let hint = if place.kind == PlaceKind::LockedVolume {
            "volume_locked_hint"
        } else {
            "volume_not_mounted_hint"
        };
        return DriveSpaceUi {
            level: -1,
            used_ratio: 0.0,
            text: size.clone(),
            text_short: size.clone(),
            hint: i18n::tr(lang, hint).replace("{size}", &size),
        };
    }
    if place.kind != PlaceKind::Drive || place.total_bytes == 0 {
        return DriveSpaceUi::none();
    }
    let total = place.total_bytes;
    let free = place.free_bytes.min(total);
    let used = total - free;
    DriveSpaceUi {
        // The warning still comes from what is LEFT: that is the figure that
        // decides whether the next operation fits, whatever the gauge draws.
        level: rfs::free_space_level(free, total),
        used_ratio: used as f32 / total as f32,
        text: rfs::format_used_total(used, total, i18n::size_units(lang)),
        text_short: rfs::format_size(used, i18n::size_units(lang)),
        hint: i18n::tr(lang, "drive_space_hint")
            .replace("{free}", &rfs::format_size(free, i18n::size_units(lang)))
            .replace("{used}", &rfs::format_size(used, i18n::size_units(lang)))
            .replace("{total}", &rfs::format_size(total, i18n::size_units(lang))),
    }
}

/// Volumes seen but not mounted, re-read only when the block topology moved.
/// Listing them spawns a process, which is why the answer is kept: the sidebar
/// is rebuilt far more often than a disk is plugged in.
pub(super) fn cached_unmounted_volumes(state: &AppState) -> Vec<favnyr_core::places::Place> {
    // Two independent facts invalidate the answer: a disk appearing or
    // leaving, which moves the block topology; and a volume of this very list
    // becoming mounted, which must drop it from the list — and mounting does
    // NOT touch `/sys/class/block`. Both reads are plain files.
    let signature = favnyr_core::places::block_signature()
        ^ favnyr_core::places::drives_signature().rotate_left(32);
    let mut cache = state.volumes_cache.borrow_mut();
    if cache.0 != signature {
        *cache = (signature, favnyr_core::places::unmounted_volumes());
    }
    cache.1.clone()
}

pub(super) fn refresh_sidebar(window: &MainWindow, state: &AppState) {
    use favnyr_core::places::{self, PlaceKind};
    let lang = state.snapshot_config().language;
    // Icon code (see SidebarItem): 0 folder · 1 home · 2 drive · 3
    // trash · 4 network · 5 phone · 6 unmounted volume · 7 locked volume.
    fn kind_code(k: PlaceKind) -> i32 {
        match k {
            PlaceKind::Folder => 0,
            PlaceKind::Home => 1,
            PlaceKind::Drive => 2,
            PlaceKind::Trash => 3,
            PlaceKind::Network => 4,
            PlaceKind::Volume => 6,
            PlaceKind::LockedVolume => 7,
        }
    }
    let place_item = |p: places::Place| -> SidebarPlace {
        let space = drive_space_ui(&p, lang);
        SidebarPlace {
            label: p.name.into(),
            // A volume that is not mounted has no path yet. Like a portable
            // device, it travels as the handle its own backend understands —
            // here the block device that would be mounted — and `kind` tells
            // the click handler how to read it.
            path: if matches!(p.kind, PlaceKind::Volume | PlaceKind::LockedVolume) {
                p.device.clone().into()
            } else {
                p.path.display().to_string().into()
            },
            kind: kind_code(p.kind),
            removable: p.removable,
            hotplug: p.hotplug,
            device: p.device.into(),
            space_level: space.level,
            space_used_ratio: space.used_ratio,
            space_text: space.text.into(),
            space_text_short: space.text_short.into(),
            space_hint: space.hint.into(),
        }
    };
    let shortcuts = places::user_places()
        .into_iter()
        .map(place_item)
        .collect::<Vec<_>>();
    // Keep sections strictly grouped: local, Windows portable
    // devices, then network. An MTP isn't a core `Place`/`PathBuf`.
    let filesystem = places::drives();
    let mut filesystem_drives = filesystem
        .iter()
        .filter(|p| p.kind != PlaceKind::Network)
        .cloned()
        .map(place_item)
        .collect::<Vec<_>>();
    // Volumes present but not mounted come after the mounted ones: what is
    // reachable now reads first.
    filesystem_drives.extend(cached_unmounted_volumes(state).into_iter().map(place_item));
    #[cfg(any(windows, target_os = "linux"))]
    let drives = {
        let mut drives = filesystem_drives;
        for (label, handle) in portable_devices() {
            drives.push(SidebarPlace {
                label: label.into(),
                path: handle.into(),
                kind: 5,
                removable: false,
                // A phone is unplugged by hand, but it offers no release entry
                // — Favnyr never mounted it.
                hotplug: true,
                device: SharedString::new(),
                // A phone is addressed through an opaque handle, not a path a
                // filesystem call can measure; asking its capacity would be a
                // device query on the UI thread.
                space_level: -1,
                space_used_ratio: 0.0,
                space_text: SharedString::new(),
                space_text_short: SharedString::new(),
                space_hint: SharedString::new(),
            });
        }
        drives
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let drives = filesystem_drives;
    let network = filesystem
        .into_iter()
        .filter(|p| p.kind == PlaceKind::Network)
        .map(place_item)
        .collect::<Vec<_>>();
    let trash = vec![place_item(places::trash_place())];

    // The four models remain semantic and stable; only their Slint rank
    // varies with the global preference. Trash stays outside this ordering.
    window.set_sidebar_places_drives(ModelRc::new(VecModel::from(drives)));
    window.set_sidebar_places_shortcuts(ModelRc::new(VecModel::from(shortcuts)));
    window.set_sidebar_places_network(ModelRc::new(VecModel::from(network)));
    window.set_sidebar_places_trash(ModelRc::new(VecModel::from(trash)));
}

/// Pushes the workspace-specific collapse state and the global config order to Slint.
/// Called at startup, when loading a workspace, and on reset; no
/// parallel visual logic decides default values.
pub(super) fn push_sidebar_sections_ui(window: &MainWindow, state: &AppState) {
    let sections = state.sidebar_sections.get();
    window.set_sidebar_shortcuts_collapsed(sections.shortcuts_collapsed);
    window.set_fav_section_collapsed(sections.favorites_collapsed);
    window.set_sidebar_drives_collapsed(sections.drives_collapsed);
    window.set_sidebar_network_collapsed(sections.network_collapsed);
    window.set_sidebar_section_order(ModelRc::new(VecModel::from(
        state
            .snapshot_config()
            .sidebar_section_order
            .into_iter()
            .map(|section| section.index() as i32)
            .collect::<Vec<_>>(),
    )));
}

/// Portable devices (phones, cameras) listed next to the drives. They carry no
/// filesystem path, so each platform resolves them through its own backend and
/// returns `(label, handle)` — the handle is opaque, and only the backend that
/// produced it knows how to open it.
#[cfg(any(windows, target_os = "linux"))]
pub(super) fn portable_devices() -> Vec<(String, String)> {
    #[cfg(windows)]
    {
        crate::winportable::devices()
            .into_iter()
            .map(|device| (device.name, device.shell_path))
            .collect()
    }
    #[cfg(target_os = "linux")]
    {
        crate::linportable::devices()
            .into_iter()
            .map(|device| (device.name, device.uri))
            .collect()
    }
}

/// Opens a portable device through the backend that produced its handle.
#[cfg(any(windows, target_os = "linux"))]
pub(super) fn open_portable_device(handle: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        crate::winportable::open(handle)
    }
    #[cfg(target_os = "linux")]
    {
        crate::linportable::open(handle)
    }
}

/// Combined sidebar signature: core drive letters/mounts + the portable-device
/// list. The read is instant and never touches COM.
pub(super) fn sidebar_drives_signature() -> u64 {
    let filesystem = favnyr_core::places::drives_signature();
    #[cfg(windows)]
    {
        filesystem ^ crate::winportable::signature().rotate_left(32)
    }
    #[cfg(target_os = "linux")]
    {
        // Mounts, portable devices and block topology are three independent
        // sources: a disk plugged in but never mounted moves only the last.
        filesystem
            ^ crate::linportable::signature().rotate_left(32)
            ^ favnyr_core::places::block_signature().rotate_left(16)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        filesystem
    }
}
