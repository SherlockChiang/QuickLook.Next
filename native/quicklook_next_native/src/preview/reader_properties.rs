//! Reproducible properties at the reader boundary, included in ordinary CI tests.
//! This is bounded mutation coverage, not a sanitizer or coverage-guided fuzzer.

use std::cell::Cell;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use proptest::prelude::*;
use proptest::test_runner::{contextualize_config, Config, RngSeed};

use super::{
    extract_archive_entry_to_writer_reader, render_archive_reader, render_database_reader,
    render_mail_reader, render_torrent_reader, DatabaseCompanionReader, ReaderPreviewError,
};

mod seeds;

fn config() -> Config {
    // Environment overrides support longer local runs and alternate seeds. Failures
    // shrink and persist beside the source via proptest's default persistence.
    contextualize_config(Config {
        cases: 128,
        rng_seed: RngSeed::Fixed(0x514C_2026_100C),
        max_shrink_iters: 1024,
        max_shrink_time: 1000,
        ..Config::default()
    })
}

struct BudgetReader<'a> {
    inner: Cursor<&'a [u8]>,
    chunk: usize,
    reads: usize,
    seeks: usize,
    bytes: usize,
    cancel_after: Option<usize>,
}

impl<'a> BudgetReader<'a> {
    fn new(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            inner: Cursor::new(bytes),
            chunk,
            reads: 0,
            seeks: 0,
            bytes: 0,
            cancel_after: None,
        }
    }

    fn untouched(&self) -> bool {
        self.reads == 0 && self.seeks == 0
    }
}

impl Read for BudgetReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel_after.is_some() {
            assert!(
                !READ_CANCELLED.with(Cell::get),
                "underlying read after cancellation"
            );
        }
        self.reads += 1;
        assert!(self.reads <= 131_072, "reader call budget exceeded");
        let length = buffer.len().min(self.chunk);
        let read = self.inner.read(&mut buffer[..length])?;
        self.bytes += read;
        assert!(self.bytes <= 2 * 1024 * 1024, "reader byte budget exceeded");
        if read > 0 && self.cancel_after == Some(self.reads) {
            READ_CANCELLED.with(|cancelled| cancelled.set(true));
        }
        Ok(read)
    }
}

impl Seek for BudgetReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.seeks += 1;
        assert!(self.seeks <= 4096, "reader seek budget exceeded");
        self.inner.seek(position)
    }
}

#[derive(Clone, Copy, Debug)]
enum Family {
    Zip,
    Torrent,
    Msg,
    Sqlite,
}

const FAMILIES: [Family; 4] = [Family::Zip, Family::Torrent, Family::Msg, Family::Sqlite];

fn render(
    family: Family,
    main: &mut BudgetReader<'_>,
    length: u64,
    wal: &mut BudgetReader<'_>,
    shm: &mut BudgetReader<'_>,
    cancel: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    match family {
        Family::Zip => render_archive_reader(main, "reader-property.zip", length, 0, cancel),
        Family::Torrent => {
            render_torrent_reader(main, "reader-property.torrent", length as i64, 0, cancel)
        }
        Family::Msg => render_mail_reader(main, "reader-property.msg", length, 0, cancel),
        Family::Sqlite => {
            let wal_length = wal.inner.get_ref().len() as u64;
            let shm_length = shm.inner.get_ref().len() as u64;
            render_database_reader(
                main,
                length,
                DatabaseCompanionReader {
                    reader: Some(wal),
                    length: wal_length,
                },
                DatabaseCompanionReader {
                    reader: Some(shm),
                    length: shm_length,
                },
                "reader-property.db",
                0,
                cancel,
            )
        }
    }
}

fn checked_json(json: &str) -> serde_json::Value {
    assert!(
        json.len() <= 512 * 1024,
        "small input produced oversized JSON"
    );
    let value: serde_json::Value = serde_json::from_str(json).expect("valid preview JSON");
    assert!(value["kind"].is_string());
    assert!(value["title"].is_string());
    value
}

fn compare_reads(family: Family, bytes: &[u8], wal: &[u8], shm: &[u8], chunk: usize) {
    let run = |chunk| {
        render(
            family,
            &mut BudgetReader::new(bytes, chunk),
            bytes.len() as u64,
            &mut BudgetReader::new(wal, chunk),
            &mut BudgetReader::new(shm, chunk),
            None,
        )
    };
    let full = run(usize::MAX);
    let fragmented = run(chunk);
    assert_eq!(full, fragmented, "fragmentation changed {family:?} outcome");
    if let Ok(json) = full {
        checked_json(&json);
    }
}

fn mutations() -> impl Strategy<Value = Vec<(usize, u8)>> {
    prop::collection::vec(
        (
            prop_oneof![3 => 0usize..512, 1 => 512usize..32768],
            any::<u8>(),
        ),
        0..12,
    )
}

fn mutate(mut bytes: Vec<u8>, changes: &[(usize, u8)], cut: Option<usize>, tail: &[u8]) -> Vec<u8> {
    for &(offset, value) in changes {
        let index = offset % bytes.len();
        bytes[index] = value;
    }
    bytes.extend_from_slice(tail);
    if let Some(cut) = cut {
        bytes.truncate(cut % (bytes.len() + 1));
    }
    bytes
}

fn run_mutations(
    family: Family,
    changes: &[(usize, u8)],
    cut: Option<usize>,
    tail: &[u8],
    chunk: usize,
) {
    let seed = match family {
        Family::Zip => seeds::zip(b"reader payload", false),
        Family::Torrent => seeds::torrent("payload.bin", 14),
        Family::Msg => seeds::msg("Reader subject"),
        Family::Sqlite => seeds::sqlite(4096, 7),
    };
    if !matches!(family, Family::Sqlite) {
        compare_reads(family, &mutate(seed, changes, cut, tail), &[], &[], chunk);
        return;
    }
    // Mutate one input at a time so broken main headers do not mask WAL parsing.
    let (bytes, mut wal) = if chunk.is_multiple_of(2) {
        (
            seed,
            mutate(
                seeds::wal(&seeds::sqlite(4096, 19), false),
                changes,
                cut,
                tail,
            ),
        )
    } else {
        (
            mutate(seed, changes, cut, tail),
            seeds::wal(&seeds::sqlite(4096, 19), false),
        )
    };
    // Some WAL cases recompute checksums to reach page/header parsing.
    if chunk.is_multiple_of(4) {
        seeds::resign_wal(&mut wal, false);
    }
    compare_reads(family, &bytes, &wal, tail, chunk);
}

extern "C" fn always_cancel() -> bool {
    true
}

thread_local! {
    static READ_CANCELLED: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn cancel_after_read() -> bool {
    READ_CANCELLED.with(Cell::get)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn zip_mutations_preserve_reader_contract(changes in mutations(), cut in prop::option::of(0usize..32768), tail in prop::collection::vec(any::<u8>(), 0..256), chunk in 1usize..65) {
        run_mutations(Family::Zip, &changes, cut, &tail, chunk);
    }

    #[test]
    fn torrent_mutations_preserve_reader_contract(changes in mutations(), cut in prop::option::of(0usize..32768), tail in prop::collection::vec(any::<u8>(), 0..256), chunk in 1usize..65) {
        run_mutations(Family::Torrent, &changes, cut, &tail, chunk);
    }

    #[test]
    fn msg_mutations_preserve_reader_contract(changes in mutations(), cut in prop::option::of(0usize..32768), tail in prop::collection::vec(any::<u8>(), 0..256), chunk in 1usize..65) {
        run_mutations(Family::Msg, &changes, cut, &tail, chunk);
    }

    #[test]
    fn sqlite_wal_mutations_preserve_reader_contract(changes in mutations(), cut in prop::option::of(0usize..32768), tail in prop::collection::vec(any::<u8>(), 0..256), chunk in 1usize..65) {
        run_mutations(Family::Sqlite, &changes, cut, &tail, chunk);
    }

    #[test]
    fn arbitrary_bytes_preserve_reader_contract(bytes in prop::collection::vec(any::<u8>(), 0..8193), chunk in 1usize..65) {
        for family in FAMILIES { compare_reads(family, &bytes, &bytes, &bytes, chunk); }
    }

    #[test]
    fn valid_zip_lists_and_extracts_exact_payload(data in prop::collection::vec(any::<u8>(), 0..4097), deflate in any::<bool>(), chunk in 1usize..65) {
        let bytes = seeds::zip(&data, deflate);
        compare_reads(Family::Zip, &bytes, &[], &[], chunk);
        let json = render_archive_reader(BudgetReader::new(&bytes, chunk), "reader-property.zip", bytes.len() as u64, 0, None).unwrap();
        let value = checked_json(&json);
        prop_assert_eq!(&value["listing"]["items"][0]["name"], "payload.bin");
        prop_assert_eq!(value["listing"]["items"][0]["size"].as_u64(), Some(data.len() as u64));
        let mut output = Vec::new();
        let extracted = extract_archive_entry_to_writer_reader(BudgetReader::new(&bytes, chunk), bytes.len() as u64, "reader-property.zip", "payload.bin", &mut output, data.len() as u64, None).unwrap();
        prop_assert_eq!(extracted, data.len() as u64);
        prop_assert_eq!(&output, &data);
        if !data.is_empty() {
            output.clear();
            let result = extract_archive_entry_to_writer_reader(BudgetReader::new(&bytes, chunk), bytes.len() as u64, "reader-property.zip", "payload.bin", &mut output, (data.len() - 1) as u64, None);
            prop_assert!(result.is_err());
            prop_assert!(output.len() < data.len());
        }
    }

    #[test]
    fn valid_torrent_preserves_name_and_length(name in "[a-zA-Z0-9_]{1,32}", length in any::<u32>(), chunk in 1usize..65) {
        let bytes = seeds::torrent(&name, length);
        let json = render_torrent_reader(&mut BudgetReader::new(&bytes, chunk), "reader-property.torrent", bytes.len() as i64, 0, None).unwrap();
        let value = checked_json(&json);
        prop_assert_eq!(value["listing"]["rootName"].as_str(), Some(name.as_str()));
        prop_assert_eq!(value["listing"]["items"][0]["size"].as_u64(), Some(u64::from(length)));
    }

    #[test]
    fn valid_msg_decodes_utf16_subject(subject in "[a-zA-Z0-9中文é]{1,32}", chunk in 1usize..65) {
        let bytes = seeds::msg(&subject);
        let json = render_mail_reader(BudgetReader::new(&bytes, chunk), "reader-property.msg", bytes.len() as u64, 0, None).unwrap();
        let expected = format!("Subject: {subject}");
        prop_assert!(checked_json(&json)["text"].as_str().unwrap().contains(&expected));
    }

    #[test]
    fn valid_wal_uses_committed_page(version in 1u32..u32::MAX, page_size in prop::sample::select(vec![512usize, 1024, 4096]), big_endian in any::<bool>(), chunk in 1usize..65) {
        let bytes = seeds::sqlite(page_size, 0);
        let wal = seeds::wal(&seeds::sqlite(page_size, version), big_endian);
        compare_reads(Family::Sqlite, &bytes, &wal, &[], chunk);
        let json = render(Family::Sqlite, &mut BudgetReader::new(&bytes, chunk), bytes.len() as u64, &mut BudgetReader::new(&wal, chunk), &mut BudgetReader::new(&[], chunk), None).unwrap();
        let expected = format!("User version: {version}");
        prop_assert!(checked_json(&json)["text"].as_str().unwrap().contains(&expected));
    }

    #[test]
    fn precancellation_does_no_io_and_lengths_fail_closed(bytes in prop::collection::vec(any::<u8>(), 0..1025), extra in 1u64..1025, chunk in 1usize..65) {
        for family in FAMILIES {
            let mut main = BudgetReader::new(&bytes, chunk);
            let mut wal = BudgetReader::new(&bytes, chunk);
            let mut shm = BudgetReader::new(&bytes, chunk);
            let result = render(family, &mut main, bytes.len() as u64, &mut wal, &mut shm, Some(always_cancel));
            prop_assert_eq!(result, Err(ReaderPreviewError::Cancelled));
            prop_assert!(main.untouched() && wal.untouched() && shm.untouched());
            let result = render(family, &mut main, bytes.len() as u64 + extra, &mut wal, &mut shm, None);
            prop_assert_eq!(result, Err(ReaderPreviewError::LengthMismatch));
        }
    }

    #[test]
    fn cancellation_during_reads_stops_io_and_allows_fresh_preview(after in 1usize..4, chunk in 1usize..9) {
        let wal_bytes = seeds::wal(&seeds::sqlite(4096, 19), false);
        for (family, bytes) in [
            (Family::Zip, seeds::zip(b"reader payload", false)),
            (Family::Torrent, seeds::torrent("payload.bin", 14)),
            (Family::Msg, seeds::msg("Reader subject")),
            (Family::Sqlite, seeds::sqlite(4096, 7)),
        ] {
            READ_CANCELLED.with(|cancelled| cancelled.set(false));
            let mut main = BudgetReader::new(&bytes, chunk);
            main.cancel_after = Some(after);
            let mut wal = BudgetReader::new(&wal_bytes, chunk);
            let mut shm = BudgetReader::new(&[], chunk);
            let result = render(family, &mut main, bytes.len() as u64, &mut wal, &mut shm, Some(cancel_after_read));
            prop_assert_eq!(result, Err(ReaderPreviewError::Cancelled));
            prop_assert_eq!(main.reads, after);
            prop_assert!(wal.untouched() && shm.untouched());
            READ_CANCELLED.with(|cancelled| cancelled.set(false));
            let json = render(family, &mut BudgetReader::new(&bytes, chunk), bytes.len() as u64, &mut BudgetReader::new(&wal_bytes, chunk), &mut BudgetReader::new(&[], chunk), None).unwrap();
            checked_json(&json);
        }
    }
}
