//! Serialization / deserialization of the workspace state.
//!
//! The workspace captures the **session layout**: the panel tree
//! (with their width ratios), the tabs open in each one
//! (current path + sort order), the active tab and panel, as well as
//! the collapsed state of the left sidebar sections. Their order is
//! a global preference kept in `config.toml`.
//!
//! Stored as TOML in `$XDG_DATA_HOME/favnyr/workspace.toml` (see
//! `paths::workspace_path`). The window size, meanwhile, stays in
//! `config.toml` (a cosmetic preference persisted by `persist_window_size`).
//!
//! The **selection** and **history** (back/forward) are NOT persisted:
//! they are volatile session states, with no restoration value.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::fs::{GroupMode, SortColumn, SortOrder};
use crate::layout::LayoutNode;

mod named;
mod state;

#[cfg(test)]
mod tests;

pub use named::*;
pub use state::*;
