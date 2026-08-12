use std::fs;
use std::io::Write;

use super::super::listing::render_archive;
use super::super::{MAX_ARCHIVE_EXTRACT_BYTES, MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES};
use super::{
    archive_entry_within_extract_budget, archive_extract_output_name, create_archive_extract_root,
    discard_archive_extract_path, extract_archive_entry_to_temp,
};

#[path = "external_zip.rs"]
mod external_zip;

#[test]
fn archive_extract_budget_rejects_oversized_or_extreme_entries() {
    assert!(archive_entry_within_extract_budget(1024, 128));
    assert!(archive_entry_within_extract_budget(0, 0));
    assert!(!archive_entry_within_extract_budget(
        MAX_ARCHIVE_EXTRACT_BYTES + 1,
        1024
    ));
    assert!(!archive_entry_within_extract_budget(
        1024,
        MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES + 1
    ));
    assert!(!archive_entry_within_extract_budget(1_000_001, 1000));
    assert!(!archive_entry_within_extract_budget(1, 0));
}

#[test]
fn encrypted_zip_entries_are_reported_and_not_extracted() {
    let path = std::env::temp_dir().join(format!(
        "quicklook-next-encrypted-{}.zip",
        std::process::id()
    ));
    let file = fs::File::create(&path).expect("create encrypted zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .with_aes_encryption(zip::AesMode::Aes128, "test-password");
    writer
        .start_file("secret.txt", options)
        .expect("start encrypted entry");
    writer.write_all(b"secret").expect("write encrypted entry");
    writer.finish().expect("finish encrypted zip");

    let json = render_archive(path.to_str().unwrap(), None);
    let extracted = extract_archive_entry_to_temp(path.to_str().unwrap(), "secret.txt", None);
    let _ = fs::remove_file(path);

    assert!(json.contains("\"encryptedFileCount\":1"));
    assert!(json.contains("\"isEncrypted\":true"));
    assert!(extracted.is_none());
}

#[test]
fn archive_extract_output_name_is_lossless_and_keeps_safe_extension() {
    let first = archive_extract_output_name("folder/a:b?.png");
    let second = archive_extract_output_name("folder/a<b>.png");

    assert_ne!(first, second);
    assert!(first.ends_with(".png"));
    assert!(first.starts_with("entry-666f6c6465722f613a623f2e706e67"));
}

#[test]
fn archive_extract_discard_only_removes_generated_roots() {
    let generated_root = create_archive_extract_root().expect("generated extract root");
    let generated_target = generated_root.join("entry-test");
    fs::write(&generated_target, b"temporary").expect("write generated extraction");
    discard_archive_extract_path(generated_target.to_str().unwrap());
    assert!(!generated_root.exists());

    let foreign_root = std::env::temp_dir().join(format!(
        "quicklook-next-foreign-root-{}",
        std::process::id()
    ));
    fs::create_dir_all(&foreign_root).expect("create foreign root");
    let foreign_target = foreign_root.join("entry-test");
    fs::write(&foreign_target, b"keep").expect("write foreign extraction");
    discard_archive_extract_path(foreign_target.to_str().unwrap());
    assert!(foreign_target.exists());
    let _ = fs::remove_dir_all(foreign_root);
}
