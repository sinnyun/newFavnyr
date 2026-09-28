//! Listing, classification, sorting, formatting, and FS operations.
//!
//! This file covers **listing** a folder, **classification** by type,
//! **sorting**, and **formatting**. File operations
//! (copy / move / trash / properties) live in the
//! [`ops`] submodule.

pub mod ops;

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::Result;

mod format;
mod sort;
mod stats;
mod typed_path;

#[cfg(test)]
mod tests;

pub use format::*;
pub use sort::*;
pub use stats::*;
pub use typed_path::*;

/// Category of a file — drives icon selection on the Slint side.
///
/// The `u8` values are **stable**: they are serialized to Slint
/// (`int`) on the `FileRow.kind` side and used in ternary cascades
/// `entry.kind == 0 ? @image-url(...)`. **Do not reorder.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum FileKind {
    Folder = 0,
    File = 1,
    Application = 2,
    Archive = 3,
    Audio = 4,
    Document = 5,
    Image = 6,
    Video = 7,
    /// Configuration files / structured data (toml, yaml, json, ini…).
    Config = 8,
}

/// Extensions recognized as images, sorted to allow binary search.
/// This list is also the authority for the Windows gallery filter: a new
/// extension therefore never needs to be added in two places.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "af", "afdesign", "afphoto", "afpub", "arw", "avif", "bmp", "cr2", "cr3", "dds", "dng", "exr",
    "gif", "hdr", "heic", "heif", "ico", "jpeg", "jpg", "nef", "orf", "pam", "pbm", "pgm", "png",
    "pnm", "ppm", "psd", "qoi", "raf", "raw", "rw2", "srw", "svg", "tga", "tif", "tiff", "webp",
    "xcf",
];

/// `extension` must be normalized to lowercase, without a dot.
pub fn is_image_extension(extension: &str) -> bool {
    IMAGE_EXTENSIONS.binary_search(&extension).is_ok()
}

/// Extensions recognized as archives, sorted to allow binary search.
/// Single source of truth: it classifies rows AND pre-fills the extension
/// filter of the ready-made "extract" commands, so an archive format never
/// needs adding in two places.
pub const ARCHIVE_EXTENSIONS: &[&str] = &[
    "7z", "ar", "bz2", "cab", "gz", "iso", "lz", "lzma", "lzo", "rar", "tar", "tbz2", "tgz", "txz",
    "xz", "zip", "zst",
];

/// `extension` must be normalized to lowercase, without a dot.
pub fn is_archive_extension(extension: &str) -> bool {
    ARCHIVE_EXTENSIONS.binary_search(&extension).is_ok()
}

impl FileKind {
    pub fn as_i32(self) -> i32 {
        self as u8 as i32
    }

    /// Inverse of [`FileKind::as_i32`]: rebuilds the kind from its wire code,
    /// i.e. the value carried by a UI row (`FileRow::kind`). Returns `None` for
    /// a code no variant maps to, so a stale/foreign value never silently
    /// becomes `Folder`. Kept beside `as_i32` so the two stay in step.
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Folder,
            1 => Self::File,
            2 => Self::Application,
            3 => Self::Archive,
            4 => Self::Audio,
            5 => Self::Document,
            6 => Self::Image,
            7 => Self::Video,
            8 => Self::Config,
            _ => return None,
        })
    }

    /// Can a file of this type be a launchable PROGRAM / SCRIPT (with
    /// dropped files as arguments)? Used to filter out false positives from
    /// the Unix executable bit: on a FAT/NTFS/exFAT mount (everything is
    /// `0777`) or for a file copied from Windows, an image/video/audio/document/
    /// archive/config can appear "executable" without being a program. Only a
    /// binary/package/script of a recognized type (`Application`) or a file of
    /// an UNRECOGNIZED type (`File` — typically a binary or a script WITHOUT an
    /// extension, common on Linux) are real candidates.
    pub fn can_be_program(self) -> bool {
        matches!(self, FileKind::Application | FileKind::File)
    }
}

/// An entry listed in a folder.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
    pub mtime_unix: Option<i64>,
    pub is_dir: bool,
    pub kind: FileKind,
    /// **Hidden** entry (Unix dotfile OR Windows HIDDEN attribute) — for a
    /// discreet visual marker when showing hidden items is enabled.
    pub hidden: bool,
    /// Symbolic link (or Windows junction). `is_dir` reflects the TARGET;
    /// this flag is used for the visual marker — a "folder + arrow" icon for
    /// a folder-link, to distinguish it from a real folder.
    pub is_symlink: bool,
    /// Unix executable bit computed during listing (avoids a second `stat`
    /// in the GUI for the "launch with dropped files" feedback).
    /// Always `false` on Windows, where extensions define this action.
    pub executable: bool,
}

/// Classifies a file from its extension (lower-case, without the dot).
/// For a folder, returns `Folder` regardless of the name.
pub fn classify_kind(extension: Option<&str>, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Folder;
    }
    let Some(ext) = extension else {
        return FileKind::File;
    };
    let ext = ext.to_ascii_lowercase();
    if is_image_extension(&ext) {
        return FileKind::Image;
    }
    if is_archive_extension(&ext) {
        return FileKind::Archive;
    }
    match ext.as_str() {
        // Applications / binaries
        "exe" | "msi" | "appimage" | "deb" | "rpm" | "flatpak" | "snap" | "apk" | "dmg" | "pkg"
        | "sh" | "bat" | "ps1" | "cmd" => FileKind::Application,

        // Audio
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "aac" | "m4a" | "wma" | "aiff" | "alac" => {
            FileKind::Audio
        }

        // Documents (text, word processing, presentations, spreadsheets)
        "txt" | "md" | "rst" | "pdf" | "doc" | "docx" | "odt" | "rtf" | "tex" | "epub" | "djvu"
        | "xls" | "xlsx" | "ods" | "csv" | "tsv" | "ppt" | "pptx" | "odp" | "pages" | "numbers"
        | "key" => FileKind::Document,

        // Video
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg" | "ts"
        | "3gp" => FileKind::Video,

        // Configuration / structured data
        "toml" | "yaml" | "yml" | "json" | "json5" | "ini" | "cfg" | "conf" | "config" | "xml"
        | "plist" | "env" | "reg" => FileKind::Config,

        _ => FileKind::File,
    }
}

/// True if `md` carries the Windows `FILE_ATTRIBUTE_HIDDEN` attribute.
/// On other OSes: always false (hidden files there follow the `.` prefix
/// convention, handled separately). `const`-evaluated → optimized.
#[cfg(windows)]
fn is_hidden_attr(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0000_0002;
    md.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}
#[cfg(not(windows))]
fn is_hidden_attr(_md: &std::fs::Metadata) -> bool {
    false
}

/// Lists the entries of a folder.
///
/// `include_hidden = false` filters out **hidden** entries:
///   - Unix convention: name starting with `.` (on all OSes);
///   - Windows: `FILE_ATTRIBUTE_HIDDEN` attribute (Windows hidden files
///     generally don't use the `.` prefix).
///
/// Per-entry errors (permission, broken link…) are **silently ignored**
/// at the item level but logged: a partial listing is preferred over
/// giving up entirely because of one exotic file.
pub fn list_dir(path: &Path, include_hidden: bool) -> Result<Vec<Entry>> {
    Ok(list_dir_counted(path, include_hidden)?.0)
}

/// Like [`list_dir`], but ALSO returns the number of hidden entries present
/// in the folder — whether they're included (`include_hidden`) or not. Used to
/// discreetly signal to the user that hidden items exist (they
/// sometimes forget to check "show hidden files").
/// UNC server root (`\\HOST` **without** a share) → host name. `None` for any
/// other path (local, or UNC with a share `\\HOST\share`). Pure string
/// analysis (no I/O). Used to enumerate a network machine's shares.
pub fn unc_server_root(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    let rest = s.strip_prefix(r"\\").or_else(|| s.strip_prefix("//"))?;
    let rest = rest.trim_end_matches(['\\', '/']);
    if rest.is_empty() || rest.contains('\\') || rest.contains('/') {
        return None; // just "\\", or there's already a share
    }
    Some(rest.to_string())
}

/// Is a path listable by Favnyr? A classic folder, OR (Windows) a UNC
/// server root whose shares we know how to enumerate. Allows navigating to
/// `\\HOST`, which `Path::is_dir()` refuses (it's not a folder in the FS sense).
///
pub fn is_listable(path: &Path) -> bool {
    path.is_dir() || (cfg!(windows) && unc_server_root(path).is_some())
}

/// "Parent" of a UNC SHARE ROOT: `\\HOST\share` → `\\HOST` (the
/// server root, navigable via share enumeration. `None` for any
/// other path — notably a share subfolder (`\\HOST\share\x`), for which
/// `Path::parent()` works normally. Fills the gap left by `Path::parent()`,
/// which returns `None` on a share root.
pub fn unc_share_parent(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let mut comps = path.components();
    let Some(Component::Prefix(p)) = comps.next() else {
        return None;
    };
    let server = match p.kind() {
        Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => server.to_string_lossy(),
        _ => return None,
    };
    // Only the share root (nothing after prefix + root).
    if comps.any(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(PathBuf::from(format!(r"\\{server}")))
}

/// Network UNC path (`\\server…`)? Excludes **local** namespaces
/// `\\?\` (verbatim) and `\\.\` (device). Used to only offer the system
/// login on real network paths.
pub fn is_unc_path(path: &Path) -> bool {
    match path.to_str() {
        Some(s) => {
            (s.starts_with(r"\\") || s.starts_with("//"))
                && !s.starts_with(r"\\?\")
                && !s.starts_with(r"\\.\")
        }
        None => false,
    }
}

/// Lists the shares of a UNC server root as `Entry` items (folders) —
/// `\\HOST` becomes "browsable" like in Explorer. `Err(PermissionDenied)`
/// if authentication is required (→ GUI-side login prompt), `Err(NotConnected)`
/// if unreachable → "unavailable" banner.
#[cfg(windows)]
fn list_server_shares(server: &str) -> Result<(Vec<Entry>, usize)> {
    use crate::places::NetShares;
    match crate::places::net_shares(server) {
        NetShares::Ok(shares) => {
            let entries = shares
                .into_iter()
                .map(|sh| Entry {
                    path: PathBuf::from(format!(r"\\{server}\{sh}")),
                    name: sh,
                    size_bytes: None,
                    mtime_unix: None,
                    is_dir: true,
                    kind: FileKind::Folder,
                    hidden: false,
                    is_symlink: false,
                    executable: false,
                })
                .collect();
            Ok((entries, 0))
        }
        NetShares::AuthNeeded => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "network authentication required",
        )
        .into()),
        NetShares::Unreachable => Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "network server unreachable",
        )
        .into()),
    }
}

pub fn list_dir_counted(path: &Path, include_hidden: bool) -> Result<(Vec<Entry>, usize)> {
    // UNC server root (`\\HOST`): its SMB SHARES are enumerated (std::fs can
    // only list a single share, not a server). Shares are displayed as folders.
    #[cfg(windows)]
    {
        if let Some(server) = unc_server_root(path) {
            return list_server_shares(&server);
        }
    }
    let read = std::fs::read_dir(path)?;
    let mut out = Vec::with_capacity(64);
    let mut hidden_count = 0usize;
    for entry_res in read {
        let dir_entry = match entry_res {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(error = %err, path = %path.display(), "read_dir item failed");
                continue;
            }
        };

        let name = dir_entry.file_name().to_string_lossy().into_owned();
        // Dotfiles (Unix convention) — fast filter, no `stat`. The dotfile is
        // counted as hidden HERE, which preserves the original optimization
        // (the `stat` is skipped when hidden items aren't shown).
        let is_dotfile = name.starts_with('.');
        if is_dotfile {
            hidden_count += 1;
            if !include_hidden {
                continue;
            }
        }

        let full_path = dir_entry.path();

        let md_res = dir_entry.metadata();
        // HIDDEN attribute (Windows): read from the already-loaded `metadata`
        // (no extra `stat`). No-op on Linux/macOS.
        let hidden_attr = md_res.as_ref().map(is_hidden_attr).unwrap_or(false);
        // A non-dotfile hidden by attribute → counted here (`&& !is_dotfile`
        // avoids double-counting a dotfile that would ALSO have the hidden attribute).
        if hidden_attr && !is_dotfile {
            hidden_count += 1;
        }
        if !include_hidden && hidden_attr {
            continue;
        }
        // `hidden` = dotfile OR HIDDEN attribute → visual marker on the GUI side.
        let hidden = is_dotfile || hidden_attr;

        // `DirEntry::metadata()` does NOT follow symbolic links → a link to a
        // FOLDER would report `is_dir=false` and Favnyr would open it in the
        // system file manager instead of navigating into it. The TARGET is
        // therefore resolved (via `fs::metadata`, which follows) ONLY for
        // links — the cost of an extra `stat` reserved for this rare case.
        // Broken link → target not found → treated as a non-folder.
        let is_symlink = dir_entry
            .file_type()
            .map(|t| t.is_symlink())
            .unwrap_or(false);
        let (is_dir, size_bytes, mtime_unix, executable) = match md_res {
            Ok(md) => {
                // A link describes itself, not what it points at: its own
                // length is the size of the stored path and its own timestamp
                // is when the link was made. On Unix its permissions are
                // usually 0777 as well. Every displayed field therefore reads
                // the FOLLOWED metadata, so a link to a file reports that
                // file's size, date and executability rather than the link's.
                // A broken link has no target to follow and keeps its own.
                let followed_md = is_symlink
                    .then(|| std::fs::metadata(&full_path).ok())
                    .flatten();
                let effective_md = followed_md.as_ref().unwrap_or(&md);
                let is_dir = effective_md.is_dir();
                let size = if is_dir {
                    None
                } else {
                    Some(effective_md.len())
                };
                let mtime = effective_md
                    .modified()
                    .ok()
                    .and_then(|st| st.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64);
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    !is_dir
                        && (!is_symlink || followed_md.is_some())
                        && effective_md.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = false;
                (is_dir, size, mtime, executable)
            }
            Err(err) => {
                tracing::debug!(error = %err, path = %full_path.display(), "metadata failed");
                // The entry is kept but without size/mtime, assuming a file.
                (false, None, None, false)
            }
        };

        let ext = full_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_owned());
        let kind = classify_kind(ext.as_deref(), is_dir);

        out.push(Entry {
            name,
            path: full_path,
            size_bytes,
            mtime_unix,
            is_dir,
            kind,
            hidden,
            is_symlink,
            executable,
        });
    }
    Ok((out, hidden_count))
}
