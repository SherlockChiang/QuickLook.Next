//! Native preview providers for Text, Info, Archive, and Folder.
//!
//! These replace the equivalent .NET plugins with pure-Rust implementations callable directly
//! from the App via C ABI, bypassing the .NET plugin pipeline entirely.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use flate2::read::GzDecoder;
use image::GenericImageView;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;
use tar::Archive as TarArchive;
use zip::ZipArchive;

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
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OfficeLayoutItemDto {
    kind: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<String>,
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

// ── Text preview ─────────────────────────────────────────────────────────────

const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_TABLE_ROWS: usize = 500;
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
    make: Option<String>,
    model: Option<String>,
    date_time: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    orientation: Option<u16>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

pub fn render_image_metadata(path: &str) -> String {
    parse_jpeg_exif_metadata(path)
        .map(|metadata| to_json(&metadata))
        .unwrap_or_default()
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
    parse_exif_ifd(tiff, ifd0, endian, &mut metadata, &mut exif_ifd, &mut gps_ifd);
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
            0x010F => metadata.make = exif_ascii(tiff, entry, endian),
            0x0110 => metadata.model = exif_ascii(tiff, entry, endian),
            0x0112 => metadata.orientation = exif_u16_value(tiff, entry, endian),
            0x0132 | 0x9003 => {
                if metadata.date_time.is_none() {
                    metadata.date_time = exif_ascii(tiff, entry, endian);
                }
            }
            0x8769 => *exif_ifd = exif_u32_value(tiff, entry, endian).map(|v| v as usize),
            0x8825 => *gps_ifd = exif_u32_value(tiff, entry, endian).map(|v| v as usize),
            0xA002 => metadata.width = exif_u32_or_u16_value(tiff, entry, endian),
            0xA003 => metadata.height = exif_u32_or_u16_value(tiff, entry, endian),
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
    let mut lat = None;
    let mut lon = None;
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
            _ => {}
        }
    }

    metadata.latitude = signed_gps_coordinate(lat, lat_ref.as_deref(), "S");
    metadata.longitude = signed_gps_coordinate(lon, lon_ref.as_deref(), "W");
}

fn exif_ascii(tiff: &[u8], entry: usize, endian: u8) -> Option<String> {
    let bytes = exif_value_bytes(tiff, entry, endian)?;
    let text = String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string();
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

fn exif_gps_coordinate(tiff: &[u8], entry: usize, endian: u8) -> Option<f64> {
    if read_u16_endian(tiff, entry + 2, endian)? != 5 || read_u32_endian(tiff, entry + 4, endian)? < 3 {
        return None;
    }
    let offset = read_u32_endian(tiff, entry + 8, endian)? as usize;
    let degrees = exif_rational(tiff, offset, endian)?;
    let minutes = exif_rational(tiff, offset + 8, endian)?;
    let seconds = exif_rational(tiff, offset + 16, endian)?;
    Some(degrees + minutes / 60.0 + seconds / 3600.0)
}

fn exif_rational(tiff: &[u8], offset: usize, endian: u8) -> Option<f64> {
    let numerator = read_u32_endian(tiff, offset, endian)? as f64;
    let denominator = read_u32_endian(tiff, offset + 4, endian)? as f64;
    if denominator == 0.0 {
        return None;
    }
    Some(numerator / denominator)
}

fn signed_gps_coordinate(value: Option<f64>, reference: Option<&str>, negative_ref: &str) -> Option<f64> {
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
        (".ini", "code", "ini"),
        (".cfg", "code", "ini"),
        (".conf", "code", "ini"),
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

    // BOM-aware decode via encoding_rs
    let (text, _enc, _had_bom) = if bytes.len() >= 3 && &bytes[..3] == &[0xEF, 0xBB, 0xBF] {
        encoding_rs::UTF_8.decode(&bytes[3..])
    } else if bytes.len() >= 2 && &bytes[..2] == &[0xFF, 0xFE] {
        encoding_rs::UTF_16LE.decode(&bytes[2..])
    } else if bytes.len() >= 2 && &bytes[..2] == &[0xFE, 0xFF] {
        encoding_rs::UTF_16BE.decode(&bytes[2..])
    } else {
        encoding_rs::UTF_8.decode(&bytes)
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
            blocks.push(markdown_block("code", 0, code.trim_end_matches('\n'), &language));
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

fn markdown_inline(kind: &str, text: &str, url: &str, children: Vec<PreviewMarkdownInlineDto>) -> PreviewMarkdownInlineDto {
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
    if let Some(text) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
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
    let mut block = markdown_block(if ordered { "orderedList" } else { "unorderedList" }, 0, "", "");
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
        && cells
            .iter()
            .all(|cell| cell.trim().chars().all(|c| c == '-' || c == ':' || c.is_whitespace()) && cell.contains('-'))
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
                out.push(markdown_inline("strong", "", "", parse_markdown_inlines(inner)));
                i += end + 4;
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let inner = &after[..end];
                out.push(markdown_inline("emphasis", "", "", parse_markdown_inlines(inner)));
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    let label = &rest[1..close];
                    let url = &rest[close + 2..close + 2 + end];
                    out.push(markdown_inline("link", "", url, parse_markdown_inlines(label)));
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
            finish_table_row(
                &mut records,
                &mut row,
                &mut total_records,
                &mut is_partial,
            );
        } else if cell.chars().count() < MAX_TABLE_CELL_CHARS {
            cell.push(ch);
        } else {
            is_partial = true;
        }
    }

    if saw_any && (!cell.is_empty() || !row.is_empty()) {
        finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
        finish_table_row(
            &mut records,
            &mut row,
            &mut total_records,
            &mut is_partial,
        );
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
    row.iter().any(|cell| cell.chars().any(|ch| ch.is_alphabetic()))
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

/// Check if a file is text-like (extension known or a small UTF-8 printable header).
pub fn is_text(ext: &str, magic: &[u8]) -> bool {
    if known_text_formats()
        .iter()
        .any(|(e, _, _)| e.eq_ignore_ascii_case(ext))
    {
        return true;
    }
    is_probably_utf8_text(magic)
}

fn is_probably_utf8_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return false;
    }
    let printable = bytes
        .iter()
        .filter(|b| matches!(**b, b'\t' | b'\r' | b'\n' | 0x20..=0x7E) || **b >= 0x80)
        .count();
    printable * 100 / bytes.len().max(1) >= 90
}

// ── Office preview (OOXML / ODF lightweight extraction) ─────────────────────

const MAX_OFFICE_TEXT_CHARS: usize = 96 * 1024;
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

pub fn render_office(path: &str, _cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "docx" => render_docx(path),
        "xlsx" | "xlsm" => render_xlsx(path),
        "pptx" | "pptm" => render_pptx(path),
        "odt" | "ods" | "odp" => render_odf(path),
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

fn render_docx(path: &str) -> String {
    let filename = file_name(path);
    let mut zip = match open_zip(path) {
        Some(zip) => zip,
        None => return String::new(),
    };
    let media_entries = office_media_entries(&mut zip, &["word/media/"]);
    let xml = match read_zip_text(&mut zip, "word/document.xml", 16 * 1024 * 1024) {
        Some(xml) => xml,
        None => return office_error_json(path, "DOCX", "word/document.xml not found"),
    };
    let body = extract_wordprocessing_text(&xml);
    let layout = build_docx_layout(&mut zip, &body, &media_entries);
    let mut text = format!("Name: {filename}\nKind: Word document\n");
    append_office_media_summary(&mut text, &media_entries);
    let text = if body.trim().is_empty() {
        text.push_str("Status: no extractable text");
        text
    } else {
        text.push('\n');
        text.push_str(&truncate_preview_text(&body));
        text
    };
    office_preview_json_with_layout(path, "DOCX", text, "plain", "text", layout)
}

fn render_pptx(path: &str) -> String {
    let filename = file_name(path);
    let mut zip = match open_zip(path) {
        Some(zip) => zip,
        None => return String::new(),
    };
    let media_entries = office_media_entries(&mut zip, &["ppt/media/"]);
    let mut slides = Vec::new();
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(xml) = read_zip_text(&mut zip, &name, 8 * 1024 * 1024) else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };
        let text = extract_ppt_text(&xml);
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
    let layout = build_pptx_layout(&mut zip);
    office_preview_json_with_layout(
        path,
        "PowerPoint presentation",
        text,
        "plain",
        "text",
        layout,
    )
}

fn render_xlsx(path: &str) -> String {
    let filename = file_name(path);
    let mut zip = match open_zip(path) {
        Some(zip) => zip,
        None => return String::new(),
    };
    let media_entries = office_media_entries(&mut zip, &["xl/media/"]);
    let shared_strings = read_zip_text(&mut zip, "xl/sharedStrings.xml", 16 * 1024 * 1024)
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();

    let mut sections = Vec::new();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(xml) = read_zip_text(&mut zip, &name, 16 * 1024 * 1024) else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };
        let rows = parse_worksheet_rows(&xml, &shared_strings);
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
    let layout = build_xlsx_layout(&mut zip, &shared_strings);
    office_preview_json_with_layout(path, "Excel workbook", text, "code", "text", layout)
}

fn render_odf(path: &str) -> String {
    let filename = file_name(path);
    let mut zip = match open_zip(path) {
        Some(zip) => zip,
        None => return String::new(),
    };
    let xml = match read_zip_text(&mut zip, "content.xml", 16 * 1024 * 1024) {
        Some(xml) => xml,
        None => return office_error_json(path, "OpenDocument", "content.xml not found"),
    };
    let body = extract_wordprocessing_text(&xml);
    office_text_json(
        path,
        "OpenDocument",
        format!(
            "Name: {filename}\nKind: OpenDocument\n\n{}",
            truncate_preview_text(&body)
        ),
    )
}

fn open_zip(path: &str) -> Option<ZipArchive<fs::File>> {
    let file = fs::File::open(path).ok()?;
    ZipArchive::new(file).ok()
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

    for i in 0..zip.len() {
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

fn read_zip_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max_size: u64,
) -> Option<Vec<u8>> {
    if let Ok(mut entry) = zip.by_name(name) {
        if entry.size() > max_size {
            return None;
        }
        return read_limited_to_end(&mut entry, max_size);
    }

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).ok()?;
        if !entry.name().replace('\\', "/").eq_ignore_ascii_case(name) {
            continue;
        }
        if entry.size() > max_size {
            return None;
        }
        return read_limited_to_end(&mut entry, max_size);
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

fn office_media_entries<R: Read + Seek>(zip: &mut ZipArchive<R>, roots: &[&str]) -> Vec<String> {
    let mut entries = Vec::new();
    for i in 0..zip.len() {
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
    entries
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

fn build_pptx_layout(zip: &mut ZipArchive<fs::File>) -> Option<OfficeLayoutDto> {
    let presentation_xml = read_zip_text(zip, "ppt/presentation.xml", 4 * 1024 * 1024);
    let (slide_width, slide_height) = presentation_xml
        .as_deref()
        .and_then(parse_ppt_slide_size)
        .unwrap_or((960.0, 540.0));

    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let slide_name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(slide_xml) = read_zip_text(zip, &slide_name, 8 * 1024 * 1024) else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };

        let rels_name = format!("ppt/slides/_rels/slide{slide_idx}.xml.rels");
        let rels = read_zip_text(zip, &rels_name, 2 * 1024 * 1024)
            .map(|xml| parse_relationships(&xml))
            .unwrap_or_default();
        let background_color = parse_ppt_slide_background(&slide_xml);
        let items = parse_ppt_slide_items(zip, "ppt/slides/", &slide_xml, &rels, &mut image_budget);
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
        return None;
    }

    Some(OfficeLayoutDto {
        layout_kind: "presentation".to_string(),
        width: slide_width,
        height: slide_height,
        pages,
    })
}

fn build_xlsx_layout(
    zip: &mut ZipArchive<fs::File>,
    shared_strings: &[String],
) -> Option<OfficeLayoutDto> {
    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let sheet_name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(sheet_xml) = read_zip_text(zip, &sheet_name, 16 * 1024 * 1024) else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };

        let metrics = parse_xlsx_sheet_metrics(&sheet_xml);
        let merge_regions = parse_xlsx_merge_regions(&sheet_xml);
        let (freeze_rows, freeze_columns) = parse_xlsx_freeze_pane(&sheet_xml);
        let mut cells =
            parse_worksheet_layout_cells(&sheet_xml, shared_strings, &metrics, &merge_regions);
        let mut items =
            parse_xlsx_sheet_images(zip, sheet_idx, &sheet_xml, &metrics, &mut image_budget);
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
        return None;
    }

    let width = pages.iter().map(|p| p.width).fold(0.0, f64::max);
    let height = pages.first().map(|p| p.height).unwrap_or(420.0);
    Some(OfficeLayoutDto {
        layout_kind: "workbook".to_string(),
        width,
        height,
        pages,
    })
}

fn build_docx_layout(
    zip: &mut ZipArchive<fs::File>,
    body: &str,
    media_entries: &[String],
) -> Option<OfficeLayoutDto> {
    let page_width = 760.0;
    let page_height = 980.0;
    let margin = 58.0;
    let mut pages = Vec::new();
    let mut items = Vec::new();
    let mut page_index = 1usize;
    let mut y = margin;

    for paragraph in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let clipped = paragraph.chars().take(420).collect::<String>();
        let line_count = (clipped.chars().count() as f64 / 72.0).ceil().clamp(1.0, 5.0);
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
            text: Some(clipped),
            shape: None,
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
        let Some(bytes) = read_zip_bytes(zip, entry, MAX_OFFICE_INLINE_IMAGE_BYTES) else {
            continue;
        };
        image_budget = image_budget.saturating_sub(1);
        items.push(OfficeLayoutItemDto {
            kind: "image".to_string(),
            x: margin,
            y,
            width: 260.0,
            height: 170.0,
            text: None,
            shape: None,
            fill_color: None,
            stroke_color: None,
            image_name: Some(entry.rsplit('/').next().unwrap_or(entry.as_str()).to_string()),
            mime_type: image_mime_type(&lower).map(str::to_string),
            image_base64: Some(base64_encode(&bytes)),
        });
        y += 188.0;
    }

    if !items.is_empty() || pages.is_empty() {
        push_docx_page(&mut pages, page_index, page_width, page_height, items);
    }

    if pages.iter().all(|page| page.items.is_empty()) {
        return None;
    }

    Some(OfficeLayoutDto {
        layout_kind: "document".to_string(),
        width: page_width,
        height: page_height,
        pages,
    })
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

fn parse_ppt_slide_size(xml: &str) -> Option<(f64, f64)> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "sldsz" {
                    let cx = attr_f64(&e, "cx")?;
                    let cy = attr_f64(&e, "cy")?;
                    return Some((
                        (cx / OFFICE_EMUS_PER_DIP).max(320.0),
                        (cy / OFFICE_EMUS_PER_DIP).max(180.0),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    None
}

fn parse_ppt_slide_background(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut in_background = false;
    let mut depth = 0usize;

    loop {
        match reader.read_event() {
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
                        return office_color_from_element(&e);
                    }
                }
            }
            Ok(Event::Empty(e)) if in_background => {
                let local = local_xml_name(e.name().as_ref());
                if local == "srgbclr" || local == "schemeclr" {
                    if let Some(color) = office_color_from_element(&e) {
                        return Some(color);
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
    None
}

fn parse_ppt_slide_items<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    xml: &str,
    rels: &BTreeMap<String, String>,
    image_budget: &mut usize,
) -> Vec<OfficeLayoutItemDto> {
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
    let mut preset_shape: Option<String> = None;
    let mut fill_color: Option<String> = None;
    let mut stroke_color: Option<String> = None;
    let mut color_target = "";

    loop {
        match reader.read_event() {
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
                    preset_shape = None;
                    fill_color = None;
                    stroke_color = None;
                    color_target = "";
                    continue;
                }
                if in_shape {
                    shape_depth += 1;
                    if local == "t" {
                        in_text = true;
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
                    } else if local == "solidfill" {
                        color_target = "fill";
                    } else if local == "ln" {
                        color_target = "stroke";
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
                                text: (!normalized.is_empty()).then_some(normalized),
                                shape: preset_shape.clone(),
                                fill_color: fill_color.clone(),
                                stroke_color: stroke_color.clone(),
                                image_name: None,
                                mime_type: None,
                                image_base64: None,
                            });
                        }
                    } else if let Some(item) = image_item_from_relationship(
                        zip,
                        base_dir,
                        rels,
                        &rel_id,
                        x,
                        y,
                        width,
                        height,
                        image_budget,
                    ) {
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
    items
}

fn extract_ppt_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;

    loop {
        match reader.read_event() {
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

    normalize_preview_lines(&out)
}

fn parse_worksheet_layout_cells(
    xml: &str,
    shared_strings: &[String],
    metrics: &XlsxSheetMetrics,
    merge_regions: &BTreeMap<(usize, usize), XlsxMergeRegion>,
) -> Vec<OfficeCellDto> {
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
    let mut cell_type = String::new();
    let mut cell_value = String::new();

    loop {
        match reader.read_event() {
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
                                number_format: None,
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
    cells
}

fn parse_xlsx_sheet_images<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    sheet_idx: usize,
    sheet_xml: &str,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> Vec<OfficeLayoutItemDto> {
    let Some(drawing_rid) = parse_worksheet_drawing_rid(sheet_xml) else {
        return Vec::new();
    };
    let sheet_rels_name = format!("xl/worksheets/_rels/sheet{sheet_idx}.xml.rels");
    let sheet_rels = read_zip_text(zip, &sheet_rels_name, 2 * 1024 * 1024)
        .map(|xml| parse_relationships(&xml))
        .unwrap_or_default();
    let Some(drawing_target) = sheet_rels.get(&drawing_rid) else {
        return Vec::new();
    };
    let drawing_path = normalize_zip_target("xl/worksheets/", drawing_target);
    let Some(drawing_xml) = read_zip_text(zip, &drawing_path, 4 * 1024 * 1024) else {
        return Vec::new();
    };
    let drawing_rels_path = rels_path_for_part(&drawing_path);
    let drawing_rels = read_zip_text(zip, &drawing_rels_path, 2 * 1024 * 1024)
        .map(|xml| parse_relationships(&xml))
        .unwrap_or_default();
    let base = part_base_dir(&drawing_path);
    parse_xlsx_drawing_items(
        zip,
        &base,
        &drawing_xml,
        &drawing_rels,
        metrics,
        image_budget,
    )
}

fn parse_worksheet_drawing_rid(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "drawing" {
                    return attr_value(&e, "id");
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    None
}

fn parse_xlsx_drawing_items<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    xml: &str,
    rels: &BTreeMap<String, String>,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> Vec<OfficeLayoutItemDto> {
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

    loop {
        match reader.read_event() {
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
                        zip,
                        base_dir,
                        rels,
                        &rel_id,
                        x,
                        y,
                        width,
                        height,
                        image_budget,
                    ) {
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
    items
}

fn image_item_from_relationship<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    rels: &BTreeMap<String, String>,
    rel_id: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    image_budget: &mut usize,
) -> Option<OfficeLayoutItemDto> {
    if rel_id.is_empty() || *image_budget == 0 || width <= 1.0 || height <= 1.0 {
        return None;
    }
    let target = rels.get(rel_id)?;
    let path = normalize_zip_target(base_dir, target);
    let lower = path.to_ascii_lowercase();
    if !is_supported_zip_image_name(&lower) {
        return None;
    }
    let bytes = read_zip_bytes(zip, &path, MAX_OFFICE_INLINE_IMAGE_BYTES)?;
    *image_budget = (*image_budget).saturating_sub(1);
    Some(OfficeLayoutItemDto {
        kind: "image".to_string(),
        x,
        y,
        width,
        height,
        text: None,
        shape: None,
        fill_color: None,
        stroke_color: None,
        image_name: Some(path.rsplit('/').next().unwrap_or(path.as_str()).to_string()),
        mime_type: image_mime_type(&lower).map(str::to_string),
        image_base64: Some(base64_encode(&bytes)),
    })
}

fn parse_relationships(xml: &str) -> BTreeMap<String, String> {
    let mut reader = Reader::from_str(xml);
    let mut rels = BTreeMap::new();
    loop {
        match reader.read_event() {
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
    rels
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

fn parse_xlsx_sheet_metrics(xml: &str) -> XlsxSheetMetrics {
    let mut reader = Reader::from_str(xml);
    let mut metrics = XlsxSheetMetrics::default();

    loop {
        match reader.read_event() {
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

    metrics
}

fn parse_xlsx_freeze_pane(xml: &str) -> (Option<usize>, Option<usize>) {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "pane" {
                    let state = attr_value(&e, "state").unwrap_or_default();
                    if state != "frozen" && state != "frozenSplit" {
                        return (None, None);
                    }
                    let rows = attr_f64(&e, "ysplit").map(|value| value.max(0.0) as usize);
                    let columns = attr_f64(&e, "xsplit").map(|value| value.max(0.0) as usize);
                    return (rows.filter(|value| *value > 0), columns.filter(|value| *value > 0));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    (None, None)
}

fn parse_xlsx_merge_regions(xml: &str) -> BTreeMap<(usize, usize), XlsxMergeRegion> {
    let mut reader = Reader::from_str(xml);
    let mut regions = BTreeMap::new();

    loop {
        match reader.read_event() {
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

    regions
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
    (0..col.min(64)).map(|idx| xlsx_col_width(metrics, idx)).sum()
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

fn extract_wordprocessing_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;

    loop {
        match reader.read_event() {
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
                } else if local == "p" || local == "tr" {
                    if paragraph_had_text && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    paragraph_had_text = false;
                } else if local == "tab" {
                    out.push('\t');
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

    normalize_preview_lines(&out)
}

fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;

    loop {
        match reader.read_event() {
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

    values
}

fn parse_worksheet_rows(xml: &str, shared_strings: &[String]) -> Vec<Vec<String>> {
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

    loop {
        match reader.read_event() {
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

    rows
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
    let digits = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X"));
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
        text: Some(body.unwrap_or_else(|| format!(
            "Name: {filename}\nKind: {kind}\nSize: {}\nModified: {}",
            format_number(size),
            format_timestamp(modified_unix)
        ))),
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
            text.push_str(&format!("\nText encoding: {}", sqlite_encoding_name(encoding)));
        }
        if let Some(user_version) = read_u32_be(&bytes, 60) {
            text.push_str(&format!("\nUser version: {}", user_version));
        }
        if let Some(app_id) = read_u32_be(&bytes, 68) {
            text.push_str(&format!("\nApplication ID: 0x{app_id:08X}"));
        }
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
    } else {
        let content = String::from_utf8_lossy(&bytes);
        let headers = parse_mail_headers(&content);
        text.push_str("\nFormat: RFC 5322 message");
        for key in ["From", "To", "Cc", "Subject", "Date", "Content-Type"] {
            if let Some(value) = headers.iter().find_map(|(k, v)| k.eq_ignore_ascii_case(key).then_some(v)) {
                text.push_str(&format!("\n{key}: {}", value.trim()));
            }
        }
        if filename.to_ascii_lowercase().ends_with(".mbox") {
            let count = content.lines().filter(|line| line.starts_with("From ")).count();
            text.push_str(&format!("\nMailbox messages observed: {}", format_number(count as i64)));
        }
    }
    generic_info_json(path, "mail", size, modified_unix, Some(text))
}

fn render_chm_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, 128).unwrap_or_default();
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
    } else {
        text.push_str("\nFormat: CHM-like help file");
    }
    generic_info_json(path, "chm", size, modified_unix, Some(text))
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
            text.push_str(&format!("\nTimestamp: {}", format_timestamp(timestamp as i64)));
        }
        if let Some(flags) = read_u64(&bytes, 24) {
            text.push_str(&format!("\nFlags: 0x{flags:016X}"));
        }
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
    let bytes = read_file_prefix(path, 64).unwrap_or_default();
    let mut text = base_info_text(filename, kind, size, modified_unix);
    text.push_str(&format!("\nContainer: {}", media_container_name(path, &bytes)));
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
    family: String,
    subfamily: String,
    full_name: String,
    postscript_name: String,
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
            ..Default::default()
        });
    }
    if bytes.starts_with(b"wOF2") {
        return Some(FontSummary {
            format: "WOFF2 font",
            tables: read_u16_be(bytes, 12).unwrap_or(0),
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
            _ => {}
        }
    }
}

fn decode_font_name(platform: u16, bytes: &[u8]) -> String {
    if platform == 0 || platform == 3 {
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units).trim_matches('\0').trim().to_string()
    } else {
        String::from_utf8_lossy(bytes).trim_matches('\0').trim().to_string()
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
    let image_size = read_u32(bytes, opt + 56).unwrap_or(0);
    let subsystem = read_u16(bytes, opt + 68).unwrap_or(0);

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
    })
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

    let container = read_zip_text(
        &mut zip,
        "META-INF/container.xml",
        MAX_EBOOK_XML_BYTES,
    );
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
        let Some(chapter_xml) = read_zip_text(&mut zip, &chapter_path, MAX_EBOOK_CHAPTER_BYTES) else {
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
                set_epub_metadata(&mut opf, &current_meta, &String::from_utf8_lossy(e.as_ref()));
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
    opf.manifest.insert(id, EpubManifestItem { href, media_type });
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
        format!("## {}\n\n{}", markdown_escape_line(fallback_title), out.trim())
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
                    "section" if in_body => {
                        flush_ebook_block(&mut markdown, &mut current_block, 0, &mut saw_body_heading)
                    }
                    "title" if in_body => {
                        flush_ebook_block(&mut markdown, &mut current_block, 0, &mut saw_body_heading);
                        current_meta = "body-title".to_string();
                    }
                    "p" if in_body => {
                        flush_ebook_block(&mut markdown, &mut current_block, 0, &mut saw_body_heading)
                    }
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
                        flush_ebook_block(&mut markdown, &mut current_block, 0, &mut saw_body_heading);
                        in_body = false;
                    }
                    "title" if current_meta == "body-title" => {
                        flush_ebook_block(&mut markdown, &mut current_block, 2, &mut saw_body_heading);
                        current_meta.clear();
                    }
                    "p" if in_body => {
                        flush_ebook_block(&mut markdown, &mut current_block, 0, &mut saw_body_heading)
                    }
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
const MAX_ARCHIVE_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;

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
pub fn render_archive(path: &str, _cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    let lower = path.to_ascii_lowercase();
    if is_package_path(&lower) {
        return render_package(path);
    }
    if TAR_GZ_EXTS.iter().any(|e| lower.ends_with(e)) {
        return render_tar_gz_archive(path);
    }
    if TAR_EXTS.iter().any(|e| lower.ends_with(e)) {
        return render_tar_archive(path);
    }
    if GZ_EXTS.iter().any(|e| lower.ends_with(e)) && !lower.ends_with(".tar.gz") {
        return render_gzip_member(path);
    }
    render_zip_archive(path)
}

pub fn extract_archive_entry_to_temp(archive_path: &str, entry_path: &str) -> Option<String> {
    let lower = archive_path.to_ascii_lowercase();
    if TAR_EXTS.iter().any(|e| lower.ends_with(e))
        || TAR_GZ_EXTS.iter().any(|e| lower.ends_with(e))
        || (GZ_EXTS.iter().any(|e| lower.ends_with(e)) && !lower.ends_with(".tar.gz"))
    {
        return None;
    }

    let normalized = normalize_archive_entry_path(entry_path)?;
    let file = fs::File::open(archive_path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name(&normalized).ok()?;
    if entry.is_dir() || entry.size() > MAX_ARCHIVE_EXTRACT_BYTES {
        return None;
    }

    let target = archive_extract_target_path(archive_path, &normalized)?;
    if target.exists() {
        if let Ok(meta) = fs::metadata(&target) {
            if meta.len() == entry.size() {
                return target.to_str().map(|s| s.to_string());
            }
        }
    }

    let bytes = read_limited_to_end(&mut entry, MAX_ARCHIVE_EXTRACT_BYTES)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    fs::write(&target, bytes).ok()?;
    target.to_str().map(|s| s.to_string())
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

fn archive_extract_target_path(archive_path: &str, entry_path: &str) -> Option<std::path::PathBuf> {
    let mut hasher = DefaultHasher::new();
    archive_path.hash(&mut hasher);
    if let Ok(meta) = fs::metadata(archive_path) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                duration.as_secs().hash(&mut hasher);
            }
        }
    }
    let archive_hash = hasher.finish();
    let mut path = std::env::temp_dir();
    path.push("QuickLookNext");
    path.push("archive-preview");
    path.push(format!("{archive_hash:016x}"));
    for part in entry_path.split('/') {
        path.push(sanitize_temp_path_component(part));
    }
    Some(path)
}

fn sanitize_temp_path_component(part: &str) -> String {
    let sanitized = part
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "_".to_string()
    } else {
        sanitized
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

fn render_package(path: &str) -> String {
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

    for i in 0..zip.len() {
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
    for i in 0..zip.len() {
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
    for (path_score, name) in candidates.into_iter().take(24) {
        let Ok(mut entry) = zip.by_name(&name) else {
            continue;
        };
        let Some(bytes) = read_limited_to_end(&mut entry, MAX_OFFICE_MEDIA_BYTES) else {
            continue;
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
        let score =
            package_icon_candidate_score(&normalized_name) + manifest_icon_candidate_score(&normalized_name, &manifest_icons);
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
        let normalized = path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase();
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
    let lower = name.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase();
    if manifest_icons.iter().any(|candidate| candidate == &lower) {
        return 320;
    }

    let Some((stem, _)) = lower.rsplit_once('.') else {
        return 0;
    };
    manifest_icons
        .iter()
        .filter_map(|candidate| candidate.rsplit_once('.').map(|(candidate_stem, _)| candidate_stem))
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

fn render_zip_archive(path: &str) -> String {
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

    for i in 0..zip.len() {
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

fn render_tar_archive(path: &str) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    render_tar_entries(path, "archive", file)
}

fn render_tar_gz_archive(path: &str) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    render_tar_entries(path, "archive", GzDecoder::new(file))
}

fn render_tar_entries<R: Read>(path: &str, kind: &str, reader: R) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let mut archive = TarArchive::new(reader);
    let mut entries: BTreeMap<String, (String, String, bool, i64, i64, i64)> = BTreeMap::new();
    let mut file_count = 0u64;
    let mut uncompressed = 0i64;
    let mut seen = 0usize;
    let mut partial = false;

    let archive_entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    for entry in archive_entries.flatten() {
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
pub fn render_folder(path: &str, _cancel_cb: Option<extern "C" fn() -> bool>) -> String {
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
    use std::io::Cursor;

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
    fn office_text_truncation_is_char_boundary_safe() {
        let text = "中".repeat(MAX_OFFICE_TEXT_CHARS + 1);
        let truncated = truncate_preview_text(&text);

        assert!(truncated.starts_with(&"中".repeat(8)));
        assert!(truncated.contains("[Preview truncated at"));
    }

    #[test]
    fn ppt_text_extraction_preserves_paragraphs_tabs_and_breaks() {
        let text = extract_ppt_text(
            r#"<p:sld xmlns:p="p" xmlns:a="a">
                <p:sp><p:txBody>
                    <a:p><a:r><a:t>Title</a:t></a:r></a:p>
                    <a:p><a:r><a:t>Left</a:t></a:r><a:tab/><a:r><a:t>Right</a:t></a:r></a:p>
                    <a:p><a:r><a:t>Line 1</a:t></a:r><a:br/><a:r><a:t>Line 2</a:t></a:r></a:p>
                </p:txBody></p:sp>
            </p:sld>"#,
        );

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
        let items = parse_ppt_slide_items(
            &mut zip,
            "ppt/slides/",
            r#"<p:sld xmlns:p="p" xmlns:a="a">
                <p:sp>
                    <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></p:spPr>
                    <p:txBody>
                        <a:p><a:r><a:t>First</a:t></a:r></a:p>
                        <a:p><a:r><a:t>Second</a:t></a:r></a:p>
                    </p:txBody>
                </p:sp>
            </p:sld>"#,
            &BTreeMap::new(),
            &mut image_budget,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text.as_deref(), Some("First\nSecond"));
    }

    #[test]
    fn jpeg_exif_metadata_reads_core_fields_and_gps() {
        let mut tiff = vec![0u8; 8 + 2 + 6 * 12 + 4];
        tiff[0..4].copy_from_slice(&[b'I', b'I', 42, 0]);
        write_le_u32(&mut tiff, 4, 8);
        write_le_u16(&mut tiff, 8, 6);

        let ifd0_entries = 10;
        write_ascii_entry(&mut tiff, ifd0_entries, 0, 0x010F, "Acme");
        write_ascii_entry(&mut tiff, ifd0_entries, 1, 0x0110, "PhoneCam");
        write_short_entry(&mut tiff, ifd0_entries, 2, 0x0112, 6);

        let exif_ifd = tiff.len() as u32;
        write_long_entry(&mut tiff, ifd0_entries, 3, 0x8769, exif_ifd);
        append_exif_ifd(&mut tiff);

        let gps_ifd = tiff.len() as u32;
        write_long_entry(&mut tiff, ifd0_entries, 4, 0x8825, gps_ifd);
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
        assert!((metadata.latitude.unwrap() - 31.2304).abs() < 0.0001);
        assert!((metadata.longitude.unwrap() - 121.4737).abs() < 0.0001);

        let path = std::env::temp_dir().join("quicklook-next-exif-smoke.jpg");
        fs::write(&path, &jpeg).expect("write temp jpeg");
        let from_file = parse_jpeg_exif_metadata(path.to_str().unwrap()).expect("file exif metadata");
        let _ = fs::remove_file(path);
        assert_eq!(from_file.make.as_deref(), Some("Acme"));
    }

    fn append_exif_ifd(tiff: &mut Vec<u8>) {
        let offset = tiff.len();
        tiff.resize(offset + 2 + 3 * 12 + 4, 0);
        write_le_u16(tiff, offset, 3);
        let entries = offset + 2;
        write_ascii_entry(tiff, entries, 0, 0x9003, "2026:07:05 13:04:47");
        write_long_entry(tiff, entries, 1, 0xA002, 4032);
        write_long_entry(tiff, entries, 2, 0xA003, 3024);
    }

    fn append_gps_ifd(tiff: &mut Vec<u8>) {
        let offset = tiff.len();
        tiff.resize(offset + 2 + 4 * 12 + 4, 0);
        write_le_u16(tiff, offset, 4);
        let entries = offset + 2;
        write_ascii_entry(tiff, entries, 0, 1, "N");
        write_rational3_entry(tiff, entries, 1, 2, [(31, 1), (13, 1), (4944, 100)]);
        write_ascii_entry(tiff, entries, 2, 3, "E");
        write_rational3_entry(tiff, entries, 3, 4, [(121, 1), (28, 1), (2532, 100)]);
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

    fn write_rational3_entry(tiff: &mut Vec<u8>, entries: usize, index: usize, tag: u16, values: [(u32, u32); 3]) {
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

    fn write_le_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn xlsx_merge_regions_preserve_spans() {
        let regions = parse_xlsx_merge_regions(
            r#"<worksheet><mergeCells><mergeCell ref="B2:D4"/></mergeCells></worksheet>"#,
        );

        let region = regions.get(&(1, 1)).expect("merged region");
        assert_eq!(region.row_span, 3);
        assert_eq!(region.column_span, 3);
        assert!(is_inside_non_origin_merge(&regions, 2, 2));
        assert!(!is_inside_non_origin_merge(&regions, 1, 1));
    }

    #[test]
    fn xlsx_freeze_pane_reads_split_counts() {
        let (rows, columns) = parse_xlsx_freeze_pane(
            r#"<worksheet><sheetViews><sheetView><pane xSplit="2" ySplit="1" state="frozen"/></sheetView></sheetViews></worksheet>"#,
        );

        assert_eq!(rows, Some(1));
        assert_eq!(columns, Some(2));
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
        let (blocks, partial) = parse_markdown_blocks("# 中文标题\n\n这是一个含有 **加粗** 的中文字符串。");
        assert!(!partial);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "heading");
        assert_eq!(blocks[0].text, "中文标题");
        assert_eq!(blocks[1].kind, "paragraph");
        assert!(blocks[1].inlines.iter().any(|i| i.kind == "strong"));
    }

    #[test]
    fn markdown_parser_emits_lists_quotes_and_code() {
        let (blocks, partial) = parse_markdown_blocks("> note\n\n- one\n- two\n\n```rs\nfn main() {}\n```");

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
        assert_eq!(blocks[0].table_headers, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(blocks[0].table_rows[0], vec!["1".to_string(), "2".to_string()]);
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
        assert_eq!(ebook_item_label("Text/chapter-01_intro.xhtml"), "chapter 01 intro");
    }

    #[test]
    fn font_summary_detects_woff_tables() {
        let mut bytes = vec![0u8; 44];
        bytes[0..4].copy_from_slice(b"wOFF");
        bytes[12..14].copy_from_slice(&3u16.to_be_bytes());

        let summary = parse_font_summary(&bytes).expect("woff summary");

        assert_eq!(summary.format, "WOFF font");
        assert_eq!(summary.tables, 3);
    }

    #[test]
    fn mail_header_parser_unfolds_continuations() {
        let headers = parse_mail_headers("Subject: hello\r\n world\r\nFrom: a@example.test\r\n\r\nbody");

        assert_eq!(headers[0], ("Subject".to_string(), "hello world".to_string()));
        assert_eq!(headers[1], ("From".to_string(), "a@example.test".to_string()));
    }

    #[test]
    fn elf_summary_detects_64_bit_little_endian() {
        let mut bytes = vec![0u8; 64];
        bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x401000u64.to_le_bytes());

        let mut text = String::new();
        append_elf_summary(&mut text, &bytes);

        assert!(text.contains("ELF64"));
        assert!(text.contains("x86-64"));
        assert!(text.contains("0x0000000000401000"));
    }
}
