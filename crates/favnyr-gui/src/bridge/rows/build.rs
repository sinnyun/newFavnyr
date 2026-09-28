use super::*;

mod entry;

pub(in crate::bridge) use entry::*;

/// Everything turning an `Entry` into a displayable row needs beyond the entry
/// itself. Bundled because both builders below take the same set, and threading
/// it as loose parameters had grown past the point where a call site reads.
/// Every field is `Copy`, so the two functions destructure it back into the
/// plain names their bodies use.
pub(in crate::bridge) struct RowContext<'a> {
    pub(in crate::bridge) lang: Lang,
    pub(in crate::bridge) now_unix: i64,
    pub(in crate::bridge) style: RowStyle,
    pub(in crate::bridge) annotations: &'a favnyr_core::annotations::AnnotationStore,
    /// The tab asks for "show subfolder contents": the source's subfolder
    /// sections are part of the row model.
    pub(in crate::bridge) subfolders: bool,
}

/// One direct subfolder of the displayed folder, with its own entries: the
/// unit of the "show subfolder contents" sections. `pending` marks a scan
/// still running for it (the header then shows "…" instead of a count).
#[derive(Debug, Clone)]
pub(in crate::bridge) struct SubFolder {
    pub(in crate::bridge) name: String,
    pub(in crate::bridge) path: PathBuf,
    pub(in crate::bridge) entries: Vec<Entry>,
    pub(in crate::bridge) pending: bool,
}

/// Everything a rebuild of the row model needs. Kept beside the model so that
/// folding a section, switching the display mode or resizing a grid panel
/// never re-reads the disk.
#[derive(Debug, Clone)]
pub(in crate::bridge) struct RowsSource {
    pub(in crate::bridge) root: PathBuf,
    /// The current folder's own entries, already sorted and filtered.
    pub(in crate::bridge) own: Vec<Entry>,
    /// One level of direct subfolders, in section order. Empty unless the tab
    /// asks for "show subfolder contents".
    pub(in crate::bridge) dirs: Vec<SubFolder>,
}

/// One block of the view: an optional header (label empty = none) followed by
/// its entries.
pub(in crate::bridge) struct Section {
    /// Stable key of the section, used to remember that it is folded. Empty
    /// for the unlabelled section.
    key: String,
    label: String,
    /// Folder the section's entries live in (`entry_to_row` reads its parent
    /// path from there: a section is exactly one folder).
    parent: PathBuf,
    /// Header carries a folder glyph (subfolder sections do).
    is_folder: bool,
    /// Header shows "…" instead of a count (its scan is still running).
    pending: bool,
    entries: Vec<Entry>,
}

/// Section key of a subfolder. Path-based: two subfolders can share a name
/// across a network path and a local one, the fold must follow the folder.
pub(in crate::bridge) fn sub_section_key(path: &Path) -> String {
    format!("sub:{}", path.display())
}

pub(in crate::bridge) fn category_section_key(category: Category) -> String {
    format!("cat:{}", category.code())
}

/// Label of a category section, translated.
pub(in crate::bridge) fn category_label(lang: Lang, category: Category) -> String {
    let key = match category {
        Category::Folder => "category_folder",
        Category::Image => "category_image",
        Category::Video => "category_video",
        Category::Audio => "category_audio",
        Category::Document => "category_document",
        Category::Other => "category_other",
    };
    i18n::tr(lang, key)
}

/// Splits the sections of a listing: the current folder's own entries (grouped
/// by category when the tab asks for it), then one section per direct
/// subfolder when "show subfolder contents" is on.
pub(in crate::bridge) fn build_sections(
    source: &RowsSource,
    group: GroupMode,
    ctx: &RowContext,
) -> Vec<Section> {
    let lang = ctx.lang;
    let mut sections: Vec<Section> = Vec::new();
    let root = source.root.clone();
    if group == GroupMode::Category {
        // `rfs::sort` ranked the entries by category: they arrive as
        // consecutive runs, in the section order.
        for entry in &source.own {
            let category = Category::of(entry.kind);
            let key = category_section_key(category);
            match sections.last_mut() {
                Some(last) if last.key == key => last.entries.push(entry.clone()),
                _ => sections.push(Section {
                    key,
                    label: category_label(lang, category),
                    parent: root.clone(),
                    is_folder: false,
                    pending: false,
                    entries: vec![entry.clone()],
                }),
            }
        }
    } else if !source.own.is_empty() {
        sections.push(Section {
            key: String::new(),
            label: String::new(),
            parent: root.clone(),
            is_folder: false,
            pending: false,
            entries: source.own.clone(),
        });
    }
    for dir in source.dirs.iter().filter(|_| ctx.subfolders) {
        sections.push(Section {
            key: sub_section_key(&dir.path),
            label: dir.name.clone(),
            parent: dir.path.clone(),
            is_folder: true,
            pending: dir.pending,
            entries: dir.entries.clone(),
        });
    }
    sections
}

/// The "show subfolder contents" sections of a fresh listing: every direct
/// subfolder of `own`, waiting for its scan (`pending`). Shown immediately so
/// the view never stays blank while the background scan runs.
pub(in crate::bridge) fn pending_subfolders(root: &Path, own: &[Entry]) -> Vec<SubFolder> {
    own.iter()
        .filter(|entry| entry.is_dir)
        .map(|entry| SubFolder {
            name: entry.name.clone(),
            path: root.join(&entry.name),
            entries: Vec::new(),
            pending: true,
        })
        .collect()
}

/// A section header row. `name` stays empty on purpose: every name-based
/// lookup (type-ahead, focus after navigation, rename) then walks past a
/// header instead of matching its label.
pub(in crate::bridge) fn section_row(section: &Section, lang: Lang) -> FileRow {
    FileRow {
        name: SharedString::default(),
        name_base: SharedString::default(),
        name_ext: SharedString::default(),
        ext: SharedString::default(),
        path: SharedString::default(),
        size: SharedString::default(),
        modified: SharedString::default(),
        is_dir: section.is_folder,
        drop_runnable: false,
        is_symlink: false,
        folder_slot: 0,
        comment: SharedString::default(),
        // No kind: `FileKind::from_code` rejects it, so no thumbnail is ever
        // requested for a header.
        kind: -1,
        selected: false,
        cut: false,
        hidden: false,
        age: SharedString::default(),
        age_bucket: -1,
        resolution: SharedString::default(),
        depth: SharedString::default(),
        thumbnail: Image::default(),
        app_icon: Image::default(),
        link_folder: false,
        preview_capable: false,
        visual_x: 0.0,
        visual_w: 0.0,
        visual_y: 0.0,
        visual_h: SECTION_HEADER_H,
        model_index: 0,
        rendered: false,
        role: ROW_ROLE_SECTION,
        section: section.key.clone().into(),
        section_label: section.label.clone().into(),
        section_count_text: if section.pending {
            // The scan is still running: the count is not known yet.
            i18n::tr(lang, "list_ellipsis").into()
        } else {
            i18n::footer_items_text(lang, section.entries.len()).into()
        },
        section_pending: section.pending,
    }
}

/// Builds the whole row model of a listing: one header per labelled section
/// (the folded ones keep their header and drop their entries), then the
/// entries, then the geometry for the current display mode.
pub(in crate::bridge) fn build_rows(
    source: &RowsSource,
    group: GroupMode,
    ctx: &RowContext,
    collapsed: &[String],
) -> (Vec<FileRow>, i32) {
    let sections = build_sections(source, group, ctx);
    let mut rows: Vec<FileRow> = Vec::new();
    for section in &sections {
        let folded = !section.key.is_empty() && collapsed.iter().any(|k| k == &section.key);
        if !section.label.is_empty() {
            rows.push(section_row(section, ctx.lang));
        }
        if folded {
            continue;
        }
        let parent_display = section.parent.display().to_string();
        for entry in &section.entries {
            rows.push(entry_to_row(entry, &parent_display, ctx));
        }
    }
    let cols = layout_rows(&mut rows, ctx.style);
    (rows, cols)
}

/// Full path of an entry row. `None` for a section header: they carry no path
/// on purpose (see `section_row`), so every path-keyed walk skips them.
pub(in crate::bridge) fn row_path(row: &FileRow) -> Option<PathBuf> {
    if row.role != ROW_ROLE_ENTRY || row.name.is_empty() {
        return None;
    }
    Some(PathBuf::from(row.path.as_str()).join(row.name.as_str()))
}

/// Index of the first row holding `path`, headers skipped.
pub(in crate::bridge) fn row_index_of_path(rows: &[FileRow], path: &Path) -> Option<usize> {
    rows.iter()
        .position(|row| row_path(row).as_deref() == Some(path))
}

/// First ENTRY (a row that is not a section header) at or after `from` when
/// `step` is 1, at or before it when it is -1. `None` when the walk leaves the
/// model. Section headers are not entries: the keyboard cursor never rests on
/// one, and neither does a selection.
pub(in crate::bridge) fn walk_entries<M: Model<Data = FileRow>>(
    model: &M,
    from: i32,
    step: i32,
) -> Option<i32> {
    let n = model.row_count() as i32;
    let mut i = from;
    while i >= 0 && i < n {
        if model
            .row_data(i as usize)
            .is_some_and(|row| row.role == ROW_ROLE_ENTRY)
        {
            return Some(i);
        }
        i += step;
    }
    None
}

/// Grid neighbour of entry `base`: `dy` steps to the line above/below (`dx` to
/// the tile beside it, same line). Reads the geometry the layout published, so
/// a section header — which restarts the packing — is an edge, and the target
/// keeps the column as closely as its (possibly shorter) line allows.
pub(in crate::bridge) fn grid_neighbour<M: Model<Data = FileRow>>(
    model: &M,
    base: i32,
    dx: i32,
    dy: i32,
) -> Option<i32> {
    let row = model.row_data(usize::try_from(base).ok()?)?;
    if dx != 0 {
        let i = usize::try_from(base + dx).ok()?;
        let other = model.row_data(i)?;
        return (other.role == ROW_ROLE_ENTRY && other.visual_y == row.visual_y)
            .then_some(i as i32);
    }
    // The line of `base`: the contiguous run of entries sharing its band.
    let mut lo = base;
    while lo > 0 {
        match model.row_data((lo - 1) as usize) {
            Some(prev) if prev.role == ROW_ROLE_ENTRY && prev.visual_y == row.visual_y => lo -= 1,
            _ => break,
        }
    }
    let mut hi = base;
    while hi + 1 < model.row_count() as i32 {
        match model.row_data((hi + 1) as usize) {
            Some(next) if next.role == ROW_ROLE_ENTRY && next.visual_y == row.visual_y => hi += 1,
            _ => break,
        }
    }
    let n = model.row_count() as i32;
    let probe = if dy < 0 { lo - 1 } else { hi + 1 };
    let first = model.row_data(usize::try_from(probe).ok()?)?;
    if first.role != ROW_ROLE_ENTRY {
        return None; // a header (or the end of the listing) borders the line
    }
    // Bounds of that line, its own run of tiles.
    let (mut start, mut end) = (probe, probe);
    if dy < 0 {
        while start > 0 {
            match model.row_data((start - 1) as usize) {
                Some(prev) if prev.role == ROW_ROLE_ENTRY && prev.visual_y == first.visual_y => {
                    start -= 1;
                }
                _ => break,
            }
        }
    } else {
        while end + 1 < n {
            match model.row_data((end + 1) as usize) {
                Some(next) if next.role == ROW_ROLE_ENTRY && next.visual_y == first.visual_y => {
                    end += 1;
                }
                _ => break,
            }
        }
    }
    // The nearest tile of that line to the current column.
    let mut best: Option<(f32, i32)> = None;
    for i in start..=end {
        let Some(r) = model.row_data(i as usize) else {
            continue;
        };
        let d = (r.visual_x - row.visual_x).abs();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Paths of the selected rows of a model, headers excluded.
pub(in crate::bridge) fn selected_paths_of<M: Model<Data = FileRow>>(model: &M) -> Vec<PathBuf> {
    (0..model.row_count())
        .filter_map(|index| model.row_data(index))
        .filter(|row| row.selected)
        .filter_map(|row| row_path(&row))
        .collect()
}

/// Path of the row a selection anchor points at, headers excluded.
pub(in crate::bridge) fn anchor_path_of<M: Model<Data = FileRow>>(
    model: &M,
    anchor: i32,
) -> Option<PathBuf> {
    let index = usize::try_from(anchor).ok()?;
    row_path(&model.row_data(index)?)
}

/// Re-applies a preserved selection and the cut marks after a re-listing, by
/// PATH: a row model can now hold entries of several folders (subfolder
/// sections), where two different files may share one name.
pub(in crate::bridge) fn apply_preserved_marks(
    rows: &mut [FileRow],
    selected: &[PathBuf],
    cut: &[PathBuf],
) {
    if !selected.is_empty() {
        let set: HashSet<&Path> = selected.iter().map(PathBuf::as_path).collect();
        for row in rows.iter_mut() {
            if row_path(row).is_some_and(|path| set.contains(path.as_path())) {
                row.selected = true;
            }
        }
    }
    if !cut.is_empty() {
        let set: HashSet<&Path> = cut.iter().map(PathBuf::as_path).collect();
        for row in rows.iter_mut() {
            if row_path(row).is_some_and(|path| set.contains(path.as_path())) {
                row.cut = true;
            }
        }
    }
}

/// Entries the clipboard holds as cut. Marked by full path, so an entry of a
/// subfolder section is marked exactly when it is itself a cut item.
pub(in crate::bridge) fn clipboard_cut_paths(clipboard: &ClipboardState) -> Vec<PathBuf> {
    if matches!(clipboard.op, Some(ClipOp::Cut)) {
        clipboard.paths.clone()
    } else {
        Vec::new()
    }
}

/// Row style of a panel right now: the active tab's mode and zoom, and the
/// list-area width last reported by the view.
pub(in crate::bridge) fn panel_row_style(panel: &Panel, compact_icon_rows: bool) -> RowStyle {
    let tab = &panel.tabs.tabs[panel.tabs.active];
    RowStyle {
        mode: tab.mode,
        zoom: tab.zoom,
        compact_icon_rows,
        width: panel.grid_width.get(),
    }
}

/// Rebuilds a panel's row model from its CACHED listing. This is the single
/// path of every change that touches only the SHAPE of the view — folding a
/// section, switching the display mode or the grouping, zooming, resizing a
/// grid — and it reads no disk. Selection and cut marks survive, keyed by path.
/// Returns the number of grid columns.
pub(in crate::bridge) fn rebuild_panel_rows(
    panel: &mut Panel,
    lang: Lang,
    compact_icon_rows: bool,
    annotations: &favnyr_core::annotations::AnnotationStore,
    clipboard: &ClipboardState,
) -> i32 {
    let style = panel_row_style(panel, compact_icon_rows);
    let (group, collapsed, anchor, subfolders) = {
        let tab = &panel.tabs.tabs[panel.tabs.active];
        (
            tab.group_mode,
            tab.collapsed.clone(),
            tab.selection_anchor,
            tab.subfolders,
        )
    };
    let selected = selected_paths_of(&*panel.rows_model);
    let anchor_path = anchor_path_of(&*panel.rows_model, anchor);
    let cut = clipboard_cut_paths(clipboard);
    if panel.source.borrow().is_none() {
        return 0;
    }
    let mut cols = 0;
    let mut rows = Vec::new();
    {
        let source = panel.source.borrow();
        if let Some(source) = source.as_ref() {
            let ctx = RowContext {
                lang,
                now_unix: now_unix(),
                style,
                annotations,
                subfolders,
            };
            let built = build_rows(source, group, &ctx, &collapsed);
            rows = built.0;
            cols = built.1;
        }
    }
    apply_preserved_marks(&mut rows, &selected, &cut);
    let new_anchor = anchor_path
        .as_deref()
        .and_then(|path| row_index_of_path(&rows, path))
        .map(|index| index as i32)
        .unwrap_or(-1);
    {
        let active = panel.tabs.active;
        panel.tabs.tabs[active].selection_anchor = new_anchor;
        panel.tabs.tabs[active].cursor = new_anchor;
    }
    panel.grid_cols.set(cols);
    panel.replace_rows(rows);
    cols
}

/// Installs a fresh listing into a panel: keeps it as the panel's source (what
/// every shape-only rebuild reads), then builds and publishes the row model and
/// the counts. The result of the previous subfolder scan is DROPPED — the
/// caller restarts the scan if the tab still asks for it.
///
/// `selected` / `anchor` are paths captured before the listing came back: a
/// re-listing of the same folder re-selects what survived, a context change
/// passes nothing. The anchor is re-indexed and becomes the cursor.
#[allow(clippy::too_many_arguments)]
pub(in crate::bridge) fn install_rows(
    panel: &mut Panel,
    path: &Path,
    own: Vec<Entry>,
    style: RowStyle,
    group: GroupMode,
    collapsed: &[String],
    subfolders: bool,
    lang: Lang,
    annotations: &favnyr_core::annotations::AnnotationStore,
    clipboard: &ClipboardState,
    selected: &[PathBuf],
    anchor: Option<&Path>,
) -> usize {
    let count = own.len();
    let cut = clipboard_cut_paths(clipboard);
    let ctx = RowContext {
        lang,
        now_unix: now_unix(),
        style,
        annotations,
        subfolders,
    };
    let (mut rows, cols) = {
        let mut source = RowsSource {
            root: path.to_path_buf(),
            own,
            dirs: Vec::new(),
        };
        // The subfolder sections appear at once, each waiting for its scan:
        // the background pass then only fills them in.
        if subfolders {
            source.dirs = pending_subfolders(&source.root, &source.own);
        }
        let built = build_rows(&source, group, &ctx, collapsed);
        *panel.source.borrow_mut() = Some(source);
        built
    };
    apply_preserved_marks(&mut rows, selected, &cut);
    let new_anchor = anchor
        .and_then(|path| row_index_of_path(&rows, path))
        .map(|index| index as i32)
        .unwrap_or(-1);
    {
        let active = panel.tabs.active;
        panel.tabs.tabs[active].selection_anchor = new_anchor;
        panel.tabs.tabs[active].cursor = new_anchor;
    }
    // A scan in flight belongs to the listing it was started for: it is
    // stale now, delivery-side, without any bookkeeping.
    panel.sub_gen.set(panel.sub_gen.get() + 1);
    panel.entry_count.set(count);
    panel.grid_cols.set(cols);
    panel.replace_rows(rows);
    count
}

/// Selection of a panel's model right now, plus the path its anchor points at:
/// what a re-listing of the same folder must hand back to `install_rows`.
pub(in crate::bridge) fn preserved_selection_of(panel: &Panel) -> (Vec<PathBuf>, Option<PathBuf>) {
    let anchor = panel.tabs.tabs[panel.tabs.active].selection_anchor;
    (
        selected_paths_of(&*panel.rows_model),
        anchor_path_of(&*panel.rows_model, anchor),
    )
}

/// Requests the "show subfolder contents" scan of a panel: one listing per
/// direct subfolder, on a background thread. Called when the feature is turned
/// on and after every fresh listing of a tab that uses it; the sections already
/// exist (pending) by the time the results come back, so they only get filled.
pub(in crate::bridge) fn request_subfolder_scan(state: &AppState, panel_idx: usize) {
    let job = {
        let panels = state.panels.borrow();
        let Some(panel) = panels.get(panel_idx) else {
            return;
        };
        let tab = &panel.tabs.tabs[panel.tabs.active];
        if !tab.subfolders {
            return;
        }
        let source = panel.source.borrow();
        let Some(source) = source.as_ref() else {
            return;
        };
        // One level only: the direct subfolders of the listing.
        let dirs: Vec<(String, PathBuf)> = source
            .own
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| (entry.name.clone(), source.root.join(&entry.name)))
            .collect();
        if dirs.is_empty() {
            return;
        }
        SubScanJob {
            panel: panel_idx,
            sub_gen: panel.sub_gen.get(),
            root: source.root.clone(),
            dirs,
            sort: (tab.sort.column, tab.sort.order),
            group: subfolder_group(tab.group_mode),
            show_hidden: tab.show_hidden,
        }
    };
    let _ = state.subscan_tx.send(job);
}

/// Sort criterion used INSIDE a subfolder section. The category grouping has no
/// header of its own down there, so its rank order would read as noise: a
/// subfolder keeps the folders-first ordering instead.
pub(in crate::bridge) fn subfolder_group(group: GroupMode) -> GroupMode {
    if group == GroupMode::Category {
        GroupMode::FoldersFirst
    } else {
        group
    }
}

/// Reads every direct subfolder of one job, then hands the result to the UI
/// thread through the shared queue. A subfolder that cannot be read yields an
/// empty section rather than failing the whole scan.
pub(in crate::bridge) fn spawn_subscan_worker(
    rx: mpsc::Receiver<SubScanJob>,
    queue: Arc<std::sync::Mutex<VecDeque<SubScanDelivery>>>,
    weak: slint::Weak<MainWindow>,
) {
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            let mut dirs = Vec::with_capacity(job.dirs.len());
            for (name, path) in &job.dirs {
                let mut entries = match rfs::list_dir_counted(path, job.show_hidden) {
                    Ok((entries, _hidden)) => entries,
                    Err(err) => {
                        debug!(error = %err, path = %path.display(), "subfolder scan failed");
                        Vec::new()
                    }
                };
                rfs::sort(&mut entries, job.sort.0, job.sort.1, job.group);
                dirs.push(SubFolder {
                    name: name.clone(),
                    path: path.to_path_buf(),
                    entries,
                    pending: false,
                });
            }
            let delivery = SubScanDelivery {
                panel: job.panel,
                sub_gen: job.sub_gen,
                root: job.root,
                dirs,
            };
            let queued = queue
                .lock()
                .map(|mut pending| pending.push_back(delivery))
                .is_ok();
            if queued {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_subfolders_drain();
                    }
                });
            }
        }
    });
}

/// Fills the pending subfolder sections of a panel with a finished scan. A
/// delivery whose view has navigated, re-listed, or turned the feature off is
/// dropped: it describes a listing that is no longer on screen.
pub(in crate::bridge) fn apply_subfolder_scan(
    window: &MainWindow,
    state: &AppState,
    delivery: SubScanDelivery,
) {
    let compact = state.config.borrow().compact_icon_rows_in_preview;
    let lang = state.config.borrow().language;
    {
        let mut panels = state.panels.borrow_mut();
        let Some(panel) = panels.get_mut(delivery.panel) else {
            return;
        };
        let active = panel.tabs.active;
        if panel.sub_gen.get() != delivery.sub_gen || !panel.tabs.tabs[active].subfolders {
            return;
        }
        {
            let mut source = panel.source.borrow_mut();
            let Some(source) = source.as_mut() else {
                return;
            };
            if source.root != delivery.root {
                return;
            }
            source.dirs = delivery.dirs;
        }
        // The rows are rebuilt from the filled source: selection and cut marks
        // come back by path, section folds are preserved as they are.
        rebuild_panel_rows(
            panel,
            lang,
            compact,
            &annotations_now(state),
            &state.clipboard.borrow(),
        );
    }
    debug!(panel = delivery.panel, "subfolder scan applied");
    update_panels_ui(window, state);
    request_thumbnails(state);
}

/// Applies a display mode to a panel's active tab. The mode change drives the
/// zoom level (the list sits at the floor, previews restore the thumbnail
/// default, the grid keeps whatever level it had — its tile size IS that
/// level), then the rows are laid out again from the cached listing. Shared by
/// the 3-option view menu and the Ctrl+P cycle so the two can never disagree.
pub(in crate::bridge) fn apply_view_mode(
    w: &MainWindow,
    st: &AppState,
    idx: usize,
    mode: ViewMode,
) {
    let compact = st.config.borrow().compact_icon_rows_in_preview;
    let thumbnails = {
        let mut panels = st.panels.borrow_mut();
        let Some(panel) = panels.get_mut(idx) else {
            return;
        };
        let active = panel.tabs.active;
        let changed = {
            let t = &mut panel.tabs.tabs[active];
            if t.mode == mode {
                false
            } else {
                t.mode = mode;
                t.zoom = match mode {
                    ViewMode::List => LIST_DEFAULT_ZOOM,
                    ViewMode::Previews => {
                        if t.zoom < THUMB_ZOOM {
                            THUMB_DEFAULT_ZOOM
                        } else {
                            t.zoom
                        }
                    }
                    ViewMode::Grid => t.zoom.max(THUMB_ZOOM),
                };
                true
            }
        };
        if changed {
            rebuild_panel_rows(
                panel,
                st.config.borrow().language,
                compact,
                &annotations_now(st),
                &st.clipboard.borrow(),
            );
        }
        panel.tabs.tabs[active].mode.thumbnails()
    };
    update_panels_ui(w, st);
    request_thumbnails(st);
    // Turning thumbnails on is when they matter: it's when we warn (once)
    // if `ffmpeg` is missing for video (Linux).
    if thumbnails {
        maybe_warn_ffmpeg_missing(w, st);
    }
}
