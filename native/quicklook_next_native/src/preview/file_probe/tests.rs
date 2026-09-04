use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{classify, probe_path_json};
use crate::{preview, rar_listing};

static PROBE_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_probe_path(extension: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "quicklook-next-probe-{}-{}-{}.{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after Unix epoch")
            .as_nanos(),
        PROBE_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        extension
    ))
}

#[test]
fn classify_accepts_known_and_sniffed_config_text() {
    assert_eq!(
        classify("app.config", ".config", b"<configuration>", false),
        "text"
    );
    assert_eq!(
        classify("mysql.cnf", ".cnf", b"[client]\r\nport=3306\r\n", false),
        "text"
    );
    assert_eq!(
        classify(
            "vendor.custom",
            ".custom",
            b"feature.enabled=true\r\n",
            false
        ),
        "text"
    );
    assert_eq!(classify("settings", "", b"root = true\r\n", false), "text");
    assert_eq!(
        classify("legacy.vendor", ".vendor", b"name=caf\xE9\r\n", false),
        "text"
    );
}

#[test]
fn classify_routes_svg_to_image_preview() {
    assert_eq!(
        classify("drawing.svg", ".svg", b"<svg xmlns=", false),
        "image"
    );
}

#[test]
fn classify_routes_sqlite_auxiliary_suffixes_to_database_preview() {
    assert_eq!(classify("data.db-wal", ".db-wal", &[], false), "database");
    assert_eq!(
        classify("data.sqlite3-shm", ".sqlite3-shm", &[], false),
        "database"
    );
}

#[test]
fn classify_requires_rar_magic_instead_of_trusting_the_extension() {
    assert_eq!(
        classify("archive.rar", ".rar", rar_listing::RAR5_SIGNATURE, false),
        "archive"
    );
    assert_eq!(
        classify("legacy.bin", ".bin", rar_listing::RAR4_SIGNATURE, false),
        "archive"
    );
    assert_eq!(
        classify("renamed.rar", ".rar", &[0, 1, 2, 3, 4, 5, 6, 7], false),
        "binary"
    );
    assert!(!preview::is_archive(
        ".rar",
        "archive",
        b"Rar!\x1a\x07\x02\x00"
    ));
}

#[test]
fn classify_accepts_known_text_file_names_with_empty_content() {
    assert_eq!(classify("Dockerfile", "", b"", true), "text");
    assert_eq!(classify("Makefile", "", b"", true), "text");
    assert_eq!(classify(".editorconfig", "", b"", true), "text");
    assert_eq!(classify(".gitignore", "", b"", true), "text");
    assert_eq!(classify(".env", "", b"", true), "text");
    assert_eq!(classify("settings.vendor", ".vendor", b"", true), "text");
    assert_eq!(classify("settings", "", b"", true), "text");
    assert_eq!(classify("empty.zip", ".zip", b"", true), "archive");
}

#[test]
fn classify_accepts_utf16_windows_config_text() {
    let utf16_le = [0xFF, 0xFE, b'W', 0, b'i', 0, b'n', 0];
    let utf16_be = [0xFE, 0xFF, 0, b'W', 0, b'i', 0, b'n'];
    let utf16_localized = [0xFF, 0xFE, 0x4D, 0x50, 0x3D, 0, 0x3C, 0x50];
    assert_eq!(classify("settings.reg", ".reg", &utf16_le, false), "text");
    assert_eq!(
        classify("settings.unknown", ".unknown", &utf16_be, false),
        "text"
    );
    assert_eq!(
        classify("settings.unknown", ".unknown", &utf16_localized, false),
        "text"
    );
}

#[test]
fn classify_does_not_treat_binary_prefixes_as_text() {
    assert_eq!(
        classify("file.unknown", ".unknown", &[0, 1, 2, 3, 4], false),
        "binary"
    );
    assert_eq!(
        classify("file.unknown", ".unknown", &[0xFF, 0xD9, 0x80], false),
        "binary"
    );
    assert_eq!(
        classify("file.bin", ".bin", b"d4:fake4:datae", false),
        "binary"
    );
    assert_eq!(
        classify("file.unknown", ".unknown", b"MZprintable header", false),
        "executable"
    );
}

#[test]
fn probe_cache_invalidates_when_file_changes_within_one_second() {
    let path = temp_probe_path("vendor");
    fs::write(&path, b"enabled=true\r\n").expect("write text config");
    let first = probe_path_json(path.to_str().unwrap()).expect("probe text config");
    assert!(first.contains("\"kind\":\"text\""));

    fs::write(&path, [0u8, 1, 2, 3, 4]).expect("replace with binary");
    let second = probe_path_json(path.to_str().unwrap()).expect("probe replaced config");
    let _ = fs::remove_file(path);

    assert!(second.contains("\"kind\":\"binary\""));
    assert!(second.contains("\"size\":5"));
}
