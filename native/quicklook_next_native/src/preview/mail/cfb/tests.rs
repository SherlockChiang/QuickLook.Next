use std::io::{Cursor, Read, Seek, SeekFrom};

use super::{
    CfbSource, CFB_END_OF_CHAIN, CFB_FAT_SECTOR, CFB_FREE_SECTOR, MAX_CFB_TOTAL_READ_BYTES,
};
use crate::preview::mail::{render_mail_reader, MAX_MAIL_HEADER_BYTES};
use crate::preview::ReaderPreviewError;

const CFB_V4_SECTOR_SIZE: usize = 4096;
const CFB_V4_FAT_SECTOR: u32 = 0;
const CFB_V4_DIRECTORY_SECTOR: u32 = 1;
const CFB_V4_LATE_PROPERTY_SECTOR: u32 = 64;

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn sector_offset(sector: u32) -> usize {
    usize::try_from(sector)
        .expect("fixture sector")
        .checked_add(1)
        .and_then(|index| index.checked_mul(CFB_V4_SECTOR_SIZE))
        .expect("fixture offset")
}

struct DirectoryEntry<'a> {
    name: &'a str,
    object_type: u8,
    child: u32,
    start_sector: u32,
    size: u64,
}

fn write_directory_entry(bytes: &mut [u8], index: usize, entry: DirectoryEntry<'_>) {
    let offset = sector_offset(CFB_V4_DIRECTORY_SECTOR) + index * 128;
    let mut units = entry.name.encode_utf16().collect::<Vec<_>>();
    assert!(units.len() <= 31);
    units.push(0);
    for (unit_index, unit) in units.iter().enumerate() {
        write_u16(bytes, offset + unit_index * 2, *unit);
    }
    write_u16(
        bytes,
        offset + 64,
        u16::try_from(units.len() * 2).expect("fixture name length"),
    );
    bytes[offset + 66] = entry.object_type;
    bytes[offset + 67] = 1;
    write_u32(bytes, offset + 68, CFB_FREE_SECTOR);
    write_u32(bytes, offset + 72, CFB_FREE_SECTOR);
    write_u32(bytes, offset + 76, entry.child);
    write_u32(bytes, offset + 116, entry.start_sector);
    write_u64(bytes, offset + 120, entry.size);
}

fn late_property_msg_fixture() -> Vec<u8> {
    let mut bytes = vec![0u8; sector_offset(CFB_V4_LATE_PROPERTY_SECTOR) + CFB_V4_SECTOR_SIZE];
    bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    write_u16(&mut bytes, 24, 0x003E);
    write_u16(&mut bytes, 26, 4);
    write_u16(&mut bytes, 28, 0xFFFE);
    write_u16(&mut bytes, 30, 12);
    write_u16(&mut bytes, 32, 6);
    write_u32(&mut bytes, 40, 1);
    write_u32(&mut bytes, 44, 1);
    write_u32(&mut bytes, 48, CFB_V4_DIRECTORY_SECTOR);
    write_u32(&mut bytes, 56, 4096);
    write_u32(&mut bytes, 60, CFB_END_OF_CHAIN);
    write_u32(&mut bytes, 64, 0);
    write_u32(&mut bytes, 68, CFB_END_OF_CHAIN);
    write_u32(&mut bytes, 72, 0);
    bytes[76..512].fill(0xFF);
    write_u32(&mut bytes, 76, CFB_V4_FAT_SECTOR);

    let fat = sector_offset(CFB_V4_FAT_SECTOR);
    bytes[fat..fat + CFB_V4_SECTOR_SIZE].fill(0xFF);
    write_u32(&mut bytes, fat, CFB_FAT_SECTOR);
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_V4_DIRECTORY_SECTOR).expect("directory sector") * 4,
        CFB_END_OF_CHAIN,
    );
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_V4_LATE_PROPERTY_SECTOR).expect("property sector") * 4,
        CFB_END_OF_CHAIN,
    );

    write_directory_entry(
        &mut bytes,
        0,
        DirectoryEntry {
            name: "Root Entry",
            object_type: 5,
            child: 1,
            start_sector: CFB_END_OF_CHAIN,
            size: 0,
        },
    );
    write_directory_entry(
        &mut bytes,
        1,
        DirectoryEntry {
            name: "__substg1.0_0037001F",
            object_type: 2,
            child: CFB_FREE_SECTOR,
            start_sector: CFB_V4_LATE_PROPERTY_SECTOR,
            size: CFB_V4_SECTOR_SIZE as u64,
        },
    );

    let property_offset = sector_offset(CFB_V4_LATE_PROPERTY_SECTOR);
    let subject = "Beyond the legacy prefix"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    bytes[property_offset..property_offset + subject.len()].copy_from_slice(&subject);
    bytes
}

struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: usize,
    max_start_seek: u64,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read: 0,
            max_start_seek: 0,
        }
    }
}

impl Read for TrackingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.bytes_read += read;
        Ok(read)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        if let SeekFrom::Start(offset) = position {
            self.max_start_seek = self.max_start_seek.max(offset);
        }
        self.inner.seek(position)
    }
}

extern "C" fn always_cancel() -> bool {
    true
}

#[test]
fn msg_reader_reads_regular_property_beyond_legacy_prefix() {
    let bytes = late_property_msg_fixture();
    let source_len = bytes.len() as u64;
    let property_offset = sector_offset(CFB_V4_LATE_PROPERTY_SECTOR) as u64;
    assert!(property_offset > MAX_MAIL_HEADER_BYTES as u64);
    let mut reader = TrackingReader::new(bytes);

    let json = render_mail_reader(&mut reader, "logical.msg", source_len, 0, None)
        .expect("render late MSG property");

    assert!(json.contains("Subject: Beyond the legacy prefix"));
    assert!(reader.max_start_seek >= property_offset);
    assert!(reader.bytes_read <= MAX_CFB_TOTAL_READ_BYTES + 8);
    assert!(reader.bytes_read < source_len as usize);
}

#[test]
fn cfb_source_enforces_cumulative_read_budget() {
    let bytes = vec![0u8; MAX_CFB_TOTAL_READ_BYTES + 1];
    let source_len = bytes.len() as u64;
    let mut reader = Cursor::new(bytes);
    let mut source = CfbSource::new(&mut reader, source_len, None);

    assert!(source.read_at(0, MAX_CFB_TOTAL_READ_BYTES + 1).is_none());
    assert_eq!(source.failure(), ReaderPreviewError::LimitExceeded);
}

#[test]
fn mail_reader_reports_length_mismatch_and_cancellation() {
    let bytes = b"Subject: pinned mail\r\n\r\nbody";
    assert_eq!(
        render_mail_reader(
            Cursor::new(bytes),
            "logical.eml",
            bytes.len() as u64 + 1,
            0,
            None,
        ),
        Err(ReaderPreviewError::LengthMismatch)
    );
    assert_eq!(
        render_mail_reader(
            Cursor::new(bytes),
            "logical.eml",
            bytes.len() as u64,
            0,
            Some(always_cancel),
        ),
        Err(ReaderPreviewError::Cancelled)
    );
}
