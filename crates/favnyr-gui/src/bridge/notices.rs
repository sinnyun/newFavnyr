use super::*;

/// Kind of a "notice" toast: determines its tone and its icon.
#[derive(Clone, Copy)]
pub(super) enum NoticeKind {
    /// Danger + closed padlock: access denied, eject failure.
    Error,
    /// Success + open padlock: successful removal/disconnection.
    EjectOk,
    /// Generic success + checkmark: save, validation, etc.
    Success,
    /// Success + bookmark: successfully added to favorites.
    FavAdded,
    /// Info (accent) + bookmark: already present in favorites (neither error nor success).
    FavExists,
    /// Danger + bookmark: favorite whose target cannot be found.
    FavMissing,
    /// Info + closed padlock: a volume that cannot be entered as things
    /// stand — not mounted, or still encrypted. Neither a failure nor a
    /// success, so it borrows the danger tone from neither.
    Unavailable,
}

impl NoticeKind {
    /// `(tone, icon)` — MUST match the Slint codes: tone 0 danger /
    /// 1 success / 2 info; icon 0 padlock / 1 open padlock / 2 checkmark / 3 bookmark.
    fn codes(self) -> (i32, i32) {
        match self {
            NoticeKind::Error => (0, 0),
            NoticeKind::Unavailable => (2, 0),
            NoticeKind::EjectOk => (1, 1),
            NoticeKind::Success => (1, 2),
            NoticeKind::FavAdded => (1, 3),
            NoticeKind::FavExists => (2, 3),
            NoticeKind::FavMissing => (0, 3),
        }
    }
}

/// Displays a "notice" toast with a reading duration **adaptive to the
/// message's length**: ~2.6 s + 55 ms/character, clamped to [2.8 s; 8 s]. A
/// long message (e.g. "Removal failed: 'service X' is using the device")
/// thus stays displayed long enough to be read.
pub(super) fn show_notice(w: &MainWindow, text: impl Into<SharedString>) {
    notice(w, text, NoticeKind::Error);
}

pub(super) fn path_notice_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Shared source of truth for deletions and renames refused by a
/// Windows lock. The diagnostic is only triggered AFTER the actual failure.
pub(super) fn locked_item_notice(path: &Path, lang: Lang) -> Option<String> {
    let lock = favnyr_core::process_lock::diagnose(path)?;
    Some(i18n::item_in_use(
        lang,
        &path_notice_name(path),
        &lock.processes,
        lock.truncated,
    ))
}

/// Opens the menu to the keyboard, with nothing highlighted yet.
///
/// The menu builds itself right after this and each row registers as it
/// appears, so nothing here has to restate which entries the menu will show.
/// Set from here rather than from a `changed` handler so the order is certain:
/// the state is ready before the first entry exists.
pub(super) fn arm_context_menu_navigation(window: &MainWindow) {
    let nav = window.global::<CtxNav>();
    nav.set_menu_y(-1.0);
    nav.set_sub_y(-1.0);
    nav.set_registering(1);
    nav.set_level(1);
}

/// Message for an entry an operation stepped over. Names the program holding
/// it when Windows attributes the conflict — the useful half of the answer,
/// since the user then knows what to close — and falls back to the system
/// error everywhere that diagnostic does not exist.
pub(super) fn skipped_entry_notice(path: &Path, lang: Lang, error: &str) -> String {
    locked_item_notice(path, lang)
        .unwrap_or_else(|| i18n::item_skipped(lang, &path_notice_name(path), error))
}

/// Message for a move that copied its item but could not remove the original.
/// Runs the same lock diagnostic as a refused deletion — it is only reached
/// after an actual failure, and always from a worker thread — so the program
/// holding the file can be named when Windows attributes it. Falls back to the
/// system error, which stays informative where no such diagnostic exists.
pub(super) fn move_source_kept_notice(path: &Path, lang: Lang, error: &str) -> String {
    i18n::move_source_kept(
        lang,
        &path_notice_name(path),
        &lock_reason(path, lang, error),
    )
}

/// Why an entry resisted: the program holding it where Windows attributes the
/// conflict, the system error everywhere else. Only ever reached after an
/// actual failure, and always from a worker thread — the diagnostic can probe
/// a folder's children and must never run on the UI thread.
pub(super) fn lock_reason(path: &Path, lang: Lang, error: &str) -> String {
    favnyr_core::process_lock::diagnose(path)
        .map(|lock| i18n::process_list(lang, &lock.processes, lock.truncated))
        .filter(|processes| !processes.is_empty())
        .unwrap_or_else(|| error.to_string())
}

/// A locked folder may require a bounded probe of its children: never
/// perform this diagnostic on the Slint thread. `candidates` contains the
/// source then, for "Force replace", the target which can also be locked.
pub(super) fn report_rename_failure(
    weak: slint::Weak<MainWindow>,
    candidates: Vec<PathBuf>,
    lang: Lang,
    reason: String,
) {
    let item = candidates
        .first()
        .map(|path| path_notice_name(path))
        .unwrap_or_default();
    let fallback = i18n::rename_failed(lang, &item, &reason);

    // Restart Manager doesn't exist outside Windows: don't create a thread that
    // could only immediately return the generic message already prepared.
    #[cfg(not(windows))]
    {
        if let Some(window) = weak.upgrade() {
            show_notice(&window, fallback);
        }
    }

    #[cfg(windows)]
    let fallback = Arc::new(fallback);
    #[cfg(windows)]
    let fallback_worker = fallback.clone();
    #[cfg(windows)]
    let weak_worker = weak.clone();
    #[cfg(windows)]
    let spawn = std::thread::Builder::new()
        .name("favnyr-lock-diagnose".to_owned())
        .spawn(move || {
            let message = candidates
                .iter()
                .find_map(|path| locked_item_notice(path, lang))
                .unwrap_or_else(|| (*fallback_worker).clone());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_worker.upgrade() {
                    show_notice(&window, message);
                }
            });
        });
    #[cfg(windows)]
    if let Err(error) = spawn {
        error!(error = %error, "rename lock diagnostic worker unavailable");
        if let Some(window) = weak.upgrade() {
            show_notice(&window, (*fallback).clone());
        }
    }
}

/// "Success" variant (green, open padlock) — e.g. successful safe removal.
pub(super) fn show_notice_ok(w: &MainWindow, text: impl Into<SharedString>) {
    notice(w, text, NoticeKind::EjectOk);
}
pub(super) fn show_notice_unavailable(w: &MainWindow, text: impl Into<SharedString>) {
    notice(w, text, NoticeKind::Unavailable);
}

pub(super) fn notice(w: &MainWindow, text: impl Into<SharedString>, kind: NoticeKind) {
    let text = text.into();
    let chars = text.chars().count() as f32;
    let ms = (2600.0 + 55.0 * chars).clamp(2800.0, 8000.0) as i64;
    notice_for(w, text, kind, ms);
}

/// Variant with an explicit duration for very short confirmations. The visual
/// route remains strictly the same as for other notices.
pub(super) fn notice_for(
    w: &MainWindow,
    text: impl Into<SharedString>,
    kind: NoticeKind,
    duration_ms: i64,
) {
    let (tone, icon) = kind.codes();
    w.global::<crate::PanelsApi>().set_notice_tone(tone);
    w.global::<crate::PanelsApi>().set_notice_icon(icon);
    w.global::<crate::PanelsApi>()
        .set_notice_duration(duration_ms);
    w.global::<crate::PanelsApi>().set_notice_text(text.into());
}

/// Non-blocking startup-by-workspace-name warning. Exposed to the
/// binary only to reuse exactly the global toast route.
pub fn show_workspace_not_found_notice(window: &MainWindow, state: &AppState, name: &str) {
    let lang = state.config.borrow().language;
    let message = i18n::tr(lang, "ws_not_found").replace("{name}", name);
    notice(window, message, NoticeKind::Error);
}

/// Warns ONCE (state persisted in the config) that `ffmpeg` is missing
/// → no video thumbnails. No-op if already shown, or if `ffmpeg` is present —
/// always the case on Windows, where video goes through the native shell, so no
/// warning there. Called when the user turns on Previews (the moment when
/// thumbnails become relevant). Reuses the global toast route.
pub(super) fn maybe_warn_ffmpeg_missing(window: &MainWindow, state: &AppState) {
    if state.config.borrow().ffmpeg_hint_shown || actions::ffmpeg_available() {
        return;
    }
    let lang = state.config.borrow().language;
    notice(window, i18n::tr(lang, "ffmpeg_missing"), NoticeKind::Error);
    state.persist_config(|c| c.ffmpeg_hint_shown = true);
}
