use super::*;

// ---------- Deletion ----------

/// Result of a trash request.
///
/// Windows network shares have no recycle bin: after GUI confirmation, the
/// same route then performs a permanent deletion. This distinction prevents
/// the calling layer from offering a misleading Ctrl+Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashDisposition {
    Trashed,
    PermanentlyDeleted,
}

/// Permanent deletion, recursive for a REAL folder. **No recycle bin.**
///
/// A reparse point — a symlink OR a Windows junction — is removed as a LINK and
/// never recursed into (recursing would delete the TARGET's contents). A
/// *directory* reparse point needs `remove_dir` on Windows: `remove_file` fails
/// on it (the bug this guards against), while `remove_dir_all` would follow it
/// into the target. A file symlink uses `remove_file`. On Unix, `remove_file`
/// unlinks any symlink and `symlink_metadata` never reports a symlink as a dir.
pub fn permanent_delete(path: &Path) -> Result<()> {
    let md = std::fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        let attrs = md.file_attributes();
        let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            // Symlink/junction → drop the link itself, never its target.
            if is_dir {
                std::fs::remove_dir(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
        } else if is_dir {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
    }
    #[cfg(not(windows))]
    {
        if md.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            // Regular file OR any symlink (unlinked, target untouched).
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Sends a file/folder to the trash — **cross-platform** via the `trash`
/// crate:
///   - **Linux**: FreeDesktop spec (`~/.local/share/Trash`, + volume
///     support), without depending on an external tool like `gio`.
///   - **Windows**: Recycle Bin via `IFileOperation`.
///
/// On failure (trash unavailable, volume without a recycle bin…), the error
/// is propagated; the caller can then offer permanent deletion with
/// confirmation.
pub fn trash_with_disposition(path: &Path) -> Result<TrashDisposition> {
    // Windows network volumes have no recycle bin. The GUI therefore confirms
    // permanent deletion before this call. `IFileOperation` is avoided for these
    // paths: verbatim UNC paths are not reliably accepted, and its modal
    // warning cannot be attached to the window from the operation thread.
    #[cfg(windows)]
    if crate::places::is_network_path(path) {
        permanent_delete(path)?;
        return Ok(TrashDisposition::PermanentlyDeleted);
    }
    trash::delete(path)
        .map(|()| TrashDisposition::Trashed)
        .map_err(|e| Error::Workspace(format!("trash {}: {e}", path.display())))
}

/// Legacy variant preserving the old unit signature.
pub fn trash(path: &Path) -> Result<()> {
    trash_with_disposition(path).map(|_| ())
}

/// Data-only failure reason for [`restore_from_trash`]. Carries no
/// natural-language text on purpose: the GUI translates each variant via its
/// i18n layer before showing it to the user. `detail`, when present, is the
/// underlying `trash` crate's own (English) message — arbitrary text outside
/// Favnyr's control, passed through as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashError {
    /// Could not enumerate the OS trash.
    ListFailed(String),
    /// No trashed item matches the requested original path.
    ItemNotFound,
    /// The OS refused to restore the matched item.
    RestoreFailed(String),
    /// No implementation for this OS.
    PlatformUnsupported,
}

/// Restores from the trash the most recently deleted item whose original
/// path matches `original_path` exactly.
///
/// `trash::delete` does not return the identifier created by the OS. Favnyr's
/// lightweight history is therefore associated with the original path, and
/// the most recent occurrence is picked here. The operation is available on
/// Windows and on FreeDesktop Linux environments, without an external tool.
#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
pub fn restore_from_trash(original_path: &Path) -> std::result::Result<(), TrashError> {
    let items = trash::os_limited::list().map_err(|e| TrashError::ListFailed(e.to_string()))?;
    // The FreeDesktop trash (Linux) records the **canonicalized** original
    // path (symbolic links resolved — e.g. `/home` → `/var/home` on Fedora
    // Atomic), whereas Favnyr recorded the navigation path as-is. A version
    // whose PARENT (which still exists after trashing) has its links resolved
    // is therefore also accepted. The raw match is still tried first
    // (Windows: Recycle Bin already returns the clean path).
    let resolved = resolve_parent_symlinks(original_path);
    let item = items
        .into_iter()
        .filter(|item| {
            let ip = item.original_path();
            paths_equal(&ip, original_path) || paths_equal(&ip, &resolved)
        })
        .max_by_key(|item| item.time_deleted)
        .ok_or(TrashError::ItemNotFound)?;
    trash::os_limited::restore_all(std::iter::once(item))
        .map_err(|e| TrashError::RestoreFailed(e.to_string()))
}

/// Platforms without a restore API exposed by `trash::os_limited`.
#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn restore_from_trash(_original_path: &Path) -> std::result::Result<(), TrashError> {
    Err(TrashError::PlatformUnsupported)
}

/// Empties the trash (**permanent** deletion of all its contents).
/// Cross-OS via `trash::os_limited` (FreeDesktop on Linux, Recycle Bin on
/// Windows). **Destructive**: the caller must confirm before calling.
/// Log-only failure (never shown in the UI as-is): the message stays in
/// English on purpose (`favnyr-core` has no i18n of its own).
#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
pub fn empty_trash() -> Result<()> {
    let items = trash::os_limited::list()
        .map_err(|e| Error::Workspace(format!("listing the trash: {e}")))?;
    trash::os_limited::purge_all(items)
        .map_err(|e| Error::Workspace(format!("emptying the trash: {e}")))
}

/// Platforms without `os_limited` support (e.g. macOS): not implemented.
#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "macos")))))]
pub fn empty_trash() -> Result<()> {
    Err(Error::Workspace(
        "empty_trash is not supported on this platform".into(),
    ))
}
