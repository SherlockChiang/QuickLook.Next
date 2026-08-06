use std::collections::BTreeMap;
use std::io::{Read, Seek};

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

#[cfg(test)]
mod tests;

use super::super::{
    append_office_media_summary, attr_bool, attr_f64, attr_value, file_name, local_xml_name,
    normalize_hex_color, normalize_zip_target, office_media_entries,
    office_preview_json_with_layout, read_office_zip_text, truncate_preview_text, xml_general_ref,
    xml_unescape_bytes, OfficeCellDto, OfficeContext, OfficeLayoutDto, OfficeLayoutItemDto,
    OfficePageDto, OfficeResult, MAX_OFFICE_LAYOUT_IMAGES, OFFICE_EMUS_PER_DIP,
};
use super::layout::{
    image_item_from_relationship, parse_relationships, part_base_dir, rels_path_for_part,
    OfficeImagePlacement,
};

const MAX_OFFICE_ROWS: usize = 48;
const MAX_OFFICE_SHEETS: usize = 6;
const MAX_OFFICE_TABLE_CELL_WIDTH: usize = 36;
const XLSX_CELL_WIDTH: f64 = 96.0;
const XLSX_ROW_HEIGHT: f64 = 28.0;

pub(in crate::preview) fn render_xlsx<R: Read + Seek>(
    path: &str,
    zip: &mut ZipArchive<R>,
    context: &mut OfficeContext,
) -> OfficeResult<String> {
    let filename = file_name(path);
    let media_entries = office_media_entries(context, zip, &["xl/media/"])?;
    let shared_strings =
        read_office_zip_text(context, zip, "xl/sharedStrings.xml", 16 * 1024 * 1024)?
            .map(|xml| parse_shared_strings(context, &xml))
            .transpose()?
            .unwrap_or_default();

    let mut sections = Vec::new();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(xml) = read_office_zip_text(context, zip, &name, 16 * 1024 * 1024)? else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };
        let rows = parse_worksheet_rows(context, &xml, &shared_strings)?;
        if rows.is_empty() {
            continue;
        }
        sections.push(format!(
            "Sheet {sheet_idx}\n{}",
            format_table_rows(&rows).join("\n")
        ));
    }

    let body = if sections.is_empty() {
        "Status: no extractable worksheet cells".to_string()
    } else {
        sections.join("\n\n")
    };
    let mut text = format!("Name: {filename}\nKind: Excel workbook\n");
    append_office_media_summary(&mut text, &media_entries);
    text.push('\n');
    text.push_str(&truncate_preview_text(&body));
    let layout = build_xlsx_layout(context, zip, &shared_strings)?;
    Ok(office_preview_json_with_layout(
        path,
        "Excel workbook",
        text,
        "code",
        "text",
        layout,
    ))
}

fn build_xlsx_layout<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    shared_strings: &[String],
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    let styles = read_office_zip_text(context, zip, "xl/styles.xml", 4 * 1024 * 1024)?
        .map(|xml| parse_xlsx_styles(context, &xml))
        .transpose()?
        .unwrap_or_default();
    for sheet_idx in 1..=MAX_OFFICE_SHEETS {
        let sheet_name = format!("xl/worksheets/sheet{sheet_idx}.xml");
        let Some(sheet_xml) = read_office_zip_text(context, zip, &sheet_name, 16 * 1024 * 1024)?
        else {
            if sheet_idx == 1 {
                continue;
            }
            break;
        };

        let metrics = parse_xlsx_sheet_metrics(context, &sheet_xml)?;
        let merge_regions = parse_xlsx_merge_regions(context, &sheet_xml)?;
        let (freeze_rows, freeze_columns) = parse_xlsx_freeze_pane(context, &sheet_xml)?;
        let mut cells = parse_worksheet_layout_cells(
            context,
            &sheet_xml,
            shared_strings,
            &metrics,
            &merge_regions,
            &styles,
        )?;
        let mut items = parse_xlsx_sheet_images(
            context,
            zip,
            sheet_idx,
            &sheet_xml,
            &metrics,
            &mut image_budget,
        )?;
        let (width, height) = xlsx_page_size(&cells, &items);
        if cells.is_empty() && items.is_empty() {
            continue;
        }
        cells.sort_by_key(|cell| (cell.row, cell.column));
        items.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
        pages.push(OfficePageDto {
            title: format!("Sheet {sheet_idx}"),
            index: sheet_idx,
            width,
            height,
            background_color: None,
            freeze_rows,
            freeze_columns,
            cells,
            items,
        });
    }

    if pages.is_empty() {
        return Ok(None);
    }

    let width = pages.iter().map(|p| p.width).fold(0.0, f64::max);
    let height = pages.first().map(|p| p.height).unwrap_or(420.0);
    Ok(Some(OfficeLayoutDto {
        layout_kind: "workbook".to_string(),
        width,
        height,
        pages,
    }))
}

fn parse_worksheet_layout_cells(
    context: &OfficeContext,
    xml: &str,
    shared_strings: &[String],
    metrics: &XlsxSheetMetrics,
    merge_regions: &BTreeMap<(usize, usize), XlsxMergeRegion>,
    styles: &[XlsxStyle],
) -> OfficeResult<Vec<OfficeCellDto>> {
    let mut reader = Reader::from_str(xml);
    let mut cells = Vec::new();
    let mut in_row = false;
    let mut in_cell = false;
    let mut in_value = false;
    let mut in_inline_text = false;
    let mut display_row = 0usize;
    let mut row_index = 0usize;
    let mut next_col = 0usize;
    let mut cell_col = 0usize;
    let mut cell_style = 0usize;
    let mut cell_type = String::new();
    let mut cell_value = String::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "row" => {
                        in_row = true;
                        display_row += 1;
                        row_index = attr_value(&e, "r")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(display_row)
                            .saturating_sub(1);
                        next_col = 0;
                    }
                    "c" if in_row => {
                        in_cell = true;
                        cell_type.clear();
                        cell_value.clear();
                        cell_col = attr_value(&e, "r")
                            .and_then(|reference| cell_reference_column(&reference))
                            .unwrap_or(next_col);
                        next_col = cell_col + 1;
                        cell_style = attr_value(&e, "s")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        cell_type = attr_value(&e, "t").unwrap_or_default();
                    }
                    "v" if in_cell => in_value = true,
                    "t" if in_cell => in_inline_text = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "v" => in_value = false,
                    "t" => in_inline_text = false,
                    "c" if in_cell => {
                        let value = resolve_cell_value(&cell_value, &cell_type, shared_strings);
                        let merge = merge_regions.get(&(row_index, cell_col));
                        if merge.is_none()
                            && is_inside_non_origin_merge(merge_regions, row_index, cell_col)
                        {
                            in_cell = false;
                            continue;
                        }
                        if !value.trim().is_empty() && row_index < MAX_OFFICE_ROWS && cell_col < 32
                        {
                            let row_span = merge.map(|m| m.row_span).unwrap_or(1);
                            let column_span = merge.map(|m| m.column_span).unwrap_or(1);
                            cells.push(OfficeCellDto {
                                row: row_index,
                                column: cell_col,
                                text: clean_table_cell(&value),
                                x: xlsx_col_x(metrics, cell_col),
                                y: xlsx_row_y(metrics, row_index),
                                width: xlsx_col_span_width(
                                    metrics,
                                    cell_col,
                                    cell_col + column_span,
                                ),
                                height: xlsx_row_span_height(
                                    metrics,
                                    row_index,
                                    row_index + row_span,
                                ),
                                row_span,
                                column_span,
                                number_format: styles
                                    .get(cell_style)
                                    .and_then(|style| style.number_format.clone()),
                                fill_color: styles
                                    .get(cell_style)
                                    .and_then(|style| style.fill_color.clone()),
                                text_color: styles
                                    .get(cell_style)
                                    .and_then(|style| style.text_color.clone()),
                                horizontal_alignment: styles
                                    .get(cell_style)
                                    .and_then(|style| style.horizontal_alignment.clone()),
                                vertical_alignment: styles
                                    .get(cell_style)
                                    .and_then(|style| style.vertical_alignment.clone()),
                                bold: styles
                                    .get(cell_style)
                                    .map(|style| style.bold)
                                    .unwrap_or(false),
                                italic: styles
                                    .get(cell_style)
                                    .map(|style| style.italic)
                                    .unwrap_or(false),
                                font_size: styles.get(cell_style).and_then(|style| style.font_size),
                                wrap_text: styles
                                    .get(cell_style)
                                    .map(|style| style.wrap_text)
                                    .unwrap_or(false),
                            });
                        }
                        in_cell = false;
                    }
                    "row" => in_row = false,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_unescape_bytes(e.as_ref()))
            }
            Ok(Event::GeneralRef(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_general_ref(e.as_ref()))
            }
            Ok(Event::CData(e)) if in_value || in_inline_text => {
                cell_value.push_str(&String::from_utf8_lossy(e.as_ref()))
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(cells)
}

#[derive(Clone, Default)]
struct XlsxStyle {
    number_format: Option<String>,
    fill_color: Option<String>,
    text_color: Option<String>,
    horizontal_alignment: Option<String>,
    vertical_alignment: Option<String>,
    bold: bool,
    italic: bool,
    font_size: Option<f64>,
    wrap_text: bool,
}

fn parse_xlsx_styles(context: &OfficeContext, xml: &str) -> OfficeResult<Vec<XlsxStyle>> {
    let mut reader = Reader::from_str(xml);
    let mut custom_formats = BTreeMap::<u32, String>::new();
    let mut font_bold = Vec::<bool>::new();
    let mut font_italic = Vec::<bool>::new();
    let mut font_sizes = Vec::<Option<f64>>::new();
    let mut font_colors = Vec::<Option<String>>::new();
    let mut fill_colors = Vec::<Option<String>>::new();
    let mut styles = Vec::<XlsxStyle>::new();
    let mut in_fonts = false;
    let mut in_font = false;
    let mut in_fills = false;
    let mut in_fill = false;
    let mut in_cell_xfs = false;
    let mut in_xf = false;
    let mut current_xf: Option<XlsxStyle> = None;
    let mut current_font_bold = false;
    let mut current_font_italic = false;
    let mut current_font_size: Option<f64> = None;
    let mut current_font_color: Option<String> = None;
    let mut current_fill_color: Option<String> = None;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "fonts" {
                    in_fonts = true;
                } else if local == "font" && in_fonts {
                    in_font = true;
                    current_font_bold = false;
                    current_font_italic = false;
                    current_font_size = None;
                    current_font_color = None;
                } else if local == "b" && in_font {
                    current_font_bold = true;
                } else if local == "i" && in_font {
                    current_font_italic = true;
                } else if local == "sz" && in_font {
                    current_font_size = attr_f64(&e, "val").or(current_font_size);
                } else if local == "color" && in_font {
                    current_font_color = xlsx_color_from_element(&e).or(current_font_color);
                } else if local == "fills" {
                    in_fills = true;
                } else if local == "fill" && in_fills {
                    in_fill = true;
                    current_fill_color = None;
                } else if (local == "fgcolor" || local == "bgcolor") && in_fill {
                    current_fill_color = xlsx_color_from_element(&e).or(current_fill_color);
                } else if local == "cellxfs" {
                    in_cell_xfs = true;
                } else if local == "xf" && in_cell_xfs {
                    in_xf = true;
                    current_xf = Some(xlsx_style_from_xf(
                        &e,
                        &custom_formats,
                        &fill_colors,
                        &font_bold,
                        &font_italic,
                        &font_sizes,
                        &font_colors,
                    ));
                } else if local == "alignment" && in_xf {
                    if let Some(style) = current_xf.as_mut() {
                        apply_xlsx_alignment(style, &e);
                    }
                } else if local == "numfmt" {
                    collect_xlsx_custom_number_format(&e, &mut custom_formats);
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "b" && in_font {
                    current_font_bold = true;
                } else if local == "i" && in_font {
                    current_font_italic = true;
                } else if local == "sz" && in_font {
                    current_font_size = attr_f64(&e, "val").or(current_font_size);
                } else if local == "color" && in_font {
                    current_font_color = xlsx_color_from_element(&e).or(current_font_color);
                } else if local == "fill" && in_fills {
                    fill_colors.push(None);
                } else if (local == "fgcolor" || local == "bgcolor") && in_fill {
                    current_fill_color = xlsx_color_from_element(&e).or(current_fill_color);
                } else if local == "xf" && in_cell_xfs {
                    styles.push(xlsx_style_from_xf(
                        &e,
                        &custom_formats,
                        &fill_colors,
                        &font_bold,
                        &font_italic,
                        &font_sizes,
                        &font_colors,
                    ));
                } else if local == "alignment" && in_xf {
                    if let Some(style) = current_xf.as_mut() {
                        apply_xlsx_alignment(style, &e);
                    }
                } else if local == "numfmt" {
                    collect_xlsx_custom_number_format(&e, &mut custom_formats);
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "font" && in_font {
                    font_bold.push(current_font_bold);
                    font_italic.push(current_font_italic);
                    font_sizes.push(current_font_size);
                    font_colors.push(current_font_color.take());
                    in_font = false;
                    current_font_bold = false;
                } else if local == "fonts" {
                    in_fonts = false;
                } else if local == "fill" && in_fill {
                    fill_colors.push(current_fill_color.take());
                    in_fill = false;
                } else if local == "fills" {
                    in_fills = false;
                } else if local == "xf" && in_xf {
                    if let Some(style) = current_xf.take() {
                        styles.push(style);
                    }
                    in_xf = false;
                } else if local == "cellxfs" {
                    in_cell_xfs = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(styles)
}

#[cfg(test)]
fn parse_xlsx_style_number_formats(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<Vec<Option<String>>> {
    Ok(parse_xlsx_styles(context, xml)?
        .into_iter()
        .map(|style| style.number_format)
        .collect())
}

fn collect_xlsx_custom_number_format(e: &BytesStart, formats: &mut BTreeMap<u32, String>) {
    let Some(id) = attr_value(e, "numfmtid").and_then(|value| value.parse::<u32>().ok()) else {
        return;
    };
    let Some(format) = attr_value(e, "formatcode") else {
        return;
    };
    if !format.trim().is_empty() {
        formats.insert(id, format);
    }
}

fn xlsx_style_number_format(
    e: &BytesStart,
    custom_formats: &BTreeMap<u32, String>,
) -> Option<String> {
    let id = attr_value(e, "numfmtid").and_then(|value| value.parse::<u32>().ok())?;
    custom_formats
        .get(&id)
        .cloned()
        .or_else(|| xlsx_builtin_number_format(id).map(str::to_string))
}

fn xlsx_style_from_xf(
    e: &BytesStart,
    custom_formats: &BTreeMap<u32, String>,
    fill_colors: &[Option<String>],
    font_bold: &[bool],
    font_italic: &[bool],
    font_sizes: &[Option<f64>],
    font_colors: &[Option<String>],
) -> XlsxStyle {
    let fill_color = attr_value(e, "fillid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| fill_colors.get(id).cloned().flatten());
    let bold = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_bold.get(id).copied())
        .unwrap_or(false);
    let text_color = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_colors.get(id).cloned().flatten());
    let italic = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_italic.get(id).copied())
        .unwrap_or(false);
    let font_size = attr_value(e, "fontid")
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|id| font_sizes.get(id).copied().flatten());
    XlsxStyle {
        number_format: xlsx_style_number_format(e, custom_formats),
        fill_color,
        text_color,
        bold,
        italic,
        font_size,
        ..Default::default()
    }
}

fn apply_xlsx_alignment(style: &mut XlsxStyle, e: &BytesStart) {
    style.horizontal_alignment = attr_value(e, "horizontal")
        .and_then(|value| normalize_xlsx_horizontal_alignment(&value))
        .or_else(|| style.horizontal_alignment.clone());
    style.vertical_alignment = attr_value(e, "vertical")
        .and_then(|value| normalize_xlsx_vertical_alignment(&value))
        .or_else(|| style.vertical_alignment.clone());
    style.wrap_text = attr_bool(e, "wraptext").unwrap_or(style.wrap_text);
}

fn normalize_xlsx_horizontal_alignment(value: &str) -> Option<String> {
    match value {
        "left" | "center" | "right" | "general" | "fill" | "justify" | "distributed" => {
            Some(value.to_string())
        }
        _ => None,
    }
}

fn normalize_xlsx_vertical_alignment(value: &str) -> Option<String> {
    match value {
        "top" | "center" | "bottom" | "justify" | "distributed" => Some(value.to_string()),
        _ => None,
    }
}

fn xlsx_color_from_element(e: &BytesStart) -> Option<String> {
    attr_value(e, "rgb")
        .and_then(|value| {
            let trimmed = value.trim().trim_start_matches('#');
            let rgb = if trimmed.len() == 8 {
                &trimmed[2..]
            } else {
                trimmed
            };
            normalize_hex_color(rgb)
        })
        .or_else(|| {
            attr_value(e, "indexed")
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(xlsx_indexed_color)
        })
}

fn xlsx_indexed_color(index: u32) -> Option<String> {
    Some(match index {
        0 | 8 => "#000000".to_string(),
        1 | 9 => "#FFFFFF".to_string(),
        2 => "#FF0000".to_string(),
        3 => "#00FF00".to_string(),
        4 => "#0000FF".to_string(),
        5 => "#FFFF00".to_string(),
        6 => "#FF00FF".to_string(),
        7 => "#00FFFF".to_string(),
        22 => "#C0C0C0".to_string(),
        23 => "#808080".to_string(),
        _ => return None,
    })
}

fn xlsx_builtin_number_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => return None,
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0;(#,##0)",
        38 => "#,##0;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

fn parse_xlsx_sheet_images<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    sheet_idx: usize,
    sheet_xml: &str,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let Some(drawing_rid) = parse_worksheet_drawing_rid(context, sheet_xml)? else {
        return Ok(Vec::new());
    };
    let sheet_rels_name = format!("xl/worksheets/_rels/sheet{sheet_idx}.xml.rels");
    let sheet_rels = read_office_zip_text(context, zip, &sheet_rels_name, 2 * 1024 * 1024)?
        .map(|xml| parse_relationships(context, &xml))
        .transpose()?
        .unwrap_or_default();
    let Some(drawing_target) = sheet_rels.get(&drawing_rid) else {
        return Ok(Vec::new());
    };
    let drawing_path = normalize_zip_target("xl/worksheets/", drawing_target);
    let Some(drawing_xml) = read_office_zip_text(context, zip, &drawing_path, 4 * 1024 * 1024)?
    else {
        return Ok(Vec::new());
    };
    let drawing_rels_path = rels_path_for_part(&drawing_path);
    let drawing_rels = read_office_zip_text(context, zip, &drawing_rels_path, 2 * 1024 * 1024)?
        .map(|xml| parse_relationships(context, &xml))
        .transpose()?
        .unwrap_or_default();
    let base = part_base_dir(&drawing_path);
    parse_xlsx_drawing_items(
        context,
        zip,
        &base,
        &drawing_xml,
        &drawing_rels,
        metrics,
        image_budget,
    )
}

fn parse_worksheet_drawing_rid(context: &OfficeContext, xml: &str) -> OfficeResult<Option<String>> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "drawing" {
                    return Ok(attr_value(&e, "id"));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_xlsx_drawing_items<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    xml: &str,
    rels: &BTreeMap<String, String>,
    metrics: &XlsxSheetMetrics,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut in_anchor = false;
    let mut anchor_depth = 0usize;
    let mut marker = "";
    let mut current_tag = "";
    let mut from_col = 0usize;
    let mut from_row = 0usize;
    let mut to_col = 0usize;
    let mut to_row = 0usize;
    let mut ext_w = 0.0;
    let mut ext_h = 0.0;
    let mut rel_id = String::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_anchor && (local == "twocellanchor" || local == "onecellanchor") {
                    in_anchor = true;
                    anchor_depth = 1;
                    marker = "";
                    current_tag = "";
                    from_col = 0;
                    from_row = 0;
                    to_col = 0;
                    to_row = 0;
                    ext_w = 0.0;
                    ext_h = 0.0;
                    rel_id.clear();
                    continue;
                }
                if in_anchor {
                    anchor_depth += 1;
                    if local == "from" || local == "to" {
                        marker = if local == "from" { "from" } else { "to" };
                    } else if matches!(local.as_str(), "col" | "row") {
                        current_tag = if local == "col" { "col" } else { "row" };
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
                    }
                }
            }
            Ok(Event::Empty(e)) if in_anchor => {
                let local = local_xml_name(e.name().as_ref());
                if local == "ext" {
                    ext_w = attr_f64(&e, "cx").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    ext_h = attr_f64(&e, "cy").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                } else if local == "blip" {
                    rel_id = attr_value(&e, "embed").unwrap_or_default();
                }
            }
            Ok(Event::End(e)) if in_anchor => {
                let local = local_xml_name(e.name().as_ref());
                if local == "from" || local == "to" {
                    marker = "";
                } else if local == "col" || local == "row" {
                    current_tag = "";
                }
                anchor_depth = anchor_depth.saturating_sub(1);
                if anchor_depth == 0 {
                    let x = xlsx_col_x(metrics, from_col);
                    let y = xlsx_row_y(metrics, from_row);
                    let width = if to_col > from_col {
                        xlsx_col_span_width(metrics, from_col, to_col)
                    } else {
                        ext_w.max(140.0)
                    };
                    let height = if to_row > from_row {
                        xlsx_row_span_height(metrics, from_row, to_row)
                    } else {
                        ext_h.max(90.0)
                    };
                    if let Some(item) = image_item_from_relationship(
                        context,
                        zip,
                        base_dir,
                        rels,
                        OfficeImagePlacement {
                            rel_id: &rel_id,
                            x,
                            y,
                            width,
                            height,
                            z_index: items.len(),
                        },
                        image_budget,
                    )? {
                        items.push(item);
                    }
                    in_anchor = false;
                }
            }
            Ok(Event::Text(e)) if in_anchor && !marker.is_empty() && !current_tag.is_empty() => {
                let value = xml_unescape_bytes(e.as_ref())
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(0);
                match (marker, current_tag) {
                    ("from", "col") => from_col = value,
                    ("from", "row") => from_row = value,
                    ("to", "col") => to_col = value,
                    ("to", "row") => to_row = value,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(items)
}

#[derive(Default)]
struct XlsxSheetMetrics {
    col_widths: BTreeMap<usize, f64>,
    row_heights: BTreeMap<usize, f64>,
}

#[derive(Clone, Copy)]
struct XlsxMergeRegion {
    first_row: usize,
    first_col: usize,
    last_row: usize,
    last_col: usize,
    row_span: usize,
    column_span: usize,
}

fn parse_xlsx_sheet_metrics(context: &OfficeContext, xml: &str) -> OfficeResult<XlsxSheetMetrics> {
    let mut reader = Reader::from_str(xml);
    let mut metrics = XlsxSheetMetrics::default();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "col" {
                    let Some(min) = attr_value(&e, "min").and_then(|v| v.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    let max = attr_value(&e, "max")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(min);
                    let Some(width) = attr_f64(&e, "width").map(xlsx_column_width_to_dip) else {
                        continue;
                    };
                    for one_based_col in min..=max.min(64) {
                        metrics
                            .col_widths
                            .insert(one_based_col.saturating_sub(1), width);
                    }
                } else if local == "row" {
                    let Some(row) = attr_value(&e, "r").and_then(|v| v.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if let Some(height) = attr_f64(&e, "ht").map(xlsx_row_height_to_dip) {
                        metrics.row_heights.insert(row.saturating_sub(1), height);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(metrics)
}

fn parse_xlsx_freeze_pane(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<(Option<usize>, Option<usize>)> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "pane" {
                    let state = attr_value(&e, "state").unwrap_or_default();
                    if state != "frozen" && state != "frozenSplit" {
                        return Ok((None, None));
                    }
                    let rows = attr_f64(&e, "ysplit").map(|value| value.max(0.0) as usize);
                    let columns = attr_f64(&e, "xsplit").map(|value| value.max(0.0) as usize);
                    return Ok((
                        rows.filter(|value| *value > 0),
                        columns.filter(|value| *value > 0),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok((None, None))
}

fn parse_xlsx_merge_regions(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<BTreeMap<(usize, usize), XlsxMergeRegion>> {
    let mut reader = Reader::from_str(xml);
    let mut regions = BTreeMap::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "mergecell" {
                    let Some(reference) = attr_value(&e, "ref") else {
                        continue;
                    };
                    let Some(region) = parse_xlsx_merge_reference(&reference) else {
                        continue;
                    };
                    if region.first_row < MAX_OFFICE_ROWS && region.first_col < 32 {
                        regions.insert((region.first_row, region.first_col), region);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(regions)
}

fn parse_xlsx_merge_reference(reference: &str) -> Option<XlsxMergeRegion> {
    let (start, end) = reference.split_once(':')?;
    let (first_row, first_col) = cell_reference_position(start)?;
    let (last_row, last_col) = cell_reference_position(end)?;
    let (first_row, last_row) = (first_row.min(last_row), first_row.max(last_row));
    let (first_col, last_col) = (first_col.min(last_col), first_col.max(last_col));
    Some(XlsxMergeRegion {
        first_row,
        first_col,
        last_row,
        last_col,
        row_span: last_row.saturating_sub(first_row) + 1,
        column_span: last_col.saturating_sub(first_col) + 1,
    })
}

fn is_inside_non_origin_merge(
    regions: &BTreeMap<(usize, usize), XlsxMergeRegion>,
    row: usize,
    col: usize,
) -> bool {
    regions.values().any(|region| {
        (row != region.first_row || col != region.first_col)
            && row >= region.first_row
            && row <= region.last_row
            && col >= region.first_col
            && col <= region.last_col
    })
}

fn xlsx_column_width_to_dip(width: f64) -> f64 {
    (width * 7.0 + 12.0).clamp(36.0, 260.0)
}

fn xlsx_row_height_to_dip(height_points: f64) -> f64 {
    (height_points * 96.0 / 72.0).clamp(18.0, 120.0)
}

fn xlsx_col_width(metrics: &XlsxSheetMetrics, col: usize) -> f64 {
    metrics
        .col_widths
        .get(&col)
        .copied()
        .unwrap_or(XLSX_CELL_WIDTH)
}

fn xlsx_row_height(metrics: &XlsxSheetMetrics, row: usize) -> f64 {
    metrics
        .row_heights
        .get(&row)
        .copied()
        .unwrap_or(XLSX_ROW_HEIGHT)
}

fn xlsx_col_x(metrics: &XlsxSheetMetrics, col: usize) -> f64 {
    (0..col.min(64))
        .map(|idx| xlsx_col_width(metrics, idx))
        .sum()
}

fn xlsx_row_y(metrics: &XlsxSheetMetrics, row: usize) -> f64 {
    (0..row.min(MAX_OFFICE_ROWS))
        .map(|idx| xlsx_row_height(metrics, idx))
        .sum()
}

fn xlsx_col_span_width(metrics: &XlsxSheetMetrics, from_col: usize, to_col: usize) -> f64 {
    (from_col..to_col.min(64))
        .map(|idx| xlsx_col_width(metrics, idx))
        .sum::<f64>()
        .max(24.0)
}

fn xlsx_row_span_height(metrics: &XlsxSheetMetrics, from_row: usize, to_row: usize) -> f64 {
    (from_row..to_row.min(MAX_OFFICE_ROWS))
        .map(|idx| xlsx_row_height(metrics, idx))
        .sum::<f64>()
        .max(18.0)
}

fn xlsx_page_size(cells: &[OfficeCellDto], items: &[OfficeLayoutItemDto]) -> (f64, f64) {
    let cell_width = cells
        .iter()
        .map(|cell| cell.x + cell.width)
        .fold(0.0, f64::max);
    let cell_height = cells
        .iter()
        .map(|cell| cell.y + cell.height)
        .fold(0.0, f64::max);
    let item_width = items
        .iter()
        .map(|item| item.x + item.width)
        .fold(0.0, f64::max);
    let item_height = items
        .iter()
        .map(|item| item.y + item.height)
        .fold(0.0, f64::max);
    (
        cell_width.max(item_width).max(480.0) + 24.0,
        cell_height.max(item_height).max(260.0) + 24.0,
    )
}

fn parse_shared_strings(context: &OfficeContext, xml: &str) -> OfficeResult<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "si" {
                    in_si = true;
                    current.clear();
                } else if in_si && local == "t" {
                    in_t = true;
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_t = false;
                } else if local == "si" {
                    values.push(current.clone());
                    in_si = false;
                }
            }
            Ok(Event::Text(e)) if in_t => current.push_str(&xml_unescape_bytes(e.as_ref())),
            Ok(Event::GeneralRef(e)) if in_t => current.push_str(&xml_general_ref(e.as_ref())),
            Ok(Event::CData(e)) if in_t => current.push_str(&String::from_utf8_lossy(e.as_ref())),
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(values)
}

fn parse_worksheet_rows(
    context: &OfficeContext,
    xml: &str,
    shared_strings: &[String],
) -> OfficeResult<Vec<Vec<String>>> {
    let mut reader = Reader::from_str(xml);
    let mut rows = Vec::new();
    let mut row = Vec::<String>::new();
    let mut in_row = false;
    let mut in_cell = false;
    let mut in_value = false;
    let mut in_inline_text = false;
    let mut cell_type = String::new();
    let mut cell_value = String::new();
    let mut cell_col: Option<usize> = None;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "row" => {
                        in_row = true;
                        row.clear();
                    }
                    "c" if in_row => {
                        in_cell = true;
                        cell_type.clear();
                        cell_value.clear();
                        cell_col = None;
                        for attr in e.attributes().flatten() {
                            let key = local_xml_name(attr.key.as_ref());
                            if key == "t" {
                                cell_type = attr
                                    .normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                            } else if key == "r" {
                                let reference = attr
                                    .normalized_value(XmlVersion::Implicit1_0)
                                    .ok()
                                    .map(|v| v.to_string())
                                    .unwrap_or_default();
                                cell_col = cell_reference_column(&reference);
                            }
                        }
                    }
                    "v" if in_cell => in_value = true,
                    "t" if in_cell => in_inline_text = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                match local.as_str() {
                    "v" => in_value = false,
                    "t" => in_inline_text = false,
                    "c" if in_cell => {
                        let value = resolve_cell_value(&cell_value, &cell_type, shared_strings);
                        if let Some(col) = cell_col {
                            while row.len() < col {
                                row.push(String::new());
                            }
                            if row.len() == col {
                                row.push(value);
                            } else {
                                row[col] = value;
                            }
                        } else {
                            row.push(value);
                        }
                        in_cell = false;
                    }
                    "row" if in_row => {
                        while row
                            .last()
                            .map(|cell| cell.trim().is_empty())
                            .unwrap_or(false)
                        {
                            row.pop();
                        }
                        if row.iter().any(|cell| !cell.trim().is_empty()) {
                            rows.push(row.clone());
                            if rows.len() >= MAX_OFFICE_ROWS {
                                break;
                            }
                        }
                        in_row = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_unescape_bytes(e.as_ref()))
            }
            Ok(Event::GeneralRef(e)) if in_value || in_inline_text => {
                cell_value.push_str(&xml_general_ref(e.as_ref()))
            }
            Ok(Event::CData(e)) if in_value || in_inline_text => {
                cell_value.push_str(&String::from_utf8_lossy(e.as_ref()))
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(rows)
}

fn cell_reference_column(reference: &str) -> Option<usize> {
    let mut col = 0usize;
    let mut saw_letter = false;
    for ch in reference.chars() {
        if !ch.is_ascii_alphabetic() {
            break;
        }
        saw_letter = true;
        col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    saw_letter.then_some(col.saturating_sub(1))
}

fn cell_reference_position(reference: &str) -> Option<(usize, usize)> {
    let mut col = 0usize;
    let mut row = 0usize;
    let mut saw_letter = false;
    let mut saw_digit = false;
    for ch in reference.chars() {
        if ch.is_ascii_alphabetic() {
            if saw_digit {
                return None;
            }
            saw_letter = true;
            col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else if ch.is_ascii_digit() {
            saw_digit = true;
            row = row * 10 + (ch as usize - '0' as usize);
        } else if ch == '$' {
            continue;
        } else {
            break;
        }
    }
    (saw_letter && saw_digit && row > 0 && col > 0).then_some((row - 1, col - 1))
}

fn format_table_rows(rows: &[Vec<String>]) -> Vec<String> {
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return Vec::new();
    }

    let mut widths = vec![3usize; col_count];
    for row in rows {
        for (i, width) in widths.iter_mut().enumerate() {
            let value = row
                .get(i)
                .map(|cell| clean_table_cell(cell))
                .unwrap_or_default();
            let len = value.chars().count().min(MAX_OFFICE_TABLE_CELL_WIDTH);
            *width = (*width).max(len);
        }
    }

    rows.iter()
        .map(|row| {
            let mut parts = Vec::with_capacity(col_count);
            for (i, width) in widths.iter().copied().enumerate() {
                let value = row
                    .get(i)
                    .map(|cell| clean_table_cell(cell))
                    .unwrap_or_default();
                let cell = truncate_table_cell(&value, width);
                if i + 1 == col_count {
                    parts.push(cell);
                } else {
                    parts.push(format!("{cell:<width$}"));
                }
            }
            parts.join("  ").trim_end().to_string()
        })
        .collect()
}

fn clean_table_cell(cell: &str) -> String {
    cell.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_table_cell(cell: &str, width: usize) -> String {
    if cell.chars().count() <= width {
        return cell.to_string();
    }

    let keep = width.saturating_sub(3).max(1);
    let mut out = cell.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn resolve_cell_value(raw: &str, cell_type: &str, shared_strings: &[String]) -> String {
    if cell_type == "s" {
        raw.trim()
            .parse::<usize>()
            .ok()
            .and_then(|i| shared_strings.get(i).cloned())
            .unwrap_or_default()
    } else {
        raw.trim().to_string()
    }
}
