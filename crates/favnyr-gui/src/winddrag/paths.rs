use super::*;

pub(super) struct CapturedPathDrop {
    pub(super) paths: Vec<PathBuf>,
    pub(super) temp_dir: PathBuf,
    pub(super) used_hard_links: bool,
}

/// Captures application-provided CF_HDROP paths before returning from OLE
/// `Drop`. 7-Zip, for example, deletes its extraction directory immediately
/// after `DoDragDrop` returns, before Favnyr's deferred transfer can start.
pub(super) fn capture_application_paths(paths: &[PathBuf]) -> Option<CapturedPathDrop> {
    if paths.is_empty() {
        return None;
    }
    let dir = create_drop_dir("favnyr-path-dnd")?;
    let mut out = Vec::with_capacity(paths.len());
    let mut used_hard_links = false;
    for (index, source) in paths.iter().enumerate() {
        let Some(name) = source.file_name() else {
            warn!(path = %source.display(), "application drop path has no file name");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        };
        // Separate roots preserve same-named items from different source
        // folders; Favnyr's existing conflict resolver handles them later.
        let item_dir = dir.join(index.to_string());
        if let Err(err) = std::fs::create_dir(&item_dir) {
            warn!(error = %err, path = %item_dir.display(), "creating application drop item directory failed");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        let captured = item_dir.join(name);
        match capture_path_tree(source, &captured) {
            Ok(item_used_hard_links) => used_hard_links |= item_used_hard_links,
            Err(err) => {
                warn!(error = %err, path = %source.display(), "capturing application drop path failed");
                let _ = std::fs::remove_dir_all(&dir);
                return None;
            }
        }
        out.push(captured);
    }
    Some(CapturedPathDrop {
        paths: out,
        temp_dir: dir,
        used_hard_links,
    })
}

pub(super) fn create_drop_dir(prefix: &str) -> Option<PathBuf> {
    for _ in 0..32 {
        let serial = DROP_SERIAL.with(|counter| {
            let value = counter.get().wrapping_add(1);
            counter.set(value);
            value
        });
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{serial}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Some(dir),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                warn!(error = %err, path = %dir.display(), "creating drop directory failed");
                return None;
            }
        }
    }
    warn!(prefix, "could not allocate a unique drop directory");
    None
}

/// Recreates a tree without following Windows reparse points. Regular files
/// use hard links first so the OLE callback stays independent of file size;
/// cross-volume and unsupported filesystems fall back to a physical copy.
fn capture_path_tree(source: &Path, destination: &Path) -> std::io::Result<bool> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reparse points are not supported in transient file drops",
        ));
    }
    if metadata.is_dir() {
        std::fs::create_dir(destination)?;
        let mut used_hard_links = false;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            used_hard_links |=
                capture_path_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(used_hard_links)
    } else if metadata.is_file() {
        match std::fs::hard_link(source, destination) {
            Ok(()) => Ok(true),
            Err(_) => {
                std::fs::copy(source, destination)?;
                Ok(false)
            }
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported filesystem object in transient file drop",
        ))
    }
}

thread_local! {
    static DROP_SERIAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
