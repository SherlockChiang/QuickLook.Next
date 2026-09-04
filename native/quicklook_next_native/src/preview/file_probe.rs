//! Shared path and HANDLE file probing with bounded classification and caching.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{is_text_file, probe_image_animation_reader};
use crate::rar_listing;

struct ProbeCacheEntry {
    modified: Option<SystemTime>,
    size: u64,
    sequence: u64,
    json: String,
}

static PROBE_CACHE: OnceLock<Mutex<HashMap<String, ProbeCacheEntry>>> = OnceLock::new();
static PROBE_CACHE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PROBE_CACHE_MAX: usize = 500;

fn probe_cache() -> &'static Mutex<HashMap<String, ProbeCacheEntry>> {
    PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Evict oldest entries when the cache exceeds `PROBE_CACHE_MAX`. Called after insertion.
fn probe_cache_evict(cache: &mut HashMap<String, ProbeCacheEntry>) {
    if cache.len() <= PROBE_CACHE_MAX {
        return;
    }
    let oldest_key = cache
        .iter()
        .min_by_key(|(_, entry)| entry.sequence)
        .map(|(key, _)| key.clone());
    if let Some(key) = oldest_key {
        cache.remove(&key);
    }
}

pub(crate) fn probe_path_json(path: &str) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let precise_modified = meta.modified().ok();
    let modified = precise_modified
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    if let Ok(cache) = probe_cache().lock() {
        if let Some(entry) = cache.get(path) {
            if entry.modified == precise_modified && entry.size == size {
                return Some(entry.json.clone());
            }
        }
    }

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension.to_lowercase()))
        .unwrap_or_default();
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    let mut prefix = [0u8; 64];
    let read = if meta.is_dir() {
        0
    } else {
        fs::File::open(path)
            .ok()
            .map(|mut file| file.read(&mut prefix).unwrap_or(0))
            .unwrap_or(0)
    };
    let magic = &prefix[..read];

    let kind = if meta.is_dir() {
        "folder"
    } else {
        classify(file_name, &ext, magic, size == 0)
    };
    let magic_hex: String = magic.iter().map(|byte| format!("{byte:02X}")).collect();
    let animation = if kind == "image" {
        fs::File::open(path)
            .ok()
            .and_then(|mut file| probe_image_animation_reader(&mut file, file_name, size))
    } else {
        None
    }
    .unwrap_or_default();
    let is_animated = match animation.is_animated {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };

    let json = format!(
        "{{\"path\":\"{}\",\"extension\":\"{}\",\"magicHex\":\"{}\",\"kind\":\"{}\",\"size\":{},\"modifiedUnix\":{},\"isAnimated\":{}}}",
        json_escape(path),
        json_escape(&ext),
        magic_hex,
        kind,
        size,
        modified,
        is_animated
    );

    if let Ok(mut cache) = probe_cache().lock() {
        cache.insert(
            path.to_string(),
            ProbeCacheEntry {
                modified: precise_modified,
                size,
                sequence: PROBE_CACHE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                json: json.clone(),
            },
        );
        probe_cache_evict(&mut cache);
    }
    Some(json)
}

pub(crate) fn probe_reader_json(
    file: &mut fs::File,
    logical_name: &str,
    size: u64,
    modified_unix: i64,
) -> std::result::Result<String, i32> {
    let ext = std::path::Path::new(logical_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_lowercase()))
        .unwrap_or_default();
    let mut prefix = [0u8; 64];
    let read = file.read(&mut prefix).map_err(|_| crate::QL_ERROR_IO)?;
    let magic = &prefix[..read];
    let kind = classify(logical_name, &ext, magic, size == 0);
    let magic_hex: String = magic.iter().map(|value| format!("{value:02X}")).collect();
    let animation = if kind == "image" {
        probe_image_animation_reader(file, logical_name, size).unwrap_or_default()
    } else {
        Default::default()
    };
    let is_animated = match animation.is_animated {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    Ok(format!(
        "{{\"path\":\"{}\",\"extension\":\"{}\",\"magicHex\":\"{}\",\"kind\":\"{}\",\"size\":{},\"modifiedUnix\":{},\"isAnimated\":{}}}",
        json_escape(logical_name),
        json_escape(&ext),
        magic_hex,
        kind,
        size,
        modified_unix,
        is_animated
    ))
}

/// Coarse type classification. Container formats are recognized by extension first (e.g. .docx is
/// ZIP by magic but should be "office"), then images/pdf/archives by magic, then text.
fn classify(file_name: &str, ext: &str, magic: &[u8], is_empty: bool) -> &'static str {
    const OFFICE_EXTS: &[&str] = &[
        ".doc", ".docx", ".docm", ".xls", ".xlsx", ".xlsm", ".ppt", ".pptx", ".pptm", ".rtf",
        ".odt", ".ods", ".odp",
    ];
    const VIDEO_EXTS: &[&str] = &[
        ".mp4", ".mkv", ".avi", ".mov", ".webm", ".flv", ".wmv", ".m4v", ".mpg", ".mpeg", ".3gp",
    ];
    const AUDIO_EXTS: &[&str] = &[
        ".mp3", ".wav", ".flac", ".aac", ".ogg", ".m4a", ".wma", ".opus", ".mid",
    ];
    const ARCHIVE_EXTS: &[&str] = &[
        ".zip", ".jar", ".nupkg", ".vsix", ".whl", ".cbz", ".xpi", ".tar", ".tgz", ".gz",
    ];
    const EBOOK_EXTS: &[&str] = &[".epub", ".fb2", ".mobi", ".azw", ".azw3"];
    const IMAGE_EXTS: &[&str] = &[
        ".png", ".jpg", ".jpeg", ".jpe", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".webp", ".ico",
        ".heic", ".heif", ".avif", ".jxl", ".svg",
    ];
    const PACKAGE_EXTS: &[&str] = &[
        ".apk",
        ".apks",
        ".aab",
        ".msix",
        ".msixbundle",
        ".appx",
        ".appxbundle",
    ];
    const DISK_IMAGE_EXTS: &[&str] = &[".img", ".iso", ".vhd", ".vhdx", ".vmdk", ".dmg"];
    const EXECUTABLE_EXTS: &[&str] = &[".exe", ".dll", ".sys", ".scr", ".cpl", ".ocx"];
    const CERTIFICATE_EXTS: &[&str] = &[".cer", ".crt", ".der", ".pem", ".p7b", ".p7c"];
    const FONT_EXTS: &[&str] = &[".ttf", ".otf", ".ttc", ".otc", ".woff", ".woff2"];
    const DATABASE_EXTS: &[&str] = &[
        ".sqlite",
        ".sqlite3",
        ".db",
        ".db3",
        ".s3db",
        ".sqlite-shm",
        ".sqlite-wal",
        ".mdb",
        ".accdb",
    ];
    const MAIL_EXTS: &[&str] = &[".eml", ".msg", ".mbox", ".emlx"];
    const CHM_EXTS: &[&str] = &[".chm"];
    const DUMP_EXTS: &[&str] = &[".dmp", ".mdmp", ".dump", ".core"];
    const ELF_EXTS: &[&str] = &[".elf", ".so", ".o"];
    if OFFICE_EXTS.contains(&ext) {
        return "office";
    }
    if EBOOK_EXTS.contains(&ext) {
        return "ebook";
    }
    if CERTIFICATE_EXTS.contains(&ext) {
        return "certificate";
    }
    if EXECUTABLE_EXTS.contains(&ext) || magic.starts_with(b"MZ") {
        return "executable";
    }
    if FONT_EXTS.contains(&ext) {
        return "font";
    }
    let lower_file_name = file_name.to_ascii_lowercase();
    if DATABASE_EXTS.contains(&ext)
        || lower_file_name.ends_with("-wal")
        || lower_file_name.ends_with("-shm")
    {
        return "database";
    }
    if MAIL_EXTS.contains(&ext) {
        return "mail";
    }
    if CHM_EXTS.contains(&ext) {
        return "chm";
    }
    if DUMP_EXTS.contains(&ext) {
        return "dump";
    }
    if ELF_EXTS.contains(&ext) {
        return "elf";
    }
    if ext == ".torrent" {
        return "torrent";
    }
    if DISK_IMAGE_EXTS.contains(&ext) {
        return "disk-image";
    }
    if PACKAGE_EXTS.contains(&ext) {
        return "package";
    }
    if VIDEO_EXTS.contains(&ext) {
        return "video";
    }
    if AUDIO_EXTS.contains(&ext) {
        return "audio";
    }
    if ARCHIVE_EXTS.contains(&ext) {
        return "archive";
    }
    if IMAGE_EXTS.contains(&ext) {
        return "image";
    }

    if magic.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || magic.starts_with(&[0xFF, 0xD8, 0xFF])
        || magic.starts_with(b"GIF8")
        || magic.starts_with(b"BM")
        || magic.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || magic.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
        || (magic.len() >= 12 && &magic[0..4] == b"RIFF" && &magic[8..12] == b"WEBP")
    {
        return "image";
    }
    if magic.starts_with(b"%PDF") {
        return "pdf";
    }
    if magic.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || magic.starts_with(b"OTTO")
        || magic.starts_with(b"ttcf")
        || magic.starts_with(b"wOFF")
        || magic.starts_with(b"wOF2")
    {
        return "font";
    }
    if magic.starts_with(b"SQLite format 3\0") {
        return "database";
    }
    if magic.starts_with(b"ITSF") {
        return "chm";
    }
    if magic.starts_with(b"MDMP") {
        return "dump";
    }
    if magic.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return "elf";
    }
    if magic.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || magic.starts_with(&[0x1F, 0x8B])
        || rar_listing::is_rar_magic(magic)
    {
        return "archive";
    }

    if is_empty || is_text_file(file_name, ext, magic) {
        return "text";
    }
    "binary"
}

fn json_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 8);
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' => {
                let _ = write!(&mut output, "\\u{:04X}", character as u32);
            }
            character => output.push(character),
        }
    }
    output
}

#[cfg(test)]
mod tests;
