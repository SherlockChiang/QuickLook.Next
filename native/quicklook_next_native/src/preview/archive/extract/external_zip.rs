use std::io::{Cursor, Write};

use super::super::super::listing::render_archive_reader;
use super::super::extract_archive_entry_to_writer_reader;
use crate::preview::archive::MAX_ARCHIVE_EXTRACT_BYTES;
use crate::preview::ReaderPreviewError;

#[derive(Clone, Copy)]
struct Entry<'a> {
    name: &'a [u8],
    data: &'a [u8],
    method: u16,
    utf8: bool,
}

fn p16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn p32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn p64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn raw_stored_deflate(data: &[u8]) -> Vec<u8> {
    let length = u16::try_from(data.len()).expect("small independent DEFLATE fixture");
    let mut out = Vec::with_capacity(data.len() + 5);
    out.push(1);
    p16(&mut out, length);
    p16(&mut out, !length);
    out.extend_from_slice(data);
    out
}

fn make_zip(entries: &[Entry<'_>], zip64: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let compressed = if entry.method == 8 {
            raw_stored_deflate(entry.data)
        } else {
            entry.data.to_vec()
        };
        let name_len = u16::try_from(entry.name.len()).expect("fixture name");
        let flags = if entry.utf8 { 1 << 11 } else { 0 };
        let crc = crc32(entry.data);
        let csize = compressed.len() as u64;
        let usize = entry.data.len() as u64;
        let offset = out.len() as u64;
        p32(&mut out, 0x0403_4b50);
        p16(&mut out, if zip64 { 45 } else { 20 });
        p16(&mut out, flags);
        p16(&mut out, entry.method);
        p16(&mut out, 0);
        p16(&mut out, 0x21);
        p32(&mut out, crc);
        p32(&mut out, if zip64 { u32::MAX } else { csize as u32 });
        p32(&mut out, if zip64 { u32::MAX } else { usize as u32 });
        p16(&mut out, name_len);
        p16(&mut out, if zip64 { 20 } else { 0 });
        out.extend_from_slice(entry.name);
        if zip64 {
            p16(&mut out, 1);
            p16(&mut out, 16);
            p64(&mut out, usize);
            p64(&mut out, csize);
        }
        out.extend_from_slice(&compressed);
        central.push((
            entry.name.to_vec(),
            flags,
            entry.method,
            crc,
            csize,
            usize,
            offset,
        ));
    }

    let central_offset = out.len() as u64;
    for (name, flags, method, crc, csize, usize, offset) in &central {
        p32(&mut out, 0x0201_4b50);
        p16(&mut out, if zip64 { 45 } else { 20 });
        p16(&mut out, if zip64 { 45 } else { 20 });
        p16(&mut out, *flags);
        p16(&mut out, *method);
        p16(&mut out, 0);
        p16(&mut out, 0x21);
        p32(&mut out, *crc);
        p32(&mut out, if zip64 { u32::MAX } else { *csize as u32 });
        p32(&mut out, if zip64 { u32::MAX } else { *usize as u32 });
        p16(&mut out, name.len() as u16);
        p16(&mut out, if zip64 { 28 } else { 0 });
        p16(&mut out, 0);
        p16(&mut out, 0);
        p16(&mut out, 0);
        p32(&mut out, 0);
        p32(&mut out, if zip64 { u32::MAX } else { *offset as u32 });
        out.extend_from_slice(name);
        if zip64 {
            p16(&mut out, 1);
            p16(&mut out, 24);
            p64(&mut out, *usize);
            p64(&mut out, *csize);
            p64(&mut out, *offset);
        }
    }
    let central_size = out.len() as u64 - central_offset;
    let count = central.len() as u64;
    if zip64 {
        let end_offset = out.len() as u64;
        p32(&mut out, 0x0606_4b50);
        p64(&mut out, 44);
        p16(&mut out, 45);
        p16(&mut out, 45);
        p32(&mut out, 0);
        p32(&mut out, 0);
        p64(&mut out, count);
        p64(&mut out, count);
        p64(&mut out, central_size);
        p64(&mut out, central_offset);
        p32(&mut out, 0x0706_4b50);
        p32(&mut out, 0);
        p64(&mut out, end_offset);
        p32(&mut out, 1);
    }
    p32(&mut out, 0x0605_4b50);
    p16(&mut out, 0);
    p16(&mut out, 0);
    p16(&mut out, if zip64 { u16::MAX } else { count as u16 });
    p16(&mut out, if zip64 { u16::MAX } else { count as u16 });
    p32(&mut out, if zip64 { u32::MAX } else { central_size as u32 });
    p32(
        &mut out,
        if zip64 {
            u32::MAX
        } else {
            central_offset as u32
        },
    );
    p16(&mut out, 0);
    out
}

fn extract(bytes: &[u8], name: &str) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut output = Vec::new();
    extract_archive_entry_to_writer_reader(
        Cursor::new(bytes),
        bytes.len() as u64,
        "external.zip",
        name,
        &mut output,
        MAX_ARCHIVE_EXTRACT_BYTES,
        None,
    )?;
    output.flush().expect("memory writer flush");
    Ok(output)
}

#[test]
fn external_zip_fixture_covers_stored_deflate_unicode_cp437_and_crc() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    let stored = b"stored payload";
    let deflated = b"deflated payload";
    let legacy = b"legacy payload";
    let mut bytes = make_zip(
        &[
            Entry {
                name: b"stored.txt",
                data: stored,
                method: 0,
                utf8: false,
            },
            Entry {
                name: "unicode/世界.txt".as_bytes(),
                data: deflated,
                method: 8,
                utf8: true,
            },
            Entry {
                name: b"legacy/caf\x82.txt",
                data: legacy,
                method: 0,
                utf8: false,
            },
        ],
        false,
    );
    let listing: serde_json::Value = serde_json::from_str(
        &render_archive_reader(
            Cursor::new(&bytes),
            "external.zip",
            bytes.len() as u64,
            0,
            None,
        )
        .expect("independent ZIP listing"),
    )
    .expect("listing JSON");
    let items = listing["listing"]["items"].as_array().expect("items");
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
        .expect("stored listing item");
    assert_eq!(stored_item["size"], stored.len() as u64);
    assert_eq!(stored_item["packedSize"], stored.len() as u64);
    let deflated_item = items
        .iter()
        .find(|item| item["path"] == "unicode/世界.txt")
        .expect("deflated listing item");
    assert_eq!(deflated_item["size"], deflated.len() as u64);
    assert_eq!(deflated_item["packedSize"], (deflated.len() + 5) as u64);
    assert_eq!(extract(&bytes, "stored.txt").expect("stored"), stored);
    assert_eq!(
        extract(&bytes, "unicode/世界.txt").expect("deflated"),
        deflated
    );
    assert_eq!(extract(&bytes, "legacy/café.txt").expect("CP437"), legacy);
    let payload_offset = bytes
        .windows(stored.len())
        .position(|w| w == stored)
        .unwrap();
    bytes[payload_offset] ^= 1;
    assert_eq!(
        extract(&bytes, "stored.txt"),
        Err(ReaderPreviewError::Malformed)
    );
}

#[test]
fn external_zip_fixture_covers_zip64_unknown_method_and_bad_eocd() {
    let payload = b"zip64 payload";
    let bytes = make_zip(
        &[Entry {
            name: b"zip64/item.txt",
            data: payload,
            method: 8,
            utf8: false,
        }],
        true,
    );
    let listing = render_archive_reader(
        Cursor::new(&bytes),
        "zip64.zip",
        bytes.len() as u64,
        0,
        None,
    )
    .expect("ZIP64 listing");
    assert!(listing.contains("zip64/item.txt"));
    assert_eq!(
        extract(&bytes, "zip64/item.txt").expect("ZIP64 extraction"),
        payload
    );

    let unknown = make_zip(
        &[Entry {
            name: b"unknown.bin",
            data: b"opaque",
            method: 222,
            utf8: false,
        }],
        false,
    );
    assert!(render_archive_reader(
        Cursor::new(&unknown),
        "unknown.zip",
        unknown.len() as u64,
        0,
        None
    )
    .expect("unknown method listing")
    .contains("unknown.bin"));
    assert_eq!(
        extract(&unknown, "unknown.bin"),
        Err(ReaderPreviewError::Malformed)
    );

    let mut malformed = make_zip(
        &[Entry {
            name: b"entry.txt",
            data: b"payload",
            method: 0,
            utf8: false,
        }],
        false,
    );
    let end = malformed.len() - 2;
    malformed[end..].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        render_archive_reader(
            Cursor::new(&malformed),
            "bad.zip",
            malformed.len() as u64,
            0,
            None
        ),
        Err(ReaderPreviewError::Malformed)
    );
}
