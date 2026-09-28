use super::*;

pub(super) const PSD_THUMBNAIL_RESOURCE_V4: u16 = 1033;
pub(super) const PSD_THUMBNAIL_RESOURCE_V5: u16 = 1036;
const PSD_THUMBNAIL_HEADER_BYTES: u64 = 28;
pub(super) const MAX_PSD_THUMBNAIL_BYTES: usize = 16 * 1024 * 1024;
const MAX_PSD_RESOURCE_BLOCKS: usize = 4096;

/// Extracts Photoshop's optional embedded JPEG thumbnail from a PSD resource
/// section. This deliberately does not attempt to render layers or decode the
/// full composite image: both require substantially broader color-mode and
/// compression support. Missing or malformed resources therefore remain a
/// normal icon fallback.
pub(super) fn from_psd_thumbnail(path: &Path, max_px: u32) -> Option<Thumbnail> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();

    let mut header = [0u8; 26];
    file.read_exact(&mut header).ok()?;
    if &header[..4] != b"8BPS"
        || header[4..6] != 1u16.to_be_bytes()
        || header[6..12].iter().any(|byte| *byte != 0)
    {
        return None;
    }

    let color_data_len = u64::from(read_be_u32(&mut file)?);
    let resources_len_position = file.stream_position().ok()?.checked_add(color_data_len)?;
    if resources_len_position.checked_add(4)? > file_len {
        return None;
    }
    file.seek(SeekFrom::Start(resources_len_position)).ok()?;

    let resources_len = u64::from(read_be_u32(&mut file)?);
    let resources_start = file.stream_position().ok()?;
    let resources_end = resources_start.checked_add(resources_len)?;
    if resources_end > file_len {
        return None;
    }

    let mut fallback = None;
    let mut blocks_checked = 0usize;
    loop {
        let block_start = file.stream_position().ok()?;
        if block_start == resources_end {
            break;
        }
        if blocks_checked >= MAX_PSD_RESOURCE_BLOCKS {
            return fallback;
        }
        blocks_checked += 1;

        // Signature, resource ID, shortest padded Pascal name and data size.
        // Check the declared section before reading so trailing PSD sections
        // can never be mistaken for a truncated resource header.
        if resources_end.checked_sub(block_start)? < 12 {
            return fallback;
        }

        let mut signature = [0u8; 4];
        file.read_exact(&mut signature).ok()?;
        if &signature != b"8BIM" {
            return fallback;
        }
        let resource_id = read_be_u16(&mut file)?;

        let mut name_len = [0u8; 1];
        file.read_exact(&mut name_len).ok()?;
        let pascal_bytes = 1u64.checked_add(u64::from(name_len[0]))?;
        let padded_pascal_bytes = pascal_bytes.checked_add(pascal_bytes & 1)?;
        let after_name = file
            .stream_position()
            .ok()?
            .checked_add(padded_pascal_bytes.checked_sub(1)?)?;
        if after_name.checked_add(4)? > resources_end {
            return fallback;
        }
        file.seek(SeekFrom::Start(after_name)).ok()?;

        let resource_len = u64::from(read_be_u32(&mut file)?);
        let resource_start = file.stream_position().ok()?;
        let resource_end = resource_start.checked_add(resource_len)?;
        let padded_resource_end = resource_end.checked_add(resource_len & 1)?;
        if padded_resource_end > resources_end {
            return fallback;
        }

        if matches!(
            resource_id,
            PSD_THUMBNAIL_RESOURCE_V4 | PSD_THUMBNAIL_RESOURCE_V5
        ) && resource_len >= PSD_THUMBNAIL_HEADER_BYTES
        {
            let mut thumbnail_header = [0u8; PSD_THUMBNAIL_HEADER_BYTES as usize];
            file.read_exact(&mut thumbnail_header).ok()?;
            if let Some(jpeg_len) = psd_thumbnail_jpeg_len(&thumbnail_header, resource_len) {
                let mut encoded = vec![0u8; jpeg_len];
                file.read_exact(&mut encoded).ok()?;
                if let Some(thumbnail) = from_encoded(&encoded, max_px) {
                    if resource_id == PSD_THUMBNAIL_RESOURCE_V5 {
                        return Some(thumbnail);
                    }
                    if fallback.is_none() {
                        fallback = Some(thumbnail);
                    }
                }
            }
        }

        file.seek(SeekFrom::Start(padded_resource_end)).ok()?;
    }
    fallback
}

fn read_be_u16(reader: &mut impl std::io::Read) -> Option<u16> {
    let mut bytes = [0u8; 2];
    reader.read_exact(&mut bytes).ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_be_u32(reader: &mut impl std::io::Read) -> Option<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_be_bytes(bytes))
}

pub(super) fn psd_thumbnail_jpeg_len(header: &[u8; 28], resource_len: u64) -> Option<usize> {
    let format = u32::from_be_bytes(header[..4].try_into().ok()?);
    let compressed_len =
        usize::try_from(u32::from_be_bytes(header[20..24].try_into().ok()?)).ok()?;
    if format != 1 || compressed_len == 0 || compressed_len > MAX_PSD_THUMBNAIL_BYTES {
        return None;
    }
    PSD_THUMBNAIL_HEADER_BYTES
        .checked_add(u64::try_from(compressed_len).ok()?)
        .filter(|required| *required <= resource_len)?;
    Some(compressed_len)
}
