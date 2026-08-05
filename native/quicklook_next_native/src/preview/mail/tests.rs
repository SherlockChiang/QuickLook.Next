use super::{
    append_msg_compound_summary, decode_mail_header_value, mail_attachment_filenames,
    mail_header_parameter, mail_mime_part_summaries, parse_mail_headers,
};

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
fn msg_compound_summary_reads_common_property_streams() {
    fn write_entry(
        bytes: &mut [u8],
        index: usize,
        name: &str,
        object_type: u8,
        start_sector: u32,
        size: u64,
    ) {
        let offset = 1024 + index * 128;
        let mut units = name.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        for (unit_index, unit) in units.iter().enumerate() {
            let pos = offset + unit_index * 2;
            bytes[pos..pos + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[offset + 64..offset + 66].copy_from_slice(&((units.len() * 2) as u16).to_le_bytes());
        bytes[offset + 66] = object_type;
        bytes[offset + 116..offset + 120].copy_from_slice(&start_sector.to_le_bytes());
        bytes[offset + 120..offset + 128].copy_from_slice(&size.to_le_bytes());
    }

    fn write_utf16_stream(bytes: &mut [u8], sector: u32, value: &str) -> u64 {
        let offset = (sector as usize + 1) * 1024;
        let data = value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        bytes[offset..offset + data.len()].copy_from_slice(&data);
        data.len() as u64
    }

    let mut bytes = vec![0u8; 8192];
    bytes[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    bytes[30..32].copy_from_slice(&10u16.to_le_bytes());
    bytes[48..52].copy_from_slice(&0u32.to_le_bytes());
    let subject_len = write_utf16_stream(&mut bytes, 1, "Quarterly Update");
    let sender_len = write_utf16_stream(&mut bytes, 2, "Alice Example");
    let recipients_len = write_utf16_stream(&mut bytes, 3, "Bob Example; Carol Example");
    let sent_filetime = 116_444_736_000_000_000u64 + 1_700_000_000u64 * 10_000_000;
    bytes[5 * 1024..5 * 1024 + 8].copy_from_slice(&sent_filetime.to_le_bytes());
    write_entry(&mut bytes, 0, "Root Entry", 5, 0, 0);
    write_entry(&mut bytes, 1, "__substg1.0_0037001F", 2, 1, subject_len);
    write_entry(&mut bytes, 2, "__substg1.0_0C1A001F", 2, 2, sender_len);
    write_entry(&mut bytes, 3, "__substg1.0_0E04001F", 2, 3, recipients_len);
    write_entry(&mut bytes, 4, "__substg1.0_0E060040", 2, 4, 8);
    write_entry(&mut bytes, 5, "__substg1.0_1000001F", 2, 5, 12);
    write_entry(
        &mut bytes,
        6,
        "__attach_version1.0_#00000000",
        1,
        0xFFFF_FFFF,
        0,
    );
    write_entry(
        &mut bytes,
        7,
        "__recip_version1.0_#00000000",
        1,
        0xFFFF_FFFF,
        0,
    );
    let mut text = String::new();

    append_msg_compound_summary(&mut text, &bytes);

    assert!(text.contains("Recipients: 1"));
    assert!(text.contains("Attachments: 1"));
    assert!(text.contains("Subject: Quarterly Update"));
    assert!(text.contains("Sender: Alice Example"));
    assert!(text.contains("Recipients display: Bob Example; Carol Example"));
    assert!(text.contains("Sent time:"));
    assert!(text.contains("Body available: yes"));
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
    let names = mail_attachment_filenames(
        "Content-Disposition: attachment; filename=\"=?UTF-8?Q?report_Q1.pdf?=\"\r\n",
    );
    assert_eq!(names, vec!["report Q1.pdf".to_string()]);

    let names = mail_attachment_filenames(
        "Content-Disposition: attachment; filename*=UTF-8''report%20Q2.pdf\r\n",
    );
    assert_eq!(names, vec!["report Q2.pdf".to_string()]);

    let names = mail_attachment_filenames(
        "Content-Disposition: attachment; filename*0*=UTF-8''quarterly%20; filename*1*=summary.pdf\r\n",
    );
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
