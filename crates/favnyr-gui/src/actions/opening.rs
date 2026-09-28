use super::*;

/// Windows primitive shared by the `open` and `runas` Shell verbs.
///
/// Centralizes UTF-16 encoding, optional parameters, and the working
/// directory. The calling code keeps interpreting the return code, since
/// a UAC cancellation (`runas`) doesn't carry the same message as an open failure.
#[cfg(windows)]
pub(super) fn shell_execute(
    verb: &[u16],
    target: &OsStr,
    parameters: Option<&OsStr>,
    working_directory: Option<&Path>,
) -> isize {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: isize,
            op: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }

    const SW_SHOWNORMAL: i32 = 1;
    let target_w: Vec<u16> = target.encode_wide().chain(std::iter::once(0)).collect();
    let parameters_w: Option<Vec<u16>> =
        parameters.map(|value| value.encode_wide().chain(std::iter::once(0)).collect());
    let parameters_ptr = parameters_w
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    let directory_w: Option<Vec<u16>> = working_directory
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        });
    let directory_ptr = directory_w
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());

    // ShellExecute is fire-and-forget (independent child, like Explorer).
    unsafe {
        ShellExecuteW(
            0,
            verb.as_ptr(),
            target_w.as_ptr(),
            parameters_ptr,
            directory_ptr,
            SW_SHOWNORMAL,
        )
    }
}

/// Opens a path or a URI with the Shell. `working_directory` is only
/// provided for physical paths: a URI like `ms-photos:` has no
/// working directory.
#[cfg(windows)]
fn shell_execute_open(target: &OsStr, working_directory: Option<&Path>) -> Result<()> {
    let result = shell_execute(SHELL_VERB_OPEN, target, None, working_directory);
    if result > 32 {
        Ok(())
    } else {
        Err(anyhow!(
            "opening {} failed (ShellExecute code {result})",
            target.to_string_lossy()
        ))
    }
}

/// Opens `path` with the OS's default application.
///
/// - **Windows**: `ShellExecuteW("open", …)` for the standard association of an
///   item: documents, `.bat`/`.cmd` scripts, `.exe`, and `.lnk`. An image
///   opened from a view first tries [`try_open_image_gallery`] to
///   convey the gallery context that `ShellExecuteW` doesn't carry.
/// - **Other OSes**: `open` crate (xdg-open).
#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<()> {
    info!(path = %path.display(), "open with default app (ShellExecute)");
    // `lpDirectory` points to the folder containing the target so that
    // executables and scripts can resolve their relative paths.
    shell_execute_open(path.as_os_str(), path.parent())
}

#[cfg(windows)]
fn default_handler_app_id(extension: &str) -> Result<Option<String>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{ASSOCF_NONE, ASSOCSTR_APPID, AssocQueryStringW};
    use windows::core::{PCWSTR, PWSTR};

    if extension.is_empty() {
        return Ok(None);
    }

    let association = format!(".{}", extension.trim_start_matches('.'));
    let association_w: Vec<u16> = OsStr::new(&association)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut length = 0_u32;

    // The first call just requests the size. Its HRESULT normally
    // indicates the buffer is too small; only the size matters.
    let _ = unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_APPID,
            PCWSTR(association_w.as_ptr()),
            PCWSTR::null(),
            None,
            &mut length,
        )
    };
    if length <= 1 {
        return Ok(None);
    }

    let mut output = vec![0_u16; length as usize];
    unsafe {
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_APPID,
            PCWSTR(association_w.as_ptr()),
            PCWSTR::null(),
            Some(PWSTR(output.as_mut_ptr())),
            &mut length,
        )
    }
    .ok()
    .map_err(|error| anyhow!("Windows association for {association}: {error}"))?;

    let text_length = output
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(output.len());
    Ok(Some(String::from_utf16_lossy(&output[..text_length])))
}

#[cfg(windows)]
pub(super) fn is_microsoft_photos_app_id(app_id: &str) -> bool {
    // The `8wekyb3d8bbwe` suffix is Microsoft's publisher identity: staying exact
    // prevents another similarly-named package from hijacking the reserved route.
    app_id
        .trim()
        .eq_ignore_ascii_case("Microsoft.Windows.Photos_8wekyb3d8bbwe!App")
}

#[cfg(windows)]
pub(super) fn photos_gallery_target(path: &Path) -> Result<OsString> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    if !path.is_absolute() {
        return Err(anyhow!(
            "the Photos protocol requires an absolute path: {}",
            path.display()
        ));
    }

    // The parameter is a Windows path, not a URL segment: we keep
    // the separators and the "C:", then percent-encode any UTF-8 byte that could
    // change the query's structure (`%`, `#`, `&`, `+`, space, non-ASCII…).
    // Photos 2025.11030+ once again accepts this correct form, unlike
    // a few intermediate 2025 builds that are obsolete today.
    // An invalid UTF-16 sequence can't be faithfully represented in
    // a UTF-8 URI. We reject it here: the caller will fall back to `open_path`,
    // which passes the original `OsStr` to ShellExecuteW without any conversion.
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("non-Unicode path incompatible with the Photos protocol"))?;
    let mut target = String::with_capacity("ms-photos:viewer?fileName=".len() + path.len());
    target.push_str("ms-photos:viewer?fileName=");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':' | b'\\' | b'/')
        {
            target.push(byte as char);
        } else {
            target.push('%');
            target.push(HEX[(byte >> 4) as usize] as char);
            target.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    Ok(OsString::from(target))
}

/// Attempts the gallery activation reserved for Microsoft Photos.
///
/// Microsoft Photos on Windows 11 now ignores `NeighboringFilesQuery`,
/// despite a successful WinRT activation. When Photos is genuinely the
/// default handler for the extension, its official protocol restores
/// navigation within the folder. Returns `false` without launching anything for
/// any other viewer: the caller then falls back to the standard OS open.
#[cfg(windows)]
pub fn try_open_image_gallery(path: &Path, extension: &str) -> Result<bool> {
    let Some(app_id) = default_handler_app_id(extension)? else {
        return Ok(false);
    };
    if !is_microsoft_photos_app_id(&app_id) {
        return Ok(false);
    }

    let target = photos_gallery_target(path)?;
    info!(path = %path.display(), app_id, "open image in Microsoft Photos gallery");
    shell_execute_open(target.as_os_str(), None)?;
    Ok(true)
}

/// Is `path` a program this desktop would refuse to start for us?
///
/// The executable bit alone is not enough: a FAT/NTFS/exFAT mount reports
/// everything as `0777`, so an image copied from Windows would qualify. The
/// same filter as the listing is applied — only a recognized binary/script, or
/// a file of unrecognized type (a binary without an extension, common here).
///
/// `.desktop` is deliberately left out even though it looks executable: it is a
/// launcher description, not a program, and only the desktop environment knows
/// how to act on it. Running it as a script would do the wrong thing.
#[cfg(not(windows))]
pub(super) fn is_launchable_program(path: &Path) -> bool {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if extension.as_deref() == Some("desktop") {
        return false;
    }
    if !favnyr_core::fs::classify_kind(extension.as_deref(), false).can_be_program() {
        return false;
    }
    has_exec_bit(path)
}

/// Executable bit set on a regular file. Follows links, like the listing does:
/// a shortcut to a binary is launchable, and it is the target's bit that
/// decides.
#[cfg(not(windows))]
fn has_exec_bit(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|md| md.is_file() && md.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Is `path` a desktop launcher?
///
/// The executable bit is deliberately NOT required here, unlike for a native
/// program. It was at first, by analogy, and that was wrong: the desktop's own
/// rule for a launcher never looks at it — it reads the entry and starts what
/// it names. Requiring it made every launcher that did not happen to carry the
/// bit fall through to the generic opener, which hands a launcher back as the
/// text file it is made of, in an editor.
///
/// Whether the entry is actually usable is left to the launch itself: an
/// unreadable or `Exec`-less file fails there, and the caller then opens it
/// like any other document.
#[cfg(not(windows))]
pub(super) fn is_desktop_launcher(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("desktop"))
}

#[cfg(not(windows))]
pub fn open_path(path: &Path) -> Result<()> {
    // A launcher is acted upon, not opened. Asking the desktop to open one
    // gives back its source text in an editor: KIO only runs an entry when the
    // caller opted in, exactly as it does for a binary, so the same delegation
    // has to be skipped. Favnyr reads the entry itself and starts what it
    // names.
    if is_desktop_launcher(path) {
        match crate::openwith::launch_desktop_file(path) {
            Ok(()) => {
                info!(path = %path.display(), "launch desktop entry");
                return Ok(());
            }
            // Carries the `.desktop` name but nothing to start: no `Exec`, no
            // `URL`, or unreadable. It is then just a file, and opening it the
            // ordinary way is more useful than reporting a failure.
            Err(err) => {
                debug!(error = %err, path = %path.display(),
                    "not a usable desktop entry, opening it as a document");
            }
        }
    }
    // A program is started directly instead of being handed to the desktop.
    //
    // Handing it over does not work on KDE: the request reaches KIO's
    // `OpenUrlJob`, which runs an executable only when the calling application
    // asked for it through `setRunExecutables(true)`. Dolphin does; the
    // `xdg-open` path does not, and the launch comes back refused with "For
    // security reasons, launching executables is not allowed in this context."
    // That flag belongs to the KDE-side API, so no argument passed to
    // `xdg-open` can reach it — the delegation itself is what has to be
    // skipped, and only for this case.
    if is_launchable_program(path) {
        info!(path = %path.display(), "run program directly");
        // Its own folder, as if started from a terminal there, so a program
        // reading a file next to itself finds it whatever folder Favnyr was
        // started from.
        return spawn_detached_in(path, Vec::new(), false, path.parent());
    }
    info!(path = %path.display(), "open with default app");
    open::that_detached(path).map_err(|e| anyhow!("opening {}: {e}", path.display()))
}

/// Opens the **parent** folder of `path` and, if possible, **selects**
/// the item (Windows: `explorer /select,`). If `path` is a folder, it's
/// opened directly.
pub fn open_parent(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("explorer");
        if path.is_dir() {
            // A folder is opened directly to show its contents.
            cmd.arg(path);
        } else {
            // FILE → we REVEAL it: `explorer /select,<path>` opens the
            // containing folder AND highlights the file. The path MUST be
            // passed as a RAW argument and quoted AFTER the comma: with `.arg(...)`,
            // `std` wraps the whole `/select,C:\...` token in quotes as soon as there's
            // a space → explorer no longer recognizes `/select,` and opens
            // "Documents" by default instead. `raw_arg` writes exactly
            // `/select,"<path>"`.
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        }
        // (explorer often returns a nonzero code even on success → we don't test
        // the status, we just launch it.)
        cmd.spawn().map_err(|e| anyhow!("spawn explorer: {e}"))?;
        info!(path = %path.display(), is_dir = path.is_dir(), "reveal/open in explorer");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| anyhow!("no parent for {}", path.display()))?
        };
        info!(target = %target.display(), "opening parent folder");
        open::that_detached(&target).map_err(|e| anyhow!("opening {}: {e}", target.display()))
    }
}
