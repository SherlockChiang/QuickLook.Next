use std::io::{Cursor, Write};

use zip::ZipArchive;

use super::super::super::OfficeContext;
use super::{
    parse_shared_strings, parse_worksheet_rows, parse_xlsx_drawing_items, parse_xlsx_freeze_pane,
    parse_xlsx_merge_regions, parse_xlsx_style_number_formats, parse_xlsx_styles, XlsxSheetMetrics,
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
fn xlsx_merge_regions_preserve_spans() {
    let context = test_office_context();
    let regions = parse_xlsx_merge_regions(
        &context,
        r#"<worksheet><mergeCells><mergeCell ref="B2:D4"/></mergeCells></worksheet>"#,
    )
    .expect("merge regions");

    let region = regions.get(&(1, 1)).expect("merged region");
    assert_eq!(region.row_span, 3);
    assert_eq!(region.column_span, 3);
    assert!(super::is_inside_non_origin_merge(&regions, 2, 2));
    assert!(!super::is_inside_non_origin_merge(&regions, 1, 1));
}

#[test]
fn xlsx_freeze_pane_reads_split_counts() {
    let context = test_office_context();
    let (rows, columns) = parse_xlsx_freeze_pane(
        &context,
        r#"<worksheet><sheetViews><sheetView><pane xSplit="2" ySplit="1" state="frozen"/></sheetView></sheetViews></worksheet>"#,
    )
    .expect("freeze pane");

    assert_eq!(rows, Some(1));
    assert_eq!(columns, Some(2));
}

#[test]
fn xlsx_style_number_formats_include_custom_and_builtin_formats() {
    let context = test_office_context();
    let formats = parse_xlsx_style_number_formats(
        &context,
        r#"<styleSheet>
            <numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts>
            <cellXfs count="3">
                <xf numFmtId="0"/>
                <xf numFmtId="14"/>
                <xf numFmtId="164"/>
            </cellXfs>
        </styleSheet>"#,
    )
    .expect("style number formats");

    assert_eq!(formats.first(), Some(&None));
    assert_eq!(formats.get(1), Some(&Some("m/d/yy".to_string())));
    assert_eq!(formats.get(2), Some(&Some("yyyy-mm-dd".to_string())));
}

#[test]
fn xlsx_styles_include_fill_colors() {
    let context = test_office_context();
    let styles = parse_xlsx_styles(
        &context,
        r#"<styleSheet>
            <fonts count="2">
                <font><sz val="11"/></font>
                <font><b/><i/><color rgb="FF9C0006"/><sz val="14"/></font>
            </fonts>
            <fills count="3">
                <fill><patternFill patternType="none"/></fill>
                <fill><patternFill patternType="gray125"/></fill>
                <fill><patternFill patternType="solid"><fgColor rgb="FFFFE699"/></patternFill></fill>
            </fills>
            <cellXfs count="2">
                <xf numFmtId="0" fillId="0"/>
                <xf numFmtId="14" fillId="2" fontId="1"><alignment horizontal="center" vertical="top" wrapText="1"/></xf>
            </cellXfs>
        </styleSheet>"#,
    )
    .expect("styles");

    assert_eq!(
        styles.first().and_then(|style| style.fill_color.as_deref()),
        None
    );
    assert_eq!(
        styles.get(1).and_then(|style| style.fill_color.as_deref()),
        Some("#FFE699")
    );
    assert_eq!(
        styles
            .get(1)
            .and_then(|style| style.number_format.as_deref()),
        Some("m/d/yy")
    );
    assert_eq!(
        styles
            .get(1)
            .and_then(|style| style.horizontal_alignment.as_deref()),
        Some("center")
    );
    assert_eq!(
        styles
            .get(1)
            .and_then(|style| style.vertical_alignment.as_deref()),
        Some("top")
    );
    assert_eq!(styles.get(1).map(|style| style.bold), Some(true));
    assert_eq!(styles.get(1).map(|style| style.italic), Some(true));
    assert_eq!(styles.get(1).and_then(|style| style.font_size), Some(14.0));
    assert_eq!(styles.get(1).map(|style| style.wrap_text), Some(true));
    assert_eq!(
        styles.get(1).and_then(|style| style.text_color.as_deref()),
        Some("#9C0006")
    );
}

#[test]
fn xlsx_shared_strings_and_worksheet_rows_resolve_cells() {
    let context = test_office_context();
    let shared = parse_shared_strings(
        &context,
        r#"<sst xmlns="x"><si><t>Hello</t></si><si><r><t>multi</t></r><r><t>part</t></r></si></sst>"#,
    )
    .expect("shared strings");
    assert_eq!(shared, ["Hello", "multipart"]);

    let rows = parse_worksheet_rows(
        &context,
        r#"<worksheet xmlns="x"><sheetData>
            <row r="2"><c r="B2" t="s"><v>1</v></c><c r="D2" t="inlineStr"><is><t>Inline</t></is></c></row>
        </sheetData></worksheet>"#,
        &shared,
    )
    .expect("worksheet rows");
    assert_eq!(
        rows,
        vec![vec![
            String::new(),
            "multipart".into(),
            String::new(),
            "Inline".into()
        ]]
    );
}

#[test]
fn xlsx_drawing_anchor_resolves_image_reference_and_geometry() {
    let entries = [("xl/media/image1.png", b"not-a-decoded-image".as_slice())];
    let bytes = test_zip_bytes(&entries);
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("drawing ZIP");
    let mut context = test_office_context();
    let rels = [("rId1".to_string(), "../media/image1.png".to_string())]
        .into_iter()
        .collect();
    let metrics = XlsxSheetMetrics::default();
    let mut image_budget = 1;
    let items = parse_xlsx_drawing_items(
        &mut context,
        &mut zip,
        "xl/drawings/",
        r#"<xdr:wsDr xmlns:xdr="xdr" xmlns:a="a" xmlns:r="r">
            <xdr:twoCellAnchor>
                <xdr:from><xdr:col>1</xdr:col><xdr:row>2</xdr:row></xdr:from>
                <xdr:to><xdr:col>3</xdr:col><xdr:row>4</xdr:row></xdr:to>
                <xdr:pic><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic>
            </xdr:twoCellAnchor>
        </xdr:wsDr>"#,
        &rels,
        &metrics,
        &mut image_budget,
    )
    .expect("drawing items");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "image");
    assert_eq!(items[0].image_ref.as_deref(), Some("xl/media/image1.png"));
    assert_eq!(items[0].image_byte_length, Some(19));
    assert_eq!(items[0].x, 96.0);
    assert_eq!(items[0].y, 56.0);
    assert!(items[0].width > 0.0);
    assert!(items[0].height > 0.0);
}
