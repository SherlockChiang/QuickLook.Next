use std::time::Duration;

mod extract;
mod listing;

pub(crate) use extract::{
    discard_archive_extract_path, extract_archive_entry_to_temp,
    extract_archive_entry_to_temp_reader, extract_archive_entry_to_writer_reader,
};
pub(super) use listing::render_zip_archive_from_zip;
pub(crate) use listing::{is_archive, render_archive, render_archive_reader};

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
