use super::*;

fn tempdir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "favnyr-thumb-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn synchsafe_bytes(value: usize) -> [u8; 4] {
    assert!(value < (1 << 28));
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn encoded_cover(format: image::ImageFormat, rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(20, 10, image::Rgba(rgba));
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, format)
        .unwrap();
    cursor.into_inner()
}

fn id3v23_frame(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut frame = id.to_vec();
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]); // status + format flags
    frame.extend_from_slice(payload);
    frame
}

fn id3v23_picture(picture_type: u8, encoded: &[u8]) -> Vec<u8> {
    let mut payload = vec![0]; // ISO-8859-1 description
    payload.extend_from_slice(b"image/png\0");
    payload.push(picture_type);
    payload.push(0); // empty description
    payload.extend_from_slice(encoded);
    id3v23_frame(b"APIC", &payload)
}

fn write_id3_file(path: &Path, version: u8, flags: u8, tag: &[u8]) {
    let mut file = b"ID3".to_vec();
    file.extend_from_slice(&[version, 0, flags]);
    file.extend_from_slice(&synchsafe_bytes(tag.len()));
    file.extend_from_slice(tag);
    file.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]); // inert MPEG-like tail
    std::fs::write(path, file).unwrap();
}

fn flac_picture(picture_type: u32, encoded: &[u8]) -> Vec<u8> {
    let mut block = picture_type.to_be_bytes().to_vec();
    block.extend_from_slice(&(b"image/png".len() as u32).to_be_bytes());
    block.extend_from_slice(b"image/png");
    block.extend_from_slice(&0u32.to_be_bytes()); // empty description
    block.extend_from_slice(&20u32.to_be_bytes()); // width
    block.extend_from_slice(&10u32.to_be_bytes()); // height
    block.extend_from_slice(&32u32.to_be_bytes()); // color depth
    block.extend_from_slice(&0u32.to_be_bytes()); // not indexed
    block.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
    block.extend_from_slice(encoded);
    block
}

fn write_flac_file(path: &Path, blocks: &[(u8, Vec<u8>)]) {
    assert!(!blocks.is_empty());
    let mut file = b"fLaC".to_vec();
    for (index, (block_type, payload)) in blocks.iter().enumerate() {
        assert!(*block_type < 0x80 && payload.len() <= 0x00ff_ffff);
        let last = index + 1 == blocks.len();
        file.push(*block_type | if last { 0x80 } else { 0 });
        file.extend_from_slice(&[
            ((payload.len() >> 16) & 0xff) as u8,
            ((payload.len() >> 8) & 0xff) as u8,
            (payload.len() & 0xff) as u8,
        ]);
        file.extend_from_slice(payload);
    }
    file.extend_from_slice(&[0xff, 0xf8, 0x69]); // inert FLAC-like audio tail
    std::fs::write(path, file).unwrap();
}

fn psd_thumbnail_data(encoded: &[u8]) -> Vec<u8> {
    let mut data = 1u32.to_be_bytes().to_vec(); // JPEG thumbnail
    data.extend_from_slice(&20u32.to_be_bytes()); // width
    data.extend_from_slice(&10u32.to_be_bytes()); // height
    data.extend_from_slice(&60u32.to_be_bytes()); // padded row bytes
    data.extend_from_slice(&600u32.to_be_bytes()); // uncompressed size
    data.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
    data.extend_from_slice(&24u16.to_be_bytes()); // bits per pixel
    data.extend_from_slice(&1u16.to_be_bytes()); // planes
    data.extend_from_slice(encoded);
    data
}

fn psd_resource(id: u16, data: &[u8]) -> Vec<u8> {
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&id.to_be_bytes());
    resource.extend_from_slice(&[0, 0]); // empty Pascal name and padding
    resource.extend_from_slice(&(data.len() as u32).to_be_bytes());
    resource.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        resource.push(0);
    }
    resource
}

fn write_psd_file(path: &Path, resources: &[Vec<u8>]) {
    let resources_len: usize = resources.iter().map(Vec::len).sum();
    let mut file = b"8BPS".to_vec();
    file.extend_from_slice(&1u16.to_be_bytes()); // PSD, not PSB
    file.extend_from_slice(&[0; 6]);
    file.extend_from_slice(&3u16.to_be_bytes()); // channels
    file.extend_from_slice(&10u32.to_be_bytes()); // height
    file.extend_from_slice(&20u32.to_be_bytes()); // width
    file.extend_from_slice(&8u16.to_be_bytes()); // channel depth
    file.extend_from_slice(&3u16.to_be_bytes()); // RGB
    file.extend_from_slice(&0u32.to_be_bytes()); // color mode data
    file.extend_from_slice(&(resources_len as u32).to_be_bytes());
    for resource in resources {
        file.extend_from_slice(resource);
    }
    std::fs::write(path, file).unwrap();
}

fn patterned_png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([
            x.wrapping_mul(17) as u8,
            y.wrapping_mul(29) as u8,
            x.wrapping_mul(11).wrapping_add(y.wrapping_mul(7)) as u8,
            255,
        ])
    });
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}

#[test]
fn video_seek_targets_thirty_percent_with_safe_fallback() {
    // Known duration → 30% of the timeline.
    assert_eq!(video_seek_offset(Some(100.0)), 30.0);
    assert_eq!(video_seek_offset(Some(10.0)), 3.0);
    // Unknown / invalid duration → flat 5s intro skip (value
    // hardcoded in the assertion: an accidental change must fail the test).
    assert_eq!(VIDEO_THUMB_SKIP_SECS, 5.0);
    assert_eq!(video_seek_offset(None), 5.0);
    assert_eq!(video_seek_offset(Some(0.0)), 5.0);
    assert_eq!(video_seek_offset(Some(-5.0)), 5.0);
    assert_eq!(video_seek_offset(Some(f64::NAN)), 5.0);
    assert_eq!(video_seek_offset(Some(f64::INFINITY)), 5.0);
}

/// Builds a minimal MP4 `ftyp` + `moov`/`mvhd` (version 0) carrying
/// `timescale`/`duration`, to validate box traversal without depending
/// on a committed binary file or on ffmpeg.
fn write_fake_mp4(path: &std::path::Path, timescale: u32, duration: u32) {
    fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }
    let mut mvhd = vec![0u8; 4]; // version 0 + flags
    mvhd.extend_from_slice(&0u32.to_be_bytes()); // creation
    mvhd.extend_from_slice(&0u32.to_be_bytes()); // modification
    mvhd.extend_from_slice(&timescale.to_be_bytes());
    mvhd.extend_from_slice(&duration.to_be_bytes());
    mvhd.extend_from_slice(&[0u8; 80]); // rest of mvhd (ignored)

    let mut file = boxed(b"ftyp", b"isom\0\0\x02\0isomiso2");
    // Large `mdat` BEFORE `moov`: traversal must skip it cleanly.
    file.extend_from_slice(&boxed(b"mdat", &vec![0u8; 4096]));
    file.extend_from_slice(&boxed(b"moov", &boxed(b"mvhd", &mvhd)));
    std::fs::write(path, file).unwrap();
}

#[test]
fn mp4_duration_read_from_header_without_spawning_ffprobe() {
    let dir = tempdir();
    // 19.064s at scale 1000 (real-world case of a common MP4).
    let p = dir.join("clip.mp4");
    write_fake_mp4(&p, 1000, 19_064);
    let d = mp4_duration_secs(&p).expect("duration readable from the header");
    assert!((d - 19.064).abs() < 1e-6, "duration read = {d}");
    // The targeted seek is indeed 30% of this duration.
    assert!((video_seek_offset(Some(d)) - 5.7192).abs() < 1e-4);

    // Fragmented MP4 (zero duration in mvhd) → None (ffprobe fallback on the caller's side).
    let frag = dir.join("frag.mp4");
    write_fake_mp4(&frag, 1000, 0);
    assert_eq!(mp4_duration_secs(&frag), None);

    // Non-ISO-BMFF file → None, without panicking or looping.
    let txt = dir.join("not.mp4");
    std::fs::write(&txt, b"definitely not an mp4 file").unwrap();
    assert_eq!(mp4_duration_secs(&txt), None);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_thumbnail_preserves_aspect_and_bounds() {
    let dir = tempdir();
    let path = dir.join("red.png");
    // 100×50 opaque red, encoded as PNG via `image`.
    let buf = image::RgbaImage::from_pixel(100, 50, image::Rgba([255, 0, 0, 255]));
    buf.save(&path).unwrap();

    let thumb = from_image(&path, 32).expect("decode");
    // 2:1 aspect preserved, bounded to 32 → 32×16.
    assert!(thumb.width <= 32 && thumb.height <= 32);
    assert_eq!(thumb.width, 32);
    assert_eq!(thumb.height, 16);
    assert_eq!(thumb.rgba.len() as u32, thumb.width * thumb.height * 4);
    // Red pixel preserved.
    assert_eq!(&thumb.rgba[0..4], &[255, 0, 0, 255]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mp3_thumbnail_prefers_the_front_cover_without_decoding_audio() {
    let dir = tempdir();
    let path = dir.join("song.mp3");
    let blue = encoded_cover(image::ImageFormat::Png, [0, 0, 255, 255]);
    let red = encoded_cover(image::ImageFormat::Png, [255, 0, 0, 255]);
    let mut tag = id3v23_picture(4, &blue); // back cover comes first
    tag.extend_from_slice(&id3v23_picture(3, &red)); // front cover wins
    write_id3_file(&path, 3, 0, &tag);

    let thumb = from_audio(&path, 12).expect("embedded front cover");
    assert_eq!((thumb.width, thumb.height), (12, 6));
    assert_eq!(&thumb.rgba[..4], &[255, 0, 0, 255]);
    assert!(generate(&path, FileKind::Audio, 12).is_some());

    // Dispatch remains strict even if another extension happens to begin
    // with bytes that look like an ID3 tag.
    let flac = dir.join("song.flac");
    std::fs::copy(&path, &flac).unwrap();
    assert!(from_audio(&flac, 12).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn flac_thumbnail_prefers_front_cover_and_skips_audio_frames() {
    let dir = tempdir();
    let path = dir.join("song.flac");
    let blue = encoded_cover(image::ImageFormat::Png, [0, 0, 255, 255]);
    let red = encoded_cover(image::ImageFormat::Png, [255, 0, 0, 255]);
    write_flac_file(
        &path,
        &[
            (0, vec![0; 34]),            // mandatory STREAMINFO
            (6, flac_picture(4, &blue)), // back cover comes first
            (6, flac_picture(3, &red)),  // front cover wins
        ],
    );

    let thumb = from_audio(&path, 12).expect("embedded FLAC front cover");
    assert_eq!((thumb.width, thumb.height), (12, 6));
    assert_eq!(&thumb.rgba[..4], &[255, 0, 0, 255]);
    assert!(generate(&path, FileKind::Audio, 12).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn psd_thumbnail_prefers_the_modern_embedded_resource() {
    let dir = tempdir();
    let path = dir.join("artwork.PSD");
    let blue = encoded_cover(image::ImageFormat::Jpeg, [0, 0, 255, 255]);
    let red = encoded_cover(image::ImageFormat::Jpeg, [255, 0, 0, 255]);
    write_psd_file(
        &path,
        &[
            psd_resource(PSD_THUMBNAIL_RESOURCE_V4, &psd_thumbnail_data(&blue)),
            psd_resource(PSD_THUMBNAIL_RESOURCE_V5, &psd_thumbnail_data(&red)),
        ],
    );

    let thumb = from_image(&path, 12).expect("embedded PSD thumbnail");
    assert_eq!((thumb.width, thumb.height), (12, 6));
    assert!(thumb.rgba[0] > 200 && thumb.rgba[2] < 40);
    assert!(generate(&path, FileKind::Image, 12).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn psd_thumbnail_accepts_the_legacy_resource_as_a_fallback() {
    let dir = tempdir();
    let path = dir.join("legacy.psd");
    let cover = encoded_cover(image::ImageFormat::Jpeg, [20, 180, 70, 255]);
    write_psd_file(
        &path,
        &[psd_resource(
            PSD_THUMBNAIL_RESOURCE_V4,
            &psd_thumbnail_data(&cover),
        )],
    );

    assert!(from_image(&path, 16).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_or_oversized_psd_thumbnail_falls_back_safely() {
    let dir = tempdir();
    let coverless = dir.join("coverless.psd");
    write_psd_file(&coverless, &[]);
    assert!(from_image(&coverless, 32).is_none());

    let truncated = dir.join("truncated.psd");
    let mut bytes = std::fs::read(&coverless).unwrap();
    bytes[30..34].copy_from_slice(&100u32.to_be_bytes());
    std::fs::write(&truncated, bytes).unwrap();
    assert!(from_image(&truncated, 32).is_none());

    let mut header = [0u8; 28];
    header[..4].copy_from_slice(&1u32.to_be_bytes());
    header[20..24].copy_from_slice(&((MAX_PSD_THUMBNAIL_BYTES + 1) as u32).to_be_bytes());
    assert!(psd_thumbnail_jpeg_len(&header, u64::MAX).is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn affinity_thumbnail_streams_across_buffers_and_selects_the_smallest_png() {
    let dir = tempdir();
    let path = dir.join("drawing.AFPHOTO");
    let large = patterned_png(128, 64);
    let small = encoded_cover(image::ImageFormat::Png, [240, 30, 10, 255]);
    assert!(small.len() < large.len());

    // Start the first signature three bytes before the scan-buffer edge to
    // exercise matching across reads, then place the actual thumbnail later.
    let mut affinity = vec![0x55; AFFINITY_SCAN_BUFFER_BYTES - 3];
    affinity.extend_from_slice(&large);
    affinity.extend_from_slice(b"proprietary document records");
    affinity.extend_from_slice(&small);
    std::fs::write(&path, affinity).unwrap();

    let thumb = from_image(&path, 12).expect("embedded Affinity thumbnail");
    assert_eq!((thumb.width, thumb.height), (12, 6));
    assert_eq!(&thumb.rgba[..4], &[240, 30, 10, 255]);
    assert!(generate(&path, FileKind::Image, 12).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_affinity_png_signature_falls_back_without_decoding() {
    let dir = tempdir();
    for extension in ["af", "afdesign", "afphoto", "afpub"] {
        let path = dir.join(format!("broken.{extension}"));
        let mut bytes = b"Affinity-like prefix".to_vec();
        bytes.extend_from_slice(PNG_SIGNATURE);
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT"); // a PNG must begin with IHDR
        bytes.extend_from_slice(&[0; 17]);
        std::fs::write(&path, bytes).unwrap();
        assert!(from_image(&path, 32).is_none(), "{extension}");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_or_coverless_flac_falls_back_without_large_allocation() {
    let dir = tempdir();
    let coverless = dir.join("coverless.flac");
    write_flac_file(&coverless, &[(0, vec![0; 34])]);
    assert!(from_audio(&coverless, 32).is_none());

    // The largest representable FLAC block would cross Favnyr's bounded
    // metadata budget. Its payload is deliberately absent: rejection must
    // happen from the four-byte header, before any allocation or read.
    let oversized = dir.join("oversized.flac");
    std::fs::write(&oversized, b"fLaC\x86\xff\xff\xff").unwrap();
    assert!(from_audio(&oversized, 32).is_none());

    // A URL-valued PICTURE block is valid FLAC metadata but must never
    // trigger network access in Favnyr's fully local thumbnail pipeline.
    let mut remote = 3u32.to_be_bytes().to_vec();
    remote.extend_from_slice(&3u32.to_be_bytes());
    remote.extend_from_slice(b"-->");
    remote.extend_from_slice(&0u32.to_be_bytes());
    remote.extend_from_slice(&[0; 16]);
    remote.extend_from_slice(&19u32.to_be_bytes());
    remote.extend_from_slice(b"https://example.test");
    assert!(flac_picture_data(&remote).is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mp3_thumbnail_supports_id3v22_pic_and_tag_unsynchronization() {
    let dir = tempdir();

    let png = encoded_cover(image::ImageFormat::Png, [12, 34, 56, 255]);
    let mut pic_payload = vec![0];
    pic_payload.extend_from_slice(b"PNG");
    pic_payload.push(3);
    pic_payload.push(0);
    pic_payload.extend_from_slice(&png);
    let mut pic_frame = b"PIC".to_vec();
    let size = pic_payload.len();
    pic_frame.extend_from_slice(&[
        ((size >> 16) & 0xff) as u8,
        ((size >> 8) & 0xff) as u8,
        (size & 0xff) as u8,
    ]);
    pic_frame.extend_from_slice(&pic_payload);
    let v22 = dir.join("old.mp3");
    write_id3_file(&v22, 2, 0, &pic_frame);
    assert!(from_audio(&v22, 16).is_some());

    // JPEG contains 0xff marker bytes. ID3 tag-level unsynchronization
    // inserts a zero after each one; decoding must remove it before image
    // detection while keeping the original APIC frame size meaningful.
    let jpeg = encoded_cover(image::ImageFormat::Jpeg, [90, 140, 190, 255]);
    let frame = id3v23_picture(3, &jpeg);
    let mut unsynchronized = Vec::with_capacity(frame.len());
    for byte in frame {
        unsynchronized.push(byte);
        if byte == 0xff {
            unsynchronized.push(0);
        }
    }
    let v23_unsync = dir.join("unsync.mp3");
    write_id3_file(&v23_unsync, 3, 0x80, &unsynchronized);
    assert!(from_audio(&v23_unsync, 16).is_some());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_or_coverless_mp3_falls_back_without_large_allocation() {
    let dir = tempdir();
    let coverless = dir.join("coverless.mp3");
    let title = id3v23_frame(b"TIT2", b"\0A song");
    write_id3_file(&coverless, 3, 0, &title);
    assert!(from_audio(&coverless, 32).is_none());

    let oversized = dir.join("oversized.mp3");
    let mut header = b"ID3".to_vec();
    header.extend_from_slice(&[3, 0, 0]);
    header.extend_from_slice(&synchsafe_bytes(MAX_AUDIO_METADATA_BYTES + 1));
    std::fs::write(&oversized, header).unwrap();
    assert!(from_audio(&oversized, 32).is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_meta_reads_dimensions_and_depth() {
    let dir = tempdir();
    let path = dir.join("rgba.png");
    // 64×32 RGBA8: 24 color bits + one separate alpha channel.
    image::RgbaImage::from_pixel(64, 32, image::Rgba([1, 2, 3, 4]))
        .save(&path)
        .unwrap();
    let (w, h, bits, alpha) = image_meta(&path).expect("metadata readable");
    assert_eq!((w, h), (64, 32));
    assert_eq!(bits, 24);
    assert!(alpha);
    // A non-image file → None.
    let txt = dir.join("x.txt");
    std::fs::write(&txt, b"nope").unwrap();
    assert!(image_meta(&txt).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unsupported_path_returns_none() {
    let dir = tempdir();
    let path = dir.join("not-an-image.txt");
    std::fs::write(&path, b"hello").unwrap();
    assert!(from_image(&path, 32).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn generate_dispatches_by_kind() {
    let dir = tempdir();
    let path = dir.join("p.png");
    image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 255, 0, 255]))
        .save(&path)
        .unwrap();
    // Image → thumbnail; other types → None (no preview).
    assert!(generate(&path, FileKind::Image, 16).is_some());
    assert!(generate(&path, FileKind::Document, 16).is_none());
    assert!(generate(&path, FileKind::Folder, 16).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60,
        0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01, 0xf6, 0x17, 0x38, 0x55, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const OVERSIZED_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0xea, 0x60, 0x00, 0x00, 0xea, 0x60, 0x08, 0x02, 0x00, 0x00, 0x00, 0x0f,
        0xb0, 0xe2, 0x15, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60,
        0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01, 0xf6, 0x17, 0x38, 0x55, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn an_ordinary_image_still_decodes() {
        assert!(from_encoded(TINY_PNG, 256).is_some());
    }

    #[test]
    fn an_image_declaring_absurd_dimensions_is_refused() {
        // Sixty-nine bytes announcing 3.6 gigapixels. The refusal comes from
        // the header, so nothing is ever allocated for it — which is the whole
        // point: a decompression bomb costs the reader, not the memory.
        assert_eq!(OVERSIZED_PNG.len(), TINY_PNG.len());
        assert!(from_encoded(OVERSIZED_PNG, 256).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_never_finishes_is_killed() {
        let started = Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        assert!(run_bounded(cmd, Duration::from_millis(200)).is_none());
        // Killed on the deadline rather than waited out.
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_finishes_in_time_returns_its_output() {
        let mut cmd = Command::new("echo");
        cmd.arg("ready");
        let out = run_bounded(cmd, Duration::from_secs(5)).expect("echo should succeed");
        assert_eq!(String::from_utf8_lossy(&out).trim(), "ready");
    }
}
