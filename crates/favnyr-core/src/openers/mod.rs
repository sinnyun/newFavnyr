//! "Open with" — model for **openers**: with which program, and
//! how, to open a file. **Cross-platform, with NO OS or UI dependency.**
//!
//! The core does ONLY: modeling, TOML persistence (dedicated `openers.toml`, see
//! [`crate::paths::openers_path`]), **tag substitution**, and **validation**.
//! The process `spawn` and the native dialog live on the GUI side (`actions/`).
//!
//! Security principle: arguments are a **list** (`Vec<String>`), one
//! element = one argument. Substitution is token-by-token → a path with
//! spaces/metacharacters stays ONE opaque argument. The GUI passes the `argv`
//! as-is to `std::process::Command` (never a shell, never concatenation).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::error::Error;

/// Built-in icon attached to a command created from a ready-made recipe.
///
/// The value is persisted with the opener rather than inferred from its
/// executable path: users remain free to replace `7z` or `tar` with a wrapper
/// script without losing the recipe's visual identity. `None` keeps commands
/// created manually and older `openers.toml` files unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenerIcon {
    #[default]
    None,
    SevenZip,
    Archive,
    Email,
    Device,
}

impl OpenerIcon {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }

    /// Stable numeric value passed to Slint. Keep the matching UI component
    /// mapping in sync when adding a variant.
    pub fn as_i32(self) -> i32 {
        match self {
            Self::None => 0,
            Self::SevenZip => 1,
            Self::Archive => 2,
            Self::Email => 3,
            Self::Device => 4,
        }
    }

    pub fn from_i32(value: i32) -> Self {
        match value {
            1 => Self::SevenZip,
            2 => Self::Archive,
            3 => Self::Email,
            4 => Self::Device,
            _ => Self::None,
        }
    }
}

/// An "opener": a program + an argument template (with tags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opener {
    /// Stable identifier (generated).
    pub id: String,
    /// Displayed label ("Image Editor", "Text Editor"…).
    pub label: String,
    /// ABSOLUTE path of the executable (never interpreted by a shell). Can be
    /// empty if `assoc` is set (OS app with no directly launchable exe).
    pub program: String,
    /// Key of an OS association handler (Windows: ProgID/AUMID of a
    /// UWP/Store app; Linux: `.desktop` id) for apps WITHOUT a classic exe.
    /// If set, launching goes through the OS (`IAssocHandler::Invoke` / `.desktop`)
    /// rather than `Command` — see `favnyr-gui::actions::run_opener`.
    #[serde(default)]
    pub assoc: Option<String>,
    /// Visual family inherited from a ready-made recipe. It has no influence
    /// on execution and is omitted for ordinary custom commands.
    #[serde(default, skip_serializing_if = "OpenerIcon::is_none")]
    pub icon: OpenerIcon,
    /// Argument templates BEFORE substitution (one element = one argument).
    /// Empty → `{file}` added at execution time.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extensions (no dot, lowercase) for which THIS opener is the
    /// default **within Favnyr** (double-click / Enter), without touching the OS.
    #[serde(default)]
    pub default_exts: Vec<String>,
    /// Extensions (no dot, lowercase) **learned through use**: each time
    /// the opener is launched on a file, its extension is recorded here. Used
    /// to only offer, in the "Open with" flyout, programs that are ACTUALLY
    /// suited to the file's extension (no more text editor offered for a .mp4).
    #[serde(default)]
    pub used_exts: Vec<String>,
    /// Usage counter (MRU for "suggested applications").
    #[serde(default)]
    pub use_count: u32,
    /// Last used (epoch seconds; breaks ties in the MRU). 0 = never.
    #[serde(default)]
    pub last_used: u64,
    /// Launch the program as ADMINISTRATOR (UAC elevation via the
    /// "runas" verb) — **Windows only**. Ignored on other OSes (elevating
    /// a GUI app is not a standard mechanism there).
    #[serde(default)]
    pub elevated: bool,
    /// Pinning to the views' CONTEXT MENU: bitmask —
    /// 1 = right-click on a FILE, 2 = on a FOLDER, 4 = on the view's
    /// BACKGROUND (empty area; the command then targets the current folder).
    /// 0 = absent (default). Enables entries like "Git Bash here" without a
    /// shell extension.
    #[serde(default)]
    pub ctx_menu: u8,
    /// Extensions (no dot, lowercase) the FILE entry is restricted to, or
    /// [`CTX_EXT_ALL`] for every file. Only meaningful with [`CTX_FILE`] —
    /// folders and the view background have no extension.
    ///
    /// Empty means no restriction, so commands saved before this field existed
    /// keep showing up everywhere, as they did.
    ///
    /// Exists because an "Extract here" command is meaningless on a text file,
    /// yet used to appear on right-click for every file.
    #[serde(default)]
    pub ctx_exts: Vec<String>,
}

/// Bits of [`Opener::ctx_menu`].
pub const CTX_FILE: u8 = 1;
pub const CTX_DIR: u8 = 2;
pub const CTX_BACKGROUND: u8 = 4;

/// Wildcard accepted in [`Opener::ctx_exts`]: show the entry on every file.
pub const CTX_EXT_ALL: &str = "*";

mod model;
mod store;
mod tag;

#[cfg(test)]
mod tests;

use model::*;
pub use store::*;
pub use tag::*;
