use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use flate2::read::GzDecoder;
use tar::Archive as TarArchive;
use zip::ZipArchive;

use crate::rar_listing::{self, RarScanError};

use super::super::{
    add_parent_folders, format_bytes, format_number, is_package_path, open_validated_zip,
    parent_of, prepare_seekable_reader, preview_cancelled, read_exact_cancelable, render_package,
    to_json, type_for_ext, ArchiveListingEntry, CancelableSeekReader, PreviewListingDto,
    PreviewListingItemDto, PreviewReadyDto, ReaderPreviewError, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_SCAN_ENTRIES,
};
use super::{
    GZ_EXTS, MAX_ARCHIVE_HANDLE_INPUT_BYTES, MAX_ARCHIVE_ZIP_ENTRIES, MAX_RAR_RETAINED_PATH_BYTES,
    MAX_TAR_SCAN_BYTES, RAR_EXTS, TAR_EXTS, TAR_GZ_EXTS, TAR_SCAN_DEADLINE, ZIP_EXTS,
};

#[cfg(test)]
mod tests;

pub(crate) fn is_archive(ext: &str, kind: &str, magic: &[u8]) -> bool {
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

pub(super) fn reader_starts_with_rar_magic<R: Read + Seek>(
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
pub(crate) fn render_archive(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
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

pub(crate) fn render_archive_reader<R: Read + Seek>(
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

pub(in crate::preview) fn render_zip_archive_from_zip<R: Read + Seek>(
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
