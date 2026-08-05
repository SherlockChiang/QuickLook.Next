use std::io::Cursor;

use super::super::common::read_u32_be;
use super::sqlite::{
    append_sqlite_header_details, append_sqlite_schema_group, count_sqlite_table_rows,
    decode_sqlite_utf16, parse_sqlite_schema_leaf_page, parse_sqlite_schema_record,
    parse_sqlite_schema_rows, parse_sqlite_schema_summary, parse_sqlite_table_column_names,
    parse_sqlite_table_columns, parse_sqlite_table_record, sqlite_record_integer, SqliteSchemaRow,
    MAX_SQLITE_SAMPLE_CELL_CHARS,
};
use super::wal::{
    append_sqlite_wal_summary, apply_sqlite_wal_snapshot, inspect_sqlite_wal_snapshot,
    sqlite_wal_checksum,
};
use super::{
    render_database_reader, DatabaseCompanionReader, ReaderPreviewError, MAX_SQLITE_WAL_BYTES,
};

extern "C" fn always_cancel() -> bool {
    true
}

static WAL_CANCEL_CHECKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn cancel_inside_wal_frame() -> bool {
    WAL_CANCEL_CHECKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 5
}

#[test]
fn sqlite_header_details_include_journal_and_schema_fields() {
    let mut bytes = vec![0u8; 100];
    bytes[0..16].copy_from_slice(b"SQLite format 3\0");
    bytes[18] = 2;
    bytes[19] = 2;
    bytes[36..40].copy_from_slice(&7u32.to_be_bytes());
    bytes[40..44].copy_from_slice(&11u32.to_be_bytes());
    bytes[44..48].copy_from_slice(&4u32.to_be_bytes());
    bytes[96..100].copy_from_slice(&3_045_000u32.to_be_bytes());
    let mut text = String::new();

    append_sqlite_header_details(&mut text, &bytes);

    assert!(text.contains("Journal mode: WAL"));
    assert!(text.contains("Schema format: 4 (current)"));
    assert!(text.contains("Schema cookie: 11"));
    assert!(text.contains("Freelist pages: 7"));
    assert!(text.contains("SQLite version: 3045000"));
}

fn sqlite_test_page(page_size: usize, user_version: u32, page_count: u32) -> Vec<u8> {
    let mut page = vec![0u8; page_size];
    page[0..16].copy_from_slice(b"SQLite format 3\0");
    let encoded_page_size = if page_size == 65_536 {
        1
    } else {
        page_size as u16
    };
    page[16..18].copy_from_slice(&encoded_page_size.to_be_bytes());
    page[18] = 2;
    page[19] = 2;
    page[21] = 64;
    page[22] = 32;
    page[23] = 32;
    page[28..32].copy_from_slice(&page_count.to_be_bytes());
    page[44..48].copy_from_slice(&4u32.to_be_bytes());
    page[56..60].copy_from_slice(&1u32.to_be_bytes());
    page[60..64].copy_from_slice(&user_version.to_be_bytes());
    page[96..100].copy_from_slice(&3_050_000u32.to_be_bytes());
    page[100] = 0x0D;
    page[105..107].copy_from_slice(&(page_size as u16).to_be_bytes());
    page
}

fn sqlite_test_wal(
    big_endian_checksum: bool,
    page_size: usize,
    frames: &[(u32, u32, Vec<u8>)],
) -> Vec<u8> {
    let salt = (0x1020_3040u32, 0x5060_7080u32);
    let mut wal = vec![0u8; 32];
    let magic = if big_endian_checksum {
        0x377F_0683u32
    } else {
        0x377F_0682u32
    };
    wal[0..4].copy_from_slice(&magic.to_be_bytes());
    wal[4..8].copy_from_slice(&3_007_000u32.to_be_bytes());
    wal[8..12].copy_from_slice(&(page_size as u32).to_be_bytes());
    wal[12..16].copy_from_slice(&7u32.to_be_bytes());
    wal[16..20].copy_from_slice(&salt.0.to_be_bytes());
    wal[20..24].copy_from_slice(&salt.1.to_be_bytes());
    let mut checksum = sqlite_wal_checksum(&wal[..24], big_endian_checksum, (0, 0));
    wal[24..28].copy_from_slice(&checksum.0.to_be_bytes());
    wal[28..32].copy_from_slice(&checksum.1.to_be_bytes());

    for (page_number, commit_pages, page) in frames {
        assert_eq!(page.len(), page_size);
        let mut frame_header = [0u8; 24];
        frame_header[0..4].copy_from_slice(&page_number.to_be_bytes());
        frame_header[4..8].copy_from_slice(&commit_pages.to_be_bytes());
        frame_header[8..12].copy_from_slice(&salt.0.to_be_bytes());
        frame_header[12..16].copy_from_slice(&salt.1.to_be_bytes());
        checksum = sqlite_wal_checksum(&frame_header[..8], big_endian_checksum, checksum);
        checksum = sqlite_wal_checksum(page, big_endian_checksum, checksum);
        frame_header[16..20].copy_from_slice(&checksum.0.to_be_bytes());
        frame_header[20..24].copy_from_slice(&checksum.1.to_be_bytes());
        wal.extend_from_slice(&frame_header);
        wal.extend_from_slice(page);
    }
    wal
}

#[test]
fn sqlite_wal_summary_reports_frames_and_partial_tail() {
    let mut bytes = vec![0u8; 32 + 2 * (24 + 512) + 1];
    bytes[0..4].copy_from_slice(&0x377F_0682u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&3_007_000u32.to_be_bytes());
    bytes[8..12].copy_from_slice(&512u32.to_be_bytes());
    bytes[12..16].copy_from_slice(&7u32.to_be_bytes());
    let mut text = String::new();

    append_sqlite_wal_summary(&mut text, &bytes, bytes.len() as i64);

    assert!(text.contains("Format: SQLite write-ahead log"));
    assert!(text.contains("Page size: 512 bytes"));
    assert!(text.contains("Frames observed: 2 (trailing partial frame)"));
    assert!(text.contains("Checkpoint sequence: 7"));
}

#[test]
fn sqlite_wal_snapshot_applies_both_checksum_byte_orders() {
    for big_endian_checksum in [false, true] {
        let main = sqlite_test_page(512, 1, 1);
        let committed = sqlite_test_page(512, 42, 1);
        let wal = sqlite_test_wal(big_endian_checksum, 512, &[(1, 1, committed)]);
        let mut reader = Cursor::new(wal.clone());

        let snapshot = inspect_sqlite_wal_snapshot(&mut reader, wal.len() as u64, 512, None)
            .expect("valid WAL snapshot");
        let mut visible = main;
        apply_sqlite_wal_snapshot(&mut visible, 512, &snapshot).expect("apply committed snapshot");

        assert_eq!(read_u32_be(&visible, 60), Some(42));
        assert_eq!(snapshot.valid_frames, 1);
        assert_eq!(snapshot.last_commit_frame, 1);
        assert_eq!(snapshot.committed_pages, 1);
    }
}

#[test]
fn sqlite_wal_checksum_matches_known_header_vectors() {
    let little_endian = sqlite_test_wal(false, 512, &[]);
    assert_eq!(read_u32_be(&little_endian, 24), Some(0x1BFB_2323));
    assert_eq!(read_u32_be(&little_endian, 28), Some(0x5B45_5B18));

    let big_endian = sqlite_test_wal(true, 512, &[]);
    assert_eq!(read_u32_be(&big_endian, 24), Some(0x2624_FB1E));
    assert_eq!(read_u32_be(&big_endian, 28), Some(0x1D5E_455E));
}

#[test]
fn sqlite_wal_snapshot_ignores_uncommitted_and_bad_tail_frames() {
    let main = sqlite_test_page(512, 1, 1);
    let first_commit = sqlite_test_page(512, 11, 1);
    let uncommitted = sqlite_test_page(512, 22, 1);
    let wal = sqlite_test_wal(
        false,
        512,
        &[(1, 1, first_commit.clone()), (1, 0, uncommitted)],
    );
    let mut reader = Cursor::new(wal.clone());
    let snapshot = inspect_sqlite_wal_snapshot(&mut reader, wal.len() as u64, 512, None).unwrap();
    let mut visible = main.clone();
    apply_sqlite_wal_snapshot(&mut visible, 512, &snapshot).unwrap();
    assert_eq!(read_u32_be(&visible, 60), Some(11));
    assert_eq!(snapshot.valid_frames, 2);
    assert_eq!(snapshot.last_commit_frame, 1);

    let mut bad_tail = sqlite_test_wal(
        false,
        512,
        &[(1, 1, first_commit), (1, 0, sqlite_test_page(512, 33, 1))],
    );
    let second_checksum = 32 + (24 + 512) + 16;
    bad_tail[second_checksum] ^= 0x80;
    let mut reader = Cursor::new(bad_tail.clone());
    let snapshot = inspect_sqlite_wal_snapshot(&mut reader, bad_tail.len() as u64, 512, None)
        .expect("bad tail recovers prior commit");
    let mut visible = main;
    apply_sqlite_wal_snapshot(&mut visible, 512, &snapshot).unwrap();
    assert_eq!(read_u32_be(&visible, 60), Some(11));
    assert_eq!(snapshot.valid_frames, 1);
    assert_eq!(snapshot.stopped_frame, Some(2));
    assert_eq!(snapshot.stopped_reason, Some("checksum mismatch"));

    let mut partial_tail = sqlite_test_wal(false, 512, &[(1, 1, sqlite_test_page(512, 44, 1))]);
    partial_tail.extend_from_slice(&[1, 2, 3, 4, 5]);
    let mut reader = Cursor::new(partial_tail.clone());
    let snapshot = inspect_sqlite_wal_snapshot(&mut reader, partial_tail.len() as u64, 512, None)
        .expect("partial tail recovers prior commit");
    assert_eq!(snapshot.trailing_bytes, 5);
    assert_eq!(snapshot.last_commit_frame, 1);
}

#[test]
fn sqlite_wal_snapshot_rejects_bad_first_frame_and_page_size() {
    let page = sqlite_test_page(512, 7, 1);
    let mut bad_frame = sqlite_test_wal(false, 512, &[(1, 1, page)]);
    bad_frame[32 + 8] ^= 1;
    let mut reader = Cursor::new(bad_frame.clone());
    assert_eq!(
        inspect_sqlite_wal_snapshot(&mut reader, bad_frame.len() as u64, 512, None).err(),
        Some(ReaderPreviewError::Malformed)
    );

    let mismatched = sqlite_test_wal(false, 1024, &[(1, 1, sqlite_test_page(1024, 8, 1))]);
    let mut reader = Cursor::new(mismatched.clone());
    assert_eq!(
        inspect_sqlite_wal_snapshot(&mut reader, mismatched.len() as u64, 512, None).err(),
        Some(ReaderPreviewError::Malformed)
    );

    let mut bad_header_checksum = sqlite_test_wal(false, 512, &[]);
    bad_header_checksum[24] ^= 0x80;
    let mut reader = Cursor::new(bad_header_checksum.clone());
    assert_eq!(
        inspect_sqlite_wal_snapshot(&mut reader, bad_header_checksum.len() as u64, 512, None,)
            .err(),
        Some(ReaderPreviewError::Malformed)
    );

    let mut wrong_version = sqlite_test_wal(false, 512, &[]);
    wrong_version[4..8].copy_from_slice(&3_007_001u32.to_be_bytes());
    let checksum = sqlite_wal_checksum(&wrong_version[..24], false, (0, 0));
    wrong_version[24..28].copy_from_slice(&checksum.0.to_be_bytes());
    wrong_version[28..32].copy_from_slice(&checksum.1.to_be_bytes());
    let mut reader = Cursor::new(wrong_version.clone());
    assert_eq!(
        inspect_sqlite_wal_snapshot(&mut reader, wrong_version.len() as u64, 512, None).err(),
        Some(ReaderPreviewError::Malformed)
    );
}

#[test]
fn sqlite_wal_frame_scan_honors_cancellation() {
    let wal = sqlite_test_wal(false, 512, &[(1, 1, sqlite_test_page(512, 8, 1))]);
    WAL_CANCEL_CHECKS.store(0, std::sync::atomic::Ordering::SeqCst);
    let mut reader = Cursor::new(wal.clone());

    assert_eq!(
        inspect_sqlite_wal_snapshot(
            &mut reader,
            wal.len() as u64,
            512,
            Some(cancel_inside_wal_frame),
        )
        .err(),
        Some(ReaderPreviewError::Cancelled)
    );
    assert!(reader.position() >= 32);
    assert!(reader.position() < wal.len() as u64);
}

#[test]
fn sqlite_wal_snapshot_reuses_early_page_after_shrink_and_grow() {
    let page_two = vec![0xA5; 512];
    let shrink = sqlite_test_page(512, 2, 1);
    let grow = sqlite_test_page(512, 3, 2);
    let wal = sqlite_test_wal(true, 512, &[(2, 2, page_two), (1, 1, shrink), (1, 2, grow)]);
    let mut reader = Cursor::new(wal.clone());

    let snapshot = inspect_sqlite_wal_snapshot(&mut reader, wal.len() as u64, 512, None).unwrap();
    assert_eq!(snapshot.last_commit_frame, 3);
    assert_eq!(snapshot.committed_pages, 2);
    assert_eq!(
        snapshot.committed_prefix_pages.get(&2),
        Some(&vec![0xA5; 512])
    );

    let mut visible = sqlite_test_page(512, 1, 1);
    apply_sqlite_wal_snapshot(&mut visible, 512, &snapshot).unwrap();
    assert_eq!(visible.len(), 1024);
    assert!(visible[512..].iter().all(|byte| *byte == 0xA5));
}

#[test]
fn sqlite_wal_snapshot_rejects_page_one_header_page_size_change() {
    let mut changed_header = sqlite_test_page(512, 9, 1);
    changed_header[16..18].copy_from_slice(&1024u16.to_be_bytes());
    let wal = sqlite_test_wal(false, 512, &[(1, 1, changed_header)]);
    let mut reader = Cursor::new(wal.clone());
    let snapshot = inspect_sqlite_wal_snapshot(&mut reader, wal.len() as u64, 512, None).unwrap();
    let mut visible = sqlite_test_page(512, 1, 1);

    assert_eq!(
        apply_sqlite_wal_snapshot(&mut visible, 512, &snapshot),
        Err(ReaderPreviewError::Malformed)
    );
}

#[test]
fn database_reader_renders_committed_wal_and_bounded_shm_diagnostics() {
    let main = sqlite_test_page(512, 1, 1);
    let committed = sqlite_test_page(512, 99, 1);
    let wal = sqlite_test_wal(false, 512, &[(1, 1, committed)]);
    let mut main_reader = Cursor::new(main);
    let mut wal_reader = Cursor::new(wal.clone());
    let mut shm = vec![0u8; 48];
    shm[0..4].copy_from_slice(&3_022_000u32.to_ne_bytes());
    shm[12] = 1;
    shm[16..20].copy_from_slice(&1u32.to_ne_bytes());
    shm[20..24].copy_from_slice(&1u32.to_ne_bytes());
    let mut shm_reader = Cursor::new(shm.clone());

    let json = render_database_reader(
        &mut main_reader,
        512,
        DatabaseCompanionReader {
            reader: Some(&mut wal_reader),
            length: wal.len() as u64,
        },
        DatabaseCompanionReader {
            reader: Some(&mut shm_reader),
            length: shm.len() as u64,
        },
        r"C:\does-not-exist\renamed.db",
        0,
        None,
    )
    .expect("database HANDLE preview");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let text = value["text"].as_str().unwrap();
    assert_eq!(value["kind"], "database");
    assert!(value["title"].as_str().unwrap().starts_with("renamed.db"));
    assert!(text.contains("User version: 99"));
    assert!(text.contains("Snapshot: WAL HANDLE through commit frame 1"));
    assert!(text.contains("SHM HANDLE: diagnostic only"));
}

#[test]
fn database_reader_enforces_companion_limits_and_cancellation() {
    let mut main = Cursor::new(sqlite_test_page(512, 1, 1));
    let mut wal = Cursor::new(Vec::<u8>::new());
    assert_eq!(
        render_database_reader(
            &mut main,
            512,
            DatabaseCompanionReader {
                reader: Some(&mut wal),
                length: MAX_SQLITE_WAL_BYTES + 1,
            },
            DatabaseCompanionReader {
                reader: None,
                length: 0,
            },
            "bounded.db",
            0,
            None,
        )
        .err(),
        Some(ReaderPreviewError::LimitExceeded)
    );

    let mut main = Cursor::new(sqlite_test_page(512, 1, 1));
    assert_eq!(
        render_database_reader(
            &mut main,
            512,
            DatabaseCompanionReader {
                reader: None,
                length: 0,
            },
            DatabaseCompanionReader {
                reader: None,
                length: 0,
            },
            "cancelled.db",
            0,
            Some(always_cancel),
        )
        .err(),
        Some(ReaderPreviewError::Cancelled)
    );
}

#[test]
fn database_reader_rejects_short_or_unaligned_main_with_wal() {
    let wal = sqlite_test_wal(false, 512, &[(1, 1, sqlite_test_page(512, 2, 1))]);
    for mut main in [
        sqlite_test_page(512, 1, 1)[..100].to_vec(),
        sqlite_test_page(512, 1, 1),
    ] {
        if main.len() == 512 {
            main.push(0);
        }
        let main_length = main.len() as u64;
        let mut main_reader = Cursor::new(main);
        let mut wal_reader = Cursor::new(wal.clone());
        assert_eq!(
            render_database_reader(
                &mut main_reader,
                main_length,
                DatabaseCompanionReader {
                    reader: Some(&mut wal_reader),
                    length: wal.len() as u64,
                },
                DatabaseCompanionReader {
                    reader: None,
                    length: 0,
                },
                "malformed.db",
                0,
                None,
            )
            .err(),
            Some(ReaderPreviewError::Malformed)
        );
    }
}

#[test]
fn sqlite_schema_record_extracts_object_summary() {
    let mut payload = vec![6, 23, 23, 23, 1, 97];
    payload.extend_from_slice(b"table");
    payload.extend_from_slice(b"users");
    payload.extend_from_slice(b"users");
    payload.push(2);
    payload.extend_from_slice(b"CREATE TABLE users(id INTEGER PRIMARY KEY)");

    let row = parse_sqlite_schema_record(&payload, 1).expect("schema row");
    assert_eq!(row.typ, "table");
    assert_eq!(row.name, "users");
    assert_eq!(row.table_name, "users");
    assert_eq!(row.root_page, 2);
    assert_eq!(row.sql, "CREATE TABLE users(id INTEGER PRIMARY KEY)");
}

#[test]
fn sqlite_schema_parser_traverses_interior_pages() {
    let page_size = 512usize;
    let mut payload = vec![6, 23, 23, 23, 1, 97];
    payload.extend_from_slice(b"table");
    payload.extend_from_slice(b"users");
    payload.extend_from_slice(b"users");
    payload.push(2);
    payload.extend_from_slice(b"CREATE TABLE users(id INTEGER PRIMARY KEY)");
    let mut bytes = vec![0u8; page_size * 2];
    bytes[0..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
    bytes[100] = 0x05;
    bytes[103..105].copy_from_slice(&1u16.to_be_bytes());
    bytes[112..114].copy_from_slice(&200u16.to_be_bytes());
    bytes[200..204].copy_from_slice(&2u32.to_be_bytes());
    bytes[204] = 1;
    let leaf = page_size;
    bytes[leaf] = 0x0D;
    bytes[leaf + 3..leaf + 5].copy_from_slice(&1u16.to_be_bytes());
    bytes[leaf + 8..leaf + 10].copy_from_slice(&400u16.to_be_bytes());
    let cell = leaf + 400;
    bytes[cell] = payload.len() as u8;
    bytes[cell + 1] = 1;
    bytes[cell + 2..cell + 2 + payload.len()].copy_from_slice(&payload);

    let rows = parse_sqlite_schema_rows(&bytes, page_size, 8);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "users");
    assert_eq!(rows[0].root_page, 2);
}

#[test]
fn sqlite_schema_summary_marks_missing_pages_partial() {
    let page_size = 512usize;
    let mut bytes = vec![0u8; page_size];
    bytes[0..16].copy_from_slice(b"SQLite format 3\0");
    bytes[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
    bytes[100] = 0x05;
    bytes[103..105].copy_from_slice(&1u16.to_be_bytes());
    bytes[112..114].copy_from_slice(&200u16.to_be_bytes());
    bytes[200..204].copy_from_slice(&2u32.to_be_bytes());
    bytes[204] = 1;

    let summary = parse_sqlite_schema_summary(&bytes, page_size, 8, None);

    assert!(summary.rows.is_empty());
    assert!(summary.partial);
}

#[test]
fn sqlite_schema_leaf_marks_invalid_cells_partial() {
    let mut page = vec![0u8; 512];
    page[0] = 0x0D;
    page[3..5].copy_from_slice(&1u16.to_be_bytes());
    page[8..10].copy_from_slice(&511u16.to_be_bytes());
    let mut rows = Vec::new();

    let partial = parse_sqlite_schema_leaf_page(&page, 0, 8, 1, &mut rows);

    assert!(rows.is_empty());
    assert!(partial);
}

#[test]
fn sqlite_record_integer_decodes_wide_root_pages() {
    for (serial, bytes, expected) in [
        (3, vec![0x01, 0x02, 0x03], 0x010203),
        (5, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06], 0x010203040506),
        (6, vec![0, 0, 0, 0, 0, 0, 0x01, 0x02], 0x0102),
    ] {
        let mut pos = 0;
        assert_eq!(
            sqlite_record_integer(&bytes, &mut pos, serial),
            Some(expected)
        );
        assert_eq!(pos, bytes.len());
    }

    let mut pos = 0;
    assert_eq!(
        sqlite_record_integer(&[0xFF, 0xFF, 0xFE], &mut pos, 3),
        Some(-2)
    );
}

#[test]
fn sqlite_schema_record_decodes_utf16_text() {
    fn utf16_le(value: &str) -> Vec<u8> {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    let typ = utf16_le("table");
    let name = utf16_le("users");
    let sql = utf16_le("CREATE TABLE t(x)");
    let text_serial = |bytes: &[u8]| 13 + bytes.len() as u8 * 2;
    let mut payload = vec![
        6,
        text_serial(&typ),
        text_serial(&name),
        text_serial(&name),
        1,
        text_serial(&sql),
    ];
    payload.extend_from_slice(&typ);
    payload.extend_from_slice(&name);
    payload.extend_from_slice(&name);
    payload.push(2);
    payload.extend_from_slice(&sql);

    let row = parse_sqlite_schema_record(&payload, 2).expect("UTF-16 schema row");

    assert_eq!(row.typ, "table");
    assert_eq!(row.name, "users");
    assert_eq!(row.root_page, 2);
    assert_eq!(row.sql, "CREATE TABLE t(x)");
    assert!(decode_sqlite_utf16(&[0x41], true).is_none());
}

#[test]
fn sqlite_schema_groups_keep_indexes_from_displacing_tables() {
    let rows = vec![
        SqliteSchemaRow {
            typ: "index".to_string(),
            name: "users_name".to_string(),
            table_name: "users".to_string(),
            root_page: 3,
            sql: "CREATE INDEX users_name ON users(name)".to_string(),
        },
        SqliteSchemaRow {
            typ: "table".to_string(),
            name: "users".to_string(),
            table_name: "users".to_string(),
            root_page: 2,
            sql: "CREATE TABLE users(id INTEGER, name TEXT)".to_string(),
        },
    ];
    let mut text = String::new();

    append_sqlite_schema_group(&mut text, &[], 512, &rows, "table", "Tables", None);
    append_sqlite_schema_group(&mut text, &[], 512, &rows, "index", "Indexes", None);

    assert!(text.contains("\nTables:"));
    assert!(text.contains("\nIndexes:"));
    assert!(text.contains("- users (table: users, root: 2)"));
    assert!(text.contains("- users_name (table: users, root: 3)"));
}

#[test]
fn sqlite_table_column_parser_summarizes_columns() {
    let columns = parse_sqlite_table_columns(
        r#"CREATE TABLE "users"(
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                "display,name" VARCHAR(80),
                balance DECIMAL(10, 2) DEFAULT 0,
                CONSTRAINT users_name UNIQUE(name)
            )"#,
        8,
    );

    assert_eq!(
        columns,
        vec![
            "id INTEGER".to_string(),
            "name TEXT".to_string(),
            "display,name VARCHAR(80)".to_string(),
            "balance DECIMAL(10, 2)".to_string(),
        ]
    );
}

#[test]
fn sqlite_table_record_formats_bounded_cells() {
    let text = "x".repeat(MAX_SQLITE_SAMPLE_CELL_CHARS + 10);
    let serial = 13 + text.len() as u64 * 2;
    let mut header = vec![4, 1, 7];
    if serial < 128 {
        header.push(serial as u8);
    } else {
        header.push(((serial >> 7) as u8) | 0x80);
        header.push((serial & 0x7F) as u8);
        header[0] = 5;
    }
    let mut payload = header;
    payload.push(42);
    payload.extend_from_slice(&1.5f64.to_bits().to_be_bytes());
    payload.extend_from_slice(text.as_bytes());

    let (cells, columns) = parse_sqlite_table_record(&payload, 3, 1).expect("table record");

    assert_eq!(columns, 3);
    assert_eq!(cells[0], "42");
    assert_eq!(cells[1], "1.5");
    assert_eq!(cells[2].chars().count(), MAX_SQLITE_SAMPLE_CELL_CHARS + 3);
}

#[test]
fn sqlite_table_column_names_ignore_constraints() {
    let names = parse_sqlite_table_column_names(
        "CREATE TABLE users(id INTEGER, name TEXT, CONSTRAINT users_pk PRIMARY KEY(id))",
        32,
    );

    assert_eq!(names, vec!["id".to_string(), "name".to_string()]);
}

#[test]
fn sqlite_row_counter_counts_leaf_and_interior_pages() {
    let page_size = 512usize;
    let mut bytes = vec![0u8; page_size * 4];
    bytes[page_size] = 0x05;
    bytes[page_size + 3..page_size + 5].copy_from_slice(&1u16.to_be_bytes());
    bytes[page_size + 8..page_size + 12].copy_from_slice(&4u32.to_be_bytes());
    bytes[page_size + 12..page_size + 14].copy_from_slice(&100u16.to_be_bytes());
    bytes[page_size + 100..page_size + 104].copy_from_slice(&3u32.to_be_bytes());
    bytes[page_size * 2] = 0x0D;
    bytes[page_size * 2 + 3..page_size * 2 + 5].copy_from_slice(&2u16.to_be_bytes());
    bytes[page_size * 3] = 0x0D;
    bytes[page_size * 3 + 3..page_size * 3 + 5].copy_from_slice(&3u16.to_be_bytes());

    let count = count_sqlite_table_rows(&bytes, page_size, 2, 128, None).expect("row count");

    assert_eq!(count.rows, 5);
    assert!(!count.partial);
}

#[test]
fn sqlite_row_counter_marks_missing_pages_partial() {
    let page_size = 512usize;
    let mut bytes = vec![0u8; page_size * 2];
    bytes[page_size] = 0x05;
    bytes[page_size + 3..page_size + 5].copy_from_slice(&1u16.to_be_bytes());
    bytes[page_size + 12..page_size + 14].copy_from_slice(&100u16.to_be_bytes());
    bytes[page_size + 100..page_size + 104].copy_from_slice(&3u32.to_be_bytes());

    let count = count_sqlite_table_rows(&bytes, page_size, 2, 128, None).expect("row count");

    assert_eq!(count.rows, 0);
    assert!(count.partial);
}
