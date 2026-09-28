use super::*;

/// A panel represents an independent file view: tabs, row
/// model, and selection. A global watcher tracks the active panel and rearms on
/// each navigation.
pub(in crate::bridge) struct Panel {
    pub(in crate::bridge) tabs: TabBook,
    pub(in crate::bridge) rows_model: Rc<VecModel<FileRow>>,
    /// Virtualized window over `rows_model`. The filter follows the
    /// technical field `FileRow.rendered`; operations keep using the
    /// full model and its stable indices.
    pub(in crate::bridge) rendered_rows_model: ModelRc<FileRow>,
    /// Revision of order/geometry. Unlike content notifications
    /// (selection, thumbnail...), it forces hit-tests under a motionless pointer.
    pub(in crate::bridge) rows_revision: Cell<i32>,
    /// Context change (folder/tab) requiring an exact return to the top.
    pub(in crate::bridge) viewport_reset_gen: Cell<i32>,
    /// Last exact viewport published by Slint, in content coordinates.
    pub(in crate::bridge) viewport_top: Cell<f32>,
    pub(in crate::bridge) viewport_height: Cell<f32>,
    /// Half-open interval currently accepted by `rendered_rows_model`.
    pub(in crate::bridge) rendered_first: Cell<usize>,
    pub(in crate::bridge) rendered_end: Cell<usize>,
    /// Path actually loaded into `rows_model`. Lets us distinguish
    /// a refresh on the same folder (preserve selection) from a
    /// context change like a tab/panel switch (reset selection).
    pub(in crate::bridge) displayed_path: PathBuf,
    /// Columns specific to the panel (order, visibility, and width).
    /// Independent from other panels; persisted per panel in the
    /// workspace .toml. Always normalized (`columns::sanitize`).
    pub(in crate::bridge) columns: Vec<ColumnSpec>,
    /// Number of hidden entries in the displayed folder, recomputed on
    /// each listing. Feeds the "· K hidden" footer reminder when
    /// hidden entries aren't shown. Session only (not persisted).
    pub(in crate::bridge) hidden_count: usize,
    /// The displayed folder is UNAVAILABLE (doesn't exist / network drive not
    /// started) → "not found" banner + automatic re-check. Session.
    pub(in crate::bridge) unavailable: bool,
    /// Current scroll of the tab bar along its MAIN AXIS
    /// (`viewport-x` in horizontal mode, `viewport-y` in vertical mode, ≤ 0),
    /// REPORTED by the view (`panel-tabs-scrolled`). Used for the hit-test of the
    /// insertion gap for a tab received from another instance. Session.
    pub(in crate::bridge) tabs_viewport_x: f32,
    /// Tab bar position: 0 = horizontal at the top,
    /// 1 = vertical on the left, 2 = vertical on the right. Persisted per view.
    pub(in crate::bridge) tab_bar_mode: u8,
    /// USER width of the vertical bar (logical px), set via the
    /// handle. `0` = automatic (clamped 40% formula). Persisted per view.
    pub(in crate::bridge) vbar_user_w: f32,
    /// INITIAL listing still awaited from the startup thread: the
    /// startup population is asynchronous so as not to block the display
    /// on a slow network path. Cleared by delivery or by any more
    /// recent listing (navigation, F5, watcher) so as to discard a stale result.
    pub(in crate::bridge) pending_initial: bool,
    /// Ordinary network listing in progress. Distinct from `pending_initial` so
    /// that a stale initial result can't win a race during an
    /// A → B → A navigation.
    pub(in crate::bridge) pending_listing: bool,
    /// Identifier of the network request awaited by this panel.
    pub(in crate::bridge) listing_gen: u64,
    /// Child to select after an asynchronous upward navigation.
    pub(in crate::bridge) pending_select: Option<String>,
    /// Listing the row model was built from. Kept so that folding a section,
    /// switching the display mode or resizing a grid never re-reads the disk.
    pub(in crate::bridge) source: RefCell<Option<RowsSource>>,
    /// Entries actually listed (section headers excluded). The footer counts
    /// what the listing holds, not what the virtualized model contains.
    pub(in crate::bridge) entry_count: Cell<usize>,
    /// Width of the list area, last reported by the view. The grid packs its
    /// tiles with it. `0` = never reported (a default width is used).
    pub(in crate::bridge) grid_width: Cell<f32>,
    /// Columns of the last grid layout (0 outside grid mode); published to the
    /// view for up/down-by-a-line moves.
    pub(in crate::bridge) grid_cols: Cell<i32>,
    /// Generation of the subfolder scan worker, so a stale scan (folder left,
    /// tab closed, option turned off meanwhile) is never delivered.
    pub(in crate::bridge) sub_gen: Cell<u64>,
}

impl Panel {
    /// New panel with an explicit tab bar position (settings
    /// default, inherited on split, instance detached via tear-off).
    pub(in crate::bridge) fn with_mode(initial: PathBuf, columns: Vec<ColumnSpec>, tab_bar_mode: u8) -> Self {
        Self::from_tab(Tab::new(initial), columns, tab_bar_mode, 0.0)
    }

    /// Shared panel initialization for a new location and a tab moved by drag.
    /// The latter keeps its history and view options instead of rebuilding it.
    pub(in crate::bridge) fn from_tab(
        tab: Tab,
        columns: Vec<ColumnSpec>,
        tab_bar_mode: u8,
        vbar_user_w: f32,
    ) -> Self {
        let initial = tab.current_path.clone();
        let (rows_model, rendered_rows_model) = new_row_models();
        Self {
            tabs: TabBook {
                tabs: vec![tab],
                active: 0,
            },
            rows_model,
            rendered_rows_model,
            rows_revision: Cell::new(0),
            viewport_reset_gen: Cell::new(0),
            viewport_top: Cell::new(0.0),
            viewport_height: Cell::new(DEFAULT_RENDER_VIEWPORT_HEIGHT),
            rendered_first: Cell::new(0),
            rendered_end: Cell::new(0),
            displayed_path: initial,
            columns: columns::sanitize(columns),
            hidden_count: 0,
            unavailable: false,
            tabs_viewport_x: 0.0,
            tab_bar_mode: tab_bar_mode.min(2),
            vbar_user_w: vbar_user_w.max(0.0),
            pending_initial: false,
            pending_listing: false,
            listing_gen: 0,
            pending_select: None,
            source: RefCell::new(None),
            entry_count: Cell::new(0),
            grid_width: Cell::new(0.0),
            grid_cols: Cell::new(0),
            sub_gen: Cell::new(0),
        }
    }

    pub(in crate::bridge) fn bump_rows_revision(&self) {
        self.rows_revision
            .set(self.rows_revision.get().wrapping_add(1));
    }

    pub(in crate::bridge) fn reset_rows_viewport(&self) {
        self.viewport_top.set(0.0);
        self.viewport_reset_gen
            .set(self.viewport_reset_gen.get().wrapping_add(1));
    }

    /// Replaces the logical model, marking BEFORE the notification the small
    /// interval to render around the current viewport.
    pub(in crate::bridge) fn replace_rows(&self, mut rows: Vec<FileRow>) {
        let (first, end) = mark_render_window(
            &mut rows,
            self.viewport_top.get(),
            self.viewport_height.get(),
        );
        self.rendered_first.set(first);
        self.rendered_end.set(end);
        self.rows_model.set_vec(rows);
        self.bump_rows_revision();
    }
}

pub(in crate::bridge) fn file_row_is_rendered(row: &FileRow) -> bool {
    row.rendered
}

pub(in crate::bridge) fn new_row_models() -> (Rc<VecModel<FileRow>>, ModelRc<FileRow>) {
    let rows = Rc::new(VecModel::<FileRow>::default());
    let rendered = FilterModel::new(rows.clone(), file_row_is_rendered as fn(&FileRow) -> bool);
    (rows, ModelRc::new(rendered))
}

pub(in crate::bridge) type AsyncListingResult = std::result::Result<(Vec<Entry>, usize), bool>;

/// One "show subfolder contents" scan of a view: every direct subfolder of the
/// listing, read on a background thread. Send by construction — the worker
/// never touches the AppState.
pub(in crate::bridge) struct SubScanJob {
    pub(in crate::bridge) panel: usize,
    /// `Panel::sub_gen` at the time of the request: a scan whose view has moved
    /// on is dropped instead of applied.
    pub(in crate::bridge) sub_gen: u64,
    pub(in crate::bridge) root: PathBuf,
    /// Direct subfolders to read: `(name, path)`, in section order.
    pub(in crate::bridge) dirs: Vec<(String, PathBuf)>,
    pub(in crate::bridge) sort: (SortColumn, SortOrder),
    pub(in crate::bridge) group: GroupMode,
    pub(in crate::bridge) show_hidden: bool,
}

/// Result of one subfolder scan. `Err` = at least one subfolder could not be
/// read (its section says so rather than pretending to be empty).
pub(in crate::bridge) struct SubScanDelivery {
    pub(in crate::bridge) panel: usize,
    pub(in crate::bridge) sub_gen: u64,
    pub(in crate::bridge) root: PathBuf,
    pub(in crate::bridge) dirs: Vec<SubFolder>,
}

/// Send delivery of a network listing. `Err(true)` = access denied; other
/// errors become an unavailable path, same as in the synchronous path.
pub(in crate::bridge) struct AsyncListingDelivery {
    pub(in crate::bridge) panel: usize,
    pub(in crate::bridge) r#gen: u64,
    pub(in crate::bridge) path: PathBuf,
    pub(in crate::bridge) result: AsyncListingResult,
    pub(in crate::bridge) lang: Lang,
    pub(in crate::bridge) collapsed: Vec<String>,
    pub(in crate::bridge) preserved_selected: Vec<PathBuf>,
    pub(in crate::bridge) preserved_anchor: Option<PathBuf>,
}
