use super::*;

/// Limits handed to every decoder reading untrusted bytes.
fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

/// Decodes what `reader` holds, refusing anything past [`MAX_DECODE_PIXELS`].
///
/// The dimensions come from the header, so an oversized image is turned away
/// before a single pixel is allocated for it.
pub(super) fn decode_bounded<R: std::io::BufRead + std::io::Seek>(
    mut reader: image::ImageReader<R>,
) -> Option<image::DynamicImage> {
    use image::ImageDecoder;
    reader.limits(decode_limits());
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_DECODE_PIXELS {
        return None;
    }
    image::DynamicImage::from_decoder(decoder).ok()
}

/// Runs `cmd` and returns its standard output, killing the child if it outlives
/// [`TOOL_TIMEOUT`]. `None` if it failed, was killed, or could not start.
///
/// The pipe is drained on its own thread. Polling the exit status while nobody
/// reads standard output would let a child fill its pipe buffer and block —
/// indistinguishable from the hang this is meant to catch, and it would turn
/// every large frame into a false timeout.
pub(super) fn run_bounded(mut cmd: Command, limit: Duration) -> Option<Vec<u8>> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    let drain = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain.join();
                return None;
            }
            Ok(None) => std::thread::sleep(TOOL_POLL),
            Err(_) => return None,
        }
    };
    let bytes = drain.join().ok()?;
    status.success().then_some(bytes)
}

/// On Windows, prevents a **console window** from opening (a "DOS" flash)
/// when launching a CLI (ffmpeg/poppler) from a GUI app. No-op elsewhere.
pub(super) fn no_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}
