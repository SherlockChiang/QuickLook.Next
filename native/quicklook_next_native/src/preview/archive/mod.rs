use std::collections::BTreeMap;
use std::time::Duration;

mod extract;
mod listing;

pub(crate) use extract::{
    discard_archive_extract_path, extract_archive_entry_to_temp,
    extract_archive_entry_to_temp_reader, extract_archive_entry_to_writer_reader,
};
pub(super) use listing::render_zip_archive_from_zip;
pub(crate) use listing::{is_archive, render_archive, render_archive_reader};

pub(crate) const MAX_ARCHIVE_ENTRIES: usize = 5000;
pub(crate) const MAX_ARCHIVE_SCAN_ENTRIES: usize = 10_000;
pub(crate) type ArchiveListingEntry = (String, String, bool, i64, i64, i64, bool);

pub(crate) fn add_parent_folders(path: &str, entries: &mut BTreeMap<String, ArchiveListingEntry>) {
    let mut start = 0;
    while let Some(idx) = path[start..].find('/') {
        let full_idx = start + idx;
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            return;
        }
        let folder_path = format!("{}/", &path[..full_idx]);
        if !entries.contains_key(&folder_path) {
            let name = path[..full_idx]
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            entries.insert(
                folder_path.clone(),
                (name, parent_of(&folder_path), true, 0, 0, 0, false),
            );
        }
        start = full_idx + 1;
    }
}

pub(crate) fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(idx) => trimmed[..idx + 1].to_string(),
        None => String::new(),
    }
}

const MAX_RAR_RETAINED_PATH_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_HANDLE_INPUT_BYTES: u64 = 16 * 1024 * 1024 * 1024 * 1024;
pub(super) const MAX_ARCHIVE_ZIP_ENTRIES: u64 = 100_000;
const MAX_TAR_SCAN_BYTES: u64 = 512 * 1024 * 1024;
const TAR_SCAN_DEADLINE: Duration = Duration::from_secs(4);
pub(crate) const MAX_ARCHIVE_EXTRACT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_EXTRACT_RATIO: u64 = 1_000;
const ARCHIVE_EXTRACT_DEADLINE: Duration = Duration::from_secs(4);
const MAX_ARCHIVE_EXTRACT_ROOTS: usize = 32;
const ARCHIVE_EXTRACT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

const ZIP_EXTS: &[&str] = &[
    ".zip",
    ".jar",
    ".apk",
    ".apks",
    ".aab",
    ".msix",
    ".msixbundle",
    ".appx",
    ".appxbundle",
    ".nupkg",
    ".vsix",
    ".whl",
    ".cbz",
    ".xpi",
];
const TAR_EXTS: &[&str] = &[".tar"];
const TAR_GZ_EXTS: &[&str] = &[".tar.gz", ".tgz"];
const GZ_EXTS: &[&str] = &[".gz"];
const RAR_EXTS: &[&str] = &[".rar"];
