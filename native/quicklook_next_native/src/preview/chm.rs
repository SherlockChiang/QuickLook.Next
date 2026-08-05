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
const CHM_ITSF_V2_HEADER_LEN: usize = 0x58;
const CHM_ITSF_V3_HEADER_LEN: usize = 0x60;
const CHM_ITSF_LAST_MODIFIED_OFFSET: usize = 0x10;
const CHM_ITSF_LANG_ID_OFFSET: usize = 0x14;
const CHM_ITSF_DIR_OFFSET: usize = 0x48;
const CHM_ITSF_DIR_LEN_OFFSET: usize = 0x50;
const CHM_ITSF_DATA_OFFSET: usize = 0x58;
const CHM_ITSP_HEADER_LEN: usize = 0x54;
const CHM_PMGL_HEADER_LEN: usize = 0x14;

struct ChmItsfHeader {
    version: u32,
    header_len: usize,
    last_modified: u32,
    lang_id: u32,
    dir_offset: u64,
    dir_len: u64,
    data_offset: u64,
}

impl ChmItsfHeader {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..4) != Some(b"ITSF") {
            return None;
        }
        let version = read_u32(bytes, 4)?;
        let expected_header_len = match version {
            2 => CHM_ITSF_V2_HEADER_LEN,
            3 => CHM_ITSF_V3_HEADER_LEN,
            _ => return None,
        };
        let header_len = usize::try_from(read_u32(bytes, 8)?).ok()?;
        if header_len != expected_header_len || header_len > bytes.len() {
            return None;
        }
        let last_modified = read_u32(bytes, CHM_ITSF_LAST_MODIFIED_OFFSET)?;
        let lang_id = read_u32(bytes, CHM_ITSF_LANG_ID_OFFSET)?;
        let dir_offset = read_u64(bytes, CHM_ITSF_DIR_OFFSET)?;
        let dir_len = read_u64(bytes, CHM_ITSF_DIR_LEN_OFFSET)?;
        let data_offset = match version {
            2 => dir_offset.checked_add(dir_len)?,
            3 => read_u64(bytes, CHM_ITSF_DATA_OFFSET)?,
            _ => unreachable!(),
        };
        Some(Self {
            version,
            header_len,
            last_modified,
            lang_id,
            dir_offset,
            dir_len,
            data_offset,
        })
    }
}

pub(super) fn render_chm_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_CHM_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "chm", size, modified_unix);
    if bytes.starts_with(b"ITSF") {
        text.push_str("\nFormat: Microsoft Compiled HTML Help");
        let itsf = ChmItsfHeader::parse(&bytes);
        if let Some(header) = itsf.as_ref() {
            text.push_str(&format!("\nITSF version: {}", header.version));
            text.push_str(&format!("\nHeader length: {} bytes", header.header_len));
        } else {
            if let Some(version) = read_u32(&bytes, 4) {
                text.push_str(&format!("\nITSF version: {version}"));
            }
            if let Some(header_len) = read_u32(&bytes, 8) {
                text.push_str(&format!("\nHeader length: {header_len} bytes"));
            }
        }
        if let Some(header) = itsf {
            text.push_str(&format!("\nLanguage ID: 0x{:08X}", header.lang_id));
            if header.last_modified > 0 {
                text.push_str(&format!(
                    "\nTimestamp: {}",
                    format_timestamp(i64::from(header.last_modified))
                ));
            }
            if header.dir_offset > 0 {
                text.push_str(&format!("\nDirectory offset: 0x{:016X}", header.dir_offset));
            }
            if header.dir_len > 0 {
                let formatted_len = i64::try_from(header.dir_len)
                    .map(format_bytes)
                    .unwrap_or_else(|_| format!("{} bytes", header.dir_len));
                text.push_str(&format!("\nDirectory length: {formatted_len}"));
            }
            append_chm_itsp_summary(&mut text, &bytes, &header);
        }
    } else {
        text.push_str("\nFormat: CHM-like help file");
    }
    generic_info_json(path, "chm", size, modified_unix, Some(text))
}

fn append_chm_itsp_summary(text: &mut String, bytes: &[u8], itsf: &ChmItsfHeader) {
    let Some(dir_end_u64) = itsf.dir_offset.checked_add(itsf.dir_len) else {
        return;
    };
    let (Ok(dir_offset), Ok(dir_end)) = (
        usize::try_from(itsf.dir_offset),
        usize::try_from(dir_end_u64),
    ) else {
        return;
    };
    let data_offset = usize::try_from(itsf.data_offset).ok();
    let Some(min_header_end) = dir_offset.checked_add(CHM_ITSP_HEADER_LEN) else {
        return;
    };
    if dir_offset == 0
        || min_header_end > dir_end
        || min_header_end > bytes.len()
        || bytes.get(dir_offset..dir_offset + 4) != Some(b"ITSP")
    {
        return;
    }
    let version = read_u32(bytes, dir_offset + 4).unwrap_or(0);
    let header_len = read_u32(bytes, dir_offset + 8).unwrap_or(0);
    let block_len = read_u32(bytes, dir_offset + 16).unwrap_or(0);
    let index_depth = read_u32(bytes, dir_offset + 24).unwrap_or(0);
    let index_root = read_u32(bytes, dir_offset + 28).unwrap_or(0);
    let index_head = read_u32(bytes, dir_offset + 32).unwrap_or(0);
    let block_count = read_u32(bytes, dir_offset + 40).unwrap_or(0);
    let Ok(header_len_usize) = usize::try_from(header_len) else {
        return;
    };
    if version != 1
        || header_len_usize != CHM_ITSP_HEADER_LEN
        || dir_offset
            .checked_add(header_len_usize)
            .is_none_or(|end| end > dir_end || end > bytes.len())
    {
        return;
    }
    text.push_str(&format!(
        "\nITSP version: {version}\nITSP header length: {header_len} bytes\nDirectory block length: {block_len} bytes\nDirectory block count: {block_count}\nDirectory index depth/root/head: {index_depth}/{index_root}/{index_head}"
    ));
    let entries = chm_directory_entries(
        bytes,
        dir_offset,
        dir_end,
        header_len_usize,
        usize::try_from(block_len).unwrap_or(0),
    );
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
        if let Some(data_offset) = data_offset {
            for (label, value) in chm_system_summary(bytes, data_offset, &entries) {
                text.push_str(&format!("\n{label}: {value}"));
            }
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
            format_chm_bytes(self.len)
        )
    }
}

fn format_chm_bytes(value: usize) -> String {
    i64::try_from(value)
        .map(format_bytes)
        .unwrap_or_else(|_| format!("{value} bytes"))
}

fn chm_directory_entries(
    bytes: &[u8],
    dir_offset: usize,
    dir_end: usize,
    header_len: usize,
    block_len: usize,
) -> Vec<ChmDirectoryEntry> {
    if header_len < CHM_ITSP_HEADER_LEN || block_len < CHM_PMGL_HEADER_LEN {
        return Vec::new();
    }
    let Some(block_offset) = dir_offset.checked_add(header_len) else {
        return Vec::new();
    };
    let Some(block_end) = block_offset
        .checked_add(block_len)
        .filter(|end| *end <= dir_end && *end <= bytes.len())
    else {
        return Vec::new();
    };
    if bytes.get(block_offset..block_offset + 4) != Some(b"PMGL") {
        return Vec::new();
    }
    let Some(free_space) =
        read_u32(bytes, block_offset + 4).and_then(|value| usize::try_from(value).ok())
    else {
        return Vec::new();
    };
    let entries_end = block_end.saturating_sub(free_space.min(block_len));
    let Some(mut offset) = block_offset.checked_add(CHM_PMGL_HEADER_LEN) else {
        return Vec::new();
    };
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
            summary.push(format!("{} ({})", entry.name, format_chm_bytes(entry.len)));
        } else if lower.ends_with("/content") && lower.contains("mscompressed") {
            summary.push(format!(
                "compressed content {}",
                format_chm_bytes(entry.len)
            ));
        }
        if summary.len() >= MAX_CHM_COMPRESSED_STREAMS {
            break;
        }
    }
    summary
}

fn chm_system_summary(
    bytes: &[u8],
    data_offset: usize,
    entries: &[ChmDirectoryEntry],
) -> Vec<(&'static str, String)> {
    let Some(system) = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("/#SYSTEM") && entry.section == 0)
    else {
        return Vec::new();
    };
    if system.len == 0 || system.len > MAX_CHM_SYSTEM_STREAM_BYTES {
        return Vec::new();
    }
    let Some(system_offset) = data_offset.checked_add(system.offset) else {
        return Vec::new();
    };
    let Some(system_end) = system_offset
        .checked_add(system.len)
        .filter(|end| *end <= bytes.len())
    else {
        return Vec::new();
    };
    let data = &bytes[system_offset..system_end];
    if data.len() < 4 {
        return Vec::new();
    }
    let mut offset = 4usize;
    let mut fields_scanned = 0usize;
    let mut values = Vec::new();
    while fields_scanned < MAX_CHM_SYSTEM_FIELDS {
        let Some(header_end) = offset.checked_add(4).filter(|end| *end <= data.len()) else {
            break;
        };
        let code = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let len = usize::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]]));
        fields_scanned += 1;
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
            .checked_add(usize::from(byte & 0x7F))?;
        if byte & 0x80 == 0 {
            return Some((value, current));
        }
    }
    None
}

#[cfg(test)]
mod tests;
