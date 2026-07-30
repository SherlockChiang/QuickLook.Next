//! Bounded GIF, WebP, and APNG animation classification.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{
    parse_gif_metadata_from_bytes, parse_png_metadata_from_bytes, parse_webp_metadata_from_bytes,
    read_reader_prefix,
};

pub(super) const MAX_IMAGE_ANIMATION_PROBE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ImageAnimationProbe {
    pub(crate) is_animated: Option<bool>,
}

pub(crate) fn probe_image_animation_reader<R: Read + Seek>(
    reader: &mut R,
    logical_name: &str,
    source_size: u64,
) -> Option<ImageAnimationProbe> {
    let extension = Path::new(logical_name)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "gif" | "webp" | "png") {
        return None;
    }
    reader.seek(SeekFrom::Start(0)).ok()?;
    let bytes = read_reader_prefix(reader, MAX_IMAGE_ANIMATION_PROBE_BYTES)?;

    let is_animated = match extension.as_str() {
        "gif" => {
            if bytes.get(0..6)? != b"GIF87a" && bytes.get(0..6)? != b"GIF89a" {
                return None;
            }
            if super::read_u16(&bytes, 6)? == 0 || super::read_u16(&bytes, 8)? == 0 {
                return None;
            }
            let metadata = parse_gif_metadata_from_bytes(&bytes);
            let detected = metadata.as_ref().and_then(|value| value.animated);
            match detected {
                Some(true) => Some(true),
                Some(false) if source_size <= bytes.len() as u64 => Some(false),
                _ => None,
            }
        }
        "webp" => {
            let metadata = parse_webp_metadata_from_bytes(&bytes)?;
            if metadata.width? == 0 || metadata.height? == 0 {
                return None;
            }
            metadata.animated
        }
        "png" => {
            let metadata = parse_png_metadata_from_bytes(&bytes)?;
            if metadata.width? == 0 || metadata.height? == 0 {
                return None;
            }
            match metadata.animated {
                Some(value) => Some(value),
                None if source_size <= bytes.len() as u64 => Some(false),
                None => None,
            }
        }
        _ => return None,
    };
    Some(ImageAnimationProbe { is_animated })
}
