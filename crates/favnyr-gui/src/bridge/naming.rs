use super::*;

/// True if `name` cannot be used as a copy name: empty,
/// a path separator, `.`/`..`, or already present in the target folder.
pub(super) fn paste_name_invalid(state: &AppState, name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return true;
    }
    match state.paste_job.borrow().as_ref() {
        Some(job) => {
            let target = job.dst_dir.join(name);
            target_taken(state, &target) || job.claims(&target)
        }
        None => true,
    }
}

/// `true` if a destination is unavailable: already on disk, or claimed by an
/// operation still running. Both are needed — checking only the filesystem
/// would let two concurrent pastes resolve to the very same path.
pub(super) fn target_taken(state: &AppState, path: &Path) -> bool {
    path.exists() || state.ops.is_reserved(path)
}

/// Distinct destinations for a whole batch, in order.
///
/// Each name is picked against `taken` AND against what earlier items of the
/// same batch already took. That last part matters: nothing is on disk yet
/// while the batch is being planned, so duplicating `x.txt` together with
/// `x - Copy01.txt` would otherwise hand both of them `x - Copy02.txt` and one
/// result would overwrite the other.
pub(super) fn plan_unique_targets(
    sources: &[PathBuf],
    taken: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut planned: Vec<PathBuf> = Vec::with_capacity(sources.len());
    // Set rather than a scan of `planned`: picking a name already probes the
    // filesystem candidate by candidate, and a batch of same-named items would
    // make a linear lookup grow on top of that.
    let mut claimed: HashSet<PathBuf> = HashSet::with_capacity(sources.len());
    for src in sources {
        let dst = ops::unique_sibling_where(src, |p| taken(p) || claimed.contains(p));
        claimed.insert(dst.clone());
        planned.push(dst);
    }
    planned
}

/// State of a name proposed in the Rename dialog. The distinction between
/// `Conflict` and `ReplaceableFile` avoids displaying "Force replace"
/// for a syntactically invalid name or for a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenameNameStatus {
    Valid,
    Invalid,
    Conflict,
    ReplaceableFile,
}

/// Availability of a name in a folder, shared by the Create and
/// Rename dialogs. `ExistingNonDirectory` includes files and links: a
/// directory entry, even a broken link, always occupies its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryNameAvailability {
    Available,
    Invalid,
    ExistingNonDirectory,
    ExistingDirectory,
    /// Nothing on disk yet, but a running operation will write there. Occupied
    /// for every purpose, and never replaceable: there is no entry to replace.
    Reserved,
}

/// `entry_name_availability` widened to the destinations of operations in
/// flight. A name a running copy is about to write is not free either, even
/// though nothing occupies it on disk yet — creating or renaming onto it would
/// have that entry overwritten as soon as the operation reaches it.
pub(super) fn entry_name_availability_now(
    state: &AppState,
    parent: &Path,
    name: &str,
) -> EntryNameAvailability {
    let on_disk = entry_name_availability(parent, name);
    with_reservation(on_disk, state.ops.is_reserved(&parent.join(name)))
}

/// Folds a reservation into a filesystem-only verdict. Only a name that was
/// otherwise free becomes `Reserved`: a real entry keeps its own kind, which is
/// what the dialogs report on.
pub(super) fn with_reservation(
    on_disk: EntryNameAvailability,
    reserved: bool,
) -> EntryNameAvailability {
    match on_disk {
        EntryNameAvailability::Available if reserved => EntryNameAvailability::Reserved,
        other => other,
    }
}

pub(super) fn entry_name_availability(parent: &Path, name: &str) -> EntryNameAvailability {
    if ops::check_file_name(name).is_err() {
        return EntryNameAvailability::Invalid;
    }
    match std::fs::symlink_metadata(parent.join(name)) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => EntryNameAvailability::Available,
        // A name the OS refuses outright never reached the disk, so there is no
        // entry occupying it: reporting a conflict would send the user looking
        // for a duplicate that does not exist. `check_file_name` catches these
        // beforehand; this keeps the verdict honest for whatever it cannot know
        // in advance, such as a name a specific filesystem rejects on its own.
        Err(err) if err.kind() == std::io::ErrorKind::InvalidFilename => {
            EntryNameAvailability::Invalid
        }
        Err(_) => EntryNameAvailability::ExistingNonDirectory,
        Ok(meta) if meta.is_dir() => EntryNameAvailability::ExistingDirectory,
        Ok(_) => EntryNameAvailability::ExistingNonDirectory,
    }
}

/// Publishes a verdict on the typed name to the Create popup: what the confirm
/// button may do, and what the user is told. Single point of truth for the
/// three places that judge the name — the live check on each keystroke and the
/// two safeguards on confirmation, which must not word the same verdict
/// differently.
pub(super) fn push_create_name_status(
    window: &MainWindow,
    lang: Lang,
    name: &str,
    availability: EntryNameAvailability,
) {
    window
        .global::<crate::OperationsApi>()
        .set_create_name_valid(availability != EntryNameAvailability::Invalid);
    window
        .global::<crate::OperationsApi>()
        .set_create_conflict(matches!(
            availability,
            EntryNameAvailability::ExistingNonDirectory
                | EntryNameAvailability::ExistingDirectory
                | EntryNameAvailability::Reserved
        ));
    window
        .global::<crate::OperationsApi>()
        .set_create_name_error(name_error_text(lang, name, availability).into());
}

/// Translated explanation of why a name cannot be used, or `""` when it can.
/// The interface shows this verbatim, so the reason reaches the user instead of
/// the catch-all "already taken".
pub(super) fn name_error_text(
    lang: Lang,
    name: &str,
    availability: EntryNameAvailability,
) -> String {
    match availability {
        EntryNameAvailability::Available => String::new(),
        EntryNameAvailability::ExistingNonDirectory
        | EntryNameAvailability::ExistingDirectory
        | EntryNameAvailability::Reserved => i18n::tr(lang, "paste_conflict_taken"),
        EntryNameAvailability::Invalid => match ops::check_file_name(name) {
            // A field the user has not filled in yet: the disabled button says
            // enough, an error message would only scold them for typing nothing.
            Ok(()) | Err(ops::NameRejection::Empty) => String::new(),
            Err(ops::NameRejection::DotEntry) => i18n::tr(lang, "name_error_dot_entry"),
            Err(ops::NameRejection::ReservedDevice) => i18n::tr(lang, "name_error_reserved"),
            Err(ops::NameRejection::TrailingDotOrSpace) => i18n::tr(lang, "name_error_trailing"),
            Err(ops::NameRejection::ForbiddenChar) => {
                i18n::tr(lang, "name_error_chars").replace("{chars}", ops::FORBIDDEN_NAME_CHARS)
            }
        },
    }
}

/// `rename_name_status` accounting for the operations in flight. Kept apart
/// from the pure form so the naming rules stay testable without app state.
pub(super) fn rename_name_status_now(
    state: &AppState,
    source: &Path,
    name: &str,
) -> RenameNameStatus {
    let on_disk = rename_name_status(source, name);
    let reserved = source
        .parent()
        .is_some_and(|parent| state.ops.is_reserved(&parent.join(name)));
    rename_with_reservation(on_disk, reserved)
}

/// Folds a reservation into a filesystem-only rename verdict. A destination a
/// running operation will write offers no entry to replace, so it downgrades to
/// a plain conflict and "Force replace" is never proposed for it.
pub(super) fn rename_with_reservation(
    on_disk: RenameNameStatus,
    reserved: bool,
) -> RenameNameStatus {
    match on_disk {
        RenameNameStatus::Valid if reserved => RenameNameStatus::Conflict,
        other => other,
    }
}

pub(super) fn rename_name_status(source: &Path, name: &str) -> RenameNameStatus {
    let Some(parent) = source.parent() else {
        return RenameNameStatus::Invalid;
    };
    let old = source.file_name().map(|n| n.to_string_lossy());
    if old.as_deref() == Some(name) {
        return RenameNameStatus::Valid;
    }
    // Case change on Windows: the "target" is the entry itself,
    // this isn't a destructive replacement. Purely lexical comparison,
    // without `canonicalize`: this check runs on every keystroke and shouldn't
    // add a network round-trip. (The `target` computation is ONLY used here → it
    // stays inside the Windows block so it isn't "unused" on Linux.)
    #[cfg(windows)]
    {
        let target = parent.join(name);
        if source
            .to_string_lossy()
            .eq_ignore_ascii_case(&target.to_string_lossy())
        {
            return RenameNameStatus::Valid;
        }
    }

    match entry_name_availability(parent, name) {
        EntryNameAvailability::Available => RenameNameStatus::Valid,
        EntryNameAvailability::Invalid => RenameNameStatus::Invalid,
        EntryNameAvailability::ExistingDirectory => RenameNameStatus::Conflict,
        EntryNameAvailability::Reserved => RenameNameStatus::Conflict,
        EntryNameAvailability::ExistingNonDirectory => {
            let Ok(source_meta) = std::fs::symlink_metadata(source) else {
                return RenameNameStatus::Conflict;
            };
            if source_meta.is_dir() {
                RenameNameStatus::Conflict
            } else {
                RenameNameStatus::ReplaceableFile
            }
        }
    }
}

/// UTF-8 offset expected by `TextInput::set_selection_offsets`.
///
/// For a file, the caret is placed before the extension recognized by the same
/// rule as display and duplication (`fs::ops::split_name`). Dotfiles
/// and purely numeric suffixes therefore remain whole names. A
/// folder is never split, even if its name contains a period.
pub(super) fn filename_caret_offset(name: &str, is_dir: bool) -> i32 {
    let offset = if is_dir {
        name.len()
    } else {
        let (stem, extension) = ops::split_name(name);
        if extension.is_empty() {
            name.len()
        } else {
            stem.len()
        }
    };
    offset.min(i32::MAX as usize) as i32
}
