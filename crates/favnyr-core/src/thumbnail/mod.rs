//! On-the-fly preview (thumbnail) generation.
//!
//! - **Images**: **100% Rust** decoding via the `image` crate (no C, no
//!   network). PSD files get an additional best-effort path that extracts
//!   their embedded Photoshop JPEG thumbnail without rendering layers.
//!   Affinity files similarly expose a bounded embedded PNG without decoding
//!   the proprietary document. Other formats that can't be decoded (RAW,
//!   sometimes HEIF/AVIF…) → `None` → the caller falls back to the type icon.
//! - **MP3 / FLAC**: extracts embedded ID3v2 `APIC`/`PIC` or FLAC `PICTURE`
//!   cover art and decodes it with the existing image pipeline. Only bounded
//!   metadata is read; the audio stream itself is never decoded.
//! - **Videos**: frame extraction via the **`ffmpeg` CLI** in a **single
//!   process** (best-effort, same spirit as `gio trash` / `xdg-open`). The
//!   `thumbnail` filter picks a **representative** frame (avoids the black
//!   screen at the start, without a separate `ffprobe` probe) and `scale`
//!   bounds the largest side **inside** ffmpeg → no full-resolution PNG
//!   re-decoded on the Rust side. Falls back to the icon if `ffmpeg` is
//!   absent or fails. On Windows, the process is launched without a console
//!   window (`no_console`).
//! - **PDF**: renders the 1st page via the **`pdftoppm`** then
//!   **`pdftocairo`** CLIs (poppler-utils, same spirit as `ffmpeg` —
//!   best-effort, no dependency).
//! - **SVG**: NOT here — rendered natively by Slint on the GUI side (see
//!   `bridge`), because `image` doesn't decode SVG and Slint already embeds
//!   an SVG engine (resvg).
//!
//! **Windows (standalone)**: the GUI does NOT call these CLIs for video/PDF;
//! it uses the **shell's thumbnail API** (`IShellItemImageFactory`, see
//! `favnyr-gui/src/winthumb.rs`) — OS-provided, no external binary or
//! console window. The CLIs above are therefore only used on Linux.
//!
//! **No disk cache**: these functions are pure (path in → pixels out).
//! Memoization (in-memory LRU cache, session only) is handled by the calling
//! layer (GUI).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::fs::FileKind;

/// Most pixels an image may declare before its thumbnail is refused.
///
/// A crafted file can announce enormous dimensions in a few hundred bytes; the
/// decoder would then try to materialise them. This is checked against the
/// HEADER, so such a file costs nothing beyond the header read. The crate's own
/// allocation ceiling is documented as non-strict — some decoders ignore it —
/// which is why the real bound is enforced here rather than delegated.
///
/// Eighty megapixels sits well above the largest consumer sensor of the day
/// (61 Mpx) and above most scans, and caps one decode near 320 MB. Above it the
/// entry falls back to its type icon, exactly as a format the crate cannot
/// decode already does. The thumbnail workers run in parallel, so the figure is
/// paid several times over — that is what makes the ceiling worth having.
const MAX_DECODE_PIXELS: u64 = 80_000_000;

/// Allocation ceiling handed to the decoder, aligned with the pixel budget
/// above (80 Mpx in RGBA8 needs ~320 MB, plus room for the decoder's own
/// scratch). Best-effort by design, hence the strict check beside it.
const MAX_DECODE_ALLOC: u64 = 384 * 1024 * 1024;

/// How long an external rendering tool may run before it is killed.
///
/// `Command::output()` waits without any bound: a crafted file can keep the
/// tool spinning forever, and the thumbnail queue runs several workers, so a
/// handful of such files would starve it entirely. Generous for real content —
/// extracting one frame after an input seek takes well under a second.
const TOOL_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the bounded wait checks whether the tool has finished.
const TOOL_POLL: Duration = Duration::from_millis(25);

mod affinity;
mod audio;
mod decode;
mod pdf;
mod psd;
mod video;

#[cfg(test)]
mod tests;

use affinity::*;
pub use audio::*;
use decode::*;
pub use pdf::*;
use psd::*;
pub use video::*;

/// Decoded thumbnail ready for display: interleaved RGBA8 (`rgba.len() ==
/// width * height * 4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Generates a thumbnail for `path` based on its type. `max_px` bounds the
/// largest side (aspect is preserved). Returns `None` if the type has no
/// preview or if generation fails (→ icon fallback on the caller's side).
pub fn generate(path: &Path, kind: FileKind, max_px: u32) -> Option<Thumbnail> {
    match kind {
        FileKind::Image => from_image(path, max_px),
        FileKind::Audio => from_audio(path, max_px),
        FileKind::Video => from_video(path, max_px),
        // PDFs are classified as `Document`; `from_pdf` filters on the extension
        // (other documents — doc/txt/… — have no preview).
        FileKind::Document => from_pdf(path, max_px),
        _ => None,
    }
}

/// Metadata of an image read from the HEADER only (no pixel decoding):
/// `(width, height, color bits per pixel, alpha present)`. When an
/// alpha channel exists, its depth is subtracted from the third element: an
/// RGBA8 image therefore returns `(width, height, 24, true)`. `None` if the
/// format isn't handled by `image` or the file is unreadable. Fast
/// (a few KB read) → suited to filling columns on the fly.
pub fn image_meta(path: &Path) -> Option<(u32, u32, u16, bool)> {
    use image::ImageDecoder;
    let decoder = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (w, h) = decoder.dimensions();
    let color = decoder.color_type();
    let has_alpha = color.has_alpha();
    let total_bits = color.bits_per_pixel();
    // The ColorType variants exposed by `image` have channels of equal depth
    // (RGBA8, RGBA16, RGBA32F, LA8, LA16). Removing one channel therefore gives
    // the correct color depth, without counting alpha twice in the display.
    let color_bits = if has_alpha {
        total_bits - total_bits / u16::from(color.channel_count())
    } else {
        total_bits
    };
    Some((w, h, color_bits, has_alpha))
}

/// Decodes ENCODED image bytes (PNG/BMP/JPEG…) into a thumbnail bounded to
/// `max_px`. Used for renders produced outside `image` — e.g. a PDF page rendered
/// by the WinRT API on Windows (cf. `favnyr-gui/src/winthumb.rs`).
pub fn from_encoded(bytes: &[u8], max_px: u32) -> Option<Thumbnail> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    Some(downscale(decode_bounded(reader)?, max_px))
}

/// Thumbnail of an image file (pure-Rust decoding). `None` if the format
/// isn't supported / the file is unreadable.
pub fn from_image(path: &Path, max_px: u32) -> Option<Thumbnail> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("psd") {
        return from_psd_thumbnail(path, max_px);
    }
    if is_affinity_extension(extension) {
        return from_affinity_thumbnail(path, max_px);
    }

    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    Some(downscale(decode_bounded(reader)?, max_px))
}

/// Shrinks `img` to fit within `max_px × max_px` (aspect preserved) and produces
/// RGBA8. `DynamicImage::thumbnail` is fast (suited to generating many
/// thumbnails).
fn downscale(img: image::DynamicImage, max_px: u32) -> Thumbnail {
    let small = img.thumbnail(max_px, max_px).to_rgba8();
    Thumbnail {
        width: small.width(),
        height: small.height(),
        rgba: small.into_raw(),
    }
}
