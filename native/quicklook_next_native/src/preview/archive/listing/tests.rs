use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Write};
use std::time::{Duration, Instant};

use super::{
    archive_largest_file_summary, archive_project_summary, archive_type_summary,
    render_archive_reader, TarScanReader,
};
use crate::preview::{MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_SCAN_ENTRIES};

#[test]
fn archive_reader_supports_tar_tgz_and_gzip_without_a_path() {
    let payload = b"reader archive";
    let mut tar_builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("folder/item.txt").expect("set TAR path");
    header.set_size(payload.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar_builder
        .append(&header, payload.as_slice())
        .expect("append TAR entry");
    let tar_bytes = tar_builder.into_inner().expect("finish TAR");
    let tar_json = render_archive_reader(
        Cursor::new(tar_bytes.clone()),
        r"C:\missing\logical.tar",
        tar_bytes.len() as u64,
        0,
        None,
    )
    .expect("TAR reader preview");
    assert!(tar_json.contains("\"rootPath\":\"\""));
    assert!(tar_json.contains("\"canPreviewEntries\":false"));
    assert!(tar_json.contains("folder/item.txt"));

    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar_bytes).expect("compress TAR");
    let tgz_bytes = gzip.finish().expect("finish TGZ");
    let tgz_json = render_archive_reader(
        Cursor::new(tgz_bytes.clone()),
        "logical.tgz",
        tgz_bytes.len() as u64,
        0,
        None,
    )
    .expect("TGZ reader preview");
    assert!(tgz_json.contains("\"canPreviewEntries\":false"));
    assert!(tgz_json.contains("folder/item.txt"));

    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(payload).expect("compress GZIP member");
    let gzip_bytes = gzip.finish().expect("finish GZIP");
    let gzip_json = render_archive_reader(
        Cursor::new(gzip_bytes.clone()),
        "logical.txt.gz",
        gzip_bytes.len() as u64,
        123,
        None,
    )
    .expect("GZIP reader preview");
    let gzip_json: serde_json::Value = serde_json::from_str(&gzip_json).expect("GZIP listing JSON");
    assert_eq!(gzip_json["listing"]["rootPath"], "");
    assert_eq!(gzip_json["listing"]["canPreviewEntries"], false);
    assert_eq!(gzip_json["listing"]["items"][0]["path"], "logical.txt");
    assert_eq!(
        gzip_json["listing"]["items"][0]["size"],
        payload.len() as u64
    );
}

#[test]
fn archive_zip_reader_retains_partial_listing_below_hard_entry_cap() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..=MAX_ARCHIVE_SCAN_ENTRIES {
        writer
            .start_file(
                format!("entry-{index:05}.txt"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start bounded ZIP entry");
    }
    let bytes = writer.finish().expect("finish large ZIP").into_inner();
    let json = render_archive_reader(
        Cursor::new(bytes.clone()),
        "many.zip",
        bytes.len() as u64,
        0,
        None,
    )
    .expect("partial archive listing");
    let json: serde_json::Value = serde_json::from_str(&json).expect("archive JSON");
    assert_eq!(json["listing"]["isPartial"], true);
    assert!(json["listing"]["items"].as_array().unwrap().len() <= MAX_ARCHIVE_ENTRIES);
}

#[test]
fn tar_scan_reader_stops_at_decompressed_byte_budget() {
    let mut reader = TarScanReader {
        reader: Cursor::new(vec![1, 2, 3, 4, 5]),
        remaining: 4,
        deadline: Instant::now() + Duration::from_secs(1),
        cancel_cb: None,
    };
    let mut buffer = [0u8; 8];

    assert_eq!(reader.read(&mut buffer).expect("read within budget"), 4);
    assert_eq!(
        reader
            .read(&mut buffer)
            .expect_err("budget exhaustion")
            .kind(),
        io::ErrorKind::Interrupted
    );
}

extern "C" fn always_cancel() -> bool {
    true
}

#[test]
fn tar_scan_reader_honors_cancellation() {
    let mut reader = TarScanReader::new(Cursor::new(vec![1]), Some(always_cancel));
    let mut buffer = [0u8; 1];

    assert_eq!(
        reader.read(&mut buffer).expect_err("cancelled scan").kind(),
        io::ErrorKind::Interrupted
    );
}

#[test]
fn tar_scan_reader_honors_deadline() {
    let mut reader = TarScanReader {
        reader: Cursor::new(vec![1]),
        remaining: 1,
        deadline: Instant::now() - Duration::from_secs(1),
        cancel_cb: None,
    };
    let mut buffer = [0u8; 1];

    assert_eq!(
        reader.read(&mut buffer).expect_err("expired scan").kind(),
        io::ErrorKind::Interrupted
    );
}

#[test]
fn archive_type_summary_counts_common_types() {
    let mut entries = BTreeMap::new();
    entries.insert(
        "src/".to_string(),
        ("src".to_string(), "".to_string(), true, 0, 0, 0, false),
    );
    entries.insert(
        "src/main.rs".to_string(),
        (
            "main.rs".to_string(),
            "src/".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );
    entries.insert(
        "src/lib.rs".to_string(),
        (
            "lib.rs".to_string(),
            "src/".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );
    entries.insert(
        "README.md".to_string(),
        (
            "README.md".to_string(),
            "".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );

    assert_eq!(
        archive_type_summary(&entries).as_deref(),
        Some("RS File 2, MD File 1")
    );
}

#[test]
fn archive_project_summary_detects_project_markers() {
    let mut entries = BTreeMap::new();
    entries.insert(
        "app/package.json".to_string(),
        (
            "package.json".to_string(),
            "app/".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );
    entries.insert(
        "src/QuickLook.Next.csproj".to_string(),
        (
            "QuickLook.Next.csproj".to_string(),
            "src/".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );

    assert_eq!(
        archive_project_summary(&entries).as_deref(),
        Some(".csproj, package.json")
    );
}

#[test]
fn archive_largest_file_summary_is_bounded_and_sorted() {
    let mut entries = BTreeMap::new();
    entries.insert(
        "small.txt".to_string(),
        (
            "small.txt".to_string(),
            "".to_string(),
            false,
            10,
            8,
            0,
            false,
        ),
    );
    entries.insert(
        "assets/large.bin".to_string(),
        (
            "large.bin".to_string(),
            "assets/".to_string(),
            false,
            4096,
            100,
            0,
            false,
        ),
    );
    entries.insert(
        "assets/medium.bin".to_string(),
        (
            "medium.bin".to_string(),
            "assets/".to_string(),
            false,
            2048,
            100,
            0,
            false,
        ),
    );
    entries.insert(
        "assets/tiny.bin".to_string(),
        (
            "tiny.bin".to_string(),
            "assets/".to_string(),
            false,
            1,
            1,
            0,
            false,
        ),
    );

    let summary = archive_largest_file_summary(&entries).expect("largest files");
    assert_eq!(
        summary,
        "assets/large.bin (4.00 KB), assets/medium.bin (2.00 KB), small.txt (10 B)"
    );
    assert!(!summary.contains("tiny.bin"));
}
