//! Native file drag to EXTERNAL applications — **Windows**.
//!
//! HYBRID approach: internal drag (between views / onto a folder) stays
//! handled by Slint (ghost, auto-scroll, menu). When the cursor LEAVES the
//! window during a drag, we switch here to a native OLE drag so that
//! external applications receive the files (shell `CF_HDROP` format).
//!
//! On the way out, the `IDataObject` is built by the shell
//! (`IShellFolder::GetUIObjectOf`) and `SHDoDragDrop` provides the default
//! `IDropSource` + the drag image. On the way in, our small `IDropTarget`
//! replaces winit's so we can receive `DragOver` and reuse Favnyr's
//! hover/menu.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};
use windows::Win32::Foundation::{HGLOBAL, HWND, POINTL};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows::Win32::System::Com::{
    CoTaskMemFree, DVASPECT_CONTENT, FORMATETC, IDataObject, IStream, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, OleInitialize,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, CFSTR_SHELLIDLIST, DragQueryFileW, FILEDESCRIPTORW,
    FILEGROUPDESCRIPTORW, HDROP, IShellFolder, SHDoDragDrop, SHGetDesktopFolder,
    SHParseDisplayName,
};
use windows::core::{PCWSTR, Ref, implement};

/// Event from an incoming OLE drag. Favnyr replaces winit's minimal drop
/// target so it can also receive `DragOver` coordinates, essential for
/// real-time row hover.
pub enum IncomingFileDrag {
    Hover {
        screen_x: i32,
        screen_y: i32,
        copy: bool,
    },
    Leave,
    Drop {
        paths: Vec<PathBuf>,
        screen_x: i32,
        screen_y: i32,
        copy: bool,
        /// Present when Favnyr had to take ownership of source data before
        /// returning from OLE `Drop` (for example an email attachment or a
        /// temporary path exposed by an archive manager).
        staging: Option<DropStaging>,
    },
    /// The source advertised supported external data, but Favnyr could not
    /// take ownership of usable paths or bytes during `Drop`.
    ExternalDropFailed,
}

/// Favnyr-owned data that must stay alive until the asynchronous transfer
/// finishes. Hard-linked captures require a real copy at the destination so a
/// persistent third-party source can never share file identity with it.
pub struct DropStaging {
    pub temp_dir: PathBuf,
    pub copy_from_staging: bool,
}

type DropHandler = Box<dyn Fn(IncomingFileDrag)>;

thread_local! {
    /// Explicitly keeps our COM object alive. OLE also holds a reference
    /// between RegisterDragDrop and window destruction.
    static DROP_TARGET: RefCell<Option<IDropTarget>> = const { RefCell::new(None) };
}

/// Replaces the very minimal file drop target installed by winit. The latter
/// only reports "file entered/left/dropped", without the DragOver position;
/// Favnyr needs that position to target the panel, row, and executable. Must
/// be called after the HWND is created, on the UI thread.
pub fn init_drop_target(hwnd: isize, handler: impl Fn(IncomingFileDrag) + 'static) -> bool {
    if hwnd == 0 {
        return false;
    }
    DROP_TARGET.with(|slot| {
        if slot.borrow().is_some() {
            return true;
        }
        unsafe {
            if let Err(err) = OleInitialize(None) {
                warn!(error = %err, "initializing OLE for Favnyr drop target failed");
                return false;
            }
            // winit already registers a CF_HDROP target. It isn't exposed to
            // Slint and doesn't provide DragOver: we cleanly replace it.
            let _ = RevokeDragDrop(HWND(hwnd as *mut c_void));
        }
        let target: IDropTarget = FavnyrDropTarget {
            handler: Box::new(handler),
            incoming_kind: Cell::new(IncomingDataKind::None),
        }
        .into();
        match unsafe { RegisterDragDrop(HWND(hwnd as *mut c_void), &target) } {
            Ok(()) => {
                *slot.borrow_mut() = Some(target);
                debug!(hwnd, "Favnyr OLE drop target registered");
                true
            }
            Err(err) => {
                warn!(error = %err, "register Favnyr OLE drop target failed");
                false
            }
        }
    })
}

/// Re-registers the already-created Favnyr target after the native window has
/// completed its first event-loop iterations. Slint's declarative startup
/// timer can expire while winit is still finalizing its own CF_HDROP target;
/// reusing the same COM object once the loop is stable keeps Favnyr's richer
/// target authoritative without adding any recurring work.
pub fn rebind_drop_target(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    DROP_TARGET.with(|slot| {
        let Some(target) = slot.borrow().as_ref().cloned() else {
            warn!(
                hwnd,
                "Favnyr OLE drop target was unavailable for delayed registration"
            );
            return false;
        };
        let revoke = unsafe { RevokeDragDrop(HWND(hwnd as *mut c_void)) };
        let register = unsafe { RegisterDragDrop(HWND(hwnd as *mut c_void), &target) };
        if let Err(err) = revoke {
            debug!(error = %err, hwnd, "revoke before delayed drop-target registration failed");
        }
        match register {
            Ok(()) => {
                debug!(hwnd, "Favnyr OLE drop target re-registered after startup");
                true
            }
            Err(err) => {
                warn!(error = %err, hwnd, "delayed Favnyr OLE drop-target registration failed");
                false
            }
        }
    })
}

/// NUL-terminated wide string (kept alive by the caller for as long as the
/// pointer is in use).
fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
fn wide_str(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

mod drag_out;
mod formats;
mod paths;
mod target;
mod virtual_files;

#[cfg(test)]
mod tests;

pub use drag_out::*;
use formats::*;
use paths::*;
use target::*;
use virtual_files::*;
