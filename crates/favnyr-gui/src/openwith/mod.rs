//! "Open with" — enumeration of the OS's candidate applications.
//!
//! We build OUR OWN picker (Slint) instead of relying on the system
//! dialog: this lets us capture the user's choice, persist it as
//! [`favnyr_core::openers::Opener`], and replay it later.
//!
//! - **Windows**: `SHAssocEnumHandlers` (what Explorer itself uses) →
//!   `IAssocHandler` (display name, key, recommended); launched via `Invoke`.
//! - **Linux**: parsing XDG `.desktop` files (no dependency), filtered by type.
//!
//! Common neutral `AppHandler` type → the GUI never sees the OS difference.

use std::path::Path;

use anyhow::Result;
use favnyr_core::Lang;

use crate::i18n;

/// A candidate application, OS-neutral.
#[derive(Debug, Clone)]
pub struct AppHandler {
    /// Displayed name ("Image Editor", "Text Editor"…).
    pub name: String,
    /// Persistable key: exe path (classic apps) or identifier
    /// (Windows UWP / Linux `*.desktop`).
    pub key: String,
    /// Path of a directly launchable executable (if the key is one).
    /// `None` → relaunch via the OS path (Invoke / `.desktop`).
    pub exe: Option<String>,
    /// Recommended by the OS for this file type (shown first).
    pub recommended: bool,
}

// ===================== Windows =====================
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

// ===================== Linux =====================
#[cfg(not(windows))]
#[path = "linux.rs"]
mod imp;

#[cfg(test)]
mod tests;

/// Icon associated with `path` (RGBA pixels), or `None`.
pub fn icon_rgba(path: &str) -> Option<(Vec<u8>, u32, u32)> {
    imp::icon_rgba(path)
}

/// Starts the application a `.desktop` launcher describes. Linux desktops only:
/// Windows has no such file, and its shell already runs `.lnk` shortcuts.
#[cfg(not(windows))]
pub fn launch_desktop_file(path: &Path) -> Result<()> {
    imp::launch_desktop_file(path)
}

/// Path of the image a `.desktop` launcher declares, resolved through the icon
/// theme when it names one instead of pointing at a file.
#[cfg(not(windows))]
pub fn desktop_icon_path(path: &Path) -> Option<std::path::PathBuf> {
    imp::desktop_icon_path(path)
}

/// Resolves the target of a Windows `.lnk` shortcut (pointed-to path). `None`
/// outside Windows or if it's not a valid link. (Only called under
/// `#[cfg(windows)]`; the API stays symmetric across platforms.)
#[cfg_attr(not(windows), allow(dead_code))]
pub fn resolve_shortcut(path: &Path) -> Option<std::path::PathBuf> {
    imp::resolve_shortcut(path)
}

/// Creates a Windows `lnk_path` shortcut pointing to `target`. Errors outside
/// Windows (the caller only invokes this on Windows).
pub fn create_shortcut(lnk_path: &Path, target: &Path) -> Result<()> {
    imp::create_shortcut(lnk_path, target)
}

/// Native picker for a shortcut's target — FILE (`None` if cancelled/outside
/// Windows).
pub fn browse_for_target(lang: Lang) -> Option<String> {
    imp::browse_for_target(lang)
}

/// Native picker for a target FOLDER shortcut (`None` if cancelled/outside
/// Windows).
pub fn browse_for_folder() -> Option<String> {
    imp::browse_for_folder()
}

/// Icon of the default application ASSOCIATED WITH EXTENSION `ext` (no dot),
/// as RGBA pixels `(buf, w, h)` — WITHOUT touching disk. Used to show the OS's
/// real "file type" icon in the view (faster recognition);
/// result should be cached per extension. `big=false` → ~32 px (list mode);
/// `big=true` → ~256 px (preview mode, downscaled based on zoom). `None` if unavailable.
pub fn icon_rgba_for_ext(ext: &str, big: bool) -> Option<(Vec<u8>, u32, u32)> {
    imp::icon_rgba_for_ext(ext, big)
}

/// Icon SPECIFIC to file `path` (embedded resources) — for types where
/// each file carries its own icon (`.exe`…). `None` outside Windows.
pub fn icon_rgba_for_path(path: &str, big: bool) -> Option<(Vec<u8>, u32, u32)> {
    imp::icon_rgba_for_path(path, big)
}

/// Native file picker to choose an executable (`None` if cancelled/unavailable).
pub fn browse_for_exe(lang: Lang) -> Option<String> {
    imp::browse_for_exe(lang)
}

/// OS candidate applications for extension `ext` (no dot).
pub fn handlers_for_ext(ext: &str, include_without_mime: bool) -> Vec<AppHandler> {
    imp::handlers_for_ext(ext, include_without_mime)
}

/// Launches handler `key` on `path` (`ext` = the file's extension).
pub fn launch(key: &str, ext: &str, path: &Path) -> Result<()> {
    imp::launch(key, ext, path)
}
