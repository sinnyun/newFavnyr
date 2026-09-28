use super::*;

/// Advances the `PasteJob`: opens the popup for the next conflict, or
/// executes the operation if all conflicts are resolved.
/// Resolves a conflict by **overwriting**: the target keeps its original name and will be
/// replaced (the worker removes the existing item before the copy). Safeguard: a
/// "self-overwrite" (copying a file into its own folder) is
/// ignored — never delete the source.
pub(super) fn resolve_replace(job: &mut PasteJob, src: PathBuf) {
    let Some(name) = src.file_name() else { return };
    let target = job.dst_dir.join(name);
    // Case-insensitively on Windows: paths reaching a paste can come from
    // another application's clipboard, which spells them however it likes. An
    // exact comparison would let a differently-spelled self-overwrite through,
    // and this branch is the one that has the source deleted.
    if ops::paths_equal(&target, &src) {
        return; // copy onto itself → we don't overwrite (the item is simply skipped)
    }
    // An earlier item of this same paste already aims there. "Replace" means
    // replacing what was in the folder, never what this operation has just put
    // there: honouring it would make the second copy destroy the first, and the
    // user would end up with one file where they pasted two. The popup no
    // longer offers it (see `advance_paste`); this refuses it outright, since
    // "Replace all" walks the queue without asking again.
    if job.claims(&target) {
        return;
    }
    job.resolved.push((src, target, true));
}

pub(super) fn advance_paste(window: &MainWindow, state: &AppState) {
    enum Step {
        Conflict {
            original: String,
            suggested: String,
            rename_only: bool,
            is_dir: bool,
        },
        Execute(PasteJob),
    }
    let step = {
        let mut guard = state.paste_job.borrow_mut();
        if guard.is_none() {
            return;
        }
        let has_pending = guard
            .as_ref()
            .map(|j| !j.pending.is_empty())
            .unwrap_or(false);
        if has_pending {
            let job = guard.as_mut().unwrap();
            let src = job.pending.pop_front().unwrap();
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            // The suggestion must also clear the destinations this same paste
            // has already resolved, none of which exist on disk yet.
            let base = job.dst_dir.join(&name);
            let suggested =
                ops::unique_sibling_where(&base, |p| target_taken(state, p) || job.claims(p))
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| name.clone());
            // Copy WITHIN THE SAME FOLDER (src already in dst_dir): overwrite =
            // destroy the source (neutralized), skip = cancel → only
            // renaming makes sense. We therefore hide Replace/Skip.
            // Renaming is the only meaningful answer in two cases: copying
            // within the folder the item already sits in — where replacing
            // would destroy the source and skipping equals cancelling — and a
            // destination an earlier item of this same paste already owns,
            // where replacing would destroy that one. Same reasoning as the
            // rename dialog, which never offers "Force replace" for a
            // destination a running operation has reserved.
            let rename_only = src
                .parent()
                .is_some_and(|parent| ops::paths_equal(parent, &job.dst_dir))
                || job.claims(&base);
            // `is_dir()` deliberately follows a symlink if present: in the
            // dialog, a link to a folder is handled like a folder.
            let is_dir = src.is_dir();
            job.current = Some(src);
            Step::Conflict {
                original: name,
                suggested,
                rename_only,
                is_dir,
            }
        } else {
            Step::Execute(guard.take().unwrap())
        }
    };
    match step {
        Step::Conflict {
            original,
            suggested,
            rename_only,
            is_dir,
        } => {
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_original(original.into());
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_name(suggested.into());
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_name_taken(false);
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_rename_only(rename_only);
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_is_dir(is_dir);
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_open(true);
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_focus_gen(
                    window
                        .global::<crate::OperationsApi>()
                        .get_paste_conflict_focus_gen()
                        .wrapping_add(1),
                );
        }
        Step::Execute(job) => {
            window
                .global::<crate::OperationsApi>()
                .set_paste_conflict_open(false);
            execute_paste(window, state, job);
        }
    }
}

/// Starts a paste operation (`op`) from `sources` to `dst_dir`:
/// splits into `resolved` (no conflict) / `pending` (conflict → popup), then
/// `advance_paste`. Shared by paste (clipboard) and drag'n'drop.
pub(super) fn begin_paste(
    window: &MainWindow,
    state: &AppState,
    op: ClipOp,
    dst_dir: PathBuf,
    sources: Vec<PathBuf>,
) {
    begin_paste_with_cleanup(window, state, op, dst_dir, sources, None);
}

pub(super) fn begin_paste_with_cleanup(
    window: &MainWindow,
    state: &AppState,
    op: ClipOp,
    dst_dir: PathBuf,
    sources: Vec<PathBuf>,
    transient_cleanup: Option<TransientDropGuard>,
) {
    if dst_dir.as_os_str().is_empty() || sources.is_empty() {
        return;
    }
    // Refuses a second paste while one is still being resolved.
    if state.paste_job.borrow().is_some() {
        return;
    }
    let mut job = PasteJob {
        op,
        dst_dir: dst_dir.clone(),
        pending: std::collections::VecDeque::new(),
        resolved: Vec::new(),
        current: None,
        transient_cleanup,
    };
    for src in sources {
        // Never write an item into itself or into one of its own descendants.
        // The walk would keep meeting the copy it has just created and recurse
        // until the path length or the disk gives out. The drag route refused
        // this on its own; the clipboard reached here unguarded, so the rule
        // now sits on the single funnel every route goes through.
        if ops::is_within(&dst_dir, &src) {
            continue;
        }
        // Ignores drops of an item onto itself / into its own folder.
        if matches!(op, ClipOp::Cut)
            && src
                .parent()
                .is_some_and(|parent| ops::paths_equal(parent, &dst_dir))
        {
            continue;
        }
        let Some(name) = src.file_name() else {
            continue;
        };
        let target = dst_dir.join(name);
        // Occupied means: on disk, claimed by a running operation, or already
        // taken by an earlier item of this very paste. The last case happens
        // with same-named sources from different folders (OS clipboard, drop
        // from another app) — without it both would land on the same path.
        if target_taken(state, &target) || job.claims(&target) {
            job.pending.push_back(src);
        } else {
            job.resolved.push((src, target, false));
        }
    }
    *state.paste_job.borrow_mut() = Some(job);
    advance_paste(window, state);
}

/// Executes the resolved copies/moves (targets guaranteed free) on a
/// background thread with a progress bar.
pub(super) fn execute_paste(window: &MainWindow, state: &AppState, job: PasteJob) {
    if job.resolved.is_empty() {
        // Everything was skipped/cancelled: nothing to execute, just refresh.
        refresh_listing(window, state, &job.dst_dir);
        return;
    }
    let lang = state.snapshot_config().language;
    // Item to highlight once the copy/move finishes: the
    // 1st target (the order of `resolved` follows the original selection's order).
    // Sorting often places the newcomer off-screen → `op-finished` will bring it back into view.
    let pending_focus = job.resolved.first().map(|(_, dst, _)| dst.clone());
    let cut = matches!(job.op, ClipOp::Cut);
    let work = match job.op {
        ClipOp::Copy => Heavy::Copy(job.resolved),
        ClipOp::Cut => Heavy::Move(job.resolved),
    };
    start_heavy_op(
        window,
        state,
        work,
        lang,
        pending_focus,
        job.transient_cleanup,
    );
    // The cut is consumed once the operation is under way, never before: its
    // paths are what the operation moves, and dropping them early would lose
    // the pending selection if the paste did not start.
    if cut {
        let mut clip = state.clipboard.borrow_mut();
        clip.paths.clear();
        clip.op = None;
    }
}
