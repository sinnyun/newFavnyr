use super::*;

/// Why a name cannot become a directory entry. Returned instead of a plain
/// `false` so the caller can say *what* is wrong: reporting "already taken" for
/// a name the filesystem simply refuses sends the user hunting for a
/// non-existent duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRejection {
    /// Empty, or nothing but whitespace.
    Empty,
    /// Holds a character the filesystem cannot store — see
    /// [`FORBIDDEN_NAME_CHARS`] and the control characters.
    ForbiddenChar,
    /// `.` or `..`, which already denote the folder and its parent.
    DotEntry,
    /// A name Windows keeps for a device, with or without an extension
    /// (`NUL`, `COM1`, `LPT1.txt`, …). Windows-only.
    ReservedDevice,
    /// Windows silently drops a trailing space or dot, so the entry would not
    /// carry the name that was typed. Windows-only.
    TrailingDotOrSpace,
}

/// Characters no entry name may contain on the running platform, as a
/// display-ready list. Every OS rejects its own separators; Windows adds the
/// set reserved by the Win32 naming rules.
///
/// Single source of truth: [`check_file_name`] tests against these very
/// characters, and the interface quotes the same string back to the user.
#[cfg(windows)]
pub const FORBIDDEN_NAME_CHARS: &str = "\\ / : * ? \" < > |";
#[cfg(not(windows))]
pub const FORBIDDEN_NAME_CHARS: &str = "/";

/// The characters above, in matchable form. `\` stays rejected everywhere for
/// the reason given on [`rename_in_place`]: a name is never a path.
const FORBIDDEN_CHARS: &[char] = if cfg!(windows) {
    &['/', '\\', ':', '*', '?', '"', '<', '>', '|']
} else {
    &['/', '\\']
};

/// Names Windows resolves to a device rather than a file, whatever the folder.
/// Creating one fails, or worse, writes to the device.
#[cfg(windows)]
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validates a name destined for the **filesystem**, with the running
/// platform's rules.
///
/// Deliberately stricter than [`is_valid_entry_name`], which stays the rule for
/// names that never reach the disk (favourite and container labels): a bookmark
/// may legitimately be called "Draft?" while a file may not.
pub fn check_file_name(name: &str) -> std::result::Result<(), NameRejection> {
    if name.is_empty() {
        return Err(NameRejection::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameRejection::DotEntry);
    }
    // Control characters are unusable on every filesystem and invisible in the
    // field, so they are reported with the other forbidden characters.
    if name.contains(FORBIDDEN_CHARS) || name.contains(|c: char| (c as u32) < 0x20) {
        return Err(NameRejection::ForbiddenChar);
    }
    #[cfg(windows)]
    {
        if name.ends_with(' ') || name.ends_with('.') {
            return Err(NameRejection::TrailingDotOrSpace);
        }
        // The device is matched on the stem: "NUL.txt" is the NUL device too.
        let stem = name.split('.').next().unwrap_or(name);
        if RESERVED_DEVICE_NAMES
            .iter()
            .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        {
            return Err(NameRejection::ReservedDevice);
        }
    }
    Ok(())
}

/// [`check_file_name`] reduced to a yes/no, for the call sites that only gate
/// an operation and have no message to build.
pub fn is_valid_file_name(name: &str) -> bool {
    check_file_name(name).is_ok()
}

/// Creates an entry (a folder if `is_dir`, otherwise an empty file) named `name`
/// in `parent`. Rejects an invalid name ([`check_file_name`]) and a target
/// that **already exists** (never overwrites). Returns the created path.
pub fn create_entry(parent: &Path, name: &str, is_dir: bool) -> Result<PathBuf> {
    let name = name.trim();
    if let Err(reason) = check_file_name(name) {
        return Err(Error::Workspace(format!(
            "invalid name: {name:?} ({reason:?})"
        )));
    }
    let target = parent.join(name);
    // `exists()` ignores broken symlinks. `symlink_metadata`, by contrast,
    // treats any directory entry as occupied, which is exactly the rule
    // expected for a creation without replacement.
    match std::fs::symlink_metadata(&target) {
        Ok(_) => {
            return Err(Error::Workspace(format!(
                "{} already exists",
                target.display()
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    if is_dir {
        std::fs::create_dir(&target)?;
    } else {
        // `create_new` also closes the race window between the check above
        // and the creation: a file that appears in the meantime is never
        // truncated, even on a share watched by another application.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
    }
    Ok(target)
}

// ---------- Unique name generation ----------

/// Starting from `original`, finds a free path in the same folder by
/// suffixing `- Copy01`, `- Copy02`, … **after** the name (before a real
/// extension):
///   - `notes.txt`   → `notes - Copy01.txt`, `notes - Copy02.txt`, …
///   - `MyFile 5.7`  → `MyFile 5.7 - Copy01` (`.7` is NOT an extension)
///   - `folder`      → `folder - Copy01`, …
///   - `notes - Copy01.txt` (already a copy) → `notes - Copy02.txt`
///     (suffixes are not stacked `- CopyNN`)
///
/// Returns the **target path** (which does not exist yet at computation time).
/// No anti-race guarantee: to create it, the caller follows up with
/// `copy()` / `rename()` (atomic operations on the OS side).
pub fn unique_sibling(original: &Path) -> PathBuf {
    unique_sibling_where(original, |p| p.exists())
}

/// `unique_sibling` with a caller-supplied notion of an occupied name.
///
/// The default only knows the filesystem, which is not enough when several
/// operations run at once: a name can be free on disk yet already claimed as
/// the destination of a copy still in flight. Callers pass a `taken` that
/// covers those as well, so two concurrent operations never pick the same
/// target.
pub fn unique_sibling_where(original: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let parent = original.parent().unwrap_or(Path::new("."));
    let file_name = original.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // If the original name is already free, keep it as is.
    let candidate = parent.join(file_name);
    if !taken(&candidate) {
        return candidate;
    }

    let (stem, ext) = split_name(file_name);
    let base = strip_copy_suffix(&stem);

    for n in 1..=10_000 {
        let p = parent.join(compose_copy(&base, &ext, n));
        if !taken(&p) {
            return p;
        }
    }
    // Pathological case: 10,000 collisions → timestamped suffix, likely free.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let new_name = if ext.is_empty() {
        format!("{base} - Copy{stamp}")
    } else {
        format!("{base} - Copy{stamp}.{ext}")
    };
    parent.join(new_name)
}

/// Composes `base - CopyNN[.ext]` (NN zero-padded to 2 digits, ≥ 01).
fn compose_copy(base: &str, ext: &str, n: usize) -> String {
    if ext.is_empty() {
        format!("{base} - Copy{n:02}")
    } else {
        format!("{base} - Copy{n:02}.{ext}")
    }
}

/// Strips a trailing `- CopyNN` suffix (NN = digits) to avoid stacking copy
/// markers (`x - Copy01` copied again → `x - Copy02`, not
/// `x - Copy01 - Copy01`).
pub(super) fn strip_copy_suffix(stem: &str) -> String {
    if let Some(pos) = stem.rfind(" - Copy") {
        let after = &stem[pos + " - Copy".len()..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
            return stem[..pos].to_string();
        }
    }
    stem.to_string()
}

/// Splits `name` into `(stem, ext)`. Returns `(name, "")` if:
///   - the name starts with `.` (e.g. `.bashrc`),
///   - there is no dot,
///   - or the segment after the last dot contains **no letters**
///     (e.g. `MyFile 5.7`, `data.2024` → `.7`/`.2024` are not extensions).
///
/// So `.jpg`, `.7z`, `.mp3` remain extensions, but not `.7`.
/// Public: reused by the GUI to colorize the extension in the list.
pub fn split_name(name: &str) -> (String, String) {
    if name.starts_with('.') {
        return (name.to_string(), String::new());
    }
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.chars().any(|c| c.is_ascii_alphabetic()) => {
            (s.to_string(), e.to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

/// Extension of a file name FOR TYPING PURPOSES (the "ext" column, sorting by
/// type, filtering by extension), lowercased. Unlike [`split_name`] (dedicated
/// to DISPLAY, which leaves dotfiles whole), treats a dotfile's suffix as an
/// extension: `.gitignore` → `gitignore`, `.config.json` → `json`.
/// Same "at least one letter" guard as `split_name` (`data.2024` → `""`).
/// Empty if there is no dot or the suffix is not relevant.
pub fn ext_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, e)) if e.chars().any(|c| c.is_ascii_alphabetic()) => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}
