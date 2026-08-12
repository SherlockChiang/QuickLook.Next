use std::fs;
use std::io::{Cursor, Write};

use super::super::listing::{render_archive, render_archive_reader};
use super::super::{MAX_ARCHIVE_EXTRACT_BYTES, MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES};
use super::{
    archive_entry_within_extract_budget, archive_extract_output_name, create_archive_extract_root,
    discard_archive_extract_path, extract_archive_entry_to_temp,
    extract_archive_entry_to_writer_reader,
};
use crate::preview::ReaderPreviewError;

#[derive(Clone, Copy)]
enum ManualZipMethod {
    Stored,
    Deflated,
    Unsupported(u16),
}

struct ManualZipEntry<'a> {
    name: &'a [u8],
    payload: &'a [u8],
    method: ManualZipMethod,
    utf8_name: bool,
}

struct ManualZipCentralEntry {
    name: Vec<u8>,
    flags: u16,
    method: u16,
    checksum: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

fn append_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn append_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn raw_stored_deflate(payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len()).expect("manual DEFLATE fixture length");
    let mut bytes = Vec::with_capacity(payload.len() + 5);
    // One final, byte-aligned DEFLATE stored block. This deliberately avoids flate2 and zip's
    // writer so the compatibility fixture has an independent encoding path.
    bytes.push(0x01);
    append_u16(&mut bytes, length);
    append_u16(&mut bytes, !length);
    bytes.extend_from_slice(payload);
    bytes
}

fn manual_zip(entries: &[ManualZipEntry<'_>]) -> Vec<u8> {
    manual_zip_with_end(entries, false)
}

fn manual_zip64(entries: &[ManualZipEntry<'_>]) -> Vec<u8> {
    manual_zip_with_end(entries, true)
}

fn manual_zip_with_end(entries: &[ManualZipEntry<'_>], zip64: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut central_entries = Vec::with_capacity(entries.len());

    for entry in entries {
        let (method, compressed) = match entry.method {
            ManualZipMethod::Stored => (0u16, entry.payload.to_vec()),
            ManualZipMethod::Deflated => (8u16, raw_stored_deflate(entry.payload)),
            ManualZipMethod::Unsupported(method) => (method, entry.payload.to_vec()),
        };
        let flags = if entry.utf8_name { 1 << 11 } else { 0 };
        let checksum = crc32(entry.payload);
        let compressed_size = u64::try_from(compressed.len()).expect("manual ZIP compressed size");
        let uncompressed_size =
            u64::try_from(entry.payload.len()).expect("manual ZIP uncompressed size");
        let name_len = u16::try_from(entry.name.len()).expect("manual ZIP name length");
        let local_header_offset = u64::try_from(bytes.len()).expect("manual ZIP offset");

        append_u32(&mut bytes, 0x0403_4b50);
        append_u16(&mut bytes, if zip64 { 45 } else { 20 });
        append_u16(&mut bytes, flags);
        append_u16(&mut bytes, method);
        append_u16(&mut bytes, 0);
        append_u16(&mut bytes, 0x0021);
        append_u32(&mut bytes, checksum);
        append_u32(
            &mut bytes,
            if zip64 {
                u32::MAX
            } else {
                u32::try_from(compressed_size).expect("manual ZIP 32-bit compressed size")
            },
        );
        append_u32(
            &mut bytes,
            if zip64 {
                u32::MAX
            } else {
                u32::try_from(uncompressed_size).expect("manual ZIP 32-bit uncompressed size")
            },
        );
        append_u16(&mut bytes, name_len);
        append_u16(&mut bytes, if zip64 { 20 } else { 0 });
        bytes.extend_from_slice(entry.name);
        if zip64 {
            append_u16(&mut bytes, 0x0001);
            append_u16(&mut bytes, 16);
            append_u64(&mut bytes, uncompressed_size);
            append_u64(&mut bytes, compressed_size);
        }
        bytes.extend_from_slice(&compressed);

        central_entries.push(ManualZipCentralEntry {
            name: entry.name.to_vec(),
            flags,
            method,
            checksum,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
    }

    let central_offset = u64::try_from(bytes.len()).expect("manual ZIP central offset");
    for entry in &central_entries {
        append_u32(&mut bytes, 0x0201_4b50);
        append_u16(&mut bytes, if zip64 { 45 } else { 20 });
        append_u16(&mut bytes, if zip64 { 45 } else { 20 });
        append_u16(&mut bytes, entry.flags);
        append_u16(&mut bytes, entry.method);
        append_u16(&mut bytes, 0);
        append_u16(&mut bytes, 0x0021);
        append_u32(&mut bytes, entry.checksum);
        append_u32(
            &mut bytes,
            if zip64 {
                u32::MAX
            } else {
                u32::try_from(entry.compressed_size)
                    .expect("manual ZIP 32-bit central compressed size")
            },
        );
        append_u32(
            &mut bytes,
            if zip64 {
                u32::MAX
            } else {
                u32::try_from(entry.uncompressed_size)
                    .expect("manual ZIP 32-bit central uncompressed size")
            },
        );
        append_u16(
            &mut bytes,
            u16::try_from(entry.name.len()).expect("manual ZIP central name length"),
        );
        append_u16(&mut bytes, if zip64 { 28 } else { 0 });
        append_u16(&mut bytes, 0);
        append_u16(&mut bytes, 0);
        append_u16(&mut bytes, 0);
        append_u32(&mut bytes, 0);
        append_u32(
            &mut bytes,
            if zip64 {
                u32::MAX
            } else {
                u32::try_from(entry.local_header_offset)
                    .expect("manual ZIP 32-bit local header offset")
            },
        );
        bytes.extend_from_slice(&entry.name);
        if zip64 {
            append_u16(&mut bytes, 0x0001);
            append_u16(&mut bytes, 24);
            append_u64(&mut bytes, entry.uncompressed_size);
            append_u64(&mut bytes, entry.compressed_size);
            append_u64(&mut bytes, entry.local_header_offset);
        }
    }
    let central_size = u64::try_from(bytes.len()).expect("manual ZIP size") - central_offset;
    let entry_count = u64::try_from(entries.len()).expect("manual ZIP entry count");

    if zip64 {
        let zip64_offset = u64::try_from(bytes.len()).expect("manual ZIP64 end offset");
        append_u32(&mut bytes, 0x0606_4b50);
        append_u64(&mut bytes, 44);
        append_u16(&mut bytes, 45);
        append_u16(&mut bytes, 45);
        append_u32(&mut bytes, 0);
        append_u32(&mut bytes, 0);
        append_u64(&mut bytes, entry_count);
        append_u64(&mut bytes, entry_count);
        append_u64(&mut bytes, central_size);
        append_u64(&mut bytes, central_offset);
        append_u32(&mut bytes, 0x0706_4b50);
        append_u32(&mut bytes, 0);
        append_u64(&mut bytes, zip64_offset);
        append_u32(&mut bytes, 1);
    }

    append_u32(&mut bytes, 0x0605_4b50);
    append_u16(&mut bytes, 0);
    append_u16(&mut bytes, 0);
    append_u16(
        &mut bytes,
        if zip64 {
            u16::MAX
        } else {
            u16::try_from(entry_count).expect("manual ZIP 16-bit entry count")
        },
    );
    append_u16(
        &mut bytes,
        if zip64 {
            u16::MAX
        } else {
            u16::try_from(entry_count).expect("manual ZIP 16-bit entry count")
        },
    );
    append_u32(
        &mut bytes,
        if zip64 {
            u32::MAX
        } else {
            u32::try_from(central_size).expect("manual ZIP 32-bit central size")
        },
    );
    append_u32(
        &mut bytes,
        if zip64 {
            u32::MAX
        } else {
            u32::try_from(central_offset).expect("manual ZIP 32-bit central offset")
        },
    );
    append_u16(&mut bytes, 0);
    bytes
}

fn extract_manual_zip_entry(bytes: &[u8], path: &str) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut output = Vec::new();
    extract_archive_entry_to_writer_reader(
        Cursor::new(bytes),
        bytes.len() as u64,
        "external.zip",
        path,
        &mut output,
        1024 * 1024,
        None,
    )?;
    Ok(output)
}

#[test]
fn external_zip_fixture_lists_and_extracts_stored_deflated_and_cp437_entries() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);

    let stored = b"stored payload from an independent ZIP encoder";
    let deflated = b"deflated payload from an independent ZIP encoder";
    let legacy = b"legacy filename payload";
    let bytes = manual_zip(&[
        ManualZipEntry {
            name: b"stored.txt",
            payload: stored,
            method: ManualZipMethod::Stored,
            utf8_name: false,
        },
        ManualZipEntry {
            name: "unicode/世界.txt".as_bytes(),
            payload: deflated,
            method: ManualZipMethod::Deflated,
            utf8_name: true,
        },
        ManualZipEntry {
            name: b"legacy/caf\x82.txt",
            payload: legacy,
            method: ManualZipMethod::Stored,
            utf8_name: false,
        },
    ]);

    let listing = render_archive_reader(
        Cursor::new(&bytes),
        "external.zip",
        bytes.len() as u64,
        0,
        None,
    )
    .expect("render independently encoded ZIP");
    let listing: serde_json::Value =
        serde_json::from_str(&listing).expect("independent ZIP listing JSON");
    let items = listing["listing"]["items"]
        .as_array()
        .expect("independent ZIP items");
    let paths: Vec<&str> = items
        .iter()
        .filter_map(|item| item["path"].as_str())
        .collect();
    assert!(paths.contains(&"stored.txt"));
    assert!(paths.contains(&"unicode/世界.txt"));
    assert!(paths.contains(&"legacy/café.txt"));
    let stored_item = items
        .iter()
        .find(|item| item["path"] == "stored.txt")
        .expect("stored fixture listing item");
    assert_eq!(stored_item["size"], stored.len() as u64);
    assert_eq!(stored_item["packedSize"], stored.len() as u64);
    let deflated_item = items
        .iter()
        .find(|item| item["path"] == "unicode/世界.txt")
        .expect("deflated fixture listing item");
    assert_eq!(deflated_item["size"], deflated.len() as u64);
    assert_eq!(deflated_item["packedSize"], (deflated.len() + 5) as u64);

    assert_eq!(
        extract_manual_zip_entry(&bytes, "stored.txt").expect("extract stored fixture"),
        stored
    );
    assert_eq!(
        extract_manual_zip_entry(&bytes, "unicode/世界.txt")
            .expect("extract deflated UTF-8 fixture"),
        deflated
    );
    assert_eq!(
        extract_manual_zip_entry(&bytes, "legacy/café.txt").expect("extract stored CP437 fixture"),
        legacy
    );
}

#[test]
fn external_zip_fixture_rejects_a_crc_mismatch() {
    let payload = b"unique payload whose CRC must be checked";
    let mut bytes = manual_zip(&[ManualZipEntry {
        name: b"payload.txt",
        payload,
        method: ManualZipMethod::Stored,
        utf8_name: false,
    }]);
    let payload_offset = bytes
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("manual ZIP payload offset");
    bytes[payload_offset] ^= 0x01;

    assert_eq!(
        extract_manual_zip_entry(&bytes, "payload.txt"),
        Err(ReaderPreviewError::Malformed)
    );
}

#[test]
fn external_zip64_fixture_lists_and_extracts_a_small_entry() {
    let payload = b"small payload with forced ZIP64 metadata";
    let bytes = manual_zip64(&[ManualZipEntry {
        name: b"zip64/item.txt",
        payload,
        method: ManualZipMethod::Deflated,
        utf8_name: false,
    }]);

    let listing = render_archive_reader(
        Cursor::new(&bytes),
        "external-zip64.zip",
        bytes.len() as u64,
        0,
        None,
    )
    .expect("render independently encoded ZIP64 archive");
    assert!(listing.contains("zip64/item.txt"));
    assert_eq!(
        extract_manual_zip_entry(&bytes, "zip64/item.txt")
            .expect("extract independently encoded ZIP64 entry"),
        payload
    );
}

#[test]
fn external_zip_fixture_lists_but_does_not_extract_an_unknown_method() {
    let payload = b"opaque compressed bytes";
    let bytes = manual_zip(&[ManualZipEntry {
        name: b"unknown.bin",
        payload,
        method: ManualZipMethod::Unsupported(222),
        utf8_name: false,
    }]);

    let listing = render_archive_reader(
        Cursor::new(&bytes),
        "external.zip",
        bytes.len() as u64,
        0,
        None,
    )
    .expect("list unsupported compression metadata");
    assert!(listing.contains("unknown.bin"));
    assert_eq!(
        extract_manual_zip_entry(&bytes, "unknown.bin"),
        Err(ReaderPreviewError::Malformed)
    );
}

#[test]
fn external_zip_fixture_rejects_an_eocd_comment_length_mismatch() {
    let mut bytes = manual_zip(&[ManualZipEntry {
        name: b"entry.txt",
        payload: b"payload",
        method: ManualZipMethod::Stored,
        utf8_name: false,
    }]);
    let comment_length_offset = bytes.len() - 2;
    bytes[comment_length_offset..].copy_from_slice(&1u16.to_le_bytes());

    assert_eq!(
        render_archive_reader(
            Cursor::new(&bytes),
            "malformed.zip",
            bytes.len() as u64,
            0,
            None,
        ),
        Err(ReaderPreviewError::Malformed)
    );
}

#[test]
fn archive_extract_budget_rejects_oversized_or_extreme_entries() {
    assert!(archive_entry_within_extract_budget(1024, 128));
    assert!(archive_entry_within_extract_budget(0, 0));
    assert!(!archive_entry_within_extract_budget(
        MAX_ARCHIVE_EXTRACT_BYTES + 1,
        1024
    ));
    assert!(!archive_entry_within_extract_budget(
        1024,
        MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES + 1
    ));
    assert!(!archive_entry_within_extract_budget(1_000_001, 1000));
    assert!(!archive_entry_within_extract_budget(1, 0));
}

#[test]
fn encrypted_zip_entries_are_reported_and_not_extracted() {
    let path = std::env::temp_dir().join(format!(
        "quicklook-next-encrypted-{}.zip",
        std::process::id()
    ));
    let file = fs::File::create(&path).expect("create encrypted zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .with_aes_encryption(zip::AesMode::Aes128, "test-password");
    writer
        .start_file("secret.txt", options)
        .expect("start encrypted entry");
    writer.write_all(b"secret").expect("write encrypted entry");
    writer.finish().expect("finish encrypted zip");

    let json = render_archive(path.to_str().unwrap(), None);
    let extracted = extract_archive_entry_to_temp(path.to_str().unwrap(), "secret.txt", None);
    let _ = fs::remove_file(path);

    assert!(json.contains("\"encryptedFileCount\":1"));
    assert!(json.contains("\"isEncrypted\":true"));
    assert!(extracted.is_none());
}

#[test]
fn archive_extract_output_name_is_lossless_and_keeps_safe_extension() {
    let first = archive_extract_output_name("folder/a:b?.png");
    let second = archive_extract_output_name("folder/a<b>.png");

    assert_ne!(first, second);
    assert!(first.ends_with(".png"));
    assert!(first.starts_with("entry-666f6c6465722f613a623f2e706e67"));
}

#[test]
fn archive_extract_discard_only_removes_generated_roots() {
    let generated_root = create_archive_extract_root().expect("generated extract root");
    let generated_target = generated_root.join("entry-test");
    fs::write(&generated_target, b"temporary").expect("write generated extraction");
    discard_archive_extract_path(generated_target.to_str().unwrap());
    assert!(!generated_root.exists());

    let foreign_root = std::env::temp_dir().join(format!(
        "quicklook-next-foreign-root-{}",
        std::process::id()
    ));
    fs::create_dir_all(&foreign_root).expect("create foreign root");
    let foreign_target = foreign_root.join("entry-test");
    fs::write(&foreign_target, b"keep").expect("write foreign extraction");
    discard_archive_extract_path(foreign_target.to_str().unwrap());
    assert!(foreign_target.exists());
    let _ = fs::remove_dir_all(foreign_root);
}
