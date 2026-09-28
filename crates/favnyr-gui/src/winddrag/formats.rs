use super::*;

/// Classifies the formats without rendering their data. In particular,
/// calling `GetData(CF_HDROP)` from `DragEnter` can make archive managers
/// extract files merely because the pointer crossed the window.
pub(super) fn classify_incoming_data(data: &IDataObject) -> IncomingDataKind {
    let has_hdrop = has_hdrop(data);
    let has_stream_hdrop = has_stream_hdrop(data);
    let has_shell_id_list = has_shell_id_list(data);
    let has_virtual_files = has_virtual_files(data);
    classify_formats(
        has_hdrop,
        has_stream_hdrop,
        has_shell_id_list,
        has_virtual_files,
    )
}

pub(super) fn classify_formats(
    has_paths: bool,
    has_stream_hdrop: bool,
    has_shell_id_list: bool,
    has_virtual_files: bool,
) -> IncomingDataKind {
    if has_paths {
        if has_shell_id_list && !has_stream_hdrop {
            IncomingDataKind::ShellPaths
        } else {
            // Standard CF_HDROP uses HGLOBAL. Some application data objects
            // additionally advertise IStream and synthesize both CF_HDROP and
            // Shell IDList Array from temporary paths (PeaZip's public
            // TDropFileSource implementation is one example). CIDA is not a
            // lifetime guarantee in that case, so capture before Drop returns.
            IncomingDataKind::ApplicationPaths
        }
    } else if has_virtual_files {
        IncomingDataKind::VirtualFiles
    } else {
        IncomingDataKind::None
    }
}

fn has_hdrop(data: &IDataObject) -> bool {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe { data.QueryGetData(&format).is_ok() }
}

/// Detects a non-standard, application-provided CF_HDROP representation.
/// Microsoft's CF_HDROP contract uses TYMED_HGLOBAL; accepting IStream as well
/// is a useful provider-level signal that the paths are synthesized rather
/// than a normal Shell filesystem selection. QueryGetData never renders them.
fn has_stream_hdrop(data: &IDataObject) -> bool {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_ISTREAM.0 as u32,
    };
    unsafe { data.QueryGetData(&format).is_ok() }
}

/// The Shell IDList Array is a positive marker for data objects produced by
/// Explorer and Favnyr's own Shell-based outgoing drag. Those paths remain on
/// the existing Move/Copy/Link route.
fn has_shell_id_list(data: &IDataObject) -> bool {
    let format = FORMATETC {
        cfFormat: clip_format(CFSTR_SHELLIDLIST),
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe { data.QueryGetData(&format).is_ok() }
}

/// Extracts and COPIES the CF_HDROP paths from an IDataObject. `ReleaseStgMedium`
/// always releases the medium after reading; no Shell data leaks into the
/// application state.
pub(super) fn file_paths(data: &IDataObject) -> Vec<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut medium = match unsafe { data.GetData(&format) } {
        Ok(medium) => medium,
        Err(err) => {
            debug!(error = %err, "CF_HDROP could not be rendered during Drop");
            return Vec::new();
        }
    };
    if medium.tymed != TYMED_HGLOBAL.0 as u32 {
        warn!(
            tymed = medium.tymed,
            "CF_HDROP source returned an unexpected storage medium"
        );
        unsafe {
            ReleaseStgMedium(&mut medium);
        }
        return Vec::new();
    }
    let mut out = Vec::new();
    unsafe {
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        if out.try_reserve(count as usize).is_err() {
            warn!(count, "CF_HDROP path list is too large to allocate");
        } else {
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                if len == 0 {
                    continue;
                }
                let mut wide = vec![0u16; len + 1];
                if DragQueryFileW(hdrop, i, Some(&mut wide)) > 0 {
                    out.push(PathBuf::from(String::from_utf16_lossy(&wide[..len])));
                }
            }
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}
