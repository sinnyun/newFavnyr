use super::*;

// Tabs ----------

/// A tab groups a current path, its navigation history, and its
/// sort order. Selection is not persisted between tabs; it is,
/// however, preserved when refreshing the same folder via the
/// `same-dir` mechanism of `refresh_listing`.
#[derive(Debug)]
pub(in crate::bridge) struct Tab {
    pub(in crate::bridge) current_path: PathBuf,
    pub(in crate::bridge) history: NavHistory,
    pub(in crate::bridge) sort: SortState,
    /// Selection anchor for Shift+click. `-1` if none.
    pub(in crate::bridge) selection_anchor: i32,
    /// Display mode: list, previews (thumbnails), or grid. DERIVED from
    /// `zoom` for the two historical modes (`previews = zoom >= THUMB_ZOOM`)
    /// and persisted per tab in the workspace TOML — a workspace written
    /// before the level existed carries only the old `preview` flag.
    pub(in crate::bridge) mode: ViewMode,
    /// Zoom level of entries (Ctrl+wheel). Mapped to a row height by
    /// `zoom_to_height` — or to a tile size in grid mode — and persisted per
    /// tab in the workspace TOML: a view left at a chosen size reopens at that
    /// size, not at the default for its mode.
    pub(in crate::bridge) zoom: i32,
    /// "Show subfolder contents": after the current folder's own entries, the
    /// listing carries one section per direct subfolder holding its entries
    /// (ONE level down, no recursion). Persisted per tab in the workspace TOML.
    pub(in crate::bridge) subfolders: bool,
    /// Sections the user folded away, by section key ("cat:image",
    /// "sub:C:\dir"). Keeps the listing itself untouched: folding only rebuilds
    /// the rows. Persisted per tab in the workspace TOML.
    pub(in crate::bridge) collapsed: Vec<String>,
    /// Whether hidden files are shown (dotfiles + Windows HIDDEN attribute).
    /// `false` by default. Persisted per tab in the workspace TOML.
    pub(in crate::bridge) show_hidden: bool,
    /// Grouping by type (folders first / files first / mixed).
    /// Persisted per tab in the workspace TOML.
    pub(in crate::bridge) group_mode: GroupMode,
    /// "Cursor" row (head of arrow-key keyboard navigation). `-1`
    /// = none. Session only (not persisted).
    pub(in crate::bridge) cursor: i32,
    /// Counter incremented on each keyboard cursor movement → triggers
    /// scroll-into-view on the Slint side (without doing so on other refreshes).
    pub(in crate::bridge) scroll_gen: i32,
    /// Extension filter: bar enabled from the columns menu.
    /// `ext_filter_on` = bar shown; `ext_filter` = free text ("jpg, png",
    /// loose syntax). Session only (not persisted in the workspace).
    pub(in crate::bridge) ext_filter_on: bool,
    pub(in crate::bridge) ext_filter: String,
}

impl Tab {
    pub(in crate::bridge) fn new(initial: PathBuf) -> Self {
        let mut h = NavHistory::default();
        h.push(initial.clone());
        Self {
            current_path: initial,
            history: h,
            sort: SortState::default(),
            selection_anchor: -1,
            mode: ViewMode::List,
            zoom: LIST_DEFAULT_ZOOM,
            show_hidden: false,
            group_mode: GroupMode::FoldersFirst,
            subfolders: false,
            collapsed: Vec::new(),
            cursor: -1,
            scroll_gen: 0,
            ext_filter_on: false,
            ext_filter: String::new(),
        }
    }

    /// Builds a tab restored from a workspace: path + sort + display
    /// mode. History restarts with a single entry (back/forward not
    /// persisted).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::bridge) fn restored(
        path: PathBuf,
        sort: SortState,
        mode: ViewMode,
        zoom: Option<i32>,
        show_hidden: bool,
        group_mode: GroupMode,
        subfolders: bool,
        collapsed: Vec<String>,
    ) -> Self {
        let mut h = NavHistory::default();
        h.push(path.clone());
        // A workspace saved before the level was persisted carries none: the
        // tab reopens at the default for the mode it was in, exactly as it
        // used to. A hand-edited file is clamped rather than trusted.
        let zoom = match zoom {
            Some(z) => z.clamp(MIN_ZOOM, MAX_ZOOM),
            None if mode.thumbnails() => THUMB_DEFAULT_ZOOM,
            None => LIST_DEFAULT_ZOOM,
        };
        // The two historical modes stay derived from the level, never read
        // back: the two are stored side by side and an edited workspace could
        // hold a contradictory pair. The finer value wins. Only the grid is a
        // mode of its own, so only it survives a zoom in the list range.
        let mode = if mode.is_grid() {
            mode
        } else if zoom >= THUMB_ZOOM {
            ViewMode::Previews
        } else {
            ViewMode::List
        };
        Self {
            current_path: path,
            history: h,
            sort,
            selection_anchor: -1,
            mode,
            zoom,
            show_hidden,
            group_mode,
            subfolders,
            collapsed,
            cursor: -1,
            scroll_gen: 0,
            ext_filter_on: false,
            ext_filter: String::new(),
        }
    }
}

pub(in crate::bridge) fn tab_to_state(tab: &Tab) -> TabState {
    TabState {
        path: tab.current_path.display().to_string(),
        sort_column: tab.sort.column,
        sort_order: tab.sort.order,
        preview: tab.mode.thumbnails(),
        show_hidden: tab.show_hidden,
        group_mode: tab.group_mode,
        zoom: Some(tab.zoom),
        view_mode: Some(tab.mode.code().to_string()),
        subfolders: tab.subfolders,
        collapsed: tab.collapsed.clone(),
    }
}

/// Display mode carried by a persisted tab: the explicit `view_mode` when it
/// was written, the legacy `preview` flag otherwise (a workspace older than the
/// grid). A hand-edited file with an unknown code falls back the same way.
pub(in crate::bridge) fn tab_mode_of(state: &TabState) -> ViewMode {
    state
        .view_mode
        .as_deref()
        .and_then(ViewMode::from_code)
        .unwrap_or(if state.preview {
            ViewMode::Previews
        } else {
            ViewMode::List
        })
}

pub(in crate::bridge) fn tab_from_state(tab: TabState) -> Tab {
    let mode = tab_mode_of(&tab);
    Tab::restored(
        resolve_restored_path(&tab.path),
        SortState {
            column: tab.sort_column,
            order: tab.sort_order,
        },
        mode,
        tab.zoom,
        tab.show_hidden,
        tab.group_mode,
        tab.subfolders,
        tab.collapsed,
    )
}

#[derive(Debug)]
pub(in crate::bridge) struct TabBook {
    pub(in crate::bridge) tabs: Vec<Tab>,
    pub(in crate::bridge) active: usize,
}

impl TabBook {
    /// Inserts a tab at a visible gap, clamping stale UI coordinates, and
    /// activates it. Shared by every source that creates or transfers a tab.
    pub(in crate::bridge) fn insert_tab_at(&mut self, at: usize, tab: Tab) -> usize {
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.active = at;
        at
    }

    pub(in crate::bridge) fn open(&mut self, path: PathBuf) -> usize {
        self.insert_tab_at(self.tabs.len(), Tab::new(path))
    }

    /// Inserts `tab` right after tab `idx` and activates the new entry.
    /// Common primitive for contextual openings and duplication.
    pub(in crate::bridge) fn insert_after(&mut self, idx: usize, tab: Tab) -> Option<usize> {
        if idx >= self.tabs.len() {
            return None;
        }
        Some(self.insert_tab_at(idx + 1, tab))
    }

    /// Opens a target right after the active tab. A `TabBook` always has
    /// at least one tab; the fallback at the end nonetheless protects this invariant.
    pub(in crate::bridge) fn open_after_active(&mut self, path: PathBuf) -> usize {
        if self.active >= self.tabs.len() {
            return self.open(path);
        }
        self.insert_after(self.active, Tab::new(path))
            .expect("active tab was validated")
    }

    /// Duplicates tab `idx`: inserts a copy (same path + view settings)
    /// RIGHT AFTER it and activates it. `false` if the index is out of bounds.
    pub(in crate::bridge) fn duplicate(&mut self, idx: usize) -> bool {
        let Some(src) = self.tabs.get(idx) else {
            return false;
        };
        let dup = Tab::restored(
            src.current_path.clone(),
            src.sort,
            src.mode,
            Some(src.zoom),
            src.show_hidden,
            src.group_mode,
            src.subfolders,
            src.collapsed.clone(),
        );
        self.insert_after(idx, dup).is_some()
    }

    /// Closes tab `idx` and returns it. Refuses to close the last one.
    pub(in crate::bridge) fn close(&mut self, idx: usize) -> Option<Tab> {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
        Some(closed)
    }

    pub(in crate::bridge) fn select(&mut self, idx: usize) -> bool {
        if idx < self.tabs.len() && idx != self.active {
            self.active = idx;
            true
        } else {
            false
        }
    }

    /// Moves the tab at `from` to an **insertion position** `to_pre`
    /// computed BEFORE the removal: the user saw the cursor (and the
    /// preview line) at index position `to_pre` in the current list
    /// (so `to_pre ∈ [0, len]` — `len` = insert at the end).
    ///
    /// Internally, this is converted to a "post-remove index":
    ///   - if `to_pre > from`, the post-remove index is `to_pre - 1` (because
    ///     removing `from` shifted the elements to its right);
    ///   - otherwise, the post-remove index stays `to_pre`.
    ///
    /// Adjusts `active` so it keeps pointing at the same Tab.
    pub(in crate::bridge) fn move_tab(&mut self, from: usize, to_pre: usize) -> bool {
        let n = self.tabs.len();
        if from >= n {
            return false;
        }
        let to = if to_pre > from { to_pre - 1 } else { to_pre };
        let to = to.min(n - 1);
        if to == from {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
        true
    }
}

// View panel ----------

/// Display mode of a tab: the shape a listing takes on screen.
/// - `List`: one 28px line per entry, type icon + columns.
/// - `Previews`: one full-width row per entry, thumbnail height from `zoom`.
/// - `Grid`: tiles packed left to right, icon/thumbnail above the name.
///
/// Orthogonal to `GroupMode` (which decides the sections) and to `zoom`
/// (which sizes a row or a tile). `Grid` is a layout, not a size, so it is
/// kept as-is when the zoom changes; the two other modes are derived from the
/// zoom level, exactly as they were before the grid existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::bridge) enum ViewMode {
    List,
    Previews,
    Grid,
}

impl ViewMode {
    pub(in crate::bridge) fn code(self) -> &'static str {
        match self {
            ViewMode::List => "list",
            ViewMode::Previews => "previews",
            ViewMode::Grid => "grid",
        }
    }

    pub(in crate::bridge) fn from_code(s: &str) -> Option<Self> {
        Some(match s {
            "list" => ViewMode::List,
            "previews" => ViewMode::Previews,
            "grid" => ViewMode::Grid,
            _ => return None,
        })
    }

    /// Does this mode display content thumbnails (and enlarged app icons)?
    /// Every mode but the plain list.
    pub(in crate::bridge) fn thumbnails(self) -> bool {
        self != ViewMode::List
    }

    pub(in crate::bridge) fn is_grid(self) -> bool {
        self == ViewMode::Grid
    }
}
