use std::collections::BTreeMap;
use std::io::{Read, Seek};

use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

#[cfg(test)]
mod tests;

use super::super::{
    attr_value, image_mime_type, local_xml_name, normalize_zip_target, office_media_root_for_part,
    read_office_layout_image_reference, OfficeContext, OfficeLayoutItemDto, OfficeResult,
};

pub(super) struct OfficeImagePlacement<'a> {
    pub(super) rel_id: &'a str,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) z_index: usize,
}

pub(super) fn image_item_from_relationship<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    base_dir: &str,
    rels: &BTreeMap<String, String>,
    placement: OfficeImagePlacement<'_>,
    image_budget: &mut usize,
) -> OfficeResult<Option<OfficeLayoutItemDto>> {
    let OfficeImagePlacement {
        rel_id,
        x,
        y,
        width,
        height,
        z_index,
    } = placement;
    if rel_id.is_empty() || *image_budget == 0 || width <= 1.0 || height <= 1.0 {
        return Ok(None);
    }
    let Some(target) = rels.get(rel_id) else {
        return Ok(None);
    };
    let path = normalize_zip_target(base_dir, target);
    let Some(expected_root) = office_media_root_for_part(base_dir) else {
        return Ok(None);
    };
    let Some((image_ref, image_byte_length)) =
        read_office_layout_image_reference(context, zip, &path, expected_root)?
    else {
        return Ok(None);
    };
    let lower = image_ref.to_ascii_lowercase();
    *image_budget = (*image_budget).saturating_sub(1);
    Ok(Some(OfficeLayoutItemDto {
        kind: "image".to_string(),
        x,
        y,
        width,
        height,
        z_index,
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
    }))
}

pub(super) fn parse_relationships(
    context: &OfficeContext,
    xml: &str,
) -> OfficeResult<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut rels = BTreeMap::new();
    let mut event_count = 0;
    loop {
        let event = reader.read_event();
        event_count += 1;
        context.check_xml_event(event_count)?;
        match event {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) == "relationship" {
                    if let (Some(id), Some(target)) =
                        (attr_value(&e, "id"), attr_value(&e, "target"))
                    {
                        rels.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(rels)
}

pub(super) fn rels_path_for_part(part_path: &str) -> String {
    let normalized = part_path.replace('\\', "/");
    let (dir, name) = match normalized.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name.to_string()),
        None => (String::new(), normalized),
    };
    format!("{dir}_rels/{name}.rels")
}

pub(super) fn part_base_dir(part_path: &str) -> String {
    let normalized = part_path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default()
}
