use super::*;

/// Creates a **link** to `src` in `dst_dir` (unique name on conflict). Cross-OS:
///   - **Linux**: symbolic link (`symlink`), valid for both files AND folders,
///     across filesystems and mounts.
///   - **Windows**: a symbolic link when allowed (Developer Mode / admin — the
///     only Windows link that crosses volumes and network shares), otherwise a
///     hard link for a file or a junction for a folder on the same volume. See
///     `windows_link`.
///
/// Returns the path of the created link.
pub fn link_into(src: &Path, dst_dir: &Path) -> Result<PathBuf> {
    let name = src
        .file_name()
        .ok_or_else(|| Error::Workspace(format!("no file name for {}", src.display())))?;
    let target = unique_sibling(&dst_dir.join(name));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, &target)?;
    }
    #[cfg(windows)]
    {
        windows_link(src, &target)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = &target;
        return Err(Error::Workspace(
            "link_into is not supported on this platform".into(),
        ));
    }
    Ok(target)
}

/// Creates a link at an EXACT path `dst_path` (chosen name) pointing to `src`,
/// with the SAME mechanism as [`link_into`] (Unix symlink; on Windows a symbolic
/// link when allowed, else a hard link / junction on the same volume). Rejects an
/// already-existing target (never overwrites). Used by the "Create shortcut" menu
/// (symlink tab), which targets the current folder with a given name.
pub fn link_as(src: &Path, dst_path: &Path) -> Result<()> {
    if dst_path.exists() {
        return Err(Error::Workspace(format!(
            "{} already exists",
            dst_path.display()
        )));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst_path)?;
    }
    #[cfg(windows)]
    {
        windows_link(src, dst_path)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        return Err(Error::Workspace(
            "link_as is not supported on this platform".into(),
        ));
    }
    Ok(())
}

/// Creates a symbolic link `link` → `target` through Win32, asking for the
/// unprivileged flag so it also succeeds under **Developer Mode** (no admin).
/// A symbolic link is the only Windows link that spans volumes (USB sticks,
/// network shares) and, like on Unix, covers both files and folders. Windows
/// older than 10 1703 rejects the unprivileged flag with `ERROR_INVALID_PARAMETER`;
/// we retry without it there (an admin token is then required).
#[cfg(windows)]
fn create_symlink_win(link: &Path, target: &Path, is_dir: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateSymbolicLinkW(symlink: *const u16, target: *const u16, flags: u32) -> u8;
    }
    const FLAG_DIRECTORY: u32 = 0x1;
    const FLAG_ALLOW_UNPRIVILEGED_CREATE: u32 = 0x2;
    const ERROR_INVALID_PARAMETER: i32 = 87;

    // Null-terminated UTF-16, byte-exact for non-ASCII paths.
    let to_wide = |p: &Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let link_w = to_wide(link);
    let target_w = to_wide(target);
    let base = if is_dir { FLAG_DIRECTORY } else { 0 };

    let attempt = |flags: u32| -> std::io::Result<()> {
        let ok = unsafe { CreateSymbolicLinkW(link_w.as_ptr(), target_w.as_ptr(), flags) };
        if ok != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };

    match attempt(base | FLAG_ALLOW_UNPRIVILEGED_CREATE) {
        Ok(()) => Ok(()),
        // Legacy Windows that doesn't know the unprivileged flag → retry plain.
        Err(e) if e.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => attempt(base),
        Err(e) => Err(e),
    }
}

/// Creates a Windows junction `link` → `src` (folders only, local volume) WITHOUT
/// exposing the paths to `cmd`'s metacharacter parsing. `mklink /J` is a `cmd`
/// built-in, so it must run under `cmd /C`; cmd interprets `& | < > ( ) ^` BEFORE
/// splitting argv, so a folder such as `R&D` would otherwise break the command and
/// a crafted name could inject a second one. Both paths are wrapped in double
/// quotes, built as an `OsString` (so non-ASCII paths stay byte-exact) and passed
/// via `raw_arg` (Rust auto-quotes only args containing spaces); cmd treats a
/// quoted run as one literal token, and Windows paths cannot contain `"`, so the
/// quoting cannot be escaped out of. (`%VAR%` is still expanded by cmd inside
/// quotes, so a folder literally named after a defined variable could mis-target —
/// a wrong link, never code execution.)
#[cfg(windows)]
fn windows_junction(src: &Path, link: &Path) -> Result<()> {
    use std::ffi::OsString;
    use std::os::windows::process::CommandExt;
    let mut cmdline = OsString::from("/C mklink /J \"");
    cmdline.push(link.as_os_str());
    cmdline.push("\" \"");
    cmdline.push(src.as_os_str());
    cmdline.push("\"");
    let status = std::process::Command::new("cmd")
        .raw_arg(&cmdline)
        .status()
        .map_err(|e| Error::Workspace(format!("spawn mklink: {e}")))?;
    if !status.success() {
        return Err(Error::Workspace(format!(
            "mklink /J failed for {}",
            link.display()
        )));
    }
    Ok(())
}

/// Creates a Windows link named `link` pointing at `src`.
///
/// A **symbolic link** is tried first: it matches the Unix behaviour (one kind
/// of link for files and folders), points to a path like a shortcut, and — key
/// here — is the only Windows link that crosses volumes (USB, network shares).
/// It needs Developer Mode or admin. Without that privilege it fails, and we
/// fall back to a link that needs none but is **volume-local**: a hard link for
/// a file, a junction for a folder. If both fail (e.g. a cross-volume drop with
/// no Developer Mode, or a target filesystem without reparse points), the error
/// carries BOTH reasons so the caller can surface a meaningful message.
#[cfg(windows)]
fn windows_link(src: &Path, link: &Path) -> Result<()> {
    let is_dir = src.is_dir();
    let Err(sym_err) = create_symlink_win(link, src, is_dir) else {
        return Ok(());
    };
    let fallback = if is_dir {
        windows_junction(src, link)
    } else {
        std::fs::hard_link(src, link)
            .map_err(|e| Error::Workspace(format!("hard link {}: {e}", link.display())))
    };
    if let Err(fb_err) = fallback {
        return Err(Error::Workspace(format!(
            "cannot link {} → {}: {fb_err}; a symbolic link (needed across \
             volumes and network shares) also failed: {sym_err} — enable \
             Developer Mode or run as administrator",
            link.display(),
            src.display()
        )));
    }
    Ok(())
}
