use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use zip::ZipArchive;

use super::{
    build_pptx_layout, extract_ppt_text, normalize_ppt_slide_title, parse_ppt_slide_items,
    ppt_slide_summary, ppt_slide_title, OfficeContext, PptSlideInput,
};

fn test_office_context() -> OfficeContext {
    OfficeContext::new(None)
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

#[test]
fn ppt_text_extraction_preserves_paragraphs_tabs_and_breaks() {
    let context = test_office_context();
    let text = extract_ppt_text(
        &context,
        r#"<p:sld xmlns:p="p" xmlns:a="a">
            <p:sp><p:txBody>
                <a:p><a:r><a:t>Title</a:t></a:r></a:p>
                <a:p><a:r><a:t>Left</a:t></a:r><a:tab/><a:r><a:t>Right</a:t></a:r></a:p>
                <a:p><a:r><a:t>Line 1</a:t></a:r><a:br/><a:r><a:t>Line 2</a:t></a:r></a:p>
            </p:txBody></p:sp>
        </p:sld>"#,
    )
    .expect("ppt text");

    assert_eq!(text, "Title\nLeft\tRight\nLine 1\nLine 2");
}

#[test]
fn ppt_layout_text_items_preserve_paragraph_boundaries() {
    let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
        .finish()
        .expect("empty zip archive bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
    let mut image_budget = 0;
    let mut context = OfficeContext::new(None);
    let items = parse_ppt_slide_items(
        &mut context,
        &mut zip,
        PptSlideInput {
            base_dir: "ppt/slides/",
            xml: r#"<p:sld xmlns:p="p" xmlns:a="a">
            <p:sp>
                <p:nvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
                <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></p:spPr>
                <p:txBody>
                    <a:p><a:r><a:rPr b="1" i="1" sz="2400"/><a:t>First</a:t></a:r></a:p>
                    <a:p><a:r><a:t>Second</a:t></a:r></a:p>
                </p:txBody>
            </p:sp>
        </p:sld>"#,
            rels: &BTreeMap::new(),
            inherited_placeholders: &BTreeMap::new(),
            slide_width: 960.0,
            slide_height: 540.0,
        },
        &mut image_budget,
    )
    .expect("text-only layout");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].z_index, 0);
    assert_eq!(items[0].text.as_deref(), Some("First\nSecond"));
    assert_eq!(items[0].placeholder_type.as_deref(), Some("ctrTitle"));
    assert!(items[0].bold);
    assert!(items[0].italic);
    assert_eq!(items[0].font_size, Some(24.0));
    assert_eq!(ppt_slide_title(&items, 1, 540.0), "First Second");
}

#[test]
fn ppt_layout_text_items_preserve_bullets_and_alignment_hints() {
    let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
        .finish()
        .expect("empty zip archive bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
    let mut image_budget = 0;
    let mut context = OfficeContext::new(None);
    let items = parse_ppt_slide_items(
        &mut context,
        &mut zip,
        PptSlideInput {
            base_dir: "ppt/slides/",
            xml: r#"<p:sld xmlns:p="p" xmlns:a="a">
            <p:sp>
                <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm></p:spPr>
                <p:txBody>
                    <a:p><a:pPr algn="ctr"><a:buChar char="•"/></a:pPr><a:r><a:t>Centered bullet</a:t></a:r></a:p>
                    <a:p><a:pPr algn="r"/><a:r><a:t>Right aligned</a:t></a:r></a:p>
                </p:txBody>
            </p:sp>
        </p:sld>"#,
            rels: &BTreeMap::new(),
            inherited_placeholders: &BTreeMap::new(),
            slide_width: 960.0,
            slide_height: 540.0,
        },
        &mut image_budget,
    )
    .expect("text-only layout");

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].text.as_deref(),
        Some("[center] • Centered bullet\n[right] Right aligned")
    );
    assert_eq!(
        ppt_slide_title(&items, 2, 540.0),
        "• Centered bullet Right aligned"
    );
}

#[test]
fn ppt_layout_inherits_title_placeholder_type_from_slide_layout() {
    let bytes = test_zip_bytes(&[
        (
            "ppt/presentation.xml",
            br#"<p:presentation xmlns:p="p"><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLayout" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#,
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml",
            br#"<p:sldLayout xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title" idx="7"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="457200"/><a:ext cx="7315200" cy="914400"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sldLayout>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            br#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph idx="7"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:p><a:pPr algn="ctr"/><a:r><a:t>Inherited title</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
        ),
    ]);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("PPTX archive");
    let mut context = OfficeContext::new(None);

    let layout = build_pptx_layout(&mut context, &mut zip)
        .expect("PPTX layout")
        .expect("presentation pages");

    assert_eq!(layout.pages.len(), 1);
    assert_eq!(layout.pages[0].title, "Inherited title");
    assert_eq!(layout.pages[0].items[0].x, 96.0);
    assert_eq!(layout.pages[0].items[0].width, 768.0);
    assert_eq!(
        layout.pages[0].items[0].placeholder_type.as_deref(),
        Some("title")
    );
}

#[test]
fn ppt_layout_inherits_title_type_and_geometry_from_master_once() {
    let presentation_xml =
        br#"<p:presentation xmlns:p="p"><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#;
    let slide_rels_xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLayout" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#;
    let layout_xml = br#"<p:sldLayout xmlns:p="p"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph/></p:nvPr></p:nvSpPr><p:spPr/></p:sp></p:spTree></p:cSld></p:sldLayout>"#;
    let layout_rels_xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdMaster" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#;
    let master_xml = br#"<p:sldMaster xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="457200"/><a:ext cx="7315200" cy="914400"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sldMaster>"#;
    let slide1_xml = br#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:p><a:r><a:t>Master title one</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let slide2_xml = br#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:p><a:r><a:t>Master title two</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let bytes = test_zip_bytes(&[
        ("ppt/presentation.xml", presentation_xml),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels_xml),
        ("ppt/slides/_rels/slide2.xml.rels", slide_rels_xml),
        ("ppt/slideLayouts/slideLayout1.xml", layout_xml),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels_xml,
        ),
        ("ppt/slideMasters/slideMaster1.xml", master_xml),
        ("ppt/slides/slide1.xml", slide1_xml),
        ("ppt/slides/slide2.xml", slide2_xml),
    ]);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("PPTX archive");
    let mut context = OfficeContext::new(None);
    let initial_budget = context.remaining_decompressed_bytes;

    let layout = build_pptx_layout(&mut context, &mut zip)
        .expect("PPTX layout")
        .expect("presentation pages");

    assert_eq!(layout.pages.len(), 2);
    assert_eq!(layout.pages[0].title, "Master title one");
    assert_eq!(layout.pages[1].title, "Master title two");
    for page in &layout.pages {
        assert_eq!(page.items[0].placeholder_type.as_deref(), Some("title"));
        assert_eq!(page.items[0].x, 96.0);
        assert_eq!(page.items[0].y, 48.0);
        assert_eq!(page.items[0].width, 768.0);
        assert_eq!(page.items[0].height, 96.0);
    }
    let expected_read_bytes = presentation_xml.len()
        + slide_rels_xml.len() * 2
        + layout_xml.len()
        + layout_rels_xml.len()
        + master_xml.len()
        + slide1_xml.len()
        + slide2_xml.len();
    assert_eq!(
        initial_budget - context.remaining_decompressed_bytes,
        expected_read_bytes as u64,
        "shared layout/master parts must only consume the decompression budget once"
    );
}

#[test]
fn ppt_vertical_title_is_retained_without_explicit_geometry() {
    let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
        .finish()
        .expect("empty zip archive bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
    let mut image_budget = 0;
    let mut context = OfficeContext::new(None);
    let items = parse_ppt_slide_items(
        &mut context,
        &mut zip,
        PptSlideInput {
            base_dir: "ppt/slides/",
            xml: r#"<p:sld xmlns:p="p" xmlns:a="a"><p:sp><p:nvSpPr><p:nvPr><p:ph type="vertTitle"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:p><a:r><a:t>Vertical title</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#,
            rels: &BTreeMap::new(),
            inherited_placeholders: &BTreeMap::new(),
            slide_width: 960.0,
            slide_height: 540.0,
        },
        &mut image_budget,
    )
    .expect("vertical title layout");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].placeholder_type.as_deref(), Some("vertTitle"));
    assert_eq!(items[0].width, 864.0);
    assert_eq!(ppt_slide_title(&items, 1, 540.0), "Vertical title");
}

#[test]
fn ppt_fallback_prefers_large_top_text_over_header_subtitle_and_footer() {
    let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
        .finish()
        .expect("empty zip archive bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
    let mut image_budget = 0;
    let mut context = OfficeContext::new(None);
    let items = parse_ppt_slide_items(
        &mut context,
        &mut zip,
        PptSlideInput {
            base_dir: "ppt/slides/",
            xml: r#"<p:sld xmlns:p="p" xmlns:a="a">
            <p:sp><p:spPr><a:xfrm><a:off x="0" y="95250"/><a:ext cx="9144000" cy="190500"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:rPr sz="1000"/><a:t>Small header</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="ftr"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="190500"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:rPr sz="6000"/><a:t>Footer metadata</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="subTitle"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="2857500"/><a:ext cx="9144000" cy="457200"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:rPr sz="1800"/><a:t>Lower subtitle</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:spPr><a:xfrm><a:off x="0" y="476250"/><a:ext cx="9144000" cy="914400"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:rPr sz="4400"/><a:t>Manual title</a:t></a:r></a:p></p:txBody></p:sp>
        </p:sld>"#,
            rels: &BTreeMap::new(),
            inherited_placeholders: &BTreeMap::new(),
            slide_width: 960.0,
            slide_height: 540.0,
        },
        &mut image_budget,
    )
    .expect("fallback title candidates");

    assert_eq!(ppt_slide_title(&items, 1, 540.0), "Manual title");
}

#[test]
fn ppt_slide_summary_removes_one_multiline_title_occurrence() {
    let title = normalize_ppt_slide_title("Line one\nLine two").expect("title");
    let summary = ppt_slide_summary(&title, "Kicker\nLine one\nLine two\nBody");

    assert_eq!(summary, "Line one Line two\nKicker\nBody");
    assert_eq!(ppt_slide_summary("Only title", "Only title"), "Only title");
}

#[test]
fn ppt_slide_title_uses_top_text_box_when_no_title_placeholder_exists() {
    let mut cursor = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()))
        .finish()
        .expect("empty zip archive bytes");
    cursor.set_position(0);
    let mut zip = ZipArchive::new(cursor).expect("empty zip archive");
    let mut image_budget = 0;
    let mut context = OfficeContext::new(None);
    let items = parse_ppt_slide_items(
        &mut context,
        &mut zip,
        PptSlideInput {
            base_dir: "ppt/slides/",
            xml: r#"<p:sld xmlns:p="p" xmlns:a="a">
            <p:sp>
                <p:spPr><a:xfrm><a:off x="0" y="457200"/><a:ext cx="9144000" cy="914400"/></a:xfrm></p:spPr>
                <p:txBody><a:p><a:r><a:t>Manual title</a:t></a:r></a:p></p:txBody>
            </p:sp>
            <p:sp>
                <p:spPr><a:xfrm><a:off x="0" y="2286000"/><a:ext cx="9144000" cy="1828800"/></a:xfrm></p:spPr>
                <p:txBody><a:p><a:r><a:t>Body content</a:t></a:r></a:p></p:txBody>
            </p:sp>
        </p:sld>"#,
            rels: &BTreeMap::new(),
            inherited_placeholders: &BTreeMap::new(),
            slide_width: 960.0,
            slide_height: 540.0,
        },
        &mut image_budget,
    )
    .expect("text-only layout");

    assert_eq!(ppt_slide_title(&items, 3, 540.0), "Manual title");
    assert_eq!(ppt_slide_title(&[], 3, 540.0), "Slide 3");
}
