//! Lazy, bounded PDF text extraction for search and clipboard follow-ups.

use std::io::Read;

use lopdf::{Document, Error as PdfError, LoadOptions};
use serde::Serialize;

use super::ReaderPreviewError;
use crate::CancelCallback;

pub(crate) const MAX_PDF_TEXT_INPUT_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_PDF_TEXT_PAGES: usize = 2_048;
const MAX_PDF_OBJECTS: usize = 250_000;
const MAX_PDF_LOAD_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_PDF_PAGE_DECOMPRESSED_BYTES: usize = 8 * 1024 * 1024;
const MAX_PDF_TEXT_PAGE_BYTES: usize = 64 * 1024;
const MAX_PDF_TEXT_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfTextModel {
    status: &'static str,
    page_count: usize,
    pages: Vec<PdfTextPage>,
    truncated: bool,
    failed_pages: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfTextPage {
    page_index: usize,
    text: String,
    truncated: bool,
}

pub(crate) fn render_pdf_text_reader<R: Read>(
    reader: R,
    source_len: u64,
    cancel_cb: Option<CancelCallback>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_PDF_TEXT_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    check_cancelled(cancel_cb)?;

    let options = LoadOptions::with_max_decompressed_size(MAX_PDF_LOAD_STREAM_BYTES);
    let mut document = Document::load_from_with_options(reader, options).map_err(map_pdf_error)?;
    check_cancelled(cancel_cb)?;
    if document.objects.len() > MAX_PDF_OBJECTS {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if document.is_encrypted() && document.decrypt("").is_err() {
        return serialize_model(PdfTextModel {
            status: "encrypted",
            page_count: 0,
            pages: Vec::new(),
            truncated: false,
            failed_pages: 0,
        });
    }

    let page_numbers: Vec<u32> = document.get_pages().keys().copied().collect();
    let page_count = page_numbers.len();
    let mut pages = Vec::with_capacity(page_count.min(MAX_PDF_TEXT_PAGES));
    let mut remaining_text_bytes = MAX_PDF_TEXT_TOTAL_BYTES;
    let mut truncated = page_count > MAX_PDF_TEXT_PAGES;
    let mut failed_pages = 0usize;
    let mut has_text = false;

    for (page_index, page_number) in page_numbers
        .into_iter()
        .take(MAX_PDF_TEXT_PAGES)
        .enumerate()
    {
        check_cancelled(cancel_cb)?;
        if remaining_text_bytes == 0 {
            truncated = true;
            break;
        }

        let extracted = match document
            .extract_text_with_limit(&[page_number], MAX_PDF_PAGE_DECOMPRESSED_BYTES)
        {
            Ok(text) => text,
            Err(error) if is_limit_error(&error) => {
                truncated = true;
                failed_pages += 1;
                pages.push(PdfTextPage {
                    page_index,
                    text: String::new(),
                    truncated: true,
                });
                continue;
            }
            Err(_) => {
                failed_pages += 1;
                pages.push(PdfTextPage {
                    page_index,
                    text: String::new(),
                    truncated: false,
                });
                continue;
            }
        };
        check_cancelled(cancel_cb)?;

        let page_budget = remaining_text_bytes.min(MAX_PDF_TEXT_PAGE_BYTES);
        let (text, page_truncated) = truncate_utf8_bytes(extracted, page_budget);
        remaining_text_bytes = remaining_text_bytes.saturating_sub(text.len());
        truncated |= page_truncated;
        has_text |= !text.trim().is_empty();
        pages.push(PdfTextPage {
            page_index,
            text,
            truncated: page_truncated,
        });
    }

    let status = if failed_pages > 0 {
        "partial"
    } else if has_text {
        "ready"
    } else {
        "noText"
    };
    serialize_model(PdfTextModel {
        status,
        page_count,
        pages,
        truncated,
        failed_pages,
    })
}

fn serialize_model(model: PdfTextModel) -> Result<String, ReaderPreviewError> {
    serde_json::to_string(&model).map_err(|_| ReaderPreviewError::Malformed)
}

fn check_cancelled(cancel_cb: Option<CancelCallback>) -> Result<(), ReaderPreviewError> {
    if cancel_cb.is_some_and(|callback| callback()) {
        Err(ReaderPreviewError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_pdf_error(error: PdfError) -> ReaderPreviewError {
    if is_limit_error(&error) {
        ReaderPreviewError::LimitExceeded
    } else {
        ReaderPreviewError::Malformed
    }
}

fn is_limit_error(error: &PdfError) -> bool {
    matches!(
        error,
        PdfError::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })
    )
}

fn truncate_utf8_bytes(mut text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};
    use serde_json::Value;

    use super::{render_pdf_text_reader, truncate_utf8_bytes, MAX_PDF_TEXT_INPUT_BYTES};
    use crate::preview::ReaderPreviewError;

    extern "C" fn cancelled() -> bool {
        true
    }

    #[test]
    fn extracts_bounded_text_with_page_mapping() {
        let bytes = create_pdf(&["Alpha needle", "Beta needle"]);
        let json = render_pdf_text_reader(Cursor::new(&bytes), bytes.len() as u64, None)
            .expect("extract pdf text");
        let model: Value = serde_json::from_str(&json).expect("parse model");

        assert_eq!(model["status"], "ready");
        assert_eq!(model["pageCount"], 2);
        assert_eq!(model["pages"][0]["pageIndex"], 0);
        assert!(model["pages"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Alpha needle")));
        assert_eq!(model["pages"][1]["pageIndex"], 1);
        assert!(model["pages"][1]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Beta needle")));
    }

    #[test]
    fn rejects_oversized_or_cancelled_inputs_before_parsing() {
        assert!(matches!(
            render_pdf_text_reader(
                Cursor::new(Vec::<u8>::new()),
                MAX_PDF_TEXT_INPUT_BYTES + 1,
                None,
            ),
            Err(ReaderPreviewError::LimitExceeded)
        ));
        assert!(matches!(
            render_pdf_text_reader(Cursor::new(Vec::<u8>::new()), 0, Some(cancelled)),
            Err(ReaderPreviewError::Cancelled)
        ));
    }

    #[test]
    fn rejects_malformed_pdf() {
        assert!(matches!(
            render_pdf_text_reader(Cursor::new(b"%PDF-broken"), 11, None),
            Err(ReaderPreviewError::Malformed)
        ));
    }

    #[test]
    fn byte_truncation_preserves_utf8_boundaries() {
        let (text, truncated) = truncate_utf8_bytes("abc中文".to_string(), 5);
        assert_eq!(text, "abc");
        assert!(truncated);
    }

    fn create_pdf(page_texts: &[&str]) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::new();
        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 16.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("encode content"),
            ));
            page_ids.push(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            }));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => page_texts.len() as i64,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut output = Cursor::new(Vec::new());
        document.save_to(&mut output).expect("save pdf");
        output.into_inner()
    }
}
