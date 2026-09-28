//! Bridge between Rust state and the Slint UI.
//!
//! Responsibilities:
//!   - filesystem navigation (open a folder, parent,
//!     home, back/forward via history),
//!   - column sorting (click on header, cyclic asc/desc),
//!   - refresh (F5 / button),
//!   - `notify` watcher on the current folder (auto-refresh),
//!   - preference persistence (language, theme, window size).
//!
//! Fast local listings can be handled on the UI thread. The
//! initial population, network paths, and expensive work (thumbnails,
//! recursive mtime, image metadata) are offloaded to background threads.

use std::cell::Cell;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use slint::{
    ComponentHandle, FilterModel, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer,
    SharedString, VecModel,
};
use tracing::{debug, error, info, warn};

use favnyr_core::SidebarSection;
use favnyr_core::favorites::{self, FlatFav};
use favnyr_core::fs as rfs;
use favnyr_core::fs::ops::{self};
use favnyr_core::fs::{Category, Entry, FileKind, GroupMode, SortColumn, SortOrder};
use favnyr_core::layout::{Layout, LayoutNode, NodePath, Rect, SplitDir};
use favnyr_core::openers;
use favnyr_core::shortcuts::{self, Chord};
use favnyr_core::thumbnail::{self, Thumbnail};
use favnyr_core::workspace::{self, PanelState, SidebarSectionsState, TabState, WorkspaceState};
use favnyr_core::{Config, Lang, Theme, paths};

use crate::actions;
use crate::clipboard;
use crate::i18n;
use crate::openwith;
use favnyr_core::columns::{self, ColumnSpec};

use crate::{
    ColumnInfo, Crumb, CtxNav, FavNode, FileRow, MainWindow, MenuShortcuts, OpProgress, OpenerItem,
    OrphanRow, OwRecipe, PanelBox, PanelView, ShellCtxEntry, ShellExtRow, ShortcutCap,
    ShortcutGroup, ShortcutRow, SidebarPlace, SplitterView, TabInfo, WorkspaceEntry,
};

// ---------- Callback installation ----------

/// Defers `f` to the next tick of the Slint event loop (single-shot 0 ms Timer).
/// Avoids "Recursion detected" re-entrancy when modifying a model from
/// one of its item's callbacks. Doesn't require `Send` (unlike
/// `invoke_from_event_loop`), so it's compatible with `AppState` (`Rc<RefCell>`).
fn defer(f: impl FnOnce() + 'static) {
    slint::Timer::single_shot(std::time::Duration::from_millis(0), f);
}

mod install;
pub use install::*;

mod workspaces;
use workspaces::*;

mod notices;
pub use notices::*;

mod settings;
use settings::*;

mod drives;
use drives::*;

mod datafiles;
use datafiles::*;

mod open_with;
use open_with::*;

mod favpanel;
use favpanel::*;

mod nav;
use nav::*;

mod tabs;
pub use tabs::*;

mod naming;
use naming::*;

mod paste;
use paste::*;

mod progress;
use progress::*;

mod listing;
use listing::*;

mod geometry;
use geometry::*;

mod keys;
use keys::*;

mod thumbs;
use thumbs::*;

mod stats;
use stats::*;

mod rows;
use rows::*;

mod selection;
use selection::*;

// The cross-view insertion index comes from the exact gap claimed on the Slint side
// on the real geometry, then read via `drag-target-gap`.

mod language;
use language::*;

mod restore;
use restore::*;

mod tabstrip;
use tabstrip::*;

mod watcher;
use watcher::*;

mod window;
pub use window::*;

mod colinfo;
use colinfo::*;

mod state;
pub use state::*;

#[cfg(test)]
mod tests;
