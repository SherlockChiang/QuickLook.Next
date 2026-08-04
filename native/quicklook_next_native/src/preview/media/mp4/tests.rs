use super::super::container_name;
use super::{
    append_metadata, collect_atom_payloads, estimate_bitrate, find_atom_payload, major_brand,
    mp4_time_to_unix, parse_chunk_summary, parse_co64_offsets, parse_ctts_summary,
    parse_elst_summary, parse_movie_created_unix, parse_movie_duration_seconds,
    parse_sample_descriptions, parse_stco_offsets, parse_stsc_entries, parse_stts_timeline,
    rotation_degrees, summarize_chunks, tracks, SampleSizes, StscEntry, TrackSummary,
    MAX_CHUNK_TABLE_ENTRIES, MAX_COLLECTED_ATOMS, MAX_SAMPLE_COUNT, MAX_TIMELINE_ENTRIES,
    MP4_TO_UNIX_SECONDS,
};

#[test]
fn atom_traversal_accepts_empty_siblings_and_rejects_excessive_depth() {
    let mut with_empty_sibling = atom(b"free", &[]);
    with_empty_sibling.extend_from_slice(&atom(b"mvhd", &[0; 20]));
    assert_eq!(
        find_atom_payload(&with_empty_sibling, b"mvhd").map(<[u8]>::len),
        Some(20)
    );

    let mut nested = atom(b"mvhd", &[0; 20]);
    for _ in 0..6 {
        nested = atom(b"moov", &nested);
    }
    assert!(find_atom_payload(&nested, b"mvhd").is_none());
}

#[test]
fn atom_traversal_rejects_malformed_extended_sizes() {
    let mut smaller_than_header = Vec::from([0, 0, 0, 1]);
    smaller_than_header.extend_from_slice(b"free");
    smaller_than_header.extend_from_slice(&15u64.to_be_bytes());
    assert!(find_atom_payload(&smaller_than_header, b"free").is_none());

    let mut beyond_input = Vec::from([0, 0, 0, 1]);
    beyond_input.extend_from_slice(b"free");
    beyond_input.extend_from_slice(&32u64.to_be_bytes());
    assert!(find_atom_payload(&beyond_input, b"free").is_none());
}

#[test]
fn atom_collection_stops_at_budget() {
    let mut bytes = Vec::new();
    for _ in 0..=MAX_COLLECTED_ATOMS {
        bytes.extend_from_slice(&atom(b"trak", &[0]));
    }
    let mut found = Vec::new();

    collect_atom_payloads(&bytes, b"trak", &mut found);

    assert_eq!(found.len(), MAX_COLLECTED_ATOMS);
}

#[test]
fn movie_header_time_and_duration_fail_closed() {
    assert_eq!(mp4_time_to_unix(MP4_TO_UNIX_SECONDS), Some(0));
    assert!(mp4_time_to_unix(u64::MAX).is_none());

    let mut mvhd = vec![0u8; 20];
    mvhd[16..20].copy_from_slice(&90u32.to_be_bytes());
    assert!(parse_movie_duration_seconds(&mvhd).is_none());
    mvhd[12..16].copy_from_slice(&1u32.to_be_bytes());
    assert_eq!(parse_movie_duration_seconds(&mvhd), Some(90.0));
}

#[test]
fn stsc_rejects_zero_duplicate_descending_and_truncated_entries() {
    for entries in [
        vec![(0, 1, 1)],
        vec![(1, 0, 1)],
        vec![(1, 1, 0)],
        vec![(2, 1, 1)],
        vec![(1, 1, 1), (1, 2, 1)],
        vec![(1, 1, 1), (3, 1, 1), (2, 1, 1)],
    ] {
        assert!(parse_stsc_entries(&stsc_payload(&entries)).is_none());
    }

    let mut truncated = stsc_payload(&[(1, 1, 1)]);
    truncated.pop();
    assert!(parse_stsc_entries(&truncated).is_none());
}

#[test]
fn large_stsc_mapping_remains_linear() {
    const ENTRY_COUNT: u32 = 65_000;
    let mut stco = vec![0u8; 8];
    stco[4..8].copy_from_slice(&ENTRY_COUNT.to_be_bytes());
    let mut stsc = vec![0u8; 8];
    stsc[4..8].copy_from_slice(&ENTRY_COUNT.to_be_bytes());
    for chunk in 1..=ENTRY_COUNT {
        stco.extend_from_slice(&(chunk - 1).to_be_bytes());
        stsc.extend_from_slice(&chunk.to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
    }
    let mut stsz = vec![0u8; 12];
    stsz[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsz[8..12].copy_from_slice(&ENTRY_COUNT.to_be_bytes());
    let trak = [atom(b"stco", &stco), atom(b"stsc", &stsc)].concat();

    let summary =
        parse_chunk_summary(&trak, SampleSizes::parse(&stsz)).expect("large linear chunk mapping");

    assert_eq!(summary.chunks, ENTRY_COUNT);
    assert_eq!(summary.data_bytes, u64::from(ENTRY_COUNT));
    assert_eq!(summary.first_offset, 0);
    assert_eq!(summary.last_end, u64::from(ENTRY_COUNT));
    assert_eq!(summary.first_chunk_samples, Some(1));
    assert_eq!(summary.first_sample_size, Some(1));
}

#[test]
fn fixed_stsz_is_compact_and_rejects_over_budget_counts() {
    let mut fixed = vec![0u8; 12];
    fixed[4..8].copy_from_slice(&7u32.to_be_bytes());
    fixed[8..12].copy_from_slice(&(MAX_SAMPLE_COUNT as u32).to_be_bytes());
    assert!(matches!(
        SampleSizes::parse(&fixed),
        Some(SampleSizes::Fixed {
            size: 7,
            count: MAX_SAMPLE_COUNT
        })
    ));

    fixed[8..12].copy_from_slice(&((MAX_SAMPLE_COUNT as u32) + 1).to_be_bytes());
    assert!(SampleSizes::parse(&fixed).is_none());

    let mut truncated_variable = vec![0u8; 15];
    truncated_variable[8..12].copy_from_slice(&1u32.to_be_bytes());
    assert!(SampleSizes::parse(&truncated_variable).is_none());
}

#[test]
fn table_parsers_reject_truncated_and_over_budget_counts() {
    assert!(parse_stts_timeline(&declared_table(0, 1, 15)).is_none());
    assert!(parse_ctts_summary(&declared_table(1, 1, 15)).is_none());
    assert!(parse_elst_summary(&declared_table(0, 1, 19)).is_none());
    assert!(parse_elst_summary(&declared_table(1, 1, 27)).is_none());
    assert!(parse_stco_offsets(&declared_table(0, 1, 11)).is_none());
    assert!(parse_co64_offsets(&declared_table(0, 1, 15)).is_none());

    let over_budget = declared_table(0, (MAX_TIMELINE_ENTRIES as u32) + 1, 8);
    assert!(parse_stts_timeline(&over_budget).is_none());

    let over_chunk_budget = declared_table(0, (MAX_CHUNK_TABLE_ENTRIES as u32) + 1, 8);
    assert!(parse_stco_offsets(&over_chunk_budget).is_none());
    assert!(parse_co64_offsets(&over_chunk_budget).is_none());

    let only_one_of_two_edits = declared_table(0, 2, 20);
    assert!(parse_elst_summary(&only_one_of_two_edits).is_none());
}

#[test]
fn timeline_tables_reject_versions_and_tick_overflow() {
    let invalid_version = declared_table(2, 0, 8);
    assert!(parse_stts_timeline(&invalid_version).is_none());
    assert!(parse_ctts_summary(&invalid_version).is_none());
    assert!(parse_elst_summary(&invalid_version).is_none());

    let mut overflow = declared_table(0, 2, 24);
    for offset in [8usize, 16] {
        overflow[offset..offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        overflow[offset + 4..offset + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    }
    assert!(parse_stts_timeline(&overflow).is_none());

    let mut signed_ctts = declared_table(1, 1, 16);
    signed_ctts[8..12].copy_from_slice(&1u32.to_be_bytes());
    signed_ctts[12..16].copy_from_slice(&(-40i32).to_be_bytes());
    let composition = parse_ctts_summary(&signed_ctts).expect("signed ctts");
    assert_eq!(composition.first_offset, Some(-40));
    assert_eq!(composition.offset_range, Some((-40, -40)));

    let mut fractional_rate = declared_table(0, 1, 20);
    fractional_rate[16..18].copy_from_slice(&1i16.to_be_bytes());
    fractional_rate[18..20].copy_from_slice(&(-32_768i16).to_be_bytes());
    assert_eq!(
        parse_elst_summary(&fractional_rate).and_then(|summary| summary.first_rate),
        Some(0.5)
    );
}

#[test]
fn chunk_summary_rejects_offset_overflow_and_sample_mismatch() {
    let one_sample = SampleSizes::Fixed { size: 1, count: 1 };
    let one_per_chunk = [StscEntry {
        first_chunk: 1,
        samples_per_chunk: 1,
    }];
    assert!(summarize_chunks(&[u64::MAX], one_sample, &one_per_chunk).is_none());

    let two_per_chunk = [StscEntry {
        first_chunk: 1,
        samples_per_chunk: 2,
    }];
    assert!(summarize_chunks(&[0], one_sample, &two_per_chunk).is_none());
    assert!(summarize_chunks(
        &[0],
        SampleSizes::Fixed { size: 1, count: 2 },
        &one_per_chunk
    )
    .is_none());

    let malformed_co64 = declared_table(0, 1, 15);
    let mut stco = declared_table(0, 1, 12);
    stco[8..12].copy_from_slice(&0u32.to_be_bytes());
    let trak = [
        atom(b"co64", &malformed_co64),
        atom(b"stco", &stco),
        atom(b"stsc", &stsc_payload(&[(1, 1, 1)])),
    ]
    .concat();
    assert!(parse_chunk_summary(&trak, Some(one_sample)).is_none());
}

#[test]
fn sample_descriptions_reject_zero_and_truncated_entries() {
    let mut summary = TrackSummary {
        kind: "Video",
        ..Default::default()
    };
    let mut zero_size = declared_table(0, 1, 12);
    zero_size[8..12].copy_from_slice(&0u32.to_be_bytes());
    assert!(parse_sample_descriptions(&zero_size, &mut summary).is_none());

    let mut truncated = declared_table(0, 1, 12);
    truncated[8..12].copy_from_slice(&36u32.to_be_bytes());
    assert!(parse_sample_descriptions(&truncated, &mut summary).is_none());
}

#[test]
fn media_info_reads_mp4_tracks_and_stable_output() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&16u32.to_be_bytes());
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(b"isom");
    bytes.extend_from_slice(&0u32.to_be_bytes());

    let mut mvhd_payload = vec![0u8; 20];
    mvhd_payload[4..8].copy_from_slice(&2_082_844_800u32.to_be_bytes());
    mvhd_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd_payload[16..20].copy_from_slice(&90_000u32.to_be_bytes());
    let mvhd = atom(b"mvhd", &mvhd_payload);

    let mut tkhd_payload = vec![0u8; 84];
    tkhd_payload[44..48].copy_from_slice(&65_536i32.to_be_bytes());
    tkhd_payload[64..68].copy_from_slice(&0x4000_0000i32.to_be_bytes());
    tkhd_payload[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
    tkhd_payload[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
    let tkhd = atom(b"tkhd", &tkhd_payload);

    let mut mdhd_payload = vec![0u8; 24];
    mdhd_payload[12..16].copy_from_slice(&30_000u32.to_be_bytes());
    mdhd_payload[16..20].copy_from_slice(&2_700_000u32.to_be_bytes());
    mdhd_payload[20..22].copy_from_slice(&0x15C7u16.to_be_bytes());
    let mdhd = atom(b"mdhd", &mdhd_payload);

    let mut hdlr_payload = vec![0u8; 12];
    hdlr_payload[8..12].copy_from_slice(b"vide");
    let hdlr = atom(b"hdlr", &hdlr_payload);

    let avcc = atom(b"avcC", &[1, 0x64, 0, 31, 0xFF, 0, 0, 0xFE, 0xFE, 0xFE]);
    let entry_size = 86 + avcc.len();
    let mut stsd_payload = vec![0u8; 8 + entry_size];
    stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
    stsd_payload[8..12].copy_from_slice(&(entry_size as u32).to_be_bytes());
    stsd_payload[12..16].copy_from_slice(b"avc1");
    stsd_payload[40..42].copy_from_slice(&1920u16.to_be_bytes());
    stsd_payload[42..44].copy_from_slice(&1080u16.to_be_bytes());
    stsd_payload[94..94 + avcc.len()].copy_from_slice(&avcc);
    let stsd = atom(b"stsd", &stsd_payload);

    let mut stsz_payload = vec![0u8; 20];
    stsz_payload[8..12].copy_from_slice(&2u32.to_be_bytes());
    stsz_payload[12..16].copy_from_slice(&600_000u32.to_be_bytes());
    stsz_payload[16..20].copy_from_slice(&700_000u32.to_be_bytes());
    let stsz = atom(b"stsz", &stsz_payload);

    let stsc = atom(b"stsc", &stsc_payload(&[(1, 1, 1)]));
    let mut stco_payload = declared_table(0, 2, 16);
    stco_payload[8..12].copy_from_slice(&1000u32.to_be_bytes());
    stco_payload[12..16].copy_from_slice(&2000u32.to_be_bytes());
    let stco = atom(b"stco", &stco_payload);

    let mut stts_payload = declared_table(0, 1, 16);
    stts_payload[8..12].copy_from_slice(&90u32.to_be_bytes());
    stts_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
    let stts = atom(b"stts", &stts_payload);
    let mut ctts_payload = declared_table(0, 1, 16);
    ctts_payload[8..12].copy_from_slice(&90u32.to_be_bytes());
    ctts_payload[12..16].copy_from_slice(&40u32.to_be_bytes());
    let ctts = atom(b"ctts", &ctts_payload);
    let mut elst_payload = declared_table(0, 1, 20);
    elst_payload[8..12].copy_from_slice(&90_000u32.to_be_bytes());
    elst_payload[12..16].copy_from_slice(&0i32.to_be_bytes());
    elst_payload[16..18].copy_from_slice(&1i16.to_be_bytes());
    let edts = atom(b"edts", &atom(b"elst", &elst_payload));

    let stbl = atom(b"stbl", &[stsd, stsz, stsc, stco, stts, ctts].concat());
    let minf = atom(b"minf", &stbl);
    let mdia = atom(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = atom(b"trak", &[tkhd, edts, mdia].concat());
    bytes.extend_from_slice(&atom(b"moov", &[mvhd, trak].concat()));

    assert_eq!(container_name("clip.mp4", &bytes), "ISO BMFF / MP4");
    assert_eq!(major_brand(&bytes).as_deref(), Some("isom"));
    let movie_header = find_atom_payload(&bytes, b"mvhd").expect("movie header");
    assert_eq!(parse_movie_duration_seconds(movie_header), Some(90.0));
    assert_eq!(parse_movie_created_unix(movie_header), Some(0));
    assert_eq!(rotation_degrees(&bytes), Some(90));

    let summaries = tracks(&bytes);
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.kind, "Video");
    assert_eq!(summary.codec, "avc1");
    assert_eq!(
        summary.codec_detail,
        "AVC profile 0x64, compat 0x00, level 3.1, 4-byte NAL length, chroma 2, 14-bit luma, 14-bit chroma"
    );
    assert_eq!(summary.language, "eng");
    assert_eq!(summary.width, Some(1920));
    assert_eq!(summary.height, Some(1080));
    assert_eq!(summary.duration_seconds, Some(90.0));
    assert_eq!(summary.data_bytes, Some(1_300_000));
    assert_eq!(summary.timing_entries, Some(1));
    assert_eq!(summary.samples, Some(90));
    assert_eq!(summary.decode_ticks, Some(90_000));
    assert_eq!(summary.first_sample_delta, Some(1000));
    assert_eq!(summary.composition_entries, Some(1));
    assert_eq!(summary.composition_samples, Some(90));
    assert_eq!(summary.first_composition_offset, Some(40));
    assert_eq!(summary.composition_offset_range, Some((40, 40)));
    assert_eq!(summary.edit_entries, Some(1));
    assert_eq!(summary.first_edit_duration, Some(90_000));
    assert_eq!(summary.first_edit_media_time, Some(0));
    assert_eq!(summary.first_edit_rate, Some(1.0));
    assert_eq!(summary.chunks, Some(2));
    assert_eq!(summary.first_chunk_offset, Some(1000));
    assert_eq!(summary.last_chunk_end, Some(702_000));
    assert_eq!(summary.first_chunk_samples, Some(1));
    assert_eq!(summary.first_chunk_bytes, Some(600_000));
    assert_eq!(summary.first_sample_size, Some(600_000));
    assert_eq!(
        summary.chunk_details,
        vec![
            "#1 @0x3E8 1 samples 600000 bytes".to_string(),
            "#2 @0x7D0 1 samples 700000 bytes".to_string()
        ]
    );
    assert_eq!(estimate_bitrate(17_280_000, 90.0), Some(1_536_000.0));

    let mut text = String::new();
    append_metadata(&mut text, &bytes, 17_280_000);
    assert_eq!(
        text,
        concat!(
            "\nBrand: isom",
            "\nDuration: 1:30",
            "\nBitrate: 1.54 Mbps",
            "\nCreated: —",
            "\nRotation: 90°",
            "\nVideo track 1: avc1 (AVC profile 0x64, compat 0x00, level 3.1, 4-byte NAL length, chroma 2, 14-bit luma, 14-bit chroma)",
            "\nVideo language: eng",
            "\nVideo size: 1920x1080",
            "\nVideo duration: 1:30",
            "\nVideo bitrate: 116 kbps",
            "\nVideo timing entries: 1",
            "\nVideo samples: 90",
            "\nVideo decode ticks: 90,000",
            "\nVideo first sample delta: 1000",
            "\nVideo composition offsets: 1",
            "\nVideo composition samples: 90",
            "\nVideo first composition offset: 40",
            "\nVideo composition offset range: 40..40",
            "\nVideo edit list entries: 1",
            "\nVideo first edit duration: 90,000",
            "\nVideo first edit media time: 0",
            "\nVideo first edit rate: 1.00",
            "\nVideo chunks: 2 (0x3E8-0xAB630)",
            "\nVideo first chunk samples: 1",
            "\nVideo first chunk bytes: 600,000",
            "\nVideo first sample size: 600,000",
            "\nVideo chunk map: #1 @0x3E8 1 samples 600000 bytes, #2 @0x7D0 1 samples 700000 bytes"
        )
    );
}

fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = 8usize.checked_add(payload.len()).expect("test atom size");
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&(size as u32).to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(payload);
    bytes
}

fn stsc_payload(entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let mut payload = vec![0u8; 8];
    payload[4..8].copy_from_slice(&(entries.len() as u32).to_be_bytes());
    for (first_chunk, samples_per_chunk, description_index) in entries {
        payload.extend_from_slice(&first_chunk.to_be_bytes());
        payload.extend_from_slice(&samples_per_chunk.to_be_bytes());
        payload.extend_from_slice(&description_index.to_be_bytes());
    }
    payload
}

fn declared_table(version: u8, entries: u32, length: usize) -> Vec<u8> {
    let mut payload = vec![0u8; length];
    payload[0] = version;
    payload[4..8].copy_from_slice(&entries.to_be_bytes());
    payload
}
