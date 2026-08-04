use super::common::{format_bytes, format_number, read_u16_be, read_u32_be};
use super::{
    base_info_text, file_name, generic_info_json, read_file_prefix, MAX_INFO_HEADER_BYTES,
};

pub(super) fn render_font_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_INFO_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "font", size, modified_unix);
    if let Some(summary) = parse_font_summary(&bytes) {
        text.push_str(&format!("\nFormat: {}", summary.format));
        if summary.faces > 0 {
            text.push_str(&format!("\nFaces: {}", summary.faces));
        }
        if summary.tables > 0 {
            text.push_str(&format!("\nTables: {}", summary.tables));
        }
        if summary.glyphs > 0 {
            text.push_str(&format!(
                "\nGlyphs: {}",
                format_number(summary.glyphs as i64)
            ));
        }
        if summary.sfnt_size > 0 {
            text.push_str(&format!(
                "\nDecoded sfnt size: {}",
                format_bytes(summary.sfnt_size as i64)
            ));
        }
        if summary.compressed_size > 0 {
            text.push_str(&format!(
                "\nCompressed data size: {}",
                format_bytes(summary.compressed_size as i64)
            ));
        }
        if summary.metadata_size > 0 {
            text.push_str(&format!(
                "\nMetadata block: {}",
                format_bytes(summary.metadata_size as i64)
            ));
        }
        if !summary.family.is_empty() {
            text.push_str(&format!("\nFamily: {}", summary.family));
        }
        if !summary.subfamily.is_empty() {
            text.push_str(&format!("\nStyle: {}", summary.subfamily));
        }
        if !summary.full_name.is_empty() {
            text.push_str(&format!("\nFull name: {}", summary.full_name));
        }
        if !summary.postscript_name.is_empty() {
            text.push_str(&format!("\nPostScript: {}", summary.postscript_name));
        }
        if !summary.version.is_empty() {
            text.push_str(&format!("\nVersion: {}", summary.version));
        }
        if !summary.license.is_empty() {
            text.push_str(&format!("\nLicense: {}", summary.license));
        }
        if !summary.license_url.is_empty() {
            text.push_str(&format!("\nLicense URL: {}", summary.license_url));
        }
    }
    generic_info_json(path, "font", size, modified_unix, Some(text))
}

#[derive(Default)]
struct FontSummary {
    format: &'static str,
    faces: u32,
    tables: u16,
    glyphs: u16,
    sfnt_size: u32,
    compressed_size: u32,
    metadata_size: u32,
    family: String,
    subfamily: String,
    full_name: String,
    postscript_name: String,
    version: String,
    license: String,
    license_url: String,
}

fn parse_font_summary(bytes: &[u8]) -> Option<FontSummary> {
    if bytes.starts_with(b"ttcf") {
        return Some(FontSummary {
            format: "TrueType Collection",
            faces: read_u32_be(bytes, 8).unwrap_or(0),
            ..Default::default()
        });
    }
    if bytes.starts_with(b"wOFF") {
        return Some(FontSummary {
            format: "WOFF font",
            tables: read_u16_be(bytes, 12).unwrap_or(0),
            sfnt_size: read_u32_be(bytes, 16).unwrap_or(0),
            metadata_size: read_u32_be(bytes, 28).unwrap_or(0),
            ..Default::default()
        });
    }
    if bytes.starts_with(b"wOF2") {
        return Some(FontSummary {
            format: "WOFF2 font",
            tables: read_u16_be(bytes, 12).unwrap_or(0),
            sfnt_size: read_u32_be(bytes, 16).unwrap_or(0),
            compressed_size: read_u32_be(bytes, 20).unwrap_or(0),
            metadata_size: read_u32_be(bytes, 32).unwrap_or(0),
            ..Default::default()
        });
    }

    let format = if bytes.starts_with(&[0, 1, 0, 0]) {
        "TrueType font"
    } else if bytes.starts_with(b"OTTO") {
        "OpenType/CFF font"
    } else {
        return None;
    };
    let tables = read_u16_be(bytes, 4)?;
    let mut summary = FontSummary {
        format,
        faces: 1,
        tables,
        ..Default::default()
    };
    if let Some((offset, length)) = find_sfnt_table(bytes, "name", tables) {
        parse_font_name_table(bytes, offset, length, &mut summary);
    }
    if let Some((offset, length)) = find_sfnt_table(bytes, "maxp", tables) {
        summary.glyphs = parse_font_maxp_glyph_count(bytes, offset, length).unwrap_or(0);
    }
    Some(summary)
}

fn find_sfnt_table(bytes: &[u8], tag: &str, tables: u16) -> Option<(usize, usize)> {
    let table_count = tables.min(256) as usize;
    for index in 0..table_count {
        let record = 12 + index * 16;
        let record_end = record.checked_add(16)?;
        let tag_bytes = bytes.get(record..record + 4)?;
        if record_end > bytes.len() || tag_bytes != tag.as_bytes() {
            continue;
        }
        let offset = read_u32_be(bytes, record + 8)? as usize;
        let length = read_u32_be(bytes, record + 12)? as usize;
        if offset.checked_add(length)? <= bytes.len() {
            return Some((offset, length));
        }
    }
    None
}

fn parse_font_name_table(bytes: &[u8], offset: usize, length: usize, summary: &mut FontSummary) {
    let end = offset.saturating_add(length).min(bytes.len());
    if offset + 6 > end {
        return;
    }
    let count = read_u16_be(bytes, offset + 2).unwrap_or(0).min(256) as usize;
    let storage = offset + read_u16_be(bytes, offset + 4).unwrap_or(0) as usize;
    for index in 0..count {
        let record = offset + 6 + index * 12;
        if record + 12 > end {
            break;
        }
        let platform = read_u16_be(bytes, record).unwrap_or(0);
        let name_id = read_u16_be(bytes, record + 6).unwrap_or(0);
        let len = read_u16_be(bytes, record + 8).unwrap_or(0) as usize;
        let off = read_u16_be(bytes, record + 10).unwrap_or(0) as usize;
        let value_start = storage.saturating_add(off);
        let value_end = value_start.saturating_add(len);
        let Some(raw) = bytes.get(value_start..value_end) else {
            continue;
        };
        let value = decode_font_name(platform, raw);
        if value.is_empty() {
            continue;
        }
        match name_id {
            1 if summary.family.is_empty() => summary.family = value,
            2 if summary.subfamily.is_empty() => summary.subfamily = value,
            4 if summary.full_name.is_empty() => summary.full_name = value,
            6 if summary.postscript_name.is_empty() => summary.postscript_name = value,
            5 if summary.version.is_empty() => summary.version = value,
            13 if summary.license.is_empty() => summary.license = value,
            14 if summary.license_url.is_empty() => summary.license_url = value,
            _ => {}
        }
    }
}

fn parse_font_maxp_glyph_count(bytes: &[u8], offset: usize, length: usize) -> Option<u16> {
    if length < 6 || offset.checked_add(6)? > bytes.len() {
        return None;
    }
    read_u16_be(bytes, offset + 4)
}

fn decode_font_name(platform: u16, bytes: &[u8]) -> String {
    if platform == 0 || platform == 3 {
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .trim()
            .to_string()
    } else {
        String::from_utf8_lossy(bytes)
            .trim_matches('\0')
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_font_summary;

    #[test]
    fn font_summary_detects_woff_tables() {
        let mut bytes = vec![0u8; 44];
        bytes[0..4].copy_from_slice(b"wOFF");
        bytes[12..14].copy_from_slice(&3u16.to_be_bytes());
        bytes[16..20].copy_from_slice(&4096u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&256u32.to_be_bytes());

        let summary = parse_font_summary(&bytes).expect("woff summary");

        assert_eq!(summary.format, "WOFF font");
        assert_eq!(summary.tables, 3);
        assert_eq!(summary.sfnt_size, 4096);
        assert_eq!(summary.metadata_size, 256);
    }

    #[test]
    fn font_summary_reads_names_and_glyph_count() {
        fn utf16be(value: &str) -> Vec<u8> {
            value
                .encode_utf16()
                .flat_map(|unit| unit.to_be_bytes())
                .collect()
        }

        let names = [
            (1u16, utf16be("Quick Sans")),
            (5u16, utf16be("Version 1.2")),
            (13u16, utf16be("Open Font License")),
            (14u16, utf16be("https://example.test/ofl")),
        ];
        let name_offset = 44usize;
        let name_storage_offset = 6 + names.len() * 12;
        let name_len = name_storage_offset + names.iter().map(|(_, v)| v.len()).sum::<usize>();
        let maxp_offset = name_offset + name_len;
        let mut bytes = vec![0u8; maxp_offset + 6];
        bytes[0..4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[4..6].copy_from_slice(&2u16.to_be_bytes());
        bytes[12..16].copy_from_slice(b"name");
        bytes[20..24].copy_from_slice(&(name_offset as u32).to_be_bytes());
        bytes[24..28].copy_from_slice(&(name_len as u32).to_be_bytes());
        bytes[28..32].copy_from_slice(b"maxp");
        bytes[36..40].copy_from_slice(&(maxp_offset as u32).to_be_bytes());
        bytes[40..44].copy_from_slice(&6u32.to_be_bytes());
        bytes[name_offset + 2..name_offset + 4]
            .copy_from_slice(&(names.len() as u16).to_be_bytes());
        bytes[name_offset + 4..name_offset + 6]
            .copy_from_slice(&(name_storage_offset as u16).to_be_bytes());
        let mut storage_pos = 0usize;
        for (index, (name_id, value)) in names.iter().enumerate() {
            let record = name_offset + 6 + index * 12;
            bytes[record..record + 2].copy_from_slice(&3u16.to_be_bytes());
            bytes[record + 6..record + 8].copy_from_slice(&name_id.to_be_bytes());
            bytes[record + 8..record + 10].copy_from_slice(&(value.len() as u16).to_be_bytes());
            bytes[record + 10..record + 12].copy_from_slice(&(storage_pos as u16).to_be_bytes());
            let value_start = name_offset + name_storage_offset + storage_pos;
            bytes[value_start..value_start + value.len()].copy_from_slice(value);
            storage_pos += value.len();
        }
        bytes[maxp_offset..maxp_offset + 4].copy_from_slice(&[0, 1, 0, 0]);
        bytes[maxp_offset + 4..maxp_offset + 6].copy_from_slice(&321u16.to_be_bytes());

        let summary = parse_font_summary(&bytes).expect("font summary");

        assert_eq!(summary.family, "Quick Sans");
        assert_eq!(summary.version, "Version 1.2");
        assert_eq!(summary.license, "Open Font License");
        assert_eq!(summary.license_url, "https://example.test/ofl");
        assert_eq!(summary.glyphs, 321);
    }
}
