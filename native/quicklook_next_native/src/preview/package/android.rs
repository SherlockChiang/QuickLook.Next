use std::io::{Read, Seek};

use image::{DynamicImage, Rgba, RgbaImage};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

use super::super::{
    image_to_bgra, is_supported_zip_image_name, load_bounded_embedded_image, preview_cancelled,
    MAX_ARCHIVE_SCAN_ENTRIES,
};
use super::{
    read_zip_bytes, MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS, MAX_ANDROID_RESOURCE_TABLE_BYTES,
    MAX_PACKAGE_ICON_BYTES,
};

#[cfg(test)]
mod tests;

pub(super) fn extract_android_package_icon<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    let manifest = read_zip_bytes(zip, "AndroidManifest.xml", MAX_PACKAGE_ICON_BYTES)?;
    let resources = read_zip_bytes(zip, "resources.arsc", MAX_ANDROID_RESOURCE_TABLE_BYTES);
    let manifest = decode_android_xml(&manifest, resources.as_deref())?;
    let icon_ref = android_manifest_icon_reference(&manifest)?;
    let mut decode_attempts = 0usize;
    let image = load_android_resource_image(
        zip,
        resources.as_deref(),
        &icon_ref,
        0,
        &mut decode_attempts,
        cancel_cb,
    )?;
    image_to_bgra(image, 512)
}

fn decode_android_xml(bytes: &[u8], resources: Option<&[u8]>) -> Option<String> {
    if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'<') {
        return std::str::from_utf8(bytes).ok().map(str::to_owned);
    }
    decode_android_binary_xml(bytes, resources)
}

fn decode_android_binary_xml(bytes: &[u8], resources: Option<&[u8]>) -> Option<String> {
    let (document_type, header_size, _) = android_chunk_header(bytes, 0)?;
    if document_type != 0x0003 {
        return None;
    }
    let string_pool_offset = android_find_chunk(bytes, header_size, 0x0001)?;
    let strings = android_string_pool(bytes, string_pool_offset)?;
    let (_, _, string_pool_size) = android_chunk_header(bytes, string_pool_offset)?;
    let mut offset = string_pool_offset + string_pool_size;
    let mut xml = String::new();
    while offset < bytes.len() {
        let (chunk_type, chunk_header, chunk_size) = android_chunk_header(bytes, offset)?;
        match chunk_type {
            0x0102 if chunk_header >= 16 && chunk_size >= 36 => {
                let name = android_u32(bytes, offset + 20)
                    .and_then(|index| strings.get(index as usize))?;
                let attribute_start = android_u16(bytes, offset + 24)? as usize;
                let attribute_size = android_u16(bytes, offset + 26)? as usize;
                let attribute_count = android_u16(bytes, offset + 28)? as usize;
                if attribute_size < 20 || attribute_count > 4096 {
                    return None;
                }
                xml.push('<');
                xml.push_str(name);
                let attributes = offset.checked_add(16 + attribute_start)?;
                for index in 0..attribute_count {
                    let attribute = attributes.checked_add(index.checked_mul(attribute_size)?)?;
                    if attribute.checked_add(20)? > offset + chunk_size {
                        return None;
                    }
                    let key = android_u32(bytes, attribute + 4)
                        .and_then(|value| strings.get(value as usize))?;
                    let raw = android_u32(bytes, attribute + 8)?;
                    let value_type = *bytes.get(attribute + 15)?;
                    let data = android_u32(bytes, attribute + 16)?;
                    let value = if raw != u32::MAX {
                        strings.get(raw as usize)?.clone()
                    } else {
                        android_typed_value(value_type, data, &strings, resources)?
                    };
                    xml.push(' ');
                    xml.push_str(key);
                    xml.push_str("=\"");
                    xml.push_str(&xml_escape(&value));
                    xml.push('"');
                }
                xml.push('>');
            }
            0x0103 if chunk_header >= 16 => {
                let name = android_u32(bytes, offset + 20)
                    .and_then(|index| strings.get(index as usize))?;
                xml.push_str("</");
                xml.push_str(name);
                xml.push('>');
            }
            _ => {}
        }
        offset = offset.checked_add(chunk_size)?;
    }
    (!xml.is_empty()).then_some(xml)
}

fn android_typed_value(
    value_type: u8,
    data: u32,
    strings: &[String],
    resources: Option<&[u8]>,
) -> Option<String> {
    match value_type {
        0x01 => Some(
            resources
                .and_then(|table| android_resource_reference_by_id(table, data))
                .unwrap_or_else(|| format!("@0x{data:08x}")),
        ),
        0x03 => strings.get(data as usize).cloned(),
        0x04 => Some(f32::from_bits(data).to_string()),
        0x10 => Some(data.to_string()),
        0x12 => Some(if data == 0 { "false" } else { "true" }.to_string()),
        0x1c..=0x1f => Some(format!("#{data:08x}")),
        _ => Some(data.to_string()),
    }
}

fn android_manifest_icon_reference(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut round_icon = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"application" => {
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    let Ok(value) = attr.normalized_value(XmlVersion::Implicit1_0) else {
                        continue;
                    };
                    if key == b"android:icon" || key == b"icon" {
                        return Some(value.into_owned());
                    }
                    if key == b"android:roundIcon" || key == b"roundIcon" {
                        round_icon = Some(value.into_owned());
                    }
                }
                return round_icon;
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn load_android_resource_image<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    reference: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if depth > 6 || preview_cancelled(cancel_cb) {
        return None;
    }
    if let Some(color) = parse_android_color(reference) {
        return Some(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            512, 512, color,
        )));
    }
    if let Some(table) = resources {
        for value in resolve_android_resource_values(table, reference) {
            let resolved = match value {
                AndroidResourceValue::Path(path) => load_android_archive_image(
                    zip,
                    resources,
                    &path,
                    depth,
                    decode_attempts,
                    cancel_cb,
                ),
                AndroidResourceValue::Color(color) => Some(DynamicImage::ImageRgba8(
                    RgbaImage::from_pixel(512, 512, color),
                )),
                AndroidResourceValue::Reference(reference) => load_android_resource_image(
                    zip,
                    resources,
                    &reference,
                    depth + 1,
                    decode_attempts,
                    cancel_cb,
                ),
            };
            if resolved.is_some() {
                return resolved;
            }
        }
    }
    let (kind, name) = parse_android_resource_reference(reference)?;
    let candidates = android_resource_candidates(zip, kind, name);
    for candidate in candidates {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        if let Some(image) = load_android_archive_image(
            zip,
            resources,
            &candidate,
            depth,
            decode_attempts,
            cancel_cb,
        ) {
            return Some(image);
        }
    }
    None
}

fn load_android_archive_image<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    path: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if *decode_attempts >= MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS {
        return None;
    }
    *decode_attempts += 1;
    let bytes = read_zip_bytes(zip, path, MAX_PACKAGE_ICON_BYTES)?;
    if is_supported_zip_image_name(&path.to_ascii_lowercase()) {
        return load_bounded_embedded_image(&bytes);
    }
    let xml = decode_android_xml(&bytes, resources)?;
    render_android_icon_xml(zip, resources, &xml, depth + 1, decode_attempts, cancel_cb)
}

enum AndroidResourceValue {
    Path(String),
    Color(Rgba<u8>),
    Reference(String),
}

fn android_resource_reference_by_id(table: &[u8], resource_id: u32) -> Option<String> {
    let wanted_package = (resource_id >> 24) as u8;
    let wanted_type = ((resource_id >> 16) & 0xff) as u8;
    let wanted_entry = (resource_id & 0xffff) as usize;
    let mut offset = 12usize;
    while let Some((chunk_type, header_size, chunk_size)) = android_chunk_header(table, offset) {
        if chunk_type == 0x0200 && header_size >= 284 {
            let package_id = android_u32(table, offset + 8)? as u8;
            if package_id == wanted_package {
                let type_strings = android_u32(table, offset + 268)
                    .and_then(|value| android_string_pool(table, offset + value as usize))?;
                let key_strings = android_u32(table, offset + 276)
                    .and_then(|value| android_string_pool(table, offset + value as usize))?;
                let type_name = wanted_type
                    .checked_sub(1)
                    .and_then(|index| type_strings.get(index as usize))?;
                let end = offset + chunk_size;
                let mut child = offset + header_size;
                while child < end {
                    let (child_type, child_header, child_size) =
                        android_chunk_header(table, child)?;
                    if child_type == 0x0201 && table.get(child + 8).copied() == Some(wanted_type) {
                        let count = android_u32(table, child + 12)? as usize;
                        if wanted_entry < count {
                            let entries_start = child + android_u32(table, child + 16)? as usize;
                            let offsets_start = child + child_header;
                            let sparse = table.get(child + 9).copied().unwrap_or(0) & 0x01 != 0;
                            let relative = if sparse {
                                let offset_count = entries_start.saturating_sub(offsets_start) / 4;
                                (0..offset_count).find_map(|index| {
                                    let at = offsets_start + index * 4;
                                    if android_u16(table, at)? as usize != wanted_entry {
                                        return None;
                                    }
                                    Some(u32::from(android_u16(table, at + 2)?) * 4)
                                })?
                            } else {
                                android_u32(table, offsets_start + wanted_entry * 4)?
                            };
                            if relative != u32::MAX {
                                let entry = entries_start + relative as usize;
                                let key = android_u32(table, entry + 4)
                                    .and_then(|index| key_strings.get(index as usize))?;
                                return Some(format!("@{type_name}/{key}"));
                            }
                        }
                    }
                    child = child.checked_add(child_size)?;
                }
            }
        }
        offset = offset.checked_add(chunk_size)?;
    }
    None
}

fn resolve_android_resource_values(table: &[u8], reference: &str) -> Vec<AndroidResourceValue> {
    let Some((kind, name)) = parse_android_resource_reference(reference) else {
        return Vec::new();
    };
    let Some(global_pool_offset) = android_find_chunk(table, 12, 0x0001) else {
        return Vec::new();
    };
    let Some(global_strings) = android_string_pool(table, global_pool_offset) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut offset = 12usize;
    while let Some((chunk_type, header_size, chunk_size)) = android_chunk_header(table, offset) {
        if chunk_type == 0x0200 && header_size >= 284 {
            let type_offset = android_u32(table, offset + 268).map(|value| offset + value as usize);
            let key_offset = android_u32(table, offset + 276).map(|value| offset + value as usize);
            let (Some(type_strings), Some(key_strings)) = (
                type_offset.and_then(|at| android_string_pool(table, at)),
                key_offset.and_then(|at| android_string_pool(table, at)),
            ) else {
                offset += chunk_size;
                continue;
            };
            let end = offset.saturating_add(chunk_size).min(table.len());
            let mut child = offset + header_size;
            while child < end {
                let Some((child_type, child_header, child_size)) =
                    android_chunk_header(table, child)
                else {
                    break;
                };
                if child_type == 0x0201 && child_header >= 20 {
                    let type_id = table.get(child + 8).copied().unwrap_or(0);
                    let type_name = type_id
                        .checked_sub(1)
                        .and_then(|id| type_strings.get(id as usize));
                    if type_name.map(String::as_str) == Some(kind) {
                        collect_android_type_values(
                            table,
                            AndroidTypeChunk {
                                offset: child,
                                header_size: child_header,
                                size: child_size,
                            },
                            &key_strings,
                            name,
                            &global_strings,
                            &mut values,
                        );
                    }
                }
                child = child.saturating_add(child_size);
            }
        }
        offset = offset.saturating_add(chunk_size);
    }
    values.sort_by_key(|value| match value {
        AndroidResourceValue::Path(path) if path.to_ascii_lowercase().ends_with(".xml") => 0,
        AndroidResourceValue::Reference(_) => 1,
        AndroidResourceValue::Path(_) => 2,
        AndroidResourceValue::Color(_) => 3,
    });
    values
}

#[derive(Clone, Copy)]
struct AndroidTypeChunk {
    offset: usize,
    header_size: usize,
    size: usize,
}

fn collect_android_type_values(
    table: &[u8],
    chunk: AndroidTypeChunk,
    keys: &[String],
    wanted_name: &str,
    global_strings: &[String],
    output: &mut Vec<AndroidResourceValue>,
) {
    let Some(entry_count) = android_u32(table, chunk.offset + 12).map(|value| value as usize)
    else {
        return;
    };
    let Some(entries_start) =
        android_u32(table, chunk.offset + 16).map(|value| chunk.offset + value as usize)
    else {
        return;
    };
    let offsets_start = chunk.offset + chunk.header_size;
    let chunk_end = chunk.offset.saturating_add(chunk.size).min(table.len());
    let sparse = table.get(chunk.offset + 9).copied().unwrap_or(0) & 0x01 != 0;
    let offset_count = if sparse {
        entries_start.saturating_sub(offsets_start) / 4
    } else {
        entry_count
    };
    for index in 0..offset_count {
        let relative = if sparse {
            let Some(value) = android_u16(table, offsets_start + index * 4 + 2) else {
                break;
            };
            u32::from(value) * 4
        } else {
            let Some(value) = android_u32(table, offsets_start + index * 4) else {
                break;
            };
            if value == u32::MAX {
                continue;
            }
            value
        };
        let entry = entries_start.saturating_add(relative as usize);
        if entry + 16 > chunk_end {
            continue;
        }
        let flags = android_u16(table, entry + 2).unwrap_or(0);
        if flags & 1 != 0 {
            continue;
        }
        let key = android_u32(table, entry + 4).and_then(|value| keys.get(value as usize));
        if key.map(String::as_str) != Some(wanted_name) {
            continue;
        }
        let value_type = table[entry + 11];
        let data = android_u32(table, entry + 12).unwrap_or(0);
        match value_type {
            0x01 => {
                if let Some(reference) = android_resource_reference_by_id(table, data) {
                    output.push(AndroidResourceValue::Reference(reference));
                }
            }
            0x03 => {
                if let Some(path) = global_strings.get(data as usize) {
                    output.push(AndroidResourceValue::Path(path.replace('\\', "/")));
                }
            }
            0x1c..=0x1f => output.push(AndroidResourceValue::Color(Rgba([
                ((data >> 16) & 0xff) as u8,
                ((data >> 8) & 0xff) as u8,
                (data & 0xff) as u8,
                ((data >> 24) & 0xff) as u8,
            ]))),
            _ => {}
        }
    }
}

fn android_find_chunk(bytes: &[u8], mut offset: usize, wanted: u16) -> Option<usize> {
    while offset < bytes.len() {
        let (chunk_type, _, chunk_size) = android_chunk_header(bytes, offset)?;
        if chunk_type == wanted {
            return Some(offset);
        }
        offset = offset.checked_add(chunk_size)?;
    }
    None
}

fn android_chunk_header(bytes: &[u8], offset: usize) -> Option<(u16, usize, usize)> {
    let chunk_type = android_u16(bytes, offset)?;
    let header_size = android_u16(bytes, offset + 2)? as usize;
    let chunk_size = android_u32(bytes, offset + 4)? as usize;
    (header_size >= 8
        && chunk_size >= header_size
        && offset.checked_add(chunk_size)? <= bytes.len())
    .then_some((chunk_type, header_size, chunk_size))
}

fn android_string_pool(bytes: &[u8], offset: usize) -> Option<Vec<String>> {
    let (chunk_type, header_size, chunk_size) = android_chunk_header(bytes, offset)?;
    if chunk_type != 0x0001 || header_size < 28 {
        return None;
    }
    let count = android_u32(bytes, offset + 8)? as usize;
    if count > 1_000_000 {
        return None;
    }
    let flags = android_u32(bytes, offset + 16)?;
    let strings_start = offset.checked_add(android_u32(bytes, offset + 20)? as usize)?;
    let end = offset + chunk_size;
    let offsets_start = offset + header_size;
    let mut strings = Vec::with_capacity(count);
    for index in 0..count {
        let relative = android_u32(bytes, offsets_start + index * 4)? as usize;
        let at = strings_start.checked_add(relative)?;
        let value = if flags & 0x100 != 0 {
            android_utf8_string(bytes, at, end)
        } else {
            android_utf16_string(bytes, at, end)
        }?;
        strings.push(value);
    }
    Some(strings)
}

fn android_utf8_string(bytes: &[u8], mut offset: usize, end: usize) -> Option<String> {
    let (_, next) = android_length8(bytes, offset, end)?;
    offset = next;
    let (length, next) = android_length8(bytes, offset, end)?;
    offset = next;
    let value = bytes.get(offset..offset.checked_add(length)?.min(end))?;
    std::str::from_utf8(value).ok().map(str::to_owned)
}

fn android_length8(bytes: &[u8], offset: usize, end: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(offset)?;
    if offset >= end {
        return None;
    }
    if first & 0x80 == 0 {
        Some((first as usize, offset + 1))
    } else {
        Some((
            (((first & 0x7f) as usize) << 8) | *bytes.get(offset + 1)? as usize,
            offset + 2,
        ))
    }
}

fn android_utf16_string(bytes: &[u8], mut offset: usize, end: usize) -> Option<String> {
    let first = android_u16(bytes, offset)?;
    offset += 2;
    let length = if first & 0x8000 == 0 {
        first as usize
    } else {
        let second = android_u16(bytes, offset)?;
        offset += 2;
        (((first & 0x7fff) as usize) << 16) | second as usize
    };
    let byte_length = length.checked_mul(2)?;
    if offset.checked_add(byte_length)? > end {
        return None;
    }
    let units = bytes[offset..offset + byte_length]
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]));
    Some(
        char::decode_utf16(units)
            .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
    )
}

fn android_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn android_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn parse_android_resource_reference(reference: &str) -> Option<(&str, &str)> {
    let value = reference.trim().strip_prefix('@')?;
    let value = value
        .split_once(':')
        .map(|(_, value)| value)
        .unwrap_or(value);
    let (kind, name) = value.split_once('/')?;
    (!kind.is_empty() && !name.is_empty()).then_some((kind, name))
}

fn android_resource_candidates<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    kind: &str,
    name: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let path = entry.name().replace('\\', "/");
        let lower = path.to_ascii_lowercase();
        let Some(file_name) = lower.rsplit('/').next() else {
            continue;
        };
        let Some((stem, extension)) = file_name.rsplit_once('.') else {
            continue;
        };
        let directory_match =
            lower.starts_with(&format!("res/{kind}")) || lower.contains(&format!("/res/{kind}"));
        if directory_match
            && stem == name.to_ascii_lowercase()
            && matches!(extension, "png" | "webp" | "jpg" | "jpeg" | "bmp" | "xml")
            && entry.size() <= MAX_PACKAGE_ICON_BYTES
        {
            candidates.push((android_density_score(&lower), path));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().map(|(_, path)| path).collect()
}

fn android_density_score(path: &str) -> i32 {
    if path.contains("anydpi-v26") {
        1000
    } else if path.contains("anydpi") {
        900
    } else if path.contains("xxxhdpi") {
        640
    } else if path.contains("xxhdpi") {
        480
    } else if path.contains("xhdpi") {
        320
    } else if path.contains("hdpi") {
        240
    } else if path.contains("mdpi") {
        160
    } else {
        0
    }
}

fn render_android_icon_xml<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    resources: Option<&[u8]>,
    xml: &str,
    depth: usize,
    decode_attempts: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<DynamicImage> {
    if xml.contains("<adaptive-icon") {
        let background = android_xml_drawable_reference(xml, "background").and_then(|value| {
            load_android_resource_image(zip, resources, &value, depth, decode_attempts, cancel_cb)
        });
        let foreground = android_xml_drawable_reference(xml, "foreground").and_then(|value| {
            load_android_resource_image(zip, resources, &value, depth, decode_attempts, cancel_cb)
        })?;
        let mut canvas = background
            .map(|image| {
                image
                    .resize_exact(512, 512, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            })
            .unwrap_or_else(|| RgbaImage::new(512, 512));
        let foreground = foreground
            .resize_exact(512, 512, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        image::imageops::overlay(&mut canvas, &foreground, 0, 0);
        return Some(DynamicImage::ImageRgba8(mask_android_adaptive_icon(canvas)));
    }
    if xml.contains("<color") {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(Event::Text(text)) => {
                    if let Some(color) = text.decode().ok().and_then(|value| {
                        quick_xml::escape::unescape(&value)
                            .ok()
                            .and_then(|unescaped| parse_android_color(&unescaped))
                    }) {
                        return Some(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
                            512, 512, color,
                        )));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    render_android_vector(xml)
}

fn mask_android_adaptive_icon(canvas: RgbaImage) -> RgbaImage {
    // Android adaptive layers use a 108dp canvas, while the launcher mask occupies the centered
    // 72dp. Cropping that motion-safe perimeter matches the installed icon's apparent scale.
    let crop_size = canvas.width().min(canvas.height()) * 2 / 3;
    let crop_x = (canvas.width() - crop_size) / 2;
    let crop_y = (canvas.height() - crop_size) / 2;
    let cropped =
        image::imageops::crop_imm(&canvas, crop_x, crop_y, crop_size, crop_size).to_image();
    let mut output =
        image::imageops::resize(&cropped, 512, 512, image::imageops::FilterType::Lanczos3);
    let center = 255.5_f32;
    let radius = 255.5_f32;
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
        let mask_alpha = ((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u32;
        pixel[3] = ((u32::from(pixel[3]) * mask_alpha + 127) / 255) as u8;
    }
    output
}

fn android_xml_drawable_reference(xml: &str, element: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().as_ref() == element.as_bytes() =>
            {
                for attr in e.attributes().flatten() {
                    if matches!(
                        attr.key.as_ref(),
                        b"android:drawable" | b"drawable" | b"android:color" | b"color"
                    ) {
                        return attr
                            .normalized_value(XmlVersion::Implicit1_0)
                            .ok()
                            .map(|value| value.into_owned());
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn parse_android_color(value: &str) -> Option<Rgba<u8>> {
    let hex = value.trim().strip_prefix('#')?;
    let raw = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(Rgba([
            ((raw >> 16) & 0xff) as u8,
            ((raw >> 8) & 0xff) as u8,
            (raw & 0xff) as u8,
            255,
        ])),
        8 => Some(Rgba([
            ((raw >> 16) & 0xff) as u8,
            ((raw >> 8) & 0xff) as u8,
            (raw & 0xff) as u8,
            ((raw >> 24) & 0xff) as u8,
        ])),
        _ => None,
    }
}

fn render_android_vector(xml: &str) -> Option<DynamicImage> {
    if !xml.contains("<vector") {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    let mut viewport_width = 24.0_f32;
    let mut viewport_height = 24.0_f32;
    let mut paths = String::new();
    let mut group_depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"vector" => {
                viewport_width =
                    android_float_attribute(&e, b"android:viewportWidth").unwrap_or(viewport_width);
                viewport_height = android_float_attribute(&e, b"android:viewportHeight")
                    .unwrap_or(viewport_height);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"path" => {
                let data = android_string_attribute(&e, b"android:pathData")?;
                let fill = android_string_attribute(&e, b"android:fillColor")
                    .filter(|value| parse_android_color(value).is_some())
                    .unwrap_or_else(|| "none".to_string());
                let stroke = android_string_attribute(&e, b"android:strokeColor")
                    .filter(|value| parse_android_color(value).is_some());
                if fill != "none" || stroke.is_some() {
                    paths.push_str(&format!(
                        "<path d=\"{}\" fill=\"{}\"",
                        xml_escape(&data),
                        fill
                    ));
                    if let Some(stroke) = stroke {
                        let width =
                            android_float_attribute(&e, b"android:strokeWidth").unwrap_or(1.0);
                        paths.push_str(&format!(
                            " stroke=\"{}\" stroke-width=\"{}\"",
                            stroke, width
                        ));
                    }
                    paths.push_str("/>");
                }
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"group" => {
                paths.push_str(&android_svg_group_start(&e));
                group_depth += 1;
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"group" && group_depth > 0 => {
                paths.push_str("</g>");
                group_depth -= 1;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    for _ in 0..group_depth {
        paths.push_str("</g>");
    }
    if paths.is_empty() || viewport_width <= 0.0 || viewport_height <= 0.0 {
        return None;
    }
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {viewport_width} {viewport_height}\">{paths}</svg>");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(512, 512)?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(512.0 / viewport_width, 512.0 / viewport_height);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            pixel[0] = ((u32::from(pixel[0]) * 255 + alpha / 2) / alpha).min(255) as u8;
            pixel[1] = ((u32::from(pixel[1]) * 255 + alpha / 2) / alpha).min(255) as u8;
            pixel[2] = ((u32::from(pixel[2]) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    RgbaImage::from_raw(512, 512, rgba).map(DynamicImage::ImageRgba8)
}

fn android_svg_group_start(element: &BytesStart<'_>) -> String {
    let pivot_x = android_float_attribute(element, b"android:pivotX").unwrap_or(0.0);
    let pivot_y = android_float_attribute(element, b"android:pivotY").unwrap_or(0.0);
    let scale_x = android_float_attribute(element, b"android:scaleX").unwrap_or(1.0);
    let scale_y = android_float_attribute(element, b"android:scaleY").unwrap_or(1.0);
    let translate_x = android_float_attribute(element, b"android:translateX").unwrap_or(0.0);
    let translate_y = android_float_attribute(element, b"android:translateY").unwrap_or(0.0);
    let rotation = android_float_attribute(element, b"android:rotation").unwrap_or(0.0);
    format!(
        "<g transform=\"translate({},{}) rotate({}) scale({},{}) translate({},{})\">",
        pivot_x + translate_x,
        pivot_y + translate_y,
        rotation,
        scale_x,
        scale_y,
        -pivot_x,
        -pivot_y,
    )
}

fn android_string_attribute(element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attr| {
            attr.key.as_ref() == name
                || attr.key.as_ref() == name.strip_prefix(b"android:").unwrap_or(name)
        })
        .and_then(|attr| {
            attr.normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

fn android_float_attribute(element: &BytesStart<'_>, name: &[u8]) -> Option<f32> {
    android_string_attribute(element, name)?
        .trim_end_matches("dp")
        .parse()
        .ok()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
