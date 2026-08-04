use super::{
    base_info_text,
    common::{format_bytes, format_timestamp, read_u32, read_u64},
    file_name, generic_info_json, read_file_prefix,
};

const MAX_CHM_HEADER_BYTES: usize = 8 * 1024;
const MAX_CHM_DIRECTORY_ENTRIES: usize = 12;
const MAX_CHM_ENTRY_NAME_BYTES: usize = 260;
const MAX_CHM_COMPRESSED_STREAM_SCAN: usize = 32;
const MAX_CHM_COMPRESSED_STREAMS: usize = 8;
const MAX_CHM_SYSTEM_STREAM_BYTES: usize = 4 * 1024;
const MAX_CHM_SYSTEM_FIELDS: usize = 8;
const MAX_CHM_ENCINT_BYTES: usize = 8;

pub(super) fn render_chm_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_CHM_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "chm", size, modified_unix);
    if bytes.starts_with(b"ITSF") {
        text.push_str("\nFormat: Microsoft Compiled HTML Help");
        if let Some(version) = read_u32(&bytes, 4) {
            text.push_str(&format!("\nITSF version: {}", version));
        }
        if let Some(header_len) = read_u32(&bytes, 8) {
            text.push_str(&format!("\nHeader length: {} bytes", header_len));
        }
        if let Some(lang_id) = read_u32(&bytes, 20) {
            text.push_str(&format!("\nLanguage ID: 0x{lang_id:08X}"));
        }
        if let Some(timestamp) = read_u32(&bytes, 24).filter(|value| *value > 0) {
            text.push_str(&format!(
                "\nTimestamp: {}",
                format_timestamp(timestamp as i64)
            ));
        }
        if let Some(dir_offset) = read_u64(&bytes, 40).filter(|value| *value > 0) {
            text.push_str(&format!("\nDirectory offset: 0x{dir_offset:016X}"));
        }
        if let Some(dir_len) = read_u64(&bytes, 48).filter(|value| *value > 0) {
            let formatted_len = i64::try_from(dir_len)
                .map(format_bytes)
                .unwrap_or_else(|_| format!("{dir_len} bytes"));
            text.push_str(&format!("\nDirectory length: {}", formatted_len));
        }
        append_chm_itsp_summary(&mut text, &bytes);
    } else {
        text.push_str("\nFormat: CHM-like help file");
    }
    generic_info_json(path, "chm", size, modified_unix, Some(text))
}

fn append_chm_itsp_summary(text: &mut String, bytes: &[u8]) {
    let Some(dir_offset) = read_u64(bytes, 40)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
    else {
        return;
    };
    let Some(header_end) = dir_offset.checked_add(56) else {
        return;
    };
    if header_end > bytes.len() || bytes.get(dir_offset..dir_offset + 4) != Some(b"ITSP") {
        return;
    }
    let version = read_u32(bytes, dir_offset + 4).unwrap_or(0);
    let header_len = read_u32(bytes, dir_offset + 8).unwrap_or(0);
    let block_len = read_u32(bytes, dir_offset + 16).unwrap_or(0);
    let index_depth = read_u32(bytes, dir_offset + 24).unwrap_or(0);
    let index_root = read_u32(bytes, dir_offset + 28).unwrap_or(0);
    let index_head = read_u32(bytes, dir_offset + 32).unwrap_or(0);
    let block_count = read_u32(bytes, dir_offset + 40).unwrap_or(0);
    text.push_str(&format!(
        "\nITSP version: {version}\nITSP header length: {header_len} bytes\nDirectory block length: {block_len} bytes\nDirectory block count: {block_count}\nDirectory index depth/root/head: {index_depth}/{index_root}/{index_head}"
    ));
    let entries = chm_directory_entries(bytes, dir_offset, header_len as usize, block_len as usize);
    if !entries.is_empty() {
        text.push_str(&format!(
            "\nDirectory entries: {}",
            entries
                .iter()
                .map(ChmDirectoryEntry::summary)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let compressed = chm_compressed_stream_summary(&entries);
        if !compressed.is_empty() {
            text.push_str(&format!("\nCompressed streams: {}", compressed.join(", ")));
        }
        for (label, value) in chm_system_summary(bytes, &entries) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
}

struct ChmDirectoryEntry {
    name: String,
    section: usize,
    offset: usize,
    len: usize,
}

impl ChmDirectoryEntry {
    fn summary(&self) -> String {
        format!(
            "{} [section {}, offset {}, {}]",
            self.name,
            self.section,
            self.offset,
            format_bytes(self.len as i64)
        )
    }
}

fn chm_directory_entries(
    bytes: &[u8],
    dir_offset: usize,
    header_len: usize,
    block_len: usize,
) -> Vec<ChmDirectoryEntry> {
    if header_len == 0 || block_len < 32 {
        return Vec::new();
    }
    let Some(block_offset) = dir_offset.checked_add(header_len) else {
        return Vec::new();
    };
    let Some(block_end) = block_offset
        .checked_add(block_len)
        .filter(|end| *end <= bytes.len())
    else {
        return Vec::new();
    };
    if bytes.get(block_offset..block_offset + 4) != Some(b"PMGL") {
        return Vec::new();
    }
    let free_space = read_u32(bytes, block_offset + 4).unwrap_or(0) as usize;
    let entries_end = block_end.saturating_sub(free_space.min(block_len));
    let mut offset = block_offset + 20;
    let mut entries = Vec::new();
    while offset < entries_end && entries.len() < MAX_CHM_DIRECTORY_ENTRIES {
        let Some((name_len, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        if name_len == 0 || name_len > MAX_CHM_ENTRY_NAME_BYTES {
            break;
        }
        let Some(name_end) = offset
            .checked_add(name_len)
            .filter(|end| *end <= entries_end)
        else {
            break;
        };
        let name = String::from_utf8_lossy(&bytes[offset..name_end]).to_string();
        offset = name_end;
        let Some((section, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        let Some((file_offset, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        let Some((file_len, next)) = read_chm_encint(bytes, offset, entries_end) else {
            break;
        };
        offset = next;
        if !name.is_empty() {
            entries.push(ChmDirectoryEntry {
                name,
                section,
                offset: file_offset,
                len: file_len,
            });
        }
    }
    entries
}

fn chm_compressed_stream_summary(entries: &[ChmDirectoryEntry]) -> Vec<String> {
    let mut summary = Vec::new();
    for entry in entries.iter().take(MAX_CHM_COMPRESSED_STREAM_SCAN) {
        let lower = entry.name.to_ascii_lowercase();
        if lower.contains("::dataspace/storage/") || lower.contains("::dataspace/namelist") {
            summary.push(format!(
                "{} ({})",
                entry.name,
                format_bytes(entry.len as i64)
            ));
        } else if lower.ends_with("/content") && lower.contains("mscompressed") {
            summary.push(format!(
                "compressed content {}",
                format_bytes(entry.len as i64)
            ));
        }
        if summary.len() >= MAX_CHM_COMPRESSED_STREAMS {
            break;
        }
    }
    summary
}

fn chm_system_summary(bytes: &[u8], entries: &[ChmDirectoryEntry]) -> Vec<(&'static str, String)> {
    let Some(system) = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("/#SYSTEM") && entry.section == 0)
    else {
        return Vec::new();
    };
    if system.len == 0 || system.len > MAX_CHM_SYSTEM_STREAM_BYTES {
        return Vec::new();
    }
    let Some(system_end) = system
        .offset
        .checked_add(system.len)
        .filter(|end| *end <= bytes.len())
    else {
        return Vec::new();
    };
    let data = &bytes[system.offset..system_end];
    let mut offset = 0usize;
    let mut values = Vec::new();
    while values.len() < MAX_CHM_SYSTEM_FIELDS {
        let Some(header_end) = offset.checked_add(4).filter(|end| *end <= data.len()) else {
            break;
        };
        let code = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset = header_end;
        if len == 0 {
            break;
        }
        let Some(value_end) = offset.checked_add(len).filter(|end| *end <= data.len()) else {
            break;
        };
        let value = String::from_utf8_lossy(&data[offset..value_end])
            .trim_matches('\0')
            .trim()
            .to_string();
        match code {
            2 if !value.is_empty() => values.push(("Default topic", value)),
            3 if !value.is_empty() => values.push(("Title", value)),
            _ => {}
        }
        offset = value_end;
    }
    values
}

fn read_chm_encint(bytes: &[u8], offset: usize, limit: usize) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut current = offset;
    for _ in 0..MAX_CHM_ENCINT_BYTES {
        let byte = *bytes.get(current).filter(|_| current < limit)?;
        current += 1;
        value = value
            .checked_mul(128)?
            .checked_add((byte & 0x7F) as usize)?;
        if byte & 0x80 == 0 {
            return Some((value, current));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::append_chm_itsp_summary;

    #[test]
    fn chm_itsp_summary_rejects_hostile_directory_offsets() {
        let mut bytes = vec![0u8; 56];
        bytes[0..4].copy_from_slice(b"ITSF");
        bytes[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut text = String::new();

        append_chm_itsp_summary(&mut text, &bytes);

        assert!(text.is_empty());
    }

    #[test]
    fn chm_itsp_summary_reads_directory_header() {
        let mut bytes = vec![0u8; 512];
        bytes[0..4].copy_from_slice(b"ITSF");
        bytes[40..48].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[0x100..0x104].copy_from_slice(b"ITSP");
        bytes[0x104..0x108].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x108..0x10C].copy_from_slice(&84u32.to_le_bytes());
        bytes[0x110..0x114].copy_from_slice(&128u32.to_le_bytes());
        bytes[0x118..0x11C].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x11C..0x120].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x120..0x124].copy_from_slice(&4u32.to_le_bytes());
        bytes[0x128..0x12C].copy_from_slice(&7u32.to_le_bytes());
        bytes[0x154..0x158].copy_from_slice(b"PMGL");
        bytes[0x158..0x15C].copy_from_slice(&36u32.to_le_bytes());
        bytes[0x168] = 10;
        bytes[0x169..0x173].copy_from_slice(b"/index.htm");
        bytes[0x173] = 0;
        bytes[0x174] = 123;
        bytes[0x175] = 45;
        bytes[0x176] = 40;
        bytes[0x177..0x19F].copy_from_slice(b"::DataSpace/Storage/MSCompressed/Content");
        bytes[0x19F] = 1;
        bytes[0x1A0] = 0;
        bytes[0x1A1] = 0x81;
        bytes[0x1A2] = 0x48;
        bytes[0x1A3] = 8;
        bytes[0x1A4..0x1AC].copy_from_slice(b"/#SYSTEM");
        bytes[0x1AC] = 0;
        bytes[0x1AD] = 0x83;
        bytes[0x1AE] = 0x40;
        bytes[0x1AF] = 28;
        bytes[0x1C0..0x1C2].copy_from_slice(&3u16.to_le_bytes());
        bytes[0x1C2..0x1C4].copy_from_slice(&10u16.to_le_bytes());
        bytes[0x1C4..0x1CE].copy_from_slice(b"Help Title");
        bytes[0x1CE..0x1D0].copy_from_slice(&2u16.to_le_bytes());
        bytes[0x1D0..0x1D2].copy_from_slice(&10u16.to_le_bytes());
        bytes[0x1D2..0x1DC].copy_from_slice(b"/index.htm");
        let mut text = String::new();

        append_chm_itsp_summary(&mut text, &bytes);

        assert!(text.contains("ITSP version: 1"));
        assert!(text.contains("ITSP header length: 84 bytes"));
        assert!(text.contains("Directory block length: 128 bytes"));
        assert!(text.contains("Directory block count: 7"));
        assert!(text.contains("Directory index depth/root/head: 2/3/4"));
        assert!(text.contains("Directory entries: /index.htm [section 0, offset 123, 45 B]"));
        assert!(
            text.contains("Compressed streams: ::DataSpace/Storage/MSCompressed/Content (200 B)")
        );
        assert!(text.contains("Title: Help Title"));
        assert!(text.contains("Default topic: /index.htm"));
    }
}
