use super::{
    append_chm_itsp_summary, chm_system_summary, ChmDirectoryEntry, ChmItsfHeader,
    CHM_ITSF_V2_HEADER_LEN, CHM_ITSF_V3_HEADER_LEN, CHM_ITSP_HEADER_LEN, CHM_PMGL_HEADER_LEN,
};

const DIR_OFFSET: usize = 0x100;
const BLOCK_LEN: usize = 0x100;
const DIR_LEN: usize = CHM_ITSP_HEADER_LEN + BLOCK_LEN;
const V3_DATA_OFFSET: usize = 0x300;
const SYSTEM_OFFSET: usize = 0x20;
const SYSTEM_LEN: usize = 32;

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn encint(value: usize) -> Vec<u8> {
    let mut value = value;
    let mut encoded = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        encoded.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    encoded.reverse();
    encoded
}

fn write_encint(bytes: &mut [u8], offset: &mut usize, value: usize) {
    let encoded = encint(value);
    bytes[*offset..*offset + encoded.len()].copy_from_slice(&encoded);
    *offset += encoded.len();
}

fn write_entry(
    bytes: &mut [u8],
    offset: &mut usize,
    name: &str,
    section: usize,
    file_offset: usize,
    len: usize,
) {
    write_encint(bytes, offset, name.len());
    bytes[*offset..*offset + name.len()].copy_from_slice(name.as_bytes());
    *offset += name.len();
    for value in [section, file_offset, len] {
        write_encint(bytes, offset, value);
    }
}

fn fixture(version: u32) -> Vec<u8> {
    let header_len = match version {
        2 => CHM_ITSF_V2_HEADER_LEN,
        3 => CHM_ITSF_V3_HEADER_LEN,
        _ => panic!("unsupported fixture version"),
    };
    let data_offset = if version == 2 {
        DIR_OFFSET + DIR_LEN
    } else {
        V3_DATA_OFFSET
    };
    let mut bytes = vec![0u8; 0x500];
    bytes[..4].copy_from_slice(b"ITSF");
    write_u32(&mut bytes, 4, version);
    write_u32(&mut bytes, 8, header_len as u32);
    write_u32(&mut bytes, 0x10, 1_700_000_000);
    write_u32(&mut bytes, 0x14, 0x0409);
    write_u64(&mut bytes, 0x48, DIR_OFFSET as u64);
    write_u64(&mut bytes, 0x50, DIR_LEN as u64);
    if version == 3 {
        write_u64(&mut bytes, 0x58, data_offset as u64);
    }

    bytes[DIR_OFFSET..DIR_OFFSET + 4].copy_from_slice(b"ITSP");
    write_u32(&mut bytes, DIR_OFFSET + 4, 1);
    write_u32(&mut bytes, DIR_OFFSET + 8, CHM_ITSP_HEADER_LEN as u32);
    write_u32(&mut bytes, DIR_OFFSET + 0x10, BLOCK_LEN as u32);
    write_u32(&mut bytes, DIR_OFFSET + 0x18, 2);
    write_u32(&mut bytes, DIR_OFFSET + 0x1c, 3);
    write_u32(&mut bytes, DIR_OFFSET + 0x20, 4);
    write_u32(&mut bytes, DIR_OFFSET + 0x28, 7);

    let block_offset = DIR_OFFSET + CHM_ITSP_HEADER_LEN;
    let block_end = block_offset + BLOCK_LEN;
    bytes[block_offset..block_offset + 4].copy_from_slice(b"PMGL");
    let mut entry_offset = block_offset + CHM_PMGL_HEADER_LEN;
    write_entry(&mut bytes, &mut entry_offset, "/index.htm", 0, 123, 45);
    write_entry(
        &mut bytes,
        &mut entry_offset,
        "::DataSpace/Storage/MSCompressed/Content",
        1,
        0,
        200,
    );
    write_entry(
        &mut bytes,
        &mut entry_offset,
        "/#SYSTEM",
        0,
        SYSTEM_OFFSET,
        SYSTEM_LEN,
    );
    write_u32(
        &mut bytes,
        block_offset + 4,
        (block_end - entry_offset) as u32,
    );

    let system = data_offset + SYSTEM_OFFSET;
    write_u32(&mut bytes, system, 3);
    bytes[system + 4..system + 6].copy_from_slice(&3u16.to_le_bytes());
    bytes[system + 6..system + 8].copy_from_slice(&10u16.to_le_bytes());
    bytes[system + 8..system + 18].copy_from_slice(b"Help Title");
    bytes[system + 18..system + 20].copy_from_slice(&2u16.to_le_bytes());
    bytes[system + 20..system + 22].copy_from_slice(&10u16.to_le_bytes());
    bytes[system + 22..system + 32].copy_from_slice(b"/index.htm");
    bytes
}

fn summary(bytes: &[u8]) -> String {
    let header = ChmItsfHeader::parse(bytes).expect("valid ITSF fixture");
    let mut text = String::new();
    append_chm_itsp_summary(&mut text, bytes, &header);
    text
}

#[test]
fn chm_v3_uses_real_itsf_layout_and_data_base() {
    let bytes = fixture(3);
    let header = ChmItsfHeader::parse(&bytes).expect("v3 header");

    assert_eq!(header.header_len, 0x60);
    assert_eq!(header.last_modified, 1_700_000_000);
    assert_eq!(header.lang_id, 0x0409);
    assert_eq!(header.dir_offset, 0x100);
    assert_eq!(header.dir_len, DIR_LEN as u64);
    assert_eq!(header.data_offset, 0x300);

    let text = summary(&bytes);
    assert!(text.contains("ITSP version: 1"));
    assert!(text.contains("ITSP header length: 84 bytes"));
    assert!(text.contains("Directory block length: 256 bytes"));
    assert!(text.contains("Directory block count: 7"));
    assert!(text.contains("Directory index depth/root/head: 2/3/4"));
    assert!(text.contains("Directory entries: /index.htm [section 0, offset 123, 45 B]"));
    assert!(text.contains("Compressed streams: ::DataSpace/Storage/MSCompressed/Content (200 B)"));
    assert!(text.contains("Title: Help Title"));
    assert!(text.contains("Default topic: /index.htm"));
}

#[test]
fn chm_v2_derives_data_base_with_checked_addition() {
    let bytes = fixture(2);
    let header = ChmItsfHeader::parse(&bytes).expect("v2 header");

    assert_eq!(header.header_len, 0x58);
    assert_eq!(header.data_offset, (DIR_OFFSET + DIR_LEN) as u64);
    assert!(summary(&bytes).contains("Title: Help Title"));

    let mut hostile = bytes[..CHM_ITSF_V2_HEADER_LEN].to_vec();
    write_u64(&mut hostile, 0x48, u64::MAX);
    write_u64(&mut hostile, 0x50, 1);
    assert!(ChmItsfHeader::parse(&hostile).is_none());
}

#[test]
fn chm_itsp_summary_rejects_hostile_directory_offsets() {
    let mut bytes = fixture(3);
    write_u64(&mut bytes, 0x48, u64::MAX);
    let header = ChmItsfHeader::parse(&bytes).expect("v3 header remains structurally valid");
    let mut text = String::new();

    append_chm_itsp_summary(&mut text, &bytes, &header);

    assert!(text.is_empty());
}

#[test]
fn chm_header_and_itsp_truncation_fail_soft() {
    let bytes = fixture(3);
    assert!(ChmItsfHeader::parse(&bytes[..CHM_ITSF_V3_HEADER_LEN - 1]).is_none());

    let truncated = &bytes[..DIR_OFFSET + CHM_ITSP_HEADER_LEN - 1];
    let header = ChmItsfHeader::parse(truncated).expect("complete ITSF header");
    let mut text = String::new();
    append_chm_itsp_summary(&mut text, truncated, &header);
    assert!(text.is_empty());

    let mut unsupported_itsp = bytes.clone();
    write_u32(&mut unsupported_itsp, DIR_OFFSET + 4, 2);
    assert!(summary(&unsupported_itsp).is_empty());

    let mut oversized_itsp = bytes;
    write_u32(
        &mut oversized_itsp,
        DIR_OFFSET + 8,
        (CHM_ITSP_HEADER_LEN + 4) as u32,
    );
    assert!(summary(&oversized_itsp).is_empty());
}

#[test]
fn chm_directory_rejects_out_of_bounds_pmgl() {
    let mut bytes = fixture(3);
    write_u32(&mut bytes, DIR_OFFSET + 0x10, u32::MAX);

    let text = summary(&bytes);

    assert!(text.contains("ITSP version: 1"));
    assert!(!text.contains("Directory entries:"));
}

#[test]
fn chm_directory_rejects_unterminated_encint() {
    let mut bytes = fixture(3);
    let block_offset = DIR_OFFSET + CHM_ITSP_HEADER_LEN;
    let entries_offset = block_offset + CHM_PMGL_HEADER_LEN;
    bytes[entries_offset..entries_offset + 8].fill(0x80);
    write_u32(
        &mut bytes,
        block_offset + 4,
        (BLOCK_LEN - CHM_PMGL_HEADER_LEN - 8) as u32,
    );

    assert!(!summary(&bytes).contains("Directory entries:"));
}

#[test]
fn chm_system_stream_rejects_relative_range_overflow() {
    let bytes = fixture(3);
    let entries = vec![ChmDirectoryEntry {
        name: "/#SYSTEM".to_string(),
        section: 0,
        offset: usize::MAX,
        len: SYSTEM_LEN,
    }];

    assert!(chm_system_summary(&bytes, V3_DATA_OFFSET, &entries).is_empty());
}

#[test]
fn chm_system_stream_caps_all_scanned_fields() {
    let mut bytes = vec![0u8; 128];
    let mut offset = 4usize;
    for _ in 0..8 {
        bytes[offset..offset + 2].copy_from_slice(&99u16.to_le_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&1u16.to_le_bytes());
        bytes[offset + 4] = b'x';
        offset += 5;
    }
    bytes[offset..offset + 2].copy_from_slice(&3u16.to_le_bytes());
    bytes[offset + 2..offset + 4].copy_from_slice(&10u16.to_le_bytes());
    bytes[offset + 4..offset + 14].copy_from_slice(b"Late Title");
    offset += 14;
    let entries = vec![ChmDirectoryEntry {
        name: "/#SYSTEM".to_string(),
        section: 0,
        offset: 0,
        len: offset,
    }];

    assert!(chm_system_summary(&bytes, 0, &entries).is_empty());
}
