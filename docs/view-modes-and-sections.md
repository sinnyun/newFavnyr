# View modes, sections and subfolder contents

How a listing is turned into rows: the three display modes, the grouping
(including the coarse categories), and the one-level expansion of a folder's
subfolders. Everything here is decided in `crates/favnyr-gui/src/bridge.rs`;
the `.slint` side only draws the geometry it is handed.

## Three display modes

| Mode | Code | Zoom | What a row is |
| --- | --- | --- | --- |
| List | `list` | floor (0) | one line, columns |
| Previews | `previews` | ≥ 1 (default 2) | one line, thumbnail in the icon slot |
| Grid | `grid` | ≥ 1 | a tile: art above the name |

`ViewMode` (`bridge.rs`) carries `code()` / `from_code()` for persistence and
`thumbnails()` / `is_grid()` for behavior. The three modes are chosen from the
view button's menu (list / previews / grid) and cycled by the
`toggle-view-mode` shortcut (Ctrl+P by default). Entry zoom (Ctrl+wheel) still
works in every mode: it is a *size*, while the mode is a *layout* — the grid
keeps whatever level it had, which is exactly its tile size.

### Grid geometry

`layout_rows_grid` packs tiles left to right; a section header restarts the
line. The metrics come from `grid_metrics(zoom, width)`:

- cell = `zoom_to_height(zoom)` (the square art box) + `GRID_NAME_BAND` (34px),
- the cell is never narrower than 72px nor wider than the space available,
- columns = as many as fit with `GRID_GAP` (8px) between them, inside
  `GRID_PAD` (10px) on both sides.

`rows` keep a **non-decreasing `visual_y`** — a whole line shares one band —
so every binary search over the geometry (render window, hit-test, band
selection) stays valid in grid mode exactly as it is in list mode.

The layout needs the width of the list area, which only the view knows: the
panel reports it through `rows-area-width` when `list-area.width` changes
(`Panel::grid_width`). Only a grid re-packs on a width change, and only when the
new width lands in a different *packing bucket* — `grid_metrics` is compared
before and after, so dragging a window edge across a bucket boundary does not
rebuild every row of the model.

## Sections

`build_sections` splits a listing into sections; `build_rows` emits one header
row per labelled section, then its entries (a collapsed section keeps its
header and drops its entries). A header is a row with `role = 1`
(`ROW_ROLE_SECTION`), `kind = -1`, no name and no path — so no thumbnail is
requested for it, no path-keyed operation ever sees it, and it can never be a
drop target.

The grouping modes (`GroupMode`, core) are:

- **Folders first / files first / mixed** — one unlabelled section (`key = ""`)
  holding the whole listing; the order comes from `rfs::sort`.
- **By category** (`GroupMode::Category`) — the coarse buckets
  `Folder / Image / Video / Audio / Document / Other` (`Category::of(FileKind)`,
  core). `rfs::sort` ranks the entries by category, they arrive as consecutive
  runs, and one section per run is emitted with a localized label.

Section keys are stable strings:

- `cat:<code>` — a category section,
- `sub:<path>` — the section of subfolder `<path>`,
- `""` — the unlabelled section, which cannot be folded.

A tab stores the folded keys (`Tab::collapsed`): folding is a view state, not a
re-listing. Headers show a fold caret, an icon, the label, and the item count
(the localized "N items"; `…` while a subfolder scan has not answered yet).

## Subfolder contents ("show subfolder contents")

One level only: every **direct** subfolder gets its own section under the
listing's own entries, holding that subfolder's entries indented by one band.

- `Panel::source` is the cached listing: `own` (this folder's entries) plus
  `dirs` (the subfolder sections). Turning the feature on primes one *pending*
  section per direct subfolder (`pending_subfolders`) so the view never stays
  blank, then asks for a scan.
- `request_subfolder_scan` sends one job per panel to a background worker
  (`spawn_subscan_worker`): it reads each direct subfolder with
  `rfs::list_dir_counted` and sorts with the tab's criterion. A subfolder that
  cannot be read yields an empty section rather than failing the whole scan.
  Inside a subfolder the category grouping falls back to folders-first: without
  a header of its own down there, the category rank would read as noise.
- The result is applied on the UI thread (`apply_subfolder_scan`): the pending
  sections are filled in the cached source and the rows are rebuilt from it, so
  selection, cut marks and folds survive (they are keyed by path).
- Staleness is decided by `Panel::sub_gen`, bumped on every fresh listing,
  every toggle, and every navigation. A delivery whose generation, flag, or root
  no longer matches is dropped: it describes a listing that is not on screen.
- Turning the feature off only hides the sections (the cached source keeps
  them): flipping it back on is instant.

A subfolder's entries live in the same row model as the parent's, so two
different files may share a name — every path-keyed walk (`row_path`,
`selected_paths_of`, drag and drop, the selection anchor) works on full paths
for that reason.

## Pointer and keyboard over headers

A header is a band, not an entry:

- clicking it folds/unfolds the section (right-click opens the background menu),
- it is never selected: `selection_set_only`, `selection_set_range`,
  `selection_set_all`, `selection_apply_band` and `count_selected` all skip
  `role != 0` rows, so Ctrl+A and rubber-band selections count entries only,
- the keyboard cursor never rests on one: `walk_entries` steps over headers,
  `cursor-left` / `cursor-right` move within a grid line (`grid_neighbour`, an
  edge at a header), and `cursor-up` / `cursor-down` move a whole line in grid
  mode and one entry in list mode.

Rubber-band selection stays **index-contiguous** even in grid mode: a band
selects the entries its vertical extent covers, all columns included.

## Persistence

`TabState` (core, serde `default` on the new fields) carries:

- `view_mode` — the mode's `code()`; a workspace written before the grid has
  only the legacy `preview` flag, which `tab_mode_of` maps to list/previews,
- `zoom`, `subfolders`, `collapsed` — with the rest of the tab's view state.

## New user-facing strings

All of them go through `i18n.rs` (`i18n/*.toml`): `view_mode_list`,
`view_mode_previews`, `view_mode_grid`, `show_subfolders_tooltip`,
`group_category`, `category_folder` … `category_other`, and the shortcut names
`cursor-left` / `cursor-right` / `extend-left` / `extend-right` (the last two
groups under `shortcut_action`).
