use super::*;

/// Thumbnail of a PDF's 1st page via the **poppler** tools (`pdftoppm` then
/// `pdftocairo`), best-effort and dependency-free (same spirit as `ffmpeg`).
/// `None` if the file isn't a `.pdf`, if no tool is present, or on
/// failure. Both tools write `<prefix>.png` with `-singlefile`.
pub fn from_pdf(path: &Path, max_px: u32) -> Option<Thumbnail> {
    // Strict filter: `generate` routes ALL `Document`s here; only PDF has a
    // render (avoids launching poppler on a .txt/.docx).
    let is_pdf = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return None;
    }
    // Unique output prefix in the temp folder → `<prefix>.png`.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let prefix = std::env::temp_dir().join(format!("favnyr-pdf-{stamp}"));
    let png = prefix.with_extension("png");

    // `-f 1 -l 1 -singlefile` = 1st page only; `-scale-to N` bounds the large side.
    let ran = run_pdf_tool("pdftoppm", path, &prefix, max_px)
        || run_pdf_tool("pdftocairo", path, &prefix, max_px);

    let result = if ran {
        std::fs::read(&png)
            .ok()
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| downscale(img, max_px))
    } else {
        None
    };
    let _ = std::fs::remove_file(&png); // best-effort cleanup
    result
}

/// Runs a poppler tool (`pdftoppm`/`pdftocairo`) to render `pdf`'s 1st page
/// into `<out_prefix>.png`. Returns `true` if the tool finished successfully.
fn run_pdf_tool(tool: &str, pdf: &Path, out_prefix: &Path, max_px: u32) -> bool {
    let mut cmd = Command::new(tool);
    cmd.args(["-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to"])
        .arg(max_px.to_string())
        .arg(pdf)
        .arg(out_prefix);
    no_console(&mut cmd);
    run_bounded(cmd, TOOL_TIMEOUT).is_some()
}
