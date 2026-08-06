use std::collections::BTreeMap;
use std::io::{Read, Seek};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

#[cfg(test)]
mod tests;

use super::super::{
    attr_bool, attr_f64, attr_value, file_name, local_xml_name, normalize_preview_lines,
    normalize_zip_target, office_color_from_element, office_preview_json_with_layout,
    read_office_zip_text, truncate_preview_text, xml_general_ref, xml_unescape_bytes,
    OfficeContext, OfficeLayoutDto, OfficeLayoutItemDto, OfficePageDto, OfficeResult,
    MAX_OFFICE_LAYOUT_IMAGES, OFFICE_EMUS_PER_DIP,
};
use super::image::{append_office_media_summary, office_media_entries};
use super::layout::{
    image_item_from_relationship, parse_relationships, part_base_dir, rels_path_for_part,
    OfficeImagePlacement,
};

const MAX_OFFICE_SLIDES: usize = 30;
const MAX_PPT_SLIDE_TITLE_CHARS: usize = 160;

#[derive(Clone, Debug, Default)]
struct PptPlaceholderInfo {
    placeholder_type: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
}

impl PptPlaceholderInfo {
    fn inherit_missing(&mut self, parent: &Self) {
        if self.placeholder_type.is_none() {
            self.placeholder_type.clone_from(&parent.placeholder_type);
        }
        if self.x.is_none() {
            self.x = parent.x;
        }
        if self.y.is_none() {
            self.y = parent.y;
        }
        if self.width.is_none() {
            self.width = parent.width;
        }
        if self.height.is_none() {
            self.height = parent.height;
        }
    }
}

#[derive(Debug, Default)]
struct PptPlaceholderCache {
    layouts: BTreeMap<String, BTreeMap<String, PptPlaceholderInfo>>,
    masters: BTreeMap<String, BTreeMap<String, PptPlaceholderInfo>>,
}

pub(in crate::preview) fn render_pptx<R: Read + Seek>(
    path: &str,
    zip: &mut ZipArchive<R>,
    context: &mut OfficeContext,
) -> OfficeResult<String> {
    let filename = file_name(path);
    let media_entries = office_media_entries(context, zip, &["ppt/media/"])?;
    let layout = build_pptx_layout(context, zip)?;
    let mut slides = Vec::new();
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(xml) = read_office_zip_text(context, zip, &name, 8 * 1024 * 1024)? else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };
        let text = extract_ppt_text(context, &xml)?;
        if !text.trim().is_empty() {
            let slide_title = layout
                .as_ref()
                .and_then(|layout| layout.pages.iter().find(|page| page.index == slide_idx))
                .map(|page| page.title.clone())
                .unwrap_or_else(|| format!("Slide {slide_idx}"));
            slides.push(ppt_slide_summary(&slide_title, &text));
        }
    }

    let body = if slides.is_empty() {
        "Status: no extractable slide text".to_string()
    } else {
        slides.join("\n\n")
    };
    let mut text = format!("Name: {filename}\nKind: PowerPoint presentation\n");
    append_office_media_summary(&mut text, &media_entries);
    text.push('\n');
    text.push_str(&truncate_preview_text(&body));
    Ok(office_preview_json_with_layout(
        path,
        "PowerPoint presentation",
        text,
        "plain",
        "text",
        layout,
    ))
}

fn build_pptx_layout<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
) -> OfficeResult<Option<OfficeLayoutDto>> {
    let presentation_xml =
        read_office_zip_text(context, zip, "ppt/presentation.xml", 4 * 1024 * 1024)?;
    let (slide_width, slide_height) = presentation_xml
        .as_deref()
        .map(|xml| parse_ppt_slide_size(context, xml))
        .transpose()?
        .flatten()
        .unwrap_or((960.0, 540.0));

    let mut pages = Vec::new();
    let mut image_budget = MAX_OFFICE_LAYOUT_IMAGES;
    let mut placeholder_cache = PptPlaceholderCache::default();
    let empty_placeholders = BTreeMap::new();
    for slide_idx in 1..=MAX_OFFICE_SLIDES {
        let slide_name = format!("ppt/slides/slide{slide_idx}.xml");
        let Some(slide_xml) = read_office_zip_text(context, zip, &slide_name, 8 * 1024 * 1024)?
        else {
            if slide_idx == 1 {
                continue;
            }
            break;
        };

        let rels_name = format!("ppt/slides/_rels/slide{slide_idx}.xml.rels");
        let rels_xml = read_office_zip_text(context, zip, &rels_name, 2 * 1024 * 1024)?;
        let rels = rels_xml
            .as_deref()
            .map(|xml| parse_relationships(context, xml))
            .transpose()?
            .unwrap_or_default();
        let layout_path = rels_xml
            .as_deref()
            .map(|xml| {
                ppt_part_relationship_target(
                    context,
                    xml,
                    "ppt/slides/",
                    "slideLayout",
                    "ppt/slidelayouts/",
                )
            })
            .transpose()?
            .flatten();
        let placeholder_cache_key = layout_path
            .as_deref()
            .map(|path| {
                cache_ppt_slide_layout_placeholders(context, zip, path, &mut placeholder_cache)
            })
            .transpose()?;
        let placeholders = placeholder_cache_key
            .as_ref()
            .and_then(|key| placeholder_cache.layouts.get(key))
            .unwrap_or(&empty_placeholders);
        let background_color = parse_ppt_slide_background(context, &slide_xml)?;
        let items = parse_ppt_slide_items(
            context,
            zip,
            PptSlideInput {
                base_dir: "ppt/slides/",
                xml: &slide_xml,
                rels: &rels,
                inherited_placeholders: placeholders,
                slide_width,
                slide_height,
            },
            &mut image_budget,
        )?;
        let title = ppt_slide_title(&items, slide_idx, slide_height);
        pages.push(OfficePageDto {
            title,
            index: slide_idx,
            width: slide_width,
            height: slide_height,
            background_color,
            freeze_rows: None,
            freeze_columns: None,
            cells: Vec::new(),
            items,
        });
    }

    if pages.is_empty() {
        return Ok(None);
    }

    Ok(Some(OfficeLayoutDto {
        layout_kind: "presentation".to_string(),
        width: slide_width,
        height: slide_height,
        pages,
    }))
}

fn parse_ppt_slide_size(context: &OfficeContext, xml: &str) -> OfficeResult<Option<(f64, f64)>> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "sldsz" {
                    let Some(cx) = attr_f64(&e, "cx") else {
                        continue;
                    };
                    let Some(cy) = attr_f64(&e, "cy") else {
                        continue;
                    };
                    return Ok(Some((
                        (cx / OFFICE_EMUS_PER_DIP).max(320.0),
                        (cy / OFFICE_EMUS_PER_DIP).max(180.0),
                    )));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_ppt_slide_background(context: &OfficeContext, xml: &str) -> OfficeResult<Option<String>> {
    let mut reader = Reader::from_str(xml);
    let mut in_background = false;
    let mut depth = 0usize;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_background && (local == "bg" || local == "bgpr") {
                    in_background = true;
                    depth = 1;
                } else if in_background {
                    depth += 1;
                    if (local == "srgbclr" || local == "schemeclr")
                        && office_color_from_element(&e).is_some()
                    {
                        return Ok(office_color_from_element(&e));
                    }
                }
            }
            Ok(Event::Empty(e)) if in_background => {
                let local = local_xml_name(e.name().as_ref());
                if local == "srgbclr" || local == "schemeclr" {
                    if let Some(color) = office_color_from_element(&e) {
                        return Ok(Some(color));
                    }
                }
            }
            Ok(Event::End(_)) if in_background => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    in_background = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

struct PptSlideInput<'a> {
    base_dir: &'a str,
    xml: &'a str,
    rels: &'a BTreeMap<String, String>,
    inherited_placeholders: &'a BTreeMap<String, PptPlaceholderInfo>,
    slide_width: f64,
    slide_height: f64,
}

fn parse_ppt_slide_items<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    input: PptSlideInput<'_>,
    image_budget: &mut usize,
) -> OfficeResult<Vec<OfficeLayoutItemDto>> {
    let PptSlideInput {
        base_dir,
        xml,
        rels,
        inherited_placeholders,
        slide_width,
        slide_height,
    } = input;
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut in_shape = false;
    let mut shape_depth = 0usize;
    let mut shape_kind = "";
    let mut x = 0.0;
    let mut y = 0.0;
    let mut width = 0.0;
    let mut height = 0.0;
    let mut has_offset = false;
    let mut has_extent = false;
    let mut rel_id = String::new();
    let mut text = String::new();
    let mut in_text = false;
    let mut shape_paragraph_had_text = false;
    let mut paragraph_prefix = String::new();
    let mut preset_shape: Option<String> = None;
    let mut placeholder_type: Option<String> = None;
    let mut inherited_placeholder: Option<PptPlaceholderInfo> = None;
    let mut text_bold = false;
    let mut text_italic = false;
    let mut text_font_size: Option<f64> = None;
    let mut fill_color: Option<String> = None;
    let mut stroke_color: Option<String> = None;
    let mut color_target = "";
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_shape && (local == "sp" || local == "pic") {
                    in_shape = true;
                    shape_depth = 1;
                    shape_kind = if local == "pic" { "image" } else { "text" };
                    x = 0.0;
                    y = 0.0;
                    width = 0.0;
                    height = 0.0;
                    has_offset = false;
                    has_extent = false;
                    rel_id.clear();
                    text.clear();
                    in_text = false;
                    shape_paragraph_had_text = false;
                    paragraph_prefix.clear();
                    preset_shape = None;
                    placeholder_type = None;
                    inherited_placeholder = None;
                    text_bold = false;
                    text_italic = false;
                    text_font_size = None;
                    fill_color = None;
                    stroke_color = None;
                    color_target = "";
                    continue;
                }
                if in_shape {
                    shape_depth += 1;
                    if local == "t" {
                        if !paragraph_prefix.is_empty() && !shape_paragraph_had_text {
                            text.push_str(&paragraph_prefix);
                        }
                        in_text = true;
                    } else if local == "ppr" {
                        paragraph_prefix = ppt_paragraph_prefix(&e);
                    } else if local == "blip" {
                        rel_id = attr_value(&e, "embed").unwrap_or_default();
                    } else if local == "solidfill" {
                        color_target = "fill";
                    } else if local == "ln" {
                        color_target = "stroke";
                    } else if local == "ph" {
                        (placeholder_type, inherited_placeholder) =
                            ppt_placeholder(&e, inherited_placeholders);
                    } else if local == "rpr" {
                        apply_ppt_run_style(
                            &e,
                            &mut text_bold,
                            &mut text_italic,
                            &mut text_font_size,
                        );
                    }
                }
            }
            Ok(Event::Empty(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "off" {
                    x = attr_f64(&e, "x").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    y = attr_f64(&e, "y").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    has_offset = true;
                } else if local == "ext" {
                    width = attr_f64(&e, "cx").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    height = attr_f64(&e, "cy").unwrap_or(0.0) / OFFICE_EMUS_PER_DIP;
                    has_extent = true;
                } else if local == "blip" {
                    rel_id = attr_value(&e, "embed").unwrap_or_default();
                } else if local == "prstgeom" {
                    preset_shape = attr_value(&e, "prst");
                } else if local == "ph" {
                    (placeholder_type, inherited_placeholder) =
                        ppt_placeholder(&e, inherited_placeholders);
                } else if local == "rpr" {
                    apply_ppt_run_style(&e, &mut text_bold, &mut text_italic, &mut text_font_size);
                } else if local == "srgbclr" || local == "schemeclr" {
                    let color = office_color_from_element(&e);
                    if color_target == "stroke" {
                        stroke_color = color.or(stroke_color);
                    } else {
                        fill_color = color.or(fill_color);
                    }
                } else if local == "tab" && shape_kind == "text" {
                    text.push('\t');
                    shape_paragraph_had_text = true;
                } else if local == "br" && shape_kind == "text" {
                    text.push('\n');
                    shape_paragraph_had_text = false;
                } else if local == "ppr" && shape_kind == "text" {
                    paragraph_prefix = ppt_paragraph_prefix(&e);
                } else if local == "buchar" && shape_kind == "text" {
                    append_ppt_bullet_prefix(&mut paragraph_prefix, &e);
                }
            }
            Ok(Event::End(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" && shape_kind == "text" {
                    if shape_paragraph_had_text && !text.ends_with('\n') {
                        text.push('\n');
                    }
                    shape_paragraph_had_text = false;
                    paragraph_prefix.clear();
                } else if local == "solidfill" || local == "ln" {
                    color_target = "";
                }
                shape_depth = shape_depth.saturating_sub(1);
                if shape_depth == 0 {
                    if shape_kind == "text" {
                        if let Some(inherited) = inherited_placeholder.as_ref() {
                            if !has_offset {
                                x = inherited.x.unwrap_or(x);
                                y = inherited.y.unwrap_or(y);
                            }
                            if !has_extent {
                                width = inherited.width.unwrap_or(width);
                                height = inherited.height.unwrap_or(height);
                            }
                        }
                        let normalized = normalize_preview_lines(&text);
                        if !normalized.is_empty()
                            && (width <= 2.0 || height <= 2.0)
                            && placeholder_type
                                .as_deref()
                                .is_some_and(ppt_is_title_placeholder)
                        {
                            x = slide_width * 0.05;
                            y = slide_height * 0.05;
                            width = slide_width * 0.9;
                            height = slide_height * 0.2;
                        }
                        if width > 2.0
                            && height > 2.0
                            && (!normalized.is_empty()
                                || preset_shape.is_some()
                                || fill_color.is_some()
                                || stroke_color.is_some())
                        {
                            items.push(OfficeLayoutItemDto {
                                kind: if normalized.is_empty() {
                                    "shape".to_string()
                                } else {
                                    "text".to_string()
                                },
                                x,
                                y,
                                width,
                                height,
                                z_index: items.len(),
                                text: (!normalized.is_empty()).then_some(normalized),
                                shape: preset_shape.clone(),
                                placeholder_type: placeholder_type.clone(),
                                bold: text_bold,
                                italic: text_italic,
                                font_size: text_font_size,
                                fill_color: fill_color.clone(),
                                stroke_color: stroke_color.clone(),
                                image_name: None,
                                mime_type: None,
                                image_ref: None,
                                image_byte_length: None,
                            });
                        }
                    } else if let Some(item) = image_item_from_relationship(
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
                    in_shape = false;
                }
            }
            Ok(Event::Text(e)) if in_shape && in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    text.push_str(&value);
                    shape_paragraph_had_text = true;
                }
            }
            Ok(Event::GeneralRef(e)) if in_shape && in_text => {
                text.push_str(&xml_general_ref(e.as_ref()));
                shape_paragraph_had_text = true;
            }
            Ok(Event::CData(e)) if in_shape && in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    text.push_str(&value);
                    shape_paragraph_had_text = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(items)
}

fn ppt_part_relationship_target(
    context: &OfficeContext,
    xml: &str,
    base_dir: &str,
    relationship_kind: &str,
    expected_root: &str,
) -> OfficeResult<Option<String>> {
    let mut reader = Reader::from_str(xml);
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_xml_name(e.name().as_ref()) == "relationship" =>
            {
                if attr_value(&e, "targetmode")
                    .is_some_and(|mode| !mode.eq_ignore_ascii_case("internal"))
                {
                    continue;
                }
                let is_expected_type = attr_value(&e, "type").is_some_and(|value| {
                    value
                        .rsplit('/')
                        .next()
                        .is_some_and(|kind| kind.eq_ignore_ascii_case(relationship_kind))
                });
                if !is_expected_type {
                    continue;
                }
                let Some(target) = attr_value(&e, "target") else {
                    continue;
                };
                let path = normalize_zip_target(base_dir, &target);
                let lower = path.to_ascii_lowercase();
                if lower.starts_with(expected_root) && lower.ends_with(".xml") {
                    return Ok(Some(path));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(None)
}

fn ppt_part_cache_key(path: &str) -> String {
    normalize_zip_target("", path)
}

fn cache_ppt_slide_layout_placeholders<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    layout_path: &str,
    cache: &mut PptPlaceholderCache,
) -> OfficeResult<String> {
    let cache_key = ppt_part_cache_key(layout_path);
    if cache.layouts.contains_key(&cache_key) {
        return Ok(cache_key);
    }

    let mut placeholders = read_office_zip_text(context, zip, layout_path, 4 * 1024 * 1024)?
        .map(|xml| parse_ppt_placeholders(context, &xml))
        .transpose()?
        .unwrap_or_default();

    let rels_path = rels_path_for_part(layout_path);
    let rels_xml = read_office_zip_text(context, zip, &rels_path, 2 * 1024 * 1024)?;
    let master_path = rels_xml
        .as_deref()
        .map(|xml| {
            ppt_part_relationship_target(
                context,
                xml,
                &part_base_dir(layout_path),
                "slideMaster",
                "ppt/slidemasters/",
            )
        })
        .transpose()?
        .flatten();
    if let Some(master_path) = master_path {
        let master_key = cache_ppt_slide_master_placeholders(context, zip, &master_path, cache)?;
        if let Some(master_placeholders) = cache.masters.get(&master_key) {
            inherit_ppt_layout_placeholders(&mut placeholders, master_placeholders);
        }
    }

    cache.layouts.insert(cache_key.clone(), placeholders);
    Ok(cache_key)
}

fn cache_ppt_slide_master_placeholders<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    master_path: &str,
    cache: &mut PptPlaceholderCache,
) -> OfficeResult<String> {
    let cache_key = ppt_part_cache_key(master_path);
    if cache.masters.contains_key(&cache_key) {
        return Ok(cache_key);
    }
    let placeholders = read_office_zip_text(context, zip, master_path, 4 * 1024 * 1024)?
        .map(|xml| parse_ppt_placeholders(context, &xml))
        .transpose()?
        .unwrap_or_default();
    cache.masters.insert(cache_key.clone(), placeholders);
    Ok(cache_key)
}

fn inherit_ppt_layout_placeholders(
    placeholders: &mut BTreeMap<String, PptPlaceholderInfo>,
    master_placeholders: &BTreeMap<String, PptPlaceholderInfo>,
) {
    for (index, placeholder) in placeholders {
        let explicit_type = placeholder.placeholder_type.as_deref();
        let parent = explicit_type
            .and_then(|placeholder_type| {
                master_placeholders.values().find(|master| {
                    master
                        .placeholder_type
                        .as_deref()
                        .is_some_and(|master_type| {
                            master_type.eq_ignore_ascii_case(placeholder_type)
                        })
                })
            })
            .or_else(|| {
                explicit_type.and_then(|placeholder_type| {
                    master_placeholders.values().find(|master| {
                        master
                            .placeholder_type
                            .as_deref()
                            .is_some_and(|master_type| {
                                ppt_placeholder_family_matches(placeholder_type, master_type)
                            })
                    })
                })
            })
            .or_else(|| master_placeholders.get(index));
        if let Some(parent) = parent {
            placeholder.inherit_missing(parent);
        }
    }
}

fn ppt_placeholder_family_matches(left: &str, right: &str) -> bool {
    (ppt_is_title_placeholder(left) && ppt_is_title_placeholder(right))
        || (ppt_is_body_placeholder(left) && ppt_is_body_placeholder(right))
}

fn ppt_is_body_placeholder(value: &str) -> bool {
    value.eq_ignore_ascii_case("body")
        || value.eq_ignore_ascii_case("obj")
        || value.eq_ignore_ascii_case("vertBody")
        || value.eq_ignore_ascii_case("subTitle")
}

fn parse_ppt_placeholders(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<BTreeMap<String, PptPlaceholderInfo>> {
    let mut reader = Reader::from_str(xml);
    let mut placeholders = BTreeMap::new();
    let mut in_shape = false;
    let mut shape_depth = 0usize;
    let mut index: Option<String> = None;
    let mut placeholder_type: Option<String> = None;
    let mut x: Option<f64> = None;
    let mut y: Option<f64> = None;
    let mut width: Option<f64> = None;
    let mut height: Option<f64> = None;
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if !in_shape && local == "sp" {
                    in_shape = true;
                    shape_depth = 1;
                    index = None;
                    placeholder_type = None;
                    x = None;
                    y = None;
                    width = None;
                    height = None;
                    continue;
                }
                if in_shape {
                    shape_depth += 1;
                    if local == "ph" {
                        index = Some(ppt_placeholder_index(&e));
                        placeholder_type =
                            attr_value(&e, "type").filter(|value| !value.trim().is_empty());
                    } else if local == "off" {
                        x = attr_f64(&e, "x").map(|value| value / OFFICE_EMUS_PER_DIP);
                        y = attr_f64(&e, "y").map(|value| value / OFFICE_EMUS_PER_DIP);
                    } else if local == "ext" {
                        width = attr_f64(&e, "cx").map(|value| value / OFFICE_EMUS_PER_DIP);
                        height = attr_f64(&e, "cy").map(|value| value / OFFICE_EMUS_PER_DIP);
                    }
                }
            }
            Ok(Event::Empty(e)) if in_shape => {
                let local = local_xml_name(e.name().as_ref());
                if local == "ph" {
                    index = Some(ppt_placeholder_index(&e));
                    placeholder_type =
                        attr_value(&e, "type").filter(|value| !value.trim().is_empty());
                } else if local == "off" {
                    x = attr_f64(&e, "x").map(|value| value / OFFICE_EMUS_PER_DIP);
                    y = attr_f64(&e, "y").map(|value| value / OFFICE_EMUS_PER_DIP);
                } else if local == "ext" {
                    width = attr_f64(&e, "cx").map(|value| value / OFFICE_EMUS_PER_DIP);
                    height = attr_f64(&e, "cy").map(|value| value / OFFICE_EMUS_PER_DIP);
                }
            }
            Ok(Event::End(_)) if in_shape => {
                shape_depth = shape_depth.saturating_sub(1);
                if shape_depth == 0 {
                    if placeholder_type.is_some() || index.is_some() {
                        placeholders
                            .entry(index.clone().unwrap_or_else(|| "0".to_string()))
                            .or_insert_with(|| PptPlaceholderInfo {
                                placeholder_type: placeholder_type.clone(),
                                x,
                                y,
                                width,
                                height,
                            });
                    }
                    in_shape = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(placeholders)
}

fn ppt_placeholder(
    element: &BytesStart<'_>,
    inherited_placeholders: &BTreeMap<String, PptPlaceholderInfo>,
) -> (Option<String>, Option<PptPlaceholderInfo>) {
    let index = ppt_placeholder_index(element);
    let inherited = inherited_placeholders.get(&index).cloned();
    let placeholder_type = attr_value(element, "type")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            inherited
                .as_ref()
                .and_then(|placeholder| placeholder.placeholder_type.clone())
        })
        .or_else(|| Some("obj".to_string()));
    (placeholder_type, inherited)
}

fn ppt_placeholder_index(element: &BytesStart<'_>) -> String {
    let Some(index) = attr_value(element, "idx") else {
        return "0".to_string();
    };
    index
        .parse::<u32>()
        .map(|value| value.to_string())
        .unwrap_or(index)
}

fn ppt_slide_title(items: &[OfficeLayoutItemDto], slide_index: usize, slide_height: f64) -> String {
    let explicit_title = items.iter().find(|item| {
        item.text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
            && item
                .placeholder_type
                .as_deref()
                .is_some_and(ppt_is_title_placeholder)
    });

    let fallback_title = items
        .iter()
        .filter(|item| {
            item.text
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        })
        .filter(|item| !ppt_is_auxiliary_placeholder(item.placeholder_type.as_deref()))
        .min_by(|left, right| {
            ppt_fallback_title_rank(left, slide_height)
                .cmp(&ppt_fallback_title_rank(right, slide_height))
        });

    explicit_title
        .or(fallback_title)
        .and_then(|item| item.text.as_deref())
        .and_then(normalize_ppt_slide_title)
        .unwrap_or_else(|| format!("Slide {slide_index}"))
}

fn ppt_fallback_title_rank(item: &OfficeLayoutItemDto, slide_height: f64) -> (u8, i64, i64, u8) {
    let placeholder_rank = match item.placeholder_type.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("subTitle") => 0,
        None => 1,
        Some(value) if value.eq_ignore_ascii_case("body") || value.eq_ignore_ascii_case("obj") => 2,
        Some(_) => 1,
    };
    let vertical_rank = u8::from(item.y > slide_height.max(1.0) * 0.5);
    let y_rank = (item.y.max(0.0) * 100.0).round() as i64;
    let font_rank = -((item.font_size.unwrap_or(0.0).max(0.0) * 100.0).round() as i64);
    (vertical_rank, font_rank, y_rank, placeholder_rank)
}

fn ppt_is_title_placeholder(value: &str) -> bool {
    value.eq_ignore_ascii_case("title")
        || value.eq_ignore_ascii_case("ctrTitle")
        || value.eq_ignore_ascii_case("vertTitle")
}

fn ppt_is_auxiliary_placeholder(placeholder_type: Option<&str>) -> bool {
    placeholder_type.is_some_and(|value| {
        value.eq_ignore_ascii_case("dt")
            || value.eq_ignore_ascii_case("ftr")
            || value.eq_ignore_ascii_case("sldNum")
            || value.eq_ignore_ascii_case("hdr")
    })
}

fn normalize_ppt_slide_title(text: &str) -> Option<String> {
    let mut title = String::new();
    let mut char_count = 0usize;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let line = line
            .strip_prefix("[center] ")
            .or_else(|| line.strip_prefix("[right] "))
            .unwrap_or(line);
        for word in line.split_whitespace() {
            if !title.is_empty() {
                if char_count == MAX_PPT_SLIDE_TITLE_CHARS {
                    title.push('…');
                    return Some(title);
                }
                title.push(' ');
                char_count += 1;
            }
            for character in word.chars() {
                if char_count == MAX_PPT_SLIDE_TITLE_CHARS {
                    title.push('…');
                    return Some(title);
                }
                title.push(character);
                char_count += 1;
            }
        }
    }
    if title.is_empty() {
        return None;
    }
    Some(title)
}

fn ppt_slide_summary(title: &str, text: &str) -> String {
    let body = remove_ppt_title_from_text(text, title);
    if body.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{body}")
    }
}

fn remove_ppt_title_from_text(text: &str, title: &str) -> String {
    const MAX_SCANNED_LINES: usize = 32;
    const MAX_TITLE_LINES: usize = 8;

    let text = text.trim();
    let mut spans = Vec::with_capacity(MAX_SCANNED_LINES);
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n').take(MAX_SCANNED_LINES) {
        let content_length = segment.trim_end_matches(['\r', '\n']).len();
        spans.push((offset, offset + content_length, offset + segment.len()));
        offset += segment.len();
    }

    for start in 0..spans.len() {
        for end in start..(start + MAX_TITLE_LINES).min(spans.len()) {
            let candidate = &text[spans[start].0..spans[end].1];
            if normalize_ppt_slide_title(candidate).as_deref() != Some(title) {
                continue;
            }
            let before = text[..spans[start].0].trim_end();
            let after = text[spans[end].2..].trim_start();
            return match (before.is_empty(), after.is_empty()) {
                (true, true) => String::new(),
                (true, false) => after.to_string(),
                (false, true) => before.to_string(),
                (false, false) => format!("{before}\n{after}"),
            };
        }
    }

    text.to_string()
}

fn ppt_paragraph_prefix(e: &BytesStart<'_>) -> String {
    let mut prefix = String::new();
    match attr_value(e, "algn").as_deref() {
        Some("ctr") => prefix.push_str("[center] "),
        Some("r") => prefix.push_str("[right] "),
        _ => {}
    }
    prefix
}

fn append_ppt_bullet_prefix(prefix: &mut String, e: &BytesStart<'_>) {
    let bullet = attr_value(e, "char")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "•".to_string());
    prefix.push_str(&bullet);
    prefix.push(' ');
}

fn apply_ppt_run_style(
    e: &BytesStart<'_>,
    bold: &mut bool,
    italic: &mut bool,
    font_size: &mut Option<f64>,
) {
    if attr_bool(e, "b") == Some(true) {
        *bold = true;
    }
    if attr_bool(e, "i") == Some(true) {
        *italic = true;
    }
    if let Some(size) = attr_f64(e, "sz") {
        *font_size = Some((size / 100.0).clamp(6.0, 60.0));
    }
}

fn extract_ppt_text(context: &OfficeContext, xml: &str) -> OfficeResult<String> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    let mut paragraph_had_text = false;
    let mut event_count = 0;

    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Start(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = true;
                }
            }
            Ok(Event::End(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "t" {
                    in_text = false;
                } else if local == "p" {
                    if paragraph_had_text && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    paragraph_had_text = false;
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_xml_name(e.name().as_ref());
                if local == "tab" {
                    out.push('\t');
                    paragraph_had_text = true;
                } else if local == "br" {
                    out.push('\n');
                    paragraph_had_text = false;
                }
            }
            Ok(Event::Text(e)) if in_text => {
                let value = xml_unescape_bytes(e.as_ref());
                if !value.is_empty() {
                    out.push_str(&value);
                    paragraph_had_text = true;
                }
            }
            Ok(Event::GeneralRef(e)) if in_text => {
                out.push_str(&xml_general_ref(e.as_ref()));
                paragraph_had_text = true;
            }
            Ok(Event::CData(e)) if in_text => {
                let value = String::from_utf8_lossy(e.as_ref());
                if !value.is_empty() {
                    out.push_str(&value);
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
