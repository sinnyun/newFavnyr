# 超 800 行文件拆分方案（只做方案，不动代码）

本文件给出 13 个超 800 行自有文件的拆分方案：现状结构、切分判据、目标文件树、迁移顺序与风险。
正文中文，文件路径 / 类型 / 函数名保留英文原文。

- 统计日期：2026-09-28（基于当时 HEAD），行号均为**当前文件内行号**，迁移时以函数名为准
- 统计口径与文件清单见 [code-map.md](code-map.md)
- 本文是拆分施工的依据；**执行进度与实际偏差见 §16**（本文其余部分保持方案原样，不追改）
- `vendor/parley/` 内另有 5 个超 800 行文件（`editing/editor.rs` 1 286、`layout/line_break.rs` 1 166、`tests/test_analysis.rs` 1 158、`layout/data.rs` 952、`bidi.rs` 898），属第三方源码，不在拆分范围

## 0. 切分判据与统一约定

**为什么要拆**：`bridge.rs`（19 001 行）与 `main_window.slint`（15 897 行）合计占自有代码 56%，单文件已无法整体阅读；改动一处需在万行文件里定位与滚动，review 与并发改动成本都高。

**怎么切**：沿**已有的内聚边界**切，不重新设计架构。

- 文件内在 `// ---------- Section ----------` 分隔注释处已有天然分组，优先按它切。
- 平台分支（`#[cfg(windows)]` / `cfg(not(windows))`、`mod imp`）切成独立文件，符合 AGENTS.md 的 `win*.rs` 约定。
- 测试模块（`#[cfg(test)]`）一律移到同目录的 `tests.rs`（或 `tests/` 目录），用 `#[cfg(test)] mod tests;` 挂载。**这一步单独做、先做**：它零风险地把大文件缩小 10%–45%。
- 目标文件大小参考区间 **250–800 行**；超过 1 000 行的再按子主题拆一层目录。
- 语法机制：
  - Rust 子模块：`bridge.rs` → `bridge/mod.rs` + `bridge/*.rs`（`mod.rs` 用 `pub(crate) use` 重导出，**外部路径 `crate::bridge::…` 保持不变**）。
  - 跨文件的私有项：需要提升为 `pub(super)` 或 `pub(crate)`；只增可见性，不改签名。
  - Slint：`component X` → `export component X`，`global` → `export global`，其它文件 `import { X } from "…";`。
- **不动的东西**：`.slint` 名称面（`MainWindow` 的 518 个属性/回调）、`FileKind` 数值、i18n 键名、配置键名、`vendor/parley`。契约清单见 [code-map.md §5](code-map.md#5-跨文件契约拆分时必须保持)。

**统一验证**（每完成一个文件或一个阶段跑一次）：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings   # CI 口径：警告即错误
cargo test -p favnyr-core -p favnyr-gui
```

GUI 行为不在单测覆盖内，涉及 `bridge`/`.slint` 的阶段必须人工点检：导航、标签页撕离与跨窗口拖放、文件拖放（含虚拟文件）、粘贴/复制进度、缩略图、设置面板、工作区切换、快捷键。

## 1. `crates/favnyr-gui/src/bridge.rs` — 19 001 行（测试 2 352）

**现状**：单一文件承载全部桥接逻辑。内部已有 15 处 `// ---------- Section ----------` 分隔注释，但**分布不均**：`pub fn install()` 从 1 691 行到 7 081 行，**一个函数 5 391 行、含 201 处回调与闭包安装**；其余约 9 000 行为可独立成模块的纯函数。测试区 16 650–19 001。

**拆分策略：先外围、后核心。** 外围函数彼此依赖少（多数只依赖 `AppState` 与 `MainWindow`），可机械搬移；`install()` 是唯一的高风险点，放最后按"一个特性一节"逐块抽函数。

**目标文件树**（`crates/favnyr-gui/src/bridge/`）：

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `bridge/mod.rs` | 模块声明、`use`、`defer`、`MAX_PANELS`、拆分后的 `install()` 编排骨架、对外的 `pub(crate) use` | ~250 |
| `bridge/state.rs` | 61–180 剪贴板与拖拽暂存；181–249 `NavHistory`/`SortState`；249–580 `Tab`/`TabBook`/`ViewMode`；580–734 `Panel` 与行模型工具；734–911 后台作业类型、`OpRegistry`；1 049–1 691 `ImgMeta`/`AppState` 及其 `impl` | ~1 650 |
| `bridge/columns.rs` | 911–1 049 列宽计算、列重排、`ColumnInfo` 推送 | ~140 |
| `bridge/install/` | 1 691–7 081 的 `install()`，拆成 `mod.rs` + `nav.rs`/`tabs.rs`/`rows.rs`/`sidebar.rs`/`openwith.rs`/`settings.rs`/`ops.rs`/`dnd.rs`/`menu.rs`/`shortcuts.rs`（见下方"install 拆法"） | 5 400 → 10 个文件 |
| `bridge/workspace.rs` | 7 088–7 296 具名工作区的签名/脏检查/加载/重置/列表 UI | ~210 |
| `bridge/notices.rs` | 7 296–7 504 `NoticeKind`、toast 入口、锁定/跳过/改名失败等文案、`report_rename_failure` | ~210 |
| `bridge/settings.rs` | 7 505–7 580 UI 缩放预设与应用、ffmpeg 信息展示 | ~80 |
| `bridge/drives.rs` | 7 582–7 949 `EjectOp`/`spawn_eject`、`DriveSpaceUi`、`refresh_sidebar`、`push_sidebar_sections_ui`、便携设备、盘符签名 | ~370 |
| `bridge/datafiles.rs` | 7 949–8 090 批注/收藏/打开方式的 stamp、同步、保存；`flat_to_favnode` | ~150 |
| `bridge/openwith/` | 8 081–9 330：`picker.rs`（枚举与过滤、图标）、`shellmenu.rs`（shell 菜单与扩展扫描）、`recipes.rs`（`Recipe`/`RECIPES`/参数解析）、`launch.rs`（`Launch`/`plan_open`/默认打开/图像画廊） | ~1 250 |
| `bridge/favorites.rs` | 9 321–9 615 收藏面板、路径解析、打开、拖拽落点与排序 | ~300 |
| `bridge/nav.rs` | 9 615–10 045 首次填充、刷新全部、`relist_panel`、面板可用性复查、`NavAction`/`install_nav_callback`、`load_directory` | ~430 |
| `bridge/tabs.rs` | 10 060–10 774 `take_view`、撕离（`tear_off_*`）、序列化、跨窗口转移、外部拖放、`split_with_*` | ~720 |
| `bridge/naming.rs` | 10 774–11 015 名称冲突、唯一目标规划、名字可用性、重命名状态、光标偏移 | ~240 |
| `bridge/paste.rs` | 11 015–11 232 `resolve_replace`、`advance_paste`、`begin_paste*`、`execute_paste` | ~220 |
| `bridge/ops.rs` | 11 232–11 860 进度常量、toast 栈、`OpRegistry` 驱动、`start_heavy_op`、`run_heavy` | ~630 |
| `bridge/listing.rs` | 11 857–12 351 网络路径提示、异步/同步列目录刷新、异步结果落地、孤儿行 | ~500 |
| `bridge/geometry.rs` | 12 351–12 741 布局几何、splitter 视图、均衡与撤销、`update_panels_ui`、窗口标题 | ~390 |
| `bridge/shortcuts.rs` | 12 741–12 971 快捷键分组/显示/冲突、菜单快捷键、覆盖写回、页脚 | ~230 |
| `bridge/thumbs.rs` | 12 971–13 828 `ThumbScheduler`/`ThumbLru`/工作线程、请求与失效、`thumbnail_kind_for_row`、渲染窗口 | ~860 |
| `bridge/stats.rs` | 13 828–14 107 递归 mtime worker、图片元数据 worker | ~280 |
| `bridge/rows/` | 14 101–15 700：`mod.rs`（缩放常量、行高、`RowStyle`、分区构建、行模型重建）、`grid.rs`（网格度量、虚拟化渲染窗口、缩放锚点）、`icons.rs`（扩展名/路径/`.lnk` 图标解析、过滤器） | ~1 600 |
| `bridge/selection.rs` | 15 670–16 095 选中集合操作、焦点与打字定位、名称过滤、面板取行、剪切标记 | ~430 |
| `bridge/language.rs` | 16 095–16 130 `apply_language` | ~40 |
| `bridge/restore.rs` | 16 120–16 210 `home_dir`、`build_panels`、`resolve_restored_path` | ~90 |
| `bridge/tabstrip.rs` | 16 210–16 490 面包屑、标签宽度与滚动、竖排标签条几何、`tab_title` | ~280 |
| `bridge/watcher.rs` | 16 484–16 573 `notify` 监听与防抖 | ~90 |
| `bridge/window.rs` | 16 573–16 650 窗口尺寸持久化 | ~80 |
| `bridge/tests.rs` | 16 650–19 001 现有测试，可后续按模块再分 `tests/` | 2 352 |

**`install()`（1 691–7 081）拆法**——三步走，每步独立提交：

1. **原地抽函数**：在 `bridge.rs` 内，把每个 `window.on_xxx(...)` 块与相邻的 `state`/`window` 闭包抽成 `fn install_xxx(window: &MainWindow, state: &AppState)`。闭包改函数只需把捕获变量（`state`/`weak`/常量）改为显式参数，语义不变。
2. **按簇分文件**：把 `install_*` 按上表归入 `bridge/install/*.rs`，`install()` 只剩有序调用（建议保持调用顺序不变，避免改变初始化时序）。
3. **复查重入**：`defer()`（0 ms 单次定时器）用于避免从模型回调里改模型触发 "Recursion detected"；搬运时不得把 `defer` 包着的调用改成直接调用。

**风险与注意**：

- 闭包捕获 `Rc`/`Weak` 的克隆必须原样保留（`AppState` 是 `Rc<RefCell>`，非 `Send`；不要顺手改用 `invoke_from_event_loop`）。
- 私有项的可见性提升是必然成本（`pub(super)`），但**不要**把内部函数提升为 `pub` 以免扩大 `crate::bridge` 的对外面。
- `bridge/mod.rs` 需保留 `install`、`persist_window_size`、`suppress_workspace_persist`、`show_workspace_not_found_notice` 四个对外符号（`main.rs` 使用）。
- 建议顺序：先搬 `tests.rs`（减 2 352 行），再搬无相互依赖的外围模块（thumbs/stats/geometry/selection/watcher/window/settings/notices），最后才是 `install`。

## 2. `crates/favnyr-gui/src/ui/main_window.slint` — 15 897 行

**现状**：单文件包含 21 个 `export struct`、7 个 `global`、46 个 `component`、`MainWindow`（8 451–15 897，7 446 行）与 41 个覆盖层区块。文件内在 759、3 720、8 346 行有粗分隔注释；`MainWindow` 内部靠 `// ===== 名称 =====` 注释分块（共 40 余处，已按块测出行区间，见下表）。这正是可用的切分边界。

**为什么必须拆**：`build.rs` 已在 64 MB 栈线程里编译它（Windows 主线程栈约 1 MB，之前会 `STATUS_STACK_OVERFLOW`）；文件继续增长会让编译期与可导航性继续恶化。

**目标文件树**（`crates/favnyr-gui/src/ui/`，`main_window.slint` 仍是编译入口）：

| 新文件 | 内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `ui/structs.slint` | 20–757 全部 `export struct`（`FileRow`、`PanelView`、`Strings` 等） | ~740 |
| `ui/theme.slint` | 759–1 186 调色板注释 + `Theme`/`Tokens`/`Note`/`Tip`/`CtxNav`/`Dismiss`/`WindowFocus`（改成 `export global`） | ~430 |
| `ui/widgets/rows.slint` | 1 303 `ColumnHeader`、1 520 `FileRowView`、1 882 `SectionHeaderView`、1 953 `FileTileView`、2 299 `FolderSwatchRow`、2 736 `MarkBox` | ~1 050 |
| `ui/widgets/inputs.slint` | 2 120–2 232 输入类、2 786 `FieldLink`、2 823 `FilterBar`、2 919 `HintEdit`、3 068 `SettingsRowButton`、3 107 `PathRow` | ~560 |
| `ui/widgets/menus.slint` | 2 355–2 736 `MenuItem`/`SubmenuRow`/`CommandIcon`/`IconMenuRow`/`ShellMenuRow`/`OwRecipeButton`、2 956–3 068 `SplitMenuItem`/`TabBarMenuItem` | ~760 |
| `ui/widgets/tabs.slint` | 1 188–1 303 `NavIconButton`/`TabScrollButton`、2 212–2 355 计时器/边滚动、2 764 `ModalBackdrop`、3 152 `TabItem`、3 375 `VTabBar`、3 656 `PanelSplitterAbs` | ~830 |
| `ui/panel.slint` | 3 722–6 611 `PanelComponent`（2 889 行）→ 建议再拆 `panel/{header,columns,rows,footer}.slint` | 2 889 |
| `ui/workspaces.slint` | 6 611–7 020 `WsButton`/`WsIconBtn`/`WorkspaceRow` | ~410 |
| `ui/progress.slint` | 7 020–7 252 `ProgressToast`/`DriveGauge`、8 351–8 451 `FfmpegCmdRow` | ~330 |
| `ui/sidebar.slint` | 7 252–8 351 `SidebarItem`…`FavPanel`/`ActivityRail` | ~1 100 |
| `ui/overlays/menus.slint` | 11 004–11 480（上下文/Split/标签栏/视图/URL/回收站/盘符菜单）、11 943–12 019（标签栏死区与标签菜单）、12 246–12 445（列菜单）、13 839–13 867（快捷键菜单） | ~700 |
| `ui/overlays/openwith.slint` | 11 481–11 865 打开方式/新建子菜单、shell 级联、应用选择器、提升 toast | ~390 |
| `ui/overlays/favorites.slint` | 11 866–12 245 收藏容器与收藏项、命名弹窗、保存为收藏、新建容器（含 12 084–12 245 完整流程） | ~380 |
| `ui/overlays/settings.slint` | 12 478–13 838 设置面板与检测到的 shell 条目；建议把 9 353–9 865 的设置声明区一并搬入 | ~1 360 |
| `ui/overlays/workspaces.slint` | 13 867–14 211 工作区覆盖层（保存区、列表、反馈 toast） | ~350 |
| `ui/overlays/notes.slint` | 14 212–14 578 重命名 / 备注编辑器 / 新建文件夹文件弹窗、14 579–14 804 颜色与备注清理列表、15 834–15 875 条目备注 | ~700 |
| `ui/overlays/dialogs.slint` | 14 805–15 123 永久删除、打开全部、未保存更改、粘贴冲突 | ~320 |
| `ui/overlays/toasts.slint` | 15 124–15 463 进度 toast、拖放指示、swap pill、全局通知 | ~340 |
| `ui/overlays/custom_command.slint` | 15 464–15 833 自定义命令弹窗 | ~370 |
| `ui/main_window.slint` | 头部注释、`MainWindow` 的 API 声明（8 466–10 044 主体）、`FocusScope`、布局骨架、侧栏与面板容器实例化（10 263–11 003）、全局提示（15 876–15 897） | ~2 500 |

**迁移顺序**（由易到难，每步可独立验证）：

1. `ui/structs.slint` + `ui/theme.slint`：**零风险**——只搬定义并加 `export`，其它文件 `import` 即可，不改任何绑定。
2. `ui/widgets/*.slint`：组件原本已通过 `in property <Strings> strings` 等参数解耦，搬移只需补 `export component`。
3. `ui/overlays/*.slint`：每个覆盖层只依赖自身局部 state + `strings` + `Theme`，但**引用了大量 `root.xxx`**（`MainWindow` 的属性与回调）。提取时需：
   - 把用到的 `root.xxx` 改成组件自己的 `in property` / `in-out property`；
   - 把 `root.action-*()` 这类回调改成组件 `callback`，由 `MainWindow` 侧转发。
   建议一个overlay 一次提交，先做纯展示型（toasts、dialogs），最后做设置面板与上下文菜单（引用面最大）。
4. `ui/panel.slint`（`PanelComponent`）：先整体搬出，再按内部块（标签栏 / 路径与列头 / 行区 / 页脚）拆第二层。
5. `ui/main_window.slint` 收尾：只留 API 契约、布局骨架与实例化。

**风险与注意**：

- `MainWindow` 的 518 个成员名与类型是 Rust 侧 `install()` 的绑定目标，**一个都不能改名**；这是硬约束，不是风格问题。
- 相对资源路径：`@image-url("../../assets/…")` 是相对**当前文件**解析的。文件下沉到 `ui/overlays/` 后要改成 `../../../assets/…`，这是最容易漏的机械错误（建议拆分后用 `grep -n "@image-url"` 全量核对一遍）。
- `Palette.color-scheme` 由 `apply-theme()` 单一写入；拆文件后仍只能有一个写入点，避免主题漂移。
- 提取组件后 `MainWindow` 的绑定表达式会变长（转发层），这是可接受代价；不要用 `@children` 之类的技巧去绕，保持显式转发可读性最好。
- 完成后 `build.rs` 的 64 MB 栈线程可保留（无害）；是否撤销留待拆分落地后再评估。

## 3. `crates/favnyr-core/src/fs.rs` — 1 919 行（测试 811）

**现状**：四个主题挤在一起——列目录（含 UNC 与共享枚举）、地址栏输入解析、排序与分组、格式化（大小/时间/年龄）。测试区 1 109–1 919，占 42%。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `fs/mod.rs` | 1–257 文档、`FileKind` + 扩展名白名单 + `Entry` + `classify_kind` + 隐藏属性；413–569 `list_dir`/`list_dir_counted`/UNC 与共享枚举；`mod` 声明与 re-export | ~415 |
| `fs/typed_path.rs` | 257–413 地址栏输入解析（`expand_typed_path`、`strip_home_prefix`、`expand_variables` 两平台各一份） | ~156 |
| `fs/sort.rs` | 569–804 `SortColumn`/`SortOrder`/`Category`/`GroupMode` 与 `sort()` | ~235 |
| `fs/format.rs` | 804–924 + 1 016–1 108 大小/时间/年龄格式化、`civil_from_days`、`free_space_level` | ~215 |
| `fs/stats.rs` | 924–1 016 `recursive_folder_stats`、`recursive_max_mtime`（目录遍历，与格式化无关，独立成文件更清晰） | ~95 |
| `fs/tests.rs` | 1 109–1 919 现有测试（可按目标模块再分） | 811 |

**注意**：`lib.rs` 的 re-export（`Entry`/`FileKind`/`SortColumn`/`SortOrder`）与 `fs::ops` 路径保持不变；`sort()` 与 `Category` 在 `flatten` 时用 `pub use` 从 `fs/mod.rs` 导出，避免调用方（`bridge.rs`、`workspace.rs`）改路径。

## 4. `crates/favnyr-core/src/fs/ops.rs` — 1 834 行（测试 599）

**现状**：已是子模块，但内含改名、名字校验、唯一名生成、复制/移动、链接、删除/回收站、路径关系六块，测试区 1 237–1 834。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `fs/ops/mod.rs` | 文档 + `OpStatus` + `mod` 声明 + re-export | ~40 |
| `fs/ops/rename.rs` | 32–96 `rename_in_place`、`rename_in_place_replacing_file`、`is_valid_entry_name` | ~65 |
| `fs/ops/name.rs` | 96–339 `NameRejection`、禁用字符、保留设备名、`check_file_name`、`create_entry`、`unique_sibling*`、`split_name`、`ext_of` | ~245 |
| `fs/ops/copy.rs` | 339–791 + 958–1 021 `copy_path*`（环检测）、`path_size`、`copy_tree_progress`、`copy_file_progress`、`copy_into`、`move_into`、`move_to`、`duplicate` | ~520 |
| `fs/ops/link.rs` | 791–958 `link_into`、`link_as`、`create_symlink_win`、`windows_junction`、`windows_link` | ~170 |
| `fs/ops/trash.rs` | 1 021–1 156 + 1 211–1 236 `TrashDisposition`、`permanent_delete`、`trash*`、`TrashError`、`restore_from_trash`、`empty_trash` | ~170 |
| `fs/ops/path_eq.rs` | 1 156–1 211 `resolve_parent_symlinks`、`paths_equal`、`is_within` | ~55 |
| `fs/ops/tests.rs` | 1 237–1 834 | 599 |

**注意**：`error.rs` 的 `Error::Workspace` 用法与错误文案不变；`_with_progress` 系列的回调签名不变。

## 5. `crates/favnyr-core/src/places.rs` — 1 771 行（测试 306）

**现状**：跨平台位置 + 大段 Linux 专属探测（`lsblk`/udev/gvfs）+ 嵌套 `mod windrives`（473–1 034，约 560 行，Windows 盘符枚举）。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `places/mod.rs` | 1–473 文档、`PlaceKind`/`Place`、`user_places`、`drives`/`drives_signature`、网络共享与连接、热插拔、`block_signature`、`unmounted_volumes`、`network_extra` | ~470 |
| `places/windrives.rs` | 473–1 034 现有 `mod windrives` 原样上提为文件（`#[cfg(windows)]`） | ~560 |
| `places/linux.rs` | 1 034–1 466 `trash_place`、标签与 udev 工具函数、gvfs 挂载、`block_inventory`/`parse_block_rows`/`volume_kind`/`volume_place`、`is_user_facing_mount`（`#[cfg(target_os = "linux")]`） | ~430 |
| `places/tests.rs` | 1 466–1 771 | 306 |

**注意**：`PlaceKind` 值被 GUI 按 `kind` 解释（`SidebarPlace.kind`），枚举顺序与含义不得改动；两平台都要能编译，Linux 专属函数需 `#[cfg]` 门控或保留空实现。

## 6. `crates/favnyr-core/src/thumbnail.rs` — 1 733 行（测试 566）

**现状**：按格式分成多段，段与段之间只通过 `generate()` 与两个解码工具函数耦合。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `thumbnail/mod.rs` | 1–70 文档与常量、150–231 `Thumbnail`/`generate`/`image_meta`/`from_encoded`/`from_image`、1 159–1 169 `downscale`、`mod` 声明 | ~250 |
| `thumbnail/decode.rs` | 75–150 `decode_limits`/`decode_bounded`/`run_bounded`/`no_console`（被图片、PSD、Affinity、音频共用） | ~85 |
| `thumbnail/psd.rs` | 231–369 PSD 内嵌 JPEG 提取 | ~140 |
| `thumbnail/affinity.rs` | 369–546 Affinity 内嵌 PNG 扫描 | ~180 |
| `thumbnail/audio.rs` | 546–905 ID3v2 与 FLAC 封面（含 synchsafe、unsynchronization、帧头解析） | ~360 |
| `thumbnail/video.rs` | 905–1 110 ffmpeg 抽帧、MP4 时长解析、`ffprobe` 兜底 | ~205 |
| `thumbnail/pdf.rs` | 1 110–1 159 `pdftoppm`/`pdftocairo` | ~50 |
| `thumbnail/tests.rs` | 1 169–1 733（含 `bounds_tests`） | 566 |

**注意**：本模块是**纯函数**（路径进 → 像素出），无状态，拆分风险最低；`generate` 的分派逻辑与两个资源上限常量保持不变。

## 7. `crates/favnyr-gui/src/actions.rs` — 1 563 行（测试 210）

**现状**：同一文件里用 `#[cfg(windows)]` / `#[cfg(not(windows))]` 交错提供两套实现（`open_path`、`show_native_properties`、`local_utc_offset_secs` 等各两份），另有 ffmpeg 环境探测与测试区（1 354–1 563）。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `actions/mod.rs` | 1–25 文档、48–78 `with_clipboard`/`copy_to_clipboard`、1 071–1 156 通用 helpers（`pick_terminal`/`which`/`resolve_program`/`program_is_valid`）、`mod` 声明与 re-export | ~200 |
| `actions/spawn.rs` | 433–682 `run_opener*`、`spawn_program*`、`spawn_new_instance`、`spawn_detached*`、`run_with_files` | ~250 |
| `actions/windows.rs` | Windows 专属：25–47 常量、78–347 `shell_execute*`/`default_handler_app_id`/照片图库/可执行判定、392–433 `open_parent`·改、682–790 `open_elevated`/`shell_execute_runas`/`win_append_arg`、790–811 `open_with`、811–881 属性、996–1 071 终端、1 276–1 323 时区 | ~700 |
| `actions/linux.rs` | Linux 专属：295–392 `open_path`/可执行判定、881–996 D-Bus 与 `FileManager1` 属性、996–1 071 终端、1 323–1 355 时区 | ~330 |
| `actions/ffmpeg.rs` | 1 156–1 276 `FfmpegInfo`、`ffmpeg_available`/`ffmpeg_info`/`ffmpeg_version`、`in_flatpak`、发行版索引 | ~120 |
| `actions/tests.rs` | 1 354–1 563 四组测试 | 210 |

**注意**：`mod.rs` 保持对外符号（`open_path`、`open_terminal`、`run_opener`、`spawn_new_instance`、`ffmpeg_info`、`local_utc_offset_secs` 等）不变，`bridge.rs` 无需改动调用路径。平台文件整体 `#[cfg]` 包裹，保证两平台都能编译。

## 8. `crates/favnyr-gui/src/openwith.rs` — 1 386 行（测试 264）

**现状**：已经用 `mod imp` 分成 Windows（37–444）与 Linux（447–1 048）两套实现，中性 API 在 1 049–1 123，测试 1 123–1 386。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | --- |
| `openwith/mod.rs` | 1–35 文档、22–35 `AppHandler`、1 049–1 123 中性入口（转发到 `imp`） | ~145 |
| `openwith/windows.rs` | 37–444 `SHAssocEnumHandlers`/`IAssocHandler` 实现 | ~410 |
| `openwith/linux.rs` | 447–1 048 XDG `.desktop` 解析 | ~600 |
| `openwith/tests.rs` | 1 123–1 386 | 264 |

**这是 13 个文件里最机械的一次拆分**：`imp` 模块边界已存在，只需把两段 `mod imp { … }` 提为带 `#[cfg]` 的文件，并在 `mod.rs` 里 `#[cfg(windows)] use windows as imp;` / `#[cfg(not(windows))] use linux as imp;`。

## 9. `crates/favnyr-core/src/openers.rs` — 1 268 行（测试 555）

**现状**：模型 + 标签替换 + 存储三段，测试区 714–1 268 占 44%。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `openers/mod.rs` | 1–141 文档、`OpenerIcon`、`Opener`、上下文位与常量、`mod` 声明与 re-export | ~180 |
| `openers/tag.rs` | 141–296 `TAGS`/`LIST_TAG`/`LIST_NAMES_TAG`、`TagContext`、`file_uri`、`substitute` | ~155 |
| `openers/model.rs` | 296–403 `impl Opener`（校验、`matches`、上下文判定、`generate_id` 与计数器） | ~110 |
| `openers/store.rs` | 403–714 `OpenerStore` 的增删改查与 `openers.toml` 读写 | ~310 |
| `openers/tests.rs` | 714–1 268 | 555 |

**注意**：`substitute` 的安全语义（参数列表化、按 token 替换、路径永不分词）是安全边界，拆分时**只搬不改**；`lib.rs` 的 re-export 保持。

## 10. `crates/favnyr-core/src/layout.rs` — 1 140 行（测试 525）

**现状**：树结构定义 + 约 460 行的 `impl LayoutNode`（拆分/合并/调整比例/均衡/路径定位）+ `split_area` 几何 + 测试区 616–1 140。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `layout/mod.rs` | 1–114 文档、`SplitDir`/`LayoutNode`/`NodePath`/`Side`/`Rect`/`PanelGeom`/`SplitterGeom`/`Layout`、`RATIO_EPSILON`、`mod` 声明与 re-export | ~115 |
| `layout/tree.rs` | 114–574 `impl LayoutNode`（拆分、合并、比例调整、均衡、按路径寻址、扁平化入口） | ~460 |
| `layout/geom.rs` | 574–616 `split_area` 与纯几何 | ~45 |
| `layout/tests.rs` | 616–1 140 | 525 |

**注意**：`lib.rs` 的 re-export（`Layout`/`LayoutNode`/`PanelGeom`/`Rect`/`Side`/`SplitDir`/`SplitterGeom`）与 `bridge.rs` 的调用路径保持；树操作是不变量最密的代码（比例之和、最小比例、叶子下标），建议这一节**只搬移、不重构**，并靠现有 525 行测试兜底。

## 11. `crates/favnyr-gui/src/winddrag.rs` — 1 046 行（测试 112）

**现状**：仅 Windows 编译（`main.rs` 里 `#[cfg(windows)] mod winddrag;`）。含自建 `IDropTarget`、路径抓取、虚拟文件物化、拖出四块。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `winddrag/mod.rs` | 1–45 文档、`IncomingFileDrag`/`DropStaging`/`DropHandler`、378–465 `init_drop_target`/`rebind_drop_target`/分类入口、`mod` 声明与 re-export | ~150 |
| `winddrag/target.rs` | 45–263 `FavnyrDropTarget` 与 `IDropTarget_Impl` 实现 | ~220 |
| `winddrag/paths.rs` | 263–378 路径抓取与临时落盘（`capture_application_paths`、`create_drop_dir`、`capture_path_tree`）、532–590 `file_paths` | ~175 |
| `winddrag/virtual_files.rs` | 590–837 虚拟文件物化（文件描述符名、`IStream`/`HGLOBAL` 写出） | ~250 |
| `winddrag/drag_out.rs` | 837–935 `drag_files`/`drag_files_inner`（shell `IDataObject` + `SHDoDragDrop`） | ~100 |
| `winddrag/tests.rs` | 935–1 046 | 112 |

**注意**：临时目录的清理语义（`TransientDropGuard`/`DropStaging` 生命周期）跨文件后仍必须与拖放生命周期绑定，不能因为搬移而提前/延后释放；建议这一步逐函数核对 `Drop` 实现。

## 12. `crates/favnyr-core/src/workspace.rs` — 1 026 行（测试 547）

**现状**：会话状态模型（26–294）+ 具名工作区文件 CRUD（294–480）+ 测试区 480–1 026（占 53%）。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `workspace/mod.rs` | 1–26 文档、`MAX_CLOSED_TABS`、`mod` 声明与 re-export | ~40 |
| `workspace/state.rs` | 26–294 `TabState`/`PanelState`/`SidebarSectionsState`/`WorkspaceState`、默认值与 `impl WorkspaceState` | ~270 |
| `workspace/named.rs` | 294–480 `NamedWorkspaceFile`/`NamedWorkspaceMeta`、`id_is_safe`、`workspace_file_path`、`list`/`find`/`save`/`overwrite`/`load`/`rename`/`delete`、`generate_id`/`now_nanos` | ~190 |
| `workspace/tests.rs` | 480–1 026 | 547 |

**注意**：`TabState` 的向后兼容字段（`view_mode`/`subfolders`/`collapsed` 与旧的 `preview` 标志）在拆分中不得改动语义；`lib.rs` 的 re-export 保持。

## 13. `crates/favnyr-gui/src/i18n.rs` — 994 行（测试 254）

**现状**：目录装载（1–121）+ `strings_for` 构造 321 字段的 Slint `Strings`（121–451，约 330 行）+ 标签/单位/错误文案（451–741）+ 测试区 741–994。

| 新文件 | 迁移内容（现行号） | 预估行数 |
| --- | --- | ---: |
| `i18n/mod.rs` | 1–121 文档、`EMBED_*`、`embedded`、`flatten_into`、`overlay`、`build_catalog`、`catalog`、`tr`、`mod` 声明与 re-export | ~140 |
| `i18n/strings.rs` | 121–451 `strings_for`（译名取值的机械展开，单独成文件便于按键名对照） | ~330 |
| `i18n/labels.rs` | 451–539 `language_labels`/`theme_labels`/`tabbar_labels`/`footer_items_text`/`op_more_text`/`footer_text`/`shortcut_*` | ~90 |
| `i18n/units.rs` | 539–604 `size_units`/`age_units`（桥接 core 的 `SizeUnits`/`AgeUnits`） | ~65 |
| `i18n/messages.rs` | 604–741 `access_denied`、`process_list`、`item_in_use`、`item_skipped`、`move_source_kept`、`rename_failed`、`eject_error_message`、`mount_error_message`、`trash_error_message` | ~140 |
| `i18n/tests.rs` | 741–994 | 254 |

**注意**：级联回退（本语言 → 英文 → 键名）与用户覆盖目录的语义必须保持不变；`Strings` 字段名与 `.slint`/`bridge.rs` 的用法绑定，`strings.rs` 里只搬不改键名。

## 14. 分批执行顺序

阶段之间相互独立，可按需要重排；阶段内建议按列出的文件顺序。

| 阶段 | 内容 | 风险 | 说明 |
| --- | --- | --- | --- |
| 0 | 先单独把各文件测试区移到 `tests.rs`（13 个文件） | 极低 | 只搬代码，`#[cfg(test)] mod tests;` 挂载；立即缩小每文件 10%–45% |
| 1 | `favnyr-core` 六个模块：`workspace.rs` → `layout.rs` → `openers.rs` → `fs.rs` → `fs/ops.rs` → `places.rs` → `thumbnail.rs` | 低 | crate 对外 API 不变，`lib.rs` re-export 兜底；有单测保护 |
| 2 | GUI 外围：`i18n.rs` → `openwith.rs` → `actions.rs` → `winddrag.rs` | 中 | 平台分文件，需两平台编译验证；`bridge.rs` 调用路径不变 |
| 3 | `bridge.rs` 外围模块（state/columns → notices/settings → drives/datafiles → favorites → openwith/ → nav → tabs → naming/paste → ops → listing → geometry → shortcuts → thumbs → stats → rows/ → selection → language/restore/tabstrip → watcher/window → tests） | 中高 | 每次一个文件；`bridge/mod.rs` 用 re-export 保持 `crate::bridge::…` 路径不变 |
| 4 | `bridge.rs` 的 `install()` 拆分（先原地抽函数，再分文件） | 高 | 5 391 行、201 处回调；逐特性迁移，每步人工点检 |
| 5 | `.slint` 拆分：`structs`/`theme` → `widgets/*` → `overlays/*` → `panel` → `main_window` 收尾 | 中 → 高 | 名称面与资源路径是主要风险点 |

**建议的第一个提交**（如果只做一件事）：阶段 0。纯搬移、零行为变化、审查成本最低，且立刻让 13 个文件的可读性显著改善。

## 15. 通用注意事项（各阶段共用）

1. **一次只搬一个文件/一个模块**，`cargo test` 全绿再继续；不要在一次提交里同时改结构与改行为。
2. **可见性只升不降**：私有项搬出后需要 `pub(super)`/`pub(crate)`；不要把内部函数提升为 `pub`。
3. **`mod.rs` 做 re-export**（`pub(crate) use state::*;` 等），让 `crate::bridge::AppState`、`favnyr_core::fs::Entry` 这类既有路径继续可用——调用方零改动是这次拆分的主要验收标准。
4. **平台文件用 `#[cfg]` 整体门控**，文件名遵循 `win*.rs` 约定（AGENTS.md）；每次都要在两个平台（Windows 本机 + CI）编译。
5. **契约不动**：[code-map.md §5](code-map.md#5-跨文件契约拆分时必须保持) 列出的 `FileKind` 数值、Slint 名称面、i18n 键、配置键、`SidebarSection` 数值全部保持不变。
6. **CI 口径**：`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check`；CI 把警告当错误，拆分后常见的 `unused import`/`dead_code` 会直接失败，需在提交前清理。
7. **文档同步**：拆分完成后更新 `docs/CHANGELOG.md`（追加记录），并检查 `AGENTS.md` 中提到的文件路径（例如 "两个决策点都在 `crates/favnyr-gui/src/bridge.rs`" 若指向新文件需改写）。
8. **不做的事**：本次不引入新依赖、不新增功能、不调整 UI 行为、不修改 `vendor/parley`。

## 16. 执行状态与偏差记录（2026-09-28 拆分落地）

**进度**（方案原样保留；本节记录实际执行）：

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| 0 | 各文件测试区移到 `tests.rs` | ✅ 全部完成 |
| 1 | `favnyr-core` 七块：`fs`、`fs/ops`、`places`、`thumbnail`、`openers`、`layout`、`workspace` | ✅ 全部完成 |
| 2 | GUI 外围：`i18n`、`openwith`、`actions`、`winddrag` | ✅ 全部完成 |
| 3 | `bridge.rs` 外围模块（47 个文件） | ✅ 全部完成（`bridge/mod.rs` 151 行） |
| 4 | `bridge.rs` 的 `install()`（791 + 12 个 `cb_*.rs`） | ✅ 完成 |
| 5 | `.slint` 拆分 | ✅ `structs`/`theme`/`widgets/*`/`panel`/`sidebar`/`workspaces`/`progress` 已分文件；**`overlays/*` 10 个文件、11 个组件已完成**（`main_window.slint` 5 541 → 3 195 行），**设置对话框按页拆分已完成**（`overlays/settings.slint` 1 507 → 外壳 385 + `settings/{general,shortcuts,openwith}`），**`panel.slint` 第二层已完成**（2 897 → 732 行 + `panel/` 6 个文件），**`sidebar.slint` 按组件拆分已完成**（1 113 → 287 行 + `sidebar/{item,fav,rail}` 3 个文件），**`widgets/rows.slint` 按组件拆分已完成**（922 → 596 行 + `widgets/rows/{header,marks}` 2 个文件） |

**验证**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 干净；`cargo test -p favnyr-core -p favnyr-gui` 全绿（core 205 + gui 138，0 失败）；GUI 冷启动冒烟通过（恢复工作区 + 进入事件循环，无 panic）。覆盖层每一层搬完后都另有**行级校验**：搬走的区块按去缩进后与原文逐行比对（`block verbatim`），并核对组件成员与实例转发一一对应、无遗漏，再跑 `cargo check -p favnyr-gui`。**人工点检仍欠**：见 §2"迁移顺序"前的说明。

**实际偏差**（与方案表格不同之处，均为命名/组织选择，不影响验收标准）：

1. **子模块改名避开名字遮蔽**：`bridge/mod.rs` 已 `use` 了 `openwith`/`workspace`/`favorites`/`ops`/`shortcuts`/`columns` 等 crate 模块名，故 plan 中的 `bridge/openwith/`→`open_with/`、`workspace.rs`→`workspaces.rs`、`favorites.rs`→`favpanel.rs`、`ops.rs`→`progress.rs`、`shortcuts.rs`→`keys.rs`、`columns.rs`→`colinfo.rs`、`settings.rs`（install 内）→`cb_prefs.rs`。
2. `bridge/rows/` 实际为 `mod.rs` + `build.rs` + `icons.rs`（方案中的 `grid.rs` 内容并入 `mod.rs`/`build.rs`）。
3. 孙模块（`install/`、`open_with/`、`rows/` 的子项）可见性用 `pub(in crate::bridge)`，`bridge/mod.rs` 直接引用的用 `pub(super)`。
4. core 的删除模块命名为 `fs/ops/deletion.rs`（方案写 `trash.rs`）。
5. `openwith` 用 `#[path]` 把 `windows.rs`/`linux.rs` 挂成 `imp`，`mod.rs` 用 `#[cfg]` 选择；比方案设想的再包一层 `mod imp` 更少一层嵌套。
6. `actions/` 按**功能**分文件（`opening`/`spawn`/`shell`/`properties`/`terminal`/`program`/`ffmpeg`/`timezone`），而非方案的"整平台分文件"——各函数的平台差异原本就是就地 `#[cfg]`，按平台切需要重排大量代码，风险更高收益更低。
7. `winddrag/` 增加 `formats.rs`（剪贴板格式分类，方案并入 `paths.rs`）。
8. `install/` 用 `cb_*` 前缀命名 12 个回调簇文件（对应方案的 `nav.rs`/`tabs.rs`/… 十项归并），函数统一为 `pub(super) fn install_*`。
9. `state.rs` 与 `colinfo.rs` 对应方案的 `state.rs` + `columns.rs`（`colinfo` 命名理由见第 1 条）。
10. `.slint` 拆分需要一处新增：`main_window.slint` 里加 `export { CtxNav } from "theme.slint";`——只有根文档的导出会被 `include_modules!()` 再导出，Rust 侧 `crate::CtxNav` 依赖它。
11. 所有 `.slint` 文件统一为 CRLF（仓库 `autocrlf=true`），否则 `sed` 等工具会把单个文件改成 LF 造成混行。
12. GUI 拆分的 Linux 编译无法在本机（Windows）验证，交由 CI 的 Linux `check` 任务覆盖；本机已确认 `#[cfg]` 门控的原样保留。
13. `ui/overlays/*` 实际为 **10 个文件、11 个组件**：`toasts`(1)、`dialogs`(1)、`workspaces`(1)、`custom_command`(1)、`notes`(2：`OverlayNotes` + `OverlayNoteBubble`)、`favorites`(1)、`openwith`(1)、`panel_menus`(1)、`menus`(1)、`settings`(1)。每个覆盖层一次提交（`ad2ae31`…`c0a24cd`）。
14. 方案的 `overlays/menus.slint`（预估 ~700 行）实施为**两个文件**：`menus.slint`（641：主右键菜单、Split、标签栏位置、视图模式、URL、回收站、盘符/网络、"颜色与备注"飞出菜单）+ `panel_menus.slint`（363：标签栏死区、标签页、列、跨视图拖放菜单），以守住 800 行上限。
15. 快捷键菜单（方案归入 `menus.slint`）实际落在 `overlays/settings.slint`：它必须渲染在设置面板**之上**，因此与面板同文件并声明在其后（原始次序即如此），避免跨文件的 z 序耦合。
16. `overlays/favorites.slint` 吸收收藏容器/收藏项的右键菜单（方案列在 `menus.slint` 区块）——它与三个收藏弹窗共用 `fav-menu-*` 状态，合在一起内聚更高。
17. `notes.slint` / `panel_menus.slint` / `settings.slint` 新增组件内 `callback return-focus();`：原先直接调用窗口的 `key-scope.focus()`，覆盖层拿不到该 id，改由实例处接回。
18. 覆盖层提取后 `main_window.slint` 的 import 表按"只留仍在用的名字"重写（`FfmpegCmdRow`/`SidebarHeaderAction`/`MenuItem`/`ModalBackdrop`/`PathRow`/`SearchField` 等移出），并在 `overlays/*.slint` 里把 `@image-url` 下沉为 `../../../assets/…`。
19. 设置对话框按页拆为**子目录** `ui/overlays/settings/`（`general`/`shortcuts`/`openwith`），每页是 `export component SettingsX inherits ScrollView`，页面内容即原 `ScrollView` 的子女（`vertical-stretch: 1` 留在实例处）；外壳 `overlays/settings.slint` 只剩 385 行。每页一次提交（`7f8f6cc`、`a7cb679`、`1d5e3af`），搬走的区块仍按行级校验：去缩进后与原文逐行比对，唯一允许的改写是 `@image-url` 再深一级（`../../../assets/` → `../../../../assets/`，共 12 处），已用"无浅路径残留"断言覆盖。
20. 三页的成员方向取自脚本分类（`root.x` 被赋值或出现在 `<=>` 右侧 → `in-out`；被调用 → `callback`；其余 → `in`），再逐页核对"页面用到的 root 成员 = 声明成员 = 外壳转发"三者相等；外壳的 import 同时在最后一页提交里按"只留仍在用的名字"再剪一次（`PathRow`/`SearchField`/`SettingsRowButton`/`CommandIcon`/`OwRecipeButton`/`FfmpegCmdRow`/`SidebarHeaderAction`/`Button`/`CheckBox`/`ComboBox`/`HorizontalBox`/`ScrollView`/`SpinBox` 移出，余 7 行）。
21. `panel.slint` 的第二层按**现有子块的自然边界**落地为 6 个文件——`panel/tabs_bar.slint`(373)、`panel/nav_bar.slint`(563)、`panel/selection.slint`(751)、`panel/list.slint`(761)、`panel/scrollbars.slint`(139)、`panel/overlays.slint`(123)——而不是方案写的 `{header,columns,rows,footer}`：列头与行视图本就住在 `widgets/rows.slint`，页脚只有约 30 行，再切"列/页脚"只会把 `PanelList` 的内部状态多摊一层。`panel.slint` 2 897 → 732 行，一次一块 8 次提交（`6e1fe75`…`489ff74`）。
22. 方案点名的障碍（面板级成员**反向读写**区块内部 id）用**状态下沉**解决，而不是把 Flickable 视口用 `in-out` 别名接回面板：`sel-touch.*` 的读取（`hovered-idx`、`note-row`、`menu-key-watch`）随选择层一起沉进 `selection.slint`；`rows-scroll.viewport-*` 与橡皮筋状态沉进 `list.slint`（它的两个指针层——选择层与左侧 gutter 条——都在同一文件内，band 状态因此退回私有属性，5 个 `in-out` 转发消失），面板只读三个 `out`——`list.area-abs-x` / `list.area-abs-y` 换算 `body-top` / `list-inset-x`，`list.drag-hover-row` 供拖放高亮，并把 `gutter-w` 这一个 `in` 传下去；标签栏的 `tabs-flick` 视口数值在 `tabs_bar.slint` 内部自闭环，面板改为发 `scroll-tabs-*` 回调触发滚动。`FileKind` 的数值约定与 `MainWindow` 成员名一律未动。
23. 本轮搬走的每一行仍做行级校验（去缩进后与 `git show HEAD:` 的原文逐行比对），允许的改写只有三类：`@image-url` 多下沉一级、`root.<id>.<prop>` 换成契约属性名（如 `flickable.viewport-x` → `root.flick-vx`）、绝对坐标换算改读子组件的 `out`；校验脚本（成员集合相等、子文件自包含、大括号平衡、单组件、无重复空行、行数上限）任一不过即拒绝写文件。收尾另有三处**纯空白**修正（`9342964`、`1cebf6c`、`f0806dc`）：历次"子树换成子组件"都让被删 wrapper 的兄弟们保留了旧缩进，9 个 `.slint` 文件里整块内容比真实层级少 4–16 列，`selection.slint` 甚至有一行落在第 0 列；现按"每层 4 空格"统一，被折行的表达式保持手写偏移，`git diff -w` 为空、各文件行数不变。收尾还修掉一处提取遗留：`panel.slint` 的 nav bar 实例上方重复了一行区块注释（`99f3c8d`，733 → 732 行）。
24. `ui/sidebar.slint`（1 113 行，方案表格写"7 个组件同属侧栏一族；组件接口已显式，后续可机械分文件"）落地为**外壳 + 三个同级组件**：`sidebar/item.slint`(271，`SidebarItem`)、`sidebar/fav.slint`(467，`FavRow` + `FavPanel`)、`sidebar/rail.slint`(115，`RailBtn` + `ActivityRail`)，外壳 287 行只剩节头一族（`SidebarHeaderAction` / `SidebarHeaderActionSlot` / `SidebarSectionHeader`）与 `Sidebar`。一次一块 3 次提交（`21ba83e` 收藏树、`c268611` 活动栏、`c267f74` Places 行）。
25. 节头一族**留在 `sidebar.slint`**：`SidebarSectionHeader` 是四个可重排节共用的表头，`SidebarHeaderAction` 还被 `overlays/settings/openwith.slint` 引用（第 228–232 行的配方按钮），搬走会让"设置页"反向依赖"收藏页"。由此产生的方向是 `sidebar.slint` ← `sidebar/fav.slint`（fav import 节头）与 `sidebar/item.slint` ← `sidebar.slint`（外壳 import 行），两条边不冲突，无环。
26. 本轮的**提取顺序由 800 行上限倒推**：先搬 `SidebarItem`（271 行）会让剪完之后的外壳停在 847 行、仍越界（校验脚本直接拒绝写盘），因此改为先搬最大的内聚块——收藏树 `FavRow` + `FavPanel`，外壳 1 113 → 651 行，随后活动栏 → 546 行，最后 Places 行 → 287 行。校验仍分两层：每个子文件写盘前跑同一套断言（成员集合、自包含、大括号平衡、无重复空行、无死 import、行数上限，`@image-url` 只允许多下沉一级），收尾另做**整体 round-trip**——把三块按原声明顺序缝回外壳、去掉空行/import/注释、把 `../../../assets/` 还原为 `../../assets/`，与 `21ba83e~1` 的原文逐行比对，849 行代码完全一致。
27. `main_window.slint` 相应只有两行变化：`import { Sidebar } from "sidebar.slint";` 一行换成 `Sidebar` / `FavPanel` / `ActivityRail` 三行（3 193 → 3 195 行），518 个契约成员与实例处的转发均未改动。
28. `ui/widgets/rows.slint`（922 行、6 个组件，方案 §2 表格把它列在"`widgets/*` 纯展示组件"一族）落地为**三个行形状 + 两个子组件**：`widgets/rows/header.slint`(223，`ColumnHeader`)、`widgets/rows/marks.slint`(114，`FolderSwatchRow` + `MarkBox`)，父文件剩 596 行的 `FileRowView` / `SectionHeaderView` / `FileTileView`。一次一块 2 次提交（`619fae1` 列头、`cbd8f8f` 两个标记控件），另加两次纯版面修正（`b2caa9f` 注释归位、`4e1a3bf` 去掉提取留下的空行）。
29. 本轮的取舍标准是**形状是否自带指针**，而不是组件名字里有没有"row"：三个行形状纯视觉——点击/拖拽/hover 由 `panel/selection.slint` 里那个稳定的 `sel-touch` 统一路由，这正是行能虚拟化的前提，且三者同读一份 `ColumnInfo` 列模型、行与瓦片还共用 `row-h` 与 `Tokens.row-*` 口径，再切就把一份渲染口径摊到两个文件；`ColumnHeader` 是这一族里唯一自带 TouchArea 的（排序点击 + 重排拖拽 + 右边缘改宽），`FolderSwatchRow` / `MarkBox` 服务的是菜单飞出与清理清单、各自带 TouchArea，与文件视图无关，只是原本同处一文件。搬走后方向仍是 `widgets/rows.slint` ← `widgets/rows/*`，两个子文件只 import `theme`，是 `widgets/*` 的叶子，无环。
30. 消费方只有三行 import 变化，成员面一字未动：`panel/list.slint` 的一行拆成两行（三个形状 + 列头，761 → 762 行），`overlays/menus.slint` 与 `overlays/notes.slint` 各把一行指向 `rows/marks.slint`。校验仍按第 23、26 条那一套（写盘前断言 + 收尾整体 round-trip：三块按原声明顺序缝回、去空行/import/注释、`../../../../assets/` 还原为 `../../../assets/`，与 `6ead5e3` 的 682 行代码逐行一致）。

**剩余项**（有意保留，不是遗漏）：

- ✅ `ui/main_window.slint` 的约 40 个覆盖层区块（约 4 000 行）→ `ui/overlays/*.slint`（10 文件 11 组件），已完成；`main_window.slint` 5 541 → 3 193 行（含 552 行转发）。**仍需维护者人工点检**：导航、标签页 tear-off、拖放（含虚拟文件）、粘贴进度、缩略图、设置各页、快捷键（含 capture 与冲突）。
- ✅ `ui/panel.slint`（2 897 行）的第二层分块已完成：`panel/{tabs_bar,nav_bar,selection,list,scrollbars,overlays}.slint` 六个组件 + 面板外壳 732 行，实际形状与偏差见上文第 21–23 条。方案原设想要先盘点读写方向再动手，实施时确实是这一步决定了下沉策略（脚本统计 `root.X` 的读/写方向 → `in` / `in-out` / `callback`），也正是它把列表边界上的 5 个 `in-out` 转发全部消掉，只剩 1 个 `in`（`gutter-w`）与 3 个 `out`（`area-abs-x` / `area-abs-y` / `drag-hover-row`）。**仍需维护者人工点检**：列表区选择、左侧 gutter 条拉出的橡皮筋（含拖到边缘自动滚动）、列头拖拽改序与改宽、标签条 tear-off、跨视图拖放。
- ✅ `ui/overlays/settings.slint`（1 507 行）按页分块（`settings/{general,shortcuts,openwith}.slint` + 面板外壳）已完成：外壳 385 行，三页 680 / 323 / 331 行；快捷键菜单留在外壳里（须在面板之上）。
- ✅ `ui/sidebar.slint`（1 113 行）按组件拆分已完成：外壳 287 行（节头一族 + `Sidebar`）+ `sidebar/{item,fav,rail}.slint`（271 / 467 / 115），三次提交各一块，实际形状与偏差见上文第 24–27 条。**仍需维护者人工点检**：Places 行的 hover / 拖拽 / 盘符容量条、收藏树的展开折叠与重排拖拽、activity rail 三个按钮（侧栏开关 / 工作区 / 设置）与其 tooltip、节的拖拽重排。
- ✅ `ui/widgets/rows.slint`（922 行）按组件拆分已完成：父文件 596 行（`FileRowView` / `SectionHeaderView` / `FileTileView` 三个纯视觉形状）+ `widgets/rows/{header,marks}.slint`（223 / 114），两次提交各一块，取舍标准与偏差见上文第 28–30 条。**仍需维护者人工点检**：列头的排序点击 / 拖拽改序 / 右缘改宽 / 右键菜单，列表行与分组头的渲染（缩略图、应用图标、链接徽标、age 药丸），网格瓦片，"颜色与备注"飞出里的色条与清理清单的红勾——色条与红勾要打开飞出菜单才出现，冷启动冒烟覆盖不到。
- `bridge/tests.rs`（2 346）按模块再分（方案原文即"可后续"）；`bridge/state.rs`（1 508）、`ui/main_window.slint`（3 195，契约面）、`bridge/thumbs.rs`（862）、`bridge/rows/build.rs`（855）、`core/src/fs/tests.rs`（807）为拆分后仍超 800 的文件，理由见 [code-map.md §6](code-map.md#6-拆分前基线与剩余项)。
