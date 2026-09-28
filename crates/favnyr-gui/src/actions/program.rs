use super::*;

// ----- Internal helpers (Linux only) -----

#[cfg(not(windows))]
pub(crate) fn pick_terminal() -> Option<String> {
    if let Ok(t) = std::env::var("TERMINAL") {
        if !t.is_empty() && which(&t) {
            return Some(t);
        } else if !t.is_empty() {
            tracing::warn!(term = %t, "$TERMINAL set but not found, falling back");
        }
    }
    const CANDIDATES: &[&str] = &[
        "kgx",            // GNOME Console
        "konsole",        // KDE
        "gnome-terminal", // legacy GNOME
        "kitty",
        "alacritty",
        "foot",
        "wezterm",
        "xfce4-terminal",
        "lxterminal",
        "tilix",
        "xterm",
    ];
    CANDIDATES
        .iter()
        .find(|c| which(c))
        .map(|c| (*c).to_string())
}

#[cfg(not(windows))]
pub(super) fn which(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(cmd);
        candidate.is_file() || candidate.is_symlink()
    })
}

/// First of `candidates` that can actually be launched, or `None`.
///
/// Drives the ready-made commands offered in the settings: they are only
/// listed when their tool is really installed, so the list never advertises
/// something the machine cannot run.
///
/// A single list serves both platforms. Bare names resolve through PATH
/// outside Windows and fail the file test on it; absolute Windows paths do the
/// reverse. Each entry is therefore silently skipped where it makes no sense.
pub fn resolve_program(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|c| program_is_valid(c))
        .map(|c| (*c).to_string())
}

/// Is an opener's "Program" field launchable? An EXISTING file path
/// (Windows case, where one browses to an `.exe`), OR — especially on
/// Linux — a simple COMMAND resolvable via `PATH` (`viewer`, `editor`…).
///
/// A valid program isn't always a file path: a PATH command
/// entered as-is (`viewer`, `editor`) is valid on Linux without
/// being an existing file, unlike a Windows executable which is always
/// resolved to an absolute path.
pub fn program_is_valid(program: &str) -> bool {
    let p = program.trim();
    if p.is_empty() {
        return false;
    }
    if Path::new(p).is_file() {
        return true;
    }
    // Bare name (no path separator) → resolved via PATH, outside Windows
    // (where an opener command is, by convention, an absolute path to the exe).
    #[cfg(not(windows))]
    if !p.contains('/') {
        return which(p);
    }
    false
}
