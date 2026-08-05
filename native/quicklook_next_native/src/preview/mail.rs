use std::collections::BTreeSet;

use super::{
    base_info_text,
    common::{format_number, format_timestamp, read_u16, read_u32, read_u64},
    file_name, generic_info_json, read_file_prefix,
};

const MAX_MAIL_HEADER_BYTES: usize = 256 * 1024;
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const CFB_MAX_REGULAR_SECTOR: u32 = 0xFFFF_FFFA;
const CFB_DIFAT_SECTOR: u32 = 0xFFFF_FFFC;
const CFB_FAT_SECTOR: u32 = 0xFFFF_FFFD;
const CFB_END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const CFB_FREE_SECTOR: u32 = 0xFFFF_FFFF;
const CFB_NO_STREAM: u32 = 0xFFFF_FFFF;
const MAX_CFB_FAT_SECTORS: usize = 16;
const MAX_CFB_DIFAT_SECTORS: usize = 8;
const MAX_CFB_DIRECTORY_SECTORS: usize = 16;
const MAX_CFB_DIRECTORY_ENTRIES: usize = 256;
const MAX_CFB_MINI_FAT_SECTORS: usize = 16;
const MAX_CFB_MINI_STREAM_BYTES: usize = MAX_MAIL_HEADER_BYTES;
const MAX_CFB_MINI_STREAM_SECTORS: usize = MAX_CFB_MINI_STREAM_BYTES / 512;
const MAX_CFB_TREE_NODES: usize = MAX_CFB_DIRECTORY_ENTRIES;
const MAX_CFB_PROPERTY_SECTORS: usize = 128;
const MAX_CFB_MINI_CHAIN_SECTORS: usize = 1024;
const MAX_MSG_PROPERTY_BYTES: usize = 4 * 1024;
const MAX_MSG_PROPERTIES_STREAM_BYTES: usize = 64 * 1024;
const MAX_MSG_PROPERTY_ENTRIES: usize = 128;
const MAX_MSG_UTF16_UNITS: usize = 512;

pub(super) fn render_mail_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_MAIL_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "mail", size, modified_unix);
    if bytes.starts_with(&CFB_SIGNATURE) {
        text.push_str("\nFormat: Outlook MSG / Compound File");
        append_msg_compound_summary(&mut text, &bytes);
    } else {
        let content = String::from_utf8_lossy(&bytes);
        let headers = parse_mail_headers(&content);
        text.push_str("\nFormat: RFC 5322 message");
        for key in [
            "From",
            "To",
            "Cc",
            "Reply-To",
            "Subject",
            "Date",
            "Message-ID",
            "MIME-Version",
            "Content-Type",
        ] {
            if let Some(value) = headers
                .iter()
                .find_map(|(k, v)| k.eq_ignore_ascii_case(key).then_some(v))
            {
                text.push_str(&format!(
                    "\n{key}: {}",
                    decode_mail_header_value(value).trim()
                ));
            }
        }
        if let Some(content_type) = header_value(&headers, "Content-Type") {
            if let Some(boundary) = mail_header_parameter(content_type, "boundary") {
                text.push_str(&format!("\nMIME boundary: {boundary}"));
                let parts = mail_mime_part_summaries(&content, &boundary);
                if !parts.is_empty() {
                    text.push_str(&format!("\nMIME parts: {}", parts.len()));
                    text.push_str(&format!("\nMIME part details: {}", parts.join("; ")));
                }
            }
        }
        let attachments = content
            .lines()
            .filter(|line| {
                line.to_ascii_lowercase()
                    .contains("content-disposition: attachment")
            })
            .count();
        if attachments > 0 {
            text.push_str(&format!(
                "\nAttachments observed: {}",
                format_number(attachments as i64)
            ));
            let filenames = mail_attachment_filenames(&content);
            if !filenames.is_empty() {
                text.push_str(&format!("\nAttachment names: {}", filenames.join(", ")));
            }
        }
        if filename.to_ascii_lowercase().ends_with(".mbox") {
            let count = content
                .lines()
                .filter(|line| line.starts_with("From "))
                .count();
            text.push_str(&format!(
                "\nMailbox messages observed: {}",
                format_number(count as i64)
            ));
        }
    }
    generic_info_json(path, "mail", size, modified_unix, Some(text))
}

fn parse_mail_headers(content: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    headers
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
}

fn mail_header_parameter(value: &str, name: &str) -> Option<String> {
    mail_header_parameters(value)
        .into_iter()
        .find_map(|(key, raw_value)| key.eq_ignore_ascii_case(name).then_some(raw_value))
        .filter(|value| !value.is_empty())
}

fn mail_header_parameters(value: &str) -> Vec<(String, String)> {
    value
        .split(';')
        .skip(1)
        .filter_map(|part| {
            let (key, raw_value) = part.trim().split_once('=')?;
            Some((
                key.trim().to_string(),
                raw_value.trim().trim_matches('"').trim().to_string(),
            ))
        })
        .collect()
}

fn decode_mail_header_value(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("=?") {
        output.push_str(&rest[..start]);
        let encoded = &rest[start + 2..];
        let Some(charset_end) = encoded.find('?') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let charset = &encoded[..charset_end];
        let encoded = &encoded[charset_end + 1..];
        let Some(encoding_end) = encoded.find('?') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let encoding = &encoded[..encoding_end];
        let encoded = &encoded[encoding_end + 1..];
        let Some(value_end) = encoded.find("?=") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let encoded_value = &encoded[..value_end];
        if let Some(decoded) = decode_rfc2047_word(charset, encoding, encoded_value) {
            output.push_str(&decoded);
        } else {
            output.push_str(
                &rest[start..start + 2 + charset_end + 1 + encoding_end + 1 + value_end + 2],
            );
        }
        rest = &encoded[value_end + 2..];
    }
    output.push_str(rest);
    output
}

fn decode_rfc2047_word(charset: &str, encoding: &str, encoded: &str) -> Option<String> {
    if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
        return None;
    }
    let bytes = if encoding.eq_ignore_ascii_case("q") {
        let mut bytes = Vec::new();
        let mut chars = encoded.as_bytes().iter().copied();
        while let Some(byte) = chars.next() {
            match byte {
                b'_' => bytes.push(b' '),
                b'=' => {
                    let hi = chars.next()?;
                    let lo = chars.next()?;
                    bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
                }
                _ => bytes.push(byte),
            }
        }
        bytes
    } else if encoding.eq_ignore_ascii_case("b") {
        decode_base64(encoded)?
    } else {
        return None;
    };
    String::from_utf8(bytes).ok()
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut bytes = Vec::new();
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let sextet = base64_value(byte)? as u32;
        bits = (bits << 6) | sextet;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            bytes.push(((bits >> bit_count) & 0xFF) as u8);
        }
    }
    Some(bytes)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn mail_attachment_filenames(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|line| {
            line.to_ascii_lowercase()
                .contains("content-disposition: attachment")
        })
        .filter_map(mail_attachment_filename_from_disposition)
        .map(|name| decode_mail_header_value(&name))
        .take(5)
        .collect()
}

fn mail_mime_part_summaries(content: &str, boundary: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    mail_mime_part_summaries_inner(content, boundary, 0, &mut summaries);
    summaries
}

fn mail_mime_part_summaries_inner(
    content: &str,
    boundary: &str,
    depth: usize,
    summaries: &mut Vec<String>,
) {
    if depth > 4 || summaries.len() >= 32 {
        return;
    }
    let marker = format!("--{boundary}");
    for part in content.split(&marker).skip(1).take(32) {
        let trimmed = part.trim_start_matches(['\r', '\n']);
        if summaries.len() >= 32 || trimmed.starts_with("--") || trimmed.trim().is_empty() {
            continue;
        }
        let (header_text, body) = trimmed
            .split_once("\r\n\r\n")
            .or_else(|| trimmed.split_once("\n\n"))
            .unwrap_or((trimmed, ""));
        let headers = parse_mail_headers(header_text);
        let content_type_header = header_value(&headers, "Content-Type");
        let content_type = content_type_header
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text/plain".to_string());
        let disposition = header_value(&headers, "Content-Disposition")
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .filter(|value| !value.is_empty());
        let filename = header_value(&headers, "Content-Disposition")
            .and_then(mail_attachment_filename_from_disposition);
        let encoding = header_value(&headers, "Content-Transfer-Encoding")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let is_text_plain = content_type.eq_ignore_ascii_case("text/plain");
        let is_multipart = content_type.to_ascii_lowercase().starts_with("multipart/");
        let mut summary = if depth == 0 {
            content_type
        } else {
            format!("{}{}", ">".repeat(depth), content_type)
        };
        if let Some(disposition) = disposition {
            summary.push_str(&format!(" ({disposition})"));
        }
        if let Some(filename) = filename {
            summary.push_str(&format!(" filename={filename}"));
        }
        if let Some(encoding) = &encoding {
            summary.push_str(&format!(" encoding={encoding}"));
        }
        let body_len = body.trim_matches(|ch| ch == '\r' || ch == '\n').len();
        summary.push_str(&format!(" body={body_len} bytes"));
        if let Some(decoded_len) = encoding
            .as_deref()
            .and_then(|encoding| mail_decoded_body_len(body, encoding))
        {
            summary.push_str(&format!(" decoded={decoded_len} bytes"));
        }
        if is_text_plain {
            if let Some(preview) = mail_text_body_preview(body, encoding.as_deref()) {
                summary.push_str(&format!(" preview=\"{preview}\""));
            }
        }
        summaries.push(summary);
        if is_multipart {
            if let Some(child_boundary) =
                content_type_header.and_then(|value| mail_header_parameter(value, "boundary"))
            {
                mail_mime_part_summaries_inner(body, &child_boundary, depth + 1, summaries);
            }
        }
    }
}

fn mail_text_body_preview(body: &str, encoding: Option<&str>) -> Option<String> {
    let trimmed = body.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.is_empty() || trimmed.len() > 1024 * 1024 {
        return None;
    }
    let text = if encoding.is_some_and(|value| value.eq_ignore_ascii_case("base64")) {
        String::from_utf8_lossy(&decode_base64(trimmed)?).to_string()
    } else if encoding.is_some_and(|value| value.eq_ignore_ascii_case("quoted-printable")) {
        String::from_utf8_lossy(&decode_quoted_printable(trimmed.as_bytes())?).to_string()
    } else {
        trimmed.to_string()
    };
    let preview = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect::<String>();
    (!preview.is_empty()).then_some(preview)
}

fn mail_decoded_body_len(body: &str, encoding: &str) -> Option<usize> {
    let trimmed = body.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.len() > 1024 * 1024 {
        return None;
    }
    if encoding.eq_ignore_ascii_case("base64") {
        decode_base64(trimmed).map(|bytes| bytes.len())
    } else if encoding.eq_ignore_ascii_case("quoted-printable") {
        quoted_printable_decoded_len(trimmed.as_bytes())
    } else {
        None
    }
}

fn quoted_printable_decoded_len(bytes: &[u8]) -> Option<usize> {
    decode_quoted_printable(bytes).map(|bytes| bytes.len())
}

fn decode_quoted_printable(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'=' {
            if bytes.get(index + 1) == Some(&b'\r') && bytes.get(index + 2) == Some(&b'\n') {
                index += 3;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
                continue;
            }
            if index + 2 < bytes.len()
                && hex_nibble(bytes[index + 1]).is_some()
                && hex_nibble(bytes[index + 2]).is_some()
            {
                output.push((hex_nibble(bytes[index + 1])? << 4) | hex_nibble(bytes[index + 2])?);
                if output.len() > 1024 * 1024 {
                    return None;
                }
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        if output.len() > 1024 * 1024 {
            return None;
        }
        index += 1;
    }
    Some(output)
}

fn mail_attachment_filename_from_disposition(line: &str) -> Option<String> {
    if let Some(value) = mail_header_parameter(line, "filename") {
        return Some(value);
    }
    let parameters = mail_header_parameters(line);
    if let Some(value) = parameters
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case("filename*").then_some(value))
    {
        return decode_rfc2231_value(value);
    }

    let mut joined = String::new();
    for index in 0..32 {
        let encoded_key = format!("filename*{index}*");
        let plain_key = format!("filename*{index}");
        if let Some(value) = parameters.iter().find_map(|(key, value)| {
            (key.eq_ignore_ascii_case(&encoded_key) || key.eq_ignore_ascii_case(&plain_key))
                .then_some(value)
        }) {
            joined.push_str(value);
        } else {
            break;
        }
    }
    (!joined.is_empty()).then(|| decode_rfc2231_value(&joined).unwrap_or(joined))
}

fn decode_rfc2231_value(value: &str) -> Option<String> {
    let encoded = if let Some((charset, rest)) = value.split_once('\'') {
        let (_, encoded) = rest.split_once('\'')?;
        if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("us-ascii") {
            return None;
        }
        encoded
    } else {
        value
    };
    String::from_utf8(percent_decode(encoded)?).ok()
}

fn percent_decode(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut iter = value.as_bytes().iter().copied();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let hi = iter.next()?;
            let lo = iter.next()?;
            bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
        } else {
            bytes.push(byte);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
struct CfbHeader {
    major_version: u16,
    sector_size: usize,
    mini_sector_size: usize,
    directory_sector_count: usize,
    fat_sector_count: usize,
    first_directory_sector: u32,
    mini_stream_cutoff: usize,
    first_mini_fat_sector: u32,
    mini_fat_sector_count: usize,
    first_difat_sector: u32,
    difat_sector_count: usize,
}

impl CfbHeader {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..CFB_SIGNATURE.len()) != Some(CFB_SIGNATURE.as_slice()) || bytes.len() < 512 {
            return None;
        }
        let major_version = read_u16(bytes, 26)?;
        let sector_shift = read_u16(bytes, 30)?;
        let expected_sector_shift = match major_version {
            3 => 9,
            4 => 12,
            _ => return None,
        };
        if read_u16(bytes, 28)? != 0xFFFE
            || sector_shift != expected_sector_shift
            || read_u16(bytes, 32)? != 6
        {
            return None;
        }
        let sector_size = 1usize.checked_shl(u32::from(sector_shift))?;
        if bytes.len() < sector_size {
            return None;
        }
        let directory_sector_count = usize::try_from(read_u32(bytes, 40)?).ok()?;
        if (major_version == 3 && directory_sector_count != 0)
            || directory_sector_count > MAX_CFB_DIRECTORY_SECTORS
            || (major_version == 4 && directory_sector_count == 0)
        {
            return None;
        }
        let fat_sector_count = usize::try_from(read_u32(bytes, 44)?).ok()?;
        if fat_sector_count == 0 || fat_sector_count > MAX_CFB_FAT_SECTORS {
            return None;
        }
        let first_directory_sector = read_u32(bytes, 48)?;
        if !cfb_is_regular_sector(first_directory_sector) {
            return None;
        }
        let mini_stream_cutoff = usize::try_from(read_u32(bytes, 56)?).ok()?;
        if mini_stream_cutoff != 4096 {
            return None;
        }
        let first_mini_fat_sector = read_u32(bytes, 60)?;
        let mini_fat_sector_count = usize::try_from(read_u32(bytes, 64)?).ok()?;
        if mini_fat_sector_count > MAX_CFB_MINI_FAT_SECTORS
            || (mini_fat_sector_count > 0 && !cfb_is_regular_sector(first_mini_fat_sector))
            || (mini_fat_sector_count == 0
                && !matches!(first_mini_fat_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR))
        {
            return None;
        }
        let first_difat_sector = read_u32(bytes, 68)?;
        let difat_sector_count = usize::try_from(read_u32(bytes, 72)?).ok()?;
        if difat_sector_count > MAX_CFB_DIFAT_SECTORS
            || (difat_sector_count > 0 && !cfb_is_regular_sector(first_difat_sector))
            || (difat_sector_count == 0
                && !matches!(first_difat_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR))
        {
            return None;
        }
        Some(Self {
            major_version,
            sector_size,
            mini_sector_size: 64,
            directory_sector_count,
            fat_sector_count,
            first_directory_sector,
            mini_stream_cutoff,
            first_mini_fat_sector,
            mini_fat_sector_count,
            first_difat_sector,
            difat_sector_count,
        })
    }
}

#[derive(Clone)]
struct CfbDirectoryEntry {
    name: String,
    object_type: u8,
    left_sibling: u32,
    right_sibling: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

impl CfbDirectoryEntry {
    fn empty() -> Self {
        Self {
            name: String::new(),
            object_type: 0,
            left_sibling: CFB_NO_STREAM,
            right_sibling: CFB_NO_STREAM,
            child: CFB_NO_STREAM,
            start_sector: CFB_END_OF_CHAIN,
            size: 0,
        }
    }
}

struct CfbDocument<'a> {
    bytes: &'a [u8],
    header: CfbHeader,
    fat: Vec<u32>,
    entries: Vec<CfbDirectoryEntry>,
    root_children: Vec<usize>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
}

impl<'a> CfbDocument<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = CfbHeader::parse(bytes)?;
        let fat = cfb_read_fat(bytes, &header)?;
        let directory_bytes = cfb_read_regular_chain(
            bytes,
            &header,
            &fat,
            header.first_directory_sector,
            None,
            MAX_CFB_DIRECTORY_SECTORS,
            MAX_CFB_DIRECTORY_SECTORS.checked_mul(header.sector_size)?,
        )?;
        if header.major_version == 4
            && directory_bytes.len()
                != header
                    .directory_sector_count
                    .checked_mul(header.sector_size)?
        {
            return None;
        }
        let entries = cfb_parse_directory_entries(&directory_bytes, header.major_version)?;
        let root = entries.first()?;
        if root.object_type != 5 || !root.name.eq_ignore_ascii_case("Root Entry") {
            return None;
        }
        let root_children = cfb_tree_children(&entries, root.child)?;
        let (mini_fat, mini_stream) = if header.mini_fat_sector_count == 0 {
            if root.size != 0 {
                return None;
            }
            (Vec::new(), Vec::new())
        } else {
            let mini_fat_bytes = header
                .mini_fat_sector_count
                .checked_mul(header.sector_size)?;
            let raw_mini_fat = cfb_read_regular_chain(
                bytes,
                &header,
                &fat,
                header.first_mini_fat_sector,
                Some(mini_fat_bytes),
                MAX_CFB_MINI_FAT_SECTORS,
                MAX_CFB_MINI_FAT_SECTORS.checked_mul(header.sector_size)?,
            )?;
            let mini_fat = raw_mini_fat
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect::<Vec<_>>();
            let mini_stream_bytes = usize::try_from(root.size).ok()?;
            if mini_stream_bytes > MAX_CFB_MINI_STREAM_BYTES {
                return None;
            }
            let mini_stream = cfb_read_regular_chain(
                bytes,
                &header,
                &fat,
                root.start_sector,
                Some(mini_stream_bytes),
                MAX_CFB_MINI_STREAM_SECTORS,
                MAX_CFB_MINI_STREAM_BYTES,
            )?;
            (mini_fat, mini_stream)
        };
        Some(Self {
            bytes,
            header,
            fat,
            entries,
            root_children,
            mini_fat,
            mini_stream,
        })
    }

    fn root_entry_named(&self, name: &str) -> Option<&CfbDirectoryEntry> {
        self.root_children.iter().find_map(|index| {
            self.entries
                .get(*index)
                .filter(|entry| entry.name.eq_ignore_ascii_case(name))
        })
    }

    fn read_root_stream(&self, name: &str, max_bytes: usize) -> Option<Vec<u8>> {
        let entry = self.root_entry_named(name)?;
        if entry.object_type != 2 {
            return None;
        }
        let size = usize::try_from(entry.size).ok()?;
        if size > max_bytes {
            return None;
        }
        if size == 0 {
            return Some(Vec::new());
        }
        if size < self.header.mini_stream_cutoff {
            return cfb_read_mini_chain(
                &self.mini_stream,
                &self.mini_fat,
                self.header.mini_sector_size,
                entry.start_sector,
                size,
                MAX_CFB_MINI_CHAIN_SECTORS,
            );
        }
        cfb_read_regular_chain(
            self.bytes,
            &self.header,
            &self.fat,
            entry.start_sector,
            Some(size),
            MAX_CFB_PROPERTY_SECTORS,
            max_bytes,
        )
    }
}

fn append_msg_compound_summary(text: &mut String, bytes: &[u8]) {
    let Some(document) = CfbDocument::parse(bytes) else {
        return;
    };
    let attachments = document
        .root_children
        .iter()
        .filter_map(|index| document.entries.get(*index))
        .filter(|entry| {
            entry.object_type == 1
                && entry
                    .name
                    .to_ascii_lowercase()
                    .starts_with("__attach_version1.0_")
        })
        .count();
    let recipients = document
        .root_children
        .iter()
        .filter_map(|index| document.entries.get(*index))
        .filter(|entry| {
            entry.object_type == 1
                && entry
                    .name
                    .to_ascii_lowercase()
                    .starts_with("__recip_version1.0_")
        })
        .count();
    if recipients > 0 {
        text.push_str(&format!("\nRecipients: {recipients}"));
    }
    if attachments > 0 {
        text.push_str(&format!("\nAttachments: {attachments}"));
    }
    for (label, property) in [
        ("Subject", "0037001F"),
        ("Sender", "0C1A001F"),
        ("Recipients display", "0E04001F"),
    ] {
        if let Some(value) = msg_unicode_property(&document, property) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
    if let Some(sent_time) = msg_fixed_filetime_property(&document, 0x0E06)
        .or_else(|| msg_fixed_filetime_property(&document, 0x0039))
        .or_else(|| msg_filetime_stream_property(&document, "0E060040"))
    {
        text.push_str(&format!("\nSent time: {sent_time}"));
    }
    let has_body = document.root_children.iter().any(|index| {
        document.entries.get(*index).is_some_and(|entry| {
            entry.object_type == 2
                && (entry.name.eq_ignore_ascii_case("__substg1.0_1000001F")
                    || entry.name.eq_ignore_ascii_case("__substg1.0_10090102"))
        })
    });
    if has_body {
        text.push_str("\nBody available: yes");
    }
}

fn cfb_is_regular_sector(sector: u32) -> bool {
    sector <= CFB_MAX_REGULAR_SECTOR
}

fn cfb_sector_bytes<'a>(bytes: &'a [u8], header: &CfbHeader, sector: u32) -> Option<&'a [u8]> {
    if !cfb_is_regular_sector(sector) {
        return None;
    }
    let sector_index = usize::try_from(sector).ok()?.checked_add(1)?;
    let offset = sector_index.checked_mul(header.sector_size)?;
    bytes.get(offset..offset.checked_add(header.sector_size)?)
}

fn cfb_read_fat(bytes: &[u8], header: &CfbHeader) -> Option<Vec<u32>> {
    let mut fat_sector_ids = Vec::with_capacity(header.fat_sector_count);
    let mut seen_fat_sectors = BTreeSet::new();
    for index in 0..109usize {
        let offset = 76usize.checked_add(index.checked_mul(4)?)?;
        let sector = read_u32(bytes, offset)?;
        if sector == CFB_FREE_SECTOR {
            continue;
        }
        if fat_sector_ids.len() >= header.fat_sector_count
            || !cfb_is_regular_sector(sector)
            || !seen_fat_sectors.insert(sector)
        {
            return None;
        }
        fat_sector_ids.push(sector);
    }

    let mut difat_sector_ids = Vec::with_capacity(header.difat_sector_count);
    let mut seen_difat_sectors = BTreeSet::new();
    let mut current_difat = header.first_difat_sector;
    for _ in 0..header.difat_sector_count {
        if !cfb_is_regular_sector(current_difat) || !seen_difat_sectors.insert(current_difat) {
            return None;
        }
        difat_sector_ids.push(current_difat);
        let sector_bytes = cfb_sector_bytes(bytes, header, current_difat)?;
        let entry_count = header.sector_size.checked_div(4)?.checked_sub(1)?;
        for index in 0..entry_count {
            let sector = read_u32(sector_bytes, index.checked_mul(4)?)?;
            if sector == CFB_FREE_SECTOR {
                continue;
            }
            if fat_sector_ids.len() >= header.fat_sector_count
                || !cfb_is_regular_sector(sector)
                || !seen_fat_sectors.insert(sector)
            {
                return None;
            }
            fat_sector_ids.push(sector);
        }
        current_difat = read_u32(sector_bytes, header.sector_size.checked_sub(4)?)?;
    }
    if current_difat != CFB_END_OF_CHAIN || fat_sector_ids.len() != header.fat_sector_count {
        return None;
    }

    let entries_per_sector = header.sector_size.checked_div(4)?;
    let mut fat = Vec::with_capacity(header.fat_sector_count.checked_mul(entries_per_sector)?);
    for sector in &fat_sector_ids {
        let sector_bytes = cfb_sector_bytes(bytes, header, *sector)?;
        fat.extend(
            sector_bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
        );
    }
    for sector in fat_sector_ids {
        if fat.get(usize::try_from(sector).ok()?) != Some(&CFB_FAT_SECTOR) {
            return None;
        }
    }
    for sector in difat_sector_ids {
        if fat.get(usize::try_from(sector).ok()?) != Some(&CFB_DIFAT_SECTOR) {
            return None;
        }
    }
    Some(fat)
}

fn cfb_read_regular_chain(
    bytes: &[u8],
    header: &CfbHeader,
    fat: &[u32],
    start_sector: u32,
    expected_bytes: Option<usize>,
    max_sectors: usize,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    if expected_bytes == Some(0) {
        return matches!(start_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR).then(Vec::new);
    }
    if expected_bytes.is_some_and(|size| size > max_bytes) {
        return None;
    }
    let required_sectors = if let Some(size) = expected_bytes {
        Some(
            size.checked_add(header.sector_size.checked_sub(1)?)?
                .checked_div(header.sector_size)?,
        )
    } else {
        None
    };
    if required_sectors.is_some_and(|count| count == 0 || count > max_sectors) {
        return None;
    }
    let mut output = Vec::with_capacity(expected_bytes.unwrap_or(0).min(max_bytes));
    let mut visited = BTreeSet::new();
    let mut current = start_sector;
    for index in 0..max_sectors {
        if !cfb_is_regular_sector(current) || !visited.insert(current) {
            return None;
        }
        let sector_bytes = cfb_sector_bytes(bytes, header, current)?;
        let take = expected_bytes
            .map(|size| size.saturating_sub(output.len()).min(header.sector_size))
            .unwrap_or(header.sector_size);
        if output.len().checked_add(take)? > max_bytes {
            return None;
        }
        output.extend_from_slice(sector_bytes.get(..take)?);
        let next = *fat.get(usize::try_from(current).ok()?)?;
        if let Some(required) = required_sectors {
            if index + 1 == required {
                return (next == CFB_END_OF_CHAIN).then_some(output);
            }
            if index + 1 > required || !cfb_is_regular_sector(next) {
                return None;
            }
        } else if next == CFB_END_OF_CHAIN {
            return Some(output);
        } else if !cfb_is_regular_sector(next) {
            return None;
        }
        current = next;
    }
    None
}

fn cfb_parse_directory_entries(bytes: &[u8], major_version: u16) -> Option<Vec<CfbDirectoryEntry>> {
    let mut entries = Vec::new();
    for chunk in bytes.chunks_exact(128).take(MAX_CFB_DIRECTORY_ENTRIES) {
        let object_type = *chunk.get(66)?;
        if object_type == 0 {
            entries.push(CfbDirectoryEntry::empty());
            continue;
        }
        if !matches!(object_type, 1 | 2 | 5) {
            return None;
        }
        let name_len = usize::from(read_u16(chunk, 64)?);
        if !(2..=64).contains(&name_len)
            || name_len % 2 != 0
            || chunk.get(name_len.checked_sub(2)?..name_len) != Some([0, 0].as_slice())
        {
            return None;
        }
        let units = chunk
            .get(..name_len.checked_sub(2)?)?
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let name = String::from_utf16(&units).ok()?;
        if name.is_empty() {
            return None;
        }
        let size = if major_version == 3 {
            u64::from(read_u32(chunk, 120)?)
        } else {
            read_u64(chunk, 120)?
        };
        entries.push(CfbDirectoryEntry {
            name,
            object_type,
            left_sibling: read_u32(chunk, 68)?,
            right_sibling: read_u32(chunk, 72)?,
            child: read_u32(chunk, 76)?,
            start_sector: read_u32(chunk, 116)?,
            size,
        });
    }
    (!entries.is_empty()).then_some(entries)
}

fn cfb_tree_children(entries: &[CfbDirectoryEntry], root: u32) -> Option<Vec<usize>> {
    if root == CFB_NO_STREAM {
        return Some(Vec::new());
    }
    let mut children = Vec::new();
    let mut stack = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(stream_id) = stack.pop() {
        if stream_id == CFB_NO_STREAM {
            continue;
        }
        if children.len() >= MAX_CFB_TREE_NODES || !visited.insert(stream_id) {
            return None;
        }
        let index = usize::try_from(stream_id).ok()?;
        if index == 0 {
            return None;
        }
        let entry = entries.get(index)?;
        if entry.object_type == 0 {
            return None;
        }
        children.push(index);
        stack.push(entry.right_sibling);
        stack.push(entry.left_sibling);
    }
    Some(children)
}

fn cfb_read_mini_chain(
    mini_stream: &[u8],
    mini_fat: &[u32],
    mini_sector_size: usize,
    start_sector: u32,
    expected_bytes: usize,
    max_sectors: usize,
) -> Option<Vec<u8>> {
    if expected_bytes == 0 {
        return matches!(start_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR).then(Vec::new);
    }
    let required_sectors = expected_bytes
        .checked_add(mini_sector_size.checked_sub(1)?)?
        .checked_div(mini_sector_size)?;
    if required_sectors == 0 || required_sectors > max_sectors {
        return None;
    }
    let mut output = Vec::with_capacity(expected_bytes);
    let mut visited = BTreeSet::new();
    let mut current = start_sector;
    for index in 0..required_sectors {
        if !cfb_is_regular_sector(current) || !visited.insert(current) {
            return None;
        }
        let mini_index = usize::try_from(current).ok()?;
        let offset = mini_index.checked_mul(mini_sector_size)?;
        let take = expected_bytes
            .saturating_sub(output.len())
            .min(mini_sector_size);
        output.extend_from_slice(mini_stream.get(offset..offset.checked_add(take)?)?);
        let next = *mini_fat.get(mini_index)?;
        if index + 1 == required_sectors {
            return (next == CFB_END_OF_CHAIN).then_some(output);
        }
        if !cfb_is_regular_sector(next) {
            return None;
        }
        current = next;
    }
    None
}

fn msg_unicode_property(document: &CfbDocument<'_>, property: &str) -> Option<String> {
    let stream =
        document.read_root_stream(&format!("__substg1.0_{property}"), MAX_MSG_PROPERTY_BYTES)?;
    if stream.len() % 2 != 0 {
        return None;
    }
    let units = stream
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take(MAX_MSG_UTF16_UNITS)
        .collect::<Vec<_>>();
    let value = String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn msg_fixed_filetime_property(document: &CfbDocument<'_>, property_id: u16) -> Option<String> {
    let stream =
        document.read_root_stream("__properties_version1.0", MAX_MSG_PROPERTIES_STREAM_BYTES)?;
    let entries = stream.get(32..)?;
    let expected_tag = (u32::from(property_id) << 16) | 0x0040;
    for entry in entries.chunks_exact(16).take(MAX_MSG_PROPERTY_ENTRIES) {
        if read_u32(entry, 0)? == expected_tag {
            return format_msg_filetime(read_u64(entry, 8)?);
        }
    }
    None
}

fn msg_filetime_stream_property(document: &CfbDocument<'_>, property: &str) -> Option<String> {
    let stream =
        document.read_root_stream(&format!("__substg1.0_{property}"), MAX_MSG_PROPERTY_BYTES)?;
    format_msg_filetime(read_u64(&stream, 0)?)
}

fn format_msg_filetime(filetime: u64) -> Option<String> {
    const WINDOWS_TO_UNIX_TICKS: u64 = 116_444_736_000_000_000;
    let unix = filetime.checked_sub(WINDOWS_TO_UNIX_TICKS)? / 10_000_000;
    Some(format_timestamp(i64::try_from(unix).ok()?))
}
