//! System actions (desktop integration) for the context menu — **cross-platform**.
//!
//! Integrations favor local crates and OS APIs:
//!   - `open_path` / `open_parent`: `open` crate (xdg-open / ShellExecute);
//!     `open_parent` also reveals the item (`explorer /select,` on Windows).
//!   - `open_terminal`: launches a terminal with the desired `cwd` (known list
//!     on Linux; Windows Terminal then `cmd` on Windows).
//!   - `copy_to_clipboard`: `arboard` crate (Win / X11 /
//!     Wayland), via a persistent instance (see note below).
//!
//! All actions are **fire-and-forget**: we log the error if one
//! occurs and don't propagate it, so the Slint UI stays responsive.

use std::cell::RefCell;
#[cfg(windows)]
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, anyhow};
use favnyr_core::openers::{Opener, TagContext};
use tracing::{debug, info};

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const SHELL_VERB_OPEN: &[u16] = &[b'o' as u16, b'p' as u16, b'e' as u16, b'n' as u16, 0];
#[cfg(windows)]
const SHELL_VERB_RUNAS: &[u16] = &[
    b'r' as u16,
    b'u' as u16,
    b'n' as u16,
    b'a' as u16,
    b's' as u16,
    0,
];

// The `arboard` clipboard must stay ALIVE to serve the selection on
// X11/Wayland (the content is served by the owning app). We therefore keep
// ONE instance per UI thread, reused on every copy/read, instead of
// creating a disposable one (which would lose the content on Wayland when dropped).
thread_local! {
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

fn with_clipboard<R>(
    f: impl FnOnce(&mut arboard::Clipboard) -> std::result::Result<R, arboard::Error>,
) -> Result<R> {
    CLIPBOARD.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().map_err(|e| anyhow!("init clipboard: {e}"))?);
        }
        // `unwrap` is safe: we just guaranteed `Some`.
        f(slot.as_mut().unwrap()).map_err(|e| anyhow!("clipboard: {e}"))
    })
}

// Reading the clipboard used to live here, for the URL bar's "Paste". That
// menu now goes through the field's own paste, which drops the text at the
// caret instead of replacing the whole line — so nothing reads the clipboard
// from this side any more. Writing it still does, just below.

/// Pushes `text` into the clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    debug!(len = text.len(), "copy to clipboard");
    with_clipboard(|c| c.set_text(text.to_owned()))
}

mod ffmpeg;
mod opening;
mod program;
mod properties;
mod shell;
mod spawn;
mod terminal;
mod timezone;

#[cfg(test)]
mod tests;

pub use ffmpeg::*;
pub use opening::*;
pub use program::*;
pub use properties::*;
pub use shell::*;
pub use spawn::*;
pub use terminal::*;
pub use timezone::*;
