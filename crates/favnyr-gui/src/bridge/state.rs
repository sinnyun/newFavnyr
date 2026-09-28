use super::*;

mod clipdrop;
mod nav;
mod tab;
mod panel;
mod opreg;

pub(super) use clipdrop::*;
pub(super) use nav::*;
pub(super) use tab::*;
pub(super) use panel::*;
pub(super) use opreg::*;

/// Minimum ratio for one side of a split (guards against degenerate panels).
pub(super) const MIN_SPLIT_RATIO: f32 = 0.08;

/// Context remembered for the "Open with" picker between enumeration
/// (opening) and the user's choice.
pub(super) struct OwPickCtx {
    pub(super) ext: String,
    pub(super) path: PathBuf,
    pub(super) handlers: Vec<openwith::AppHandler>,
    /// Cached rows keep icon extraction out of the typing path.
    pub(super) items: Vec<OpenerItem>,
}

// ---------- Shared application state ----------
//
// Since Slint is single-threaded, `AppState` uses `Rc<RefCell<…>>` and doesn't need
// to be `Send`/`Sync`. The `notify` watcher stays on its internal thread without
// accessing the state; it only schedules a callback on the Slint event loop.

/// Memoized image metadata: `(unix mtime, resolution, depth)`. The mtime
/// is the file VERSION for which resolution/depth were read.
pub(super) type ImgMeta = (i64, String, String);

/// Shared state of the window.
///
/// EVERY field must sit behind an `Rc`. `install` takes this by value and each
/// callback keeps a `clone()` of it, so a field that is not shared is
/// DEEP-COPIED: the handler that writes it and the handler that reads it then
/// hold two different values, and the second sees nothing. It compiles, it runs,
/// and the feature silently does nothing.
#[derive(Clone)]
pub struct AppState {
    pub config: Rc<RefCell<Config>>,
    pub(super) panels: Rc<RefCell<Vec<Panel>>>,
    pub(super) active_panel: Rc<RefCell<usize>>,
    /// Layout tree — source of truth for the arrangement. Leaves
    /// reference an index into `panels`. Always valid for
    /// `panels.len()`.
    pub(super) layout: Rc<RefCell<LayoutNode>>,
    /// Watcher for the displayed folder. `Arc<Mutex>` allows its creation and
    /// installation on a background thread, since `watch` can suffer SMB latency.
    pub(super) watcher: Arc<std::sync::Mutex<Option<RecommendedWatcher>>>,
    /// Watcher generation: incremented on each (re)installation or release
    /// → a STALE background install (more recent navigation, ejection) discards its
    /// watcher instead of installing it.
    pub(super) watcher_gen: Arc<std::sync::atomic::AtomicU64>,
    /// Results of network listings produced off the UI thread then drained via a
    /// Slint callback. The queue is shared since `AppState` itself is `Rc`.
    pub(super) async_listings: Arc<std::sync::Mutex<VecDeque<AsyncListingDelivery>>>,
    pub(super) listing_serial: Arc<AtomicU64>,
    pub(super) clipboard: Rc<RefCell<ClipboardState>>,
    /// Paths received from an OLE drop coming from another Windows instance/app.
    /// Kept until the Move/Copy/Link choice from the drop menu.
    pub(super) external_drop_paths: Rc<RefCell<Vec<PathBuf>>>,
    pub(super) pending_file_drop: Rc<RefCell<Option<PendingFileDrop>>>,
    /// Paste in progress (resolving name conflicts). `None` at rest.
    pub(super) paste_job: Rc<RefCell<Option<PasteJob>>>,
    /// Exact source of the rename popup. A path, not the visual row
    /// index: a watcher/re-sort can rebuild the model while the
    /// dialog is open without ever causing a different entry to be renamed.
    pub(super) rename_source: Rc<RefCell<Option<PathBuf>>>,
    /// Selection frozen at the moment the permanent-delete warning
    /// is opened. A watcher may re-list underneath the popup: the confirmation must
    /// always apply to the announced paths, never to a new row.
    pub(super) delete_pending: Rc<RefCell<Vec<PathBuf>>>,
    /// Last item actually sent to a recoverable trash.
    /// History deliberately limited to one entry, session-only.
    pub(super) last_trashed: Rc<RefCell<Option<PathBuf>>>,
    /// Deliveries from the trash workers to the UI thread.
    pub(super) op_deliveries: Arc<std::sync::Mutex<VecDeque<OpDelivery>>>,
    /// Long operations currently in flight. Empty at rest.
    pub(super) ops: Rc<OpRegistry>,
    /// Item the next re-listing should select and scroll to — the result of the
    /// last operation to finish. Staged here rather than applied on the spot:
    /// the row only exists once the views have been re-read.
    pub(super) focus_after_refresh: Rc<RefCell<Option<PathBuf>>>,
    /// One row per operation toast on screen. Tracks `ops` loosely: a row only
    /// appears past the deferred-appearance threshold, and outlives its
    /// operation until the toast is dismissed.
    pub(super) ops_model: Rc<VecModel<OpProgress>>,
    // Previews / thumbnails -----
    /// Decode scheduler. It deduplicates paths, drops
    /// requests that became unnecessary, and re-prioritizes the queue based on the
    /// visible row ranges published by each panel while scrolling.
    pub(super) thumb_scheduler: Arc<ThumbScheduler>,
    /// In-memory LRU cache (path → image). Session only, nothing on disk.
    pub(super) thumb_cache: Rc<RefCell<ThumbLru>>,
    /// Per-path annotations: the colour assigned to a folder, and the note
    /// attached to an item. Global and written the moment it changes, like the
    /// favorites tree — the data belongs to a path, not to a window layout, so
    /// it deliberately stays out of the workspace "modified" signature.
    ///
    /// Never reached directly: go through `annotations_now` to read and
    /// `annotations_for_update` to write, so the file is re-read first when
    /// another instance has touched it.
    pub(super) annotations: Rc<RefCell<favnyr_core::annotations::AnnotationStore>>,
    /// The way back from the last "even out the views": where it was applied,
    /// the ratios it replaced, and the ones it wrote.
    ///
    /// Session only, never written to disk: a set of proportions is worth
    /// something for a few seconds, unlike a closed tab. Keeping the ratios it
    /// WROTE is what tells a second double-click apart from one that follows a
    /// hand resize — no need to hook every other thing that moves the layout.
    pub(super) equalize_undo: Rc<RefCell<Option<EqualizeUndo>>>,
    /// Annotations whose item is gone, as they stood when the settings panel
    /// was opened, and the keys currently ticked for removal.
    ///
    /// The snapshot is taken once — listing them is a filesystem walk — and the
    /// tick set lives HERE rather than in the view: that is what lets "Select
    /// all" cover every orphan instead of only the rows on screen, and what
    /// keeps the view holding nothing but a mirror of it.
    pub(super) orphans: Rc<RefCell<Vec<favnyr_core::annotations::Orphan>>>,
    pub(super) orphan_selection: Rc<RefCell<std::collections::BTreeSet<String>>>,
    /// Files waiting on the "open all these?" answer. Replaced at every
    /// request, so a selection the user turned down can never be opened later
    /// by a stale confirmation.
    pub(super) pending_open: Rc<RefCell<Vec<PathBuf>>>,
    /// What the file looked like when it was last read or written — modified
    /// time and length. Two Favnyr instances share one file, and a hand edit
    /// changes it too; comparing this is how a stale copy is noticed.
    pub(super) annotations_stamp: Rc<RefCell<Option<(std::time::SystemTime, u64)>>>,
    /// Same role for the favorites tree, which is shared the same way.
    pub(super) favorites_stamp: Rc<RefCell<Option<(std::time::SystemTime, u64)>>>,
    /// Volumes seen but not mounted, with the block-topology signature they
    /// were read at. Listing them costs a subprocess, so it is paid only when
    /// that signature moves — plugging or removing a disk — and never on the
    /// twelve-second beat that merely refreshes capacities.
    pub(super) volumes_cache: Rc<RefCell<(u64, Vec<favnyr_core::places::Place>)>>,
    /// Paths reported by the filesystem watcher whose cached content texture
    /// must be invalidated on the UI thread before the debounced re-list.
    pub(super) thumb_invalidations: Arc<Mutex<HashSet<PathBuf>>>,
    // Recursive folder stats: modification date + size -----
    /// Queue of recursive folder-stats computations — mtime + size in ONE shared
    /// walk — consumed by the background worker.
    pub(super) rmtime_tx: mpsc::Sender<RMtimeJob>,
    pub(super) rmtime_rx: Rc<RefCell<Option<mpsc::Receiver<RMtimeJob>>>>,
    /// Generation: invalidates stale jobs (folder left / a depth changed).
    pub(super) rmtime_gen: Arc<AtomicU64>,
    /// In-memory cache (folder path → recursive Unix mtime). Session only.
    pub(super) rmtime_cache: Rc<RefCell<HashMap<String, i64>>>,
    /// In-memory cache (folder path → recursive total size in bytes). Session
    /// only. Separate from the mtime cache: the two options toggle independently.
    pub(super) size_cache: Rc<RefCell<HashMap<String, u64>>>,
    // "Show subfolder contents": one listing per direct subfolder -----
    /// Queue of subfolder scans — one job per view, each reading every direct
    /// subfolder — consumed by the background worker.
    pub(super) subscan_tx: mpsc::Sender<SubScanJob>,
    pub(super) subscan_rx: Rc<RefCell<Option<mpsc::Receiver<SubScanJob>>>>,
    /// Results produced off the UI thread, drained via a Slint callback.
    pub(super) subscans: Arc<std::sync::Mutex<VecDeque<SubScanDelivery>>>,
    // Image metadata: resolution / depth -----
    /// Queue of image header reads (background worker).
    pub(super) imgmeta_tx: mpsc::Sender<ImgMetaJob>,
    pub(super) imgmeta_rx: Rc<RefCell<Option<mpsc::Receiver<ImgMetaJob>>>>,
    /// Generation: invalidates stale jobs (folder left).
    pub(super) imgmeta_gen: Arc<AtomicU64>,
    /// Cache (file path → (unix mtime, resolution, depth)). Session
    /// only. The mtime records the file VERSION for which the
    /// metadata was read: if the file is modified (image editing → new
    /// mtime), the cache is detected as stale and recomputed (resolution/depth
    /// columns update on their own, without F5).
    pub(super) imgmeta_cache: Rc<RefCell<HashMap<String, ImgMeta>>>,
    /// "Type-ahead" filter of the active view (substring, case-insensitive).
    /// Cleared on navigation / active panel change. Session only.
    pub(super) filter: Rc<RefCell<String>>,
    /// Bounded history of actually closed tabs, persisted in the
    /// current workspace. Order old → recent; `pop()` restores the last one.
    pub(super) closed_tabs: Rc<RefCell<Vec<TabState>>>,
    /// Name of the named workspace the current state derives from, for the
    /// window title. `None` = ad hoc state → title "Favnyr". Persisted in the
    /// current state file via `capture_workspace`.
    pub(super) current_workspace: Rc<RefCell<Option<String>>>,
    /// SAVED snapshot of the current named workspace. This is the single
    /// reference for "modified" detection: both the title and the Update button
    /// query `current_workspace_is_dirty`, which compares this snapshot to the live state
    /// via `workspace_signature`. No I/O is done during normal use.
    pub(super) saved_workspace: Rc<RefCell<Option<WorkspaceState>>>,
    /// Collapsed state of the Shortcuts/Favorites/Drives/Network sections. Copyable and
    /// purely in-memory: it feeds into `capture_workspace` and thus into the same
    /// dirty signature as the tabs, without reading the visual state from Slint at
    /// save time.
    pub(super) sidebar_sections: Rc<Cell<SidebarSectionsState>>,
    /// Effective shortcut map: defaults + config overrides.
    /// Rebuilt on every rebind/reset.
    pub(super) keymap: Rc<RefCell<shortcuts::Keymap>>,
    /// Shortcut conflict pending resolution:
    /// (targeted action, other conflicting action, serialized proposed chord).
    pub(super) pending_conflict: Rc<RefCell<Option<(String, String, String)>>>,
    /// Search filter for the shortcuts list (settings).
    pub(super) shortcut_filter: Rc<RefCell<String>>,
    /// Favorites tree, shared storage (`favorites.toml`).
    pub(super) favorites: Rc<RefCell<favorites::FavStore>>,
    /// "Open with" openers, dedicated store `openers.toml`.
    pub(super) openers: Rc<RefCell<openers::OpenerStore>>,
    /// Filter for the Settings > Open with > Open with list.
    pub(super) opener_filter: Rc<RefCell<String>>,
    /// Full model already built (icons included). Typing in the filter
    /// only clones/filters these items: no new icon extraction
    /// nor path check is triggered on every keystroke.
    pub(super) opener_settings_cache: Rc<RefCell<Vec<OpenerItem>>>,
    /// Application picker context: ext + path + handlers
    /// enumerated by the OS, remembered between opening the picker and the choice.
    pub(super) ow_pick_ctx: Rc<RefCell<Option<OwPickCtx>>>,
    /// Paths waiting to be saved by the "Save as favorite" popup.
    pub(super) fav_save_pending: Rc<RefCell<Vec<PathBuf>>>,
    /// Container ids in the popup dropdown order (index 0 = root).
    pub(super) fav_container_ids: Rc<RefCell<Vec<String>>>,
    /// Last drives signature seen. The periodic poll of the
    /// "Drives" sidebar compares `places::drives_signature()` to this value
    /// and only rebuilds the model if it has changed (subst, USB, `net use`…).
    pub(super) last_drives_sig: Rc<Cell<u64>>,
    /// EPHEMERAL instance (detached via tab tear-off): NEVER persists
    /// the shared workspace — otherwise it would overwrite the layout of
    /// the main instance on every mutation (sort, grouping…).
    pub(super) ephemeral: Rc<Cell<bool>>,
    /// Windows SHELL context menu session: the `IContextMenu` object
    /// and its HMENU stay alive between opening the menu and invocation
    /// (the ids are only valid for this instance). UI thread only (STA).
    pub(super) shell_menu: Rc<RefCell<Option<crate::shellmenu::ShellMenuSession>>>,
    /// Submenus (cascades) of the current shell menu: children ready to
    /// push into the flyout when hovering the parent (index = `ShellCtxEntry.sub`).
    pub(super) shell_subs: Rc<RefCell<Vec<Vec<ShellCtxEntry>>>>,
    /// Top-level shell menu labels SEEN (lazy probe + right-clicks) —
    /// feeds the "Detected Windows entries" box in settings. Session.
    pub(super) shell_known: Rc<RefCell<Vec<String>>>,
    /// Has the lazy shell menu probe already run? Only once
    /// per session: right-clicks then enrich the list.
    pub(super) shell_scanned: Rc<Cell<bool>>,
    /// Screen (OS) scale captured ONCE at startup, BEFORE any application
    /// UI zoom. Stable reference for `apply_ui_scale` (scale = base ×
    /// factor). `0.0` = not yet captured.
    pub(super) ui_base_scale: Rc<Cell<f32>>,
}

/// Maximum number of panels. Splits are "unlimited" by design;
/// this generous ceiling is just a safeguard against pathological
/// layouts (the minimum size bounds the useful depth anyway).
pub const MAX_PANELS: usize = 16;

impl AppState {
    /// Default state: one panel, one tab on `$HOME`. The tab bar
    /// takes the DEFAULT position from settings.
    pub fn new(config: Config) -> Self {
        let mode = config.default_tab_bar_mode.min(2);
        Self::new_at(config, home_dir(), mode)
    }

    /// Opens `dirs` as further tabs of the sole view, then comes back to the
    /// first one.
    ///
    /// Used when a whole view has been detached into a window of its own: the
    /// new instance starts on the tab that was active and adopts the rest here,
    /// in the order they had.
    pub fn adopt_tabs(&self, dirs: &[PathBuf]) {
        if dirs.is_empty() {
            return;
        }
        self.with_tabs_mut(|book| {
            for dir in dirs {
                book.open(dir.clone());
            }
            book.active = 0;
        });
    }

    /// "Blank" state starting on `initial` (one panel, one tab) — used
    /// by tab tear-off (`favnyr --detached-tab <folder> [x y [mode]]`).
    /// `tab_bar_mode`: tab bar position inherited from the source view
    /// (0 top / 1 left / 2 right).
    pub fn new_at(config: Config, initial: PathBuf, tab_bar_mode: u8) -> Self {
        let (rmtime_tx, rmtime_rx) = mpsc::channel();
        let (imgmeta_tx, imgmeta_rx) = mpsc::channel();
        let (subscan_tx, subscan_rx) = mpsc::channel();
        let default_cols = config.default_columns.clone();
        let keymap = shortcuts::Keymap::build(&config.shortcut_overrides);
        Self {
            config: Rc::new(RefCell::new(config)),
            panels: Rc::new(RefCell::new(vec![Panel::with_mode(
                initial,
                default_cols,
                tab_bar_mode,
            )])),
            active_panel: Rc::new(RefCell::new(0)),
            layout: Rc::new(RefCell::new(LayoutNode::Leaf { panel: 0 })),
            watcher: Arc::new(std::sync::Mutex::new(None)),
            watcher_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            async_listings: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            listing_serial: Arc::new(AtomicU64::new(0)),
            clipboard: Rc::new(RefCell::new(ClipboardState::default())),
            external_drop_paths: Rc::new(RefCell::new(Vec::new())),
            pending_file_drop: Rc::new(RefCell::new(None)),
            paste_job: Rc::new(RefCell::new(None)),
            rename_source: Rc::new(RefCell::new(None)),
            delete_pending: Rc::new(RefCell::new(Vec::new())),
            last_trashed: Rc::new(RefCell::new(None)),
            op_deliveries: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            ops: Rc::new(OpRegistry::default()),
            focus_after_refresh: Rc::new(RefCell::new(None)),
            ops_model: Rc::new(VecModel::default()),
            thumb_scheduler: Arc::new(ThumbScheduler::new()),
            thumb_cache: Rc::new(RefCell::new(ThumbLru::new(512))),
            annotations: Rc::new(RefCell::new(
                favnyr_core::annotations::AnnotationStore::load(&paths::annotations_path()),
            )),
            annotations_stamp: Rc::new(RefCell::new(annotations_stamp())),
            favorites_stamp: Rc::new(RefCell::new(favorites_stamp())),
            equalize_undo: Rc::new(RefCell::new(None)),
            pending_open: Rc::new(RefCell::new(Vec::new())),
            orphans: Rc::new(RefCell::new(Vec::new())),
            orphan_selection: Rc::new(RefCell::new(std::collections::BTreeSet::new())),
            volumes_cache: Rc::new(RefCell::new((0, Vec::new()))),
            thumb_invalidations: Arc::new(Mutex::new(HashSet::new())),
            rmtime_tx,
            rmtime_rx: Rc::new(RefCell::new(Some(rmtime_rx))),
            rmtime_gen: Arc::new(AtomicU64::new(0)),
            rmtime_cache: Rc::new(RefCell::new(HashMap::new())),
            size_cache: Rc::new(RefCell::new(HashMap::new())),
            subscans: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            subscan_tx,
            subscan_rx: Rc::new(RefCell::new(Some(subscan_rx))),
            imgmeta_tx,
            imgmeta_rx: Rc::new(RefCell::new(Some(imgmeta_rx))),
            imgmeta_gen: Arc::new(AtomicU64::new(0)),
            imgmeta_cache: Rc::new(RefCell::new(HashMap::new())),
            filter: Rc::new(RefCell::new(String::new())),
            closed_tabs: Rc::new(RefCell::new(Vec::new())),
            current_workspace: Rc::new(RefCell::new(None)),
            saved_workspace: Rc::new(RefCell::new(None)),
            sidebar_sections: Rc::new(Cell::new(SidebarSectionsState::default())),
            keymap: Rc::new(RefCell::new(keymap)),
            pending_conflict: Rc::new(RefCell::new(None)),
            shortcut_filter: Rc::new(RefCell::new(String::new())),
            favorites: Rc::new(RefCell::new(favorites::FavStore::load(
                &paths::favorites_path(),
            ))),
            openers: Rc::new(RefCell::new(openers::OpenerStore::load(
                &paths::openers_path(),
            ))),
            opener_filter: Rc::new(RefCell::new(String::new())),
            opener_settings_cache: Rc::new(RefCell::new(Vec::new())),
            ow_pick_ctx: Rc::new(RefCell::new(None)),
            fav_save_pending: Rc::new(RefCell::new(Vec::new())),
            fav_container_ids: Rc::new(RefCell::new(Vec::new())),
            last_drives_sig: Rc::new(Cell::new(0)),
            ephemeral: Rc::new(Cell::new(false)),
            shell_menu: Rc::new(RefCell::new(None)),
            shell_subs: Rc::new(RefCell::new(Vec::new())),
            shell_known: Rc::new(RefCell::new(Vec::new())),
            shell_scanned: Rc::new(Cell::new(false)),
            ui_base_scale: Rc::new(Cell::new(0.0)),
        }
    }

    /// Builds application state from a restored workspace.
    /// Nonexistent paths (folder deleted/unmounted since the last
    /// session) are replaced with `$HOME` — a tab is never lost,
    /// it's just brought back to a safe point.
    pub fn from_workspace(config: Config, ws: WorkspaceState) -> Self {
        let current_workspace = ws.workspace_name.clone();
        let closed_tabs = ws.closed_tabs.clone();
        let sidebar_sections = ws.sidebar_sections;
        let keymap = shortcuts::Keymap::build(&config.shortcut_overrides);
        let (panels, layout, active_panel) = build_panels(ws);
        let (rmtime_tx, rmtime_rx) = mpsc::channel();
        let (imgmeta_tx, imgmeta_rx) = mpsc::channel();
        let (subscan_tx, subscan_rx) = mpsc::channel();
        let state = Self {
            config: Rc::new(RefCell::new(config)),
            panels: Rc::new(RefCell::new(panels)),
            active_panel: Rc::new(RefCell::new(active_panel)),
            layout: Rc::new(RefCell::new(layout)),
            watcher: Arc::new(std::sync::Mutex::new(None)),
            watcher_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            async_listings: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            listing_serial: Arc::new(AtomicU64::new(0)),
            clipboard: Rc::new(RefCell::new(ClipboardState::default())),
            external_drop_paths: Rc::new(RefCell::new(Vec::new())),
            pending_file_drop: Rc::new(RefCell::new(None)),
            paste_job: Rc::new(RefCell::new(None)),
            rename_source: Rc::new(RefCell::new(None)),
            delete_pending: Rc::new(RefCell::new(Vec::new())),
            last_trashed: Rc::new(RefCell::new(None)),
            op_deliveries: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            ops: Rc::new(OpRegistry::default()),
            focus_after_refresh: Rc::new(RefCell::new(None)),
            ops_model: Rc::new(VecModel::default()),
            thumb_scheduler: Arc::new(ThumbScheduler::new()),
            thumb_cache: Rc::new(RefCell::new(ThumbLru::new(512))),
            annotations: Rc::new(RefCell::new(
                favnyr_core::annotations::AnnotationStore::load(&paths::annotations_path()),
            )),
            annotations_stamp: Rc::new(RefCell::new(annotations_stamp())),
            favorites_stamp: Rc::new(RefCell::new(favorites_stamp())),
            equalize_undo: Rc::new(RefCell::new(None)),
            pending_open: Rc::new(RefCell::new(Vec::new())),
            orphans: Rc::new(RefCell::new(Vec::new())),
            orphan_selection: Rc::new(RefCell::new(std::collections::BTreeSet::new())),
            volumes_cache: Rc::new(RefCell::new((0, Vec::new()))),
            thumb_invalidations: Arc::new(Mutex::new(HashSet::new())),
            rmtime_tx,
            rmtime_rx: Rc::new(RefCell::new(Some(rmtime_rx))),
            rmtime_gen: Arc::new(AtomicU64::new(0)),
            rmtime_cache: Rc::new(RefCell::new(HashMap::new())),
            size_cache: Rc::new(RefCell::new(HashMap::new())),
            subscans: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            subscan_tx,
            subscan_rx: Rc::new(RefCell::new(Some(subscan_rx))),
            imgmeta_tx,
            imgmeta_rx: Rc::new(RefCell::new(Some(imgmeta_rx))),
            imgmeta_gen: Arc::new(AtomicU64::new(0)),
            imgmeta_cache: Rc::new(RefCell::new(HashMap::new())),
            filter: Rc::new(RefCell::new(String::new())),
            closed_tabs: Rc::new(RefCell::new(closed_tabs)),
            current_workspace: Rc::new(RefCell::new(current_workspace)),
            saved_workspace: Rc::new(RefCell::new(None)),
            sidebar_sections: Rc::new(Cell::new(sidebar_sections)),
            keymap: Rc::new(RefCell::new(keymap)),
            pending_conflict: Rc::new(RefCell::new(None)),
            shortcut_filter: Rc::new(RefCell::new(String::new())),
            favorites: Rc::new(RefCell::new(favorites::FavStore::load(
                &paths::favorites_path(),
            ))),
            openers: Rc::new(RefCell::new(openers::OpenerStore::load(
                &paths::openers_path(),
            ))),
            opener_filter: Rc::new(RefCell::new(String::new())),
            opener_settings_cache: Rc::new(RefCell::new(Vec::new())),
            ow_pick_ctx: Rc::new(RefCell::new(None)),
            fav_save_pending: Rc::new(RefCell::new(Vec::new())),
            fav_container_ids: Rc::new(RefCell::new(Vec::new())),
            last_drives_sig: Rc::new(Cell::new(0)),
            ephemeral: Rc::new(Cell::new(false)),
            shell_menu: Rc::new(RefCell::new(None)),
            shell_subs: Rc::new(RefCell::new(Vec::new())),
            shell_known: Rc::new(RefCell::new(Vec::new())),
            shell_scanned: Rc::new(Cell::new(false)),
            ui_base_scale: Rc::new(Cell::new(0.0)),
        };
        // A single read at startup: subsequent live comparisons are
        // entirely in memory.
        state.reload_saved_workspace();
        state
    }

    /// Rebuilds the shortcut map from the config overrides.
    pub(super) fn rebuild_keymap(&self) {
        let km = shortcuts::Keymap::build(&self.config.borrow().shortcut_overrides);
        *self.keymap.borrow_mut() = km;
    }

    /// Replaces the current layout **in place** (panels/tabs/
    /// stretches/active) with that of a loaded workspace. Mutates the content
    /// of the `Rc<RefCell<…>>` without changing the `Rc`s themselves, so that all
    /// shared clones (captured by closures) see the change.
    /// The caller must then repopulate the rows + refresh the UI.
    pub fn replace_with_workspace(&self, ws: WorkspaceState) {
        let closed_tabs = ws.closed_tabs.clone();
        let sidebar_sections = ws.sidebar_sections;
        let (panels, layout, active_panel) = build_panels(ws);
        *self.panels.borrow_mut() = panels;
        *self.layout.borrow_mut() = layout;
        *self.active_panel.borrow_mut() = active_panel;
        *self.closed_tabs.borrow_mut() = closed_tabs;
        self.sidebar_sections.set(sidebar_sections);
    }

    /// Resets the layout to the "blank" first-launch state:
    /// a single panel, a single tab on `$HOME`. Mutates the
    /// `Rc<RefCell<…>>` in place (same `Rc`s, so shared closures see the
    /// change). The caller repopulates the rows + refreshes the UI afterwards.
    pub fn reset_to_blank(&self) {
        // Reset → default columns and tab bar position from the
        // config (user settings).
        let default_cols = self.config.borrow().default_columns.clone();
        let default_mode = self.config.borrow().default_tab_bar_mode.min(2);
        *self.panels.borrow_mut() = vec![Panel::with_mode(home_dir(), default_cols, default_mode)];
        *self.layout.borrow_mut() = LayoutNode::Leaf { panel: 0 };
        *self.active_panel.borrow_mut() = 0;
        self.closed_tabs.borrow_mut().clear();
        self.external_drop_paths.borrow_mut().clear();
        *self.pending_file_drop.borrow_mut() = None;
        // Ad hoc state: no longer tied to a named workspace → title "Favnyr".
        *self.current_workspace.borrow_mut() = None;
        *self.saved_workspace.borrow_mut() = None;
        self.sidebar_sections.set(SidebarSectionsState::default());
    }

    /// Reloads the saved snapshot of the current workspace from disk.
    /// Called at startup only; subsequent saves/loads then update
    /// the snapshot directly from the live state.
    pub(super) fn reload_saved_workspace(&self) {
        let name = self.current_workspace.borrow().clone();
        let saved = name.as_deref().and_then(|name| {
            let dir = paths::workspaces_dir();
            let meta = workspace::list_named_workspaces(&dir)
                .into_iter()
                .find(|m| m.name == name)?;
            workspace::load_named_workspace(&dir, &meta.id)
                .ok()
                .map(|(_, ws)| ws)
        });
        *self.saved_workspace.borrow_mut() = saved;
    }

    /// The live state was just saved successfully: it becomes the new
    /// shared reference for the title and the Update button.
    pub(super) fn remember_workspace_saved(&self) {
        *self.saved_workspace.borrow_mut() = Some(self.capture_workspace().sanitized());
    }

    /// Captures the current panels/tabs state into a serializable
    /// `WorkspaceState`.
    pub fn capture_workspace(&self) -> WorkspaceState {
        let panels = self.panels.borrow();
        let panel_states = panels
            .iter()
            .map(|p| PanelState {
                // The `layout` tree carries the proportions. `stretch` stays neutral
                // and is only used as a fallback if the tree is absent.
                stretch: 1.0,
                active_tab: p.tabs.active,
                tabs: p.tabs.tabs.iter().map(tab_to_state).collect(),
                columns: p.columns.clone(),
                tab_bar_mode: p.tab_bar_mode,
                vbar_width: p.vbar_user_w,
            })
            .collect();
        WorkspaceState {
            active_panel: *self.active_panel.borrow(),
            panels: panel_states,
            layout: Some(self.layout.borrow().clone()),
            workspace_name: self.current_workspace.borrow().clone(),
            closed_tabs: self.closed_tabs.borrow().clone(),
            sidebar_sections: self.sidebar_sections.get(),
        }
    }

    /// Marks this instance as ephemeral: all workspace persistence
    /// becomes no-ops so that a detached window never replaces the shared
    /// state of the main instance.
    pub fn set_ephemeral(&self) {
        self.ephemeral.set(true);
    }

    /// Serializes and writes the current workspace to disk. Errors are
    /// logged without preventing the application from closing.
    pub fn persist_workspace(&self) {
        if self.ephemeral.get() {
            return; // detached instance: never write the shared workspace
        }
        let ws = self.capture_workspace();
        if let Err(err) = ws.save(&paths::workspace_path()) {
            error!(error = %err, "saving workspace failed");
        }
    }

    pub fn snapshot_config(&self) -> Config {
        self.config.borrow().clone()
    }

    pub(super) fn persist_config(&self, mutator: impl FnOnce(&mut Config)) {
        // Several instances share `config.toml`. Starting from the latest
        // on-disk version before each mutation prevents an older instance from
        // later overwriting its full snapshot and reverting, for example,
        // the global sidebar order chosen in another window.
        let path = paths::config_path();
        let mut cfg = match Config::load_or_default(&path) {
            Ok(latest) => latest,
            Err(err) => {
                error!(error = %err, "reloading config before update failed");
                self.config.borrow().clone()
            }
        };
        mutator(&mut cfg);
        let cfg = cfg.sanitized();
        *self.config.borrow_mut() = cfg.clone();
        if let Err(err) = cfg.save(&path) {
            error!(error = %err, "saving config failed");
        }
    }

    /// Current path of the active panel (clone of the active Tab).
    pub(super) fn current_path(&self) -> PathBuf {
        self.with_tabs(|book| {
            let a = book.active;
            book.tabs[a].current_path.clone()
        })
    }

    /// Selection anchor of the active panel (-1 if none).
    pub(super) fn selection_anchor(&self) -> i32 {
        self.with_tabs(|book| {
            let a = book.active;
            book.tabs[a].selection_anchor
        })
    }

    pub(super) fn set_selection_anchor(&self, anchor: i32) {
        self.with_tabs_mut(|book| {
            let a = book.active;
            book.tabs[a].selection_anchor = anchor;
            // The keyboard cursor follows the selection made by the click, so
            // that arrow keys continue from the last clicked row.
            book.tabs[a].cursor = anchor;
        });
    }

    pub(super) fn active_rows_model(&self) -> Rc<VecModel<FileRow>> {
        let panels = self.panels.borrow();
        let idx = *self.active_panel.borrow();
        panels[idx].rows_model.clone()
    }

    /// Read/write access to the active panel's TabBook.
    pub(super) fn with_tabs<R>(&self, f: impl FnOnce(&TabBook) -> R) -> R {
        let panels = self.panels.borrow();
        let idx = *self.active_panel.borrow();
        f(&panels[idx].tabs)
    }
    pub(super) fn with_tabs_mut<R>(&self, f: impl FnOnce(&mut TabBook) -> R) -> R {
        let mut panels = self.panels.borrow_mut();
        let idx = *self.active_panel.borrow();
        f(&mut panels[idx].tabs)
    }
    /// Is the active tab's "extension filter" mode enabled?
    /// Used to route keyboard input (shared with the type-ahead filter).
    pub(super) fn active_ext_filter_on(&self) -> bool {
        self.with_tabs(|b| b.tabs[b.active].ext_filter_on)
    }

    pub(super) fn remember_closed_tab(&self, tab: Tab) {
        let mut history = self.closed_tabs.borrow_mut();
        if history.len() >= workspace::MAX_CLOSED_TABS {
            history.remove(0);
        }
        history.push(tab_to_state(&tab));
    }

    pub(super) fn pop_closed_tab(&self) -> Option<TabState> {
        self.closed_tabs.borrow_mut().pop()
    }

    pub(super) fn has_closed_tabs(&self) -> bool {
        !self.closed_tabs.borrow().is_empty()
    }
}

/// Records all tabs of an explicitly closed panel. The tab that
/// was active is pushed last: "Reopen" therefore restores it first.
pub(super) fn remember_closed_panel(state: &AppState, mut panel: Panel) {
    if panel.tabs.tabs.is_empty() {
        return;
    }
    let active = panel.tabs.active.min(panel.tabs.tabs.len() - 1);
    let active_tab = panel.tabs.tabs.remove(active);
    for tab in panel.tabs.tabs {
        state.remember_closed_tab(tab);
    }
    state.remember_closed_tab(active_tab);
}
