use super::super::{
    common::{read_i16_be, read_i32_be, read_i64_be, read_u32_be, read_u64_be},
    Mp4TrackSummary,
};

const MAX_ATOM_DEPTH: usize = 4;
const MAX_COLLECTED_ATOMS: usize = 1024;
const MAX_TIMELINE_ENTRIES: usize = 100_000;
const MAX_CHUNK_TABLE_ENTRIES: usize = 1_000_000;
const MAX_SAMPLE_COUNT: usize = 1_000_000;
const MAX_CHUNK_DETAILS: usize = 4;
const MP4_TO_UNIX_SECONDS: u64 = 2_082_844_800;

struct Atom<'a> {
    kind: &'a [u8],
    payload_start: usize,
    end: usize,
}

pub(super) fn find_atom_payload<'a>(bytes: &'a [u8], atom: &[u8; 4]) -> Option<&'a [u8]> {
    find_atom_payload_in_range(bytes, 0, bytes.len(), atom, 0)
}

pub(super) fn collect_atom_payloads<'a>(
    bytes: &'a [u8],
    atom: &[u8; 4],
    found: &mut Vec<&'a [u8]>,
) {
    collect_atom_payloads_in_range(bytes, 0, bytes.len(), atom, 0, found);
}

fn collect_atom_payloads_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
    found: &mut Vec<&'a [u8]>,
) {
    if depth > MAX_ATOM_DEPTH
        || start >= end
        || end > bytes.len()
        || found.len() >= MAX_COLLECTED_ATOMS
    {
        return;
    }
    let mut position = start;
    while position < end && found.len() < MAX_COLLECTED_ATOMS {
        let Some(current) = read_atom(bytes, position, end) else {
            break;
        };
        if current.kind == atom {
            if let Some(payload) = bytes.get(current.payload_start..current.end) {
                found.push(payload);
            }
        }
        if found.len() >= MAX_COLLECTED_ATOMS {
            return;
        }
        if is_container_atom(current.kind) {
            collect_atom_payloads_in_range(
                bytes,
                current.payload_start,
                current.end,
                atom,
                depth + 1,
                found,
            );
        }
        position = current.end;
    }
}

pub(super) fn find_atom_payload_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
) -> Option<&'a [u8]> {
    if depth > MAX_ATOM_DEPTH || start >= end || end > bytes.len() {
        return None;
    }
    let mut position = start;
    while position < end {
        let current = read_atom(bytes, position, end)?;
        if current.kind == atom {
            return bytes.get(current.payload_start..current.end);
        }
        if is_container_atom(current.kind) {
            if let Some(found) = find_atom_payload_in_range(
                bytes,
                current.payload_start,
                current.end,
                atom,
                depth + 1,
            ) {
                return Some(found);
            }
        }
        position = current.end;
    }
    None
}

fn read_atom(bytes: &[u8], position: usize, logical_end: usize) -> Option<Atom<'_>> {
    let minimum_end = position.checked_add(8)?;
    if minimum_end > logical_end || logical_end > bytes.len() {
        return None;
    }
    let size32 = read_u32_be(bytes, position)? as u64;
    let kind = bytes.get(position.checked_add(4)?..minimum_end)?;
    let (header_size, atom_end) = if size32 == 1 {
        let header_end = position.checked_add(16)?;
        if header_end > logical_end {
            return None;
        }
        let size64 = read_u64_be(bytes, minimum_end)?;
        let size = usize::try_from(size64).ok()?;
        (16usize, position.checked_add(size)?)
    } else if size32 == 0 {
        (8usize, logical_end)
    } else {
        let size = usize::try_from(size32).ok()?;
        (8usize, position.checked_add(size)?)
    };
    let payload_start = position.checked_add(header_size)?;
    if atom_end > logical_end || atom_end < payload_start {
        return None;
    }
    Some(Atom {
        kind,
        payload_start,
        end: atom_end,
    })
}

fn is_container_atom(kind: &[u8]) -> bool {
    matches!(
        kind,
        b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts"
    )
}

pub(super) fn parse_movie_duration_seconds(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;
    match version {
        0 => {
            let timescale = read_u32_be(payload, 12)?;
            let duration = read_u32_be(payload, 16)? as u64;
            duration_from_timescale(duration, timescale)
        }
        1 => {
            let timescale = read_u32_be(payload, 20)?;
            let duration = read_u64_be(payload, 24)?;
            duration_from_timescale(duration, timescale)
        }
        _ => None,
    }
}

pub(super) fn parse_movie_created_unix(payload: &[u8]) -> Option<i64> {
    let version = *payload.first()?;
    let mac_time = match version {
        0 => read_u32_be(payload, 4)? as u64,
        1 => read_u64_be(payload, 4)?,
        _ => return None,
    };
    mp4_time_to_unix(mac_time)
}

fn mp4_time_to_unix(mac_time: u64) -> Option<i64> {
    let unix_time = mac_time.checked_sub(MP4_TO_UNIX_SECONDS)?;
    i64::try_from(unix_time).ok()
}

pub(super) fn rotation_degrees(bytes: &[u8]) -> Option<i32> {
    let mut rotations = Vec::new();
    collect_atom_payloads(bytes, b"tkhd", &mut rotations);
    rotations
        .into_iter()
        .filter_map(parse_track_rotation_degrees)
        .find(|degrees| *degrees != 0)
}

fn parse_track_rotation_degrees(payload: &[u8]) -> Option<i32> {
    let version = *payload.first()?;
    let matrix_offset = match version {
        0 => 40,
        1 => 52,
        _ => return None,
    };
    let a = read_i32_be(payload, matrix_offset)? as f64 / 65_536.0;
    let b = read_i32_be(payload, matrix_offset.checked_add(4)?)? as f64 / 65_536.0;
    let degrees = b.atan2(a).to_degrees().round() as i32;
    Some(degrees.rem_euclid(360))
}

pub(super) fn duration_from_timescale(duration: u64, timescale: u32) -> Option<f64> {
    (timescale > 0).then(|| duration as f64 / timescale as f64)
}

pub(super) fn apply_track_tables(trak: &[u8], summary: &mut Mp4TrackSummary) {
    let sample_sizes = find_atom_payload(trak, b"stsz").and_then(SampleSizes::parse);
    if let Some(sizes) = sample_sizes {
        summary.data_bytes = sizes.total_bytes();
    }

    if let Some(timeline) = find_atom_payload(trak, b"stts").and_then(parse_stts_timeline) {
        summary.timing_entries = Some(timeline.entries);
        summary.samples = Some(timeline.samples);
        summary.decode_ticks = Some(timeline.decode_ticks);
        summary.first_sample_delta = timeline.first_delta;
    }
    if let Some(composition) = find_atom_payload(trak, b"ctts").and_then(parse_ctts_summary) {
        summary.composition_entries = Some(composition.entries);
        summary.composition_samples = Some(composition.samples);
        summary.first_composition_offset = composition.first_offset;
        summary.composition_offset_range = composition.offset_range;
    }
    if let Some(edit) = find_atom_payload(trak, b"elst").and_then(parse_elst_summary) {
        summary.edit_entries = Some(edit.entries);
        summary.first_edit_duration = edit.first_duration;
        summary.first_edit_media_time = edit.first_media_time;
        summary.first_edit_rate = edit.first_rate;
    }
    if let Some(chunks) = parse_chunk_summary(trak, sample_sizes) {
        summary.chunks = Some(chunks.chunks);
        summary.first_chunk_offset = Some(chunks.first_offset);
        summary.last_chunk_end = Some(chunks.last_end);
        summary.data_bytes = Some(chunks.data_bytes);
        summary.first_chunk_samples = chunks.first_chunk_samples;
        summary.first_chunk_bytes = chunks.first_chunk_bytes;
        summary.first_sample_size = chunks.first_sample_size;
        summary.chunk_details = chunks.chunk_details;
    }
}

struct SttsTimeline {
    entries: u32,
    samples: u64,
    decode_ticks: u64,
    first_delta: Option<u32>,
}

struct CttsSummary {
    entries: u32,
    samples: u64,
    first_offset: Option<i64>,
    offset_range: Option<(i64, i64)>,
}

struct ElstSummary {
    entries: u32,
    first_duration: Option<u64>,
    first_media_time: Option<i64>,
    first_rate: Option<f64>,
}

struct ChunkSummary {
    chunks: u32,
    first_offset: u64,
    last_end: u64,
    data_bytes: u64,
    first_chunk_samples: Option<u32>,
    first_chunk_bytes: Option<u64>,
    first_sample_size: Option<u32>,
    chunk_details: Vec<String>,
}

#[derive(Clone, Copy)]
enum SampleSizes<'a> {
    Fixed { size: u32, count: usize },
    Variable { bytes: &'a [u8], count: usize },
}

impl<'a> SampleSizes<'a> {
    fn parse(payload: &'a [u8]) -> Option<Self> {
        if *payload.first()? != 0 {
            return None;
        }
        let size = read_u32_be(payload, 4)?;
        let count = usize::try_from(read_u32_be(payload, 8)?).ok()?;
        if count > MAX_SAMPLE_COUNT {
            return None;
        }
        if size > 0 {
            return Some(Self::Fixed { size, count });
        }
        let end = checked_table_end(12, count, 4, payload.len())?;
        Some(Self::Variable {
            bytes: payload.get(12..end)?,
            count,
        })
    }

    fn len(self) -> usize {
        match self {
            Self::Fixed { count, .. } | Self::Variable { count, .. } => count,
        }
    }

    fn is_empty(self) -> bool {
        self.len() == 0
    }

    fn first(self) -> Option<u32> {
        self.get(0)
    }

    fn get(self, index: usize) -> Option<u32> {
        match self {
            Self::Fixed { size, count } => (index < count).then_some(size),
            Self::Variable { bytes, count } => {
                if index >= count {
                    return None;
                }
                read_u32_be(bytes, index.checked_mul(4)?)
            }
        }
    }

    fn total_bytes(self) -> Option<u64> {
        self.sum_range(0, self.len())
    }

    fn sum_range(self, start: usize, count: usize) -> Option<u64> {
        let end = start.checked_add(count)?;
        if end > self.len() {
            return None;
        }
        match self {
            Self::Fixed { size, .. } => u64::from(size).checked_mul(u64::try_from(count).ok()?),
            Self::Variable { .. } => {
                let mut total = 0u64;
                for index in start..end {
                    total = total.checked_add(u64::from(self.get(index)?))?;
                }
                Some(total)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct StscEntry {
    first_chunk: u32,
    samples_per_chunk: u32,
}

fn parse_stts_timeline(payload: &[u8]) -> Option<SttsTimeline> {
    if *payload.first()? != 0 {
        return None;
    }
    let (entries, count) = validated_entry_count(payload, 8, 8, MAX_TIMELINE_ENTRIES)?;
    let mut offset = 8usize;
    let mut samples = 0u64;
    let mut decode_ticks = 0u64;
    let mut first_delta = None;
    for _ in 0..count {
        let sample_count = u64::from(read_u32_be(payload, offset)?);
        let sample_delta = read_u32_be(payload, offset.checked_add(4)?)?;
        if first_delta.is_none() {
            first_delta = Some(sample_delta);
        }
        samples = samples.checked_add(sample_count)?;
        decode_ticks =
            decode_ticks.checked_add(sample_count.checked_mul(u64::from(sample_delta))?)?;
        offset = offset.checked_add(8)?;
    }
    Some(SttsTimeline {
        entries,
        samples,
        decode_ticks,
        first_delta,
    })
}

fn parse_ctts_summary(payload: &[u8]) -> Option<CttsSummary> {
    let version = *payload.first()?;
    if !matches!(version, 0 | 1) {
        return None;
    }
    let (entries, count) = validated_entry_count(payload, 8, 8, MAX_TIMELINE_ENTRIES)?;
    let mut offset = 8usize;
    let mut samples = 0u64;
    let mut first_offset = None;
    let mut min_offset = i64::MAX;
    let mut max_offset = i64::MIN;
    for _ in 0..count {
        let sample_count = u64::from(read_u32_be(payload, offset)?);
        let composition_offset = if version == 1 {
            i64::from(read_i32_be(payload, offset.checked_add(4)?)?)
        } else {
            i64::from(read_u32_be(payload, offset.checked_add(4)?)?)
        };
        if first_offset.is_none() {
            first_offset = Some(composition_offset);
        }
        min_offset = min_offset.min(composition_offset);
        max_offset = max_offset.max(composition_offset);
        samples = samples.checked_add(sample_count)?;
        offset = offset.checked_add(8)?;
    }
    Some(CttsSummary {
        entries,
        samples,
        first_offset,
        offset_range: first_offset.map(|_| (min_offset, max_offset)),
    })
}

fn parse_elst_summary(payload: &[u8]) -> Option<ElstSummary> {
    let version = *payload.first()?;
    let stride = match version {
        0 => 12,
        1 => 20,
        _ => return None,
    };
    let (entries, count) = validated_entry_count(payload, 8, stride, MAX_TIMELINE_ENTRIES)?;
    if count == 0 {
        return Some(ElstSummary {
            entries,
            first_duration: None,
            first_media_time: None,
            first_rate: None,
        });
    }
    let offset = 8usize;
    let (duration, media_time, rate_offset) = if version == 1 {
        (
            read_u64_be(payload, offset)?,
            read_i64_be(payload, offset.checked_add(8)?)?,
            offset.checked_add(16)?,
        )
    } else {
        (
            u64::from(read_u32_be(payload, offset)?),
            i64::from(read_i32_be(payload, offset.checked_add(4)?)?),
            offset.checked_add(8)?,
        )
    };
    let rate_integer = f64::from(read_i16_be(payload, rate_offset)?);
    let rate_fraction = f64::from(read_i16_be(payload, rate_offset.checked_add(2)?)?) / 65_536.0;
    Some(ElstSummary {
        entries,
        first_duration: Some(duration),
        first_media_time: Some(media_time),
        first_rate: Some(rate_integer + rate_fraction),
    })
}

fn parse_chunk_summary(trak: &[u8], sample_sizes: Option<SampleSizes<'_>>) -> Option<ChunkSummary> {
    let chunk_offsets = if let Some(co64) = find_atom_payload(trak, b"co64") {
        parse_co64_offsets(co64)?
    } else {
        find_atom_payload(trak, b"stco").and_then(parse_stco_offsets)?
    };
    let sample_to_chunks = find_atom_payload(trak, b"stsc").and_then(parse_stsc_entries)?;
    summarize_chunks(&chunk_offsets, sample_sizes?, &sample_to_chunks)
}

fn summarize_chunks(
    chunk_offsets: &[u64],
    sample_sizes: SampleSizes<'_>,
    sample_to_chunks: &[StscEntry],
) -> Option<ChunkSummary> {
    if chunk_offsets.is_empty() || sample_sizes.is_empty() || sample_to_chunks.is_empty() {
        return None;
    }

    let mut sample_index = 0usize;
    let mut stsc_index = 0usize;
    let mut data_bytes = 0u64;
    let mut last_end = 0u64;
    let mut first_chunk_samples = None;
    let mut first_chunk_bytes = None;
    let mut chunk_details = Vec::new();
    for (chunk_index, chunk_offset) in chunk_offsets.iter().copied().enumerate() {
        let chunk_number = u32::try_from(chunk_index.checked_add(1)?).ok()?;
        while let Some(next) = sample_to_chunks.get(stsc_index.checked_add(1)?) {
            if chunk_number < next.first_chunk {
                break;
            }
            stsc_index = stsc_index.checked_add(1)?;
        }
        let samples_per_chunk =
            usize::try_from(sample_to_chunks.get(stsc_index)?.samples_per_chunk).ok()?;
        let chunk_bytes = sample_sizes.sum_range(sample_index, samples_per_chunk)?;
        sample_index = sample_index.checked_add(samples_per_chunk)?;
        if chunk_index == 0 {
            first_chunk_samples = Some(u32::try_from(samples_per_chunk).ok()?);
            first_chunk_bytes = Some(chunk_bytes);
        }
        if chunk_details.len() < MAX_CHUNK_DETAILS {
            chunk_details.push(format!(
                "#{} @0x{:X} {} samples {} bytes",
                chunk_number, chunk_offset, samples_per_chunk, chunk_bytes
            ));
        }
        data_bytes = data_bytes.checked_add(chunk_bytes)?;
        last_end = last_end.max(chunk_offset.checked_add(chunk_bytes)?);
    }
    if sample_index != sample_sizes.len() || stsc_index.checked_add(1)? != sample_to_chunks.len() {
        return None;
    }

    Some(ChunkSummary {
        chunks: u32::try_from(chunk_offsets.len()).ok()?,
        first_offset: *chunk_offsets.first()?,
        last_end,
        data_bytes,
        first_chunk_samples,
        first_chunk_bytes,
        first_sample_size: sample_sizes.first(),
        chunk_details,
    })
}

fn parse_stco_offsets(payload: &[u8]) -> Option<Vec<u64>> {
    if *payload.first()? != 0 {
        return None;
    }
    let (_, count) = validated_entry_count(payload, 8, 4, MAX_CHUNK_TABLE_ENTRIES)?;
    let mut offsets = Vec::new();
    offsets.try_reserve_exact(count).ok()?;
    let mut offset = 8usize;
    for _ in 0..count {
        offsets.push(u64::from(read_u32_be(payload, offset)?));
        offset = offset.checked_add(4)?;
    }
    Some(offsets)
}

fn parse_co64_offsets(payload: &[u8]) -> Option<Vec<u64>> {
    if *payload.first()? != 0 {
        return None;
    }
    let (_, count) = validated_entry_count(payload, 8, 8, MAX_CHUNK_TABLE_ENTRIES)?;
    let mut offsets = Vec::new();
    offsets.try_reserve_exact(count).ok()?;
    let mut offset = 8usize;
    for _ in 0..count {
        offsets.push(read_u64_be(payload, offset)?);
        offset = offset.checked_add(8)?;
    }
    Some(offsets)
}

fn parse_stsc_entries(payload: &[u8]) -> Option<Vec<StscEntry>> {
    if *payload.first()? != 0 {
        return None;
    }
    let (_, count) = validated_entry_count(payload, 8, 12, MAX_CHUNK_TABLE_ENTRIES)?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).ok()?;
    let mut offset = 8usize;
    let mut previous_first_chunk = 0u32;
    for _ in 0..count {
        let first_chunk = read_u32_be(payload, offset)?;
        let samples_per_chunk = read_u32_be(payload, offset.checked_add(4)?)?;
        let sample_description_index = read_u32_be(payload, offset.checked_add(8)?)?;
        if first_chunk == 0
            || first_chunk <= previous_first_chunk
            || samples_per_chunk == 0
            || sample_description_index == 0
        {
            return None;
        }
        entries.push(StscEntry {
            first_chunk,
            samples_per_chunk,
        });
        previous_first_chunk = first_chunk;
        offset = offset.checked_add(12)?;
    }
    if entries.first().is_some_and(|entry| entry.first_chunk != 1) {
        return None;
    }
    Some(entries)
}

fn validated_entry_count(
    payload: &[u8],
    header_size: usize,
    stride: usize,
    maximum: usize,
) -> Option<(u32, usize)> {
    let entries = read_u32_be(payload, 4)?;
    let count = usize::try_from(entries).ok()?;
    if count > maximum {
        return None;
    }
    checked_table_end(header_size, count, stride, payload.len())?;
    Some((entries, count))
}

fn checked_table_end(
    header_size: usize,
    count: usize,
    stride: usize,
    payload_len: usize,
) -> Option<usize> {
    let end = header_size.checked_add(count.checked_mul(stride)?)?;
    (end <= payload_len).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_atom_payloads, find_atom_payload, mp4_time_to_unix, parse_chunk_summary,
        parse_co64_offsets, parse_ctts_summary, parse_elst_summary, parse_movie_duration_seconds,
        parse_stco_offsets, parse_stsc_entries, parse_stts_timeline, summarize_chunks, SampleSizes,
        StscEntry, MAX_CHUNK_TABLE_ENTRIES, MAX_COLLECTED_ATOMS, MAX_SAMPLE_COUNT,
        MAX_TIMELINE_ENTRIES, MP4_TO_UNIX_SECONDS,
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

        let summary = parse_chunk_summary(&trak, SampleSizes::parse(&stsz))
            .expect("large linear chunk mapping");

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
}
