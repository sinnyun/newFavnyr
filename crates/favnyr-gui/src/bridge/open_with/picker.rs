use super::*;

/// Clamped position of the context menu so it doesn't overflow the window.
/// The dimensions follow the Slint template (`width: 240px`; height 402 px
/// for the full menu, 220 px for the "empty area" menu, see `ctx-on-empty`).
pub(in crate::bridge) fn clamp_ctx_menu_pos(
    w: &MainWindow,
    x: f32,
    y: f32,
    menu_h: f32,
) -> (f32, f32) {
    const MENU_W: f32 = 240.0;
    let win_size = w.window().size();
    let scale = w.window().scale_factor().max(0.01);
    let win_w = win_size.width as f32 / scale;
    let win_h = win_size.height as f32 / scale;
    (
        x.min(win_w - MENU_W - 4.0).max(4.0),
        y.min(win_h - menu_h - 4.0).max(4.0),
    )
}

/// Is the selection an executable (→ "Use as application")?
pub(in crate::bridge) fn is_executable_path(p: &Path) -> bool {
    #[cfg(windows)]
    {
        matches!(
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref(),
            Some("exe") | Some("com")
        )
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        p.is_file()
            && p.metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
}

/// "Launchable by drop" target: broader than `is_executable_path` —
/// includes batch SCRIPTS, which are launched with the dropped files as arguments.
/// Windows: exe/com/cmd/bat. Other OSes: executable bit AND a type plausibly a
/// program (`FileKind::can_be_program`) → an "executable" image/doc (mode
/// 0777) is NOT a target. MUST stay consistent with the rows' `drop_runnable`
/// field (same hover feedback as the actual action).
pub(in crate::bridge) fn is_drop_runnable(p: &Path) -> bool {
    #[cfg(windows)]
    {
        matches!(
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref(),
            Some("exe") | Some("com") | Some("cmd") | Some("bat")
        )
    }
    #[cfg(not(windows))]
    {
        is_executable_path(p)
            && rfs::classify_kind(p.extension().and_then(|e| e.to_str()), false).can_be_program()
    }
}

/// Builds a `slint::Image` from the exe's icon (empty if unavailable).
pub(in crate::bridge) fn opener_icon(path: &str) -> Image {
    match openwith::icon_rgba(path) {
        Some((rgba, w, h)) => Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &rgba, w, h,
        )),
        None => Image::default(),
    }
}

/// Filters the already-built picker rows. Images are shared when cloned, so
/// typing never re-enumerates the OS or extracts executable icons again.
pub(in crate::bridge) fn filtered_ow_picker_items(
    items: &[OpenerItem],
    filter: &str,
) -> Vec<OpenerItem> {
    let needle = filter.trim().to_lowercase();
    items
        .iter()
        .filter(|item| needle.is_empty() || item.label.to_lowercase().contains(&needle))
        .cloned()
        .collect()
}

pub(in crate::bridge) fn push_filtered_ow_picker_ui(
    window: &MainWindow,
    state: &AppState,
    filter: &str,
) {
    let visible = state
        .ow_pick_ctx
        .borrow()
        .as_ref()
        .map(|context| filtered_ow_picker_items(&context.items, filter))
        .unwrap_or_default();
    window.set_ow_picker_handlers(ModelRc::new(VecModel::from(visible)));
}

/// Rebuilds both the visible application list and the handler context used when
/// the user picks an entry. Keeping them together prevents stale selections.
pub(in crate::bridge) fn refresh_ow_picker_handlers(
    window: &MainWindow,
    state: &AppState,
    ext: &str,
    path: &Path,
    include_without_mime: bool,
) {
    let mut handlers = openwith::handlers_for_ext(ext, include_without_mime);
    handlers.sort_by(|a, b| {
        b.recommended
            .cmp(&a.recommended)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let items: Vec<OpenerItem> = handlers
        .iter()
        .map(|handler| OpenerItem {
            id: handler.key.clone().into(),
            label: handler.name.clone().into(),
            available: true,
            icon: opener_icon(handler.exe.as_deref().unwrap_or_default()),
            icon_kind: openers::OpenerIcon::None.as_i32(),
        })
        .collect();
    *state.ow_pick_ctx.borrow_mut() = Some(OwPickCtx {
        ext: ext.to_owned(),
        path: path.to_owned(),
        handlers,
        items,
    });
    let filter = window.get_ow_picker_search_text();
    push_filtered_ow_picker_ui(window, state, filter.as_str());
}

pub(in crate::bridge) fn opener_matches_filter(opener: &openers::Opener, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {} {} {}",
        opener.label,
        opener.program,
        opener.assoc.as_deref().unwrap_or_default(),
        opener.args.join(" "),
        opener.default_exts.join(" "),
        opener.used_exts.join(" ")
    )
    .to_lowercase();
    haystack.contains(needle)
}

/// Returns the persisted recipe icon, with a lightweight fallback for commands
/// created by Favnyr versions that predate the `icon` field. Matching the
/// executable leaf also gives manually written commands for these well-known
/// tools the unsurprising icon, without probing the filesystem.
pub(in crate::bridge) fn effective_opener_icon(opener: &openers::Opener) -> openers::OpenerIcon {
    if opener.icon != openers::OpenerIcon::None {
        return opener.icon;
    }
    let program = opener.program.trim().to_ascii_lowercase();
    let leaf = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program.as_str());
    let leaf = leaf.strip_suffix(".exe").unwrap_or(leaf);
    match leaf {
        "7z" | "7zz" | "7za" => openers::OpenerIcon::SevenZip,
        "tar" => openers::OpenerIcon::Archive,
        "xdg-email" => openers::OpenerIcon::Email,
        "kdeconnect-handler" | "bluetooth-sendto" | "blueman-sendto" => openers::OpenerIcon::Device,
        _ => openers::OpenerIcon::None,
    }
}

/// Builds the shared Slint row for Settings, the Open-with flyout and pinned
/// context-menu commands. Built-in SVGs need no executable-icon extraction.
pub(in crate::bridge) fn opener_to_item(opener: &openers::Opener) -> OpenerItem {
    let icon = effective_opener_icon(opener);
    OpenerItem {
        id: opener.id.clone().into(),
        label: opener.label.clone().into(),
        available: opener.assoc.is_some() || Path::new(&opener.program).is_file(),
        icon: if icon == openers::OpenerIcon::None {
            opener_icon(&opener.program)
        } else {
            Image::default()
        },
        icon_kind: icon.as_i32(),
    }
}

/// Applies the filter to the cache of already-built items. Cloning a Slint `Image`
/// is a resource share; no Shell extraction or `Path::is_file` happens here.
pub(in crate::bridge) fn push_filtered_openers_ui(window: &MainWindow, state: &AppState) {
    let needle = state.opener_filter.borrow().trim().to_lowercase();
    let store = state.openers.borrow();
    let cache = state.opener_settings_cache.borrow();
    let visible: Vec<OpenerItem> = cache
        .iter()
        .filter(|item| {
            store
                .get(item.id.as_str())
                .is_some_and(|opener| opener_matches_filter(opener, &needle))
        })
        .cloned()
        .collect();
    window.set_openers_all(ModelRc::new(VecModel::from(visible)));
}

/// Pushes the opener lists to the GUI (suggested submenu + Settings list).
/// The "Open with" flyout is filtered by the selected file's EXTENSION
/// → only suited programs, no more mixing. The Settings
/// list is built once, cached, then filtered in memory.
pub(in crate::bridge) fn push_openers_ui(window: &MainWindow, state: &AppState) {
    // Extension of the 1st selected file (empty → falls back to global MRU).
    let ext = selected_paths(state)
        .first()
        .and_then(|p| {
            p.extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
        })
        .unwrap_or_default();
    let (suggested, all) = {
        let st = state.openers.borrow();
        let suggested: Vec<OpenerItem> = st
            .suggested_for_ext(&ext, 8)
            .into_iter()
            .map(opener_to_item)
            .collect();
        let all: Vec<OpenerItem> = st.openers.iter().map(opener_to_item).collect();
        (suggested, all)
    };
    window.set_openers_suggested(ModelRc::new(VecModel::from(suggested)));
    *state.opener_settings_cache.borrow_mut() = all;
    push_filtered_openers_ui(window, state);
}

/// Pushes into `ctx-custom-entries` the commands pinned for context
/// `bit` (CTX_FILE / CTX_DIR / CTX_BACKGROUND) and returns their count — used
/// on every context menu OPENING for both content AND height.
pub(in crate::bridge) fn push_ctx_custom_entries(
    window: &MainWindow,
    state: &AppState,
    bit: u8,
    targets: &[PathBuf],
) -> usize {
    let store = state.openers.borrow();
    let items: Vec<OpenerItem> = store
        .for_context(bit)
        .into_iter()
        .filter(|o| ctx_entry_applies(o, bit, targets))
        .map(opener_to_item)
        .collect();
    let n = items.len();
    window.set_ctx_custom_entries(ModelRc::new(VecModel::from(items)));
    n
}

/// Does a pinned command belong in the menu for `targets`?
///
/// Only the FILE entry is filtered — folders and the view background have no
/// extension. EVERY selected file must match: the command runs on all of them,
/// so offering it when some would fail is a trap. A single selection, the
/// common case, is unaffected either way.
pub(in crate::bridge) fn ctx_entry_applies(
    opener: &openers::Opener,
    bit: u8,
    targets: &[PathBuf],
) -> bool {
    if bit != openers::CTX_FILE || opener.ctx_exts.is_empty() {
        return true;
    }
    targets.iter().all(|p| {
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        opener.matches_ctx_ext(&ext)
    })
}
