# How Favnyr produces previews

This note records where a preview comes from on each system, and the single
function that decides it, so the behaviour stays in step with the README.

## The rule

A preview is **requested** for a row, then **produced**:

- **Requested** — `thumbnail_kind_for_row` (`crates/favnyr-gui/src/bridge.rs`)
  maps a row's `kind` code and extension to a request. On **Windows** it accepts
  every non-folder entry, letting the system decide; on other platforms it keeps
  a whitelist of the types Favnyr can actually render (image, video, MP3/FLAC,
  PDF). Its answer also drives `preview_capable`, and therefore the row geometry.
- **Produced** — `generate_thumb` (`crates/favnyr-gui/src/bridge.rs`) picks the
  source. On **Windows** the system shell thumbnail API is the single source of
  truth; Favnyr's own decoders are only a fallback.

## Windows: the system renders, Favnyr asks

`crates/favnyr-gui/src/winthumb.rs`:

- `shell_thumbnail` — `IShellItemImageFactory::GetImage` with
  `SIIGBF_THUMBNAILONLY | SIIGBF_RESIZETOFIT`. This is the path Explorer itself
  uses: the shell's thumbnail providers, backed by the OS thumbnail cache. It is
  asked first for **every** request, so anything Windows can preview — photos,
  video, PDF, Office documents, e-books, fonts, `.lnk` shortcuts… — shows up
  without Favnyr reimplementing a decoder. `THUMBNAILONLY` means a file the
  system has no thumbnail for returns nothing, and the row keeps its type icon.
- `pdf_thumbnail` — WinRT `Windows.Data.Pdf`, a last resort reached only when
  the shell gave no PDF thumbnail.

When the shell returns nothing, `generate_thumb` falls back to the in-house
decoders below (and to WinRT for PDFs).

## Linux (and the Windows fallback): Favnyr renders

`crates/favnyr-core/src/thumbnail.rs` (pure Rust, `path → pixels`, no disk
cache):

- images via the `image` crate; PSD embedded JPEG; Affinity embedded PNG;
- MP3/FLAC embedded cover art (ID3v2 `APIC` / FLAC `PICTURE`);
- video via the `ffmpeg` CLI, PDF via `pdftoppm`/`pdftocairo` (best-effort,
  Linux only — on Windows the shell/WinRT paths above take over);
- SVG is not here: Slint renders it on the GUI side.

## Caching

Favnyr keeps a bounded, session-only **memory** LRU (`thumb_cache`). It writes
nothing to disk; on Windows it benefits from the thumbnail cache the OS already
maintains.

## Change log

- **2026-09-28** — Windows previews unified on the system shell thumbnail API.
  Before: images/audio were decoded by Favnyr first (the shell was only a
  fallback for them), PDFs were rendered by WinRT, and only image/video/audio/PDF
  rows ever asked for a preview — so many types Windows can preview (Office
  documents, e-books, fonts, archives…) never got one. Now: `generate_thumb`
  asks the shell first for every file, `thumbnail_kind_for_row` accepts every
  non-folder entry on Windows, the `.lnk` special case and the now-unused
  `lnk_thumbnail_kind` were removed, and `FileKind::from_code` (core) was added
  to decode a row's kind code. Behaviour on Linux is unchanged.
