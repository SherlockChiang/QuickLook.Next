use std::fs;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use super::super::{
    open_validated_zip, preview_cancelled, ReaderPreviewError, MAX_ARCHIVE_SCAN_ENTRIES,
};
use super::listing::reader_starts_with_rar_magic;
use super::{
    ARCHIVE_EXTRACT_DEADLINE, ARCHIVE_EXTRACT_RETENTION, GZ_EXTS, MAX_ARCHIVE_EXTRACT_BYTES,
    MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES, MAX_ARCHIVE_EXTRACT_RATIO, MAX_ARCHIVE_EXTRACT_ROOTS,
    MAX_ARCHIVE_HANDLE_INPUT_BYTES, MAX_ARCHIVE_ZIP_ENTRIES, RAR_EXTS, TAR_EXTS, TAR_GZ_EXTS,
};

#[cfg(test)]
mod tests;

pub(crate) fn extract_archive_entry_to_temp(
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

pub(crate) fn extract_archive_entry_to_temp_reader<R: Read + Seek>(
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
pub(crate) fn extract_archive_entry_to_writer_reader<R: Read + Seek, W: Write>(
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
