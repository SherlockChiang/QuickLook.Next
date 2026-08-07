//! Shared cancellation-aware bounded readers and ZIP preflight primitives.
//!
//! Format modules should use these helpers for all reader-backed input.  The helpers keep
//! cancellation checks and exact-length/central-directory validation in one place so that the
//! archive, ebook, Office, and package routes cannot accidentally diverge.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};

use zip::ZipArchive;

use super::common::{read_u16, read_u32, read_u64};
use super::types::ReaderPreviewError;

pub(super) fn preview_cancelled(cancel_cb: Option<extern "C" fn() -> bool>) -> bool {
    cancel_cb.map(|callback| callback()).unwrap_or(false)
}

pub(super) fn read_file_prefix(path: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    read_reader_prefix(&mut file, max_bytes)
}

pub(super) fn read_reader_prefix<R: Read>(reader: &mut R, max_bytes: usize) -> Option<Vec<u8>> {
    let mut reader = reader.take(max_bytes as u64);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    reader.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

pub(super) fn read_reader_prefix_cancelable<R: Read>(
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
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Io),
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(bytes)
}

pub(super) fn read_reader_exact_bounded_cancelable<R: Read>(
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
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
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

pub(super) fn read_exact_cancelable<R: Read + ?Sized>(
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
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReaderPreviewError::Io),
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(())
}

pub(super) fn drain_exact_cancelable<R: Read + ?Sized>(
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

pub(super) fn read_limited_to_end<R: Read>(reader: &mut R, max_size: u64) -> Option<Vec<u8>> {
    let cap = max_size.min(64 * 1024) as usize;
    let mut limited = reader.take(max_size.saturating_add(1));
    let mut bytes = Vec::with_capacity(cap);
    limited.read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max_size {
        return None;
    }
    Some(bytes)
}

pub(super) fn prepare_seekable_reader<R: Seek>(
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

pub(super) const MAX_ZIP_CENTRAL_DIRECTORY_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const ZIP_EOCD_MIN_BYTES: u64 = 22;
pub(super) const ZIP_EOCD_MAX_TAIL_BYTES: u64 = ZIP_EOCD_MIN_BYTES + u16::MAX as u64;

pub(super) fn validate_zip_container<R: Read + Seek>(
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

pub(super) struct CancelableSeekReader<R> {
    reader: R,
    cancel_cb: Option<extern "C" fn() -> bool>,
}

impl<R> CancelableSeekReader<R> {
    pub(super) fn new(reader: R, cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
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

pub(super) fn open_validated_zip<R: Read + Seek>(
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

#[cfg(test)]
mod tests;
