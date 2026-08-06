use std::io::{Cursor, Write};

use zip::ZipArchive;

use super::super::super::OfficeReadError;
use super::{
    docx_header_footer_entries, extract_docx_header_footer_text, extract_wordprocessing_text,
    OfficeContext,
};

extern "C" fn always_cancel() -> bool {
    true
}

fn test_office_context() -> OfficeContext {
    OfficeContext::new(None)
}

#[test]
fn office_xml_parser_honors_cancellation() {
    let xml = format!(
        "<w:document xmlns:w=\"w\"><w:body>{}</w:body></w:document>",
        "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(128)
    );
    let context = OfficeContext::new(Some(always_cancel));

    assert!(matches!(
        extract_wordprocessing_text(&context, &xml),
        Err(OfficeReadError::Cancelled)
    ));
}

#[test]
fn docx_text_extraction_marks_headings() {
    let context = test_office_context();
    let text = extract_wordprocessing_text(
        &context,
        r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>Details</w:t></w:r></w:p>
            <w:p><w:r><w:t>Body copy</w:t></w:r></w:p>
        </w:body></w:document>"#,
    )
    .expect("docx text");

    assert_eq!(text, "# Overview\n### Details\nBody copy");
}

#[test]
fn docx_text_extraction_formats_table_rows() {
    let context = test_office_context();
    let text = extract_wordprocessing_text(
        &context,
        r#"<w:document xmlns:w="w"><w:body>
            <w:tbl>
                <w:tr>
                    <w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc>
                    <w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc>
                </w:tr>
                <w:tr>
                    <w:tc><w:p><w:r><w:t>Rows</w:t></w:r></w:p></w:tc>
                    <w:tc><w:p><w:r><w:t>42</w:t></w:r></w:p></w:tc>
                </w:tr>
            </w:tbl>
        </w:body></w:document>"#,
    )
    .expect("docx text");

    assert_eq!(text, "| Name | Value |\n| Rows | 42 |");
}

#[test]
fn docx_text_extraction_marks_page_and_section_breaks() {
    let context = test_office_context();
    let text = extract_wordprocessing_text(&context,
        r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:r><w:t>First page</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:t>Second page</w:t></w:r></w:p>
            <w:sectPr/>
            <w:p><w:r><w:t>Next section</w:t></w:r></w:p>
        </w:body></w:document>"#,
    )
    .expect("docx text");

    assert!(text.contains("First page\n[page break]\nSecond page"));
    assert!(text.contains("[section break]\nNext section"));
}

#[test]
fn docx_text_extraction_marks_numbered_paragraphs_as_list_items() {
    let context = test_office_context();
    let text = extract_wordprocessing_text(&context,
        r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
        </w:body></w:document>"#,
    )
    .expect("docx text");

    assert_eq!(text, "- First\n- Second");
}

#[test]
fn docx_header_footer_entries_extract_text() {
    let cursor = Cursor::new(Vec::<u8>::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("word/header1.xml", options)
        .expect("header file");
    writer
        .write_all(br#"<w:hdr xmlns:w="w"><w:p><w:r><w:t>Confidential</w:t></w:r></w:p></w:hdr>"#)
        .expect("header xml");
    writer
        .start_file("word/footer1.xml", options)
        .expect("footer file");
    writer
        .write_all(br#"<w:ftr xmlns:w="w"><w:p><w:r><w:t>Page footer</w:t></w:r></w:p></w:ftr>"#)
        .expect("footer xml");
    let mut cursor = writer.finish().expect("zip bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("docx zip");

    let entries = docx_header_footer_entries(&mut OfficeContext::new(None), &mut zip)
        .expect("header and footer entries");
    let text = extract_docx_header_footer_text(&mut OfficeContext::new(None), &mut zip, &entries)
        .expect("header and footer text");

    assert_eq!(
        entries,
        vec![
            "word/footer1.xml".to_string(),
            "word/header1.xml".to_string()
        ]
    );
    assert!(text.contains("footer1.xml: Page footer"));
    assert!(text.contains("header1.xml: Confidential"));
}
