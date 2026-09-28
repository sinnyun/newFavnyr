use super::*;

// ---------- Renaming ----------

/// Renames `from` to `new_name` (without changing folder). `new_name` must
/// be a **file name**, not a path (safety: any path component is rejected
/// to avoid unintended moves).
///
/// **Both** separators `/` and `\` are rejected regardless of OS: on
/// Windows `MAIN_SEPARATOR` is `\`, but `/` is also a valid separator there
/// → testing only `MAIN_SEPARATOR` would let `foo/bar` through (a move).
pub fn rename_in_place(from: &Path, new_name: &str) -> Result<PathBuf> {
    if let Err(reason) = check_file_name(new_name) {
        return Err(Error::Workspace(format!(
            "invalid new name: {new_name:?} ({reason:?})"
        )));
    }
    let parent = from
        .parent()
        .ok_or_else(|| Error::Workspace(format!("no parent for {}", from.display())))?;
    let to = parent.join(new_name);
    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Renames `from`, replacing the existing **file** of the same name.
///
/// The replacement is deliberately limited to files (and links treated as
/// files): recursively merging/replacing two folders under the guise of a
/// simple rename would be a far more ambiguous and destructive operation. The
/// replacement is delegated to the OS's atomic primitive (`rename` on Unix /
/// `MoveFileExW` on Windows), never the dangerous `remove_file` then `rename`
/// sequence.
pub fn rename_in_place_replacing_file(from: &Path, new_name: &str) -> Result<PathBuf> {
    if let Err(reason) = check_file_name(new_name) {
        return Err(Error::Workspace(format!(
            "invalid new name: {new_name:?} ({reason:?})"
        )));
    }
    let parent = from
        .parent()
        .ok_or_else(|| Error::Workspace(format!("no parent for {}", from.display())))?;
    let to = parent.join(new_name);
    if from == to {
        return Ok(to);
    }

    let source_meta = std::fs::symlink_metadata(from)?;
    let target_meta = std::fs::symlink_metadata(&to)?;
    if source_meta.is_dir() || target_meta.is_dir() {
        return Err(Error::Workspace(
            "force replace is limited to files".to_string(),
        ));
    }

    std::fs::rename(from, &to)?;
    Ok(to)
}

/// Universal **name** validator (not a path): non-empty, no `/` or `\`
/// separator (rejected on all OSes, see the note on `rename_in_place`), and
/// different from `.` / `..`. Deliberately independent of the filesystem
/// (does not check existence) → reusable anywhere a name is entered:
/// creating/renaming folders and files, **and** bookmark labels
/// (containers / bookmarks) — see the naming popup. The caller adds an
/// existence check where relevant (e.g. `create_entry`).
pub fn is_valid_entry_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}
