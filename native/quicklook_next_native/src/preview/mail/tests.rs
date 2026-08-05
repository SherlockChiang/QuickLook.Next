use super::{
    append_msg_compound_summary, decode_base64, decode_mail_header_value,
    mail_attachment_filename_from_disposition, mail_attachment_summary, mail_header_parameter,
    mail_header_parameters, mail_mime_part_summaries, mail_text_body_preview, parse_mail_headers,
    CFB_END_OF_CHAIN, CFB_FAT_SECTOR, CFB_FREE_SECTOR, MAX_MAIL_DECODED_BODY_BYTES,
    MAX_MAIL_DECODED_HEADER_BYTES, MAX_MAIL_ENCODED_WORDS, MAX_MAIL_FILENAME_BYTES,
    MAX_MAIL_HEADERS, MAX_MAIL_HEADER_PARAMETERS, MAX_MAIL_HEADER_VALUE_BYTES,
    MAX_MAIL_MIME_BOUNDARY_BYTES, MAX_MAIL_MIME_DEPTH, MAX_MAIL_MIME_PARTS,
    MAX_MAIL_TEXT_PREVIEW_CHARS,
};

const CFB_SECTOR_SIZE: usize = 512;
const CFB_FAT_SECTOR_ID: u32 = 0;
const CFB_FIRST_DIRECTORY_SECTOR: u32 = 1;
const CFB_SECOND_DIRECTORY_SECTOR: u32 = 2;
const CFB_MINI_FAT_SECTOR_ID: u32 = 3;
const CFB_MINI_STREAM_SECTOR_ID: u32 = 4;

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn cfb_sector_offset(sector: u32) -> usize {
    usize::try_from(sector)
        .expect("fixture sector")
        .checked_add(1)
        .and_then(|index| index.checked_mul(CFB_SECTOR_SIZE))
        .expect("fixture offset")
}

fn cfb_directory_entry_offset(index: usize) -> usize {
    let sector = CFB_FIRST_DIRECTORY_SECTOR
        .checked_add(u32::try_from(index / 4).expect("fixture directory sector"))
        .expect("fixture directory sector");
    cfb_sector_offset(sector) + (index % 4) * 128
}

struct FixtureDirectoryEntry<'a> {
    name: &'a str,
    object_type: u8,
    right_sibling: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

fn write_cfb_directory_entry(bytes: &mut [u8], index: usize, entry: FixtureDirectoryEntry<'_>) {
    let offset = cfb_directory_entry_offset(index);
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
    write_u32(bytes, offset + 72, entry.right_sibling);
    write_u32(bytes, offset + 76, entry.child);
    write_u32(bytes, offset + 116, entry.start_sector);
    write_u64(bytes, offset + 120, entry.size);
}

fn write_cfb_mini_stream(bytes: &mut [u8], mini_sector: usize, value: &[u8]) -> u64 {
    assert!(value.len() <= 64);
    let offset = cfb_sector_offset(CFB_MINI_STREAM_SECTOR_ID) + mini_sector * 64;
    bytes[offset..offset + value.len()].copy_from_slice(value);
    u64::try_from(value.len()).expect("fixture mini stream length")
}

fn write_cfb_mini_utf16(bytes: &mut [u8], mini_sector: usize, value: &str) -> u64 {
    let encoded = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    write_cfb_mini_stream(bytes, mini_sector, &encoded)
}

fn real_msg_fixture() -> Vec<u8> {
    let mut bytes = vec![0u8; cfb_sector_offset(CFB_MINI_STREAM_SECTOR_ID) + CFB_SECTOR_SIZE];
    bytes[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    write_u16(&mut bytes, 24, 0x003E);
    write_u16(&mut bytes, 26, 3);
    write_u16(&mut bytes, 28, 0xFFFE);
    write_u16(&mut bytes, 30, 9);
    write_u16(&mut bytes, 32, 6);
    write_u32(&mut bytes, 40, 0);
    write_u32(&mut bytes, 44, 1);
    write_u32(&mut bytes, 48, CFB_FIRST_DIRECTORY_SECTOR);
    write_u32(&mut bytes, 56, 4096);
    write_u32(&mut bytes, 60, CFB_MINI_FAT_SECTOR_ID);
    write_u32(&mut bytes, 64, 1);
    write_u32(&mut bytes, 68, CFB_END_OF_CHAIN);
    write_u32(&mut bytes, 72, 0);
    bytes[76..CFB_SECTOR_SIZE].fill(0xFF);
    write_u32(&mut bytes, 76, CFB_FAT_SECTOR_ID);

    let fat = cfb_sector_offset(CFB_FAT_SECTOR_ID);
    bytes[fat..fat + CFB_SECTOR_SIZE].fill(0xFF);
    write_u32(&mut bytes, fat, CFB_FAT_SECTOR);
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_FIRST_DIRECTORY_SECTOR).expect("fixture sector") * 4,
        CFB_SECOND_DIRECTORY_SECTOR,
    );
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_SECOND_DIRECTORY_SECTOR).expect("fixture sector") * 4,
        CFB_END_OF_CHAIN,
    );
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_MINI_FAT_SECTOR_ID).expect("fixture sector") * 4,
        CFB_END_OF_CHAIN,
    );
    write_u32(
        &mut bytes,
        fat + usize::try_from(CFB_MINI_STREAM_SECTOR_ID).expect("fixture sector") * 4,
        CFB_END_OF_CHAIN,
    );

    let mini_fat = cfb_sector_offset(CFB_MINI_FAT_SECTOR_ID);
    bytes[mini_fat..mini_fat + CFB_SECTOR_SIZE].fill(0xFF);
    for mini_sector in 0..5usize {
        write_u32(&mut bytes, mini_fat + mini_sector * 4, CFB_END_OF_CHAIN);
    }

    let subject_len = write_cfb_mini_utf16(&mut bytes, 0, "Quarterly Update");
    let sender_len = write_cfb_mini_utf16(&mut bytes, 1, "Alice Example");
    let recipients_len = write_cfb_mini_utf16(&mut bytes, 2, "Bob Example; Carol Example");
    let sent_filetime = 116_444_736_000_000_000u64 + 1_700_000_000u64 * 10_000_000;
    let mut properties = vec![0u8; 48];
    write_u32(&mut properties, 32, 0x0E06_0040);
    write_u64(&mut properties, 40, sent_filetime);
    let properties_len = write_cfb_mini_stream(&mut bytes, 3, &properties);
    let body_len = write_cfb_mini_stream(&mut bytes, 4, &[0, 0]);

    write_cfb_directory_entry(
        &mut bytes,
        0,
        FixtureDirectoryEntry {
            name: "Root Entry",
            object_type: 5,
            right_sibling: CFB_FREE_SECTOR,
            child: 1,
            start_sector: CFB_MINI_STREAM_SECTOR_ID,
            size: CFB_SECTOR_SIZE as u64,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        1,
        FixtureDirectoryEntry {
            name: "__substg1.0_0037001F",
            object_type: 2,
            right_sibling: 2,
            child: CFB_FREE_SECTOR,
            start_sector: 0,
            size: subject_len,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        2,
        FixtureDirectoryEntry {
            name: "__substg1.0_0C1A001F",
            object_type: 2,
            right_sibling: 3,
            child: CFB_FREE_SECTOR,
            start_sector: 1,
            size: sender_len,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        3,
        FixtureDirectoryEntry {
            name: "__substg1.0_0E04001F",
            object_type: 2,
            right_sibling: 4,
            child: CFB_FREE_SECTOR,
            start_sector: 2,
            size: recipients_len,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        4,
        FixtureDirectoryEntry {
            name: "__properties_version1.0",
            object_type: 2,
            right_sibling: 5,
            child: CFB_FREE_SECTOR,
            start_sector: 3,
            size: properties_len,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        5,
        FixtureDirectoryEntry {
            name: "__substg1.0_1000001F",
            object_type: 2,
            right_sibling: 6,
            child: CFB_FREE_SECTOR,
            start_sector: 4,
            size: body_len,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        6,
        FixtureDirectoryEntry {
            name: "__attach_version1.0_#00000000",
            object_type: 1,
            right_sibling: 7,
            child: CFB_FREE_SECTOR,
            start_sector: CFB_END_OF_CHAIN,
            size: 0,
        },
    );
    write_cfb_directory_entry(
        &mut bytes,
        7,
        FixtureDirectoryEntry {
            name: "__recip_version1.0_#00000000",
            object_type: 1,
            right_sibling: CFB_FREE_SECTOR,
            child: CFB_FREE_SECTOR,
            start_sector: CFB_END_OF_CHAIN,
            size: 0,
        },
    );
    bytes
}

fn msg_summary(bytes: &[u8]) -> String {
    let mut text = String::new();
    append_msg_compound_summary(&mut text, bytes);
    text
}

#[test]
fn mail_header_parser_caps_header_count_and_values() {
    let mut content = format!(
        "Subject: {}\r\n",
        "x".repeat(MAX_MAIL_HEADER_VALUE_BYTES + 100)
    );
    for index in 0..(MAX_MAIL_HEADERS + 8) {
        content.push_str(&format!("X-Test-{index}: value\r\n"));
    }
    content.push_str("\r\n");

    let headers = parse_mail_headers(&content);

    assert_eq!(headers.len(), MAX_MAIL_HEADERS);
    assert_eq!(headers[0].1.len(), MAX_MAIL_HEADER_VALUE_BYTES);

    let utf8 = format!(
        "Subject: {}\r\n\r\n",
        "é".repeat(MAX_MAIL_HEADER_VALUE_BYTES)
    );
    let utf8_headers = parse_mail_headers(&utf8);
    assert_eq!(utf8_headers[0].1.len(), MAX_MAIL_HEADER_VALUE_BYTES);
    assert!(utf8_headers[0].1.is_char_boundary(utf8_headers[0].1.len()));
}

#[test]
fn mail_mime_summary_caps_parts_and_rejects_hostile_boundary() {
    let mut content = String::new();
    for _ in 0..(MAX_MAIL_MIME_PARTS + 8) {
        content.push_str("--parts\r\nContent-Type: text/plain\r\n\r\nhello\r\n");
    }
    content.push_str("--parts--\r\n");

    assert_eq!(
        mail_mime_part_summaries(&content, "parts").len(),
        MAX_MAIL_MIME_PARTS
    );
    assert!(
        mail_mime_part_summaries(&content, &"x".repeat(MAX_MAIL_MIME_BOUNDARY_BYTES + 1))
            .is_empty()
    );
    assert!(mail_mime_part_summaries(&content, "bad\0boundary").is_empty());

    let nested_boundary = "x".repeat(MAX_MAIL_MIME_BOUNDARY_BYTES + 1);
    let nested = format!(
        "--root\r\nContent-Type: multipart/mixed; boundary=\"{nested_boundary}\"\r\n\r\n--{nested_boundary}\r\nContent-Type: text/plain\r\n\r\nchild\r\n--{nested_boundary}--\r\n--root--\r\n"
    );
    assert_eq!(mail_mime_part_summaries(&nested, "root").len(), 1);

    let false_delimiters = "--root\r\nContent-Type: text/plain\r\n\r\nprefix --root suffix\r\n--rootSuffix\r\n--root--\r\n";
    assert_eq!(mail_mime_part_summaries(false_delimiters, "root").len(), 1);
}

#[test]
fn mail_mime_summary_caps_nesting_depth() {
    fn nested_part(depth: usize) -> String {
        let boundary = format!("nested-{}", depth + 1);
        let body = if depth > MAX_MAIL_MIME_DEPTH {
            "Content-Type: text/plain\r\n\r\nleaf".to_string()
        } else {
            nested_part(depth + 1)
        };
        format!(
            "Content-Type: multipart/mixed; boundary={boundary}\r\n\r\n--{boundary}\r\n{body}\r\n--{boundary}--\r\n"
        )
    }

    let content = format!("--root\r\n{}\r\n--root--\r\n", nested_part(0));
    let summaries = mail_mime_part_summaries(&content, "root");
    assert_eq!(summaries.len(), MAX_MAIL_MIME_DEPTH + 1);
    assert!(!summaries
        .iter()
        .any(|summary| summary.contains("text/plain")));
}

#[test]
fn mail_decoders_keep_header_and_body_budgets() {
    let encoded_words = "=?UTF-8?Q?xxxxxxxxxxxxxxxx?=".repeat(1024);
    assert!(decode_mail_header_value(&encoded_words).len() <= MAX_MAIL_DECODED_HEADER_BYTES);
    let limited_words = "=?UTF-8?Q?x?=".repeat(MAX_MAIL_ENCODED_WORDS + 1);
    assert!(decode_mail_header_value(&limited_words).contains("=?UTF-8?Q?x?="));

    assert_eq!(
        decode_base64("QUJDRA==", 4).as_deref(),
        Some(b"ABCD".as_slice())
    );
    assert!(decode_base64("QUJDRA==", 3).is_none());
    assert!(decode_base64("QQ==trailing", 64).is_none());

    let oversized_body = "x".repeat(MAX_MAIL_DECODED_BODY_BYTES + 1);
    assert!(mail_text_body_preview(&oversized_body, None).is_none());

    let preview_source = format!("{} second", "x".repeat(MAX_MAIL_TEXT_PREVIEW_CHARS - 1));
    let preview = mail_text_body_preview(&preview_source, None).expect("text preview");
    assert!(preview.chars().count() <= MAX_MAIL_TEXT_PREVIEW_CHARS);
    assert!(!preview.ends_with(char::is_whitespace));

    let disposition = format!(
        "attachment; filename=\"{}\"",
        "x".repeat(MAX_MAIL_FILENAME_BYTES + 100)
    );
    let filename =
        mail_attachment_filename_from_disposition(&disposition).expect("attachment filename");
    assert_eq!(filename.len(), MAX_MAIL_FILENAME_BYTES);

    let parameters = (0..MAX_MAIL_HEADER_PARAMETERS + 8)
        .map(|index| format!("key{index}=value"))
        .collect::<Vec<_>>()
        .join(";");
    assert_eq!(
        mail_header_parameters(&format!("text/plain;{parameters}")).len(),
        MAX_MAIL_HEADER_PARAMETERS
    );
}

#[test]
fn msg_compound_summary_reads_real_fat_and_mini_streams() {
    let text = msg_summary(&real_msg_fixture());

    assert!(text.contains("Recipients: 1"));
    assert!(text.contains("Attachments: 1"));
    assert!(text.contains("Subject: Quarterly Update"));
    assert!(text.contains("Sender: Alice Example"));
    assert!(text.contains("Recipients display: Bob Example; Carol Example"));
    assert!(text.contains("Sent time:"));
    assert!(text.contains("Body available: yes"));
}

#[test]
fn msg_compound_summary_rejects_truncated_and_invalid_headers() {
    let bytes = real_msg_fixture();
    assert!(msg_summary(&bytes[..511]).is_empty());

    let mut invalid_byte_order = bytes.clone();
    write_u16(&mut invalid_byte_order, 28, 0);
    assert!(msg_summary(&invalid_byte_order).is_empty());

    let mut invalid_sector_shift = bytes.clone();
    write_u16(&mut invalid_sector_shift, 30, 10);
    assert!(msg_summary(&invalid_sector_shift).is_empty());

    let mut excessive_fat = bytes;
    write_u32(&mut excessive_fat, 44, 17);
    assert!(msg_summary(&excessive_fat).is_empty());
}

#[test]
fn msg_compound_summary_rejects_directory_fat_and_tree_cycles() {
    let mut fat_cycle = real_msg_fixture();
    let fat = cfb_sector_offset(CFB_FAT_SECTOR_ID);
    write_u32(
        &mut fat_cycle,
        fat + usize::try_from(CFB_FIRST_DIRECTORY_SECTOR).expect("fixture sector") * 4,
        CFB_FIRST_DIRECTORY_SECTOR,
    );
    assert!(msg_summary(&fat_cycle).is_empty());

    let mut tree_cycle = real_msg_fixture();
    write_u32(&mut tree_cycle, cfb_directory_entry_offset(1) + 72, 1);
    assert!(msg_summary(&tree_cycle).is_empty());
}

#[test]
fn msg_compound_summary_rejects_truncated_directory_and_mini_stream() {
    let mut directory_truncated = real_msg_fixture();
    directory_truncated.truncate(cfb_sector_offset(CFB_SECOND_DIRECTORY_SECTOR) + 100);
    assert!(msg_summary(&directory_truncated).is_empty());

    let mut mini_stream_truncated = real_msg_fixture();
    mini_stream_truncated
        .truncate(cfb_sector_offset(CFB_MINI_STREAM_SECTOR_ID) + CFB_SECTOR_SIZE - 1);
    assert!(msg_summary(&mini_stream_truncated).is_empty());
}

#[test]
fn msg_compound_summary_fails_soft_on_hostile_mini_properties() {
    let mut mini_cycle = real_msg_fixture();
    write_u32(
        &mut mini_cycle,
        cfb_sector_offset(CFB_MINI_FAT_SECTOR_ID),
        0,
    );
    let text = msg_summary(&mini_cycle);
    assert!(!text.contains("Subject:"));
    assert!(text.contains("Sender: Alice Example"));

    let mut hostile_sector = real_msg_fixture();
    write_u32(
        &mut hostile_sector,
        cfb_directory_entry_offset(1) + 116,
        u32::MAX,
    );
    assert!(!msg_summary(&hostile_sector).contains("Subject:"));

    let mut oversized_property = real_msg_fixture();
    write_u64(
        &mut oversized_property,
        cfb_directory_entry_offset(1) + 120,
        4097,
    );
    assert!(!msg_summary(&oversized_property).contains("Subject:"));

    let mut truncated_properties = real_msg_fixture();
    write_u64(
        &mut truncated_properties,
        cfb_directory_entry_offset(4) + 120,
        40,
    );
    assert!(!msg_summary(&truncated_properties).contains("Sent time:"));
}

#[test]
fn mail_header_parser_unfolds_continuations() {
    let headers =
        parse_mail_headers("Subject: hello\r\n world\r\nFrom: a@example.test\r\n\r\nbody");

    assert_eq!(
        headers[0],
        ("Subject".to_string(), "hello world".to_string())
    );
    assert_eq!(
        headers[1],
        ("From".to_string(), "a@example.test".to_string())
    );
}
#[test]
fn mail_header_parameter_extracts_boundary() {
    let value = "multipart/mixed; boundary=\"abc-123\"; charset=utf-8";

    assert_eq!(
        mail_header_parameter(value, "boundary").as_deref(),
        Some("abc-123")
    );
}

#[test]
fn mail_header_decoder_reads_q_encoded_words_and_filenames() {
    assert_eq!(
        decode_mail_header_value("=?UTF-8?Q?Quarterly_Report?="),
        "Quarterly Report"
    );
    assert_eq!(decode_mail_header_value("=?UTF-8?Q?caf=C3=A9?="), "café");
    assert_eq!(
        decode_mail_header_value("=?UTF-8?B?UmVwb3J0IEphbnVhcnk=?="),
        "Report January"
    );
    let names = mail_attachment_summary(
        "Content-Disposition: attachment; filename=\"=?UTF-8?Q?report_Q1.pdf?=\"\r\n",
    )
    .1;
    assert_eq!(names, vec!["report Q1.pdf".to_string()]);

    let names = mail_attachment_summary(
        "Content-Disposition: attachment; filename*=UTF-8''report%20Q2.pdf\r\n",
    )
    .1;
    assert_eq!(names, vec!["report Q2.pdf".to_string()]);

    let names = mail_attachment_summary(
        "Content-Disposition: attachment; filename*0*=UTF-8''quarterly%20; filename*1*=summary.pdf\r\n",
    )
    .1;
    assert_eq!(names, vec!["quarterly summary.pdf".to_string()]);
}

#[test]
fn mail_mime_part_summaries_list_types_and_attachments() {
    let content = "Content-Type: multipart/mixed; boundary=abc\r\n\r\n--abc\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHello\r\n--abc\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=report.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--abc--\r\n";

    assert_eq!(
        mail_mime_part_summaries(content, "abc"),
        vec![
            "text/plain encoding=quoted-printable body=5 bytes decoded=5 bytes preview=\"Hello\"".to_string(),
            "application/pdf (attachment) filename=report.pdf encoding=base64 body=8 bytes decoded=4 bytes".to_string(),
        ]
    );
}

#[test]
fn mail_mime_part_summaries_include_nested_parts() {
    let content = "Content-Type: multipart/mixed; boundary=outer\r\n\r\n--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\nContent-Type: text/plain\r\n\r\nNested hello\r\n--inner--\r\n--outer--\r\n";

    assert_eq!(
        mail_mime_part_summaries(content, "outer"),
        vec![
            "multipart/alternative body=60 bytes".to_string(),
            ">text/plain body=12 bytes preview=\"Nested hello\"".to_string(),
        ]
    );
}
