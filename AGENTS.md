# AGENTS.md — Favnyr

面向在本仓库工作的 AI 助手与贡献者的规范。与用户级全局规范冲突时，以本文件（更具体）为准。

## 项目概览

Favnyr 是一个便携式文件浏览器（Windows / Linux），Rust + Slint 编写：单一可执行文件、免安装、无网络请求、无遥测。

- `crates/favnyr-core` — 与 UI 无关的核心逻辑（文件系统、缩略图、配置、打开方式、工作区、i18n…）。
- `crates/favnyr-gui` — Slint 界面与平台集成（`winthumb.rs`、`shellmenu.rs`、`winutil.rs` 等）。
- `crates/favnyr-gui/src/ui/main_window.slint` — 界面与数据模型定义。

## 构建与测试

- 构建运行：`cargo run --release --bin favnyr`
- 单元测试：`cargo test -p favnyr-core -p favnyr-gui`
- 交付前保证测试全绿；CI 把警告当错误（见 `rust-toolchain.toml`、`deny.toml`）。
- 版本号在 workspace 的 `Cargo.toml`；发布以 git tag 表达。

## 代码规范

- **注释与文档字符串用英文**，与现有代码保持一致（`favnyr-core`/`favnyr-gui` 现有注释全为英文）。如需改用中文注释，先与维护者确认。
- 面向用户的文案必须走 i18n（`i18n.rs`），不要硬编码字符串。
- 平台差异用 `#[cfg(windows)]` / `#[cfg(not(windows))]` 就地处理，保证两个平台都能编译。
- Windows 专属代码放在 `favnyr-gui/src/win*.rs`；新增 Windows API 依赖时，在 `crates/favnyr-gui/Cargo.toml` 的 `[target.'cfg(windows)'.dependencies]` 下按需精确开启 feature。
- 优先使用官方 / 系统提供的实现，避免自造（例如缩略图直接调用系统 shell API）。
- 修改 `FileKind` 这类 `#[repr(u8)]` 取值是**破坏性**的：`.slint` 侧按数值引用，禁止重排序。需要“数值 → 枚举”时用 `FileKind::from_code`。

## 缩略图（预览）

两个决策点都在 `crates/favnyr-gui/src/bridge.rs`：

- `thumbnail_kind_for_row` — 决定**哪些行请求**预览。Windows：所有非文件夹文件（由系统决定有无缩略图）；其它平台：image / video / MP3-FLAC / PDF 白名单。其结果同时驱动 `preview_capable` 与行高。
- `generate_thumb` — 决定**来源**。Windows：先系统 shell 缩略图，系统给不出时才回退自研解码（PDF 以 WinRT 兜底）；Linux：一律自研解码。

Windows 路径见 `crates/favnyr-gui/src/winthumb.rs`（`IShellItemImageFactory`，即系统缩略图缓存）；自研解码见 `crates/favnyr-core/src/thumbnail.rs`。完整说明见 `docs/thumbnails.md`。

## 文档与记录

- 设计方案 / 规划写成**新文档**放在 `docs/` 下，**不覆盖**既有文档。
- 每次代码改动后，在 `docs/CHANGELOG.md` 末尾追加一条记录（日期 + 变更摘要 + 涉及文件）。
- 本文件随代码演进同步更新，避免规范过时。

## 交流

- 与用户对话使用中文。
