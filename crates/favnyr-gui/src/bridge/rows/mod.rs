use super::*;

mod build;
mod icons;

pub(super) use build::*;
pub(super) use icons::*;

// ===== Entry zoom =====
// Zoom level (Ctrl+wheel) → row height. LIST mode is a floor
// at a single level (zoom 0): Ctrl+wheel down no longer zooms out from there. The FIRST notch
// upward (zoom == THUMB_ZOOM) switches DIRECTLY to thumbnails — we
// removed the old "smaller" (−1) and "larger without
// thumbnail" (+1) list levels, which added nothing and were confusing.
pub(super) const THUMB_ZOOM: i32 = 1; // 1st notch above the list = thumbnails
pub(super) const MIN_ZOOM: i32 = 0; // floor = list mode (no zooming out below)
pub(super) const MAX_ZOOM: i32 = 8; // 220px ≤ MAX_PX (256) → sharp thumbnails
pub(super) const LIST_DEFAULT_ZOOM: i32 = 0; // → 28px (list; coincides with the floor)
pub(super) const THUMB_DEFAULT_ZOOM: i32 = 2; // → 76px ("Previews" button's default)
// Scale invariants verified at COMPILE TIME: the list is the
// single floor and the first notch above enters thumbnail mode.
const _: () = {
    assert!(MIN_ZOOM == LIST_DEFAULT_ZOOM); // no list level below the default
    assert!(LIST_DEFAULT_ZOOM + 1 == THUMB_ZOOM); // one notch = thumbnails (no enlarged list)
    assert!(THUMB_DEFAULT_ZOOM >= THUMB_ZOOM); // the "Previews" button is indeed in thumbnail mode
    assert!(MAX_ZOOM >= THUMB_DEFAULT_ZOOM);
};
/// Historical height of a normal row, kept for single-icon
/// entries when compact Previews mode is active.
pub(super) const COMPACT_ICON_ROW_HEIGHT: f32 = 28.0;
/// Starting height used before Slint publishes the body's actual size.
/// A one-viewport margin is rendered on each side, so there's no cold screen.
pub(super) const DEFAULT_RENDER_VIEWPORT_HEIGHT: f32 = 900.0;

/// Maps a zoom level to a row height in logical px. Must remain
/// the SOLE source (pushed to Slint via `PanelView.row_h`, and reused for the
/// rubber-band hit-test). List: 0 (single floor); thumbnails: ≥ 1.
pub(super) fn zoom_to_height(zoom: i32) -> f32 {
    match zoom.clamp(MIN_ZOOM, MAX_ZOOM) {
        0 => 28.0,                                  // list (floor)
        z => 52.0 + (z - THUMB_ZOOM) as f32 * 24.0, // 1→52, 2→76, … 8→220
    }
}

pub(super) fn effective_row_height(
    preview_capable: bool,
    zoom: i32,
    compact_icon_rows: bool,
) -> f32 {
    let zoomed = zoom_to_height(zoom);
    if zoom >= THUMB_ZOOM && compact_icon_rows && !preview_capable {
        COMPACT_ICON_ROW_HEIGHT
    } else {
        zoomed
    }
}

// ===== Sections =====
// A section is a band of the view introduced by a header row: the categories
// of `GroupMode::Category`, or one direct subfolder in "show subfolder
// contents" mode. Folding a section (header click) drops its entries from the
// row model — the listing behind it never moves.
/// Row role: a plain entry (clickable, selectable, operable).
pub(super) const ROW_ROLE_ENTRY: i32 = 0;
/// Row role: a section header (click folds/unfolds, never selected, never an
/// operation target).
pub(super) const ROW_ROLE_SECTION: i32 = 1;
/// Height of a section header band, in every display mode.
pub(super) const SECTION_HEADER_H: f32 = 26.0;

// ===== Grid =====
/// Gap between two tiles (both axes) and the padding around the packed grid.
pub(super) const GRID_GAP: f32 = 8.0;
pub(super) const GRID_PAD: f32 = 10.0;
/// Band under the icon that carries the name (and size) of a tile.
pub(super) const GRID_NAME_BAND: f32 = 34.0;
/// Width used by the grid before Slint publishes the list area's real width.
pub(super) const DEFAULT_GRID_WIDTH: f32 = 560.0;

/// Layout inputs of a listing: the display mode, the zoom level, and the
/// width available to the rows (the grid packs its tiles with it).
#[derive(Debug, Clone, Copy)]
pub(super) struct RowStyle {
    pub(super) mode: ViewMode,
    pub(super) zoom: i32,
    pub(super) compact_icon_rows: bool,
    /// Width of the list area in logical px. `0` = not published yet.
    pub(super) width: f32,
}

impl RowStyle {
    pub(super) fn width_or_default(self) -> f32 {
        if self.width > 0.0 {
            self.width
        } else {
            DEFAULT_GRID_WIDTH
        }
    }
}

/// Tile metrics of the grid for a zoom level and an available width:
/// `(cell_w, cell_h, columns)`. The tile is a square icon box (the zoom's row
/// height) plus a fixed band for the name, and is never wider than what the
/// list area can hold — a very narrow view gets a single, narrower column.
pub(super) fn grid_metrics(zoom: i32, width: f32) -> (f32, f32, i32) {
    let side = zoom_to_height(zoom);
    let cell_h = side + GRID_NAME_BAND;
    let avail = (width - 2.0 * GRID_PAD).max(1.0);
    let cell_w = (side + 28.0).max(72.0).min(avail);
    let cols = (((avail + GRID_GAP) / (cell_w + GRID_GAP)).floor() as i32).max(1);
    (cell_w, cell_h, cols)
}

/// Computes the full vertical geometry once. The same values are
/// then consumed by Slint and by all the Rust hit-tests: no parallel
/// formula can drift when the heights become heterogeneous.
/// Returns the number of grid columns (0 outside grid mode).
pub(super) fn layout_rows(rows: &mut [FileRow], style: RowStyle) -> i32 {
    if style.mode.is_grid() {
        return layout_rows_grid(rows, style);
    }
    let width = style.width_or_default();
    let mut y = 0.0_f32;
    for (index, row) in rows.iter_mut().enumerate() {
        row.model_index = index as i32;
        row.visual_x = 0.0;
        row.visual_w = width;
        row.visual_y = y;
        row.visual_h = if row.role == ROW_ROLE_SECTION {
            SECTION_HEADER_H
        } else {
            effective_row_height(row.preview_capable, style.zoom, style.compact_icon_rows)
        };
        row.rendered = false;
        y += row.visual_h;
    }
    0
}

/// Grid geometry: tiles packed left to right, a section header taking a
/// full-width band and restarting the line under it. Rows keep a
/// non-decreasing `visual_y` — a whole line shares one band — so every
/// binary search over the geometry (render window, hit-test, band selection)
/// stays valid.
pub(super) fn layout_rows_grid(rows: &mut [FileRow], style: RowStyle) -> i32 {
    let (cell_w, cell_h, cols) = grid_metrics(style.zoom, style.width_or_default());
    let width = style.width_or_default();
    let mut y = 0.0_f32;
    let mut col = 0i32;
    for (index, row) in rows.iter_mut().enumerate() {
        row.model_index = index as i32;
        row.rendered = false;
        if row.role == ROW_ROLE_SECTION {
            if col > 0 {
                y += cell_h + GRID_GAP;
                col = 0;
            }
            row.visual_x = 0.0;
            row.visual_w = width;
            row.visual_y = y;
            row.visual_h = SECTION_HEADER_H;
            y += SECTION_HEADER_H;
            continue;
        }
        if col >= cols {
            y += cell_h + GRID_GAP;
            col = 0;
        }
        row.visual_x = GRID_PAD + col as f32 * (cell_w + GRID_GAP);
        row.visual_w = cell_w;
        row.visual_y = y;
        row.visual_h = cell_h;
        col += 1;
    }
    cols
}

/// Half-open interval of rows that intersect `[low_y, high_y)`.
/// The search is exact because `layout_rows` produces contiguous, monotonic
/// bands. It serves both virtualized rendering and invariant tests.
pub(super) fn row_range_for_content_span<M: Model<Data = FileRow>>(
    model: &M,
    low_y: f32,
    high_y: f32,
) -> (usize, usize) {
    let n = model.row_count();
    if n == 0 || high_y <= low_y {
        return (0, 0);
    }
    let mut first = 0usize;
    let mut right = n;
    while first < right {
        let mid = first + (right - first) / 2;
        let row = model.row_data(mid).expect("row geometry model is dense");
        if row.visual_y + row.visual_h <= low_y {
            first = mid + 1;
        } else {
            right = mid;
        }
    }
    let mut end = first;
    let mut end_right = n;
    while end < end_right {
        let mid = end + (end_right - end) / 2;
        let row = model.row_data(mid).expect("row geometry model is dense");
        if row.visual_y < high_y {
            end = mid + 1;
        } else {
            end_right = mid;
        }
    }
    (first.min(n), end.min(n).max(first.min(n)))
}

pub(super) fn row_range_for_slice(rows: &[FileRow], low_y: f32, high_y: f32) -> (usize, usize) {
    if rows.is_empty() || high_y <= low_y {
        return (0, 0);
    }
    let first = rows.partition_point(|row| row.visual_y + row.visual_h <= low_y);
    let end = rows.partition_point(|row| row.visual_y < high_y);
    (first, end.max(first))
}

/// Marks a viewport + one overscan screen before/after. The number of delegates
/// stays proportional to what can actually be displayed, never to the folder.
pub(super) fn mark_render_window(rows: &mut [FileRow], top: f32, height: f32) -> (usize, usize) {
    let height = height.max(1.0);
    let low = (top - height).max(0.0);
    let high = top.max(0.0) + height * 2.0;
    let (first, end) = row_range_for_slice(rows, low, high);
    for row in &mut rows[first..end] {
        row.rendered = true;
    }
    (first, end)
}

pub(super) fn render_window_changed_indices(
    old_first: usize,
    old_end: usize,
    new_first: usize,
    new_end: usize,
) -> Vec<usize> {
    (old_first..old_end)
        .filter(|index| *index < new_first || *index >= new_end)
        .chain((new_first..new_end).filter(|index| *index < old_first || *index >= old_end))
        .collect()
}

/// Semantic point preserved during a zoom relayout. `row_fraction` also
/// keeps the exact point within a row whose height changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ZoomAnchor {
    pub(super) row_index: usize,
    pub(super) row_fraction: f32,
    pub(super) viewport_y: f32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ZoomViewport {
    pub(super) top: f32,
    pub(super) height: f32,
    pub(super) pointer_y: f32,
}

pub(super) fn row_index_at_content_y_slice(rows: &[FileRow], y: f32) -> Option<usize> {
    if y < 0.0 {
        return None;
    }
    let index = rows.partition_point(|row| row.visual_y + row.visual_h <= y);
    rows.get(index)
        .filter(|row| y >= row.visual_y && y < row.visual_y + row.visual_h)
        .map(|_| index)
}

pub(super) fn zoom_anchor_on_row(
    rows: &[FileRow],
    row_index: usize,
    content_y: f32,
    viewport_top: f32,
) -> Option<ZoomAnchor> {
    let row = rows.get(row_index)?;
    let row_fraction = if row.visual_h > 0.0 {
        ((content_y - row.visual_y) / row.visual_h).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(ZoomAnchor {
        row_index,
        row_fraction,
        viewport_y: content_y - viewport_top,
    })
}

/// Chooses the zoom anchor without depending on virtualized rendering:
/// 1. the single selection if it is currently visible;
/// 2. the point under the pointer;
/// 3. the viewport's center.
///
/// A single off-screen selection deliberately causes no jump: the
/// zoom stays anchored to the context the user is actually
/// looking at.
pub(super) fn capture_zoom_anchor(
    rows: &[FileRow],
    viewport_top: f32,
    viewport_height: f32,
    pointer_y: f32,
) -> Option<ZoomAnchor> {
    if rows.is_empty() {
        return None;
    }
    let top = viewport_top.max(0.0);
    let height = viewport_height.max(1.0);
    let bottom = top + height;

    let mut selected = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.selected)
        .map(|(index, _)| index);
    let first_selected = selected.next();
    let unique_selected = first_selected.filter(|_| selected.next().is_none());
    if let Some(index) = unique_selected {
        let row = &rows[index];
        let visible_top = row.visual_y.max(top);
        let visible_bottom = (row.visual_y + row.visual_h).min(bottom);
        if visible_bottom > visible_top {
            // The middle of the visible portion is always strictly within the
            // row, even if it's partially cut off by the viewport.
            let content_y = (visible_top + visible_bottom) * 0.5;
            return zoom_anchor_on_row(rows, index, content_y, top);
        }
    }

    // An event landing exactly on the bottom edge still belongs to this
    // TouchArea, but `top + height` is already outside the semi-open viewport.
    let pointer_viewport_y = pointer_y.clamp(0.0, (height - 0.5).max(0.0));
    let pointer_content_y = top + pointer_viewport_y;
    if let Some(index) = row_index_at_content_y_slice(rows, pointer_content_y) {
        return zoom_anchor_on_row(rows, index, pointer_content_y, top);
    }

    let center_y = top + height * 0.5;
    row_index_at_content_y_slice(rows, center_y)
        .and_then(|index| zoom_anchor_on_row(rows, index, center_y, top))
}

pub(super) fn restore_zoom_viewport_top(
    rows: &[FileRow],
    anchor: Option<ZoomAnchor>,
    fallback_top: f32,
    viewport_height: f32,
) -> f32 {
    let content_height = rows
        .last()
        .map(|row| row.visual_y + row.visual_h)
        .unwrap_or(0.0);
    let max_top = (content_height - viewport_height.max(1.0)).max(0.0);
    let desired = anchor
        .and_then(|anchor| {
            rows.get(anchor.row_index)
                .map(|row| row.visual_y + row.visual_h * anchor.row_fraction - anchor.viewport_y)
        })
        .unwrap_or(fallback_top);
    desired.clamp(0.0, max_top)
}

/// Specialized relayout for Ctrl+wheel. The anchor capture, any
/// mode-change icons, and the new geometry share the same
/// vector: no second model clone, no listing, and no I/O added
/// to the hot path of notches staying within the same mode.
pub(super) fn zoom_panel_visuals(
    panel: &Panel,
    style: RowStyle,
    crossed_mode: bool,
    viewport: ZoomViewport,
) -> f32 {
    let mut rows: Vec<FileRow> = (0..panel.rows_model.row_count())
        .filter_map(|index| panel.rows_model.row_data(index))
        .collect();
    let anchor = capture_zoom_anchor(&rows, viewport.top, viewport.height, viewport.pointer_y);
    let previous: Vec<(f32, f32)> = rows
        .iter()
        .map(|row| (row.visual_y, row.visual_h))
        .collect();

    if crossed_mode {
        refresh_rows_visuals(&mut rows, style.mode.thumbnails(), style.compact_icon_rows);
    }

    let cols = layout_rows(&mut rows, style);
    panel.grid_cols.set(cols);
    let geometry_changed = rows.iter().zip(previous).any(|(row, (y, h))| {
        (row.visual_y - y).abs() > f32::EPSILON || (row.visual_h - h).abs() > f32::EPSILON
    });
    let anchored_top = restore_zoom_viewport_top(&rows, anchor, viewport.top, viewport.height);

    // `replace_rows` directly pre-marks the virtualized window around the
    // destination, not around the old, now-stale scroll.
    panel.viewport_top.set(anchored_top);
    panel.viewport_height.set(viewport.height.max(1.0));
    if crossed_mode || geometry_changed {
        panel.replace_rows(rows);
    }
    anchored_top
}

pub(super) fn refresh_rows_visuals(rows: &mut [FileRow], preview: bool, compact_icon_rows: bool) {
    for row in rows {
        if row.role != ROW_ROLE_ENTRY {
            continue; // a section header has no icon of its own
        }
        let use_large_icon = preview && (!compact_icon_rows || row.preview_capable);
        let (app_icon, link_folder) = row_app_icon(
            row.path.as_str(),
            row.name.as_str(),
            row.ext.as_str(),
            row.is_dir,
            use_large_icon,
        );
        row.app_icon = app_icon;
        row.link_folder = link_folder;
        if !preview {
            row.thumbnail = Image::default();
        }
    }
}

/// Re-derives the visuals of every thumbnail-bearing panel (the "compact icon
/// rows" setting changed). Rebuilds from the cached listing: no disk I/O, and
/// the textures come back from the LRU with the next thumbnail pass.
pub(super) fn refresh_preview_panel_visuals(state: &AppState) {
    let (compact, lang) = {
        let config = state.config.borrow();
        (config.compact_icon_rows_in_preview, config.language)
    };
    let mut panels = state.panels.borrow_mut();
    for panel in panels.iter_mut() {
        if !panel.tabs.tabs[panel.tabs.active].mode.thumbnails() {
            continue;
        }
        rebuild_panel_rows(
            panel,
            lang,
            compact,
            &annotations_now(state),
            &state.clipboard.borrow(),
        );
    }
}

/// Exact index of the row containing `y` in the vertical content.
/// O(log n) binary search, used on every drag/hover move.
pub(super) fn row_index_at_content_y<M: Model<Data = FileRow>>(model: &M, y: f32) -> i32 {
    if y < 0.0 {
        return -1;
    }
    let mut lo = 0usize;
    let mut hi = model.row_count();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let Some(row) = model.row_data(mid) else {
            return -1;
        };
        if y < row.visual_y {
            hi = mid;
        } else if y >= row.visual_y + row.visual_h {
            lo = mid + 1;
        } else {
            return mid as i32;
        }
    }
    -1
}

/// Indices covered by a vertical content band. The ends outside the
/// content are naturally clamped, unlike the single-point hit-test.
pub(super) fn row_band_for_content_range(
    model: &VecModel<FileRow>,
    y1: f32,
    y2: f32,
) -> (i32, i32) {
    let n = model.row_count();
    if n == 0 {
        return (0, -1);
    }
    let low_y = y1.min(y2);
    let high_y = y1.max(y2);

    let mut lo = 0usize;
    let mut right = n;
    while lo < right {
        let mid = lo + (right - lo) / 2;
        let row = model.row_data(mid).expect("row geometry model is dense");
        if row.visual_y + row.visual_h <= low_y {
            lo = mid + 1;
        } else {
            right = mid;
        }
    }

    let mut upper = 0usize;
    let mut upper_right = n;
    while upper < upper_right {
        let mid = upper + (upper_right - upper) / 2;
        let row = model.row_data(mid).expect("row geometry model is dense");
        if row.visual_y <= high_y {
            upper = mid + 1;
        } else {
            upper_right = mid;
        }
    }
    let hi = upper.saturating_sub(1);
    if lo >= n || upper == 0 || hi < lo {
        (lo as i32, lo as i32 - 1)
    } else {
        (lo as i32, hi as i32)
    }
}

/// Local<->UTC offset (seconds) applied to the "Modified" column. Set
/// by the GUI at startup and on every timezone setting change; read by
/// the row builders (`entry_to_row`, `apply_rmtime_to_row`, preview).
/// `0` = UTC. Atomic global: the initial listing can build rows
/// from a background thread.
static MTIME_OFFSET_SECS: AtomicI64 = AtomicI64::new(0);

pub(super) fn mtime_offset() -> i64 {
    MTIME_OFFSET_SECS.load(Ordering::Relaxed)
}

/// Recomputes the offset from `clock_utc`: `0` (UTC) or the current local offset.
pub(super) fn refresh_mtime_offset(state: &AppState) {
    let off = if state.config.borrow().clock_utc {
        0
    } else {
        actions::local_utc_offset_secs()
    };
    MTIME_OFFSET_SECS.store(off, Ordering::Relaxed);
}

/// Unix seconds for "now" (0 if the clock precedes the epoch). Computed ONCE
/// per listing, then passed to `entry_to_row` for the "age" column.
pub(super) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
