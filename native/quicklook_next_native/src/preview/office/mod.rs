use std::collections::BTreeSet;
use std::io::{Read, Seek};

use zip::ZipArchive;

mod document;
mod image;
mod layout;
mod presentation;
mod workbook;

use super::{OfficeContext, OfficeResult, MAX_OFFICE_ZIP_ENTRIES};

pub(super) use document::{render_docx, render_odf};
pub(super) use image::{
    extract_office_image_bgra, extract_office_image_bgra_reader,
    extract_office_layout_image_bgra_reader, office_layout_image_ref_is_valid,
};
pub(super) use presentation::render_pptx;
pub(super) use workbook::render_xlsx;

fn count_numbered_parts<R: Read + Seek>(
    context: &OfficeContext,
    zip: &mut ZipArchive<R>,
    directory: &str,
    stem: &str,
) -> OfficeResult<usize> {
    let mut indices = BTreeSet::new();
    for index in 0..zip.len().min(MAX_OFFICE_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(index) else {
            continue;
        };
        let normalized = entry.name().replace('\\', "/").to_ascii_lowercase();
        let Some(file_name) = normalized.strip_prefix(directory) else {
            continue;
        };
        let Some(number) = file_name
            .strip_prefix(stem)
            .and_then(|value| value.strip_suffix(".xml"))
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
        else {
            continue;
        };
        indices.insert(number);
    }
    Ok(indices.len())
}
