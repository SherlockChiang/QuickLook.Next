use std::io::{Cursor, Write};

use super::{
    open_validated_zip, read_limited_to_end, read_reader_exact_bounded_cancelable,
    validate_zip_container, MAX_ZIP_CENTRAL_DIRECTORY_BYTES, ZIP_EOCD_MAX_TAIL_BYTES,
};
use crate::preview::archive::MAX_ARCHIVE_ZIP_ENTRIES;
use crate::preview::ReaderPreviewError;

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
fn limited_reader_rejects_payloads_over_cap() {
    let mut reader = Cursor::new(vec![1, 2, 3, 4, 5]);

    assert!(read_limited_to_end(&mut reader, 4).is_none());
}

extern "C" fn always_cancel() -> bool {
    true
}
