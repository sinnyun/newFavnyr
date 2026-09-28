use super::*;

// Tree-structured favorites ----------

/// Persists the favorites tree (logs on failure; never fatal).
/// Fingerprint of a shared store file as it currently sits on disk.
///
/// Modified time AND length: on a filesystem whose timestamps are coarse, two
/// edits within the same tick would otherwise look identical, and a length
/// change is free to read alongside.
///
/// Every store shared between instances uses this: reading it is a `stat` —
/// microseconds — while parsing is not, so the cheap check gates the expensive
/// one.
pub(super) fn file_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(super) fn annotations_stamp() -> Option<(std::time::SystemTime, u64)> {
    file_stamp(&paths::annotations_path())
}

pub(super) fn favorites_stamp() -> Option<(std::time::SystemTime, u64)> {
    file_stamp(&paths::favorites_path())
}

/// Re-reads the annotation store when the file changed under us.
///
/// One file is shared by every running instance, so a colour assigned in one
/// window must show up in the next listing of another. Reading the fingerprint
/// is a `stat` — microseconds — while parsing is not, so the cheap check gates
/// the expensive one. Same shape as the block signature gating the volume
/// inventory.
pub(super) fn sync_annotations(state: &AppState) {
    let stamp = annotations_stamp();
    if *state.annotations_stamp.borrow() == stamp {
        return;
    }
    *state.annotations.borrow_mut() =
        favnyr_core::annotations::AnnotationStore::load(&paths::annotations_path());
    *state.annotations_stamp.borrow_mut() = stamp;
}

/// The store, for reading a listing.
pub(super) fn annotations_now(
    state: &AppState,
) -> std::cell::Ref<'_, favnyr_core::annotations::AnnotationStore> {
    sync_annotations(state);
    state.annotations.borrow()
}

/// The store, for changing it. The re-read happens FIRST, so a change made
/// meanwhile by another instance is merged rather than overwritten.
pub(super) fn annotations_for_update(
    state: &AppState,
) -> std::cell::RefMut<'_, favnyr_core::annotations::AnnotationStore> {
    sync_annotations(state);
    state.annotations.borrow_mut()
}

/// Writes the store and records the fingerprint it produced, so the next check
/// does not re-read what this instance just wrote.
///
/// Cosmetic data: a failure is logged and never interrupts the operation that
/// asked for it.
pub(super) fn save_annotations(
    state: &AppState,
    annotations: &favnyr_core::annotations::AnnotationStore,
) {
    if let Err(err) = annotations.save(&paths::annotations_path()) {
        error!(error = %err, "save annotations failed");
        return;
    }
    *state.annotations_stamp.borrow_mut() = annotations_stamp();
}

/// Re-reads the favorites tree when the file changed under us. Same shape, and
/// the same reason, as `sync_annotations`: one file is shared by every running
/// instance, so a tree saved from this window must start from what the others
/// have written rather than from the view loaded at launch — which would drop
/// their additions without a trace.
pub(super) fn sync_favorites(state: &AppState) {
    let stamp = favorites_stamp();
    if *state.favorites_stamp.borrow() == stamp {
        return;
    }
    *state.favorites.borrow_mut() = favorites::FavStore::load(&paths::favorites_path());
    *state.favorites_stamp.borrow_mut() = stamp;
}

/// The tree, for reading.
pub(super) fn favorites_now(state: &AppState) -> std::cell::Ref<'_, favorites::FavStore> {
    sync_favorites(state);
    state.favorites.borrow()
}

/// The tree, for changing it. The re-read happens FIRST, so a change made
/// meanwhile by another instance is kept rather than overwritten.
pub(super) fn favorites_for_update(state: &AppState) -> std::cell::RefMut<'_, favorites::FavStore> {
    sync_favorites(state);
    state.favorites.borrow_mut()
}

/// Writes the tree and records the fingerprint it produced, so the next check
/// does not re-read what this instance just wrote.
pub(super) fn save_favorites(state: &AppState) {
    if let Err(err) = state.favorites.borrow().save(&paths::favorites_path()) {
        error!(error = %err, "save favorites failed");
        return;
    }
    *state.favorites_stamp.borrow_mut() = favorites_stamp();
}

/// Converts a flattened core node into a Slint struct (existence checked for
/// favorites → grayed out if the path is missing).
pub(super) fn flat_to_favnode(f: &FlatFav) -> FavNode {
    // One metadata read answers both questions and follows directory symlinks,
    // matching navigation. Containers carry no filesystem path.
    let metadata = (!f.is_container && !f.path.is_empty())
        .then(|| std::fs::metadata(&f.path).ok())
        .flatten();
    let available = f.is_container || f.path.is_empty() || metadata.is_some();
    let is_dir = metadata.is_some_and(|entry| entry.is_dir());
    FavNode {
        id: f.id.clone().into(),
        label: f.name.clone().into(),
        path: f.path.clone().into(),
        is_container: f.is_container,
        depth: f.depth,
        expanded: f.expanded,
        has_children: f.has_children,
        available,
        is_dir,
    }
}

// "Open with" openers ----------

/// Persists the openers store (logs on failure; never fatal).
pub(super) fn save_openers(state: &AppState) {
    if let Err(err) = state.openers.borrow().save(&paths::openers_path()) {
        error!(error = %err, "save openers failed");
    }
}
