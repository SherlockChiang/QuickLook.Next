use std::collections::BTreeMap;
use std::io::Read;

use super::super::{
    common::{format_bytes, read_u32_be},
    drain_exact_cancelable, preview_cancelled, read_exact_cancelable, ReaderPreviewError,
    MAX_INFO_HEADER_BYTES, MAX_SQLITE_SHM_BYTES, MAX_SQLITE_WAL_BYTES,
};
use super::sqlite::database_page_size as sqlite_database_page_size;

#[derive(Default)]
pub(super) struct SqliteWalSnapshot {
    pub(super) valid_frames: u64,
    pub(super) last_commit_frame: u64,
    pub(super) committed_pages: u32,
    pub(super) stopped_frame: Option<u64>,
    pub(super) stopped_reason: Option<&'static str>,
    pub(super) trailing_bytes: u64,
    pub(super) unscanned_bytes: u64,
    pub(super) committed_prefix_pages: BTreeMap<u32, Vec<u8>>,
}

impl SqliteWalSnapshot {
    pub(super) fn summary(&self) -> String {
        let mut summary = if self.last_commit_frame == 0 {
            format!(
                "WAL HANDLE: {} valid frames, no commit frame; main database view used",
                self.valid_frames
            )
        } else {
            format!(
                "Snapshot: WAL HANDLE through commit frame {} ({} pages, {} valid frames)",
                self.last_commit_frame, self.committed_pages, self.valid_frames
            )
        };
        if let (Some(frame), Some(reason)) = (self.stopped_frame, self.stopped_reason) {
            summary.push_str(&format!("; scan stopped at frame {frame}: {reason}"));
        } else if self.trailing_bytes > 0 {
            summary.push_str(&format!(
                "; scan stopped before {} trailing partial bytes",
                self.trailing_bytes
            ));
        } else {
            summary.push_str("; full WAL validated");
        }
        if self.unscanned_bytes > 0 {
            summary.push_str(&format!("; {} later bytes ignored", self.unscanned_bytes));
        }
        summary
    }
}

pub(super) fn inspect_sqlite_wal_snapshot(
    reader: &mut dyn Read,
    wal_length: u64,
    database_page_size: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<SqliteWalSnapshot, ReaderPreviewError> {
    if wal_length > MAX_SQLITE_WAL_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if wal_length < 32 {
        drain_exact_cancelable(reader, wal_length, cancel_cb)?;
        return Err(ReaderPreviewError::Malformed);
    }

    let mut header = [0u8; 32];
    read_exact_cancelable(reader, &mut header, cancel_cb)?;
    let checksum_big_endian = match read_u32_be(&header, 0) {
        Some(0x377F_0682) => false,
        Some(0x377F_0683) => true,
        _ => return Err(ReaderPreviewError::Malformed),
    };
    if read_u32_be(&header, 4) != Some(3_007_000) {
        return Err(ReaderPreviewError::Malformed);
    }
    let wal_page_size = read_u32_be(&header, 8)
        .map(|value| value as usize)
        .filter(|value| (512..=65_536).contains(value) && value.is_power_of_two())
        .ok_or(ReaderPreviewError::Malformed)?;
    if wal_page_size != database_page_size {
        return Err(ReaderPreviewError::Malformed);
    }
    let mut checksum = sqlite_wal_checksum(&header[..24], checksum_big_endian, (0, 0));
    if read_u32_be(&header, 24) != Some(checksum.0) || read_u32_be(&header, 28) != Some(checksum.1)
    {
        return Err(ReaderPreviewError::Malformed);
    }
    let salt = (
        read_u32_be(&header, 16).ok_or(ReaderPreviewError::Malformed)?,
        read_u32_be(&header, 20).ok_or(ReaderPreviewError::Malformed)?,
    );

    let frame_size = 24u64 + wal_page_size as u64;
    let mut remaining = wal_length - 32;
    let mut frame_number = 0u64;
    let mut pending_prefix_pages = BTreeMap::<u32, Vec<u8>>::new();
    let mut snapshot = SqliteWalSnapshot::default();
    while remaining >= frame_size {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        frame_number += 1;
        let mut frame_header = [0u8; 24];
        let mut page = vec![0u8; wal_page_size];
        read_exact_cancelable(reader, &mut frame_header, cancel_cb)?;
        read_exact_cancelable(reader, &mut page, cancel_cb)?;
        remaining -= frame_size;

        let page_number = read_u32_be(&frame_header, 0).ok_or(ReaderPreviewError::Malformed)?;
        let commit_pages = read_u32_be(&frame_header, 4).ok_or(ReaderPreviewError::Malformed)?;
        let frame_salt = (
            read_u32_be(&frame_header, 8).ok_or(ReaderPreviewError::Malformed)?,
            read_u32_be(&frame_header, 12).ok_or(ReaderPreviewError::Malformed)?,
        );
        let stopped_reason = if page_number == 0 {
            Some("invalid page number")
        } else if frame_salt != salt {
            Some("salt mismatch")
        } else {
            let mut next_checksum =
                sqlite_wal_checksum(&frame_header[..8], checksum_big_endian, checksum);
            next_checksum = sqlite_wal_checksum(&page, checksum_big_endian, next_checksum);
            if read_u32_be(&frame_header, 16) != Some(next_checksum.0)
                || read_u32_be(&frame_header, 20) != Some(next_checksum.1)
            {
                Some("checksum mismatch")
            } else {
                checksum = next_checksum;
                None
            }
        };
        if let Some(reason) = stopped_reason {
            snapshot.stopped_frame = Some(frame_number);
            snapshot.stopped_reason = Some(reason);
            snapshot.unscanned_bytes = remaining;
            drain_exact_cancelable(reader, remaining, cancel_cb)?;
            remaining = 0;
            break;
        }

        snapshot.valid_frames += 1;
        let page_offset = (u64::from(page_number) - 1).saturating_mul(wal_page_size as u64);
        if page_offset < MAX_INFO_HEADER_BYTES as u64 {
            pending_prefix_pages.insert(page_number, page);
        }
        if commit_pages != 0 {
            for (page_number, page) in std::mem::take(&mut pending_prefix_pages) {
                snapshot.committed_prefix_pages.insert(page_number, page);
            }
            snapshot.last_commit_frame = frame_number;
            snapshot.committed_pages = commit_pages;
        }
    }
    if remaining > 0 {
        snapshot.trailing_bytes = remaining;
        drain_exact_cancelable(reader, remaining, cancel_cb)?;
    }
    if snapshot.stopped_frame.is_some() && snapshot.last_commit_frame == 0 {
        return Err(ReaderPreviewError::Malformed);
    }
    Ok(snapshot)
}

pub(super) fn sqlite_wal_checksum(
    bytes: &[u8],
    big_endian: bool,
    mut checksum: (u32, u32),
) -> (u32, u32) {
    debug_assert_eq!(bytes.len() % 8, 0);
    for pair in bytes.chunks_exact(8) {
        let first = if big_endian {
            u32::from_be_bytes(pair[0..4].try_into().unwrap())
        } else {
            u32::from_le_bytes(pair[0..4].try_into().unwrap())
        };
        let second = if big_endian {
            u32::from_be_bytes(pair[4..8].try_into().unwrap())
        } else {
            u32::from_le_bytes(pair[4..8].try_into().unwrap())
        };
        checksum.0 = checksum.0.wrapping_add(first).wrapping_add(checksum.1);
        checksum.1 = checksum.1.wrapping_add(second).wrapping_add(checksum.0);
    }
    checksum
}

pub(super) fn apply_sqlite_wal_snapshot(
    database_prefix: &mut Vec<u8>,
    page_size: usize,
    snapshot: &SqliteWalSnapshot,
) -> Result<(), ReaderPreviewError> {
    if snapshot.last_commit_frame == 0 {
        return Ok(());
    }
    let logical_size = u64::from(snapshot.committed_pages)
        .checked_mul(page_size as u64)
        .ok_or(ReaderPreviewError::Malformed)?;
    let prefix_size = logical_size.min(MAX_INFO_HEADER_BYTES as u64) as usize;
    database_prefix.resize(prefix_size, 0);
    for (page_number, page) in &snapshot.committed_prefix_pages {
        let start = (u64::from(*page_number) - 1)
            .checked_mul(page_size as u64)
            .ok_or(ReaderPreviewError::Malformed)? as usize;
        let end = start
            .checked_add(page_size)
            .ok_or(ReaderPreviewError::Malformed)?;
        if end <= database_prefix.len() {
            database_prefix[start..end].copy_from_slice(page);
        }
    }
    if !database_prefix.starts_with(b"SQLite format 3\0")
        || database_prefix.len() < 32
        || sqlite_database_page_size(database_prefix) != Some(page_size)
    {
        return Err(ReaderPreviewError::Malformed);
    }
    database_prefix[28..32].copy_from_slice(&snapshot.committed_pages.to_be_bytes());
    Ok(())
}

pub(super) fn inspect_sqlite_shm(
    reader: &mut dyn Read,
    shm_length: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if shm_length > MAX_SQLITE_SHM_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    let inspected = shm_length.min(4096) as usize;
    let mut prefix = vec![0u8; inspected];
    read_exact_cancelable(reader, &mut prefix, cancel_cb)?;
    let mut summary = format!(
        "SHM HANDLE: diagnostic only, {} bytes ({} inspected)",
        format_bytes(shm_length as i64),
        format_bytes(inspected as i64)
    );
    if prefix.len() >= 24 {
        let version = u32::from_ne_bytes(prefix[0..4].try_into().unwrap());
        let initialized = prefix[12] != 0;
        let max_frame = u32::from_ne_bytes(prefix[16..20].try_into().unwrap());
        let database_pages = u32::from_ne_bytes(prefix[20..24].try_into().unwrap());
        summary.push_str(&format!(
            "; WAL-index version {version}, initialized {initialized}, max frame {max_frame}, database pages {database_pages}"
        ));
    }
    Ok(summary)
}

pub(super) fn append_sqlite_wal_summary(text: &mut String, bytes: &[u8], size: i64) {
    text.push_str("\nFormat: SQLite write-ahead log");
    let magic = read_u32_be(bytes, 0).unwrap_or(0);
    if !matches!(magic, 0x377F_0682 | 0x377F_0683) {
        text.push_str("\nHeader: unrecognized or incomplete");
        text.push_str(&format!(
            "\nInspected: {}",
            format_bytes(bytes.len() as i64)
        ));
        return;
    }
    if let Some(version) = read_u32_be(bytes, 4) {
        text.push_str(&format!("\nWAL version: {}", version));
    }
    if let Some(page_size) = read_u32_be(bytes, 8).filter(|value| *value > 0) {
        text.push_str(&format!("\nPage size: {} bytes", page_size));
        if size >= 32 {
            let frame_size = i64::from(page_size) + 24;
            text.push_str(&format!("\nFrames observed: {}", (size - 32) / frame_size));
            if (size - 32) % frame_size != 0 {
                text.push_str(" (trailing partial frame)");
            }
        }
    }
    if let Some(sequence) = read_u32_be(bytes, 12) {
        text.push_str(&format!("\nCheckpoint sequence: {}", sequence));
    }
    text.push_str(&format!(
        "\nInspected: {}",
        format_bytes(bytes.len() as i64)
    ));
}
