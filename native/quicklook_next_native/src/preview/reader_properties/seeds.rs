//! Small synthetic inputs, never private corpus files or filesystem paths.

use std::io::{Cursor, Write};

pub(super) fn zip(data: &[u8], deflate: bool) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let method = if deflate {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    };
    writer
        .start_file(
            "payload.bin",
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap().into_inner()
}

pub(super) fn torrent(name: &str, length: u32) -> Vec<u8> {
    format!(
        "d4:infod6:lengthi{length}e4:name{}:{name}12:piece lengthi16384e6:pieces0:ee",
        name.len()
    )
    .into_bytes()
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

// CFB v4 with one FAT, one directory and one regular UTF-16 subject stream.
// The 4096-byte stream avoids needing a mini-FAT in this initial seed.
pub(super) fn msg(subject: &str) -> Vec<u8> {
    const END: u32 = 0xFFFF_FFFE;
    const FREE: u32 = 0xFFFF_FFFF;
    let mut bytes = vec![0; 4 * 4096];
    bytes[..8].copy_from_slice(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1");
    put16(&mut bytes, 24, 0x003E);
    put16(&mut bytes, 26, 4);
    put16(&mut bytes, 28, 0xFFFE);
    put16(&mut bytes, 30, 12);
    put16(&mut bytes, 32, 6);
    put32(&mut bytes, 40, 1);
    put32(&mut bytes, 44, 1);
    put32(&mut bytes, 48, 1);
    put32(&mut bytes, 56, 4096);
    put32(&mut bytes, 60, END);
    put32(&mut bytes, 68, END);
    bytes[76..512].fill(0xFF);
    put32(&mut bytes, 76, 0);
    bytes[4096..8192].fill(0xFF);
    put32(&mut bytes, 4096, 0xFFFF_FFFD);
    put32(&mut bytes, 4100, END);
    put32(&mut bytes, 4104, END);

    for (index, name, kind, child, sector, size) in [
        (0, "Root Entry", 5, 1, END, 0u64),
        (1, "__substg1.0_0037001F", 2, FREE, 2, 4096),
    ] {
        let offset = 8192 + index * 128;
        let units: Vec<_> = name.encode_utf16().chain([0]).collect();
        for (i, unit) in units.iter().enumerate() {
            put16(&mut bytes, offset + i * 2, *unit);
        }
        put16(&mut bytes, offset + 64, (units.len() * 2) as u16);
        bytes[offset + 66] = kind;
        bytes[offset + 67] = 1;
        put32(&mut bytes, offset + 68, FREE);
        put32(&mut bytes, offset + 72, FREE);
        put32(&mut bytes, offset + 76, child);
        put32(&mut bytes, offset + 116, sector);
        bytes[offset + 120..offset + 128].copy_from_slice(&size.to_le_bytes());
    }
    for (i, unit) in subject.encode_utf16().enumerate() {
        put16(&mut bytes, 12288 + i * 2, unit);
    }
    bytes
}

pub(super) fn sqlite(page_size: usize, version: u32) -> Vec<u8> {
    let mut bytes = vec![0; page_size];
    bytes[..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
    bytes[18..24].copy_from_slice(&[2, 2, 0, 64, 32, 32]);
    bytes[28..32].copy_from_slice(&1u32.to_be_bytes());
    bytes[44..48].copy_from_slice(&4u32.to_be_bytes());
    bytes[56..60].copy_from_slice(&1u32.to_be_bytes());
    bytes[60..64].copy_from_slice(&version.to_be_bytes());
    bytes[100] = 0x0D; // Empty schema leaf.
    bytes[105..107].copy_from_slice(&(page_size as u16).to_be_bytes());
    bytes
}

pub(super) fn wal(page: &[u8], big_endian: bool) -> Vec<u8> {
    let mut bytes = vec![0; 56];
    let magic = if big_endian {
        0x377F_0683u32
    } else {
        0x377F_0682
    };
    bytes[..4].copy_from_slice(&magic.to_be_bytes());
    bytes[4..8].copy_from_slice(&3_007_000u32.to_be_bytes());
    bytes[8..12].copy_from_slice(&(page.len() as u32).to_be_bytes());
    bytes[16..24].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    bytes[32..36].copy_from_slice(&1u32.to_be_bytes());
    bytes[36..40].copy_from_slice(&1u32.to_be_bytes());
    bytes.copy_within(16..24, 40);
    bytes.extend_from_slice(page);
    resign_wal(&mut bytes, big_endian);
    bytes
}

// Independent fixture checksum, also allowing mutations past the checksum gate.
pub(super) fn resign_wal(bytes: &mut [u8], big_endian: bool) {
    if bytes.len() < 56 || !(bytes.len() - 56).is_multiple_of(8) {
        return;
    }
    let mut sums = [0u32; 2];
    for range in [0..24, 32..40, 56..bytes.len()] {
        for pair in bytes[range.clone()].chunks_exact(8) {
            for i in 0..2 {
                let word = pair[i * 4..i * 4 + 4].try_into().unwrap();
                let value = if big_endian {
                    u32::from_be_bytes(word)
                } else {
                    u32::from_le_bytes(word)
                };
                sums[i] = sums[i].wrapping_add(value).wrapping_add(sums[1 - i]);
            }
        }
        let offset = if range.start == 0 { 24 } else { 48 };
        bytes[offset..offset + 4].copy_from_slice(&sums[0].to_be_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&sums[1].to_be_bytes());
    }
}
