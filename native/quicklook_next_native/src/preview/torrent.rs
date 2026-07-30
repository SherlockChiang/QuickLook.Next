//! Torrent metadata and bencode parsing.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use super::common::{format_bytes, format_number, type_for_ext};
use super::types::{
    to_json, PreviewListingDto, PreviewListingItemDto, PreviewReadyDto, ReaderPreviewError,
};
use super::{
    add_parent_folders, file_size_modified, format_timestamp, parent_of, preview_cancelled,
    read_reader_exact_bounded_cancelable, render_info, ArchiveListingEntry, MAX_ARCHIVE_ENTRIES,
};

#[derive(Debug, Clone)]
pub(super) enum BValue {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<BValue>),
    Dict(BTreeMap<Vec<u8>, BValue>),
}

const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_BENCODE_DEPTH: usize = 64;
pub(super) const MAX_BENCODE_NODES: usize = 100_000;

pub fn render_torrent(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let (size, modified_unix) = file_size_modified(path);
    if size < 0 || size as u64 > MAX_TORRENT_BYTES {
        return render_info(path, "torrent", size, modified_unix);
    }
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    render_torrent_reader(&mut file, path, size, modified_unix, cancel_cb).unwrap_or_default()
}

pub fn render_torrent_reader<R: Read>(
    reader: &mut R,
    logical_name: &str,
    size: i64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    if size < 0 || size as u64 > MAX_TORRENT_BYTES {
        return Ok(render_info(logical_name, "torrent", size, modified_unix));
    }

    let bytes =
        read_reader_exact_bounded_cancelable(reader, size as u64, MAX_TORRENT_BYTES, cancel_cb)?;
    let root = match parse_bencode(&bytes, cancel_cb) {
        Some((value, _)) => value,
        None if preview_cancelled(cancel_cb) => return Err(ReaderPreviewError::Cancelled),
        None => return Err(ReaderPreviewError::Malformed),
    };
    let dict = match root {
        BValue::Dict(d) => d,
        _ => return Err(ReaderPreviewError::Malformed),
    };

    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let announce = dict_get_string(&dict, b"announce").unwrap_or_default();
    let created_by = dict_get_string(&dict, b"created by").unwrap_or_default();
    let creation_date = dict_get_int(&dict, b"creation date").unwrap_or(0);
    let comment = dict_get_string(&dict, b"comment").unwrap_or_default();
    let info = match dict.get(b"info".as_slice()) {
        Some(BValue::Dict(d)) => d,
        _ => return Err(ReaderPreviewError::Malformed),
    };

    let name = dict_get_string(info, b"name").unwrap_or_else(|| filename.to_string());
    let piece_length = dict_get_int(info, b"piece length").unwrap_or(0);
    let pieces = match info.get(b"pieces".as_slice()) {
        Some(BValue::Bytes(b)) => b.len() / 20,
        _ => 0,
    };

    let mut entries: BTreeMap<String, ArchiveListingEntry> = BTreeMap::new();
    let mut total_size = 0i64;
    let mut file_count = 0u64;
    let mut partial = false;

    if let Some(BValue::List(files)) = info.get(b"files".as_slice()) {
        for file in files {
            if preview_cancelled(cancel_cb) {
                return Err(ReaderPreviewError::Cancelled);
            }
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
            total_size = total_size.saturating_add(size);
            file_count = file_count.saturating_add(1);
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
                (item_name, parent_of(&full_name), false, size, 0, 0, false),
            );
        }
    } else if let Some(length) = dict_get_int(info, b"length") {
        total_size = length;
        file_count = 1;
        entries.insert(
            name.clone(),
            (name.clone(), String::new(), false, length, 0, 0, false),
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
    for (path, (name, parent, is_folder, size, packed, modified, is_encrypted)) in &entries {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
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
            is_encrypted: *is_encrypted,
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

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(to_json(&PreviewReadyDto {
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
            can_preview_entries: false,
            encrypted_file_count: 0,
            items,
        }),
        table: None,
        markdown: None,
    }))
}

pub(super) fn parse_bencode(
    bytes: &[u8],
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(BValue, usize)> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let mut remaining_nodes = MAX_BENCODE_NODES;
    parse_bencode_at(bytes, 0, 0, &mut remaining_nodes, cancel_cb)
}

fn parse_bencode_at(
    bytes: &[u8],
    mut i: usize,
    depth: usize,
    remaining_nodes: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(BValue, usize)> {
    if preview_cancelled(cancel_cb) || depth > MAX_BENCODE_DEPTH || *remaining_nodes == 0 {
        return None;
    }
    *remaining_nodes -= 1;
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
                if preview_cancelled(cancel_cb) {
                    return None;
                }
                let (value, next) =
                    parse_bencode_at(bytes, i, depth + 1, remaining_nodes, cancel_cb)?;
                values.push(value);
                i = next;
            }
            Some((BValue::List(values), i + 1))
        }
        b'd' => {
            i += 1;
            let mut values = BTreeMap::new();
            while *bytes.get(i)? != b'e' {
                if preview_cancelled(cancel_cb) {
                    return None;
                }
                let (key, next) = parse_bytes_at(bytes, i)?;
                let (value, next) =
                    parse_bencode_at(bytes, next, depth + 1, remaining_nodes, cancel_cb)?;
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
