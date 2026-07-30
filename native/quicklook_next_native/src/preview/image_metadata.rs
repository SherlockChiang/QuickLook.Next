//! Bounded image metadata extraction for JPEG, PNG, GIF, WebP, and TIFF.

use std::fs;
use std::io::{Read, Seek, SeekFrom};

use serde::Serialize;

use super::types::{to_json, ReaderPreviewError};
use super::{
    preview_cancelled, read_i32_endian, read_reader_prefix_cancelable, read_u16, read_u16_be,
    read_u16_endian, read_u32, read_u32_be, read_u32_endian, xml_unescape_str,
};
#[cfg(test)]
use super::read_file_prefix;

const MAX_EXIF_BYTES: usize = 256 * 1024;
const MAX_ANIMATED_METADATA_BYTES: usize = 1024 * 1024;
const METADATA_MAGIC_BYTES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExifMetadata {
    pub(super) format: Option<String>,
    pub(super) title: Option<String>,
    pub(super) comment: Option<String>,
    pub(super) make: Option<String>,
    pub(super) model: Option<String>,
    pub(super) date_time: Option<String>,
    pub(super) width: Option<u32>,
    pub(super) height: Option<u32>,
    pub(super) orientation: Option<u16>,
    pub(super) lens_make: Option<String>,
    pub(super) lens_model: Option<String>,
    pub(super) software: Option<String>,
    pub(super) f_number: Option<f64>,
    pub(super) max_aperture: Option<f64>,
    pub(super) exposure_time: Option<f64>,
    pub(super) iso: Option<u32>,
    pub(super) focal_length: Option<f64>,
    pub(super) focal_length_in_35mm_film: Option<u32>,
    pub(super) exposure_bias: Option<f64>,
    pub(super) exposure_program: Option<u16>,
    pub(super) exposure_mode: Option<u16>,
    pub(super) metering_mode: Option<u16>,
    pub(super) flash: Option<u16>,
    pub(super) white_balance: Option<u16>,
    pub(super) light_source: Option<u16>,
    pub(super) digital_zoom_ratio: Option<f64>,
    pub(super) subject_distance: Option<f64>,
    pub(super) contrast: Option<u16>,
    pub(super) saturation: Option<u16>,
    pub(super) sharpness: Option<u16>,
    pub(super) gain_control: Option<u16>,
    pub(super) color_space: Option<u16>,
    pub(super) exif_version: Option<String>,
    pub(super) camera_serial: Option<String>,
    pub(super) lens_serial: Option<String>,
    pub(super) latitude: Option<f64>,
    pub(super) longitude: Option<f64>,
    pub(super) altitude: Option<f64>,
    pub(super) direction: Option<f64>,
    pub(super) horizontal_resolution: Option<f64>,
    pub(super) vertical_resolution: Option<f64>,
    pub(super) photometric_interpretation: Option<String>,
    pub(super) bit_depth: Option<u8>,
    pub(super) color_type: Option<String>,
    pub(super) compression: Option<String>,
    pub(super) has_alpha: Option<bool>,
    pub(super) interlace: Option<String>,
    pub(super) animated: Option<bool>,
    pub(super) frame_count: Option<u32>,
    pub(super) duration_ms: Option<u32>,
    #[serde(skip)]
    resolution_unit: Option<u16>,
}

pub fn render_image_metadata(path: &str) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    render_image_metadata_reader(&mut file, path, None).unwrap_or_default()
}

pub(crate) fn render_image_metadata_reader<R: Read + Seek>(
    reader: &mut R,
    _logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let source_length = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    let magic = read_image_metadata_prefix(reader, METADATA_MAGIC_BYTES, cancel_cb)?;
    let (metadata, complete) = if magic.starts_with(&[0xFF, 0xD8]) {
        let bytes = read_image_metadata_prefix(reader, MAX_EXIF_BYTES, cancel_cb)?;
        (
            parse_jpeg_exif_metadata_from_bytes(&bytes),
            source_length <= bytes.len() as u64,
        )
    } else if magic.starts_with(b"\x89PNG\r\n\x1A\n") {
        let bytes = read_image_metadata_prefix(reader, MAX_EXIF_BYTES, cancel_cb)?;
        (
            parse_png_metadata_from_bytes(&bytes),
            source_length <= bytes.len() as u64,
        )
    } else if magic.starts_with(b"GIF87a") || magic.starts_with(b"GIF89a") {
        let bytes = read_image_metadata_prefix(reader, MAX_ANIMATED_METADATA_BYTES, cancel_cb)?;
        (
            parse_gif_metadata_from_bytes(&bytes),
            source_length <= bytes.len() as u64,
        )
    } else if magic.get(0..4) == Some(b"RIFF") && magic.get(8..12) == Some(b"WEBP") {
        let bytes = read_image_metadata_prefix(reader, MAX_ANIMATED_METADATA_BYTES, cancel_cb)?;
        (
            parse_webp_metadata_from_bytes(&bytes),
            source_length <= bytes.len() as u64,
        )
    } else if magic.starts_with(b"II*\0") || magic.starts_with(b"MM\0*") {
        let bytes = read_image_metadata_prefix(reader, MAX_EXIF_BYTES, cancel_cb)?;
        (
            parse_tiff_exif_metadata(&bytes).map(|mut metadata| {
                metadata.format = Some("TIFF".to_string());
                metadata
            }),
            source_length <= bytes.len() as u64,
        )
    } else {
        (None, true)
    };

    let mut metadata = metadata.ok_or(ReaderPreviewError::Malformed)?;
    if !complete {
        sanitize_partial_animation_metadata(&mut metadata);
    }
    image_metadata_json(metadata, cancel_cb)
}

fn sanitize_partial_animation_metadata(metadata: &mut ExifMetadata) {
    if metadata.animated != Some(true) {
        metadata.animated = None;
    }
    metadata.frame_count = None;
    metadata.duration_ms = None;
}

fn read_image_metadata_prefix<R: Read + Seek>(
    reader: &mut R,
    max_bytes: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<Vec<u8>, ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    read_reader_prefix_cancelable(reader, max_bytes, cancel_cb)
}

fn image_metadata_json(
    metadata: ExifMetadata,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(to_json(&metadata))
}

pub(super) fn parse_webp_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.len() < 12 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let mut offset = 12usize;
    let mut width = None;
    let mut height = None;
    let mut has_alpha = None;
    let mut animated = false;
    let mut frames = 0u32;
    let mut duration_ms = 0u32;
    let mut sidecar = ExifMetadata::default();
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let chunk = bytes.get(offset..offset + 4)?;
        let size = read_u32(bytes, offset + 4)? as usize;
        let payload = offset + 8;
        let payload_end = payload.checked_add(size)?;
        if payload_end > bytes.len() {
            break;
        }
        match chunk {
            b"VP8X" if size >= 10 => {
                let flags = bytes[payload];
                has_alpha = Some(flags & 0x10 != 0);
                animated = flags & 0x02 != 0;
                width = Some(read_u24_le(bytes, payload + 4)? + 1);
                height = Some(read_u24_le(bytes, payload + 7)? + 1);
            }
            b"VP8 "
                if size >= 10
                    && bytes.get(payload + 3..payload + 6) == Some(&[0x9D, 0x01, 0x2A]) =>
            {
                width = Some((read_u16(bytes, payload + 6)? & 0x3FFF) as u32);
                height = Some((read_u16(bytes, payload + 8)? & 0x3FFF) as u32);
                has_alpha.get_or_insert(false);
            }
            b"VP8L" if size >= 5 && bytes.get(payload).copied() == Some(0x2F) => {
                let b1 = bytes[payload + 1] as u32;
                let b2 = bytes[payload + 2] as u32;
                let b3 = bytes[payload + 3] as u32;
                let b4 = bytes[payload + 4] as u32;
                width = Some(1 + b1 + ((b2 & 0x3F) << 8));
                height = Some(1 + ((b2 & 0xC0) >> 6) + (b3 << 2) + ((b4 & 0x0F) << 10));
                has_alpha.get_or_insert(b4 & 0x10 != 0);
            }
            b"ANMF" if size >= 16 => {
                animated = true;
                frames = frames.saturating_add(1);
                duration_ms = duration_ms
                    .saturating_add(read_u24_le(bytes, payload + 12).unwrap_or_default());
            }
            b"ALPH" => {
                has_alpha = Some(true);
            }
            b"EXIF" => {
                let exif_payload = bytes.get(payload..payload_end).unwrap_or_default();
                let tiff = exif_payload
                    .strip_prefix(b"Exif\0\0")
                    .unwrap_or(exif_payload);
                if let Some(exif) = parse_tiff_exif_metadata(tiff) {
                    merge_missing_metadata(&mut sidecar, exif);
                }
            }
            b"XMP " => {
                if let Some(xmp) =
                    parse_xmp_metadata(bytes.get(payload..payload_end).unwrap_or_default())
                {
                    merge_missing_metadata(&mut sidecar, xmp);
                }
            }
            _ => {}
        }
        offset = payload_end + (size % 2);
    }
    sidecar.format = Some("WebP".to_string());
    sidecar.width = width.or(sidecar.width);
    sidecar.height = height.or(sidecar.height);
    sidecar.has_alpha = sidecar.has_alpha.or(has_alpha);
    sidecar.animated = Some(animated);
    sidecar.frame_count = sidecar.frame_count.or((frames > 0).then_some(frames));
    sidecar.duration_ms = sidecar
        .duration_ms
        .or((duration_ms > 0).then_some(duration_ms));
    Some(sidecar)
}

fn merge_missing_metadata(target: &mut ExifMetadata, source: ExifMetadata) {
    target.title = target.title.take().or(source.title);
    target.comment = target.comment.take().or(source.comment);
    target.make = target.make.take().or(source.make);
    target.model = target.model.take().or(source.model);
    target.date_time = target.date_time.take().or(source.date_time);
    target.width = target.width.or(source.width);
    target.height = target.height.or(source.height);
    target.orientation = target.orientation.or(source.orientation);
    target.bit_depth = target.bit_depth.or(source.bit_depth);
    target.color_type = target.color_type.take().or(source.color_type);
    target.compression = target.compression.take().or(source.compression);
    target.lens_make = target.lens_make.take().or(source.lens_make);
    target.lens_model = target.lens_model.take().or(source.lens_model);
    target.software = target.software.take().or(source.software);
    target.f_number = target.f_number.or(source.f_number);
    target.max_aperture = target.max_aperture.or(source.max_aperture);
    target.exposure_time = target.exposure_time.or(source.exposure_time);
    target.iso = target.iso.or(source.iso);
    target.focal_length = target.focal_length.or(source.focal_length);
    target.focal_length_in_35mm_film = target
        .focal_length_in_35mm_film
        .or(source.focal_length_in_35mm_film);
    target.exposure_bias = target.exposure_bias.or(source.exposure_bias);
    target.exposure_program = target.exposure_program.or(source.exposure_program);
    target.exposure_mode = target.exposure_mode.or(source.exposure_mode);
    target.metering_mode = target.metering_mode.or(source.metering_mode);
    target.flash = target.flash.or(source.flash);
    target.white_balance = target.white_balance.or(source.white_balance);
    target.light_source = target.light_source.or(source.light_source);
    target.digital_zoom_ratio = target.digital_zoom_ratio.or(source.digital_zoom_ratio);
    target.subject_distance = target.subject_distance.or(source.subject_distance);
    target.contrast = target.contrast.or(source.contrast);
    target.saturation = target.saturation.or(source.saturation);
    target.sharpness = target.sharpness.or(source.sharpness);
    target.gain_control = target.gain_control.or(source.gain_control);
    target.color_space = target.color_space.or(source.color_space);
    target.exif_version = target.exif_version.take().or(source.exif_version);
    target.camera_serial = target.camera_serial.take().or(source.camera_serial);
    target.lens_serial = target.lens_serial.take().or(source.lens_serial);
    target.latitude = target.latitude.or(source.latitude);
    target.longitude = target.longitude.or(source.longitude);
    target.altitude = target.altitude.or(source.altitude);
    target.direction = target.direction.or(source.direction);
    target.horizontal_resolution = target
        .horizontal_resolution
        .or(source.horizontal_resolution);
    target.vertical_resolution = target.vertical_resolution.or(source.vertical_resolution);
    target.photometric_interpretation = target
        .photometric_interpretation
        .take()
        .or(source.photometric_interpretation);
    target.resolution_unit = target.resolution_unit.or(source.resolution_unit);
    target.has_alpha = target.has_alpha.or(source.has_alpha);
    target.interlace = target.interlace.take().or(source.interlace);
    target.animated = target.animated.or(source.animated);
    target.frame_count = target.frame_count.or(source.frame_count);
    target.duration_ms = target.duration_ms.or(source.duration_ms);
}

fn parse_xmp_metadata(bytes: &[u8]) -> Option<ExifMetadata> {
    let text = String::from_utf8_lossy(bytes);
    let mut metadata = ExifMetadata::default();
    metadata.title = extract_xml_text(&text, &["dc:title", "title"]);
    metadata.comment =
        extract_xml_text(&text, &["dc:description", "description", "xmp:Description"]);
    metadata.software = extract_xml_text(&text, &["xmp:CreatorTool", "CreatorTool", "software"]);
    (metadata.title.is_some() || metadata.comment.is_some() || metadata.software.is_some())
        .then_some(metadata)
}

fn extract_xml_text(text: &str, names: &[&str]) -> Option<String> {
    for name in names {
        let open = format!("<{name}");
        let Some(start) = text.find(&open) else {
            continue;
        };
        let Some(content_start) = text[start..].find('>').map(|idx| start + idx + 1) else {
            continue;
        };
        let close = format!("</{name}>");
        let Some(content_end) = text[content_start..]
            .find(&close)
            .map(|idx| content_start + idx)
        else {
            continue;
        };
        let value = strip_xml_tags(&text[content_start..content_end]);
        if !value.is_empty() {
            return Some(value.chars().take(512).collect());
        }
    }
    None
}

fn strip_xml_tags(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    xml_unescape_str(out.trim()).trim().to_string()
}

fn read_u24_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(
        *bytes.get(offset)? as u32
            | ((*bytes.get(offset + 1)? as u32) << 8)
            | ((*bytes.get(offset + 2)? as u32) << 16),
    )
}

pub(super) fn parse_gif_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.get(0..6)? != b"GIF87a" && bytes.get(0..6)? != b"GIF89a" {
        return None;
    }
    let width = read_u16(bytes, 6)? as u32;
    let height = read_u16(bytes, 8)? as u32;
    let packed = *bytes.get(10)?;
    let mut offset = 13usize;
    if packed & 0x80 != 0 {
        let colors = 1usize << ((packed & 0x07) + 1);
        offset = offset.checked_add(colors.checked_mul(3)?)?;
    }
    let mut frames = 0u32;
    let mut duration_ms = 0u32;
    while offset < bytes.len() {
        match bytes[offset] {
            0x2C => {
                frames = frames.saturating_add(1);
                let Some(next) = offset.checked_add(10) else {
                    break;
                };
                offset = next;
                let Some(image_packed) = bytes.get(offset - 1).copied() else {
                    break;
                };
                if image_packed & 0x80 != 0 {
                    let colors = 1usize << ((image_packed & 0x07) + 1);
                    let Some(table_bytes) = colors.checked_mul(3) else {
                        break;
                    };
                    let Some(next) = offset.checked_add(table_bytes) else {
                        break;
                    };
                    offset = next;
                }
                let Some(next) = offset.checked_add(1) else {
                    break;
                };
                offset = next;
                let Some(next) = skip_gif_sub_blocks(bytes, offset) else {
                    break;
                };
                offset = next;
            }
            0x21 => {
                let Some(label) = bytes.get(offset + 1).copied() else {
                    break;
                };
                if label == 0xF9 && bytes.get(offset + 2).copied() == Some(4) {
                    let delay = read_u16(bytes, offset + 4).unwrap_or(0) as u32;
                    duration_ms = duration_ms.saturating_add(delay.saturating_mul(10));
                    let Some(next) = offset.checked_add(8) else {
                        break;
                    };
                    offset = next;
                } else {
                    let Some(start) = offset.checked_add(2) else {
                        break;
                    };
                    let Some(next) = skip_gif_sub_blocks(bytes, start) else {
                        break;
                    };
                    offset = next;
                }
            }
            0x3B => break,
            _ => break,
        }
    }
    Some(ExifMetadata {
        format: Some("GIF".to_string()),
        width: Some(width),
        height: Some(height),
        animated: Some(frames > 1),
        frame_count: (frames > 0).then_some(frames),
        duration_ms: (duration_ms > 0).then_some(duration_ms),
        ..Default::default()
    })
}

fn skip_gif_sub_blocks(bytes: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let len = *bytes.get(offset)? as usize;
        offset = offset.checked_add(1)?;
        if len == 0 {
            return Some(offset);
        }
        offset = offset.checked_add(len)?;
        if offset > bytes.len() {
            return None;
        }
    }
}

pub(super) fn parse_png_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.get(0..8)? != b"\x89PNG\r\n\x1A\n" {
        return None;
    }
    if read_u32_be(bytes, 8)? != 13 || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let color = *bytes.get(25)?;
    let (title, comment, software) = parse_png_text_chunks(bytes);
    let animation = parse_png_animation_chunks(bytes);
    let (horizontal_resolution, vertical_resolution) = parse_png_resolution(bytes);
    Some(ExifMetadata {
        format: Some("PNG".to_string()),
        title,
        comment,
        width: read_u32_be(bytes, 16),
        height: read_u32_be(bytes, 20),
        software,
        horizontal_resolution,
        vertical_resolution,
        bit_depth: bytes.get(24).copied(),
        color_type: Some(png_color_type_name(color).to_string()),
        has_alpha: Some(matches!(color, 4 | 6)),
        interlace: Some(match bytes.get(28).copied().unwrap_or(0) {
            1 => "Adam7".to_string(),
            _ => "none".to_string(),
        }),
        animated: animation.as_ref().map(|summary| summary.animated),
        frame_count: animation.as_ref().and_then(|summary| summary.frame_count),
        duration_ms: animation.and_then(|summary| summary.duration_ms),
        ..Default::default()
    })
}

fn parse_png_resolution(bytes: &[u8]) -> (Option<f64>, Option<f64>) {
    let mut offset = 8usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let Some(length) = read_u32_be(bytes, offset).map(|value| value as usize) else {
            break;
        };
        let chunk_type = bytes.get(offset + 4..offset + 8).unwrap_or_default();
        let payload_start = offset + 8;
        let Some(payload_end) = payload_start.checked_add(length) else {
            break;
        };
        let Some(next) = payload_end.checked_add(4) else {
            break;
        };
        if payload_end > bytes.len() {
            break;
        }
        if chunk_type == b"pHYs" && length == 9 && bytes.get(payload_start + 8) == Some(&1) {
            let horizontal = read_u32_be(bytes, payload_start)
                .filter(|value| *value > 0)
                .map(|value| value as f64 * 0.0254);
            let vertical = read_u32_be(bytes, payload_start + 4)
                .filter(|value| *value > 0)
                .map(|value| value as f64 * 0.0254);
            return (horizontal, vertical);
        }
        if chunk_type == b"IEND" {
            break;
        }
        offset = next;
    }
    (None, None)
}

#[derive(Debug, Clone, Default)]
struct PngAnimationSummary {
    animated: bool,
    frame_count: Option<u32>,
    duration_ms: Option<u32>,
}

fn parse_png_animation_chunks(bytes: &[u8]) -> Option<PngAnimationSummary> {
    let mut offset = 8usize;
    let mut actl_frames = None;
    let mut fctl_frames = 0u32;
    let mut duration_ms = 0u32;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = read_u32_be(bytes, offset)? as usize;
        let chunk_type = bytes.get(offset + 4..offset + 8).unwrap_or_default();
        let payload_start = offset + 8;
        let payload_end = payload_start.checked_add(length)?;
        let next = payload_end.checked_add(4)?;
        if payload_end > bytes.len() {
            break;
        }
        match chunk_type {
            b"acTL" if length >= 8 => actl_frames = read_u32_be(bytes, payload_start),
            b"fcTL" if length >= 26 => {
                fctl_frames = fctl_frames.saturating_add(1);
                let numerator = read_u16_be(bytes, payload_start + 20).unwrap_or(0) as u32;
                let denominator = read_u16_be(bytes, payload_start + 22).unwrap_or(100) as u32;
                let denominator = if denominator == 0 { 100 } else { denominator };
                duration_ms =
                    duration_ms.saturating_add(numerator.saturating_mul(1000) / denominator);
            }
            b"IEND" => break,
            _ => {}
        }
        offset = next;
    }

    if actl_frames.is_none() && fctl_frames == 0 {
        return None;
    }
    let frames = actl_frames.or((fctl_frames > 0).then_some(fctl_frames));
    Some(PngAnimationSummary {
        animated: frames.unwrap_or(0) > 1 || fctl_frames > 1,
        frame_count: frames,
        duration_ms: (duration_ms > 0).then_some(duration_ms),
    })
}

fn parse_png_text_chunks(bytes: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let mut title = None;
    let mut comment = None;
    let mut software = None;
    let mut offset = 8usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let Some(length) = read_u32_be(bytes, offset).map(|value| value as usize) else {
            break;
        };
        let chunk_type = bytes.get(offset + 4..offset + 8).unwrap_or_default();
        let payload_start = offset + 8;
        let Some(payload_end) = payload_start.checked_add(length) else {
            break;
        };
        let Some(next) = payload_end.checked_add(4) else {
            break;
        };
        if payload_end > bytes.len() {
            break;
        }
        if chunk_type == b"tEXt" {
            if let Some((keyword, value)) =
                parse_png_text_chunk(bytes.get(payload_start..payload_end).unwrap_or_default())
            {
                match keyword.to_ascii_lowercase().as_str() {
                    "title" if title.is_none() => title = Some(value),
                    "description" | "comment" if comment.is_none() => comment = Some(value),
                    "software" if software.is_none() => software = Some(value),
                    _ => {}
                }
            }
        }
        if chunk_type == b"IEND" {
            break;
        }
        offset = next;
    }
    (title, comment, software)
}

fn parse_png_text_chunk(payload: &[u8]) -> Option<(String, String)> {
    let separator = payload.iter().position(|byte| *byte == 0)?;
    let keyword = String::from_utf8_lossy(payload.get(..separator)?)
        .trim()
        .to_string();
    let value = String::from_utf8_lossy(payload.get(separator + 1..)?)
        .trim_matches('\0')
        .trim()
        .chars()
        .take(512)
        .collect::<String>();
    (!keyword.is_empty() && !value.is_empty()).then_some((keyword, value))
}

fn png_color_type_name(value: u8) -> &'static str {
    match value {
        0 => "grayscale",
        2 => "truecolor",
        3 => "indexed color",
        4 => "grayscale with alpha",
        6 => "truecolor with alpha",
        _ => "unknown",
    }
}

#[cfg(test)]
pub(super) fn parse_jpeg_exif_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, MAX_EXIF_BYTES)?;
    parse_jpeg_exif_metadata_from_bytes(&bytes)
}

pub(super) fn parse_jpeg_exif_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }

    let mut metadata = find_jpeg_exif_tiff(bytes)
        .and_then(parse_tiff_exif_metadata)
        .unwrap_or_default();
    metadata.format = Some("JPEG".to_string());
    if let Some((width, height, bit_depth, components)) = find_jpeg_frame_summary(bytes) {
        metadata.width = metadata.width.or(Some(width));
        metadata.height = metadata.height.or(Some(height));
        metadata.bit_depth = metadata.bit_depth.or(Some(bit_depth));
        metadata.color_type = metadata.color_type.take().or_else(|| {
            Some(
                match components {
                    1 => "grayscale",
                    3 => "YCbCr",
                    4 => "CMYK",
                    _ => "unknown",
                }
                .to_string(),
            )
        });
    }
    Some(metadata)
}

fn find_jpeg_exif_tiff(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }

    let mut offset = 2usize;
    while offset.checked_add(4)? <= bytes.len() {
        if bytes[offset] != 0xFF {
            return None;
        }
        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = read_u16_be(bytes, offset)? as usize;
        if len < 2 {
            return None;
        }
        let payload_start = offset.checked_add(2)?;
        let payload_end = offset.checked_add(len)?;
        let payload = bytes.get(payload_start..payload_end)?;
        if marker == 0xE1 && payload.starts_with(b"Exif\0\0") {
            return payload.get(6..);
        }
        offset = payload_end;
    }

    None
}

fn find_jpeg_frame_summary(bytes: &[u8]) -> Option<(u32, u32, u8, u8)> {
    let mut offset = 2usize;
    while offset.checked_add(4)? <= bytes.len() {
        if bytes[offset] != 0xFF {
            return None;
        }
        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        let len = read_u16_be(bytes, offset)? as usize;
        if len < 2 {
            return None;
        }
        let payload_start = offset.checked_add(2)?;
        let payload_end = offset.checked_add(len)?;
        let payload = bytes.get(payload_start..payload_end)?;
        if matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        ) {
            let bit_depth = *payload.first()?;
            let height = read_u16_be(payload, 1)? as u32;
            let width = read_u16_be(payload, 3)? as u32;
            let components = *payload.get(5)?;
            if width == 0 || height == 0 || components == 0 {
                return None;
            }
            return Some((width, height, bit_depth, components));
        }
        offset = payload_end;
    }
    None
}

pub(super) fn parse_tiff_exif_metadata(tiff: &[u8]) -> Option<ExifMetadata> {
    let endian = match tiff.get(0..2)? {
        b"II" => 1,
        b"MM" => 2,
        _ => return None,
    };
    if read_u16_endian(tiff, 2, endian)? != 42 {
        return None;
    }

    let ifd0 = read_u32_endian(tiff, 4, endian)? as usize;
    let mut metadata = ExifMetadata::default();
    let mut exif_ifd = None;
    let mut gps_ifd = None;
    parse_exif_ifd(
        tiff,
        ifd0,
        endian,
        &mut metadata,
        &mut exif_ifd,
        &mut gps_ifd,
    );
    if let Some(offset) = exif_ifd {
        parse_exif_ifd(tiff, offset, endian, &mut metadata, &mut None, &mut None);
    }
    if let Some(offset) = gps_ifd {
        parse_gps_ifd(tiff, offset, endian, &mut metadata);
    }
    match metadata.resolution_unit.unwrap_or(2) {
        2 => {}
        3 => {
            metadata.horizontal_resolution =
                metadata.horizontal_resolution.map(|value| value * 2.54);
            metadata.vertical_resolution = metadata.vertical_resolution.map(|value| value * 2.54);
        }
        _ => {
            metadata.horizontal_resolution = None;
            metadata.vertical_resolution = None;
        }
    }
    metadata.resolution_unit = None;

    Some(metadata)
}

fn parse_exif_ifd(
    tiff: &[u8],
    offset: usize,
    endian: u8,
    metadata: &mut ExifMetadata,
    exif_ifd: &mut Option<usize>,
    gps_ifd: &mut Option<usize>,
) {
    let Some(count) = read_u16_endian(tiff, offset, endian).map(usize::from) else {
        return;
    };
    let entries = offset.saturating_add(2);
    for index in 0..count.min(128) {
        let entry = entries.saturating_add(index.saturating_mul(12));
        let Some(tag) = read_u16_endian(tiff, entry, endian) else {
            break;
        };
        match tag {
            0x0100 => {
                metadata.width = metadata
                    .width
                    .or_else(|| exif_u32_or_u16_value(tiff, entry, endian))
            }
            0x0101 => {
                metadata.height = metadata
                    .height
                    .or_else(|| exif_u32_or_u16_value(tiff, entry, endian))
            }
            0x0102 => {
                metadata.bit_depth = metadata
                    .bit_depth
                    .or_else(|| tiff_bits_per_sample(tiff, entry, endian))
            }
            0x0103 => {
                metadata.compression = metadata.compression.take().or_else(|| {
                    tiff_compression_name(exif_u16_value(tiff, entry, endian)?).map(str::to_string)
                })
            }
            0x0106 => {
                metadata.photometric_interpretation =
                    metadata.photometric_interpretation.take().or_else(|| {
                        tiff_photometric_name(exif_u16_value(tiff, entry, endian)?)
                            .map(str::to_string)
                    })
            }
            0x010F => metadata.make = exif_ascii(tiff, entry, endian),
            0x0110 => metadata.model = exif_ascii(tiff, entry, endian),
            0x0112 => metadata.orientation = exif_u16_value(tiff, entry, endian),
            0x011A => metadata.horizontal_resolution = exif_rational_value(tiff, entry, endian),
            0x011B => metadata.vertical_resolution = exif_rational_value(tiff, entry, endian),
            0x0128 => metadata.resolution_unit = exif_u16_value(tiff, entry, endian),
            0x0131 => metadata.software = exif_ascii(tiff, entry, endian),
            0x0132 => {
                if metadata.date_time.is_none() {
                    metadata.date_time = exif_ascii(tiff, entry, endian);
                }
            }
            0x9003 => {
                if let Some(date_time_original) = exif_ascii(tiff, entry, endian) {
                    metadata.date_time = Some(date_time_original);
                }
            }
            0x829A => metadata.exposure_time = exif_rational_value(tiff, entry, endian),
            0x829D => metadata.f_number = exif_rational_value(tiff, entry, endian),
            0x8822 => metadata.exposure_program = exif_u16_value(tiff, entry, endian),
            0x8827 => metadata.iso = exif_u32_or_u16_value(tiff, entry, endian),
            0x8769 => *exif_ifd = exif_u32_value(tiff, entry, endian).map(|v| v as usize),
            0x8825 => *gps_ifd = exif_u32_value(tiff, entry, endian).map(|v| v as usize),
            0x9204 => metadata.exposure_bias = exif_signed_rational_value(tiff, entry, endian),
            0x9205 => metadata.max_aperture = exif_rational_value(tiff, entry, endian),
            0x9206 => metadata.subject_distance = exif_rational_value(tiff, entry, endian),
            0x9207 => metadata.metering_mode = exif_u16_value(tiff, entry, endian),
            0x9208 => metadata.light_source = exif_u16_value(tiff, entry, endian),
            0x9209 => metadata.flash = exif_u16_value(tiff, entry, endian),
            0x920A => metadata.focal_length = exif_rational_value(tiff, entry, endian),
            0x9000 => metadata.exif_version = exif_version(tiff, entry, endian),
            0xA001 => metadata.color_space = exif_u16_value(tiff, entry, endian),
            0xA002 => metadata.width = exif_u32_or_u16_value(tiff, entry, endian),
            0xA003 => metadata.height = exif_u32_or_u16_value(tiff, entry, endian),
            0xA402 => metadata.exposure_mode = exif_u16_value(tiff, entry, endian),
            0xA403 => metadata.white_balance = exif_u16_value(tiff, entry, endian),
            0xA404 => metadata.digital_zoom_ratio = exif_rational_value(tiff, entry, endian),
            0xA405 => {
                metadata.focal_length_in_35mm_film = exif_u32_or_u16_value(tiff, entry, endian)
            }
            0xA407 => metadata.gain_control = exif_u16_value(tiff, entry, endian),
            0xA408 => metadata.contrast = exif_u16_value(tiff, entry, endian),
            0xA409 => metadata.saturation = exif_u16_value(tiff, entry, endian),
            0xA40A => metadata.sharpness = exif_u16_value(tiff, entry, endian),
            0xA431 => metadata.camera_serial = exif_ascii(tiff, entry, endian),
            0xA433 => metadata.lens_make = exif_ascii(tiff, entry, endian),
            0xA434 => metadata.lens_model = exif_ascii(tiff, entry, endian),
            0xA435 => metadata.lens_serial = exif_ascii(tiff, entry, endian),
            _ => {}
        }
    }
}

fn parse_gps_ifd(tiff: &[u8], offset: usize, endian: u8, metadata: &mut ExifMetadata) {
    let Some(count) = read_u16_endian(tiff, offset, endian).map(usize::from) else {
        return;
    };
    let entries = offset.saturating_add(2);
    let mut lat_ref = None;
    let mut lon_ref = None;
    let mut altitude_ref = None;
    let mut lat = None;
    let mut lon = None;
    let mut altitude = None;
    let mut direction = None;
    for index in 0..count.min(64) {
        let entry = entries.saturating_add(index.saturating_mul(12));
        let Some(tag) = read_u16_endian(tiff, entry, endian) else {
            break;
        };
        match tag {
            1 => lat_ref = exif_ascii(tiff, entry, endian),
            2 => lat = exif_gps_coordinate(tiff, entry, endian),
            3 => lon_ref = exif_ascii(tiff, entry, endian),
            4 => lon = exif_gps_coordinate(tiff, entry, endian),
            5 => altitude_ref = exif_u8_value(tiff, entry, endian),
            6 => altitude = exif_rational_value(tiff, entry, endian),
            17 => direction = exif_rational_value(tiff, entry, endian),
            _ => {}
        }
    }

    metadata.latitude = signed_gps_coordinate(lat, lat_ref.as_deref(), "S");
    metadata.longitude = signed_gps_coordinate(lon, lon_ref.as_deref(), "W");
    metadata.altitude = altitude.map(|value| {
        if altitude_ref == Some(1) {
            -value
        } else {
            value
        }
    });
    metadata.direction = direction;
}

fn exif_ascii(tiff: &[u8], entry: usize, endian: u8) -> Option<String> {
    let bytes = exif_value_bytes(tiff, entry, endian)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(bytes.get(..end)?)
        .trim()
        .chars()
        .take(512)
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

fn exif_version(tiff: &[u8], entry: usize, endian: u8) -> Option<String> {
    let bytes = exif_value_bytes(tiff, entry, endian)?;
    let text: String = bytes
        .iter()
        .filter_map(|b| b.is_ascii_graphic().then_some(*b as char))
        .collect();
    (!text.is_empty()).then_some(text)
}

fn exif_u16_value(tiff: &[u8], entry: usize, endian: u8) -> Option<u16> {
    if read_u16_endian(tiff, entry + 2, endian)? != 3 {
        return None;
    }
    if read_u32_endian(tiff, entry + 4, endian)? == 0 {
        return None;
    }
    read_u16_endian(tiff, entry + 8, endian)
}

fn exif_u8_value(tiff: &[u8], entry: usize, endian: u8) -> Option<u16> {
    if read_u16_endian(tiff, entry + 2, endian)? != 1
        || read_u32_endian(tiff, entry + 4, endian)? == 0
    {
        return None;
    }
    tiff.get(entry + 8).copied().map(u16::from)
}

fn exif_u32_value(tiff: &[u8], entry: usize, endian: u8) -> Option<u32> {
    match read_u16_endian(tiff, entry + 2, endian)? {
        3 => exif_u16_value(tiff, entry, endian).map(u32::from),
        4 => read_u32_endian(tiff, entry + 8, endian),
        _ => None,
    }
}

fn exif_u32_or_u16_value(tiff: &[u8], entry: usize, endian: u8) -> Option<u32> {
    match read_u16_endian(tiff, entry + 2, endian)? {
        3 => exif_u16_value(tiff, entry, endian).map(u32::from),
        4 => exif_u32_value(tiff, entry, endian),
        _ => None,
    }
}

fn tiff_bits_per_sample(tiff: &[u8], entry: usize, endian: u8) -> Option<u8> {
    if read_u16_endian(tiff, entry + 2, endian)? != 3 {
        return None;
    }
    let count = read_u32_endian(tiff, entry + 4, endian)? as usize;
    if count == 0 {
        return None;
    }
    let value = if count == 1 {
        read_u16_endian(tiff, entry + 8, endian)?
    } else {
        let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
        read_u16_endian(tiff, offset, endian)?
    };
    u8::try_from(value).ok()
}

fn tiff_compression_name(value: u16) -> Option<&'static str> {
    Some(match value {
        1 => "none",
        2 => "CCITT Group 3 1-D",
        3 => "Group 3 fax",
        4 => "Group 4 fax",
        5 => "LZW",
        6 => "old JPEG",
        7 => "JPEG",
        8 => "Deflate",
        32773 => "PackBits",
        _ => return None,
    })
}

fn tiff_photometric_name(value: u16) -> Option<&'static str> {
    Some(match value {
        0 => "white is zero",
        1 => "black is zero",
        2 => "RGB",
        3 => "palette color",
        4 => "transparency mask",
        5 => "separated",
        6 => "YCbCr",
        8 => "CIELab",
        _ => return None,
    })
}

fn exif_gps_coordinate(tiff: &[u8], entry: usize, endian: u8) -> Option<f64> {
    if read_u16_endian(tiff, entry + 2, endian)? != 5
        || read_u32_endian(tiff, entry + 4, endian)? < 3
    {
        return None;
    }
    let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
    let degrees = exif_rational(tiff, offset, endian)?;
    let minutes = exif_rational(tiff, offset + 8, endian)?;
    let seconds = exif_rational(tiff, offset + 16, endian)?;
    Some(degrees + minutes / 60.0 + seconds / 3600.0)
}

fn exif_rational_value(tiff: &[u8], entry: usize, endian: u8) -> Option<f64> {
    if read_u16_endian(tiff, entry + 2, endian)? != 5
        || read_u32_endian(tiff, entry + 4, endian)? == 0
    {
        return None;
    }
    let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
    exif_rational(tiff, offset, endian)
}

fn exif_signed_rational_value(tiff: &[u8], entry: usize, endian: u8) -> Option<f64> {
    if read_u16_endian(tiff, entry + 2, endian)? != 10
        || read_u32_endian(tiff, entry + 4, endian)? == 0
    {
        return None;
    }
    let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
    exif_signed_rational(tiff, offset, endian)
}

fn exif_rational(tiff: &[u8], offset: usize, endian: u8) -> Option<f64> {
    let numerator = read_u32_endian(tiff, offset, endian)? as f64;
    let denominator = read_u32_endian(tiff, offset + 4, endian)? as f64;
    if denominator == 0.0 {
        return None;
    }
    Some(numerator / denominator)
}

fn exif_signed_rational(tiff: &[u8], offset: usize, endian: u8) -> Option<f64> {
    let numerator = read_i32_endian(tiff, offset, endian)? as f64;
    let denominator = read_i32_endian(tiff, offset + 4, endian)? as f64;
    if denominator == 0.0 {
        return None;
    }
    Some(numerator / denominator)
}

fn signed_gps_coordinate(
    value: Option<f64>,
    reference: Option<&str>,
    negative_ref: &str,
) -> Option<f64> {
    let mut value = value?;
    if reference?.trim().eq_ignore_ascii_case(negative_ref) {
        value = -value;
    }
    Some(value)
}

fn exif_value_bytes(tiff: &[u8], entry: usize, endian: u8) -> Option<&[u8]> {
    let typ = read_u16_endian(tiff, entry + 2, endian)?;
    let count = read_u32_endian(tiff, entry + 4, endian)? as usize;
    let unit = match typ {
        1 | 2 | 7 => 1,
        3 => 2,
        4 | 9 => 4,
        5 | 10 => 8,
        _ => return None,
    };
    let len = count.checked_mul(unit)?;
    if len <= 4 {
        return tiff.get(entry + 8..entry + 8 + len);
    }
    let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
    tiff.get(offset..offset.checked_add(len)?)
}
