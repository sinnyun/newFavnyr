# 代码地图：Favnyr 全部文件说明

本文件是仓库的"文件级说明书"：每个自有文件负责什么、关键标识符、依赖关系、契约。
正文中文，文件路径 / 类型 / 函数名保留英文原文。

- 统计日期：2026-09-29（**`main_window.slint` 的侧栏列提取落地后**统计；§1 合计已含 `.slint` 全部 36 个文件，拆分前基线与两批对照见 §6）
- 统计口径：`find` 遍历仓库，行数取 `wc -l`；**代码文件** = `.rs` / `.slint` / `.toml` / `.bat` / `.yml` / `.md`；**排除** `vendor/`（第三方源码）、`target/`、`.git/`、`.reasonix/`（工具快照）；资源文件（`assets/icons/*.svg` 等、`Cargo.lock`、`LICENSE`、`.gitattributes`、`.gitignore`）不计入
- 拆分方案与执行记录另见 [split-plan.md](split-plan.md)（§16 为进度与偏差）

## 1. 总览

| 范围 | 文件数 | 行数 |
| --- | --- | --- |
| `crates/favnyr-core`（.rs） | 53 | 15 214 |
| `crates/favnyr-gui`（.rs，含 `build.rs`） | 99 | 28 505 |
| `crates/favnyr-gui`（.slint） | 36 | 18 203 |
| `crates/favnyr-gui/i18n`（6 个 .toml） | 6 | 2 850 |
| 两个 crate 的 `Cargo.toml` | 2 | 115 |
| 工作区根 / CI / 文档（.toml / .bat / .yml / .md） | 16 | 2 394 |
| **自有合计** | **212** | **67 281** |
| `vendor/parley`（第三方，不计入，按其全部文件计） | 46 | 13 997 |

两个拆分重点的现状（拆分前：`bridge.rs` 19 001 行、`main_window.slint` 15 897 行，合计占自有代码 56%）：

- `crates/favnyr-gui/src/bridge.rs` → **`bridge/` 62 个文件**，`mod.rs` 仅 151 行（模块声明 + 重导出 + `defer`）。最大者：`install/mod.rs` 791、`tests.rs` 790、`tabs.rs` 735、`install/cb_files.rs` 714、`state.rs` 674。对外的 `crate::bridge::…` 路径全部不变。
- `crates/favnyr-gui/src/ui/main_window.slint` → **`ui/` 36 个 .slint**：`main_window.slint`（3 010 行）保留 `MainWindow` 契约面（518 个成员，见 §4.4）、布局骨架与区块实例（转发层）；内部组件按 `structs` / `theme` / `widgets/` / `panel` / `sidebar` / `workspaces` / `progress` 分文件，`MainWindow` 内的覆盖层区块（右键菜单、弹窗、设置面板、toast…）已全部提取到 `ui/overlays/*.slint`，设置面板的三页再下沉到 `ui/overlays/settings/*.slint`；`panel.slint` 的第二层同样落地，面板现有外壳 + `ui/panel/{tabs_bar,nav_bar,selection,list,scrollbars,overlays}.slint` 六个组件；`sidebar.slint` 也按组件再分，侧栏现有外壳（节头一族 + `Sidebar`）+ `ui/sidebar/{item,fav,rail}.slint` 三个组件，窗口里的整条侧栏列（四个可重排节 + 收藏树 + 回收站 + 落点指示线）再提成 `ui/sidebar/column.slint`；`widgets/rows.slint` 的最后一块再拆出 `ui/widgets/rows/{header,marks}.slint`（可交互的列头 + 菜单用的两个标记控件），行/分组头/瓦片三个纯视觉形状留在原文件。方案与执行记录见 split-plan.md §2 步骤 3 与 §16。
- 13 个超 800 行文件（拆分前）已全部拆到目标结构；`main_window.slint` 的转发层、`panel.slint` 的第二层、`sidebar.slint` 与 `widgets/rows.slint` 的分块均已完成。第一批拆出的子文件里仍有 5 个超 800（`bridge/tests.rs`、`bridge/state.rs`、`bridge/thumbs.rs`、`bridge/rows/build.rs`、`core/src/fs/tests.rs`），已在第二批下沉为子模块（见 §6）。现在全仓（`vendor/parley` 除外）只剩 `main_window.slint`（3 010，契约面）一个文件超过 800 行——`MainWindow` 的 518 个契约成员必须留在组件内（Slint 没有"部分组件"或文件包含机制可以搬走声明，理由见 §6）；能搬的只有布局子树，侧栏列已于 2026-09-29 提成 `ui/sidebar/column.slint`（3 195 → 3 010）。

## 2. 仓库结构

```
newFavnyr/
├── Cargo.toml                     工作区定义（成员、版本号、共享依赖版本）
├── Cargo.lock                     依赖锁定
├── rust-toolchain.toml            固定 Rust 1.97.1（CI 警告即错误，故意不用 stable）
├── deny.toml                      cargo-deny 配置（许可证 / 重复依赖 / 安全公告）
├── dev.bat                        Windows 一键开发启动（debug 构建 + 控制台日志）
├── reasonix.toml                  本机工具权限白名单（与构建无关）
├── README.md / AGENTS.md          对外说明 / 面向 AI 与贡献者的规范
├── .cargo/config.toml             受限网络的 crates.io 镜像（rsproxy）
├── .github/workflows/             ci.yml（check + deny）、release.yml（打 tag 出包）
├── docs/                          设计文档与变更记录（本目录）
├── vendor/parley/                 打补丁的第三方文本排版引擎（vendored，不属自有代码）
└── crates/
    ├── favnyr-core/               与 UI 无关的核心逻辑（无网络、无 UI 依赖）
    │   └── src/
    │       ├── fs/mod.rs  fs/{typed_path,sort,format,stats,tests}.rs  fs/tests/sorting.rs
    │       ├── fs/ops/mod.rs  fs/ops/{rename,name,copy,link,deletion,path_eq,tests}.rs
    │       ├── places/mod.rs  places/{windrives,linux,tests}.rs   mount.rs  eject.rs
    │       ├── thumbnail/mod.rs  thumbnail/{decode,psd,affinity,audio,video,pdf,tests}.rs
    │       ├── layout/mod.rs  layout/{tree,geom,tests}.rs
    │       ├── workspace/mod.rs  workspace/{state,named,tests}.rs
    │       ├── openers/mod.rs  openers/{tag,model,store,tests}.rs
    │       ├── favorites.rs  annotations.rs  config.rs
    │       ├── shortcuts.rs  columns.rs  paths.rs  process_lock.rs  logging.rs
    │       └── i18n.rs  error.rs  lib.rs
    └── favnyr-gui/                Slint 界面与平台集成
        ├── build.rs               编译 .slint（大栈线程）+ 嵌入 Windows 图标
        ├── assets/                favnyr.svg / favnyr.ico / icons/（65 个 SVG）
        ├── i18n/                  en fr es de it zh 六套 .toml 翻译（各 415 键）
        └── src/
            ├── main.rs            二进制入口
            ├── ui/main_window.slint      编译入口：MainWindow 契约面 + 布局骨架 + 覆盖层实例
            ├── ui/{structs,theme,panel,sidebar,workspaces,progress}.slint
            ├── ui/panel/          面板第二层 6 个组件（tabs_bar / nav_bar / selection / list / scrollbars / overlays）
            ├── ui/sidebar/        侧栏 4 个组件（column 整条列 / item / fav / rail）
            ├── ui/widgets/{rows,inputs,menus,tabs}.slint
            ├── ui/widgets/rows/   行形状的组件 2 个（header 列头 / marks 色条与红勾）
            ├── ui/overlays/              11 个覆盖层组件（menus / openwith / favorites / settings / …）
            ├── ui/overlays/settings/     设置面板的三页（general / shortcuts / openwith）
            ├── bridge/mod.rs      模块声明 + 重导出 + defer
            ├── bridge/*.rs        桥接外围模块（state/colinfo/nav/tabs/thumbs/…）
            ├── bridge/state/      AppState 的数据类型（tab / panel / clipdrop / nav / opreg）
            ├── bridge/thumbs/     scheduler.rs：缩略图队列、LRU 与在途登记
            ├── bridge/install/    install() 编排 + cb_*.rs 12 个回调簇
            ├── bridge/open_with/  picker / shellmenu / recipes / launch
            ├── bridge/rows/       行模型：mod.rs / build.rs / build/{entry,subscan}.rs / icons.rs
            ├── bridge/tests/      桥接测试的 7 个分节（opening / opregistry / openwith / thumbnails / geometry / workspace / grid）
            ├── i18n/mod.rs  i18n/{strings,labels,units,messages,tests}.rs
            ├── actions/mod.rs  actions/{opening,program,properties,shell,spawn,terminal,ffmpeg,timezone,tests}.rs
            ├── openwith/mod.rs  openwith/{windows,linux,tests}.rs
            ├── winddrag/mod.rs  winddrag/{target,paths,virtual_files,drag_out,formats,tests}.rs
            ├── winmsg.rs          跨实例 IPC（WM_COPYDATA，标签页跨窗口）
            ├── shellmenu.rs       Windows Shell 上下文菜单宿主
            ├── clipboard.rs       与系统文件管理器互通的剪贴板
            ├── winportable.rs     Windows 便携设备（MTP/WPD）
            ├── linportable.rs     Linux MTP 设备（sysfs）
            ├── winthumb.rs        系统缩略图 API
            ├── winshare.rs        Windows 分享面板
            └── winutil.rs         Windows 小工具（宽字符 / 长路径）
```

## 3. 文件清单（按行数降序）

### 3.1 `crates/favnyr-core`（53 个 .rs）

| 行数 | 文件 | 一句话职责 |
| ---: | --- | --- |
| 754 | `src/eject.rs` | 设备安全移除 / 网络盘断开 |
| 666 | `src/annotations.rs` | 路径级颜色与备注存储 |
| 621 | `src/fs/tests.rs` | fs 模块测试（排序那组已下沉到 `fs/tests/sorting.rs`） |
| 596 | `src/fs/ops/tests.rs` | 文件操作测试 |
| 590 | `src/shortcuts.rs` | 快捷键目录、解析与键位映射 |
| 563 | `src/thumbnail/tests.rs` | 预览生成测试 |
| 555 | `src/places/windrives.rs` | Windows 盘符枚举（原嵌套 `mod windrives` 提为文件） |
| 552 | `src/openers/tests.rs` | 打开方式模型测试 |
| 542 | `src/workspace/tests.rs` | 工作区测试 |
| 533 | `src/process_lock.rs` | 文件占用诊断（Restart Manager） |
| 522 | `src/layout/tests.rs` | 布局树测试 |
| 509 | `src/fs/ops/copy.rs` | 复制 / 移动 / 环检测 / 进度回调 |
| 480 | `src/places/mod.rs` | 侧栏位置模型与跨平台探测入口 |
| 471 | `src/favorites.rs` | 收藏树模型与持久化 |
| 454 | `src/layout/tree.rs` | `impl LayoutNode`（拆分 / 合并 / 比例 / 均衡 / 寻址） |
| 435 | `src/places/linux.rs` | Linux 卷探测（lsblk / udev / gvfs） |
| 429 | `src/fs/mod.rs` | 列目录、类型分类、`FileKind`、UNC、re-export |
| 414 | `src/config.rs` | `config.toml` 全局偏好与 `SidebarSection` |
| 360 | `src/thumbnail/audio.rs` | ID3v2 / FLAC 内嵌封面 |
| 320 | `src/paths.rs` | XDG / Windows 目录定位与原子写 |
| 314 | `src/openers/store.rs` | `OpenerStore` 增删改查与 `openers.toml` |
| 303 | `src/places/tests.rs` | 位置模块测试 |
| 253 | `src/workspace/state.rs` | 会话状态模型（`TabState`/`PanelState`/`WorkspaceState`） |
| 240 | `src/fs/ops/name.rs` | 名字校验、唯一名生成、创建条目 |
| 236 | `src/fs/sort.rs` | 排序 / 分组模型与 `sort()` |
| 207 | `src/fs/format.rs` | 大小 / 时间 / 年龄格式化 |
| 206 | `src/thumbnail/video.rs` | ffmpeg 抽帧与时长探测 |
| 205 | `src/workspace/named.rs` | 具名工作区文件 CRUD |
| 198 | `src/columns.rs` | 列顺序 / 可见性 / 宽度模型 |
| 189 | `src/fs/tests/sorting.rs` | fs 测试的排序 / 分组 / 类别那一组 |
| 187 | `src/mount.rs` | Linux 挂载未挂载卷（udisksctl） |
| 185 | `src/thumbnail/mod.rs` | 预览生成入口与资源上限 |
| 179 | `src/i18n.rs` | `Lang` / `Theme` 枚举与序列化 |
| 175 | `src/fs/ops/link.rs` | 符号链接 / junction / 硬链接 |
| 174 | `src/thumbnail/affinity.rs` | Affinity 内嵌 PNG 扫描 |
| 158 | `src/fs/ops/deletion.rs` | 删除 / 回收站 / 恢复 / 清空 |
| 158 | `src/openers/tag.rs` | 参数标签替换与 `TagContext` |
| 152 | `src/fs/typed_path.rs` | 地址栏输入解析（两平台各一份） |
| 148 | `src/openers/mod.rs` | 打开方式模型与上下文位 |
| 139 | `src/thumbnail/psd.rs` | PSD 内嵌 JPEG 提取 |
| 120 | `src/layout/mod.rs` | 布局树类型定义与 re-export |
| 106 | `src/openers/model.rs` | `impl Opener`（校验 / matches / 上下文判定） |
| 98 | `src/fs/stats.rs` | 递归目录统计（大小 / mtime） |
| 78 | `src/logging.rs` | tracing 初始化（stderr + 日志文件） |
| 74 | `src/thumbnail/decode.rs` | 解码上限与子进程约束 |
| 69 | `src/fs/ops/rename.rs` | 原地改名与名字合法性 |
| 59 | `src/fs/ops/path_eq.rs` | 路径关系（相等 / 包含 / 父级符号链接） |
| 51 | `src/thumbnail/pdf.rs` | `pdftoppm` → `pdftocairo` |
| 50 | `src/layout/geom.rs` | `split_area` 与纯几何 |
| 47 | `src/fs/ops/mod.rs` | `OpStatus` 与 re-export |
| 38 | `src/lib.rs` | crate 根与 re-export |
| 31 | `src/workspace/mod.rs` | `MAX_CLOSED_TABS` 与 re-export |
| 21 | `src/error.rs` | 统一错误类型 |

### 3.2 `crates/favnyr-gui`（.rs，99 个）

| 行数 | 文件 | 一句话职责 |
| ---: | --- | --- |
| 791 | `src/bridge/install/mod.rs` | `install()` 编排：197 个 `install_*` 调用 + 模块声明 |
| 790 | `src/bridge/tests.rs` | 桥接测试外壳：共享 helper 与未分节的用例（7 个分节见 `tests/`） |
| 790 | `src/main.rs` | 二进制入口与会话恢复（未拆分） |
| 735 | `src/bridge/tabs.rs` | 标签页与视图的取出 / 撕离 / 序列化 / 跨窗口转移 |
| 714 | `src/bridge/install/cb_files.rs` | 安装：文件与操作回调簇（23 个） |
| 674 | `src/bridge/state.rs` | `AppState`、`ImgMeta`、`MAX_PANELS` 与 `impl AppState`（数据类型见 `state/`） |
| 633 | `src/winportable.rs` | Windows 便携设备（MTP/WPD） |
| 625 | `src/bridge/progress.rs` | 进度 toast 栈、`OpRegistry` 驱动、重活入口 |
| 603 | `src/bridge/rows/build.rs` | 行模型重建（分区 / 分组 / 样式；条目与子扫描见 `build/`） |
| 594 | `src/openwith/linux.rs` | XDG `.desktop` 解析 |
| 579 | `src/bridge/install/cb_view.rs` | 安装：视图 / 排序 / 分组 / 标签栏回调簇（23 个） |
| 561 | `src/shellmenu.rs` | Windows Shell 上下文菜单宿主 |
| 548 | `src/bridge/rows/mod.rs` | 行高 / 缩放 / 行模型入口 |
| 500 | `src/bridge/listing.rs` | 异步与同步列目录、孤儿行、网络提示 |
| 499 | `src/clipboard.rs` | 与系统文件管理器互通的剪贴板 |
| 463 | `src/bridge/open_with/recipes.rs` | 参数配方（`Recipe`/`RECIPES`/参数解析与引用） |
| 462 | `src/bridge/thumbs/scheduler.rs` | 调度器本体：`ThumbJob`/`ThumbQueue`/`ThumbScheduler`/`ThumbLru` 与在途登记 |
| 454 | `src/bridge/tests/geometry.rs` | 桥接测试：标签几何、缩放后的行几何与图标路由 |
| 445 | `src/bridge/install/cb_sidebar.rs` | 安装：侧栏回调簇（14 个） |
| 445 | `src/bridge/nav.rs` | 首次填充、刷新、`relist_panel`、加载目录 |
| 441 | `src/bridge/selection.rs` | 选中集合、焦点与打字定位、名称过滤 |
| 438 | `src/bridge/install/cb_favorites.rs` | 安装：收藏面板回调簇（21 个） |
| 426 | `src/bridge/install/cb_openwith.rs` | 安装：打开方式回调簇（16 个） |
| 426 | `src/bridge/install/cb_prefs.rs` | 安装：设置项回调簇（27 个） |
| 418 | `src/winmsg.rs` | 跨实例 IPC（标签页跨窗口） |
| 409 | `src/bridge/thumbs.rs` | 缩略图的两个决策点与 worker 接线（调度器已下沉 `thumbs/scheduler.rs`） |
| 404 | `src/openwith/windows.rs` | `SHAssocEnumHandlers` / `IAssocHandler` |
| 392 | `src/bridge/install/cb_nav.rs` | 安装：导航回调簇（15 个） |
| 388 | `src/bridge/geometry.rs` | 面板几何、splitter 视图、均衡与撤销 |
| 364 | `src/bridge/drives.rs` | 盘符 / 卷空间 / 弹出、侧栏刷新 |
| 359 | `src/actions/opening.rs` | 打开路径 / 系统默认处理 / ShellExecute |
| 358 | `src/bridge/install/cb_workspace.rs` | 安装：工作区回调簇（14 个） |
| 356 | `src/bridge/install/cb_dnd.rs` | 安装：拖放回调簇（7 个） |
| 352 | `src/bridge/open_with/launch.rs` | 启动规划、快捷方式 / 链接、新标签打开 |
| 337 | `src/bridge/install/cb_rows.rs` | 安装：行区宽度 / 缩放 / 图片元数据回调簇（9 个） |
| 336 | `src/bridge/state/tab.rs` | `Tab`/`TabBook`/`ViewMode` 与会话状态的双向转换 |
| 331 | `src/i18n/strings.rs` | `strings_for`：321 字段 `Strings` 的机械展开 |
| 324 | `src/linportable.rs` | Linux MTP 设备发现 |
| 320 | `src/bridge/tests/thumbnails.rs` | 桥接测试：调度优先级、去重、失效世代与 LRU |
| 301 | `src/bridge/tests/openwith.rs` | 桥接测试：扩展名解析、配方与参数引用 / 预览 |
| 294 | `src/bridge/favpanel.rs` | 收藏面板推送、路径解析、打开与拖拽落点 |
| 293 | `src/bridge/install/cb_open.rs` | 安装：打开 / 上下文菜单回调簇（16 个） |
| 287 | `src/bridge/open_with/picker.rs` | 自建打开方式 picker 的枚举与图标 |
| 284 | `src/bridge/tabstrip.rs` | 面包屑、标签宽度与滚动、竖排标签条 |
| 282 | `src/bridge/stats.rs` | 递归 mtime 与图片元数据 worker |
| 268 | `src/bridge/install/cb_clipboard.rs` | 安装：剪贴板回调簇（12 个） |
| 267 | `src/actions/spawn.rs` | 进程启动（opener / 新实例 / 分离视图） |
| 261 | `src/openwith/tests.rs` | 打开方式测试 |
| 255 | `src/bridge/naming.rs` | 名称冲突、唯一目标规划、光标偏移 |
| 250 | `src/i18n/tests.rs` | 翻译测试 |
| 237 | `src/winddrag/virtual_files.rs` | 虚拟文件物化 |
| 236 | `src/bridge/keys.rs` | 快捷键分组 / 显示 / 冲突、菜单快捷键 |
| 222 | `src/bridge/paste.rs` | 粘贴冲突解决与推进 |
| 220 | `src/bridge/rows/icons.rs` | 扩展名 / 路径 / `.lnk` 图标解析 |
| 216 | `src/bridge/notices.rs` | `NoticeKind` 与 toast 文案 |
| 210 | `src/actions/tests.rs` | 系统动作测试 |
| 210 | `src/bridge/workspaces.rs` | 具名工作区签名 / 脏检查 / 加载 / 重置 |
| 207 | `src/bridge/state/panel.rs` | `Panel` 与异步列表 / 子扫描的交付类型、行模型入口 |
| 193 | `src/bridge/open_with/shellmenu.rs` | Windows shell 菜单与扩展扫描 |
| 187 | `src/winddrag/target.rs` | 自建 `IDropTarget` |
| 183 | `src/winddrag/mod.rs` | 拖放分类入口与注册 |
| 180 | `src/actions/properties.rs` | 原生属性对话框 / D-Bus `FileManager1` |
| 180 | `src/bridge/tests/grid.rs` | 桥接测试：网格排布、光标跨分区头、子文件夹与类别分组 |
| 175 | `src/winutil.rs` | Windows 宽字符 / 长路径工具 |
| 151 | `src/bridge/mod.rs` | 模块声明 + 重导出 + `defer` |
| 146 | `src/bridge/rows/build/subscan.rs` | 子文件夹扫描：请求、分组、worker 与回填 |
| 145 | `src/bridge/datafiles.rs` | 批注 / 收藏 / 打开方式的 stamp 与同步 |
| 138 | `src/i18n/messages.rs` | 错误与提示文案构造 |
| 137 | `src/bridge/state/opreg.rs` | 后台作业登记（`OpHandle`/`OpRegistry`/`OpDelivery`/占用键） |
| 136 | `src/bridge/colinfo.rs` | 列宽计算、列重排、`ColumnInfo` 推送 |
| 132 | `src/winddrag/formats.rs` | 剪贴板格式分类（`CF_HDROP` / 流 / shell IDList） |
| 130 | `src/i18n/mod.rs` | 翻译装载、目录与 `tr` |
| 125 | `src/bridge/tests/workspace.rs` | 桥接测试：`workspaces_differ` 的判定与易变字段 |
| 121 | `src/openwith/mod.rs` | 中性入口与平台选择 |
| 119 | `src/winthumb.rs` | 系统缩略图 API + PDF 的 WinRT 兜底 |
| 116 | `src/actions/ffmpeg.rs` | ffmpeg 环境探测 |
| 115 | `src/winshare.rs` | Windows 分享面板 |
| 114 | `src/bridge/rows/build/entry.rs` | `entry_to_row`：`Entry` → `FileRow` 的字段填充 |
| 110 | `src/winddrag/paths.rs` | 路径抓取与临时落盘 |
| 110 | `src/bridge/state/clipdrop.rs` | 剪贴板与拖拽暂存（`ClipOp`/`PasteJob`/落点守卫） |
| 109 | `src/actions/shell.rs` | 提权运行 / 打开方式 / 命令行引用 |
| 109 | `src/winddrag/tests.rs` | 拖放测试 |
| 104 | `src/winddrag/drag_out.rs` | shell `IDataObject` + `SHDoDragDrop` 拖出 |
| 100 | `src/bridge/tests/opregistry.rs` | 桥接测试：`OpRegistry` 的 id、目的地占用与删除声明 |
| 91 | `src/actions/mod.rs` | 系统动作公共 helpers 与 re-export |
| 91 | `src/bridge/tests/opening.rs` | 桥接测试：`plan_open` 的分组与顺序 |
| 90 | `src/bridge/watcher.rs` | `notify` 监听与防抖 |
| 89 | `src/actions/terminal.rs` | 终端 / 回收站打开 |
| 85 | `src/actions/timezone.rs` | `local_utc_offset_secs` |
| 83 | `src/actions/program.rs` | `pick_terminal` / `which` / `resolve_program` |
| 83 | `src/i18n/labels.rs` | 语言 / 主题 / 页脚等标签 |
| 82 | `src/bridge/restore.rs` | `home_dir`、`build_panels`、恢复路径 |
| 82 | `src/bridge/window.rs` | 窗口尺寸持久化 |
| 77 | `src/bridge/settings.rs` | UI 缩放预设、ffmpeg 信息展示 |
| 74 | `src/i18n/units.rs` | 大小 / 年龄单位桥接 |
| 65 | `src/bridge/state/nav.rs` | `NavHistory` 与 `SortState` |
| 30 | `build.rs` | 编译 .slint（64 MB 栈线程）+ 嵌入图标 |
| 23 | `src/bridge/language.rs` | `apply_language` |
| 11 | `src/bridge/open_with/mod.rs` | `open_with` 子模块声明与重导出 |

### 3.3 `crates/favnyr-gui/src/ui`（36 个 .slint）

| 行数 | 文件 | 内容 |
| ---: | --- | --- |
| 3 010 | `main_window.slint` | 编译入口：`MainWindow`（518 个契约成员 + `FocusScope` + 布局骨架 + 侧栏列/面板容器 + 全部覆盖层实例 + 全局提示） |
| 792 | `widgets/tabs.slint` | `NavIconButton` / `TabScrollButton` / 计时器 / 边滚动 / `ModalBackdrop` / `TabItem` / `VTabBar` / `PanelSplitterAbs` |
| 762 | `panel/list.slint` | `PanelList`：列头 + 行区（列表/网格两种视图）+ "标签不可用"横幅 + 两条自定义滚动条的数据源 + 选择层实例 + 左侧 gutter 条（两块共用同一份橡皮筋状态） |
| 751 | `panel/selection.slint` | `PanelSelection`：盖在列表区之上的指针层（悬停、单击/Ctrl/Shift、橡皮筋、中键自动滚动、延迟重命名、拖到边缘滚动） |
| 742 | `structs.slint` | 21 个 `export struct`（Rust ↔ Slint 数据模型） |
| 742 | `overlays/notes.slint` | `OverlayNotes` / `OverlayNoteBubble`：重命名、备注编辑器、新建文件夹/文件弹窗、颜色与备注清理列表、条目备注气泡 |
| 732 | `panel.slint` | `PanelComponent`：面板外壳与协调层——几何/拖放状态、面板级回调，以及垂直标签条 ×2 / 标签条 / 导航条 / 筛选条 / 列表 / 页脚 / 覆盖层七个实例的转发 |
| 680 | `overlays/settings/general.slint` | `SettingsGeneral`（ScrollView）：语言 / 主题 / 缩放 / 标签栏 / 时钟 / 列 / 深度 / ffmpeg / 路径 |
| 641 | `overlays/menus.slint` | `OverlayMenus`：主右键菜单、Split、标签栏位置、视图模式、URL、回收站、盘符/网络菜单与"颜色与备注"飞出菜单 |
| 596 | `widgets/rows.slint` | `FileRowView` / `SectionHeaderView` / `FileTileView`：文件视图的三种行/瓦片形状，全部纯视觉（指针由 `panel/selection.slint` 的统一 TouchArea 接管，因此行可以虚拟化） |
| 563 | `panel/nav_bar.slint` | `PanelNavBar`：面包屑（显示/编辑两态）、URL 区的滚动与右键、返回/前进/上跳/搜索/刷新、菜单与视图按钮、拖放把手 |
| 487 | `widgets/menus.slint` | `MenuItem` / `SubmenuRow` / `CommandIcon` / `IconMenuRow` / `ShellMenuRow` / `OwRecipeButton` / `SplitMenuItem` / `TabBarMenuItem` |
| 467 | `sidebar/fav.slint` | `FavRow` / `FavPanel`：收藏树的节头 + 展平模型 + 折叠状态 + 重排拖拽（行本身纯视觉，手势上报给面板） |
| 432 | `theme.slint` | `Theme` / `Tokens` / `Note` / `Tip` / `CtxNav` / `Dismiss` / `WindowFocus`（`export global`） |
| 414 | `workspaces.slint` | `WsButton` / `WsIconBtn` / `WorkspaceRow` |
| 412 | `overlays/openwith.slint` | `OverlayOpenWith`：打开方式/新建子菜单、shell 级联、应用选择器、提升 toast |
| 393 | `overlays/custom_command.slint` | `OverlayCustomCommand`：自定义命令编辑器 / 已购应用只读视图 |
| 385 | `overlays/settings.slint` | `OverlaySettings`：设置对话框外壳（窗口、标题栏、三页标签）与快捷键行的"取消分配"菜单；三页见 `overlays/settings/*` |
| 383 | `overlays/favorites.slint` | `OverlayFavorites`：收藏容器的命名/保存/新建三个弹窗 + 容器与收藏项右键菜单 |
| 383 | `overlays/workspaces.slint` | `OverlayWorkspaces`：工作区保存行、列表与反馈 toast |
| 379 | `overlays/dialogs.slint` | `OverlayDialogs`：永久删除、打开全部、未保存更改、粘贴冲突 |
| 376 | `widgets/inputs.slint` | `EditTextInput` / `EditLineEdit` / `SearchField` / `FieldLink` / `FilterBar` / `HintEdit` / `SettingsRowButton` / `PathRow` |
| 374 | `sidebar/column.slint` | `SidebarColumn`：整条侧栏列——四个可重排 `Sidebar` 节 + `FavPanel` 收藏树 + 固定回收站 + 落点指示线，共享一个本地参照系（网格 / 发丝线 / 指示线的坐标不再跨容器混用） |
| 373 | `panel/tabs_bar.slint` | `PanelTabsBar`：标签条（可滚动、带 "+" 按钮）、标签项的拖出/排序/关闭热区、溢出时的边缘滚动 |
| 363 | `overlays/panel_menus.slint` | `OverlayPanelMenus`：标签栏死区、标签页、列与跨视图拖放菜单 |
| 343 | `progress.slint` | `ProgressToast` / `DriveGauge` / `FfmpegCmdRow` |
| 331 | `overlays/settings/openwith.slint` | `SettingsOpenWith`（ScrollView）：打开方式列表、配方两列、Windows shell 条目、ffmpeg |
| 323 | `overlays/settings/shortcuts.slint` | `SettingsShortcuts`（ScrollView）：快捷键搜索、捕获、冲突解决、重置 |
| 302 | `overlays/toasts.slint` | `OverlayToasts`：进度 toast、拖放指示、swap pill、全局通知 |
| 287 | `sidebar.slint` | `SidebarHeaderAction(Slot)` / `SidebarSectionHeader` / `Sidebar`：可重排节头一族与侧栏容器（节顺序、拖拽重排、把 Places 节交给 `SidebarItem`） |
| 271 | `sidebar/item.slint` | `SidebarItem`：Places 单行——图标/标签/盘符容量（`DriveGauge`）、hover 与选中态、拖拽与右键把手、tooltip |
| 223 | `widgets/rows/header.slint` | `ColumnHeader`：列头单体——排序点击、重排拖拽与右边缘改宽把手（行/瓦片一族里唯一自带 TouchArea 的形状） |
| 139 | `panel/scrollbars.slint` | `PanelScrollbars`：列表的两条自绘滚动条（横向＝列溢出，纵向＝行溢出），只读写列表已有的两个偏移量 |
| 123 | `panel/overlays.slint` | `PanelOverlays`：面板之上的纯视觉层——文件落点 veil、关闭/合并/交换预览、标签拖放的落区板块（无 TouchArea，不拦截事件） |
| 115 | `sidebar/rail.slint` | `RailBtn` / `ActivityRail`：最左侧活动栏的按钮（图标 + 右侧 tooltip + 当前高亮）与三个按钮的竖排（面板开关 / 工作区 / 设置） |
| 114 | `widgets/rows/marks.slint` | `FolderSwatchRow` / `MarkBox`：菜单与弹窗共用的两个标记控件——"颜色与备注"飞出里的文件夹色条、清理清单的红勾（各自带 TouchArea，与纯视觉的行形状相反） |

### 3.4 工作区根 / CI / 文档 / 翻译

| 行数 | 文件 | 一句话职责 |
| ---: | --- | --- |
| 475 ×6 | `favnyr-gui/i18n/*.toml` | 六套翻译目录（en/fr/es/de/it/zh，各 415 键） |
| 596 | `docs/code-map.md` | 本文件 |
| 398 | `docs/split-plan.md` | 超 800 行文件拆分方案与执行记录 |
| 365 | `docs/CHANGELOG.md` | 变更记录（按日期倒序追加） |
| 178 | `deny.toml` | cargo-deny 配置 |
| 134 | `.github/workflows/release.yml` | 打 tag 构建发布产物 |
| 132 | `docs/view-modes-and-sections.md` | 视图模式 / 分组 / 子文件夹展开设计 |
| 129 | `README.md` | 对外说明 |
| 102 | `dev.bat` | Windows 一键开发启动 |
| 95 | `favnyr-gui/Cargo.toml` | GUI crate 依赖与平台特性开关 |
| 83 | `.github/workflows/ci.yml` | CI：check（警告即错误）+ deny |
| 65 | `Cargo.toml` | 工作区定义与共享依赖 |
| 64 | `docs/mirrors.md` | 受限网络构建说明 |
| 63 | `docs/thumbnails.md` | 各平台预览来源说明 |
| 47 | `AGENTS.md` | 贡献与 AI 协作规范 |
| 24 | `.cargo/config.toml` | crates.io 镜像配置 |
| 20 | `favnyr-core/Cargo.toml` | core crate 依赖 |
| 12 | `rust-toolchain.toml` | 工具链锁定 |
| 2 | `reasonix.toml` | 本机工具权限白名单 |

## 4. 详细说明

### 4.1 工作区根

| 文件 | 说明 |
| --- | --- |
| `Cargo.toml` | 工作区成员（`favnyr-core`、`favnyr-gui`）、版本号（发布以 git tag 表达）、`[workspace.dependencies]` 统一依赖版本。 |
| `rust-toolchain.toml` | 锁定 `1.97.1` + `rustfmt`/`clippy`。CI 警告即错误，浮动 channel 会让 CI 与本地编译器产生差异，因此故意不用 `stable`；升级工具链是单独的一次提交。 |
| `deny.toml` | `cargo-deny` 配置：许可证白名单、重复依赖、安全公告。CI 的 `deny` 任务执行。 |
| `.cargo/config.toml` | 把 crates.io 源替换为 `rsproxy.cn` 稀疏镜像，并让 git 依赖走 git CLI；用户级配置可覆盖。详见 `docs/mirrors.md`。 |
| `dev.bat` | Windows 一键开发启动：从仓库根构建 debug 版并启动，参数透传给 `favnyr`（如具名工作区名）；控制台故意保留以显示日志。 |
| `reasonix.toml` | 本机 AI 工具的命令白名单，与项目构建无关。 |
| `.github/workflows/ci.yml` | `check` 任务（两平台：fmt / clippy / test，警告即错误）+ `deny` 任务。 |
| `.github/workflows/release.yml` | 打 tag 时构建发布产物。 |

### 4.2 `crates/favnyr-core`（与 UI 无关）

拆分后按主题分目录；**所有对外路径（`lib.rs` re-export 与 `favnyr_core::fs::…` 等）保持不变**。

**`fs/`（列目录 / 分类 / 排序 / 格式化）**

- `mod.rs`：`FileKind`（`#[repr(u8)]`，`Folder=0 … Config=8`）——**数值被 `.slint` 直接引用，禁止重排序**，数值→枚举只走 `FileKind::from_code`；扩展名白名单（`IMAGE_EXTENSIONS`/`ARCHIVE_EXTENSIONS`）与 `classify_kind`；`list_dir`/`list_dir_counted`（带计数）、`is_listable`、UNC 处理（`unc_server_root`/`unc_share_parent`/`is_unc_path`/`list_server_shares`）、隐藏属性判定（两平台各一份 `is_hidden_attr`）。
- `typed_path.rs`：地址栏输入解析——`expand_typed_path`、`strip_home_prefix`、`expand_variables`（`%VAR%` / `$VAR` 各平台一份）。
- `sort.rs`：`SortColumn`、`SortOrder`、`Category`、`GroupMode`（Folders/…/Category）、`sort()`。
- `format.rs`：`SizeUnits`/`format_size`/`format_used_total`、`free_space_level`、`format_mtime`、`AgeUnits`/`format_age`/`age_bucket`、`civil_from_days`。
- `stats.rs`：`recursive_folder_stats`、`recursive_max_mtime`（目录遍历，与格式化无关）。
- `tests.rs`：fs 模块测试（621 行），排序 / 分组 / 类别那组 9 个用例在 `tests/sorting.rs`（189 行，含 `typed_entry` helper）。

**`fs/ops/`（文件操作，全部同步，由 bridge 决定是否放后台线程）**

- `rename.rs`：`rename_in_place`、`rename_in_place_replacing_file`、`is_valid_entry_name`。
- `name.rs`：`NameRejection`、`FORBIDDEN_NAME_CHARS`、`RESERVED_DEVICE_NAMES`、`check_file_name`、`create_entry`、`unique_sibling*`、`split_name`、`ext_of`。
- `copy.rs`：`copy_path`（含环检测 `dir_mark`/`loops_back`）、`path_size`、`copy_tree_progress`、`copy_file_progress`（大缓冲 + 进度回调）、`copy_into`、`move_into`、`move_to`、`duplicate`。
- `link.rs`：`link_into`、`link_as`、`create_symlink_win`、`windows_junction`、`windows_link`。
- `deletion.rs`：`TrashDisposition`、`permanent_delete`、`trash_with_disposition`、`trash`、`TrashError`、`restore_from_trash`、`empty_trash`（原方案中的 `trash.rs` 落到此名）。
- `path_eq.rs`：`resolve_parent_symlinks`、`paths_equal`、`is_within`。
- 契约：`error::Error::Workspace` 用法与 `_with_progress` 系列回调签名不变。

**`places/`（"位置"侧栏数据源，跨平台）**

- `mod.rs`：`PlaceKind`（Home/Folder/Drive/Volume/LockedVolume/Network/Trash）、`Place`、`user_places()`（dirs crate）、`drives()`/`drives_signature()`/`is_removable_mount`/`is_network_fs`/`HIDDEN_VOLUME_FS`/`ENCRYPTED_FS`、网络（`is_network_path`/`NetShares`/`net_shares`/`net_connect_prompt`/`network_extra`）、热插拔与未挂载卷（`HOTPLUG_BUSES`/`is_hotplug_device`/`block_signature`/`unmounted_volumes`）。
- `windrives.rs`：`GetLogicalDrives`/`GetDriveTypeW`/`GetDiskFreeSpaceExW` 等盘符枚举（原嵌套模块，`#[cfg(windows)]`）。
- `linux.rs`：`trash_place`、udev 工具函数、gvfs 挂载（`gvfs_mounts`/`gvfs_pretty_name`/`udev_unescape`）、`block_inventory`（`lsblk` 解析）、`volume_kind`。
- 契约：`PlaceKind` 值被 GUI 按 `kind` 解释（`SidebarPlace.kind`），**枚举顺序与含义不得改动**；两平台都要能编译。

**`thumbnail/`（预览图生成，纯函数、无磁盘缓存；内存 LRU 由调用方维护）**

- `mod.rs`：`generate`（按 `FileKind` 分派）、`from_encoded`、`from_image`、`image_meta`、`downscale`；资源上限 `MAX_DECODE_PIXELS`/`MAX_DECODE_ALLOC`。
- `decode.rs`：`decode_limits`/`decode_bounded`/`run_bounded`（超时 + 输出上限）/`no_console`（Windows 不弹控制台）。
- `psd.rs` / `affinity.rs`：PSD 内嵌 JPEG、Affinity 内嵌 PNG 扫描。
- `audio.rs`：`from_audio`、`from_id3_cover`（ID3v2 `APIC`，含 unsynchronization 还原）、`from_flac_cover`。
- `video.rs`：`from_video`（ffmpeg `thumbnail`+`scale`）、`video_seek_offset`、`video_duration_secs`（先 MP4 mvhd，再 `ffprobe`）。
- `pdf.rs`：`from_pdf`（`pdftoppm` → `pdftocairo`）。
- SVG 不在此处：由 Slint 原生渲染（见 `docs/thumbnails.md`）。

**`layout/`（面板布局树）**

- `mod.rs`：`SplitDir`、`LayoutNode`（二叉树，叶子是 `panels` 下标）、`NodePath`/`Side`、`Rect`/`PanelGeom`/`SplitterGeom`/`Layout`、`RATIO_EPSILON`。
- `tree.rs`：`impl LayoutNode`（拆分 / 合并 / 比例调整 / 均衡 / 按路径寻址 / 扁平化入口）；`geom.rs`：`split_area` 与纯几何。
- 存在意义：Slint 不能递归实例化组件，因此树在 Rust 侧操作，渲染前**扁平化**为绝对矩形列表。
- 不变量最密（比例之和、最小比例、叶子下标）：只搬移、不重构，靠 522 行测试兜底。

**`workspace/`（会话序列化）**

- `state.rs`：`TabState`（路径 + 排序 + `view_mode`/`subfolders`/`collapsed`）、`PanelState`、`SidebarSectionsState`、`WorkspaceState`；`MAX_CLOSED_TABS`；**选中态与前进/后退历史不持久化**（无恢复价值）。
- `named.rs`：`NamedWorkspaceFile`/`NamedWorkspaceMeta`、`id_is_safe`、`workspace_file_path`、`list`/`find`/`save`/`overwrite`/`load`/`rename`/`delete`、`generate_id`/`now_nanos`。
- 契约：`TabState` 的向后兼容字段（`view_mode`/`subfolders`/`collapsed` 与旧 `preview` 标志）语义不得改动。

**`openers/`（"打开方式"模型，不含进程启动）**

- `mod.rs`：`OpenerIcon`、`Opener`（程序 / 参数 / 上下文位 / 扩展名 / 图标）、`CTX_FILE`/`CTX_DIR`/`CTX_BACKGROUND`/`CTX_EXT_ALL`。
- `tag.rs`：`TAGS`、`LIST_TAG`（`{files}`）、`LIST_NAMES_TAG`（`{names}`）、`TagContext`、`file_uri`、`substitute`。**安全边界**：参数是列表，按 token 替换，路径含空格/元字符仍是一个不透明参数，GUI 直接交给 `Command`，绝不过 shell——只搬不改。
- `model.rs`：`impl Opener`（校验、`matches`、上下文判定、`generate_id` 计数器）；`store.rs`：`OpenerStore`（`openers.toml`）。

**其余单文件**

| 文件 | 说明 |
| --- | --- |
| `eject.rs`（754） | `safe_remove(device, power_off)`：Linux `udisksctl unmount`+`power-off`；Windows `CM_Request_Device_Eject`（与通知区域同路径）。`disconnect(device)` 断开映射网络盘。`EjectError` 分类错误供 GUI 出本地化提示。 |
| `annotations.rs`（666） | `Annotation`（颜色槽 + 备注）、`AnnotationStore`、`Orphan`（路径已不存在，供清理 UI）。`MAX_COLOR_SLOT = 7`、`MAX_NOTE_CHARS = 240`。数据属于**路径**而非窗口，跨工作区跨会话生效，改动即写盘。 |
| `shortcuts.rs`（590） | `ActionGroup`、`ActionDef`、`ACTIONS`（44 条可绑定动作）、`Chord`（解析/序列化）、`Keymap`（默认 + 用户覆盖 + 冲突检测）。GUI 把 Slint `Key.*` 规范化后传入并执行。 |
| `process_lock.rs`（533） | 占用诊断，**只在破坏性操作失败后调用**。Windows：先 `CreateFileW` 检测共享冲突，再把受影响文件一次批量交给 Restart Manager。`PathLock`、`diagnose(path)`。 |
| `favorites.rs`（471） | `FavNode`/`FavStore`/`FlatFav`；增删改名移动折叠，单一共享 `favorites.toml`。 |
| `config.rs`（414） | `config.toml` 全部字段；`SidebarSection`（`Drives=0 … Network=3`，`COUNT`）定义侧栏分区顺序。 |
| `paths.rs`（320） | `data_dir`/`config_dir`/`cache_dir`（Linux XDG，Windows `%APPDATA%`/`%LOCALAPPDATA%`）、各类路径、`ensure_dirs`、`write_atomic`、`preserve_unreadable`。 |
| `columns.rs`（198） | `KNOWN_COLUMNS`、`ColumnSpec`、`default_columns`、`sanitize`；`name` 是锚列（`ANCHOR_COLUMN`）永远第一且可见；列标识用 `&str` 代码便于加列。 |
| `mount.rs`（187） | Linux 挂载（`udisksctl`，polkit 自管授权）；`MountError`、`mount(device) -> PathBuf`。与 `eject.rs` 对称。 |
| `i18n.rs`（179） | 仅两个枚举及其 TOML 序列化：`Lang`（En/Fr/Es/De/It/Zh）、`Theme`（Auto/Light/Dark）。翻译表在 GUI 侧。 |
| `logging.rs`（78） | tracing 初始化：stderr + `cache/favnyr.log`。release 版无控制台，日志文件是唯一排障入口。 |
| `error.rs`（21） | `Error`（thiserror）五个变体：`Io`/`Config`/`Workspace`/`Favorites`/`Openers`；`Result<T>` 别名。 |
| `lib.rs`（38） | crate 根：声明 18 个 `pub mod` 并 re-export 主要类型（`Config`/`SidebarSection`、`Error`/`Result`、`FavNode`/`FavStore`/`FlatFav`、`Entry`/`FileKind`/`SortColumn`/`SortOrder`、`Lang`/`Theme`、`Layout`/`LayoutNode`/`Rect`/`Side`/`SplitDir`、`Opener`/`OpenerIcon`/`OpenerStore`/`TagContext`、`Place`/`PlaceKind`、`Thumbnail`、`WorkspaceState`…）。 |

### 4.3 `crates/favnyr-gui`

**`main.rs`（790 行，未拆分）** — 二进制入口。

- 启动顺序：tracing → 建目录 → 载入/创建 `config.toml` → 恢复会话（含具名工作区回退：找不到时退回默认并提示）→ `slint::include_modules!()` → `bridge::install()` → Slint 事件循环。
- 命令行（`parse_startup_request`）：无参数 = 正常启动；普通参数 = 要打开的具名工作区名（多词拼成一个名字）；两个内部协议 `--detached-tab <dir> <x> <y> <tab_bar_mode>` 与 `--detached-view <x> <y> <n> <dirs…>` 由撕离/分离视图自身带出，用于把窗口摆到拖放点所在显示器；分离实例是临时的，不写回工作区。
- 退出：`persist_window_size`（`suppress_workspace_persist` 为真时跳过）、控制台 Ctrl+C 处理器。
- 平台模块声明在此：`linportable` 仅 Linux；`winddrag`/`winportable`/`winshare`/`winthumb`/`winutil` 仅 Windows；`winmsg`/`shellmenu`/`clipboard`/`openwith`/`actions` 两平台都编（内部按平台分实现）。

**`bridge/`（62 个文件）** — Rust 状态与 Slint 界面的唯一桥梁。`mod.rs` 只做模块声明与重导出（`pub use state::*;` 等），**`crate::bridge::…` 对外路径不变**。

| 模块 | 文件（行数） | 内容 |
| --- | --- | --- |
| 状态 | `state.rs`（674）+ `state/`（855） | 外壳留 `AppState`（`Rc` 共享）、`ImgMeta`、`MAX_PANELS`、`impl AppState`、`remember_closed_panel`；数据类型分文件：`tab.rs`（`Tab`/`TabBook`/`ViewMode` 与序列化）、`panel.rs`（`Panel`、`AsyncListing*`/`SubScan*`）、`clipdrop.rs`（`ClipOp`/`TransientDropGuard`/`IncomingDropStaging`/`PasteJob`）、`nav.rs`（`NavHistory`/`SortState`）、`opreg.rs`（`OpRegistry`/`OpHandle`/`OpDelivery`/`reservation_key`）。子模块一律 `pub(in crate::bridge)` + 父层 `pub(super) use x::*;`，对外路径不变 |
| 列信息 | `colinfo.rs`（136） | `col_width`/`set_col_width`/`reorder_column_by_delta`/`push_settings_columns` |
| 通知 | `notices.rs`（216） | `NoticeKind`、`show_notice`、锁定/跳过/改名失败文案 |
| 设置 | `settings.rs`（77） | `UI_SCALE_PRESETS`、缩放应用、ffmpeg 信息展示 |
| 盘符 | `drives.rs`（364） | `EjectOp`/`spawn_eject`、`DriveSpaceUi`、`refresh_sidebar`、`push_sidebar_sections_ui` |
| 数据文件 | `datafiles.rs`（145） | 批注/收藏/打开方式的 stamp、同步、保存；`flat_to_favnode` |
| 打开方式 | `open_with/`（1 306） | `picker.rs`（枚举与图标）、`shellmenu.rs`（shell 菜单与扩展扫描）、`recipes.rs`（`Recipe`/参数解析）、`launch.rs`（`Launch`/`plan_open`/快捷方式） |
| 收藏面板 | `favpanel.rs`（294） | `push_favorites_ui`、路径解析、打开、拖拽落点与排序 |
| 导航 | `nav.rs`（445） | 首次填充、`refresh_all_panels`、`relist_panel`、`load_directory` |
| 标签页 | `tabs.rs`（735） | `take_view`、撕离（`tear_off_*`）、序列化、跨窗口转移、外部拖放、`split_with_*` |
| 命名 | `naming.rs`（255） | 名称冲突、唯一目标规划、`EntryNameAvailability`、光标偏移 |
| 粘贴 | `paste.rs`（222） | `resolve_replace`、`advance_paste`、`begin_paste*`、`execute_paste` |
| 进度 | `progress.rs`（625） | 进度常量、toast 栈、`OpRegistry` 驱动、`start_heavy_op`/`run_heavy` |
| 列目录 | `listing.rs`（500） | 网络路径提示、异步/同步列目录、异步结果落地、孤儿行 |
| 几何 | `geometry.rs`（388） | 布局几何、splitter 视图、`EqualizeUndo`、`update_panels_ui` |
| 快捷键 | `keys.rs`（236） | 分组/显示/冲突、菜单快捷键、覆盖写回、页脚 |
| 缩略图 | `thumbs.rs`（409）+ `thumbs/scheduler.rs`（462） | 外壳留两个决策点 `thumbnail_kind_for_row`/`generate_thumb` 与 worker 接线、渲染窗口；`scheduler.rs` 留 `ThumbJob`/`ThumbLocation`/`ThumbRequest`/`ScheduledThumb`/`InFlightThumb`/`ThumbWork`/`ThumbPriority`/`ThumbQueue`/`ThumbScheduler`/`ThumbLru` |
| 统计 | `stats.rs`（282） | 递归 mtime worker、图片元数据 worker |
| 行模型 | `rows/`（1 631） | `mod.rs`（缩放常量、行高、`RowStyle`、分区构建）、`build.rs`（`RowsSource`/`Section`/样式）+ `build/entry.rs`（`entry_to_row`）+ `build/subscan.rs`（子文件夹扫描的请求/分组/worker/回填）、`icons.rs`（扩展名/路径/`.lnk` 图标） |
| 选中 | `selection.rs`（441） | 选中集合操作、焦点与打字定位、名称过滤、剪切标记 |
| 其他 | `language.rs`（23）、`restore.rs`（82）、`tabstrip.rs`（284）、`watcher.rs`（90）、`window.rs`（82）、`workspaces.rs`（210） | `apply_language`；恢复会话的 `home_dir`/`build_panels`；面包屑与标签条几何；`notify` 防抖；窗口尺寸持久化；具名工作区签名与加载 |
| 安装 | `install/mod.rs`（791）+ `cb_*.rs` ×12 | `install()` 编排（197 个调用，**顺序即初始化时序，不得重排**）；回调按簇分文件：`cb_nav`(15)/`cb_view`(23)/`cb_rows`(9)/`cb_open`(16)/`cb_openwith`(16)/`cb_files`(23)/`cb_sidebar`(14)/`cb_favorites`(21)/`cb_clipboard`(12)/`cb_dnd`(7)/`cb_workspace`(14)/`cb_prefs`(27) |
| 测试 | `tests.rs`（790）+ `tests/`（1 571） | 原 `bridge.rs` 测试区。外壳留共享 helper（`plain_style`/`layout_at`/`opener_for`/`files`、`tab`/`panel`/`ws`、`grid_style`/`named_row`/`header_row`/`test_entry`、`op_handle`/`op_handle_writing`）与未分节的用例；7 个分节下沉 `tests/`：`opening`(6)/`opregistry`(7)/`openwith`(10)/`thumbnails`(9)/`geometry`(14)/`workspace`(7)/`grid`(6) 个用例，外壳自留 33 个。测试项保持私有，故子模块只声明 `mod x;`，不加重导出 |

关键约定（拆分后仍必须遵守）：闭包捕获的 `Rc`/`Weak` 克隆原样保留（`AppState` 是 `Rc<RefCell>`，非 `Send`）；`defer()`（0 ms 定时器）不得改成直接调用（避免模型回调里改模型触发 "Recursion detected"）；可见性只升不降，内部项用 `pub(super)` / `pub(in crate::bridge)`，不提升为 `pub`。

**`i18n/`（6 个文件）** — GUI 翻译。

- `mod.rs`：六个 `.toml` 用 `include_str!` 内嵌（`EMBED_*`）；用户可在 `<config>/favnyr/i18n/{lang}.toml` 覆盖任意键；级联回退**本语言 → 英文 → 键名本身**（不 panic、不返回空串）；`tr`。
- `strings.rs`：`strings_for` 构造 Slint `Strings`（321 字段，机械展开，按键名对照）。
- `labels.rs`：语言/主题/标签栏/页脚标签与快捷键文案；`units.rs`：`size_units`/`age_units`（桥接 core 的 `SizeUnits`/`AgeUnits`）；`messages.rs`：eject/mount/trash/占用/改名失败等错误文案。

**`actions/`（10 个文件）** — 系统动作，全部 fire-and-forget（记日志，不向 UI 抛错）。按功能分文件，平台差异用 `#[cfg]` 就地处理（不是按平台分文件）：

- `opening.rs`：`shell_execute*`、`open_path`、`default_handler_app_id`、照片图库激活。
- `spawn.rs`：`run_opener*`、`spawn_program*`、`spawn_new_instance`、`spawn_detached_view`、`run_with_files`。
- `shell.rs`：`open_elevated`（`runas`）、`open_with`、`win_append_arg`（命令行引用）；`properties.rs`：`show_native_properties`、D-Bus `FileManager1`。
- `terminal.rs`：`open_terminal`（Linux 已知终端列表 + `pick_terminal`；Windows Terminal → `cmd`）、`open_trash`。
- `program.rs`：`pick_terminal`/`which`/`resolve_program`/`program_is_valid`；`ffmpeg.rs`：`ffmpeg_available`/`ffmpeg_info`/`in_flatpak`/发行版索引；`timezone.rs`：`local_utc_offset_secs`（两平台各一份）。
- `mod.rs`：`with_clipboard`/`copy_to_clipboard`（arboard，进程内持久实例）与 re-export；对外符号（`open_path`/`open_terminal`/`run_opener`/`spawn_new_instance`/`ffmpeg_info`/`local_utc_offset_secs`…）不变。

**`openwith/`（4 个文件）** — "打开方式"候选枚举；自建 picker 而非系统对话框，以便把用户选择持久化为 `Opener`。中性类型 `AppHandler` 在 `mod.rs`，平台实现用 `#[path]` 挂到 `imp`：Windows（`windows.rs`）走 `SHAssocEnumHandlers` → `IAssocHandler`（显示名、key、是否推荐），启动走 `Invoke`；Linux（`linux.rs`）解析 XDG `.desktop`（不引依赖）。附加 `icon_rgba*`、`launch`、`resolve_shortcut`/`create_shortcut`、`browse_for_*`。

**`winddrag/`（7 个文件，仅 Windows 编译）** — 原生拖放（混合方案：窗内仍由 Slint 处理）。

- `target.rs`：自建 `FavnyrDropTarget`（`IDropTarget_Impl`）替换 winit 的，接收 `DragOver` 并复用 Favnyr 自身的悬停与菜单。
- `formats.rs`：剪贴板格式分类（`CF_HDROP` / 流式 HDROP / shell IDList / 虚拟文件判定）、`file_paths`。
- `paths.rs`：路径抓取与临时落盘（`capture_application_paths`/`create_drop_dir`/`capture_path_tree`）。
- `virtual_files.rs`：邮件附件/压缩包条目/浏览器图片等无路径数据 → 物化到临时目录（`write_istream`/`write_hglobal`、`TransientDropGuard` 式清理）。
- `drag_out.rs`：`drag_files` → shell 构造 `IDataObject` + `SHDoDragDrop`（默认拖拽图）。
- 契约：临时目录清理语义跨文件后仍必须与拖放生命周期绑定（`Drop` 实现不得提前/延后释放）。

**平台单文件**（`main.rs` 声明，未拆分）

| 文件 | 说明 |
| --- | --- |
| `winmsg.rs`（418） | 跨实例 IPC（标签页跨窗口拖放）。不引依赖，直接 FFI `user32`/`comctl32`：`SetPropW` 标记 + `SetWindowSubclass` 拦截 `WM_COPYDATA`；拖放时源实例用 `WindowFromPoint` 找目标窗口并 `SendMessageW`。 |
| `shellmenu.rs`（561） | Windows Shell 上下文菜单宿主：`IShellItemArray` → `BindToHandler(BHID_SFUIObject)` → `IContextMenu`，枚举条目（含位图图标）、按黑名单过滤、回放选中项；非 Windows 提供空实现 stub。 |
| `clipboard.rs`（499） | 与系统文件管理器互通的剪贴板：Windows `CF_HDROP` + Preferred DropEffect；Linux Wayland `text/uri-list` + KDE/GNOME 剪切标记。`write_files(paths, cut)`、`read_files()`。 |
| `winportable.rs`（633） | Windows 便携设备（安卓 MTP/WPD，无盘符）：Shell 名称空间检测 + `PortableDevice` 列表、`signature`（增量刷新）、`request_refresh`、`open`。 |
| `linportable.rs`（324） | Linux 对应实现：MTP 不是文件系统，靠 USB 接口描述串 + sysfs 识别；`devices`/`signature`/`open`（gvfs `mtp://` URI）。 |
| `winthumb.rs`（119） | `shell_thumbnail`（`IShellItemImageFactory::GetImage`，系统缩略图缓存）为唯一主来源；`pdf_thumbnail`（WinRT 兜底）。两个决策点已移至 `bridge/thumbs.rs`。 |
| `winshare.rs`（115） | Windows 分享面板：非打包应用直接调 shell Share 动词会 `ERROR_INVALID_WINDOW_HANDLE`，因此自行注册 `DataTransferManager`、提供 `StorageItems` 并弹 UI。 |
| `winutil.rs`（175） | `wide`（UTF-16）、`long_path`（`\\?\` 长路径）、`has_short_component`。 |
| `build.rs`（30） | ① 在**独立 64 MB 栈线程**里调 `slint_build::compile("src/ui/main_window.slint")`（Windows 主线程栈约 1 MB，大文件递归解析曾溢出）；② Windows 下用 `winresource` 嵌入 `assets/favnyr.ico`（失败仅告警，不阻断构建）。 |

### 4.4 `crates/favnyr-gui/src/ui`（36 个 .slint）

**依赖方向**（无环，编译器强制）：`structs` ← `theme` ← `widgets/*`（`widgets/rows/{header,marks}.slint` 只 import `theme`，是 `widgets/*` 的叶子）← `panel/{overlays,scrollbars,tabs_bar,nav_bar,selection}` ← `panel/list.slint`（再 import `selection`/`scrollbars`、`widgets/rows.slint` 与 `widgets/rows/header.slint`）← `panel.slint`；侧栏一族 `progress` / `widgets/tabs` ← `sidebar/{item,fav}.slint`、`sidebar/item.slint` ← `sidebar.slint` ← `sidebar/fav.slint` ← `sidebar/column.slint`（整条列只用这两个组件的公开成员，列本身再汇入窗口），`sidebar/rail.slint` 只依赖 `structs`/`theme`，是独立叶子；并行分支 `workspaces`、`overlays/*` ← `overlays/settings/*`（`overlays/menus.slint` 取 `FolderSwatchRow`、`overlays/notes.slint` 取 `MarkBox`，均来自 `widgets/rows/marks.slint`）；各链都汇入 `main_window.slint`（编译入口）。

- `structs.slint`：21 个 `export struct`——Rust ↔ Slint 数据模型（`FileRow`、`PanelView`、`ColumnInfo`、`Strings`、`FavNode`、`SidebarPlace`…）。
- `theme.slint`：`export global`：`Theme`（调色板，`apply-theme()` 单一写入点）、`Tokens`（尺寸/圆角/语义色）、`Note`/`Tip`/`CtxNav`/`Dismiss`/`WindowFocus`。`CtxNav` 由根文档 `export { CtxNav } from "theme.slint";` 重导出——**只有根文档的导出会被 `include_modules!()` 再导出**，Rust 侧的 `crate::CtxNav` 依赖于此。
- `widgets/*`：纯展示组件，靠 `in property <Strings> strings` 等显式参数解耦；引用 `@image-url` 时路径相对**声明文件**解析（`widgets/*.slint`、`overlays/*.slint`、`panel/*.slint` 与 `sidebar/*.slint` 用 `../../../assets/…`，`overlays/settings/*.slint` 与 `widgets/rows/*.slint` 用 `../../../../assets/…`，`ui/*.slint` 用 `../../assets/…`）。
- `widgets/rows.slint`（596）与 `widgets/rows/*.slint`：文件视图的行形状。原文件 922 行、6 个组件，本轮搬出两块——`rows/header.slint`(223：`ColumnHeader`，列头一格的排序点击、重排拖拽与右边缘改宽把手，是这一族里**唯一自带 TouchArea** 的形状)、`rows/marks.slint`(114：`FolderSwatchRow` 文件夹色条 + `MarkBox` 红勾，本就是给菜单/弹窗用的，与文件视图无关)。留在 `rows.slint` 的 `FileRowView` / `SectionHeaderView` / `FileTileView` 三个形状**纯视觉**（手势由 `panel/selection.slint` 的统一 `sel-touch` 路由，这正是行能虚拟化的前提），三者同读一份 `ColumnInfo` 列模型、行与瓦片还共用 `row-h` 与 `Tokens.row-*` 口径，因此不再下切。消费方只需改 import 路径：`panel/list.slint` 一行拆成两行（三个形状 + 列头），`overlays/menus.slint` 与 `overlays/notes.slint` 各把一行指向 `rows/marks.slint`；`@image-url` 在子目录共下沉一级（4 句：上下行箭头 ×2、勾选图标 ×2）。顺带修掉一处上轮遗留：`ModalBackdrop` 的区块注释在前次提取时留在了 `rows.slint`，现已归位到声明它的 `widgets/tabs.slint`（787 → 792，纯注释）。
- `panel.slint`：`PanelComponent` 的外壳（732 行）——垂直标签条（`VTabBar` 左右两条）、标签条、导航条、筛选条、列表、页脚、视觉覆盖层七个实例的放置与转发，加上面板级状态（工作区、列模型、拖放标志、几何常量）。
- `panel/*.slint`：面板的**第二层**，按职责而非方案的 header/columns/rows/footer 切分——`tabs_bar`(373：滚动标签条 + chevrons + 滚轮 + 拖拽重排时的边缘自动滚动)、`nav_bar`(563：历史/视图按钮、面包屑 ↔ 编辑框、跨视图 grip、split/close)、`selection`(751：列表区的指针层——hover、点击/多选、橡皮筋、中键与边缘自动滚动、延后重命名)、`list`(762：列头 + 行/网格视图 + "标签不可用"横幅，持有选择层与 gutter 条共用的橡皮筋状态)、`scrollbars`(139：两条自定义滚动条)、`overlays`(123：拖放遮罩/预览/分区高亮，纯视觉、无 TouchArea)。
  - **状态下沉**优先于跨边界别名：方案原本设想用 `in-out` 把 `tabs-flick.viewport-*` / `sel-touch.*` / `rows-scroll.*` 接出去，实际改为把拥有该状态的逻辑一起搬走，5 处 `in-out` 转发因此消失。跨 `PanelList` 边界的几何只剩三个成员——`in property <length> gutter-w`（面板的左内缩）与 `out property` 的 `area-abs-x` / `area-abs-y`（列表区在窗口里的绝对位置，面板据此算 `body-top` / `list-inset-x`）和 `drag-hover-row`；`tabs-flick` 完全私有，面板改经 `scroll-tabs-*` 回调驱动。
  - `panel/list.slint` 再下一层：`PanelSelection` 与 `PanelScrollbars` 由它实例化（`scrollbars` 必须是**列表**的直接子元素，因为 Flickable 与 `list-area` 都会裁剪，条被裁掉就看不见）。
- `sidebar.slint`：侧栏外壳（287 行）——`SidebarHeaderAction` / `SidebarHeaderActionSlot` / `SidebarSectionHeader`（四个可重排节共用的节头一族）与 `Sidebar`（一节 Places：节内排布、拖拽重排、把每行交给 `sidebar/item.slint`）。
- `sidebar/*.slint`：从 `sidebar.slint` 搬出的三个组件，对外成员面逐字未变（`main_window.slint` 只是把一行 import 换成三行）——`item`(271：Places 单行，盘符容量条 `DriveGauge`、hover/选中、拖拽与右键把手、tooltip)、`fav`(467：收藏树 `FavRow` + `FavPanel`，展平模型、折叠状态与重排拖拽；行本身纯视觉，手势上报面板)、`rail`(115：活动栏 `RailBtn` + `ActivityRail`)。`fav.slint` 反向 import `sidebar.slint` 取 `SidebarSectionHeader`（四节共用，留在原文件）；三处新文件的 `@image-url` 共 16 句下沉一级（`../../../assets/…`）。
- `sidebar/column.slint`(374)：从 `main_window.slint` 的布局骨架里搬出的**整条侧栏列**（原 `left-column := Rectangle`，245 行 / 56 个 `root.` 名）。`export component SidebarColumn inherits Rectangle`，成员面按方向三分：33 个 `in`（四节 Places、收藏树模型、折叠态、重排态、item/收藏内部拖拽、全局 tab/file 拖拽绝对坐标）、7 个 `in-out`（收藏右键菜单的 `fav-menu-*`，与 `overlays/favorites.slint` 共用同一份状态）、20 个 `callback`（窗口侧的状态改动一律上报，`finish-sidebar-item-drag` 带 `-> bool` 返回值，与 `panel/selection.slint` 的 `row-clicked` 同一手法）。列自己读不回来的东西反向留 11 个 `out`（`hover-container` 等四个 hover 态、两个重排光标、四个 `sidebar-stack` 框架数值）与两个 `public pure function section-top/section-bottom`（节的 y 与底边，窗口侧的重排算法要用）。**节序号 `sidebar-section-rank()` 没有复制**：改由窗口把 5 个已解析的整数（四节的 rank + 拖拽源的 rank）传进来，避免同一份排序回退逻辑存在两份。列内没有 `@image-url`（图标全在 `Sidebar` / `FavPanel` 里），因此本文件不需要调整资源路径。
- `overlays/*`：10 个文件、11 个覆盖层组件（`notes.slint` 另含 `OverlayNoteBubble`），每个都是 `export component OverlayX inherits Rectangle { width: 100%; height: 100%; … }`——**透明宿主：自身不接收输入**，子元素命中原样生效；实例化位置保持原区块在子元素列表中的次序，因此 z 序不变。块内 `root.` 现在指向宿主（尺寸与窗口一致），窗口契约通过实例处的显式转发接入：只读 → `x: root.x;`、双向（覆盖层自关、字段编辑）→ `x <=> root.x;`、动作 → `cb(a) => { root.cb(a); }`。
  - 拿不到的外层 id（`key-scope.focus()`）改为组件内 `callback return-focus();`，在实例处接回 `key-scope`（`notes`/`panel_menus`/`settings`）。
  - 面板类覆盖层里唯一的例外：`overlays/settings.slint` 的快捷键菜单必须画在设置面板**之上**，故与面板同文件、声明在其后（原始次序即如此）。
- `overlays/settings/*`：设置对话框的**第二层**——三页各自一个文件（`general` / `shortcuts` / `openwith`），每个都是 `export component SettingsX inherits ScrollView`，内容是原 `ScrollView` 的子女（页面自带的 `vertical-stretch: 1` 留在实例处）；外壳保留窗口、标题栏、三页标签与"取消分配"菜单，并按 `in`/`in-out`/`callback` 三向转发各页真正用到的成员。子目录下 `@image-url` 再深一级（`../../../../assets/…`）。
- `main_window.slint`：`MainWindow` 契约面（518 个成员，逐字未变）+ `FocusScope` 键盘处理 + 布局骨架 + `SidebarColumn` / 面板容器实例化 + 全部 `Overlay*` 实例（转发层）+ 全局提示。覆盖层提取后 `main_window.slint` 是 3 195 行、其中 552 行是覆盖层转发；侧栏列提走后（−185 行 → 3 010）转发层再加 60 行列实例，共约 610 行。

**`MainWindow` 契约面（拆分后逐字未变，518 个成员）**：

| 类别 | 数量 |
| --- | ---: |
| `in-out property` | 239 |
| `callback` | 207 |
| `in property` | 65 |
| `out property` | 6 |
| `public function` | 1 |
| **合计** | **518** |

另有 20 个内部 `function` 与 8 个 `pure function`（非 Rust 契约，但覆盖层提取时会用到其中一部分）。这些名字与类型是 Rust 侧 `install()` 的绑定目标，**一个都不能改名**。

### 4.5 翻译、资源与文档

| 位置 | 说明 |
| --- | --- |
| `favnyr-gui/i18n/*.toml` | 六套翻译（en/fr/es/de/it/zh），各 475 行、415 个键。新增语言 = 新增 `.toml` + 在 `i18n/mod.rs` 的 `embedded()`/`catalog()` 登记。 |
| `favnyr-gui/assets/favnyr.svg` / `.ico` | 窗口图标（Slint）与可执行文件图标（build.rs 嵌入）。 |
| `favnyr-gui/assets/icons/*.svg` | 65 个界面图标（类型图标、工具栏、状态）。 |
| `docs/thumbnails.md` | 各平台预览来源；两个决策点 `thumbnail_kind_for_row` / `generate_thumb`（现位于 `bridge/thumbs.rs`）。 |
| `docs/view-modes-and-sections.md` | 三种视图模式、网格几何、分组/分类/分区模型、子文件夹展开。 |
| `docs/mirrors.md` | 受限网络构建（镜像）说明。 |
| `docs/CHANGELOG.md` | 变更记录；每次代码改动后**末尾追加**（日期 + 摘要 + 涉及文件）。 |
| `docs/split-plan.md` | 超 800 行文件的拆分方案、迁移顺序与执行记录（§16 为进度与偏差）。 |
| `docs/code-map.md` | 本文件（拆分后版本）。 |

### 4.6 不在统计口径内

- `vendor/parley/`（46 个文件、13 997 行）：打补丁的第三方文本排版引擎（`PATCHES.md` 记录补丁）。修改它属于上游补丁维护，不适用本项目的拆分规则。
- `.reasonix/tasks/`：本机 AI 工具的会话快照，非代码。
- `target/`：构建产物。

## 5. 跨文件契约（拆分时必须保持）

| 契约 | 位置 | 约束 |
| --- | --- | --- |
| `FileKind` 数值 | `favnyr-core/src/fs/mod.rs` ↔ `.slint` | `#[repr(u8)]` 值被 `.slint` 直接比较/引用，**禁止重排序**；数值→枚举只走 `FileKind::from_code`。 |
| `SidebarSection` 数值 | `core/src/config.rs` ↔ `bridge/` | `Drives=0 … Network=3` 与侧栏分区顺序绑定。 |
| Slint 名称面 | `bridge/install/` ↔ `MainWindow` | 518 个属性/回调的名字与类型是两侧契约（§4.4）；改一侧必须同步另一侧。 |
| `CtxNav` 导出链 | `ui/theme.slint` → `ui/main_window.slint` | 只有根文档的导出会被 `include_modules!()` 再导出，`CtxNav` 必须由 `main_window.slint` 重导出，否则 `crate::CtxNav` 失效。 |
| 行数据 | `bridge/rows/` ↔ `FileRow` | 分区头行 `kind = -1` 且路径为空，`row_path` 返回 `None`；缩略图/批注键依赖这一点。 |
| i18n | `gui/src/i18n/` ↔ 6 个 `.toml` | 键名双向对应；缺键回退英文，不允许硬编码用户可见文案。 |
| 路径与原子写 | `core/src/paths.rs` | 所有配置/状态写入走 `write_atomic` 与 `preserve_unreadable`。 |
| 平台分支 | 各 `win*.rs` / `#[cfg]` | 两平台都必须能编译；Windows 专属代码放 `favnyr-gui/src/win*.rs` 或模块内的平台文件。 |
| 桥接对外路径 | `crate::bridge::…` | `bridge/mod.rs` 用 `pub use` 重导出子模块符号；`main.rs` 使用的 `install`/`persist_window_size`/`suppress_workspace_persist`/`show_workspace_not_found_notice` 保持可用。 |
| vendor 补丁 | `vendor/parley` | 只在必要时打补丁，并记录在 `PATCHES.md`。 |

## 6. 拆分前基线与剩余项

拆分前（2026-09-28 早，HEAD `bea5195`）的 13 个超 800 行文件与现状：

| 拆分前 | 行数 | 现状 |
| --- | ---: | --- |
| `favnyr-gui/src/bridge.rs` | 19 001 | ✅ `bridge/` 62 文件（`mod.rs` 151 行） |
| `favnyr-gui/src/ui/main_window.slint` | 15 897 | 🟡 36 个 .slint；`main_window.slint` 3 010 行（覆盖层已全部提取，只剩契约面 + 骨架 + 转发；侧栏列另提为 `sidebar/column.slint`），设置面板三页另在 `overlays/settings/`，面板内部另有一层 `panel/`（2 897 → 外壳 732 + 6 个组件），侧栏拆为外壳 + `sidebar/{column,item,fav,rail}`（1 113 → 287 + 4 个组件），行形状拆为 `widgets/rows.slint` + `widgets/rows/{header,marks}`（922 → 596 + 2 个组件） |
| `favnyr-core/src/fs.rs` | 1 919 | ✅ `fs/` 7 文件（`tests.rs` 621 + `tests/sorting.rs` 189） |
| `favnyr-core/src/fs/ops.rs` | 1 834 | ✅ `fs/ops/` 8 文件 |
| `favnyr-core/src/places.rs` | 1 771 | ✅ `places/` 4 文件 |
| `favnyr-core/src/thumbnail.rs` | 1 733 | ✅ `thumbnail/` 8 文件 |
| `favnyr-gui/src/actions.rs` | 1 563 | ✅ `actions/` 10 文件（按功能分，平台 `#[cfg]` 就地保留） |
| `favnyr-gui/src/openwith.rs` | 1 386 | ✅ `openwith/` 4 文件（`#[path]` 挂 `imp`） |
| `favnyr-core/src/openers.rs` | 1 268 | ✅ `openers/` 5 文件 |
| `favnyr-core/src/layout.rs` | 1 140 | ✅ `layout/` 4 文件 |
| `favnyr-gui/src/winddrag.rs` | 1 046 | ✅ `winddrag/` 7 文件 |
| `favnyr-core/src/workspace.rs` | 1 026 | ✅ `workspace/` 4 文件 |
| `favnyr-gui/src/i18n.rs` | 994 | ✅ `i18n/` 6 文件 |

**仍超 800 行的文件**（有意保留，原因如下）：

| 文件 | 行数 | 原因 / 后续 |
| --- | ---: | --- |
| `ui/main_window.slint` | 3 010 | 518 个契约成员（约 1 000 行）+ 布局骨架 + 转发（覆盖层 552 行 + 侧栏列 60 行）。成员声明搬不走（Slint 无部分组件 / 无 include，`global` 又读不到实例属性），转发是显式契约，不再机械下推；剩下的布局子树只有 `panels-container`（434 行 / 156 个 `root.` 名，两个面板实例 + 分隔条），照侧栏列的同一套做法还能再降约 180 行，但要新增 150 个左右的成员转发，收益/风险不划算，2026-09-29 决定停在侧栏列 |

**第二批落地的 5 个文件**（同日继续，全部逐行校验的机械搬移，行为与契约不变）：

| 拆分前 | 行数 | 现状 |
| --- | ---: | --- |
| `bridge/tests.rs` | 2 346 | ✅ `tests.rs` 790（共享 helper + 33 个用例）+ `tests/` 7 个分节模块 |
| `bridge/state.rs` | 1 508 | ✅ `state.rs` 674（`AppState` 外壳）+ `state/` 5 个类型模块（`tab`/`panel`/`clipdrop`/`nav`/`opreg`） |
| `bridge/thumbs.rs` | 862 | ✅ `thumbs.rs` 409（两个决策点 + worker 接线）+ `thumbs/scheduler.rs` 462 |
| `bridge/rows/build.rs` | 855 | ✅ `build.rs` 603 + `build/entry.rs` 114 + `build/subscan.rs` 146 |
| `core/src/fs/tests.rs` | 807 | ✅ `tests.rs` 621 + `tests/sorting.rs` 189 |

至此除上表的 `main_window.slint`（`vendor/parley` 不计入），仓库内**已没有超过 800 行的代码文件**。
