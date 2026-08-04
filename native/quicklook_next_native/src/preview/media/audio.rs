use super::super::common::{format_number, read_u16, read_u32};
use super::format_duration;

struct WavSummary {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    bits_per_sample: u16,
    data_bytes: u32,
}

pub(super) fn append_wav_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_wav_summary(bytes) else {
        return;
    };
    text.push_str(&format!(
        "\nAudio format: {}",
        wav_audio_format_name(summary.audio_format)
    ));
    text.push_str(&format!("\nChannels: {}", summary.channels));
    text.push_str(&format!(
        "\nSample rate: {} Hz",
        format_number(summary.sample_rate as i64)
    ));
    if summary.bits_per_sample > 0 {
        text.push_str(&format!("\nBits per sample: {}", summary.bits_per_sample));
    }
    if summary.byte_rate > 0 && summary.data_bytes > 0 {
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(summary.data_bytes as f64 / summary.byte_rate as f64)
        ));
    }
}

fn parse_wav_summary(bytes: &[u8]) -> Option<WavSummary> {
    if bytes.len() < 12 || bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return None;
    }
    let mut offset = 12usize;
    let mut format: Option<WavSummary> = None;
    let mut data_bytes = 0u32;
    while offset.checked_add(8)? <= bytes.len() {
        let chunk_id = bytes.get(offset..offset + 4)?;
        let chunk_size = read_u32(bytes, offset + 4)? as usize;
        let payload = offset + 8;
        let next = payload.checked_add(chunk_size + (chunk_size % 2))?;
        if payload.checked_add(chunk_size)? > bytes.len() && chunk_id != b"data" {
            break;
        }
        if chunk_id == b"fmt " && chunk_size >= 16 {
            format = Some(WavSummary {
                audio_format: read_u16(bytes, payload)?,
                channels: read_u16(bytes, payload + 2)?,
                sample_rate: read_u32(bytes, payload + 4)?,
                byte_rate: read_u32(bytes, payload + 8)?,
                bits_per_sample: read_u16(bytes, payload + 14).unwrap_or(0),
                data_bytes,
            });
        } else if chunk_id == b"data" {
            data_bytes = chunk_size as u32;
            if let Some(summary) = format.as_mut() {
                summary.data_bytes = data_bytes;
            }
        }
        offset = next;
    }
    format.map(|mut summary| {
        summary.data_bytes = data_bytes;
        summary
    })
}

fn wav_audio_format_name(value: u16) -> String {
    match value {
        1 => "PCM".to_string(),
        3 => "IEEE float".to_string(),
        6 => "A-law".to_string(),
        7 => "mu-law".to_string(),
        0xFFFE => "extensible".to_string(),
        _ => format!("0x{value:04X}"),
    }
}

struct FlacSummary {
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    total_samples: u64,
}

pub(super) fn append_flac_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_flac_summary(bytes) else {
        return;
    };
    if summary.channels > 0 {
        text.push_str(&format!("\nChannels: {}", summary.channels));
    }
    if summary.sample_rate > 0 {
        text.push_str(&format!(
            "\nSample rate: {} Hz",
            format_number(summary.sample_rate as i64)
        ));
    }
    if summary.bits_per_sample > 0 {
        text.push_str(&format!("\nBits per sample: {}", summary.bits_per_sample));
    }
    if summary.sample_rate > 0 && summary.total_samples > 0 {
        text.push_str(&format!(
            "\nDuration: {}",
            format_duration(summary.total_samples as f64 / summary.sample_rate as f64)
        ));
    }
}

fn parse_flac_summary(bytes: &[u8]) -> Option<FlacSummary> {
    if bytes.len() < 42 || bytes.get(0..4) != Some(b"fLaC") {
        return None;
    }
    let mut offset = 4usize;
    while offset.checked_add(4)? <= bytes.len() {
        let block_type = bytes[offset] & 0x7F;
        let block_len = ((bytes[offset + 1] as usize) << 16)
            | ((bytes[offset + 2] as usize) << 8)
            | bytes[offset + 3] as usize;
        let payload = offset + 4;
        if payload.checked_add(block_len)? > bytes.len() {
            return None;
        }
        if block_type == 0 && block_len >= 34 {
            let stream = bytes.get(payload..payload + 34)?;
            let sample_rate = ((stream[10] as u32) << 12)
                | ((stream[11] as u32) << 4)
                | ((stream[12] as u32) >> 4);
            let channels = ((stream[12] >> 1) & 0x07) + 1;
            let bits_per_sample = (((stream[12] & 0x01) << 4) | (stream[13] >> 4)) + 1;
            let total_samples = ((stream[13] as u64 & 0x0F) << 32)
                | ((stream[14] as u64) << 24)
                | ((stream[15] as u64) << 16)
                | ((stream[16] as u64) << 8)
                | stream[17] as u64;
            return Some(FlacSummary {
                sample_rate,
                channels,
                bits_per_sample,
                total_samples,
            });
        }
        offset = payload + block_len;
    }
    None
}

#[derive(Default)]
struct OggSummary {
    codec: String,
    channels: u8,
    sample_rate: u32,
    vendor: String,
    comments: u32,
}

pub(super) fn append_ogg_metadata(text: &mut String, bytes: &[u8]) {
    let Some(summary) = parse_ogg_summary(bytes) else {
        return;
    };
    if !summary.codec.is_empty() {
        text.push_str(&format!("\nAudio codec: {}", summary.codec));
    }
    if summary.channels > 0 {
        text.push_str(&format!("\nChannels: {}", summary.channels));
    }
    if summary.sample_rate > 0 {
        text.push_str(&format!(
            "\nSample rate: {} Hz",
            format_number(summary.sample_rate as i64)
        ));
    }
    if !summary.vendor.is_empty() {
        text.push_str(&format!("\nVendor: {}", summary.vendor));
    }
    if summary.comments > 0 {
        text.push_str(&format!("\nTags: {}", summary.comments));
    }
}

fn parse_ogg_summary(bytes: &[u8]) -> Option<OggSummary> {
    let packets = read_ogg_packets(bytes, 8);
    if packets.is_empty() {
        return None;
    }

    let mut summary = OggSummary::default();
    for packet in packets {
        if packet.starts_with(b"OpusHead") && packet.len() >= 19 {
            summary.codec = "Opus".to_string();
            summary.channels = packet[9];
            summary.sample_rate = read_u32(&packet, 12).unwrap_or(48_000);
        } else if packet.starts_with(b"OpusTags") && packet.len() >= 16 {
            parse_ogg_comment_packet(&packet, 8, &mut summary);
        } else if packet.starts_with(b"\x01vorbis") && packet.len() >= 30 {
            summary.codec = "Vorbis".to_string();
            summary.channels = packet[11];
            summary.sample_rate = read_u32(&packet, 12).unwrap_or(0);
        } else if packet.starts_with(b"\x03vorbis") && packet.len() >= 11 {
            parse_ogg_comment_packet(&packet, 7, &mut summary);
        }
    }

    (!summary.codec.is_empty() || !summary.vendor.is_empty()).then_some(summary)
}

fn parse_ogg_comment_packet(packet: &[u8], offset: usize, summary: &mut OggSummary) {
    let Some(vendor_len) = read_u32(packet, offset).map(|value| value as usize) else {
        return;
    };
    let vendor_start = offset + 4;
    let Some(vendor_end) = vendor_start.checked_add(vendor_len) else {
        return;
    };
    let Some(vendor) = packet.get(vendor_start..vendor_end) else {
        return;
    };
    if summary.vendor.is_empty() {
        summary.vendor = String::from_utf8_lossy(vendor)
            .trim_matches('\0')
            .trim()
            .chars()
            .take(128)
            .collect();
    }
    summary.comments = read_u32(packet, vendor_end).unwrap_or(0);
}

fn read_ogg_packets(bytes: &[u8], max_packets: usize) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut current = Vec::new();
    let mut offset = 0usize;
    while offset.checked_add(27).is_some_and(|end| end <= bytes.len())
        && packets.len() < max_packets
    {
        if bytes.get(offset..offset + 4) != Some(b"OggS") {
            break;
        }
        let segments = bytes[offset + 26] as usize;
        let lacing_start = offset + 27;
        let payload_start = lacing_start + segments;
        if payload_start > bytes.len() {
            break;
        }
        let payload_len: usize = bytes[lacing_start..payload_start]
            .iter()
            .map(|value| *value as usize)
            .sum();
        let Some(payload_end) = payload_start.checked_add(payload_len) else {
            break;
        };
        if payload_end > bytes.len() {
            break;
        }
        let mut payload_offset = payload_start;
        for segment_len in bytes[lacing_start..payload_start].iter().copied() {
            let segment_len = segment_len as usize;
            let segment_end = payload_offset + segment_len;
            current.extend_from_slice(bytes.get(payload_offset..segment_end).unwrap_or_default());
            payload_offset = segment_end;
            if segment_len < 255 {
                packets.push(std::mem::take(&mut current));
                if packets.len() >= max_packets {
                    break;
                }
            }
        }
        offset = payload_end;
    }
    packets
}

#[cfg(test)]
mod tests {
    use super::super::container_name;
    use super::{
        append_flac_metadata, append_ogg_metadata, append_wav_metadata, parse_flac_summary,
        parse_ogg_summary, parse_wav_summary,
    };

    #[test]
    fn media_info_reads_wav_format_and_duration() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&52u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&176_400u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&352_800u32.to_le_bytes());

        let summary = parse_wav_summary(&bytes).expect("wav summary");
        let mut text = String::new();
        append_wav_metadata(&mut text, &bytes);

        assert_eq!(container_name("clip.wav", &bytes), "WAV");
        assert_eq!(summary.audio_format, 1);
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.bits_per_sample, 16);
        assert_eq!(summary.data_bytes, 352_800);
        assert!(text.contains("Audio format: PCM"));
        assert!(text.contains("Duration: 0:02"));
    }

    #[test]
    fn media_info_reads_flac_streaminfo() {
        let sample_rate = 44_100u64;
        let channels = 2u64;
        let bits_per_sample = 16u64;
        let total_samples = 88_200u64;
        let packed = (sample_rate << 44)
            | ((channels - 1) << 41)
            | ((bits_per_sample - 1) << 36)
            | total_samples;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"fLaC");
        bytes.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]);
        let mut stream = [0u8; 34];
        stream[10..18].copy_from_slice(&packed.to_be_bytes());
        bytes.extend_from_slice(&stream);

        let summary = parse_flac_summary(&bytes).expect("flac summary");
        let mut text = String::new();
        append_flac_metadata(&mut text, &bytes);

        assert_eq!(container_name("clip.bin", &bytes), "FLAC");
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.bits_per_sample, 16);
        assert_eq!(summary.total_samples, 88_200);
        assert!(text.contains("Sample rate: 44,100 Hz"));
        assert!(text.contains("Duration: 0:02"));
    }

    #[test]
    fn media_info_reads_ogg_opus_summary() {
        let mut head = b"OpusHead".to_vec();
        head.extend_from_slice(&[1, 2]);
        head.extend_from_slice(&312u16.to_le_bytes());
        head.extend_from_slice(&48_000u32.to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.push(0);
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&7u32.to_le_bytes());
        tags.extend_from_slice(b"libopus");
        tags.extend_from_slice(&2u32.to_le_bytes());
        let bytes = [ogg_page(&head), ogg_page(&tags)].concat();

        let summary = parse_ogg_summary(&bytes).expect("ogg summary");
        let mut text = String::new();
        append_ogg_metadata(&mut text, &bytes);

        assert_eq!(container_name("clip.ogg", &bytes), "Ogg");
        assert_eq!(summary.codec, "Opus");
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 48_000);
        assert_eq!(summary.vendor, "libopus");
        assert_eq!(summary.comments, 2);
        assert!(text.contains("Audio codec: Opus"));
        assert!(text.contains("Tags: 2"));
    }

    #[test]
    fn media_info_reads_ogg_vorbis_summary() {
        let mut ident = b"\x01vorbis".to_vec();
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.push(2);
        ident.extend_from_slice(&44_100u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes());
        ident.extend_from_slice(&[0, 1]);
        let mut comment = b"\x03vorbis".to_vec();
        comment.extend_from_slice(&10u32.to_le_bytes());
        comment.extend_from_slice(b"Xiph.Org  ");
        comment.extend_from_slice(&1u32.to_le_bytes());
        let bytes = [ogg_page(&ident), ogg_page(&comment)].concat();

        let summary = parse_ogg_summary(&bytes).expect("ogg summary");
        let mut text = String::new();
        append_ogg_metadata(&mut text, &bytes);

        assert_eq!(summary.codec, "Vorbis");
        assert_eq!(summary.channels, 2);
        assert_eq!(summary.sample_rate, 44_100);
        assert_eq!(summary.vendor, "Xiph.Org");
        assert_eq!(summary.comments, 1);
        assert!(text.contains("Audio codec: Vorbis"));
    }

    fn ogg_page(packet: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OggS");
        out.extend_from_slice(&[0; 22]);
        out.push(1);
        out.push(packet.len() as u8);
        out.extend_from_slice(packet);
        out
    }
}
