use super::*;

/// Creates a Windows `<name>.lnk` shortcut in `cur` pointing to `target`.
/// Empty `name` → derived from the target's name. Adds the `.lnk` extension if
/// missing; refuses an invalid name or an already-existing target/`.lnk`. Returns the
/// created file name (for the cursor), or `None` on failure.
pub(in crate::bridge) fn create_shortcut_entry(
    cur: &Path,
    name: &str,
    target: &str,
) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let target_path = PathBuf::from(target);
    // Entered name, otherwise the target's name (without extension).
    let base = if name.is_empty() {
        target_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        name.to_string()
    };
    if base.is_empty() || !ops::is_valid_entry_name(&base) {
        error!(name = base, "invalid shortcut name");
        return None;
    }
    let file_name = if base.to_ascii_lowercase().ends_with(".lnk") {
        base
    } else {
        format!("{base}.lnk")
    };
    let lnk_path = cur.join(&file_name);
    if lnk_path.exists() {
        error!(path = %lnk_path.display(), "shortcut already exists");
        return None;
    }
    match openwith::create_shortcut(&lnk_path, &target_path) {
        Ok(()) => {
            info!(lnk = %lnk_path.display(), target, "created shortcut");
            Some(file_name)
        }
        Err(err) => {
            error!(error = %err, "create shortcut failed");
            None
        }
    }
}

/// "Create shortcut" (file context menu): creates in `cur` a
/// link named `name` to `target`. Symlink tab → `ops::link_as` (Unix symlink
/// / Windows junction|hardlink, like "Link here"); Shortcut tab → `.lnk`
/// (reuses `create_shortcut_entry`). Returns the created name (for the cursor).
pub(in crate::bridge) fn create_link_entry(
    window: &MainWindow,
    lang: Lang,
    cur: &Path,
    name: &str,
    target: &str,
) -> Option<String> {
    if name.is_empty() || target.is_empty() {
        return None;
    }
    // Symlink tab checked → direct link with the exact name; otherwise .lnk shortcut.
    if window
        .global::<crate::OperationsApi>()
        .get_create_link_symlink()
    {
        if !ops::is_valid_entry_name(name) {
            error!(name, "invalid link name");
            return None;
        }
        let dst = cur.join(name);
        match ops::link_as(&PathBuf::from(target), &dst) {
            Ok(()) => {
                info!(link = %dst.display(), target, "created symlink");
                Some(name.to_string())
            }
            Err(err) => {
                // Give visible feedback instead of failing silently. The full
                // reason (cross-volume, privilege, filesystem) goes to the log;
                // the toast stays concise.
                error!(error = %err, "create symlink failed");
                show_notice(window, i18n::tr(lang, "link_failed"));
                None
            }
        }
    } else {
        create_shortcut_entry(cur, name, target)
    }
}

/// Opens `dir` in a NEW tab of the active view (same mechanics as
/// `on_action_open_new_tab`). Used for folder `.lnk` shortcuts.
// Only used by the `.lnk` path (see `open_shortcut_as_tab`, Windows).
#[cfg(windows)]
pub(in crate::bridge) fn open_dir_in_new_tab(window: &MainWindow, state: &AppState, dir: &Path) {
    let opened = state.with_tabs_mut(|book| {
        let a = book.open_after_active(dir.to_path_buf());
        book.tabs[a].current_path.clone()
    });
    load_directory(window, state, &opened, false);
}

/// If `path` is a Windows `.lnk` shortcut pointing to a FOLDER, opens it
/// in a new tab of the active view and returns `true` (the caller stops
/// there); otherwise `false` (default opening). A `.lnk` to a FILE/app
/// falls back to normal opening (the shell launches the target). Always `false`
/// outside Windows: symbolic links there are followed natively by the listing
/// (a folder symlink is navigated like a folder).
pub(in crate::bridge) fn open_shortcut_as_tab(
    window: &MainWindow,
    state: &AppState,
    path: &Path,
) -> bool {
    #[cfg(windows)]
    {
        let is_lnk = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);
        if is_lnk
            && let Some(target) = crate::openwith::resolve_shortcut(path)
            && target.is_dir()
        {
            info!(lnk = %path.display(), target = %target.display(), "open .lnk folder in new tab");
            open_dir_in_new_tab(window, state, &target);
            return true;
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = (window, state, path);
        false
    }
}

/// Whether `p` should be treated as a FOLDER in the context menu: a real
/// directory, a folder symlink/junction (already followed by `Path::is_dir`), or
/// a Windows `.lnk` whose target is a directory. The `.lnk` case is a COM
/// resolution, so the cheap extension test gates it and it runs only for the
/// single right-clicked item — never in a hot loop.
pub(in crate::bridge) fn acts_as_dir(p: &Path) -> bool {
    if p.is_dir() {
        return true;
    }
    #[cfg(windows)]
    {
        if p.extension()
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            return crate::openwith::resolve_shortcut(p)
                .map(|t| t.is_dir())
                .unwrap_or(false);
        }
    }
    false
}

/// Lowercase extension of `path`, empty when it carries none.
pub(in crate::bridge) fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// One launch: the files handed to a single application in one go.
#[derive(Debug, PartialEq)]
pub(in crate::bridge) enum Launch {
    /// Files sharing a Favnyr opener travel together, so an editor opens one
    /// window holding all of them rather than one window each.
    Opener { id: String, paths: Vec<PathBuf> },
    /// No opener of its own: handed to the system, one launch per file — what
    /// the desktop file manager does.
    System(PathBuf),
}

/// Splits a selection into the launches that will open it, each file resolved
/// by ITS OWN extension: a text file and a picture chosen together each reach
/// their own application instead of both reaching the first one's.
///
/// Order is kept — a group appears where its first file did — so what opens
/// first is what the user sees first in the list.
pub(in crate::bridge) fn plan_open(
    paths: &[PathBuf],
    opener_for: impl Fn(&str) -> Option<String>,
) -> Vec<Launch> {
    let mut out: Vec<Launch> = Vec::new();
    for path in paths {
        let Some(id) = opener_for(&ext_of(path)) else {
            out.push(Launch::System(path.clone()));
            continue;
        };
        let group = out.iter_mut().find_map(|launch| match launch {
            Launch::Opener { id: other, paths } if *other == id => Some(paths),
            _ => None,
        });
        match group {
            Some(group) => group.push(path.clone()),
            None => out.push(Launch::Opener {
                id,
                paths: vec![path.clone()],
            }),
        }
    }
    out
}

/// Runs one opener over the files it was chosen for, and records the use so the
/// "Open with" list keeps its order of preference.
pub(in crate::bridge) fn run_default_opener(
    state: &AppState,
    op: &openers::Opener,
    paths: &[PathBuf],
    ext: &str,
) {
    if let Err(err) = actions::run_opener(op, paths) {
        error!(error = %err, "run default opener failed");
        return;
    }
    state.openers.borrow_mut().record_use(&op.id, Some(ext));
    save_openers(state);
}

/// Opens ONE file (never a folder): the Favnyr default for its extension if
/// there is one, otherwise the OS default.
pub(in crate::bridge) fn open_one_file_default(state: &AppState, path: &PathBuf) {
    let ext = ext_of(path);
    // Bound BEFORE the branch, and it must stay that way. The `Ref` a scrutinee
    // produces lives for the WHOLE body of an `if let`, and running the opener
    // takes the same cell mutably to record the use. Inlining this reads fine
    // and compiles fine; it panics at run time.
    let opener = state.openers.borrow().default_for(&ext).cloned();
    if let Some(op) = opener {
        run_default_opener(state, &op, std::slice::from_ref(path), &ext);
        return;
    }
    #[cfg(windows)]
    if should_try_image_gallery(&ext) {
        match actions::try_open_image_gallery(path, &ext) {
            Ok(true) => return,
            Ok(false) => {}
            Err(err) => {
                // Unreadable association, incompatible URI, or Photos protocol
                // unavailable: the standard OS opening below still
                // preserves access to the file, possibly without the gallery.
                debug!(error = %err, path = %path.display(), "Photos gallery activation unavailable");
            }
        }
    }
    if let Err(err) = actions::open_path(path) {
        error!(error = %err, path = %path.display(), "open file failed");
    }
}

/// Above this many files, opening them all is put to the user first. Each one
/// starts an application, so a selection made with Ctrl+A and an Enter pressed
/// out of habit would otherwise start a few hundred at once. The figure is the
/// one the Windows file manager has long used for the same guard.
pub(in crate::bridge) const OPEN_MANY_PROMPT_AT: usize = 15;

/// Opens a whole selection of files, asking first when there are enough of them
/// for the answer to matter.
pub(in crate::bridge) fn open_selected_files(
    window: &MainWindow,
    state: &AppState,
    paths: Vec<PathBuf>,
) {
    if paths.len() <= OPEN_MANY_PROMPT_AT {
        open_file_default(state, &paths);
        return;
    }
    let lang = state.config.borrow().language;
    window.global::<crate::OperationsApi>().set_open_many_body(
        i18n::tr(lang, "open_many_body")
            .replace("{count}", &paths.len().to_string())
            .into(),
    );
    *state.pending_open.borrow_mut() = paths;
    window
        .global::<crate::OperationsApi>()
        .set_open_many_open(true);
}

/// Opens FILE(s), never a folder. Shared by the double-click AND by
/// Enter / the "Open" menu entry → identical behaviour.
///
/// Several files open together, each getting exactly the treatment it would
/// get alone — one rule to predict rather than two. Files that share a Favnyr
/// opener are the one exception, handed over in a single go so an editor opens
/// one window instead of several.
pub(in crate::bridge) fn open_file_default(state: &AppState, paths: &[PathBuf]) {
    if let [only] = paths {
        open_one_file_default(state, only);
        return;
    }
    let plan = plan_open(paths, |ext| {
        state
            .openers
            .borrow()
            .default_for(ext)
            .map(|o| o.id.clone())
    });
    for launch in plan {
        match launch {
            Launch::Opener { id, paths } => {
                let Some(op) = state.openers.borrow().get(&id).cloned() else {
                    continue;
                };
                let ext = paths.first().map(|p| ext_of(p)).unwrap_or_default();
                run_default_opener(state, &op, &paths, &ext);
            }
            Launch::System(path) => open_one_file_default(state, &path),
        }
    }
}

/// The Photos gallery attempt is strictly a Windows enhancement for
/// images opened via the OS association. Favnyr openers are processed before
/// this predicate, so Linux keeps its historical `xdg-open` path.
#[cfg(windows)]
pub(in crate::bridge) fn should_try_image_gallery(extension: &str) -> bool {
    rfs::classify_kind(Some(extension), false) == FileKind::Image
}

/// Opens the "Custom command" popup in CREATE mode (optional
/// pre-filling of program/name). Shared by "Custom command", "Use as
/// application", and the promotion toast.
pub(in crate::bridge) fn open_ow_create(
    window: &MainWindow,
    state: &AppState,
    program: &str,
    name: &str,
) {
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_id(SharedString::new());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_name(name.into());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_program(program.into());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_icon_kind(openers::OpenerIcon::None.as_i32());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_is_store(false); // creation = command with an executable
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_args(SharedString::new());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_default_ext(SharedString::new());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_used_ext(SharedString::new());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_elevated(false); // unchecked by default
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_ctx_file(false); // not pinned by default
    // Every file, so ticking "Files" changes nothing about who sees the entry
    // until the user narrows it down deliberately.
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_ctx_ext(openers::CTX_EXT_ALL.into());
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_ctx_dir(false);
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_ctx_bg(false);
    window.global::<crate::SettingsApi>().set_ow_popup_add(true);
    recompute_ow_preview(window, state);
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_open(true);
    window
        .global::<crate::SettingsApi>()
        .set_ow_popup_focus_armed(true);
}
