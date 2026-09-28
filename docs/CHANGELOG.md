# Changelog

Newest entries at the bottom. Each entry records the date, what changed, and the
files touched, so a behaviour can be traced back to its source.

## 2026-09-28 — Windows previews: unified on the system shell thumbnail API

**Changed**

- `crates/favnyr-gui/src/bridge.rs`
  - `generate_thumb` now asks the system shell thumbnail API
    (`winthumb::shell_thumbnail`, i.e. `IShellItemImageFactory`, backed by the OS
    thumbnail cache) first for **every** file on Windows; Favnyr's own decoders
    are only a fallback when the shell returns nothing (PDFs keep WinRT as a last
    resort). The previous per-type routing (images/audio decoded in-house first,
    PDF via WinRT) is gone.
  - `thumbnail_kind_for_row` accepts **every non-folder entry** on Windows (the
    system decides whether it has a thumbnail); other platforms keep the previous
    image / video / MP3-FLAC / PDF whitelist. Since `preview_capable` derives from
    it, every file can now show a preview in previews mode.
  - Removed the now-unused `.lnk` special case and the `lnk_thumbnail_kind`
    function (`.lnk` now goes through the shell like any other file).
- `crates/favnyr-core/src/fs.rs`
  - Added `FileKind::from_code` (inverse of `as_i32`) to decode a row's kind code,
    with a round-trip test.

**Docs**

- `README.md` — rewrote the "Previews" section; corrected the thumbnail-cache
  wording under "Private by construction".
- `docs/thumbnails.md` — new: where previews come from on each system, and the
  single function that decides it.

**Notes**

- Effect: types the OS can preview but Favnyr cannot decode (Office documents,
  e-books, fonts, archives, shortcuts…) now show a thumbnail on Windows. Files
  with no system thumbnail keep their type icon (`SIIGBF_THUMBNAILONLY`).
- Behaviour on Linux is unchanged.
- Verified: `cargo test -p favnyr-core -p favnyr-gui` → 332 passed, 0 failed.

## 2026-09-28 — Grid mode, section headers, category grouping, subfolder contents

**Added**

- **Grid display mode** — a third `ViewMode` beside list and previews. Tiles sit
  in a square art box above the name; `grid_metrics(zoom, width)` packs as many
  columns as fit, a section header restarts the line, and the panel publishes its
  list-area width through the new `rows-area-width` callback so the packing can
  happen. Ctrl+wheel keeps resizing (the zoom *is* the tile size); a window
  resize only re-packs when the new width falls in a different packing bucket.
- **Section headers** — grouping now emits a real header row (label + localized
  "N items", collapsible) instead of only ordering entries. Section keys are
  `cat:<code>` (categories), `sub:<path>` (subfolders) or empty (unlabelled, not
  foldable). Folded sections are stored on the tab as `collapsed`.
- **By category** — a fourth group mode beside folders-first / files-first /
  mixed, bucketing into Folders / Images / Video / Audio / Documents / Other
  (`favnyr_core::fs::Category`); the tab's sort criterion keeps ordering entries
  inside a bucket.
- **Show subfolder contents** — a toolbar toggle that appends one section per
  direct subfolder holding that folder's entries (one level, no recursion). The
  sections appear pending and are filled by a background scan worker
  (`sub_gen` marks stale results), so the listing never blocks on N reads.
- **Keyboard cursor left/right** — `cursor-left` / `cursor-right` walk the tiles
  of a grid line (no-op in the single-column list) and `extend-left` /
  `extend-right` extend the selection. Headers are never a cursor stop and never
  enter a selection (select-all, range, rubber band and the footer count all
  skip them).
- **Persistence** — a tab now stores `view_mode`, `subfolders` and `collapsed`
  next to its path and sort; workspaces written before the grid still load
  through the legacy `preview` flag.
- i18n keys `view_mode_list/previews/grid`, `show_subfolders_tooltip`,
  `group_category`, `category_folder/image/video/audio/document/other` in all six
  catalogs, plus the four new `[shortcut_action]` labels.

**Changed**

- `crates/favnyr-gui/src/bridge.rs` — `ViewMode::Grid` + `is_grid()`;
  `layout_rows_grid` / `grid_metrics`; `build_sections` emits header rows and the
  category buckets; `RowStyle` carries the packing width; `grid_neighbour` /
  `walk_entries` for cursor movement; selection helpers ignore headers;
  `on_toggle_subfolder_contents`, `request_subfolder_scan`,
  `apply_subfolder_scan`, `subfolder_group`; `Tab::restored` grew `subfolders`
  and `collapsed`.
- `crates/favnyr-gui/src/ui/main_window.slint` — `FileTileView` and
  `SectionHeaderView`, grid-aware hit-testing (`row-idx-at-xy`) and scrolling, the
  view button's three-option menu, the new toolbar button, and the
  `rows-area-width` report. `toggle-section` collapses a header; headers are
  routed in the release branch so they never start a rubber band.
- `crates/favnyr-gui/src/i18n.rs` — the `Strings` struct gained the new labels.
- `crates/favnyr-core/src/fs.rs` — `Category` (`of`, `of_code`, `from_code`) and
  `GroupMode::Category`; section ordering in `sort`.
- `crates/favnyr-core/src/shortcuts.rs` — the four new cursor actions.
- `crates/favnyr-core/src/workspace.rs` — `view_mode`, `subfolders`, `collapsed`
  fields on `TabState` (backward compatible: absent keys keep the old meaning).
- `crates/favnyr-gui/assets/icons/previews.svg`, `folder-expand.svg` — new.

**Docs**

- `docs/view-modes-and-sections.md` — new: the three modes, grid geometry, the
  grouping/category/section model, the one-level subfolder scan and how the tab
  persists it.

**Notes**

- Headers are rows with `kind = -1` and an empty path, so they can never be
  opened, dragged or dropped on; `row_path()` returns `None` for them, which is
  also how the thumbnail/annotation keys stay unambiguous once subfolder
  sections mix folders into one model.
- Rubber-band selection picks by row index, so a band that spans a header also
  touches the entries on the other side of it — a deliberate simplification, the
  headers themselves are never selected.
- Verified: `cargo test -p favnyr-core -p favnyr-gui` → 205 + 138 passed,
  0 failed (1 ignored in `favnyr-core`).
- The GUI itself still needs a visual check: grid mode, the view menu, the
  subfolder-contents button and the category grouping were not exercised on a
  screen.
