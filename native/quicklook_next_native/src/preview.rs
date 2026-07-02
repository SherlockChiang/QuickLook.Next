//! Native preview providers for Text, Info, Archive, and Folder.
//!
//! These replace the equivalent .NET plugins with pure-Rust implementations callable directly
//! from the App via C ABI, bypassing the .NET plugin pipeline entirely.

use std::collections::BTreeMap;
use std::fs;
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

fn read_text_preview_bytes(path: &str) -> Option<(Vec<u8>, bool)> {
    let file = fs::File::open(path).ok()?;
    let mut reader = file.take((MAX_TEXT_BYTES + 1) as u64);
    let mut bytes = Vec::with_capacity(64 * 1024);
    reader.read_to_end(&mut bytes).ok()?;

    let truncated = bytes.len() > MAX_TEXT_BYTES;
    if truncated {
        bytes.truncate(MAX_TEXT_BYTES);
    }
    Some((bytes, truncated))
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
    })
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

pub fn render_office(path: &str) -> String {
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
    office_text_json(path, "DOCX", text)
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
        let text = extract_wordprocessing_text(&xml);
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
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).ok()?;
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
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).ok()?;
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
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).ok()?;
        return Some(bytes);
    }

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).ok()?;
        if !entry.name().replace('\\', "/").eq_ignore_ascii_case(name) {
            continue;
        }
        if entry.size() > max_size {
            return None;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).ok()?;
        return Some(bytes);
    }

    None
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
        let items = parse_ppt_slide_items(zip, "ppt/slides/", &slide_xml, &rels, &mut image_budget);
        pages.push(OfficePageDto {
            title: format!("Slide {slide_idx}"),
            index: slide_idx,
            width: slide_width,
            height: slide_height,
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

        let mut cells = parse_worksheet_layout_cells(&sheet_xml, shared_strings);
        let mut items = parse_xlsx_sheet_images(zip, sheet_idx, &sheet_xml, &mut image_budget);
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
                    continue;
                }
                if in_shape {
                    shape_depth += 1;
                    if local == "t" {
                        in_text = true;
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
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
                } else if local == "br" && shape_kind == "text" {
                    text.push('\n');
                }
            }
            Ok(Event::End(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                }
                shape_depth = shape_depth.saturating_sub(1);
                if shape_depth == 0 {
                    if shape_kind == "text" {
                        let normalized = normalize_preview_lines(&text);
                        if !normalized.is_empty() && width > 2.0 && height > 2.0 {
                            items.push(OfficeLayoutItemDto {
                                kind: "text".to_string(),
                                x,
                                y,
                                width,
                                height,
                                text: Some(normalized),
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
                text.push_str(&xml_unescape_bytes(e.as_ref()));
            }
            Ok(Event::CData(e)) if in_shape && in_text => {
                text.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    items
}

fn parse_worksheet_layout_cells(xml: &str, shared_strings: &[String]) -> Vec<OfficeCellDto> {
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
                        if !value.trim().is_empty() && row_index < MAX_OFFICE_ROWS && cell_col < 32
                        {
                            cells.push(OfficeCellDto {
                                row: row_index,
                                column: cell_col,
                                text: clean_table_cell(&value),
                                x: cell_col as f64 * XLSX_CELL_WIDTH,
                                y: row_index as f64 * XLSX_ROW_HEIGHT,
                                width: XLSX_CELL_WIDTH,
                                height: XLSX_ROW_HEIGHT,
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
    parse_xlsx_drawing_items(zip, &base, &drawing_xml, &drawing_rels, image_budget)
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
                    let x = from_col as f64 * XLSX_CELL_WIDTH;
                    let y = from_row as f64 * XLSX_ROW_HEIGHT;
                    let width = if to_col > from_col {
                        (to_col - from_col) as f64 * XLSX_CELL_WIDTH
                    } else {
                        ext_w.max(140.0)
                    };
                    let height = if to_row > from_row {
                        (to_row - from_row) as f64 * XLSX_ROW_HEIGHT
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
    if text.len() <= MAX_OFFICE_TEXT_CHARS {
        text.to_string()
    } else {
        format!(
            "{}\n\n[Preview truncated at {} characters]",
            &text[..MAX_OFFICE_TEXT_CHARS],
            MAX_OFFICE_TEXT_CHARS
        )
    }
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
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
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
        text: Some(format!(
            "Name: {filename}\nKind: {kind}\nSize: {}\nModified: {}",
            format_number(size),
            format_timestamp(modified_unix)
        )),
        office_layout: None,
        listing: None,
    })
}

// ── Executable preview ──────────────────────────────────────────────────────

pub fn render_executable(path: &str) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    let meta = fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
    let modified_unix = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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

// ── Archive preview ──────────────────────────────────────────────────────────

const MAX_ARCHIVE_ENTRIES: usize = 5000;

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
    ".epub",
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
pub fn render_archive(path: &str) -> String {
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
        if lower_name == "appxmanifest.xml" && entry.size() <= 2 * 1024 * 1024 {
            has_manifest = true;
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            if entry.read_to_end(&mut bytes).is_ok() {
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
    })
}

#[derive(Default)]
struct AppxManifestSummary {
    name: Option<String>,
    version: Option<String>,
    publisher: Option<String>,
    display_name: Option<String>,
    executable: Option<String>,
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
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
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

    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).ok()?;
        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let score = package_icon_candidate_score(&normalized_name);
        if score > 0 && entry.size() <= 8 * 1024 * 1024 {
            candidates.push((score, raw_name));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    for (path_score, name) in candidates.into_iter().take(32) {
        let mut entry = zip.by_name(&name).ok()?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
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
            root_path: String::new(),
            listing_kind: "archive".to_string(),
            summary,
            is_partial: partial,
            items,
        }),
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
        "epub" => "EPUB File",
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
pub fn render_folder(path: &str) -> String {
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
