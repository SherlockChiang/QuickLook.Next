use std::io::{Cursor, Write};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use zip::ZipArchive;

use super::super::super::{
    OfficeContext, OfficeReadError, ReaderPreviewError, MAX_OFFICE_INLINE_IMAGE_BYTES,
    MAX_OFFICE_INPUT_BYTES, MAX_OFFICE_LAYOUT_IMAGE_DIMENSION,
};
use super::{
    extract_office_layout_image_bgra_reader, office_layout_image_ref_is_valid,
    office_media_entries, read_office_layout_image_reference,
};

extern "C" fn always_cancel() -> bool {
    true
}

fn test_zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("start ZIP entry");
        writer.write_all(bytes).expect("write ZIP entry");
    }
    writer.finish().expect("finish ZIP").into_inner()
}

fn oversized_dimension_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8193, 1, Rgba([20, 40, 60, 255])));
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("encode oversized-dimension PNG");
    encoded.into_inner()
}

#[test]
fn office_media_entries_are_unique_canonical_and_root_scoped() {
    let bytes = test_zip_bytes(&[
        ("word/media/cover.png", b"cover"),
        ("word/media/duplicate-a.png", b"first"),
        ("WORD/MEDIA/DUPLICATE-A.PNG", b"second"),
        ("word/media/../escape.png", b"escape"),
        ("word/media/readme.txt", b"text"),
        ("ppt/media/slide.png", b"slide"),
    ]);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("Office ZIP");
    let entries = office_media_entries(&mut OfficeContext::new(None), &mut zip, &["word/media/"])
        .expect("media entries");

    assert_eq!(entries, vec!["word/media/cover.png".to_string()]);
}

#[test]
fn office_layout_image_refs_require_canonical_matching_roots() {
    assert!(office_layout_image_ref_is_valid(
        "report.docx",
        "word/media/image1.png"
    ));
    assert!(office_layout_image_ref_is_valid(
        "slides.PPTM",
        "ppt/media/cover.JPEG"
    ));

    for image_ref in [
        "ppt/media/image1.png",
        "word/media/../image1.png",
        "word\\media\\image1.png",
        "/word/media/image1.png",
        "C:/word/media/image1.png",
        "word/media/not-an-image.txt",
    ] {
        assert!(!office_layout_image_ref_is_valid("report.docx", image_ref));
    }
    assert!(!office_layout_image_ref_is_valid(
        "report.odt",
        "word/media/image1.png"
    ));
}

#[test]
fn office_layout_image_reference_rejects_ambiguous_entries() {
    let bytes = test_zip_bytes(&[
        ("word/media/unique.png", b"unique"),
        ("word/media/duplicate-a.png", b"first"),
        ("WORD/MEDIA/DUPLICATE-A.PNG", b"second"),
    ]);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("Office ZIP");
    let mut context = OfficeContext::new(None);

    assert_eq!(
        read_office_layout_image_reference(
            &mut context,
            &mut zip,
            "word/media/unique.png",
            "word/media/",
        )
        .expect("unique reference"),
        Some(("word/media/unique.png".to_string(), 6))
    );
    assert_eq!(
        read_office_layout_image_reference(
            &mut context,
            &mut zip,
            "Word/Media/Duplicate-A.Png",
            "word/media/",
        )
        .expect("ambiguous reference"),
        None
    );
}

#[test]
fn office_layout_image_decode_enforces_source_and_dimension_bounds() {
    assert!(matches!(
        extract_office_layout_image_bgra_reader(
            Cursor::new(Vec::<u8>::new()),
            MAX_OFFICE_INPUT_BYTES + 1,
            "report.docx",
            "word/media/image.png",
            64,
            64,
            None,
        ),
        Err(ReaderPreviewError::LimitExceeded)
    ));

    let oversized_bytes = vec![0u8; MAX_OFFICE_INLINE_IMAGE_BYTES as usize + 1];
    let archive = test_zip_bytes(&[("word/media/image.png", &oversized_bytes)]);
    assert!(matches!(
        extract_office_layout_image_bgra_reader(
            Cursor::new(archive.clone()),
            archive.len() as u64,
            "report.docx",
            "word/media/image.png",
            64,
            64,
            None,
        ),
        Err(ReaderPreviewError::LimitExceeded)
    ));

    let oversized_png = oversized_dimension_png();
    let archive = test_zip_bytes(&[("word/media/image.png", &oversized_png)]);
    assert!(matches!(
        extract_office_layout_image_bgra_reader(
            Cursor::new(archive.clone()),
            archive.len() as u64,
            "report.docx",
            "word/media/image.png",
            64,
            64,
            None,
        ),
        Err(ReaderPreviewError::LimitExceeded)
    ));

    let empty_archive = test_zip_bytes(&[]);
    assert!(matches!(
        extract_office_layout_image_bgra_reader(
            Cursor::new(empty_archive.clone()),
            empty_archive.len() as u64,
            "report.docx",
            "word/media/image.png",
            MAX_OFFICE_LAYOUT_IMAGE_DIMENSION + 1,
            64,
            None,
        ),
        Err(ReaderPreviewError::LimitExceeded)
    ));
}

#[test]
fn office_image_scans_and_decode_honor_cancellation() {
    let bytes = test_zip_bytes(&[("word/media/image.png", b"image")]);
    let mut zip = ZipArchive::new(Cursor::new(bytes.clone())).expect("Office ZIP");

    assert!(matches!(
        office_media_entries(
            &mut OfficeContext::new(Some(always_cancel)),
            &mut zip,
            &["word/media/"],
        ),
        Err(OfficeReadError::Cancelled)
    ));
    assert!(matches!(
        extract_office_layout_image_bgra_reader(
            Cursor::new(bytes.clone()),
            bytes.len() as u64,
            "report.docx",
            "word/media/image.png",
            64,
            64,
            Some(always_cancel),
        ),
        Err(ReaderPreviewError::Cancelled)
    ));
}
