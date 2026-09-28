use super::*;

// ---------- Copying ----------

/// **Windows**: attempts to RECREATE the symbolic link `src` → `dst`
/// (`symlink_dir` if the target is a folder, `symlink_file` otherwise), just
/// like the Unix branch. Returns `(recreated, target_is_dir)`. `recreated = false`
/// means creation failed — typically for lack of privilege (Windows symbolic
/// links require developer mode or admin, see `link_into`); the caller must
/// then materialize the target, recursively for a folder.
#[cfg(windows)]
fn try_clone_symlink(src: &Path, dst: &Path) -> (bool, bool) {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    // `metadata` FOLLOWS the link → type of the TARGET (broken link → treated as a file).
    let target_is_dir = std::fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false);
    let created = match std::fs::read_link(src) {
        Ok(target) => {
            if target_is_dir {
                symlink_dir(&target, dst).is_ok()
            } else {
                symlink_file(&target, dst).is_ok()
            }
        }
        Err(_) => false,
    };
    (created, target_is_dir)
}

/// Identity of a directory, as the copy walks use it to notice they are about
/// to enter one they are already inside.
///
/// A tree of real directories cannot loop on its own, but two things make one
/// appear: following a directory link that points back up (Windows
/// materialises such a link when it cannot recreate it), and a bind mount that
/// grafts a directory under itself (Linux). Both end as a directory the walk
/// has already entered, so both are caught the same way.
///
/// Unix answers with the device and inode, the identity the filesystem itself
/// uses. Elsewhere the resolved path stands in, which is what `canonicalize`
/// is for.
#[cfg(unix)]
type DirMark = (u64, u64);
#[cfg(not(unix))]
type DirMark = PathBuf;

#[cfg(unix)]
fn dir_mark(path: &Path) -> Option<DirMark> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|md| (md.dev(), md.ino()))
}

#[cfg(not(unix))]
fn dir_mark(path: &Path) -> Option<DirMark> {
    std::fs::canonicalize(path).ok()
}

/// The error a walk reports instead of descending into itself for ever.
pub(super) fn loops_back(path: &Path) -> Error {
    Error::Workspace(format!(
        "{} is already part of this copy; descending into it would never end",
        path.display()
    ))
}

/// Recursive copy of `src` to `dst`. `dst` must not exist.
/// For folders, recreates the tree; for files, `std::fs::copy`.
pub fn copy_path(src: &Path, dst: &Path) -> Result<()> {
    // Same reasoning as `copy_tree_progress`: only a followed directory link
    // can make the walk re-enter itself, so the chain of real directories
    // already entered is what stops it.
    let mut entered: Vec<DirMark> = Vec::new();
    copy_path_inner(src, dst, &mut entered)
}

fn copy_path_inner(src: &Path, dst: &Path, entered: &mut Vec<DirMark>) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)?;
    if metadata.file_type().is_symlink() {
        // Copy the link as-is (re-creation of the link).
        #[cfg(unix)]
        {
            let target = std::fs::read_link(src)?;
            std::os::unix::fs::symlink(target, dst)?;
            return Ok(());
        }
        #[cfg(windows)]
        {
            let (cloned, target_is_dir) = try_clone_symlink(src, dst);
            if cloned {
                return Ok(());
            }
            // Insufficient privilege → materialize the TARGET. For a folder,
            // recursively copy the RESOLVED target (canonicalize → real folder,
            // not a link → no infinite recursion). For a file, `fs::copy`
            // follows `src` and copies the target's content.
            if target_is_dir {
                // Materialising follows the link out of the tree being copied.
                // Where it lands is checked on entry like any other directory.
                return copy_path_inner(&std::fs::canonicalize(src)?, dst, entered);
            }
            std::fs::copy(src, dst)?;
            return Ok(());
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::fs::copy(src, dst)?;
            return Ok(());
        }
    }

    if metadata.is_dir() {
        let mark = dir_mark(src);
        if let Some(mark) = mark.as_ref()
            && entered.contains(mark)
        {
            return Err(loops_back(src));
        }
        std::fs::create_dir(dst)?;
        // Moved rather than cloned: the mark is a pair of integers on Unix and
        // a path elsewhere, and only one of the two would tolerate a clone.
        let marked = mark.is_some();
        if let Some(mark) = mark {
            entered.push(mark);
        }
        let walk = (|| -> Result<()> {
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                let src_child = entry.path();
                let dst_child = dst.join(entry.file_name());
                copy_path_inner(&src_child, &dst_child, entered)?;
            }
            Ok(())
        })();
        if marked {
            entered.pop();
        }
        walk?;
    } else {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

/// Recursive size (bytes) of a file/folder, used to pre-scan for a progress
/// bar. Symbolic links count as 0 (the link is recreated, not its target).
/// Best-effort: any access error counts as 0.
pub fn path_size(path: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if md.file_type().is_symlink() {
        0
    } else if md.is_dir() {
        let mut total = 0;
        if let Ok(rd) = std::fs::read_dir(path) {
            for entry in rd.flatten() {
                total += path_size(&entry.path());
            }
        }
        total
    } else {
        md.len()
    }
}

/// Recursive copy of `src` to `dst` (which must not exist) with:
///   - **byte reporting**: `on_bytes(delta)` is called as writing progresses,
///   - **cooperative cancellation**: `cancel()` is checked at file boundaries
///     AND during the copy of a large file (in chunks, see
///     `COPY_BUF_BYTES`).
///
/// If cancelled while writing a file, the partial file is deleted (a
/// truncated file is never left behind).
/// `on_skipped` is called for each entry the walk could not copy — a file held
/// by another program, one the account may not read. Such an entry is reported
/// and stepped over rather than ending the copy: its siblings have nothing to
/// do with it, and a folder of a thousand files should not be lost to one of
/// them. Only a failure on the root itself is returned as an error, since there
/// is then nothing to continue with.
pub fn copy_tree_progress(
    src: &Path,
    dst: &Path,
    on_bytes: &mut dyn FnMut(u64),
    on_skipped: &mut dyn FnMut(&Path, &crate::Error),
    cancel: &dyn Fn() -> bool,
) -> Result<OpStatus> {
    // The directories this branch has already entered, so it can refuse to
    // enter one of them a second time. See `DirMark`.
    let mut entered: Vec<DirMark> = Vec::new();
    copy_tree_inner(src, dst, on_bytes, on_skipped, cancel, &mut entered)
}

fn copy_tree_inner(
    src: &Path,
    dst: &Path,
    on_bytes: &mut dyn FnMut(u64),
    on_skipped: &mut dyn FnMut(&Path, &crate::Error),
    cancel: &dyn Fn() -> bool,
    entered: &mut Vec<DirMark>,
) -> Result<OpStatus> {
    if cancel() {
        return Ok(OpStatus::Cancelled);
    }
    let md = std::fs::symlink_metadata(src)?;

    if md.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = std::fs::read_link(src)?;
            std::os::unix::fs::symlink(target, dst)?;
        }
        #[cfg(windows)]
        {
            let (cloned, target_is_dir) = try_clone_symlink(src, dst);
            if !cloned {
                // Insufficient privilege: materialize the target with byte
                // tracking and cancellation support. See `copy_path`.
                if target_is_dir {
                    // Materialising follows the link out of the tree being
                    // copied. Where it lands is checked on entry like any
                    // other directory, and a refusal is reported as an entry
                    // the copy could not take — so the rest still lands.
                    return copy_tree_inner(
                        &std::fs::canonicalize(src)?,
                        dst,
                        on_bytes,
                        on_skipped,
                        cancel,
                        entered,
                    );
                }
                return copy_file_progress(src, dst, on_bytes, cancel);
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::fs::copy(src, dst)?;
        }
        return Ok(OpStatus::Done);
    }

    if md.is_dir() {
        let mark = dir_mark(src);
        if let Some(mark) = mark.as_ref()
            && entered.contains(mark)
        {
            return Err(loops_back(src));
        }
        std::fs::create_dir(dst)?;
        // Moved rather than cloned: the mark is a pair of integers on Unix and
        // a path elsewhere, and only one of the two would tolerate a clone.
        let marked = mark.is_some();
        if let Some(mark) = mark {
            entered.push(mark);
        }
        let walk = (|| -> Result<OpStatus> {
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                if cancel() {
                    return Ok(OpStatus::Cancelled);
                }
                let child = entry.path();
                match copy_tree_inner(
                    &child,
                    &dst.join(entry.file_name()),
                    on_bytes,
                    on_skipped,
                    cancel,
                    entered,
                ) {
                    Ok(OpStatus::Cancelled) => return Ok(OpStatus::Cancelled),
                    Ok(OpStatus::Done) => {}
                    // Reported, then stepped over: the rest of the folder is
                    // copied, which is what the user asked for.
                    Err(err) => on_skipped(&child, &err),
                }
            }
            Ok(OpStatus::Done)
        })();
        if marked {
            entered.pop();
        }
        walk
    } else {
        copy_file_progress(src, dst, on_bytes, cancel)
    }
}

/// Copy chunk size: 4 MiB. Each chunk is a sequential read THEN write — on a
/// NETWORK share, each call pays the SMB latency: at 256 KiB throughput
/// plateaued well below Explorer's (which does large I/O). 4 MiB roughly
/// matches `CopyFileEx`'s throughput while still allowing progress reporting
/// and cancellation per chunk. Used on Linux and macOS; Windows relies on
/// `CopyFileExW`, see below.
#[cfg(not(windows))]
const COPY_BUF_BYTES: usize = 4 * 1024 * 1024;

#[cfg(not(windows))]
fn copy_file_progress(
    src: &Path,
    dst: &Path,
    on_bytes: &mut dyn FnMut(u64),
    cancel: &dyn Fn() -> bool,
) -> Result<OpStatus> {
    let mut reader = std::fs::File::open(src)?;
    let mut writer = std::fs::File::create(dst)?;
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    let outcome = loop {
        if cancel() {
            break Ok(OpStatus::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break Ok(OpStatus::Done),
            Ok(n) => n,
            Err(err) => break Err(Error::from(err)),
        };
        if let Err(err) = writer.write_all(&buf[..n]) {
            break Err(Error::from(err));
        }
        on_bytes(n as u64);
    };
    // Anything but a clean end of file leaves a truncated destination, which
    // looks exactly like a complete copy. Since the walk now steps over a
    // failing entry and carries on, those remains must not survive to be taken
    // for one: they go before the outcome is reported. Windows needs no
    // equivalent — `CopyFileExW` deletes its own partial target.
    if !matches!(outcome, Ok(OpStatus::Done)) {
        drop(writer);
        let _ = std::fs::remove_file(dst);
        return outcome;
    }
    // Best-effort: preserve permissions (Unix mode).
    if let Ok(meta) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dst, meta.permissions());
    }
    Ok(OpStatus::Done)
}

/// Copy of A SINGLE file via `CopyFileExW` — the same engine as Explorer:
/// pipelined I/O (reads/writes in parallel), and on an SMB share, **server-side**
/// copy (`FSCTL_SRV_COPYCHUNK`) when source and target live on the same server.
/// Progress goes through the native callback; `PROGRESS_CANCEL` interrupts the
/// operation and deletes the partial target. Attributes, including read-only,
/// are copied by the API.
#[cfg(windows)]
fn copy_file_progress(
    src: &Path,
    dst: &Path,
    on_bytes: &mut dyn FnMut(u64),
    cancel: &dyn Fn() -> bool,
) -> Result<OpStatus> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CopyFileExW(
            existing: *const u16,
            new: *const u16,
            progress: Option<CopyProgress>,
            data: *mut core::ffi::c_void,
            cancel_flag: *mut i32,
            flags: u32,
        ) -> i32;
    }
    // `LPPROGRESS_ROUTINE` — called for each chunk copied (CALLBACK_CHUNK_FINISHED).
    type CopyProgress = unsafe extern "system" fn(
        total: i64,
        transferred: i64,
        stream_size: i64,
        stream_transferred: i64,
        stream_num: u32,
        reason: u32,
        h_src: isize,
        h_dst: isize,
        data: *mut core::ffi::c_void,
    ) -> u32;
    const PROGRESS_CONTINUE: u32 = 0;
    const PROGRESS_CANCEL: u32 = 1;
    const ERROR_REQUEST_ABORTED: i32 = 1235;

    // The C callback receives the TOTAL transferred; the DELTA is handed to
    // `on_bytes` (existing contract) via this context passed through `data`.
    struct Ctx<'a> {
        on_bytes: &'a mut dyn FnMut(u64),
        cancel: &'a dyn Fn() -> bool,
        last: u64,
    }
    unsafe extern "system" fn progress(
        _total: i64,
        transferred: i64,
        _stream_size: i64,
        _stream_transferred: i64,
        _stream_num: u32,
        _reason: u32,
        _h_src: isize,
        _h_dst: isize,
        data: *mut core::ffi::c_void,
    ) -> u32 {
        unsafe {
            let ctx = &mut *(data as *mut Ctx);
            if (ctx.cancel)() {
                return PROGRESS_CANCEL; // CopyFileExW deletes the partial target
            }
            let done = transferred as u64;
            if done > ctx.last {
                (ctx.on_bytes)(done - ctx.last);
                ctx.last = done;
            }
            PROGRESS_CONTINUE
        }
    }

    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let src_w = wide(src);
    let dst_w = wide(dst);
    let mut ctx = Ctx {
        on_bytes,
        cancel,
        last: 0,
    };
    // flags = 0: overwrites an existing target; the caller has already
    // resolved name conflicts.
    let ok = unsafe {
        CopyFileExW(
            src_w.as_ptr(),
            dst_w.as_ptr(),
            Some(progress),
            &mut ctx as *mut Ctx as *mut core::ffi::c_void,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_REQUEST_ABORTED) {
            return Ok(OpStatus::Cancelled); // cancelled via PROGRESS_CANCEL
        }
        return Err(err.into());
    }
    Ok(OpStatus::Done)
}

/// Copies `src` into `dst_dir`. If the name already exists there, suffixes
/// `(1)`, `(2)`… Returns the final target path.
pub fn copy_into(src: &Path, dst_dir: &Path) -> Result<PathBuf> {
    let name = src
        .file_name()
        .ok_or_else(|| Error::Workspace(format!("no file name for {}", src.display())))?;
    let proposed = dst_dir.join(name);
    let target = unique_sibling(&proposed);
    copy_path(src, &target)?;
    Ok(target)
}

/// Moves `src` into `dst_dir`. Uses `std::fs::rename` (fast, atomic on the
/// same filesystem) and falls back to copy + remove on ANY rename failure —
/// crossing filesystems is the common case, but a bind mount or an exotic
/// driver can refuse the rename with a different error, and copy + remove
/// handles those just as well.
///
/// The fallback is not atomic: if the copy succeeds and the source cannot then
/// be removed, the item exists in BOTH places and the removal error is
/// returned. The caller must surface that outcome rather than treat it as a
/// completed move.
pub fn move_into(src: &Path, dst_dir: &Path) -> Result<PathBuf> {
    let name = src
        .file_name()
        .ok_or_else(|| Error::Workspace(format!("no file name for {}", src.display())))?;
    let proposed = dst_dir.join(name);
    let target = unique_sibling(&proposed);

    match std::fs::rename(src, &target) {
        Ok(()) => Ok(target),
        Err(_) => {
            // Copy first, then drop the source: a failure here leaves the copy
            // in place, which the propagated error tells the caller about.
            copy_path(src, &target)?;
            permanent_delete(src)?;
            Ok(target)
        }
    }
}

/// Moves `src` to the **exact** path `dst` (which must not exist).
/// Fast `rename`, with the same copy + remove fallback as [`move_into`] —
/// including its non-atomicity: a source that cannot be removed after a
/// successful copy leaves the item in both places and returns the error.
/// Used when the target name has already been resolved (e.g. after the
/// conflict popup).
pub fn move_to(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_path(src, dst)?;
            permanent_delete(src)?;
            Ok(())
        }
    }
}

/// Duplicates `src` in the same location with a unique suffix.
pub fn duplicate(src: &Path) -> Result<PathBuf> {
    let target = unique_sibling(src);
    copy_path(src, &target)?;
    Ok(target)
}
