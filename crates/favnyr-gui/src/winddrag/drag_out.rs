use super::*;

/// Starts a native OLE drag of `paths` (all assumed to be in the SAME folder —
/// which is the case for a selection coming from a view). External apps
/// receive the files as `CF_HDROP` (COPY effect). Blocking (modal loop of
/// `SHDoDragDrop`) until dropped/cancelled. `true` if a drop occurred.
/// Fire-and-forget: any error is logged, never propagated (no panic).
pub fn drag_files(paths: &[PathBuf]) -> bool {
    match unsafe { drag_files_inner(paths) } {
        Ok(dropped) => dropped,
        Err(err) => {
            warn!(error = %err, "native OLE drag failed");
            false
        }
    }
}

unsafe fn drag_files_inner(paths: &[PathBuf]) -> windows::core::Result<bool> {
    unsafe {
        if paths.is_empty() {
            return Ok(false);
        }
        // Common parent folder (the selection comes from a single view).
        let Some(parent_dir) = paths[0].parent() else {
            return Ok(false);
        };

        // `SHDoDragDrop`/OLE require OLE init on the UI thread. Idempotent
        // (S_FALSE if already initialized); we ignore failure (already in MTA →
        // we try anyway).
        let _ = OleInitialize(None);

        let desktop: IShellFolder = SHGetDesktopFolder()?;

        // Absolute PIDL of the parent folder → IShellFolder of the parent.
        let parent_w = wide(parent_dir);
        let mut parent_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(parent_w.as_ptr()), None, &mut parent_pidl, 0, None)?;
        // On failure `parent_pidl` (already allocated) must still be freed — a
        // bare `?` here leaked it.
        let parent: IShellFolder = match desktop.BindToObject(parent_pidl, None) {
            Ok(p) => p,
            Err(e) => {
                CoTaskMemFree(Some(parent_pidl as *const c_void));
                return Err(e);
            }
        };

        // Child (simple) PIDL of each file, relative to the parent.
        let mut child_pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
        for p in paths {
            let Some(name) = p.file_name() else { continue };
            let name_w = wide_str(name);
            let mut cpidl: *mut ITEMIDLIST = std::ptr::null_mut();
            // pcheaten=None, pdwattributes=null (attributes not requested).
            if parent
                .ParseDisplayName(
                    HWND::default(),
                    None,
                    PCWSTR(name_w.as_ptr()),
                    None,
                    &mut cpidl,
                    std::ptr::null_mut(),
                )
                .is_ok()
                && !cpidl.is_null()
            {
                child_pidls.push(cpidl);
            }
        }

        // Capture the outcome instead of `?`-returning: the shell-allocated
        // PIDLs below must be freed even when GetUIObjectOf / SHDoDragDrop fail
        // (both previously leaked parent_pidl AND every child PIDL).
        let dropped: windows::core::Result<bool> = if child_pidls.is_empty() {
            Ok(false)
        } else {
            let ptrs: Vec<*const ITEMIDLIST> = child_pidls.iter().map(|p| *p as *const _).collect();
            // IDataObject built by the shell (CF_HDROP + shell formats) — no
            // homegrown COM. `pdsrc = None` → SHDoDragDrop provides IDropSource +
            // the drag image.
            match parent.GetUIObjectOf(HWND::default(), &ptrs, None) {
                Ok(data) => {
                    let data: IDataObject = data;
                    match SHDoDragDrop(None, &data, None, DROPEFFECT_COPY) {
                        Ok(effect) => {
                            debug!(effect = effect.0, "SHDoDragDrop returned");
                            Ok(effect.0 != 0)
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        };

        // Free the PIDLs (shell-allocated → CoTaskMem) on every path.
        for c in &child_pidls {
            CoTaskMemFree(Some(*c as *const c_void));
        }
        CoTaskMemFree(Some(parent_pidl as *const c_void));
        dropped
    }
}
