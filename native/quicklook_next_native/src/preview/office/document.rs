use std::io::{Read, Seek};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

#[cfg(test)]
mod tests;

use super::super::{
    attr_value, file_name, local_xml_name, normalize_preview_lines, office_error_json,
    office_preview_json_with_layout, office_text_json, read_office_zip_text, truncate_preview_text,
    xml_general_ref, xml_unescape_bytes, OfficeContext, OfficeLayoutDto, OfficeLayoutItemDto,
    OfficePageDto, OfficeResult, MAX_OFFICE_LAYOUT_IMAGES, MAX_OFFICE_ZIP_ENTRIES,
};
use super::image::{
    append_office_media_summary, image_mime_type, office_media_entries,
    read_office_layout_image_reference,
};

pub(in crate::preview) fn render_docx<R: Read + Seek>(
    path: &str,
    zip: &mut ZipArchive<R>,
    context: &mut OfficeContext,
) -> OfficeResult<String> {
    let filename = file_name(path);
    let media_entries = office_media_entries(context, zip, &["word/media/"])?;
    let header_footer_entries = docx_header_footer_entries(context, zip)?;
    let xml = match read_office_zip_text(context, zip, "word/document.xml", 16 * 1024 * 1024)? {
        Some(xml) => xml,
        None => {
            return Ok(office_error_json(
                path,
                "DOCX",
                "word/document.xml not found",
            ))
        }
    };
    let header_footer_text = extract_docx_header_footer_text(context, zip, &header_footer_entries)?;
    let body = extract_wordprocessing_text(context, &xml)?;
    let layout = build_docx_layout(context, zip, &body, &media_entries)?;
    let mut text = format!("Name: {filename}\nKind: Word document\n");
    append_office_media_summary(&mut text, &media_entries);
    if !header_footer_text.is_empty() {
        text.push_str("Headers/footers:\n");
        text.push_str(&header_footer_text);
        text.push('\n');
    }
    let text = if body.trim().is_empty() {
        text.push_str("Status: no extractable text");
        text
    } else {
        text.push('\n');
        text.push_str(&truncate_preview_text(&body));
        text
    };
    Ok(office_preview_json_with_layout(
        path, "DOCX", text, "plain", "text", layout,
    ))
}

pub(in crate::preview) fn render_odf<R: Read + Seek>(
    path: &str,
    zip: &mut ZipArchive<R>,
    context: &mut OfficeContext,
) -> OfficeResult<String> {
    let filename = file_name(path);
    let xml = match read_office_zip_text(context, zip, "content.xml", 16 * 1024 * 1024)? {
        Some(xml) => xml,
        None => {
            return Ok(office_error_json(
                path,
                "OpenDocument",
                "content.xml not found",
            ))
        }
    };
    let body = extract_wordprocessing_text(context, &xml)?;
    Ok(office_text_json(
        path,
        "OpenDocument",
        format!(
            "Name: {filename}\nKind: OpenDocument\n\n{}",
            truncate_preview_text(&body)
        ),
    ))
}

fn build_docx_layout<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    body: &str,
    media_entries: &[String],
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let page_width = 760.0;
    let page_height = 980.0;
    let margin = 58.0;
    let mut pages = Vec::new();
    let mut items = Vec::new();
    let mut page_index = 1usize;
    let mut y = margin;

    for paragraph in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let clipped = paragraph.chars().take(420).collect::<String>();
        let line_count = (clipped.chars().count() as f64 / 72.0)
            .ceil()
            .clamp(1.0, 5.0);
        let height = 24.0 * line_count + 10.0;
        if y + height > page_height - margin {
            push_docx_page(&mut pages, page_index, page_width, page_height, items);
            page_index += 1;
            items = Vec::new();
            y = margin;
        }

        items.push(OfficeLayoutItemDto {
            kind: "text".to_string(),
            x: margin,
            y,
            width: page_width - margin * 2.0,
            height,
            z_index: items.len(),
            text: Some(clipped),
            shape: None,
            placeholder_type: None,
            bold: false,
            italic: false,
            font_size: None,
            fill_color: None,
            stroke_color: None,
            image_name: None,
            mime_type: None,
            image_ref: None,
            image_byte_length: None,
        });
        y += height + 6.0;

        if pages.len() >= 8 {
            break;
        }
    }

    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES.min(6);
    for entry in media_entries.iter().take(6) {
        if image_budget == 0 {
            break;
        }
        if y + 180.0 > page_height - margin {
            push_docx_page(&mut pages, page_index, page_width, page_height, items);
            page_index += 1;
            items = Vec::new();
            y = margin;
        }
        let Some((image_ref, image_byte_length)) =
            read_office_layout_image_reference(context, zip, entry, "word/media/")?
        else {
            continue;
        };
        let lower = image_ref.to_ascii_lowercase();
        image_budget = image_budget.saturating_sub(1);
        items.push(OfficeLayoutItemDto {
            kind: "image".to_string(),
            x: margin,
            y,
            width: 260.0,
            height: 170.0,
            z_index: items.len(),
            text: None,
            shape: None,
            placeholder_type: None,
            bold: false,
            italic: false,
            font_size: None,
            fill_color: None,
            stroke_color: None,
            image_name: Some(
                image_ref
                    .rsplit('/')
                    .next()
                    .unwrap_or(image_ref.as_str())
                    .to_string(),
            ),
            mime_type: image_mime_type(&lower).map(str::to_string),
            image_ref: Some(image_ref),
            image_byte_length: Some(image_byte_length),
        });
        y += 188.0;
    }

    if !items.is_empty() || pages.is_empty() {
        push_docx_page(&mut pages, page_index, page_width, page_height, items);
    }

    if pages.iter().all(|page| page.items.is_empty()) {
        return Ok(None);
    }

    Ok(Some(OfficeLayoutDto {
        layout_kind: "document".to_string(),
        width: page_width,
        height: page_height,
        pages,
    }))
}

fn push_docx_page(
    pages: &mut Vec<OfficePageDto>,
    page_index: usize,
    width: f64,
    height: f64,
    items: Vec<OfficeLayoutItemDto>,
) {
    pages.push(OfficePageDto {
        title: format!("Page {page_index}"),
        index: page_index,
        width,
        height,
        background_color: Some("#FFFFFF".to_string()),
        freeze_rows: None,
        freeze_columns: None,
        cells: Vec::new(),
        items,
    });
}

fn docx_header_footer_entries<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
) -> OfficeResult<Vec<String>> {
    let mut entries = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > 1024 * 1024 {
            continue;
        }
        let normalized = entry.name().replace('\\', "/");
        if is_docx_header_footer_name(&normalized) {
            entries.push(normalized);
        }
    }
    entries.sort();
    entries.truncate(8);
    Ok(entries)
}

fn is_docx_header_footer_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(file) = lower.rsplit('/').next() else {
        return false;
    };
    lower.starts_with("word/")
        && lower.ends_with(".xml")
        && (file.starts_with("header") || file.starts_with("footer"))
}

fn extract_docx_header_footer_text<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    entries: &[String],
) -> OfficeResult<String> {
    let mut out = Vec::new();
    for entry in entries.iter().take(8) {
        let Some(xml) = read_office_zip_text(context, zip, entry, 1024 * 1024)? else {
            continue;
        };
        let text = extract_wordprocessing_text(context, &xml)?;
        if !text.trim().is_empty() {
            out.push(format!(
                "- {}: {}",
                file_name(entry),
                normalize_preview_lines(&text)
            ));
        }
    }
    Ok(out.join("\n"))
}

fn extract_wordprocessing_text(context: &OfficeContext, xml: &str) -> OfficeResult<String> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;
    let mut paragraph_prefix = String::new();
    let mut in_table = false;
    let mut in_row = false;
    let mut in_cell = false;
    let mut cell_text = String::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    if !paragraph_prefix.is_empty() && !paragraph_had_text {
                        if in_cell {
                            cell_text.push_str(&paragraph_prefix);
                        } else {
                            out.push_str(&paragraph_prefix);
                        }
                    }
                    in_text = true;
                } else if local == "tbl" {
                    in_table = true;
                } else if local == "tr" && in_table {
                    in_row = true;
                    row_cells.clear();
                } else if local == "tc" && in_row {
                    in_cell = true;
                    cell_text.clear();
                    paragraph_had_text = false;
                } else if local == "pstyle" {
                    paragraph_prefix = docx_paragraph_prefix(&e);
                } else if local == "numpr" {
                    paragraph_prefix = docx_numbered_paragraph_prefix(&paragraph_prefix);
                } else if local == "sectpr" && !in_cell {
                    append_docx_block_marker(&mut out, "[section break]");
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" {
                    if paragraph_had_text {
                        if in_cell {
                            if !cell_text.ends_with(' ') {
                                cell_text.push(' ');
                            }
                        } else if !out.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                    paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "tc" && in_cell {
                    row_cells.push(normalize_preview_lines(&cell_text).replace('\n', " "));
                    cell_text.clear();
                    in_cell = false;
                    paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "tr" && in_row {
                    if !row_cells.iter().all(|cell| cell.trim().is_empty()) {
                        out.push_str("| ");
                        out.push_str(&row_cells.join(" | "));
                        out.push_str(" |\n");
                    }
                    row_cells.clear();
                    in_row = false;
                } else if local == "tbl" {
                    in_table = false;
                } else if local == "tab" {
                    if in_cell {
                        cell_text.push('\t');
                    } else {
                        out.push('\t');
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "tab" {
                    if in_cell {
                        cell_text.push('\t');
                    } else {
                        out.push('\t');
                    }
                    paragraph_had_text = true;
                } else if local == "br" {
                    if in_cell {
                        cell_text.push(' ');
                    } else if attr_value(&e, "type").as_deref() == Some("page") {
                        append_docx_block_marker(&mut out, "[page break]");
                    } else {
                        out.push('\n');
                    }
                    paragraph_had_text = false;
                } else if local == "pstyle" {
                    paragraph_prefix = docx_paragraph_prefix(&e);
                } else if local == "numpr" {
                    paragraph_prefix = docx_numbered_paragraph_prefix(&paragraph_prefix);
                } else if local == "sectpr" && !in_cell {
                    append_docx_block_marker(&mut out, "[section break]");
                }
            }
            Ok(Event::Text(e)) if in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    if in_cell {
                        cell_text.push_str(&value);
                    } else {
                        out.push_str(&value);
                    }
                    paragraph_had_text = true;
                }
            }
            Ok(Event::GeneralRef(e)) if in_text => {
                let value = xml_general_ref(e.as_ref());
                if in_cell {
                    cell_text.push_str(&value);
                } else {
                    out.push_str(&value);
                }
                paragraph_had_text = true;
            }
            Ok(Event::CData(e)) if in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    if in_cell {
                        cell_text.push_str(&value);
                    } else {
                        out.push_str(&value);
                    }
                    paragraph_had_text = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(normalize_preview_lines(&out))
}

fn append_docx_block_marker(out: &mut String, marker: &str) {
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out.push_str(marker);
    out.push('\n');
}

fn docx_paragraph_prefix(e: &BytesStart<'_>) -> String {
    let Some(style) = attr_value(e, "val") else {
        return String::new();
    };
    let lower = style.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("heading") {
        let level = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .unwrap_or(1)
            .clamp(1, 6);
        return format!("{} ", "#".repeat(level));
    }
    if lower == "title" {
        return "# ".to_string();
    }
    String::new()
}

fn docx_numbered_paragraph_prefix(current: &str) -> String {
    if current.trim().is_empty() {
        "- ".to_string()
    } else {
        current.to_string()
    }
}
