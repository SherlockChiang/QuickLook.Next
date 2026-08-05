use super::{
    decode_mail_header_value, mail_attachment_filenames, mail_header_parameter,
    mail_mime_part_summaries, parse_mail_headers,
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
