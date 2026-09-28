use super::*;

/// Resolves the symbolic links of `p`'s **parent** (which still exists even
/// if `p` has been trashed) then rejoins the file name. Used to match the
/// canonicalized original path stored by the trash. If the parent is
/// missing/unresolvable, returns `p` unchanged (safe fallback).
#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
pub(super) fn resolve_parent_symlinks(p: &Path) -> PathBuf {
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            std::fs::canonicalize(parent)
                .map(|cp| cp.join(name))
                .unwrap_or_else(|_| p.to_path_buf())
        }
        _ => p.to_path_buf(),
    }
}

/// Lexical comparison following the platform's usual rules, without
/// `canonicalize` or disk access (hence safe on a slow network share).
pub fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Is `path` `ancestor` itself, or somewhere inside it? Compares component by
/// component, so a shared spelling prefix is never mistaken for containment
/// (`/ab/c` is not inside `/a`), and follows the platform's case rules like
/// [`paths_equal`]. Lexical: no `canonicalize`, hence safe on a slow share.
///
/// The rule an operation needs before copying or moving an item: a destination
/// inside its own source makes the walk keep meeting the copy it has just
/// created, and recurse until the path length or the disk gives out.
pub fn is_within(path: &Path, ancestor: &Path) -> bool {
    let mut here = path.components();
    for expected in ancestor.components() {
        let Some(actual) = here.next() else {
            return false; // `path` is shorter: it cannot contain `ancestor`
        };
        #[cfg(windows)]
        let same = actual
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy());
        #[cfg(not(windows))]
        let same = actual == expected;
        if !same {
            return false;
        }
    }
    true
}
