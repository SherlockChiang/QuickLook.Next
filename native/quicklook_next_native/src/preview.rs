//! Native preview providers for Text, Info, Archive, and Folder.
//!
//! These replace the equivalent .NET plugins with pure-Rust implementations callable directly
//! from the App via C ABI, bypassing the .NET plugin pipeline entirely.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use image::GenericImageView;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;
use tar::Archive as TarArchive;
use zip::ZipArchive;

fn preview_cancelled(cancel_cb: Option<extern "C" fn() -> bool>) -> bool {
    cancel_cb.map(|callback| callback()).unwrap_or(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewReadyDto {
    kind: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    office_layout: Option<OfficeLayoutDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    listing: Option<PreviewListingDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<PreviewTableDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown: Option<PreviewMarkdownDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeLayoutDto {
    layout_kind: String,
    width: f64,
    height: f64,
    pages: Vec<OfficePageDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficePageDto {
    title: String,
    index: usize,
    width: f64,
    height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    freeze_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    freeze_columns: Option<usize>,
    cells: Vec<OfficeCellDto>,
    items: Vec<OfficeLayoutItemDto>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OfficeCellDto {
    row: usize,
    column: usize,
    text: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    row_span: usize,
    column_span: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    number_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    horizontal_alignment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vertical_alignment: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    wrap_text: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OfficeLayoutItemDto {
    kind: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(skip_serializing_if = "is_zero_usize")]
    z_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    placeholder_type: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_base64: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewListingDto {
    root_name: String,
    root_path: String,
    listing_kind: String,
    summary: String,
    is_partial: bool,
    items: Vec<PreviewListingItemDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewListingItemDto {
    name: String,
    path: String,
    parent_path: String,
    is_folder: bool,
    size: i64,
    packed_size: i64,
    modified_unix: i64,
    #[serde(rename = "type")]
    typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewTableDto {
    format: String,
    delimiter: String,
    headers: Vec<String>,
    rows: Vec<PreviewTableRowDto>,
    total_rows: usize,
    total_columns: usize,
    is_partial: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewTableRowDto {
    cells: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewMarkdownDto {
    blocks: Vec<PreviewMarkdownBlockDto>,
    is_partial: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PreviewMarkdownBlockDto {
    kind: String,
    level: usize,
    text: String,
    language: String,
    inlines: Vec<PreviewMarkdownInlineDto>,
    children: Vec<PreviewMarkdownBlockDto>,
    table_headers: Vec<String>,
    table_rows: Vec<Vec<String>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PreviewMarkdownInlineDto {
    kind: String,
    text: String,
    url: String,
    children: Vec<PreviewMarkdownInlineDto>,
}

#[derive(Debug, Clone)]
enum BValue {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<BValue>),
    Dict(BTreeMap<Vec<u8>, BValue>),
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

// ── Text preview ─────────────────────────────────────────────────────────────

const MAX_TEXT_BYTES: usize = 512 * 1024;
// The WinUI grid currently realizes every displayed cell, so keep synchronous UI work bounded.
const MAX_TABLE_ROWS: usize = 160;
const MAX_TABLE_COLUMNS: usize = 64;
const MAX_TABLE_CELL_CHARS: usize = 240;
const MAX_MARKDOWN_BLOCKS: usize = 500;
const MAX_MARKDOWN_LIST_ITEMS: usize = 300;
const MAX_MARKDOWN_TABLE_ROWS: usize = 120;
const MAX_MARKDOWN_INLINE_CHARS: usize = 4096;
const MAX_EXECUTABLE_HEADER_BYTES: usize = 4 * 1024 * 1024;
const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_APPX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PACKAGE_ICON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INFO_HEADER_BYTES: usize = 1024 * 1024;
const MAX_MAIL_HEADER_BYTES: usize = 256 * 1024;
const MAX_EBOOK_XML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EBOOK_CHAPTER_BYTES: u64 = 768 * 1024;
const MAX_EBOOK_CHAPTERS: usize = 10;
const MAX_EBOOK_TEXT_CHARS: usize = 140 * 1024;
const MAX_EXIF_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExifMetadata {
    format: Option<String>,
    title: Option<String>,
    comment: Option<String>,
    make: Option<String>,
    model: Option<String>,
    date_time: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    orientation: Option<u16>,
    lens_make: Option<String>,
    lens_model: Option<String>,
    software: Option<String>,
    f_number: Option<f64>,
    max_aperture: Option<f64>,
    exposure_time: Option<f64>,
    iso: Option<u32>,
    focal_length: Option<f64>,
    focal_length_in_35mm_film: Option<u32>,
    exposure_bias: Option<f64>,
    exposure_program: Option<u16>,
    exposure_mode: Option<u16>,
    metering_mode: Option<u16>,
    flash: Option<u16>,
    white_balance: Option<u16>,
    light_source: Option<u16>,
    digital_zoom_ratio: Option<f64>,
    subject_distance: Option<f64>,
    contrast: Option<u16>,
    saturation: Option<u16>,
    sharpness: Option<u16>,
    gain_control: Option<u16>,
    color_space: Option<u16>,
    exif_version: Option<String>,
    camera_serial: Option<String>,
    lens_serial: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    altitude: Option<f64>,
    direction: Option<f64>,
    bit_depth: Option<u8>,
    color_type: Option<String>,
    compression: Option<String>,
    has_alpha: Option<bool>,
    interlace: Option<String>,
    animated: Option<bool>,
    frame_count: Option<u32>,
    duration_ms: Option<u32>,
}

pub fn render_image_metadata(path: &str) -> String {
    parse_jpeg_exif_metadata(path)
        .or_else(|| parse_png_metadata(path))
        .or_else(|| parse_gif_metadata(path))
        .or_else(|| parse_webp_metadata(path))
        .or_else(|| parse_tiff_metadata(path))
        .map(|metadata| to_json(&metadata))
        .unwrap_or_default()
}

fn parse_tiff_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, MAX_EXIF_BYTES)?;
    let mut metadata = parse_tiff_exif_metadata(&bytes)?;
    metadata.format = Some("TIFF".to_string());
    Some(metadata)
}

fn parse_webp_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, 1024 * 1024)?;
    parse_webp_metadata_from_bytes(&bytes)
}

fn parse_webp_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.len() < 12 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let mut offset = 12usize;
    let mut width = None;
    let mut height = None;
    let mut has_alpha = None;
    let mut animated = false;
    let mut frames = 0u32;
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
                has_alpha.get_or_insert(true);
            }
            b"ANMF" => {
                animated = true;
                frames = frames.saturating_add(1);
            }
            b"ALPH" => {
                has_alpha = Some(true);
            }
            b"EXIF" => {
                if let Some(exif) =
                    parse_tiff_exif_metadata(bytes.get(payload..payload_end).unwrap_or_default())
                {
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
    sidecar.width = sidecar.width.or(width);
    sidecar.height = sidecar.height.or(height);
    sidecar.has_alpha = sidecar.has_alpha.or(has_alpha);
    sidecar.animated = Some(animated);
    sidecar.frame_count = sidecar.frame_count.or((frames > 0).then_some(frames));
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
    target.exposure_time = target.exposure_time.or(source.exposure_time);
    target.iso = target.iso.or(source.iso);
    target.focal_length = target.focal_length.or(source.focal_length);
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

fn parse_gif_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, 1024 * 1024)?;
    parse_gif_metadata_from_bytes(&bytes)
}

fn parse_gif_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
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
                offset = offset.checked_add(10)?;
                let image_packed = *bytes.get(offset - 1)?;
                if image_packed & 0x80 != 0 {
                    let colors = 1usize << ((image_packed & 0x07) + 1);
                    offset = offset.checked_add(colors.checked_mul(3)?)?;
                }
                offset = offset.checked_add(1)?;
                offset = skip_gif_sub_blocks(bytes, offset)?;
            }
            0x21 => {
                let label = *bytes.get(offset + 1)?;
                if label == 0xF9 && bytes.get(offset + 2).copied() == Some(4) {
                    let delay = read_u16(bytes, offset + 4).unwrap_or(0) as u32;
                    duration_ms = duration_ms.saturating_add(delay.saturating_mul(10));
                    offset = offset.checked_add(8)?;
                } else {
                    offset = skip_gif_sub_blocks(bytes, offset + 2)?;
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

fn parse_png_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, 256 * 1024)?;
    parse_png_metadata_from_bytes(&bytes)
}

fn parse_png_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    if bytes.get(0..8)? != b"\x89PNG\r\n\x1A\n" {
        return None;
    }
    if read_u32_be(bytes, 8)? != 13 || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let color = *bytes.get(25)?;
    let (title, comment, software) = parse_png_text_chunks(bytes);
    let animation = parse_png_animation_chunks(bytes);
    Some(ExifMetadata {
        format: Some("PNG".to_string()),
        title,
        comment,
        width: read_u32_be(bytes, 16),
        height: read_u32_be(bytes, 20),
        software,
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

fn file_size_modified(path: &str) -> (i64, i64) {
    let meta = fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
    let modified_unix = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (size, modified_unix)
}

fn read_file_prefix(path: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut reader = file.take(max_bytes as u64);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    reader.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn parse_jpeg_exif_metadata(path: &str) -> Option<ExifMetadata> {
    let bytes = read_file_prefix(path, MAX_EXIF_BYTES)?;
    parse_jpeg_exif_metadata_from_bytes(&bytes)
}

fn parse_jpeg_exif_metadata_from_bytes(bytes: &[u8]) -> Option<ExifMetadata> {
    let tiff = find_jpeg_exif_tiff(bytes)?;
    parse_tiff_exif_metadata(tiff)
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

fn parse_tiff_exif_metadata(tiff: &[u8]) -> Option<ExifMetadata> {
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
                metadata.color_type = metadata.color_type.take().or_else(|| {
                    tiff_photometric_name(exif_u16_value(tiff, entry, endian)?).map(str::to_string)
                })
            }
            0x010F => metadata.make = exif_ascii(tiff, entry, endian),
            0x0110 => metadata.model = exif_ascii(tiff, entry, endian),
            0x0112 => metadata.orientation = exif_u16_value(tiff, entry, endian),
            0x0131 => metadata.software = exif_ascii(tiff, entry, endian),
            0x0132 | 0x9003 => {
                if metadata.date_time.is_none() {
                    metadata.date_time = exif_ascii(tiff, entry, endian);
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
            5 => altitude_ref = exif_u16_value(tiff, entry, endian),
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
    let text = String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string();
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
        5 => "CMYK",
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

fn read_text_preview_bytes(path: &str) -> Option<(Vec<u8>, bool)> {
    let mut bytes = read_file_prefix(path, MAX_TEXT_BYTES + 1)?;

    let truncated = bytes.len() > MAX_TEXT_BYTES;
    if truncated {
        bytes.truncate(MAX_TEXT_BYTES);
        trim_text_bytes_to_safe_boundary(&mut bytes);
    }
    Some((bytes, truncated))
}

fn trim_text_bytes_to_safe_boundary(bytes: &mut Vec<u8>) {
    if bytes.len() < 2 {
        return;
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        if (bytes.len() - 2) % 2 != 0 {
            bytes.pop();
        }
        return;
    }

    let start = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    if start >= bytes.len() {
        return;
    }

    let min_end = bytes.len().saturating_sub(3).max(start);
    for end in (min_end..=bytes.len()).rev() {
        if std::str::from_utf8(&bytes[start..end]).is_ok() {
            bytes.truncate(end);
            return;
        }
    }
}

fn known_text_formats() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (".md", "markdown", "markdown"),
        (".markdown", "markdown", "markdown"),
        (".txt", "plain", "text"),
        (".log", "plain", "log"),
        (".csv", "plain", "csv"),
        (".tsv", "plain", "tsv"),
        (".env", "code", "env"),
        (".bat", "code", "batch"),
        (".cmd", "code", "batch"),
        (".ps1", "code", "powershell"),
        (".sh", "code", "shell"),
        (".bash", "code", "shell"),
        (".zsh", "code", "shell"),
        (".json", "code", "json"),
        (".xml", "code", "xml"),
        (".xaml", "code", "xaml"),
        (".xsd", "code", "xml"),
        (".resx", "code", "xml"),
        (".config", "code", "xml"),
        (".manifest", "code", "xml"),
        (".policy", "code", "xml"),
        (".settings", "code", "xml"),
        (".ini", "code", "ini"),
        (".cfg", "code", "ini"),
        (".conf", "code", "ini"),
        (".cnf", "code", "ini"),
        (".inf", "code", "ini"),
        (".url", "code", "ini"),
        (".desktop", "code", "ini"),
        (".service", "code", "ini"),
        (".reg", "code", "ini"),
        (".rdp", "code", "properties"),
        (".rc", "code", "properties"),
        (".prefs", "code", "properties"),
        (".properties", "code", "properties"),
        (".yml", "code", "yaml"),
        (".yaml", "code", "yaml"),
        (".toml", "code", "toml"),
        (".cs", "code", "csharp"),
        (".csproj", "code", "xml"),
        (".sln", "plain", "text"),
        (".props", "code", "xml"),
        (".targets", "code", "xml"),
        (".rs", "code", "rust"),
        (".js", "code", "javascript"),
        (".jsx", "code", "javascript"),
        (".mjs", "code", "javascript"),
        (".cjs", "code", "javascript"),
        (".ts", "code", "typescript"),
        (".tsx", "code", "typescript"),
        (".css", "code", "css"),
        (".scss", "code", "scss"),
        (".sass", "code", "sass"),
        (".less", "code", "less"),
        (".html", "code", "html"),
        (".htm", "code", "html"),
        (".py", "code", "python"),
        (".c", "code", "c"),
        (".h", "code", "c"),
        (".cc", "code", "cpp"),
        (".cpp", "code", "cpp"),
        (".cxx", "code", "cpp"),
        (".hpp", "code", "cpp"),
        (".hxx", "code", "cpp"),
        (".java", "code", "java"),
        (".go", "code", "go"),
        (".php", "code", "php"),
        (".rb", "code", "ruby"),
        (".pl", "code", "perl"),
        (".swift", "code", "swift"),
        (".kt", "code", "kotlin"),
        (".kts", "code", "kotlin"),
        (".sql", "code", "sql"),
        (".lua", "code", "lua"),
        (".fs", "code", "fsharp"),
        (".fsx", "code", "fsharp"),
        (".vb", "code", "vb"),
        (".dart", "code", "dart"),
        (".scala", "code", "scala"),
        (".r", "code", "r"),
        (".dockerfile", "code", "dockerfile"),
    ]
}

fn known_text_filenames() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("Dockerfile", "code", "dockerfile"),
        ("Containerfile", "code", "dockerfile"),
        ("Makefile", "code", "makefile"),
        ("CMakeLists.txt", "code", "cmake"),
        (".editorconfig", "code", "ini"),
        (".gitignore", "plain", "text"),
        (".gitattributes", "plain", "text"),
        (".dockerignore", "plain", "text"),
        (".env", "code", "env"),
    ]
}

/// Produce JSON for a text preview: `{"kind":"text","title":"...","format":"...","language":"...","text":"..."}`.
/// Returns empty string on failure.
pub fn render_text(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let (format, language) = known_text_filenames()
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(filename))
        .or_else(|| {
            known_text_formats()
                .iter()
                .find(|(e, _, _)| e.eq_ignore_ascii_case(&ext))
        })
        .map(|(_, f, l)| (*f, *l))
        .unwrap_or(("plain", "text"));

    let (bytes, truncated) = match read_text_preview_bytes(path) {
        Some(result) => result,
        None => return String::new(),
    };

    // BOM-aware Unicode first, then strict UTF-8 and Windows-1252 for legacy configuration files.
    let text = if bytes.len() >= 3 && &bytes[..3] == &[0xEF, 0xBB, 0xBF] {
        encoding_rs::UTF_8.decode(&bytes[3..]).0
    } else if bytes.len() >= 2 && &bytes[..2] == &[0xFF, 0xFE] {
        encoding_rs::UTF_16LE.decode(&bytes[2..]).0
    } else if bytes.len() >= 2 && &bytes[..2] == &[0xFE, 0xFF] {
        encoding_rs::UTF_16BE.decode(&bytes[2..]).0
    } else if std::str::from_utf8(&bytes).is_ok() {
        encoding_rs::UTF_8.decode(&bytes).0
    } else {
        encoding_rs::WINDOWS_1252.decode(&bytes).0
    };

    let mut text = text.into_owned();
    if format == "markdown" {
        return render_markdown_json(filename, &text, truncated);
    }
    if language == "csv" || language == "tsv" {
        return render_delimited_table_json(
            filename,
            &text,
            if language == "tsv" { '\t' } else { ',' },
            language,
            truncated,
        );
    }

    if truncated {
        text.push_str(&format!(
            "\n\n[Preview truncated at {} bytes]",
            MAX_TEXT_BYTES
        ));
    }

    let kind = if format == "markdown" {
        "markdown"
    } else {
        "text"
    };
    to_json(&PreviewReadyDto {
        kind: kind.to_string(),
        title: filename.to_string(),
        format: Some(format.to_string()),
        language: Some(language.to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn render_markdown_json(filename: &str, text: &str, input_truncated: bool) -> String {
    let (blocks, parse_partial) = parse_markdown_blocks(text);
    to_json(&PreviewReadyDto {
        kind: "markdown".to_string(),
        title: filename.to_string(),
        format: Some("markdown".to_string()),
        language: Some("markdown".to_string()),
        text: Some(if input_truncated {
            format!("{text}\n\n[Preview truncated at {MAX_TEXT_BYTES} bytes]")
        } else {
            text.to_string()
        }),
        office_layout: None,
        listing: None,
        table: None,
        markdown: Some(PreviewMarkdownDto {
            blocks,
            is_partial: input_truncated || parse_partial,
        }),
    })
}

fn parse_markdown_blocks(text: &str) -> (Vec<PreviewMarkdownBlockDto>, bool) {
    let lines = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = lines.split('\n').collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let mut partial = false;

    while i < lines.len() {
        if blocks.len() >= MAX_MARKDOWN_BLOCKS {
            partial = true;
            break;
        }

        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if let Some(language) = fenced_code_language(trimmed) {
            i += 1;
            let mut code = String::new();
            while i < lines.len() {
                if lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            blocks.push(markdown_block(
                "code",
                0,
                code.trim_end_matches('\n'),
                &language,
            ));
            continue;
        }

        if is_markdown_rule(trimmed) {
            blocks.push(markdown_block("thematicBreak", 0, "", ""));
            i += 1;
            continue;
        }

        if let Some((level, heading)) = parse_heading(trimmed) {
            let mut block = markdown_block("heading", level, heading, "");
            block.inlines = parse_markdown_inlines(heading);
            blocks.push(block);
            i += 1;
            continue;
        }

        if is_markdown_table_start(&lines, i) {
            let (block, next, table_partial) = parse_markdown_table(&lines, i);
            blocks.push(block);
            partial |= table_partial;
            i = next;
            continue;
        }

        if let Some((ordered, start_text)) = parse_list_item(trimmed) {
            let (block, next, list_partial) = parse_markdown_list(&lines, i, ordered, start_text);
            blocks.push(block);
            partial |= list_partial;
            i = next;
            continue;
        }

        if trimmed.starts_with('>') {
            let (block, next) = parse_markdown_quote(&lines, i);
            blocks.push(block);
            i = next;
            continue;
        }

        let mut paragraph = String::new();
        while i < lines.len() {
            let candidate = lines[i].trim();
            if candidate.is_empty()
                || fenced_code_language(candidate).is_some()
                || parse_heading(candidate).is_some()
                || is_markdown_rule(candidate)
                || is_markdown_table_start(&lines, i)
                || parse_list_item(candidate).is_some()
                || candidate.starts_with('>')
            {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(candidate);
            i += 1;
        }

        if !paragraph.is_empty() {
            let mut block = markdown_block("paragraph", 0, &paragraph, "");
            block.inlines = parse_markdown_inlines(&paragraph);
            blocks.push(block);
        } else {
            i += 1;
        }
    }

    (blocks, partial)
}

fn markdown_block(kind: &str, level: usize, text: &str, language: &str) -> PreviewMarkdownBlockDto {
    PreviewMarkdownBlockDto {
        kind: kind.to_string(),
        level,
        text: truncate_markdown_text(text),
        language: language.to_string(),
        inlines: Vec::new(),
        children: Vec::new(),
        table_headers: Vec::new(),
        table_rows: Vec::new(),
    }
}

fn markdown_inline(
    kind: &str,
    text: &str,
    url: &str,
    children: Vec<PreviewMarkdownInlineDto>,
) -> PreviewMarkdownInlineDto {
    PreviewMarkdownInlineDto {
        kind: kind.to_string(),
        text: truncate_markdown_text(text),
        url: url.to_string(),
        children,
    }
}

fn truncate_markdown_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(MAX_MARKDOWN_INLINE_CHARS) {
        out.push(ch);
    }
    out
}

fn fenced_code_language(trimmed: &str) -> Option<String> {
    trimmed
        .strip_prefix("```")
        .map(|rest| rest.trim().trim_matches('`').to_string())
}

fn parse_heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = trimmed[level..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some((level, rest.trim_end_matches('#').trim_end()))
}

fn is_markdown_rule(trimmed: &str) -> bool {
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3 && chars.iter().all(|c| *c == '-' || *c == '*' || *c == '_')
}

fn parse_list_item(trimmed: &str) -> Option<(bool, &str)> {
    if let Some(text) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some((false, text.trim()));
    }
    let dot = trimmed.find('.')?;
    if dot == 0 || dot > 6 || !trimmed[..dot].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let after = trimmed[dot + 1..].trim_start();
    if after.is_empty() {
        None
    } else {
        Some((true, after))
    }
}

fn parse_markdown_list(
    lines: &[&str],
    start: usize,
    ordered: bool,
    first_text: &str,
) -> (PreviewMarkdownBlockDto, usize, bool) {
    let mut block = markdown_block(
        if ordered {
            "orderedList"
        } else {
            "unorderedList"
        },
        0,
        "",
        "",
    );
    let mut i = start;
    let mut partial = false;
    let mut next_text = Some(first_text.to_string());

    while i < lines.len() {
        let trimmed = lines[i].trim();
        let item_text = if let Some(text) = next_text.take() {
            text
        } else if let Some((item_ordered, text)) = parse_list_item(trimmed) {
            if item_ordered != ordered {
                break;
            }
            text.to_string()
        } else {
            break;
        };

        let mut item = markdown_block("listItem", 0, &item_text, "");
        item.inlines = parse_markdown_inlines(&item_text);
        if block.children.len() < MAX_MARKDOWN_LIST_ITEMS {
            block.children.push(item);
        } else {
            partial = true;
        }
        i += 1;
    }

    (block, i, partial)
}

fn parse_markdown_quote(lines: &[&str], start: usize) -> (PreviewMarkdownBlockDto, usize) {
    let mut text = String::new();
    let mut i = start;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        let Some(rest) = trimmed.strip_prefix('>') else {
            break;
        };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(rest.trim_start());
        i += 1;
    }
    let mut block = markdown_block("blockquote", 0, &text, "");
    block.inlines = parse_markdown_inlines(&text);
    (block, i)
}

fn is_markdown_table_start(lines: &[&str], index: usize) -> bool {
    if index + 1 >= lines.len() {
        return false;
    }
    let header = lines[index].trim();
    let separator = lines[index + 1].trim();
    header.contains('|') && is_markdown_table_separator(separator)
}

fn is_markdown_table_separator(line: &str) -> bool {
    let cells = split_markdown_table_row(line);
    cells.len() >= 2
        && cells.iter().all(|cell| {
            cell.trim()
                .chars()
                .all(|c| c == '-' || c == ':' || c.is_whitespace())
                && cell.contains('-')
        })
}

fn parse_markdown_table(lines: &[&str], start: usize) -> (PreviewMarkdownBlockDto, usize, bool) {
    let mut block = markdown_block("table", 0, "", "");
    block.table_headers = split_markdown_table_row(lines[start])
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect();
    let mut i = start + 2;
    let mut partial = false;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            break;
        }
        if block.table_rows.len() < MAX_MARKDOWN_TABLE_ROWS {
            block.table_rows.push(
                split_markdown_table_row(trimmed)
                    .into_iter()
                    .map(|cell| cell.trim().to_string())
                    .collect(),
            );
        } else {
            partial = true;
        }
        i += 1;
    }
    (block, i, partial)
}

fn split_markdown_table_row(row: &str) -> Vec<String> {
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn parse_markdown_inlines(text: &str) -> Vec<PreviewMarkdownInlineDto> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                out.push(markdown_inline("code", &after[..end], "", Vec::new()));
                i += end + 2;
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                let inner = &after[..end];
                out.push(markdown_inline(
                    "strong",
                    "",
                    "",
                    parse_markdown_inlines(inner),
                ));
                i += end + 4;
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let inner = &after[..end];
                out.push(markdown_inline(
                    "emphasis",
                    "",
                    "",
                    parse_markdown_inlines(inner),
                ));
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    let label = &rest[1..close];
                    let url = &rest[close + 2..close + 2 + end];
                    out.push(markdown_inline(
                        "link",
                        "",
                        url,
                        parse_markdown_inlines(label),
                    ));
                    i += close + 3 + end;
                    continue;
                }
            }
        }

        let next = next_markdown_inline_token(rest);
        out.push(markdown_inline("text", &rest[..next], "", Vec::new()));
        i += next;
    }
    out
}

fn next_markdown_inline_token(text: &str) -> usize {
    let mut next = text.len();
    let start = text.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
    if start > 0 {
        for token in ["`", "**", "*", "["] {
            if let Some(at) = text[start..].find(token) {
                next = next.min(at + start);
            }
        }
    }
    next.max(start)
}

fn render_delimited_table_json(
    filename: &str,
    text: &str,
    delimiter: char,
    format: &str,
    input_truncated: bool,
) -> String {
    let (mut records, total_records, total_columns, parse_partial) =
        parse_delimited_records(text, delimiter);
    if records.is_empty() {
        records.push(vec![String::new()]);
    }

    let first_record = records.first().cloned().unwrap_or_default();
    let has_header = looks_like_header_row(&first_record);
    let display_total_columns = total_columns.max(1);
    let column_count = display_total_columns.clamp(1, MAX_TABLE_COLUMNS);
    let headers = if has_header {
        normalize_table_headers(first_record, column_count)
    } else {
        (0..column_count)
            .map(|i| format!("Column {}", i + 1))
            .collect()
    };

    let data_records = if has_header {
        records.into_iter().skip(1).collect::<Vec<_>>()
    } else {
        records
    };
    let total_rows = total_records.saturating_sub(usize::from(has_header));
    let rows = data_records
        .into_iter()
        .take(MAX_TABLE_ROWS)
        .map(|record| PreviewTableRowDto {
            cells: normalize_table_cells(record, headers.len()),
        })
        .collect::<Vec<_>>();
    let is_partial = input_truncated
        || parse_partial
        || total_rows > MAX_TABLE_ROWS
        || display_total_columns > MAX_TABLE_COLUMNS;

    to_json(&PreviewReadyDto {
        kind: "table".to_string(),
        title: format!("{filename} - Table"),
        format: Some(format.to_string()),
        language: Some(format.to_string()),
        text: None,
        office_layout: None,
        listing: None,
        table: Some(PreviewTableDto {
            format: format.to_string(),
            delimiter: delimiter.to_string(),
            headers,
            rows,
            total_rows,
            total_columns: display_total_columns,
            is_partial,
        }),
        markdown: None,
    })
}

fn parse_delimited_records(text: &str, delimiter: char) -> (Vec<Vec<String>>, usize, usize, bool) {
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut total_records = 0usize;
    let mut total_columns = 0usize;
    let mut is_partial = false;
    let mut in_quotes = false;
    let mut saw_any = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        saw_any = true;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    push_table_char(&mut cell, '"');
                    if cell.chars().count() >= MAX_TABLE_CELL_CHARS {
                        is_partial = true;
                    }
                } else {
                    in_quotes = false;
                }
            } else {
                if cell.chars().count() < MAX_TABLE_CELL_CHARS {
                    cell.push(ch);
                } else if !ch.is_control() {
                    is_partial = true;
                }
            }
            continue;
        }

        if ch == '"' && cell.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
            finish_table_row(&mut records, &mut row, &mut total_records, &mut is_partial);
        } else if cell.chars().count() < MAX_TABLE_CELL_CHARS {
            cell.push(ch);
        } else {
            is_partial = true;
        }
    }

    if saw_any && (!cell.is_empty() || !row.is_empty()) {
        finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
        finish_table_row(&mut records, &mut row, &mut total_records, &mut is_partial);
    }

    (records, total_records, total_columns, is_partial)
}

fn push_table_char(cell: &mut String, ch: char) {
    if cell.chars().count() < MAX_TABLE_CELL_CHARS {
        cell.push(ch);
    }
}

fn finish_table_cell(
    row: &mut Vec<String>,
    cell: &mut String,
    total_columns: &mut usize,
    is_partial: &mut bool,
) {
    *total_columns = (*total_columns).max(row.len() + 1);
    if row.len() < MAX_TABLE_COLUMNS {
        row.push(cell.to_string());
    } else {
        *is_partial = true;
    }
    cell.clear();
}

fn finish_table_row(
    records: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    total_records: &mut usize,
    is_partial: &mut bool,
) {
    *total_records += 1;
    if records.len() < MAX_TABLE_ROWS + 1 {
        records.push(std::mem::take(row));
    } else {
        row.clear();
        *is_partial = true;
    }
}

fn looks_like_header_row(row: &[String]) -> bool {
    row.iter()
        .any(|cell| cell.chars().any(|ch| ch.is_alphabetic()))
}

fn normalize_table_headers(mut headers: Vec<String>, column_count: usize) -> Vec<String> {
    headers.truncate(column_count);
    while headers.len() < column_count {
        headers.push(String::new());
    }
    headers
        .into_iter()
        .enumerate()
        .map(|(index, header)| {
            let header = header.trim();
            if header.is_empty() {
                format!("Column {}", index + 1)
            } else {
                header.to_string()
            }
        })
        .collect()
}

fn normalize_table_cells(mut cells: Vec<String>, column_count: usize) -> Vec<String> {
    cells.truncate(column_count);
    while cells.len() < column_count {
        cells.push(String::new());
    }
    cells
}

/// Check if a file is text-like (extension known or a small printable Unicode header).
pub fn is_text(ext: &str, magic: &[u8]) -> bool {
    if known_text_formats()
        .iter()
        .any(|(e, _, _)| e.eq_ignore_ascii_case(ext))
    {
        return true;
    }
    is_probably_utf8_text(magic)
}

pub fn is_text_file(file_name: &str, ext: &str, magic: &[u8]) -> bool {
    known_text_filenames()
        .iter()
        .any(|(name, _, _)| name.eq_ignore_ascii_case(file_name))
        || is_text(ext, magic)
}

fn is_probably_utf8_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return is_probably_utf16_text(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return is_probably_utf16_text(&bytes[2..], false);
    }
    if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return is_probably_windows_1252_text(bytes);
    }
    let printable = bytes
        .iter()
        .filter(|b| matches!(**b, b'\t' | b'\r' | b'\n' | 0x20..=0x7E) || **b >= 0x80)
        .count();
    printable * 100 / bytes.len().max(1) >= 90
}

fn is_probably_windows_1252_text(bytes: &[u8]) -> bool {
    if bytes.is_empty()
        || bytes.contains(&0)
        || !bytes
            .iter()
            .any(|byte| matches!(*byte, b'=' | b':' | b'[' | b'#' | b';' | b'\r' | b'\n'))
    {
        return false;
    }
    let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    let char_count = text.chars().count();
    let printable = text
        .chars()
        .filter(|ch| matches!(*ch, '\t' | '\r' | '\n') || !ch.is_control())
        .count();
    char_count > 0 && printable * 100 / char_count >= 90
}

fn is_probably_utf16_text(bytes: &[u8], little_endian: bool) -> bool {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return false;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|unit| {
            if little_endian {
                u16::from_le_bytes([unit[0], unit[1]])
            } else {
                u16::from_be_bytes([unit[0], unit[1]])
            }
        })
        .collect();
    let Ok(text) = String::from_utf16(&units) else {
        return false;
    };
    let char_count = text.chars().count();
    let printable = text
        .chars()
        .filter(|ch| matches!(*ch, '\t' | '\r' | '\n') || !ch.is_control())
        .count();
    char_count > 0 && printable * 100 / char_count >= 90
}

// ── Office preview (OOXML / ODF lightweight extraction) ─────────────────────

const MAX_OFFICE_TEXT_CHARS: usize = 96 * 1024;
const MAX_OFFICE_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OFFICE_ZIP_ENTRIES: usize = 8_192;
const MAX_OFFICE_ROWS: usize = 48;
const MAX_OFFICE_SHEETS: usize = 6;
const MAX_OFFICE_SLIDES: usize = 30;
const MAX_OFFICE_TABLE_CELL_WIDTH: usize = 36;
const MAX_OFFICE_MEDIA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OFFICE_LAYOUT_IMAGES: usize = 18;
const MAX_OFFICE_INLINE_IMAGE_BYTES: u64 = 768 * 1024;
const OFFICE_EMUS_PER_DIP: f64 = 9525.0;
const XLSX_CELL_WIDTH: f64 = 96.0;
const XLSX_ROW_HEIGHT: f64 = 28.0;

type OfficeResult<T> = Result<T, OfficeReadError>;

#[derive(Debug)]
enum OfficeReadError {
    Cancelled,
    BudgetExhausted,
}

struct OfficeContext {
    remaining_decompressed_bytes: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
}

impl OfficeContext {
    fn new(cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
        Self {
            remaining_decompressed_bytes: MAX_OFFICE_DECOMPRESSED_BYTES,
            cancel_cb,
        }
    }

    fn check_cancelled(&self) -> OfficeResult<()> {
        if preview_cancelled(self.cancel_cb) {
            Err(OfficeReadError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn consume(&mut self, bytes: u64) -> OfficeResult<()> {
        self.check_cancelled()?;
        if bytes > self.remaining_decompressed_bytes {
            return Err(OfficeReadError::BudgetExhausted);
        }
        self.remaining_decompressed_bytes -= bytes;
        Ok(())
    }

    fn check_xml_event(&self, event_count: usize) -> OfficeResult<()> {
        if event_count % 256 == 0 {
            self.check_cancelled()?;
        }
        Ok(())
    }
}

pub fn render_office(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    if fs::metadata(path)
        .ok()
        .is_some_and(|metadata| metadata.len() > MAX_OFFICE_INPUT_BYTES)
    {
        return String::new();
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut context = OfficeContext::new(cancel_cb);
    match ext.as_str() {
        "docx" => render_docx(path, &mut context).unwrap_or_default(),
        "xlsx" | "xlsm" => render_xlsx(path, &mut context).unwrap_or_default(),
        "pptx" | "pptm" => render_pptx(path, &mut context).unwrap_or_default(),
        "odt" | "ods" | "odp" => render_odf(path, &mut context).unwrap_or_default(),
        _ => {
            let meta = fs::metadata(path).ok();
            let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            render_info(path, "office", size, modified)
        }
    }
}

fn render_docx(path: &str, context: &mut OfficeContext) -> OfficeResult<String> {
    let filename = file_name(path);
    let mut zip = match open_office_zip(path) {
        Some(zip) => zip,
        None => return Ok(String::new()),
    };
    let media_entries = office_media_entries(context, &mut zip, &["word/media/"])?;
    let header_footer_entries = docx_header_footer_entries(context, &mut zip)?;
    let xml = match read_office_zip_text(context, &mut zip, "word/document.xml", 16 * 1024 * 1024)?
    {
        Some(xml) => xml,
        None => {
            return Ok(office_error_json(
                path,
                "DOCX",
                "word/document.xml not found",
            ))
        }
    };
    let header_footer_text =
        extract_docx_header_footer_text(context, &mut zip, &header_footer_entries)?;
    let body = extract_wordprocessing_text(context, &xml)?;
    let layout = build_docx_layout(context, &mut zip, &body, &media_entries)?;
    let mut text = format!("Name: {filename}\nKind: Word document\n");
    append_office_media_summary(&mut text, &media_entries);
    if !header_footer_text.is_empty() {
        text.push_str("Headers/footers:\n");
        text.push_str(&header_footer_text);
        text.push('\n');
    }
    let text = if body.trim().is_empty() {
        text.push_str("Status: no extractable text");
        text
    } else {
        text.push('\n');
        text.push_str(&truncate_preview_text(&body));
        text
    };
    Ok(office_preview_json_with_layout(
        path, "DOCX", text, "plain", "text", layout,
    ))
}

fn render_pptx(path: &str, context: &mut OfficeContext) -> OfficeResult<String> {
    let filename = file_name(path);
    let mut zip = match open_office_zip(path) {
        Some(zip) => zip,
        None => return Ok(String::new()),
    };
    let media_entries = office_media_entries(context, &mut zip, &["ppt/media/"])?;
    let mut slides = Vec::new();
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(xml) = read_office_zip_text(context, &mut zip, &name, 8 * 1024 * 1024)? else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };
        let text = extract_ppt_text(context, &xml)?;
        if !text.trim().is_empty() {
            slides.push(format!("Slide {slide_idx}\n{}", text.trim()));
        }
    }

    let body = if slides.is_empty() {
        "Status: no extractable slide text".to_string()
    } else {
        slides.join("\n\n")
    };
    let mut text = format!("Name: {filename}\nKind: PowerPoint presentation\n");
    append_office_media_summary(&mut text, &media_entries);
    text.push('\n');
    text.push_str(&truncate_preview_text(&body));
    let layout = build_pptx_layout(context, &mut zip)?;
    Ok(office_preview_json_with_layout(
        path,
        "PowerPoint presentation",
        text,
        "plain",
        "text",
        layout,
    ))
}

fn render_xlsx(path: &str, context: &mut OfficeContext) -> OfficeResult<String> {
    let filename = file_name(path);
    let mut zip = match open_office_zip(path) {
        Some(zip) => zip,
        None => return Ok(String::new()),
    };
    let media_entries = office_media_entries(context, &mut zip, &["xl/media/"])?;
    let shared_strings =
        read_office_zip_text(context, &mut zip, "xl/sharedStrings.xml", 16 * 1024 * 1024)?
            .map(|xml| parse_shared_strings(context, &xml))
            .transpose()?
            .unwrap_or_default();

    let mut sections = Vec::new();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(xml) = read_office_zip_text(context, &mut zip, &name, 16 * 1024 * 1024)? else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };
        let rows = parse_worksheet_rows(context, &xml, &shared_strings)?;
        if rows.is_empty() {
            continue;
        }
        sections.push(format!(
            "Sheet {sheet_idx}\n{}",
            format_table_rows(&rows).join("\n")
        ));
    }

    let body = if sections.is_empty() {
        "Status: no extractable worksheet cells".to_string()
    } else {
        sections.join("\n\n")
    };
    let mut text = format!("Name: {filename}\nKind: Excel workbook\n");
    append_office_media_summary(&mut text, &media_entries);
    text.push('\n');
    text.push_str(&truncate_preview_text(&body));
    let layout = build_xlsx_layout(context, &mut zip, &shared_strings)?;
    Ok(office_preview_json_with_layout(
        path,
        "Excel workbook",
        text,
        "code",
        "text",
        layout,
    ))
}

fn render_odf(path: &str, context: &mut OfficeContext) -> OfficeResult<String> {
    let filename = file_name(path);
    let mut zip = match open_office_zip(path) {
        Some(zip) => zip,
        None => return Ok(String::new()),
    };
    let xml = match read_office_zip_text(context, &mut zip, "content.xml", 16 * 1024 * 1024)? {
        Some(xml) => xml,
        None => {
            return Ok(office_error_json(
                path,
                "OpenDocument",
                "content.xml not found",
            ))
        }
    };
    let body = extract_wordprocessing_text(context, &xml)?;
    Ok(office_text_json(
        path,
        "OpenDocument",
        format!(
            "Name: {filename}\nKind: OpenDocument\n\n{}",
            truncate_preview_text(&body)
        ),
    ))
}

fn open_zip(path: &str) -> Option<ZipArchive<fs::File>> {
    let file = fs::File::open(path).ok()?;
    ZipArchive::new(file).ok()
}

fn open_office_zip(path: &str) -> Option<ZipArchive<fs::File>> {
    let zip = open_zip(path)?;
    (zip.len() <= MAX_OFFICE_ZIP_ENTRIES).then_some(zip)
}

fn read_zip_text<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max_size: u64,
) -> Option<String> {
    if let Ok(mut entry) = zip.by_name(name) {
        if entry.size() > max_size {
            return None;
        }
        let bytes = read_limited_to_end(&mut entry, max_size)?;
        return Some(String::from_utf8_lossy(&bytes).to_string());
    }

    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        let mut entry = zip.by_index(i).ok()?;
        if !entry.name().replace('\\', "/").eq_ignore_ascii_case(name) {
            continue;
        }
        if entry.size() > max_size {
            return None;
        }
        let bytes = read_limited_to_end(&mut entry, max_size)?;
        return Some(String::from_utf8_lossy(&bytes).to_string());
    }

    None
}

fn read_limited_to_end<R: Read>(reader: &mut R, max_size: u64) -> Option<Vec<u8>> {
    let cap = max_size.min(64 * 1024) as usize;
    let mut limited = reader.take(max_size.saturating_add(1));
    let mut bytes = Vec::with_capacity(cap);
    limited.read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max_size {
        return None;
    }
    Some(bytes)
}

fn office_media_entries<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    roots: &[&str],
) -> OfficeResult<Vec<String>> {
    let mut entries = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > MAX_OFFICE_MEDIA_BYTES {
            continue;
        }

        let normalized = entry.name().replace('\\', "/");
        let lower = normalized.to_ascii_lowercase();
        if roots.iter().any(|root| lower.starts_with(root)) && is_supported_zip_image_name(&lower) {
            entries.push(normalized);
        }
    }
    entries.sort();
    Ok(entries)
}

fn append_office_media_summary(out: &mut String, entries: &[String]) {
    out.push_str(&format!("Images: {}\n", entries.len()));
    if entries.is_empty() {
        return;
    }

    let names = entries
        .iter()
        .take(6)
        .map(|name| name.rsplit('/').next().unwrap_or(name.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("Image files: {names}\n"));
}

fn build_pptx_layout(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<fs::File>,
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let presentation_xml =
        read_office_zip_text(context, zip, "ppt/presentation.xml", 4 * 1024 * 1024)?;
    let (slide_width, slide_height) = presentation_xml
        .as_deref()
        .map(|xml| parse_ppt_slide_size(context, xml))
        .transpose()?
        .flatten()
        .unwrap_or((960.0, 540.0));

    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let slide_name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(slide_xml) = read_office_zip_text(context, zip, &slide_name, 8 * 1024 * 1024)?
        else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };

        let rels_name = format!("ppt/slides/_rels/slide{slide_idx}.xml.rels");
        let rels = read_office_zip_text(context, zip, &rels_name, 2 * 1024 * 1024)?
            .map(|xml| parse_relationships(context, &xml))
            .transpose()?
            .unwrap_or_default();
        let background_color = parse_ppt_slide_background(context, &slide_xml)?;
        let items = parse_ppt_slide_items(
            context,
            zip,
            "ppt/slides/",
            &slide_xml,
            &rels,
            &mut image_budget,
        )?;
        pages.push(OfficePageDto {
            title: format!("Slide {slide_idx}"),
            index: slide_idx,
            width: slide_width,
            height: slide_height,
            background_color,
            freeze_rows: None,
            freeze_columns: None,
            cells: Vec::new(),
            items,
        });
    }

    if pages.is_empty() {
        return Ok(None);
    }

    Ok(Some(OfficeLayoutDto {
        layout_kind: "presentation".to_string(),
        width: slide_width,
        height: slide_height,
        pages,
    }))
}

fn build_xlsx_layout(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<fs::File>,
    shared_strings: &[String],
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    let styles = read_office_zip_text(context, zip, "xl/styles.xml", 4 * 1024 * 1024)?
        .map(|xml| parse_xlsx_styles(context, &xml))
        .transpose()?
        .unwrap_or_default();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let sheet_name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(sheet_xml) = read_office_zip_text(context, zip, &sheet_name, 16 * 1024 * 1024)?
        else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };

        let metrics = parse_xlsx_sheet_metrics(context, &sheet_xml)?;
        let merge_regions = parse_xlsx_merge_regions(context, &sheet_xml)?;
        let (freeze_rows, freeze_columns) = parse_xlsx_freeze_pane(context, &sheet_xml)?;
        let mut cells = parse_worksheet_layout_cells(
            context,
            &sheet_xml,
            shared_strings,
            &metrics,
            &merge_regions,
            &styles,
        )?;
        let mut items = parse_xlsx_sheet_images(
            context,
            zip,
            sheet_idx,
            &sheet_xml,
            &metrics,
            &mut image_budget,
        )?;
        let (width, height) = xlsx_page_size(&cells, &items);
        if cells.is_empty() && items.is_empty() {
            continue;
        }
        cells.sort_by_key(|cell| (cell.row, cell.column));
        items.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
        pages.push(OfficePageDto {
            title: format!("Sheet {sheet_idx}"),
            index: sheet_idx,
            width,
            height,
            background_color: None,
            freeze_rows,
            freeze_columns,
            cells,
            items,
        });
    }

    if pages.is_empty() {
        return Ok(None);
    }

    let width = pages.iter().map(|p| p.width).fold(0.0, f64::max);
    let height = pages.first().map(|p| p.height).unwrap_or(420.0);
    Ok(Some(OfficeLayoutDto {
        layout_kind: "workbook".to_string(),
        width,
        height,
        pages,
    }))
}

fn build_docx_layout(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<fs::File>,
    body: &str,
    media_entries: &[String],
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let page_width = 760.0;
    let page_height = 980.0;
    let margin = 58.0;
    let mut pages = Vec::new();
    let mut items = Vec::new();
    let mut page_index = 1usize;
    let mut y = margin;

    for paragraph in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let clipped = paragraph.chars().take(420).collect::<String>();
        let line_count = (clipped.chars().count() as f64 / 72.0)
            .ceil()
            .clamp(1.0, 5.0);
        let height = 24.0 * line_count + 10.0;
        if y + height > page_height - margin {
            push_docx_page(&mut pages, page_index, page_width, page_height, items);
            page_index += 1;
            items = Vec::new();
            y = margin;
        }

        items.push(OfficeLayoutItemDto {
            kind: "text".to_string(),
            x: margin,
            y,
            width: page_width - margin * 2.0,
            height,
            z_index: items.len(),
            text: Some(clipped),
            shape: None,
            placeholder_type: None,
            bold: false,
            italic: false,
            font_size: None,
            fill_color: None,
            stroke_color: None,
            image_name: None,
            mime_type: None,
            image_base64: None,
        });
        y += height + 6.0;

        if pages.len() >= 8 {
            break;
        }
    }

    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES.min(6);
    for entry in media_entries.iter().take(6) {
        if image_budget == 0 {
            break;
        }
        if y + 180.0 > page_height - margin {
            push_docx_page(&mut pages, page_index, page_width, page_height, items);
            page_index += 1;
            items = Vec::new();
            y = margin;
        }
        let lower = entry.to_ascii_lowercase();
        let Some(bytes) =
            read_office_zip_bytes(context, zip, entry, MAX_OFFICE_INLINE_IMAGE_BYTES)?
        else {
            continue;
        };
        image_budget = image_budget.saturating_sub(1);
        items.push(OfficeLayoutItemDto {
            kind: "image".to_string(),
            x: margin,
            y,
            width: 260.0,
            height: 170.0,
            z_index: items.len(),
            text: None,
            shape: None,
            placeholder_type: None,
            bold: false,
            italic: false,
            font_size: None,
            fill_color: None,
            stroke_color: None,
            image_name: Some(
                entry
                    .rsplit('/')
                    .next()
                    .unwrap_or(entry.as_str())
                    .to_string(),
            ),
            mime_type: image_mime_type(&lower).map(str::to_string),
            image_base64: Some(base64_encode(&bytes)),
        });
        y += 188.0;
    }

    if !items.is_empty() || pages.is_empty() {
        push_docx_page(&mut pages, page_index, page_width, page_height, items);
    }

    if pages.iter().all(|page| page.items.is_empty()) {
        return Ok(None);
    }

    Ok(Some(OfficeLayoutDto {
        layout_kind: "document".to_string(),
        width: page_width,
        height: page_height,
        pages,
    }))
}

fn push_docx_page(
    pages: &mut Vec<OfficePageDto>,
    page_index: usize,
    width: f64,
    height: f64,
    items: Vec<OfficeLayoutItemDto>,
) {
    pages.push(OfficePageDto {
        title: format!("Page {page_index}"),
        index: page_index,
        width,
        height,
        background_color: Some("#FFFFFF".to_string()),
        freeze_rows: None,
        freeze_columns: None,
        cells: Vec::new(),
        items,
    });
}

fn parse_ppt_slide_size(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<Option<(f64, f64)>> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "sldsz" {
                    let Some(cx) = attr_f64(&e, "cx") else {
                        continue;
                    };
                    let Some(cy) = attr_f64(&e, "cy") else {
                        continue;
                    };
                    return Ok(Some((
                        (cx / OFFICE_EMUS_PER_DIP).max(320.0),
                        (cy / OFFICE_EMUS_PER_DIP).max(180.0),
                    )));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_ppt_slide_background(context: &OfficeContext, xml: &str) -> OfficeResult<Option<String>> {
    let mut reader = Reader::from_str(xml);
    let mut in_background = false;
    let mut depth = 0usize;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_background && (local == "bg" || local == "bgpr") {
                    in_background = true;
                    depth = 1;
                } else if in_background {
                    depth += 1;
                    if (local == "srgbclr" || local == "schemeclr")
                        && office_color_from_element(&e).is_some()
                    {
                        return Ok(office_color_from_element(&e));
                    }
                }
            }
            Ok(Event::Empty(e)) if in_background => {
                let local = local_xml_name(e.name().as_ref());
                if local == "srgbclr" || local == "schemeclr" {
                    if let Some(color) = office_color_from_element(&e) {
                        return Ok(Some(color));
                    }
                }
            }
            Ok(Event::End(_)) if in_background => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    in_background = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_ppt_slide_items<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    xml: &str,
    rels: &BTreeMap<String, String>,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut in_shape = false;
    let mut shape_depth = 0usize;
    let mut shape_kind = "";
    let mut x = 0.0;
    let mut y = 0.0;
    let mut width = 0.0;
    let mut height = 0.0;
    let mut rel_id = String::new();
    let mut text = String::new();
    let mut in_text = false;
    let mut shape_paragraph_had_text = false;
    let mut paragraph_prefix = String::new();
    let mut preset_shape: Option<String> = None;
    let mut placeholder_type: Option<String> = None;
    let mut text_bold = false;
    let mut text_italic = false;
    let mut text_font_size: Option<f64> = None;
    let mut fill_color: Option<String> = None;
    let mut stroke_color: Option<String> = None;
    let mut color_target = "";
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_shape && (local == "sp" || local == "pic") {
                    in_shape = true;
                    shape_depth = 1;
                    shape_kind = if local == "pic" { "image" } else { "text" };
                    x = 0.0;
                    y = 0.0;
                    width = 0.0;
                    height = 0.0;
                    rel_id.clear();
                    text.clear();
                    in_text = false;
                    shape_paragraph_had_text = false;
                    paragraph_prefix.clear();
                    preset_shape = None;
                    placeholder_type = None;
                    text_bold = false;
                    text_italic = false;
                    text_font_size = None;
                    fill_color = None;
                    stroke_color = None;
                    color_target = "";
                    continue;
                }
                if in_shape {
                    shape_depth += 1;
                    if local == "t" {
                        if !paragraph_prefix.is_empty() && !shape_paragraph_had_text {
                            text.push_str(&paragraph_prefix);
                        }
                        in_text = true;
                    } else if local == "ppr" {
                        paragraph_prefix = ppt_paragraph_prefix(&e);
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
                    } else if local == "solidfill" {
                        color_target = "fill";
                    } else if local == "ln" {
                        color_target = "stroke";
                    } else if local == "ph" {
                        placeholder_type =
                            attr_value(&e, "type").or_else(|| Some("body".to_string()));
                    } else if local == "rpr" {
                        apply_ppt_run_style(
                            &e,
                            &mut text_bold,
                            &mut text_italic,
                            &mut text_font_size,
                        );
                    }
                }
            }
            Ok(Event::Empty(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "off" {
                    x = attr_f64(&e, "x").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    y = attr_f64(&e, "y").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                } else if local == "ext" {
                    width = attr_f64(&e, "cx").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    height = attr_f64(&e, "cy").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                } else if local == "blip" {
                    rel_id = attr_value(&e, "embed").unwrap_or_default();
                } else if local == "prstgeom" {
                    preset_shape = attr_value(&e, "prst");
                } else if local == "ph" {
                    placeholder_type = attr_value(&e, "type").or_else(|| Some("body".to_string()));
                } else if local == "rpr" {
                    apply_ppt_run_style(&e, &mut text_bold, &mut text_italic, &mut text_font_size);
                } else if local == "srgbclr" || local == "schemeclr" {
                    let color = office_color_from_element(&e);
                    if color_target == "stroke" {
                        stroke_color = color.or(stroke_color);
                    } else {
                        fill_color = color.or(fill_color);
                    }
                } else if local == "tab" && shape_kind == "text" {
                    text.push('\t');
                    shape_paragraph_had_text = true;
                } else if local == "br" && shape_kind == "text" {
                    text.push('\n');
                    shape_paragraph_had_text = false;
                } else if local == "ppr" && shape_kind == "text" {
                    paragraph_prefix = ppt_paragraph_prefix(&e);
                } else if local == "buchar" && shape_kind == "text" {
                    append_ppt_bullet_prefix(&mut paragraph_prefix, &e);
                }
            }
            Ok(Event::End(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" && shape_kind == "text" {
                    if shape_paragraph_had_text && !text.ends_with('\n') {
                        text.push('\n');
                    }
                    shape_paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "solidfill" || local == "ln" {
                    color_target = "";
                }
                shape_depth = shape_depth.saturating_sub(1);
                if shape_depth == 0 {
                    if shape_kind == "text" {
                        let normalized = normalize_preview_lines(&text);
                        if width > 2.0
                            && height > 2.0
                            && (!normalized.is_empty()
                                || preset_shape.is_some()
                                || fill_color.is_some()
                                || stroke_color.is_some())
                        {
                            items.push(OfficeLayoutItemDto {
                                kind: if normalized.is_empty() {
                                    "shape".to_string()
                                } else {
                                    "text".to_string()
                                },
                                x,
                                y,
                                width,
                                height,
                                z_index: items.len(),
                                text: (!normalized.is_empty()).then_some(normalized),
                                shape: preset_shape.clone(),
                                placeholder_type: placeholder_type.clone(),
                                bold: text_bold,
                                italic: text_italic,
                                font_size: text_font_size,
                                fill_color: fill_color.clone(),
                                stroke_color: stroke_color.clone(),
                                image_name: None,
                                mime_type: None,
                                image_base64: None,
                            });
                        }
                    } else if let Some(item) = image_item_from_relationship(
                        context,
                        zip,
                        base_dir,
                        rels,
                        &rel_id,
                        x,
                        y,
                        width,
                        height,
                        items.len(),
                        image_budget,
                    )? {
                        items.push(item);
                    }
                    in_shape = false;
                }
            }
            Ok(Event::Text(e)) if in_shape && in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    text.push_str(&value);
                    shape_paragraph_had_text = true;
                }
            }
            Ok(Event::CData(e)) if in_shape && in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    text.push_str(&value);
                    shape_paragraph_had_text = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(items)
}

fn ppt_paragraph_prefix(e: &BytesStart<'_>) -> String {
    let mut prefix = String::new();
    match attr_value(e, "algn").as_deref() {
        Some("ctr") => prefix.push_str("[center] "),
        Some("r") => prefix.push_str("[right] "),
        _ => {}
    }
    prefix
}

fn append_ppt_bullet_prefix(prefix: &mut String, e: &BytesStart<'_>) {
    let bullet = attr_value(e, "char")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "•".to_string());
    prefix.push_str(&bullet);
    prefix.push(' ');
}

fn apply_ppt_run_style(
    e: &BytesStart<'_>,
    bold: &mut bool,
    italic: &mut bool,
    font_size: &mut Option<f64>,
) {
    if attr_bool(e, "b") == Some(true) {
        *bold = true;
    }
    if attr_bool(e, "i") == Some(true) {
        *italic = true;
    }
    if let Some(size) = attr_f64(e, "sz") {
        *font_size = Some((size / 100.0).clamp(6.0, 60.0));
    }
}

fn extract_ppt_text(context: &OfficeContext, xml: &str) -> OfficeResult<String> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = true;
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" {
                    if paragraph_had_text && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    paragraph_had_text = false;
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "tab" {
                    out.push('\t');
                    paragraph_had_text = true;
                } else if local == "br" {
                    out.push('\n');
                    paragraph_had_text = false;
                }
            }
            Ok(Event::Text(e)) if in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    out.push_str(&value);
                    paragraph_had_text = true;
                }
            }
            Ok(Event::CData(e)) if in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    out.push_str(&value);
                    paragraph_had_text = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(normalize_preview_lines(&out))
}

fn parse_worksheet_layout_cells(
    context: &OfficeContext,
    xml: &str,
    shared_strings: &[String],
    metrics: &XlsxSheetMetrics,
    merge_regions: &BTreeMap<(usize, usize), XlsxMergeRegion>,
    styles: &[XlsxStyle],
) -> OfficeResult<Vec<OfficeCellDto>> {
    let mut reader = Reader::from_str(xml);
    let mut cells = Vec::new();
    let mut in_row = false;
    let mut in_cell = false;
    let mut in_value = false;
    let mut in_inline_text = false;
    let mut display_row = 0usize;
    let mut row_index = 0usize;
    let mut next_col = 0usize;
    let mut cell_col = 0usize;
    let mut cell_style = 0usize;
    let mut cell_type = String::new();
    let mut cell_value = String::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "row" => {
                        in_row = true;
                        display_row += 1;
                        row_index = attr_value(&e, "r")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(display_row)
                            .saturating_sub(1);
                        next_col = 0;
                    }
                    "c" if in_row => {
                        in_cell = true;
                        cell_type.clear();
                        cell_value.clear();
                        cell_col = attr_value(&e, "r")
                            .and_then(|reference| cell_reference_column(&reference))
                            .unwrap_or(next_col);
                        next_col = cell_col + 1;
                        cell_style = attr_value(&e, "s")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        cell_type = attr_value(&e, "t").unwrap_or_default();
                    }
                    "v" if in_cell => in_value = true,
                    "t" if in_cell => in_inline_text = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "v" => in_value = false,
                    "t" => in_inline_text = false,
                    "c" if in_cell => {
                        let value = resolve_cell_value(&cell_value, &cell_type, shared_strings);
                        let merge = merge_regions.get(&(row_index, cell_col));
                        if merge.is_none()
                            && is_inside_non_origin_merge(merge_regions, row_index, cell_col)
                        {
                            in_cell = false;
                            continue;
                        }
                        if !value.trim().is_empty() && row_index < MAX_OFFICE_ROWS && cell_col < 32
                        {
                            let row_span = merge.map(|m| m.row_span).unwrap_or(1);
                            let column_span = merge.map(|m| m.column_span).unwrap_or(1);
                            cells.push(OfficeCellDto {
                                row: row_index,
                                column: cell_col,
                                text: clean_table_cell(&value),
                                x: xlsx_col_x(metrics, cell_col),
                                y: xlsx_row_y(metrics, row_index),
                                width: xlsx_col_span_width(
                                    metrics,
                                    cell_col,
                                    cell_col + column_span,
                                ),
                                height: xlsx_row_span_height(
                                    metrics,
                                    row_index,
                                    row_index + row_span,
                                ),
                                row_span,
                                column_span,
                                number_format: styles
                                    .get(cell_style)
                                    .and_then(|style| style.number_format.clone()),
                                fill_color: styles
                                    .get(cell_style)
                                    .and_then(|style| style.fill_color.clone()),
                                text_color: styles
                                    .get(cell_style)
                                    .and_then(|style| style.text_color.clone()),
                                horizontal_alignment: styles
                                    .get(cell_style)
                                    .and_then(|style| style.horizontal_alignment.clone()),
                                vertical_alignment: styles
                                    .get(cell_style)
                                    .and_then(|style| style.vertical_alignment.clone()),
                                bold: styles
                                    .get(cell_style)
                                    .map(|style| style.bold)
                                    .unwrap_or(false),
                                italic: styles
                                    .get(cell_style)
                                    .map(|style| style.italic)
                                    .unwrap_or(false),
                                font_size: styles.get(cell_style).and_then(|style| style.font_size),
                                wrap_text: styles
                                    .get(cell_style)
                                    .map(|style| style.wrap_text)
                                    .unwrap_or(false),
                            });
                        }
                        in_cell = false;
                    }
                    "row" => in_row = false,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_unescape_bytes(e.as_ref()))
            }
            Ok(Event::CData(e)) if in_value || in_inline_text => {
                cell_value.push_str(&String::from_utf8_lossy(e.as_ref()))
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(cells)
}

#[derive(Clone, Default)]
struct XlsxStyle {
    number_format: Option<String>,
    fill_color: Option<String>,
    text_color: Option<String>,
    horizontal_alignment: Option<String>,
    vertical_alignment: Option<String>,
    bold: bool,
    italic: bool,
    font_size: Option<f64>,
    wrap_text: bool,
}

fn parse_xlsx_styles(context: &OfficeContext, xml: &str) -> OfficeResult<Vec<XlsxStyle>> {
    let mut reader = Reader::from_str(xml);
    let mut custom_formats = BTreeMap::<u32, String>::new();
    let mut font_bold = Vec::<bool>::new();
    let mut font_italic = Vec::<bool>::new();
    let mut font_sizes = Vec::<Option<f64>>::new();
    let mut font_colors = Vec::<Option<String>>::new();
    let mut fill_colors = Vec::<Option<String>>::new();
    let mut styles = Vec::<XlsxStyle>::new();
    let mut in_fonts = false;
    let mut in_font = false;
    let mut in_fills = false;
    let mut in_fill = false;
    let mut in_cell_xfs = false;
    let mut in_xf = false;
    let mut current_xf: Option<XlsxStyle> = None;
    let mut current_font_bold = false;
    let mut current_font_italic = false;
    let mut current_font_size: Option<f64> = None;
    let mut current_font_color: Option<String> = None;
    let mut current_fill_color: Option<String> = None;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "fonts" {
                    in_fonts = true;
                } else if local == "font" && in_fonts {
                    in_font = true;
                    current_font_bold = false;
                    current_font_italic = false;
                    current_font_size = None;
                    current_font_color = None;
                } else if local == "b" && in_font {
                    current_font_bold = true;
                } else if local == "i" && in_font {
                    current_font_italic = true;
                } else if local == "sz" && in_font {
                    current_font_size = attr_f64(&e, "val").or(current_font_size);
                } else if local == "color" && in_font {
                    current_font_color = xlsx_color_from_element(&e).or(current_font_color);
                } else if local == "fills" {
                    in_fills = true;
                } else if local == "fill" && in_fills {
                    in_fill = true;
                    current_fill_color = None;
                } else if (local == "fgcolor" || local == "bgcolor") && in_fill {
                    current_fill_color = xlsx_color_from_element(&e).or(current_fill_color);
                } else if local == "cellxfs" {
                    in_cell_xfs = true;
                } else if local == "xf" && in_cell_xfs {
                    in_xf = true;
                    current_xf = Some(xlsx_style_from_xf(
                        &e,
                        &custom_formats,
                        &fill_colors,
                        &font_bold,
                        &font_italic,
                        &font_sizes,
                        &font_colors,
                    ));
                } else if local == "alignment" && in_xf {
                    if let Some(style) = current_xf.as_mut() {
                        apply_xlsx_alignment(style, &e);
                    }
                } else if local == "numfmt" {
                    collect_xlsx_custom_number_format(&e, &mut custom_formats);
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "b" && in_font {
                    current_font_bold = true;
                } else if local == "i" && in_font {
                    current_font_italic = true;
                } else if local == "sz" && in_font {
                    current_font_size = attr_f64(&e, "val").or(current_font_size);
                } else if local == "color" && in_font {
                    current_font_color = xlsx_color_from_element(&e).or(current_font_color);
                } else if local == "fill" && in_fills {
                    fill_colors.push(None);
                } else if (local == "fgcolor" || local == "bgcolor") && in_fill {
                    current_fill_color = xlsx_color_from_element(&e).or(current_fill_color);
                } else if local == "xf" && in_cell_xfs {
                    styles.push(xlsx_style_from_xf(
                        &e,
                        &custom_formats,
                        &fill_colors,
                        &font_bold,
                        &font_italic,
                        &font_sizes,
                        &font_colors,
                    ));
                } else if local == "alignment" && in_xf {
                    if let Some(style) = current_xf.as_mut() {
                        apply_xlsx_alignment(style, &e);
                    }
                } else if local == "numfmt" {
                    collect_xlsx_custom_number_format(&e, &mut custom_formats);
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "font" && in_font {
                    font_bold.push(current_font_bold);
                    font_italic.push(current_font_italic);
                    font_sizes.push(current_font_size);
                    font_colors.push(current_font_color.take());
                    in_font = false;
                    current_font_bold = false;
                } else if local == "fonts" {
                    in_fonts = false;
                } else if local == "fill" && in_fill {
                    fill_colors.push(current_fill_color.take());
                    in_fill = false;
                } else if local == "fills" {
                    in_fills = false;
                } else if local == "xf" && in_xf {
                    if let Some(style) = current_xf.take() {
                        styles.push(style);
                    }
                    in_xf = false;
                } else if local == "cellxfs" {
                    in_cell_xfs = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(styles)
}

#[cfg(test)]
fn parse_xlsx_style_number_formats(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<Vec<Option<String>>> {
    Ok(parse_xlsx_styles(context, xml)?
        .into_iter()
        .map(|style| style.number_format)
        .collect())
}

fn collect_xlsx_custom_number_format(e: &BytesStart, formats: &mut BTreeMap<u32, String>) {
    let Some(id) = attr_value(e, "numfmtid").and_then(|value| value.parse::<u32>().ok()) else {
        return;
    };
    let Some(format) = attr_value(e, "formatcode") else {
        return;
    };
    if !format.trim().is_empty() {
        formats.insert(id, format);
    }
}

fn docx_header_footer_entries<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
) -> OfficeResult<Vec<String>> {
    let mut entries = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > 1024 * 1024 {
            continue;
        }
        let normalized = entry.name().replace('\\', "/");
        if is_docx_header_footer_name(&normalized) {
            entries.push(normalized);
        }
    }
    entries.sort();
    entries.truncate(8);
    Ok(entries)
}

fn is_docx_header_footer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(file) = lower.rsplit('/').next() else {
        return false;
    };
    lower.starts_with("word/")
        && lower.ends_with(".xml")
        && (file.starts_with("header") || file.starts_with("footer"))
}

fn extract_docx_header_footer_text<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    entries: &[String],
) -> OfficeResult<String> {
    let mut out = Vec::new();
    for entry in entries.iter().take(8) {
        let Some(xml) = read_office_zip_text(context, zip, entry, 1024 * 1024)? else {
            continue;
        };
        let text = extract_wordprocessing_text(context, &xml)?;
        if !text.trim().is_empty() {
            out.push(format!(
                "- {}: {}",
                file_name(entry),
                normalize_preview_lines(&text)
            ));
        }
    }
    Ok(out.join("\n"))
}

fn xlsx_style_number_format(
    e: &BytesStart,
    custom_formats: &BTreeMap<u32, String>,
) -> Option<String> {
    let id = attr_value(e, "numfmtid").and_then(|value| value.parse::<u32>().ok())?;
    custom_formats
        .get(&id)
        .cloned()
        .or_else(|| xlsx_builtin_number_format(id).map(str::to_string))
}

fn xlsx_style_from_xf(
    e: &BytesStart,
    custom_formats: &BTreeMap<u32, String>,
    fill_colors: &[Option<String>],
    font_bold: &[bool],
    font_italic: &[bool],
    font_sizes: &[Option<f64>],
    font_colors: &[Option<String>],
) -> XlsxStyle {
    let fill_color = attr_value(e, "fillid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| fill_colors.get(id).cloned().flatten());
    let bold = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_bold.get(id).copied())
        .unwrap_or(false);
    let text_color = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_colors.get(id).cloned().flatten());
    let italic = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_italic.get(id).copied())
        .unwrap_or(false);
    let font_size = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_sizes.get(id).copied().flatten());
    XlsxStyle {
        number_format: xlsx_style_number_format(e, custom_formats),
        fill_color,
        text_color,
        bold,
        italic,
        font_size,
        ..Default::default()
    }
}

fn apply_xlsx_alignment(style: &mut XlsxStyle, e: &BytesStart) {
    style.horizontal_alignment = attr_value(e, "horizontal")
        .and_then(|value| normalize_xlsx_horizontal_alignment(&value))
        .or_else(|| style.horizontal_alignment.clone());
    style.vertical_alignment = attr_value(e, "vertical")
        .and_then(|value| normalize_xlsx_vertical_alignment(&value))
        .or_else(|| style.vertical_alignment.clone());
    style.wrap_text = attr_bool(e, "wraptext").unwrap_or(style.wrap_text);
}

fn normalize_xlsx_horizontal_alignment(value: &str) -> Option<String> {
    match value {
        "left" | "center" | "right" | "general" | "fill" | "justify" | "distributed" => {
            Some(value.to_string())
        }
        _ => None,
    }
}

fn normalize_xlsx_vertical_alignment(value: &str) -> Option<String> {
    match value {
        "top" | "center" | "bottom" | "justify" | "distributed" => Some(value.to_string()),
        _ => None,
    }
}

fn xlsx_color_from_element(e: &BytesStart) -> Option<String> {
    attr_value(e, "rgb")
        .and_then(|value| {
            let trimmed = value.trim().trim_start_matches('#');
            let rgb = if trimmed.len() == 8 {
                &trimmed[2..]
            } else {
                trimmed
            };
            normalize_hex_color(rgb)
        })
        .or_else(|| {
            attr_value(e, "indexed")
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(xlsx_indexed_color)
        })
}

fn xlsx_indexed_color(index: u32) -> Option<String> {
    Some(match index {
        0 | 8 => "#000000".to_string(),
        1 | 9 => "#FFFFFF".to_string(),
        2 => "#FF0000".to_string(),
        3 => "#00FF00".to_string(),
        4 => "#0000FF".to_string(),
        5 => "#FFFF00".to_string(),
        6 => "#FF00FF".to_string(),
        7 => "#00FFFF".to_string(),
        22 => "#C0C0C0".to_string(),
        23 => "#808080".to_string(),
        _ => return None,
    })
}

fn xlsx_builtin_number_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => return None,
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0;(#,##0)",
        38 => "#,##0;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

fn parse_xlsx_sheet_images<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    sheet_idx: usize,
    sheet_xml: &str,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let Some(drawing_rid) = parse_worksheet_drawing_rid(context, sheet_xml)? else {
        return Ok(Vec::new());
    };
    let sheet_rels_name = format!("xl/worksheets/_rels/sheet{sheet_idx}.xml.rels");
    let sheet_rels = read_office_zip_text(context, zip, &sheet_rels_name, 2 * 1024 * 1024)?
        .map(|xml| parse_relationships(context, &xml))
        .transpose()?
        .unwrap_or_default();
    let Some(drawing_target) = sheet_rels.get(&drawing_rid) else {
        return Ok(Vec::new());
    };
    let drawing_path = normalize_zip_target("xl/worksheets/", drawing_target);
    let Some(drawing_xml) = read_office_zip_text(context, zip, &drawing_path, 4 * 1024 * 1024)?
    else {
        return Ok(Vec::new());
    };
    let drawing_rels_path = rels_path_for_part(&drawing_path);
    let drawing_rels = read_office_zip_text(context, zip, &drawing_rels_path, 2 * 1024 * 1024)?
        .map(|xml| parse_relationships(context, &xml))
        .transpose()?
        .unwrap_or_default();
    let base = part_base_dir(&drawing_path);
    parse_xlsx_drawing_items(
        context,
        zip,
        &base,
        &drawing_xml,
        &drawing_rels,
        metrics,
        image_budget,
    )
}

fn parse_worksheet_drawing_rid(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<Option<String>> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "drawing" {
                    return Ok(attr_value(&e, "id"));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_xlsx_drawing_items<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    xml: &str,
    rels: &BTreeMap<String, String>,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut in_anchor = false;
    let mut anchor_depth = 0usize;
    let mut marker = "";
    let mut current_tag = "";
    let mut from_col = 0usize;
    let mut from_row = 0usize;
    let mut to_col = 0usize;
    let mut to_row = 0usize;
    let mut ext_w = 0.0;
    let mut ext_h = 0.0;
    let mut rel_id = String::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_anchor && (local == "twocellanchor" || local == "onecellanchor") {
                    in_anchor = true;
                    anchor_depth = 1;
                    marker = "";
                    current_tag = "";
                    from_col = 0;
                    from_row = 0;
                    to_col = 0;
                    to_row = 0;
                    ext_w = 0.0;
                    ext_h = 0.0;
                    rel_id.clear();
                    continue;
                }
                if in_anchor {
                    anchor_depth += 1;
                    if local == "from" || local == "to" {
                        marker = if local == "from" { "from" } else { "to" };
                    } else if matches!(local.as_str(), "col" | "row") {
                        current_tag = if local == "col" { "col" } else { "row" };
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
                    }
                }
            }
            Ok(Event::Empty(e)) if in_anchor => {
                let local = local_xml_name(e.name().as_ref());
                if local == "ext" {
                    ext_w = attr_f64(&e, "cx").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    ext_h = attr_f64(&e, "cy").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                } else if local == "blip" {
                    rel_id = attr_value(&e, "embed").unwrap_or_default();
                }
            }
            Ok(Event::End(e)) if in_anchor => {
                let local = local_xml_name(e.name().as_ref());
                if local == "from" || local == "to" {
                    marker = "";
                } else if local == "col" || local == "row" {
                    current_tag = "";
                }
                anchor_depth = anchor_depth.saturating_sub(1);
                if anchor_depth == 0 {
                    let x = xlsx_col_x(metrics, from_col);
                    let y = xlsx_row_y(metrics, from_row);
                    let width = if to_col > from_col {
                        xlsx_col_span_width(metrics, from_col, to_col)
                    } else {
                        ext_w.max(140.0)
                    };
                    let height = if to_row > from_row {
                        xlsx_row_span_height(metrics, from_row, to_row)
                    } else {
                        ext_h.max(90.0)
                    };
                    if let Some(item) = image_item_from_relationship(
                        context,
                        zip,
                        base_dir,
                        rels,
                        &rel_id,
                        x,
                        y,
                        width,
                        height,
                        items.len(),
                        image_budget,
                    )? {
                        items.push(item);
                    }
                    in_anchor = false;
                }
            }
            Ok(Event::Text(e)) if in_anchor && !marker.is_empty() && !current_tag.is_empty() => {
                let value = xml_unescape_bytes(e.as_ref())
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(0);
                match (marker, current_tag) {
                    ("from", "col") => from_col = value,
                    ("from", "row") => from_row = value,
                    ("to", "col") => to_col = value,
                    ("to", "row") => to_row = value,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(items)
}

fn image_item_from_relationship<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    rels: &BTreeMap<String, String>,
    rel_id: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    z_index: usize,
    image_budget: &mut usize,
) -> OfficeResult<Option<OfficeLayoutItemDto>> {
    if rel_id.is_empty() || *image_budget == 0 || width <= 1.0 || height <= 1.0 {
        return Ok(None);
    }
    let Some(target) = rels.get(rel_id) else {
        return Ok(None);
    };
    let path = normalize_zip_target(base_dir, target);
    let lower = path.to_ascii_lowercase();
    if !is_supported_zip_image_name(&lower) {
        return Ok(None);
    }
    let Some(bytes) = read_office_zip_bytes(context, zip, &path, MAX_OFFICE_INLINE_IMAGE_BYTES)?
    else {
        return Ok(None);
    };
    *image_budget = (*image_budget).saturating_sub(1);
    Ok(Some(OfficeLayoutItemDto {
        kind: "image".to_string(),
        x,
        y,
        width,
        height,
        z_index,
        text: None,
        shape: None,
        placeholder_type: None,
        bold: false,
        italic: false,
        font_size: None,
        fill_color: None,
        stroke_color: None,
        image_name: Some(path.rsplit('/').next().unwrap_or(path.as_str()).to_string()),
        mime_type: image_mime_type(&lower).map(str::to_string),
        image_base64: Some(base64_encode(&bytes)),
    }))
}

fn parse_relationships(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut rels = BTreeMap::new();
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "relationship" {
                    if let (Some(id), Some(target)) =
                        (attr_value(&e, "id"), attr_value(&e, "target"))
                    {
                        rels.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(rels)
}

fn attr_value(e: &BytesStart<'_>, name: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if local_xml_name(attr.key.as_ref()) == name {
            return attr.unescape_value().ok().map(|v| v.to_string());
        }
    }
    None
}

fn attr_f64(e: &BytesStart<'_>, name: &str) -> Option<f64> {
    attr_value(e, name).and_then(|v| v.parse::<f64>().ok())
}

fn attr_bool(e: &BytesStart<'_>, name: &str) -> Option<bool> {
    attr_value(e, name).and_then(|v| match v.as_str() {
        "1" | "true" | "TRUE" | "True" => Some(true),
        "0" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    })
}

fn office_color_from_element(e: &BytesStart<'_>) -> Option<String> {
    if local_xml_name(e.name().as_ref()) == "srgbclr" {
        return attr_value(e, "val").and_then(|value| normalize_hex_color(&value));
    }
    if local_xml_name(e.name().as_ref()) == "schemeclr" {
        return attr_value(e, "val").and_then(|value| match value.as_str() {
            "bg1" | "lt1" => Some("#FFFFFF".to_string()),
            "tx1" | "dk1" => Some("#000000".to_string()),
            "accent1" => Some("#4472C4".to_string()),
            "accent2" => Some("#ED7D31".to_string()),
            "accent3" => Some("#A5A5A5".to_string()),
            "accent4" => Some("#FFC000".to_string()),
            "accent5" => Some("#5B9BD5".to_string()),
            "accent6" => Some("#70AD47".to_string()),
            _ => None,
        });
    }
    None
}

fn normalize_hex_color(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches('#');
    if trimmed.len() != 6 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", trimmed.to_ascii_uppercase()))
}

#[derive(Default)]
struct XlsxSheetMetrics {
    col_widths: BTreeMap<usize, f64>,
    row_heights: BTreeMap<usize, f64>,
}

#[derive(Clone, Copy)]
struct XlsxMergeRegion {
    first_row: usize,
    first_col: usize,
    last_row: usize,
    last_col: usize,
    row_span: usize,
    column_span: usize,
}

fn parse_xlsx_sheet_metrics(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<XlsxSheetMetrics> {
    let mut reader = Reader::from_str(xml);
    let mut metrics = XlsxSheetMetrics::default();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "col" {
                    let Some(min) = attr_value(&e, "min").and_then(|v| v.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    let max = attr_value(&e, "max")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(min);
                    let Some(width) = attr_f64(&e, "width").map(xlsx_column_width_to_dip) else {
                        continue;
                    };
                    for one_based_col in min..=max.min(64) {
                        metrics
                            .col_widths
                            .insert(one_based_col.saturating_sub(1), width);
                    }
                } else if local == "row" {
                    let Some(row) = attr_value(&e, "r").and_then(|v| v.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if let Some(height) = attr_f64(&e, "ht").map(xlsx_row_height_to_dip) {
                        metrics.row_heights.insert(row.saturating_sub(1), height);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(metrics)
}

fn parse_xlsx_freeze_pane(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<(Option<usize>, Option<usize>)> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "pane" {
                    let state = attr_value(&e, "state").unwrap_or_default();
                    if state != "frozen" && state != "frozenSplit" {
                        return Ok((None, None));
                    }
                    let rows = attr_f64(&e, "ysplit").map(|value| value.max(0.0) as usize);
                    let columns = attr_f64(&e, "xsplit").map(|value| value.max(0.0) as usize);
                    return Ok((
                        rows.filter(|value| *value > 0),
                        columns.filter(|value| *value > 0),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok((None, None))
}

fn parse_xlsx_merge_regions(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<BTreeMap<(usize, usize), XlsxMergeRegion>> {
    let mut reader = Reader::from_str(xml);
    let mut regions = BTreeMap::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "mergecell" {
                    let Some(reference) = attr_value(&e, "ref") else {
                        continue;
                    };
                    let Some(region) = parse_xlsx_merge_reference(&reference) else {
                        continue;
                    };
                    if region.first_row < MAX_OFFICE_ROWS && region.first_col < 32 {
                        regions.insert((region.first_row, region.first_col), region);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(regions)
}

fn parse_xlsx_merge_reference(reference: &str) -> Option<XlsxMergeRegion> {
    let (start, end) = reference.split_once(':')?;
    let (first_row, first_col) = cell_reference_position(start)?;
    let (last_row, last_col) = cell_reference_position(end)?;
    let (first_row, last_row) = (first_row.min(last_row), first_row.max(last_row));
    let (first_col, last_col) = (first_col.min(last_col), first_col.max(last_col));
    Some(XlsxMergeRegion {
        first_row,
        first_col,
        last_row,
        last_col,
        row_span: last_row.saturating_sub(first_row) + 1,
        column_span: last_col.saturating_sub(first_col) + 1,
    })
}

fn is_inside_non_origin_merge(
    regions: &BTreeMap<(usize, usize), XlsxMergeRegion>,
    row: usize,
    col: usize,
) -> bool {
    regions.values().any(|region| {
        (row != region.first_row || col != region.first_col)
            && row >= region.first_row
            && row <= region.last_row
            && col >= region.first_col
            && col <= region.last_col
    })
}

fn xlsx_column_width_to_dip(width: f64) -> f64 {
    (width * 7.0 + 12.0).clamp(36.0, 260.0)
}

fn xlsx_row_height_to_dip(height_points: f64) -> f64 {
    (height_points * 96.0 / 72.0).clamp(18.0, 120.0)
}

fn xlsx_col_width(metrics: &XlsxSheetMetrics, col: usize) -> f64 {
    metrics
        .col_widths
        .get(&col)
        .copied()
        .unwrap_or(XLSX_CELL_WIDTH)
}

fn xlsx_row_height(metrics: &XlsxSheetMetrics, row: usize) -> f64 {
    metrics
        .row_heights
        .get(&row)
        .copied()
        .unwrap_or(XLSX_ROW_HEIGHT)
}

fn xlsx_col_x(metrics: &XlsxSheetMetrics, col: usize) -> f64 {
    (0..col.min(64))
        .map(|idx| xlsx_col_width(metrics, idx))
        .sum()
}

fn xlsx_row_y(metrics: &XlsxSheetMetrics, row: usize) -> f64 {
    (0..row.min(MAX_OFFICE_ROWS))
        .map(|idx| xlsx_row_height(metrics, idx))
        .sum()
}

fn xlsx_col_span_width(metrics: &XlsxSheetMetrics, from_col: usize, to_col: usize) -> f64 {
    (from_col..to_col.min(64))
        .map(|idx| xlsx_col_width(metrics, idx))
        .sum::<f64>()
        .max(24.0)
}

fn xlsx_row_span_height(metrics: &XlsxSheetMetrics, from_row: usize, to_row: usize) -> f64 {
    (from_row..to_row.min(MAX_OFFICE_ROWS))
        .map(|idx| xlsx_row_height(metrics, idx))
        .sum::<f64>()
        .max(18.0)
}

fn xlsx_page_size(cells: &[OfficeCellDto], items: &[OfficeLayoutItemDto]) -> (f64, f64) {
    let cell_width = cells
        .iter()
        .map(|cell| cell.x + cell.width)
        .fold(0.0, f64::max);
    let cell_height = cells
        .iter()
        .map(|cell| cell.y + cell.height)
        .fold(0.0, f64::max);
    let item_width = items
        .iter()
        .map(|item| item.x + item.width)
        .fold(0.0, f64::max);
    let item_height = items
        .iter()
        .map(|item| item.y + item.height)
        .fold(0.0, f64::max);
    (
        cell_width.max(item_width).max(480.0) + 24.0,
        cell_height.max(item_height).max(260.0) + 24.0,
    )
}

fn rels_path_for_part(part_path: &str) -> String {
    let normalized = part_path.replace('\\', "/");
    let (dir, name) = match normalized.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name.to_string()),
        None => (String::new(), normalized),
    };
    format!("{dir}_rels/{name}.rels")
}

fn part_base_dir(part_path: &str) -> String {
    let normalized = part_path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default()
}

fn normalize_zip_target(base_dir: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("{base_dir}{target}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

fn image_mime_type(lower: &str) -> Option<&'static str> {
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".bmp") {
        Some("image/bmp")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".ico") {
        Some("image/x-icon")
    } else {
        None
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        out.push(TABLE[(n & 0x3F) as usize] as char);
        i += 3;
    }
    match bytes.len() - i {
        1 => {
            let n = (bytes[i] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

fn extract_wordprocessing_text(context: &OfficeContext, xml: &str) -> OfficeResult<String> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;
    let mut paragraph_prefix = String::new();
    let mut in_table = false;
    let mut in_row = false;
    let mut in_cell = false;
    let mut cell_text = String::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    if !paragraph_prefix.is_empty() && !paragraph_had_text {
                        if in_cell {
                            cell_text.push_str(&paragraph_prefix);
                        } else {
                            out.push_str(&paragraph_prefix);
                        }
                    }
                    in_text = true;
                } else if local == "tbl" {
                    in_table = true;
                } else if local == "tr" && in_table {
                    in_row = true;
                    row_cells.clear();
                } else if local == "tc" && in_row {
                    in_cell = true;
                    cell_text.clear();
                    paragraph_had_text = false;
                } else if local == "pstyle" {
                    paragraph_prefix = docx_paragraph_prefix(&e);
                } else if local == "numpr" {
                    paragraph_prefix = docx_numbered_paragraph_prefix(&paragraph_prefix);
                } else if local == "sectpr" && !in_cell {
                    append_docx_block_marker(&mut out, "[section break]");
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" {
                    if paragraph_had_text {
                        if in_cell {
                            if !cell_text.ends_with(' ') {
                                cell_text.push(' ');
                            }
                        } else if !out.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                    paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "tc" && in_cell {
                    row_cells.push(normalize_preview_lines(&cell_text).replace('\n', " "));
                    cell_text.clear();
                    in_cell = false;
                    paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "tr" && in_row {
                    if !row_cells.iter().all(|cell| cell.trim().is_empty()) {
                        out.push_str("| ");
                        out.push_str(&row_cells.join(" | "));
                        out.push_str(" |\n");
                    }
                    row_cells.clear();
                    in_row = false;
                } else if local == "tbl" {
                    in_table = false;
                } else if local == "tab" {
                    if in_cell {
                        cell_text.push('\t');
                    } else {
                        out.push('\t');
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "tab" {
                    if in_cell {
                        cell_text.push('\t');
                    } else {
                        out.push('\t');
                    }
                    paragraph_had_text = true;
                } else if local == "br" {
                    if in_cell {
                        cell_text.push(' ');
                    } else if attr_value(&e, "type").as_deref() == Some("page") {
                        append_docx_block_marker(&mut out, "[page break]");
                    } else {
                        out.push('\n');
                    }
                    paragraph_had_text = false;
                } else if local == "pstyle" {
                    paragraph_prefix = docx_paragraph_prefix(&e);
                } else if local == "numpr" {
                    paragraph_prefix = docx_numbered_paragraph_prefix(&paragraph_prefix);
                } else if local == "sectpr" && !in_cell {
                    append_docx_block_marker(&mut out, "[section break]");
                }
            }
            Ok(Event::Text(e)) if in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    if in_cell {
                        cell_text.push_str(&value);
                    } else {
                        out.push_str(&value);
                    }
                    paragraph_had_text = true;
                }
            }
            Ok(Event::CData(e)) if in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    if in_cell {
                        cell_text.push_str(&value);
                    } else {
                        out.push_str(&value);
                    }
                    paragraph_had_text = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(normalize_preview_lines(&out))
}

fn append_docx_block_marker(out: &mut String, marker: &str) {
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out.push_str(marker);
    out.push('\n');
}

fn docx_paragraph_prefix(e: &BytesStart<'_>) -> String {
    let Some(style) = attr_value(e, "val") else {
        return String::new();
    };
    let lower = style.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("heading") {
        let level = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .unwrap_or(1)
            .clamp(1, 6);
        return format!("{} ", "#".repeat(level));
    }
    if lower == "title" {
        return "# ".to_string();
    }
    String::new()
}

fn docx_numbered_paragraph_prefix(current: &str) -> String {
    if current.trim().is_empty() {
        "- ".to_string()
    } else {
        current.to_string()
    }
}

fn parse_shared_strings(context: &OfficeContext, xml: &str) -> OfficeResult<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "si" {
                    in_si = true;
                    current.clear();
                } else if in_si && local == "t" {
                    in_t = true;
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_t = false;
                } else if local == "si" {
                    values.push(current.clone());
                    in_si = false;
                }
            }
            Ok(Event::Text(e)) if in_t => current.push_str(&xml_unescape_bytes(e.as_ref())),
            Ok(Event::CData(e)) if in_t => current.push_str(&String::from_utf8_lossy(e.as_ref())),
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(values)
}

fn parse_worksheet_rows(
    context: &OfficeContext,
    xml: &str,
    shared_strings: &[String],
) -> OfficeResult<Vec<Vec<String>>> {
    let mut reader = Reader::from_str(xml);
    let mut rows = Vec::new();
    let mut row = Vec::<String>::new();
    let mut in_row = false;
    let mut in_cell = false;
    let mut in_value = false;
    let mut in_inline_text = false;
    let mut cell_type = String::new();
    let mut cell_value = String::new();
    let mut cell_col: Option<usize> = None;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "row" => {
                        in_row = true;
                        row.clear();
                    }
                    "c" if in_row => {
                        in_cell = true;
                        cell_type.clear();
                        cell_value.clear();
                        cell_col = None;
                        for attr in e.attributes().flatten() {
                            let key = local_xml_name(attr.key.as_ref());
                            if key == "t" {
                                cell_type = attr
                                    .unescape_value()
                                    .ok()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                            } else if key == "r" {
                                let reference = attr
                                    .unescape_value()
                                    .ok()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                cell_col = cell_reference_column(&reference);
                            }
                        }
                    }
                    "v" if in_cell => in_value = true,
                    "t" if in_cell => in_inline_text = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "v" => in_value = false,
                    "t" => in_inline_text = false,
                    "c" if in_cell => {
                        let value = resolve_cell_value(&cell_value, &cell_type, shared_strings);
                        if let Some(col) = cell_col {
                            while row.len() < col {
                                row.push(String::new());
                            }
                            if row.len() == col {
                                row.push(value);
                            } else {
                                row[col] = value;
                            }
                        } else {
                            row.push(value);
                        }
                        in_cell = false;
                    }
                    "row" if in_row => {
                        while row
                            .last()
                            .map(|cell| cell.trim().is_empty())
                            .unwrap_or(false)
                        {
                            row.pop();
                        }
                        if row.iter().any(|cell| !cell.trim().is_empty()) {
                            rows.push(row.clone());
                            if rows.len() >= MAX_OFFICE_ROWS {
                                break;
                            }
                        }
                        in_row = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_unescape_bytes(e.as_ref()))
            }
            Ok(Event::CData(e)) if in_value || in_inline_text => {
                cell_value.push_str(&String::from_utf8_lossy(e.as_ref()))
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(rows)
}

fn cell_reference_column(reference: &str) -> Option<usize> {
    let mut col = 0usize;
    let mut saw_letter = false;
    for ch in reference.chars() {
        if !ch.is_ascii_alphabetic() {
            break;
        }
        saw_letter = true;
        col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    saw_letter.then_some(col.saturating_sub(1))
}

fn cell_reference_position(reference: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_letter = false;
    let mut saw_digit = false;
    for ch in reference.chars() {
        if ch.is_ascii_alphabetic() {
            if saw_digit {
                return None;
            }
            saw_letter = true;
            col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else if ch.is_ascii_digit() {
            saw_digit = true;
            row = row * 10 + (ch as usize - '0' as usize);
        } else if ch == '$' {
            continue;
        } else {
            break;
        }
    }
    (saw_letter && saw_digit && row > 0 && col > 0).then_some((row - 1, col - 1))
}

fn format_table_rows(rows: &[Vec<String>]) -> Vec<String> {
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return Vec::new();
    }

    let mut widths = vec![3usize; col_count];
    for row in rows {
        for i in 0..col_count {
            let value = row
                .get(i)
                .map(|cell| clean_table_cell(cell))
                .unwrap_or_default();
            let len = value.chars().count().min(MAX_OFFICE_TABLE_CELL_WIDTH);
            widths[i] = widths[i].max(len);
        }
    }

    rows.iter()
        .map(|row| {
            let mut parts = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let value = row
                    .get(i)
                    .map(|cell| clean_table_cell(cell))
                    .unwrap_or_default();
                let cell = truncate_table_cell(&value, widths[i]);
                if i + 1 == col_count {
                    parts.push(cell);
                } else {
                    parts.push(format!("{cell:<width$}", width = widths[i]));
                }
            }
            parts.join("  ").trim_end().to_string()
        })
        .collect()
}

fn clean_table_cell(cell: &str) -> String {
    cell.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_table_cell(cell: &str, width: usize) -> String {
    if cell.chars().count() <= width {
        return cell.to_string();
    }

    let keep = width.saturating_sub(3).max(1);
    let mut out = cell.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn resolve_cell_value(raw: &str, cell_type: &str, shared_strings: &[String]) -> String {
    if cell_type == "s" {
        raw.trim()
            .parse::<usize>()
            .ok()
            .and_then(|i| shared_strings.get(i).cloned())
            .unwrap_or_default()
    } else {
        raw.trim().to_string()
    }
}

fn office_text_json(path: &str, kind_label: &str, text: String) -> String {
    office_text_json_with_format(path, kind_label, text, "plain", "text")
}

fn office_text_json_with_format(
    path: &str,
    kind_label: &str,
    text: String,
    format: &str,
    language: &str,
) -> String {
    office_preview_json_with_layout(path, kind_label, text, format, language, None)
}

fn office_preview_json_with_layout(
    path: &str,
    kind_label: &str,
    text: String,
    format: &str,
    language: &str,
    office_layout: Option<OfficeLayoutDto>,
) -> String {
    let filename = file_name(path);
    to_json(&PreviewReadyDto {
        kind: "office".to_string(),
        title: format!("{filename} - {kind_label}"),
        format: Some(format.to_string()),
        language: Some(language.to_string()),
        text: Some(text),
        office_layout,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn office_error_json(path: &str, kind_label: &str, message: &str) -> String {
    let filename = file_name(path);
    office_text_json(
        path,
        kind_label,
        format!("Name: {filename}\nKind: {kind_label}\nStatus: {message}"),
    )
}

fn truncate_preview_text(text: &str) -> String {
    let Some((end, _)) = text.char_indices().nth(MAX_OFFICE_TEXT_CHARS) else {
        return text.to_string();
    };
    format!(
        "{}\n\n[Preview truncated at {} characters]",
        &text[..end],
        MAX_OFFICE_TEXT_CHARS
    )
}

fn normalize_preview_lines(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn local_xml_name(bytes: &[u8]) -> String {
    let name = std::str::from_utf8(bytes).unwrap_or("");
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

fn xml_unescape_bytes(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    xml_unescape_str(&s)
}

fn xml_unescape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let entity_start = amp + 1;
        let Some(semi_rel) = rest[entity_start..].find(';') else {
            out.push_str(&rest[amp..]);
            return out;
        };

        let entity_end = entity_start + semi_rel;
        let entity = &rest[entity_start..entity_end];
        if let Some(ch) = decode_xml_entity(entity) {
            out.push(ch);
        } else {
            out.push('&');
            out.push_str(entity);
            out.push(';');
        }
        rest = &rest[(entity_end + 1)..];
    }
    out.push_str(rest);
    out
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => decode_numeric_xml_entity(entity),
    }
}

fn decode_numeric_xml_entity(entity: &str) -> Option<char> {
    let digits = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"));
    let value = if let Some(hex) = digits {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        let dec = entity.strip_prefix('#')?;
        dec.parse::<u32>().ok()?
    };
    char::from_u32(value)
}

fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

// ── Info preview ─────────────────────────────────────────────────────────────

/// Produce JSON for an info-only preview: `{"kind":"...","title":"... - N bytes · date"}`.
pub fn render_info(path: &str, kind: &str, size: i64, modified_unix: i64) -> String {
    match kind {
        "font" => return render_font_info(path, size, modified_unix),
        "database" => return render_database_info(path, size, modified_unix),
        "mail" => return render_mail_info(path, size, modified_unix),
        "chm" => return render_chm_info(path, size, modified_unix),
        "dump" => return render_dump_info(path, size, modified_unix),
        "elf" => return render_elf_info(path, size, modified_unix),
        "video" | "audio" | "media" => return render_media_info(path, kind, size, modified_unix),
        _ => {}
    }
    generic_info_json(path, kind, size, modified_unix, None)
}

fn generic_info_json(
    path: &str,
    kind: &str,
    size: i64,
    modified_unix: i64,
    body: Option<String>,
) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let summary = format!(
        "{} bytes · {}",
        format_number(size),
        format_timestamp(modified_unix)
    );
    to_json(&PreviewReadyDto {
        kind: kind.to_string(),
        title: format!("{filename} — {summary}"),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(body.unwrap_or_else(|| {
            format!(
                "Name: {filename}\nKind: {kind}\nSize: {}\nModified: {}",
                format_number(size),
                format_timestamp(modified_unix)
            )
        })),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn render_font_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_INFO_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "font", size, modified_unix);
    if let Some(summary) = parse_font_summary(&bytes) {
        text.push_str(&format!("\nFormat: {}", summary.format));
        if summary.faces > 0 {
            text.push_str(&format!("\nFaces: {}", summary.faces));
        }
        if summary.tables > 0 {
            text.push_str(&format!("\nTables: {}", summary.tables));
        }
        if summary.glyphs > 0 {
            text.push_str(&format!(
                "\nGlyphs: {}",
                format_number(summary.glyphs as i64)
            ));
        }
        if summary.sfnt_size > 0 {
            text.push_str(&format!(
                "\nDecoded sfnt size: {}",
                format_bytes(summary.sfnt_size as i64)
            ));
        }
        if summary.compressed_size > 0 {
            text.push_str(&format!(
                "\nCompressed data size: {}",
                format_bytes(summary.compressed_size as i64)
            ));
        }
        if summary.metadata_size > 0 {
            text.push_str(&format!(
                "\nMetadata block: {}",
                format_bytes(summary.metadata_size as i64)
            ));
        }
        if !summary.family.is_empty() {
            text.push_str(&format!("\nFamily: {}", summary.family));
        }
        if !summary.subfamily.is_empty() {
            text.push_str(&format!("\nStyle: {}", summary.subfamily));
        }
        if !summary.full_name.is_empty() {
            text.push_str(&format!("\nFull name: {}", summary.full_name));
        }
        if !summary.postscript_name.is_empty() {
            text.push_str(&format!("\nPostScript: {}", summary.postscript_name));
        }
        if !summary.version.is_empty() {
            text.push_str(&format!("\nVersion: {}", summary.version));
        }
        if !summary.license.is_empty() {
            text.push_str(&format!("\nLicense: {}", summary.license));
        }
        if !summary.license_url.is_empty() {
            text.push_str(&format!("\nLicense URL: {}", summary.license_url));
        }
    }
    generic_info_json(path, "font", size, modified_unix, Some(text))
}

fn render_database_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_INFO_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "database", size, modified_unix);
    if bytes.starts_with(b"SQLite format 3\0") {
        let page_size = read_u16_be(&bytes, 16)
            .map(|value| if value == 1 { 65536 } else { value as u32 })
            .unwrap_or(0);
        text.push_str("\nFormat: SQLite 3");
        text.push_str(&format!("\nPage size: {} bytes", page_size));
        if let Some(pages) = read_u32_be(&bytes, 28) {
            text.push_str(&format!("\nPages: {}", format_number(pages as i64)));
            if page_size > 0 {
                text.push_str(&format!(
                    "\nDatabase size from header: {}",
                    format_bytes(pages as i64 * page_size as i64)
                ));
            }
        }
        if let Some(encoding) = read_u32_be(&bytes, 56) {
            text.push_str(&format!(
                "\nText encoding: {}",
                sqlite_encoding_name(encoding)
            ));
        }
        if let Some(user_version) = read_u32_be(&bytes, 60) {
            text.push_str(&format!("\nUser version: {}", user_version));
        }
        if let Some(app_id) = read_u32_be(&bytes, 68) {
            text.push_str(&format!("\nApplication ID: 0x{app_id:08X}"));
        }
        append_sqlite_header_details(&mut text, &bytes);
        append_sqlite_schema_summary(&mut text, &bytes, page_size as usize);
    } else if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        text.push_str("\nFormat: Microsoft Compound File database");
    } else {
        text.push_str("\nFormat: database file");
    }
    generic_info_json(path, "database", size, modified_unix, Some(text))
}

fn render_mail_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_MAIL_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "mail", size, modified_unix);
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        text.push_str("\nFormat: Outlook MSG / Compound File");
        append_msg_compound_summary(&mut text, &bytes);
    } else {
        let content = String::from_utf8_lossy(&bytes);
        let headers = parse_mail_headers(&content);
        text.push_str("\nFormat: RFC 5322 message");
        for key in [
            "From",
            "To",
            "Cc",
            "Reply-To",
            "Subject",
            "Date",
            "Message-ID",
            "MIME-Version",
            "Content-Type",
        ] {
            if let Some(value) = headers
                .iter()
                .find_map(|(k, v)| k.eq_ignore_ascii_case(key).then_some(v))
            {
                text.push_str(&format!(
                    "\n{key}: {}",
                    decode_mail_header_value(value).trim()
                ));
            }
        }
        if let Some(content_type) = header_value(&headers, "Content-Type") {
            if let Some(boundary) = mail_header_parameter(content_type, "boundary") {
                text.push_str(&format!("\nMIME boundary: {boundary}"));
                let parts = mail_mime_part_summaries(&content, &boundary);
                if !parts.is_empty() {
                    text.push_str(&format!("\nMIME parts: {}", parts.len()));
                    text.push_str(&format!("\nMIME part details: {}", parts.join("; ")));
                }
            }
        }
        let attachments = content
            .lines()
            .filter(|line| {
                line.to_ascii_lowercase()
                    .contains("content-disposition: attachment")
            })
            .count();
        if attachments > 0 {
            text.push_str(&format!(
                "\nAttachments observed: {}",
                format_number(attachments as i64)
            ));
            let filenames = mail_attachment_filenames(&content);
            if !filenames.is_empty() {
                text.push_str(&format!("\nAttachment names: {}", filenames.join(", ")));
            }
        }
        if filename.to_ascii_lowercase().ends_with(".mbox") {
            let count = content
                .lines()
                .filter(|line| line.starts_with("From "))
                .count();
            text.push_str(&format!(
                "\nMailbox messages observed: {}",
                format_number(count as i64)
            ));
        }
    }
    generic_info_json(path, "mail", size, modified_unix, Some(text))
}

fn render_chm_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, 8192).unwrap_or_default();
    let mut text = base_info_text(filename, "chm", size, modified_unix);
    if bytes.starts_with(b"ITSF") {
        text.push_str("\nFormat: Microsoft Compiled HTML Help");
        if let Some(version) = read_u32(&bytes, 4) {
            text.push_str(&format!("\nITSF version: {}", version));
        }
        if let Some(header_len) = read_u32(&bytes, 8) {
            text.push_str(&format!("\nHeader length: {} bytes", header_len));
        }
        if let Some(lang_id) = read_u32(&bytes, 20) {
            text.push_str(&format!("\nLanguage ID: 0x{lang_id:08X}"));
        }
        if let Some(timestamp) = read_u32(&bytes, 24).filter(|value| *value > 0) {
            text.push_str(&format!(
                "\nTimestamp: {}",
                format_timestamp(timestamp as i64)
            ));
        }
        if let Some(dir_offset) = read_u64(&bytes, 40).filter(|value| *value > 0) {
            text.push_str(&format!("\nDirectory offset: 0x{dir_offset:016X}"));
        }
        if let Some(dir_len) = read_u64(&bytes, 48).filter(|value| *value > 0) {
            text.push_str(&format!(
                "\nDirectory length: {}",
                format_bytes(dir_len as i64)
            ));
        }
        append_chm_itsp_summary(&mut text, &bytes);
    } else {
        text.push_str("\nFormat: CHM-like help file");
    }
    generic_info_json(path, "chm", size, modified_unix, Some(text))
}

fn append_chm_itsp_summary(text: &mut String, bytes: &[u8]) {
    let Some(dir_offset) = read_u64(bytes, 40)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
    else {
        return;
    };
    if dir_offset + 56 > bytes.len() || bytes.get(dir_offset..dir_offset + 4) != Some(b"ITSP") {
        return;
    }
    let version = read_u32(bytes, dir_offset + 4).unwrap_or(0);
    let header_len = read_u32(bytes, dir_offset + 8).unwrap_or(0);
    let block_len = read_u32(bytes, dir_offset + 16).unwrap_or(0);
    let index_depth = read_u32(bytes, dir_offset + 24).unwrap_or(0);
    let index_root = read_u32(bytes, dir_offset + 28).unwrap_or(0);
    let index_head = read_u32(bytes, dir_offset + 32).unwrap_or(0);
    let block_count = read_u32(bytes, dir_offset + 40).unwrap_or(0);
    text.push_str(&format!(
        "\nITSP version: {version}\nITSP header length: {header_len} bytes\nDirectory block length: {block_len} bytes\nDirectory block count: {block_count}\nDirectory index depth/root/head: {index_depth}/{index_root}/{index_head}"
    ));
    let entries = chm_directory_entries(bytes, dir_offset, header_len as usize, block_len as usize);
    if !entries.is_empty() {
        text.push_str(&format!(
            "\nDirectory entries: {}",
            entries
                .iter()
                .map(ChmDirectoryEntry::summary)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let compressed = chm_compressed_stream_summary(&entries);
        if !compressed.is_empty() {
            text.push_str(&format!("\nCompressed streams: {}", compressed.join(", ")));
        }
        for (label, value) in chm_system_summary(bytes, &entries) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
}

struct ChmDirectoryEntry {
    name: String,
    section: usize,
    offset: usize,
    len: usize,
}

impl ChmDirectoryEntry {
    fn summary(&self) -> String {
        format!(
            "{} [section {}, offset {}, {}]",
            self.name,
            self.section,
            self.offset,
            format_bytes(self.len as i64)
        )
    }
}

fn chm_directory_entries(
    bytes: &[u8],
    dir_offset: usize,
    header_len: usize,
    block_len: usize,
) -> Vec<ChmDirectoryEntry> {
    if header_len == 0 || block_len < 32 {
        return Vec::new();
    }
    let block_offset = dir_offset.saturating_add(header_len);
    if block_offset.saturating_add(block_len) > bytes.len()
        || bytes.get(block_offset..block_offset + 4) != Some(b"PMGL")
    {
        return Vec::new();
    }
    let free_space = read_u32(bytes, block_offset + 4).unwrap_or(0) as usize;
    let entries_end = block_offset
        .saturating_add(block_len)
        .saturating_sub(free_space.min(block_len));
    let mut offset = block_offset + 20;
    let mut entries = Vec::new();
    while offset < entries_end && entries.len() < 12 {
        let Some((name_len, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        if name_len == 0 || name_len > 260 || offset.saturating_add(name_len) > entries_end {
            break;
        }
        let name = String::from_utf8_lossy(&bytes[offset..offset + name_len]).to_string();
        offset += name_len;
        let Some((section, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        let Some((file_offset, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        let Some((file_len, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        if !name.is_empty() {
            entries.push(ChmDirectoryEntry {
                name,
                section,
                offset: file_offset,
                len: file_len,
            });
        }
    }
    entries
}

fn chm_compressed_stream_summary(entries: &[ChmDirectoryEntry]) -> Vec<String> {
    let mut summary = Vec::new();
    for entry in entries.iter().take(32) {
        let lower = entry.name.to_ascii_lowercase();
        if lower.contains("::dataspace/storage/") || lower.contains("::dataspace/namelist") {
            summary.push(format!(
                "{} ({})",
                entry.name,
                format_bytes(entry.len as i64)
            ));
        } else if lower.ends_with("/content") && lower.contains("mscompressed") {
            summary.push(format!(
                "compressed content {}",
                format_bytes(entry.len as i64)
            ));
        }
        if summary.len() >= 8 {
            break;
        }
    }
    summary
}

fn chm_system_summary(bytes: &[u8], entries: &[ChmDirectoryEntry]) -> Vec<(&'static str, String)> {
    let Some(system) = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("/#SYSTEM") && entry.section == 0)
    else {
        return Vec::new();
    };
    if system.len == 0
        || system.len > 4096
        || system.offset.saturating_add(system.len) > bytes.len()
    {
        return Vec::new();
    }
    let data = &bytes[system.offset..system.offset + system.len];
    let mut offset = 0usize;
    let mut values = Vec::new();
    while offset + 4 <= data.len() && values.len() < 8 {
        let code = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;
        if len == 0 || offset.saturating_add(len) > data.len() {
            break;
        }
        let value = String::from_utf8_lossy(&data[offset..offset + len])
            .trim_matches('\0')
            .trim()
            .to_string();
        match code {
            2 if !value.is_empty() => values.push(("Default topic", value)),
            3 if !value.is_empty() => values.push(("Title", value)),
            _ => {}
        }
        offset += len;
    }
    values
}

fn read_chm_encint(bytes: &[u8], offset: usize, limit: usize) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut current = offset;
    for _ in 0..8 {
        let byte = *bytes.get(current).filter(|_| current < limit)?;
        current += 1;
        value = value.checked_shl(7)?.checked_add((byte & 0x7F) as usize)?;
        if byte & 0x80 == 0 {
            return Some((value, current));
        }
    }
    None
}

fn render_dump_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, 512).unwrap_or_default();
    let mut text = base_info_text(filename, "dump", size, modified_unix);
    if bytes.starts_with(b"MDMP") {
        text.push_str("\nFormat: Windows minidump");
        if let Some(version) = read_u32(&bytes, 4) {
            text.push_str(&format!("\nVersion: 0x{version:08X}"));
        }
        if let Some(streams) = read_u32(&bytes, 8) {
            text.push_str(&format!("\nStreams: {}", streams));
        }
        if let Some(directory_rva) = read_u32(&bytes, 12) {
            text.push_str(&format!("\nDirectory RVA: 0x{directory_rva:08X}"));
        }
        if let Some(timestamp) = read_u32(&bytes, 20) {
            text.push_str(&format!(
                "\nTimestamp: {}",
                format_timestamp(timestamp as i64)
            ));
        }
        if let Some(flags) = read_u64(&bytes, 24) {
            text.push_str(&format!("\nFlags: 0x{flags:016X}"));
        }
        append_minidump_streams(&mut text, &bytes);
    } else if bytes.starts_with(&[0x7F, b'E', b'L', b'F']) {
        text.push_str("\nFormat: ELF core/dump");
        append_elf_summary(&mut text, &bytes);
    } else {
        text.push_str("\nFormat: memory dump");
    }
    generic_info_json(path, "dump", size, modified_unix, Some(text))
}

fn render_elf_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, 512).unwrap_or_default();
    let mut text = base_info_text(filename, "elf", size, modified_unix);
    append_elf_summary(&mut text, &bytes);
    generic_info_json(path, "elf", size, modified_unix, Some(text))
}

fn render_media_info(path: &str, kind: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_INFO_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, kind, size, modified_unix);
    text.push_str(&format!(
        "\nContainer: {}",
        media_container_name(path, &bytes)
    ));
    let mp4 = mp4_summary(&bytes);
    if let Some(brand) = mp4.as_ref().and_then(|summary| summary.brand.as_deref()) {
        text.push_str(&format!("\nBrand: {brand}"));
    }
    if let Some(duration) = mp4.as_ref().and_then(|summary| summary.duration_seconds) {
        text.push_str(&format!("\nDuration: {}", format_duration(duration)));
        if let Some(bitrate) = estimate_bitrate(size, duration) {
            text.push_str(&format!("\nBitrate: {}", format_bitrate(bitrate)));
        }
    }
    if let Some(created_unix) = mp4.as_ref().and_then(|summary| summary.created_unix) {
        text.push_str(&format!("\nCreated: {}", format_timestamp(created_unix)));
    }
    if let Some(rotation) = mp4.as_ref().and_then(|summary| summary.rotation_degrees) {
        text.push_str(&format!("\nRotation: {}", format_rotation(rotation)));
    }
    if let Some(summary) = mp4.as_ref() {
        append_mp4_tracks(&mut text, &summary.tracks);
    }
    append_mkv_metadata(&mut text, &bytes);
    append_wav_metadata(&mut text, &bytes);
    append_flac_metadata(&mut text, &bytes);
    append_ogg_metadata(&mut text, &bytes);
    append_id3_metadata(&mut text, &bytes);
    generic_info_json(path, kind, size, modified_unix, Some(text))
}

fn base_info_text(filename: &str, kind: &str, size: i64, modified_unix: i64) -> String {
    format!(
        "Name: {filename}\nKind: {kind}\nSize: {}\nModified: {}",
        format_number(size),
        format_timestamp(modified_unix)
    )
}

#[derive(Default)]
struct FontSummary {
    format: &'static str,
    faces: u32,
    tables: u16,
    glyphs: u16,
    sfnt_size: u32,
    compressed_size: u32,
    metadata_size: u32,
    family: String,
    subfamily: String,
    full_name: String,
    postscript_name: String,
    version: String,
    license: String,
    license_url: String,
}

fn parse_font_summary(bytes: &[u8]) -> Option<FontSummary> {
    if bytes.starts_with(b"ttcf") {
        return Some(FontSummary {
            format: "TrueType Collection",
            faces: read_u32_be(bytes, 8).unwrap_or(0),
            ..Default::default()
        });
    }
    if bytes.starts_with(b"wOFF") {
        return Some(FontSummary {
            format: "WOFF font",
            tables: read_u16_be(bytes, 12).unwrap_or(0),
            sfnt_size: read_u32_be(bytes, 16).unwrap_or(0),
            metadata_size: read_u32_be(bytes, 28).unwrap_or(0),
            ..Default::default()
        });
    }
    if bytes.starts_with(b"wOF2") {
        return Some(FontSummary {
            format: "WOFF2 font",
            tables: read_u16_be(bytes, 12).unwrap_or(0),
            sfnt_size: read_u32_be(bytes, 16).unwrap_or(0),
            compressed_size: read_u32_be(bytes, 20).unwrap_or(0),
            metadata_size: read_u32_be(bytes, 32).unwrap_or(0),
            ..Default::default()
        });
    }

    let format = if bytes.starts_with(&[0, 1, 0, 0]) {
        "TrueType font"
    } else if bytes.starts_with(b"OTTO") {
        "OpenType/CFF font"
    } else {
        return None;
    };
    let tables = read_u16_be(bytes, 4)?;
    let mut summary = FontSummary {
        format,
        faces: 1,
        tables,
        ..Default::default()
    };
    if let Some((offset, length)) = find_sfnt_table(bytes, "name", tables) {
        parse_font_name_table(bytes, offset, length, &mut summary);
    }
    if let Some((offset, length)) = find_sfnt_table(bytes, "maxp", tables) {
        summary.glyphs = parse_font_maxp_glyph_count(bytes, offset, length).unwrap_or(0);
    }
    Some(summary)
}

fn find_sfnt_table(bytes: &[u8], tag: &str, tables: u16) -> Option<(usize, usize)> {
    let table_count = tables.min(256) as usize;
    for index in 0..table_count {
        let record = 12 + index * 16;
        let record_end = record.checked_add(16)?;
        let tag_bytes = bytes.get(record..record + 4)?;
        if record_end > bytes.len() || tag_bytes != tag.as_bytes() {
            continue;
        }
        let offset = read_u32_be(bytes, record + 8)? as usize;
        let length = read_u32_be(bytes, record + 12)? as usize;
        if offset.checked_add(length)? <= bytes.len() {
            return Some((offset, length));
        }
    }
    None
}

fn parse_font_name_table(bytes: &[u8], offset: usize, length: usize, summary: &mut FontSummary) {
    let end = offset.saturating_add(length).min(bytes.len());
    if offset + 6 > end {
        return;
    }
    let count = read_u16_be(bytes, offset + 2).unwrap_or(0).min(256) as usize;
    let storage = offset + read_u16_be(bytes, offset + 4).unwrap_or(0) as usize;
    for index in 0..count {
        let record = offset + 6 + index * 12;
        if record + 12 > end {
            break;
        }
        let platform = read_u16_be(bytes, record).unwrap_or(0);
        let name_id = read_u16_be(bytes, record + 6).unwrap_or(0);
        let len = read_u16_be(bytes, record + 8).unwrap_or(0) as usize;
        let off = read_u16_be(bytes, record + 10).unwrap_or(0) as usize;
        let value_start = storage.saturating_add(off);
        let value_end = value_start.saturating_add(len);
        let Some(raw) = bytes.get(value_start..value_end) else {
            continue;
        };
        let value = decode_font_name(platform, raw);
        if value.is_empty() {
            continue;
        }
        match name_id {
            1 if summary.family.is_empty() => summary.family = value,
            2 if summary.subfamily.is_empty() => summary.subfamily = value,
            4 if summary.full_name.is_empty() => summary.full_name = value,
            6 if summary.postscript_name.is_empty() => summary.postscript_name = value,
            5 if summary.version.is_empty() => summary.version = value,
            13 if summary.license.is_empty() => summary.license = value,
            14 if summary.license_url.is_empty() => summary.license_url = value,
            _ => {}
        }
    }
}

fn parse_font_maxp_glyph_count(bytes: &[u8], offset: usize, length: usize) -> Option<u16> {
    if length < 6 || offset.checked_add(6)? > bytes.len() {
        return None;
    }
    read_u16_be(bytes, offset + 4)
}

fn decode_font_name(platform: u16, bytes: &[u8]) -> String {
    if platform == 0 || platform == 3 {
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .trim()
            .to_string()
    } else {
        String::from_utf8_lossy(bytes)
            .trim_matches('\0')
            .trim()
            .to_string()
    }
}

fn sqlite_encoding_name(value: u32) -> &'static str {
    match value {
        1 => "UTF-8",
        2 => "UTF-16le",
        3 => "UTF-16be",
        _ => "unknown",
    }
}

fn append_sqlite_header_details(text: &mut String, bytes: &[u8]) {
    let write_version = bytes.get(18).copied().unwrap_or(0);
    let read_version = bytes.get(19).copied().unwrap_or(0);
    if write_version > 0 || read_version > 0 {
        text.push_str(&format!(
            "\nJournal mode: {}",
            sqlite_journal_mode_name(write_version, read_version)
        ));
    }
    if let Some(schema_format) = read_u32_be(bytes, 44) {
        text.push_str(&format!(
            "\nSchema format: {}",
            sqlite_schema_format_name(schema_format)
        ));
    }
    if let Some(schema_cookie) = read_u32_be(bytes, 40) {
        text.push_str(&format!("\nSchema cookie: {}", schema_cookie));
    }
    if let Some(freelist_pages) = read_u32_be(bytes, 36) {
        text.push_str(&format!(
            "\nFreelist pages: {}",
            format_number(freelist_pages as i64)
        ));
    }
    if let Some(version) = read_u32_be(bytes, 96) {
        if version > 0 {
            text.push_str(&format!("\nSQLite version: {}", version));
        }
    }
}

fn sqlite_journal_mode_name(write_version: u8, read_version: u8) -> &'static str {
    match (write_version, read_version) {
        (2, 2) => "WAL",
        (1, 1) => "rollback journal",
        _ => "mixed/unknown",
    }
}

fn sqlite_schema_format_name(value: u32) -> String {
    match value {
        1 => "1 (legacy)".to_string(),
        2 => "2".to_string(),
        3 => "3".to_string(),
        4 => "4 (current)".to_string(),
        _ => format!("{value}"),
    }
}

#[derive(Debug, PartialEq)]
struct SqliteSchemaRow {
    typ: String,
    name: String,
    table_name: String,
    root_page: i64,
    sql: String,
}

fn append_sqlite_schema_summary(text: &mut String, bytes: &[u8], page_size: usize) {
    let summary = parse_sqlite_schema_summary(bytes, page_size, 8);
    let rows = summary.rows;
    if rows.is_empty() {
        return;
    }

    text.push_str(if summary.partial {
        "\nSchema objects (partial):"
    } else {
        "\nSchema objects:"
    });
    for row in rows {
        text.push_str(&format!(
            "\n- {} {} (table: {}, root: {})",
            row.typ, row.name, row.table_name, row.root_page
        ));
        if !row.sql.is_empty() {
            text.push_str(&format!(
                "\n  SQL: {}",
                truncate_sqlite_schema_sql(&row.sql)
            ));
        }
        if row.typ.eq_ignore_ascii_case("table") {
            let columns = parse_sqlite_table_columns(&row.sql, 8);
            if !columns.is_empty() {
                text.push_str("\n  Columns: ");
                text.push_str(&columns.join(", "));
            }
            if let Some(count) = count_sqlite_table_rows(bytes, page_size, row.root_page, 128) {
                text.push_str(&format!(
                    "\n  Rows observed: {}{}",
                    format_number(count.rows as i64),
                    if count.partial { " (partial)" } else { "" }
                ));
            }
        }
    }
}

struct SqliteSchemaSummary {
    rows: Vec<SqliteSchemaRow>,
    partial: bool,
}

struct SqliteRowCount {
    rows: u64,
    partial: bool,
}

fn count_sqlite_table_rows(
    bytes: &[u8],
    page_size: usize,
    root_page: i64,
    max_pages: usize,
) -> Option<SqliteRowCount> {
    if page_size < 512 || root_page <= 0 || max_pages == 0 {
        return None;
    }
    let mut stack = vec![root_page as u32];
    let mut seen = BTreeSet::<u32>::new();
    let mut rows = 0u64;
    let mut partial = false;
    while let Some(page_no) = stack.pop() {
        if seen.len() >= max_pages {
            partial = true;
            break;
        }
        if !seen.insert(page_no) {
            continue;
        }
        let Some(page) = sqlite_page(bytes, page_size, page_no) else {
            partial = true;
            continue;
        };
        let header = if page_no == 1 { 100 } else { 0 };
        let page_type = page.get(header).copied().unwrap_or(0);
        let cell_count = read_u16_be(page, header + 3).unwrap_or(0) as u64;
        match page_type {
            0x0D => rows = rows.saturating_add(cell_count),
            0x05 => {
                for child in sqlite_table_interior_children(page, header) {
                    stack.push(child);
                }
            }
            _ => return None,
        }
    }
    Some(SqliteRowCount { rows, partial })
}

fn sqlite_page(bytes: &[u8], page_size: usize, page_no: u32) -> Option<&[u8]> {
    if page_no == 0 {
        return None;
    }
    let start = (page_no as usize).checked_sub(1)?.checked_mul(page_size)?;
    let end = start.checked_add(page_size)?;
    bytes.get(start..end)
}

fn sqlite_table_interior_children(page: &[u8], header: usize) -> Vec<u32> {
    let cell_count = read_u16_be(page, header + 3).unwrap_or(0).min(512) as usize;
    let mut children = Vec::new();
    if let Some(rightmost) = read_u32_be(page, header + 8) {
        if rightmost > 0 {
            children.push(rightmost);
        }
    }
    for index in 0..cell_count {
        let ptr_offset = header + 12 + index * 2;
        let Some(cell_offset) = read_u16_be(page, ptr_offset).map(usize::from) else {
            break;
        };
        if let Some(child) = read_u32_be(page, cell_offset) {
            if child > 0 {
                children.push(child);
            }
        }
    }
    children
}

#[cfg(test)]
fn parse_sqlite_schema_rows(bytes: &[u8], page_size: usize, limit: usize) -> Vec<SqliteSchemaRow> {
    parse_sqlite_schema_summary(bytes, page_size, limit).rows
}

fn parse_sqlite_schema_summary(
    bytes: &[u8],
    page_size: usize,
    limit: usize,
) -> SqliteSchemaSummary {
    if page_size < 512 || bytes.len() < 128 || !bytes.starts_with(b"SQLite format 3\0") {
        return SqliteSchemaSummary {
            rows: Vec::new(),
            partial: false,
        };
    }
    let mut stack = vec![1u32];
    let mut seen = BTreeSet::<u32>::new();
    let mut rows = Vec::new();
    let mut partial = false;
    while let Some(page_no) = stack.pop() {
        if rows.len() >= limit || seen.len() >= 32 {
            partial = true;
            break;
        }
        if !seen.insert(page_no) {
            continue;
        }
        let Some(page) = sqlite_page(bytes, page_size, page_no) else {
            partial = true;
            continue;
        };
        let header = if page_no == 1 { 100usize } else { 0usize };
        match page.get(header).copied().unwrap_or(0) {
            0x0D => parse_sqlite_schema_leaf_page(page, header, limit, &mut rows),
            0x05 => {
                for child in sqlite_table_interior_children(page, header) {
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    SqliteSchemaSummary { rows, partial }
}

fn parse_sqlite_schema_leaf_page(
    page: &[u8],
    header: usize,
    limit: usize,
    rows: &mut Vec<SqliteSchemaRow>,
) {
    let cell_count = read_u16_be(page, header + 3).unwrap_or(0).min(256) as usize;
    for index in 0..cell_count {
        if rows.len() >= limit {
            break;
        }
        let ptr_offset = header + 8 + index * 2;
        let Some(cell_offset) = read_u16_be(page, ptr_offset).map(usize::from) else {
            break;
        };
        if let Some(row) = parse_sqlite_schema_leaf_cell(page, cell_offset) {
            rows.push(row);
        }
    }
}

fn parse_sqlite_schema_leaf_cell(page: &[u8], offset: usize) -> Option<SqliteSchemaRow> {
    let (payload_len, mut pos) = read_sqlite_varint(page, offset)?;
    let (_rowid, next) = read_sqlite_varint(page, pos)?;
    pos = next;
    let end = pos.checked_add(payload_len as usize)?;
    parse_sqlite_schema_record(page.get(pos..end)?)
}

fn parse_sqlite_schema_record(payload: &[u8]) -> Option<SqliteSchemaRow> {
    let (header_len, mut pos) = read_sqlite_varint(payload, 0)?;
    let header_len = header_len as usize;
    if header_len == 0 || header_len > payload.len() {
        return None;
    }
    let mut serials = Vec::new();
    while pos < header_len && serials.len() < 5 {
        let (serial, next) = read_sqlite_varint(payload, pos)?;
        serials.push(serial);
        pos = next;
    }
    if serials.len() < 5 {
        return None;
    }

    let mut value_pos = header_len;
    let typ = sqlite_record_text(payload, &mut value_pos, serials[0])?;
    let name = sqlite_record_text(payload, &mut value_pos, serials[1])?;
    let table_name = sqlite_record_text(payload, &mut value_pos, serials[2])?;
    let root_page = sqlite_record_integer(payload, &mut value_pos, serials[3])?;
    let sql = sqlite_record_text(payload, &mut value_pos, serials[4])?;
    Some(SqliteSchemaRow {
        typ,
        name,
        table_name,
        root_page,
        sql,
    })
}

fn truncate_sqlite_schema_sql(sql: &str) -> String {
    let compact = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_SQL_CHARS: usize = 160;
    if compact.chars().count() <= MAX_SQL_CHARS {
        return compact;
    }
    let mut out = compact.chars().take(MAX_SQL_CHARS).collect::<String>();
    out.push_str("...");
    out
}

fn parse_sqlite_table_columns(sql: &str, limit: usize) -> Vec<String> {
    let Some(body) = sqlite_parenthesized_body(sql) else {
        return Vec::new();
    };
    sqlite_split_top_level_csv(body)
        .into_iter()
        .filter_map(sqlite_column_summary)
        .take(limit)
        .collect()
}

fn sqlite_parenthesized_body(sql: &str) -> Option<&str> {
    let start = sql.find('(')?;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut previous = '\0';
    for (index, ch) in sql[start..].char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote && previous != '\\' {
                in_quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + index;
                    return sql.get(start + 1..end);
                }
            }
            _ => {}
        }
        previous = ch;
    }
    None
}

fn sqlite_split_top_level_csv(body: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut previous = '\0';
    for (index, ch) in body.char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote && previous != '\\' {
                in_quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                if let Some(item) = body.get(start..index) {
                    items.push(item.trim());
                }
                start = index + 1;
            }
            _ => {}
        }
        previous = ch;
    }
    if let Some(item) = body.get(start..) {
        items.push(item.trim());
    }
    items
}

fn sqlite_column_summary(definition: &str) -> Option<String> {
    let trimmed = definition.trim();
    if trimmed.is_empty() || sqlite_is_table_constraint(trimmed) {
        return None;
    }
    let (name, rest) = sqlite_take_identifier(trimmed)?;
    let typ = sqlite_column_type(rest);
    Some(if typ.is_empty() {
        name
    } else {
        format!("{name} {typ}")
    })
}

fn sqlite_is_table_constraint(value: &str) -> bool {
    let upper = value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK"
    )
}

fn sqlite_take_identifier(value: &str) -> Option<(String, &str)> {
    let value = value.trim_start();
    let mut chars = value.char_indices();
    let (_, first) = chars.next()?;
    if matches!(first, '"' | '\'' | '`' | '[') {
        let quote = if first == '[' { ']' } else { first };
        for (index, ch) in chars {
            if ch == quote {
                let name = value.get(first.len_utf8()..index)?.to_string();
                let rest = value.get(index + ch.len_utf8()..).unwrap_or_default();
                return Some((name, rest));
            }
        }
        return None;
    }
    let end = value
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    Some((
        value.get(..end)?.to_string(),
        value.get(end..).unwrap_or_default(),
    ))
}

fn sqlite_column_type(rest: &str) -> String {
    let rest = rest.trim_start();
    let mut token_start: Option<usize> = None;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    for (index, ch) in rest.char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => depth -= 1,
            ch if ch.is_whitespace() && depth == 0 => {
                if let Some(start) = token_start.take() {
                    if sqlite_is_column_constraint_keyword(&rest[start..index]) {
                        return rest[..start].trim().trim_end_matches(',').to_string();
                    }
                }
            }
            _ if token_start.is_none() && depth == 0 => token_start = Some(index),
            _ => {}
        }
    }
    if let Some(start) = token_start {
        if sqlite_is_column_constraint_keyword(&rest[start..]) {
            return rest[..start].trim().trim_end_matches(',').to_string();
        }
    }
    rest.trim().trim_end_matches(',').to_string()
}

fn sqlite_is_column_constraint_keyword(token: &str) -> bool {
    matches!(
        token.trim_matches(',').to_ascii_uppercase().as_str(),
        "PRIMARY"
            | "NOT"
            | "NULL"
            | "DEFAULT"
            | "COLLATE"
            | "REFERENCES"
            | "CHECK"
            | "UNIQUE"
            | "CONSTRAINT"
            | "GENERATED"
            | "AS"
    )
}

fn read_sqlite_varint(bytes: &[u8], offset: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for i in 0..9 {
        let b = *bytes.get(offset + i)?;
        if i == 8 {
            value = (value << 8) | b as u64;
            return Some((value, offset + i + 1));
        }
        value = (value << 7) | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            return Some((value, offset + i + 1));
        }
    }
    None
}

fn sqlite_record_text(payload: &[u8], pos: &mut usize, serial: u64) -> Option<String> {
    if serial < 13 || serial % 2 == 0 {
        sqlite_skip_record_value(payload, pos, serial)?;
        return Some(String::new());
    }
    let len = ((serial - 13) / 2) as usize;
    let end = pos.checked_add(len)?;
    let text = String::from_utf8_lossy(payload.get(*pos..end)?).to_string();
    *pos = end;
    Some(text)
}

fn sqlite_record_integer(payload: &[u8], pos: &mut usize, serial: u64) -> Option<i64> {
    match serial {
        0 => Some(0),
        1 => {
            let value = *payload.get(*pos)? as i8 as i64;
            *pos += 1;
            Some(value)
        }
        2 => {
            let end = pos.checked_add(2)?;
            let value = i16::from_be_bytes(payload.get(*pos..end)?.try_into().ok()?) as i64;
            *pos = end;
            Some(value)
        }
        4 => {
            let end = pos.checked_add(4)?;
            let value = i32::from_be_bytes(payload.get(*pos..end)?.try_into().ok()?) as i64;
            *pos = end;
            Some(value)
        }
        8 => Some(0),
        9 => Some(1),
        _ => {
            sqlite_skip_record_value(payload, pos, serial)?;
            Some(0)
        }
    }
}

fn sqlite_skip_record_value(payload: &[u8], pos: &mut usize, serial: u64) -> Option<()> {
    let len = match serial {
        0 | 8 | 9 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 | 7 => 8,
        n if n >= 12 => ((n - 12) / 2) as usize,
        _ => return None,
    };
    *pos = pos.checked_add(len)?;
    (payload.len() >= *pos).then_some(())
}

fn parse_mail_headers(content: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    headers
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
}

fn mail_header_parameter(value: &str, name: &str) -> Option<String> {
    mail_header_parameters(value)
        .into_iter()
        .find_map(|(key, raw_value)| key.eq_ignore_ascii_case(name).then_some(raw_value))
        .filter(|value| !value.is_empty())
}

fn mail_header_parameters(value: &str) -> Vec<(String, String)> {
    value
        .split(';')
        .skip(1)
        .filter_map(|part| {
            let (key, raw_value) = part.trim().split_once('=')?;
            Some((
                key.trim().to_string(),
                raw_value.trim().trim_matches('"').trim().to_string(),
            ))
        })
        .collect()
}

fn decode_mail_header_value(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("=?") {
        output.push_str(&rest[..start]);
        let encoded = &rest[start + 2..];
        let Some(charset_end) = encoded.find('?') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let charset = &encoded[..charset_end];
        let encoded = &encoded[charset_end + 1..];
        let Some(encoding_end) = encoded.find('?') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let encoding = &encoded[..encoding_end];
        let encoded = &encoded[encoding_end + 1..];
        let Some(value_end) = encoded.find("?=") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let encoded_value = &encoded[..value_end];
        if let Some(decoded) = decode_rfc2047_word(charset, encoding, encoded_value) {
            output.push_str(&decoded);
        } else {
            output.push_str(
                &rest[start..start + 2 + charset_end + 1 + encoding_end + 1 + value_end + 2],
            );
        }
        rest = &encoded[value_end + 2..];
    }
    output.push_str(rest);
    output
}

fn decode_rfc2047_word(charset: &str, encoding: &str, encoded: &str) -> Option<String> {
    if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
        return None;
    }
    let bytes = if encoding.eq_ignore_ascii_case("q") {
        let mut bytes = Vec::new();
        let mut chars = encoded.as_bytes().iter().copied();
        while let Some(byte) = chars.next() {
            match byte {
                b'_' => bytes.push(b' '),
                b'=' => {
                    let hi = chars.next()?;
                    let lo = chars.next()?;
                    bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
                }
                _ => bytes.push(byte),
            }
        }
        bytes
    } else if encoding.eq_ignore_ascii_case("b") {
        decode_base64(encoded)?
    } else {
        return None;
    };
    String::from_utf8(bytes).ok()
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut bytes = Vec::new();
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let sextet = base64_value(byte)? as u32;
        bits = (bits << 6) | sextet;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            bytes.push(((bits >> bit_count) & 0xFF) as u8);
        }
    }
    Some(bytes)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn mail_attachment_filenames(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            line.to_ascii_lowercase()
                .contains("content-disposition: attachment")
                .then_some(line)
        })
        .filter_map(mail_attachment_filename_from_disposition)
        .map(|name| decode_mail_header_value(&name))
        .take(5)
        .collect()
}

fn mail_mime_part_summaries(content: &str, boundary: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    mail_mime_part_summaries_inner(content, boundary, 0, &mut summaries);
    summaries
}

fn mail_mime_part_summaries_inner(
    content: &str,
    boundary: &str,
    depth: usize,
    summaries: &mut Vec<String>,
) {
    if depth > 4 || summaries.len() >= 32 {
        return;
    }
    let marker = format!("--{boundary}");
    for part in content.split(&marker).skip(1).take(32) {
        let trimmed = part.trim_start_matches(|ch| ch == '\r' || ch == '\n');
        if summaries.len() >= 32 || trimmed.starts_with("--") || trimmed.trim().is_empty() {
            continue;
        }
        let (header_text, body) = trimmed
            .split_once("\r\n\r\n")
            .or_else(|| trimmed.split_once("\n\n"))
            .unwrap_or((trimmed, ""));
        let headers = parse_mail_headers(header_text);
        let content_type_header = header_value(&headers, "Content-Type");
        let content_type = content_type_header
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text/plain".to_string());
        let disposition = header_value(&headers, "Content-Disposition")
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| !value.is_empty());
        let filename = header_value(&headers, "Content-Disposition")
            .and_then(mail_attachment_filename_from_disposition);
        let encoding = header_value(&headers, "Content-Transfer-Encoding")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let is_text_plain = content_type.eq_ignore_ascii_case("text/plain");
        let is_multipart = content_type.to_ascii_lowercase().starts_with("multipart/");
        let mut summary = if depth == 0 {
            content_type
        } else {
            format!("{}{}", ">".repeat(depth), content_type)
        };
        if let Some(disposition) = disposition {
            summary.push_str(&format!(" ({disposition})"));
        }
        if let Some(filename) = filename {
            summary.push_str(&format!(" filename={filename}"));
        }
        if let Some(encoding) = &encoding {
            summary.push_str(&format!(" encoding={encoding}"));
        }
        let body_len = body
            .trim_matches(|ch| ch == '\r' || ch == '\n')
            .as_bytes()
            .len();
        summary.push_str(&format!(" body={body_len} bytes"));
        if let Some(decoded_len) = encoding
            .as_deref()
            .and_then(|encoding| mail_decoded_body_len(body, encoding))
        {
            summary.push_str(&format!(" decoded={decoded_len} bytes"));
        }
        if is_text_plain {
            if let Some(preview) = mail_text_body_preview(body, encoding.as_deref()) {
                summary.push_str(&format!(" preview=\"{preview}\""));
            }
        }
        summaries.push(summary);
        if is_multipart {
            if let Some(child_boundary) =
                content_type_header.and_then(|value| mail_header_parameter(value, "boundary"))
            {
                mail_mime_part_summaries_inner(body, &child_boundary, depth + 1, summaries);
            }
        }
    }
}

fn mail_text_body_preview(body: &str, encoding: Option<&str>) -> Option<String> {
    let trimmed = body.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.is_empty() || trimmed.len() > 1024 * 1024 {
        return None;
    }
    let text = if encoding.is_some_and(|value| value.eq_ignore_ascii_case("base64")) {
        String::from_utf8_lossy(&decode_base64(trimmed)?).to_string()
    } else if encoding.is_some_and(|value| value.eq_ignore_ascii_case("quoted-printable")) {
        String::from_utf8_lossy(&decode_quoted_printable(trimmed.as_bytes())?).to_string()
    } else {
        trimmed.to_string()
    };
    let preview = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect::<String>();
    (!preview.is_empty()).then_some(preview)
}

fn mail_decoded_body_len(body: &str, encoding: &str) -> Option<usize> {
    let trimmed = body.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.len() > 1024 * 1024 {
        return None;
    }
    if encoding.eq_ignore_ascii_case("base64") {
        decode_base64(trimmed).map(|bytes| bytes.len())
    } else if encoding.eq_ignore_ascii_case("quoted-printable") {
        quoted_printable_decoded_len(trimmed.as_bytes())
    } else {
        None
    }
}

fn quoted_printable_decoded_len(bytes: &[u8]) -> Option<usize> {
    decode_quoted_printable(bytes).map(|bytes| bytes.len())
}

fn decode_quoted_printable(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'=' {
            if bytes.get(index + 1) == Some(&b'\r') && bytes.get(index + 2) == Some(&b'\n') {
                index += 3;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
                continue;
            }
            if index + 2 < bytes.len()
                && hex_nibble(bytes[index + 1]).is_some()
                && hex_nibble(bytes[index + 2]).is_some()
            {
                output.push((hex_nibble(bytes[index + 1])? << 4) | hex_nibble(bytes[index + 2])?);
                if output.len() > 1024 * 1024 {
                    return None;
                }
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        if output.len() > 1024 * 1024 {
            return None;
        }
        index += 1;
    }
    Some(output)
}

fn mail_attachment_filename_from_disposition(line: &str) -> Option<String> {
    if let Some(value) = mail_header_parameter(line, "filename") {
        return Some(value);
    }
    let parameters = mail_header_parameters(line);
    if let Some(value) = parameters
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case("filename*").then_some(value))
    {
        return decode_rfc2231_value(value);
    }

    let mut joined = String::new();
    for index in 0..32 {
        let encoded_key = format!("filename*{index}*");
        let plain_key = format!("filename*{index}");
        if let Some(value) = parameters.iter().find_map(|(key, value)| {
            (key.eq_ignore_ascii_case(&encoded_key) || key.eq_ignore_ascii_case(&plain_key))
                .then_some(value)
        }) {
            joined.push_str(value);
        } else {
            break;
        }
    }
    (!joined.is_empty()).then(|| decode_rfc2231_value(&joined).unwrap_or(joined))
}

#[derive(Clone)]
struct CfbDirectoryEntry {
    name: String,
    object_type: u8,
    start_sector: u32,
    size: u64,
}

fn append_msg_compound_summary(text: &mut String, bytes: &[u8]) {
    let entries = cfb_directory_entries(bytes);
    if entries.is_empty() {
        return;
    }
    let attachments = entries
        .iter()
        .filter(|entry| entry.object_type == 1 && entry.name.starts_with("__attach_version1.0_"))
        .count();
    let recipients = entries
        .iter()
        .filter(|entry| entry.object_type == 1 && entry.name.starts_with("__recip_version1.0_"))
        .count();
    if recipients > 0 {
        text.push_str(&format!("\nRecipients: {recipients}"));
    }
    if attachments > 0 {
        text.push_str(&format!("\nAttachments: {attachments}"));
    }
    for (label, property) in [
        ("Subject", "0037001F"),
        ("Sender", "0C1A001F"),
        ("Recipients display", "0E04001F"),
    ] {
        if let Some(value) = msg_unicode_property(bytes, &entries, property) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
    if let Some(sent_time) = msg_filetime_property(bytes, &entries, "0E060040") {
        text.push_str(&format!("\nSent time: {sent_time}"));
    }
    let has_body = entries.iter().any(|entry| {
        entry.object_type == 2
            && (entry.name.eq_ignore_ascii_case("__substg1.0_1000001F")
                || entry.name.eq_ignore_ascii_case("__substg1.0_10090102"))
    });
    if has_body {
        text.push_str("\nBody available: yes");
    }
}

fn cfb_directory_entries(bytes: &[u8]) -> Vec<CfbDirectoryEntry> {
    if !bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Vec::new();
    }
    let sector_shift = read_u16(bytes, 30).unwrap_or(9).min(12);
    let sector_size = 1usize << sector_shift;
    let first_directory_sector = read_u32(bytes, 48).unwrap_or(0xFFFF_FFFF);
    if first_directory_sector == 0xFFFF_FFFF {
        return Vec::new();
    }
    let Some(directory_offset) = cfb_sector_offset(first_directory_sector, sector_size) else {
        return Vec::new();
    };
    if directory_offset.saturating_add(sector_size) > bytes.len() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    for chunk in bytes[directory_offset..directory_offset + sector_size]
        .chunks_exact(128)
        .take(64)
    {
        let name_len = u16::from_le_bytes([chunk[64], chunk[65]]) as usize;
        if !(2..=64).contains(&name_len) {
            continue;
        }
        let name_bytes = &chunk[..name_len.saturating_sub(2).min(64)];
        let units = name_bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let name = String::from_utf16_lossy(&units);
        let object_type = chunk[66];
        if name.is_empty() || object_type == 0 {
            continue;
        }
        let start_sector = u32::from_le_bytes([chunk[116], chunk[117], chunk[118], chunk[119]]);
        let size = u64::from_le_bytes(chunk[120..128].try_into().ok().unwrap_or([0; 8]));
        entries.push(CfbDirectoryEntry {
            name,
            object_type,
            start_sector,
            size,
        });
    }
    entries
}

fn cfb_sector_offset(sector: u32, sector_size: usize) -> Option<usize> {
    (sector as usize).checked_add(1)?.checked_mul(sector_size)
}

fn msg_property_stream<'a>(
    bytes: &'a [u8],
    entries: &[CfbDirectoryEntry],
    property: &str,
) -> Option<&'a [u8]> {
    let entry = entries.iter().find(|entry| {
        entry.object_type == 2
            && entry
                .name
                .eq_ignore_ascii_case(&format!("__substg1.0_{property}"))
    })?;
    let sector_size = 1usize << read_u16(bytes, 30).unwrap_or(9).min(12);
    let offset = cfb_sector_offset(entry.start_sector, sector_size)?;
    let len = (entry.size as usize).min(4096);
    bytes.get(offset..offset.checked_add(len)?)
}

fn msg_unicode_property(
    bytes: &[u8],
    entries: &[CfbDirectoryEntry],
    property: &str,
) -> Option<String> {
    let stream = msg_property_stream(bytes, entries, property)?;
    let units = stream
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take(512)
        .collect::<Vec<_>>();
    let value = String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn msg_filetime_property(
    bytes: &[u8],
    entries: &[CfbDirectoryEntry],
    property: &str,
) -> Option<String> {
    let stream = msg_property_stream(bytes, entries, property)?;
    let filetime = u64::from_le_bytes(stream.get(0..8)?.try_into().ok()?);
    if filetime < 116_444_736_000_000_000 {
        return None;
    }
    let unix = ((filetime - 116_444_736_000_000_000) / 10_000_000) as i64;
    Some(format_timestamp(unix))
}

fn decode_rfc2231_value(value: &str) -> Option<String> {
    let encoded = if let Some((charset, rest)) = value.split_once('\'') {
        let (_, encoded) = rest.split_once('\'')?;
        if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
            return None;
        }
        encoded
    } else {
        value
    };
    String::from_utf8(percent_decode(encoded)?).ok()
}

fn percent_decode(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut iter = value.as_bytes().iter().copied();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let hi = iter.next()?;
            let lo = iter.next()?;
            bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
        } else {
            bytes.push(byte);
        }
    }
    Some(bytes)
}

fn append_minidump_streams(text: &mut String, bytes: &[u8]) {
    let streams = read_u32(bytes, 8).unwrap_or(0).min(64) as usize;
    let directory_rva = read_u32(bytes, 12).unwrap_or(0) as usize;
    if streams == 0 || directory_rva == 0 {
        return;
    }

    let mut names = Vec::new();
    let mut system_info = None;
    let mut exception_info = None;
    let mut thread_info = None;
    let mut module_info = None;
    let mut memory_info = None;
    let mut memory64_info = None;
    let mut thread_names_info = None;
    let mut handle_info = None;
    let mut unloaded_module_info = None;
    let mut misc_info = None;
    for index in 0..streams {
        let offset = directory_rva + index * 12;
        let Some(stream_type) = read_u32(bytes, offset) else {
            break;
        };
        let Some(data_size) = read_u32(bytes, offset + 4) else {
            break;
        };
        let Some(rva) = read_u32(bytes, offset + 8) else {
            break;
        };
        names.push(format!(
            "{} ({} @ 0x{rva:08X})",
            minidump_stream_name(stream_type),
            format_bytes(data_size as i64)
        ));
        if stream_type == 7 {
            system_info = parse_minidump_system_info(bytes, rva as usize, data_size as usize);
        } else if stream_type == 6 {
            exception_info = parse_minidump_exception(bytes, rva as usize, data_size as usize);
        } else if stream_type == 3 {
            thread_info = parse_minidump_thread_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 4 {
            module_info = parse_minidump_module_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 5 {
            memory_info = parse_minidump_memory_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 9 {
            memory64_info = parse_minidump_memory64_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 24 {
            thread_names_info =
                parse_minidump_thread_names(bytes, rva as usize, data_size as usize);
        } else if stream_type == 17 {
            handle_info = parse_minidump_handle_data(bytes, rva as usize, data_size as usize);
        } else if stream_type == 11 {
            unloaded_module_info =
                parse_minidump_unloaded_module_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 12 {
            misc_info = parse_minidump_misc_info(bytes, rva as usize, data_size as usize);
        }
    }
    if !names.is_empty() {
        text.push_str(&format!("\nStream summary: {}", names.join(", ")));
    }
    if let Some(system_info) = system_info {
        text.push_str(&system_info);
    }
    if let Some(exception_info) = exception_info {
        text.push_str(&exception_info);
    }
    if let Some(thread_info) = thread_info {
        text.push_str(&thread_info);
    }
    if let Some(module_info) = module_info {
        text.push_str(&module_info);
    }
    if let Some(memory_info) = memory_info {
        text.push_str(&memory_info);
    }
    if let Some(memory64_info) = memory64_info {
        text.push_str(&memory64_info);
    }
    if let Some(thread_names_info) = thread_names_info {
        text.push_str(&thread_names_info);
    }
    if let Some(handle_info) = handle_info {
        text.push_str(&handle_info);
    }
    if let Some(unloaded_module_info) = unloaded_module_info {
        text.push_str(&unloaded_module_info);
    }
    if let Some(misc_info) = misc_info {
        text.push_str(&misc_info);
    }
}

fn parse_minidump_misc_info(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 8 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let size_of_info = read_u32(bytes, offset).unwrap_or(0) as usize;
    let available = size
        .min(size_of_info)
        .min(bytes.len().saturating_sub(offset));
    if available < 8 {
        return None;
    }
    let flags = read_u32(bytes, offset + 4).unwrap_or(0);
    let mut lines = vec![format!("\nMiscInfo flags: 0x{flags:08X}")];
    if flags & 0x1 != 0 && available >= 12 {
        let process_id = read_u32(bytes, offset + 8).unwrap_or(0);
        lines.push(format!("Process ID: {process_id}"));
    }
    if flags & 0x2 != 0 && available >= 24 {
        let create_time = read_u32(bytes, offset + 12).unwrap_or(0);
        let user_time = read_u32(bytes, offset + 16).unwrap_or(0);
        let kernel_time = read_u32(bytes, offset + 20).unwrap_or(0);
        lines.push(format!("Process create time: {create_time}"));
        lines.push(format!("Process user time: {user_time}s"));
        lines.push(format!("Process kernel time: {kernel_time}s"));
    }
    if flags & 0x4 != 0 && available >= 44 {
        let max_mhz = read_u32(bytes, offset + 24).unwrap_or(0);
        let current_mhz = read_u32(bytes, offset + 28).unwrap_or(0);
        let mhz_limit = read_u32(bytes, offset + 32).unwrap_or(0);
        let max_idle_state = read_u32(bytes, offset + 36).unwrap_or(0);
        let current_idle_state = read_u32(bytes, offset + 40).unwrap_or(0);
        lines.push(format!(
            "Processor power: max {max_mhz} MHz; current {current_mhz} MHz; limit {mhz_limit} MHz; idle {current_idle_state}/{max_idle_state}"
        ));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_unloaded_module_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 12 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let header_size = read_u32(bytes, offset).unwrap_or(0).max(12) as usize;
    let entry_size = read_u32(bytes, offset + 4).unwrap_or(0) as usize;
    let count = read_u32(bytes, offset + 8).unwrap_or(0) as usize;
    if entry_size < 24 || header_size > size {
        return None;
    }
    let mut lines = vec![format!("\nUnloaded modules: {count}")];
    let mut entry_offset = offset + header_size;
    for _ in 0..count.min(12) {
        if entry_offset + entry_size > offset + size || entry_offset + 24 > bytes.len() {
            break;
        }
        let base = read_u64(bytes, entry_offset).unwrap_or(0);
        let image_size = read_u32(bytes, entry_offset + 8).unwrap_or(0) as u64;
        let end = base.saturating_add(image_size);
        let checksum = read_u32(bytes, entry_offset + 12).unwrap_or(0);
        let timestamp = read_u32(bytes, entry_offset + 16).unwrap_or(0);
        let name_rva = read_u32(bytes, entry_offset + 20).unwrap_or(0) as usize;
        let name =
            read_minidump_utf16_string(bytes, name_rva).unwrap_or_else(|| "<unnamed>".to_string());
        lines.push(format!(
            "Unloaded module {name}: range 0x{base:016X}-0x{end:016X}; timestamp 0x{timestamp:08X}; checksum 0x{checksum:08X}"
        ));
        entry_offset += entry_size;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_handle_data(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 16 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let header_size = read_u32(bytes, offset).unwrap_or(0).max(16) as usize;
    let descriptor_size = read_u32(bytes, offset + 4).unwrap_or(0) as usize;
    let count = read_u32(bytes, offset + 8).unwrap_or(0) as usize;
    if descriptor_size < 32 || header_size > size {
        return None;
    }
    let mut lines = vec![format!("\nHandles: {count}")];
    let mut descriptor_offset = offset + header_size;
    for _ in 0..count.min(8) {
        if descriptor_offset + descriptor_size > offset + size
            || descriptor_offset + 32 > bytes.len()
        {
            break;
        }
        let handle = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let type_name_rva = read_u32(bytes, descriptor_offset + 8).unwrap_or(0) as usize;
        let object_name_rva = read_u32(bytes, descriptor_offset + 12).unwrap_or(0) as usize;
        let attributes = read_u32(bytes, descriptor_offset + 16).unwrap_or(0);
        let granted_access = read_u32(bytes, descriptor_offset + 20).unwrap_or(0);
        let handle_count = read_u32(bytes, descriptor_offset + 24).unwrap_or(0);
        let pointer_count = read_u32(bytes, descriptor_offset + 28).unwrap_or(0);
        let type_name = read_minidump_utf16_string(bytes, type_name_rva)
            .unwrap_or_else(|| "<unknown>".to_string());
        let object_name = read_minidump_utf16_string(bytes, object_name_rva)
            .unwrap_or_else(|| "<unnamed>".to_string());
        lines.push(format!(
            "Handle 0x{handle:016X}: {type_name} {object_name}; access 0x{granted_access:08X}; attributes 0x{attributes:08X}; handles {handle_count}; pointers {pointer_count}"
        ));
        descriptor_offset += descriptor_size;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_thread_names(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nThread names: {count}")];
    let mut entry_offset = offset + 4;
    for _ in 0..count.min(12) {
        if entry_offset + 16 > offset + size || entry_offset + 16 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, entry_offset).unwrap_or(0);
        let name_rva = read_u64(bytes, entry_offset + 8).unwrap_or(0) as usize;
        if let Some(name) = read_minidump_utf16_string(bytes, name_rva) {
            lines.push(format!("Thread {id} name: {name}"));
        }
        entry_offset += 16;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_memory64_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 16 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u64(bytes, offset)? as usize;
    let base_rva = read_u64(bytes, offset + 8).unwrap_or(0);
    let mut total = 0u64;
    let mut lines = vec![
        format!("\nMemory64 ranges: {count}"),
        format!("Memory64 base RVA: 0x{base_rva:X}"),
    ];
    let mut descriptor_offset = offset + 16;
    for _ in 0..count.min(8) {
        if descriptor_offset + 16 > offset + size || descriptor_offset + 16 > bytes.len() {
            break;
        }
        let start = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let data_size = read_u64(bytes, descriptor_offset + 8).unwrap_or(0);
        total = total.saturating_add(data_size);
        let end = start.saturating_add(data_size);
        lines.push(format!(
            "Memory64 0x{start:016X}-0x{end:016X} ({data_size} bytes)"
        ));
        descriptor_offset += 16;
    }
    if count > 0 {
        lines.insert(2, format!("Memory64 bytes listed: {total}"));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_memory_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut total = 0u64;
    let mut lines = vec![format!("\nMemory ranges: {count}")];
    let mut descriptor_offset = offset + 4;
    for _ in 0..count.min(8) {
        if descriptor_offset + 16 > offset + size || descriptor_offset + 16 > bytes.len() {
            break;
        }
        let start = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let data_size = read_u32(bytes, descriptor_offset + 8).unwrap_or(0) as u64;
        total = total.saturating_add(data_size);
        let end = start.saturating_add(data_size);
        lines.push(format!(
            "Memory 0x{start:016X}-0x{end:016X} ({data_size} bytes)"
        ));
        descriptor_offset += 16;
    }
    if count > 0 {
        lines.insert(1, format!("Memory bytes listed: {total}"));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_module_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nModules: {count}")];
    let mut module_offset = offset + 4;
    for _ in 0..count.min(12) {
        if module_offset + 108 > offset + size || module_offset + 108 > bytes.len() {
            break;
        }
        let base = read_u64(bytes, module_offset).unwrap_or(0);
        let image_size = read_u32(bytes, module_offset + 8).unwrap_or(0);
        let timestamp = read_u32(bytes, module_offset + 16).unwrap_or(0);
        let name_rva = read_u32(bytes, module_offset + 20).unwrap_or(0) as usize;
        let name =
            read_minidump_utf16_string(bytes, name_rva).unwrap_or_else(|| "<unnamed>".to_string());
        let mut line = format!(
            "Module {name}: base 0x{base:016X}; size {image_size}; timestamp 0x{timestamp:08X}"
        );
        if let Some(version) = parse_minidump_fixed_version(bytes, module_offset + 24) {
            line.push_str(&format!(
                "; file version {}; product version {}; type {}; flags 0x{:08X}",
                version.file_version, version.product_version, version.file_type, version.flags
            ));
        }
        lines.push(line);
        module_offset += 108;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_fixed_version(bytes: &[u8], offset: usize) -> Option<PeFixedVersion> {
    if offset + 52 > bytes.len() || read_u32(bytes, offset)? != 0xFEEF_04BD {
        return None;
    }
    let file_ms = read_u32(bytes, offset + 8)?;
    let file_ls = read_u32(bytes, offset + 12)?;
    let product_ms = read_u32(bytes, offset + 16)?;
    let product_ls = read_u32(bytes, offset + 20)?;
    let flags_mask = read_u32(bytes, offset + 24).unwrap_or(0);
    let flags = read_u32(bytes, offset + 28).unwrap_or(0) & flags_mask;
    let file_type = read_u32(bytes, offset + 36).unwrap_or(0);
    Some(PeFixedVersion {
        file_version: format_pe_version(file_ms, file_ls),
        product_version: format_pe_version(product_ms, product_ls),
        flags,
        file_type: pe_version_file_type(file_type),
    })
}

fn parse_minidump_thread_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nThreads: {count}")];
    let mut thread_offset = offset + 4;
    for _ in 0..count.min(6) {
        if thread_offset + 48 > offset + size || thread_offset + 48 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, thread_offset).unwrap_or(0);
        let priority = read_u32(bytes, thread_offset + 12).unwrap_or(0);
        let stack_start = read_u64(bytes, thread_offset + 24).unwrap_or(0);
        let stack_size = read_u32(bytes, thread_offset + 32).unwrap_or(0);
        let stack_end = stack_start.saturating_add(stack_size as u64);
        lines.push(format!(
            "Thread {id}: priority {priority}; stack 0x{stack_start:016X}-0x{stack_end:016X}"
        ));
        thread_offset += 48;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_exception(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 32 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let thread_id = read_u32(bytes, offset)?;
    let code = read_u32(bytes, offset + 8)?;
    let flags = read_u32(bytes, offset + 12)?;
    let address = read_u64(bytes, offset + 24)?;
    let parameters = read_u32(bytes, offset + 32).unwrap_or(0);
    Some(format!(
        "\nException thread: {thread_id}\nException code: {}\nException flags: 0x{flags:08X}\nException address: 0x{address:016X}\nException parameters: {parameters}",
        minidump_exception_name(code)
    ))
}

fn minidump_exception_name(code: u32) -> String {
    match code {
        0x8000_0003 => "breakpoint".to_string(),
        0xC000_0005 => "access violation".to_string(),
        0xC000_001D => "illegal instruction".to_string(),
        0xC000_0094 => "integer divide by zero".to_string(),
        0xC000_00FD => "stack overflow".to_string(),
        _ => format!("0x{code:08X}"),
    }
}

fn parse_minidump_system_info(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 32 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let arch = read_u16(bytes, offset)?;
    let processors = *bytes.get(offset + 6)?;
    let product_type = *bytes.get(offset + 7)?;
    let major = read_u32(bytes, offset + 8)?;
    let minor = read_u32(bytes, offset + 12)?;
    let build = read_u32(bytes, offset + 16)?;
    let platform = read_u32(bytes, offset + 20)?;
    let csd_rva = read_u32(bytes, offset + 24).unwrap_or(0);
    let suite_mask = read_u16(bytes, offset + 28).unwrap_or(0);
    let mut text = format!(
        "\nSystem architecture: {}\nProcessors: {}\nWindows version: {}.{}.{}\nProduct type: {}\nPlatform ID: {}",
        minidump_processor_architecture(arch),
        processors,
        major,
        minor,
        build,
        minidump_product_type(product_type),
        platform
    );
    if suite_mask > 0 {
        text.push_str(&format!("\nSuite mask: 0x{suite_mask:04X}"));
    }
    if let Some(csd) = read_minidump_utf16_string(bytes, csd_rva as usize) {
        text.push_str(&format!("\nService pack: {csd}"));
    }
    Some(text)
}

fn read_minidump_utf16_string(bytes: &[u8], offset: usize) -> Option<String> {
    if offset == 0 || offset + 4 > bytes.len() {
        return None;
    }
    let len = read_u32(bytes, offset)? as usize;
    let start = offset + 4;
    let end = start.checked_add(len)?;
    let raw = bytes.get(start..end)?;
    let units = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let value = String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn minidump_processor_architecture(value: u16) -> &'static str {
    match value {
        0 => "x86",
        5 => "ARM",
        9 => "x64",
        12 => "ARM64",
        _ => "unknown",
    }
}

fn minidump_product_type(value: u8) -> &'static str {
    match value {
        1 => "workstation",
        2 => "domain controller",
        3 => "server",
        _ => "unknown",
    }
}

fn minidump_stream_name(value: u32) -> &'static str {
    match value {
        3 => "ThreadList",
        4 => "ModuleList",
        5 => "MemoryList",
        6 => "Exception",
        7 => "SystemInfo",
        9 => "Memory64List",
        11 => "UnloadedModuleList",
        12 => "MiscInfo",
        15 => "MemoryInfoList",
        16 => "ThreadInfoList",
        17 => "HandleData",
        _ => "Unknown",
    }
}

fn append_elf_summary(text: &mut String, bytes: &[u8]) {
    if !bytes.starts_with(&[0x7F, b'E', b'L', b'F']) || bytes.len() < 20 {
        text.push_str("\nFormat: ELF-like binary");
        return;
    }
    let class = bytes.get(4).copied().unwrap_or(0);
    let endian = bytes.get(5).copied().unwrap_or(1);
    text.push_str(&format!(
        "\nFormat: ELF{}",
        match class {
            1 => "32",
            2 => "64",
            _ => "",
        }
    ));
    text.push_str(&format!(
        "\nEndian: {}",
        if endian == 2 { "big" } else { "little" }
    ));
    if let Some(kind) = read_u16_endian(bytes, 16, endian) {
        text.push_str(&format!("\nType: {}", elf_type_name(kind)));
    }
    if let Some(machine) = read_u16_endian(bytes, 18, endian) {
        text.push_str(&format!("\nMachine: {}", elf_machine_name(machine)));
    }
    let entry = if class == 2 {
        read_u64_endian(bytes, 24, endian).map(|v| format!("0x{v:016X}"))
    } else {
        read_u32_endian(bytes, 24, endian).map(|v| format!("0x{v:08X}"))
    };
    if let Some(entry) = entry {
        text.push_str(&format!("\nEntry: {entry}"));
    }
    let phnum_offset = if class == 2 { 56 } else { 44 };
    let shnum_offset = if class == 2 { 60 } else { 48 };
    if let Some(phnum) = read_u16_endian(bytes, phnum_offset, endian) {
        text.push_str(&format!("\nProgram headers: {}", phnum));
    }
    if let Some(shnum) = read_u16_endian(bytes, shnum_offset, endian) {
        text.push_str(&format!("\nSection headers: {}", shnum));
    }
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian)
    } else {
        read_u32_endian(bytes, 28, endian).map(u64::from)
    };
    if let Some(phoff) = phoff.filter(|value| *value > 0) {
        text.push_str(&format!("\nProgram header offset: 0x{phoff:X}"));
    }
    let shoff = if class == 2 {
        read_u64_endian(bytes, 40, endian)
    } else {
        read_u32_endian(bytes, 32, endian).map(u64::from)
    };
    if let Some(shoff) = shoff.filter(|value| *value > 0) {
        text.push_str(&format!("\nSection header offset: 0x{shoff:X}"));
    }
    let flags_offset = if class == 2 { 48 } else { 36 };
    if let Some(flags) = read_u32_endian(bytes, flags_offset, endian).filter(|value| *value > 0) {
        text.push_str(&format!("\nFlags: 0x{flags:08X}"));
    }
    if let Some(interpreter) = elf_interpreter(bytes, class, endian) {
        text.push_str(&format!("\nInterpreter: {interpreter}"));
    }
    let needed = elf_needed_libraries(bytes, class, endian);
    if !needed.is_empty() {
        text.push_str(&format!("\nNeeded libraries: {}", needed.join(", ")));
    }
    for (label, value) in elf_dynamic_string_tags(bytes, class, endian) {
        text.push_str(&format!("\n{label}: {value}"));
    }
    let sections = elf_section_names(bytes, class, endian);
    if !sections.is_empty() {
        text.push_str(&format!("\nSection names: {}", sections.join(", ")));
    }
    let symbols = elf_symbol_summary(bytes, class, endian);
    if !symbols.is_empty() {
        text.push_str(&format!("\nSymbols: {}", symbols.join(", ")));
    }
    let relocations = elf_relocation_summary(bytes, class, endian);
    if !relocations.is_empty() {
        text.push_str(&format!("\nRelocations: {}", relocations.join(", ")));
    }
    let versions = elf_gnu_version_summary(bytes, class, endian);
    if !versions.is_empty() {
        text.push_str(&format!("\nGNU versions: {}", versions.join(", ")));
    }
    let notes = elf_note_summary(bytes, class, endian);
    if !notes.is_empty() {
        text.push_str(&format!("\nNotes: {}", notes.join(", ")));
    }
}

fn elf_interpreter(bytes: &[u8], class: u8, endian: u8) -> Option<String> {
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian)? as usize
    } else {
        read_u32_endian(bytes, 28, endian)? as usize
    };
    let phentsize = read_u16_endian(bytes, if class == 2 { 54 } else { 42 }, endian)? as usize;
    let phnum = read_u16_endian(bytes, if class == 2 { 56 } else { 44 }, endian)?.min(64) as usize;
    if phoff == 0 || phentsize == 0 {
        return None;
    }

    for index in 0..phnum {
        let offset = phoff.checked_add(index.checked_mul(phentsize)?)?;
        let typ = read_u32_endian(bytes, offset, endian)?;
        if typ != 3 {
            continue;
        }
        let (file_offset, file_size) = if class == 2 {
            (
                read_u64_endian(bytes, offset + 8, endian)? as usize,
                read_u64_endian(bytes, offset + 32, endian)? as usize,
            )
        } else {
            (
                read_u32_endian(bytes, offset + 4, endian)? as usize,
                read_u32_endian(bytes, offset + 16, endian)? as usize,
            )
        };
        let end = file_offset.checked_add(file_size)?;
        let raw = bytes.get(file_offset..end)?;
        let value = String::from_utf8_lossy(raw)
            .trim_matches('\0')
            .trim()
            .to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn elf_needed_libraries(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let headers = elf_program_headers(bytes, class, endian);
    let Some(dynamic) = headers.iter().find(|header| header.typ == 2) else {
        return Vec::new();
    };
    let mut strtab_vaddr = 0u64;
    let mut needed_offsets = Vec::new();
    let entry_size = if class == 2 { 16usize } else { 8usize };
    let mut offset = dynamic.file_offset as usize;
    let end = offset
        .saturating_add(dynamic.file_size as usize)
        .min(bytes.len());
    while offset + entry_size <= end && needed_offsets.len() < 32 {
        let tag = if class == 2 {
            read_u64_endian(bytes, offset, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset, endian).unwrap_or(0) as u64
        };
        let value = if class == 2 {
            read_u64_endian(bytes, offset + 8, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64
        };
        match tag {
            0 => break,
            1 => needed_offsets.push(value),
            5 => strtab_vaddr = value,
            _ => {}
        }
        offset += entry_size;
    }
    if strtab_vaddr == 0 {
        return Vec::new();
    }
    let Some(strtab_offset) = elf_vaddr_to_file_offset(&headers, strtab_vaddr) else {
        return Vec::new();
    };
    needed_offsets
        .into_iter()
        .filter_map(|name_offset| read_c_string(bytes, strtab_offset + name_offset as usize, 260))
        .filter(|name| !name.is_empty())
        .collect()
}

fn elf_dynamic_string_tags(bytes: &[u8], class: u8, endian: u8) -> Vec<(&'static str, String)> {
    let headers = elf_program_headers(bytes, class, endian);
    let Some(dynamic) = headers.iter().find(|header| header.typ == 2) else {
        return Vec::new();
    };
    let mut strtab_vaddr = 0u64;
    let mut tagged_offsets = Vec::new();
    let entry_size = if class == 2 { 16usize } else { 8usize };
    let mut offset = dynamic.file_offset as usize;
    let end = offset
        .saturating_add(dynamic.file_size as usize)
        .min(bytes.len());
    while offset + entry_size <= end && tagged_offsets.len() < 16 {
        let tag = if class == 2 {
            read_u64_endian(bytes, offset, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset, endian).unwrap_or(0) as u64
        };
        let value = if class == 2 {
            read_u64_endian(bytes, offset + 8, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64
        };
        match tag {
            0 => break,
            5 => strtab_vaddr = value,
            14 => tagged_offsets.push(("SONAME", value)),
            15 => tagged_offsets.push(("RPATH", value)),
            29 => tagged_offsets.push(("RUNPATH", value)),
            _ => {}
        }
        offset += entry_size;
    }
    if strtab_vaddr == 0 {
        return Vec::new();
    }
    let Some(strtab_offset) = elf_vaddr_to_file_offset(&headers, strtab_vaddr) else {
        return Vec::new();
    };
    tagged_offsets
        .into_iter()
        .filter_map(|(label, name_offset)| {
            read_c_string(bytes, strtab_offset + name_offset as usize, 260)
                .filter(|value| !value.is_empty())
                .map(|value| (label, value))
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ElfProgramHeader {
    typ: u32,
    file_offset: u64,
    virtual_address: u64,
    file_size: u64,
    memory_size: u64,
}

fn elf_program_headers(bytes: &[u8], class: u8, endian: u8) -> Vec<ElfProgramHeader> {
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian).unwrap_or(0) as usize
    } else {
        read_u32_endian(bytes, 28, endian).unwrap_or(0) as usize
    };
    let phentsize =
        read_u16_endian(bytes, if class == 2 { 54 } else { 42 }, endian).unwrap_or(0) as usize;
    let phnum = read_u16_endian(bytes, if class == 2 { 56 } else { 44 }, endian)
        .unwrap_or(0)
        .min(64) as usize;
    let mut headers = Vec::new();
    if phoff == 0 || phentsize == 0 {
        return headers;
    }
    for index in 0..phnum {
        let offset = phoff + index * phentsize;
        if offset + phentsize > bytes.len() {
            break;
        }
        let typ = read_u32_endian(bytes, offset, endian).unwrap_or(0);
        let header = if class == 2 {
            ElfProgramHeader {
                typ,
                file_offset: read_u64_endian(bytes, offset + 8, endian).unwrap_or(0),
                virtual_address: read_u64_endian(bytes, offset + 16, endian).unwrap_or(0),
                file_size: read_u64_endian(bytes, offset + 32, endian).unwrap_or(0),
                memory_size: read_u64_endian(bytes, offset + 40, endian).unwrap_or(0),
            }
        } else {
            ElfProgramHeader {
                typ,
                file_offset: read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64,
                virtual_address: read_u32_endian(bytes, offset + 8, endian).unwrap_or(0) as u64,
                file_size: read_u32_endian(bytes, offset + 16, endian).unwrap_or(0) as u64,
                memory_size: read_u32_endian(bytes, offset + 20, endian).unwrap_or(0) as u64,
            }
        };
        headers.push(header);
    }
    headers
}

fn elf_vaddr_to_file_offset(headers: &[ElfProgramHeader], vaddr: u64) -> Option<usize> {
    for header in headers.iter().filter(|header| header.typ == 1) {
        let span = header.memory_size.max(header.file_size).max(1);
        if vaddr >= header.virtual_address && vaddr < header.virtual_address.saturating_add(span) {
            return Some((header.file_offset + (vaddr - header.virtual_address)) as usize);
        }
    }
    None
}

fn elf_section_names(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    elf_sections(bytes, class, endian)
        .into_iter()
        .filter_map(|section| (!section.name.is_empty()).then_some(section.name))
        .take(24)
        .collect()
}

#[derive(Clone)]
struct ElfSection {
    name: String,
    typ: u32,
    offset: usize,
    size: usize,
    link: usize,
    entsize: usize,
}

fn elf_sections(bytes: &[u8], class: u8, endian: u8) -> Vec<ElfSection> {
    let shoff = if class == 2 {
        read_u64_endian(bytes, 40, endian).unwrap_or(0) as usize
    } else {
        read_u32_endian(bytes, 32, endian).unwrap_or(0) as usize
    };
    let shentsize =
        read_u16_endian(bytes, if class == 2 { 58 } else { 46 }, endian).unwrap_or(0) as usize;
    let shnum = read_u16_endian(bytes, if class == 2 { 60 } else { 48 }, endian)
        .unwrap_or(0)
        .min(128) as usize;
    let shstrndx =
        read_u16_endian(bytes, if class == 2 { 62 } else { 50 }, endian).unwrap_or(0) as usize;
    if shoff == 0 || shentsize == 0 || shstrndx >= shnum {
        return Vec::new();
    }
    let Some(str_header) = shoff.checked_add(shstrndx.saturating_mul(shentsize)) else {
        return Vec::new();
    };
    if str_header + shentsize > bytes.len() {
        return Vec::new();
    }
    let (str_offset, str_size) = if class == 2 {
        (
            read_u64_endian(bytes, str_header + 24, endian).unwrap_or(0) as usize,
            read_u64_endian(bytes, str_header + 32, endian).unwrap_or(0) as usize,
        )
    } else {
        (
            read_u32_endian(bytes, str_header + 16, endian).unwrap_or(0) as usize,
            read_u32_endian(bytes, str_header + 20, endian).unwrap_or(0) as usize,
        )
    };
    if str_offset == 0 || str_offset.saturating_add(str_size) > bytes.len() {
        return Vec::new();
    }
    let mut sections = Vec::new();
    for index in 0..shnum {
        let Some(header) = shoff.checked_add(index.saturating_mul(shentsize)) else {
            break;
        };
        if header + shentsize > bytes.len() {
            break;
        }
        let name_offset = read_u32_endian(bytes, header, endian).unwrap_or(0) as usize;
        let name = if name_offset == 0 || name_offset >= str_size {
            String::new()
        } else {
            read_c_string(bytes, str_offset + name_offset, 96).unwrap_or_default()
        };
        let typ = read_u32_endian(bytes, header + 4, endian).unwrap_or(0);
        let (offset, size, link, entsize) = if class == 2 {
            (
                read_u64_endian(bytes, header + 24, endian).unwrap_or(0) as usize,
                read_u64_endian(bytes, header + 32, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 40, endian).unwrap_or(0) as usize,
                read_u64_endian(bytes, header + 56, endian).unwrap_or(0) as usize,
            )
        } else {
            (
                read_u32_endian(bytes, header + 16, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 20, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 24, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 36, endian).unwrap_or(0) as usize,
            )
        };
        sections.push(ElfSection {
            name,
            typ,
            offset,
            size,
            link,
            entsize,
        });
    }
    sections
}

fn elf_symbol_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut symbols = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.typ == 2 || section.typ == 11)
    {
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else if class == 2 {
            24
        } else {
            16
        };
        if entry_size == 0 || section.offset.saturating_add(section.size) > bytes.len() {
            continue;
        }
        let Some(strtab) = sections.get(section.link) else {
            continue;
        };
        if strtab.offset.saturating_add(strtab.size) > bytes.len() {
            continue;
        }
        let count = section.size / entry_size;
        let mut named = Vec::new();
        for index in 0..count.min(64) {
            let offset = section.offset + index * entry_size;
            if offset + entry_size > bytes.len() {
                break;
            }
            let name_offset = read_u32_endian(bytes, offset, endian).unwrap_or(0) as usize;
            if name_offset == 0 || name_offset >= strtab.size {
                continue;
            }
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 128)
                .filter(|name| !name.is_empty())
            {
                let (info, shndx) = if class == 2 {
                    (
                        bytes.get(offset + 4).copied().unwrap_or(0),
                        read_u16_endian(bytes, offset + 6, endian).unwrap_or(0),
                    )
                } else {
                    (
                        bytes.get(offset + 12).copied().unwrap_or(0),
                        read_u16_endian(bytes, offset + 14, endian).unwrap_or(0),
                    )
                };
                named.push(format!(
                    "{}[{} {} {}]",
                    name,
                    elf_symbol_binding_name(info >> 4),
                    elf_symbol_type_name(info & 0x0F),
                    elf_symbol_section_name(&sections, shndx)
                ));
                if named.len() >= 8 {
                    break;
                }
            }
        }
        if !named.is_empty() {
            symbols.push(format!(
                "{} {} entries ({})",
                section.name,
                count,
                named.join(", ")
            ));
        }
    }
    symbols
}

fn elf_symbol_binding_name(value: u8) -> &'static str {
    match value {
        0 => "local",
        1 => "global",
        2 => "weak",
        10..=12 => "os",
        13..=15 => "proc",
        _ => "unknown",
    }
}

fn elf_symbol_type_name(value: u8) -> &'static str {
    match value {
        0 => "notype",
        1 => "object",
        2 => "func",
        3 => "section",
        4 => "file",
        5 => "common",
        6 => "tls",
        10..=12 => "os",
        13..=15 => "proc",
        _ => "unknown",
    }
}

fn elf_symbol_section_name(sections: &[ElfSection], shndx: u16) -> String {
    match shndx {
        0 => "UND".to_string(),
        0xFFF1 => "ABS".to_string(),
        0xFFF2 => "COMMON".to_string(),
        value => sections
            .get(value as usize)
            .and_then(|section| (!section.name.is_empty()).then_some(section.name.clone()))
            .unwrap_or_else(|| format!("section {value}")),
    }
}

fn elf_relocation_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let machine = read_u16_endian(bytes, 18, endian).unwrap_or(0);
    sections
        .iter()
        .filter(|section| section.typ == 4 || section.typ == 9)
        .filter_map(|section| {
            let entry_size = if section.entsize > 0 {
                section.entsize
            } else if section.typ == 4 && class == 2 {
                24
            } else if section.typ == 4 {
                12
            } else if class == 2 {
                16
            } else {
                8
            };
            if entry_size == 0
                || section.size == 0
                || section.offset.saturating_add(section.size) > bytes.len()
            {
                return None;
            }
            let count = section.size / entry_size;
            let mut types = Vec::new();
            for index in 0..count.min(8) {
                let offset = section.offset + index * entry_size;
                let rel_type = if class == 2 {
                    (read_u64_endian(bytes, offset + 8, endian).unwrap_or(0) & 0xFFFF_FFFF) as u32
                } else {
                    read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) & 0xFF
                };
                let name = elf_relocation_type_name(machine, rel_type);
                if !types.contains(&name) {
                    types.push(name);
                }
            }
            if types.is_empty() {
                Some(format!("{} {} entries", section.name, count))
            } else {
                Some(format!(
                    "{} {} entries ({})",
                    section.name,
                    count,
                    types.join(", ")
                ))
            }
        })
        .collect()
}

fn elf_relocation_type_name(machine: u16, typ: u32) -> String {
    match machine {
        62 => match typ {
            0 => "R_X86_64_NONE".to_string(),
            1 => "R_X86_64_64".to_string(),
            2 => "R_X86_64_PC32".to_string(),
            6 => "R_X86_64_GLOB_DAT".to_string(),
            7 => "R_X86_64_JUMP_SLOT".to_string(),
            8 => "R_X86_64_RELATIVE".to_string(),
            10 => "R_X86_64_32".to_string(),
            11 => "R_X86_64_32S".to_string(),
            _ => format!("x86-64:{typ}"),
        },
        183 => match typ {
            0 => "R_AARCH64_NONE".to_string(),
            257 => "R_AARCH64_ABS64".to_string(),
            1025 => "R_AARCH64_GLOB_DAT".to_string(),
            1026 => "R_AARCH64_JUMP_SLOT".to_string(),
            1027 => "R_AARCH64_RELATIVE".to_string(),
            _ => format!("AArch64:{typ}"),
        },
        _ => format!("type {typ}"),
    }
}

fn elf_gnu_version_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut versions = Vec::new();
    for section in sections
        .iter()
        .filter(|section| matches!(section.typ, 0x6FFF_FFFD..=0x6FFF_FFFF))
    {
        if section.offset.saturating_add(section.size) > bytes.len() {
            continue;
        }
        match section.typ {
            0x6FFF_FFFF => {
                let count = section.size / 2;
                let mut sample = Vec::new();
                for index in 0..count.min(8) {
                    let value = read_u16_endian(bytes, section.offset + index * 2, endian)
                        .unwrap_or(0)
                        & 0x7FFF;
                    if value > 1 && !sample.contains(&value) {
                        sample.push(value);
                    }
                }
                if sample.is_empty() {
                    versions.push(format!("{} {} entries", section.name, count));
                } else {
                    let sample = sample
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    versions.push(format!("{} {} entries ({sample})", section.name, count));
                }
            }
            0x6FFF_FFFE => {
                let names = elf_gnu_version_need_names(bytes, &sections, section, endian);
                if names.is_empty() {
                    versions.push(format!("{} need entries", section.name));
                } else {
                    versions.push(format!("{} needs {}", section.name, names.join("/")));
                }
            }
            0x6FFF_FFFD => {
                let names = elf_gnu_version_def_names(bytes, &sections, section, endian);
                if names.is_empty() {
                    versions.push(format!("{} definition entries", section.name));
                } else {
                    versions.push(format!("{} defines {}", section.name, names.join("/")));
                }
            }
            _ => {}
        }
    }
    versions
}

fn elf_gnu_version_string_table<'a>(
    sections: &'a [ElfSection],
    section: &ElfSection,
) -> Option<&'a ElfSection> {
    sections.get(section.link).filter(|strtab| strtab.typ == 3)
}

fn elf_gnu_version_need_names(
    bytes: &[u8],
    sections: &[ElfSection],
    section: &ElfSection,
    endian: u8,
) -> Vec<String> {
    let Some(strtab) = elf_gnu_version_string_table(sections, section) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    let end = section.offset + section.size;
    for _ in 0..16 {
        if offset + 16 > end {
            break;
        }
        let aux_count = read_u16_endian(bytes, offset + 2, endian)
            .unwrap_or(0)
            .min(16) as usize;
        let aux_offset = read_u32_endian(bytes, offset + 8, endian).unwrap_or(0) as usize;
        let next = read_u32_endian(bytes, offset + 12, endian).unwrap_or(0) as usize;
        let mut current_aux = offset.saturating_add(aux_offset);
        for _ in 0..aux_count {
            if current_aux + 16 > end {
                break;
            }
            let name_offset = read_u32_endian(bytes, current_aux + 8, endian).unwrap_or(0) as usize;
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let aux_next = read_u32_endian(bytes, current_aux + 12, endian).unwrap_or(0) as usize;
            if aux_next == 0 {
                break;
            }
            current_aux = current_aux.saturating_add(aux_next);
        }
        if next == 0 {
            break;
        }
        offset = offset.saturating_add(next);
    }
    names
}

fn elf_gnu_version_def_names(
    bytes: &[u8],
    sections: &[ElfSection],
    section: &ElfSection,
    endian: u8,
) -> Vec<String> {
    let Some(strtab) = elf_gnu_version_string_table(sections, section) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    let end = section.offset + section.size;
    for _ in 0..16 {
        if offset + 20 > end {
            break;
        }
        let aux_count = read_u16_endian(bytes, offset + 4, endian)
            .unwrap_or(0)
            .min(16) as usize;
        let aux_offset = read_u32_endian(bytes, offset + 12, endian).unwrap_or(0) as usize;
        let next = read_u32_endian(bytes, offset + 16, endian).unwrap_or(0) as usize;
        let mut current_aux = offset.saturating_add(aux_offset);
        for _ in 0..aux_count {
            if current_aux + 8 > end {
                break;
            }
            let name_offset = read_u32_endian(bytes, current_aux, endian).unwrap_or(0) as usize;
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let aux_next = read_u32_endian(bytes, current_aux + 4, endian).unwrap_or(0) as usize;
            if aux_next == 0 {
                break;
            }
            current_aux = current_aux.saturating_add(aux_next);
        }
        if next == 0 {
            break;
        }
        offset = offset.saturating_add(next);
    }
    names
}

fn elf_note_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut notes = Vec::new();
    for section in sections.iter().filter(|section| section.typ == 7) {
        append_elf_notes(
            &mut notes,
            bytes,
            endian,
            &section.name,
            section.offset,
            section.size,
        );
    }
    for header in elf_program_headers(bytes, class, endian)
        .iter()
        .filter(|header| header.typ == 4)
    {
        append_elf_notes(
            &mut notes,
            bytes,
            endian,
            "PT_NOTE",
            header.file_offset as usize,
            header.file_size as usize,
        );
    }
    notes
}

fn append_elf_notes(
    notes: &mut Vec<String>,
    bytes: &[u8],
    endian: u8,
    label: &str,
    file_offset: usize,
    size: usize,
) {
    if file_offset.saturating_add(size) > bytes.len() {
        return;
    }
    let mut offset = file_offset;
    let end = file_offset + size;
    while offset + 12 <= end && notes.len() < 8 {
        let namesz = read_u32_endian(bytes, offset, endian).unwrap_or(0) as usize;
        let descsz = read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as usize;
        let typ = read_u32_endian(bytes, offset + 8, endian).unwrap_or(0);
        let name_offset = offset + 12;
        let desc_offset = align4(name_offset.saturating_add(namesz));
        let next = align4(desc_offset.saturating_add(descsz));
        if namesz == 0 || desc_offset.saturating_add(descsz) > end || next <= offset {
            break;
        }
        let name = bytes
            .get(name_offset..name_offset + namesz)
            .map(|raw| {
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_default();
        let desc = bytes.get(desc_offset..desc_offset + descsz).unwrap_or(&[]);
        if name == "GNU" && typ == 3 && !desc.is_empty() {
            notes.push(format!("{} GNU build-id {}", label, bytes_to_hex(desc)));
        } else if !name.is_empty() {
            notes.push(format!(
                "{} {} type {} ({} bytes)",
                label, name, typ, descsz
            ));
        }
        offset = next;
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn elf_type_name(value: u16) -> &'static str {
    match value {
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core",
        _ => "unknown",
    }
}

fn elf_machine_name(value: u16) -> &'static str {
    match value {
        3 => "x86",
        40 => "ARM",
        62 => "x86-64",
        183 => "AArch64",
        243 => "RISC-V",
        _ => "unknown",
    }
}

fn media_container_name(path: &str, bytes: &[u8]) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if bytes.len() >= 12 && bytes.get(4..8) == Some(b"ftyp") {
        return "ISO BMFF / MP4";
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return "Matroska / WebM";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ") {
        return "AVI";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return "WAV";
    }
    if bytes.starts_with(b"fLaC") {
        return "FLAC";
    }
    if bytes.starts_with(b"ID3") || ext == "mp3" {
        return "MP3";
    }
    if bytes.starts_with(b"OggS") {
        return "Ogg";
    }
    match ext.as_str() {
        "flac" => "FLAC",
        "wav" => "WAV",
        "mkv" => "Matroska",
        "webm" => "WebM",
        "mov" => "QuickTime",
        "wmv" => "Windows Media",
        _ => "media",
    }
}

fn mp4_major_brand(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 || bytes.get(4..8) != Some(b"ftyp") {
        return None;
    }
    let brand = std::str::from_utf8(bytes.get(8..12)?).ok()?.trim();
    (!brand.is_empty()).then(|| brand.to_string())
}

#[cfg(test)]
fn mp4_duration_seconds(bytes: &[u8]) -> Option<f64> {
    find_mp4_atom_payload(bytes, b"mvhd").and_then(parse_mvhd_duration_seconds)
}

#[derive(Default)]
struct Mp4Summary {
    brand: Option<String>,
    duration_seconds: Option<f64>,
    created_unix: Option<i64>,
    rotation_degrees: Option<i32>,
    tracks: Vec<Mp4TrackSummary>,
}

#[derive(Default)]
struct Mp4TrackSummary {
    kind: &'static str,
    codec: String,
    codec_detail: String,
    language: String,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u16>,
    sample_rate: Option<u32>,
    duration_seconds: Option<f64>,
    data_bytes: Option<u64>,
    timing_entries: Option<u32>,
    samples: Option<u64>,
    decode_ticks: Option<u64>,
    first_sample_delta: Option<u32>,
    composition_entries: Option<u32>,
    composition_samples: Option<u64>,
    first_composition_offset: Option<i64>,
    composition_offset_range: Option<(i64, i64)>,
    edit_entries: Option<u32>,
    first_edit_duration: Option<u64>,
    first_edit_media_time: Option<i64>,
    first_edit_rate: Option<f64>,
    chunks: Option<u32>,
    first_chunk_offset: Option<u64>,
    last_chunk_end: Option<u64>,
    first_chunk_samples: Option<u32>,
    first_chunk_bytes: Option<u64>,
    first_sample_size: Option<u32>,
    chunk_details: Vec<String>,
}

fn mp4_summary(bytes: &[u8]) -> Option<Mp4Summary> {
    let brand = mp4_major_brand(bytes);
    let mvhd = find_mp4_atom_payload(bytes, b"mvhd");
    let duration_seconds = mvhd.and_then(parse_mvhd_duration_seconds);
    let created_unix = mvhd.and_then(parse_mvhd_created_unix);
    let rotation_degrees = mp4_rotation_degrees(bytes);
    let tracks = mp4_tracks(bytes);

    (brand.is_some()
        || duration_seconds.is_some()
        || created_unix.is_some()
        || rotation_degrees.is_some()
        || !tracks.is_empty())
    .then_some(Mp4Summary {
        brand,
        duration_seconds,
        created_unix,
        rotation_degrees,
        tracks,
    })
}

fn append_mp4_tracks(text: &mut String, tracks: &[Mp4TrackSummary]) {
    for (index, track) in tracks.iter().enumerate() {
        text.push_str(&format!("\n{} track {}", track.kind, index + 1));
        if !track.codec.is_empty() {
            text.push_str(&format!(": {}", media_codec_label(&track.codec)));
        }
        if !track.codec_detail.is_empty() {
            text.push_str(&format!(" ({})", track.codec_detail));
        }
        if !track.language.is_empty() {
            text.push_str(&format!("\n{} language: {}", track.kind, track.language));
        }
        if let (Some(width), Some(height)) = (track.width, track.height) {
            text.push_str(&format!("\n{} size: {}x{}", track.kind, width, height));
        }
        if let Some(channels) = track.channels {
            text.push_str(&format!("\n{} channels: {}", track.kind, channels));
        }
        if let Some(sample_rate) = track.sample_rate {
            text.push_str(&format!(
                "\n{} sample rate: {} Hz",
                track.kind,
                format_number(sample_rate as i64)
            ));
        }
        if let Some(duration) = track.duration_seconds {
            text.push_str(&format!(
                "\n{} duration: {}",
                track.kind,
                format_duration(duration)
            ));
            if let Some(data_bytes) = track.data_bytes {
                if let Some(bitrate) = estimate_bitrate(data_bytes as i64, duration) {
                    text.push_str(&format!(
                        "\n{} bitrate: {}",
                        track.kind,
                        format_bitrate(bitrate)
                    ));
                }
            }
        }
        if let Some(entries) = track.timing_entries {
            text.push_str(&format!("\n{} timing entries: {}", track.kind, entries));
        }
        if let Some(samples) = track.samples {
            text.push_str(&format!(
                "\n{} samples: {}",
                track.kind,
                format_number(samples as i64)
            ));
        }
        if let Some(decode_ticks) = track.decode_ticks {
            text.push_str(&format!(
                "\n{} decode ticks: {}",
                track.kind,
                format_number(decode_ticks as i64)
            ));
        }
        if let Some(delta) = track.first_sample_delta {
            text.push_str(&format!("\n{} first sample delta: {}", track.kind, delta));
        }
        if let Some(entries) = track.composition_entries {
            text.push_str(&format!(
                "\n{} composition offsets: {}",
                track.kind, entries
            ));
        }
        if let Some(samples) = track.composition_samples {
            text.push_str(&format!(
                "\n{} composition samples: {}",
                track.kind,
                format_number(samples as i64)
            ));
        }
        if let Some(offset) = track.first_composition_offset {
            text.push_str(&format!(
                "\n{} first composition offset: {}",
                track.kind, offset
            ));
        }
        if let Some((min, max)) = track.composition_offset_range {
            text.push_str(&format!(
                "\n{} composition offset range: {}..{}",
                track.kind, min, max
            ));
        }
        if let Some(entries) = track.edit_entries {
            text.push_str(&format!("\n{} edit list entries: {}", track.kind, entries));
        }
        if let Some(duration) = track.first_edit_duration {
            text.push_str(&format!(
                "\n{} first edit duration: {}",
                track.kind,
                format_number(duration as i64)
            ));
        }
        if let Some(media_time) = track.first_edit_media_time {
            text.push_str(&format!(
                "\n{} first edit media time: {}",
                track.kind, media_time
            ));
        }
        if let Some(rate) = track.first_edit_rate {
            text.push_str(&format!("\n{} first edit rate: {:.2}", track.kind, rate));
        }
        if let Some(chunks) = track.chunks {
            text.push_str(&format!("\n{} chunks: {}", track.kind, chunks));
            if let (Some(first), Some(last)) = (track.first_chunk_offset, track.last_chunk_end) {
                text.push_str(&format!(" (0x{first:X}-0x{last:X})"));
            }
        }
        if let Some(samples) = track.first_chunk_samples {
            text.push_str(&format!(
                "\n{} first chunk samples: {}",
                track.kind, samples
            ));
        }
        if let Some(bytes) = track.first_chunk_bytes {
            text.push_str(&format!(
                "\n{} first chunk bytes: {}",
                track.kind,
                format_number(bytes as i64)
            ));
        }
        if let Some(size) = track.first_sample_size {
            text.push_str(&format!(
                "\n{} first sample size: {}",
                track.kind,
                format_number(size as i64)
            ));
        }
        if !track.chunk_details.is_empty() {
            text.push_str(&format!(
                "\n{} chunk map: {}",
                track.kind,
                track.chunk_details.join(", ")
            ));
        }
    }
}

fn mp4_tracks(bytes: &[u8]) -> Vec<Mp4TrackSummary> {
    let mut payloads = Vec::new();
    collect_mp4_atom_payloads(bytes, b"trak", &mut payloads);
    payloads.into_iter().filter_map(parse_mp4_track).collect()
}

fn parse_mp4_track(trak: &[u8]) -> Option<Mp4TrackSummary> {
    let handler = find_mp4_atom_payload(trak, b"hdlr").and_then(parse_hdlr_handler_type);
    let mut summary = Mp4TrackSummary {
        kind: match handler.as_deref() {
            Some("vide") => "Video",
            Some("soun") => "Audio",
            _ => "Media",
        },
        ..Default::default()
    };

    if let Some(tkhd) = find_mp4_atom_payload(trak, b"tkhd") {
        let (width, height) = parse_tkhd_dimensions(tkhd).unwrap_or((0, 0));
        if width > 0 && height > 0 {
            summary.width = Some(width);
            summary.height = Some(height);
        }
    }
    if let Some(mdhd) = find_mp4_atom_payload(trak, b"mdhd") {
        summary.duration_seconds = parse_mdhd_duration_seconds(mdhd);
        summary.language = parse_mdhd_language(mdhd).unwrap_or_default();
    }
    if let Some(stsd) = find_mp4_atom_payload(trak, b"stsd") {
        parse_stsd_summary(stsd, &mut summary);
    }
    if let Some(stsz) = find_mp4_atom_payload(trak, b"stsz") {
        summary.data_bytes = parse_stsz_total_bytes(stsz);
    }
    if let Some(stts) = find_mp4_atom_payload(trak, b"stts") {
        summary.timing_entries = parse_mp4_entry_count(stts);
        if let Some(timeline) = parse_stts_timeline(stts) {
            summary.samples = Some(timeline.samples);
            summary.decode_ticks = Some(timeline.decode_ticks);
            summary.first_sample_delta = timeline.first_delta;
        }
    }
    if let Some(ctts) = find_mp4_atom_payload(trak, b"ctts") {
        summary.composition_entries = parse_mp4_entry_count(ctts);
        if let Some(composition) = parse_ctts_summary(ctts) {
            summary.composition_samples = Some(composition.samples);
            summary.first_composition_offset = composition.first_offset;
            summary.composition_offset_range = composition.offset_range;
        }
    }
    if let Some(elst) = find_mp4_atom_payload(trak, b"elst") {
        summary.edit_entries = parse_mp4_entry_count(elst);
        if let Some(edit) = parse_elst_summary(elst) {
            summary.first_edit_duration = edit.first_duration;
            summary.first_edit_media_time = edit.first_media_time;
            summary.first_edit_rate = edit.first_rate;
        }
    }
    if let Some(chunk_summary) = parse_mp4_chunk_summary(trak) {
        summary.chunks = Some(chunk_summary.chunks);
        summary.first_chunk_offset = Some(chunk_summary.first_offset);
        summary.last_chunk_end = Some(chunk_summary.last_end);
        summary.data_bytes = Some(chunk_summary.data_bytes);
        summary.first_chunk_samples = chunk_summary.first_chunk_samples;
        summary.first_chunk_bytes = chunk_summary.first_chunk_bytes;
        summary.first_sample_size = chunk_summary.first_sample_size;
        summary.chunk_details = chunk_summary.chunk_details;
    }

    (!summary.codec.is_empty()
        || summary.width.is_some()
        || !summary.codec_detail.is_empty()
        || !summary.language.is_empty()
        || summary.height.is_some()
        || summary.channels.is_some()
        || summary.sample_rate.is_some()
        || summary.duration_seconds.is_some()
        || summary.data_bytes.is_some()
        || summary.timing_entries.is_some()
        || summary.composition_entries.is_some()
        || summary.edit_entries.is_some()
        || summary.chunks.is_some())
    .then_some(summary)
}

fn parse_hdlr_handler_type(payload: &[u8]) -> Option<String> {
    let handler = std::str::from_utf8(payload.get(8..12)?).ok()?.trim();
    (!handler.is_empty()).then(|| handler.to_string())
}

fn parse_mdhd_duration_seconds(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;
    match version {
        0 => duration_from_timescale(read_u32_be(payload, 16)? as u64, read_u32_be(payload, 12)?),
        1 => duration_from_timescale(read_u64_be(payload, 24)?, read_u32_be(payload, 20)?),
        _ => None,
    }
}

fn parse_mdhd_language(payload: &[u8]) -> Option<String> {
    let version = *payload.first()?;
    let offset = match version {
        0 => 20,
        1 => 32,
        _ => return None,
    };
    let packed = read_u16_be(payload, offset)?;
    let mut language = String::new();
    for shift in [10, 5, 0] {
        let value = ((packed >> shift) & 0x1F) as u8;
        if value == 0 {
            return None;
        }
        language.push((value + 0x60) as char);
    }
    (language != "und").then_some(language)
}

fn parse_tkhd_dimensions(payload: &[u8]) -> Option<(u32, u32)> {
    let version = *payload.first()?;
    let offset = match version {
        0 => 76,
        1 => 88,
        _ => return None,
    };
    let width = read_u32_be(payload, offset)? >> 16;
    let height = read_u32_be(payload, offset + 4)? >> 16;
    Some((width, height))
}

fn parse_stsd_summary(payload: &[u8], summary: &mut Mp4TrackSummary) -> Option<()> {
    let entries = read_u32_be(payload, 4)?.min(16) as usize;
    let mut offset = 8usize;
    for _ in 0..entries {
        let entry_size = read_u32_be(payload, offset)? as usize;
        let codec = std::str::from_utf8(payload.get(offset + 4..offset + 8)?)
            .ok()?
            .to_string();
        if summary.codec.is_empty() {
            summary.codec = codec;
        }

        if summary.kind == "Video" && entry_size >= 36 {
            summary.width = read_u16_be(payload, offset + 32)
                .map(u32::from)
                .filter(|value| *value > 0);
            summary.height = read_u16_be(payload, offset + 34)
                .map(u32::from)
                .filter(|value| *value > 0);
            if let Some(detail) = parse_video_codec_detail(payload, offset, entry_size) {
                summary.codec_detail = detail;
            }
        } else if summary.kind == "Audio" && entry_size >= 32 {
            summary.channels = read_u16_be(payload, offset + 16).filter(|value| *value > 0);
            summary.sample_rate = read_u32_be(payload, offset + 24)
                .map(|value| value >> 16)
                .filter(|value| *value > 0);
            if let Some(detail) = parse_audio_codec_detail(payload, offset, entry_size) {
                summary.codec_detail = detail;
            }
        }

        offset = offset.checked_add(entry_size)?;
        if offset > payload.len() {
            break;
        }
    }
    Some(())
}

fn parse_video_codec_detail(
    payload: &[u8],
    entry_offset: usize,
    entry_size: usize,
) -> Option<String> {
    let start = entry_offset.checked_add(86)?;
    let end = entry_offset.checked_add(entry_size)?.min(payload.len());
    if let Some(avcc) = find_mp4_atom_payload_in_range(payload, start, end, b"avcC", 0) {
        return parse_avcc_detail(avcc);
    }
    if let Some(hvcc) = find_mp4_atom_payload_in_range(payload, start, end, b"hvcC", 0) {
        return parse_hvcc_detail(hvcc);
    }
    None
}

fn parse_audio_codec_detail(
    payload: &[u8],
    entry_offset: usize,
    entry_size: usize,
) -> Option<String> {
    let start = entry_offset.checked_add(36)?;
    let end = entry_offset.checked_add(entry_size)?.min(payload.len());
    find_mp4_atom_payload_in_range(payload, start, end, b"esds", 0).and_then(parse_esds_detail)
}

fn parse_esds_detail(payload: &[u8]) -> Option<String> {
    let body = payload.get(4..).unwrap_or(payload);
    let mut object_type = None;
    let mut audio_config = None;
    let mut offset = 0usize;
    while offset < body.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(body, offset) else {
            offset += 1;
            continue;
        };
        if tag == 0x04 {
            object_type = descriptor.first().copied();
            if audio_config.is_none() {
                audio_config = find_mpeg4_descriptor(descriptor, 0x05)
                    .and_then(parse_aac_audio_specific_config);
            }
        } else if tag == 0x05 {
            audio_config = parse_aac_audio_specific_config(descriptor);
        }
        if object_type.is_some() && audio_config.is_some() {
            break;
        }
        offset = next.max(offset + 1);
    }

    let mut parts = Vec::new();
    if let Some(value) = object_type {
        parts.push(format!("object type {}", mpeg4_object_type_name(value)));
    }
    if let Some(config) = audio_config {
        parts.push(config);
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn find_mpeg4_descriptor<'a>(bytes: &'a [u8], target: u8) -> Option<&'a [u8]> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(bytes, offset) else {
            offset += 1;
            continue;
        };
        if tag == target {
            return Some(descriptor);
        }
        offset = next.max(offset + 1);
    }
    None
}

fn read_mpeg4_descriptor(bytes: &[u8], offset: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *bytes.get(offset)?;
    let mut pos = offset + 1;
    let mut len = 0usize;
    for _ in 0..4 {
        let byte = *bytes.get(pos)?;
        pos += 1;
        len = (len << 7) | (byte & 0x7F) as usize;
        if byte & 0x80 == 0 {
            let end = pos.checked_add(len)?;
            return Some((tag, bytes.get(pos..end)?, end));
        }
    }
    None
}

fn parse_aac_audio_specific_config(bytes: &[u8]) -> Option<String> {
    let first = *bytes.first()?;
    let second = *bytes.get(1)?;
    let object_type = first >> 3;
    let frequency_index = ((first & 0x07) << 1) | (second >> 7);
    let channels = (second >> 3) & 0x0F;
    let mut parts = vec![aac_object_type_name(object_type).to_string()];
    if let Some(sample_rate) = aac_sample_rate(frequency_index) {
        parts.push(format!("{} Hz", format_number(sample_rate as i64)));
    }
    if channels > 0 {
        parts.push(format!("{} ch", channels));
    }
    Some(parts.join(", "))
}

fn mpeg4_object_type_name(value: u8) -> String {
    match value {
        0x40 => "MPEG-4 Audio".to_string(),
        0x20 => "MPEG-4 Visual".to_string(),
        0x21 => "H.264".to_string(),
        0x6B => "MP3".to_string(),
        _ => format!("0x{value:02X}"),
    }
}

fn aac_object_type_name(value: u8) -> &'static str {
    match value {
        1 => "AAC Main",
        2 => "AAC LC",
        3 => "AAC SSR",
        4 => "AAC LTP",
        5 => "HE-AAC SBR",
        29 => "HE-AACv2 PS",
        _ => "AAC",
    }
}

fn aac_sample_rate(index: u8) -> Option<u32> {
    Some(match index {
        0 => 96_000,
        1 => 88_200,
        2 => 64_000,
        3 => 48_000,
        4 => 44_100,
        5 => 32_000,
        6 => 24_000,
        7 => 22_050,
        8 => 16_000,
        9 => 12_000,
        10 => 11_025,
        11 => 8_000,
        12 => 7_350,
        _ => return None,
    })
}

fn parse_avcc_detail(payload: &[u8]) -> Option<String> {
    let profile = *payload.get(1)?;
    let compatibility = *payload.get(2)?;
    let level = *payload.get(3)?;
    let nal_length = usize::from(payload.get(4).map(|value| (value & 0x03) + 1).unwrap_or(0));
    let mut parts = vec![format!(
        "AVC profile 0x{profile:02X}, compat 0x{compatibility:02X}, level {}.{}",
        level / 10,
        level % 10
    )];
    if nal_length > 0 {
        parts.push(format!("{}-byte NAL length", nal_length));
    }
    if let Some((chroma, luma_bits, chroma_bits)) = parse_avcc_extension(payload) {
        parts.push(format!("chroma {chroma}"));
        parts.push(format!("{}-bit luma", luma_bits));
        parts.push(format!("{}-bit chroma", chroma_bits));
    }
    if let Some(sps) = parse_avcc_sps_summary(payload) {
        parts.push(sps);
    }
    Some(parts.join(", "))
}

fn parse_avcc_sps_summary(payload: &[u8]) -> Option<String> {
    let sps_count = (*payload.get(5)? & 0x1F) as usize;
    let mut offset = 6usize;
    for _ in 0..sps_count.min(1) {
        let len = read_u16_be(payload, offset)? as usize;
        offset += 2;
        let sps = payload.get(offset..offset.checked_add(len)?)?;
        let summary = parse_h264_sps_summary(sps)?;
        return Some(summary);
    }
    None
}

fn parse_h264_sps_summary(sps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(sps.get(1..)?);
    let mut bits = BitReader::new(&rbsp);
    let profile_idc = bits.read_bits(8)? as u8;
    bits.read_bits(8)?;
    bits.read_bits(8)?;
    bits.read_ue()?;
    let mut chroma_format_idc = 1u32;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = bits.read_ue()?;
        if chroma_format_idc == 3 {
            bits.read_bits(1)?;
        }
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_bits(1)?;
        if bits.read_bits(1)? != 0 {
            let lists = if chroma_format_idc == 3 { 12 } else { 8 };
            for index in 0..lists {
                if bits.read_bits(1)? != 0 {
                    skip_h264_scaling_list(&mut bits, if index < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    bits.read_ue()?;
    let pic_order_cnt_type = bits.read_ue()?;
    if pic_order_cnt_type == 0 {
        bits.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        bits.read_bits(1)?;
        bits.read_se()?;
        bits.read_se()?;
        let cycle = bits.read_ue()?.min(256);
        for _ in 0..cycle {
            bits.read_se()?;
        }
    }
    bits.read_ue()?;
    bits.read_bits(1)?;
    let width_mbs = bits.read_ue()?.checked_add(1)?;
    let height_map_units = bits.read_ue()?.checked_add(1)?;
    let frame_mbs_only = bits.read_bits(1)? != 0;
    if !frame_mbs_only {
        bits.read_bits(1)?;
    }
    bits.read_bits(1)?;
    let mut crop = (0u32, 0u32, 0u32, 0u32);
    if bits.read_bits(1)? != 0 {
        crop = (
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
        );
    }
    let vui_summary = if bits.read_bits(1).unwrap_or(0) != 0 {
        parse_h264_vui_summary(&mut bits)
    } else {
        None
    };
    let coded_width = width_mbs.checked_mul(16)?;
    let coded_height = height_map_units
        .checked_mul(16)?
        .checked_mul(if frame_mbs_only { 1 } else { 2 })?;
    let (crop_x, crop_y) = h264_crop_units(chroma_format_idc, frame_mbs_only);
    let display_width = coded_width.saturating_sub((crop.0 + crop.1).saturating_mul(crop_x));
    let display_height = coded_height.saturating_sub((crop.2 + crop.3).saturating_mul(crop_y));
    let mut parts = vec![format!("SPS coded {coded_width}x{coded_height}")];
    if display_width != coded_width || display_height != coded_height {
        parts.push(format!("crop display {display_width}x{display_height}"));
    }
    if let Some(vui) = vui_summary {
        parts.push(vui);
    }
    Some(parts.join(", "))
}

fn parse_h264_vui_summary(bits: &mut BitReader<'_>) -> Option<String> {
    let mut parts = vec!["VUI".to_string()];
    let aspect_ratio_info_present = bits.read_bits(1)? != 0;
    if aspect_ratio_info_present {
        let aspect_ratio_idc = bits.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            bits.read_bits(16)?;
            bits.read_bits(16)?;
        }
    }
    let overscan_info_present = bits.read_bits(1)? != 0;
    if overscan_info_present {
        bits.read_bits(1)?;
    }
    let video_signal_type_present = bits.read_bits(1)? != 0;
    if video_signal_type_present {
        let video_format = bits.read_bits(3)?;
        let full_range = bits.read_bits(1)? != 0;
        parts.push(format!("video format {video_format}"));
        if full_range {
            parts.push("full range".to_string());
        }
        let colour_description_present = bits.read_bits(1)? != 0;
        if colour_description_present {
            let primaries = bits.read_bits(8)? as u8;
            let transfer = bits.read_bits(8)? as u8;
            let matrix = bits.read_bits(8)? as u8;
            parts.push(format!(
                "primaries {}",
                h264_color_primaries_name(primaries)
            ));
            parts.push(format!("transfer {}", h264_transfer_name(transfer)));
            parts.push(format!("matrix {}", h264_matrix_name(matrix)));
        }
    }
    Some(parts.join(", "))
}

fn h264_color_primaries_name(value: u8) -> String {
    match value {
        1 => "BT.709".to_string(),
        4 => "BT.470M".to_string(),
        5 => "BT.470BG".to_string(),
        6 => "SMPTE 170M".to_string(),
        9 => "BT.2020".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_transfer_name(value: u8) -> String {
    match value {
        1 => "BT.709".to_string(),
        6 => "SMPTE 170M".to_string(),
        13 => "sRGB".to_string(),
        14 => "BT.2020 10-bit".to_string(),
        16 => "PQ".to_string(),
        18 => "HLG".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_matrix_name(value: u8) -> String {
    match value {
        0 => "GBR".to_string(),
        1 => "BT.709".to_string(),
        6 => "SMPTE 170M".to_string(),
        9 => "BT.2020 non-constant".to_string(),
        10 => "BT.2020 constant".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_ebsp_to_rbsp(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut zeros = 0;
    for &byte in bytes {
        if zeros >= 2 && byte == 0x03 {
            zeros = 0;
            continue;
        }
        if byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(byte);
    }
    out
}

fn h264_crop_units(chroma_format_idc: u32, frame_mbs_only: bool) -> (u32, u32) {
    let frame_factor = if frame_mbs_only { 1 } else { 2 };
    match chroma_format_idc {
        0 => (1, frame_factor),
        1 => (2, 2 * frame_factor),
        2 => (2, frame_factor),
        _ => (1, frame_factor),
    }
}

fn skip_h264_scaling_list(bits: &mut BitReader<'_>, size: usize) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = bits.read_se()?;
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        last_scale = if next_scale == 0 {
            last_scale
        } else {
            next_scale
        };
    }
    Some(())
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        if count > 32 || self.bit.checked_add(count)? > self.bytes.len() * 8 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.bytes.get(self.bit / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - (self.bit % 8))) & 1);
            self.bit += 1;
        }
        Some(value)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0usize;
        while self.read_bits(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let suffix = if zeros == 0 {
            0
        } else {
            self.read_bits(zeros)?
        };
        Some((1u32 << zeros) - 1 + suffix)
    }

    fn read_se(&mut self) -> Option<i32> {
        let value = self.read_ue()? as i32;
        Some(if value & 1 == 0 {
            -(value / 2)
        } else {
            (value + 1) / 2
        })
    }
}

fn parse_avcc_extension(payload: &[u8]) -> Option<(u8, u8, u8)> {
    let sps_count = (*payload.get(5)? & 0x1F) as usize;
    let mut offset = 6usize;
    for _ in 0..sps_count {
        let len = read_u16_be(payload, offset)? as usize;
        offset = offset.checked_add(2 + len)?;
    }
    let pps_count = *payload.get(offset)? as usize;
    offset += 1;
    for _ in 0..pps_count {
        let len = read_u16_be(payload, offset)? as usize;
        offset = offset.checked_add(2 + len)?;
    }
    let chroma = *payload.get(offset)? & 0x03;
    let luma_bits = (*payload.get(offset + 1)? & 0x07) + 8;
    let chroma_bits = (*payload.get(offset + 2)? & 0x07) + 8;
    Some((chroma, luma_bits, chroma_bits))
}

fn parse_hvcc_detail(payload: &[u8]) -> Option<String> {
    let profile = payload.get(1).map(|value| value & 0x1F)?;
    let level = *payload.get(12)?;
    let chroma = payload.get(16).map(|value| value & 0x03);
    let luma_bits = payload.get(17).map(|value| (value & 0x07) + 8);
    let chroma_bits = payload.get(18).map(|value| (value & 0x07) + 8);
    let nal_length = payload.get(21).map(|value| (value & 0x03) + 1);
    let mut parts = vec![format!(
        "HEVC profile {profile}, level {}.{}",
        level / 30,
        level % 30
    )];
    if let Some(nal_length) = nal_length {
        parts.push(format!("{}-byte NAL length", nal_length));
    }
    if let Some(chroma) = chroma {
        parts.push(format!("chroma {chroma}"));
    }
    if let Some(luma_bits) = luma_bits {
        parts.push(format!("{}-bit luma", luma_bits));
    }
    if let Some(chroma_bits) = chroma_bits {
        parts.push(format!("{}-bit chroma", chroma_bits));
    }
    if let Some(arrays) = parse_hvcc_array_summary(payload) {
        parts.push(arrays);
    }
    if let Some(vps) = parse_hvcc_vps_summary(payload) {
        parts.push(vps);
    }
    if let Some(sps) = parse_hvcc_sps_summary(payload) {
        parts.push(sps);
    }
    Some(parts.join(", "))
}

fn parse_hvcc_sps_summary(payload: &[u8]) -> Option<String> {
    let sps = find_hvcc_nal(payload, 33)?;
    parse_hevc_sps_summary(sps)
}

fn parse_hvcc_vps_summary(payload: &[u8]) -> Option<String> {
    let vps = find_hvcc_nal(payload, 32)?;
    parse_hevc_vps_summary(vps)
}

fn parse_hevc_vps_summary(vps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(vps.get(2..)?);
    let mut bits = BitReader::new(&rbsp);
    let vps_id = bits.read_bits(4)?;
    bits.read_bits(2)?;
    let max_layers_minus1 = bits.read_bits(6)?;
    let max_sub_layers_minus1 = bits.read_bits(3)?.min(7) as usize;
    let temporal_id_nesting = bits.read_bits(1)? != 0;
    bits.read_bits(16)?;
    skip_hevc_profile_tier_level(&mut bits, max_sub_layers_minus1)?;
    Some(format!(
        "VPS id {vps_id}, layers {}, sub-layers {}, temporal nesting {}",
        max_layers_minus1 + 1,
        max_sub_layers_minus1 + 1,
        if temporal_id_nesting { "yes" } else { "no" }
    ))
}

fn find_hvcc_nal(payload: &[u8], target_type: u8) -> Option<&[u8]> {
    let arrays = *payload.get(22)? as usize;
    let mut offset = 23usize;
    for _ in 0..arrays.min(32) {
        let nal_type = *payload.get(offset)? & 0x3F;
        let nal_count = read_u16_be(payload, offset + 1)? as usize;
        offset += 3;
        for _ in 0..nal_count.min(256) {
            let len = read_u16_be(payload, offset)? as usize;
            offset += 2;
            let nal = payload.get(offset..offset.checked_add(len)?)?;
            if nal_type == target_type {
                return Some(nal);
            }
            offset += len;
        }
    }
    None
}

fn parse_hevc_sps_summary(sps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(sps.get(2..)?);
    let mut bits = BitReader::new(&rbsp);
    bits.read_bits(4)?;
    let max_sub_layers_minus1 = bits.read_bits(3)?.min(7) as usize;
    bits.read_bits(1)?;
    skip_hevc_profile_tier_level(&mut bits, max_sub_layers_minus1)?;
    bits.read_ue()?;
    let chroma_format_idc = bits.read_ue()?;
    if chroma_format_idc == 3 {
        bits.read_bits(1)?;
    }
    let coded_width = bits.read_ue()?;
    let coded_height = bits.read_ue()?;
    let mut crop = (0u32, 0u32, 0u32, 0u32);
    if bits.read_bits(1)? != 0 {
        crop = (
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
        );
    }
    let luma_bits = bits.read_ue()?.checked_add(8)?;
    let chroma_bits = bits.read_ue()?.checked_add(8)?;
    let vui_summary = parse_hevc_sps_vui_summary(&mut bits, max_sub_layers_minus1);
    let (crop_x, crop_y) = hevc_crop_units(chroma_format_idc);
    let display_width = coded_width.saturating_sub((crop.0 + crop.1).saturating_mul(crop_x));
    let display_height = coded_height.saturating_sub((crop.2 + crop.3).saturating_mul(crop_y));
    let mut parts = vec![format!("SPS coded {coded_width}x{coded_height}")];
    if display_width != coded_width || display_height != coded_height {
        parts.push(format!("crop display {display_width}x{display_height}"));
    }
    parts.push(format!("chroma {chroma_format_idc}"));
    parts.push(format!("{luma_bits}-bit luma"));
    parts.push(format!("{chroma_bits}-bit chroma"));
    if let Some(vui_summary) = vui_summary {
        parts.push(vui_summary);
    }
    Some(parts.join(", "))
}

fn parse_hevc_sps_vui_summary(
    bits: &mut BitReader<'_>,
    max_sub_layers_minus1: usize,
) -> Option<String> {
    bits.read_ue()?;
    let ordering_info_all_layers = bits.read_bits(1)? == 0;
    let start_layer = if ordering_info_all_layers {
        max_sub_layers_minus1
    } else {
        0
    };
    for _ in start_layer..=max_sub_layers_minus1 {
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_ue()?;
    }
    for _ in 0..6 {
        bits.read_ue()?;
    }
    if bits.read_bits(1)? != 0 {
        return None;
    }
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    if bits.read_bits(1)? != 0 {
        return None;
    }
    if bits.read_ue()? != 0 {
        return None;
    }
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    if bits.read_bits(1)? == 0 {
        return None;
    }
    parse_hevc_vui_summary(bits)
}

fn parse_hevc_vui_summary(bits: &mut BitReader<'_>) -> Option<String> {
    if bits.read_bits(1)? != 0 {
        let aspect_ratio_idc = bits.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            bits.read_bits(16)?;
            bits.read_bits(16)?;
        }
    }
    if bits.read_bits(1)? != 0 {
        bits.read_bits(1)?;
    }
    let mut parts = vec!["VUI".to_string()];
    if bits.read_bits(1)? != 0 {
        let video_format = bits.read_bits(3)?;
        let full_range = bits.read_bits(1)? != 0;
        parts.push(format!("video format {video_format}"));
        if full_range {
            parts.push("full range".to_string());
        }
        if bits.read_bits(1)? != 0 {
            let primaries = bits.read_bits(8)? as u8;
            let transfer = bits.read_bits(8)? as u8;
            let matrix = bits.read_bits(8)? as u8;
            parts.push(format!(
                "primaries {}",
                h264_color_primaries_name(primaries)
            ));
            parts.push(format!("transfer {}", h264_transfer_name(transfer)));
            parts.push(format!("matrix {}", h264_matrix_name(matrix)));
        }
    }
    Some(parts.join(", "))
}

fn skip_hevc_profile_tier_level(
    bits: &mut BitReader<'_>,
    max_sub_layers_minus1: usize,
) -> Option<()> {
    bits.read_bits(2)?;
    bits.read_bits(1)?;
    bits.read_bits(5)?;
    bits.read_bits(32)?;
    bits.read_bits(32)?;
    bits.read_bits(16)?;
    bits.read_bits(8)?;
    let mut sub_layer_profile_present = [false; 8];
    let mut sub_layer_level_present = [false; 8];
    for index in 0..max_sub_layers_minus1 {
        sub_layer_profile_present[index] = bits.read_bits(1)? != 0;
        sub_layer_level_present[index] = bits.read_bits(1)? != 0;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            bits.read_bits(2)?;
        }
    }
    for index in 0..max_sub_layers_minus1 {
        if sub_layer_profile_present[index] {
            bits.read_bits(2)?;
            bits.read_bits(1)?;
            bits.read_bits(5)?;
            bits.read_bits(32)?;
            bits.read_bits(32)?;
            bits.read_bits(16)?;
        }
        if sub_layer_level_present[index] {
            bits.read_bits(8)?;
        }
    }
    Some(())
}

fn hevc_crop_units(chroma_format_idc: u32) -> (u32, u32) {
    match chroma_format_idc {
        1 => (2, 2),
        2 => (2, 1),
        3 => (1, 1),
        _ => (1, 1),
    }
}

fn parse_hvcc_array_summary(payload: &[u8]) -> Option<String> {
    let arrays = *payload.get(22)? as usize;
    let mut offset = 23usize;
    let mut vps = 0u32;
    let mut sps = 0u32;
    let mut pps = 0u32;
    for _ in 0..arrays.min(32) {
        let nal_type = *payload.get(offset)? & 0x3F;
        let nal_count = read_u16_be(payload, offset + 1)? as usize;
        offset += 3;
        match nal_type {
            32 => vps = vps.saturating_add(nal_count as u32),
            33 => sps = sps.saturating_add(nal_count as u32),
            34 => pps = pps.saturating_add(nal_count as u32),
            _ => {}
        }
        for _ in 0..nal_count.min(256) {
            let len = read_u16_be(payload, offset)? as usize;
            offset = offset.checked_add(2 + len)?;
            if offset > payload.len() {
                return None;
            }
        }
    }
    let mut parts = Vec::new();
    if vps > 0 {
        parts.push(format!("VPS {vps}"));
    }
    if sps > 0 {
        parts.push(format!("SPS {sps}"));
    }
    if pps > 0 {
        parts.push(format!("PPS {pps}"));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn parse_stsz_total_bytes(payload: &[u8]) -> Option<u64> {
    let sample_size = read_u32_be(payload, 4)? as u64;
    let sample_count = read_u32_be(payload, 8)? as u64;
    if sample_size > 0 {
        return sample_size.checked_mul(sample_count);
    }
    let count = sample_count.min(1_000_000) as usize;
    let mut offset = 12usize;
    let mut total = 0u64;
    for _ in 0..count {
        total = total.checked_add(read_u32_be(payload, offset)? as u64)?;
        offset = offset.checked_add(4)?;
    }
    Some(total)
}

fn parse_mp4_entry_count(payload: &[u8]) -> Option<u32> {
    read_u32_be(payload, 4)
}

struct Mp4SttsTimeline {
    samples: u64,
    decode_ticks: u64,
    first_delta: Option<u32>,
}

struct Mp4CttsSummary {
    samples: u64,
    first_offset: Option<i64>,
    offset_range: Option<(i64, i64)>,
}

struct Mp4ElstSummary {
    first_duration: Option<u64>,
    first_media_time: Option<i64>,
    first_rate: Option<f64>,
}

fn parse_elst_summary(payload: &[u8]) -> Option<Mp4ElstSummary> {
    let version = *payload.first()?;
    let entries = read_u32_be(payload, 4)?.min(100_000) as usize;
    if entries == 0 {
        return Some(Mp4ElstSummary {
            first_duration: None,
            first_media_time: None,
            first_rate: None,
        });
    }
    let offset = 8usize;
    let (duration, media_time, rate_offset) = if version == 1 {
        (
            read_u64_be(payload, offset)?,
            read_i64_be(payload, offset + 8)?,
            offset + 16,
        )
    } else {
        (
            read_u32_be(payload, offset)? as u64,
            read_i32_be(payload, offset + 4)? as i64,
            offset + 8,
        )
    };
    let rate_integer = read_i16_be(payload, rate_offset)? as f64;
    let rate_fraction = read_u16_be(payload, rate_offset + 2)? as f64 / 65536.0;
    Some(Mp4ElstSummary {
        first_duration: Some(duration),
        first_media_time: Some(media_time),
        first_rate: Some(rate_integer + rate_fraction),
    })
}

fn parse_ctts_summary(payload: &[u8]) -> Option<Mp4CttsSummary> {
    let version = *payload.first()?;
    let entries = read_u32_be(payload, 4)?.min(100_000) as usize;
    let mut offset = 8usize;
    let mut samples = 0u64;
    let mut first_offset = None;
    let mut min_offset = i64::MAX;
    let mut max_offset = i64::MIN;
    for _ in 0..entries {
        let sample_count = read_u32_be(payload, offset)? as u64;
        let composition_offset = if version == 1 {
            read_i32_be(payload, offset + 4)? as i64
        } else {
            read_u32_be(payload, offset + 4)? as i64
        };
        if first_offset.is_none() {
            first_offset = Some(composition_offset);
        }
        min_offset = min_offset.min(composition_offset);
        max_offset = max_offset.max(composition_offset);
        samples = samples.checked_add(sample_count)?;
        offset = offset.checked_add(8)?;
    }
    Some(Mp4CttsSummary {
        samples,
        first_offset,
        offset_range: first_offset.map(|_| (min_offset, max_offset)),
    })
}

fn parse_stts_timeline(payload: &[u8]) -> Option<Mp4SttsTimeline> {
    let entries = read_u32_be(payload, 4)?.min(100_000) as usize;
    let mut offset = 8usize;
    let mut samples = 0u64;
    let mut decode_ticks = 0u64;
    let mut first_delta = None;
    for _ in 0..entries {
        let sample_count = read_u32_be(payload, offset)? as u64;
        let sample_delta = read_u32_be(payload, offset + 4)?;
        if first_delta.is_none() {
            first_delta = Some(sample_delta);
        }
        samples = samples.checked_add(sample_count)?;
        decode_ticks = decode_ticks.checked_add(sample_count.checked_mul(sample_delta as u64)?)?;
        offset = offset.checked_add(8)?;
    }
    Some(Mp4SttsTimeline {
        samples,
        decode_ticks,
        first_delta,
    })
}

struct Mp4ChunkSummary {
    chunks: u32,
    first_offset: u64,
    last_end: u64,
    data_bytes: u64,
    first_chunk_samples: Option<u32>,
    first_chunk_bytes: Option<u64>,
    first_sample_size: Option<u32>,
    chunk_details: Vec<String>,
}

fn parse_mp4_chunk_summary(trak: &[u8]) -> Option<Mp4ChunkSummary> {
    let chunk_offsets = find_mp4_atom_payload(trak, b"co64")
        .and_then(parse_co64_offsets)
        .or_else(|| find_mp4_atom_payload(trak, b"stco").and_then(parse_stco_offsets))?;
    let sample_sizes = find_mp4_atom_payload(trak, b"stsz").and_then(parse_stsz_sample_sizes)?;
    let sample_to_chunks = find_mp4_atom_payload(trak, b"stsc").and_then(parse_stsc_entries)?;
    if chunk_offsets.is_empty() || sample_sizes.is_empty() || sample_to_chunks.is_empty() {
        return None;
    }

    let mut sample_index = 0usize;
    let mut data_bytes = 0u64;
    let mut last_end = 0u64;
    let mut first_chunk_samples = None;
    let mut first_chunk_bytes = None;
    let mut chunk_details = Vec::new();
    for (chunk_index, chunk_offset) in chunk_offsets.iter().enumerate() {
        let samples_per_chunk =
            samples_per_chunk_for_chunk(&sample_to_chunks, (chunk_index + 1) as u32)? as usize;
        let mut chunk_bytes = 0u64;
        for _ in 0..samples_per_chunk {
            let Some(size) = sample_sizes.get(sample_index) else {
                break;
            };
            chunk_bytes = chunk_bytes.checked_add(*size as u64)?;
            sample_index += 1;
        }
        if chunk_index == 0 {
            first_chunk_samples = Some(samples_per_chunk as u32);
            first_chunk_bytes = Some(chunk_bytes);
        }
        if chunk_details.len() < 4 {
            chunk_details.push(format!(
                "#{} @0x{:X} {} samples {} bytes",
                chunk_index + 1,
                chunk_offset,
                samples_per_chunk,
                chunk_bytes
            ));
        }
        data_bytes = data_bytes.checked_add(chunk_bytes)?;
        last_end = last_end.max(chunk_offset.saturating_add(chunk_bytes));
        if sample_index >= sample_sizes.len() {
            break;
        }
    }

    Some(Mp4ChunkSummary {
        chunks: chunk_offsets.len() as u32,
        first_offset: *chunk_offsets.first()?,
        last_end,
        data_bytes,
        first_chunk_samples,
        first_chunk_bytes,
        first_sample_size: sample_sizes.first().copied(),
        chunk_details,
    })
}

fn parse_stco_offsets(payload: &[u8]) -> Option<Vec<u64>> {
    let count = read_u32_be(payload, 4)?.min(1_000_000) as usize;
    let mut offsets = Vec::with_capacity(count.min(1024));
    let mut offset = 8usize;
    for _ in 0..count {
        offsets.push(read_u32_be(payload, offset)? as u64);
        offset += 4;
    }
    Some(offsets)
}

fn parse_co64_offsets(payload: &[u8]) -> Option<Vec<u64>> {
    let count = read_u32_be(payload, 4)?.min(1_000_000) as usize;
    let mut offsets = Vec::with_capacity(count.min(1024));
    let mut offset = 8usize;
    for _ in 0..count {
        offsets.push(read_u64_be(payload, offset)?);
        offset += 8;
    }
    Some(offsets)
}

fn parse_stsz_sample_sizes(payload: &[u8]) -> Option<Vec<u32>> {
    let sample_size = read_u32_be(payload, 4)?;
    let sample_count = read_u32_be(payload, 8)?.min(1_000_000) as usize;
    if sample_size > 0 {
        return Some(vec![sample_size; sample_count]);
    }
    let mut sizes = Vec::with_capacity(sample_count.min(1024));
    let mut offset = 12usize;
    for _ in 0..sample_count {
        sizes.push(read_u32_be(payload, offset)?);
        offset += 4;
    }
    Some(sizes)
}

fn parse_stsc_entries(payload: &[u8]) -> Option<Vec<(u32, u32)>> {
    let count = read_u32_be(payload, 4)?.min(1_000_000) as usize;
    let mut entries = Vec::with_capacity(count.min(1024));
    let mut offset = 8usize;
    for _ in 0..count {
        let first_chunk = read_u32_be(payload, offset)?;
        let samples_per_chunk = read_u32_be(payload, offset + 4)?;
        entries.push((first_chunk, samples_per_chunk));
        offset += 12;
    }
    Some(entries)
}

fn samples_per_chunk_for_chunk(entries: &[(u32, u32)], chunk: u32) -> Option<u32> {
    let mut current = None;
    for (first_chunk, samples_per_chunk) in entries {
        if chunk < *first_chunk {
            break;
        }
        current = Some(*samples_per_chunk);
    }
    current.filter(|value| *value > 0)
}

fn parse_mvhd_created_unix(payload: &[u8]) -> Option<i64> {
    let version = *payload.first()?;
    let mac_time = match version {
        0 => read_u32_be(payload, 4)? as u64,
        1 => read_u64_be(payload, 4)?,
        _ => return None,
    };
    mp4_time_to_unix(mac_time)
}

fn mp4_time_to_unix(mac_time: u64) -> Option<i64> {
    const MP4_TO_UNIX_SECONDS: u64 = 2_082_844_800;
    (mac_time >= MP4_TO_UNIX_SECONDS).then_some((mac_time - MP4_TO_UNIX_SECONDS) as i64)
}

fn mp4_rotation_degrees(bytes: &[u8]) -> Option<i32> {
    let mut rotations = Vec::new();
    collect_mp4_atom_payloads(bytes, b"tkhd", &mut rotations);
    rotations
        .into_iter()
        .filter_map(parse_tkhd_rotation_degrees)
        .find(|degrees| *degrees != 0)
}

fn parse_tkhd_rotation_degrees(payload: &[u8]) -> Option<i32> {
    let version = *payload.first()?;
    let matrix_offset = match version {
        0 => 40,
        1 => 52,
        _ => return None,
    };
    let a = read_i32_be(payload, matrix_offset)? as f64 / 65_536.0;
    let b = read_i32_be(payload, matrix_offset + 4)? as f64 / 65_536.0;
    let mut degrees = b.atan2(a).to_degrees().round() as i32;
    degrees = ((degrees % 360) + 360) % 360;
    Some(degrees)
}

fn estimate_bitrate(size: i64, duration_seconds: f64) -> Option<f64> {
    (size > 0 && duration_seconds > 0.0).then(|| size as f64 * 8.0 / duration_seconds)
}

fn format_bitrate(bits_per_second: f64) -> String {
    if bits_per_second >= 1_000_000.0 {
        format!("{:.2} Mbps", bits_per_second / 1_000_000.0)
    } else if bits_per_second >= 1_000.0 {
        format!("{:.0} kbps", bits_per_second / 1_000.0)
    } else {
        format!("{:.0} bps", bits_per_second)
    }
}

fn format_rotation(degrees: i32) -> String {
    format!("{degrees}°")
}

struct WavSummary {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    bits_per_sample: u16,
    data_bytes: u32,
}

fn append_wav_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_wav_summary(bytes) else {
        return;
    };
    text.push_str(&format!(
        "\nAudio format: {}",
        wav_audio_format_name(summary.audio_format)
    ));
    text.push_str(&format!("\nChannels: {}", summary.channels));
    text.push_str(&format!(
        "\nSample rate: {} Hz",
        format_number(summary.sample_rate as i64)
    ));
    if summary.bits_per_sample > 0 {
        text.push_str(&format!("\nBits per sample: {}", summary.bits_per_sample));
    }
    if summary.byte_rate > 0 && summary.data_bytes > 0 {
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(summary.data_bytes as f64 / summary.byte_rate as f64)
        ));
    }
}

fn parse_wav_summary(bytes: &[u8]) -> Option<WavSummary> {
    if bytes.len() < 12 || bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return None;
    }
    let mut offset = 12usize;
    let mut format: Option<WavSummary> = None;
    let mut data_bytes = 0u32;
    while offset.checked_add(8)? <= bytes.len() {
        let chunk_id = bytes.get(offset..offset + 4)?;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        let payload = offset + 8;
        let next = payload.checked_add(chunk_size + (chunk_size % 2))?;
        if payload.checked_add(chunk_size)? > bytes.len() && chunk_id != b"data" {
            break;
        }
        if chunk_id == b"fmt " && chunk_size >= 16 {
            format = Some(WavSummary {
                audio_format: read_u16(bytes, payload)?,
                channels: read_u16(bytes, payload + 2)?,
                sample_rate: read_u32(bytes, payload + 4)?,
                byte_rate: read_u32(bytes, payload + 8)?,
                bits_per_sample: read_u16(bytes, payload + 14).unwrap_or(0),
                data_bytes,
            });
        } else if chunk_id == b"data" {
            data_bytes = chunk_size as u32;
            if let Some(summary) = format.as_mut() {
                summary.data_bytes = data_bytes;
            }
        }
        offset = next;
    }
    format.map(|mut summary| {
        summary.data_bytes = data_bytes;
        summary
    })
}

fn wav_audio_format_name(value: u16) -> String {
    match value {
        1 => "PCM".to_string(),
        3 => "IEEE float".to_string(),
        6 => "A-law".to_string(),
        7 => "mu-law".to_string(),
        0xFFFE => "extensible".to_string(),
        _ => format!("0x{value:04X}"),
    }
}

struct FlacSummary {
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    total_samples: u64,
}

fn append_flac_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_flac_summary(bytes) else {
        return;
    };
    if summary.channels > 0 {
        text.push_str(&format!("\nChannels: {}", summary.channels));
    }
    if summary.sample_rate > 0 {
        text.push_str(&format!(
            "\nSample rate: {} Hz",
            format_number(summary.sample_rate as i64)
        ));
    }
    if summary.bits_per_sample > 0 {
        text.push_str(&format!("\nBits per sample: {}", summary.bits_per_sample));
    }
    if summary.sample_rate > 0 && summary.total_samples > 0 {
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(summary.total_samples as f64 / summary.sample_rate as f64)
        ));
    }
}

fn parse_flac_summary(bytes: &[u8]) -> Option<FlacSummary> {
    if bytes.len() < 42 || bytes.get(0..4) != Some(b"fLaC") {
        return None;
    }
    let mut offset = 4usize;
    while offset.checked_add(4)? <= bytes.len() {
        let block_type = bytes[offset] & 0x7F;
        let block_len = ((bytes[offset + 1] as usize) << 16)
            | ((bytes[offset + 2] as usize) << 8)
            | bytes[offset + 3] as usize;
        let payload = offset + 4;
        if payload.checked_add(block_len)? > bytes.len() {
            return None;
        }
        if block_type == 0 && block_len >= 34 {
            let stream = bytes.get(payload..payload + 34)?;
            let sample_rate = ((stream[10] as u32) << 12)
                | ((stream[11] as u32) << 4)
                | ((stream[12] as u32) >> 4);
            let channels = ((stream[12] >> 1) & 0x07) + 1;
            let bits_per_sample = (((stream[12] & 0x01) << 4) | (stream[13] >> 4)) + 1;
            let total_samples = ((stream[13] as u64 & 0x0F) << 32)
                | ((stream[14] as u64) << 24)
                | ((stream[15] as u64) << 16)
                | ((stream[16] as u64) << 8)
                | stream[17] as u64;
            return Some(FlacSummary {
                sample_rate,
                channels,
                bits_per_sample,
                total_samples,
            });
        }
        offset = payload + block_len;
    }
    None
}

#[derive(Default)]
struct OggSummary {
    codec: String,
    channels: u8,
    sample_rate: u32,
    vendor: String,
    comments: u32,
}

fn append_ogg_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_ogg_summary(bytes) else {
        return;
    };
    if !summary.codec.is_empty() {
        text.push_str(&format!("\nAudio codec: {}", summary.codec));
    }
    if summary.channels > 0 {
        text.push_str(&format!("\nChannels: {}", summary.channels));
    }
    if summary.sample_rate > 0 {
        text.push_str(&format!(
            "\nSample rate: {} Hz",
            format_number(summary.sample_rate as i64)
        ));
    }
    if !summary.vendor.is_empty() {
        text.push_str(&format!("\nVendor: {}", summary.vendor));
    }
    if summary.comments > 0 {
        text.push_str(&format!("\nTags: {}", summary.comments));
    }
}

fn parse_ogg_summary(bytes: &[u8]) -> Option<OggSummary> {
    let packets = read_ogg_packets(bytes, 8);
    if packets.is_empty() {
        return None;
    }

    let mut summary = OggSummary::default();
    for packet in packets {
        if packet.starts_with(b"OpusHead") && packet.len() >= 19 {
            summary.codec = "Opus".to_string();
            summary.channels = packet[9];
            summary.sample_rate = read_u32(&packet, 12).unwrap_or(48_000);
        } else if packet.starts_with(b"OpusTags") && packet.len() >= 16 {
            parse_ogg_comment_packet(&packet, 8, &mut summary);
        } else if packet.starts_with(b"\x01vorbis") && packet.len() >= 30 {
            summary.codec = "Vorbis".to_string();
            summary.channels = packet[11];
            summary.sample_rate = read_u32(&packet, 12).unwrap_or(0);
        } else if packet.starts_with(b"\x03vorbis") && packet.len() >= 11 {
            parse_ogg_comment_packet(&packet, 7, &mut summary);
        }
    }

    (!summary.codec.is_empty() || !summary.vendor.is_empty()).then_some(summary)
}

fn parse_ogg_comment_packet(packet: &[u8], offset: usize, summary: &mut OggSummary) {
    let Some(vendor_len) = read_u32(packet, offset).map(|value| value as usize) else {
        return;
    };
    let vendor_start = offset + 4;
    let Some(vendor_end) = vendor_start.checked_add(vendor_len) else {
        return;
    };
    let Some(vendor) = packet.get(vendor_start..vendor_end) else {
        return;
    };
    if summary.vendor.is_empty() {
        summary.vendor = String::from_utf8_lossy(vendor)
            .trim_matches('\0')
            .trim()
            .chars()
            .take(128)
            .collect();
    }
    summary.comments = read_u32(packet, vendor_end).unwrap_or(0);
}

fn read_ogg_packets(bytes: &[u8], max_packets: usize) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut current = Vec::new();
    let mut offset = 0usize;
    while offset.checked_add(27).is_some_and(|end| end <= bytes.len())
        && packets.len() < max_packets
    {
        if bytes.get(offset..offset + 4) != Some(b"OggS") {
            break;
        }
        let segments = bytes[offset + 26] as usize;
        let lacing_start = offset + 27;
        let payload_start = lacing_start + segments;
        if payload_start > bytes.len() {
            break;
        }
        let payload_len: usize = bytes[lacing_start..payload_start]
            .iter()
            .map(|value| *value as usize)
            .sum();
        let Some(payload_end) = payload_start.checked_add(payload_len) else {
            break;
        };
        if payload_end > bytes.len() {
            break;
        }
        let mut payload_offset = payload_start;
        for segment_len in bytes[lacing_start..payload_start].iter().copied() {
            let segment_len = segment_len as usize;
            let segment_end = payload_offset + segment_len;
            current.extend_from_slice(bytes.get(payload_offset..segment_end).unwrap_or_default());
            payload_offset = segment_end;
            if segment_len < 255 {
                packets.push(std::mem::take(&mut current));
                if packets.len() >= max_packets {
                    break;
                }
            }
        }
        offset = payload_end;
    }
    packets
}

#[derive(Default)]
struct MkvSummary {
    timecode_scale: u64,
    duration: Option<f64>,
    muxing_app: String,
    writing_app: String,
    video_codec: String,
    audio_codec: String,
    width: u64,
    height: u64,
    audio_channels: u64,
    sample_rate: Option<f64>,
    tracks: u32,
}

fn append_mkv_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_mkv_summary(bytes) else {
        return;
    };
    if let Some(duration) = summary.duration {
        let scale = if summary.timecode_scale > 0 {
            summary.timecode_scale as f64
        } else {
            1_000_000.0
        };
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(duration * scale / 1_000_000_000.0)
        ));
    }
    if summary.tracks > 0 {
        text.push_str(&format!("\nTracks: {}", summary.tracks));
    }
    if summary.width > 0 && summary.height > 0 {
        text.push_str(&format!("\nVideo: {}x{}", summary.width, summary.height));
    }
    if !summary.video_codec.is_empty() {
        text.push_str(&format!(
            "\nVideo codec: {}",
            media_codec_label(&summary.video_codec)
        ));
    }
    if summary.audio_channels > 0 {
        text.push_str(&format!("\nAudio channels: {}", summary.audio_channels));
    }
    if let Some(rate) = summary.sample_rate {
        text.push_str(&format!(
            "\nAudio sample rate: {} Hz",
            format_number(rate.round() as i64)
        ));
    }
    if !summary.audio_codec.is_empty() {
        text.push_str(&format!(
            "\nAudio codec: {}",
            media_codec_label(&summary.audio_codec)
        ));
    }
    if !summary.writing_app.is_empty() {
        text.push_str(&format!("\nWriting app: {}", summary.writing_app));
    }
    if !summary.muxing_app.is_empty() {
        text.push_str(&format!("\nMuxing app: {}", summary.muxing_app));
    }
}

fn media_codec_label(codec: &str) -> String {
    match codec {
        "V_MPEG4/ISO/AVC" => "H.264 / AVC".to_string(),
        "V_MPEGH/ISO/HEVC" => "H.265 / HEVC".to_string(),
        "V_AV1" => "AV1".to_string(),
        "V_VP8" => "VP8".to_string(),
        "V_VP9" => "VP9".to_string(),
        "A_AAC" | "A_AAC/MPEG2/LC" | "A_AAC/MPEG4/LC" => "AAC".to_string(),
        "A_AC3" => "AC-3".to_string(),
        "A_EAC3" => "E-AC-3".to_string(),
        "A_FLAC" => "FLAC".to_string(),
        "A_OPUS" => "Opus".to_string(),
        "A_VORBIS" => "Vorbis".to_string(),
        "A_PCM/INT/LIT" => "PCM".to_string(),
        _ => codec.to_string(),
    }
}

fn parse_mkv_summary(bytes: &[u8]) -> Option<MkvSummary> {
    if !bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return None;
    }
    let mut summary = MkvSummary::default();
    parse_mkv_elements(bytes, 0, bytes.len(), 0, &mut summary);
    (summary.duration.is_some()
        || summary.tracks > 0
        || !summary.writing_app.is_empty()
        || !summary.muxing_app.is_empty())
    .then_some(summary)
}

fn parse_mkv_elements(
    bytes: &[u8],
    mut offset: usize,
    end: usize,
    depth: usize,
    summary: &mut MkvSummary,
) {
    if depth > 6 {
        return;
    }
    while offset < end && offset < bytes.len() {
        let Some((id, id_next)) = read_ebml_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_ebml_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x1549A966 => parse_mkv_info(bytes, payload, payload_end, summary),
            0x1654AE6B => parse_mkv_elements(bytes, payload, payload_end, depth + 1, summary),
            0xAE => parse_mkv_track_entry(bytes, payload, payload_end, summary),
            0x18538067 | 0x1A45DFA3 => {
                parse_mkv_elements(bytes, payload, payload_end, depth + 1, summary)
            }
            _ => {}
        }
        if payload_end <= offset {
            break;
        }
        offset = payload_end;
    }
}

fn parse_mkv_info(bytes: &[u8], mut offset: usize, end: usize, summary: &mut MkvSummary) {
    while offset < end {
        let Some((id, id_next)) = read_ebml_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_ebml_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x2AD7B1 => {
                summary.timecode_scale =
                    read_ebml_uint(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x4489 => {
                summary.duration =
                    read_ebml_float(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x4D80 => {
                summary.muxing_app =
                    read_ebml_string(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x5741 => {
                summary.writing_app =
                    read_ebml_string(bytes.get(payload..payload_end).unwrap_or_default())
            }
            _ => {}
        }
        offset = payload_end;
    }
}

fn parse_mkv_track_entry(bytes: &[u8], mut offset: usize, end: usize, summary: &mut MkvSummary) {
    summary.tracks = summary.tracks.saturating_add(1);
    let mut track_type = 0u64;
    let mut codec = String::new();
    let mut width = 0u64;
    let mut height = 0u64;
    let mut channels = 0u64;
    let mut sample_rate = None;
    while offset < end {
        let Some((id, id_next)) = read_ebml_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_ebml_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x83 => {
                track_type = read_ebml_uint(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x86 => codec = read_ebml_string(bytes.get(payload..payload_end).unwrap_or_default()),
            0xE0 => (width, height) = parse_mkv_video(bytes, payload, payload_end),
            0xE1 => (channels, sample_rate) = parse_mkv_audio(bytes, payload, payload_end),
            _ => {}
        }
        offset = payload_end;
    }
    match track_type {
        1 => {
            if summary.video_codec.is_empty() {
                summary.video_codec = codec;
            }
            if summary.width == 0 {
                summary.width = width;
                summary.height = height;
            }
        }
        2 => {
            if summary.audio_codec.is_empty() {
                summary.audio_codec = codec;
            }
            if summary.audio_channels == 0 {
                summary.audio_channels = channels;
            }
            if summary.sample_rate.is_none() {
                summary.sample_rate = sample_rate;
            }
        }
        _ => {}
    }
}

fn parse_mkv_video(bytes: &[u8], mut offset: usize, end: usize) -> (u64, u64) {
    let mut width = 0;
    let mut height = 0;
    while offset < end {
        let Some((id, id_next)) = read_ebml_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_ebml_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0xB0 => width = read_ebml_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            0xBA => height = read_ebml_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            _ => {}
        }
        offset = payload_end;
    }
    (width, height)
}

fn parse_mkv_audio(bytes: &[u8], mut offset: usize, end: usize) -> (u64, Option<f64>) {
    let mut channels = 0;
    let mut sample_rate = None;
    while offset < end {
        let Some((id, id_next)) = read_ebml_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_ebml_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x9F => channels = read_ebml_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            0xB5 => {
                sample_rate = read_ebml_float(bytes.get(payload..payload_end).unwrap_or_default())
            }
            _ => {}
        }
        offset = payload_end;
    }
    (channels, sample_rate)
}

fn read_ebml_id(bytes: &[u8], offset: usize) -> Option<(u64, usize)> {
    let first = *bytes.get(offset)?;
    let len = (0..4).find(|bit| first & (0x80 >> bit) != 0)? + 1;
    let mut value = 0u64;
    for index in 0..len {
        value = (value << 8) | *bytes.get(offset + index)? as u64;
    }
    Some((value, offset + len))
}

fn read_ebml_size(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(offset)?;
    let len = (0..8).find(|bit| first & (0x80 >> bit) != 0)? + 1;
    let mut value = (first & !(0x80 >> (len - 1))) as u64;
    for index in 1..len {
        value = (value << 8) | *bytes.get(offset + index)? as u64;
    }
    (value <= usize::MAX as u64).then_some((value as usize, offset + len))
}

fn read_ebml_uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .take(8)
        .fold(0u64, |value, byte| (value << 8) | *byte as u64)
}

fn read_ebml_float(bytes: &[u8]) -> Option<f64> {
    match bytes.len() {
        4 => Some(f32::from_be_bytes(bytes.try_into().ok()?) as f64),
        8 => Some(f64::from_be_bytes(bytes.try_into().ok()?)),
        _ => None,
    }
}

fn read_ebml_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn append_id3_metadata(text: &mut String, bytes: &[u8]) {
    let fields = parse_id3_text_fields(bytes);
    for (label, key) in [
        ("Title", "TIT2"),
        ("Artist", "TPE1"),
        ("Album", "TALB"),
        ("Track", "TRCK"),
        ("Year", "TDRC"),
        ("Year", "TYER"),
        ("Genre", "TCON"),
        ("Comment", "COMM"),
    ] {
        if let Some(value) = fields.get(key).filter(|value| !value.is_empty()) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
}

fn parse_id3_text_fields(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    if bytes.len() < 10 || bytes.get(0..3) != Some(b"ID3") {
        return fields;
    }
    let version = bytes[3];
    if !(2..=4).contains(&version) {
        return fields;
    }
    let Some(tag_size) = read_id3_synchsafe(bytes, 6) else {
        return fields;
    };
    let tag_end = 10usize.saturating_add(tag_size).min(bytes.len());
    let mut offset = 10usize;
    while offset + 10 <= tag_end {
        let Some(frame_id) = bytes.get(offset..offset + 4) else {
            break;
        };
        if frame_id.iter().all(|b| *b == 0) {
            break;
        }
        if !frame_id
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            break;
        }
        let frame_size = if version == 4 {
            read_id3_synchsafe(bytes, offset + 4)
        } else {
            read_u32_be(bytes, offset + 4).map(|value| value as usize)
        };
        let Some(frame_size) = frame_size else {
            break;
        };
        let frame_start = offset + 10;
        let Some(frame_end) = frame_start.checked_add(frame_size) else {
            break;
        };
        if frame_size == 0 || frame_end > tag_end {
            break;
        }
        let id = String::from_utf8_lossy(frame_id).to_string();
        if matches!(
            id.as_str(),
            "TIT2" | "TPE1" | "TALB" | "TRCK" | "TDRC" | "TYER" | "TCON"
        ) {
            if let Some(value) = decode_id3_text_frame(&bytes[frame_start..frame_end]) {
                fields.entry(id).or_insert(value);
            }
        } else if id == "COMM" {
            if let Some(value) = decode_id3_comment_frame(&bytes[frame_start..frame_end]) {
                fields.entry(id).or_insert(value);
            }
        }
        offset = frame_end;
    }
    fields
}

fn read_id3_synchsafe(bytes: &[u8], offset: usize) -> Option<usize> {
    let chunk = bytes.get(offset..offset + 4)?;
    if chunk.iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    Some(
        ((chunk[0] as usize) << 21)
            | ((chunk[1] as usize) << 14)
            | ((chunk[2] as usize) << 7)
            | chunk[3] as usize,
    )
}

fn decode_id3_text_frame(bytes: &[u8]) -> Option<String> {
    let (&encoding, payload) = bytes.split_first()?;
    let value = decode_id3_text_payload(encoding, payload);
    (!value.is_empty()).then_some(value)
}

fn decode_id3_comment_frame(bytes: &[u8]) -> Option<String> {
    let (&encoding, rest) = bytes.split_first()?;
    let payload = rest.get(3..).unwrap_or_default();
    let comment = if encoding == 1 || encoding == 2 {
        let content = strip_id3_utf16_description(payload, encoding == 2);
        decode_id3_text_payload(encoding, content)
    } else {
        let content = payload
            .iter()
            .position(|b| *b == 0)
            .and_then(|index| payload.get(index + 1..))
            .unwrap_or(payload);
        decode_id3_text_payload(encoding, content)
    };
    (!comment.is_empty()).then_some(comment)
}

fn strip_id3_utf16_description(bytes: &[u8], big_endian_without_bom: bool) -> &[u8] {
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == 0 && bytes[index + 1] == 0 {
            return bytes.get(index + 2..).unwrap_or_default();
        }
        index += 2;
    }
    if big_endian_without_bom {
        bytes
    } else {
        bytes
    }
}

fn decode_id3_text_payload(encoding: u8, bytes: &[u8]) -> String {
    let raw = trim_id3_text_bytes(bytes);
    match encoding {
        1 => decode_id3_utf16(raw),
        2 => decode_id3_utf16_be(raw),
        3 => String::from_utf8_lossy(raw).trim().to_string(),
        _ => decode_latin1(raw).trim().to_string(),
    }
}

fn trim_id3_text_bytes(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

fn decode_id3_utf16(bytes: &[u8]) -> String {
    let (be, payload) = if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, &bytes[2..])
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, &bytes[2..])
    } else {
        (false, bytes)
    };
    if be {
        decode_id3_utf16_be(payload)
    } else {
        let units = payload
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .trim()
            .to_string()
    }
}

fn decode_id3_utf16_be(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

fn find_mp4_atom_payload<'a>(bytes: &'a [u8], atom: &[u8; 4]) -> Option<&'a [u8]> {
    find_mp4_atom_payload_in_range(bytes, 0, bytes.len(), atom, 0)
}

fn collect_mp4_atom_payloads<'a>(bytes: &'a [u8], atom: &[u8; 4], found: &mut Vec<&'a [u8]>) {
    collect_mp4_atom_payloads_in_range(bytes, 0, bytes.len(), atom, 0, found);
}

fn collect_mp4_atom_payloads_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
    found: &mut Vec<&'a [u8]>,
) {
    if depth > 4 || start >= end || end > bytes.len() {
        return;
    }
    let mut pos = start;
    while pos + 8 <= end {
        let Some(size32) = read_u32_be(bytes, pos).map(|size| size as usize) else {
            return;
        };
        let Some(typ) = bytes.get(pos + 4..pos + 8) else {
            return;
        };
        let (header, atom_end) = if size32 == 1 {
            let Some(size64) = read_u64_be(bytes, pos + 8).map(|size| size as usize) else {
                return;
            };
            let Some(end) = pos.checked_add(size64) else {
                return;
            };
            (16usize, end)
        } else if size32 == 0 {
            (8usize, end)
        } else {
            let Some(end) = pos.checked_add(size32) else {
                return;
            };
            (8usize, end)
        };
        if atom_end > end || atom_end <= pos + header {
            return;
        }
        let payload_start = pos + header;
        if typ == atom {
            if let Some(payload) = bytes.get(payload_start..atom_end) {
                found.push(payload);
            }
        }
        if is_mp4_container_atom(typ) {
            collect_mp4_atom_payloads_in_range(
                bytes,
                payload_start,
                atom_end,
                atom,
                depth + 1,
                found,
            );
        }
        pos = atom_end;
    }
}

fn find_mp4_atom_payload_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
) -> Option<&'a [u8]> {
    if depth > 4 || start >= end || end > bytes.len() {
        return None;
    }
    let mut pos = start;
    while pos + 8 <= end {
        let size32 = read_u32_be(bytes, pos)? as usize;
        let typ = bytes.get(pos + 4..pos + 8)?;
        let (header, atom_end) = if size32 == 1 {
            let size64 = read_u64_be(bytes, pos + 8)? as usize;
            (16usize, pos.checked_add(size64)?)
        } else if size32 == 0 {
            (8usize, end)
        } else {
            (8usize, pos.checked_add(size32)?)
        };
        if atom_end > end || atom_end <= pos + header {
            break;
        }
        let payload_start = pos + header;
        if typ == atom {
            return bytes.get(payload_start..atom_end);
        }
        if is_mp4_container_atom(typ) {
            if let Some(found) =
                find_mp4_atom_payload_in_range(bytes, payload_start, atom_end, atom, depth + 1)
            {
                return Some(found);
            }
        }
        pos = atom_end;
    }
    None
}

fn is_mp4_container_atom(typ: &[u8]) -> bool {
    matches!(
        typ,
        b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts"
    )
}

fn parse_mvhd_duration_seconds(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;
    match version {
        0 => {
            let timescale = read_u32_be(payload, 12)?;
            let duration = read_u32_be(payload, 16)? as u64;
            duration_from_timescale(duration, timescale)
        }
        1 => {
            let timescale = read_u32_be(payload, 20)?;
            let duration = read_u64_be(payload, 24)?;
            duration_from_timescale(duration, timescale)
        }
        _ => None,
    }
}

fn duration_from_timescale(duration: u64, timescale: u32) -> Option<f64> {
    (timescale > 0).then(|| duration as f64 / timescale as f64)
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// ── Executable preview ──────────────────────────────────────────────────────

pub fn render_executable(path: &str) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let (size, modified_unix) = file_size_modified(path);
    let bytes = match read_file_prefix(path, MAX_EXECUTABLE_HEADER_BYTES) {
        Some(b) => b,
        None => return String::new(),
    };

    let Some(pe) = parse_pe_headers(&bytes) else {
        return render_info(path, "executable", size, modified_unix);
    };

    let mut text = String::new();
    text.push_str(&format!("Name: {filename}\n"));
    text.push_str("Kind: executable\n");
    text.push_str(&format!("Format: {}\n", pe.format));
    text.push_str(&format!("Machine: {}\n", pe.machine));
    text.push_str(&format!("Subsystem: {}\n", pe.subsystem));
    text.push_str(&format!("Sections: {}\n", pe.sections));
    text.push_str(&format!("Entry point RVA: 0x{:08X}\n", pe.entry_point));
    text.push_str(&format!(
        "Image size: {}\n",
        format_bytes(pe.image_size as i64)
    ));
    if pe.link_timestamp > 0 {
        text.push_str(&format!(
            "Link time: {}\n",
            format_timestamp(pe.link_timestamp as i64)
        ));
    }
    text.push_str(&format!("Characteristics: 0x{:04X}\n", pe.characteristics));
    text.push_str(&format!("Image base: 0x{:016X}\n", pe.image_base));
    text.push_str(&format!(
        "Section alignment: {}\n",
        format_bytes(pe.section_alignment as i64)
    ));
    text.push_str(&format!(
        "File alignment: {}\n",
        format_bytes(pe.file_alignment as i64)
    ));
    if pe.dll_characteristics > 0 {
        text.push_str(&format!(
            "DLL characteristics: 0x{:04X}\n",
            pe.dll_characteristics
        ));
    }
    if pe.data_directories > 0 {
        text.push_str(&format!("Data directories: {}\n", pe.data_directories));
    }
    for directory in &pe.directories {
        text.push_str(&format!(
            "{} directory: 0x{:08X}, {}\n",
            directory.name,
            directory.address,
            format_bytes(directory.size as i64)
        ));
    }
    if !pe.section_names.is_empty() {
        text.push_str(&format!("Section names: {}\n", pe.section_names.join(", ")));
    }
    if !pe.imports.is_empty() {
        text.push_str(&format!("Imports: {}\n", pe.imports.join(", ")));
    }
    if !pe.imported_functions.is_empty() {
        text.push_str(&format!(
            "Imported functions: {}\n",
            pe.imported_functions.join(", ")
        ));
    }
    if !pe.exports.is_empty() {
        text.push_str(&format!("Exports: {}\n", pe.exports.join(", ")));
    }
    if !pe.export_details.is_empty() {
        text.push_str(&format!(
            "Export details: {}\n",
            pe.export_details.join(", ")
        ));
    }
    if pe.has_version_resource {
        text.push_str("Version resource: present\n");
    }
    for (name, value) in &pe.version_strings {
        text.push_str(&format!("Version {name}: {value}\n"));
    }
    if let Some(fixed) = &pe.fixed_version {
        text.push_str(&format!(
            "Fixed file version: {}; product {}; flags 0x{:08X}; type {}\n",
            fixed.file_version, fixed.product_version, fixed.flags, fixed.file_type
        ));
    }
    if let Some(certificate) = &pe.certificate {
        text.push_str(&format!(
            "Certificate table: {}, revision 0x{:04X}, type 0x{:04X}\n",
            format_bytes(certificate.length as i64),
            certificate.revision,
            certificate.typ
        ));
        if !certificate.digest_algorithms.is_empty() {
            text.push_str(&format!(
                "Certificate digest algorithms: {}\n",
                certificate.digest_algorithms.join(", ")
            ));
        }
        if !certificate.signature_algorithms.is_empty() {
            text.push_str(&format!(
                "Certificate signature algorithms: {}\n",
                certificate.signature_algorithms.join(", ")
            ));
        }
        if !certificate.signers.is_empty() {
            text.push_str(&format!(
                "Certificate signers: {}\n",
                certificate.signers.join(", ")
            ));
        }
        if !certificate.names.is_empty() {
            text.push_str(&format!(
                "Certificate names: {}\n",
                certificate.names.join(", ")
            ));
        }
        if !certificate.issuers.is_empty() {
            text.push_str(&format!(
                "Certificate issuers: {}\n",
                certificate.issuers.join(", ")
            ));
        }
        if !certificate.subjects.is_empty() {
            text.push_str(&format!(
                "Certificate subjects: {}\n",
                certificate.subjects.join(", ")
            ));
        }
    }
    if let Some(clr) = &pe.clr {
        text.push_str(&format!(
            "CLR runtime: {}.{}; metadata 0x{:08X}, {}; flags 0x{:08X}\n",
            clr.major,
            clr.minor,
            clr.metadata_rva,
            format_bytes(clr.metadata_size as i64),
            clr.flags
        ));
        if !clr.metadata_version.is_empty() {
            text.push_str(&format!("CLR metadata version: {}\n", clr.metadata_version));
        }
        if !clr.metadata_streams.is_empty() {
            text.push_str(&format!(
                "CLR metadata streams: {}\n",
                clr.metadata_streams.join(", ")
            ));
        }
        if !clr.metadata_tables.is_empty() {
            text.push_str(&format!(
                "CLR metadata tables: {}\n",
                clr.metadata_tables.join(", ")
            ));
        }
        if let Some(assembly) = &clr.assembly {
            text.push_str(&format!("CLR assembly: {assembly}\n"));
        }
        if !clr.assembly_refs.is_empty() {
            text.push_str(&format!(
                "CLR assembly references: {}\n",
                clr.assembly_refs.join(", ")
            ));
        }
        if !clr.type_defs.is_empty() {
            text.push_str(&format!(
                "CLR type definitions: {}\n",
                clr.type_defs.join(", ")
            ));
        }
        if clr.custom_attributes > 0 {
            text.push_str(&format!(
                "CLR custom attributes: {}\n",
                clr.custom_attributes
            ));
        }
    }
    text.push_str(&format!("File size: {}\n", format_bytes(size)));
    text.push_str(&format!("Modified: {}\n", format_timestamp(modified_unix)));

    to_json(&PreviewReadyDto {
        kind: "executable".to_string(),
        title: format!("{filename} - {}", pe.machine),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

struct PeSummary {
    machine: &'static str,
    format: &'static str,
    subsystem: &'static str,
    sections: u16,
    entry_point: u32,
    image_size: u32,
    link_timestamp: u32,
    characteristics: u16,
    image_base: u64,
    section_alignment: u32,
    file_alignment: u32,
    dll_characteristics: u16,
    data_directories: u32,
    section_names: Vec<String>,
    directories: Vec<PeDataDirectory>,
    imports: Vec<String>,
    imported_functions: Vec<String>,
    exports: Vec<String>,
    export_details: Vec<String>,
    has_version_resource: bool,
    version_strings: Vec<(String, String)>,
    fixed_version: Option<PeFixedVersion>,
    certificate: Option<PeCertificateSummary>,
    clr: Option<PeClrSummary>,
}

struct PeDataDirectory {
    name: &'static str,
    address: u32,
    size: u32,
}

struct PeSectionSummary {
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

struct PeCertificateSummary {
    length: u32,
    revision: u16,
    typ: u16,
    digest_algorithms: Vec<String>,
    signature_algorithms: Vec<String>,
    signers: Vec<String>,
    names: Vec<String>,
    issuers: Vec<String>,
    subjects: Vec<String>,
}

struct PeFixedVersion {
    file_version: String,
    product_version: String,
    flags: u32,
    file_type: &'static str,
}

struct PeClrSummary {
    major: u16,
    minor: u16,
    metadata_rva: u32,
    metadata_size: u32,
    flags: u32,
    metadata_version: String,
    metadata_streams: Vec<String>,
    metadata_tables: Vec<String>,
    assembly: Option<String>,
    assembly_refs: Vec<String>,
    type_defs: Vec<String>,
    custom_attributes: u32,
}

fn parse_pe_headers(bytes: &[u8]) -> Option<PeSummary> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return None;
    }
    let pe_offset = read_u32(bytes, 0x3C)? as usize;
    if pe_offset.checked_add(24)? > bytes.len() || &bytes[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return None;
    }

    let coff = pe_offset + 4;
    let machine = read_u16(bytes, coff)?;
    let sections = read_u16(bytes, coff + 2)?;
    let timestamp = read_u32(bytes, coff + 4)?;
    let opt_size = read_u16(bytes, coff + 16)? as usize;
    let characteristics = read_u16(bytes, coff + 18)?;
    let opt = coff + 20;
    if opt.checked_add(opt_size)? > bytes.len() || opt_size < 70 {
        return None;
    }

    let magic = read_u16(bytes, opt)?;
    let entry_point = read_u32(bytes, opt + 16).unwrap_or(0);
    let image_base = if magic == 0x20B {
        read_u64(bytes, opt + 24).unwrap_or(0)
    } else {
        read_u32(bytes, opt + 28).unwrap_or(0) as u64
    };
    let section_alignment = read_u32(bytes, opt + 32).unwrap_or(0);
    let file_alignment = read_u32(bytes, opt + 36).unwrap_or(0);
    let image_size = read_u32(bytes, opt + 56).unwrap_or(0);
    let subsystem = read_u16(bytes, opt + 68).unwrap_or(0);
    let dll_characteristics = read_u16(bytes, opt + 70).unwrap_or(0);
    let data_directories_offset = if magic == 0x20B { opt + 108 } else { opt + 92 };
    let data_directories = read_u32(bytes, data_directories_offset).unwrap_or(0);
    let directories =
        parse_pe_data_directories(bytes, data_directories_offset + 4, data_directories);
    let section_table = opt + opt_size;
    let section_names = parse_pe_section_names(bytes, section_table, sections);
    let section_summaries = parse_pe_sections(bytes, section_table, sections);
    let imports = directories
        .iter()
        .find(|directory| directory.name == "Import")
        .map(|directory| parse_pe_import_dlls(bytes, &section_summaries, directory.address))
        .unwrap_or_default();
    let imported_functions = directories
        .iter()
        .find(|directory| directory.name == "Import")
        .map(|directory| {
            parse_pe_import_functions(bytes, &section_summaries, directory.address, magic == 0x20B)
        })
        .unwrap_or_default();
    let exports = directories
        .iter()
        .find(|directory| directory.name == "Export")
        .map(|directory| parse_pe_export_names(bytes, &section_summaries, directory.address))
        .unwrap_or_default();
    let export_details = directories
        .iter()
        .find(|directory| directory.name == "Export")
        .map(|directory| {
            parse_pe_export_details(bytes, &section_summaries, directory.address, directory.size)
        })
        .unwrap_or_default();
    let version_resource = directories
        .iter()
        .find(|directory| directory.name == "Resource")
        .and_then(|directory| {
            pe_rva_to_file_offset(&section_summaries, directory.address)
                .and_then(|offset| pe_find_resource_data(bytes, &section_summaries, offset, 16))
        });
    let version_strings = version_resource
        .and_then(|(offset, size)| {
            bytes
                .get(offset..offset.saturating_add(size))
                .map(parse_pe_version_strings)
        })
        .unwrap_or_default();
    let fixed_version = version_resource
        .and_then(|(offset, size)| bytes.get(offset..offset.saturating_add(size)))
        .and_then(parse_pe_fixed_version);
    let has_version_resource = version_resource.is_some();
    let certificate = directories
        .iter()
        .find(|directory| directory.name == "Certificate")
        .and_then(|directory| parse_pe_certificate(bytes, directory.address));
    let clr = directories
        .iter()
        .find(|directory| directory.name == "CLR")
        .and_then(|directory| pe_rva_to_file_offset(&section_summaries, directory.address))
        .and_then(|offset| parse_pe_clr_header(bytes, &section_summaries, offset));

    Some(PeSummary {
        machine: machine_name(machine),
        format: match magic {
            0x10B => "PE32",
            0x20B => "PE32+",
            _ => "PE",
        },
        subsystem: subsystem_name(subsystem),
        sections,
        entry_point,
        image_size,
        link_timestamp: timestamp,
        characteristics,
        image_base,
        section_alignment,
        file_alignment,
        dll_characteristics,
        data_directories,
        section_names,
        directories,
        imports,
        imported_functions,
        exports,
        export_details,
        has_version_resource,
        version_strings,
        fixed_version,
        certificate,
        clr,
    })
}

fn parse_pe_data_directories(bytes: &[u8], offset: usize, count: u32) -> Vec<PeDataDirectory> {
    let names = [
        "Export",
        "Import",
        "Resource",
        "Exception",
        "Certificate",
        "Base relocation",
        "Debug",
        "Architecture",
        "Global pointer",
        "TLS",
        "Load config",
        "Bound import",
        "IAT",
        "Delay import",
        "CLR",
        "Reserved",
    ];
    let mut directories = Vec::new();
    for index in 0..count.min(names.len() as u32) as usize {
        let entry = offset + index * 8;
        let Some(address) = read_u32(bytes, entry) else {
            break;
        };
        let Some(size) = read_u32(bytes, entry + 4) else {
            break;
        };
        if address != 0 || size != 0 {
            directories.push(PeDataDirectory {
                name: names[index],
                address,
                size,
            });
        }
    }
    directories
}

fn parse_pe_section_names(bytes: &[u8], section_table: usize, sections: u16) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..sections.min(12) as usize {
        let offset = section_table + index * 40;
        let Some(raw) = bytes.get(offset..offset + 8) else {
            break;
        };
        let name = String::from_utf8_lossy(raw)
            .trim_matches('\0')
            .trim()
            .to_string();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

fn parse_pe_sections(bytes: &[u8], section_table: usize, sections: u16) -> Vec<PeSectionSummary> {
    let mut summaries = Vec::new();
    for index in 0..sections.min(96) as usize {
        let offset = section_table + index * 40;
        if offset + 40 > bytes.len() {
            break;
        }
        summaries.push(PeSectionSummary {
            virtual_size: read_u32(bytes, offset + 8).unwrap_or(0),
            virtual_address: read_u32(bytes, offset + 12).unwrap_or(0),
            raw_size: read_u32(bytes, offset + 16).unwrap_or(0),
            raw_pointer: read_u32(bytes, offset + 20).unwrap_or(0),
        });
    }
    summaries
}

fn parse_pe_import_dlls(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    import_rva: u32,
) -> Vec<String> {
    let Some(mut offset) = pe_rva_to_file_offset(sections, import_rva) else {
        return Vec::new();
    };
    let mut imports: Vec<String> = Vec::new();
    for _ in 0..64 {
        if offset + 20 > bytes.len() {
            break;
        }
        let original_first_thunk = read_u32(bytes, offset).unwrap_or(0);
        let name_rva = read_u32(bytes, offset + 12).unwrap_or(0);
        let first_thunk = read_u32(bytes, offset + 16).unwrap_or(0);
        if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        if let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) {
            if let Some(name) = read_c_string(bytes, name_offset, 260) {
                if !name.is_empty()
                    && !imports
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(&name))
                {
                    imports.push(name);
                }
            }
        }
        offset += 20;
    }
    imports
}

fn parse_pe_import_functions(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    import_rva: u32,
    pe64: bool,
) -> Vec<String> {
    let Some(mut offset) = pe_rva_to_file_offset(sections, import_rva) else {
        return Vec::new();
    };
    let mut functions = Vec::new();
    for _ in 0..64 {
        if offset + 20 > bytes.len() {
            break;
        }
        let original_first_thunk = read_u32(bytes, offset).unwrap_or(0);
        let name_rva = read_u32(bytes, offset + 12).unwrap_or(0);
        let first_thunk = read_u32(bytes, offset + 16).unwrap_or(0);
        if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        let dll = pe_rva_to_file_offset(sections, name_rva)
            .and_then(|name_offset| read_c_string(bytes, name_offset, 260))
            .unwrap_or_default();
        let thunk_rva = if original_first_thunk != 0 {
            original_first_thunk
        } else {
            first_thunk
        };
        append_pe_import_thunks(bytes, sections, thunk_rva, pe64, &dll, &mut functions);
        offset += 20;
    }
    functions
}

fn append_pe_import_thunks(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    thunk_rva: u32,
    pe64: bool,
    dll: &str,
    functions: &mut Vec<String>,
) {
    let Some(mut offset) = pe_rva_to_file_offset(sections, thunk_rva) else {
        return;
    };
    let thunk_size = if pe64 { 8 } else { 4 };
    for _ in 0..128 {
        let value = if pe64 {
            read_u64(bytes, offset).unwrap_or(0)
        } else {
            read_u32(bytes, offset).unwrap_or(0) as u64
        };
        if value == 0 {
            break;
        }
        let ordinal_mask = if pe64 {
            0x8000_0000_0000_0000
        } else {
            0x8000_0000
        };
        if value & ordinal_mask == 0 {
            if let Some(name_offset) = pe_rva_to_file_offset(sections, value as u32) {
                if let Some(name) = read_c_string(bytes, name_offset + 2, 260) {
                    if !name.is_empty() {
                        let qualified = if dll.is_empty() {
                            name
                        } else {
                            format!("{dll}!{name}")
                        };
                        if !functions
                            .iter()
                            .any(|existing| existing.eq_ignore_ascii_case(&qualified))
                        {
                            functions.push(qualified);
                        }
                    }
                }
            }
        } else {
            let ordinal = value & 0xFFFF;
            let qualified = if dll.is_empty() {
                format!("#{ordinal}")
            } else {
                format!("{dll}!#{ordinal}")
            };
            if !functions
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&qualified))
            {
                functions.push(qualified);
            }
        }
        offset += thunk_size;
    }
}

fn parse_pe_export_names(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    export_rva: u32,
) -> Vec<String> {
    let Some(offset) = pe_rva_to_file_offset(sections, export_rva) else {
        return Vec::new();
    };
    if offset + 40 > bytes.len() {
        return Vec::new();
    }
    let names = read_u32(bytes, offset + 24).unwrap_or(0).min(256) as usize;
    let names_rva = read_u32(bytes, offset + 32).unwrap_or(0);
    let Some(names_offset) = pe_rva_to_file_offset(sections, names_rva) else {
        return Vec::new();
    };
    let mut exports = Vec::new();
    for index in 0..names {
        let Some(name_rva) = read_u32(bytes, names_offset + index * 4) else {
            break;
        };
        if let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) {
            if let Some(name) = read_c_string(bytes, name_offset, 260) {
                if !name.is_empty() {
                    exports.push(name);
                }
            }
        }
    }
    exports
}

fn parse_pe_export_details(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    export_rva: u32,
    export_size: u32,
) -> Vec<String> {
    let Some(offset) = pe_rva_to_file_offset(sections, export_rva) else {
        return Vec::new();
    };
    if offset + 40 > bytes.len() {
        return Vec::new();
    }
    let ordinal_base = read_u32(bytes, offset + 16).unwrap_or(0);
    let function_count = read_u32(bytes, offset + 20).unwrap_or(0).min(4096) as usize;
    let name_count = read_u32(bytes, offset + 24).unwrap_or(0).min(256) as usize;
    let functions_rva = read_u32(bytes, offset + 28).unwrap_or(0);
    let names_rva = read_u32(bytes, offset + 32).unwrap_or(0);
    let ordinals_rva = read_u32(bytes, offset + 36).unwrap_or(0);
    let Some(functions_offset) = pe_rva_to_file_offset(sections, functions_rva) else {
        return Vec::new();
    };
    let Some(names_offset) = pe_rva_to_file_offset(sections, names_rva) else {
        return Vec::new();
    };
    let Some(ordinals_offset) = pe_rva_to_file_offset(sections, ordinals_rva) else {
        return Vec::new();
    };
    let mut details = Vec::new();
    for index in 0..name_count {
        let Some(name_rva) = read_u32(bytes, names_offset + index * 4) else {
            break;
        };
        let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) else {
            continue;
        };
        let Some(name) = read_c_string(bytes, name_offset, 260) else {
            continue;
        };
        let ordinal_index = read_u16(bytes, ordinals_offset + index * 2).unwrap_or(0) as usize;
        if ordinal_index >= function_count {
            continue;
        }
        let function_rva = read_u32(bytes, functions_offset + ordinal_index * 4).unwrap_or(0);
        let ordinal = ordinal_base + ordinal_index as u32;
        if function_rva >= export_rva && function_rva < export_rva.saturating_add(export_size) {
            if let Some(forwarder_offset) = pe_rva_to_file_offset(sections, function_rva) {
                if let Some(forwarder) = read_c_string(bytes, forwarder_offset, 260) {
                    details.push(format!("{name} #{ordinal} -> {forwarder}"));
                    continue;
                }
            }
        }
        details.push(format!("{name} #{ordinal} @ 0x{function_rva:08X}"));
    }
    details
}

fn pe_find_resource_data(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    resource_root: usize,
    typ: u16,
) -> Option<(usize, usize)> {
    pe_find_resource_data_in_directory(bytes, sections, resource_root, resource_root, typ, 0)
}

fn pe_find_resource_data_in_directory(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    root: usize,
    directory: usize,
    typ: u16,
    depth: usize,
) -> Option<(usize, usize)> {
    if depth > 2 || directory + 16 > bytes.len() {
        return None;
    }
    let named = read_u16(bytes, directory + 12).unwrap_or(0) as usize;
    let ids = read_u16(bytes, directory + 14).unwrap_or(0) as usize;
    let entries = named.saturating_add(ids).min(256);
    for index in 0..entries {
        let entry = directory + 16 + index * 8;
        if entry + 8 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, entry).unwrap_or(0);
        if depth == 0 && (id & 0x8000_0000 != 0 || (id & 0xFFFF) as u16 != typ) {
            continue;
        }
        let target = read_u32(bytes, entry + 4).unwrap_or(0);
        if target & 0x8000_0000 != 0 {
            let child = root + (target & 0x7FFF_FFFF) as usize;
            if let Some(found) =
                pe_find_resource_data_in_directory(bytes, sections, root, child, typ, depth + 1)
            {
                return Some(found);
            }
        } else {
            let data_entry = root + target as usize;
            if data_entry + 16 > bytes.len() {
                continue;
            }
            let data_rva = read_u32(bytes, data_entry).unwrap_or(0);
            let size = read_u32(bytes, data_entry + 4).unwrap_or(0) as usize;
            if let Some(data_offset) = pe_rva_to_file_offset(sections, data_rva) {
                return Some((data_offset, size));
            }
        }
    }
    None
}

fn parse_pe_version_strings(bytes: &[u8]) -> Vec<(String, String)> {
    let mut strings = Vec::new();
    parse_pe_version_node(bytes, 0, bytes.len(), &mut strings);
    strings.sort_by(|a, b| a.0.cmp(&b.0));
    strings.dedup_by(|a, b| a.0 == b.0);
    strings
}

fn parse_pe_fixed_version(bytes: &[u8]) -> Option<PeFixedVersion> {
    if bytes.len() < 6 {
        return None;
    }
    let length = read_u16(bytes, 0)? as usize;
    let value_len = read_u16(bytes, 2)? as usize;
    let typ = read_u16(bytes, 4).unwrap_or(0);
    if length == 0 || length > bytes.len() || typ != 0 || value_len < 52 {
        return None;
    }
    let (key, key_end) = read_utf16_z(bytes, 6, length)?;
    if key != "VS_VERSION_INFO" {
        return None;
    }
    let value_offset = align4(key_end);
    if value_offset + 52 > length || read_u32(bytes, value_offset)? != 0xFEEF_04BD {
        return None;
    }
    let file_ms = read_u32(bytes, value_offset + 8)?;
    let file_ls = read_u32(bytes, value_offset + 12)?;
    let product_ms = read_u32(bytes, value_offset + 16)?;
    let product_ls = read_u32(bytes, value_offset + 20)?;
    let flags_mask = read_u32(bytes, value_offset + 24).unwrap_or(0);
    let flags = read_u32(bytes, value_offset + 28).unwrap_or(0) & flags_mask;
    let file_type = read_u32(bytes, value_offset + 36).unwrap_or(0);
    Some(PeFixedVersion {
        file_version: format_pe_version(file_ms, file_ls),
        product_version: format_pe_version(product_ms, product_ls),
        flags,
        file_type: pe_version_file_type(file_type),
    })
}

fn format_pe_version(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)
}

fn pe_version_file_type(value: u32) -> &'static str {
    match value {
        1 => "application",
        2 => "DLL",
        3 => "driver",
        4 => "font",
        5 => "VxD",
        7 => "static library",
        _ => "unknown",
    }
}

fn parse_pe_version_node(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    strings: &mut Vec<(String, String)>,
) -> Option<usize> {
    if offset + 6 > limit || offset + 6 > bytes.len() {
        return None;
    }
    let length = read_u16(bytes, offset)? as usize;
    if length == 0 || offset + length > limit || offset + length > bytes.len() {
        return None;
    }
    let value_len = read_u16(bytes, offset + 2)? as usize;
    let typ = read_u16(bytes, offset + 4).unwrap_or(0);
    let (key, key_end) = read_utf16_z(bytes, offset + 6, offset + length)?;
    let value_offset = align4(key_end);
    let value_bytes = if typ == 1 {
        value_len.saturating_mul(2)
    } else {
        value_len
    };
    if typ == 1 && value_len > 0 && is_version_string_key(&key) {
        let value_end = value_offset
            .saturating_add(value_bytes)
            .min(offset + length);
        if let Some(raw) = bytes.get(value_offset..value_end) {
            let value = decode_utf16le_string(raw);
            if !value.is_empty() {
                strings.push((key.clone(), value));
            }
        }
    }
    let mut child = align4(value_offset.saturating_add(value_bytes));
    while child + 6 <= offset + length {
        let Some(next) = parse_pe_version_node(bytes, child, offset + length, strings) else {
            break;
        };
        if next <= child {
            break;
        }
        child = next;
    }
    Some(align4(offset + length))
}

fn is_version_string_key(key: &str) -> bool {
    matches!(
        key,
        "CompanyName"
            | "FileDescription"
            | "FileVersion"
            | "InternalName"
            | "OriginalFilename"
            | "ProductName"
            | "ProductVersion"
    )
}

fn read_utf16_z(bytes: &[u8], offset: usize, limit: usize) -> Option<(String, usize)> {
    let mut pos = offset;
    let mut units = Vec::new();
    while pos + 2 <= limit && pos + 2 <= bytes.len() {
        let unit = read_u16(bytes, pos)?;
        pos += 2;
        if unit == 0 {
            return Some((String::from_utf16_lossy(&units), pos));
        }
        units.push(unit);
    }
    None
}

fn decode_utf16le_string(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn parse_pe_certificate(bytes: &[u8], file_offset: u32) -> Option<PeCertificateSummary> {
    let offset = file_offset as usize;
    if offset + 8 > bytes.len() {
        return None;
    }
    let length = read_u32(bytes, offset).unwrap_or(0) as usize;
    let (issuers, subjects) = parse_authenticode_certificate_subjects(bytes, offset, length);
    Some(PeCertificateSummary {
        length: read_u32(bytes, offset)?,
        revision: read_u16(bytes, offset + 4)?,
        typ: read_u16(bytes, offset + 6)?,
        digest_algorithms: parse_authenticode_digest_algorithms(bytes, offset, length),
        signature_algorithms: parse_authenticode_signature_algorithms(bytes, offset, length),
        signers: parse_authenticode_signers(bytes, offset, length),
        names: parse_authenticode_certificate_names(bytes, offset, length),
        issuers,
        subjects,
    })
}

fn parse_authenticode_digest_algorithms(bytes: &[u8], offset: usize, length: usize) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let oid_patterns: [(&str, &[u8]); 4] = [
        ("SHA-1", &[0x06, 0x05, 0x2B, 0x0E, 0x03, 0x02, 0x1A]),
        (
            "SHA-256",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
            ],
        ),
        (
            "SHA-384",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02,
            ],
        ),
        (
            "SHA-512",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
            ],
        ),
    ];
    let mut algorithms = Vec::new();
    for (name, pattern) in oid_patterns {
        if payload
            .windows(pattern.len())
            .any(|window| window == pattern)
        {
            algorithms.push(name.to_string());
        }
    }
    algorithms
}

fn parse_authenticode_signers(bytes: &[u8], offset: usize, length: usize) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let signed_data_oid = [
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
    ];
    if !payload
        .windows(signed_data_oid.len())
        .any(|window| window == signed_data_oid)
    {
        return Vec::new();
    }
    let digest = first_oid_name(
        payload,
        &[
            ("SHA-1", &[0x06, 0x05, 0x2B, 0x0E, 0x03, 0x02, 0x1A][..]),
            (
                "SHA-256",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
                ][..],
            ),
            (
                "SHA-384",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02,
                ][..],
            ),
            (
                "SHA-512",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
                ][..],
            ),
        ],
    );
    let signature = first_oid_name(
        payload,
        &[
            (
                "SHA-1 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05,
                ][..],
            ),
            (
                "SHA-256 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
                ][..],
            ),
            (
                "SHA-384 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C,
                ][..],
            ),
            (
                "SHA-512 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D,
                ][..],
            ),
            (
                "RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
                ][..],
            ),
            (
                "ECDSA with SHA-256",
                &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02][..],
            ),
            (
                "ECDSA with SHA-384",
                &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03][..],
            ),
        ],
    );
    match (digest, signature) {
        (Some(digest), Some(signature)) => vec![format!("digest {digest}; signature {signature}")],
        (Some(digest), None) => vec![format!("digest {digest}")],
        (None, Some(signature)) => vec![format!("signature {signature}")],
        (None, None) => Vec::new(),
    }
}

fn first_oid_name(bytes: &[u8], patterns: &[(&'static str, &[u8])]) -> Option<&'static str> {
    patterns.iter().find_map(|(name, pattern)| {
        bytes
            .windows(pattern.len())
            .any(|window| window == *pattern)
            .then_some(*name)
    })
}

fn parse_authenticode_certificate_names(bytes: &[u8], offset: usize, length: usize) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let name_oids: [(&str, &[u8]); 3] = [
        ("CN", &[0x06, 0x03, 0x55, 0x04, 0x03]),
        ("O", &[0x06, 0x03, 0x55, 0x04, 0x0A]),
        ("OU", &[0x06, 0x03, 0x55, 0x04, 0x0B]),
    ];
    let mut names = Vec::new();
    for (label, oid) in name_oids {
        let mut search = 0usize;
        while search + oid.len() + 2 <= payload.len() && names.len() < 12 {
            let Some(position) = payload[search..]
                .windows(oid.len())
                .position(|window| window == oid)
            else {
                break;
            };
            let value_offset = search + position + oid.len();
            if let Some(value) = read_der_string(payload, value_offset) {
                let entry = format!("{label}={value}");
                if !names.iter().any(|existing| existing == &entry) {
                    names.push(entry);
                }
            }
            search = value_offset.saturating_add(1);
        }
    }
    names
}

fn parse_authenticode_certificate_subjects(
    bytes: &[u8],
    offset: usize,
    length: usize,
) -> (Vec<String>, Vec<String>) {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return (Vec::new(), Vec::new());
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let mut issuers = Vec::new();
    let mut subjects = Vec::new();
    let mut search = 0usize;
    while search + 4 < payload.len() && subjects.len() < 4 {
        let Some(position) = payload[search..].iter().position(|byte| *byte == 0x30) else {
            break;
        };
        let cert_offset = search + position;
        if let Some((issuer, subject, cert_end)) =
            parse_x509_certificate_names(payload, cert_offset)
        {
            if !issuer.is_empty() && !issuers.contains(&issuer) {
                issuers.push(issuer);
            }
            if !subject.is_empty() && !subjects.contains(&subject) {
                subjects.push(subject);
            }
            search = cert_end.max(cert_offset + 1);
        } else {
            search = cert_offset + 1;
        }
    }
    (issuers, subjects)
}

fn parse_x509_certificate_names(bytes: &[u8], offset: usize) -> Option<(String, String, usize)> {
    let (cert_content, cert_end) = der_tlv_content(bytes, offset, 0x30)?;
    let cert = bytes.get(cert_content..cert_end)?;
    let (tbs_content, tbs_end_rel) = der_tlv_content(cert, 0, 0x30)?;
    let tbs = cert.get(tbs_content..tbs_end_rel)?;
    let mut cursor = 0usize;
    if tbs.get(cursor) == Some(&0xA0) {
        let (_, next) = der_tlv_content(tbs, cursor, 0xA0)?;
        cursor = next;
    }
    for _ in 0..2 {
        let (_, next) = der_any_tlv_content(tbs, cursor)?;
        cursor = next;
    }
    let (issuer_content, issuer_end) = der_tlv_content(tbs, cursor, 0x30)?;
    let issuer = parse_x509_name(&tbs[issuer_content..issuer_end]);
    cursor = issuer_end;
    let (_, next) = der_tlv_content(tbs, cursor, 0x30)?;
    cursor = next;
    let (subject_content, subject_end) = der_tlv_content(tbs, cursor, 0x30)?;
    let subject = parse_x509_name(&tbs[subject_content..subject_end]);
    Some((issuer, subject, cert_end))
}

fn parse_x509_name(bytes: &[u8]) -> String {
    let name_oids: [(&str, &[u8]); 3] = [
        ("CN", &[0x06, 0x03, 0x55, 0x04, 0x03]),
        ("O", &[0x06, 0x03, 0x55, 0x04, 0x0A]),
        ("OU", &[0x06, 0x03, 0x55, 0x04, 0x0B]),
    ];
    let mut parts = Vec::new();
    for (label, oid) in name_oids {
        let mut search = 0usize;
        while search + oid.len() + 2 <= bytes.len() && parts.len() < 8 {
            let Some(position) = bytes[search..]
                .windows(oid.len())
                .position(|window| window == oid)
            else {
                break;
            };
            let value_offset = search + position + oid.len();
            if let Some(value) = read_der_string(bytes, value_offset) {
                let entry = format!("{label}={value}");
                if !parts.contains(&entry) {
                    parts.push(entry);
                }
            }
            search = value_offset.saturating_add(1);
        }
    }
    parts.join("/")
}

fn der_any_tlv_content(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let _tag = *bytes.get(offset)?;
    der_tlv_bounds(bytes, offset).map(|(content, end)| (content, end))
}

fn der_tlv_content(bytes: &[u8], offset: usize, tag: u8) -> Option<(usize, usize)> {
    (*bytes.get(offset)? == tag).then_some(())?;
    der_tlv_bounds(bytes, offset)
}

fn der_tlv_bounds(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let len_byte = *bytes.get(offset + 1)?;
    if len_byte & 0x80 == 0 {
        let content = offset + 2;
        let end = content.checked_add(len_byte as usize)?;
        return (end <= bytes.len()).then_some((content, end));
    }
    let len_len = (len_byte & 0x7F) as usize;
    if len_len == 0 || len_len > 2 || offset + 2 + len_len > bytes.len() {
        return None;
    }
    let mut len = 0usize;
    for byte in &bytes[offset + 2..offset + 2 + len_len] {
        len = (len << 8) | *byte as usize;
    }
    if len > 4096 {
        return None;
    }
    let content = offset + 2 + len_len;
    let end = content.checked_add(len)?;
    (end <= bytes.len()).then_some((content, end))
}

fn read_der_string(bytes: &[u8], offset: usize) -> Option<String> {
    let tag = *bytes.get(offset)?;
    if !matches!(tag, 0x0C | 0x13 | 0x14 | 0x16) {
        return None;
    }
    let len = *bytes.get(offset + 1)? as usize;
    if len & 0x80 != 0 || len > 128 {
        return None;
    }
    let raw = bytes.get(offset + 2..offset + 2 + len)?;
    let value = String::from_utf8_lossy(raw).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn parse_authenticode_signature_algorithms(
    bytes: &[u8],
    offset: usize,
    length: usize,
) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let oid_patterns: [(&str, &[u8]); 7] = [
        (
            "RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
            ],
        ),
        (
            "SHA-1 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05,
            ],
        ),
        (
            "SHA-256 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
            ],
        ),
        (
            "SHA-384 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C,
            ],
        ),
        (
            "SHA-512 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D,
            ],
        ),
        (
            "ECDSA with SHA-256",
            &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02],
        ),
        (
            "ECDSA with SHA-384",
            &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03],
        ),
    ];
    let mut algorithms = Vec::new();
    for (name, pattern) in oid_patterns {
        if payload
            .windows(pattern.len())
            .any(|window| window == pattern)
        {
            algorithms.push(name.to_string());
        }
    }
    algorithms
}

fn parse_pe_clr_header(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    offset: usize,
) -> Option<PeClrSummary> {
    if offset + 24 > bytes.len() || read_u32(bytes, offset)? < 24 {
        return None;
    }
    let metadata_rva = read_u32(bytes, offset + 8)?;
    let metadata_size = read_u32(bytes, offset + 12)?;
    let metadata = pe_rva_to_file_offset(sections, metadata_rva).and_then(|metadata_offset| {
        parse_clr_metadata_root(bytes, metadata_offset, metadata_size as usize)
    });
    Some(PeClrSummary {
        major: read_u16(bytes, offset + 4)?,
        minor: read_u16(bytes, offset + 6)?,
        metadata_rva,
        metadata_size,
        flags: read_u32(bytes, offset + 16)?,
        metadata_version: metadata
            .as_ref()
            .map(|root| root.version.clone())
            .unwrap_or_default(),
        metadata_streams: metadata
            .as_ref()
            .map(|root| root.streams.clone())
            .unwrap_or_default(),
        metadata_tables: metadata
            .as_ref()
            .map(|root| root.tables.clone())
            .unwrap_or_default(),
        assembly_refs: metadata
            .as_ref()
            .map(|root| root.assembly_refs.clone())
            .unwrap_or_default(),
        type_defs: metadata
            .as_ref()
            .map(|root| root.type_defs.clone())
            .unwrap_or_default(),
        custom_attributes: metadata
            .as_ref()
            .map(|root| root.custom_attributes)
            .unwrap_or(0),
        assembly: metadata.and_then(|root| root.assembly),
    })
}

struct ClrMetadataRoot {
    version: String,
    streams: Vec<String>,
    tables: Vec<String>,
    assembly: Option<String>,
    assembly_refs: Vec<String>,
    type_defs: Vec<String>,
    custom_attributes: u32,
}

fn parse_clr_metadata_root(bytes: &[u8], offset: usize, size: usize) -> Option<ClrMetadataRoot> {
    let end = offset.checked_add(size)?.min(bytes.len());
    if offset + 20 > end || read_u32(bytes, offset)? != 0x424A_5342 {
        return None;
    }
    let version_len = read_u32(bytes, offset + 12)? as usize;
    let version_start = offset + 16;
    let version_end = version_start.checked_add(version_len)?.min(end);
    let version = String::from_utf8_lossy(bytes.get(version_start..version_end)?)
        .trim_matches('\0')
        .trim()
        .to_string();
    let mut stream_offset = align4(version_end) + 4;
    if stream_offset > end {
        return Some(ClrMetadataRoot {
            version,
            streams: Vec::new(),
            tables: Vec::new(),
            assembly: None,
            assembly_refs: Vec::new(),
            type_defs: Vec::new(),
            custom_attributes: 0,
        });
    }
    let streams = read_u16(bytes, stream_offset - 2).unwrap_or(0).min(64) as usize;
    let mut names = Vec::new();
    let mut strings_heap = None;
    let mut tables_stream = None;
    for _ in 0..streams {
        if stream_offset + 8 > end {
            break;
        }
        let relative_offset = read_u32(bytes, stream_offset)? as usize;
        let stream_size = read_u32(bytes, stream_offset + 4)? as usize;
        let (name, name_end) = read_ascii_z(bytes, stream_offset + 8, end)?;
        let data_offset = offset.checked_add(relative_offset)?;
        names.push(name.clone());
        if name == "#Strings" {
            strings_heap = bytes.get(data_offset..data_offset.saturating_add(stream_size).min(end));
        } else if name == "#~" {
            tables_stream =
                bytes.get(data_offset..data_offset.saturating_add(stream_size).min(end));
        }
        stream_offset = align4(name_end);
    }
    let assembly = tables_stream
        .zip(strings_heap)
        .and_then(|(tables, strings)| parse_clr_assembly_identity(tables, strings));
    let assembly_refs = tables_stream
        .zip(strings_heap)
        .map(|(tables, strings)| parse_clr_assembly_refs(tables, strings))
        .unwrap_or_default();
    let type_defs = tables_stream
        .zip(strings_heap)
        .map(|(tables, strings)| parse_clr_type_defs(tables, strings))
        .unwrap_or_default();
    let custom_attributes = tables_stream
        .and_then(clr_tables_layout)
        .map(|layout| layout.rows[12])
        .unwrap_or(0);
    let tables = tables_stream
        .map(parse_clr_table_counts)
        .unwrap_or_default();
    Some(ClrMetadataRoot {
        version,
        streams: names,
        tables,
        assembly,
        assembly_refs,
        type_defs,
        custom_attributes,
    })
}

fn parse_clr_table_counts(tables: &[u8]) -> Vec<String> {
    if tables.len() < 24 {
        return Vec::new();
    }
    let valid = read_u64(tables, 8).unwrap_or(0);
    let mut offset = 24usize;
    let mut counts = Vec::new();
    for table in 0..64 {
        if valid & (1u64 << table) == 0 {
            continue;
        }
        if offset + 4 > tables.len() {
            break;
        }
        let rows = read_u32(tables, offset).unwrap_or(0);
        if rows > 0 {
            counts.push(format!("{}={rows}", clr_table_name(table)));
        }
        offset += 4;
        if counts.len() >= 32 {
            break;
        }
    }
    counts
}

fn clr_table_name(index: usize) -> &'static str {
    match index {
        0 => "Module",
        1 => "TypeRef",
        2 => "TypeDef",
        4 => "Field",
        6 => "MethodDef",
        8 => "Param",
        9 => "InterfaceImpl",
        10 => "MemberRef",
        11 => "Constant",
        12 => "CustomAttribute",
        13 => "FieldMarshal",
        14 => "DeclSecurity",
        15 => "ClassLayout",
        16 => "FieldLayout",
        17 => "StandAloneSig",
        18 => "EventMap",
        20 => "Event",
        21 => "PropertyMap",
        23 => "Property",
        24 => "MethodSemantics",
        25 => "MethodImpl",
        26 => "ModuleRef",
        27 => "TypeSpec",
        28 => "ImplMap",
        29 => "FieldRVA",
        32 => "Assembly",
        35 => "AssemblyRef",
        39 => "ExportedType",
        40 => "ManifestResource",
        41 => "NestedClass",
        42 => "GenericParam",
        43 => "MethodSpec",
        44 => "GenericParamConstraint",
        _ => "Table",
    }
}

fn parse_clr_assembly_identity(tables: &[u8], strings: &[u8]) -> Option<String> {
    let layout = clr_tables_layout(tables)?;
    if *layout.rows.get(32)? == 0 {
        return None;
    }
    let string_index_size = layout.string_index_size;
    let blob_index_size = layout.blob_index_size;
    let row = *layout.offsets.get(32)?;
    let major = read_u16(tables, row + 4)?;
    let minor = read_u16(tables, row + 6)?;
    let build = read_u16(tables, row + 8)?;
    let revision = read_u16(tables, row + 10)?;
    let name_index_offset = row + 16 + blob_index_size;
    let name_index = if string_index_size == 4 {
        read_u32(tables, name_index_offset)? as usize
    } else {
        read_u16(tables, name_index_offset)? as usize
    };
    let name = read_c_string(strings, name_index, 260)?;
    (!name.is_empty()).then(|| format!("{name} {major}.{minor}.{build}.{revision}"))
}

fn parse_clr_assembly_refs(tables: &[u8], strings: &[u8]) -> Vec<String> {
    let Some(layout) = clr_tables_layout(tables) else {
        return Vec::new();
    };
    let rows = layout.rows.get(35).copied().unwrap_or(0).min(16) as usize;
    let mut refs = Vec::new();
    let mut row = layout.offsets.get(35).copied().unwrap_or(0);
    for _ in 0..rows {
        if row + 12 + layout.blob_index_size + layout.string_index_size * 2 + layout.blob_index_size
            > tables.len()
        {
            break;
        }
        let major = read_u16(tables, row).unwrap_or(0);
        let minor = read_u16(tables, row + 2).unwrap_or(0);
        let build = read_u16(tables, row + 4).unwrap_or(0);
        let revision = read_u16(tables, row + 6).unwrap_or(0);
        let name_index_offset = row + 12 + layout.blob_index_size;
        let name_index = read_clr_index(tables, name_index_offset, layout.string_index_size)
            .unwrap_or(0) as usize;
        if let Some(name) = read_c_string(strings, name_index, 260).filter(|name| !name.is_empty())
        {
            refs.push(format!("{name} {major}.{minor}.{build}.{revision}"));
        }
        row += 12 + layout.blob_index_size + layout.string_index_size * 2 + layout.blob_index_size;
    }
    refs
}

fn parse_clr_type_defs(tables: &[u8], strings: &[u8]) -> Vec<String> {
    let Some(layout) = clr_tables_layout(tables) else {
        return Vec::new();
    };
    let rows = layout.rows.get(2).copied().unwrap_or(0).min(24) as usize;
    let mut types = Vec::new();
    let mut row = layout.offsets.get(2).copied().unwrap_or(0);
    for _ in 0..rows {
        if row + 4 + layout.string_index_size * 2 > tables.len() {
            break;
        }
        let name_index =
            read_clr_index(tables, row + 4, layout.string_index_size).unwrap_or(0) as usize;
        let namespace_index = read_clr_index(
            tables,
            row + 4 + layout.string_index_size,
            layout.string_index_size,
        )
        .unwrap_or(0) as usize;
        let name = read_c_string(strings, name_index, 260).unwrap_or_default();
        if !name.is_empty() {
            let namespace = read_c_string(strings, namespace_index, 260).unwrap_or_default();
            if namespace.is_empty() {
                types.push(name);
            } else {
                types.push(format!("{namespace}.{name}"));
            }
        }
        row += clr_table_row_size(2, layout.string_index_size, 2, layout.blob_index_size)
            .unwrap_or(14);
    }
    types
}

struct ClrTablesLayout {
    rows: [u32; 64],
    offsets: [usize; 64],
    string_index_size: usize,
    blob_index_size: usize,
}

fn clr_tables_layout(tables: &[u8]) -> Option<ClrTablesLayout> {
    if tables.len() < 24 {
        return None;
    }
    let heap_sizes = *tables.get(6)?;
    let valid = read_u64(tables, 8)?;
    let string_index_size = if heap_sizes & 0x01 != 0 { 4 } else { 2 };
    let guid_index_size = if heap_sizes & 0x02 != 0 { 4 } else { 2 };
    let blob_index_size = if heap_sizes & 0x04 != 0 { 4 } else { 2 };
    let mut rows = [0u32; 64];
    let mut offset = 24usize;
    for table in 0..64 {
        if valid & (1u64 << table) == 0 {
            continue;
        }
        rows[table] = read_u32(tables, offset)?;
        offset += 4;
    }
    let mut offsets = [0usize; 64];
    for table in 0..64 {
        if rows[table] == 0 {
            continue;
        }
        offsets[table] = offset;
        let row_size =
            clr_table_row_size(table, string_index_size, guid_index_size, blob_index_size)?;
        offset = offset.checked_add(row_size.checked_mul(rows[table] as usize)?)?;
    }
    Some(ClrTablesLayout {
        rows,
        offsets,
        string_index_size,
        blob_index_size,
    })
}

fn clr_table_row_size(
    table: usize,
    string_index_size: usize,
    guid_index_size: usize,
    blob_index_size: usize,
) -> Option<usize> {
    match table {
        0 => Some(2 + string_index_size + guid_index_size * 3),
        2 => Some(4 + string_index_size * 2 + 2 + 2 + 2),
        12 => Some(2 + 2 + blob_index_size),
        32 => Some(16 + blob_index_size + string_index_size * 2),
        35 => Some(12 + blob_index_size + string_index_size * 2 + blob_index_size),
        _ => None,
    }
}

fn read_clr_index(bytes: &[u8], offset: usize, size: usize) -> Option<u32> {
    match size {
        2 => read_u16(bytes, offset).map(u32::from),
        4 => read_u32(bytes, offset),
        _ => None,
    }
}

fn read_ascii_z(bytes: &[u8], offset: usize, limit: usize) -> Option<(String, usize)> {
    let end = bytes
        .get(offset..limit)?
        .iter()
        .position(|byte| *byte == 0)
        .map(|len| offset + len)?;
    let value = String::from_utf8_lossy(bytes.get(offset..end)?)
        .trim()
        .to_string();
    Some((value, end + 1))
}

fn pe_rva_to_file_offset(sections: &[PeSectionSummary], rva: u32) -> Option<usize> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_size).max(1);
        if rva >= section.virtual_address && rva < section.virtual_address.saturating_add(span) {
            return Some(
                section
                    .raw_pointer
                    .saturating_add(rva - section.virtual_address) as usize,
            );
        }
    }
    None
}

fn read_c_string(bytes: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let end = bytes
        .get(offset..offset + max_len.min(bytes.len().saturating_sub(offset)))?
        .iter()
        .position(|byte| *byte == 0)
        .map(|len| offset + len)
        .unwrap_or_else(|| offset + max_len.min(bytes.len().saturating_sub(offset)));
    let value = String::from_utf8_lossy(bytes.get(offset..end)?)
        .trim()
        .to_string();
    Some(value)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u64_be(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i32_be(bytes: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i16_be(bytes: &[u8], offset: usize) -> Option<i16> {
    let end = offset.checked_add(2)?;
    Some(i16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i64_be(bytes: &[u8], offset: usize) -> Option<i64> {
    let end = offset.checked_add(8)?;
    Some(i64::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_i32_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<i32> {
    let end = offset.checked_add(4)?;
    let chunk: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(if endian == 2 {
        i32::from_be_bytes(chunk)
    } else {
        i32::from_le_bytes(chunk)
    })
}

fn read_u16_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u16> {
    if endian == 2 {
        read_u16_be(bytes, offset)
    } else {
        read_u16(bytes, offset)
    }
}

fn read_u32_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u32> {
    if endian == 2 {
        read_u32_be(bytes, offset)
    } else {
        read_u32(bytes, offset)
    }
}

fn read_u64_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let chunk: [u8; 8] = bytes.get(offset..end)?.try_into().ok()?;
    Some(if endian == 2 {
        u64::from_be_bytes(chunk)
    } else {
        u64::from_le_bytes(chunk)
    })
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x014C => "x86",
        0x8664 => "x64",
        0x01C0 => "ARM",
        0x01C4 => "ARMv7",
        0xAA64 => "ARM64",
        0x0200 => "IA64",
        _ => "unknown",
    }
}

fn subsystem_name(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "native",
        2 => "Windows GUI",
        3 => "Windows console",
        5 => "OS/2 console",
        7 => "POSIX console",
        9 => "Windows CE GUI",
        10 => "EFI application",
        11 => "EFI boot service driver",
        12 => "EFI runtime driver",
        13 => "EFI ROM",
        14 => "Xbox",
        16 => "Windows boot application",
        _ => "unknown",
    }
}

// ── Ebook preview ───────────────────────────────────────────────────────────

pub fn render_ebook(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".epub") {
        return render_epub(path);
    }
    if lower.ends_with(".fb2") {
        return render_fb2(path);
    }
    render_binary_ebook_info(path)
}

#[derive(Default)]
struct EpubOpf {
    title: String,
    creator: String,
    language: String,
    publisher: String,
    identifier: String,
    date: String,
    description: String,
    manifest: BTreeMap<String, EpubManifestItem>,
    spine: Vec<String>,
}

#[derive(Clone)]
struct EpubManifestItem {
    href: String,
    media_type: String,
}

fn render_epub(path: &str) -> String {
    let filename = file_name(path);
    let mut zip = match open_zip(path) {
        Some(zip) => zip,
        None => return String::new(),
    };

    let container = read_zip_text(&mut zip, "META-INF/container.xml", MAX_EBOOK_XML_BYTES);
    let rootfile = container
        .as_deref()
        .and_then(parse_epub_rootfile)
        .or_else(|| find_epub_opf_path(&mut zip))
        .unwrap_or_else(|| "content.opf".to_string());

    let Some(opf_xml) = read_zip_text(&mut zip, &rootfile, MAX_EBOOK_XML_BYTES) else {
        return render_archive(path, None);
    };
    let opf = parse_epub_opf(&opf_xml);
    let title = first_non_empty_owned([opf.title.as_str(), filename]).to_string();
    let base_dir = rootfile
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();

    let mut markdown = String::new();
    markdown.push_str("# ");
    markdown.push_str(&markdown_escape_line(&title));
    markdown.push_str("\n\n");
    append_metadata_line(&mut markdown, "Author", &opf.creator);
    append_metadata_line(&mut markdown, "Language", &opf.language);
    append_metadata_line(&mut markdown, "Publisher", &opf.publisher);
    append_metadata_line(&mut markdown, "Identifier", &opf.identifier);
    append_metadata_line(&mut markdown, "Date", &opf.date);
    if !opf.description.trim().is_empty() {
        markdown.push_str("\n> ");
        markdown.push_str(&collapse_ws(&opf.description));
        markdown.push('\n');
    }

    if !opf.spine.is_empty() {
        markdown.push_str("\n## Contents\n\n");
        for idref in opf.spine.iter().take(40) {
            if let Some(item) = opf.manifest.get(idref) {
                markdown.push_str("- ");
                markdown.push_str(&markdown_escape_line(&ebook_item_label(&item.href)));
                markdown.push('\n');
            }
        }
    }

    let mut extracted = 0usize;
    for idref in &opf.spine {
        if extracted >= MAX_EBOOK_CHAPTERS || markdown.chars().count() >= MAX_EBOOK_TEXT_CHARS {
            break;
        }
        let Some(item) = opf.manifest.get(idref) else {
            continue;
        };
        if !is_epub_document_item(item) {
            continue;
        }
        let chapter_path = normalize_zip_target(&base_dir, &item.href);
        let Some(chapter_xml) = read_zip_text(&mut zip, &chapter_path, MAX_EBOOK_CHAPTER_BYTES)
        else {
            continue;
        };
        let chapter = extract_xhtml_markdown(&chapter_xml, &ebook_item_label(&item.href));
        if chapter.trim().is_empty() {
            continue;
        }
        markdown.push_str("\n\n");
        push_markdown_limited(&mut markdown, &chapter, MAX_EBOOK_TEXT_CHARS);
        extracted += 1;
    }

    if extracted == 0 {
        markdown.push_str("\n\n_No readable spine chapters were found. The archive listing is still available by opening the EPUB as a ZIP-compatible file._\n");
    }

    ebook_markdown_json("epub", &title, markdown)
}

fn parse_epub_rootfile(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut first = None;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) != "rootfile" {
                    continue;
                }
                let full_path = attr_value(&e, "full-path")?;
                if first.is_none() {
                    first = Some(full_path.clone());
                }
                let media_type = attr_value(&e, "media-type").unwrap_or_default();
                if media_type.contains("oebps-package") || full_path.ends_with(".opf") {
                    return Some(full_path);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    first
}

fn find_epub_opf_path(zip: &mut ZipArchive<fs::File>) -> Option<String> {
    for i in 0..zip.len().min(512) {
        let entry = zip.by_index_raw(i).ok()?;
        let name = entry.name().replace('\\', "/");
        if name.to_ascii_lowercase().ends_with(".opf") {
            return Some(name);
        }
    }
    None
}

fn parse_epub_opf(xml: &str) -> EpubOpf {
    let mut reader = Reader::from_str(xml);
    let mut opf = EpubOpf::default();
    let mut in_metadata = false;
    let mut current_meta = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "metadata" => in_metadata = true,
                    "item" => add_epub_manifest_item(&mut opf, &e),
                    "itemref" => {
                        if let Some(idref) = attr_value(&e, "idref") {
                            opf.spine.push(idref);
                        }
                    }
                    _ if in_metadata => current_meta = name,
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "item" => add_epub_manifest_item(&mut opf, &e),
                    "itemref" => {
                        if let Some(idref) = attr_value(&e, "idref") {
                            opf.spine.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_metadata && !current_meta.is_empty() => {
                set_epub_metadata(&mut opf, &current_meta, &xml_unescape_bytes(e.as_ref()));
            }
            Ok(Event::CData(e)) if in_metadata && !current_meta.is_empty() => {
                set_epub_metadata(
                    &mut opf,
                    &current_meta,
                    &String::from_utf8_lossy(e.as_ref()),
                );
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "metadata" {
                    in_metadata = false;
                }
                if name == current_meta {
                    current_meta.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    opf
}

fn add_epub_manifest_item(opf: &mut EpubOpf, e: &BytesStart<'_>) {
    let Some(id) = attr_value(e, "id") else {
        return;
    };
    let Some(href) = attr_value(e, "href") else {
        return;
    };
    let media_type = attr_value(e, "media-type").unwrap_or_default();
    opf.manifest
        .insert(id, EpubManifestItem { href, media_type });
}

fn set_epub_metadata(opf: &mut EpubOpf, name: &str, value: &str) {
    let value = collapse_ws(value);
    if value.is_empty() {
        return;
    }
    match name {
        "title" if opf.title.is_empty() => opf.title = value,
        "creator" if opf.creator.is_empty() => opf.creator = value,
        "language" if opf.language.is_empty() => opf.language = value,
        "publisher" if opf.publisher.is_empty() => opf.publisher = value,
        "identifier" if opf.identifier.is_empty() => opf.identifier = value,
        "date" if opf.date.is_empty() => opf.date = value,
        "description" if opf.description.is_empty() => opf.description = value,
        _ => {}
    }
}

fn is_epub_document_item(item: &EpubManifestItem) -> bool {
    let href = item.href.to_ascii_lowercase();
    item.media_type.contains("html")
        || href.ends_with(".xhtml")
        || href.ends_with(".html")
        || href.ends_with(".htm")
}

fn extract_xhtml_markdown(xml: &str, fallback_title: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_body = false;
    let mut ignored_depth = 0usize;
    let mut list_depth = 0usize;
    let mut current_block = String::new();
    let mut heading_level = 0usize;
    let mut saw_heading = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "body" {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }
                if matches!(name.as_str(), "script" | "style" | "svg" | "head") {
                    ignored_depth += 1;
                    continue;
                }
                if ignored_depth > 0 {
                    continue;
                }
                match name.as_str() {
                    "h1" => {
                        flush_ebook_block(&mut out, &mut current_block, 1, &mut saw_heading);
                        heading_level = 2;
                    }
                    "h2" => {
                        flush_ebook_block(&mut out, &mut current_block, 1, &mut saw_heading);
                        heading_level = 3;
                    }
                    "h3" | "h4" | "h5" | "h6" => {
                        flush_ebook_block(&mut out, &mut current_block, 1, &mut saw_heading);
                        heading_level = 4;
                    }
                    "p" | "div" | "section" | "blockquote" => {
                        flush_ebook_block(&mut out, &mut current_block, 0, &mut saw_heading);
                    }
                    "br" => current_block.push('\n'),
                    "ul" | "ol" => list_depth += 1,
                    "li" => {
                        flush_ebook_block(&mut out, &mut current_block, 0, &mut saw_heading);
                        current_block.push_str("- ");
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                if !in_body || ignored_depth > 0 {
                    continue;
                }
                let name = local_xml_name(e.name().as_ref());
                if name == "br" {
                    current_block.push('\n');
                }
            }
            Ok(Event::Text(e)) if in_body && ignored_depth == 0 => {
                append_text_word(&mut current_block, &xml_unescape_bytes(e.as_ref()));
            }
            Ok(Event::CData(e)) if in_body && ignored_depth == 0 => {
                append_text_word(&mut current_block, &String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "body" {
                    flush_ebook_block(&mut out, &mut current_block, 0, &mut saw_heading);
                    break;
                }
                if ignored_depth > 0 {
                    if matches!(name.as_str(), "script" | "style" | "svg" | "head") {
                        ignored_depth = ignored_depth.saturating_sub(1);
                    }
                    continue;
                }
                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            heading_level,
                            &mut saw_heading,
                        );
                        heading_level = 0;
                    }
                    "p" | "div" | "section" | "blockquote" | "li" => {
                        flush_ebook_block(&mut out, &mut current_block, 0, &mut saw_heading);
                    }
                    "ul" | "ol" => list_depth = list_depth.saturating_sub(1),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        if out.chars().count() > MAX_EBOOK_TEXT_CHARS {
            break;
        }
        let _ = list_depth;
    }

    flush_ebook_block(&mut out, &mut current_block, 0, &mut saw_heading);
    if !saw_heading && !out.trim().is_empty() {
        format!(
            "## {}\n\n{}",
            markdown_escape_line(fallback_title),
            out.trim()
        )
    } else {
        out.trim().to_string()
    }
}

fn flush_ebook_block(
    out: &mut String,
    current: &mut String,
    heading_level: usize,
    saw_heading: &mut bool,
) {
    let text = collapse_ws(current);
    current.clear();
    if text.is_empty() {
        return;
    }
    if !out.ends_with("\n\n") && !out.is_empty() {
        out.push_str("\n\n");
    }
    if heading_level > 0 {
        *saw_heading = true;
        out.push_str(&"#".repeat(heading_level));
        out.push(' ');
        out.push_str(&markdown_escape_line(&text));
    } else {
        out.push_str(&text);
    }
    out.push_str("\n\n");
}

fn render_fb2(path: &str) -> String {
    let filename = file_name(path);
    let Some(bytes) = read_file_prefix(path, MAX_EBOOK_XML_BYTES as usize) else {
        return String::new();
    };
    let xml = String::from_utf8_lossy(&bytes);
    let mut reader = Reader::from_str(&xml);
    let mut title = String::new();
    let mut lang = String::new();
    let mut author_parts = Vec::<String>::new();
    let mut current_meta = String::new();
    let mut in_title_info = false;
    let mut in_body = false;
    let mut current_block = String::new();
    let mut markdown = String::new();
    let mut saw_body_heading = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "title-info" => in_title_info = true,
                    "body" => in_body = true,
                    "section" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                    ),
                    "title" if in_body => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            0,
                            &mut saw_body_heading,
                        );
                        current_meta = "body-title".to_string();
                    }
                    "p" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                    ),
                    _ if in_title_info => current_meta = name,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let value = xml_unescape_bytes(e.as_ref());
                if in_body {
                    append_text_word(&mut current_block, &value);
                } else if in_title_info && !current_meta.is_empty() {
                    match current_meta.as_str() {
                        "book-title" if title.is_empty() => title = collapse_ws(&value),
                        "lang" if lang.is_empty() => lang = collapse_ws(&value),
                        "first-name" | "middle-name" | "last-name" | "nickname" => {
                            let part = collapse_ws(&value);
                            if !part.is_empty() {
                                author_parts.push(part);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::CData(e)) if in_body => {
                append_text_word(&mut current_block, &String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "title-info" => in_title_info = false,
                    "body" => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            0,
                            &mut saw_body_heading,
                        );
                        in_body = false;
                    }
                    "title" if current_meta == "body-title" => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            2,
                            &mut saw_body_heading,
                        );
                        current_meta.clear();
                    }
                    "p" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                    ),
                    _ if name == current_meta => current_meta.clear(),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        if markdown.chars().count() >= MAX_EBOOK_TEXT_CHARS {
            break;
        }
    }

    let title = first_non_empty_owned([title.as_str(), filename]).to_string();
    let author = author_parts.join(" ");
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&markdown_escape_line(&title));
    out.push_str("\n\n");
    append_metadata_line(&mut out, "Author", &author);
    append_metadata_line(&mut out, "Language", &lang);
    out.push('\n');
    push_markdown_limited(&mut out, markdown.trim(), MAX_EBOOK_TEXT_CHARS);
    ebook_markdown_json("fb2", &title, out)
}

fn render_binary_ebook_info(path: &str) -> String {
    let filename = file_name(path);
    let (size, modified_unix) = file_size_modified(path);
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let mut text = base_info_text(filename, "ebook", size, modified_unix);
    text.push_str(&format!("\nFormat: {ext} ebook"));
    text.push_str("\nContent preview: metadata only for this binary ebook container");
    to_json(&PreviewReadyDto {
        kind: "ebook".to_string(),
        title: format!("{filename} - ebook"),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn ebook_markdown_json(format: &str, title: &str, markdown: String) -> String {
    to_json(&PreviewReadyDto {
        kind: "ebook".to_string(),
        title: format!("{title} - {format}"),
        format: Some("markdown".to_string()),
        language: Some("markdown".to_string()),
        text: Some(markdown),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn append_metadata_line(markdown: &mut String, label: &str, value: &str) {
    let value = collapse_ws(value);
    if !value.is_empty() {
        markdown.push_str(&format!("**{label}:** {value}\n\n"));
    }
}

fn append_text_word(out: &mut String, value: &str) {
    let value = collapse_ws(value);
    if value.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with([' ', '\n']) {
        out.push(' ');
    }
    out.push_str(&value);
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn markdown_escape_line(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

fn ebook_item_label(href: &str) -> String {
    let filename = href.rsplit('/').next().unwrap_or(href);
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename);
    collapse_ws(&stem.replace(['_', '-'], " "))
}

fn push_markdown_limited(out: &mut String, value: &str, max_chars: usize) {
    let current = out.chars().count();
    if current >= max_chars {
        return;
    }
    let remaining = max_chars - current;
    let value_chars = value.chars().count();
    if value_chars <= remaining {
        out.push_str(value);
        return;
    }
    out.extend(value.chars().take(remaining));
    out.push_str("\n\n_Preview truncated._");
}

fn first_non_empty_owned<'a, const N: usize>(values: [&'a str; N]) -> &'a str {
    values
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("")
}

// ── Archive preview ──────────────────────────────────────────────────────────

const MAX_ARCHIVE_ENTRIES: usize = 5000;
const MAX_ARCHIVE_SCAN_ENTRIES: usize = 10_000;
const MAX_TAR_SCAN_BYTES: u64 = 512 * 1024 * 1024;
const TAR_SCAN_DEADLINE: Duration = Duration::from_secs(4);
const MAX_ARCHIVE_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACT_ROOTS: usize = 32;
const ARCHIVE_EXTRACT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

const ZIP_EXTS: &[&str] = &[
    ".zip",
    ".jar",
    ".apk",
    ".apks",
    ".aab",
    ".msix",
    ".msixbundle",
    ".appx",
    ".appxbundle",
    ".nupkg",
    ".vsix",
    ".whl",
    ".cbz",
    ".xpi",
];
const TAR_EXTS: &[&str] = &[".tar"];
const TAR_GZ_EXTS: &[&str] = &[".tar.gz", ".tgz"];
const GZ_EXTS: &[&str] = &[".gz"];

pub fn is_archive(ext: &str, kind: &str, magic: &[u8]) -> bool {
    if ZIP_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
        return true;
    }
    if TAR_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
        || TAR_GZ_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
        || GZ_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
    {
        return true;
    }
    (kind.eq_ignore_ascii_case("archive") || kind.eq_ignore_ascii_case("package"))
        && magic.len() >= 2
        && magic[0] == 0x50
        && magic[1] == 0x4B
}

/// Produce JSON for an archive listing: `{"kind":"archive","title":"...","listing":{...}}`.
pub fn render_archive(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let lower = path.to_ascii_lowercase();
    if is_package_path(&lower) {
        return render_package(path, cancel_cb);
    }
    if TAR_GZ_EXTS.iter().any(|e| lower.ends_with(e)) {
        return render_tar_gz_archive(path, cancel_cb);
    }
    if TAR_EXTS.iter().any(|e| lower.ends_with(e)) {
        return render_tar_archive(path, cancel_cb);
    }
    if GZ_EXTS.iter().any(|e| lower.ends_with(e)) && !lower.ends_with(".tar.gz") {
        return render_gzip_member(path);
    }
    render_zip_archive(path, cancel_cb)
}

pub fn extract_archive_entry_to_temp(
    archive_path: &str,
    entry_path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<String> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let lower = archive_path.to_ascii_lowercase();
    if TAR_EXTS.iter().any(|e| lower.ends_with(e))
        || TAR_GZ_EXTS.iter().any(|e| lower.ends_with(e))
        || (GZ_EXTS.iter().any(|e| lower.ends_with(e)) && !lower.ends_with(".tar.gz"))
    {
        return None;
    }

    let normalized = normalize_archive_entry_path(entry_path)?;
    let file = fs::File::open(archive_path).ok()?;
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let mut zip = ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name(&normalized).ok()?;
    if entry.is_dir() || entry.size() > MAX_ARCHIVE_EXTRACT_BYTES {
        return None;
    }

    let mut bytes = Vec::with_capacity((entry.size() as usize).min(1024 * 1024));
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        let read = entry.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        if bytes.len().checked_add(read)? > MAX_ARCHIVE_EXTRACT_BYTES as usize {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let root = create_archive_extract_root()?;
    let target = root.join(archive_extract_output_name(&normalized));
    let mut output = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(output) => output,
        Err(_) => {
            let _ = fs::remove_dir_all(&root);
            return None;
        }
    };
    if preview_cancelled(cancel_cb) {
        drop(output);
        let _ = fs::remove_dir_all(&root);
        return None;
    }
    if output.write_all(&bytes).is_err() {
        drop(output);
        let _ = fs::remove_dir_all(&root);
        return None;
    }
    drop(output);
    if preview_cancelled(cancel_cb) {
        let _ = fs::remove_dir_all(&root);
        return None;
    }
    target.to_str().map(|s| s.to_string())
}

fn read_office_zip_text<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    name: &str,
    max_size: u64,
) -> OfficeResult<Option<String>> {
    Ok(read_office_zip_bytes(context, zip, name, max_size)?
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string()))
}

fn read_office_zip_bytes<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    name: &str,
    max_size: u64,
) -> OfficeResult<Option<Vec<u8>>> {
    context.check_cancelled()?;
    if let Ok(mut entry) = zip.by_name(name) {
        if entry.size() > max_size {
            return Ok(None);
        }
        return read_office_limited_to_end(context, &mut entry, max_size);
    }

    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let mut entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };
        if !entry.name().replace('\\', "/").eq_ignore_ascii_case(name) {
            continue;
        }
        if entry.size() > max_size {
            return Ok(None);
        }
        return read_office_limited_to_end(context, &mut entry, max_size);
    }

    Ok(None)
}

fn read_office_limited_to_end<R: Read>(
    context: &mut OfficeContext,
    reader: &mut R,
    max_size: u64,
) -> OfficeResult<Option<Vec<u8>>> {
    let mut bytes = Vec::with_capacity(max_size.min(64 * 1024) as usize);
    let mut buffer = [0u8; 32 * 1024];
    loop {
        context.check_cancelled()?;
        let max_read = buffer.len().min(
            max_size
                .saturating_add(1)
                .saturating_sub(bytes.len() as u64) as usize,
        );
        if max_read == 0 {
            return Ok(None);
        }
        let read = match reader.read(&mut buffer[..max_read]) {
            Ok(read) => read,
            Err(_) => return Ok(None),
        };
        if read == 0 {
            return Ok(Some(bytes));
        }
        context.consume(read as u64)?;
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn normalize_archive_entry_path(path: &str) -> Option<String> {
    let path = path.replace('\\', "/").trim_start_matches('/').to_string();
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return None;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}

fn archive_extract_output_name(entry_path: &str) -> String {
    let mut name = String::with_capacity(entry_path.len().saturating_mul(2) + 6);
    name.push_str("entry-");
    for byte in entry_path.bytes() {
        use std::fmt::Write as _;
        let _ = write!(name, "{byte:02x}");
    }

    // Preserve conventional extensions so consumers can still select a preview provider.
    if let Some(extension) = Path::new(entry_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 32
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        name.push('.');
        name.push_str(extension);
    }
    name
}

fn archive_extract_base_path() -> PathBuf {
    std::env::temp_dir()
        .join("QuickLookNext")
        .join("archive-preview")
}

fn create_archive_extract_root() -> Option<PathBuf> {
    let base = archive_extract_base_path();
    fs::create_dir_all(&base).ok()?;
    cleanup_archive_extract_roots(&base, MAX_ARCHIVE_EXTRACT_ROOTS.saturating_sub(1));

    for _ in 0..16 {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).ok()?;
        let mut name = String::from("extract-");
        for byte in random {
            use std::fmt::Write as _;
            let _ = write!(name, "{byte:02x}");
        }
        let root = base.join(name);
        match fs::create_dir(&root) {
            Ok(()) => return Some(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

fn cleanup_archive_extract_roots(base: &Path, retain: usize) {
    let now = SystemTime::now();
    let mut roots = Vec::new();
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || !entry.file_name().to_string_lossy().starts_with("extract-") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        if modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > ARCHIVE_EXTRACT_RETENTION)
        {
            let _ = fs::remove_dir_all(entry.path());
        } else {
            roots.push((modified, entry.path()));
        }
    }
    roots.sort_by_key(|(modified, _)| *modified);
    let excess = roots.len().saturating_sub(retain);
    for (_, root) in roots.into_iter().take(excess) {
        let _ = fs::remove_dir_all(root);
    }
}

fn is_package_path(lower_path: &str) -> bool {
    [
        ".apk",
        ".apks",
        ".aab",
        ".msix",
        ".msixbundle",
        ".appx",
        ".appxbundle",
    ]
    .iter()
    .any(|e| lower_path.ends_with(e))
}

fn render_package(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let lower = path.to_ascii_lowercase();
    let platform = if lower.ends_with(".apk") || lower.ends_with(".apks") || lower.ends_with(".aab")
    {
        "Android"
    } else {
        "Windows"
    };

    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut zip = match ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => return String::new(),
    };

    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut uncompressed = 0i64;
    let mut compressed = 0i64;
    let mut has_icon = false;
    let mut has_manifest = false;
    let mut dex_count = 0u64;
    let mut certificate_count = 0u64;
    let mut native_abis = BTreeMap::<String, ()>::new();
    let mut appx_manifest: Option<String> = None;

    let partial = zip.len() > MAX_ARCHIVE_SCAN_ENTRIES;
    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return String::new();
        }
        let mut entry = match zip.by_index_raw(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().replace('\\', "/");
        let lower_name = name.to_ascii_lowercase();
        if lower_name.is_empty() {
            continue;
        }
        let is_folder = lower_name.ends_with('/');
        if is_folder {
            folder_count += 1;
        } else {
            file_count += 1;
            uncompressed += entry.size() as i64;
            compressed += entry.compressed_size() as i64;
        }

        if lower_name == "androidmanifest.xml" || lower_name.ends_with("/androidmanifest.xml") {
            has_manifest = true;
        }
        if lower_name == "appxmanifest.xml" && entry.size() <= MAX_APPX_MANIFEST_BYTES {
            has_manifest = true;
            if let Some(bytes) = read_limited_to_end(&mut entry, MAX_APPX_MANIFEST_BYTES) {
                appx_manifest = Some(String::from_utf8_lossy(&bytes).to_string());
            }
        }
        if lower_name.ends_with(".dex") {
            dex_count += 1;
        }
        if lower_name.starts_with("meta-inf/")
            && (lower_name.ends_with(".rsa")
                || lower_name.ends_with(".dsa")
                || lower_name.ends_with(".ec"))
        {
            certificate_count += 1;
        }
        if lower_name.starts_with("lib/") && lower_name.ends_with(".so") {
            if let Some(abi) = lower_name.split('/').nth(1) {
                if !abi.is_empty() {
                    native_abis.insert(abi.to_string(), ());
                }
            }
        }
        if package_icon_candidate_score(&name) > 0 {
            has_icon = true;
        }
    }

    let manifest = appx_manifest
        .as_deref()
        .and_then(parse_appx_manifest_summary);
    let display_name = manifest
        .as_ref()
        .and_then(|m| first_non_empty([m.display_name.as_deref(), m.name.as_deref()]))
        .unwrap_or(filename);
    let version = manifest
        .as_ref()
        .and_then(|m| m.version.as_deref())
        .unwrap_or("");
    let publisher = manifest
        .as_ref()
        .and_then(|m| m.publisher.as_deref())
        .unwrap_or("");
    let executable = manifest
        .as_ref()
        .and_then(|m| m.executable.as_deref())
        .unwrap_or("");

    let mut text = String::new();
    text.push_str(&format!("Name: {display_name}\n"));
    text.push_str(&format!("Kind: {platform} app package\n"));
    text.push_str(&format!("File: {filename}\n"));
    if !version.is_empty() {
        text.push_str(&format!("Version: {version}\n"));
    }
    if !publisher.is_empty() {
        text.push_str(&format!("Publisher: {publisher}\n"));
    }
    if !executable.is_empty() {
        text.push_str(&format!("Executable: {executable}\n"));
    }
    text.push_str(&format!("Files: {}\n", format_number(file_count as i64)));
    if folder_count > 0 {
        text.push_str(&format!(
            "Folders: {}\n",
            format_number(folder_count as i64)
        ));
    }
    text.push_str(&format!(
        "Uncompressed size: {}\n",
        format_bytes(uncompressed)
    ));
    if compressed > 0 {
        text.push_str(&format!("Package size: {}\n", format_bytes(compressed)));
    }
    text.push_str(&format!(
        "Manifest: {}\n",
        if has_manifest { "present" } else { "not found" }
    ));
    text.push_str(&format!(
        "Preview image: {}\n",
        if has_icon { "found" } else { "system fallback" }
    ));
    if partial {
        text.push_str("Listing scan stopped after 10,000 entries.\n");
    }

    if platform == "Android" {
        if dex_count > 0 {
            text.push_str(&format!("DEX files: {}\n", format_number(dex_count as i64)));
        }
        if !native_abis.is_empty() {
            text.push_str(&format!(
                "Native ABIs: {}\n",
                native_abis.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if certificate_count > 0 {
            text.push_str(&format!(
                "Signing blocks: {}\n",
                format_number(certificate_count as i64)
            ));
        }
    }

    to_json(&PreviewReadyDto {
        kind: "package".to_string(),
        title: if version.is_empty() {
            format!("{display_name} - {platform} package")
        } else {
            format!("{display_name} - {version}")
        },
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

#[derive(Default)]
struct AppxManifestSummary {
    name: Option<String>,
    version: Option<String>,
    publisher: Option<String>,
    display_name: Option<String>,
    executable: Option<String>,
    icon_paths: Vec<String>,
}

fn parse_appx_manifest_summary(xml: &str) -> Option<AppxManifestSummary> {
    let mut reader = Reader::from_str(xml);
    let mut summary = AppxManifestSummary::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .ok()?
                    .to_ascii_lowercase();
                for attr in e.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let value = attr
                        .unescape_value()
                        .ok()
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    if value.is_empty() {
                        continue;
                    }
                    match (name.as_str(), key.as_str()) {
                        ("identity", "name") => summary.name = Some(value),
                        ("identity", "version") => summary.version = Some(value),
                        ("identity", "publisher") => summary.publisher = Some(value),
                        ("application", "executable") => summary.executable = Some(value),
                        ("uap:visualelements", "displayname")
                        | ("visualelements", "displayname") => summary.display_name = Some(value),
                        ("uap:visualelements", "square150x150logo")
                        | ("visualelements", "square150x150logo")
                        | ("uap:visualelements", "square44x44logo")
                        | ("visualelements", "square44x44logo")
                        | ("uap:visualelements", "logo")
                        | ("visualelements", "logo")
                        | ("uap:defaulttile", "square310x310logo")
                        | ("defaulttile", "square310x310logo")
                        | ("uap:defaulttile", "wide310x150logo")
                        | ("defaulttile", "wide310x150logo") => summary.icon_paths.push(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(summary)
}

fn first_non_empty<'a, const N: usize>(values: [Option<&'a str>; N]) -> Option<&'a str> {
    values.into_iter().flatten().find(|v| !v.trim().is_empty())
}

pub fn extract_office_image_bgra(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let file = fs::File::open(path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let roots = office_media_roots_for_path(path);
    if roots.is_empty() {
        return None;
    }

    let mut candidates = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > MAX_OFFICE_MEDIA_BYTES {
            continue;
        }

        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let lower = normalized_name.to_ascii_lowercase();
        if !roots.iter().any(|root| lower.starts_with(root)) || !is_supported_zip_image_name(&lower)
        {
            continue;
        }

        candidates.push((office_image_candidate_score(&lower, entry.size()), raw_name));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    let mut context = OfficeContext::new(None);
    for (path_score, name) in candidates.into_iter().take(24) {
        let bytes = match read_office_zip_bytes(
            &mut context,
            &mut zip,
            &name,
            MAX_OFFICE_MEDIA_BYTES,
        ) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(OfficeReadError::BudgetExhausted) => break,
            Err(OfficeReadError::Cancelled) => break,
        };
        let image = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(_) => continue,
        };
        let (original_width, original_height) = image.dimensions();
        if original_width < 8 || original_height < 8 {
            continue;
        }

        let area_score = ((original_width.min(768) * original_height.min(768)) / 512) as i32;
        let score = path_score + area_score;
        let Some((width, height, bgra)) = image_to_bgra(image, 768) else {
            continue;
        };
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, width, height, bgra));
        }
    }

    best.map(|(_, width, height, bgra)| (width, height, bgra))
}

fn office_media_roots_for_path(path: &str) -> &'static [&'static str] {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" => &["word/media/"],
        "xlsx" | "xlsm" => &["xl/media/"],
        "pptx" | "pptm" => &["ppt/media/"],
        _ => &[],
    }
}

fn office_image_candidate_score(lower: &str, size: u64) -> i32 {
    let mut score = 0;
    if lower.ends_with(".png") {
        score += 30;
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        score += 24;
    } else if lower.ends_with(".webp") {
        score += 18;
    } else if lower.ends_with(".bmp") {
        score += 10;
    }
    if lower.contains("image") {
        score += 8;
    }
    score + ((size.min(4 * 1024 * 1024) / 4096) as i32).min(256)
}

pub fn extract_package_icon_bgra(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let file = fs::File::open(path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let mut candidates = Vec::new();
    let manifest_icons = read_zip_text(&mut zip, "AppxManifest.xml", MAX_APPX_MANIFEST_BYTES)
        .as_deref()
        .and_then(parse_appx_manifest_summary)
        .map(|summary| expand_appx_icon_candidates(&summary.icon_paths))
        .unwrap_or_default();

    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).ok()?;
        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let score = package_icon_candidate_score(&normalized_name)
            + manifest_icon_candidate_score(&normalized_name, &manifest_icons);
        if score > 0 && entry.size() <= MAX_PACKAGE_ICON_BYTES {
            candidates.push((score, raw_name));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    for (path_score, name) in candidates.into_iter().take(32) {
        let mut entry = zip.by_name(&name).ok()?;
        let Some(bytes) = read_limited_to_end(&mut entry, MAX_PACKAGE_ICON_BYTES) else {
            continue;
        };
        let image = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(_) => continue,
        };
        let (original_width, original_height) = image.dimensions();
        if original_width < 16 || original_height < 16 {
            continue;
        }
        let area_score = ((original_width.min(512) * original_height.min(512)) / 256) as i32;
        let score = path_score + area_score;
        let (width, height, bgra) = image_to_bgra(image, 512)?;
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, width, height, bgra));
        }
    }

    best.map(|(_, width, height, bgra)| (width, height, bgra))
}

fn expand_appx_icon_candidates(paths: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();
    for path in paths {
        let normalized = path
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        candidates.push(normalized.clone());
        let Some((stem, ext)) = normalized.rsplit_once('.') else {
            continue;
        };
        for qualifier in [
            ".scale-400",
            ".scale-200",
            ".scale-150",
            ".scale-125",
            ".scale-100",
            ".targetsize-256",
            ".targetsize-128",
            ".targetsize-96",
            ".targetsize-64",
            ".targetsize-48",
            ".targetsize-32",
            ".targetsize-24",
            ".targetsize-16",
            ".altform-unplated_targetsize-256",
            ".altform-unplated_targetsize-48",
        ] {
            candidates.push(format!("{stem}{qualifier}.{ext}"));
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn manifest_icon_candidate_score(name: &str, manifest_icons: &[String]) -> i32 {
    if manifest_icons.is_empty() {
        return 0;
    }
    let lower = name
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if manifest_icons.iter().any(|candidate| candidate == &lower) {
        return 320;
    }

    let Some((stem, _)) = lower.rsplit_once('.') else {
        return 0;
    };
    manifest_icons
        .iter()
        .filter_map(|candidate| {
            candidate
                .rsplit_once('.')
                .map(|(candidate_stem, _)| candidate_stem)
        })
        .any(|candidate_stem| stem.starts_with(candidate_stem))
        .then_some(260)
        .unwrap_or(0)
}

fn package_icon_candidate_score(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    if !is_supported_zip_image_name(&lower) {
        return 0;
    }

    let mut score = 0;
    if lower.contains("ic_launcher") {
        score += 260;
    }
    if lower.contains("square150x150logo") {
        score += 240;
    }
    if lower.contains("square44x44logo") {
        score += 220;
    }
    if lower.contains("storelogo") {
        score += 210;
    }
    if lower.contains("appicon") {
        score += 190;
    }
    if lower.contains("logo") {
        score += 160;
    }
    if lower.contains("icon") {
        score += 140;
    }
    if score == 0 {
        return 0;
    }

    if lower.starts_with("assets/") || lower.contains("/assets/") {
        score += 30;
    }
    if lower.contains("/mipmap") || lower.starts_with("res/mipmap") {
        score += 30;
    }
    if lower.contains("/drawable") || lower.starts_with("res/drawable") {
        score += 15;
    }
    if lower.contains("scale-400") {
        score += 24;
    } else if lower.contains("scale-200") {
        score += 18;
    } else if lower.contains("scale-150") {
        score += 12;
    } else if lower.contains("scale-100") {
        score += 6;
    }
    if lower.ends_with(".png") {
        score += 8;
    }
    score
}

fn is_supported_zip_image_name(lower: &str) -> bool {
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".ico")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
}

fn image_to_bgra(image: image::DynamicImage, max_dimension: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        return None;
    }

    let largest = original_width.max(original_height);
    let scale = if largest > max_dimension {
        max_dimension as f64 / largest as f64
    } else {
        1.0
    };
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let raster = if width == original_width && height == original_height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };

    let rgba = raster.to_rgba8();
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for px in rgba.chunks_exact(4) {
        let r = px[0] as u32;
        let g = px[1] as u32;
        let b = px[2] as u32;
        let a = px[3] as u32;
        bgra.push(((b * a + 127) / 255) as u8);
        bgra.push(((g * a + 127) / 255) as u8);
        bgra.push(((r * a + 127) / 255) as u8);
        bgra.push(a as u8);
    }
    Some((width, height, bgra))
}

// ── Torrent preview ─────────────────────────────────────────────────────────

pub fn render_torrent(path: &str) -> String {
    let (size, modified_unix) = file_size_modified(path);
    if size < 0 || size as u64 > MAX_TORRENT_BYTES {
        return render_info(path, "torrent", size, modified_unix);
    }

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let root = match parse_bencode(&bytes) {
        Some((value, _)) => value,
        None => return String::new(),
    };
    let dict = match root {
        BValue::Dict(d) => d,
        _ => return String::new(),
    };

    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let announce = dict_get_string(&dict, b"announce").unwrap_or_default();
    let created_by = dict_get_string(&dict, b"created by").unwrap_or_default();
    let creation_date = dict_get_int(&dict, b"creation date").unwrap_or(0);
    let comment = dict_get_string(&dict, b"comment").unwrap_or_default();
    let info = match dict.get(b"info".as_slice()) {
        Some(BValue::Dict(d)) => d,
        _ => return String::new(),
    };

    let name = dict_get_string(info, b"name").unwrap_or_else(|| filename.to_string());
    let piece_length = dict_get_int(info, b"piece length").unwrap_or(0);
    let pieces = match info.get(b"pieces".as_slice()) {
        Some(BValue::Bytes(b)) => b.len() / 20,
        _ => 0,
    };

    let mut entries: BTreeMap<String, (String, String, bool, i64, i64, i64)> = BTreeMap::new();
    let mut total_size = 0i64;
    let mut file_count = 0u64;
    let mut partial = false;

    if let Some(BValue::List(files)) = info.get(b"files".as_slice()) {
        for file in files {
            let BValue::Dict(file_dict) = file else {
                continue;
            };
            let size = dict_get_int(file_dict, b"length").unwrap_or(0);
            let path_parts = match file_dict.get(b"path".as_slice()) {
                Some(BValue::List(parts)) => parts
                    .iter()
                    .filter_map(|p| match p {
                        BValue::Bytes(b) => Some(bytes_to_lossy(b)),
                        _ => None,
                    })
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            if path_parts.is_empty() {
                continue;
            }
            let full_name = path_parts.join("/");
            total_size += size;
            file_count += 1;
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            add_parent_folders(&full_name, &mut entries);
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            let item_name = path_parts
                .last()
                .cloned()
                .unwrap_or_else(|| full_name.clone());
            entries.insert(
                full_name.clone(),
                (item_name, parent_of(&full_name), false, size, 0, 0),
            );
        }
    } else if let Some(length) = dict_get_int(info, b"length") {
        total_size = length;
        file_count = 1;
        entries.insert(
            name.clone(),
            (name.clone(), String::new(), false, length, 0, 0),
        );
    }

    let mut text = String::new();
    text.push_str(&format!("Name: {name}\n"));
    text.push_str(&format!("Files: {}\n", format_number(file_count as i64)));
    text.push_str(&format!("Total size: {}\n", format_bytes(total_size)));
    if piece_length > 0 {
        text.push_str(&format!("Piece length: {}\n", format_bytes(piece_length)));
    }
    if pieces > 0 {
        text.push_str(&format!("Pieces: {}\n", format_number(pieces as i64)));
    }
    if !announce.is_empty() {
        text.push_str(&format!("Tracker: {announce}\n"));
    }
    if creation_date > 0 {
        text.push_str(&format!("Created: {}\n", format_timestamp(creation_date)));
    }
    if !created_by.is_empty() {
        text.push_str(&format!("Created by: {created_by}\n"));
    }
    if !comment.is_empty() {
        text.push_str(&format!("Comment: {comment}\n"));
    }

    let mut items = Vec::with_capacity(entries.len());
    for (path, (name, parent, is_folder, size, packed, modified)) in &entries {
        items.push(PreviewListingItemDto {
            name: name.clone(),
            path: path.clone(),
            parent_path: parent.clone(),
            is_folder: *is_folder,
            size: *size,
            packed_size: *packed,
            modified_unix: *modified,
            typ: if *is_folder {
                "Folder".to_string()
            } else {
                type_for_ext(name).to_string()
            },
            native_path: None,
        });
    }

    let mut summary = format!(
        "{} files - {}",
        format_number(file_count as i64),
        format_bytes(total_size)
    );
    if !announce.is_empty() {
        summary.push_str(&format!(" - {announce}"));
    }

    to_json(&PreviewReadyDto {
        kind: "torrent".to_string(),
        title: format!("{name} - {} files", format_number(file_count as i64)),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: Some(PreviewListingDto {
            root_name: name,
            root_path: String::new(),
            listing_kind: "torrent".to_string(),
            summary,
            is_partial: partial,
            items,
        }),
        table: None,
        markdown: None,
    })
}

fn parse_bencode(bytes: &[u8]) -> Option<(BValue, usize)> {
    parse_bencode_at(bytes, 0)
}

fn parse_bencode_at(bytes: &[u8], mut i: usize) -> Option<(BValue, usize)> {
    match *bytes.get(i)? {
        b'i' => {
            i += 1;
            let end = bytes[i..].iter().position(|b| *b == b'e')? + i;
            let n = std::str::from_utf8(&bytes[i..end])
                .ok()?
                .parse::<i64>()
                .ok()?;
            Some((BValue::Int(n), end + 1))
        }
        b'l' => {
            i += 1;
            let mut values = Vec::new();
            while *bytes.get(i)? != b'e' {
                let (value, next) = parse_bencode_at(bytes, i)?;
                values.push(value);
                i = next;
            }
            Some((BValue::List(values), i + 1))
        }
        b'd' => {
            i += 1;
            let mut values = BTreeMap::new();
            while *bytes.get(i)? != b'e' {
                let (key, next) = parse_bytes_at(bytes, i)?;
                let (value, next) = parse_bencode_at(bytes, next)?;
                values.insert(key, value);
                i = next;
            }
            Some((BValue::Dict(values), i + 1))
        }
        b'0'..=b'9' => {
            let (value, next) = parse_bytes_at(bytes, i)?;
            Some((BValue::Bytes(value), next))
        }
        _ => None,
    }
}

fn parse_bytes_at(bytes: &[u8], i: usize) -> Option<(Vec<u8>, usize)> {
    let colon = bytes[i..].iter().position(|b| *b == b':')? + i;
    let len = std::str::from_utf8(&bytes[i..colon])
        .ok()?
        .parse::<usize>()
        .ok()?;
    let start = colon + 1;
    let end = start.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    Some((bytes[start..end].to_vec(), end))
}

fn dict_get_int(dict: &BTreeMap<Vec<u8>, BValue>, key: &[u8]) -> Option<i64> {
    match dict.get(key) {
        Some(BValue::Int(n)) => Some(*n),
        _ => None,
    }
}

fn dict_get_string(dict: &BTreeMap<Vec<u8>, BValue>, key: &[u8]) -> Option<String> {
    match dict.get(key) {
        Some(BValue::Bytes(b)) => Some(bytes_to_lossy(b)),
        _ => None,
    }
}

fn bytes_to_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches(char::from(0))
        .to_string()
}

fn render_zip_archive(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut zip = match ZipArchive::new(file) {
        Ok(z) => z,
        Err(_) => return String::new(),
    };

    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let mut entries: BTreeMap<String, (String, String, bool, i64, i64, i64)> = BTreeMap::new();
    // key: virtual path → (name, parent, is_folder, size, packed_size, modified_unix)
    let mut file_count = 0u64;
    let mut uncompressed = 0i64;
    let mut compressed = 0i64;
    let mut seen = 0usize;
    let mut partial = false;

    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return String::new();
        }
        let entry = match zip.by_index_raw(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let full_name = entry
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if full_name.is_empty() {
            continue;
        }
        let is_folder = full_name.ends_with('/') || entry.name().is_empty();
        let size = entry.size() as i64;
        let packed = entry.compressed_size() as i64;
        let modified = entry
            .last_modified()
            .map(|d| {
                // zip::DateTime → unix seconds (approximate: no leap seconds, no TZ)
                let secs = ((d.year() as i64 - 1970) * 365 * 86400)
                    + ((d.month() as i64 - 1) * 30 * 86400)
                    + ((d.day() as i64 - 1) * 86400);
                secs
            })
            .unwrap_or(0);
        drop(entry);

        if is_folder {
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            add_parent_folders(&full_name, &mut entries);
            let path = ensure_trailing_slash(&full_name);
            if !entries.contains_key(&path) {
                let name = path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                entries.insert(path.clone(), (name, parent_of(&path), true, 0, 0, 0));
            }
        } else {
            file_count += 1;
            uncompressed += size;
            compressed += packed;
            if seen < MAX_ARCHIVE_ENTRIES && entries.len() < MAX_ARCHIVE_ENTRIES {
                add_parent_folders(&full_name, &mut entries);
                if entries.len() >= MAX_ARCHIVE_ENTRIES {
                    partial = true;
                    continue;
                }
                let name = full_name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&full_name)
                    .to_string();
                entries.insert(
                    full_name.clone(),
                    (name, parent_of(&full_name), false, size, packed, modified),
                );
                seen += 1;
            } else {
                partial = true;
            }
        }
    }
    if zip.len() > MAX_ARCHIVE_SCAN_ENTRIES {
        partial = true;
    }

    archive_listing_json(
        filename,
        path,
        "archive",
        entries,
        file_count,
        uncompressed,
        compressed,
        partial,
    )
}

fn render_tar_archive(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    render_tar_entries(path, "archive", file, cancel_cb)
}

fn render_tar_gz_archive(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    render_tar_entries(path, "archive", GzDecoder::new(file), cancel_cb)
}

struct TarScanReader<R> {
    reader: R,
    remaining: u64,
    deadline: Instant,
    cancel_cb: Option<extern "C" fn() -> bool>,
}

impl<R> TarScanReader<R> {
    fn new(reader: R, cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
        Self {
            reader,
            remaining: MAX_TAR_SCAN_BYTES,
            deadline: Instant::now() + TAR_SCAN_DEADLINE,
            cancel_cb,
        }
    }

    fn stopped(&self) -> bool {
        self.remaining == 0 || Instant::now() >= self.deadline || preview_cancelled(self.cancel_cb)
    }
}

impl<R: Read> Read for TarScanReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.stopped() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "tar scan budget reached",
            ));
        }
        let limit = self.remaining.min(buf.len() as u64) as usize;
        let read = self.reader.read(&mut buf[..limit])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn render_tar_entries<R: Read>(
    path: &str,
    kind: &str,
    reader: R,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let mut archive = TarArchive::new(TarScanReader::new(reader, cancel_cb));
    let mut entries: BTreeMap<String, (String, String, bool, i64, i64, i64)> = BTreeMap::new();
    let mut file_count = 0u64;
    let mut uncompressed = 0i64;
    let mut seen = 0usize;
    let mut partial = false;

    let archive_entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    let mut scanned = 0usize;
    for entry in archive_entries {
        if preview_cancelled(cancel_cb) {
            return String::new();
        }
        if scanned == MAX_ARCHIVE_SCAN_ENTRIES {
            partial = true;
            break;
        }
        scanned += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                partial = true;
                break;
            }
        };
        let path_buf = match entry.path() {
            Ok(p) => p.into_owned(),
            Err(_) => continue,
        };
        let full_name = path_buf
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if full_name.is_empty() {
            continue;
        }

        let is_folder = entry.header().entry_type().is_dir() || full_name.ends_with('/');
        let size = if is_folder {
            0
        } else {
            entry.header().size().unwrap_or(0) as i64
        };
        let modified = entry.header().mtime().unwrap_or(0) as i64;
        if is_folder {
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            add_parent_folders(&full_name, &mut entries);
            let folder_path = ensure_trailing_slash(&full_name);
            if entries.len() < MAX_ARCHIVE_ENTRIES && !entries.contains_key(&folder_path) {
                let name = folder_path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                entries.insert(
                    folder_path.clone(),
                    (name, parent_of(&folder_path), true, 0, 0, modified),
                );
            }
        } else {
            file_count += 1;
            uncompressed += size;
            if seen < MAX_ARCHIVE_ENTRIES && entries.len() < MAX_ARCHIVE_ENTRIES {
                add_parent_folders(&full_name, &mut entries);
                if entries.len() >= MAX_ARCHIVE_ENTRIES {
                    partial = true;
                    continue;
                }
                let name = full_name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&full_name)
                    .to_string();
                entries.insert(
                    full_name.clone(),
                    (name, parent_of(&full_name), false, size, 0, modified),
                );
                seen += 1;
            } else {
                partial = true;
            }
        }
    }

    archive_listing_json(
        filename,
        path,
        kind,
        entries,
        file_count,
        uncompressed,
        0,
        partial,
    )
}

fn render_gzip_member(path: &str) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let member_name = filename
        .strip_suffix(".gz")
        .or_else(|| filename.strip_suffix(".GZ"))
        .filter(|s| !s.is_empty())
        .unwrap_or(filename);
    let compressed = fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    let uncompressed = gzip_uncompressed_size(path).unwrap_or(0);
    let modified = fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut entries = BTreeMap::new();
    entries.insert(
        member_name.to_string(),
        (
            member_name.to_string(),
            String::new(),
            false,
            uncompressed,
            compressed,
            modified,
        ),
    );
    archive_listing_json(
        filename,
        path,
        "archive",
        entries,
        1,
        uncompressed,
        compressed,
        false,
    )
}

fn gzip_uncompressed_size(path: &str) -> Option<i64> {
    let mut file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() < 4 {
        return None;
    }
    file.seek(SeekFrom::End(-4)).ok()?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    Some(u32::from_le_bytes(buf) as i64)
}

fn archive_listing_json(
    filename: &str,
    root_path: &str,
    kind: &str,
    entries: BTreeMap<String, (String, String, bool, i64, i64, i64)>,
    file_count: u64,
    uncompressed: i64,
    compressed: i64,
    partial: bool,
) -> String {
    let folder_count = entries
        .values()
        .filter(|(_, _, is_folder, _, _, _)| *is_folder)
        .count();
    let mut summary = format!(
        "{} files, {} folders",
        format_number(file_count as i64),
        format_number(folder_count as i64)
    );
    if uncompressed > 0 {
        summary.push_str(&format!(" - {} uncompressed", format_bytes(uncompressed)));
        if compressed > 0 {
            let saved = 100.0 - (compressed as f64 * 100.0 / uncompressed as f64);
            summary.push_str(&format!(" - {:.1}% saved", saved.clamp(0.0, 100.0)));
        }
    }
    if let Some(types) = archive_type_summary(&entries) {
        summary.push_str(&format!(" - Types: {types}"));
    }
    if let Some(projects) = archive_project_summary(&entries) {
        summary.push_str(&format!(" - Project markers: {projects}"));
    }

    let mut items = Vec::with_capacity(entries.len());
    for (path, (name, parent, is_folder, size, packed, modified)) in &entries {
        let typ = if *is_folder {
            "Folder"
        } else {
            type_for_ext(name)
        };
        items.push(PreviewListingItemDto {
            name: name.clone(),
            path: path.clone(),
            parent_path: parent.clone(),
            is_folder: *is_folder,
            size: *size,
            packed_size: *packed,
            modified_unix: *modified,
            typ: typ.to_string(),
            native_path: None,
        });
    }

    to_json(&PreviewReadyDto {
        kind: kind.to_string(),
        title: format!(
            "{filename} - {} entries",
            format_number(file_count as i64 + folder_count as i64)
        ),
        format: None,
        language: None,
        text: None,
        office_layout: None,
        listing: Some(PreviewListingDto {
            root_name: filename.to_string(),
            root_path: root_path.to_string(),
            listing_kind: "archive".to_string(),
            summary,
            is_partial: partial,
            items,
        }),
        table: None,
        markdown: None,
    })
}

fn archive_type_summary(
    entries: &BTreeMap<String, (String, String, bool, i64, i64, i64)>,
) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for (name, _, is_folder, _, _, _) in entries.values() {
        if *is_folder {
            continue;
        }
        *counts.entry(type_for_ext(name).to_string()).or_default() += 1;
    }
    if counts.is_empty() {
        return None;
    }
    let mut pairs = counts.into_iter().collect::<Vec<_>>();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Some(
        pairs
            .into_iter()
            .take(4)
            .map(|(typ, count)| format!("{typ} {count}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn archive_project_summary(
    entries: &BTreeMap<String, (String, String, bool, i64, i64, i64)>,
) -> Option<String> {
    let mut markers = Vec::<String>::new();
    for (name, _, is_folder, _, _, _) in entries.values() {
        if *is_folder {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        let label = match lower.as_str() {
            "package.json" => Some("package.json"),
            "cargo.toml" => Some("Cargo.toml"),
            "pyproject.toml" => Some("pyproject.toml"),
            "go.mod" => Some("go.mod"),
            "pom.xml" => Some("pom.xml"),
            "composer.json" => Some("composer.json"),
            "gemfile" => Some("Gemfile"),
            "makefile" => Some("Makefile"),
            "dockerfile" => Some("Dockerfile"),
            _ if lower.ends_with(".sln") => Some(".sln"),
            _ if lower.ends_with(".csproj") => Some(".csproj"),
            _ => None,
        };
        if let Some(label) = label {
            if !markers.iter().any(|existing| existing == label) {
                markers.push(label.to_string());
            }
        }
    }
    if markers.is_empty() {
        None
    } else {
        markers.sort();
        Some(markers.into_iter().take(6).collect::<Vec<_>>().join(", "))
    }
}

fn add_parent_folders(
    path: &str,
    entries: &mut BTreeMap<String, (String, String, bool, i64, i64, i64)>,
) {
    let mut start = 0;
    while let Some(idx) = path[start..].find('/') {
        let full_idx = start + idx;
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            return;
        }
        let folder_path = format!("{}/", &path[..full_idx]);
        if !entries.contains_key(&folder_path) {
            let name = path[..full_idx]
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            entries.insert(
                folder_path.clone(),
                (name, parent_of(&folder_path), true, 0, 0, 0),
            );
        }
        start = full_idx + 1;
    }
}

fn ensure_trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{}/", s)
    }
}

fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(idx) => trimmed[..idx + 1].to_string(),
        None => String::new(),
    }
}

fn type_for_ext(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext.is_empty() {
        return "File";
    }
    // Leak is fine — these are tiny static strings. But we can't return owned String,
    // so we use a match on common extensions and fall back to "File".
    match ext.to_ascii_lowercase().as_str() {
        "txt" | "log" => "TXT File",
        "md" => "MD File",
        "json" => "JSON File",
        "xml" => "XML File",
        "png" => "PNG File",
        "jpg" | "jpeg" => "JPEG File",
        "gif" => "GIF File",
        "bmp" => "BMP File",
        "pdf" => "PDF File",
        "zip" => "ZIP File",
        "jar" => "JAR File",
        "apk" => "APK File",
        "apks" => "APKS File",
        "aab" => "Android App Bundle",
        "msix" => "MSIX Package",
        "msixbundle" => "MSIX Bundle",
        "appx" => "APPX Package",
        "appxbundle" => "APPX Bundle",
        "torrent" => "Torrent File",
        "img" => "Disk Image",
        "epub" => "EPUB Book",
        "fb2" => "FB2 Book",
        "mobi" => "MOBI Book",
        "azw" | "azw3" => "Kindle Book",
        "nupkg" => "NuGet Package",
        "vsix" => "VSIX Package",
        "whl" => "Python Wheel",
        "cbz" => "CBZ File",
        "xpi" => "XPI File",
        "tar" => "TAR File",
        "tgz" => "TGZ File",
        "gz" => "GZIP File",
        "docx" => "DOCX File",
        "xlsx" => "XLSX File",
        "pptx" => "PPTX File",
        "mp4" => "MP4 File",
        "mp3" => "MP3 File",
        "exe" => "Application",
        "dll" => "Application Extension",
        "sys" => "System File",
        "scr" => "Screen Saver",
        "cs" => "CS File",
        "rs" => "RS File",
        "py" => "PY File",
        "js" => "JS File",
        "ts" => "TS File",
        "html" | "htm" => "HTML File",
        "css" => "CSS File",
        _ => "File",
    }
}

// ── Folder preview ───────────────────────────────────────────────────────────

const MAX_FOLDER_ITEMS: usize = 5000;

/// Produce JSON for a folder listing: `{"kind":"folder","title":"...","listing":{...}}`.
pub fn render_folder(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let root_name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let root_full = Path::new(path)
        .canonicalize()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| path.to_string());

    let mut items = Vec::new();
    let mut total_bytes = 0i64;
    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut skipped = 0u64;
    let mut partial = false;

    // Directories first
    if let Ok(dirs) = fs::read_dir(path) {
        for entry in dirs.flatten() {
            if preview_cancelled(cancel_cb) {
                return String::new();
            }
            if items.len() >= MAX_FOLDER_ITEMS {
                partial = true;
                break;
            }
            let entry_path = entry.path();
            if entry_path.is_dir() {
                if let Ok(meta) = entry.metadata() {
                    folder_count += 1;
                    let name = entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let native = entry_path.to_string_lossy().to_string();
                    let modified = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let virtual_path = format!("{}/", name);
                    items.push(PreviewListingItemDto {
                        name,
                        path: virtual_path,
                        parent_path: String::new(),
                        is_folder: true,
                        size: 0,
                        packed_size: 0,
                        modified_unix: modified,
                        typ: "Folder".to_string(),
                        native_path: Some(native),
                    });
                }
            }
        }
    } else {
        skipped += 1;
    }

    // Files
    if !partial {
        if let Ok(files) = fs::read_dir(path) {
            for entry in files.flatten() {
                if preview_cancelled(cancel_cb) {
                    return String::new();
                }
                if items.len() >= MAX_FOLDER_ITEMS {
                    partial = true;
                    break;
                }
                let entry_path = entry.path();
                if entry_path.is_file() {
                    if let Ok(meta) = entry.metadata() {
                        file_count += 1;
                        let size = meta.len() as i64;
                        total_bytes += size;
                        let name = entry_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        let native = entry_path.to_string_lossy().to_string();
                        let modified = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        let typ = type_for_ext(&name).to_string();
                        items.push(PreviewListingItemDto {
                            name: name.clone(),
                            path: name,
                            parent_path: String::new(),
                            is_folder: false,
                            size,
                            packed_size: 0,
                            modified_unix: modified,
                            typ,
                            native_path: Some(native),
                        });
                    }
                }
            }
        } else {
            skipped += 1;
        }
    }

    // Sort: folders first, then by name (case-insensitive)
    items.sort_by(|a, b| {
        b.is_folder.cmp(&a.is_folder).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });

    let mut summary = format!(
        "{} files, {} folders - {}",
        format_number(file_count as i64),
        format_number(folder_count as i64),
        format_bytes(total_bytes)
    );
    if skipped > 0 {
        summary.push_str(&format!(
            " - {} inaccessible",
            format_number(skipped as i64)
        ));
    }
    if partial {
        summary.push_str(" - partial");
    }

    to_json(&PreviewReadyDto {
        kind: "folder".to_string(),
        title: format!(
            "{root_name} - {} files, {} folders",
            format_number(file_count as i64),
            format_number(folder_count as i64)
        ),
        format: None,
        language: None,
        text: None,
        office_layout: None,
        listing: Some(PreviewListingDto {
            root_name: root_name.to_string(),
            root_path: root_full,
            listing_kind: "folder".to_string(),
            summary,
            is_partial: partial,
            items,
        }),
        table: None,
        markdown: None,
    })
}

// ── Shared helpers ───────────────────────────────────────────────────────────

fn format_number(n: i64) -> String {
    // Thousands separator (comma)
    let abs = n.unsigned_abs();
    let s = abs.to_string();
    let chars: Vec<char> = s.chars().rev().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    let result: String = out.chars().rev().collect();
    if n < 0 {
        format!("-{}", result)
    } else {
        result
    }
}

fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", format_number(bytes))
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

fn format_timestamp(unix: i64) -> String {
    if unix == 0 {
        return "—".to_string();
    }
    // Simple conversion without chrono: compute date from unix seconds.
    // Use Windows GetLocalTime for accuracy.
    let secs = unix as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600).rem_euclid(24);
    let minute = (time_of_day % 3600) / 60;
    // Compute date from days since 1970-01-01
    let (year, month, day) = days_to_date(days);
    format!("{}/{:02}/{} {:02}:{:02}", year, month, day, hour, minute)
}

fn days_to_date(days_since_epoch: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn test_office_context() -> OfficeContext {
        OfficeContext::new(None)
    }

    #[test]
    fn text_preview_decodes_windows_1252_config() {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-text-{}.ini",
            std::process::id()
        ));
        std::fs::write(&path, b"name=caf\xE9").expect("write Windows-1252 config");
        let json = render_text(path.to_str().unwrap());
        let _ = std::fs::remove_file(path);

        assert!(json.contains("name=café"));
        assert!(json.contains("\"language\":\"ini\""));
    }

    #[test]
    fn utf8_text_truncation_stays_on_char_boundary() {
        let mut bytes = vec![b'a'; MAX_TEXT_BYTES - 1];
        bytes.extend_from_slice("中".as_bytes());
        bytes.truncate(MAX_TEXT_BYTES);

        trim_text_bytes_to_safe_boundary(&mut bytes);

        assert_eq!(bytes.len(), MAX_TEXT_BYTES - 1);
        assert!(std::str::from_utf8(&bytes).is_ok());
    }

    #[test]
    fn utf16_text_truncation_drops_half_code_unit() {
        let mut bytes = vec![0xFF, 0xFE, 0x41];

        trim_text_bytes_to_safe_boundary(&mut bytes);

        assert_eq!(bytes, vec![0xFF, 0xFE]);
    }

    #[test]
    fn xml_unescape_supports_named_and_numeric_entities() {
        assert_eq!(
            xml_unescape_str("A&#65;&#x41;&lt;&gt;&amp;&quot;&apos;&unknown;"),
            "AAA<>&\"'&unknown;"
        );
    }

    #[test]
    fn limited_reader_rejects_payloads_over_cap() {
        let mut reader = Cursor::new(vec![1, 2, 3, 4, 5]);

        assert!(read_limited_to_end(&mut reader, 4).is_none());
    }

    #[test]
    fn office_reads_share_a_decompression_budget() {
        let mut context = OfficeContext {
            remaining_decompressed_bytes: 4,
            cancel_cb: None,
        };
        let mut first = Cursor::new(vec![1, 2, 3]);
        let mut second = Cursor::new(vec![4, 5]);

        assert_eq!(
            read_office_limited_to_end(&mut context, &mut first, 3)
                .expect("first read")
                .expect("first entry"),
            vec![1, 2, 3]
        );
        assert!(matches!(
            read_office_limited_to_end(&mut context, &mut second, 2),
            Err(OfficeReadError::BudgetExhausted)
        ));
    }

    #[test]
    fn office_read_honors_cancellation() {
        let mut context = OfficeContext::new(Some(always_cancel));
        let mut reader = Cursor::new(vec![1]);

        assert!(matches!(
            read_office_limited_to_end(&mut context, &mut reader, 1),
            Err(OfficeReadError::Cancelled)
        ));
    }

    #[test]
    fn office_entry_scans_honor_cancellation() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()));
        writer
            .start_file(
                "word/media/image.png",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("media file");
        writer.write_all(&[0]).expect("media bytes");
        let mut cursor = writer.finish().expect("zip bytes");
        cursor.set_position(0);
        let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
        let mut context = OfficeContext::new(Some(always_cancel));

        assert!(matches!(
            office_media_entries(&mut context, &mut zip, &["word/media/"]),
            Err(OfficeReadError::Cancelled)
        ));
    }

    #[test]
    fn tar_scan_reader_stops_at_decompressed_byte_budget() {
        let mut reader = TarScanReader {
            reader: Cursor::new(vec![1, 2, 3, 4, 5]),
            remaining: 4,
            deadline: Instant::now() + Duration::from_secs(1),
            cancel_cb: None,
        };
        let mut buffer = [0u8; 8];

        assert_eq!(reader.read(&mut buffer).expect("read within budget"), 4);
        assert_eq!(
            reader
                .read(&mut buffer)
                .expect_err("budget exhaustion")
                .kind(),
            io::ErrorKind::Interrupted
        );
    }

    extern "C" fn always_cancel() -> bool {
        true
    }

    #[test]
    fn tar_scan_reader_honors_cancellation() {
        let mut reader = TarScanReader::new(Cursor::new(vec![1]), Some(always_cancel));
        let mut buffer = [0u8; 1];

        assert_eq!(
            reader.read(&mut buffer).expect_err("cancelled scan").kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn tar_scan_reader_honors_deadline() {
        let mut reader = TarScanReader {
            reader: Cursor::new(vec![1]),
            remaining: 1,
            deadline: Instant::now() - Duration::from_secs(1),
            cancel_cb: None,
        };
        let mut buffer = [0u8; 1];

        assert_eq!(
            reader.read(&mut buffer).expect_err("expired scan").kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn archive_extract_output_name_is_lossless_and_keeps_safe_extension() {
        let first = archive_extract_output_name("folder/a:b?.png");
        let second = archive_extract_output_name("folder/a<b>.png");

        assert_ne!(first, second);
        assert!(first.ends_with(".png"));
        assert!(first.starts_with("entry-666f6c6465722f613a623f2e706e67"));
    }

    #[test]
    fn office_input_budget_is_below_archive_extract_budget() {
        assert!(MAX_OFFICE_INPUT_BYTES > MAX_OFFICE_MEDIA_BYTES);
        assert_eq!(MAX_OFFICE_INPUT_BYTES, 128 * 1024 * 1024);
    }

    #[test]
    fn office_text_truncation_is_char_boundary_safe() {
        let text = "中".repeat(MAX_OFFICE_TEXT_CHARS + 1);
        let truncated = truncate_preview_text(&text);

        assert!(truncated.starts_with(&"中".repeat(8)));
        assert!(truncated.contains("[Preview truncated at"));
    }

    #[test]
    fn office_xml_parser_honors_cancellation() {
        let xml = format!(
            "<w:document xmlns:w=\"w\"><w:body>{}</w:body></w:document>",
            "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(128)
        );
        let context = OfficeContext::new(Some(always_cancel));

        assert!(matches!(
            extract_wordprocessing_text(&context, &xml),
            Err(OfficeReadError::Cancelled)
        ));
    }

    #[test]
    fn ppt_text_extraction_preserves_paragraphs_tabs_and_breaks() {
        let context = test_office_context();
        let text = extract_ppt_text(&context,
            r#"<p:sld xmlns:p="p" xmlns:a="a">
                <p:sp><p:txBody>
                    <a:p><a:r><a:t>Title</a:t></a:r></a:p>
                    <a:p><a:r><a:t>Left</a:t></a:r><a:tab/><a:r><a:t>Right</a:t></a:r></a:p>
                    <a:p><a:r><a:t>Line 1</a:t></a:r><a:br/><a:r><a:t>Line 2</a:t></a:r></a:p>
                </p:txBody></p:sp>
            </p:sld>"#,
        )
        .expect("ppt text");

        assert_eq!(text, "Title\nLeft\tRight\nLine 1\nLine 2");
    }

    #[test]
    fn ppt_layout_text_items_preserve_paragraph_boundaries() {
        let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
            .finish()
            .expect("empty zip archive bytes");
        cursor.set_position(0);
        let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
        let mut image_budget = 0;
        let mut context = OfficeContext::new(None);
        let items = parse_ppt_slide_items(
            &mut context,
            &mut zip,
            "ppt/slides/",
            r#"<p:sld xmlns:p="p" xmlns:a="a">
                <p:sp>
                    <p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
                    <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></p:spPr>
                    <p:txBody>
                        <a:p><a:r><a:rPr b="1" i="1" sz="2400"/><a:t>First</a:t></a:r></a:p>
                        <a:p><a:r><a:t>Second</a:t></a:r></a:p>
                    </p:txBody>
                </p:sp>
            </p:sld>"#,
            &BTreeMap::new(),
            &mut image_budget,
        )
        .expect("text-only layout");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].z_index, 0);
        assert_eq!(items[0].text.as_deref(), Some("First\nSecond"));
        assert_eq!(items[0].placeholder_type.as_deref(), Some("title"));
        assert!(items[0].bold);
        assert!(items[0].italic);
        assert_eq!(items[0].font_size, Some(24.0));
    }

    #[test]
    fn ppt_layout_text_items_preserve_bullets_and_alignment_hints() {
        let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
            .finish()
            .expect("empty zip archive bytes");
        cursor.set_position(0);
        let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
        let mut image_budget = 0;
        let mut context = OfficeContext::new(None);
        let items = parse_ppt_slide_items(
            &mut context,
            &mut zip,
            "ppt/slides/",
            r#"<p:sld xmlns:p="p" xmlns:a="a">
                <p:sp>
                    <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></p:spPr>
                    <p:txBody>
                        <a:p><a:pPr algn="ctr"><a:buChar char="•"/></a:pPr><a:r><a:t>Centered bullet</a:t></a:r></a:p>
                        <a:p><a:pPr algn="r"/><a:r><a:t>Right aligned</a:t></a:r></a:p>
                    </p:txBody>
                </p:sp>
            </p:sld>"#,
            &BTreeMap::new(),
            &mut image_budget,
        )
        .expect("text-only layout");

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].text.as_deref(),
            Some("[center] • Centered bullet\n[right] Right aligned")
        );
    }

    #[test]
    fn docx_text_extraction_marks_headings() {
        let context = test_office_context();
        let text = extract_wordprocessing_text(&context,
            r#"<w:document xmlns:w="w"><w:body>
                <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>Details</w:t></w:r></w:p>
                <w:p><w:r><w:t>Body copy</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )
        .expect("docx text");

        assert_eq!(text, "# Overview\n### Details\nBody copy");
    }

    #[test]
    fn docx_text_extraction_formats_table_rows() {
        let context = test_office_context();
        let text = extract_wordprocessing_text(&context,
            r#"<w:document xmlns:w="w"><w:body>
                <w:tbl>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc>
                        <w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc>
                    </w:tr>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t>Rows</w:t></w:r></w:p></w:tc>
                        <w:tc><w:p><w:r><w:t>42</w:t></w:r></w:p></w:tc>
                    </w:tr>
                </w:tbl>
            </w:body></w:document>"#,
        )
        .expect("docx text");

        assert_eq!(text, "| Name | Value |\n| Rows | 42 |");
    }

    #[test]
    fn docx_text_extraction_marks_page_and_section_breaks() {
        let context = test_office_context();
        let text = extract_wordprocessing_text(&context,
            r#"<w:document xmlns:w="w"><w:body>
                <w:p><w:r><w:t>First page</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:t>Second page</w:t></w:r></w:p>
                <w:sectPr/>
                <w:p><w:r><w:t>Next section</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )
        .expect("docx text");

        assert!(text.contains("First page\n[page break]\nSecond page"));
        assert!(text.contains("[section break]\nNext section"));
    }

    #[test]
    fn docx_text_extraction_marks_numbered_paragraphs_as_list_items() {
        let context = test_office_context();
        let text = extract_wordprocessing_text(&context,
            r#"<w:document xmlns:w="w"><w:body>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )
        .expect("docx text");

        assert_eq!(text, "- First\n- Second");
    }

    #[test]
    fn docx_header_footer_entries_extract_text() {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("word/header1.xml", options)
            .expect("header file");
        writer
            .write_all(
                br#"<w:hdr xmlns:w="w"><w:p><w:r><w:t>Confidential</w:t></w:r></w:p></w:hdr>"#,
            )
            .expect("header xml");
        writer
            .start_file("word/footer1.xml", options)
            .expect("footer file");
        writer
            .write_all(
                br#"<w:ftr xmlns:w="w"><w:p><w:r><w:t>Page footer</w:t></w:r></w:p></w:ftr>"#,
            )
            .expect("footer xml");
        let mut cursor = writer.finish().expect("zip bytes");
        cursor.set_position(0);
        let mut zip = ZipArchive::new(cursor).expect("docx zip");

        let entries = docx_header_footer_entries(&mut OfficeContext::new(None), &mut zip)
            .expect("header and footer entries");
        let text =
            extract_docx_header_footer_text(&mut OfficeContext::new(None), &mut zip, &entries)
                .expect("header and footer text");

        assert_eq!(
            entries,
            vec![
                "word/footer1.xml".to_string(),
                "word/header1.xml".to_string()
            ]
        );
        assert!(text.contains("footer1.xml: Page footer"));
        assert!(text.contains("header1.xml: Confidential"));
    }

    #[test]
    fn archive_type_summary_counts_common_types() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "src/".to_string(),
            ("src".to_string(), "".to_string(), true, 0, 0, 0),
        );
        entries.insert(
            "src/main.rs".to_string(),
            ("main.rs".to_string(), "src/".to_string(), false, 10, 8, 0),
        );
        entries.insert(
            "src/lib.rs".to_string(),
            ("lib.rs".to_string(), "src/".to_string(), false, 10, 8, 0),
        );
        entries.insert(
            "README.md".to_string(),
            ("README.md".to_string(), "".to_string(), false, 10, 8, 0),
        );

        assert_eq!(
            archive_type_summary(&entries).as_deref(),
            Some("RS File 2, MD File 1")
        );
    }

    #[test]
    fn archive_project_summary_detects_project_markers() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "app/package.json".to_string(),
            (
                "package.json".to_string(),
                "app/".to_string(),
                false,
                10,
                8,
                0,
            ),
        );
        entries.insert(
            "src/QuickLook.Next.csproj".to_string(),
            (
                "QuickLook.Next.csproj".to_string(),
                "src/".to_string(),
                false,
                10,
                8,
                0,
            ),
        );

        assert_eq!(
            archive_project_summary(&entries).as_deref(),
            Some(".csproj, package.json")
        );
    }

    #[test]
    fn jpeg_exif_metadata_reads_core_fields_and_gps() {
        let mut tiff = vec![0u8; 8 + 2 + 7 * 12 + 4];
        tiff[0..4].copy_from_slice(&[b'I', b'I', 42, 0]);
        write_le_u32(&mut tiff, 4, 8);
        write_le_u16(&mut tiff, 8, 7);

        let ifd0_entries = 10;
        write_ascii_entry(&mut tiff, ifd0_entries, 0, 0x010F, "Acme");
        write_ascii_entry(&mut tiff, ifd0_entries, 1, 0x0110, "PhoneCam");
        write_short_entry(&mut tiff, ifd0_entries, 2, 0x0112, 6);
        write_ascii_entry(&mut tiff, ifd0_entries, 3, 0x0131, "QuickCamOS");

        let exif_ifd = tiff.len() as u32;
        write_long_entry(&mut tiff, ifd0_entries, 4, 0x8769, exif_ifd);
        append_exif_ifd(&mut tiff);

        let gps_ifd = tiff.len() as u32;
        write_long_entry(&mut tiff, ifd0_entries, 5, 0x8825, gps_ifd);
        append_gps_ifd(&mut tiff);

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let segment_len = (2 + 6 + tiff.len()) as u16;
        jpeg.extend_from_slice(&segment_len.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        let metadata = parse_jpeg_exif_metadata_from_bytes(&jpeg).expect("exif metadata");
        assert_eq!(metadata.make.as_deref(), Some("Acme"));
        assert_eq!(metadata.model.as_deref(), Some("PhoneCam"));
        assert_eq!(metadata.orientation, Some(6));
        assert_eq!(metadata.date_time.as_deref(), Some("2026:07:05 13:04:47"));
        assert_eq!(metadata.width, Some(4032));
        assert_eq!(metadata.height, Some(3024));
        assert_eq!(metadata.lens_make.as_deref(), Some("Acme Lens"));
        assert_eq!(metadata.lens_model.as_deref(), Some("24mm Prime"));
        assert_eq!(metadata.software.as_deref(), Some("QuickCamOS"));
        assert!((metadata.f_number.unwrap() - 1.8).abs() < 0.001);
        assert!((metadata.max_aperture.unwrap() - 2.0).abs() < 0.001);
        assert!((metadata.exposure_time.unwrap() - 0.005).abs() < 0.0001);
        assert_eq!(metadata.iso, Some(100));
        assert!((metadata.focal_length.unwrap() - 24.0).abs() < 0.001);
        assert_eq!(metadata.focal_length_in_35mm_film, Some(36));
        assert!((metadata.exposure_bias.unwrap() + 0.3333).abs() < 0.001);
        assert_eq!(metadata.exposure_program, Some(3));
        assert_eq!(metadata.exposure_mode, Some(0));
        assert_eq!(metadata.metering_mode, Some(5));
        assert_eq!(metadata.light_source, Some(10));
        assert_eq!(metadata.flash, Some(16));
        assert_eq!(metadata.white_balance, Some(1));
        assert!((metadata.digital_zoom_ratio.unwrap() - 1.5).abs() < 0.001);
        assert!((metadata.subject_distance.unwrap() - 3.25).abs() < 0.001);
        assert_eq!(metadata.contrast, Some(1));
        assert_eq!(metadata.saturation, Some(2));
        assert_eq!(metadata.sharpness, Some(0));
        assert_eq!(metadata.gain_control, Some(1));
        assert_eq!(metadata.color_space, Some(1));
        assert_eq!(metadata.exif_version.as_deref(), Some("0231"));
        assert_eq!(metadata.camera_serial.as_deref(), Some("BODY-42"));
        assert_eq!(metadata.lens_serial.as_deref(), Some("LENS-24"));
        assert!((metadata.latitude.unwrap() - 31.2304).abs() < 0.0001);
        assert!((metadata.longitude.unwrap() - 121.4737).abs() < 0.0001);
        assert!((metadata.altitude.unwrap() - 12.5).abs() < 0.001);
        assert!((metadata.direction.unwrap() - 180.0).abs() < 0.001);

        let path = std::env::temp_dir().join("quicklook-next-exif-smoke.jpg");
        fs::write(&path, &jpeg).expect("write temp jpeg");
        let from_file =
            parse_jpeg_exif_metadata(path.to_str().unwrap()).expect("file exif metadata");
        let _ = fs::remove_file(path);
        assert_eq!(from_file.make.as_deref(), Some("Acme"));
    }

    #[test]
    fn png_metadata_reads_ihdr_summary() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1A\n");
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&800u32.to_be_bytes());
        bytes.extend_from_slice(&600u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 1]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&12u32.to_be_bytes());
        bytes.extend_from_slice(b"tEXt");
        bytes.extend_from_slice(b"Title\0Sunset");
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(b"tEXt");
        bytes.extend_from_slice(b"Comment\0Wide shot");
        bytes.extend_from_slice(&0u32.to_be_bytes());

        let metadata = parse_png_metadata_from_bytes(&bytes).expect("png metadata");

        assert_eq!(metadata.format.as_deref(), Some("PNG"));
        assert_eq!(metadata.title.as_deref(), Some("Sunset"));
        assert_eq!(metadata.comment.as_deref(), Some("Wide shot"));
        assert_eq!(metadata.width, Some(800));
        assert_eq!(metadata.height, Some(600));
        assert_eq!(metadata.bit_depth, Some(8));
        assert_eq!(metadata.color_type.as_deref(), Some("truecolor with alpha"));
        assert_eq!(metadata.has_alpha, Some(true));
        assert_eq!(metadata.interlace.as_deref(), Some("Adam7"));
    }

    #[test]
    fn png_metadata_reads_apng_animation_summary() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1A\n");
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&320u32.to_be_bytes());
        bytes.extend_from_slice(&180u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(b"acTL");
        bytes.extend_from_slice(&3u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        for (sequence, delay) in [(0u32, 5u16), (1u32, 7u16), (2u32, 9u16)] {
            bytes.extend_from_slice(&26u32.to_be_bytes());
            bytes.extend_from_slice(b"fcTL");
            bytes.extend_from_slice(&sequence.to_be_bytes());
            bytes.extend_from_slice(&320u32.to_be_bytes());
            bytes.extend_from_slice(&180u32.to_be_bytes());
            bytes.extend_from_slice(&0u32.to_be_bytes());
            bytes.extend_from_slice(&0u32.to_be_bytes());
            bytes.extend_from_slice(&delay.to_be_bytes());
            bytes.extend_from_slice(&100u16.to_be_bytes());
            bytes.extend_from_slice(&[0, 0]);
            bytes.extend_from_slice(&0u32.to_be_bytes());
        }

        let metadata = parse_png_metadata_from_bytes(&bytes).expect("apng metadata");

        assert_eq!(metadata.format.as_deref(), Some("PNG"));
        assert_eq!(metadata.width, Some(320));
        assert_eq!(metadata.height, Some(180));
        assert_eq!(metadata.animated, Some(true));
        assert_eq!(metadata.frame_count, Some(3));
        assert_eq!(metadata.duration_ms, Some(210));
    }

    #[test]
    fn gif_metadata_reads_animation_summary() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0]);
        for delay in [5u16, 7u16] {
            bytes.extend_from_slice(&[0x21, 0xF9, 0x04, 0x00]);
            bytes.extend_from_slice(&delay.to_le_bytes());
            bytes.extend_from_slice(&[0x00, 0x00]);
            bytes.push(0x2C);
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&3u16.to_le_bytes());
            bytes.extend_from_slice(&[0x00, 0x02, 0x02, 0x4C, 0x01, 0x00]);
        }
        bytes.push(0x3B);

        let metadata = parse_gif_metadata_from_bytes(&bytes).expect("gif metadata");

        assert_eq!(metadata.format.as_deref(), Some("GIF"));
        assert_eq!(metadata.width, Some(2));
        assert_eq!(metadata.height, Some(3));
        assert_eq!(metadata.animated, Some(true));
        assert_eq!(metadata.frame_count, Some(2));
        assert_eq!(metadata.duration_ms, Some(120));
    }

    #[test]
    fn webp_metadata_reads_vp8x_animation_summary() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.push(0x12);
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&799u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&599u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(b"ANMF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"ANMF");
        bytes.extend_from_slice(&0u32.to_le_bytes());

        let metadata = parse_webp_metadata_from_bytes(&bytes).expect("webp metadata");

        assert_eq!(metadata.format.as_deref(), Some("WebP"));
        assert_eq!(metadata.width, Some(800));
        assert_eq!(metadata.height, Some(600));
        assert_eq!(metadata.has_alpha, Some(true));
        assert_eq!(metadata.animated, Some(true));
        assert_eq!(metadata.frame_count, Some(2));
    }

    #[test]
    fn webp_metadata_reads_xmp_text_summary() {
        let xmp = br#"<x:xmpmeta>
            <rdf:Description>
                <dc:title><rdf:Alt><rdf:li>Layered WebP</rdf:li></rdf:Alt></dc:title>
                <dc:description><rdf:Alt><rdf:li>Alpha artwork</rdf:li></rdf:Alt></dc:description>
                <xmp:CreatorTool>QuickDraw</xmp:CreatorTool>
            </rdf:Description>
        </x:xmpmeta>"#;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.push(0x14);
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&639u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&479u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(b"XMP ");
        bytes.extend_from_slice(&(xmp.len() as u32).to_le_bytes());
        bytes.extend_from_slice(xmp);
        if xmp.len() % 2 == 1 {
            bytes.push(0);
        }

        let metadata = parse_webp_metadata_from_bytes(&bytes).expect("webp metadata");

        assert_eq!(metadata.format.as_deref(), Some("WebP"));
        assert_eq!(metadata.width, Some(640));
        assert_eq!(metadata.height, Some(480));
        assert_eq!(metadata.has_alpha, Some(true));
        assert_eq!(metadata.title.as_deref(), Some("Layered WebP"));
        assert_eq!(metadata.comment.as_deref(), Some("Alpha artwork"));
        assert_eq!(metadata.software.as_deref(), Some("QuickDraw"));
    }

    #[test]
    fn tiff_metadata_reads_header_ifd_summary() {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.resize(8 + 2 + 8 * 12 + 4, 0);
        write_le_u16(&mut tiff, 8, 8);
        let entries = 10;
        write_long_entry(&mut tiff, entries, 0, 0x0100, 1024);
        write_long_entry(&mut tiff, entries, 1, 0x0101, 768);
        write_short_entry(&mut tiff, entries, 2, 0x0102, 16);
        write_short_entry(&mut tiff, entries, 3, 0x0103, 5);
        write_short_entry(&mut tiff, entries, 4, 0x0106, 2);
        write_short_entry(&mut tiff, entries, 5, 0x0112, 6);
        write_ascii_entry(&mut tiff, entries, 6, 0x0131, "ScanSoft");
        write_ascii_entry(&mut tiff, entries, 7, 0x0132, "2026:07:08 10:11:12");

        let metadata = parse_tiff_exif_metadata(&tiff).expect("tiff metadata");

        assert_eq!(metadata.width, Some(1024));
        assert_eq!(metadata.height, Some(768));
        assert_eq!(metadata.bit_depth, Some(16));
        assert_eq!(metadata.compression.as_deref(), Some("LZW"));
        assert_eq!(metadata.color_type.as_deref(), Some("RGB"));
        assert_eq!(metadata.orientation, Some(6));
        assert_eq!(metadata.software.as_deref(), Some("ScanSoft"));
        assert_eq!(metadata.date_time.as_deref(), Some("2026:07:08 10:11:12"));
    }

    fn append_exif_ifd(tiff: &mut Vec<u8>) {
        let offset = tiff.len();
        tiff.resize(offset + 2 + 30 * 12 + 4, 0);
        write_le_u16(tiff, offset, 30);
        let entries = offset + 2;
        write_ascii_entry(tiff, entries, 0, 0x9003, "2026:07:05 13:04:47");
        write_rational_entry(tiff, entries, 1, 0x829A, 1, 200);
        write_rational_entry(tiff, entries, 2, 0x829D, 18, 10);
        write_short_entry(tiff, entries, 3, 0x8827, 100);
        write_signed_rational_entry(tiff, entries, 4, 0x9204, -1, 3);
        write_short_entry(tiff, entries, 5, 0x9207, 5);
        write_short_entry(tiff, entries, 6, 0x9209, 16);
        write_rational_entry(tiff, entries, 7, 0x920A, 24, 1);
        write_short_entry(tiff, entries, 8, 0xA001, 1);
        write_long_entry(tiff, entries, 9, 0xA002, 4032);
        write_long_entry(tiff, entries, 10, 0xA003, 3024);
        write_short_entry(tiff, entries, 11, 0xA403, 1);
        write_ascii_entry(tiff, entries, 12, 0xA433, "Acme Lens");
        write_ascii_entry(tiff, entries, 13, 0xA434, "24mm Prime");
        write_rational_entry(tiff, entries, 14, 0x9205, 2, 1);
        write_rational_entry(tiff, entries, 15, 0x9206, 13, 4);
        write_short_entry(tiff, entries, 16, 0x8822, 3);
        write_short_entry(tiff, entries, 17, 0xA402, 0);
        write_short_entry(tiff, entries, 18, 0x9208, 10);
        write_rational_entry(tiff, entries, 19, 0xA404, 3, 2);
        write_short_entry(tiff, entries, 20, 0xA405, 36);
        write_short_entry(tiff, entries, 21, 0xA407, 1);
        write_short_entry(tiff, entries, 22, 0xA408, 1);
        write_short_entry(tiff, entries, 23, 0xA409, 2);
        write_short_entry(tiff, entries, 24, 0xA40A, 0);
        write_undefined_entry(tiff, entries, 25, 0x9000, b"0231");
        write_ascii_entry(tiff, entries, 26, 0xA431, "BODY-42");
        write_ascii_entry(tiff, entries, 27, 0xA435, "LENS-24");
    }

    fn append_gps_ifd(tiff: &mut Vec<u8>) {
        let offset = tiff.len();
        tiff.resize(offset + 2 + 7 * 12 + 4, 0);
        write_le_u16(tiff, offset, 7);
        let entries = offset + 2;
        write_ascii_entry(tiff, entries, 0, 1, "N");
        write_rational3_entry(tiff, entries, 1, 2, [(31, 1), (13, 1), (4944, 100)]);
        write_ascii_entry(tiff, entries, 2, 3, "E");
        write_rational3_entry(tiff, entries, 3, 4, [(121, 1), (28, 1), (2532, 100)]);
        write_short_entry(tiff, entries, 4, 5, 0);
        write_rational_entry(tiff, entries, 5, 6, 25, 2);
        write_rational_entry(tiff, entries, 6, 17, 180, 1);
    }

    fn write_ascii_entry(tiff: &mut Vec<u8>, entries: usize, index: usize, tag: u16, value: &str) {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 2);
        write_le_u32(tiff, entry + 4, bytes.len() as u32);
        if bytes.len() <= 4 {
            tiff[entry + 8..entry + 8 + bytes.len()].copy_from_slice(&bytes);
            return;
        }
        let offset = tiff.len() as u32;
        write_le_u32(tiff, entry + 8, offset);
        tiff.extend_from_slice(&bytes);
    }

    fn write_short_entry(tiff: &mut [u8], entries: usize, index: usize, tag: u16, value: u16) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 3);
        write_le_u32(tiff, entry + 4, 1);
        write_le_u16(tiff, entry + 8, value);
    }

    fn write_long_entry(tiff: &mut [u8], entries: usize, index: usize, tag: u16, value: u32) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 4);
        write_le_u32(tiff, entry + 4, 1);
        write_le_u32(tiff, entry + 8, value);
    }

    fn write_rational3_entry(
        tiff: &mut Vec<u8>,
        entries: usize,
        index: usize,
        tag: u16,
        values: [(u32, u32); 3],
    ) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 5);
        write_le_u32(tiff, entry + 4, 3);
        let offset = tiff.len();
        write_le_u32(tiff, entry + 8, offset as u32);
        tiff.resize(offset + 24, 0);
        for (i, (numerator, denominator)) in values.into_iter().enumerate() {
            write_le_u32(tiff, offset + i * 8, numerator);
            write_le_u32(tiff, offset + i * 8 + 4, denominator);
        }
    }

    fn write_rational_entry(
        tiff: &mut Vec<u8>,
        entries: usize,
        index: usize,
        tag: u16,
        numerator: u32,
        denominator: u32,
    ) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 5);
        write_le_u32(tiff, entry + 4, 1);
        let offset = tiff.len();
        write_le_u32(tiff, entry + 8, offset as u32);
        tiff.resize(offset + 8, 0);
        write_le_u32(tiff, offset, numerator);
        write_le_u32(tiff, offset + 4, denominator);
    }

    fn write_undefined_entry(
        tiff: &mut [u8],
        entries: usize,
        index: usize,
        tag: u16,
        value: &[u8],
    ) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 7);
        write_le_u32(tiff, entry + 4, value.len() as u32);
        tiff[entry + 8..entry + 8 + value.len().min(4)]
            .copy_from_slice(&value[..value.len().min(4)]);
    }

    fn write_signed_rational_entry(
        tiff: &mut Vec<u8>,
        entries: usize,
        index: usize,
        tag: u16,
        numerator: i32,
        denominator: i32,
    ) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 10);
        write_le_u32(tiff, entry + 4, 1);
        let offset = tiff.len();
        write_le_u32(tiff, entry + 8, offset as u32);
        tiff.resize(offset + 8, 0);
        bytes_write_i32(tiff, offset, numerator);
        bytes_write_i32(tiff, offset + 4, denominator);
    }

    fn bytes_write_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_le_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn xlsx_merge_regions_preserve_spans() {
        let context = test_office_context();
        let regions = parse_xlsx_merge_regions(&context,
            r#"<worksheet><mergeCells><mergeCell ref="B2:D4"/></mergeCells></worksheet>"#,
        )
        .expect("merge regions");

        let region = regions.get(&(1, 1)).expect("merged region");
        assert_eq!(region.row_span, 3);
        assert_eq!(region.column_span, 3);
        assert!(is_inside_non_origin_merge(&regions, 2, 2));
        assert!(!is_inside_non_origin_merge(&regions, 1, 1));
    }

    #[test]
    fn xlsx_freeze_pane_reads_split_counts() {
        let context = test_office_context();
        let (rows, columns) = parse_xlsx_freeze_pane(&context,
            r#"<worksheet><sheetViews><sheetView><pane xSplit="2" ySplit="1" state="frozen"/></sheetView></sheetViews></worksheet>"#,
        )
        .expect("freeze pane");

        assert_eq!(rows, Some(1));
        assert_eq!(columns, Some(2));
    }

    #[test]
    fn xlsx_style_number_formats_include_custom_and_builtin_formats() {
        let context = test_office_context();
        let formats = parse_xlsx_style_number_formats(&context,
            r#"<styleSheet>
                <numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts>
                <cellXfs count="3">
                    <xf numFmtId="0"/>
                    <xf numFmtId="14"/>
                    <xf numFmtId="164"/>
                </cellXfs>
            </styleSheet>"#,
        )
        .expect("style number formats");

        assert_eq!(formats.get(0), Some(&None));
        assert_eq!(formats.get(1), Some(&Some("m/d/yy".to_string())));
        assert_eq!(formats.get(2), Some(&Some("yyyy-mm-dd".to_string())));
    }

    #[test]
    fn xlsx_styles_include_fill_colors() {
        let context = test_office_context();
        let styles = parse_xlsx_styles(&context,
            r#"<styleSheet>
                <fonts count="2">
                    <font><sz val="11"/></font>
                    <font><b/><i/><color rgb="FF9C0006"/><sz val="14"/></font>
                </fonts>
                <fills count="3">
                    <fill><patternFill patternType="none"/></fill>
                    <fill><patternFill patternType="gray125"/></fill>
                    <fill><patternFill patternType="solid"><fgColor rgb="FFFFE699"/></patternFill></fill>
                </fills>
                <cellXfs count="2">
                    <xf numFmtId="0" fillId="0"/>
                    <xf numFmtId="14" fillId="2" fontId="1"><alignment horizontal="center" vertical="top" wrapText="1"/></xf>
                </cellXfs>
            </styleSheet>"#,
        )
        .expect("styles");

        assert_eq!(
            styles.get(0).and_then(|style| style.fill_color.as_deref()),
            None
        );
        assert_eq!(
            styles.get(1).and_then(|style| style.fill_color.as_deref()),
            Some("#FFE699")
        );
        assert_eq!(
            styles
                .get(1)
                .and_then(|style| style.number_format.as_deref()),
            Some("m/d/yy")
        );
        assert_eq!(
            styles
                .get(1)
                .and_then(|style| style.horizontal_alignment.as_deref()),
            Some("center")
        );
        assert_eq!(
            styles
                .get(1)
                .and_then(|style| style.vertical_alignment.as_deref()),
            Some("top")
        );
        assert_eq!(styles.get(1).map(|style| style.bold), Some(true));
        assert_eq!(styles.get(1).map(|style| style.italic), Some(true));
        assert_eq!(styles.get(1).and_then(|style| style.font_size), Some(14.0));
        assert_eq!(styles.get(1).map(|style| style.wrap_text), Some(true));
        assert_eq!(
            styles.get(1).and_then(|style| style.text_color.as_deref()),
            Some("#9C0006")
        );
    }

    #[test]
    fn markdown_parser_emits_heading_and_inline_ast() {
        let (blocks, partial) = parse_markdown_blocks("# Hello **QuickLook** and `Rust`");

        assert!(!partial);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "heading");
        assert_eq!(blocks[0].level, 1);
        assert!(blocks[0].inlines.iter().any(|i| i.kind == "strong"));
        assert!(blocks[0].inlines.iter().any(|i| i.kind == "code"));
    }

    #[test]
    fn markdown_parser_does_not_panic_on_non_ascii() {
        let (blocks, partial) =
            parse_markdown_blocks("# 中文标题\n\n这是一个含有 **加粗** 的中文字符串。");
        assert!(!partial);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "heading");
        assert_eq!(blocks[0].text, "中文标题");
        assert_eq!(blocks[1].kind, "paragraph");
        assert!(blocks[1].inlines.iter().any(|i| i.kind == "strong"));
    }

    #[test]
    fn markdown_parser_emits_lists_quotes_and_code() {
        let (blocks, partial) =
            parse_markdown_blocks("> note\n\n- one\n- two\n\n```rs\nfn main() {}\n```");

        assert!(!partial);
        assert_eq!(blocks[0].kind, "blockquote");
        assert_eq!(blocks[1].kind, "unorderedList");
        assert_eq!(blocks[1].children.len(), 2);
        assert_eq!(blocks[2].kind, "code");
        assert_eq!(blocks[2].language, "rs");
    }

    #[test]
    fn markdown_parser_emits_tables() {
        let (blocks, partial) = parse_markdown_blocks("| A | B |\n|---|---|\n| 1 | 2 |");

        assert!(!partial);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "table");
        assert_eq!(
            blocks[0].table_headers,
            vec!["A".to_string(), "B".to_string()]
        );
        assert_eq!(
            blocks[0].table_rows[0],
            vec!["1".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn epub_container_and_opf_metadata_parse() {
        let container = r#"
            <container>
              <rootfiles>
                <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml" />
              </rootfiles>
            </container>"#;
        assert_eq!(
            parse_epub_rootfile(container).as_deref(),
            Some("OEBPS/content.opf")
        );

        let opf = parse_epub_opf(
            r#"<package>
                <metadata>
                  <dc:title>示例书</dc:title>
                  <dc:creator>作者</dc:creator>
                  <dc:language>zh-CN</dc:language>
                </metadata>
                <manifest>
                  <item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml" />
                </manifest>
                <spine><itemref idref="c1" /></spine>
              </package>"#,
        );

        assert_eq!(opf.title, "示例书");
        assert_eq!(opf.creator, "作者");
        assert_eq!(opf.language, "zh-CN");
        assert_eq!(opf.spine, vec!["c1".to_string()]);
        assert!(opf.manifest.contains_key("c1"));
    }

    #[test]
    fn xhtml_extractor_emits_markdown_headings() {
        let markdown = extract_xhtml_markdown(
            r#"<html><body><h1>第一章</h1><p>你好，&amp; QuickLook。</p><ul><li>项目</li></ul></body></html>"#,
            "chapter",
        );

        assert!(markdown.contains("## 第一章"));
        assert!(markdown.contains("你好，& QuickLook。"));
        assert!(markdown.contains("- 项目"));
    }

    #[test]
    fn ebook_label_normalizes_file_names() {
        assert_eq!(
            ebook_item_label("Text/chapter-01_intro.xhtml"),
            "chapter 01 intro"
        );
    }

    #[test]
    fn font_summary_detects_woff_tables() {
        let mut bytes = vec![0u8; 44];
        bytes[0..4].copy_from_slice(b"wOFF");
        bytes[12..14].copy_from_slice(&3u16.to_be_bytes());
        bytes[16..20].copy_from_slice(&4096u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&256u32.to_be_bytes());

        let summary = parse_font_summary(&bytes).expect("woff summary");

        assert_eq!(summary.format, "WOFF font");
        assert_eq!(summary.tables, 3);
        assert_eq!(summary.sfnt_size, 4096);
        assert_eq!(summary.metadata_size, 256);
    }

    #[test]
    fn font_summary_reads_names_and_glyph_count() {
        fn utf16be(value: &str) -> Vec<u8> {
            value
                .encode_utf16()
                .flat_map(|unit| unit.to_be_bytes())
                .collect()
        }

        let names = [
            (1u16, utf16be("Quick Sans")),
            (5u16, utf16be("Version 1.2")),
            (13u16, utf16be("Open Font License")),
            (14u16, utf16be("https://example.test/ofl")),
        ];
        let name_offset = 44usize;
        let name_storage_offset = 6 + names.len() * 12;
        let name_len = name_storage_offset + names.iter().map(|(_, v)| v.len()).sum::<usize>();
        let maxp_offset = name_offset + name_len;
        let mut bytes = vec![0u8; maxp_offset + 6];
        bytes[0..4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[4..6].copy_from_slice(&2u16.to_be_bytes());
        bytes[12..16].copy_from_slice(b"name");
        bytes[20..24].copy_from_slice(&(name_offset as u32).to_be_bytes());
        bytes[24..28].copy_from_slice(&(name_len as u32).to_be_bytes());
        bytes[28..32].copy_from_slice(b"maxp");
        bytes[36..40].copy_from_slice(&(maxp_offset as u32).to_be_bytes());
        bytes[40..44].copy_from_slice(&6u32.to_be_bytes());
        bytes[name_offset + 2..name_offset + 4]
            .copy_from_slice(&(names.len() as u16).to_be_bytes());
        bytes[name_offset + 4..name_offset + 6]
            .copy_from_slice(&(name_storage_offset as u16).to_be_bytes());
        let mut storage_pos = 0usize;
        for (index, (name_id, value)) in names.iter().enumerate() {
            let record = name_offset + 6 + index * 12;
            bytes[record..record + 2].copy_from_slice(&3u16.to_be_bytes());
            bytes[record + 6..record + 8].copy_from_slice(&name_id.to_be_bytes());
            bytes[record + 8..record + 10].copy_from_slice(&(value.len() as u16).to_be_bytes());
            bytes[record + 10..record + 12].copy_from_slice(&(storage_pos as u16).to_be_bytes());
            let value_start = name_offset + name_storage_offset + storage_pos;
            bytes[value_start..value_start + value.len()].copy_from_slice(value);
            storage_pos += value.len();
        }
        bytes[maxp_offset..maxp_offset + 4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[maxp_offset + 4..maxp_offset + 6].copy_from_slice(&321u16.to_be_bytes());

        let summary = parse_font_summary(&bytes).expect("font summary");

        assert_eq!(summary.family, "Quick Sans");
        assert_eq!(summary.version, "Version 1.2");
        assert_eq!(summary.license, "Open Font License");
        assert_eq!(summary.license_url, "https://example.test/ofl");
        assert_eq!(summary.glyphs, 321);
    }

    #[test]
    fn sqlite_header_details_include_journal_and_schema_fields() {
        let mut bytes = vec![0u8; 100];
        bytes[0..16].copy_from_slice(b"SQLite format 3\0");
        bytes[18] = 2;
        bytes[19] = 2;
        bytes[36..40].copy_from_slice(&7u32.to_be_bytes());
        bytes[40..44].copy_from_slice(&11u32.to_be_bytes());
        bytes[44..48].copy_from_slice(&4u32.to_be_bytes());
        bytes[96..100].copy_from_slice(&3_045_000u32.to_be_bytes());
        let mut text = String::new();

        append_sqlite_header_details(&mut text, &bytes);

        assert!(text.contains("Journal mode: WAL"));
        assert!(text.contains("Schema format: 4 (current)"));
        assert!(text.contains("Schema cookie: 11"));
        assert!(text.contains("Freelist pages: 7"));
        assert!(text.contains("SQLite version: 3045000"));
    }

    #[test]
    fn sqlite_schema_record_extracts_object_summary() {
        let mut payload = vec![6, 23, 23, 23, 1, 97];
        payload.extend_from_slice(b"table");
        payload.extend_from_slice(b"users");
        payload.extend_from_slice(b"users");
        payload.push(2);
        payload.extend_from_slice(b"CREATE TABLE users(id INTEGER PRIMARY KEY)");

        let row = parse_sqlite_schema_record(&payload).expect("schema row");
        assert_eq!(row.typ, "table");
        assert_eq!(row.name, "users");
        assert_eq!(row.table_name, "users");
        assert_eq!(row.root_page, 2);
        assert_eq!(row.sql, "CREATE TABLE users(id INTEGER PRIMARY KEY)");
    }

    #[test]
    fn sqlite_schema_parser_traverses_interior_pages() {
        let page_size = 512usize;
        let mut payload = vec![6, 23, 23, 23, 1, 97];
        payload.extend_from_slice(b"table");
        payload.extend_from_slice(b"users");
        payload.extend_from_slice(b"users");
        payload.push(2);
        payload.extend_from_slice(b"CREATE TABLE users(id INTEGER PRIMARY KEY)");
        let mut bytes = vec![0u8; page_size * 2];
        bytes[0..16].copy_from_slice(b"SQLite format 3\0");
        bytes[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        bytes[100] = 0x05;
        bytes[103..105].copy_from_slice(&1u16.to_be_bytes());
        bytes[112..114].copy_from_slice(&200u16.to_be_bytes());
        bytes[200..204].copy_from_slice(&2u32.to_be_bytes());
        bytes[204] = 1;
        let leaf = page_size;
        bytes[leaf] = 0x0D;
        bytes[leaf + 3..leaf + 5].copy_from_slice(&1u16.to_be_bytes());
        bytes[leaf + 8..leaf + 10].copy_from_slice(&400u16.to_be_bytes());
        let cell = leaf + 400;
        bytes[cell] = payload.len() as u8;
        bytes[cell + 1] = 1;
        bytes[cell + 2..cell + 2 + payload.len()].copy_from_slice(&payload);

        let rows = parse_sqlite_schema_rows(&bytes, page_size, 8);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "users");
        assert_eq!(rows[0].root_page, 2);
    }

    #[test]
    fn sqlite_schema_summary_marks_missing_pages_partial() {
        let page_size = 512usize;
        let mut bytes = vec![0u8; page_size];
        bytes[0..16].copy_from_slice(b"SQLite format 3\0");
        bytes[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        bytes[100] = 0x05;
        bytes[103..105].copy_from_slice(&1u16.to_be_bytes());
        bytes[112..114].copy_from_slice(&200u16.to_be_bytes());
        bytes[200..204].copy_from_slice(&2u32.to_be_bytes());
        bytes[204] = 1;

        let summary = parse_sqlite_schema_summary(&bytes, page_size, 8);

        assert!(summary.rows.is_empty());
        assert!(summary.partial);
    }

    #[test]
    fn sqlite_table_column_parser_summarizes_columns() {
        let columns = parse_sqlite_table_columns(
            r#"CREATE TABLE "users"(
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                "display,name" VARCHAR(80),
                balance DECIMAL(10, 2) DEFAULT 0,
                CONSTRAINT users_name UNIQUE(name)
            )"#,
            8,
        );

        assert_eq!(
            columns,
            vec![
                "id INTEGER".to_string(),
                "name TEXT".to_string(),
                "display,name VARCHAR(80)".to_string(),
                "balance DECIMAL(10, 2)".to_string(),
            ]
        );
    }

    #[test]
    fn sqlite_row_counter_counts_leaf_and_interior_pages() {
        let page_size = 512usize;
        let mut bytes = vec![0u8; page_size * 4];
        bytes[page_size] = 0x05;
        bytes[page_size + 3..page_size + 5].copy_from_slice(&1u16.to_be_bytes());
        bytes[page_size + 8..page_size + 12].copy_from_slice(&4u32.to_be_bytes());
        bytes[page_size + 12..page_size + 14].copy_from_slice(&100u16.to_be_bytes());
        bytes[page_size + 100..page_size + 104].copy_from_slice(&3u32.to_be_bytes());
        bytes[page_size * 2] = 0x0D;
        bytes[page_size * 2 + 3..page_size * 2 + 5].copy_from_slice(&2u16.to_be_bytes());
        bytes[page_size * 3] = 0x0D;
        bytes[page_size * 3 + 3..page_size * 3 + 5].copy_from_slice(&3u16.to_be_bytes());

        let count = count_sqlite_table_rows(&bytes, page_size, 2, 128).expect("row count");

        assert_eq!(count.rows, 5);
        assert!(!count.partial);
    }

    #[test]
    fn sqlite_row_counter_marks_missing_pages_partial() {
        let page_size = 512usize;
        let mut bytes = vec![0u8; page_size * 2];
        bytes[page_size] = 0x05;
        bytes[page_size + 3..page_size + 5].copy_from_slice(&1u16.to_be_bytes());
        bytes[page_size + 12..page_size + 14].copy_from_slice(&100u16.to_be_bytes());
        bytes[page_size + 100..page_size + 104].copy_from_slice(&3u32.to_be_bytes());

        let count = count_sqlite_table_rows(&bytes, page_size, 2, 128).expect("row count");

        assert_eq!(count.rows, 0);
        assert!(count.partial);
    }

    #[test]
    fn media_info_reads_mp4_brand_and_duration() {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            bytes
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom");
        bytes.extend_from_slice(&0u32.to_be_bytes());

        let mut mvhd_payload = vec![0u8; 20];
        mvhd_payload[4..8].copy_from_slice(&2_082_844_800u32.to_be_bytes());
        mvhd_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd_payload[16..20].copy_from_slice(&90_000u32.to_be_bytes());
        let mvhd = atom(b"mvhd", &mvhd_payload);

        let mut tkhd_payload = vec![0u8; 84];
        tkhd_payload[44..48].copy_from_slice(&65_536i32.to_be_bytes());
        tkhd_payload[64..68].copy_from_slice(&0x4000_0000i32.to_be_bytes());
        tkhd_payload[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd_payload[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
        let tkhd = atom(b"tkhd", &tkhd_payload);

        let mut mdhd_payload = vec![0u8; 24];
        mdhd_payload[12..16].copy_from_slice(&30_000u32.to_be_bytes());
        mdhd_payload[16..20].copy_from_slice(&2_700_000u32.to_be_bytes());
        mdhd_payload[20..22].copy_from_slice(&0x15C7u16.to_be_bytes());
        let mdhd = atom(b"mdhd", &mdhd_payload);

        let mut hdlr_payload = vec![0u8; 12];
        hdlr_payload[8..12].copy_from_slice(b"vide");
        let hdlr = atom(b"hdlr", &hdlr_payload);

        let avcc = atom(b"avcC", &[1, 0x64, 0, 31, 0xFF, 0, 0, 0xFE, 0xFE, 0xFE]);
        let entry_size = 86 + avcc.len();
        let mut stsd_payload = vec![0u8; 8 + entry_size];
        stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stsd_payload[8..12].copy_from_slice(&(entry_size as u32).to_be_bytes());
        stsd_payload[12..16].copy_from_slice(b"avc1");
        stsd_payload[40..42].copy_from_slice(&1920u16.to_be_bytes());
        stsd_payload[42..44].copy_from_slice(&1080u16.to_be_bytes());
        stsd_payload[94..94 + avcc.len()].copy_from_slice(&avcc);
        let stsd = atom(b"stsd", &stsd_payload);

        let mut stsz_payload = vec![0u8; 12 + 8];
        stsz_payload[8..12].copy_from_slice(&2u32.to_be_bytes());
        stsz_payload[12..16].copy_from_slice(&600_000u32.to_be_bytes());
        stsz_payload[16..20].copy_from_slice(&700_000u32.to_be_bytes());
        let stsz = atom(b"stsz", &stsz_payload);

        let mut stsc_payload = vec![0u8; 20];
        stsc_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stsc_payload[8..12].copy_from_slice(&1u32.to_be_bytes());
        stsc_payload[12..16].copy_from_slice(&1u32.to_be_bytes());
        stsc_payload[16..20].copy_from_slice(&1u32.to_be_bytes());
        let stsc = atom(b"stsc", &stsc_payload);

        let mut stco_payload = vec![0u8; 16];
        stco_payload[4..8].copy_from_slice(&2u32.to_be_bytes());
        stco_payload[8..12].copy_from_slice(&1000u32.to_be_bytes());
        stco_payload[12..16].copy_from_slice(&2000u32.to_be_bytes());
        let stco = atom(b"stco", &stco_payload);

        let mut stts_payload = vec![0u8; 16];
        stts_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stts_payload[8..12].copy_from_slice(&90u32.to_be_bytes());
        stts_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
        let stts = atom(b"stts", &stts_payload);
        let mut ctts_payload = vec![0u8; 16];
        ctts_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        ctts_payload[8..12].copy_from_slice(&90u32.to_be_bytes());
        ctts_payload[12..16].copy_from_slice(&40u32.to_be_bytes());
        let ctts = atom(b"ctts", &ctts_payload);
        let mut elst_payload = vec![0u8; 20];
        elst_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        elst_payload[8..12].copy_from_slice(&90_000u32.to_be_bytes());
        elst_payload[12..16].copy_from_slice(&0i32.to_be_bytes());
        elst_payload[16..18].copy_from_slice(&1i16.to_be_bytes());
        let edts = atom(b"edts", &atom(b"elst", &elst_payload));

        let stbl = atom(b"stbl", &[stsd, stsz, stsc, stco, stts, ctts].concat());
        let minf = atom(b"minf", &stbl);
        let mdia = atom(b"mdia", &[mdhd, hdlr, minf].concat());
        let trak = atom(b"trak", &[tkhd, edts, mdia].concat());

        bytes.extend_from_slice(&atom(b"moov", &[mvhd, trak].concat()));

        assert_eq!(media_container_name("clip.mp4", &bytes), "ISO BMFF / MP4");
        assert_eq!(mp4_major_brand(&bytes).as_deref(), Some("isom"));
        assert_eq!(mp4_duration_seconds(&bytes), Some(90.0));
        let summary = mp4_summary(&bytes).expect("summary");
        assert_eq!(summary.brand.as_deref(), Some("isom"));
        assert_eq!(summary.duration_seconds, Some(90.0));
        assert_eq!(summary.created_unix, Some(0));
        assert_eq!(summary.rotation_degrees, Some(90));
        assert_eq!(summary.tracks.len(), 1);
        assert_eq!(summary.tracks[0].kind, "Video");
        assert_eq!(summary.tracks[0].codec, "avc1");
        assert_eq!(
            summary.tracks[0].codec_detail,
            "AVC profile 0x64, compat 0x00, level 3.1, 4-byte NAL length, chroma 2, 14-bit luma, 14-bit chroma"
        );
        assert_eq!(summary.tracks[0].language, "eng");
        assert_eq!(summary.tracks[0].width, Some(1920));
        assert_eq!(summary.tracks[0].height, Some(1080));
        assert_eq!(summary.tracks[0].duration_seconds, Some(90.0));
        assert_eq!(summary.tracks[0].data_bytes, Some(1_300_000));
        assert_eq!(summary.tracks[0].timing_entries, Some(1));
        assert_eq!(summary.tracks[0].samples, Some(90));
        assert_eq!(summary.tracks[0].decode_ticks, Some(90_000));
        assert_eq!(summary.tracks[0].first_sample_delta, Some(1000));
        assert_eq!(summary.tracks[0].composition_entries, Some(1));
        assert_eq!(summary.tracks[0].composition_samples, Some(90));
        assert_eq!(summary.tracks[0].first_composition_offset, Some(40));
        assert_eq!(summary.tracks[0].composition_offset_range, Some((40, 40)));
        assert_eq!(summary.tracks[0].edit_entries, Some(1));
        assert_eq!(summary.tracks[0].first_edit_duration, Some(90_000));
        assert_eq!(summary.tracks[0].first_edit_media_time, Some(0));
        assert_eq!(summary.tracks[0].first_edit_rate, Some(1.0));
        assert_eq!(summary.tracks[0].chunks, Some(2));
        assert_eq!(summary.tracks[0].first_chunk_offset, Some(1000));
        assert_eq!(summary.tracks[0].last_chunk_end, Some(702_000));
        assert_eq!(summary.tracks[0].first_chunk_samples, Some(1));
        assert_eq!(summary.tracks[0].first_chunk_bytes, Some(600_000));
        assert_eq!(summary.tracks[0].first_sample_size, Some(600_000));
        assert_eq!(
            summary.tracks[0].chunk_details,
            vec![
                "#1 @0x3E8 1 samples 600000 bytes".to_string(),
                "#2 @0x7D0 1 samples 700000 bytes".to_string()
            ]
        );
        assert_eq!(format_duration(90.0), "1:30");
        assert_eq!(format_bitrate(1_536_000.0), "1.54 Mbps");
    }

    #[test]
    fn media_info_reads_mp4_esds_aac_config() {
        let esds = [
            0, 0, 0, 0, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0x02, 0x12,
            0x10,
        ];

        let detail = parse_esds_detail(&esds).expect("esds detail");

        assert!(detail.contains("object type MPEG-4 Audio"));
        assert!(detail.contains("AAC LC"));
        assert!(detail.contains("44,100 Hz"));
        assert!(detail.contains("2 ch"));
    }

    #[test]
    fn h264_sps_summary_reads_dimensions_crop_and_vui() {
        struct Writer {
            bits: Vec<u8>,
        }
        impl Writer {
            fn new() -> Self {
                Self { bits: Vec::new() }
            }
            fn bit(&mut self, value: bool) {
                self.bits.push(u8::from(value));
            }
            fn bits(&mut self, value: u32, count: usize) {
                for shift in (0..count).rev() {
                    self.bit(((value >> shift) & 1) != 0);
                }
            }
            fn ue(&mut self, value: u32) {
                let code_num = value + 1;
                let bits = 32 - code_num.leading_zeros();
                for _ in 0..bits - 1 {
                    self.bit(false);
                }
                self.bits(code_num, bits as usize);
            }
            fn finish(mut self) -> Vec<u8> {
                self.bit(true);
                while self.bits.len() % 8 != 0 {
                    self.bit(false);
                }
                self.bits
                    .chunks(8)
                    .map(|chunk| chunk.iter().fold(0u8, |acc, bit| (acc << 1) | bit))
                    .collect()
            }
        }

        let mut writer = Writer::new();
        writer.bits(66, 8);
        writer.bits(0, 8);
        writer.bits(30, 8);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(1);
        writer.bit(false);
        writer.ue(39);
        writer.ue(22);
        writer.bit(true);
        writer.bit(true);
        writer.bit(true);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(4);
        writer.bit(true);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bits(5, 3);
        writer.bit(true);
        writer.bit(true);
        writer.bits(1, 8);
        writer.bits(1, 8);
        writer.bits(1, 8);
        let mut sps = vec![0x67];
        sps.extend_from_slice(&writer.finish());

        let summary = parse_h264_sps_summary(&sps).expect("sps summary");

        assert_eq!(summary, "SPS coded 640x368, crop display 640x360, VUI, video format 5, full range, primaries BT.709, transfer BT.709, matrix BT.709");
        let mut avcc = vec![1, 66, 0, 30, 0xFF, 0xE1];
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(&sps);
        avcc.push(0);
        let detail = parse_avcc_detail(&avcc).expect("avcC detail");
        assert!(detail.contains("SPS coded 640x368, crop display 640x360, VUI, video format 5, full range, primaries BT.709, transfer BT.709, matrix BT.709"));
    }

    #[test]
    fn hevc_config_summary_reads_parameter_set_arrays() {
        struct Writer {
            bits: Vec<u8>,
        }
        impl Writer {
            fn new() -> Self {
                Self { bits: Vec::new() }
            }
            fn bit(&mut self, value: bool) {
                self.bits.push(u8::from(value));
            }
            fn bits(&mut self, value: u32, count: usize) {
                for shift in (0..count).rev() {
                    self.bit(((value >> shift) & 1) != 0);
                }
            }
            fn ue(&mut self, value: u32) {
                let code_num = value + 1;
                let bits = 32 - code_num.leading_zeros();
                for _ in 0..bits - 1 {
                    self.bit(false);
                }
                self.bits(code_num, bits as usize);
            }
            fn finish(mut self) -> Vec<u8> {
                self.bit(true);
                while self.bits.len() % 8 != 0 {
                    self.bit(false);
                }
                self.bits
                    .chunks(8)
                    .map(|chunk| chunk.iter().fold(0u8, |acc, bit| (acc << 1) | bit))
                    .collect()
            }
        }

        let mut vps_writer = Writer::new();
        vps_writer.bits(3, 4);
        vps_writer.bits(3, 2);
        vps_writer.bits(1, 6);
        vps_writer.bits(0, 3);
        vps_writer.bit(true);
        vps_writer.bits(0xFFFF, 16);
        vps_writer.bits(0, 2);
        vps_writer.bit(false);
        vps_writer.bits(1, 5);
        vps_writer.bits(0, 32);
        vps_writer.bits(0, 32);
        vps_writer.bits(0, 16);
        vps_writer.bits(120, 8);
        let mut vps = vec![0x40, 0x01];
        vps.extend_from_slice(&vps_writer.finish());

        let mut writer = Writer::new();
        writer.bits(0, 4);
        writer.bits(0, 3);
        writer.bit(true);
        writer.bits(0, 2);
        writer.bit(false);
        writer.bits(1, 5);
        writer.bits(0, 32);
        writer.bits(0, 32);
        writer.bits(0, 16);
        writer.bits(120, 8);
        writer.ue(0);
        writer.ue(1);
        writer.ue(1920);
        writer.ue(1088);
        writer.bit(true);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(4);
        writer.ue(2);
        writer.ue(2);
        writer.ue(0);
        writer.bit(false);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        for _ in 0..6 {
            writer.ue(0);
        }
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.ue(0);
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bits(5, 3);
        writer.bit(true);
        writer.bit(true);
        writer.bits(9, 8);
        writer.bits(16, 8);
        writer.bits(9, 8);
        let mut sps = vec![0x42, 0x01];
        sps.extend_from_slice(&writer.finish());

        let mut hvcc = vec![0u8; 23];
        hvcc[1] = 1;
        hvcc[12] = 120;
        hvcc[16] = 1;
        hvcc[17] = 2;
        hvcc[18] = 2;
        hvcc[21] = 3;
        hvcc[22] = 3;
        hvcc.extend_from_slice(&[0xA0, 0, 1]);
        hvcc.extend_from_slice(&(vps.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(&vps);
        hvcc.extend_from_slice(&[0xA1, 0, 1]);
        hvcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(&sps);
        hvcc.extend_from_slice(&[0xA2, 0, 1, 0, 1, 0xCC]);

        let detail = parse_hvcc_detail(&hvcc).expect("hvcC detail");
        let sps_summary = parse_hevc_sps_summary(&sps).expect("hevc sps");

        assert!(detail.contains("HEVC profile 1"));
        assert!(detail.contains("4-byte NAL length"));
        assert!(detail.contains("VPS 1, SPS 1, PPS 1"));
        assert!(detail.contains("VPS id 3, layers 2, sub-layers 1, temporal nesting yes"));
        assert_eq!(sps_summary, "SPS coded 1920x1088, crop display 1920x1080, chroma 1, 10-bit luma, 10-bit chroma, VUI, video format 5, full range, primaries BT.2020, transfer PQ, matrix BT.2020 non-constant");
        assert!(detail.contains("SPS coded 1920x1088, crop display 1920x1080, chroma 1, 10-bit luma, 10-bit chroma, VUI, video format 5, full range, primaries BT.2020, transfer PQ, matrix BT.2020 non-constant"));
    }

    #[test]
    fn media_info_reads_wav_format_and_duration() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&52u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&176_400u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&352_800u32.to_le_bytes());

        let summary = parse_wav_summary(&bytes).expect("wav summary");
        let mut text = String::new();
        append_wav_metadata(&mut text, &bytes);

        assert_eq!(media_container_name("clip.wav", &bytes), "WAV");
        assert_eq!(summary.audio_format, 1);
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.bits_per_sample, 16);
        assert_eq!(summary.data_bytes, 352_800);
        assert!(text.contains("Audio format: PCM"));
        assert!(text.contains("Duration: 0:02"));
    }

    #[test]
    fn media_info_reads_flac_streaminfo() {
        let sample_rate = 44_100u64;
        let channels = 2u64;
        let bits_per_sample = 16u64;
        let total_samples = 88_200u64;
        let packed = (sample_rate << 44)
            | ((channels - 1) << 41)
            | ((bits_per_sample - 1) << 36)
            | total_samples;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"fLaC");
        bytes.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]);
        let mut stream = [0u8; 34];
        stream[10..18].copy_from_slice(&packed.to_be_bytes());
        bytes.extend_from_slice(&stream);

        let summary = parse_flac_summary(&bytes).expect("flac summary");
        let mut text = String::new();
        append_flac_metadata(&mut text, &bytes);

        assert_eq!(media_container_name("clip.bin", &bytes), "FLAC");
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.bits_per_sample, 16);
        assert_eq!(summary.total_samples, 88_200);
        assert!(text.contains("Sample rate: 44,100 Hz"));
        assert!(text.contains("Duration: 0:02"));
    }

    #[test]
    fn media_info_reads_ogg_opus_summary() {
        let mut head = b"OpusHead".to_vec();
        head.extend_from_slice(&[1, 2]);
        head.extend_from_slice(&312u16.to_le_bytes());
        head.extend_from_slice(&48_000u32.to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.push(0);
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&7u32.to_le_bytes());
        tags.extend_from_slice(b"libopus");
        tags.extend_from_slice(&2u32.to_le_bytes());
        let bytes = [ogg_page(&head), ogg_page(&tags)].concat();

        let summary = parse_ogg_summary(&bytes).expect("ogg summary");
        let mut text = String::new();
        append_ogg_metadata(&mut text, &bytes);

        assert_eq!(media_container_name("clip.ogg", &bytes), "Ogg");
        assert_eq!(summary.codec, "Opus");
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 48_000);
        assert_eq!(summary.vendor, "libopus");
        assert_eq!(summary.comments, 2);
        assert!(text.contains("Audio codec: Opus"));
        assert!(text.contains("Tags: 2"));
    }

    #[test]
    fn media_info_reads_ogg_vorbis_summary() {
        let mut ident = b"\x01vorbis".to_vec();
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(2);
        ident.extend_from_slice(&44_100u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&[0, 1]);
        let mut comment = b"\x03vorbis".to_vec();
        comment.extend_from_slice(&10u32.to_le_bytes());
        comment.extend_from_slice(b"Xiph.Org  ");
        comment.extend_from_slice(&1u32.to_le_bytes());
        let bytes = [ogg_page(&ident), ogg_page(&comment)].concat();

        let summary = parse_ogg_summary(&bytes).expect("ogg summary");
        let mut text = String::new();
        append_ogg_metadata(&mut text, &bytes);

        assert_eq!(summary.codec, "Vorbis");
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.vendor, "Xiph.Org");
        assert_eq!(summary.comments, 1);
        assert!(text.contains("Audio codec: Vorbis"));
    }

    #[test]
    fn media_info_reads_mkv_info_and_tracks() {
        fn ebml(id: &[u8], payload: Vec<u8>) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(id);
            if payload.len() < 0x7F {
                out.push(0x80 | payload.len() as u8);
            } else {
                out.push(0x40 | ((payload.len() >> 8) as u8 & 0x3F));
                out.push((payload.len() & 0xFF) as u8);
            }
            out.extend_from_slice(&payload);
            out
        }

        let info = ebml(
            &[0x15, 0x49, 0xA9, 0x66],
            [
                ebml(&[0x2A, 0xD7, 0xB1], vec![0x0F, 0x42, 0x40]),
                ebml(&[0x44, 0x89], 90_000.0f64.to_be_bytes().to_vec()),
                ebml(&[0x57, 0x41], b"QuickLook Writer".to_vec()),
            ]
            .concat(),
        );
        let video = ebml(
            &[0xAE],
            [
                ebml(&[0x83], vec![1]),
                ebml(&[0x86], b"V_MPEG4".to_vec()),
                ebml(
                    &[0xE0],
                    [
                        ebml(&[0xB0], vec![0x07, 0x80]),
                        ebml(&[0xBA], vec![0x04, 0x38]),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let audio = ebml(
            &[0xAE],
            [
                ebml(&[0x83], vec![2]),
                ebml(&[0x86], b"A_OPUS".to_vec()),
                ebml(
                    &[0xE1],
                    [
                        ebml(&[0x9F], vec![2]),
                        ebml(&[0xB5], 48_000.0f64.to_be_bytes().to_vec()),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let segment = ebml(
            &[0x18, 0x53, 0x80, 0x67],
            [
                info,
                ebml(&[0x16, 0x54, 0xAE, 0x6B], [video, audio].concat()),
            ]
            .concat(),
        );
        let bytes = [ebml(&[0x1A, 0x45, 0xDF, 0xA3], Vec::new()), segment].concat();
        let summary = parse_mkv_summary(&bytes).expect("mkv summary");
        let mut text = String::new();

        append_mkv_metadata(&mut text, &bytes);

        assert_eq!(media_container_name("clip.bin", &bytes), "Matroska / WebM");
        assert_eq!(summary.tracks, 2);
        assert_eq!(summary.width, 1920);
        assert_eq!(summary.height, 1080);
        assert_eq!(summary.video_codec, "V_MPEG4");
        assert_eq!(summary.audio_codec, "A_OPUS");
        assert_eq!(summary.audio_channels, 2);
        assert_eq!(summary.sample_rate, Some(48_000.0));
        assert!(text.contains("Duration: 1:30"));
        assert!(text.contains("Audio codec: Opus"));
        assert!(text.contains("Writing app: QuickLook Writer"));
    }

    #[test]
    fn media_info_reads_id3_text_frames() {
        let bytes = make_id3_tag(&[
            ("TIT2", b"\x03Skyline".as_slice()),
            ("TPE1", b"\x03QuickLook Next".as_slice()),
            ("TALB", b"\x03Preview Sessions".as_slice()),
            ("TRCK", b"\x031/9".as_slice()),
            ("TDRC", b"\x032026".as_slice()),
            ("TCON", b"\x03Test".as_slice()),
            ("COMM", b"\x03eng\x00Fast native preview".as_slice()),
        ]);
        let mut text = String::new();

        append_id3_metadata(&mut text, &bytes);

        assert!(text.contains("Title: Skyline"));
        assert!(text.contains("Artist: QuickLook Next"));
        assert!(text.contains("Album: Preview Sessions"));
        assert!(text.contains("Track: 1/9"));
        assert!(text.contains("Year: 2026"));
        assert!(text.contains("Genre: Test"));
        assert!(text.contains("Comment: Fast native preview"));
    }

    #[test]
    fn id3_text_decodes_utf16_bom() {
        let mut payload = vec![1, 0xFF, 0xFE];
        for unit in "北京".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        let bytes = make_id3_tag(&[("TIT2", payload.as_slice())]);
        let fields = parse_id3_text_fields(&bytes);

        assert_eq!(fields.get("TIT2").map(String::as_str), Some("北京"));
    }

    fn ogg_page(packet: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OggS");
        out.extend_from_slice(&[0; 22]);
        out.push(1);
        out.push(packet.len() as u8);
        out.extend_from_slice(packet);
        out
    }

    fn make_id3_tag(frames: &[(&str, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, payload) in frames {
            body.extend_from_slice(id.as_bytes());
            body.extend_from_slice(&id3_synchsafe_bytes(payload.len()));
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(payload);
        }
        let mut tag = Vec::new();
        tag.extend_from_slice(b"ID3");
        tag.extend_from_slice(&[4, 0, 0]);
        tag.extend_from_slice(&id3_synchsafe_bytes(body.len()));
        tag.extend_from_slice(&body);
        tag
    }

    fn id3_synchsafe_bytes(value: usize) -> [u8; 4] {
        [
            ((value >> 21) & 0x7F) as u8,
            ((value >> 14) & 0x7F) as u8,
            ((value >> 7) & 0x7F) as u8,
            (value & 0x7F) as u8,
        ]
    }

    #[test]
    fn mail_header_parser_unfolds_continuations() {
        let headers =
            parse_mail_headers("Subject: hello\r\n world\r\nFrom: a@example.test\r\n\r\nbody");

        assert_eq!(
            headers[0],
            ("Subject".to_string(), "hello world".to_string())
        );
        assert_eq!(
            headers[1],
            ("From".to_string(), "a@example.test".to_string())
        );
    }

    #[test]
    fn mail_header_parameter_extracts_boundary() {
        let value = "multipart/mixed; boundary=\"abc-123\"; charset=utf-8";

        assert_eq!(
            mail_header_parameter(value, "boundary").as_deref(),
            Some("abc-123")
        );
    }

    #[test]
    fn mail_header_decoder_reads_q_encoded_words_and_filenames() {
        assert_eq!(
            decode_mail_header_value("=?UTF-8?Q?Quarterly_Report?="),
            "Quarterly Report"
        );
        assert_eq!(decode_mail_header_value("=?UTF-8?Q?caf=C3=A9?="), "café");
        assert_eq!(
            decode_mail_header_value("=?UTF-8?B?UmVwb3J0IEphbnVhcnk=?="),
            "Report January"
        );
        let names = mail_attachment_filenames(
            "Content-Disposition: attachment; filename=\"=?UTF-8?Q?report_Q1.pdf?=\"\r\n",
        );
        assert_eq!(names, vec!["report Q1.pdf".to_string()]);

        let names = mail_attachment_filenames(
            "Content-Disposition: attachment; filename*=UTF-8''report%20Q2.pdf\r\n",
        );
        assert_eq!(names, vec!["report Q2.pdf".to_string()]);

        let names = mail_attachment_filenames(
            "Content-Disposition: attachment; filename*0*=UTF-8''quarterly%20; filename*1*=summary.pdf\r\n",
        );
        assert_eq!(names, vec!["quarterly summary.pdf".to_string()]);
    }

    #[test]
    fn mail_mime_part_summaries_list_types_and_attachments() {
        let content = "Content-Type: multipart/mixed; boundary=abc\r\n\r\n--abc\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHello\r\n--abc\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=report.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--abc--\r\n";

        assert_eq!(
            mail_mime_part_summaries(content, "abc"),
            vec![
                "text/plain encoding=quoted-printable body=5 bytes decoded=5 bytes preview=\"Hello\"".to_string(),
                "application/pdf (attachment) filename=report.pdf encoding=base64 body=8 bytes decoded=4 bytes".to_string(),
            ]
        );
    }

    #[test]
    fn mail_mime_part_summaries_include_nested_parts() {
        let content = "Content-Type: multipart/mixed; boundary=outer\r\n\r\n--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\nContent-Type: text/plain\r\n\r\nNested hello\r\n--inner--\r\n--outer--\r\n";

        assert_eq!(
            mail_mime_part_summaries(content, "outer"),
            vec![
                "multipart/alternative body=60 bytes".to_string(),
                ">text/plain body=12 bytes preview=\"Nested hello\"".to_string(),
            ]
        );
    }

    #[test]
    fn msg_compound_summary_reads_common_property_streams() {
        fn write_entry(
            bytes: &mut [u8],
            index: usize,
            name: &str,
            object_type: u8,
            start_sector: u32,
            size: u64,
        ) {
            let offset = 1024 + index * 128;
            let mut units = name.encode_utf16().collect::<Vec<_>>();
            units.push(0);
            for (unit_index, unit) in units.iter().enumerate() {
                let pos = offset + unit_index * 2;
                bytes[pos..pos + 2].copy_from_slice(&unit.to_le_bytes());
            }
            bytes[offset + 64..offset + 66]
                .copy_from_slice(&((units.len() * 2) as u16).to_le_bytes());
            bytes[offset + 66] = object_type;
            bytes[offset + 116..offset + 120].copy_from_slice(&start_sector.to_le_bytes());
            bytes[offset + 120..offset + 128].copy_from_slice(&size.to_le_bytes());
        }

        fn write_utf16_stream(bytes: &mut [u8], sector: u32, value: &str) -> u64 {
            let offset = (sector as usize + 1) * 1024;
            let data = value
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            bytes[offset..offset + data.len()].copy_from_slice(&data);
            data.len() as u64
        }

        let mut bytes = vec![0u8; 8192];
        bytes[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        bytes[30..32].copy_from_slice(&10u16.to_le_bytes());
        bytes[48..52].copy_from_slice(&0u32.to_le_bytes());
        let subject_len = write_utf16_stream(&mut bytes, 1, "Quarterly Update");
        let sender_len = write_utf16_stream(&mut bytes, 2, "Alice Example");
        let recipients_len = write_utf16_stream(&mut bytes, 3, "Bob Example; Carol Example");
        let sent_filetime = 116_444_736_000_000_000u64 + 1_700_000_000u64 * 10_000_000;
        bytes[5 * 1024..5 * 1024 + 8].copy_from_slice(&sent_filetime.to_le_bytes());
        write_entry(&mut bytes, 0, "Root Entry", 5, 0, 0);
        write_entry(&mut bytes, 1, "__substg1.0_0037001F", 2, 1, subject_len);
        write_entry(&mut bytes, 2, "__substg1.0_0C1A001F", 2, 2, sender_len);
        write_entry(&mut bytes, 3, "__substg1.0_0E04001F", 2, 3, recipients_len);
        write_entry(&mut bytes, 4, "__substg1.0_0E060040", 2, 4, 8);
        write_entry(&mut bytes, 5, "__substg1.0_1000001F", 2, 5, 12);
        write_entry(
            &mut bytes,
            6,
            "__attach_version1.0_#00000000",
            1,
            0xFFFF_FFFF,
            0,
        );
        write_entry(
            &mut bytes,
            7,
            "__recip_version1.0_#00000000",
            1,
            0xFFFF_FFFF,
            0,
        );
        let mut text = String::new();

        append_msg_compound_summary(&mut text, &bytes);

        assert!(text.contains("Recipients: 1"));
        assert!(text.contains("Attachments: 1"));
        assert!(text.contains("Subject: Quarterly Update"));
        assert!(text.contains("Sender: Alice Example"));
        assert!(text.contains("Recipients display: Bob Example; Carol Example"));
        assert!(text.contains("Sent time:"));
        assert!(text.contains("Body available: yes"));
    }

    #[test]
    fn authenticode_certificate_subjects_reads_x509_names() {
        fn der(tag: u8, content: Vec<u8>) -> Vec<u8> {
            let mut bytes = vec![tag];
            if content.len() < 128 {
                bytes.push(content.len() as u8);
            } else {
                bytes.push(0x82);
                bytes.extend_from_slice(&(content.len() as u16).to_be_bytes());
            }
            bytes.extend_from_slice(&content);
            bytes
        }
        fn seq(children: Vec<Vec<u8>>) -> Vec<u8> {
            der(0x30, children.concat())
        }
        fn name(value: &str) -> Vec<u8> {
            seq(vec![der(
                0x31,
                seq(vec![
                    vec![0x06, 0x03, 0x55, 0x04, 0x03],
                    der(0x0C, value.as_bytes().to_vec()),
                ]),
            )])
        }

        let tbs = seq(vec![
            der(0xA0, der(0x02, vec![2])),
            der(0x02, vec![1]),
            seq(vec![vec![0x06, 0x03, 0x2A, 0x03, 0x04]]),
            name("Issuer Test"),
            seq(vec![
                der(0x17, b"260101000000Z".to_vec()),
                der(0x17, b"270101000000Z".to_vec()),
            ]),
            name("Subject Test"),
            seq(vec![vec![0x06, 0x03, 0x2A, 0x03, 0x05]]),
        ]);
        let cert = seq(vec![
            tbs,
            seq(vec![vec![0x06, 0x03, 0x2A, 0x03, 0x04]]),
            der(0x03, vec![0]),
        ]);
        let mut win_cert = vec![0u8; 8];
        win_cert.extend_from_slice(&cert);
        let cert_len = win_cert.len() as u32;
        win_cert[0..4].copy_from_slice(&cert_len.to_le_bytes());
        let (issuers, subjects) =
            parse_authenticode_certificate_subjects(&win_cert, 0, win_cert.len());

        assert_eq!(issuers, vec!["CN=Issuer Test".to_string()]);
        assert_eq!(subjects, vec!["CN=Subject Test".to_string()]);
    }

    #[test]
    fn authenticode_signer_summary_reads_pkcs7_algorithms() {
        let mut bytes = vec![0u8; 8];
        bytes.extend_from_slice(&[
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
        ]);
        bytes.extend_from_slice(&[
            0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        ]);
        bytes.extend_from_slice(&[
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
        ]);
        let len = bytes.len() as u32;
        bytes[0..4].copy_from_slice(&len.to_le_bytes());

        assert_eq!(
            parse_authenticode_signers(&bytes, 0, bytes.len()),
            vec!["digest SHA-256; signature SHA-256 with RSA".to_string()]
        );
    }

    #[test]
    fn minidump_stream_summary_lists_known_streams() {
        let mut bytes = vec![0u8; 1536];
        bytes[0..4].copy_from_slice(b"MDMP");
        bytes[8..12].copy_from_slice(&9u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&32u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&4u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&128u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&7u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&56u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&0x90u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&6u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&80u32.to_le_bytes());
        bytes[64..68].copy_from_slice(&0x180u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&3u32.to_le_bytes());
        bytes[72..76].copy_from_slice(&100u32.to_le_bytes());
        bytes[76..80].copy_from_slice(&0x1D0u32.to_le_bytes());
        bytes[80..84].copy_from_slice(&4u32.to_le_bytes());
        bytes[84..88].copy_from_slice(&112u32.to_le_bytes());
        bytes[88..92].copy_from_slice(&0x250u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&5u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&36u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&0x380u32.to_le_bytes());
        bytes[104..108].copy_from_slice(&9u32.to_le_bytes());
        bytes[108..112].copy_from_slice(&48u32.to_le_bytes());
        bytes[112..116].copy_from_slice(&0x400u32.to_le_bytes());
        bytes[116..120].copy_from_slice(&24u32.to_le_bytes());
        bytes[120..124].copy_from_slice(&36u32.to_le_bytes());
        bytes[124..128].copy_from_slice(&0x440u32.to_le_bytes());
        bytes[128..132].copy_from_slice(&17u32.to_le_bytes());
        bytes[132..136].copy_from_slice(&48u32.to_le_bytes());
        bytes[136..140].copy_from_slice(&0x4C0u32.to_le_bytes());
        bytes[0x90..0x92].copy_from_slice(&9u16.to_le_bytes());
        bytes[0x96] = 8;
        bytes[0x97] = 1;
        bytes[0x98..0x9C].copy_from_slice(&10u32.to_le_bytes());
        bytes[0x9C..0xA0].copy_from_slice(&0u32.to_le_bytes());
        bytes[0xA0..0xA4].copy_from_slice(&22631u32.to_le_bytes());
        bytes[0xA4..0xA8].copy_from_slice(&2u32.to_le_bytes());
        bytes[0xA8..0xAC].copy_from_slice(&0x120u32.to_le_bytes());
        bytes[0xAC..0xAE].copy_from_slice(&0x0100u16.to_le_bytes());
        let csd: Vec<u8> = "Service Pack 1"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes[0x120..0x124].copy_from_slice(&(csd.len() as u32).to_le_bytes());
        bytes[0x124..0x124 + csd.len()].copy_from_slice(&csd);
        bytes[0x180..0x184].copy_from_slice(&42u32.to_le_bytes());
        bytes[0x188..0x18C].copy_from_slice(&0xC000_0005u32.to_le_bytes());
        bytes[0x18C..0x190].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x198..0x1A0].copy_from_slice(&0x0000_7FFF_FFFF_FFFFu64.to_le_bytes());
        bytes[0x1A0..0x1A4].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x1D0..0x1D4].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x1D4..0x1D8].copy_from_slice(&42u32.to_le_bytes());
        bytes[0x1E0..0x1E4].copy_from_slice(&15u32.to_le_bytes());
        bytes[0x1EC..0x1F4].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes[0x1F4..0x1F8].copy_from_slice(&0x4000u32.to_le_bytes());
        bytes[0x204..0x208].copy_from_slice(&99u32.to_le_bytes());
        bytes[0x210..0x214].copy_from_slice(&8u32.to_le_bytes());
        bytes[0x21C..0x224].copy_from_slice(&0x9000u64.to_le_bytes());
        bytes[0x224..0x228].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x250..0x254].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x254..0x25C].copy_from_slice(&0x0000_7FF7_0000_0000u64.to_le_bytes());
        bytes[0x25C..0x260].copy_from_slice(&0x12000u32.to_le_bytes());
        bytes[0x264..0x268].copy_from_slice(&0x6543_2100u32.to_le_bytes());
        bytes[0x268..0x26C].copy_from_slice(&0x340u32.to_le_bytes());
        bytes[0x26C..0x270].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
        bytes[0x274..0x278].copy_from_slice(&0x0001_0002u32.to_le_bytes());
        bytes[0x278..0x27C].copy_from_slice(&0x0003_0004u32.to_le_bytes());
        bytes[0x27C..0x280].copy_from_slice(&0x0005_0006u32.to_le_bytes());
        bytes[0x280..0x284].copy_from_slice(&0x0007_0008u32.to_le_bytes());
        bytes[0x284..0x288].copy_from_slice(&0x0000_0003u32.to_le_bytes());
        bytes[0x288..0x28C].copy_from_slice(&0x0000_0002u32.to_le_bytes());
        bytes[0x290..0x294].copy_from_slice(&2u32.to_le_bytes());
        let module_name: Vec<u16> = "demo.exe".encode_utf16().collect();
        bytes[0x340..0x344].copy_from_slice(&((module_name.len() * 2) as u32).to_le_bytes());
        for (index, unit) in module_name.iter().enumerate() {
            let offset = 0x344 + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[0x380..0x384].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x384..0x38C].copy_from_slice(&0x0010_0000u64.to_le_bytes());
        bytes[0x38C..0x390].copy_from_slice(&0x2000u32.to_le_bytes());
        bytes[0x394..0x39C].copy_from_slice(&0x0020_0000u64.to_le_bytes());
        bytes[0x39C..0x3A0].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x400..0x408].copy_from_slice(&2u64.to_le_bytes());
        bytes[0x408..0x410].copy_from_slice(&0x500u64.to_le_bytes());
        bytes[0x410..0x418].copy_from_slice(&0x0030_0000u64.to_le_bytes());
        bytes[0x418..0x420].copy_from_slice(&0x3000u64.to_le_bytes());
        bytes[0x420..0x428].copy_from_slice(&0x0040_0000u64.to_le_bytes());
        bytes[0x428..0x430].copy_from_slice(&0x1000u64.to_le_bytes());
        bytes[0x440..0x444].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x444..0x448].copy_from_slice(&42u32.to_le_bytes());
        bytes[0x44C..0x454].copy_from_slice(&0x480u64.to_le_bytes());
        bytes[0x454..0x458].copy_from_slice(&99u32.to_le_bytes());
        bytes[0x45C..0x464].copy_from_slice(&0x4A0u64.to_le_bytes());
        let worker: Vec<u8> = "worker".encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes[0x480..0x484].copy_from_slice(&(worker.len() as u32).to_le_bytes());
        bytes[0x484..0x484 + worker.len()].copy_from_slice(&worker);
        let io_thread: Vec<u8> = "io thread"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes[0x4A0..0x4A4].copy_from_slice(&(io_thread.len() as u32).to_le_bytes());
        bytes[0x4A4..0x4A4 + io_thread.len()].copy_from_slice(&io_thread);
        bytes[0x4C0..0x4C4].copy_from_slice(&16u32.to_le_bytes());
        bytes[0x4C4..0x4C8].copy_from_slice(&32u32.to_le_bytes());
        bytes[0x4C8..0x4CC].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x4D0..0x4D8].copy_from_slice(&0x44u64.to_le_bytes());
        bytes[0x4D8..0x4DC].copy_from_slice(&0x520u32.to_le_bytes());
        bytes[0x4DC..0x4E0].copy_from_slice(&0x540u32.to_le_bytes());
        bytes[0x4E0..0x4E4].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x4E4..0x4E8].copy_from_slice(&0x0012_019Fu32.to_le_bytes());
        bytes[0x4E8..0x4EC].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x4EC..0x4F0].copy_from_slice(&7u32.to_le_bytes());
        let file_type: Vec<u8> = "File".encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes[0x520..0x524].copy_from_slice(&(file_type.len() as u32).to_le_bytes());
        bytes[0x524..0x524 + file_type.len()].copy_from_slice(&file_type);
        let object_name: Vec<u8> = r"\Device\HarddiskVolume1\demo.txt"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes[0x540..0x544].copy_from_slice(&(object_name.len() as u32).to_le_bytes());
        bytes[0x544..0x544 + object_name.len()].copy_from_slice(&object_name);
        let mut text = String::new();

        append_minidump_streams(&mut text, &bytes);

        assert!(text.contains("ModuleList"));
        assert!(text.contains("SystemInfo"));
        assert!(text.contains("System architecture: x64"));
        assert!(text.contains("Processors: 8"));
        assert!(text.contains("Windows version: 10.0.22631"));
        assert!(text.contains("Service pack: Service Pack 1"));
        assert!(text.contains("Exception thread: 42"));
        assert!(text.contains("Exception code: access violation"));
        assert!(text.contains("Exception flags: 0x00000001"));
        assert!(text.contains("Threads: 2"));
        assert!(
            text.contains("Thread 42: priority 15; stack 0x0000000000001000-0x0000000000005000")
        );
        assert!(text.contains("Thread 99: priority 8; stack 0x0000000000009000-0x000000000000A000"));
        assert!(text.contains("Modules: 1"));
        assert!(text.contains("Module demo.exe: base 0x00007FF700000000; size 73728; timestamp 0x65432100; file version 1.2.3.4; product version 5.6.7.8; type DLL; flags 0x00000002"));
        assert!(text.contains("Memory ranges: 2"));
        assert!(text.contains("Memory bytes listed: 12288"));
        assert!(text.contains("Memory 0x0000000000100000-0x0000000000102000 (8192 bytes)"));
        assert!(text.contains("Memory64 ranges: 2"));
        assert!(text.contains("Memory64 base RVA: 0x500"));
        assert!(text.contains("Memory64 bytes listed: 16384"));
        assert!(text.contains("Memory64 0x0000000000300000-0x0000000000303000 (12288 bytes)"));
        assert!(text.contains("Thread names: 2"));
        assert!(text.contains("Thread 42 name: worker"));
        assert!(text.contains("Thread 99 name: io thread"));
        assert!(text.contains("Handles: 1"));
        assert!(text.contains(r"Handle 0x0000000000000044: File \Device\HarddiskVolume1\demo.txt; access 0x0012019F; attributes 0x00000002; handles 3; pointers 7"));
    }

    #[test]
    fn minidump_unloaded_module_list_summarizes_names_and_ranges() {
        let mut bytes = vec![0u8; 512];
        bytes[0x40..0x44].copy_from_slice(&12u32.to_le_bytes());
        bytes[0x44..0x48].copy_from_slice(&24u32.to_le_bytes());
        bytes[0x48..0x4C].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x4C..0x54].copy_from_slice(&0x0000_7FF6_1000_0000u64.to_le_bytes());
        bytes[0x54..0x58].copy_from_slice(&0x5000u32.to_le_bytes());
        bytes[0x58..0x5C].copy_from_slice(&0x1234_ABCDu32.to_le_bytes());
        bytes[0x5C..0x60].copy_from_slice(&0x6543_2100u32.to_le_bytes());
        bytes[0x60..0x64].copy_from_slice(&0x100u32.to_le_bytes());
        let module_name: Vec<u8> = "old.dll"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes[0x100..0x104].copy_from_slice(&(module_name.len() as u32).to_le_bytes());
        bytes[0x104..0x104 + module_name.len()].copy_from_slice(&module_name);

        let text = parse_minidump_unloaded_module_list(&bytes, 0x40, 36).expect("unloaded modules");

        assert!(text.contains("Unloaded modules: 1"));
        assert!(text.contains("Unloaded module old.dll: range 0x00007FF610000000-0x00007FF610005000; timestamp 0x65432100; checksum 0x1234ABCD"));
    }

    #[test]
    fn minidump_misc_info_summarizes_process_and_power_fields() {
        let mut bytes = vec![0u8; 128];
        bytes[0x20..0x24].copy_from_slice(&44u32.to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&0x7u32.to_le_bytes());
        bytes[0x28..0x2C].copy_from_slice(&4242u32.to_le_bytes());
        bytes[0x2C..0x30].copy_from_slice(&1_700_000_000u32.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&12u32.to_le_bytes());
        bytes[0x34..0x38].copy_from_slice(&34u32.to_le_bytes());
        bytes[0x38..0x3C].copy_from_slice(&4800u32.to_le_bytes());
        bytes[0x3C..0x40].copy_from_slice(&3600u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(&4200u32.to_le_bytes());
        bytes[0x44..0x48].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x48..0x4C].copy_from_slice(&1u32.to_le_bytes());

        let text = parse_minidump_misc_info(&bytes, 0x20, 44).expect("misc info");

        assert!(text.contains("MiscInfo flags: 0x00000007"));
        assert!(text.contains("Process ID: 4242"));
        assert!(text.contains("Process create time: 1700000000"));
        assert!(text.contains("Process user time: 12s"));
        assert!(text.contains("Process kernel time: 34s"));
        assert!(text
            .contains("Processor power: max 4800 MHz; current 3600 MHz; limit 4200 MHz; idle 1/3"));
    }

    #[test]
    fn chm_itsp_summary_reads_directory_header() {
        let mut bytes = vec![0u8; 512];
        bytes[0..4].copy_from_slice(b"ITSF");
        bytes[40..48].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[0x100..0x104].copy_from_slice(b"ITSP");
        bytes[0x104..0x108].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x108..0x10C].copy_from_slice(&84u32.to_le_bytes());
        bytes[0x110..0x114].copy_from_slice(&128u32.to_le_bytes());
        bytes[0x118..0x11C].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x11C..0x120].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x128..0x12C].copy_from_slice(&7u32.to_le_bytes());
        bytes[0x154..0x158].copy_from_slice(b"PMGL");
        bytes[0x158..0x15C].copy_from_slice(&36u32.to_le_bytes());
        bytes[0x168] = 10;
        bytes[0x169..0x173].copy_from_slice(b"/index.htm");
        bytes[0x173] = 0;
        bytes[0x174] = 123;
        bytes[0x175] = 45;
        bytes[0x176] = 40;
        bytes[0x177..0x19F].copy_from_slice(b"::DataSpace/Storage/MSCompressed/Content");
        bytes[0x19F] = 1;
        bytes[0x1A0] = 0;
        bytes[0x1A1] = 0x81;
        bytes[0x1A2] = 0x48;
        bytes[0x1A3] = 8;
        bytes[0x1A4..0x1AC].copy_from_slice(b"/#SYSTEM");
        bytes[0x1AC] = 0;
        bytes[0x1AD] = 0x83;
        bytes[0x1AE] = 0x40;
        bytes[0x1AF] = 28;
        bytes[0x1C0..0x1C2].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x1C2..0x1C4].copy_from_slice(&10u16.to_le_bytes());
        bytes[0x1C4..0x1CE].copy_from_slice(b"Help Title");
        bytes[0x1CE..0x1D0].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x1D0..0x1D2].copy_from_slice(&10u16.to_le_bytes());
        bytes[0x1D2..0x1DC].copy_from_slice(b"/index.htm");
        let mut text = String::new();

        append_chm_itsp_summary(&mut text, &bytes);

        assert!(text.contains("ITSP version: 1"));
        assert!(text.contains("ITSP header length: 84 bytes"));
        assert!(text.contains("Directory block length: 128 bytes"));
        assert!(text.contains("Directory block count: 7"));
        assert!(text.contains("Directory index depth/root/head: 2/3/4"));
        assert!(text.contains("Directory entries: /index.htm [section 0, offset 123, 45 B]"));
        assert!(
            text.contains("Compressed streams: ::DataSpace/Storage/MSCompressed/Content (200 B)")
        );
        assert!(text.contains("Title: Help Title"));
        assert!(text.contains("Default topic: /index.htm"));
    }

    #[test]
    fn pe_summary_reads_optional_headers_and_sections() {
        fn utf16z(value: &str) -> Vec<u8> {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        }

        fn align_vec(bytes: &mut Vec<u8>) {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
        }

        fn version_node(key: &str, value: Option<&str>, children: Vec<Vec<u8>>) -> Vec<u8> {
            let mut bytes = vec![0, 0];
            let value_units = value.map(|v| v.encode_utf16().count() + 1).unwrap_or(0) as u16;
            bytes.extend_from_slice(&value_units.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&utf16z(key));
            align_vec(&mut bytes);
            if let Some(value) = value {
                bytes.extend_from_slice(&utf16z(value));
                align_vec(&mut bytes);
            }
            for child in children {
                bytes.extend_from_slice(&child);
            }
            let len = bytes.len() as u16;
            bytes[0..2].copy_from_slice(&len.to_le_bytes());
            bytes
        }

        fn version_node_raw(key: &str, value: &[u8], children: Vec<Vec<u8>>) -> Vec<u8> {
            let mut bytes = vec![0, 0];
            bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&utf16z(key));
            align_vec(&mut bytes);
            bytes.extend_from_slice(value);
            align_vec(&mut bytes);
            for child in children {
                bytes.extend_from_slice(&child);
            }
            let len = bytes.len() as u16;
            bytes[0..2].copy_from_slice(&len.to_le_bytes());
            bytes
        }

        fn clr_metadata_root() -> Vec<u8> {
            let mut bytes = vec![0u8; 384];
            bytes[0..4].copy_from_slice(&0x424A_5342u32.to_le_bytes());
            bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
            bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
            bytes[12..16].copy_from_slice(&12u32.to_le_bytes());
            bytes[16..27].copy_from_slice(b"v4.0.30319\0");
            bytes[30..32].copy_from_slice(&2u16.to_le_bytes());
            bytes[32..36].copy_from_slice(&0x80u32.to_le_bytes());
            bytes[36..40].copy_from_slice(&0xA0u32.to_le_bytes());
            bytes[40..43].copy_from_slice(b"#~\0");
            bytes[44..48].copy_from_slice(&0x120u32.to_le_bytes());
            bytes[48..52].copy_from_slice(&0x60u32.to_le_bytes());
            bytes[52..61].copy_from_slice(b"#Strings\0");
            bytes[0x80 + 4] = 2;
            bytes[0x80 + 8..0x80 + 16].copy_from_slice(
                &((1u64 << 2) | (1u64 << 12) | (1u64 << 32) | (1u64 << 35)).to_le_bytes(),
            );
            bytes[0x80 + 24..0x80 + 28].copy_from_slice(&1u32.to_le_bytes());
            bytes[0x80 + 28..0x80 + 32].copy_from_slice(&1u32.to_le_bytes());
            bytes[0x80 + 32..0x80 + 36].copy_from_slice(&1u32.to_le_bytes());
            bytes[0x80 + 36..0x80 + 40].copy_from_slice(&1u32.to_le_bytes());
            let type_row = 0x80 + 40;
            bytes[type_row + 4..type_row + 6].copy_from_slice(&17u16.to_le_bytes());
            bytes[type_row + 6..type_row + 8].copy_from_slice(&29u16.to_le_bytes());
            let custom_attribute_row = type_row + 14;
            let row = custom_attribute_row + 6;
            bytes[row + 4..row + 6].copy_from_slice(&1u16.to_le_bytes());
            bytes[row + 6..row + 8].copy_from_slice(&2u16.to_le_bytes());
            bytes[row + 8..row + 10].copy_from_slice(&3u16.to_le_bytes());
            bytes[row + 10..row + 12].copy_from_slice(&4u16.to_le_bytes());
            bytes[row + 18..row + 20].copy_from_slice(&1u16.to_le_bytes());
            let assembly_ref = row + 22;
            bytes[assembly_ref..assembly_ref + 2].copy_from_slice(&5u16.to_le_bytes());
            bytes[assembly_ref + 2..assembly_ref + 4].copy_from_slice(&6u16.to_le_bytes());
            bytes[assembly_ref + 4..assembly_ref + 6].copy_from_slice(&7u16.to_le_bytes());
            bytes[assembly_ref + 6..assembly_ref + 8].copy_from_slice(&8u16.to_le_bytes());
            bytes[assembly_ref + 14..assembly_ref + 16].copy_from_slice(&10u16.to_le_bytes());
            bytes[0x121..0x129].copy_from_slice(b"QuickAsm");
            bytes[0x12A..0x130].copy_from_slice(b"RefAsm");
            bytes[0x131..0x13C].copy_from_slice(b"PreviewType");
            bytes[0x13D..0x14B].copy_from_slice(b"QuickLook.Next");
            bytes
        }

        let mut bytes = vec![0u8; 8192];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84usize;
        bytes[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[coff + 2..coff + 4].copy_from_slice(&2u16.to_le_bytes());
        bytes[coff + 16..coff + 18].copy_from_slice(&0xF0u16.to_le_bytes());
        let opt = coff + 20;
        bytes[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        bytes[opt + 16..opt + 20].copy_from_slice(&0x1234u32.to_le_bytes());
        bytes[opt + 24..opt + 32].copy_from_slice(&0x1400_0000u64.to_le_bytes());
        bytes[opt + 32..opt + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[opt + 36..opt + 40].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[opt + 56..opt + 60].copy_from_slice(&0x5000u32.to_le_bytes());
        bytes[opt + 68..opt + 70].copy_from_slice(&2u16.to_le_bytes());
        bytes[opt + 70..opt + 72].copy_from_slice(&0x8160u16.to_le_bytes());
        bytes[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        bytes[opt + 112..opt + 116].copy_from_slice(&0x3300u32.to_le_bytes());
        bytes[opt + 116..opt + 120].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[opt + 120..opt + 124].copy_from_slice(&0x3000u32.to_le_bytes());
        bytes[opt + 124..opt + 128].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[opt + 128..opt + 132].copy_from_slice(&0x3500u32.to_le_bytes());
        bytes[opt + 132..opt + 136].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[opt + 144..opt + 148].copy_from_slice(&0x1800u32.to_le_bytes());
        bytes[opt + 148..opt + 152].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[opt + 224..opt + 228].copy_from_slice(&0x3600u32.to_le_bytes());
        bytes[opt + 228..opt + 232].copy_from_slice(&0x48u32.to_le_bytes());
        let section_table = opt + 0xF0;
        bytes[section_table..section_table + 5].copy_from_slice(b".text");
        bytes[section_table + 8..section_table + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[section_table + 12..section_table + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[section_table + 16..section_table + 20].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[section_table + 20..section_table + 24].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[section_table + 40..section_table + 45].copy_from_slice(b".data");
        bytes[section_table + 48..section_table + 52].copy_from_slice(&0x2000u32.to_le_bytes());
        bytes[section_table + 52..section_table + 56].copy_from_slice(&0x3000u32.to_le_bytes());
        bytes[section_table + 56..section_table + 60].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[section_table + 60..section_table + 64].copy_from_slice(&0x400u32.to_le_bytes());
        bytes[0x400..0x404].copy_from_slice(&0x3120u32.to_le_bytes());
        bytes[0x400 + 12..0x400 + 16].copy_from_slice(&0x3100u32.to_le_bytes());
        bytes[0x400 + 16..0x400 + 20].copy_from_slice(&0x3200u32.to_le_bytes());
        bytes[0x500..0x50C].copy_from_slice(b"KERNEL32.dll");
        bytes[0x520..0x528].copy_from_slice(&0x3140u64.to_le_bytes());
        bytes[0x528..0x530].copy_from_slice(&0x8000_0000_0000_007Bu64.to_le_bytes());
        bytes[0x540..0x542].copy_from_slice(&0u16.to_le_bytes());
        bytes[0x542..0x54D].copy_from_slice(b"CreateFileW");
        bytes[0x700 + 16..0x700 + 20].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x700 + 20..0x700 + 24].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x700 + 24..0x700 + 28].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x700 + 28..0x700 + 32].copy_from_slice(&0x3380u32.to_le_bytes());
        bytes[0x700 + 32..0x700 + 36].copy_from_slice(&0x3340u32.to_le_bytes());
        bytes[0x700 + 36..0x700 + 40].copy_from_slice(&0x3390u32.to_le_bytes());
        bytes[0x740..0x744].copy_from_slice(&0x3360u32.to_le_bytes());
        bytes[0x760..0x76D].copy_from_slice(b"PreviewExport");
        bytes[0x780..0x784].copy_from_slice(&0x2000u32.to_le_bytes());
        bytes[0x900 + 14..0x900 + 16].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x900 + 16..0x900 + 20].copy_from_slice(&16u32.to_le_bytes());
        bytes[0x900 + 20..0x900 + 24].copy_from_slice(&0x8000_0020u32.to_le_bytes());
        bytes[0x920 + 14..0x920 + 16].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x920 + 16..0x920 + 20].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x920 + 20..0x920 + 24].copy_from_slice(&0x8000_0040u32.to_le_bytes());
        bytes[0x940 + 14..0x940 + 16].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x940 + 16..0x940 + 20].copy_from_slice(&1033u32.to_le_bytes());
        bytes[0x940 + 20..0x940 + 24].copy_from_slice(&0x60u32.to_le_bytes());
        let mut fixed = vec![0u8; 52];
        fixed[0..4].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
        fixed[4..8].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        fixed[8..12].copy_from_slice(&0x0001_0002u32.to_le_bytes());
        fixed[12..16].copy_from_slice(&0x0003_0004u32.to_le_bytes());
        fixed[16..20].copy_from_slice(&0x0005_0006u32.to_le_bytes());
        fixed[20..24].copy_from_slice(&0x0007_0008u32.to_le_bytes());
        fixed[24..28].copy_from_slice(&0x0000_003Fu32.to_le_bytes());
        fixed[28..32].copy_from_slice(&0x0000_0002u32.to_le_bytes());
        fixed[36..40].copy_from_slice(&2u32.to_le_bytes());
        let version = version_node_raw(
            "VS_VERSION_INFO",
            &fixed,
            vec![version_node(
                "StringFileInfo",
                None,
                vec![version_node(
                    "040904B0",
                    None,
                    vec![
                        version_node("CompanyName", Some("QuickLook Next"), Vec::new()),
                        version_node("FileVersion", Some("1.2.3"), Vec::new()),
                    ],
                )],
            )],
        );
        bytes[0x960..0x964].copy_from_slice(&0x3900u32.to_le_bytes());
        bytes[0x964..0x968].copy_from_slice(&(version.len() as u32).to_le_bytes());
        bytes[0xD00..0xD00 + version.len()].copy_from_slice(&version);
        bytes[0xA00..0xA04].copy_from_slice(&72u32.to_le_bytes());
        bytes[0xA04..0xA06].copy_from_slice(&2u16.to_le_bytes());
        bytes[0xA06..0xA08].copy_from_slice(&5u16.to_le_bytes());
        bytes[0xA08..0xA0C].copy_from_slice(&0x3700u32.to_le_bytes());
        bytes[0xA0C..0xA10].copy_from_slice(&0x180u32.to_le_bytes());
        bytes[0xA10..0xA14].copy_from_slice(&1u32.to_le_bytes());
        let metadata = clr_metadata_root();
        bytes[0xB00..0xB00 + metadata.len()].copy_from_slice(&metadata);
        bytes[0x1800..0x1804].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x1804..0x1806].copy_from_slice(&0x0200u16.to_le_bytes());
        bytes[0x1806..0x1808].copy_from_slice(&0x0002u16.to_le_bytes());
        bytes[0x1808..0x1813].copy_from_slice(&[
            0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        ]);
        bytes[0x1818..0x1823].copy_from_slice(&[
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
        ]);
        bytes[0x1828..0x182D].copy_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03]);
        bytes[0x182D..0x183D].copy_from_slice(b"\x0C\x0EQuickLook Test");

        let pe = parse_pe_headers(&bytes).expect("pe summary");

        assert_eq!(pe.machine, "x64");
        assert_eq!(pe.image_base, 0x1400_0000);
        assert_eq!(pe.section_alignment, 0x1000);
        assert_eq!(pe.dll_characteristics, 0x8160);
        assert_eq!(pe.data_directories, 16);
        assert_eq!(pe.directories.len(), 5);
        assert_eq!(pe.directories[0].name, "Export");
        assert_eq!(pe.directories[1].name, "Import");
        assert_eq!(pe.directories[2].name, "Resource");
        assert_eq!(pe.imports, vec!["KERNEL32.dll".to_string()]);
        assert_eq!(
            pe.imported_functions,
            vec![
                "KERNEL32.dll!CreateFileW".to_string(),
                "KERNEL32.dll!#123".to_string()
            ]
        );
        assert_eq!(pe.exports, vec!["PreviewExport".to_string()]);
        assert_eq!(
            pe.export_details,
            vec!["PreviewExport #1 @ 0x00002000".to_string()]
        );
        assert!(pe.has_version_resource);
        assert_eq!(
            pe.version_strings,
            vec![
                ("CompanyName".to_string(), "QuickLook Next".to_string()),
                ("FileVersion".to_string(), "1.2.3".to_string())
            ]
        );
        let fixed = pe.fixed_version.as_ref().expect("fixed version");
        assert_eq!(fixed.file_version, "1.2.3.4");
        assert_eq!(fixed.product_version, "5.6.7.8");
        assert_eq!(fixed.flags, 2);
        assert_eq!(fixed.file_type, "DLL");
        assert_eq!(pe.certificate.as_ref().map(|cert| cert.typ), Some(2));
        assert_eq!(
            pe.certificate
                .as_ref()
                .map(|cert| cert.digest_algorithms.clone())
                .unwrap_or_default(),
            vec!["SHA-256".to_string()]
        );
        assert_eq!(
            pe.certificate
                .as_ref()
                .map(|cert| cert.signature_algorithms.clone())
                .unwrap_or_default(),
            vec!["SHA-256 with RSA".to_string()]
        );
        assert_eq!(
            pe.certificate
                .as_ref()
                .map(|cert| cert.names.clone())
                .unwrap_or_default(),
            vec!["CN=QuickLook Test".to_string()]
        );
        assert_eq!(
            pe.clr.as_ref().map(|clr| (clr.major, clr.minor, clr.flags)),
            Some((2, 5, 1))
        );
        let clr = pe.clr.as_ref().expect("clr summary");
        assert_eq!(clr.metadata_version, "v4.0.30319");
        assert_eq!(
            clr.metadata_streams,
            vec!["#~".to_string(), "#Strings".to_string()]
        );
        assert_eq!(
            clr.metadata_tables,
            vec![
                "TypeDef=1".to_string(),
                "CustomAttribute=1".to_string(),
                "Assembly=1".to_string(),
                "AssemblyRef=1".to_string()
            ]
        );
        assert_eq!(clr.assembly.as_deref(), Some("QuickAsm 1.2.3.4"));
        assert_eq!(clr.assembly_refs, vec!["RefAsm 5.6.7.8".to_string()]);
        assert_eq!(
            clr.type_defs,
            vec!["QuickLook.Next.PreviewType".to_string()]
        );
        assert_eq!(clr.custom_attributes, 1);
        assert_eq!(
            pe.section_names,
            vec![".text".to_string(), ".data".to_string()]
        );
    }

    #[test]
    fn elf_summary_detects_64_bit_little_endian() {
        let mut bytes = vec![0u8; 2048];
        bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&0x40u64.to_le_bytes());
        bytes[40..48].copy_from_slice(&0x500u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&4u16.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&7u16.to_le_bytes());
        bytes[62..64].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x48..0x50].copy_from_slice(&0x300u64.to_le_bytes());
        bytes[0x60..0x68].copy_from_slice(&28u64.to_le_bytes());
        bytes[0x78..0x7C].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x80..0x88].copy_from_slice(&0u64.to_le_bytes());
        bytes[0x88..0x90].copy_from_slice(&0x400000u64.to_le_bytes());
        bytes[0x98..0xA0].copy_from_slice(&0x400u64.to_le_bytes());
        bytes[0xA0..0xA8].copy_from_slice(&0x400u64.to_le_bytes());
        bytes[0xB0..0xB4].copy_from_slice(&2u32.to_le_bytes());
        bytes[0xB8..0xC0].copy_from_slice(&0x200u64.to_le_bytes());
        bytes[0xD0..0xD8].copy_from_slice(&80u64.to_le_bytes());
        bytes[0xE8..0xEC].copy_from_slice(&4u32.to_le_bytes());
        bytes[0xF0..0xF8].copy_from_slice(&0x7C4u64.to_le_bytes());
        bytes[0x108..0x110].copy_from_slice(&20u64.to_le_bytes());
        bytes[0x200..0x208].copy_from_slice(&5u64.to_le_bytes());
        bytes[0x208..0x210].copy_from_slice(&0x400280u64.to_le_bytes());
        bytes[0x210..0x218].copy_from_slice(&1u64.to_le_bytes());
        bytes[0x218..0x220].copy_from_slice(&0u64.to_le_bytes());
        bytes[0x220..0x228].copy_from_slice(&14u64.to_le_bytes());
        bytes[0x228..0x230].copy_from_slice(&10u64.to_le_bytes());
        bytes[0x230..0x238].copy_from_slice(&29u64.to_le_bytes());
        bytes[0x238..0x240].copy_from_slice(&21u64.to_le_bytes());
        bytes[0x240..0x248].copy_from_slice(&0u64.to_le_bytes());
        bytes[0x280..0x28A].copy_from_slice(b"libc.so.6\0");
        bytes[0x28A..0x295].copy_from_slice(b"libdemo.so\0");
        bytes[0x295..0x29D].copy_from_slice(b"$ORIGIN\0");
        bytes[0x300..0x31B].copy_from_slice(b"/lib64/ld-linux-x86-64.so.2");
        bytes[0x540..0x544].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x580..0x584].copy_from_slice(&7u32.to_le_bytes());
        bytes[0x598..0x5A0].copy_from_slice(&0x700u64.to_le_bytes());
        bytes[0x5A0..0x5A8].copy_from_slice(&62u64.to_le_bytes());
        bytes[0x5C0..0x5C4].copy_from_slice(&17u32.to_le_bytes());
        bytes[0x5C4..0x5C8].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x5D8..0x5E0].copy_from_slice(&0x740u64.to_le_bytes());
        bytes[0x5E0..0x5E8].copy_from_slice(&48u64.to_le_bytes());
        bytes[0x5E8..0x5EC].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x5F8..0x600].copy_from_slice(&24u64.to_le_bytes());
        bytes[0x600..0x604].copy_from_slice(&25u32.to_le_bytes());
        bytes[0x604..0x608].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x618..0x620].copy_from_slice(&0x780u64.to_le_bytes());
        bytes[0x620..0x628].copy_from_slice(&13u64.to_le_bytes());
        bytes[0x640..0x644].copy_from_slice(&33u32.to_le_bytes());
        bytes[0x644..0x648].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x658..0x660].copy_from_slice(&0x790u64.to_le_bytes());
        bytes[0x660..0x668].copy_from_slice(&24u64.to_le_bytes());
        bytes[0x678..0x680].copy_from_slice(&24u64.to_le_bytes());
        bytes[0x680..0x684].copy_from_slice(&43u32.to_le_bytes());
        bytes[0x684..0x688].copy_from_slice(&7u32.to_le_bytes());
        bytes[0x698..0x6A0].copy_from_slice(&0x7B0u64.to_le_bytes());
        bytes[0x6A0..0x6A8].copy_from_slice(&20u64.to_le_bytes());
        bytes[0x700..0x73E].copy_from_slice(
            b"\0.text\0.shstrtab\0.symtab\0.strtab\0.rela.dyn\0.note.gnu.build-id\0",
        );
        bytes[0x740..0x744].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x758..0x75C].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x75C] = 0x12;
        bytes[0x75E..0x760].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x780..0x78D].copy_from_slice(b"\0main\0helper\0");
        bytes[0x7B0..0x7B4].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x7B4..0x7B8].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x7B8..0x7BC].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x7BC..0x7C0].copy_from_slice(b"GNU\0");
        bytes[0x7C0..0x7C4].copy_from_slice(&[1, 2, 3, 4]);
        bytes[0x7C4..0x7C8].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x7C8..0x7CC].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x7CC..0x7D0].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x7D0..0x7D4].copy_from_slice(b"GNU\0");
        bytes[0x7D4..0x7D8].copy_from_slice(&[5, 6, 7, 8]);
        bytes[0x798..0x7A0].copy_from_slice(&8u64.to_le_bytes());

        let mut text = String::new();
        append_elf_summary(&mut text, &bytes);

        assert!(text.contains("ELF64"));
        assert!(text.contains("x86-64"));
        assert!(text.contains("0x0000000000401000"));
        assert!(text.contains("Program headers: 4"));
        assert!(text.contains("Section headers: 7"));
        assert!(text.contains("Program header offset: 0x40"));
        assert!(text.contains("Section header offset: 0x500"));
        assert!(text.contains("Interpreter: /lib64/ld-linux-x86-64.so.2"));
        assert!(text.contains("Needed libraries: libc.so.6"));
        assert!(text.contains("SONAME: libdemo.so"));
        assert!(text.contains("RUNPATH: $ORIGIN"));
        assert!(text.contains(
            "Section names: .text, .shstrtab, .symtab, .strtab, .rela.dyn, .note.gnu.build-id"
        ));
        assert!(text.contains("Symbols: .symtab 2 entries (main[global func .text])"));
        assert!(text.contains("Relocations: .rela.dyn 1 entries (R_X86_64_RELATIVE)"));
        assert!(text.contains("Notes: .note.gnu.build-id GNU build-id 01020304"));
        assert!(text.contains("PT_NOTE GNU type 1 (4 bytes)"));
    }

    #[test]
    fn elf_summary_reads_gnu_version_sections() {
        fn write_sh64(
            bytes: &mut [u8],
            index: usize,
            name: u32,
            typ: u32,
            offset: u64,
            size: u64,
            link: u32,
            entsize: u64,
        ) {
            let base = 0x100 + index * 64;
            bytes[base..base + 4].copy_from_slice(&name.to_le_bytes());
            bytes[base + 4..base + 8].copy_from_slice(&typ.to_le_bytes());
            bytes[base + 24..base + 32].copy_from_slice(&offset.to_le_bytes());
            bytes[base + 32..base + 40].copy_from_slice(&size.to_le_bytes());
            bytes[base + 40..base + 44].copy_from_slice(&link.to_le_bytes());
            bytes[base + 56..base + 64].copy_from_slice(&entsize.to_le_bytes());
        }

        let mut bytes = vec![0u8; 1024];
        bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[40..48].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&6u16.to_le_bytes());
        bytes[62..64].copy_from_slice(&1u16.to_le_bytes());
        write_sh64(&mut bytes, 1, 1, 3, 0x300, 62, 0, 0);
        write_sh64(&mut bytes, 2, 11, 3, 0x340, 27, 0, 0);
        write_sh64(&mut bytes, 3, 19, 0x6FFF_FFFF, 0x380, 6, 0, 2);
        write_sh64(&mut bytes, 4, 32, 0x6FFF_FFFE, 0x390, 32, 2, 0);
        write_sh64(&mut bytes, 5, 47, 0x6FFF_FFFD, 0x3C0, 28, 2, 0);
        bytes[0x300..0x33E].copy_from_slice(
            b"\0.shstrtab\0.dynstr\0.gnu.version\0.gnu.version_r\0.gnu.version_d\0",
        );
        bytes[0x340..0x35B].copy_from_slice(b"\0GLIBC_2.2.5\0QUICKLOOK_1.0\0");
        bytes[0x380..0x382].copy_from_slice(&0u16.to_le_bytes());
        bytes[0x382..0x384].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x384..0x386].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x390..0x392].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x392..0x394].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x398..0x39C].copy_from_slice(&16u32.to_le_bytes());
        bytes[0x3A4..0x3A6].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x3A8..0x3AC].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x3C0..0x3C2].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x3C4..0x3C6].copy_from_slice(&1u16.to_le_bytes());
        bytes[0x3CC..0x3D0].copy_from_slice(&20u32.to_le_bytes());
        bytes[0x3D4..0x3D8].copy_from_slice(&13u32.to_le_bytes());
        let mut text = String::new();

        append_elf_summary(&mut text, &bytes);

        assert!(text.contains("GNU versions: .gnu.version 3 entries (2/3)"));
        assert!(text.contains(".gnu.version_r needs GLIBC_2.2.5"));
        assert!(text.contains(".gnu.version_d defines QUICKLOOK_1.0"));
    }
}
