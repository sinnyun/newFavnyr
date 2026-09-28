# Main Window Architecture Implementation Plan

> **For agentic workers:** Execute this plan in order and keep each domain migration independently buildable.

**Goal:** Reduce `main_window.slint` and every new Slint interface file below 800 lines by moving Rust-facing state and callbacks into small, domain-owned interfaces, while preserving Favnyr behavior.

**Architecture:** Rust remains the owner of application and filesystem state. Exported Slint globals provide typed per-window bridge interfaces grouped by domain; UI components keep short-lived gesture and presentation state locally and call domain callbacks. `MainWindow` becomes the window shell, focus coordinator, and component composition point. Rust bridge installers use the appropriate generated global API instead of a single component with hundreds of members.

**Tech Stack:** Slint 1.16.1, Rust 2024 workspace, existing `slint::include_modules!()` build.

## Global Constraints

- Preserve user-visible behavior, i18n keys, `FileKind` numeric values, settings keys, and existing Rust domain logic.
- Keep user-facing strings in `crates/favnyr-gui/src/i18n/`.
- Keep every owned `.slint` file below 800 lines; target 250–650 lines for new interface files.
- Keep globals per-window and avoid storing handles that keep a window alive.
- Keep all source comments and documentation strings in English.
- Append a dated entry to `docs/CHANGELOG.md` after code changes and update `docs/code-map.md` and `docs/split-plan.md` to reflect the new design.

---

## File and Interface Map

- `crates/favnyr-gui/src/ui/api/application.slint`: app strings, platform/configuration inputs, theme and application-level actions.
- `crates/favnyr-gui/src/ui/api/panels.slint`: panel and tab models, list/column state, panel navigation and rendering callbacks.
- `crates/favnyr-gui/src/ui/api/sidebar.slint`: Places and Favorites data, collapse/reorder state, and sidebar callbacks.
- `crates/favnyr-gui/src/ui/api/operations.slint`: file operations, confirmations, clipboard/paste state, operation progress inputs and callbacks.
- `crates/favnyr-gui/src/ui/api/overlays.slint`: settings, workspace, naming, shortcut capture, context-menu and notice contracts, grouped into separate globals if any file would exceed 650 lines.
- `crates/favnyr-gui/src/ui/api/drag_drop.slint`: view, file, tab, favorite and external drag state/callbacks shared across components.
- `crates/favnyr-gui/src/ui/main_window.slint`: re-export generated APIs for Rust, retain `MainWindow`, window sizing/theme setup, root focus coordination, top-level layout and overlay ordering.
- `crates/favnyr-gui/src/ui/window/keyboard.slint`: focus scope and keyboard dispatch, with action resolution delegated to the relevant API callbacks.
- `crates/favnyr-gui/src/bridge/install/`: migrate callback registration and property initialization from `MainWindow` methods to the owning `window.global::<...>()` APIs, preserving the existing domain module boundaries.
- `crates/favnyr-gui/src/bridge/*.rs`: replace remaining component getter/setter/callback access with the owning global; preserve current behavior and Rust state ownership.
- `docs/code-map.md`, `docs/split-plan.md`, `docs/CHANGELOG.md`: document the new interface map, migration record and file counts.

Global names are finalized from the existing member inventory before each migration. A member belongs to the domain that owns its meaning and lifecycle, not merely the component that currently displays it. UI-only state stays on its component. Cross-domain events are exposed as callbacks on the owning domain API; avoid duplicating mutable values across globals.

## Task 1: Establish Baseline and Interface Inventory

**Files:** No production files. Record inventory in this plan during execution.

- [ ] Run `cargo test -p favnyr-core -p favnyr-gui` on the unmodified checkout and record test totals and any pre-existing failures.
- [ ] Enumerate each public `MainWindow` property/callback/function with its Rust read/write sites and Slint component consumers.
- [ ] Assign each member one owner from application, panels, sidebar, operations, overlays, drag/drop, or UI-local state.
- [ ] Identify members that expose element geometry or focus and keep those in a UI component interface rather than a `global`.
- [ ] Confirm each planned interface file remains below 650 lines before moving members.

## Task 2: Add the Domain API Modules

**Files:**

- Create `crates/favnyr-gui/src/ui/api/application.slint`.
- Create `crates/favnyr-gui/src/ui/api/panels.slint`.
- Create `crates/favnyr-gui/src/ui/api/sidebar.slint`.
- Create `crates/favnyr-gui/src/ui/api/operations.slint`.
- Create `crates/favnyr-gui/src/ui/api/overlays.slint` or smaller overlay files when required by the line limit.
- Create `crates/favnyr-gui/src/ui/api/drag_drop.slint`.
- Modify `crates/favnyr-gui/src/ui/main_window.slint` only to import/re-export the API types during this task.

- [ ] Declare exported, typed globals containing only the inventory members assigned to each domain.
- [ ] Preserve property direction (`in`, `out`, `in-out`) and callback signatures exactly during the first move.
- [ ] Re-export each Rust-facing global from `main_window.slint`, following the existing `CtxNav` export pattern.
- [ ] Build with `cargo check -p favnyr-gui --tests` before migrating consumers.

## Task 3: Migrate Rust Bridge Access by Domain

**Files:**

- Modify the corresponding `crates/favnyr-gui/src/bridge/install/*.rs` modules.
- Modify domain bridge modules that currently read/write `MainWindow` members.

- [ ] Start with application settings and configuration properties, changing Rust calls to `window.global::<ApplicationApi>()`.
- [ ] Migrate panel, sidebar, operations, overlay and drag/drop property access one domain at a time.
- [ ] Register each callback on the global that declares it and retain weak-handle capture patterns where the code schedules work beyond the immediate callback.
- [ ] After each domain, run `cargo check -p favnyr-gui --tests` and `cargo test -p favnyr-gui`.
- [ ] Remove a domain's old `MainWindow` declarations only after `rg` confirms there are no Rust or Slint consumers left on the old names.

## Task 4: Migrate Slint Consumers and Extract Root Interaction Logic

**Files:**

- Modify `crates/favnyr-gui/src/ui/main_window.slint` and affected `ui/panel*.slint`, `ui/sidebar/*.slint`, `ui/overlays/*.slint`, `ui/workspaces.slint`, and `ui/progress.slint`.
- Create `crates/favnyr-gui/src/ui/window/keyboard.slint`.

- [ ] Replace consumer reads of `root.domain-member` with the owning imported global, leaving component-local presentation and gesture state private.
- [ ] Move keyboard event handling into `WindowKeyboard`; keep focus acquisition/restoration tied to the actual window and text-field lifecycle.
- [ ] Move timers to the component that owns the hover/drag state they observe; keep only window-wide timers in `MainWindow` when they depend on window activation.
- [ ] Keep layout geometry and absolute pointer coordinate conversion in UI components; communicate resulting actions through callbacks.
- [ ] Preserve the existing z-order of panels, popups, drag feedback, note bubble and global tooltip.
- [ ] Run `cargo check -p favnyr-gui --tests` after each cohesive area.

## Task 5: Reduce MainWindow to the Window Shell

**Files:**

- Modify `crates/favnyr-gui/src/ui/main_window.slint`.
- Modify API files only where interface ownership needs a final correction.

- [ ] Remove migrated root properties, callbacks and functions instead of keeping a second facade that would recreate the oversized contract.
- [ ] Keep window title/icon/size, theme initialization, root focus coordination, top-level component composition and required Rust-facing exports.
- [ ] Keep `main_window.slint` below 650 lines to leave room for future window-level behavior.
- [ ] Confirm every `ui/api/*.slint` and `ui/window/*.slint` file is below 800 lines.
- [ ] Confirm all formerly public members are either migrated to a domain API or intentionally private to a component.

## Task 6: Verify and Update Repository Documentation

**Files:**

- Modify `docs/code-map.md`.
- Modify `docs/split-plan.md`.
- Append to `docs/CHANGELOG.md`.

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `cargo test -p favnyr-core -p favnyr-gui`.
- [ ] Run a repository line-count check excluding `vendor/`, `target/`, and `.git/`; require every owned `.slint` file to be below 800 lines.
- [ ] Launch the GUI and inspect settings, workspace loading, panel navigation, keyboard shortcuts, favorites, file operation dialogs, tab tear-off, internal/external drag and drop, progress notices, and tooltip stacking.
- [ ] Update the code map's file tree, dependency direction and Rust API description; replace the old statement that the `MainWindow` contract must remain intact.
- [ ] Record the dated change, affected files and verification results in `docs/CHANGELOG.md`.
