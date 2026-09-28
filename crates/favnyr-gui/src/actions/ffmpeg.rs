use super::*;

/// Is `ffmpeg` available to generate video thumbnails? On Windows,
/// video goes through the native shell (never `ffmpeg`) → always "yes"
/// (no warning to show). On Linux, resolved via `PATH`.
pub fn ffmpeg_available() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        which("ffmpeg")
    }
}

/// State of `ffmpeg` for the Linux "Video thumbnails" settings section.
/// `detected_distro`: index of the install command to highlight
/// (0 Debian/Ubuntu · 1 Fedora · 2 Arch/Manjaro · 3 Alpine · -1 unknown).
#[derive(Clone, Debug, Default)]
pub struct FfmpegInfo {
    pub available: bool,
    pub version: String,
    pub flatpak: bool,
    pub detected_distro: i32,
}

/// Detects `ffmpeg` (presence + version), the Flatpak sandbox, and the
/// distribution family. Near-instant computations (a `which`, a `-version`, reading
/// `/etc/os-release`) → can be called when settings open and on the
/// "Recheck" button. On Windows, `ffmpeg` is never used: `available`
/// stays true and the rest is empty (the section isn't shown there anyway).
pub fn ffmpeg_info() -> FfmpegInfo {
    let available = ffmpeg_available();
    FfmpegInfo {
        available,
        version: if available {
            ffmpeg_version().unwrap_or_default()
        } else {
            String::new()
        },
        flatpak: in_flatpak(),
        detected_distro: detect_distro_index(),
    }
}

/// `ffmpeg` version cleaned up for display ("8.1.2"), read via
/// `ffmpeg -version`. Tolerates distro package prefixes/suffixes
/// ("n7.0.2" → "7.0.2", "4.4.2-0ubuntu…" → "4.4.2"). `None` if absent.
fn ffmpeg_version() -> Option<String> {
    // Windows never uses ffmpeg → no spawn (settings also open on
    // Windows and trigger re-detection).
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("ffmpeg").arg("-version").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let tok = stdout
            .lines()
            .next()?
            .strip_prefix("ffmpeg version ")?
            .split_whitespace()
            .next()?;
        // Strips a leading "n" (Arch: "n7.0.2") then cuts at the 1st "-".
        let tok = tok
            .strip_prefix('n')
            .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or(tok);
        let clean = tok.split('-').next().unwrap_or(tok);
        (!clean.is_empty()).then(|| clean.to_string())
    }
}

/// Is Favnyr running inside a **Flatpak** sandbox? In that case, `ffmpeg`
/// comes from the runtime: suggesting `apt`/`dnf`… on the host would be misleading.
fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists()
}

/// Distribution family via `/etc/os-release` → install command index,
/// or -1 if unrecognized. Usefully a no-op outside Linux (file absent).
fn detect_distro_index() -> i32 {
    distro_index_from(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
}

/// Resolves an `os-release`'s content into a command index (0 Debian · 1 Fedora ·
/// 2 Arch · 3 Alpine · -1 unknown). Tests `ID` first, then the `ID_LIKE`
/// tokens IN ORDER → Linux Mint (`ID_LIKE="ubuntu debian"`) lands on apt.
/// Pure (testable), separate from the disk read.
pub(super) fn distro_index_from(content: &str) -> i32 {
    let field = |key: &str| -> String {
        content
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().trim_matches('"').to_ascii_lowercase())
            .unwrap_or_default()
    };
    let id = field("ID=");
    let id_like = field("ID_LIKE=");
    for tok in std::iter::once(id.as_str()).chain(id_like.split_whitespace()) {
        match tok {
            "debian" | "ubuntu" => return 0,
            "fedora" | "rhel" | "centos" => return 1,
            "arch" | "manjaro" => return 2,
            "alpine" => return 3,
            _ => {}
        }
    }
    -1
}
