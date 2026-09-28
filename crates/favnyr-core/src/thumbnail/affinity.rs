use super::*;

pub(super) const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
pub(super) const AFFINITY_SCAN_BUFFER_BYTES: usize = 64 * 1024;
const MAX_AFFINITY_SCAN_BYTES: u64 = 512 * 1024 * 1024;
const MAX_AFFINITY_PNG_SPAN_BYTES: u64 = 50 * 1024 * 1024;
const MAX_AFFINITY_THUMBNAIL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AFFINITY_PNG_CHUNKS: usize = 4096;
const MAX_AFFINITY_PNG_PROBES: usize = 4096;
const MAX_AFFINITY_PNG_CANDIDATES: usize = 32;

#[derive(Clone, Copy)]
struct EmbeddedPng {
    offset: u64,
    len: u64,
}

pub(super) fn is_affinity_extension(extension: &str) -> bool {
    ["af", "afdesign", "afphoto", "afpub"]
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

/// Extracts the smallest structurally valid PNG embedded in an Affinity file.
/// The format is proprietary, so this path deliberately relies only on PNG's
/// public chunk structure and never attempts to interpret document records.
/// Scanning is streamed and bounded; candidates are allocated one at a time.
pub(super) fn from_affinity_thumbnail(path: &Path, max_px: u32) -> Option<Thumbnail> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let scan_end = file.metadata().ok()?.len().min(MAX_AFFINITY_SCAN_BYTES);
    let candidates = affinity_png_candidates(&mut file, scan_end).ok()?;
    for candidate in candidates {
        let encoded_len = usize::try_from(candidate.len).ok()?;
        file.seek(SeekFrom::Start(candidate.offset)).ok()?;
        let mut encoded = vec![0u8; encoded_len];
        file.read_exact(&mut encoded).ok()?;
        if let Some(thumbnail) = from_encoded(&encoded, max_px) {
            return Some(thumbnail);
        }
    }
    None
}

fn affinity_png_candidates(
    file: &mut std::fs::File,
    scan_end: u64,
) -> std::io::Result<Vec<EmbeddedPng>> {
    let mut candidates = Vec::new();
    let mut cursor = 0u64;
    let mut probes = 0usize;
    while probes < MAX_AFFINITY_PNG_PROBES
        && scan_end.saturating_sub(cursor) >= PNG_SIGNATURE.len() as u64
    {
        let Some(offset) = find_next_png_signature(file, cursor, scan_end)? else {
            break;
        };
        probes += 1;
        if let Some(end) = embedded_png_end(file, offset, scan_end)? {
            let len = end - offset;
            if len <= MAX_AFFINITY_THUMBNAIL_BYTES {
                retain_small_affinity_candidate(&mut candidates, EmbeddedPng { offset, len });
            }
            cursor = end;
        } else {
            cursor = offset + 1;
        }
    }
    candidates.sort_unstable_by_key(|candidate| (candidate.len, candidate.offset));
    Ok(candidates)
}

fn find_next_png_signature(
    file: &mut std::fs::File,
    start: u64,
    scan_end: u64,
) -> std::io::Result<Option<u64>> {
    use std::io::{Read, Seek, SeekFrom};

    file.seek(SeekFrom::Start(start))?;
    let mut buffer = [0u8; AFFINITY_SCAN_BUFFER_BYTES];
    let mut cursor = start;
    let mut matched = 0usize;
    while cursor < scan_end {
        let remaining = usize::try_from((scan_end - cursor).min(buffer.len() as u64))
            .expect("the scan buffer length always fits usize");
        let read = file.read(&mut buffer[..remaining])?;
        if read == 0 {
            break;
        }
        for (index, byte) in buffer[..read].iter().enumerate() {
            if *byte == PNG_SIGNATURE[matched] {
                matched += 1;
                if matched == PNG_SIGNATURE.len() {
                    let end = cursor + index as u64 + 1;
                    return Ok(Some(end - PNG_SIGNATURE.len() as u64));
                }
            } else {
                matched = usize::from(*byte == PNG_SIGNATURE[0]);
            }
        }
        cursor += read as u64;
    }
    Ok(None)
}

fn embedded_png_end(
    file: &mut std::fs::File,
    start: u64,
    scan_end: u64,
) -> std::io::Result<Option<u64>> {
    use std::io::{Read, Seek, SeekFrom};

    let Some(mut cursor) = start.checked_add(PNG_SIGNATURE.len() as u64) else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(cursor))?;
    for chunk_index in 0..MAX_AFFINITY_PNG_CHUNKS {
        let Some(header_end) = cursor.checked_add(8) else {
            return Ok(None);
        };
        if header_end > scan_end {
            return Ok(None);
        }

        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let data_len = u64::from(u32::from_be_bytes(
            header[..4].try_into().expect("four-byte PNG length"),
        ));
        let chunk_type: &[u8; 4] = header[4..].try_into().expect("four-byte PNG chunk type");
        if !chunk_type.iter().all(u8::is_ascii_alphabetic) {
            return Ok(None);
        }
        if chunk_index == 0 && (chunk_type != b"IHDR" || data_len != 13) {
            return Ok(None);
        }

        let Some(chunk_end) = header_end
            .checked_add(data_len)
            .and_then(|end| end.checked_add(4))
        else {
            return Ok(None);
        };
        if chunk_end > scan_end || chunk_end - start > MAX_AFFINITY_PNG_SPAN_BYTES {
            return Ok(None);
        }
        if chunk_type == b"IEND" {
            return Ok((data_len == 0).then_some(chunk_end));
        }

        cursor = chunk_end;
        file.seek(SeekFrom::Start(cursor))?;
    }
    Ok(None)
}

fn retain_small_affinity_candidate(candidates: &mut Vec<EmbeddedPng>, candidate: EmbeddedPng) {
    if candidates.len() < MAX_AFFINITY_PNG_CANDIDATES {
        candidates.push(candidate);
        return;
    }
    let Some((largest_index, largest)) = candidates
        .iter()
        .enumerate()
        .max_by_key(|(_, existing)| (existing.len, existing.offset))
    else {
        return;
    };
    if candidate.len < largest.len {
        candidates[largest_index] = candidate;
    }
}
