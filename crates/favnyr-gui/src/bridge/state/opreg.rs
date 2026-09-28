use super::*;

/// Send events produced by the trash workers then consumed on the
/// Slint thread. `PathBuf`s remain native: no non-UTF-8 name is lost
/// during a delete/restore on Linux.
/// What a worker thread reports back once it has finished with an item, drained
/// on the UI thread where the application state is reachable. Named for the
/// operation rather than the trash: a move reports here too.
pub(in crate::bridge) enum OpDelivery {
    Trashed(PathBuf),
    /// An item the worker ACTUALLY moved, so its colour and its note can follow
    /// it. Reported per item rather than per operation: a move that fails
    /// halfway must carry only what really moved.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// An item removed beyond recovery. The trash is NOT reported here — it can
    /// be restored, so its annotation is kept.
    PermanentlyDeleted(PathBuf),
    /// A path whose content was replaced by the operation. Its colour and note
    /// described what used to be there, and something else is there now, so
    /// they must not stay: a copy landing on the name would otherwise wear the
    /// annotation of the file it displaced. A move immediately writes the
    /// source's own annotation over it, which is the same rule seen from the
    /// other side.
    Replaced(PathBuf),
    RestoreFinished {
        /// Registry entry to release once the restore is applied.
        op_id: i32,
        original_path: PathBuf,
        error: Option<favnyr_core::fs::ops::TrashError>,
    },
}

/// A long operation in flight, tracked in `OpRegistry` under a unique id.
/// Keying by id (rather than a single "one is running" flag) lets the toast,
/// the cancel button and the completion handler each target one exact operation.
pub(in crate::bridge) struct OpHandle {
    /// Cooperative cancellation flag shared with the worker thread. `None` for
    /// operations that cannot be interrupted (restoring from the trash).
    pub(in crate::bridge) cancel: Option<Arc<AtomicBool>>,
    /// Item to highlight (selection + scroll) once THIS operation finishes: the
    /// final target of a paste/move. A full path, since the active view may have
    /// changed folder during the copy — in that case nothing is highlighted.
    pub(in crate::bridge) pending_focus: Option<PathBuf>,
    /// Paths this operation WRITES to. Held until it completes so that another
    /// operation started meanwhile cannot resolve to the same destination.
    /// Deletions write nothing and reserve nothing.
    pub(in crate::bridge) targets: Vec<PathBuf>,
    /// Source and destination names whose content-thumbnail identity may have
    /// changed when this operation finishes.
    pub(in crate::bridge) thumbnail_invalidations: Vec<PathBuf>,
    /// Held until the worker reports completion, then dropped before refresh.
    pub(in crate::bridge) transient_cleanup: Option<TransientDropGuard>,
}

/// Key a destination is reserved under.
///
/// Reservations are matched by hash, so the platform's case rules have to be
/// folded into the key rather than into the comparison. Without it, a name
/// typed in the conflict popup differing only in case from one a running
/// operation is about to write would read as free — `Path::exists` cannot help
/// there, since neither file is on disk yet — and both would land on the same
/// one.
pub(in crate::bridge) fn reservation_key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// The long operations currently running, each under a session-unique id.
#[derive(Default)]
pub(in crate::bridge) struct OpRegistry {
    pub(in crate::bridge) active: RefCell<HashMap<i32, OpHandle>>,
    /// Union of every in-flight operation's `targets`, for O(1) lookup.
    /// A destination is claimed as soon as its operation starts, well before the
    /// file physically exists — that is precisely the window in which a second
    /// operation would otherwise pick the very same name.
    pub(in crate::bridge) reserved: RefCell<HashSet<PathBuf>>,
    /// Last id handed out; ids start at 1, so 0 always means "no operation".
    pub(in crate::bridge) serial: Cell<i32>,
}

impl OpRegistry {
    /// `true` while at least one operation is in flight.
    pub(in crate::bridge) fn in_flight(&self) -> bool {
        !self.active.borrow().is_empty()
    }

    /// Registers an operation and returns the id identifying it until it
    /// completes. Ids are never reused within a session, so a late completion
    /// can't release an operation started afterwards.
    pub(in crate::bridge) fn register(&self, handle: OpHandle) -> i32 {
        let id = self.serial.get().wrapping_add(1).max(1);
        self.serial.set(id);
        self.reserved
            .borrow_mut()
            .extend(handle.targets.iter().map(|target| reservation_key(target)));
        self.active.borrow_mut().insert(id, handle);
        id
    }

    /// Removes a finished operation, releases the destinations it held and
    /// hands back its handle, whose `pending_focus` the caller consumes. `None`
    /// for an unknown id (already released — a completion delivered twice).
    pub(in crate::bridge) fn finish(&self, id: i32) -> Option<OpHandle> {
        let handle = self.active.borrow_mut().remove(&id)?;
        let mut reserved = self.reserved.borrow_mut();
        for target in &handle.targets {
            reserved.remove(&reservation_key(target));
        }
        Some(handle)
    }

    /// `true` if a running operation has already claimed this destination.
    /// Used alongside `Path::exists` wherever a free name is picked, so the
    /// answer covers files that are about to exist.
    pub(in crate::bridge) fn is_reserved(&self, path: &Path) -> bool {
        self.reserved.borrow().contains(&reservation_key(path))
    }

    /// Raises the cancellation flag of one operation, if it is still running
    /// and interruptible. Unknown or uninterruptible ids are a no-op.
    pub(in crate::bridge) fn cancel(&self, id: i32) {
        if let Some(handle) = self.active.borrow().get(&id)
            && let Some(flag) = handle.cancel.as_ref()
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}
