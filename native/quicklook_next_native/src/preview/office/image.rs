use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use zip::ZipArchive;

use super::super::{
    image_to_bgra, is_supported_zip_image_name, load_bounded_embedded_image, office_reader_error,
    open_validated_zip, preview_cancelled, read_office_limited_to_end, read_office_zip_bytes,
    OfficeContext, OfficeResult, ReaderPreviewError, MAX_EMBEDDED_IMAGE_DIMENSION,
    MAX_EMBEDDED_IMAGE_PIXELS, MAX_OFFICE_INLINE_IMAGE_BYTES, MAX_OFFICE_INPUT_BYTES,
    MAX_OFFICE_LAYOUT_IMAGE_DIMENSION, MAX_OFFICE_MEDIA_BYTES, MAX_OFFICE_ZIP_ENTRIES,
};

#[cfg(test)]
mod tests;

const OFFICE_MEDIA_ROOTS: &[&str] = &["word/media/", "ppt/media/", "xl/media/"];

pub(super) fn office_media_entries<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    roots: &[&str],
) -> OfficeResult<Vec<String>> {
    let mut entry_counts = BTreeMap::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > MAX_OFFICE_MEDIA_BYTES {
            continue;
        }

        let Some(normalized) = canonical_office_media_ref(entry.name(), None) else {
            continue;
        };
        let folded = normalized.to_ascii_lowercase();
        if roots.iter().any(|root| folded.starts_with(root)) {
            let (_, count) = entry_counts
                .entry(folded)
                .or_insert_with(|| (normalized, 0usize));
            *count += 1;
        }
    }
    Ok(entry_counts
        .into_iter()
        .filter_map(|(_, (entry, count))| (count == 1).then_some(entry))
        .collect())
}

pub(super) fn append_office_media_summary(out: &mut String, entries: &[String]) {
    out.push_str(&format!("Images: {}\n", entries.len()));
    if entries.is_empty() {
        return;
    }

    let names = entries
        .iter()
        .take(6)
        .map(|name| name.rsplit('/').next().unwrap_or(name.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("Image files: {names}\n"));
}

pub(super) fn office_media_root_for_part(part_path: &str) -> Option<&'static str> {
    let lower = part_path.to_ascii_lowercase();
    if lower.starts_with("word/") {
        Some("word/media/")
    } else if lower.starts_with("ppt/") {
        Some("ppt/media/")
    } else if lower.starts_with("xl/") {
        Some("xl/media/")
    } else {
        None
    }
}

fn office_media_root_for_path(path: &str) -> Option<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" => Some("word/media/"),
        "pptx" | "pptm" => Some("ppt/media/"),
        "xlsx" | "xlsm" => Some("xl/media/"),
        _ => None,
    }
}

fn canonical_office_media_ref(path: &str, expected_root: Option<&str>) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return None;
        }
        segments.push(segment);
    }
    let normalized = segments.join("/");
    if normalized != path {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    let root = match expected_root {
        Some(root) if OFFICE_MEDIA_ROOTS.contains(&root) => root,
        Some(_) => return None,
        None => OFFICE_MEDIA_ROOTS
            .iter()
            .copied()
            .find(|root| lower.starts_with(root))?,
    };
    if !lower.starts_with(root) || lower.len() <= root.len() || !is_supported_zip_image_name(&lower)
    {
        return None;
    }
    Some(normalized)
}

pub(super) fn image_mime_type(lower: &str) -> Option<&'static str> {
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".bmp") {
        Some("image/bmp")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".ico") {
        Some("image/x-icon")
    } else {
        None
    }
}

pub(super) fn read_office_layout_image_reference<R: Read + Seek>(
    context: &mut OfficeContext,
    zip: &mut ZipArchive<R>,
    requested_ref: &str,
    expected_root: &str,
) -> OfficeResult<Option<(String, u64)>> {
    let Some(requested_ref) = canonical_office_media_ref(requested_ref, Some(expected_root)) else {
        return Ok(None);
    };
    let mut exact_match: Option<(usize, String, u64)> = None;
    let mut exact_ambiguous = false;
    let mut folded_match: Option<(usize, String, u64)> = None;
    let mut folded_ambiguous = false;

    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let Some(actual_ref) = canonical_office_media_ref(entry.name(), Some(expected_root)) else {
            continue;
        };
        if actual_ref == requested_ref {
            if exact_match.is_some() {
                exact_ambiguous = true;
            } else {
                exact_match = Some((i, actual_ref, entry.size()));
            }
        } else if actual_ref.eq_ignore_ascii_case(&requested_ref) {
            if folded_match.is_some() {
                folded_ambiguous = true;
            } else {
                folded_match = Some((i, actual_ref, entry.size()));
            }
        }
    }

    let selected = if exact_ambiguous {
        None
    } else if exact_match.is_some() {
        exact_match
    } else if folded_ambiguous {
        None
    } else {
        folded_match
    };
    let Some((index, image_ref, declared_length)) = selected else {
        return Ok(None);
    };
    if declared_length == 0 || declared_length > MAX_OFFICE_INLINE_IMAGE_BYTES {
        return Ok(None);
    }
    let mut entry = match zip.by_index(index) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let Some(bytes) =
        read_office_limited_to_end(context, &mut entry, MAX_OFFICE_INLINE_IMAGE_BYTES)?
    else {
        return Ok(None);
    };
    if bytes.is_empty() || bytes.len() as u64 != declared_length {
        return Ok(None);
    }
    Ok(Some((image_ref, bytes.len() as u64)))
}

pub(in crate::preview) fn extract_office_image_bgra(
    path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let source_len = file.metadata().ok()?.len();
    extract_office_image_bgra_reader(file, source_len, path, cancel_cb).ok()
}

pub(in crate::preview) fn extract_office_image_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if source_len > MAX_OFFICE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let mut zip = open_validated_zip(reader, source_len, MAX_OFFICE_ZIP_ENTRIES as u64, cancel_cb)?;
    let roots = office_media_roots_for_path(logical_name);
    if roots.is_empty() {
        return Err(ReaderPreviewError::Malformed);
    }

    let mut candidates = Vec::new();
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.size() > MAX_OFFICE_MEDIA_BYTES {
            continue;
        }

        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let lower = normalized_name.to_ascii_lowercase();
        if !roots.iter().any(|root| lower.starts_with(root)) || !is_supported_zip_image_name(&lower)
        {
            continue;
        }

        candidates.push((office_image_candidate_score(&lower, entry.size()), raw_name));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    let mut context = OfficeContext::new(cancel_cb);
    for (path_score, name) in candidates.into_iter().take(24) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let bytes =
            match read_office_zip_bytes(&mut context, &mut zip, &name, MAX_OFFICE_MEDIA_BYTES) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(error) => return Err(office_reader_error(error)),
            };
        let Some(image) = load_bounded_embedded_image(&bytes) else {
            continue;
        };
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let (original_width, original_height) = image.dimensions();
        if original_width < 8 || original_height < 8 {
            continue;
        }

        let area_score = ((original_width.min(768) * original_height.min(768)) / 512) as i32;
        let score = path_score + area_score;
        let Some((width, height, bgra)) = image_to_bgra(image, 768) else {
            continue;
        };
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, width, height, bgra));
        }
    }

    best.map(|(_, width, height, bgra)| (width, height, bgra))
        .ok_or(ReaderPreviewError::Malformed)
}

pub(in crate::preview) fn office_layout_image_ref_is_valid(
    logical_name: &str,
    image_ref: &str,
) -> bool {
    let Some(expected_root) = office_media_root_for_path(logical_name) else {
        return false;
    };
    canonical_office_media_ref(image_ref, Some(expected_root))
        .is_some_and(|normalized| normalized == image_ref)
}

pub(in crate::preview) fn extract_office_layout_image_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    image_ref: &str,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if source_len > MAX_OFFICE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if target_width == 0
        || target_height == 0
        || target_width > MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
        || target_height > MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let Some(expected_root) = office_media_root_for_path(logical_name) else {
        return Err(ReaderPreviewError::Malformed);
    };
    let Some(canonical_ref) = canonical_office_media_ref(image_ref, Some(expected_root)) else {
        return Err(ReaderPreviewError::Malformed);
    };
    if canonical_ref != image_ref {
        return Err(ReaderPreviewError::Malformed);
    }
    let Some(required_format) = office_image_format(&canonical_ref) else {
        return Err(ReaderPreviewError::Malformed);
    };

    let mut zip = open_validated_zip(reader, source_len, MAX_OFFICE_ZIP_ENTRIES as u64, cancel_cb)?;
    let mut selected: Option<(usize, u64)> = None;
    for i in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.name() != canonical_ref {
            continue;
        }
        if canonical_office_media_ref(entry.name(), Some(expected_root)).as_deref()
            != Some(canonical_ref.as_str())
            || selected.is_some()
        {
            return Err(ReaderPreviewError::Malformed);
        }
        selected = Some((i, entry.size()));
    }
    let Some((index, declared_length)) = selected else {
        return Err(ReaderPreviewError::Malformed);
    };
    if declared_length == 0 || declared_length > MAX_OFFICE_INLINE_IMAGE_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }

    let mut context = OfficeContext::new(cancel_cb);
    let mut entry = zip
        .by_index(index)
        .map_err(|_| ReaderPreviewError::Malformed)?;
    let bytes = read_office_limited_to_end(&mut context, &mut entry, MAX_OFFICE_INLINE_IMAGE_BYTES)
        .map_err(office_reader_error)?
        .ok_or(ReaderPreviewError::LimitExceeded)?;
    if bytes.is_empty() || bytes.len() as u64 != declared_length {
        return Err(ReaderPreviewError::Malformed);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let image_reader = ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if image_reader.format() != Some(required_format) {
        return Err(ReaderPreviewError::Malformed);
    }
    let (width, height) = image_reader
        .into_dimensions()
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if width == 0
        || height == 0
        || width > MAX_EMBEDDED_IMAGE_DIMENSION
        || height > MAX_EMBEDDED_IMAGE_DIMENSION
        || u64::from(width)
            .checked_mul(u64::from(height))
            .is_none_or(|pixels| pixels > MAX_EMBEDDED_IMAGE_PIXELS)
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let image = image::load_from_memory_with_format(&bytes, required_format)
        .map_err(|_| ReaderPreviewError::Malformed)?;
    if image.dimensions() != (width, height) {
        return Err(ReaderPreviewError::Malformed);
    }
    office_layout_image_to_bgra(image, target_width, target_height, cancel_cb)
}

fn office_image_format(image_ref: &str) -> Option<ImageFormat> {
    let lower = image_ref.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some(ImageFormat::Png)
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some(ImageFormat::Jpeg)
    } else if lower.ends_with(".ico") {
        Some(ImageFormat::Ico)
    } else if lower.ends_with(".webp") {
        Some(ImageFormat::WebP)
    } else if lower.ends_with(".bmp") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn office_layout_image_to_bgra(
    image: DynamicImage,
    target_width: u32,
    target_height: u32,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        return Err(ReaderPreviewError::Malformed);
    }
    let scale = (target_width as f64 / original_width as f64)
        .min(target_height as f64 / original_height as f64)
        .min(1.0);
    let width = ((original_width as f64 * scale).round() as u32).max(1);
    let height = ((original_height as f64 * scale).round() as u32).max(1);
    let raster = if (width, height) == (original_width, original_height) {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let rgba = raster.to_rgba8();
    let output_length = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ReaderPreviewError::LimitExceeded)?,
    )
    .map_err(|_| ReaderPreviewError::LimitExceeded)?;
    let mut bgra = Vec::with_capacity(output_length);
    for (index, pixel) in rgba.chunks_exact(4).enumerate() {
        if index % 65_536 == 0 && preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let red = pixel[0] as u32;
        let green = pixel[1] as u32;
        let blue = pixel[2] as u32;
        let alpha = pixel[3] as u32;
        bgra.push(((blue * alpha + 127) / 255) as u8);
        bgra.push(((green * alpha + 127) / 255) as u8);
        bgra.push(((red * alpha + 127) / 255) as u8);
        bgra.push(alpha as u8);
    }
    if bgra.len() != output_length {
        return Err(ReaderPreviewError::Malformed);
    }
    Ok((width, height, bgra))
}

fn office_media_roots_for_path(path: &str) -> &'static [&'static str] {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" => &["word/media/"],
        "xlsx" | "xlsm" => &["xl/media/"],
        "pptx" | "pptm" => &["ppt/media/"],
        _ => &[],
    }
}

fn office_image_candidate_score(lower: &str, size: u64) -> i32 {
    let mut score = 0;
    if lower.ends_with(".png") {
        score += 30;
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        score += 24;
    } else if lower.ends_with(".webp") {
        score += 18;
    } else if lower.ends_with(".bmp") {
        score += 10;
    }
    if lower.contains("image") {
        score += 8;
    }
    score + ((size.min(4 * 1024 * 1024) / 4096) as i32).min(256)
}
