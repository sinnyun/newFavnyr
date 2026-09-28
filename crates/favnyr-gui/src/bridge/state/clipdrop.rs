use super::*;

// ---------- Internal clipboard ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::bridge) enum ClipOp {
    Copy,
    Cut,
}

/// Owns the private staging directory created for an incoming transient drop.
/// Keeping the guard in the paste/operation state guarantees cleanup on
/// success, error, conflict cancellation, or an early return.
pub(in crate::bridge) struct TransientDropGuard(Option<PathBuf>);

impl TransientDropGuard {
    /// Only an OLE drop creates a staging directory, and that path is Windows
    /// only. The guard type itself stays unconditional: it is a field of state
    /// shared by both platforms, holding `None` elsewhere.
    #[cfg(windows)]
    pub(in crate::bridge) fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }
}

impl Drop for TransientDropGuard {
    fn drop(&mut self) {
        let Some(path) = self.0.take() else { return };
        // Application-path captures can retain a complete hard-linked tree.
        // Delete it off the UI thread so finishing or cancelling a large drop
        // never stalls rendering.
        let cleanup_path = path.clone();
        if std::thread::Builder::new()
            .name("favnyr-dnd-cleanup".into())
            .spawn(move || cleanup_transient_drop_dir(&cleanup_path))
            .is_err()
        {
            cleanup_transient_drop_dir(&path);
        }
    }
}

pub(in crate::bridge) fn cleanup_transient_drop_dir(path: &Path) {
    if let Err(err) = std::fs::remove_dir_all(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        debug!(error = %err, path = %path.display(), "cleaning transient drop directory failed");
    }
}

/// Staging handed over by an OLE drop: the directory to clean up, and whether
/// its contents are copied out or moved out. Windows only — it is produced by
/// `winddrag` and consumed by `on_external_file_drop`, both gated the same way.
#[cfg(windows)]
pub(in crate::bridge) struct IncomingDropStaging {
    pub(in crate::bridge) cleanup: TransientDropGuard,
    pub(in crate::bridge) op: ClipOp,
}

#[derive(Debug, Default)]
pub(in crate::bridge) struct ClipboardState {
    pub(in crate::bridge) paths: Vec<PathBuf>,
    pub(in crate::bridge) op: Option<ClipOp>,
}

/// Paste operation currently resolving name conflicts.
/// Items with no conflict are directly "resolved" (original name); those
/// whose name is already taken wait for a user decision via the popup.
/// Once all conflicts are settled, `resolved` is executed.
pub(in crate::bridge) struct PasteJob {
    pub(in crate::bridge) op: ClipOp,
    pub(in crate::bridge) dst_dir: PathBuf,
    /// Sources still waiting to be arbitrated (name conflict).
    pub(in crate::bridge) pending: std::collections::VecDeque<PathBuf>,
    /// Triples (source, final target, overwrite) ready to execute. `overwrite =
    /// true` ("Replace" resolution) → the worker deletes the existing target
    /// before the copy/move; `false` (rename / no conflict) → the
    /// target is guaranteed to be free.
    pub(in crate::bridge) resolved: Vec<(PathBuf, PathBuf, bool)>,
    /// Source whose conflict is currently shown in the popup.
    pub(in crate::bridge) current: Option<PathBuf>,
    /// Keeps external staging data alive until this paste is resolved and its
    /// background copy/move has completed.
    pub(in crate::bridge) transient_cleanup: Option<TransientDropGuard>,
}

impl PasteJob {
    /// `true` if this paste already sends one of its sources to `target`.
    /// Nothing exists on disk while the job is being arbitrated, so this is the
    /// only thing keeping two of its items off the same destination.
    ///
    /// The comparison follows the platform's case rules. The filesystem check
    /// beside it does so for free — `exists` answers for a name spelled
    /// differently — but that check cannot see a destination no item has
    /// written yet. Two names typed in the popup differing only in case would
    /// otherwise both be accepted, and the second copy would land on the first.
    pub(in crate::bridge) fn claims(&self, target: &Path) -> bool {
        self.resolved
            .iter()
            .any(|(_, dst, _)| ops::paths_equal(dst, target))
    }
}

/// Context frozen at the moment of the drop, before the possible opening of the
/// Move/Copy/Link menu. Visual indices can change due to the watcher;
/// the paths, however, remain the ones the user actually targeted.
pub(in crate::bridge) struct PendingFileDrop {
    pub(in crate::bridge) sources: Vec<PathBuf>,
    pub(in crate::bridge) destination: PathBuf,
}
