//! File and folder operations.
//!
//! Covers:
//!   - copy / move (recursive for folders),
//!   - in-place rename,
//!   - duplication (generates a unique suffix `name (1).ext`, `name (2).ext`, …),
//!   - **cross-platform** trashing via the `trash` crate (FreeDesktop
//!     on Linux, Recycle Bin via `IFileOperation` on Windows),
//!   - permanent deletion (recursive, be careful),
//!   - reading detailed properties (size, dates, permissions).
//!
//! Error policy: returns `Result<…, std::io::Error>` when possible,
//! otherwise an `Error::Workspace` (see `crate::error::Error`). Operations
//! are intentionally **simple and synchronous**: the bridge layer decides
//! whether to push them onto a background thread (`std::thread`).

// Windows copies via `CopyFileExW` → the read/write loop only exists elsewhere.
#[cfg(not(windows))]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::Result;
use crate::error::Error;

/// Status of a long-running, cancellable operation (see [`copy_tree_progress`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    Done,
    Cancelled,
}

mod copy;
mod deletion;
mod link;
mod name;
mod path_eq;
mod rename;

#[cfg(test)]
mod tests;

pub use copy::*;
pub use deletion::*;
pub use link::*;
pub use name::*;
pub use path_eq::*;
pub use rename::*;
