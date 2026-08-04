use super::super::common::format_number;

pub(super) fn parse_esds_detail(payload: &[u8]) -> Option<String> {
    let body = payload.get(4..).unwrap_or(payload);
    let mut object_type = None;
    let mut audio_config = None;
    let mut offset = 0usize;
    while offset < body.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(body, offset) else {
            offset += 1;
            continue;
        };
        if tag == 0x04 {
            object_type = descriptor.first().copied();
            if audio_config.is_none() {
                audio_config = find_mpeg4_descriptor(descriptor, 0x05)
                    .and_then(parse_aac_audio_specific_config);
            }
        } else if tag == 0x05 {
            audio_config = parse_aac_audio_specific_config(descriptor);
        }
        if object_type.is_some() && audio_config.is_some() {
            break;
        }
        offset = next.max(offset + 1);
    }

    let mut parts = Vec::new();
    if let Some(value) = object_type {
        parts.push(format!("object type {}", mpeg4_object_type_name(value)));
    }
    if let Some(config) = audio_config {
        parts.push(config);
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn find_mpeg4_descriptor(bytes: &[u8], target: u8) -> Option<&[u8]> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(bytes, offset) else {
            offset += 1;
            continue;
        };
        if tag == target {
            return Some(descriptor);
        }
        offset = next.max(offset + 1);
    }
    None
}

fn read_mpeg4_descriptor(bytes: &[u8], offset: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *bytes.get(offset)?;
    let mut position = offset.checked_add(1)?;
    let mut length = 0usize;
    for _ in 0..4 {
        let byte = *bytes.get(position)?;
        position = position.checked_add(1)?;
        length = (length << 7) | (byte & 0x7F) as usize;
        if byte & 0x80 == 0 {
            let end = position.checked_add(length)?;
            return Some((tag, bytes.get(position..end)?, end));
        }
    }
    None
}

fn parse_aac_audio_specific_config(bytes: &[u8]) -> Option<String> {
    let first = *bytes.first()?;
    let second = *bytes.get(1)?;
    let object_type = first >> 3;
    let frequency_index = ((first & 0x07) << 1) | (second >> 7);
    let channels = (second >> 3) & 0x0F;
    let mut parts = vec![aac_object_type_name(object_type).to_string()];
    if let Some(sample_rate) = aac_sample_rate(frequency_index) {
        parts.push(format!("{} Hz", format_number(sample_rate as i64)));
    }
    if channels > 0 {
        parts.push(format!("{} ch", channels));
    }
    Some(parts.join(", "))
}

fn mpeg4_object_type_name(value: u8) -> String {
    match value {
        0x40 => "MPEG-4 Audio".to_string(),
        0x20 => "MPEG-4 Visual".to_string(),
        0x21 => "H.264".to_string(),
        0x6B => "MP3".to_string(),
        _ => format!("0x{value:02X}"),
    }
}

fn aac_object_type_name(value: u8) -> &'static str {
    match value {
        1 => "AAC Main",
        2 => "AAC LC",
        3 => "AAC SSR",
        4 => "AAC LTP",
        5 => "HE-AAC SBR",
        29 => "HE-AACv2 PS",
        _ => "AAC",
    }
}

fn aac_sample_rate(index: u8) -> Option<u32> {
    Some(match index {
        0 => 96_000,
        1 => 88_200,
        2 => 64_000,
        3 => 48_000,
        4 => 44_100,
        5 => 32_000,
        6 => 24_000,
        7 => 22_050,
        8 => 16_000,
        9 => 12_000,
        10 => 11_025,
        11 => 8_000,
        12 => 7_350,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_esds_detail, read_mpeg4_descriptor};

    #[test]
    fn media_info_reads_mp4_esds_aac_config() {
        let esds = [
            0, 0, 0, 0, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0x02, 0x12,
            0x10,
        ];

        let detail = parse_esds_detail(&esds).expect("esds detail");

        assert!(detail.contains("object type MPEG-4 Audio"));
        assert!(detail.contains("AAC LC"));
        assert!(detail.contains("44,100 Hz"));
        assert!(detail.contains("2 ch"));
    }

    #[test]
    fn mpeg4_descriptor_rejects_overlong_and_truncated_lengths() {
        assert!(read_mpeg4_descriptor(&[0x05, 0x80, 0x80, 0x80, 0x80], 0).is_none());
        assert!(read_mpeg4_descriptor(&[0x05, 0x04, 0x12], 0).is_none());
        assert!(parse_esds_detail(&[0, 0, 0, 0, 0x05, 0x80, 0x80, 0x80, 0x80]).is_none());
    }
}
