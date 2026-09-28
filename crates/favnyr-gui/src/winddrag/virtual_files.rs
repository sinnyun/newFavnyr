use super::*;

// ---- "Virtual files" (email attachments, zip entries, browser images…) ----
// These aren't on disk: `CFSTR_FILEDESCRIPTORW` names them, `CFSTR_FILECONTENTS`
// carries their bytes (usually an `IStream`). We materialize them into a temp
// folder during `Drop`, and the normal drop pipeline then MOVES them into the
// target folder. A guard removes the complete staging tree after the move.

pub(super) struct MaterializedFileDrop {
    pub(super) paths: Vec<PathBuf>,
    pub(super) temp_dir: PathBuf,
}

/// Registered clipboard-format id for the given name. `RegisterClipboardFormatW`
/// is idempotent (same id for a given name), so this is cheap to call.
pub(super) fn clip_format(name: PCWSTR) -> u16 {
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

/// True when the drag offers a "virtual file" descriptor (a file not on disk).
pub(super) fn has_virtual_files(data: &IDataObject) -> bool {
    let format = FORMATETC {
        cfFormat: clip_format(CFSTR_FILEDESCRIPTORW),
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe { data.QueryGetData(&format).is_ok() }
}

/// Materializes the drag's virtual files into a fresh temp folder and returns
/// their real paths (`None` on failure). Must run during `Drop`, while the
/// `IDataObject` is still valid.
pub(super) fn materialize_virtual_files(data: &IDataObject) -> Option<MaterializedFileDrop> {
    let names = read_file_descriptor_names(data)?;
    if names.is_empty() {
        warn!("virtual file descriptor contained no items");
        return None;
    }
    let dir = create_drop_dir("favnyr-dnd")?;
    let mut out = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        // A descriptor may carry a relative path (zip subfolders): keep only the
        // final component so the file always stays inside our temp folder.
        let leaf = name.rsplit(['\\', '/']).next().unwrap_or(name);
        if leaf.is_empty() || leaf == "." || leaf == ".." {
            warn!(index, "virtual file descriptor contained an invalid name");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        // Each item gets its own staging subdirectory. This preserves duplicate
        // attachment names so the existing conflict resolver can arbitrate them.
        let item_dir = dir.join(index.to_string());
        if let Err(err) = std::fs::create_dir(&item_dir) {
            warn!(error = %err, path = %item_dir.display(), "creating virtual item directory failed");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        let path = item_dir.join(leaf);
        if let Err(err) = write_file_contents(data, index as i32, &path) {
            warn!(error = %err, index, name, "materializing virtual file failed");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        out.push(path);
    }
    Some(MaterializedFileDrop {
        paths: out,
        temp_dir: dir,
    })
}

/// Reads the file NAMES from the drag's `FILEGROUPDESCRIPTORW`.
fn read_file_descriptor_names(data: &IDataObject) -> Option<Vec<String>> {
    let format = FORMATETC {
        cfFormat: clip_format(CFSTR_FILEDESCRIPTORW),
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let Ok(mut medium) = (unsafe { data.GetData(&format) }) else {
        warn!("reading virtual file descriptors failed");
        return None;
    };
    let out = unsafe {
        let hglobal = medium.u.hGlobal;
        let byte_len = GlobalSize(hglobal);
        let base = GlobalLock(hglobal) as *const u8;
        if base.is_null() {
            None
        } else {
            let descriptor_offset = std::mem::offset_of!(FILEGROUPDESCRIPTORW, fgd);
            let descriptor_size = std::mem::size_of::<FILEDESCRIPTORW>();
            let count = if byte_len >= descriptor_offset {
                std::ptr::read_unaligned(base as *const u32) as usize
            } else {
                usize::MAX
            };
            let available = byte_len
                .saturating_sub(descriptor_offset)
                .checked_div(descriptor_size)
                .unwrap_or(0);
            let result = if count > available {
                warn!(
                    count,
                    byte_len, "virtual file descriptor block was truncated"
                );
                None
            } else {
                let first = base.add(descriptor_offset) as *const FILEDESCRIPTORW;
                let mut names = Vec::with_capacity(count);
                for i in 0..count {
                    // FILEDESCRIPTORW is packed: copy the name array through a
                    // raw pointer instead of creating an unaligned reference.
                    let name_ptr = std::ptr::addr_of!((*first.add(i)).cFileName);
                    let name: [u16; 260] = std::ptr::read_unaligned(name_ptr);
                    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                    names.push(String::from_utf16_lossy(&name[..len]));
                }
                Some(names)
            };
            let _ = GlobalUnlock(hglobal);
            result
        }
    };
    unsafe {
        ReleaseStgMedium(&mut medium);
    }
    out
}

/// Requests one supported storage medium for an indexed `FILECONTENTS` item.
/// Individual requests come first because some real-world OLE providers reject
/// a standards-compliant bitmask even though they support one of its members.
fn file_contents_medium(
    data: &IDataObject,
    index: i32,
) -> Option<windows::Win32::System::Com::STGMEDIUM> {
    let mut failures = Vec::with_capacity(3);
    for requested in [
        TYMED_ISTREAM.0 as u32,
        TYMED_HGLOBAL.0 as u32,
        (TYMED_ISTREAM.0 | TYMED_HGLOBAL.0) as u32,
    ] {
        let format = FORMATETC {
            cfFormat: clip_format(CFSTR_FILECONTENTS),
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: index,
            tymed: requested,
        };
        match unsafe { data.GetData(&format) } {
            Ok(medium) => return Some(medium),
            Err(err) => failures.push(format!("0x{requested:X}: {err}")),
        }
    }
    warn!(index, errors = %failures.join("; "), "requesting virtual file contents failed");
    None
}

/// Streams one virtual file straight to disk. This keeps memory use bounded
/// even for large email attachments.
fn write_file_contents(data: &IDataObject, index: i32, path: &Path) -> Result<(), String> {
    let Some(mut medium) = file_contents_medium(data, index) else {
        return Err("the source did not provide a supported storage medium".into());
    };
    let result = unsafe {
        if medium.tymed == TYMED_ISTREAM.0 as u32 {
            (*medium.u.pstm)
                .as_ref()
                .ok_or_else(|| "the source returned a null IStream".to_string())
                .and_then(|stream| write_istream(stream, path))
        } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            write_hglobal(medium.u.hGlobal, path)
        } else {
            Err(format!(
                "the source returned unsupported TYMED 0x{:X}",
                medium.tymed
            ))
        }
    };
    unsafe {
        ReleaseStgMedium(&mut medium);
    }
    result
}

/// Writes an `IStream` to a file in fixed-size chunks.
fn write_istream(stream: &IStream, path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let mut read: u32 = 0;
        let status = unsafe {
            stream.Read(
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                Some(&mut read),
            )
        };
        if read as usize > buf.len() {
            return Err("IStream returned an invalid byte count".into());
        }
        if read > 0 {
            file.write_all(&buf[..read as usize])
                .map_err(|err| err.to_string())?;
        }
        if status.is_err() {
            return Err(format!("IStream::Read failed with {status:?}"));
        }
        if read == 0 {
            return Ok(());
        }
    }
}

/// Writes the bytes held by an `HGLOBAL` without an intermediate allocation.
fn write_hglobal(hglobal: HGLOBAL, path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let size = unsafe { GlobalSize(hglobal) };
    if size == 0 {
        return Ok(());
    }
    let ptr = unsafe { GlobalLock(hglobal) } as *const u8;
    if ptr.is_null() {
        return Err("GlobalLock failed for virtual file contents".into());
    }
    let result = file
        .write_all(unsafe { std::slice::from_raw_parts(ptr, size) })
        .map_err(|err| err.to_string());
    unsafe {
        let _ = GlobalUnlock(hglobal);
    }
    result
}
