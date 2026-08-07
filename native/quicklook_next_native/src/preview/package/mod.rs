use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use image::GenericImageView;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

use super::{
    format_bytes, format_number, image_to_bgra, is_supported_zip_image_name,
    load_bounded_embedded_image, open_validated_zip, preview_cancelled, read_limited_to_end,
    read_reader_exact_bounded_cancelable, read_zip_text, to_json, PreviewReadyDto,
    ReaderPreviewError, MAX_ARCHIVE_SCAN_ENTRIES,
};

mod android;
#[cfg(test)]
mod tests;

const MAX_APPX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PACKAGE_ICON_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_PACKAGE_HANDLE_INPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKAGE_ZIP_ENTRIES: u64 = 100_000;
pub(super) const MAX_ANDROID_RESOURCE_TABLE_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS: usize = 64;

pub(super) fn is_package_path(lower_path: &str) -> bool {
    [
        ".apk",
        ".apks",
        ".aab",
        ".msix",
        ".msixbundle",
        ".appx",
        ".appxbundle",
    ]
    .iter()
    .any(|e| lower_path.ends_with(e))
}

pub(super) fn render_package(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let source_len = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => return String::new(),
    };
    render_package_reader(file, path, source_len, cancel_cb).unwrap_or_default()
}

pub(crate) fn render_package_reader<R: Read + Seek>(
    reader: R,
    logical_name: &str,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_PACKAGE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if !is_package_path(&lower) {
        return Err(ReaderPreviewError::Malformed);
    }
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let platform = if lower.ends_with(".apk") || lower.ends_with(".apks") || lower.ends_with(".aab")
    {
        "Android"
    } else {
        "Windows"
    };

    let mut zip = open_validated_zip(reader, source_len, MAX_PACKAGE_ZIP_ENTRIES, cancel_cb)?;

    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut uncompressed = 0i64;
    let mut compressed = 0i64;
    let mut has_icon = false;
    let mut has_manifest = false;
    let mut dex_count = 0u64;
    let mut certificate_count = 0u64;
    let mut native_abis = BTreeMap::<String, ()>::new();
    let mut appx_manifest_name: Option<String> = None;

    let partial = zip.len() > MAX_ARCHIVE_SCAN_ENTRIES;
    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let entry = match zip.by_index_raw(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().replace('\\', "/");
        let lower_name = name.to_ascii_lowercase();
        if lower_name.is_empty() {
            continue;
        }
        let is_folder = lower_name.ends_with('/');
        if is_folder {
            folder_count += 1;
        } else {
            file_count += 1;
            uncompressed += entry.size() as i64;
            compressed += entry.compressed_size() as i64;
        }

        if lower_name == "androidmanifest.xml" || lower_name.ends_with("/androidmanifest.xml") {
            has_manifest = true;
        }
        if lower_name == "appxmanifest.xml" && entry.size() <= MAX_APPX_MANIFEST_BYTES {
            has_manifest = true;
            appx_manifest_name = Some(entry.name().to_string());
        }
        if lower_name.ends_with(".dex") {
            dex_count += 1;
        }
        if lower_name.starts_with("meta-inf/")
            && (lower_name.ends_with(".rsa")
                || lower_name.ends_with(".dsa")
                || lower_name.ends_with(".ec"))
        {
            certificate_count += 1;
        }
        if lower_name.starts_with("lib/") && lower_name.ends_with(".so") {
            if let Some(abi) = lower_name.split('/').nth(1) {
                if !abi.is_empty() {
                    native_abis.insert(abi.to_string(), ());
                }
            }
        }
        if package_icon_candidate_score(&name) > 0 {
            has_icon = true;
        }
    }

    let appx_manifest = appx_manifest_name
        .as_deref()
        .map(|name| read_package_zip_bytes(&mut zip, name, MAX_APPX_MANIFEST_BYTES, cancel_cb))
        .transpose()?
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string());

    let manifest = appx_manifest
        .as_deref()
        .and_then(parse_appx_manifest_summary);
    let display_name = manifest
        .as_ref()
        .and_then(|m| first_non_empty([m.display_name.as_deref(), m.name.as_deref()]))
        .unwrap_or(filename);
    let version = manifest
        .as_ref()
        .and_then(|m| m.version.as_deref())
        .unwrap_or("");
    let publisher = manifest
        .as_ref()
        .and_then(|m| m.publisher.as_deref())
        .unwrap_or("");
    let executable = manifest
        .as_ref()
        .and_then(|m| m.executable.as_deref())
        .unwrap_or("");

    let mut text = String::new();
    text.push_str(&format!("Name: {display_name}\n"));
    text.push_str(&format!("Kind: {platform} app package\n"));
    text.push_str(&format!("File: {filename}\n"));
    if !version.is_empty() {
        text.push_str(&format!("Version: {version}\n"));
    }
    if !publisher.is_empty() {
        text.push_str(&format!("Publisher: {publisher}\n"));
    }
    if !executable.is_empty() {
        text.push_str(&format!("Executable: {executable}\n"));
    }
    text.push_str(&format!("Files: {}\n", format_number(file_count as i64)));
    if folder_count > 0 {
        text.push_str(&format!(
            "Folders: {}\n",
            format_number(folder_count as i64)
        ));
    }
    text.push_str(&format!(
        "Uncompressed size: {}\n",
        format_bytes(uncompressed)
    ));
    if compressed > 0 {
        text.push_str(&format!("Package size: {}\n", format_bytes(compressed)));
    }
    text.push_str(&format!(
        "Manifest: {}\n",
        if has_manifest { "present" } else { "not found" }
    ));
    text.push_str(&format!(
        "Preview image: {}\n",
        if has_icon { "found" } else { "system fallback" }
    ));
    if partial {
        text.push_str("Listing scan stopped after 10,000 entries.\n");
    }

    if platform == "Android" {
        if dex_count > 0 {
            text.push_str(&format!("DEX files: {}\n", format_number(dex_count as i64)));
        }
        if !native_abis.is_empty() {
            text.push_str(&format!(
                "Native ABIs: {}\n",
                native_abis.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if certificate_count > 0 {
            text.push_str(&format!(
                "Signing blocks: {}\n",
                format_number(certificate_count as i64)
            ));
        }
    }

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(to_json(&PreviewReadyDto {
        kind: "package".to_string(),
        title: if version.is_empty() {
            format!("{display_name} - {platform} package")
        } else {
            format!("{display_name} - {version}")
        },
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    }))
}

#[derive(Default)]
struct AppxManifestSummary {
    name: Option<String>,
    version: Option<String>,
    publisher: Option<String>,
    display_name: Option<String>,
    executable: Option<String>,
    icon_paths: Vec<String>,
}

fn parse_appx_manifest_summary(xml: &str) -> Option<AppxManifestSummary> {
    let mut reader = Reader::from_str(xml);
    let mut summary = AppxManifestSummary::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())
                    .ok()?
                    .to_ascii_lowercase();
                for attr in e.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let value = attr
                        .normalized_value(XmlVersion::Implicit1_0)
                        .ok()
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    if value.is_empty() {
                        continue;
                    }
                    match (name.as_str(), key.as_str()) {
                        ("identity", "name") => summary.name = Some(value),
                        ("identity", "version") => summary.version = Some(value),
                        ("identity", "publisher") => summary.publisher = Some(value),
                        ("application", "executable") => summary.executable = Some(value),
                        ("uap:visualelements", "displayname")
                        | ("visualelements", "displayname") => summary.display_name = Some(value),
                        ("uap:visualelements", "square150x150logo")
                        | ("visualelements", "square150x150logo")
                        | ("uap:visualelements", "square44x44logo")
                        | ("visualelements", "square44x44logo")
                        | ("uap:visualelements", "logo")
                        | ("visualelements", "logo")
                        | ("uap:defaulttile", "square310x310logo")
                        | ("defaulttile", "square310x310logo")
                        | ("uap:defaulttile", "wide310x150logo")
                        | ("defaulttile", "wide310x150logo") => summary.icon_paths.push(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(summary)
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<&str> {
    values.into_iter().flatten().find(|v| !v.trim().is_empty())
}

pub(crate) fn extract_package_icon_bgra(
    path: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(u32, u32, Vec<u8>)> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let source_len = file.metadata().ok()?.len();
    extract_package_icon_bgra_reader(file, source_len, path, cancel_cb).ok()
}

pub(crate) fn extract_package_icon_bgra_reader<R: Read + Seek>(
    reader: R,
    source_len: u64,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(u32, u32, Vec<u8>), ReaderPreviewError> {
    if source_len > MAX_PACKAGE_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if !is_package_path(&lower) {
        return Err(ReaderPreviewError::Malformed);
    }
    let mut zip = open_validated_zip(reader, source_len, MAX_PACKAGE_ZIP_ENTRIES, cancel_cb)?;
    if lower.ends_with(".apk") {
        if let Some(icon) = android::extract_android_package_icon(&mut zip, cancel_cb) {
            return Ok(icon);
        }
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
    }
    let mut candidates = Vec::new();
    let manifest_icons = read_zip_text(&mut zip, "AppxManifest.xml", MAX_APPX_MANIFEST_BYTES)
        .as_deref()
        .and_then(parse_appx_manifest_summary)
        .map(|summary| expand_appx_icon_candidates(&summary.icon_paths))
        .unwrap_or_default();

    for i in 0..zip.len().min(MAX_ARCHIVE_SCAN_ENTRIES) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.replace('\\', "/");
        let score = package_icon_candidate_score(&normalized_name)
            + manifest_icon_candidate_score(&normalized_name, &manifest_icons);
        if score > 0 && entry.size() <= MAX_PACKAGE_ICON_BYTES {
            candidates.push((score, raw_name));
            if candidates.len() >= 256 {
                break;
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut best: Option<(i32, u32, u32, Vec<u8>)> = None;
    for (path_score, name) in candidates.into_iter().take(32) {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let Ok(mut entry) = zip.by_name(&name) else {
            continue;
        };
        let entry_size = entry.size();
        let bytes = match read_reader_exact_bounded_cancelable(
            &mut entry,
            entry_size,
            MAX_PACKAGE_ICON_BYTES,
            cancel_cb,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                if preview_cancelled(cancel_cb) {
                    return Err(package_zip_read_error(error));
                }
                continue;
            }
        };
        let Some(image) = load_bounded_embedded_image(&bytes) else {
            continue;
        };
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        let (original_width, original_height) = image.dimensions();
        if original_width < 16 || original_height < 16 {
            continue;
        }
        let area_score = ((original_width.min(512) * original_height.min(512)) / 256) as i32;
        let score = path_score + area_score;
        let Some((width, height, bgra)) = image_to_bgra(image, 512) else {
            continue;
        };
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, width, height, bgra));
        }
    }

    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    best.map(|(_, width, height, bgra)| (width, height, bgra))
        .ok_or(ReaderPreviewError::Malformed)
}

fn read_zip_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    read_limited_to_end(&mut entry, limit)
}

fn read_package_zip_bytes<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut entry = zip.by_name(name).map_err(|_| {
        if preview_cancelled(cancel_cb) {
            ReaderPreviewError::Cancelled
        } else {
            ReaderPreviewError::Malformed
        }
    })?;
    let entry_size = entry.size();
    read_reader_exact_bounded_cancelable(&mut entry, entry_size, limit, cancel_cb)
        .map_err(package_zip_read_error)
}

fn package_zip_read_error(error: ReaderPreviewError) -> ReaderPreviewError {
    match error {
        ReaderPreviewError::Cancelled | ReaderPreviewError::LimitExceeded => error,
        ReaderPreviewError::Io
        | ReaderPreviewError::Malformed
        | ReaderPreviewError::LengthMismatch => ReaderPreviewError::Malformed,
    }
}

fn expand_appx_icon_candidates(paths: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();
    for path in paths {
        let normalized = path
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        candidates.push(normalized.clone());
        let Some((stem, ext)) = normalized.rsplit_once('.') else {
            continue;
        };
        for qualifier in [
            ".scale-400",
            ".scale-200",
            ".scale-150",
            ".scale-125",
            ".scale-100",
            ".targetsize-256",
            ".targetsize-128",
            ".targetsize-96",
            ".targetsize-64",
            ".targetsize-48",
            ".targetsize-32",
            ".targetsize-24",
            ".targetsize-16",
            ".altform-unplated_targetsize-256",
            ".altform-unplated_targetsize-48",
        ] {
            candidates.push(format!("{stem}{qualifier}.{ext}"));
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn manifest_icon_candidate_score(name: &str, manifest_icons: &[String]) -> i32 {
    if manifest_icons.is_empty() {
        return 0;
    }
    let lower = name
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if manifest_icons.iter().any(|candidate| candidate == &lower) {
        return 320;
    }

    let Some((stem, _)) = lower.rsplit_once('.') else {
        return 0;
    };
    if manifest_icons
        .iter()
        .filter_map(|candidate| {
            candidate
                .rsplit_once('.')
                .map(|(candidate_stem, _)| candidate_stem)
        })
        .any(|candidate_stem| stem.starts_with(candidate_stem))
    {
        260
    } else {
        0
    }
}

fn package_icon_candidate_score(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    if !is_supported_zip_image_name(&lower) {
        return 0;
    }
    let is_android_mipmap = lower.starts_with("res/mipmap") || lower.contains("/res/mipmap");

    let mut score = 0;
    if lower.contains("ic_launcher") {
        score += 260;
    }
    if lower.contains("square150x150logo") {
        score += 240;
    }
    if lower.contains("square44x44logo") {
        score += 220;
    }
    if lower.contains("storelogo") {
        score += 210;
    }
    if lower.contains("appicon") {
        score += 190;
    }
    if lower.contains("logo") {
        score += 160;
    }
    if lower.contains("icon") {
        score += 140;
    }
    if score == 0 && !is_android_mipmap {
        return 0;
    }

    if lower.starts_with("assets/") || lower.contains("/assets/") {
        score += 30;
    }
    if is_android_mipmap {
        score += 30;
    }
    if lower.contains("/drawable") || lower.starts_with("res/drawable") {
        score += 15;
    }
    if lower.contains("scale-400") {
        score += 24;
    } else if lower.contains("scale-200") {
        score += 18;
    } else if lower.contains("scale-150") {
        score += 12;
    } else if lower.contains("scale-100") {
        score += 6;
    }
    if lower.ends_with(".png") {
        score += 8;
    }
    score
}
