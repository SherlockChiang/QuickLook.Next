use std::collections::BTreeMap;

use super::super::common::read_u32_be;

pub(super) fn append_metadata(text: &mut String, bytes: &[u8]) {
    let fields = parse_text_fields(bytes);
    for (label, key) in [
        ("Title", "TIT2"),
        ("Artist", "TPE1"),
        ("Album", "TALB"),
        ("Track", "TRCK"),
        ("Year", "TDRC"),
        ("Year", "TYER"),
        ("Genre", "TCON"),
        ("Comment", "COMM"),
    ] {
        if let Some(value) = fields.get(key).filter(|value| !value.is_empty()) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
}

fn parse_text_fields(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    if bytes.len() < 10 || bytes.get(0..3) != Some(b"ID3") {
        return fields;
    }
    let version = bytes[3];
    if !(2..=4).contains(&version) {
        return fields;
    }
    let Some(tag_size) = read_synchsafe(bytes, 6) else {
        return fields;
    };
    let tag_end = 10usize.saturating_add(tag_size).min(bytes.len());
    let mut offset = 10usize;
    while offset + 10 <= tag_end {
        let Some(frame_id) = bytes.get(offset..offset + 4) else {
            break;
        };
        if frame_id.iter().all(|byte| *byte == 0) {
            break;
        }
        if !frame_id
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            break;
        }
        let frame_size = if version == 4 {
            read_synchsafe(bytes, offset + 4)
        } else {
            read_u32_be(bytes, offset + 4).map(|value| value as usize)
        };
        let Some(frame_size) = frame_size else {
            break;
        };
        let frame_start = offset + 10;
        let Some(frame_end) = frame_start.checked_add(frame_size) else {
            break;
        };
        if frame_size == 0 || frame_end > tag_end {
            break;
        }
        let id = String::from_utf8_lossy(frame_id).to_string();
        if matches!(
            id.as_str(),
            "TIT2" | "TPE1" | "TALB" | "TRCK" | "TDRC" | "TYER" | "TCON"
        ) {
            if let Some(value) = decode_text_frame(&bytes[frame_start..frame_end]) {
                fields.entry(id).or_insert(value);
            }
        } else if id == "COMM" {
            if let Some(value) = decode_comment_frame(&bytes[frame_start..frame_end]) {
                fields.entry(id).or_insert(value);
            }
        }
        offset = frame_end;
    }
    fields
}

fn read_synchsafe(bytes: &[u8], offset: usize) -> Option<usize> {
    let chunk = bytes.get(offset..offset + 4)?;
    if chunk.iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    Some(
        ((chunk[0] as usize) << 21)
            | ((chunk[1] as usize) << 14)
            | ((chunk[2] as usize) << 7)
            | chunk[3] as usize,
    )
}

fn decode_text_frame(bytes: &[u8]) -> Option<String> {
    let (&encoding, payload) = bytes.split_first()?;
    let value = decode_text_payload(encoding, payload);
    (!value.is_empty()).then_some(value)
}

fn decode_comment_frame(bytes: &[u8]) -> Option<String> {
    let (&encoding, rest) = bytes.split_first()?;
    let payload = rest.get(3..).unwrap_or_default();
    let comment = if encoding == 1 || encoding == 2 {
        let content = strip_utf16_description(payload);
        decode_text_payload(encoding, content)
    } else {
        let content = payload
            .iter()
            .position(|byte| *byte == 0)
            .and_then(|index| payload.get(index + 1..))
            .unwrap_or(payload);
        decode_text_payload(encoding, content)
    };
    (!comment.is_empty()).then_some(comment)
}

fn strip_utf16_description(bytes: &[u8]) -> &[u8] {
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == 0 && bytes[index + 1] == 0 {
            return bytes.get(index + 2..).unwrap_or_default();
        }
        index += 2;
    }
    bytes
}

fn decode_text_payload(encoding: u8, bytes: &[u8]) -> String {
    let raw = trim_text_bytes(bytes);
    match encoding {
        1 => decode_utf16(raw),
        2 => decode_utf16_be(raw),
        3 => String::from_utf8_lossy(raw).trim().to_string(),
        _ => decode_latin1(raw).trim().to_string(),
    }
}

fn trim_text_bytes(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

fn decode_utf16(bytes: &[u8]) -> String {
    let (big_endian, payload) = if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, &bytes[2..])
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, &bytes[2..])
    } else {
        (false, bytes)
    };
    if big_endian {
        decode_utf16_be(payload)
    } else {
        let units = payload
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .trim()
            .to_string()
    }
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::{append_metadata, parse_text_fields};

    #[test]
    fn media_info_reads_id3_text_frames() {
        let bytes = make_id3_tag(&[
            ("TIT2", b"\x03Skyline".as_slice()),
            ("TPE1", b"\x03QuickLook Next".as_slice()),
            ("TALB", b"\x03Preview Sessions".as_slice()),
            ("TRCK", b"\x031/9".as_slice()),
            ("TDRC", b"\x032026".as_slice()),
            ("TCON", b"\x03Test".as_slice()),
            ("COMM", b"\x03eng\x00Fast native preview".as_slice()),
        ]);
        let mut text = String::new();

        append_metadata(&mut text, &bytes);

        assert!(text.contains("Title: Skyline"));
        assert!(text.contains("Artist: QuickLook Next"));
        assert!(text.contains("Album: Preview Sessions"));
        assert!(text.contains("Track: 1/9"));
        assert!(text.contains("Year: 2026"));
        assert!(text.contains("Genre: Test"));
        assert!(text.contains("Comment: Fast native preview"));
    }

    #[test]
    fn id3_text_decodes_utf16_bom() {
        let mut payload = vec![1, 0xFF, 0xFE];
        for unit in "北京".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        let bytes = make_id3_tag(&[("TIT2", payload.as_slice())]);
        let fields = parse_text_fields(&bytes);

        assert_eq!(fields.get("TIT2").map(String::as_str), Some("北京"));
    }

    fn make_id3_tag(frames: &[(&str, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, payload) in frames {
            body.extend_from_slice(id.as_bytes());
            body.extend_from_slice(&synchsafe_bytes(payload.len()));
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(payload);
        }
        let mut tag = Vec::new();
        tag.extend_from_slice(b"ID3");
        tag.extend_from_slice(&[4, 0, 0]);
        tag.extend_from_slice(&synchsafe_bytes(body.len()));
        tag.extend_from_slice(&body);
        tag
    }

    fn synchsafe_bytes(value: usize) -> [u8; 4] {
        [
            ((value >> 21) & 0x7F) as u8,
            ((value >> 14) & 0x7F) as u8,
            ((value >> 7) & 0x7F) as u8,
            (value & 0x7F) as u8,
        ]
    }
}
