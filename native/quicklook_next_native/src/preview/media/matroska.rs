use super::super::common::format_number;
use super::{codec_label, format_duration};

#[derive(Default)]
struct Summary {
    timecode_scale: u64,
    duration: Option<f64>,
    muxing_app: String,
    writing_app: String,
    video_codec: String,
    audio_codec: String,
    width: u64,
    height: u64,
    audio_channels: u64,
    sample_rate: Option<f64>,
    tracks: u32,
}

pub(super) fn append_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_summary(bytes) else {
        return;
    };
    if let Some(duration) = summary.duration {
        let scale = if summary.timecode_scale > 0 {
            summary.timecode_scale as f64
        } else {
            1_000_000.0
        };
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(duration * scale / 1_000_000_000.0)
        ));
    }
    if summary.tracks > 0 {
        text.push_str(&format!("\nTracks: {}", summary.tracks));
    }
    if summary.width > 0 && summary.height > 0 {
        text.push_str(&format!("\nVideo: {}x{}", summary.width, summary.height));
    }
    if !summary.video_codec.is_empty() {
        text.push_str(&format!(
            "\nVideo codec: {}",
            codec_label(&summary.video_codec)
        ));
    }
    if summary.audio_channels > 0 {
        text.push_str(&format!("\nAudio channels: {}", summary.audio_channels));
    }
    if let Some(rate) = summary.sample_rate {
        text.push_str(&format!(
            "\nAudio sample rate: {} Hz",
            format_number(rate.round() as i64)
        ));
    }
    if !summary.audio_codec.is_empty() {
        text.push_str(&format!(
            "\nAudio codec: {}",
            codec_label(&summary.audio_codec)
        ));
    }
    if !summary.writing_app.is_empty() {
        text.push_str(&format!("\nWriting app: {}", summary.writing_app));
    }
    if !summary.muxing_app.is_empty() {
        text.push_str(&format!("\nMuxing app: {}", summary.muxing_app));
    }
}

fn parse_summary(bytes: &[u8]) -> Option<Summary> {
    if !bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return None;
    }
    let mut summary = Summary::default();
    parse_elements(bytes, 0, bytes.len(), 0, &mut summary);
    (summary.duration.is_some()
        || summary.tracks > 0
        || !summary.writing_app.is_empty()
        || !summary.muxing_app.is_empty())
    .then_some(summary)
}

fn parse_elements(
    bytes: &[u8],
    mut offset: usize,
    end: usize,
    depth: usize,
    summary: &mut Summary,
) {
    if depth > 6 {
        return;
    }
    while offset < end && offset < bytes.len() {
        let Some((id, id_next)) = read_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x1549A966 => parse_info(bytes, payload, payload_end, summary),
            0x1654AE6B => parse_elements(bytes, payload, payload_end, depth + 1, summary),
            0xAE => parse_track_entry(bytes, payload, payload_end, summary),
            0x18538067 | 0x1A45DFA3 => {
                parse_elements(bytes, payload, payload_end, depth + 1, summary)
            }
            _ => {}
        }
        if payload_end <= offset {
            break;
        }
        offset = payload_end;
    }
}

fn parse_info(bytes: &[u8], mut offset: usize, end: usize, summary: &mut Summary) {
    while offset < end {
        let Some((id, id_next)) = read_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x2AD7B1 => {
                summary.timecode_scale =
                    read_uint(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x4489 => {
                summary.duration = read_float(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x4D80 => {
                summary.muxing_app =
                    read_string(bytes.get(payload..payload_end).unwrap_or_default())
            }
            0x5741 => {
                summary.writing_app =
                    read_string(bytes.get(payload..payload_end).unwrap_or_default())
            }
            _ => {}
        }
        offset = payload_end;
    }
}

fn parse_track_entry(bytes: &[u8], mut offset: usize, end: usize, summary: &mut Summary) {
    summary.tracks = summary.tracks.saturating_add(1);
    let mut track_type = 0u64;
    let mut codec = String::new();
    let mut width = 0u64;
    let mut height = 0u64;
    let mut channels = 0u64;
    let mut sample_rate = None;
    while offset < end {
        let Some((id, id_next)) = read_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x83 => track_type = read_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            0x86 => codec = read_string(bytes.get(payload..payload_end).unwrap_or_default()),
            0xE0 => (width, height) = parse_video(bytes, payload, payload_end),
            0xE1 => (channels, sample_rate) = parse_audio(bytes, payload, payload_end),
            _ => {}
        }
        offset = payload_end;
    }
    match track_type {
        1 => {
            if summary.video_codec.is_empty() {
                summary.video_codec = codec;
            }
            if summary.width == 0 {
                summary.width = width;
                summary.height = height;
            }
        }
        2 => {
            if summary.audio_codec.is_empty() {
                summary.audio_codec = codec;
            }
            if summary.audio_channels == 0 {
                summary.audio_channels = channels;
            }
            if summary.sample_rate.is_none() {
                summary.sample_rate = sample_rate;
            }
        }
        _ => {}
    }
}

fn parse_video(bytes: &[u8], mut offset: usize, end: usize) -> (u64, u64) {
    let mut width = 0;
    let mut height = 0;
    while offset < end {
        let Some((id, id_next)) = read_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0xB0 => width = read_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            0xBA => height = read_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            _ => {}
        }
        offset = payload_end;
    }
    (width, height)
}

fn parse_audio(bytes: &[u8], mut offset: usize, end: usize) -> (u64, Option<f64>) {
    let mut channels = 0;
    let mut sample_rate = None;
    while offset < end {
        let Some((id, id_next)) = read_id(bytes, offset) else {
            break;
        };
        let Some((size, payload)) = read_size(bytes, id_next) else {
            break;
        };
        let payload_end = payload.saturating_add(size).min(end).min(bytes.len());
        match id {
            0x9F => channels = read_uint(bytes.get(payload..payload_end).unwrap_or_default()),
            0xB5 => sample_rate = read_float(bytes.get(payload..payload_end).unwrap_or_default()),
            _ => {}
        }
        offset = payload_end;
    }
    (channels, sample_rate)
}

fn read_id(bytes: &[u8], offset: usize) -> Option<(u64, usize)> {
    let first = *bytes.get(offset)?;
    let length = (0..4).find(|bit| first & (0x80 >> bit) != 0)? + 1;
    let mut value = 0u64;
    for index in 0..length {
        value = (value << 8) | *bytes.get(offset + index)? as u64;
    }
    Some((value, offset + length))
}

fn read_size(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(offset)?;
    let length = (0..8).find(|bit| first & (0x80 >> bit) != 0)? + 1;
    let mut value = (first & !(0x80 >> (length - 1))) as u64;
    for index in 1..length {
        value = (value << 8) | *bytes.get(offset + index)? as u64;
    }
    (value <= usize::MAX as u64).then_some((value as usize, offset + length))
}

fn read_uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .take(8)
        .fold(0u64, |value, byte| (value << 8) | *byte as u64)
}

fn read_float(bytes: &[u8]) -> Option<f64> {
    match bytes.len() {
        4 => Some(f32::from_be_bytes(bytes.try_into().ok()?) as f64),
        8 => Some(f64::from_be_bytes(bytes.try_into().ok()?)),
        _ => None,
    }
}

fn read_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches('\0')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::container_name;
    use super::{append_metadata, parse_summary};

    #[test]
    fn media_info_reads_mkv_info_and_tracks() {
        let info = ebml(
            &[0x15, 0x49, 0xA9, 0x66],
            [
                ebml(&[0x2A, 0xD7, 0xB1], vec![0x0F, 0x42, 0x40]),
                ebml(&[0x44, 0x89], 90_000.0f64.to_be_bytes().to_vec()),
                ebml(&[0x57, 0x41], b"QuickLook Writer".to_vec()),
            ]
            .concat(),
        );
        let video = ebml(
            &[0xAE],
            [
                ebml(&[0x83], vec![1]),
                ebml(&[0x86], b"V_MPEG4".to_vec()),
                ebml(
                    &[0xE0],
                    [
                        ebml(&[0xB0], vec![0x07, 0x80]),
                        ebml(&[0xBA], vec![0x04, 0x38]),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let audio = ebml(
            &[0xAE],
            [
                ebml(&[0x83], vec![2]),
                ebml(&[0x86], b"A_OPUS".to_vec()),
                ebml(
                    &[0xE1],
                    [
                        ebml(&[0x9F], vec![2]),
                        ebml(&[0xB5], 48_000.0f64.to_be_bytes().to_vec()),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let segment = ebml(
            &[0x18, 0x53, 0x80, 0x67],
            [
                info,
                ebml(&[0x16, 0x54, 0xAE, 0x6B], [video, audio].concat()),
            ]
            .concat(),
        );
        let bytes = [ebml(&[0x1A, 0x45, 0xDF, 0xA3], Vec::new()), segment].concat();
        let summary = parse_summary(&bytes).expect("mkv summary");
        let mut text = String::new();

        append_metadata(&mut text, &bytes);

        assert_eq!(container_name("clip.bin", &bytes), "Matroska / WebM");
        assert_eq!(summary.tracks, 2);
        assert_eq!(summary.width, 1920);
        assert_eq!(summary.height, 1080);
        assert_eq!(summary.video_codec, "V_MPEG4");
        assert_eq!(summary.audio_codec, "A_OPUS");
        assert_eq!(summary.audio_channels, 2);
        assert_eq!(summary.sample_rate, Some(48_000.0));
        assert!(text.contains("Duration: 1:30"));
        assert!(text.contains("Audio codec: Opus"));
        assert!(text.contains("Writing app: QuickLook Writer"));
    }

    #[test]
    fn parser_stops_beyond_depth_budget() {
        let info = ebml(
            &[0x15, 0x49, 0xA9, 0x66],
            ebml(&[0x57, 0x41], b"too deep".to_vec()),
        );
        let mut nested = info;
        for _ in 0..8 {
            nested = ebml(&[0x18, 0x53, 0x80, 0x67], nested);
        }
        let bytes = [ebml(&[0x1A, 0x45, 0xDF, 0xA3], Vec::new()), nested].concat();

        assert!(parse_summary(&bytes).is_none());
    }

    fn ebml(id: &[u8], payload: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(id);
        if payload.len() < 0x7F {
            out.push(0x80 | payload.len() as u8);
        } else {
            out.push(0x40 | ((payload.len() >> 8) as u8 & 0x3F));
            out.push((payload.len() & 0xFF) as u8);
        }
        out.extend_from_slice(&payload);
        out
    }
}
