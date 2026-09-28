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

## 2026-09-28 — 覆盖层提取：`main_window.slint` 的 40 个区块 → `ui/overlays/*.slint`

**Changed**

仍是纯搬迁拆分：不改行为、不改对外路径、不引入依赖；`MainWindow` 的 518 个成员逐字未变。`main_window.slint` **5 541 → 3 193 行**（其中 552 行是转发），只剩契约面、`FocusScope`、布局骨架与覆盖层实例。

每个覆盖层一次提交（共 10 次），逐个搬移：

| 提交 | 新文件（行数） | 组件 |
| --- | ---: | --- |
| `ad2ae31` | `overlays/toasts.slint`（302） | `OverlayToasts`：进度 toast、拖放指示、swap pill、全局通知 |
| `6623e28` | `overlays/dialogs.slint`（379） | `OverlayDialogs`：永久删除、打开全部、未保存更改、粘贴冲突 |
| `d7a6fa7` | `overlays/workspaces.slint`（383） | `OverlayWorkspaces`：工作区保存行、列表、反馈 toast |
| `91dcce0` | `overlays/custom_command.slint`（393） | `OverlayCustomCommand`：自定义命令编辑器 / 已购应用只读视图 |
| `5919dcc` | `overlays/notes.slint`（742） | `OverlayNotes` + `OverlayNoteBubble`：重命名、备注编辑器、新建文件夹/文件、颜色与备注清理、条目备注气泡 |
| `f7b83a2` | `overlays/favorites.slint` | `OverlayFavorites`：收藏容器命名/保存为收藏/新建容器三个弹窗 |
| `4b2bb4a` | `overlays/openwith.slint`（412） | `OverlayOpenWith`：打开方式/新建子菜单、shell 级联、应用选择器、提升 toast |
| `6f35f2e` | `overlays/menus.slint`（641）+ `overlays/panel_menus.slint`（363） | `OverlayMenus`（主右键菜单、Split、标签栏位置、视图模式、URL、回收站、盘符/网络、"颜色与备注"飞出菜单）+ `OverlayPanelMenus`（标签栏死区、标签页、列、跨视图拖放菜单） |
| `b42688a` | `overlays/favorites.slint`（383） | 收藏容器/收藏项右键菜单并入 `OverlayFavorites` |
| `c0a24cd` | `overlays/settings.slint`（1 507） | `OverlaySettings`：设置面板三页 + shell 扩展列表 + 快捷键行菜单 |

做法（每个覆盖层相同）：

- 包裹组件统一为 `export component OverlayX inherits Rectangle { width: 100%; height: 100%; … }`——透明全窗口宿主，自身不接收输入；实例化在原区块位置，z 序不变。块内 `root.` 现指向宿主（尺寸与窗口一致），`parent.*` 语义不变。
- 窗口契约在实例处显式转发：只读 → `x: root.x;`；覆盖层会写回的（自关标志、表单字段、下拉索引）→ `x <=> root.x;`；动作 → `cb(a) => { root.cb(a); }`。
- 覆盖层拿不到的窗口 id 改为组件内 `callback return-focus();`，在实例处接回 `key-scope`（`notes.slint`、`panel_menus.slint`、`settings.slint`）。
- 快捷键菜单与设置面板同文件、声明在面板之后（原始次序即如此，它必须画在面板之上）。
- `overlays/*.slint` 的 `@image-url` 下沉为 `../../../assets/…`；`main_window.slint` 的 import 表按"只留仍在用的名字"重写。

**Docs**

- `docs/split-plan.md` — §16 阶段 5 状态更新；偏差记录补 6 条（13–18：overlays 实际为 10 文件 11 组件、`menus` 拆两个文件、快捷键菜单落 `settings.slint`、收藏容器菜单并入 `favorites.slint`、`return-focus` 转发、import 重写）；剩余项改写为 `panel.slint` 第二层、`settings.slint` 按页分块、`bridge/tests.rs` 再分。
- `docs/code-map.md` — §3.3 文件表（21 个 `.slint`）、§4.4 依赖方向与覆盖层转发约定、§6 剩余项同步到拆分后状态。

**Notes**

- 验证：每个覆盖层都做**行级校验**（搬走的区块去缩进后与原文逐行比对，`block verbatim`）+ 组件成员与实例转发一一对应核查 + `cargo check -p favnyr-gui` 全绿；`cargo test -p favnyr-core -p favnyr-gui` → core 205 passed / 1 ignored、gui 138 passed、0 failed。
- 尚未人工点检：导航、标签页 tear-off、拖放（含虚拟文件）、粘贴进度、缩略图、设置各页、快捷键（capture 与冲突）需要跑一次。
- 有意保留：`overlays/settings.slint`（1 507 行）按页分块、`panel.slint`（2 897）第二层分块；理由与后续见 `docs/split-plan.md` §16。

## 2026-09-28 — 设置面板按页拆分：`overlays/settings.slint` 1 507 → 外壳 385 + `settings/{general,shortcuts,openwith}`

**Changed**

仍是纯搬迁拆分：不改行为、不改对外契约；`OverlaySettings` 的 81 个成员（29 `in` / 19 `in-out` / 32 `callback` + `return-focus()`）逐字未变，`main_window.slint` 实例处的转发一行未动。三个 tab 页各一次提交：

| 提交 | 新文件（行数） | 内容 |
| --- | ---: | --- |
| `7f8f6cc` | `overlays/settings/shortcuts.slint`（323） | 快捷键页：搜索、捕获、冲突提示、重置 |
| `a7cb679` | `overlays/settings/general.slint`（680） | 常规页：语言、主题、缩放、时钟、标签栏、shell 菜单、列与尺寸 |
| `1d5e3af` | `overlays/settings/openwith.slint`（331） | 打开方式页：打开器、方案（recipes）、Windows shell 条目 |

`overlays/settings.slint` **1 507 → 385 行**：剩下窗口、标题栏、tab 条与快捷键行菜单（菜单必须画在面板之上，留在外壳）。

做法（每页相同）：

- 每页原来是 `if root.settings-tab == N : ScrollView { vertical-stretch: 1; VerticalLayout { … } }`，提取为 `export component SettingsX inherits ScrollView { … }`，页面体去一级缩进逐行搬移；外壳实例处保留 `if root.settings-tab == N : SettingsX { vertical-stretch: 1; … }`。
- 转发沿用覆盖层约定：只读 → `x: root.x;`；页面写回的 → `x <=> root.x;`；动作 → `cb(a, b) => { root.cb(a, b); }`。方向由脚本按"被赋值 / 出现在 `<=>` 右侧 / 被调用"分类。
- 页面 import 用 `"../../structs.slint"` 一级路径；`@image-url` 加深为 `../../../../assets/…`（12 处，快捷键页 5 处）。`settings-tab` 只被边界 `if` 读取，页面不声明该成员。
- 页面内未使用的成员不声明（`shortcut-unassign` 只在 shell 的菜单里被调用）；外壳 import 表只留仍在用的名字（剪掉 13 个孤儿名字）。

**Docs**

- `docs/split-plan.md` — §16 阶段 5 状态更新（设置按页拆分完成，只剩 `panel.slint` 第二层与 `bridge/tests.rs` 再分）；偏差补 19–20（页面放入 `settings/` 子目录、逐页提交与行级校验）。
- `docs/code-map.md` — §3.3 文件表（24 个 `.slint`，新增 `overlays/settings/` 三行、shell 385 行）、§4.4 依赖链改为"两层覆盖层+页面"并补第二层拆分约定、§6 剩余项同步。

**Notes**

- 验证：逐页**行级校验**（页面体去缩进后与原文逐行比对，唯一允许的改写是 12 处 `@image-url` 路径）+ 成员方向三查（声明 / 实例转发 / 页面内使用一致）+ `cargo check -p favnyr-gui` 每个提交后全绿；`cargo test -p favnyr-core -p favnyr-gui` 全绿（core 205 passed / 1 ignored、gui 138 passed、0 failed）。
- 尚未人工点检：设置三页切换与各页交互（含快捷键捕获与冲突、Open with 子页）。
- `panel.slint` 第二层的障碍记录在 `docs/split-plan.md` §16：面板级成员**反向读写**区块内部 id（`tabs-flick.viewport-x/width/viewport-width` 被面板的滚动函数与计时器写入；`sel-touch.*` / `rows-scroll.*` 被 `note-row`、`update-pointer-hover`、`report-rows-viewport` 等读出），这组双向耦合需先厘清归属再动。

## 2026-09-28 — 面板第二层拆分：`panel.slint` 2 897 → 外壳 732 + `panel/` 6 个组件

**Changed**

仍是纯搬迁拆分：不改行为、不动契约。`PanelComponent` 对 `main_window.slint` 的成员面逐字未变，`MainWindow` 的 518 个成员与 `FileKind` 的 `#[repr(u8)]` 取值均未触碰。八次搬迁提交，每次一块：

| 提交 | 新文件（行数） | 内容 |
| --- | ---: | --- |
| `6e1fe75` | `panel/tabs_bar.slint`（373，经 `8fe65dc` 去掉多余包装层） | `PanelTabsBar`：可滚动标签条 + "+" 按钮 + 左右 chevrons + 滚轮 + 标签重排时的边缘自动滚动 |
| `4bdf52e` | `panel/nav_bar.slint`（563） | `PanelNavBar`：历史/视图按钮、路径栏（面包屑 ↔ 编辑框）、跨视图交换 grip、split / close |
| `979fe53` | `panel/selection.slint`（751） | `PanelSelection`：列表区之上的指针层——hover、单击/Ctrl/Shift/右键/双击、橡皮筋 + 边缘自动滚动、中键自动滚动、延后重命名、文件拖拽的边缘滚动 |
| `6ddc7c2` | `panel/list.slint`（761） | `PanelList`：列头 + 行区（列表 / 网格两视图）+ "文件夹不可用"横幅 + 两条滚动条的数据源 |
| `cac4dd8` | （并入 `panel/list.slint`，761 → 846） | 左侧 gutter 条搬进 `PanelList`：它与选择层是**同一个橡皮筋手势**的两半（一个在裁剪区外、一个在内），必须同处一个文件才能共用 `rb-*` 状态 |
| `6db7c0e` | `panel/overlays.slint`（123） | `PanelOverlays`：文件落点 veil、关闭/合并/交换预览、标签拖放落区——纯视觉，无 TouchArea |
| `489ff74` | `panel/scrollbars.slint`（139） | `PanelScrollbars`：两条自绘滚动条（横向 = 列溢出，纵向 = 行溢出），只读写列表已有的两个偏移量——`cac4dd8` 让 `list.slint` 冲到 846 行，这一步把它拉回 761（同时回到 800 以内） |

`panel.slint` **2 897 → 732 行**：只剩各子块实例的放置与转发（垂直标签条 ×2、标签条、导航条、筛选条、列表、页脚、覆盖层），加上面板级状态（工作区、列模型、拖放标志、几何常量）。

**与方案的两点不同**（详见 `docs/split-plan.md` §16 偏差 21–23）：

- 分块按**职责**（tabs_bar / nav_bar / selection / list / scrollbars / overlays），不是方案设想的 header / columns / rows / footer——列头与行区在源文件里本就共用几何与滚动状态，拆开只会把状态推到边界上。
- 方案点名的障碍（面板级成员反向读写区块内部 id）用**状态下沉**解决，而不是用 `in-out` 别名把 Flickable 视口接回面板：`sel-touch.*` 随选择层沉进 `selection.slint`，`rows-scroll.viewport-*` 与橡皮筋状态沉进 `list.slint`（两个指针层同处一个文件，band 状态因此退回私有属性，5 个 `in-out` 转发消失），`tabs-flick` 完全私有、面板改发 `scroll-tabs-*` 回调。跨 `PanelList` 边界只剩 `in gutter-w` 与 `out area-abs-x` / `area-abs-y` / `drag-hover-row`。

- 收尾修复（`99f3c8d`）：提取脚本在 `panel.slint` 的 nav bar 实例处留下一行重复的 `// ===== Nav bar + breadcrumb =====`，已删除（2 897 → 732 行）。

另有三次**纯空白**提交（`9342964`、`1cebf6c`、`f0806dc`）：此前多轮提取把整棵子树留在了一级不足的缩进上（整块读起来像父元素的兄弟，`selection.slint` 里甚至有一句落在第 0 列）。三次改动的 `git diff -w` 均为空、行数不变，覆盖 `panel.slint`、`panel/{list,nav_bar,selection}.slint`、`main_window.slint`、`sidebar.slint`、`workspaces.slint`、`overlays/{custom_command,openwith}.slint`。

**Docs**

- `docs/split-plan.md` — §16 阶段 5 状态改为"面板第二层已完成"，偏差补 21–23（实际分块形状、状态下沉、三类允许的改写与行级校验器），剩余项里 `panel.slint` 一条划掉。
- `docs/code-map.md` — §1 概览（`ui/` 30 个 `.slint`、面板外壳 + 6 组件）、§2 结构树新增 `ui/panel/` 一行、§3.3 文件表补 6 行、§4.4 依赖链加入 `panel/*` 层并改写 `panel.slint` 条目、§6 从"仍超 800 行"表里移除 `panel.slint`。

**Notes**

- 验证：每块搬完都跑**行级校验**——脚本从 `git show HEAD:…` 取原始文件，把搬走的区块去缩进后与新区块逐行比对，只允许三类改写（标识符替换、跨边界几何改读 `root.*` / `parent.*`、注释列对齐），任何越界改写或孤儿 id 都会拒绝写盘；再核对组件声明成员与实例转发一一对应、大括号配平、单文件 ≤ 800 行。每个提交后 `cargo check -p favnyr-gui` 全绿（Slint codegen 确实重跑：`slint-build` 对每个被加载的 `.slint` 都发了 `rerun-if-changed`）。收尾：`cargo test -p favnyr-core -p favnyr-gui` → core 205 passed / 1 ignored、gui 138 passed、0 failed；`cargo fmt --check`、`cargo clippy -p favnyr-gui --all-targets -- -D warnings` 干净。
- 冒烟：`cargo build --bin favnyr` 后冷启动 debug 可执行文件，日志到 `starting Slint event loop init_ms=97`（工作区恢复 panels=1），无 panic、无 Slint 加载错误。
- 尚未人工点检（本轮新增项加粗）：导航、标签页 tear-off、拖放（含虚拟文件）、粘贴进度、缩略图、设置各页、快捷键（capture 与冲突），**列表区选择（单击 / Ctrl / Shift / 右键 / 双击）**、**左侧 gutter 条拉出的橡皮筋与拖到上下边缘的自动滚动**、**列头的排序 / 重排 / 调整宽度**、**两条自绘滚动条的拖动**。
- 本地 `main` 领先 `origin/main`（`6e1fe75`…`f0806dc` 共 11 个提交未推送），本轮未推送。

## 2026-09-28 — 侧栏拆分：`sidebar.slint` 1 113 → 外壳 287 + `sidebar/` 3 个组件

**Changed**

仍是纯搬迁拆分：不改行为、不动契约。`Sidebar` / `FavPanel` / `ActivityRail` 对 `main_window.slint` 的成员面逐字未变，`MainWindow` 的 518 个成员与 `FileKind` 的 `#[repr(u8)]` 取值均未触碰。三次提交，每次一块：

| 提交 | 新文件（行数） | 内容 |
| --- | ---: | --- |
| `21ba83e` | `sidebar/fav.slint`（467） | `FavRow` + `FavPanel`：收藏树的节头 + 展平模型 + 折叠状态 + 重排拖拽；行本身纯视觉，手势上报面板，因此两块同处一文件 |
| `c268611` | `sidebar/rail.slint`（115） | `RailBtn` + `ActivityRail`：最左侧活动栏的三个按钮（侧栏开关 / 工作区 / 设置）与右侧 tooltip |
| `c267f74` | `sidebar/item.slint`（271） | `SidebarItem`：Places 单行——图标 / 标签 / 盘符容量条（`DriveGauge`）、hover 与选中态、拖拽与右键把手 |

`sidebar.slint` **1 113 → 287 行**：只剩节头一族（`SidebarHeaderAction` / `SidebarHeaderActionSlot` / `SidebarSectionHeader`）与 `Sidebar` 容器（节顺序、拖拽重排、把 Places 每行交给 `sidebar/item.slint`）。`main_window.slint` 只改一行 import（`Sidebar` 一行 → `Sidebar` / `FavPanel` / `ActivityRail` 三行，3 193 → 3 195 行）。

**与方案的两点不同**（详见 `docs/split-plan.md` §16 偏差 24–27）：

- 节头一族留在 `sidebar.slint`：`SidebarSectionHeader` 为四个可重排节共用，`SidebarHeaderAction` 还被 `overlays/settings/openwith.slint` 引用，搬走会让设置页反向依赖收藏页。于是 `fav.slint` 反向 import `sidebar.slint`、`sidebar.slint` import `item.slint`，两条边不冲突、无环。
- 提取顺序由 800 行上限倒推：先搬 `SidebarItem` 会让外壳停在 847 行、校验脚本直接拒绝写盘，所以先搬最大的内聚块（收藏树），外壳 1 113 → 651 → 546 → 287 行。

**Docs**

- `docs/split-plan.md` — §16 阶段 5 状态补"侧栏按组件拆分已完成"，偏差补 24–27（实际形状、节头留存与依赖方向、顺序倒推与两层校验、`main_window.slint` 的两行变化），剩余项新增侧栏一条并从"仍超 800 行"里移除。
- `docs/code-map.md` — §1 概览（`ui/` 33 个 `.slint`、侧栏外壳 + 3 组件）、§2 结构树新增 `ui/sidebar/` 一行、§3.3 文件表补 4 行、§4.4 依赖链改写为侧栏一族并新增 `sidebar/*.slint` 条目、§6 从"仍超 800 行"表里移除 `ui/sidebar.slint`。

**Notes**

- 验证：与面板轮同一套写盘前断言（成员集合、子文件自包含、大括号平衡、无死 import、无重复空行、单文件 ≤ 800 行；`@image-url` 只允许多下沉一级，本轮共 16 句），另加一次**整体 round-trip**——把三个子文件按原声明顺序缝回外壳，去掉空行 / import / 注释、`../../../assets/` 还原为 `../../assets/`，与 `21ba83e~1` 的原文逐行比对：**849 行代码完全一致**。每个提交后 `cargo check -p favnyr-gui` 全绿。
- 收尾：`cargo test -p favnyr-core -p favnyr-gui` → core 205 passed / 1 ignored、gui 138 passed、0 failed；`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check` 干净。冒烟：`cargo build --bin favnyr` 后冷启动 debug 可执行文件，日志到 `starting Slint event loop init_ms=123`（工作区恢复 panels=1），15 秒内无 panic、无 Slint 加载错误。
- 尚未人工点检（本轮新增项加粗）：导航、标签页 tear-off、拖放（含虚拟文件）、粘贴进度、缩略图、设置各页、快捷键（capture 与冲突）、列表区选择、gutter 条橡皮筋与边缘自动滚动、列头排序 / 重排 / 改宽、两条自绘滚动条，**Places 行的 hover / 拖拽 / 盘符容量条**、**收藏树的展开折叠与重排拖拽**、**activity rail 三个按钮（侧栏开关 / 工作区 / 设置）与 tooltip**、**节的拖拽重排**。
- 上一轮记录"本地 `main` 领先 `origin/main`"已失效：面板轮的文档提交已推送（`6fd6853`），本轮的三个代码提交随文档一并推送。

## 2026-09-28 — 行形状拆分：`widgets/rows.slint` 922 → 596 + `widgets/rows/` 2 个组件

**Changed**

仍是纯搬迁拆分：不改行为、不动契约。`FileRowView` / `SectionHeaderView` / `FileTileView` / `ColumnHeader` / `FolderSwatchRow` / `MarkBox` 的对外成员面逐字未变，`MainWindow` 的 518 个成员与 `FileKind` 的 `#[repr(u8)]` 取值均未触碰。四个提交：

| 提交 | 变化 | 内容 |
| --- | ---: | --- |
| `619fae1` | `widgets/rows/header.slint`（223） | `ColumnHeader`：一格列头——排序点击、重排拖拽、右边缘改宽把手、右键上报绝对坐标 |
| `b2caa9f` | 纯注释 | `ModalBackdrop` 的区块注释归位到声明它的 `widgets/tabs.slint`（上轮提取时漏在 `rows.slint`） |
| `cbd8f8f` | `widgets/rows/marks.slint`（114） | `FolderSwatchRow`（"颜色与备注"飞出里的 8 色条）+ `MarkBox`（清理清单的红勾） |
| `4e1a3bf` | 纯空白 | 去掉提取留下的文件末尾空行与注释和声明之间的空行（`rows.slint` −2、`tabs.slint` −1） |

`widgets/rows.slint` **922 → 596 行**：只剩 `FileRowView` / `SectionHeaderView` / `FileTileView` 三个纯视觉形状。消费方只改 import：`panel/list.slint` 的一行拆成两行（761 → 762），`overlays/menus.slint` 与 `overlays/notes.slint` 各一行指向 `rows/marks.slint`（行数不变）。

**与方案的两点不同**（详见 `docs/split-plan.md` §16 偏差 28–30）：

- 分块判据取"形状是否自带指针"，而非名字里有没有 row：三个行形状的点击 / 拖拽 / hover 全由 `panel/selection.slint` 那个稳定的 `sel-touch` 统一路由（行能虚拟化的前提），且三者同读一份 `ColumnInfo` 列模型、行与瓦片还共用 `row-h` 与 `Tokens.row-*` 口径，留在同一文件；自带 TouchArea 的 `ColumnHeader` 与本就服务菜单的 `FolderSwatchRow` / `MarkBox` 各自搬出。
- 依赖方向仍是 `widgets/rows.slint` ← `widgets/rows/{header,marks}.slint`，两个子文件只 import `theme`，是 `widgets/*` 的叶子，无环；`@image-url` 在子目录下沉一级共 4 句（caret-up / caret-down / check ×2）。

**Docs**

- `docs/split-plan.md` — §16 阶段 5 补"行形状按组件拆分已完成"，偏差补 28–30（形状取舍、指针判据与依赖方向、消费方三行 import 与整体 round-trip），剩余项新增行形状一条并把 `widgets/rows.slint` 从"仍超 800 行"里移除；验证一句改为当前口径（fmt --all、clippy --workspace、core 205 + gui 138）。
- `docs/code-map.md` — §1 概览（`ui/` 35 个 `.slint`、`.slint` 侧只剩 `main_window.slint` 超 800）、§2 结构树新增 `ui/widgets/rows/`、§3.3 文件表补 3 行并订正 `tabs.slint`(792) / `panel/list.slint`(762)、§4.4 依赖链与新增 `widgets/rows*` 条目、§6 从"仍超 800 行"表里移除 `ui/widgets/rows.slint`。

**Notes**

- 验证：与前几轮同一套写盘前断言（成员集合、子文件自包含、大括号平衡、无死 import、无重复空行、单文件 ≤ 800 行，`@image-url` 只允许多下沉一级），收尾**整体 round-trip**——三个文件按原声明顺序缝回、去掉空行 / import / 注释、`../../../../assets/` 还原为 `../../../assets/`，与 `6ead5e3` 的原文逐行比对：**682 行代码完全一致**。
- 收尾门禁：`cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 干净；`cargo test -p favnyr-core -p favnyr-gui` → core 205 passed / 1 ignored、gui 138 passed、0 failed。冒烟：`cargo build --bin favnyr` 后冷启动 debug 可执行文件，日志到 `starting Slint event loop init_ms=241`（工作区恢复 panels=1），15 秒内无 panic、无 Slint 加载错误。
- 至此 `.slint` 侧除 `main_window.slint`（契约面）之外没有文件超过 800 行；剩余候选回到 Rust 侧：`bridge/state.rs`（1 508）、`bridge/tests.rs`（2 346）。
- 尚未人工点检（本轮新增项加粗）：导航、标签页 tear-off、拖放（含虚拟文件）、粘贴进度、缩略图、设置各页、快捷键、列表区选择、gutter 条橡皮筋与边缘自动滚动、两条自绘滚动条、Places 行、收藏树、activity rail、节重排，**列头的排序 / 拖拽改序 / 右缘改宽 / 右键菜单**、**列表行与分组头的渲染（缩略图、应用图标、链接徽标、age 药丸）**、**网格瓦片**、**"颜色与备注"飞出里的色条与清理清单的红勾**（后两组冷启动冒烟覆盖不到）。
- 本轮的四个代码提交与文档提交均未推送，`main` 领先 `origin/main`。
