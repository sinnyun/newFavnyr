use super::*;

/// A single bounded walk over `path` computing BOTH its recursive **max mtime**
/// (the most recent modification date among the folder and its descendants, up
/// to `mtime_depth` levels) AND its recursive **total size** (sum of descendant
/// files' bytes, up to `size_depth` levels). `depth = 1` = direct children, `2`
/// = grandchildren, …; a depth of `0` DISABLES that metric (its result is
/// `None`). Doing both in one traversal costs a single `stat` per entry instead
/// of two — the reason the two "folder date"/"folder size" options share it.
///
/// Symbolic links are NOT followed (`DirEntry::metadata`) → no loops or runaway
/// cost; only regular FILES add to the size. Errors (permission, broken link)
/// are ignored: a partial result beats none.
pub fn recursive_folder_stats(
    path: &Path,
    mtime_depth: u32,
    size_depth: u32,
) -> (Option<i64>, Option<u64>) {
    fn to_unix(md: &std::fs::Metadata) -> Option<i64> {
        md.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    }
    fn walk(
        dir: &Path,
        mtime_left: u32,
        size_left: u32,
        max_mtime: &mut Option<i64>,
        total_size: &mut Option<u64>,
    ) {
        if mtime_left == 0 && size_left == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let Ok(md) = entry.metadata() else {
                continue; // does not follow symbolic links
            };
            if mtime_left > 0
                && let Some(m) = to_unix(&md)
                && max_mtime.is_none_or(|cur| m > cur)
            {
                *max_mtime = Some(m);
            }
            if size_left > 0
                && md.is_file()
                && let Some(total) = total_size.as_mut()
            {
                *total = total.saturating_add(md.len());
            }
            if md.is_dir() {
                walk(
                    &entry.path(),
                    mtime_left.saturating_sub(1),
                    size_left.saturating_sub(1),
                    max_mtime,
                    total_size,
                );
            }
        }
    }
    // Bases: the folder's own mtime; a directory has no intrinsic size, so the
    // size accumulator starts at 0 and only files add to it.
    let mut max_mtime = if mtime_depth > 0 {
        std::fs::metadata(path).ok().as_ref().and_then(to_unix)
    } else {
        None
    };
    let mut total_size = if size_depth > 0 { Some(0) } else { None };
    walk(
        path,
        mtime_depth,
        size_depth,
        &mut max_mtime,
        &mut total_size,
    );
    (max_mtime, total_size)
}

/// Recursive **max mtime** up to `depth` levels — the mtime-only case of
/// [`recursive_folder_stats`]. `depth = 0` = the folder's own mtime.
pub fn recursive_max_mtime(path: &Path, depth: u32) -> Option<i64> {
    // `recursive_folder_stats` treats an mtime depth of 0 as "disabled" (`None`),
    // but this function's historical contract is that depth 0 = the folder's own
    // mtime — handled directly here.
    if depth == 0 {
        return std::fs::metadata(path)
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
    }
    recursive_folder_stats(path, depth, 0).0
}
