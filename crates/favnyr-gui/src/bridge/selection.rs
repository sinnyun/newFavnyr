use super::*;

// ---------- Selection helpers (Slint model manipulation) ----------
//
// Convention: all of them return the **number of selected items** after
// the operation, so the caller can push it directly into
// `selected-count` without recomputing it.

/// Must this row be pushed back to the model?
///
/// Yes when its selection actually changed — and yes as well when the row is
/// RENDERED, even if nothing changed. A row reaches the screen only through the
/// filtered sub-model, which hears about a row only when it is written back.
/// Skipping an unchanged write is the right economy for the thousands of rows
/// nobody is looking at; for the few dozen on screen it removes the one chance
/// a delegate left out of step had of catching up, and nothing else would ever
/// correct it short of rebuilding the whole model.
///
/// Re-pushing them costs one notification per visible row, and makes the
/// display self-healing: whatever put a delegate out of step, the next cursor
/// move puts it back.
pub(super) fn selection_needs_write(row: &FileRow, selected: bool) -> bool {
    row.selected != selected || row.rendered
}

/// Reduces the selection to `idx` alone. Returns `(count, was_already_alone)`.
///
/// The second value is what tells a plain click apart from a click that
/// COLLAPSES a wider selection. Both look identical on the row that was hit —
/// it was selected before and it is selected after — so only "did anything
/// else change" separates them, and the caller cannot see that from outside.
pub(super) fn selection_set_only<M: slint::Model<Data = FileRow>>(
    model: &M,
    idx: i32,
) -> (i32, bool) {
    let mut count = 0i32;
    let mut changed = false;
    let n = model.row_count();
    for i in 0..n {
        if let Some(mut row) = model.row_data(i) {
            let should = i as i32 == idx && row.role == ROW_ROLE_ENTRY;
            // Tracked apart from the write: a re-push that changes nothing must
            // not read as a selection that moved, or a click collapsing a wider
            // selection would stop being told from a plain one — which is what
            // arms the slow second click.
            let differs = row.selected != should;
            if selection_needs_write(&row, should) {
                row.selected = should;
                model.set_row_data(i, row);
            }
            if differs {
                changed = true;
            }
            if should {
                count += 1;
            }
        }
    }
    (count, !changed)
}

pub(super) fn selection_toggle<M: slint::Model<Data = FileRow>>(model: &M, idx: i32) -> i32 {
    let n = model.row_count();
    if idx < 0 || (idx as usize) >= n {
        return count_selected(model);
    }
    if let Some(mut row) = model.row_data(idx as usize) {
        row.selected = !row.selected;
        model.set_row_data(idx as usize, row);
    }
    count_selected(model)
}

thread_local! {
    /// State of an additive/subtractive rubber-band, UI thread. `.0` = mode
    /// (0 = replace, 1 = add [Shift], 2 = subtract [Ctrl]); `.1` = snapshot
    /// of the selection AT THE START of the drag (base combined with the band on every update).
    pub(super) static RB_STATE: RefCell<(i32, Vec<bool>)> = const { RefCell::new((0, Vec::new())) };
}

/// Boolean snapshot of the current selection (for the additive rubber-band).
pub(super) fn snapshot_selection<M: slint::Model<Data = FileRow>>(model: &M) -> Vec<bool> {
    (0..model.row_count())
        .map(|i| model.row_data(i).map(|r| r.selected).unwrap_or(false))
        .collect()
}

/// Applies a rubber-band `[lo, hi]` combined with a `base` according to `mode`
/// (0 = replace, 1 = add, 2 = subtract). Returns the selected count.
pub(super) fn selection_apply_band<M: slint::Model<Data = FileRow>>(
    model: &M,
    lo: i32,
    hi: i32,
    mode: i32,
    base: &[bool],
) -> i32 {
    let mut count = 0i32;
    let n = model.row_count();
    for i in 0..n {
        let Some(row) = model.row_data(i) else {
            continue;
        };
        // A header caught in the band is not an entry: it stays unselected.
        let in_band = (i as i32) >= lo && (i as i32) <= hi && row.role == ROW_ROLE_ENTRY;
        let based = base.get(i).copied().unwrap_or(false);
        let want = match mode {
            1 => based || in_band,  // Shift: add the band to the base selection
            2 => based && !in_band, // Ctrl: subtract the band from the base
            _ => in_band,           // replace
        };
        if row.selected != want {
            let mut row = row;
            row.selected = want;
            model.set_row_data(i, row);
        }
        if want {
            count += 1;
        }
    }
    count
}

pub(super) fn selection_set_range<M: slint::Model<Data = FileRow>>(
    model: &M,
    a: i32,
    b: i32,
) -> i32 {
    let lo = a.min(b);
    let hi = a.max(b);
    let mut count = 0i32;
    let n = model.row_count();
    for i in 0..n {
        if let Some(mut row) = model.row_data(i) {
            // A header caught in the range is not an entry: it stays unselected.
            let in_range = (i as i32) >= lo && (i as i32) <= hi && row.role == ROW_ROLE_ENTRY;
            if selection_needs_write(&row, in_range) {
                row.selected = in_range;
                model.set_row_data(i, row);
            }
            if in_range {
                count += 1;
            }
        }
    }
    count
}

pub(super) fn selection_set_all<M: slint::Model<Data = FileRow>>(model: &M, value: bool) -> i32 {
    let n = model.row_count();
    let mut count = 0i32;
    for i in 0..n {
        // Section headers are not entries: "all" never means them.
        let Some(mut row) = model.row_data(i) else {
            continue;
        };
        let value = value && row.role == ROW_ROLE_ENTRY;
        if row.selected != value {
            row.selected = value;
            model.set_row_data(i, row);
        }
        if value {
            count += 1;
        }
    }
    count
}

pub(super) fn count_selected<M: slint::Model<Data = FileRow>>(model: &M) -> i32 {
    let mut c = 0i32;
    let n = model.row_count();
    for i in 0..n {
        if let Some(row) = model.row_data(i)
            && row.selected
            && row.role == ROW_ROLE_ENTRY
        {
            c += 1;
        }
    }
    c
}

/// Targets entry `name` in the ACTIVE panel: single selection + keyboard cursor +
/// scroll-into-view. No-op if the name is absent from the current model. Used to
/// "follow" an entry that was just created. (Reusable for other
/// "reveal this entry" cases.)
pub(super) fn focus_entry_by_name(window: &MainWindow, state: &AppState, name: &str) {
    let model = state.active_rows_model();
    let idx = (0..model.row_count())
        .find(|&i| model.row_data(i).is_some_and(|r| r.name.as_str() == name));
    let Some(idx) = idx.map(|i| i as i32) else {
        return;
    };
    let (count, _) = selection_set_only(&model, idx);
    state.with_tabs_mut(|b| {
        let a = b.active;
        let t = &mut b.tabs[a];
        t.cursor = idx;
        t.selection_anchor = idx;
        t.scroll_gen += 1; // scroll-into-view on the Slint side
    });
    update_panels_ui(window, state);
    push_active_footer(window, state, count);
}

/// Reveals an entry on the next UI turn. Navigation publishes both a viewport
/// reset and a new row model; deferring the reveal guarantees that the reset is
/// applied first instead of racing the `scroll-gen` update. The directory guard
/// prevents a late callback from changing a view the user has already left.
pub(super) fn schedule_focus_entry_by_name(
    window: &MainWindow,
    state: &AppState,
    expected_dir: PathBuf,
    name: String,
) {
    let weak = window.as_weak();
    let state = state.clone();
    defer(move || {
        let Some(window) = weak.upgrade() else { return };
        if ops::paths_equal(&state.current_path(), &expected_dir) {
            focus_entry_by_name(&window, &state, &name);
        }
    });
}

/// Last "displayable" segment of a path: `file_name`, or the SHARE
/// name for a share root `\\HOST\share` (whose `file_name()` is `None`), as
/// it appears in the server's share list.
pub(super) fn child_leaf(path: &Path) -> Option<String> {
    if let Some(n) = path.file_name() {
        return Some(n.to_string_lossy().into_owned());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(p)) = path.components().next()
            && let Prefix::UNC(_, share) | Prefix::VerbatimUNC(_, share) = p.kind()
        {
            let s = share.to_string_lossy();
            if !s.is_empty() {
                return Some(s.into_owned());
            }
        }
    }
    None
}

pub(super) fn upward_navigation_child_name(target: &Path, left: &Path) -> Option<String> {
    let direct_parent = left
        .parent()
        .is_some_and(|parent| ops::paths_equal(parent, target))
        || rfs::unc_share_parent(left)
            .as_deref()
            .is_some_and(|parent| ops::paths_equal(parent, target));
    direct_parent.then(|| child_leaf(left)).flatten()
}

/// After an UPWARD navigation (back / parent folder), selects
/// child `left` we came from in the TARGET folder, if it is its DIRECT
/// child → cursor + highlight + scroll-into-view. No-op otherwise (going back to a
/// folder with no parent-child relation).
pub(super) fn select_child_from(window: &MainWindow, state: &AppState, target: &Path, left: &Path) {
    if let Some(name) = upward_navigation_child_name(target, left) {
        // On network, the listing is still on the worker: remember the target
        // and apply it atomically along with the delivered rows.
        let active = *state.active_panel.borrow();
        let deferred = {
            let mut panels = state.panels.borrow_mut();
            if let Some(panel) = panels.get_mut(active) {
                if panel.pending_listing
                    && panel.tabs.tabs[panel.tabs.active].current_path == target
                {
                    panel.pending_select = Some(name.clone());
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if deferred {
            return;
        }
        schedule_focus_entry_by_name(window, state, target.to_path_buf(), name);
    }
}

pub(super) fn apply_name_filter(entries: &mut Vec<Entry>, filter: &str) {
    if filter.is_empty() {
        return;
    }
    let needle = filter.to_lowercase();
    entries.retain(|entry| name_contains_filter(&entry.name, &needle));
}

/// `needle` is already normalized to lowercase by the listing's caller, so as
/// not to redo this work for every entry of a large folder.
pub(super) fn name_contains_filter(name: &str, needle: &str) -> bool {
    name.to_lowercase().contains(needle)
}

// Helpers ----------

/// Absolute paths of the active panel's selected rows.
pub(super) fn selected_paths(state: &AppState) -> Vec<PathBuf> {
    let rows = state.active_rows_model();
    (0..rows.row_count())
        .filter_map(|i| rows.row_data(i))
        .filter(|r| r.selected)
        .filter_map(|r| row_path(&r))
        .collect()
}

/// Current folder of a given panel (by index). Empty if the index is invalid.
pub(super) fn panel_dir(state: &AppState, panel: usize) -> PathBuf {
    let panels = state.panels.borrow();
    panels
        .get(panel)
        .map(|p| p.tabs.tabs[p.tabs.active].current_path.clone())
        .unwrap_or_default()
}

/// Path of the folder at row `row` of panel `panel`, if it is one — for
/// dropping a selection INTO a subfolder hovered during a drag.
/// `None` if the index is out of bounds or the row isn't a folder.
pub(super) fn panel_folder_at_row(state: &AppState, panel: usize, row: usize) -> Option<PathBuf> {
    let panels = state.panels.borrow();
    let p = panels.get(panel)?;
    let r = p.rows_model.row_data(row)?;
    if !r.is_dir {
        return None;
    }
    row_path(&r)
}

/// Path of row `row` (file OR folder) of panel `panel`.
pub(super) fn panel_path_at_row(state: &AppState, panel: usize, row: usize) -> Option<PathBuf> {
    let panels = state.panels.borrow();
    let p = panels.get(panel)?;
    let r = p.rows_model.row_data(row)?;
    row_path(&r)
}

/// Paths selected in a GIVEN panel (not necessarily the active one) — used
/// by cross-view drag'n'drop (the source may not be the active panel).
pub(super) fn panel_selected_paths(state: &AppState, panel: usize) -> Vec<PathBuf> {
    let panels = state.panels.borrow();
    let Some(p) = panels.get(panel) else {
        return Vec::new();
    };
    (0..p.rows_model.row_count())
        .filter_map(|i| p.rows_model.row_data(i))
        .filter(|r| r.selected)
        .filter_map(|r| row_path(&r))
        .collect()
}

/// Is a row target the source itself, or a descendant into which
/// a source folder can't be dropped? Pure, no I/O or canonicalization.
pub(super) fn paths_conflict_with_drop_target(
    sources: &[PathBuf],
    target: &Path,
    target_is_dir: bool,
) -> bool {
    sources.iter().any(|source| {
        ops::paths_equal(source, target) || (target_is_dir && ops::is_within(target, source))
    })
}

/// Lightweight validation called only when the hovered row changes.
pub(super) fn file_drop_target_invalid(
    state: &AppState,
    src_panel: i32,
    target_panel: i32,
    row: i32,
) -> bool {
    if src_panel < 0 || target_panel < 0 {
        return false; // external drag: paths validated on the native drop
    }
    let src = src_panel as usize;
    let target = target_panel as usize;

    // Hot path: within the same view, the target is exactly the source if its
    // row belongs to the selection. No list traversal needed.
    if src == target {
        if row < 0 {
            return false;
        }
        return state
            .panels
            .borrow()
            .get(target)
            .and_then(|panel| panel.rows_model.row_data(row as usize))
            .is_some_and(|entry| entry.selected);
    }

    let (target_path, target_is_dir) = if row >= 0 {
        let row = row as usize;
        let Some(path) = panel_path_at_row(state, target, row) else {
            return false;
        };
        (path, panel_folder_at_row(state, target, row).is_some())
    } else {
        (panel_dir(state, target), true)
    };
    if target_path.as_os_str().is_empty() {
        return false;
    }
    let sources = panel_selected_paths(state, src);
    paths_conflict_with_drop_target(&sources, &target_path, target_is_dir)
}

/// Sets the `cut = true` flag on rows whose name is in `names`.
pub(super) fn apply_cut_marks<M: Model<Data = FileRow>>(
    model: &M,
    names: &std::collections::HashSet<String>,
) {
    let n = model.row_count();
    for i in 0..n {
        if let Some(mut row) = model.row_data(i) {
            let should = names.contains(row.name.as_str());
            if row.cut != should {
                row.cut = should;
                model.set_row_data(i, row);
            }
        }
    }
}

/// Clears the `cut` flag on all rows (Ctrl+C after Ctrl+X, or navigation).
pub(super) fn clear_cut_marks<M: Model<Data = FileRow>>(model: &M) {
    let n = model.row_count();
    for i in 0..n {
        if let Some(mut row) = model.row_data(i)
            && row.cut
        {
            row.cut = false;
            model.set_row_data(i, row);
        }
    }
}
