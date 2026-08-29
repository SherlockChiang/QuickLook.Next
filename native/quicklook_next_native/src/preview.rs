//! Native preview providers for Text, Info, Archive, and Folder.
//!
//! These replace the equivalent .NET plugins with pure-Rust implementations callable directly
//! from the App via C ABI, bypassing the .NET plugin pipeline entirely.

use std::fs;
use std::io::{Read, Seek};
use std::path::Path;
use std::time::UNIX_EPOCH;

use image::{DynamicImage, GenericImageView, ImageReader};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

mod animation_probe;
mod archive;
mod bounded;
mod chm;
mod common;
mod database;
mod dump;
mod ebook;
mod elf;
mod executable;
mod folder;
mod font;
mod highlight;
mod image_metadata;
mod mail;
mod media;
mod office;
mod package;
mod text;
mod torrent;
mod types;

pub(crate) use animation_probe::probe_image_animation_reader;
#[cfg(test)]
use animation_probe::ImageAnimationProbe;
use archive::render_zip_archive_from_zip;
pub(crate) use archive::{
    add_parent_folders, discard_archive_extract_path, extract_archive_entry_to_temp,
    extract_archive_entry_to_temp_reader, extract_archive_entry_to_writer_reader, is_archive,
    parent_of, render_archive, render_archive_reader, ArchiveListingEntry, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_EXTRACT_BYTES, MAX_ARCHIVE_SCAN_ENTRIES,
};
use bounded::{
    drain_exact_cancelable, open_validated_zip, prepare_seekable_reader, preview_cancelled,
    read_exact_cancelable, read_file_prefix, read_limited_to_end,
    read_reader_exact_bounded_cancelable, read_reader_prefix, read_reader_prefix_cancelable,
    CancelableSeekReader,
};
use common::{
    format_bytes, format_number, format_timestamp, read_c_string, read_i32_endian, read_u16,
    read_u16_be, read_u16_endian, read_u32, read_u32_be, read_u32_endian, read_u64, type_for_ext,
};
pub(crate) use database::DatabaseCompanionReader;
pub use database::{render_database_info, render_database_reader};
#[cfg(test)]
use ebook::{
    ebook_item_label, extract_xhtml_markdown, parse_epub_opf, parse_epub_rootfile,
    read_ebook_limited_to_end, EbookContext,
};
pub use ebook::{render_ebook, render_ebook_reader};
#[cfg(test)]
use executable::{
    parse_authenticode_certificate_subjects, parse_authenticode_signers, parse_pe_headers,
};
pub use executable::{render_executable, render_executable_reader};
pub(crate) use folder::render_folder;
pub(crate) use highlight::{highlight_spans, HighlightSpan};
pub use image_metadata::render_image_metadata;
pub(crate) use image_metadata::render_image_metadata_reader;
use image_metadata::{
    parse_gif_metadata_from_bytes, parse_png_metadata_from_bytes, parse_webp_metadata_from_bytes,
};
#[cfg(test)]
use image_metadata::{
    parse_jpeg_exif_metadata, parse_jpeg_exif_metadata_from_bytes, parse_tiff_exif_metadata,
};
pub(crate) use mail::render_mail_reader;
use office::{render_docx, render_odf, render_pptx, render_xlsx};
pub(crate) use package::{
    extract_package_icon_bgra, extract_package_icon_bgra_reader, render_package_reader,
};
use package::{is_package_path, render_package};
pub(crate) use text::{is_text, is_text_file, render_text, render_text_reader};
use torrent::parse_bencode;
pub use torrent::{render_torrent, render_torrent_reader};
#[cfg(test)]
use torrent::{MAX_BENCODE_DEPTH, MAX_BENCODE_NODES};
pub(crate) use types::ReaderPreviewError;
use types::{
    to_json, OfficeCellDto, OfficeLayoutDto, OfficeLayoutItemDto, OfficePageDto, PreviewListingDto,
    PreviewListingItemDto, PreviewReadyDto,
};

const MAX_EXECUTABLE_HEADER_BYTES: usize = 4 * 1024 * 1024;
const MAX_EMBEDDED_IMAGE_DIMENSION: u32 = 8192;
const MAX_EMBEDDED_IMAGE_PIXELS: u64 = 16_000_000;
const MAX_INFO_HEADER_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_DATABASE_HANDLE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_SQLITE_WAL_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_SQLITE_SHM_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EBOOK_XML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EBOOK_CHAPTER_BYTES: u64 = 768 * 1024;
const MAX_EBOOK_CHAPTERS: usize = 10;
const MAX_EBOOK_TEXT_CHARS: usize = 140 * 1024;
pub(crate) const MAX_EBOOK_HANDLE_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EBOOK_ZIP_ENTRIES: usize = 8_192;
const MAX_EBOOK_DECOMPRESSED_BYTES: u64 = 16 * 1024 * 1024;
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

// ── Office preview (OOXML / ODF lightweight extraction) ─────────────────────

const MAX_OFFICE_TEXT_CHARS: usize = 96 * 1024;
const MAX_OFFICE_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OFFICE_ZIP_ENTRIES: usize = 8_192;
const MAX_OFFICE_MEDIA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OFFICE_LAYOUT_IMAGES: usize = 18;
const MAX_OFFICE_INLINE_IMAGE_BYTES: u64 = 768 * 1024;
pub(crate) const MAX_OFFICE_LAYOUT_IMAGE_DIMENSION: u32 = 1024;
const OFFICE_EMUS_PER_DIP: f64 = 9525.0;

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
        if event_count.is_multiple_of(256) {
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
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return String::new(),
    };
    render_office_reader(
        file,
        path,
        metadata.len(),
        metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0),
        cancel_cb,
    )
    .unwrap_or_default()
}

pub fn render_office_reader<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_OFFICE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let ext = Path::new(logical_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !matches!(
        ext.as_str(),
        "docx" | "docm" | "xlsx" | "xlsm" | "pptx" | "pptm" | "odt" | "ods" | "odp"
    ) {
        return Ok(render_info(
            logical_name,
            "office",
            i64::try_from(source_len).map_err(|_| ReaderPreviewError::LengthMismatch)?,
            modified_unix,
        ));
    }

    let mut zip = open_validated_zip(reader, source_len, MAX_OFFICE_ZIP_ENTRIES as u64, cancel_cb)?;
    let mut context = OfficeContext::new(cancel_cb);
    let rendered = match ext.as_str() {
        "docx" | "docm" => render_docx(logical_name, &mut zip, &mut context),
        "xlsx" | "xlsm" => render_xlsx(logical_name, &mut zip, &mut context),
        "pptx" | "pptm" => render_pptx(logical_name, &mut zip, &mut context),
        "odt" | "ods" | "odp" => render_odf(logical_name, &mut zip, &mut context),
        _ => unreachable!(),
    };
    match rendered {
        Ok(json) if !json.is_empty() => Ok(json),
        Ok(_) => Err(ReaderPreviewError::Malformed),
        Err(error) => Err(office_reader_error(error)),
    }
}

fn office_reader_error(error: OfficeReadError) -> ReaderPreviewError {
    match error {
        OfficeReadError::Cancelled => ReaderPreviewError::Cancelled,
        OfficeReadError::BudgetExhausted => ReaderPreviewError::LimitExceeded,
    }
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

fn attr_value(e: &BytesStart<'_>, name: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if local_xml_name(attr.key.as_ref()) == name {
            return attr
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned());
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

fn xml_general_ref(bytes: &[u8]) -> String {
    let entity = String::from_utf8_lossy(bytes);
    decode_xml_entity(&entity)
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("&{entity};"))
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
        "font" => return font::render_font_info(path, size, modified_unix),
        "database" => return database::render_database_info(path, size, modified_unix, None),
        "mail" => return mail::render_mail_info(path, size, modified_unix),
        "chm" => return chm::render_chm_info(path, size, modified_unix),
        "dump" => return dump::render_info(path, size, modified_unix),
        "elf" => return elf::render_info(path, size, modified_unix),
        "video" | "audio" | "media" => {
            return media::render_media_info(path, kind, size, modified_unix)
        }
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

fn base_info_text(filename: &str, kind: &str, size: i64, modified_unix: i64) -> String {
    format!(
        "Name: {filename}\nKind: {kind}\nSize: {}\nModified: {}",
        format_number(size),
        format_timestamp(modified_unix)
    )
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

pub fn extract_office_image_bgra(
    path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    office::extract_office_image_bgra(path, cancel_cb)
}

pub fn extract_office_image_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    office::extract_office_image_bgra_reader(reader, source_len, logical_name, cancel_cb)
}

pub(crate) fn office_layout_image_ref_is_valid(logical_name: &str, image_ref: &str) -> bool {
    office::office_layout_image_ref_is_valid(logical_name, image_ref)
}

pub(crate) fn extract_office_layout_image_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    image_ref: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    office::extract_office_layout_image_bgra_reader(
        reader,
        source_len,
        logical_name,
        image_ref,
        target_width,
        target_height,
        cancel_cb,
    )
}

fn is_supported_zip_image_name(lower: &str) -> bool {
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".ico")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
}

fn load_bounded_embedded_image(bytes: &[u8]) -> Option<DynamicImage> {
    let (width, height) = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if width == 0
        || height == 0
        || width > MAX_EMBEDDED_IMAGE_DIMENSION
        || height > MAX_EMBEDDED_IMAGE_DIMENSION
        || u64::from(width).checked_mul(u64::from(height))? > MAX_EMBEDDED_IMAGE_PIXELS
    {
        return None;
    }
    let image = image::load_from_memory(bytes).ok()?;
    if image.dimensions() != (width, height) {
        return None;
    }
    Some(image)
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
    let output_bytes = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(4)?,
    )
    .ok()?;
    let mut bgra = Vec::with_capacity(output_bytes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;

    fn animation_probe_gif(frame_count: usize) -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut encoded, 1, 1, &[]).expect("GIF encoder");
            for index in 0..frame_count {
                let mut rgba = if index % 2 == 0 {
                    vec![255, 0, 0, 255]
                } else {
                    vec![0, 0, 255, 255]
                };
                let mut frame = gif::Frame::from_rgba_speed(1, 1, &mut rgba, 10);
                frame.delay = 2;
                encoder.write_frame(&frame).expect("GIF frame");
            }
        }
        encoded
    }

    #[test]
    fn animation_probe_distinguishes_static_animated_and_unknown_gif() {
        let static_gif = animation_probe_gif(1);
        let animated_gif = animation_probe_gif(2);

        assert_eq!(
            probe_image_animation_reader(
                &mut Cursor::new(&static_gif),
                "static.gif",
                static_gif.len() as u64,
            ),
            Some(ImageAnimationProbe {
                is_animated: Some(false),
            })
        );
        assert_eq!(
            probe_image_animation_reader(
                &mut Cursor::new(&animated_gif),
                "animated.gif",
                animated_gif.len() as u64,
            ),
            Some(ImageAnimationProbe {
                is_animated: Some(true),
            })
        );
        assert_eq!(
            probe_image_animation_reader(
                &mut Cursor::new(&static_gif),
                "bounded.gif",
                static_gif.len() as u64 + 1,
            ),
            Some(ImageAnimationProbe { is_animated: None })
        );
    }

    #[test]
    fn animation_probe_skips_non_animation_extensions_before_reading() {
        let mut reader = Cursor::new(b"not an image".to_vec());
        reader.set_position(5);
        let source_size = reader.get_ref().len() as u64;

        assert_eq!(
            probe_image_animation_reader(&mut reader, "photo.jpg", source_size),
            None
        );
        assert_eq!(reader.position(), 5);
    }

    #[test]
    fn bencode_parser_rejects_excessive_nesting() {
        let mut bytes = vec![b'l'; MAX_BENCODE_DEPTH + 2];
        bytes.extend(std::iter::repeat_n(b'e', MAX_BENCODE_DEPTH + 2));

        assert!(parse_bencode(&bytes, None).is_none());
    }

    #[test]
    fn bencode_parser_rejects_excessive_node_counts() {
        let mut bytes = Vec::with_capacity(MAX_BENCODE_NODES * 2 + 2);
        bytes.push(b'l');
        bytes.extend(std::iter::repeat_n([b'0', b':'], MAX_BENCODE_NODES).flatten());
        bytes.push(b'e');

        assert!(parse_bencode(&bytes, None).is_none());
    }

    #[test]
    fn ebook_reads_share_an_aggregate_decompression_budget() {
        let mut context = EbookContext {
            remaining_decompressed_bytes: 4,
            cancel_cb: None,
        };
        let mut first = Cursor::new(vec![1, 2, 3]);
        let mut second = Cursor::new(vec![4, 5]);

        assert_eq!(
            read_ebook_limited_to_end(&mut context, &mut first, 3).expect("first ebook entry"),
            vec![1, 2, 3]
        );
        assert_eq!(
            read_ebook_limited_to_end(&mut context, &mut second, 2).err(),
            Some(ReaderPreviewError::LimitExceeded)
        );
    }

    #[test]
    fn embedded_image_dimensions_are_bounded_before_pixel_decode() {
        let mut oversized = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut oversized, 10_000, 10_000);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let _writer = encoder.write_header().expect("write oversized PNG header");
        }
        assert!(oversized.len() < 1024);
        assert!(load_bounded_embedded_image(&oversized).is_none());

        let image =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 16, Rgba([20, 40, 60, 255])));
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode bounded PNG");
        assert_eq!(
            load_bounded_embedded_image(png.get_ref()).map(|decoded| decoded.dimensions()),
            Some((32, 16))
        );
    }

    #[test]
    fn fragmented_ebook_xml_retains_bounded_linear_output() {
        let mut xhtml = String::from("<html><body>");
        for _ in 0..60_000 {
            xhtml.push_str("<p>x</p>");
        }
        xhtml.push_str("</body></html>");
        let rendered = extract_xhtml_markdown(&xhtml, "fragmented");
        assert_eq!(rendered.chars().count(), MAX_EBOOK_TEXT_CHARS);

        let large_block = "é".repeat(MAX_EBOOK_TEXT_CHARS + 1);
        let xhtml = format!("<html><body><p>{large_block}</p></body></html>");
        let rendered = extract_xhtml_markdown(&xhtml, "large block");
        assert_eq!(rendered.chars().count(), MAX_EBOOK_TEXT_CHARS);
        assert!(rendered.is_char_boundary(rendered.len()));
    }

    #[test]
    fn xml_unescape_supports_named_and_numeric_entities() {
        assert_eq!(
            xml_unescape_str("A&#65;&#x41;&lt;&gt;&amp;&quot;&apos;&unknown;"),
            "AAA<>&\"'&unknown;"
        );
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

    extern "C" fn always_cancel() -> bool {
        true
    }

    #[test]
    fn office_input_budget_is_below_archive_extract_budget() {
        const {
            assert!(MAX_OFFICE_INPUT_BYTES > MAX_OFFICE_MEDIA_BYTES);
        }
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
        write_ascii_entry(&mut tiff, ifd0_entries, 6, 0x0132, "2025:01:02 03:04:05");

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
        assert!((metadata.altitude.unwrap() + 12.5).abs() < 0.001);
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
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(b"pHYs");
        bytes.extend_from_slice(&3780u32.to_be_bytes());
        bytes.extend_from_slice(&3780u32.to_be_bytes());
        bytes.push(1);
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
        assert!((metadata.horizontal_resolution.unwrap() - 96.012).abs() < 0.001);
        assert!((metadata.vertical_resolution.unwrap() - 96.012).abs() < 0.001);
    }

    #[test]
    fn image_metadata_dispatches_by_magic_instead_of_the_logical_extension() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1A\n");
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let mut reader = std::io::Cursor::new(bytes);

        let json = render_image_metadata_reader(&mut reader, "spoof.jpg", None)
            .expect("magic-dispatched metadata");
        let metadata: serde_json::Value = serde_json::from_str(&json).expect("metadata json");

        assert_eq!(metadata["format"], "PNG");
        assert_eq!(metadata["width"], 2);
        assert_eq!(metadata["height"], 1);
    }

    #[test]
    fn jpeg_metadata_reads_frame_header_without_exif() {
        let bytes = [
            0xFF, 0xD8, // SOI
            0xFF, 0xC0, // baseline SOF
            0x00, 0x11, // segment length
            0x08, // sample precision
            0x00, 0x03, // height
            0x00, 0x02, // width
            0x03, // components
            0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xD9, // EOI
        ];

        let metadata = parse_jpeg_exif_metadata_from_bytes(&bytes).expect("jpeg frame metadata");

        assert_eq!(metadata.format.as_deref(), Some("JPEG"));
        assert_eq!(metadata.width, Some(2));
        assert_eq!(metadata.height, Some(3));
        assert_eq!(metadata.bit_depth, Some(8));
        assert_eq!(metadata.color_type.as_deref(), Some("YCbCr"));
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
        for duration in [40u32, 60u32] {
            bytes.extend_from_slice(b"ANMF");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 12]);
            bytes.extend_from_slice(&duration.to_le_bytes()[..3]);
            bytes.push(0);
        }

        let metadata = parse_webp_metadata_from_bytes(&bytes).expect("webp metadata");

        assert_eq!(metadata.format.as_deref(), Some("WebP"));
        assert_eq!(metadata.width, Some(800));
        assert_eq!(metadata.height, Some(600));
        assert_eq!(metadata.has_alpha, Some(true));
        assert_eq!(metadata.animated, Some(true));
        assert_eq!(metadata.frame_count, Some(2));
        assert_eq!(metadata.duration_ms, Some(100));
    }

    #[test]
    fn webp_lossless_alpha_flag_is_not_inferred_from_the_codec_alone() {
        let make_webp = |alpha: bool| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(b"WEBP");
            bytes.extend_from_slice(b"VP8L");
            bytes.extend_from_slice(&5u32.to_le_bytes());
            bytes.extend_from_slice(&[0x2F, 0, 0, 0, if alpha { 0x10 } else { 0 }]);
            bytes.push(0);
            bytes
        };

        let opaque =
            parse_webp_metadata_from_bytes(&make_webp(false)).expect("opaque lossless webp");
        let alpha = parse_webp_metadata_from_bytes(&make_webp(true)).expect("alpha lossless webp");

        assert_eq!(opaque.has_alpha, Some(false));
        assert_eq!(alpha.has_alpha, Some(true));
    }

    #[test]
    fn partial_gif_metadata_does_not_claim_complete_animation_totals() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.push(0x2C);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&[0, 0x02]);
        while bytes.len() < 1024 * 1024 + 512 {
            bytes.push(255);
            bytes.extend_from_slice(&[0; 255]);
        }
        let mut reader = std::io::Cursor::new(bytes);

        let json = render_image_metadata_reader(&mut reader, "large.gif", None)
            .expect("partial gif metadata");
        let metadata: serde_json::Value = serde_json::from_str(&json).expect("metadata json");

        assert_eq!(metadata["format"], "GIF");
        assert!(metadata["animated"].is_null());
        assert!(metadata["frameCount"].is_null());
        assert!(metadata["durationMs"].is_null());
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
        bytes.push(0x1C);
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes.extend_from_slice(&639u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&479u32.to_le_bytes()[..3]);

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.resize(8 + 2 + 3 * 12 + 4, 0);
        write_le_u16(&mut tiff, 8, 3);
        write_ascii_entry(&mut tiff, 10, 0, 0x010F, "Acme");
        write_rational_entry(&mut tiff, 10, 1, 0x829D, 18, 10);
        write_long_entry(&mut tiff, 10, 2, 0x0100, 999);
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&tiff);
        bytes.extend_from_slice(b"EXIF");
        bytes.extend_from_slice(&(exif.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&exif);
        if exif.len() % 2 == 1 {
            bytes.push(0);
        }

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
        assert_eq!(metadata.make.as_deref(), Some("Acme"));
        assert!((metadata.f_number.unwrap() - 1.8).abs() < 0.001);
    }

    #[test]
    fn tiff_metadata_reads_header_ifd_summary() {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.resize(8 + 2 + 11 * 12 + 4, 0);
        write_le_u16(&mut tiff, 8, 11);
        let entries = 10;
        write_long_entry(&mut tiff, entries, 0, 0x0100, 1024);
        write_long_entry(&mut tiff, entries, 1, 0x0101, 768);
        write_short_entry(&mut tiff, entries, 2, 0x0102, 16);
        write_short_entry(&mut tiff, entries, 3, 0x0103, 5);
        write_short_entry(&mut tiff, entries, 4, 0x0106, 2);
        write_short_entry(&mut tiff, entries, 5, 0x0112, 6);
        write_ascii_entry(&mut tiff, entries, 6, 0x0131, "ScanSoft");
        write_ascii_entry(&mut tiff, entries, 7, 0x0132, "2026:07:08 10:11:12");
        write_rational_entry(&mut tiff, entries, 8, 0x011A, 11811, 100);
        write_rational_entry(&mut tiff, entries, 9, 0x011B, 11811, 100);
        write_short_entry(&mut tiff, entries, 10, 0x0128, 3);

        let metadata = parse_tiff_exif_metadata(&tiff).expect("tiff metadata");

        assert_eq!(metadata.width, Some(1024));
        assert_eq!(metadata.height, Some(768));
        assert_eq!(metadata.bit_depth, Some(16));
        assert_eq!(metadata.compression.as_deref(), Some("LZW"));
        assert_eq!(metadata.photometric_interpretation.as_deref(), Some("RGB"));
        assert!((metadata.horizontal_resolution.unwrap() - 299.9994).abs() < 0.001);
        assert!((metadata.vertical_resolution.unwrap() - 299.9994).abs() < 0.001);
        assert_eq!(metadata.orientation, Some(6));
        assert_eq!(metadata.software.as_deref(), Some("ScanSoft"));
        assert_eq!(metadata.date_time.as_deref(), Some("2026:07:08 10:11:12"));
    }

    #[test]
    fn tiff_resolution_without_an_absolute_unit_is_not_reported_as_dpi() {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.resize(8 + 2 + 3 * 12 + 4, 0);
        write_le_u16(&mut tiff, 8, 3);
        let entries = 10;
        write_rational_entry(&mut tiff, entries, 0, 0x011A, 300, 1);
        write_rational_entry(&mut tiff, entries, 1, 0x011B, 300, 1);
        write_short_entry(&mut tiff, entries, 2, 0x0128, 1);

        let metadata = parse_tiff_exif_metadata(&tiff).expect("tiff metadata");

        assert_eq!(metadata.horizontal_resolution, None);
        assert_eq!(metadata.vertical_resolution, None);
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
        write_byte_entry(tiff, entries, 4, 5, 1);
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

    fn write_byte_entry(tiff: &mut [u8], entries: usize, index: usize, tag: u16, value: u8) {
        let entry = entries + index * 12;
        write_le_u16(tiff, entry, tag);
        write_le_u16(tiff, entry + 2, 1);
        write_le_u32(tiff, entry + 4, 1);
        tiff[entry + 8] = value;
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
    fn ebook_entities_preserve_metadata_and_body_spacing() {
        let opf = parse_epub_opf(
            r#"<package><metadata><dc:title>Rock &amp; Roll</dc:title><dc:creator>A &amp; B</dc:creator></metadata></package>"#,
        );
        assert_eq!(opf.title, "Rock & Roll");
        assert_eq!(opf.creator, "A & B");

        let markdown = extract_xhtml_markdown(
            r#"<html><body><p>Rock &amp; Roll</p></body></html>"#,
            "chapter",
        );
        assert!(markdown.contains("Rock & Roll"));
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
    fn pe_summary_reads_optional_headers_and_sections() {
        fn utf16z(value: &str) -> Vec<u8> {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        }

        fn align_vec(bytes: &mut Vec<u8>) {
            while !bytes.len().is_multiple_of(4) {
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

        let pe = parse_pe_headers(&bytes, None).expect("pe summary");

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
}
