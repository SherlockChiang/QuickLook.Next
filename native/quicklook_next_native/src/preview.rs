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
    listing: Option<PreviewListingDto>,
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

    let mut bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return String::new(),
    };

    let truncated = bytes.len() > MAX_TEXT_BYTES;
    if truncated {
        bytes.truncate(MAX_TEXT_BYTES);
    }

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
        text.push_str(&format!("\n\n[Preview truncated at {} bytes]", MAX_TEXT_BYTES));
    }

    let kind = if format == "markdown" { "markdown" } else { "text" };
    to_json(&PreviewReadyDto {
        kind: kind.to_string(),
        title: filename.to_string(),
        format: Some(format.to_string()),
        language: Some(language.to_string()),
        text: Some(text),
        listing: None,
    })
}

/// Check if a file is text-like (extension known or a small UTF-8 printable header).
pub fn is_text(ext: &str, magic: &[u8]) -> bool {
    if known_text_formats().iter().any(|(e, _, _)| e.eq_ignore_ascii_case(ext)) {
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

// ── Info preview ─────────────────────────────────────────────────────────────

/// Produce JSON for an info-only preview: `{"kind":"...","title":"... - N bytes · date"}`.
pub fn render_info(path: &str, kind: &str, size: i64, modified_unix: i64) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let summary = format!("{} bytes · {}", format_number(size), format_timestamp(modified_unix));
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
    text.push_str(&format!("Image size: {}\n", format_bytes(pe.image_size as i64)));
    if pe.link_timestamp > 0 {
        text.push_str(&format!("Link time: {}\n", format_timestamp(pe.link_timestamp as i64)));
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
    ".zip", ".jar", ".apk", ".apks", ".aab", ".msix", ".msixbundle", ".appx", ".appxbundle",
    ".epub", ".nupkg", ".vsix", ".whl", ".cbz", ".xpi",
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
            let BValue::Dict(file_dict) = file else { continue; };
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
            let item_name = path_parts.last().cloned().unwrap_or_else(|| full_name.clone());
            entries.insert(full_name.clone(), (item_name, parent_of(&full_name), false, size, 0, 0));
        }
    } else if let Some(length) = dict_get_int(info, b"length") {
        total_size = length;
        file_count = 1;
        entries.insert(name.clone(), (name.clone(), String::new(), false, length, 0, 0));
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
            typ: if *is_folder { "Folder".to_string() } else { type_for_ext(name).to_string() },
            native_path: None,
        });
    }

    let mut summary = format!("{} files - {}", format_number(file_count as i64), format_bytes(total_size));
    if !announce.is_empty() {
        summary.push_str(&format!(" - {announce}"));
    }

    to_json(&PreviewReadyDto {
        kind: "torrent".to_string(),
        title: format!("{name} - {} files", format_number(file_count as i64)),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
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
            let n = std::str::from_utf8(&bytes[i..end]).ok()?.parse::<i64>().ok()?;
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
    let len = std::str::from_utf8(&bytes[i..colon]).ok()?.parse::<usize>().ok()?;
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
    String::from_utf8_lossy(bytes).trim_matches(char::from(0)).to_string()
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
        let full_name = entry.name().replace('\\', "/").trim_start_matches('/').to_string();
        if full_name.is_empty() {
            continue;
        }
        let is_folder = full_name.ends_with('/') || entry.name().is_empty();
        let size = entry.size() as i64;
        let packed = entry.compressed_size() as i64;
        let modified = entry.last_modified().map(|d| {
            // zip::DateTime → unix seconds (approximate: no leap seconds, no TZ)
            let secs = ((d.year() as i64 - 1970) * 365 * 86400) + ((d.month() as i64 - 1) * 30 * 86400) + ((d.day() as i64 - 1) * 86400);
            secs
        }).unwrap_or(0);
        drop(entry);

        if is_folder {
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            add_parent_folders(&full_name, &mut entries);
            let path = ensure_trailing_slash(&full_name);
            if !entries.contains_key(&path) {
                let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
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
                let name = full_name.rsplit('/').next().unwrap_or(&full_name).to_string();
                entries.insert(full_name.clone(), (name, parent_of(&full_name), false, size, packed, modified));
                seen += 1;
            } else {
                partial = true;
            }
        }
    }

    archive_listing_json(filename, "archive", entries, file_count, uncompressed, compressed, partial)
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
        let full_name = path_buf.to_string_lossy().replace('\\', "/").trim_start_matches('/').to_string();
        if full_name.is_empty() {
            continue;
        }

        let is_folder = entry.header().entry_type().is_dir() || full_name.ends_with('/');
        let size = if is_folder { 0 } else { entry.header().size().unwrap_or(0) as i64 };
        let modified = entry.header().mtime().unwrap_or(0) as i64;
        if is_folder {
            if entries.len() >= MAX_ARCHIVE_ENTRIES {
                partial = true;
                continue;
            }
            add_parent_folders(&full_name, &mut entries);
            let folder_path = ensure_trailing_slash(&full_name);
            if entries.len() < MAX_ARCHIVE_ENTRIES && !entries.contains_key(&folder_path) {
                let name = folder_path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
                entries.insert(folder_path.clone(), (name, parent_of(&folder_path), true, 0, 0, modified));
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
                let name = full_name.rsplit('/').next().unwrap_or(&full_name).to_string();
                entries.insert(full_name.clone(), (name, parent_of(&full_name), false, size, 0, modified));
                seen += 1;
            } else {
                partial = true;
            }
        }
    }

    archive_listing_json(filename, kind, entries, file_count, uncompressed, 0, partial)
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
    archive_listing_json(filename, "archive", entries, 1, uncompressed, compressed, false)
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
    let folder_count = entries.values().filter(|(_, _, is_folder, _, _, _)| *is_folder).count();
    let mut summary = format!("{} files, {} folders", format_number(file_count as i64), format_number(folder_count as i64));
    if uncompressed > 0 {
        summary.push_str(&format!(" - {} uncompressed", format_bytes(uncompressed)));
        if compressed > 0 {
            let saved = 100.0 - (compressed as f64 * 100.0 / uncompressed as f64);
            summary.push_str(&format!(" - {:.1}% saved", saved.clamp(0.0, 100.0)));
        }
    }

    let mut items = Vec::with_capacity(entries.len());
    for (path, (name, parent, is_folder, size, packed, modified)) in &entries {
        let typ = if *is_folder { "Folder" } else { type_for_ext(name) };
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
        title: format!("{filename} - {} entries", format_number(file_count as i64 + folder_count as i64)),
        format: None,
        language: None,
        text: None,
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

fn add_parent_folders(path: &str, entries: &mut BTreeMap<String, (String, String, bool, i64, i64, i64)>) {
    let mut start = 0;
    while let Some(idx) = path[start..].find('/') {
        let full_idx = start + idx;
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            return;
        }
        let folder_path = format!("{}/", &path[..full_idx]);
        if !entries.contains_key(&folder_path) {
            let name = path[..full_idx].rsplit('/').next().unwrap_or("").to_string();
            entries.insert(folder_path.clone(), (name, parent_of(&folder_path), true, 0, 0, 0));
        }
        start = full_idx + 1;
    }
}

fn ensure_trailing_slash(s: &str) -> String {
    if s.ends_with('/') { s.to_string() } else { format!("{}/", s) }
}

fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(idx) => trimmed[..idx + 1].to_string(),
        None => String::new(),
    }
}

fn type_for_ext(name: &str) -> &'static str {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
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
                    let name = entry_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    let native = entry_path.to_string_lossy().to_string();
                    let modified = meta.modified().ok()
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
                        let name = entry_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        let native = entry_path.to_string_lossy().to_string();
                        let modified = meta.modified().ok()
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
        b.is_folder.cmp(&a.is_folder)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });

    let mut summary = format!("{} files, {} folders - {}",
        format_number(file_count as i64),
        format_number(folder_count as i64),
        format_bytes(total_bytes));
    if skipped > 0 {
        summary.push_str(&format!(" - {} inaccessible", format_number(skipped as i64)));
    }
    if partial {
        summary.push_str(" - partial");
    }

    to_json(&PreviewReadyDto {
        kind: "folder".to_string(),
        title: format!("{root_name} - {} files, {} folders", format_number(file_count as i64), format_number(folder_count as i64)),
        format: None,
        language: None,
        text: None,
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
    if n < 0 { format!("-{}", result) } else { result }
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
