use super::super::common::{
    format_number, format_timestamp, read_i16_be, read_i32_be, read_i64_be, read_u16_be,
    read_u32_be, read_u64_be,
};
use super::{
    codec::{parse_avcc_detail, parse_esds_detail, parse_hvcc_detail},
    codec_label, format_duration,
};

const MAX_ATOM_DEPTH: usize = 4;
const MAX_COLLECTED_ATOMS: usize = 1024;
const MAX_TIMELINE_ENTRIES: usize = 100_000;
const MAX_CHUNK_TABLE_ENTRIES: usize = 1_000_000;
const MAX_SAMPLE_COUNT: usize = 1_000_000;
const MAX_CHUNK_DETAILS: usize = 4;
const MAX_SAMPLE_DESCRIPTION_ENTRIES: u32 = 16;
const MP4_TO_UNIX_SECONDS: u64 = 2_082_844_800;

struct Atom<'a> {
    kind: &'a [u8],
    payload_start: usize,
    end: usize,
}

fn find_atom_payload<'a>(bytes: &'a [u8], atom: &[u8; 4]) -> Option<&'a [u8]> {
    find_atom_payload_in_range(bytes, 0, bytes.len(), atom, 0)
}

fn collect_atom_payloads<'a>(bytes: &'a [u8], atom: &[u8; 4], found: &mut Vec<&'a [u8]>) {
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

fn find_atom_payload_in_range<'a>(
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

fn parse_movie_duration_seconds(payload: &[u8]) -> Option<f64> {
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

fn parse_movie_created_unix(payload: &[u8]) -> Option<i64> {
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

fn rotation_degrees(bytes: &[u8]) -> Option<i32> {
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

fn duration_from_timescale(duration: u64, timescale: u32) -> Option<f64> {
    (timescale > 0).then(|| duration as f64 / timescale as f64)
}

#[derive(Default)]
struct TrackSummary {
    kind: &'static str,
    codec: String,
    codec_detail: String,
    language: String,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u16>,
    sample_rate: Option<u32>,
    duration_seconds: Option<f64>,
    data_bytes: Option<u64>,
    timing_entries: Option<u32>,
    samples: Option<u64>,
    decode_ticks: Option<u64>,
    first_sample_delta: Option<u32>,
    composition_entries: Option<u32>,
    composition_samples: Option<u64>,
    first_composition_offset: Option<i64>,
    composition_offset_range: Option<(i64, i64)>,
    edit_entries: Option<u32>,
    first_edit_duration: Option<u64>,
    first_edit_media_time: Option<i64>,
    first_edit_rate: Option<f64>,
    chunks: Option<u32>,
    first_chunk_offset: Option<u64>,
    last_chunk_end: Option<u64>,
    first_chunk_samples: Option<u32>,
    first_chunk_bytes: Option<u64>,
    first_sample_size: Option<u32>,
    chunk_details: Vec<String>,
}

pub(super) fn append_metadata(text: &mut String, bytes: &[u8], file_size: i64) {
    if let Some(brand) = major_brand(bytes) {
        text.push_str(&format!("\nBrand: {brand}"));
    }
    let movie_header = find_atom_payload(bytes, b"mvhd");
    if let Some(duration) = movie_header.and_then(parse_movie_duration_seconds) {
        text.push_str(&format!("\nDuration: {}", format_duration(duration)));
        if let Some(bitrate) = estimate_bitrate(file_size, duration) {
            text.push_str(&format!("\nBitrate: {}", format_bitrate(bitrate)));
        }
    }
    if let Some(created_unix) = movie_header.and_then(parse_movie_created_unix) {
        text.push_str(&format!("\nCreated: {}", format_timestamp(created_unix)));
    }
    if let Some(rotation) = rotation_degrees(bytes) {
        text.push_str(&format!("\nRotation: {}", format_rotation(rotation)));
    }
    append_tracks(text, &tracks(bytes));
}

fn major_brand(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 || bytes.get(4..8) != Some(b"ftyp") {
        return None;
    }
    let brand = std::str::from_utf8(bytes.get(8..12)?).ok()?.trim();
    (!brand.is_empty()).then(|| brand.to_string())
}

fn tracks(bytes: &[u8]) -> Vec<TrackSummary> {
    let mut payloads = Vec::new();
    collect_atom_payloads(bytes, b"trak", &mut payloads);
    payloads.into_iter().filter_map(parse_track).collect()
}

fn parse_track(trak: &[u8]) -> Option<TrackSummary> {
    let handler = find_atom_payload(trak, b"hdlr").and_then(parse_handler_type);
    let mut summary = TrackSummary {
        kind: match handler.as_deref() {
            Some("vide") => "Video",
            Some("soun") => "Audio",
            _ => "Media",
        },
        ..Default::default()
    };

    if let Some(tkhd) = find_atom_payload(trak, b"tkhd") {
        let (width, height) = parse_track_dimensions(tkhd).unwrap_or((0, 0));
        if width > 0 && height > 0 {
            summary.width = Some(width);
            summary.height = Some(height);
        }
    }
    if let Some(mdhd) = find_atom_payload(trak, b"mdhd") {
        summary.duration_seconds = parse_media_duration_seconds(mdhd);
        summary.language = parse_media_language(mdhd).unwrap_or_default();
    }
    if let Some(stsd) = find_atom_payload(trak, b"stsd") {
        parse_sample_descriptions(stsd, &mut summary);
    }
    apply_track_tables(trak, &mut summary);

    (!summary.codec.is_empty()
        || summary.width.is_some()
        || !summary.codec_detail.is_empty()
        || !summary.language.is_empty()
        || summary.height.is_some()
        || summary.channels.is_some()
        || summary.sample_rate.is_some()
        || summary.duration_seconds.is_some()
        || summary.data_bytes.is_some()
        || summary.timing_entries.is_some()
        || summary.composition_entries.is_some()
        || summary.edit_entries.is_some()
        || summary.chunks.is_some())
    .then_some(summary)
}

fn parse_handler_type(payload: &[u8]) -> Option<String> {
    let handler = std::str::from_utf8(payload.get(8..12)?).ok()?.trim();
    (!handler.is_empty()).then(|| handler.to_string())
}

fn parse_media_duration_seconds(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;
    match version {
        0 => duration_from_timescale(
            u64::from(read_u32_be(payload, 16)?),
            read_u32_be(payload, 12)?,
        ),
        1 => duration_from_timescale(read_u64_be(payload, 24)?, read_u32_be(payload, 20)?),
        _ => None,
    }
}

fn parse_media_language(payload: &[u8]) -> Option<String> {
    let version = *payload.first()?;
    let offset = match version {
        0 => 20,
        1 => 32,
        _ => return None,
    };
    let packed = read_u16_be(payload, offset)?;
    let mut language = String::new();
    for shift in [10, 5, 0] {
        let value = ((packed >> shift) & 0x1F) as u8;
        if value == 0 {
            return None;
        }
        language.push((value + 0x60) as char);
    }
    (language != "und").then_some(language)
}

fn parse_track_dimensions(payload: &[u8]) -> Option<(u32, u32)> {
    let version = *payload.first()?;
    let offset = match version {
        0 => 76,
        1 => 88,
        _ => return None,
    };
    let width = read_u32_be(payload, offset)? >> 16;
    let height = read_u32_be(payload, offset.checked_add(4)?)? >> 16;
    Some((width, height))
}

fn parse_sample_descriptions(payload: &[u8], summary: &mut TrackSummary) -> Option<()> {
    if *payload.first()? != 0 {
        return None;
    }
    let entries = read_u32_be(payload, 4)?.min(MAX_SAMPLE_DESCRIPTION_ENTRIES) as usize;
    let mut offset = 8usize;
    for _ in 0..entries {
        let entry_size = usize::try_from(read_u32_be(payload, offset)?).ok()?;
        let entry_end = offset.checked_add(entry_size)?;
        if entry_size < 8 || entry_end > payload.len() {
            return None;
        }
        let codec =
            std::str::from_utf8(payload.get(offset.checked_add(4)?..offset.checked_add(8)?)?)
                .ok()?
                .to_string();
        if summary.codec.is_empty() {
            summary.codec = codec;
        }

        if summary.kind == "Video" && entry_size >= 36 {
            summary.width = read_u16_be(payload, offset.checked_add(32)?)
                .map(u32::from)
                .filter(|value| *value > 0);
            summary.height = read_u16_be(payload, offset.checked_add(34)?)
                .map(u32::from)
                .filter(|value| *value > 0);
            if let Some(detail) = parse_video_codec_detail(payload, offset, entry_size) {
                summary.codec_detail = detail;
            }
        } else if summary.kind == "Audio" && entry_size >= 32 {
            summary.channels =
                read_u16_be(payload, offset.checked_add(16)?).filter(|value| *value > 0);
            summary.sample_rate = read_u32_be(payload, offset.checked_add(24)?)
                .map(|value| value >> 16)
                .filter(|value| *value > 0);
            if let Some(detail) = parse_audio_codec_detail(payload, offset, entry_size) {
                summary.codec_detail = detail;
            }
        }
        offset = entry_end;
    }
    Some(())
}

fn parse_video_codec_detail(
    payload: &[u8],
    entry_offset: usize,
    entry_size: usize,
) -> Option<String> {
    let start = entry_offset.checked_add(86)?;
    let end = entry_offset.checked_add(entry_size)?;
    if let Some(avcc) = find_atom_payload_in_range(payload, start, end, b"avcC", 0) {
        return parse_avcc_detail(avcc);
    }
    if let Some(hvcc) = find_atom_payload_in_range(payload, start, end, b"hvcC", 0) {
        return parse_hvcc_detail(hvcc);
    }
    None
}

fn parse_audio_codec_detail(
    payload: &[u8],
    entry_offset: usize,
    entry_size: usize,
) -> Option<String> {
    let start = entry_offset.checked_add(36)?;
    let end = entry_offset.checked_add(entry_size)?;
    find_atom_payload_in_range(payload, start, end, b"esds", 0).and_then(parse_esds_detail)
}

fn append_tracks(text: &mut String, tracks: &[TrackSummary]) {
    for (index, track) in tracks.iter().enumerate() {
        text.push_str(&format!("\n{} track {}", track.kind, index + 1));
        if !track.codec.is_empty() {
            text.push_str(&format!(": {}", codec_label(&track.codec)));
        }
        if !track.codec_detail.is_empty() {
            text.push_str(&format!(" ({})", track.codec_detail));
        }
        if !track.language.is_empty() {
            text.push_str(&format!("\n{} language: {}", track.kind, track.language));
        }
        if let (Some(width), Some(height)) = (track.width, track.height) {
            text.push_str(&format!("\n{} size: {}x{}", track.kind, width, height));
        }
        if let Some(channels) = track.channels {
            text.push_str(&format!("\n{} channels: {}", track.kind, channels));
        }
        if let Some(sample_rate) = track.sample_rate {
            text.push_str(&format!(
                "\n{} sample rate: {} Hz",
                track.kind,
                format_number(i64::from(sample_rate))
            ));
        }
        if let Some(duration) = track.duration_seconds {
            text.push_str(&format!(
                "\n{} duration: {}",
                track.kind,
                format_duration(duration)
            ));
            if let Some(data_bytes) = track.data_bytes {
                if let Some(bitrate) = estimate_bitrate(data_bytes as i64, duration) {
                    text.push_str(&format!(
                        "\n{} bitrate: {}",
                        track.kind,
                        format_bitrate(bitrate)
                    ));
                }
            }
        }
        if let Some(entries) = track.timing_entries {
            text.push_str(&format!("\n{} timing entries: {}", track.kind, entries));
        }
        if let Some(samples) = track.samples {
            text.push_str(&format!(
                "\n{} samples: {}",
                track.kind,
                format_number(samples as i64)
            ));
        }
        if let Some(decode_ticks) = track.decode_ticks {
            text.push_str(&format!(
                "\n{} decode ticks: {}",
                track.kind,
                format_number(decode_ticks as i64)
            ));
        }
        if let Some(delta) = track.first_sample_delta {
            text.push_str(&format!("\n{} first sample delta: {}", track.kind, delta));
        }
        if let Some(entries) = track.composition_entries {
            text.push_str(&format!(
                "\n{} composition offsets: {}",
                track.kind, entries
            ));
        }
        if let Some(samples) = track.composition_samples {
            text.push_str(&format!(
                "\n{} composition samples: {}",
                track.kind,
                format_number(samples as i64)
            ));
        }
        if let Some(offset) = track.first_composition_offset {
            text.push_str(&format!(
                "\n{} first composition offset: {}",
                track.kind, offset
            ));
        }
        if let Some((min, max)) = track.composition_offset_range {
            text.push_str(&format!(
                "\n{} composition offset range: {}..{}",
                track.kind, min, max
            ));
        }
        if let Some(entries) = track.edit_entries {
            text.push_str(&format!("\n{} edit list entries: {}", track.kind, entries));
        }
        if let Some(duration) = track.first_edit_duration {
            text.push_str(&format!(
                "\n{} first edit duration: {}",
                track.kind,
                format_number(duration as i64)
            ));
        }
        if let Some(media_time) = track.first_edit_media_time {
            text.push_str(&format!(
                "\n{} first edit media time: {}",
                track.kind, media_time
            ));
        }
        if let Some(rate) = track.first_edit_rate {
            text.push_str(&format!("\n{} first edit rate: {:.2}", track.kind, rate));
        }
        if let Some(chunks) = track.chunks {
            text.push_str(&format!("\n{} chunks: {}", track.kind, chunks));
            if let (Some(first), Some(last)) = (track.first_chunk_offset, track.last_chunk_end) {
                text.push_str(&format!(" (0x{first:X}-0x{last:X})"));
            }
        }
        if let Some(samples) = track.first_chunk_samples {
            text.push_str(&format!(
                "\n{} first chunk samples: {}",
                track.kind, samples
            ));
        }
        if let Some(bytes) = track.first_chunk_bytes {
            text.push_str(&format!(
                "\n{} first chunk bytes: {}",
                track.kind,
                format_number(bytes as i64)
            ));
        }
        if let Some(size) = track.first_sample_size {
            text.push_str(&format!(
                "\n{} first sample size: {}",
                track.kind,
                format_number(i64::from(size))
            ));
        }
        if !track.chunk_details.is_empty() {
            text.push_str(&format!(
                "\n{} chunk map: {}",
                track.kind,
                track.chunk_details.join(", ")
            ));
        }
    }
}

fn estimate_bitrate(size: i64, duration_seconds: f64) -> Option<f64> {
    (size > 0 && duration_seconds > 0.0).then(|| size as f64 * 8.0 / duration_seconds)
}

fn format_bitrate(bits_per_second: f64) -> String {
    if bits_per_second >= 1_000_000.0 {
        format!("{:.2} Mbps", bits_per_second / 1_000_000.0)
    } else if bits_per_second >= 1_000.0 {
        format!("{:.0} kbps", bits_per_second / 1_000.0)
    } else {
        format!("{:.0} bps", bits_per_second)
    }
}

fn format_rotation(degrees: i32) -> String {
    format!("{degrees}°")
}

fn apply_track_tables(trak: &[u8], summary: &mut TrackSummary) {
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
mod tests;
