//! Native preview providers for Text, Info, Archive, and Folder.
//!
//! These replace the equivalent .NET plugins with pure-Rust implementations callable directly
//! from the App via C ABI, bypassing the .NET plugin pipeline entirely.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Rgba, RgbaImage};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use tar::Archive as TarArchive;
use zip::ZipArchive;

use crate::rar_listing::{self, RarScanError};

mod animation_probe;
mod chm;
mod common;
mod database;
mod dump;
mod ebook;
mod elf;
mod executable;
mod folder;
mod font;
mod image_metadata;
mod mail;
mod media;
mod office;
mod text;
mod torrent;
mod types;

pub(crate) use animation_probe::probe_image_animation_reader;
#[cfg(test)]
use animation_probe::ImageAnimationProbe;
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
pub use image_metadata::render_image_metadata;
pub(crate) use image_metadata::render_image_metadata_reader;
use image_metadata::{
    parse_gif_metadata_from_bytes, parse_png_metadata_from_bytes, parse_webp_metadata_from_bytes,
};
#[cfg(test)]
use image_metadata::{
    parse_jpeg_exif_metadata, parse_jpeg_exif_metadata_from_bytes, parse_tiff_exif_metadata,
};
use office::{render_docx, render_odf, render_pptx};
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

fn preview_cancelled(cancel_cb: Option<extern "C" fn() -> bool>) -> bool {
    cancel_cb.map(|callback| callback()).unwrap_or(false)
}

const MAX_EXECUTABLE_HEADER_BYTES: usize = 4 * 1024 * 1024;
const MAX_APPX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PACKAGE_ICON_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_PACKAGE_HANDLE_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKAGE_ZIP_ENTRIES: u64 = 100_000;
const MAX_ANDROID_RESOURCE_TABLE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS: usize = 64;
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

fn read_file_prefix(path: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    read_reader_prefix(&mut file, max_bytes)
}

fn read_reader_prefix<R: Read>(reader: &mut R, max_bytes: usize) -> Option<Vec<u8>> {
    let mut reader = reader.take(max_bytes as u64);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    reader.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn read_reader_prefix_cancelable<R: Read>(
    reader: &mut R,
    max_bytes: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut chunk = [0u8; 64 * 1024];
    while bytes.len() < max_bytes {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let remaining = (max_bytes - bytes.len()).min(chunk.len());
        match reader.read(&mut chunk[..remaining]) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Io),
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(bytes)
}

fn read_reader_exact_bounded_cancelable<R: Read>(
    reader: &mut R,
    expected_bytes: u64,
    max_bytes: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut bytes = Vec::with_capacity(expected_bytes.min(64 * 1024) as usize);
    let mut chunk = [0u8; 64 * 1024];
    let read_limit = expected_bytes
        .saturating_add(1)
        .min(max_bytes.saturating_add(1));
    while (bytes.len() as u64) < read_limit {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let remaining = (read_limit - bytes.len() as u64).min(chunk.len() as u64) as usize;
        match reader.read(&mut chunk[..remaining]) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Io),
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    if bytes.len() as u64 != expected_bytes {
        return Err(ReaderPreviewError::LengthMismatch);
    }
    Ok(bytes)
}

fn read_exact_cancelable<R: Read + ?Sized>(
    reader: &mut R,
    bytes: &mut [u8],
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(), ReaderPreviewError> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let end = offset.saturating_add(64 * 1024).min(bytes.len());
        match reader.read(&mut bytes[offset..end]) {
            Ok(0) => return Err(ReaderPreviewError::LengthMismatch),
            Ok(read) => offset += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Io),
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(())
}

fn drain_exact_cancelable<R: Read + ?Sized>(
    reader: &mut R,
    mut length: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(), ReaderPreviewError> {
    let mut buffer = [0u8; 64 * 1024];
    while length > 0 {
        let read_len = length.min(buffer.len() as u64) as usize;
        read_exact_cancelable(reader, &mut buffer[..read_len], cancel_cb)?;
        length -= read_len as u64;
    }
    Ok(())
}

// ── Office preview (OOXML / ODF lightweight extraction) ─────────────────────

const MAX_OFFICE_TEXT_CHARS: usize = 96 * 1024;
const MAX_OFFICE_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OFFICE_ZIP_ENTRIES: usize = 8_192;
const MAX_OFFICE_ROWS: usize = 48;
const MAX_OFFICE_SHEETS: usize = 6;
const MAX_OFFICE_TABLE_CELL_WIDTH: usize = 36;
const MAX_OFFICE_MEDIA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OFFICE_LAYOUT_IMAGES: usize = 18;
const MAX_OFFICE_INLINE_IMAGE_BYTES: u64 = 768 * 1024;
pub(crate) const MAX_OFFICE_LAYOUT_IMAGE_DIMENSION: u32 = 1024;
const OFFICE_MEDIA_ROOTS: &[&str] = &["word/media/", "ppt/media/", "xl/media/"];
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

fn render_xlsx<R: Read + Seek>(
    path: &str,
    zip: &mut ZipArchive<R>,
    context: &mut OfficeContext,
) -> OfficeResult<String> {
    let filename = file_name(path);
    let media_entries = office_media_entries(context, zip, &["xl/media/"])?;
    let shared_strings =
        read_office_zip_text(context, zip, "xl/sharedStrings.xml", 16 * 1024 * 1024)?
            .map(|xml| parse_shared_strings(context, &xml))
            .transpose()?
            .unwrap_or_default();

    let mut sections = Vec::new();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(xml) = read_office_zip_text(context, zip, &name, 16 * 1024 * 1024)? else {
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
    let layout = build_xlsx_layout(context, zip, &shared_strings)?;
    Ok(office_preview_json_with_layout(
        path,
        "Excel workbook",
        text,
        "code",
        "text",
        layout,
    ))
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
    let mut entry_counts = BTreeMap::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > MAX_OFFICE_MEDIA_BYTES {
            continue;
        }

        let Some(normalized) = canonical_office_media_ref(entry.name(), None) else {
            continue;
        };
        let lower = normalized.to_ascii_lowercase();
        if roots.iter().any(|root| lower.starts_with(root)) {
            *entry_counts.entry(normalized).or_insert(0usize) += 1;
        }
    }
    Ok(entry_counts
        .into_iter()
        .filter_map(|(entry, count)| (count == 1).then_some(entry))
        .collect())
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

fn build_xlsx_layout<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
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
            Ok(Event::GeneralRef(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_general_ref(e.as_ref()))
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

fn parse_worksheet_drawing_rid(context: &OfficeContext, xml: &str) -> OfficeResult<Option<String>> {
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
                        OfficeImagePlacement {
                            rel_id: &rel_id,
                            x,
                            y,
                            width,
                            height,
                            z_index: items.len(),
                        },
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

struct OfficeImagePlacement<'a> {
    rel_id: &'a str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    z_index: usize,
}

fn image_item_from_relationship<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    rels: &BTreeMap<String, String>,
    placement: OfficeImagePlacement<'_>,
    image_budget: &mut usize,
) -> OfficeResult<Option<OfficeLayoutItemDto>> {
    let OfficeImagePlacement {
        rel_id,
        x,
        y,
        width,
        height,
        z_index,
    } = placement;
    if rel_id.is_empty() || *image_budget == 0 || width <= 1.0 || height <= 1.0 {
        return Ok(None);
    }
    let Some(target) = rels.get(rel_id) else {
        return Ok(None);
    };
    let path = normalize_zip_target(base_dir, target);
    let Some(expected_root) = office_media_root_for_part(base_dir) else {
        return Ok(None);
    };
    let Some((image_ref, image_byte_length)) =
        read_office_layout_image_reference(context, zip, &path, expected_root)?
    else {
        return Ok(None);
    };
    let lower = image_ref.to_ascii_lowercase();
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
        image_name: Some(
            image_ref
                .rsplit('/')
                .next()
                .unwrap_or(image_ref.as_str())
                .to_string(),
        ),
        mime_type: image_mime_type(&lower).map(str::to_string),
        image_ref: Some(image_ref),
        image_byte_length: Some(image_byte_length),
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

fn parse_xlsx_sheet_metrics(context: &OfficeContext, xml: &str) -> OfficeResult<XlsxSheetMetrics> {
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

fn office_media_root_for_part(part_path: &str) -> Option<&'static str> {
    let lower = part_path.to_ascii_lowercase();
    if lower.starts_with("word/") {
        Some("word/media/")
    } else if lower.starts_with("ppt/") {
        Some("ppt/media/")
    } else if lower.starts_with("xl/") {
        Some("xl/media/")
    } else {
        None
    }
}

fn office_media_root_for_path(path: &str) -> Option<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" => Some("word/media/"),
        "pptx" | "pptm" => Some("ppt/media/"),
        "xlsx" | "xlsm" => Some("xl/media/"),
        _ => None,
    }
}

fn canonical_office_media_ref(path: &str, expected_root: Option<&str>) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return None;
        }
        segments.push(segment);
    }
    let normalized = segments.join("/");
    if normalized != path {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    let root = match expected_root {
        Some(root) if OFFICE_MEDIA_ROOTS.contains(&root) => root,
        Some(_) => return None,
        None => OFFICE_MEDIA_ROOTS
            .iter()
            .copied()
            .find(|root| lower.starts_with(root))?,
    };
    if !lower.starts_with(root) || lower.len() <= root.len() || !is_supported_zip_image_name(&lower)
    {
        return None;
    }
    Some(normalized)
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
            Ok(Event::GeneralRef(e)) if in_t => current.push_str(&xml_general_ref(e.as_ref())),
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
                                    .normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                            } else if key == "r" {
                                let reference = attr
                                    .normalized_value(XmlVersion::Implicit1_0)
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
            Ok(Event::GeneralRef(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_general_ref(e.as_ref()))
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
        for (i, width) in widths.iter_mut().enumerate() {
            let value = row
                .get(i)
                .map(|cell| clean_table_cell(cell))
                .unwrap_or_default();
            let len = value.chars().count().min(MAX_OFFICE_TABLE_CELL_WIDTH);
            *width = (*width).max(len);
        }
    }

    rows.iter()
        .map(|row| {
            let mut parts = Vec::with_capacity(col_count);
            for (i, width) in widths.iter().copied().enumerate() {
                let value = row
                    .get(i)
                    .map(|cell| clean_table_cell(cell))
                    .unwrap_or_default();
                let cell = truncate_table_cell(&value, width);
                if i + 1 == col_count {
                    parts.push(cell);
                } else {
                    parts.push(format!("{cell:<width$}"));
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

// ── Archive preview ──────────────────────────────────────────────────────────

const MAX_ARCHIVE_ENTRIES: usize = 5000;
const MAX_ARCHIVE_SCAN_ENTRIES: usize = 10_000;
const MAX_RAR_RETAINED_PATH_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_HANDLE_INPUT_BYTES: u64 = 16 * 1024 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ZIP_ENTRIES: u64 = 100_000;
const MAX_ZIP_CENTRAL_DIRECTORY_BYTES: u64 = 32 * 1024 * 1024;
const ZIP_EOCD_MIN_BYTES: u64 = 22;
const ZIP_EOCD_MAX_TAIL_BYTES: u64 = ZIP_EOCD_MIN_BYTES + u16::MAX as u64;
const MAX_TAR_SCAN_BYTES: u64 = 512 * 1024 * 1024;
const TAR_SCAN_DEADLINE: Duration = Duration::from_secs(4);
pub(crate) const MAX_ARCHIVE_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACT_RATIO: u64 = 1000;
const ARCHIVE_EXTRACT_DEADLINE: Duration = Duration::from_secs(4);
const MAX_ARCHIVE_EXTRACT_ROOTS: usize = 32;
const ARCHIVE_EXTRACT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
type ArchiveListingEntry = (String, String, bool, i64, i64, i64, bool);

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
const RAR_EXTS: &[&str] = &[".rar"];

fn prepare_seekable_reader<R: Seek>(
    reader: &mut R,
    expected_length: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(), ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let actual_length = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    if actual_length != expected_length {
        return Err(ReaderPreviewError::LengthMismatch);
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(())
}

fn validate_zip_container<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
    max_entries: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(), ReaderPreviewError> {
    if source_len < ZIP_EOCD_MIN_BYTES {
        return Err(ReaderPreviewError::Malformed);
    }
    prepare_seekable_reader(reader, source_len, cancel_cb)?;
    let tail_len = source_len.min(ZIP_EOCD_MAX_TAIL_BYTES);
    reader
        .seek(SeekFrom::Start(source_len - tail_len))
        .map_err(|_| ReaderPreviewError::Io)?;
    let mut tail = vec![0u8; tail_len as usize];
    read_exact_cancelable(reader, &mut tail, cancel_cb)?;

    let eocd_index = (0..=tail.len().saturating_sub(ZIP_EOCD_MIN_BYTES as usize))
        .rev()
        .find(|index| {
            tail.get(*index..index + 4) == Some(b"PK\x05\x06")
                && read_u16(&tail, index + 20)
                    .is_some_and(|comment_len| index + 22 + comment_len as usize == tail.len())
        })
        .ok_or(ReaderPreviewError::Malformed)?;
    let eocd_offset = source_len - tail_len + eocd_index as u64;
    let disk = read_u16(&tail, eocd_index + 4).ok_or(ReaderPreviewError::Malformed)?;
    let central_disk = read_u16(&tail, eocd_index + 6).ok_or(ReaderPreviewError::Malformed)?;
    let entries_on_disk = read_u16(&tail, eocd_index + 8).ok_or(ReaderPreviewError::Malformed)?;
    let entries = read_u16(&tail, eocd_index + 10).ok_or(ReaderPreviewError::Malformed)?;
    let central_size = read_u32(&tail, eocd_index + 12).ok_or(ReaderPreviewError::Malformed)?;
    let central_offset = read_u32(&tail, eocd_index + 16).ok_or(ReaderPreviewError::Malformed)?;
    if disk != 0 || central_disk != 0 || entries_on_disk != entries {
        return Err(ReaderPreviewError::Malformed);
    }

    let is_zip64 = entries == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX;
    let (entries, central_size, central_offset, central_end_limit) = if is_zip64 {
        let locator_offset = eocd_offset
            .checked_sub(20)
            .ok_or(ReaderPreviewError::Malformed)?;
        reader
            .seek(SeekFrom::Start(locator_offset))
            .map_err(|_| ReaderPreviewError::Io)?;
        let mut locator = [0u8; 20];
        read_exact_cancelable(reader, &mut locator, cancel_cb)?;
        if locator.get(..4) != Some(b"PK\x06\x07")
            || read_u32(&locator, 4) != Some(0)
            || read_u32(&locator, 16) != Some(1)
        {
            return Err(ReaderPreviewError::Malformed);
        }
        let zip64_offset = read_u64(&locator, 8).ok_or(ReaderPreviewError::Malformed)?;
        if zip64_offset >= locator_offset {
            return Err(ReaderPreviewError::Malformed);
        }
        reader
            .seek(SeekFrom::Start(zip64_offset))
            .map_err(|_| ReaderPreviewError::Io)?;
        let mut zip64 = [0u8; 56];
        read_exact_cancelable(reader, &mut zip64, cancel_cb)?;
        if zip64.get(..4) != Some(b"PK\x06\x06")
            || read_u64(&zip64, 4).is_none_or(|size| size < 44)
            || read_u32(&zip64, 16) != Some(0)
            || read_u32(&zip64, 20) != Some(0)
        {
            return Err(ReaderPreviewError::Malformed);
        }
        let entries_on_disk = read_u64(&zip64, 24).ok_or(ReaderPreviewError::Malformed)?;
        let entries = read_u64(&zip64, 32).ok_or(ReaderPreviewError::Malformed)?;
        if entries_on_disk != entries {
            return Err(ReaderPreviewError::Malformed);
        }
        (
            entries,
            read_u64(&zip64, 40).ok_or(ReaderPreviewError::Malformed)?,
            read_u64(&zip64, 48).ok_or(ReaderPreviewError::Malformed)?,
            zip64_offset,
        )
    } else {
        (
            entries as u64,
            central_size as u64,
            central_offset as u64,
            eocd_offset,
        )
    };

    if entries > max_entries || central_size > MAX_ZIP_CENTRAL_DIRECTORY_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or(ReaderPreviewError::Malformed)?;
    if central_end > central_end_limit || central_end > source_len {
        return Err(ReaderPreviewError::Malformed);
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    Ok(())
}

struct CancelableSeekReader<R> {
    reader: R,
    cancel_cb: Option<extern "C" fn() -> bool>,
}

impl<R> CancelableSeekReader<R> {
    fn new(reader: R, cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
        Self { reader, cancel_cb }
    }

    fn cancelled_error() -> io::Error {
        io::Error::other("preview cancelled")
    }
}

impl<R: Read> Read for CancelableSeekReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if preview_cancelled(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        let read = self.reader.read(buffer)?;
        if preview_cancelled(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        Ok(read)
    }
}

impl<R: Seek> Seek for CancelableSeekReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if preview_cancelled(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        let offset = self.reader.seek(position)?;
        if preview_cancelled(self.cancel_cb) {
            return Err(Self::cancelled_error());
        }
        Ok(offset)
    }
}

fn open_validated_zip<R: Read + Seek>(
    mut reader: R,
    source_len: u64,
    max_entries: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<ZipArchive<CancelableSeekReader<R>>, ReaderPreviewError> {
    validate_zip_container(&mut reader, source_len, max_entries, cancel_cb)?;
    let zip = ZipArchive::new(CancelableSeekReader::new(reader, cancel_cb)).map_err(|_| {
        if preview_cancelled(cancel_cb) {
            ReaderPreviewError::Cancelled
        } else {
            ReaderPreviewError::Malformed
        }
    })?;
    // The ZIP crate can reject one EOCD candidate and fall back to an earlier one. Recheck its
    // authoritative result so that fallback selection cannot escape the declared-entry budget.
    if zip.len() as u64 > max_entries {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    // Validate the directory selected by the ZIP crate, including fallback to an earlier EOCD.
    const MAX_ZIP_DIRECTORY_TAIL_BYTES: u64 =
        MAX_ZIP_CENTRAL_DIRECTORY_BYTES + ZIP_EOCD_MAX_TAIL_BYTES + 76;
    let authoritative_tail = source_len
        .checked_sub(zip.central_directory_start())
        .ok_or(ReaderPreviewError::Malformed)?;
    if authoritative_tail > MAX_ZIP_DIRECTORY_TAIL_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    Ok(zip)
}

pub fn is_archive(ext: &str, kind: &str, magic: &[u8]) -> bool {
    if rar_listing::is_rar_magic(magic) {
        return true;
    }
    if RAR_EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
        // Unlike the ZIP family, RAR is routed only after a complete RAR4/RAR5 signature check.
        // This keeps renamed binaries out of the native header scanner.
        return false;
    }
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

fn reader_starts_with_rar_magic<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<bool, ReaderPreviewError> {
    prepare_seekable_reader(reader, source_len, cancel_cb)?;
    let prefix_len = source_len.min(rar_listing::RAR5_SIGNATURE.len() as u64) as usize;
    let mut prefix = [0_u8; 8];
    read_exact_cancelable(reader, &mut prefix[..prefix_len], cancel_cb)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| ReaderPreviewError::Io)?;
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(rar_listing::is_rar_magic(&prefix[..prefix_len]))
}

fn render_rar_entries<R: Read + Seek>(
    reader: &mut R,
    logical_name: &str,
    root_path: &str,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let mut cancelable = CancelableSeekReader::new(reader, cancel_cb);
    let listing =
        rar_listing::scan_rar(&mut cancelable, source_len, || preview_cancelled(cancel_cb))
            .map_err(|error| match error {
                RarScanError::Cancelled => ReaderPreviewError::Cancelled,
                RarScanError::Io(_) if preview_cancelled(cancel_cb) => {
                    ReaderPreviewError::Cancelled
                }
                RarScanError::Io(_) => ReaderPreviewError::Io,
                RarScanError::HeaderTooLarge | RarScanError::SizeOverflow => {
                    ReaderPreviewError::LimitExceeded
                }
                RarScanError::InvalidMagic
                | RarScanError::Truncated
                | RarScanError::Malformed(_)
                | RarScanError::HeaderCrcMismatch => ReaderPreviewError::Malformed,
            })?;

    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let file_count = listing.total_file_count;
    let uncompressed = listing.total_unpacked.min(i64::MAX as u64) as i64;
    let compressed = listing.total_packed.min(i64::MAX as u64) as i64;
    let encrypted_file_count = listing.encrypted_file_count;
    let mut partial = listing.is_partial;
    let mut entries: BTreeMap<String, ArchiveListingEntry> = BTreeMap::new();
    let mut retained_path_bytes = 0_usize;

    for entry in listing.entries {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let full_name = entry.path.trim_start_matches('/').to_string();
        if full_name.is_empty() {
            continue;
        }
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            partial = true;
            continue;
        }

        if !add_rar_parent_folders(&full_name, &mut entries, &mut retained_path_bytes) {
            partial = true;
            continue;
        }

        if entry.is_folder {
            let path = ensure_trailing_slash(&full_name);
            if entries.contains_key(&path) {
                continue;
            }
            let name = path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let parent = parent_of(&path);
            let retained = path
                .len()
                .saturating_add(name.len())
                .saturating_add(parent.len());
            if retained_path_bytes
                .checked_add(retained)
                .is_none_or(|total| total > MAX_RAR_RETAINED_PATH_BYTES)
            {
                partial = true;
                continue;
            }
            retained_path_bytes += retained;
            entries.insert(
                path,
                (
                    name,
                    parent,
                    true,
                    0,
                    0,
                    entry.modified_unix,
                    entry.is_encrypted,
                ),
            );
        } else {
            if entries.contains_key(&full_name) {
                partial = true;
                continue;
            }
            let name = full_name
                .rsplit('/')
                .next()
                .unwrap_or(&full_name)
                .to_string();
            let parent = parent_of(&full_name);
            let retained = full_name
                .len()
                .saturating_add(name.len())
                .saturating_add(parent.len());
            if retained_path_bytes
                .checked_add(retained)
                .is_none_or(|total| total > MAX_RAR_RETAINED_PATH_BYTES)
            {
                partial = true;
                continue;
            }
            retained_path_bytes += retained;
            entries.insert(
                full_name,
                (
                    name,
                    parent,
                    false,
                    entry.unpacked_size.min(i64::MAX as u64) as i64,
                    entry.packed_size.min(i64::MAX as u64) as i64,
                    entry.modified_unix,
                    entry.is_encrypted,
                ),
            );
        }
    }

    Ok(archive_listing_json(
        filename,
        root_path,
        "archive",
        entries,
        ArchiveListingStats {
            file_count,
            uncompressed,
            compressed,
            partial,
            encrypted_file_count,
            can_preview_entries: false,
        },
    ))
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
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return String::new(),
    };
    let modified_unix = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);
    render_archive_reader_with_root(file, path, path, metadata.len(), modified_unix, cancel_cb)
        .unwrap_or_default()
}

pub fn render_archive_reader<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    render_archive_reader_with_root(
        reader,
        logical_name,
        "",
        source_len,
        modified_unix,
        cancel_cb,
    )
}

fn render_archive_reader_with_root<R: Read + Seek>(
    mut reader: R,
    logical_name: &str,
    root_path: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_ARCHIVE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if is_package_path(&lower) {
        return Err(ReaderPreviewError::Malformed);
    }

    let is_rar = reader_starts_with_rar_magic(&mut reader, source_len, cancel_cb)?;
    let json = if is_rar {
        render_rar_entries(&mut reader, logical_name, root_path, source_len, cancel_cb)?
    } else if TAR_GZ_EXTS
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        prepare_seekable_reader(&mut reader, source_len, cancel_cb)?;
        render_tar_entries(
            logical_name,
            root_path,
            "archive",
            GzDecoder::new(reader),
            cancel_cb,
        )
    } else if TAR_EXTS.iter().any(|extension| lower.ends_with(extension)) {
        prepare_seekable_reader(&mut reader, source_len, cancel_cb)?;
        render_tar_entries(logical_name, root_path, "archive", reader, cancel_cb)
    } else if GZ_EXTS.iter().any(|extension| lower.ends_with(extension))
        && !lower.ends_with(".tar.gz")
    {
        prepare_seekable_reader(&mut reader, source_len, cancel_cb)?;
        render_gzip_member_reader(
            &mut reader,
            logical_name,
            root_path,
            source_len,
            modified_unix,
            cancel_cb,
        )?
    } else {
        let mut zip = open_validated_zip(reader, source_len, MAX_ARCHIVE_ZIP_ENTRIES, cancel_cb)?;
        render_zip_archive_from_zip(&mut zip, logical_name, root_path, cancel_cb)?
    };

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    if json.is_empty() {
        Err(ReaderPreviewError::Malformed)
    } else {
        Ok(json)
    }
}

pub fn extract_archive_entry_to_temp(
    archive_path: &str,
    entry_path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<String> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let file = fs::File::open(archive_path).ok()?;
    let source_len = file.metadata().ok()?.len();
    extract_archive_entry_to_temp_reader(file, source_len, archive_path, entry_path, cancel_cb).ok()
}

pub fn extract_archive_entry_to_temp_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    entry_path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let normalized =
        normalize_archive_entry_path(entry_path).ok_or(ReaderPreviewError::Malformed)?;
    let root = create_archive_extract_root().ok_or(ReaderPreviewError::Io)?;
    let target = root.join(archive_extract_output_name(&normalized));
    let result = (|| {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|_| ReaderPreviewError::Io)?;
        extract_archive_entry_to_writer_reader(
            reader,
            source_len,
            logical_name,
            &normalized,
            &mut output,
            MAX_ARCHIVE_EXTRACT_BYTES,
            cancel_cb,
        )?;
        target
            .to_str()
            .map(str::to_string)
            .ok_or(ReaderPreviewError::Io)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

/// Stream one bounded ZIP entry into a caller-provided writer.
///
/// The destination is not path-derived and receives no bytes beyond `output_capacity`. A failed or
/// cancelled call may leave a partial prefix in the caller's object; the caller must discard it.
pub fn extract_archive_entry_to_writer_reader<R: Read + Seek, W: Write>(
    mut reader: R,
    source_len: u64,
    logical_name: &str,
    entry_path: &str,
    output: &mut W,
    output_capacity: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<u64, ReaderPreviewError> {
    if source_len > MAX_ARCHIVE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if output_capacity > MAX_ARCHIVE_EXTRACT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    let is_rar = reader_starts_with_rar_magic(&mut reader, source_len, cancel_cb)?;
    if is_rar
        || RAR_EXTS.iter().any(|extension| lower.ends_with(extension))
        || TAR_EXTS.iter().any(|extension| lower.ends_with(extension))
        || TAR_GZ_EXTS
            .iter()
            .any(|extension| lower.ends_with(extension))
        || (GZ_EXTS.iter().any(|extension| lower.ends_with(extension))
            && !lower.ends_with(".tar.gz"))
    {
        return Err(ReaderPreviewError::Malformed);
    }

    let normalized =
        normalize_archive_entry_path(entry_path).ok_or(ReaderPreviewError::Malformed)?;
    let mut zip = open_validated_zip(reader, source_len, MAX_ARCHIVE_ZIP_ENTRIES, cancel_cb)?;
    let mut found_index = None;
    for index in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let entry = match zip.by_index_raw(index) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if normalize_archive_entry_path(entry.name()).as_deref() == Some(normalized.as_str()) {
            if entry.is_dir() || entry.encrypted() {
                return Err(ReaderPreviewError::Malformed);
            }
            found_index = Some(index);
            break;
        }
    }

    let mut entry = zip
        .by_index(found_index.ok_or(ReaderPreviewError::Malformed)?)
        .map_err(|_| {
            if preview_cancelled(cancel_cb) {
                ReaderPreviewError::Cancelled
            } else {
                ReaderPreviewError::Malformed
            }
        })?;
    if entry.is_dir()
        || entry.encrypted()
        || !archive_entry_within_extract_budget(entry.size(), entry.compressed_size())
        || entry.size() > output_capacity
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }

    let started = Instant::now();
    let mut written = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        if started.elapsed() > ARCHIVE_EXTRACT_DEADLINE {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        let read = match entry.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Malformed),
        };
        if read == 0 {
            break;
        }
        let Some(next_written) = written.checked_add(read as u64) else {
            return Err(ReaderPreviewError::LimitExceeded);
        };
        if next_written > output_capacity || next_written > MAX_ARCHIVE_EXTRACT_BYTES {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| ReaderPreviewError::Io)?;
        written = next_written;
    }
    drop(entry);
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    output.flush().map_err(|_| ReaderPreviewError::Io)?;
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(written)
}

pub(crate) fn discard_archive_extract_path(path: &str) {
    let target = Path::new(path);
    let Some(file_name) = target.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if !file_name.starts_with("entry-") {
        return;
    }
    let Some(root) = target.parent() else {
        return;
    };
    let Some(root_name) = root.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let random_suffix = root_name.strip_prefix("extract-").unwrap_or("");
    if random_suffix.len() != 32
        || !random_suffix
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || root.parent() != Some(archive_extract_base_path().as_path())
    {
        return;
    }
    let _ = fs::remove_dir_all(root);
}

fn archive_entry_within_extract_budget(size: u64, compressed_size: u64) -> bool {
    size <= MAX_ARCHIVE_EXTRACT_BYTES
        && compressed_size <= MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES
        && (size == 0
            || (compressed_size > 0
                && size <= compressed_size.saturating_mul(MAX_ARCHIVE_EXTRACT_RATIO)))
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

fn read_office_layout_image_reference<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    requested_ref: &str,
    expected_root: &str,
) -> OfficeResult<Option<(String, u64)>> {
    let Some(requested_ref) = canonical_office_media_ref(requested_ref, Some(expected_root)) else {
        return Ok(None);
    };
    let mut exact_match: Option<(usize, String, u64)> = None;
    let mut exact_ambiguous = false;
    let mut folded_match: Option<(usize, String, u64)> = None;
    let mut folded_ambiguous = false;

    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let Some(actual_ref) = canonical_office_media_ref(entry.name(), Some(expected_root)) else {
            continue;
        };
        if actual_ref == requested_ref {
            if exact_match.is_some() {
                exact_ambiguous = true;
            } else {
                exact_match = Some((i, actual_ref, entry.size()));
            }
        } else if actual_ref.eq_ignore_ascii_case(&requested_ref) {
            if folded_match.is_some() {
                folded_ambiguous = true;
            } else {
                folded_match = Some((i, actual_ref, entry.size()));
            }
        }
    }

    let selected = if exact_ambiguous {
        None
    } else if exact_match.is_some() {
        exact_match
    } else if folded_ambiguous {
        None
    } else {
        folded_match
    };
    let Some((index, image_ref, declared_length)) = selected else {
        return Ok(None);
    };
    if declared_length == 0 || declared_length > MAX_OFFICE_INLINE_IMAGE_BYTES {
        return Ok(None);
    }
    let mut entry = match zip.by_index(index) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let Some(bytes) =
        read_office_limited_to_end(context, &mut entry, MAX_OFFICE_INLINE_IMAGE_BYTES)?
    else {
        return Ok(None);
    };
    if bytes.is_empty() || bytes.len() as u64 != declared_length {
        return Ok(None);
    }
    Ok(Some((image_ref, bytes.len() as u64)))
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
    if let Some(root) = std::env::var_os("QUICKLOOK_NEXT_ARCHIVE_ROOT") {
        return PathBuf::from(root);
    }
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
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let source_len = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => return String::new(),
    };
    render_package_reader(file, path, source_len, cancel_cb).unwrap_or_default()
}

pub fn render_package_reader<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_PACKAGE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if !is_package_path(&lower) {
        return Err(ReaderPreviewError::Malformed);
    }
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let platform = if lower.ends_with(".apk") || lower.ends_with(".apks") || lower.ends_with(".aab")
    {
        "Android"
    } else {
        "Windows"
    };

    let mut zip = open_validated_zip(reader, source_len, MAX_PACKAGE_ZIP_ENTRIES, cancel_cb)?;

    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut uncompressed = 0i64;
    let mut compressed = 0i64;
    let mut has_icon = false;
    let mut has_manifest = false;
    let mut dex_count = 0u64;
    let mut certificate_count = 0u64;
    let mut native_abis = BTreeMap::<String, ()>::new();
    let mut appx_manifest_name: Option<String> = None;

    let partial = zip.len() > MAX_ARCHIVE_SCAN_ENTRIES;
    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let entry = match zip.by_index_raw(i) {
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
            appx_manifest_name = Some(entry.name().to_string());
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

    let appx_manifest = appx_manifest_name
        .as_deref()
        .map(|name| read_package_zip_bytes(&mut zip, name, MAX_APPX_MANIFEST_BYTES, cancel_cb))
        .transpose()?
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string());

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

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(to_json(&PreviewReadyDto {
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
    }))
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
                        .normalized_value(XmlVersion::Implicit1_0)
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

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values.into_iter().flatten().find(|v| !v.trim().is_empty())
}

pub fn extract_office_image_bgra(
    path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let source_len = file.metadata().ok()?.len();
    extract_office_image_bgra_reader(file, source_len, path, cancel_cb).ok()
}

pub fn extract_office_image_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if source_len > MAX_OFFICE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let mut zip = open_validated_zip(reader, source_len, MAX_OFFICE_ZIP_ENTRIES as u64, cancel_cb)?;
    let roots = office_media_roots_for_path(logical_name);
    if roots.is_empty() {
        return Err(ReaderPreviewError::Malformed);
    }

    let mut candidates = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
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
    let mut context = OfficeContext::new(cancel_cb);
    for (path_score, name) in candidates.into_iter().take(24) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let bytes =
            match read_office_zip_bytes(&mut context, &mut zip, &name, MAX_OFFICE_MEDIA_BYTES) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(error) => return Err(office_reader_error(error)),
            };
        let Some(image) = load_bounded_embedded_image(&bytes) else {
            continue;
        };
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
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
        .ok_or(ReaderPreviewError::Malformed)
}

pub(crate) fn office_layout_image_ref_is_valid(logical_name: &str, image_ref: &str) -> bool {
    let Some(expected_root) = office_media_root_for_path(logical_name) else {
        return false;
    };
    canonical_office_media_ref(image_ref, Some(expected_root))
        .is_some_and(|normalized| normalized == image_ref)
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
    if source_len > MAX_OFFICE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if target_width == 0
        || target_height == 0
        || target_width > MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
        || target_height > MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let Some(expected_root) = office_media_root_for_path(logical_name) else {
        return Err(ReaderPreviewError::Malformed);
    };
    let Some(canonical_ref) = canonical_office_media_ref(image_ref, Some(expected_root)) else {
        return Err(ReaderPreviewError::Malformed);
    };
    if canonical_ref != image_ref {
        return Err(ReaderPreviewError::Malformed);
    }
    let Some(required_format) = office_image_format(&canonical_ref) else {
        return Err(ReaderPreviewError::Malformed);
    };

    let mut zip = open_validated_zip(reader, source_len, MAX_OFFICE_ZIP_ENTRIES as u64, cancel_cb)?;
    let mut selected: Option<(usize, u64)> = None;
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.name() != canonical_ref {
            continue;
        }
        if canonical_office_media_ref(entry.name(), Some(expected_root)).as_deref()
            != Some(canonical_ref.as_str())
            || selected.is_some()
        {
            return Err(ReaderPreviewError::Malformed);
        }
        selected = Some((i, entry.size()));
    }
    let Some((index, declared_length)) = selected else {
        return Err(ReaderPreviewError::Malformed);
    };
    if declared_length == 0 || declared_length > MAX_OFFICE_INLINE_IMAGE_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }

    let mut context = OfficeContext::new(cancel_cb);
    let mut entry = zip
        .by_index(index)
        .map_err(|_| ReaderPreviewError::Malformed)?;
    let bytes = read_office_limited_to_end(&mut context, &mut entry, MAX_OFFICE_INLINE_IMAGE_BYTES)
        .map_err(office_reader_error)?
        .ok_or(ReaderPreviewError::LimitExceeded)?;
    if bytes.is_empty() || bytes.len() as u64 != declared_length {
        return Err(ReaderPreviewError::Malformed);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let image_reader = ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if image_reader.format() != Some(required_format) {
        return Err(ReaderPreviewError::Malformed);
    }
    let (width, height) = image_reader
        .into_dimensions()
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if width == 0
        || height == 0
        || width > MAX_EMBEDDED_IMAGE_DIMENSION
        || height > MAX_EMBEDDED_IMAGE_DIMENSION
        || u64::from(width)
            .checked_mul(u64::from(height))
            .is_none_or(|pixels| pixels > MAX_EMBEDDED_IMAGE_PIXELS)
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let image = image::load_from_memory_with_format(&bytes, required_format)
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if image.dimensions() != (width, height) {
        return Err(ReaderPreviewError::Malformed);
    }
    office_layout_image_to_bgra(image, target_width, target_height, cancel_cb)
}

fn office_image_format(image_ref: &str) -> Option<ImageFormat> {
    let lower = image_ref.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some(ImageFormat::Png)
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some(ImageFormat::Jpeg)
    } else if lower.ends_with(".ico") {
        Some(ImageFormat::Ico)
    } else if lower.ends_with(".webp") {
        Some(ImageFormat::WebP)
    } else if lower.ends_with(".bmp") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn office_layout_image_to_bgra(
    image: DynamicImage,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        return Err(ReaderPreviewError::Malformed);
    }
    let scale = (target_width as f64 / original_width as f64)
        .min(target_height as f64 / original_height as f64)
        .min(1.0);
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let raster = if (width, height) == (original_width, original_height) {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let rgba = raster.to_rgba8();
    let output_length = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ReaderPreviewError::LimitExceeded)?,
    )
    .map_err(|_| ReaderPreviewError::LimitExceeded)?;
    let mut bgra = Vec::with_capacity(output_length);
    for (index, pixel) in rgba.chunks_exact(4).enumerate() {
        if index % 65_536 == 0 && preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let red = pixel[0] as u32;
        let green = pixel[1] as u32;
        let blue = pixel[2] as u32;
        let alpha = pixel[3] as u32;
        bgra.push(((blue * alpha + 127) / 255) as u8);
        bgra.push(((green * alpha + 127) / 255) as u8);
        bgra.push(((red * alpha + 127) / 255) as u8);
        bgra.push(alpha as u8);
    }
    if bgra.len() != output_length {
        return Err(ReaderPreviewError::Malformed);
    }
    Ok((width, height, bgra))
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

pub fn extract_package_icon_bgra(
    path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let source_len = file.metadata().ok()?.len();
    extract_package_icon_bgra_reader(file, source_len, path, cancel_cb).ok()
}

pub fn extract_package_icon_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if source_len > MAX_PACKAGE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if !is_package_path(&lower) {
        return Err(ReaderPreviewError::Malformed);
    }
    let mut zip = open_validated_zip(reader, source_len, MAX_PACKAGE_ZIP_ENTRIES, cancel_cb)?;
    if lower.ends_with(".apk") {
        if let Some(icon) = extract_android_package_icon(&mut zip, cancel_cb) {
            return Ok(icon);
        }
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
    }
    let mut candidates = Vec::new();
    let manifest_icons = read_zip_text(&mut zip, "AppxManifest.xml", MAX_APPX_MANIFEST_BYTES)
        .as_deref()
        .and_then(parse_appx_manifest_summary)
        .map(|summary| expand_appx_icon_candidates(&summary.icon_paths))
        .unwrap_or_default();

    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let score = package_icon_candidate_score(&normalized_name)
            + manifest_icon_candidate_score(&normalized_name, &manifest_icons);
        if score > 0 && entry.size() <= MAX_PACKAGE_ICON_BYTES {
            candidates.push((score, raw_name));
            if candidates.len() >= 256 {
                break;
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    for (path_score, name) in candidates.into_iter().take(32) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(mut entry) = zip.by_name(&name) else {
            continue;
        };
        let entry_size = entry.size();
        let bytes = match read_reader_exact_bounded_cancelable(
            &mut entry,
            entry_size,
            MAX_PACKAGE_ICON_BYTES,
            cancel_cb,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                if preview_cancelled(cancel_cb) {
                    return Err(package_zip_read_error(error));
                }
                continue;
            }
        };
        let Some(image) = load_bounded_embedded_image(&bytes) else {
            continue;
        };
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let (original_width, original_height) = image.dimensions();
        if original_width < 16 || original_height < 16 {
            continue;
        }
        let area_score = ((original_width.min(512) * original_height.min(512)) / 256) as i32;
        let score = path_score + area_score;
        let Some((width, height, bgra)) = image_to_bgra(image, 512) else {
            continue;
        };
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, width, height, bgra));
        }
    }

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    best.map(|(_, width, height, bgra)| (width, height, bgra))
        .ok_or(ReaderPreviewError::Malformed)
}

fn extract_android_package_icon<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    let manifest = read_zip_bytes(zip, "AndroidManifest.xml", MAX_PACKAGE_ICON_BYTES)?;
    let resources = read_zip_bytes(zip, "resources.arsc", MAX_ANDROID_RESOURCE_TABLE_BYTES);
    let manifest = decode_android_xml(&manifest, resources.as_deref())?;
    let icon_ref = android_manifest_icon_reference(&manifest)?;
    let mut decode_attempts = 0usize;
    let image = load_android_resource_image(
        zip,
        resources.as_deref(),
        &icon_ref,
        0,
        &mut decode_attempts,
        cancel_cb,
    )?;
    image_to_bgra(image, 512)
}

fn read_zip_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    read_limited_to_end(&mut entry, limit)
}

fn read_package_zip_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut entry = zip.by_name(name).map_err(|_| {
        if preview_cancelled(cancel_cb) {
            ReaderPreviewError::Cancelled
        } else {
            ReaderPreviewError::Malformed
        }
    })?;
    let entry_size = entry.size();
    read_reader_exact_bounded_cancelable(&mut entry, entry_size, limit, cancel_cb)
        .map_err(package_zip_read_error)
}

fn package_zip_read_error(error: ReaderPreviewError) -> ReaderPreviewError {
    match error {
        ReaderPreviewError::Cancelled | ReaderPreviewError::LimitExceeded => error,
        ReaderPreviewError::Io
        | ReaderPreviewError::Malformed
        | ReaderPreviewError::LengthMismatch => ReaderPreviewError::Malformed,
    }
}

fn decode_android_xml(bytes: &[u8], resources: Option<&[u8]>) -> Option<String> {
    if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'<') {
        return std::str::from_utf8(bytes).ok().map(str::to_owned);
    }
    decode_android_binary_xml(bytes, resources)
}

fn decode_android_binary_xml(bytes: &[u8], resources: Option<&[u8]>) -> Option<String> {
    let (document_type, header_size, _) = android_chunk_header(bytes, 0)?;
    if document_type != 0x0003 {
        return None;
    }
    let string_pool_offset = android_find_chunk(bytes, header_size, 0x0001)?;
    let strings = android_string_pool(bytes, string_pool_offset)?;
    let (_, _, string_pool_size) = android_chunk_header(bytes, string_pool_offset)?;
    let mut offset = string_pool_offset + string_pool_size;
    let mut xml = String::new();
    while offset < bytes.len() {
        let (chunk_type, chunk_header, chunk_size) = android_chunk_header(bytes, offset)?;
        match chunk_type {
            0x0102 if chunk_header >= 16 && chunk_size >= 36 => {
                let name = android_u32(bytes, offset + 20)
                    .and_then(|index| strings.get(index as usize))?;
                let attribute_start = android_u16(bytes, offset + 24)? as usize;
                let attribute_size = android_u16(bytes, offset + 26)? as usize;
                let attribute_count = android_u16(bytes, offset + 28)? as usize;
                if attribute_size < 20 || attribute_count > 4096 {
                    return None;
                }
                xml.push('<');
                xml.push_str(name);
                let attributes = offset.checked_add(16 + attribute_start)?;
                for index in 0..attribute_count {
                    let attribute = attributes.checked_add(index.checked_mul(attribute_size)?)?;
                    if attribute.checked_add(20)? > offset + chunk_size {
                        return None;
                    }
                    let key = android_u32(bytes, attribute + 4)
                        .and_then(|value| strings.get(value as usize))?;
                    let raw = android_u32(bytes, attribute + 8)?;
                    let value_type = *bytes.get(attribute + 15)?;
                    let data = android_u32(bytes, attribute + 16)?;
                    let value = if raw != u32::MAX {
                        strings.get(raw as usize)?.clone()
                    } else {
                        android_typed_value(value_type, data, &strings, resources)?
                    };
                    xml.push(' ');
                    xml.push_str(key);
                    xml.push_str("=\"");
                    xml.push_str(&xml_escape(&value));
                    xml.push('"');
                }
                xml.push('>');
            }
            0x0103 if chunk_header >= 16 => {
                let name = android_u32(bytes, offset + 20)
                    .and_then(|index| strings.get(index as usize))?;
                xml.push_str("</");
                xml.push_str(name);
                xml.push('>');
            }
            _ => {}
        }
        offset = offset.checked_add(chunk_size)?;
    }
    (!xml.is_empty()).then_some(xml)
}

fn android_typed_value(
    value_type: u8,
    data: u32,
    strings: &[String],
    resources: Option<&[u8]>,
) -> Option<String> {
    match value_type {
        0x01 => Some(
            resources
                .and_then(|table| android_resource_reference_by_id(table, data))
                .unwrap_or_else(|| format!("@0x{data:08x}")),
        ),
        0x03 => strings.get(data as usize).cloned(),
        0x04 => Some(f32::from_bits(data).to_string()),
        0x10 => Some(data.to_string()),
        0x12 => Some(if data == 0 { "false" } else { "true" }.to_string()),
        0x1c..=0x1f => Some(format!("#{data:08x}")),
        _ => Some(data.to_string()),
    }
}

fn android_manifest_icon_reference(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut round_icon = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"application" => {
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    let Ok(value) = attr.normalized_value(XmlVersion::Implicit1_0) else {
                        continue;
                    };
                    if key == b"android:icon" || key == b"icon" {
                        return Some(value.into_owned());
                    }
                    if key == b"android:roundIcon" || key == b"roundIcon" {
                        round_icon = Some(value.into_owned());
                    }
                }
                return round_icon;
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn load_android_resource_image<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    reference: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if depth > 6 || preview_cancelled(cancel_cb) {
        return None;
    }
    if let Some(color) = parse_android_color(reference) {
        return Some(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            512, 512, color,
        )));
    }
    if let Some(table) = resources {
        for value in resolve_android_resource_values(table, reference) {
            let resolved = match value {
                AndroidResourceValue::Path(path) => load_android_archive_image(
                    zip,
                    resources,
                    &path,
                    depth,
                    decode_attempts,
                    cancel_cb,
                ),
                AndroidResourceValue::Color(color) => Some(DynamicImage::ImageRgba8(
                    RgbaImage::from_pixel(512, 512, color),
                )),
                AndroidResourceValue::Reference(reference) => load_android_resource_image(
                    zip,
                    resources,
                    &reference,
                    depth + 1,
                    decode_attempts,
                    cancel_cb,
                ),
            };
            if resolved.is_some() {
                return resolved;
            }
        }
    }
    let (kind, name) = parse_android_resource_reference(reference)?;
    let candidates = android_resource_candidates(zip, kind, name);
    for candidate in candidates {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        if let Some(image) = load_android_archive_image(
            zip,
            resources,
            &candidate,
            depth,
            decode_attempts,
            cancel_cb,
        ) {
            return Some(image);
        }
    }
    None
}

fn load_android_archive_image<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    path: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if *decode_attempts >= MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS {
        return None;
    }
    *decode_attempts += 1;
    let bytes = read_zip_bytes(zip, path, MAX_PACKAGE_ICON_BYTES)?;
    if is_supported_zip_image_name(&path.to_ascii_lowercase()) {
        return load_bounded_embedded_image(&bytes);
    }
    let xml = decode_android_xml(&bytes, resources)?;
    render_android_icon_xml(zip, resources, &xml, depth + 1, decode_attempts, cancel_cb)
}

enum AndroidResourceValue {
    Path(String),
    Color(Rgba<u8>),
    Reference(String),
}

fn android_resource_reference_by_id(table: &[u8], resource_id: u32) -> Option<String> {
    let wanted_package = (resource_id >> 24) as u8;
    let wanted_type = ((resource_id >> 16) & 0xff) as u8;
    let wanted_entry = (resource_id & 0xffff) as usize;
    let mut offset = 12usize;
    while let Some((chunk_type, header_size, chunk_size)) = android_chunk_header(table, offset) {
        if chunk_type == 0x0200 && header_size >= 284 {
            let package_id = android_u32(table, offset + 8)? as u8;
            if package_id == wanted_package {
                let type_strings = android_u32(table, offset + 268)
                    .and_then(|value| android_string_pool(table, offset + value as usize))?;
                let key_strings = android_u32(table, offset + 276)
                    .and_then(|value| android_string_pool(table, offset + value as usize))?;
                let type_name = wanted_type
                    .checked_sub(1)
                    .and_then(|index| type_strings.get(index as usize))?;
                let end = offset + chunk_size;
                let mut child = offset + header_size;
                while child < end {
                    let (child_type, child_header, child_size) =
                        android_chunk_header(table, child)?;
                    if child_type == 0x0201 && table.get(child + 8).copied() == Some(wanted_type) {
                        let count = android_u32(table, child + 12)? as usize;
                        if wanted_entry < count {
                            let entries_start = child + android_u32(table, child + 16)? as usize;
                            let offsets_start = child + child_header;
                            let sparse = table.get(child + 9).copied().unwrap_or(0) & 0x01 != 0;
                            let relative = if sparse {
                                let offset_count = entries_start.saturating_sub(offsets_start) / 4;
                                (0..offset_count).find_map(|index| {
                                    let at = offsets_start + index * 4;
                                    if android_u16(table, at)? as usize != wanted_entry {
                                        return None;
                                    }
                                    Some(u32::from(android_u16(table, at + 2)?) * 4)
                                })?
                            } else {
                                android_u32(table, offsets_start + wanted_entry * 4)?
                            };
                            if relative != u32::MAX {
                                let entry = entries_start + relative as usize;
                                let key = android_u32(table, entry + 4)
                                    .and_then(|index| key_strings.get(index as usize))?;
                                return Some(format!("@{type_name}/{key}"));
                            }
                        }
                    }
                    child = child.checked_add(child_size)?;
                }
            }
        }
        offset = offset.checked_add(chunk_size)?;
    }
    None
}

fn resolve_android_resource_values(table: &[u8], reference: &str) -> Vec<AndroidResourceValue> {
    let Some((kind, name)) = parse_android_resource_reference(reference) else {
        return Vec::new();
    };
    let Some(global_pool_offset) = android_find_chunk(table, 12, 0x0001) else {
        return Vec::new();
    };
    let Some(global_strings) = android_string_pool(table, global_pool_offset) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut offset = 12usize;
    while let Some((chunk_type, header_size, chunk_size)) = android_chunk_header(table, offset) {
        if chunk_type == 0x0200 && header_size >= 284 {
            let type_offset = android_u32(table, offset + 268).map(|value| offset + value as usize);
            let key_offset = android_u32(table, offset + 276).map(|value| offset + value as usize);
            let (Some(type_strings), Some(key_strings)) = (
                type_offset.and_then(|at| android_string_pool(table, at)),
                key_offset.and_then(|at| android_string_pool(table, at)),
            ) else {
                offset += chunk_size;
                continue;
            };
            let end = offset.saturating_add(chunk_size).min(table.len());
            let mut child = offset + header_size;
            while child < end {
                let Some((child_type, child_header, child_size)) =
                    android_chunk_header(table, child)
                else {
                    break;
                };
                if child_type == 0x0201 && child_header >= 20 {
                    let type_id = table.get(child + 8).copied().unwrap_or(0);
                    let type_name = type_id
                        .checked_sub(1)
                        .and_then(|id| type_strings.get(id as usize));
                    if type_name.map(String::as_str) == Some(kind) {
                        collect_android_type_values(
                            table,
                            AndroidTypeChunk {
                                offset: child,
                                header_size: child_header,
                                size: child_size,
                            },
                            &key_strings,
                            name,
                            &global_strings,
                            &mut values,
                        );
                    }
                }
                child = child.saturating_add(child_size);
            }
        }
        offset = offset.saturating_add(chunk_size);
    }
    values.sort_by_key(|value| match value {
        AndroidResourceValue::Path(path) if path.to_ascii_lowercase().ends_with(".xml") => 0,
        AndroidResourceValue::Reference(_) => 1,
        AndroidResourceValue::Path(_) => 2,
        AndroidResourceValue::Color(_) => 3,
    });
    values
}

#[derive(Clone, Copy)]
struct AndroidTypeChunk {
    offset: usize,
    header_size: usize,
    size: usize,
}

fn collect_android_type_values(
    table: &[u8],
    chunk: AndroidTypeChunk,
    keys: &[String],
    wanted_name: &str,
    global_strings: &[String],
    output: &mut Vec<AndroidResourceValue>,
) {
    let Some(entry_count) = android_u32(table, chunk.offset + 12).map(|value| value as usize)
    else {
        return;
    };
    let Some(entries_start) =
        android_u32(table, chunk.offset + 16).map(|value| chunk.offset + value as usize)
    else {
        return;
    };
    let offsets_start = chunk.offset + chunk.header_size;
    let chunk_end = chunk.offset.saturating_add(chunk.size).min(table.len());
    let sparse = table.get(chunk.offset + 9).copied().unwrap_or(0) & 0x01 != 0;
    let offset_count = if sparse {
        entries_start.saturating_sub(offsets_start) / 4
    } else {
        entry_count
    };
    for index in 0..offset_count {
        let relative = if sparse {
            let Some(value) = android_u16(table, offsets_start + index * 4 + 2) else {
                break;
            };
            u32::from(value) * 4
        } else {
            let Some(value) = android_u32(table, offsets_start + index * 4) else {
                break;
            };
            if value == u32::MAX {
                continue;
            }
            value
        };
        let entry = entries_start.saturating_add(relative as usize);
        if entry + 16 > chunk_end {
            continue;
        }
        let flags = android_u16(table, entry + 2).unwrap_or(0);
        if flags & 1 != 0 {
            continue;
        }
        let key = android_u32(table, entry + 4).and_then(|value| keys.get(value as usize));
        if key.map(String::as_str) != Some(wanted_name) {
            continue;
        }
        let value_type = table[entry + 11];
        let data = android_u32(table, entry + 12).unwrap_or(0);
        match value_type {
            0x01 => {
                if let Some(reference) = android_resource_reference_by_id(table, data) {
                    output.push(AndroidResourceValue::Reference(reference));
                }
            }
            0x03 => {
                if let Some(path) = global_strings.get(data as usize) {
                    output.push(AndroidResourceValue::Path(path.replace('\\', "/")));
                }
            }
            0x1c..=0x1f => output.push(AndroidResourceValue::Color(Rgba([
                ((data >> 16) & 0xff) as u8,
                ((data >> 8) & 0xff) as u8,
                (data & 0xff) as u8,
                ((data >> 24) & 0xff) as u8,
            ]))),
            _ => {}
        }
    }
}

fn android_find_chunk(bytes: &[u8], mut offset: usize, wanted: u16) -> Option<usize> {
    while offset < bytes.len() {
        let (chunk_type, _, chunk_size) = android_chunk_header(bytes, offset)?;
        if chunk_type == wanted {
            return Some(offset);
        }
        offset = offset.checked_add(chunk_size)?;
    }
    None
}

fn android_chunk_header(bytes: &[u8], offset: usize) -> Option<(u16, usize, usize)> {
    let chunk_type = android_u16(bytes, offset)?;
    let header_size = android_u16(bytes, offset + 2)? as usize;
    let chunk_size = android_u32(bytes, offset + 4)? as usize;
    (header_size >= 8
        && chunk_size >= header_size
        && offset.checked_add(chunk_size)? <= bytes.len())
    .then_some((chunk_type, header_size, chunk_size))
}

fn android_string_pool(bytes: &[u8], offset: usize) -> Option<Vec<String>> {
    let (chunk_type, header_size, chunk_size) = android_chunk_header(bytes, offset)?;
    if chunk_type != 0x0001 || header_size < 28 {
        return None;
    }
    let count = android_u32(bytes, offset + 8)? as usize;
    if count > 1_000_000 {
        return None;
    }
    let flags = android_u32(bytes, offset + 16)?;
    let strings_start = offset.checked_add(android_u32(bytes, offset + 20)? as usize)?;
    let end = offset + chunk_size;
    let offsets_start = offset + header_size;
    let mut strings = Vec::with_capacity(count);
    for index in 0..count {
        let relative = android_u32(bytes, offsets_start + index * 4)? as usize;
        let at = strings_start.checked_add(relative)?;
        let value = if flags & 0x100 != 0 {
            android_utf8_string(bytes, at, end)
        } else {
            android_utf16_string(bytes, at, end)
        }?;
        strings.push(value);
    }
    Some(strings)
}

fn android_utf8_string(bytes: &[u8], mut offset: usize, end: usize) -> Option<String> {
    let (_, next) = android_length8(bytes, offset, end)?;
    offset = next;
    let (length, next) = android_length8(bytes, offset, end)?;
    offset = next;
    let value = bytes.get(offset..offset.checked_add(length)?.min(end))?;
    std::str::from_utf8(value).ok().map(str::to_owned)
}

fn android_length8(bytes: &[u8], offset: usize, end: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(offset)?;
    if offset >= end {
        return None;
    }
    if first & 0x80 == 0 {
        Some((first as usize, offset + 1))
    } else {
        Some((
            (((first & 0x7f) as usize) << 8) | *bytes.get(offset + 1)? as usize,
            offset + 2,
        ))
    }
}

fn android_utf16_string(bytes: &[u8], mut offset: usize, end: usize) -> Option<String> {
    let first = android_u16(bytes, offset)?;
    offset += 2;
    let length = if first & 0x8000 == 0 {
        first as usize
    } else {
        let second = android_u16(bytes, offset)?;
        offset += 2;
        (((first & 0x7fff) as usize) << 16) | second as usize
    };
    let byte_length = length.checked_mul(2)?;
    if offset.checked_add(byte_length)? > end {
        return None;
    }
    let units = bytes[offset..offset + byte_length]
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]));
    Some(
        char::decode_utf16(units)
            .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
    )
}

fn android_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn android_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn parse_android_resource_reference(reference: &str) -> Option<(&str, &str)> {
    let value = reference.trim().strip_prefix('@')?;
    let value = value
        .split_once(':')
        .map(|(_, value)| value)
        .unwrap_or(value);
    let (kind, name) = value.split_once('/')?;
    (!kind.is_empty() && !name.is_empty()).then_some((kind, name))
}

fn android_resource_candidates<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    kind: &str,
    name: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let path = entry.name().replace('\\', "/");
        let lower = path.to_ascii_lowercase();
        let Some(file_name) = lower.rsplit('/').next() else {
            continue;
        };
        let Some((stem, extension)) = file_name.rsplit_once('.') else {
            continue;
        };
        let directory_match =
            lower.starts_with(&format!("res/{kind}")) || lower.contains(&format!("/res/{kind}"));
        if directory_match
            && stem == name.to_ascii_lowercase()
            && matches!(extension, "png" | "webp" | "jpg" | "jpeg" | "bmp" | "xml")
            && entry.size() <= MAX_PACKAGE_ICON_BYTES
        {
            candidates.push((android_density_score(&lower), path));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().map(|(_, path)| path).collect()
}

fn android_density_score(path: &str) -> i32 {
    if path.contains("anydpi-v26") {
        1000
    } else if path.contains("anydpi") {
        900
    } else if path.contains("xxxhdpi") {
        640
    } else if path.contains("xxhdpi") {
        480
    } else if path.contains("xhdpi") {
        320
    } else if path.contains("hdpi") {
        240
    } else if path.contains("mdpi") {
        160
    } else {
        0
    }
}

fn render_android_icon_xml<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    xml: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if xml.contains("<adaptive-icon") {
        let background = android_xml_drawable_reference(xml, "background").and_then(|value| {
            load_android_resource_image(zip, resources, &value, depth, decode_attempts, cancel_cb)
        });
        let foreground = android_xml_drawable_reference(xml, "foreground").and_then(|value| {
            load_android_resource_image(zip, resources, &value, depth, decode_attempts, cancel_cb)
        })?;
        let mut canvas = background
            .map(|image| {
                image
                    .resize_exact(512, 512, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            })
            .unwrap_or_else(|| RgbaImage::new(512, 512));
        let foreground = foreground
            .resize_exact(512, 512, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        image::imageops::overlay(&mut canvas, &foreground, 0, 0);
        return Some(DynamicImage::ImageRgba8(mask_android_adaptive_icon(canvas)));
    }
    if xml.contains("<color") {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(Event::Text(text)) => {
                    if let Some(color) = text.decode().ok().and_then(|value| {
                        quick_xml::escape::unescape(&value)
                            .ok()
                            .and_then(|unescaped| parse_android_color(&unescaped))
                    }) {
                        return Some(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
                            512, 512, color,
                        )));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    render_android_vector(xml)
}

fn mask_android_adaptive_icon(canvas: RgbaImage) -> RgbaImage {
    // Android adaptive layers use a 108dp canvas, while the launcher mask occupies the centered
    // 72dp. Cropping that motion-safe perimeter matches the installed icon's apparent scale.
    let crop_size = canvas.width().min(canvas.height()) * 2 / 3;
    let crop_x = (canvas.width() - crop_size) / 2;
    let crop_y = (canvas.height() - crop_size) / 2;
    let cropped =
        image::imageops::crop_imm(&canvas, crop_x, crop_y, crop_size, crop_size).to_image();
    let mut output =
        image::imageops::resize(&cropped, 512, 512, image::imageops::FilterType::Lanczos3);
    let center = 255.5_f32;
    let radius = 255.5_f32;
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
        let mask_alpha = ((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u32;
        pixel[3] = ((u32::from(pixel[3]) * mask_alpha + 127) / 255) as u8;
    }
    output
}

fn android_xml_drawable_reference(xml: &str, element: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().as_ref() == element.as_bytes() =>
            {
                for attr in e.attributes().flatten() {
                    if matches!(
                        attr.key.as_ref(),
                        b"android:drawable" | b"drawable" | b"android:color" | b"color"
                    ) {
                        return attr
                            .normalized_value(XmlVersion::Implicit1_0)
                            .ok()
                            .map(|value| value.into_owned());
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn parse_android_color(value: &str) -> Option<Rgba<u8>> {
    let hex = value.trim().strip_prefix('#')?;
    let raw = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(Rgba([
            ((raw >> 16) & 0xff) as u8,
            ((raw >> 8) & 0xff) as u8,
            (raw & 0xff) as u8,
            255,
        ])),
        8 => Some(Rgba([
            ((raw >> 16) & 0xff) as u8,
            ((raw >> 8) & 0xff) as u8,
            (raw & 0xff) as u8,
            ((raw >> 24) & 0xff) as u8,
        ])),
        _ => None,
    }
}

fn render_android_vector(xml: &str) -> Option<DynamicImage> {
    if !xml.contains("<vector") {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    let mut viewport_width = 24.0_f32;
    let mut viewport_height = 24.0_f32;
    let mut paths = String::new();
    let mut group_depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"vector" => {
                viewport_width =
                    android_float_attribute(&e, b"android:viewportWidth").unwrap_or(viewport_width);
                viewport_height = android_float_attribute(&e, b"android:viewportHeight")
                    .unwrap_or(viewport_height);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"path" => {
                let data = android_string_attribute(&e, b"android:pathData")?;
                let fill = android_string_attribute(&e, b"android:fillColor")
                    .filter(|value| parse_android_color(value).is_some())
                    .unwrap_or_else(|| "none".to_string());
                let stroke = android_string_attribute(&e, b"android:strokeColor")
                    .filter(|value| parse_android_color(value).is_some());
                if fill != "none" || stroke.is_some() {
                    paths.push_str(&format!(
                        "<path d=\"{}\" fill=\"{}\"",
                        xml_escape(&data),
                        fill
                    ));
                    if let Some(stroke) = stroke {
                        let width =
                            android_float_attribute(&e, b"android:strokeWidth").unwrap_or(1.0);
                        paths.push_str(&format!(
                            " stroke=\"{}\" stroke-width=\"{}\"",
                            stroke, width
                        ));
                    }
                    paths.push_str("/>");
                }
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"group" => {
                paths.push_str(&android_svg_group_start(&e));
                group_depth += 1;
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"group" && group_depth > 0 => {
                paths.push_str("</g>");
                group_depth -= 1;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    for _ in 0..group_depth {
        paths.push_str("</g>");
    }
    if paths.is_empty() || viewport_width <= 0.0 || viewport_height <= 0.0 {
        return None;
    }
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {viewport_width} {viewport_height}\">{paths}</svg>");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(512, 512)?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(512.0 / viewport_width, 512.0 / viewport_height);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            pixel[0] = ((u32::from(pixel[0]) * 255 + alpha / 2) / alpha).min(255) as u8;
            pixel[1] = ((u32::from(pixel[1]) * 255 + alpha / 2) / alpha).min(255) as u8;
            pixel[2] = ((u32::from(pixel[2]) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    RgbaImage::from_raw(512, 512, rgba).map(DynamicImage::ImageRgba8)
}

fn android_svg_group_start(element: &BytesStart<'_>) -> String {
    let pivot_x = android_float_attribute(element, b"android:pivotX").unwrap_or(0.0);
    let pivot_y = android_float_attribute(element, b"android:pivotY").unwrap_or(0.0);
    let scale_x = android_float_attribute(element, b"android:scaleX").unwrap_or(1.0);
    let scale_y = android_float_attribute(element, b"android:scaleY").unwrap_or(1.0);
    let translate_x = android_float_attribute(element, b"android:translateX").unwrap_or(0.0);
    let translate_y = android_float_attribute(element, b"android:translateY").unwrap_or(0.0);
    let rotation = android_float_attribute(element, b"android:rotation").unwrap_or(0.0);
    format!(
        "<g transform=\"translate({},{}) rotate({}) scale({},{}) translate({},{})\">",
        pivot_x + translate_x,
        pivot_y + translate_y,
        rotation,
        scale_x,
        scale_y,
        -pivot_x,
        -pivot_y,
    )
}

fn android_string_attribute(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attr| {
            attr.key.as_ref() == name
                || attr.key.as_ref() == name.strip_prefix(b"android:").unwrap_or(name)
        })
        .and_then(|attr| {
            attr.normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn android_float_attribute(element: &BytesStart<'_>, name: &[u8]) -> Option<f32> {
    android_string_attribute(element, name)?
        .trim_end_matches("dp")
        .parse()
        .ok()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
    if manifest_icons
        .iter()
        .filter_map(|candidate| {
            candidate
                .rsplit_once('.')
                .map(|(candidate_stem, _)| candidate_stem)
        })
        .any(|candidate_stem| stem.starts_with(candidate_stem))
    {
        260
    } else {
        0
    }
}

fn package_icon_candidate_score(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    if !is_supported_zip_image_name(&lower) {
        return 0;
    }
    let is_android_mipmap = lower.starts_with("res/mipmap") || lower.contains("/res/mipmap");

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
    if score == 0 && !is_android_mipmap {
        return 0;
    }

    if lower.starts_with("assets/") || lower.contains("/assets/") {
        score += 30;
    }
    if is_android_mipmap {
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

fn render_zip_archive_from_zip<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    logical_name: &str,
    root_path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let mut entries: BTreeMap<String, ArchiveListingEntry> = BTreeMap::new();
    // key: virtual path → (name, parent, is_folder, size, packed_size, modified_unix, encrypted)
    let mut file_count = 0u64;
    let mut uncompressed = 0i64;
    let mut compressed = 0i64;
    let mut seen = 0usize;
    let mut partial = false;
    let mut encrypted_file_count = 0usize;

    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
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
        let is_encrypted = entry.encrypted();
        let modified = entry
            .last_modified()
            .map(|d| {
                // zip::DateTime → unix seconds (approximate: no leap seconds, no TZ)

                ((d.year() as i64 - 1970) * 365 * 86400)
                    + ((d.month() as i64 - 1) * 30 * 86400)
                    + ((d.day() as i64 - 1) * 86400)
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
                entries.insert(path.clone(), (name, parent_of(&path), true, 0, 0, 0, false));
            }
        } else {
            file_count += 1;
            if is_encrypted {
                encrypted_file_count += 1;
            }
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
                    (
                        name,
                        parent_of(&full_name),
                        false,
                        size,
                        packed,
                        modified,
                        is_encrypted,
                    ),
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

    Ok(archive_listing_json(
        filename,
        root_path,
        "archive",
        entries,
        ArchiveListingStats {
            file_count,
            uncompressed,
            compressed,
            partial,
            encrypted_file_count,
            can_preview_entries: true,
        },
    ))
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
    logical_name: &str,
    root_path: &str,
    kind: &str,
    reader: R,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let mut archive = TarArchive::new(TarScanReader::new(reader, cancel_cb));
    let mut entries: BTreeMap<String, ArchiveListingEntry> = BTreeMap::new();
    let mut file_count = 0u64;
    let mut uncompressed = 0i64;
    let mut seen = 0usize;
    let mut partial = false;

    let archive_entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    for (scanned, entry) in archive_entries.enumerate() {
        if preview_cancelled(cancel_cb) {
            return String::new();
        }
        if scanned == MAX_ARCHIVE_SCAN_ENTRIES {
            partial = true;
            break;
        }
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
                    (name, parent_of(&folder_path), true, 0, 0, modified, false),
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
                    (name, parent_of(&full_name), false, size, 0, modified, false),
                );
                seen += 1;
            } else {
                partial = true;
            }
        }
    }

    archive_listing_json(
        filename,
        root_path,
        kind,
        entries,
        ArchiveListingStats {
            file_count,
            uncompressed,
            compressed: 0,
            partial,
            encrypted_file_count: 0,
            can_preview_entries: false,
        },
    )
}

fn render_gzip_member_reader<R: Read + Seek>(
    reader: &mut R,
    logical_name: &str,
    root_path: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let member_name = filename
        .strip_suffix(".gz")
        .or_else(|| filename.strip_suffix(".GZ"))
        .filter(|s| !s.is_empty())
        .unwrap_or(filename);
    if source_len < 4 {
        return Err(ReaderPreviewError::Malformed);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    reader
        .seek(SeekFrom::End(-4))
        .map_err(|_| ReaderPreviewError::Io)?;
    let mut trailer = [0u8; 4];
    read_exact_cancelable(reader, &mut trailer, cancel_cb)?;
    let compressed = i64::try_from(source_len).map_err(|_| ReaderPreviewError::LengthMismatch)?;
    let uncompressed = u32::from_le_bytes(trailer) as i64;
    let mut entries = BTreeMap::new();
    entries.insert(
        member_name.to_string(),
        (
            member_name.to_string(),
            String::new(),
            false,
            uncompressed,
            compressed,
            modified_unix,
            false,
        ),
    );
    Ok(archive_listing_json(
        filename,
        root_path,
        "archive",
        entries,
        ArchiveListingStats {
            file_count: 1,
            uncompressed,
            compressed,
            partial: false,
            encrypted_file_count: 0,
            can_preview_entries: false,
        },
    ))
}

struct ArchiveListingStats {
    file_count: u64,
    uncompressed: i64,
    compressed: i64,
    partial: bool,
    encrypted_file_count: usize,
    can_preview_entries: bool,
}

fn archive_listing_json(
    filename: &str,
    root_path: &str,
    kind: &str,
    entries: BTreeMap<String, ArchiveListingEntry>,
    stats: ArchiveListingStats,
) -> String {
    let ArchiveListingStats {
        file_count,
        uncompressed,
        compressed,
        partial,
        encrypted_file_count,
        can_preview_entries,
    } = stats;
    let folder_count = entries
        .values()
        .filter(|(_, _, is_folder, _, _, _, _)| *is_folder)
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
    let top_level_folders = entries
        .values()
        .filter(|(_, parent, is_folder, _, _, _, _)| *is_folder && parent.is_empty())
        .count();
    if top_level_folders > 0 {
        summary.push_str(&format!(" - {top_level_folders} top-level folders"));
    }
    if let Some(largest) = archive_largest_file_summary(&entries) {
        summary.push_str(&format!(" - Largest: {largest}"));
    }
    if let Some(types) = archive_type_summary(&entries) {
        summary.push_str(&format!(" - Types: {types}"));
    }
    if let Some(projects) = archive_project_summary(&entries) {
        summary.push_str(&format!(" - Project markers: {projects}"));
    }

    let mut items = Vec::with_capacity(entries.len());
    for (path, (name, parent, is_folder, size, packed, modified, is_encrypted)) in &entries {
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
            is_encrypted: *is_encrypted,
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
            can_preview_entries,
            encrypted_file_count,
            items,
        }),
        table: None,
        markdown: None,
    })
}

fn archive_largest_file_summary(entries: &BTreeMap<String, ArchiveListingEntry>) -> Option<String> {
    let mut files = entries
        .iter()
        .filter_map(|(path, (_, _, is_folder, size, _, _, _))| {
            (!*is_folder && *size > 0).then_some((path, *size))
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    if files.is_empty() {
        return None;
    }
    Some(
        files
            .into_iter()
            .take(3)
            .map(|(path, size)| {
                let display = if path.chars().count() > 80 {
                    format!(
                        "...{}",
                        path.chars()
                            .rev()
                            .take(77)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    path.clone()
                };
                format!("{display} ({})", format_bytes(size))
            })
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn archive_type_summary(entries: &BTreeMap<String, ArchiveListingEntry>) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for (name, _, is_folder, _, _, _, _) in entries.values() {
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

fn archive_project_summary(entries: &BTreeMap<String, ArchiveListingEntry>) -> Option<String> {
    let mut markers = Vec::<String>::new();
    for (name, _, is_folder, _, _, _, _) in entries.values() {
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

fn add_parent_folders(path: &str, entries: &mut BTreeMap<String, ArchiveListingEntry>) {
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
                (name, parent_of(&folder_path), true, 0, 0, 0, false),
            );
        }
        start = full_idx + 1;
    }
}

fn add_rar_parent_folders(
    path: &str,
    entries: &mut BTreeMap<String, ArchiveListingEntry>,
    retained_path_bytes: &mut usize,
) -> bool {
    let mut start = 0;
    while let Some(idx) = path[start..].find('/') {
        let full_idx = start + idx;
        let folder_path = format!("{}/", &path[..full_idx]);
        if !entries.contains_key(&folder_path) {
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                return false;
            }
            let name = path[..full_idx]
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let parent = parent_of(&folder_path);
            let retained = folder_path
                .len()
                .saturating_add(name.len())
                .saturating_add(parent.len());
            let Some(total) = retained_path_bytes.checked_add(retained) else {
                return false;
            };
            if total > MAX_RAR_RETAINED_PATH_BYTES {
                return false;
            }
            *retained_path_bytes = total;
            entries.insert(folder_path, (name, parent, true, 0, 0, 0, false));
        }
        start = full_idx + 1;
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::{Cursor, Write};

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

    fn test_office_context() -> OfficeContext {
        OfficeContext::new(None)
    }

    #[test]
    fn bencode_parser_rejects_excessive_nesting() {
        let mut bytes = vec![b'l'; MAX_BENCODE_DEPTH + 2];
        bytes.extend(std::iter::repeat_n(b'e', MAX_BENCODE_DEPTH + 2));

        assert!(parse_bencode(&bytes, None).is_none());
    }

    #[test]
    fn bounded_exact_reader_reports_length_mismatch_and_cancellation() {
        let mut exact = Cursor::new(b"data".to_vec());
        assert_eq!(
            read_reader_exact_bounded_cancelable(&mut exact, 4, 8, None),
            Ok(b"data".to_vec())
        );

        let mut short = Cursor::new(b"abc".to_vec());
        assert_eq!(
            read_reader_exact_bounded_cancelable(&mut short, 4, 8, None),
            Err(ReaderPreviewError::LengthMismatch)
        );

        let mut long = Cursor::new(b"abcde".to_vec());
        assert_eq!(
            read_reader_exact_bounded_cancelable(&mut long, 4, 8, None),
            Err(ReaderPreviewError::LengthMismatch)
        );

        let mut cancelled = Cursor::new(b"data".to_vec());
        assert_eq!(
            read_reader_exact_bounded_cancelable(&mut cancelled, 4, 8, Some(always_cancel)),
            Err(ReaderPreviewError::Cancelled)
        );
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
    fn archive_extract_budget_rejects_oversized_or_extreme_entries() {
        assert!(archive_entry_within_extract_budget(1024, 128));
        assert!(archive_entry_within_extract_budget(0, 0));
        assert!(!archive_entry_within_extract_budget(
            MAX_ARCHIVE_EXTRACT_BYTES + 1,
            1024
        ));
        assert!(!archive_entry_within_extract_budget(
            1024,
            MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES + 1
        ));
        assert!(!archive_entry_within_extract_budget(1_000_001, 1000));
        assert!(!archive_entry_within_extract_budget(1, 0));
    }

    fn test_zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start ZIP entry");
            writer.write_all(bytes).expect("write ZIP entry");
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    #[test]
    fn archive_reader_supports_tar_tgz_and_gzip_without_a_path() {
        let payload = b"reader archive";
        let mut tar_builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path("folder/item.txt").expect("set TAR path");
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder
            .append(&header, payload.as_slice())
            .expect("append TAR entry");
        let tar_bytes = tar_builder.into_inner().expect("finish TAR");
        let tar_json = render_archive_reader(
            Cursor::new(tar_bytes.clone()),
            r"C:\missing\logical.tar",
            tar_bytes.len() as u64,
            0,
            None,
        )
        .expect("TAR reader preview");
        assert!(tar_json.contains("\"rootPath\":\"\""));
        assert!(tar_json.contains("\"canPreviewEntries\":false"));
        assert!(tar_json.contains("folder/item.txt"));

        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar_bytes).expect("compress TAR");
        let tgz_bytes = gzip.finish().expect("finish TGZ");
        let tgz_json = render_archive_reader(
            Cursor::new(tgz_bytes.clone()),
            "logical.tgz",
            tgz_bytes.len() as u64,
            0,
            None,
        )
        .expect("TGZ reader preview");
        assert!(tgz_json.contains("\"canPreviewEntries\":false"));
        assert!(tgz_json.contains("folder/item.txt"));

        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(payload).expect("compress GZIP member");
        let gzip_bytes = gzip.finish().expect("finish GZIP");
        let gzip_json = render_archive_reader(
            Cursor::new(gzip_bytes.clone()),
            "logical.txt.gz",
            gzip_bytes.len() as u64,
            123,
            None,
        )
        .expect("GZIP reader preview");
        let gzip_json: serde_json::Value =
            serde_json::from_str(&gzip_json).expect("GZIP listing JSON");
        assert_eq!(gzip_json["listing"]["rootPath"], "");
        assert_eq!(gzip_json["listing"]["canPreviewEntries"], false);
        assert_eq!(gzip_json["listing"]["items"][0]["path"], "logical.txt");
        assert_eq!(
            gzip_json["listing"]["items"][0]["size"],
            payload.len() as u64
        );
    }

    #[test]
    fn archive_zip_reader_retains_partial_listing_below_hard_entry_cap() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..=MAX_ARCHIVE_SCAN_ENTRIES {
            writer
                .start_file(
                    format!("entry-{index:05}.txt"),
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("start bounded ZIP entry");
        }
        let bytes = writer.finish().expect("finish large ZIP").into_inner();
        let json = render_archive_reader(
            Cursor::new(bytes.clone()),
            "many.zip",
            bytes.len() as u64,
            0,
            None,
        )
        .expect("partial archive listing");
        let json: serde_json::Value = serde_json::from_str(&json).expect("archive JSON");
        assert_eq!(json["listing"]["isPartial"], true);
        assert!(json["listing"]["items"].as_array().unwrap().len() <= MAX_ARCHIVE_ENTRIES);
    }

    fn synthetic_zip64_end(entries: u64, central_size: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44u64.to_le_bytes());
        bytes.extend_from_slice(&45u16.to_le_bytes());
        bytes.extend_from_slice(&45u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&entries.to_le_bytes());
        bytes.extend_from_slice(&entries.to_le_bytes());
        bytes.extend_from_slice(&central_size.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    #[test]
    fn zip_preflight_rejects_hard_entry_and_central_directory_caps() {
        let too_many = synthetic_zip64_end(MAX_ARCHIVE_ZIP_ENTRIES + 1, 0);
        assert_eq!(
            validate_zip_container(
                &mut Cursor::new(too_many.clone()),
                too_many.len() as u64,
                MAX_ARCHIVE_ZIP_ENTRIES,
                None,
            )
            .err(),
            Some(ReaderPreviewError::LimitExceeded)
        );

        let central_too_large = synthetic_zip64_end(0, MAX_ZIP_CENTRAL_DIRECTORY_BYTES + 1);
        assert_eq!(
            validate_zip_container(
                &mut Cursor::new(central_too_large.clone()),
                central_too_large.len() as u64,
                MAX_ARCHIVE_ZIP_ENTRIES,
                None,
            )
            .err(),
            Some(ReaderPreviewError::LimitExceeded)
        );
    }

    #[test]
    fn zip_open_rechecks_authoritative_directory_tail_after_eocd_fallback() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("entry.txt", zip::write::SimpleFileOptions::default())
            .expect("start ZIP entry");
        writer.write_all(b"bounded").expect("write ZIP entry");
        let mut bytes = writer.finish().expect("finish ZIP").into_inner();
        bytes.resize(
            bytes.len()
                + MAX_ZIP_CENTRAL_DIRECTORY_BYTES as usize
                + ZIP_EOCD_MAX_TAIL_BYTES as usize
                + 1024,
            0,
        );
        // The EOCD fields are structurally valid, but its one-byte central directory cannot contain
        // the declared entry. The ZIP reader must reject it and may fall back to the real EOCD.
        let fake_central_offset = bytes.len() as u32;
        bytes.push(0);
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&fake_central_offset.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());

        let result = open_validated_zip(
            Cursor::new(bytes.clone()),
            bytes.len() as u64,
            MAX_ARCHIVE_ZIP_ENTRIES,
            None,
        );
        assert!(matches!(result, Err(ReaderPreviewError::LimitExceeded)));
    }

    static ZIP_OPEN_CANCEL_CHECKS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    extern "C" fn cancel_during_zip_open() -> bool {
        ZIP_OPEN_CANCEL_CHECKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 4
    }

    #[test]
    fn zip_archive_open_honors_cancellation_after_preflight() {
        let bytes = test_zip_bytes(&[("entry.txt", b"content")]);
        ZIP_OPEN_CANCEL_CHECKS.store(0, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            open_validated_zip(
                Cursor::new(bytes.clone()),
                bytes.len() as u64,
                MAX_ARCHIVE_ZIP_ENTRIES,
                Some(cancel_during_zip_open),
            ),
            Err(ReaderPreviewError::Cancelled)
        ));
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
    fn encrypted_zip_entries_are_reported_and_not_extracted() {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-encrypted-{}.zip",
            std::process::id()
        ));
        let file = fs::File::create(&path).expect("create encrypted zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes128, "test-password");
        writer
            .start_file("secret.txt", options)
            .expect("start encrypted entry");
        writer.write_all(b"secret").expect("write encrypted entry");
        writer.finish().expect("finish encrypted zip");

        let json = render_archive(path.to_str().unwrap(), None);
        let extracted = extract_archive_entry_to_temp(path.to_str().unwrap(), "secret.txt", None);
        let _ = fs::remove_file(path);

        assert!(json.contains("\"encryptedFileCount\":1"));
        assert!(json.contains("\"isEncrypted\":true"));
        assert!(extracted.is_none());
    }

    #[test]
    fn package_icon_candidates_accept_arbitrary_android_mipmap_names() {
        assert!(package_icon_candidate_score("res/mipmap-xxxhdpi/product_mark.png") > 0);
        assert!(package_icon_candidate_score("base/res/mipmap-hdpi/brand_asset.webp") > 0);
        assert_eq!(
            package_icon_candidate_score("res/drawable/random_photo.png"),
            0
        );
        assert_eq!(
            package_icon_candidate_score("res/mipmap-anydpi-v26/product_mark.xml"),
            0
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
    fn package_icon_resolves_manifest_adaptive_icon_layers() {
        let path = std::env::temp_dir().join(format!(
            "quicklook-next-adaptive-icon-{}.apk",
            std::process::id()
        ));
        let file = fs::File::create(&path).expect("create adaptive icon APK");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("AndroidManifest.xml", options)
            .expect("start manifest");
        writer.write_all(br#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application android:icon="@mipmap/product_mark"/></manifest>"#).expect("write manifest");
        writer
            .start_file("res/mipmap-anydpi-v26/product_mark.xml", options)
            .expect("start adaptive icon");
        writer.write_all(br##"<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android"><background android:drawable="#112233"/><foreground android:drawable="@drawable/product_foreground"/></adaptive-icon>"##).expect("write adaptive icon");
        let foreground =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 32, Rgba([20, 220, 40, 255])));
        let mut foreground_png = Cursor::new(Vec::new());
        foreground
            .write_to(&mut foreground_png, image::ImageFormat::Png)
            .expect("encode foreground");
        writer
            .start_file("res/drawable-xxxhdpi/product_foreground.png", options)
            .expect("start foreground");
        writer
            .write_all(foreground_png.get_ref())
            .expect("write foreground");
        let unrelated =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(256, 256, Rgba([240, 10, 10, 255])));
        let mut unrelated_png = Cursor::new(Vec::new());
        unrelated
            .write_to(&mut unrelated_png, image::ImageFormat::Png)
            .expect("encode unrelated image");
        writer
            .start_file("res/mipmap-xxxhdpi/unrelated.png", options)
            .expect("start unrelated image");
        writer
            .write_all(unrelated_png.get_ref())
            .expect("write unrelated image");
        writer.finish().expect("finish adaptive icon APK");

        let (width, height, bgra) =
            extract_package_icon_bgra(path.to_str().unwrap(), None).expect("extract adaptive icon");
        let _ = fs::remove_file(path);

        assert_eq!((width, height), (512, 512));
        let center = ((256 * width + 256) * 4) as usize;
        assert_eq!(&bgra[center..center + 4], &[40, 220, 20, 255]);
    }

    #[test]
    fn android_resource_table_resolves_obfuscated_icon_path() {
        fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn string_pool(values: &[&str]) -> Vec<u8> {
            let mut data = Vec::new();
            let mut offsets = Vec::new();
            for value in values {
                offsets.push(data.len() as u32);
                data.push(value.len() as u8);
                data.push(value.len() as u8);
                data.extend_from_slice(value.as_bytes());
                data.push(0);
            }
            while data.len() % 4 != 0 {
                data.push(0);
            }
            let header_size = 28usize;
            let size = header_size + offsets.len() * 4 + data.len();
            let mut pool = vec![0; size];
            put_u16(&mut pool, 0, 0x0001);
            put_u16(&mut pool, 2, header_size as u16);
            put_u32(&mut pool, 4, size as u32);
            put_u32(&mut pool, 8, values.len() as u32);
            put_u32(&mut pool, 16, 0x100);
            put_u32(&mut pool, 20, (header_size + offsets.len() * 4) as u32);
            for (index, offset) in offsets.into_iter().enumerate() {
                put_u32(&mut pool, header_size + index * 4, offset);
            }
            pool[header_size + values.len() * 4..].copy_from_slice(&data);
            pool
        }

        let global = string_pool(&["res/9w.png"]);
        let types = string_pool(&["mipmap"]);
        let keys = string_pool(&["product_mark"]);
        let mut type_chunk = vec![0; 48];
        put_u16(&mut type_chunk, 0, 0x0201);
        put_u16(&mut type_chunk, 2, 28);
        put_u32(&mut type_chunk, 4, 48);
        type_chunk[8] = 1;
        put_u32(&mut type_chunk, 12, 1);
        put_u32(&mut type_chunk, 16, 32);
        put_u32(&mut type_chunk, 20, 8);
        put_u32(&mut type_chunk, 28, 0);
        put_u16(&mut type_chunk, 32, 8);
        put_u32(&mut type_chunk, 36, 0);
        put_u16(&mut type_chunk, 40, 8);
        type_chunk[43] = 0x03;
        put_u32(&mut type_chunk, 44, 0);
        let package_size = 288 + types.len() + keys.len() + type_chunk.len();
        let mut package = vec![0; 288];
        put_u16(&mut package, 0, 0x0200);
        put_u16(&mut package, 2, 288);
        put_u32(&mut package, 4, package_size as u32);
        put_u32(&mut package, 268, 288);
        put_u32(&mut package, 276, (288 + types.len()) as u32);
        package.extend_from_slice(&types);
        package.extend_from_slice(&keys);
        package.extend_from_slice(&type_chunk);
        let table_size = 12 + global.len() + package.len();
        let mut table = vec![0; 12];
        put_u16(&mut table, 0, 0x0002);
        put_u16(&mut table, 2, 12);
        put_u32(&mut table, 4, table_size as u32);
        put_u32(&mut table, 8, 1);
        table.extend_from_slice(&global);
        table.extend_from_slice(&package);

        let values = resolve_android_resource_values(&table, "@mipmap/product_mark");
        assert!(
            matches!(values.as_slice(), [AndroidResourceValue::Path(path)] if path == "res/9w.png")
        );
    }

    #[test]
    fn android_vector_groups_render_transformed_foreground() {
        assert_eq!(
            android_typed_value(0x04, 0.135_f32.to_bits(), &[], None).as_deref(),
            Some("0.135")
        );
        let image = render_android_vector(
            r##"<vector android:viewportWidth="108" android:viewportHeight="108">
                <group android:scaleX="0.5" android:scaleY="0.5" android:translateX="27" android:translateY="27">
                    <path android:fillColor="#ff336ab6" android:pathData="M0,0 H108 V108 H0 Z"/>
                    <path android:fillColor="#ffffffff" android:pathData="M27,27 H81 V81 H27 Z"/>
                </group>
            </vector>"##,
        ).expect("render grouped Android vector").to_rgba8();
        let colors = image.pixels().map(|pixel| pixel.0).collect::<BTreeSet<_>>();

        assert!(
            colors.len() > 2,
            "grouped vector should include foreground and antialiased edges"
        );
        assert!(image.get_pixel(256, 256).0[3] > 0);
    }

    #[test]
    fn android_adaptive_icon_crops_safe_zone_and_masks_background() {
        let mut source = RgbaImage::from_pixel(108, 108, Rgba([20, 40, 60, 255]));
        for y in 45..63 {
            for x in 45..63 {
                source.put_pixel(x, y, Rgba([240, 180, 20, 255]));
            }
        }

        let output = mask_android_adaptive_icon(source);

        assert_eq!(output.get_pixel(0, 0).0[3], 0);
        assert_eq!(output.get_pixel(511, 0).0[3], 0);
        assert_eq!(output.get_pixel(256, 256).0, [240, 180, 20, 255]);
        assert!(output.get_pixel(256, 4).0[3] > 0);
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
    fn archive_extract_discard_only_removes_generated_roots() {
        let generated_root = create_archive_extract_root().expect("generated extract root");
        let generated_target = generated_root.join("entry-test");
        fs::write(&generated_target, b"temporary").expect("write generated extraction");
        discard_archive_extract_path(generated_target.to_str().unwrap());
        assert!(!generated_root.exists());

        let foreign_root = std::env::temp_dir().join(format!(
            "quicklook-next-foreign-root-{}",
            std::process::id()
        ));
        fs::create_dir_all(&foreign_root).expect("create foreign root");
        let foreign_target = foreign_root.join("entry-test");
        fs::write(&foreign_target, b"keep").expect("write foreign extraction");
        discard_archive_extract_path(foreign_target.to_str().unwrap());
        assert!(foreign_target.exists());
        let _ = fs::remove_dir_all(foreign_root);
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
    fn archive_type_summary_counts_common_types() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "src/".to_string(),
            ("src".to_string(), "".to_string(), true, 0, 0, 0, false),
        );
        entries.insert(
            "src/main.rs".to_string(),
            (
                "main.rs".to_string(),
                "src/".to_string(),
                false,
                10,
                8,
                0,
                false,
            ),
        );
        entries.insert(
            "src/lib.rs".to_string(),
            (
                "lib.rs".to_string(),
                "src/".to_string(),
                false,
                10,
                8,
                0,
                false,
            ),
        );
        entries.insert(
            "README.md".to_string(),
            (
                "README.md".to_string(),
                "".to_string(),
                false,
                10,
                8,
                0,
                false,
            ),
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
                false,
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
                false,
            ),
        );

        assert_eq!(
            archive_project_summary(&entries).as_deref(),
            Some(".csproj, package.json")
        );
    }

    #[test]
    fn archive_largest_file_summary_is_bounded_and_sorted() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "small.txt".to_string(),
            (
                "small.txt".to_string(),
                "".to_string(),
                false,
                10,
                8,
                0,
                false,
            ),
        );
        entries.insert(
            "assets/large.bin".to_string(),
            (
                "large.bin".to_string(),
                "assets/".to_string(),
                false,
                4096,
                100,
                0,
                false,
            ),
        );
        entries.insert(
            "assets/medium.bin".to_string(),
            (
                "medium.bin".to_string(),
                "assets/".to_string(),
                false,
                2048,
                100,
                0,
                false,
            ),
        );
        entries.insert(
            "assets/tiny.bin".to_string(),
            (
                "tiny.bin".to_string(),
                "assets/".to_string(),
                false,
                1,
                1,
                0,
                false,
            ),
        );

        let summary = archive_largest_file_summary(&entries).expect("largest files");
        assert_eq!(
            summary,
            "assets/large.bin (4.00 KB), assets/medium.bin (2.00 KB), small.txt (10 B)"
        );
        assert!(!summary.contains("tiny.bin"));
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
    fn xlsx_merge_regions_preserve_spans() {
        let context = test_office_context();
        let regions = parse_xlsx_merge_regions(
            &context,
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
        let formats = parse_xlsx_style_number_formats(
            &context,
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

        assert_eq!(formats.first(), Some(&None));
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
            styles.first().and_then(|style| style.fill_color.as_deref()),
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
