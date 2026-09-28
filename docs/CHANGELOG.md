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

## 2026-09-28 — 拆分 13 个超 800 行文件（bridge.rs、main_window.slint、core 七块）

**Changed**

纯搬迁拆分：不改行为、不改对外路径、不引入依赖。所有 `crate::bridge::…`、`favnyr_core::…` 调用路径与 Slint 名称面（`MainWindow` 的 518 个成员）逐字未变。

- `crates/favnyr-gui/src/bridge.rs`（19 001 行）→ `bridge/` **47 个文件**，`mod.rs` 仅 151 行（模块声明 + `pub use` 重导出 + `defer`）。其中：
  - `install()`（5 391 行、197 处回调安装）→ `bridge/install/mod.rs`（编排）+ `cb_*.rs` ×12（按特性簇：nav 15 / view 23 / rows 9 / open 16 / openwith 16 / files 23 / sidebar 14 / favorites 21 / clipboard 12 / dnd 7 / workspace 14 / prefs 27），闭包改为 `pub(super) fn install_*` 并显式传参，**调用顺序保持不变**（即初始化时序）。
  - 其余按主题：`state.rs`（1 508）、`colinfo.rs`、`notices.rs`、`settings.rs`、`drives.rs`、`datafiles.rs`、`open_with/`（picker/shellmenu/recipes/launch）、`favpanel.rs`、`nav.rs`、`tabs.rs`、`naming.rs`、`paste.rs`、`progress.rs`、`listing.rs`、`geometry.rs`、`keys.rs`、`thumbs.rs`、`stats.rs`、`rows/`（mod/build/icons）、`selection.rs`、`language.rs`、`restore.rs`、`tabstrip.rs`、`watcher.rs`、`window.rs`、`workspaces.rs`、`tests.rs`。
  - 子模块改名以避开 `bridge/mod.rs` 里已 `use` 的 crate 模块名（`open_with`/`workspaces`/`favpanel`/`progress`/`keys`/`colinfo`/`cb_prefs`）。
- `crates/favnyr-gui/src/ui/main_window.slint`（15 897 行）→ `ui/` 11 个 `.slint`：`structs.slint`、`theme.slint`、`widgets/{rows,inputs,menus,tabs}.slint`、`panel.slint`、`sidebar.slint`、`workspaces.slint`、`progress.slint`；`main_window.slint` 7 483 行，保留 `MainWindow` 契约面与布局骨架。组件体逐字节未改（只加 `export component` 与 `import`）。`main_window.slint` 新增一行 `export { CtxNav } from "theme.slint";`——只有根文档的导出会被 `include_modules!()` 再导出，Rust 侧 `crate::CtxNav` 依赖它。**覆盖层（约 40 个区块）仍留在 `main_window.slint`，未提取**（见 Notes）。
- `crates/favnyr-core`：`fs.rs` → `fs/{mod,typed_path,sort,format,stats,tests}.rs`；`fs/ops.rs` → `fs/ops/{mod,rename,name,copy,link,deletion,path_eq,tests}.rs`；`places.rs` → `places/{mod,windrives,linux,tests}.rs`；`thumbnail.rs` → `thumbnail/{mod,decode,psd,affinity,audio,video,pdf,tests}.rs`；`layout.rs` → `layout/{mod,tree,geom,tests}.rs`；`workspace.rs` → `workspace/{mod,state,named,tests}.rs`；`openers.rs` → `openers/{mod,tag,model,store,tests}.rs`。
- `crates/favnyr-gui`：`i18n.rs` → `i18n/{mod,strings,labels,units,messages,tests}.rs`；`actions.rs` → `actions/{mod,opening,spawn,shell,properties,terminal,program,ffmpeg,timezone,tests}.rs`（按功能分，平台差异保持就地 `#[cfg]`）；`openwith.rs` → `openwith/{mod,windows,linux,tests}.rs`（`#[path] mod imp` 选平台）；`winddrag.rs` → `winddrag/{mod,target,paths,virtual_files,drag_out,formats,tests}.rs`。
- 可见性只升不降：跨文件项为 `pub(super)`，孙模块为 `pub(in crate::bridge)`；无函数提升为 `pub`。
- 相关注释/文档内的旧路径同步：`crates/favnyr-core/src/openers/mod.rs`（`actions.rs` → `actions/`）。

**Docs**

- `AGENTS.md` — 更新拆分后的路径（`i18n/`、`bridge/thumbs.rs` 的两个缩略图决策点、`core/src/thumbnail/`、`ui/*.slint`），并补一条"大文件已按模块拆分"的导航说明。
- `docs/code-map.md` — 按拆分后的文件树重新生成（171 个自有文件 / 64 443 行；新增 §6 剩余项）。
- `docs/split-plan.md` — 新增 §16：执行状态、与实际拆分相比的 12 处命名/组织偏差、以及有意保留的剩余项。

**Notes**

- 验证：`cargo fmt` 干净、`cargo clippy -p favnyr-gui --all-targets -- -D warnings` 无警告、`cargo test -p favnyr-gui` 138 全绿；GUI 冷启动冒烟通过（恢复工作区 → 进入 Slint 事件循环，无 panic）。
- 尚未人工点检：GUI 行为不在单测覆盖内，导航 / 标签页撕离 / 拖放 / 粘贴进度 / 缩略图 / 设置面板 / 快捷键需要跑一次。
- 有意保留（未做）：`ui/overlays/*`（覆盖层提取需把 `root.*` 改写为显式 `in`/`in-out property` + `callback` 转发，须一个 overlay 一次提交并人工点检）、`panel.slint` 的第二层分块、`bridge/tests.rs` 的再分。拆分后仍超 800 行的文件与理由见 `docs/code-map.md` §6。
- `.slint` 文件统一为 CRLF（仓库 `autocrlf=true`）；Linux 侧编译由 CI 的 `check` 任务覆盖（本机仅 Windows）。
