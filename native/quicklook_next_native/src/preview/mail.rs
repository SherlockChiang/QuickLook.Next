use std::fs;
use std::io::{Read, Seek, SeekFrom};

use super::{
    base_info_text, common::format_number, file_name, generic_info_json, prepare_seekable_reader,
    preview_cancelled, read_reader_prefix_cancelable, ReaderPreviewError,
};

mod cfb;

#[cfg(test)]
use cfb::{CFB_END_OF_CHAIN, CFB_FAT_SECTOR, CFB_FREE_SECTOR};

const MAX_MAIL_HEADER_BYTES: usize = 256 * 1024;
const MAX_MAIL_HEADERS: usize = 128;
const MAX_MAIL_HEADER_NAME_BYTES: usize = 128;
const MAX_MAIL_HEADER_VALUE_BYTES: usize = 8 * 1024;
const MAX_MAIL_HEADER_PARAMETERS: usize = 64;
const MAX_MAIL_PARAMETER_KEY_BYTES: usize = 64;
const MAX_MAIL_PARAMETER_VALUE_BYTES: usize = 1024;
const MAX_MAIL_ENCODED_WORDS: usize = 64;
const MAX_MAIL_DECODED_HEADER_BYTES: usize = 8 * 1024;
const MAX_MAIL_ATTACHMENT_NAMES: usize = 5;
const MAX_MAIL_FILENAME_SEGMENTS: usize = 32;
const MAX_MAIL_FILENAME_BYTES: usize = 512;
// The root is depth zero, so four nested multipart levels are accepted.
const MAX_MAIL_MIME_DEPTH: usize = 4;
const MAX_MAIL_MIME_PARTS: usize = 32;
const MAX_MAIL_MIME_BOUNDARY_BYTES: usize = 200;
const MAX_MAIL_MIME_FIELD_BYTES: usize = 512;
const MAX_MAIL_DECODED_BODY_BYTES: usize = 1024 * 1024;
const MAX_MAIL_TEXT_PREVIEW_CHARS: usize = 120;
const MAX_MAIL_HANDLE_INPUT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn render_mail_info(path: &str, size: i64, modified_unix: i64) -> String {
    let fallback = || generic_mail_info(path, size, modified_unix);
    let Ok(file) = fs::File::open(path) else {
        return fallback();
    };
    let Ok(source_len) = file.metadata().map(|metadata| metadata.len()) else {
        return fallback();
    };
    render_mail_reader(file, path, source_len, modified_unix, None).unwrap_or_else(|_| fallback())
}

pub(crate) fn render_mail_reader<R: Read + Seek>(
    mut reader: R,
    logical_name: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_MAIL_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    prepare_seekable_reader(&mut reader, source_len, cancel_cb)?;
    let signature =
        read_reader_prefix_cancelable(&mut reader, cfb::CFB_SIGNATURE.len(), cancel_cb)?;
    let size = i64::try_from(source_len).map_err(|_| ReaderPreviewError::LengthMismatch)?;
    let filename = file_name(logical_name);
    let mut text = base_info_text(filename, "mail", size, modified_unix);
    if signature.starts_with(&cfb::CFB_SIGNATURE) {
        text.push_str("\nFormat: Outlook MSG / Compound File");
        cfb::append_msg_compound_summary(&mut text, &mut reader, source_len, cancel_cb)?;
    } else {
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| reader_error(cancel_cb))?;
        let bytes = read_reader_prefix_cancelable(&mut reader, MAX_MAIL_HEADER_BYTES, cancel_cb)?;
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
                if mail_mime_boundary_is_valid(&boundary) {
                    text.push_str(&format!("\nMIME boundary: {boundary}"));
                    let parts = mail_mime_part_summaries(&content, &boundary);
                    if !parts.is_empty() {
                        text.push_str(&format!("\nMIME parts: {}", parts.len()));
                        text.push_str(&format!("\nMIME part details: {}", parts.join("; ")));
                    }
                }
            }
        }
        let (attachments, filenames) = mail_attachment_summary(&content);
        if attachments > 0 {
            text.push_str(&format!(
                "\nAttachments observed: {}",
                format_number(i64::try_from(attachments).unwrap_or(i64::MAX))
            ));
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
                format_number(i64::try_from(count).unwrap_or(i64::MAX))
            ));
        }
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    Ok(generic_info_json(
        logical_name,
        "mail",
        size,
        modified_unix,
        Some(text),
    ))
}

fn generic_mail_info(path: &str, size: i64, modified_unix: i64) -> String {
    let text = format!(
        "{}\nFormat: mail",
        base_info_text(file_name(path), "mail", size, modified_unix)
    );
    generic_info_json(path, "mail", size, modified_unix, Some(text))
}

fn reader_error(cancel_cb: Option<extern "C" fn() -> bool>) -> ReaderPreviewError {
    if preview_cancelled(cancel_cb) {
        ReaderPreviewError::Cancelled
    } else {
        ReaderPreviewError::Io
    }
}

fn bounded_mail_text(value: &str, max_bytes: usize) -> String {
    let mut output = String::new();
    push_bounded_mail_text(&mut output, value, max_bytes);
    output
}

fn push_bounded_mail_text(output: &mut String, value: &str, max_bytes: usize) {
    let remaining = max_bytes.saturating_sub(output.len());
    if remaining == 0 {
        return;
    }
    let mut end = value.len().min(remaining);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&value[..end]);
}

fn parse_mail_headers(content: &str) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    let mut can_continue = false;
    for line in content.lines() {
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if can_continue {
                if let Some((_, value)) = headers.last_mut() {
                    push_bounded_mail_text(value, " ", MAX_MAIL_HEADER_VALUE_BYTES);
                    push_bounded_mail_text(value, line.trim(), MAX_MAIL_HEADER_VALUE_BYTES);
                }
            }
            continue;
        }
        can_continue = false;
        if headers.len() >= MAX_MAIL_HEADERS {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            if name.is_empty() || name.len() > MAX_MAIL_HEADER_NAME_BYTES {
                continue;
            }
            headers.push((
                bounded_mail_text(name, MAX_MAIL_HEADER_NAME_BYTES),
                bounded_mail_text(value.trim(), MAX_MAIL_HEADER_VALUE_BYTES),
            ));
            can_continue = true;
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
        .take(MAX_MAIL_HEADER_PARAMETERS)
        .filter_map(|part| {
            let (key, raw_value) = part.trim().split_once('=')?;
            let key = key.trim();
            if key.is_empty() || key.len() > MAX_MAIL_PARAMETER_KEY_BYTES {
                return None;
            }
            Some((
                bounded_mail_text(key, MAX_MAIL_PARAMETER_KEY_BYTES),
                bounded_mail_text(
                    raw_value.trim().trim_matches('"').trim(),
                    MAX_MAIL_PARAMETER_VALUE_BYTES,
                ),
            ))
        })
        .collect()
}

fn decode_mail_header_value(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    let mut encoded_words = 0usize;
    while let Some(start) = rest.find("=?") {
        push_bounded_mail_text(&mut output, &rest[..start], MAX_MAIL_DECODED_HEADER_BYTES);
        let encoded = &rest[start + 2..];
        let Some(charset_end) = encoded.find('?') else {
            push_bounded_mail_text(&mut output, &rest[start..], MAX_MAIL_DECODED_HEADER_BYTES);
            return output;
        };
        let charset = &encoded[..charset_end];
        let encoded = &encoded[charset_end + 1..];
        let Some(encoding_end) = encoded.find('?') else {
            push_bounded_mail_text(&mut output, &rest[start..], MAX_MAIL_DECODED_HEADER_BYTES);
            return output;
        };
        let encoding = &encoded[..encoding_end];
        let encoded = &encoded[encoding_end + 1..];
        let Some(value_end) = encoded.find("?=") else {
            push_bounded_mail_text(&mut output, &rest[start..], MAX_MAIL_DECODED_HEADER_BYTES);
            return output;
        };
        let encoded_value = &encoded[..value_end];
        if encoded_words >= MAX_MAIL_ENCODED_WORDS {
            push_bounded_mail_text(&mut output, &rest[start..], MAX_MAIL_DECODED_HEADER_BYTES);
            return output;
        }
        if let Some(decoded) = decode_rfc2047_word(charset, encoding, encoded_value) {
            push_bounded_mail_text(&mut output, &decoded, MAX_MAIL_DECODED_HEADER_BYTES);
        } else {
            push_bounded_mail_text(
                &mut output,
                &rest[start..start + 2 + charset_end + 1 + encoding_end + 1 + value_end + 2],
                MAX_MAIL_DECODED_HEADER_BYTES,
            );
        }
        encoded_words += 1;
        rest = &encoded[value_end + 2..];
    }
    push_bounded_mail_text(&mut output, rest, MAX_MAIL_DECODED_HEADER_BYTES);
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
                b'_' => {
                    if bytes.len() >= MAX_MAIL_DECODED_HEADER_BYTES {
                        return None;
                    }
                    bytes.push(b' ');
                }
                b'=' => {
                    let hi = chars.next()?;
                    let lo = chars.next()?;
                    if bytes.len() >= MAX_MAIL_DECODED_HEADER_BYTES {
                        return None;
                    }
                    bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
                }
                _ => {
                    if bytes.len() >= MAX_MAIL_DECODED_HEADER_BYTES {
                        return None;
                    }
                    bytes.push(byte);
                }
            }
        }
        bytes
    } else if encoding.eq_ignore_ascii_case("b") {
        decode_base64(encoded, MAX_MAIL_DECODED_HEADER_BYTES)?
    } else {
        return None;
    };
    String::from_utf8(bytes).ok()
}

fn decode_base64(value: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let capacity = value
        .len()
        .saturating_mul(3)
        .saturating_div(4)
        .min(max_bytes);
    let mut bytes = Vec::with_capacity(capacity);
    decode_base64_into(value, max_bytes, |byte| bytes.push(byte))?;
    Some(bytes)
}

fn decode_base64_into(value: &str, max_bytes: usize, mut emit: impl FnMut(u8)) -> Option<usize> {
    let mut quartet = [0u8; 4];
    let mut quartet_len = 0usize;
    let mut decoded_len = 0usize;
    let mut finished = false;
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if finished {
            return None;
        }
        quartet[quartet_len] = if byte == b'=' {
            64
        } else {
            if quartet[..quartet_len].contains(&64) {
                return None;
            }
            base64_value(byte)?
        };
        quartet_len += 1;
        if quartet_len < quartet.len() {
            continue;
        }
        if quartet[0] >= 64 || quartet[1] >= 64 {
            return None;
        }
        let output_count = match (quartet[2], quartet[3]) {
            (64, 64) if quartet[1] & 0x0F == 0 => 1,
            (value, 64) if value < 64 && value & 0x03 == 0 => 2,
            (value, tail) if value < 64 && tail < 64 => 3,
            _ => return None,
        };
        if decoded_len.checked_add(output_count)? > max_bytes {
            return None;
        }
        emit((quartet[0] << 2) | (quartet[1] >> 4));
        if output_count >= 2 {
            emit((quartet[1] << 4) | (quartet[2] >> 2));
        }
        if output_count == 3 {
            emit((quartet[2] << 6) | quartet[3]);
        }
        decoded_len += output_count;
        finished = output_count < 3;
        quartet_len = 0;
    }
    (quartet_len == 0).then_some(decoded_len)
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

fn mail_attachment_summary(content: &str) -> (usize, Vec<String>) {
    let mut count = 0usize;
    let mut filenames = Vec::new();
    for line in content
        .lines()
        .filter(|line| ascii_contains_ignore_case(line, "content-disposition: attachment"))
    {
        count = count.saturating_add(1);
        if filenames.len() < MAX_MAIL_ATTACHMENT_NAMES {
            if let Some(name) = mail_attachment_filename_from_disposition(line) {
                filenames.push(bounded_mail_text(
                    &decode_mail_header_value(&name),
                    MAX_MAIL_FILENAME_BYTES,
                ));
            }
        }
    }
    (count, filenames)
}

fn ascii_contains_ignore_case(value: &str, needle: &str) -> bool {
    !needle.is_empty()
        && value
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn mail_mime_part_summaries(content: &str, boundary: &str) -> Vec<String> {
    let mut summaries = Vec::new();
    mail_mime_part_summaries_inner(content, boundary, 0, &mut summaries);
    summaries
}

fn mail_mime_boundary_is_valid(boundary: &str) -> bool {
    !boundary.is_empty()
        && boundary.len() <= MAX_MAIL_MIME_BOUNDARY_BYTES
        && boundary.is_ascii()
        && !boundary.bytes().any(|byte| byte.is_ascii_control())
}

fn mail_mime_parts<'a>(content: &'a str, boundary: &str) -> Vec<&'a str> {
    let marker = format!("--{boundary}");
    let mut parts = Vec::new();
    let mut part_start = None;
    let mut offset = 0usize;
    for segment in content.split_inclusive('\n') {
        let line_start = offset;
        offset += segment.len();
        let line = segment.trim_end_matches(['\r', '\n']);
        let Some(closing) = mail_mime_delimiter(line, &marker) else {
            continue;
        };
        if let Some(start) = part_start.take() {
            parts.push(&content[start..line_start]);
            if parts.len() >= MAX_MAIL_MIME_PARTS {
                break;
            }
        }
        if closing {
            break;
        }
        part_start = Some(offset);
    }
    if parts.len() < MAX_MAIL_MIME_PARTS {
        if let Some(start) = part_start {
            parts.push(&content[start..]);
        }
    }
    parts
}

fn mail_mime_delimiter(line: &str, marker: &str) -> Option<bool> {
    let suffix = line.strip_prefix(marker)?;
    if let Some(rest) = suffix.strip_prefix("--") {
        rest.bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
            .then_some(true)
    } else {
        suffix
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
            .then_some(false)
    }
}

fn mail_mime_part_summaries_inner(
    content: &str,
    boundary: &str,
    depth: usize,
    summaries: &mut Vec<String>,
) {
    if depth > MAX_MAIL_MIME_DEPTH
        || summaries.len() >= MAX_MAIL_MIME_PARTS
        || !mail_mime_boundary_is_valid(boundary)
    {
        return;
    }
    for part in mail_mime_parts(content, boundary) {
        let trimmed = part.trim_start_matches(['\r', '\n']);
        if summaries.len() >= MAX_MAIL_MIME_PARTS
            || trimmed.starts_with("--")
            || trimmed.trim().is_empty()
        {
            continue;
        }
        let (header_text, body) = trimmed
            .split_once("\r\n\r\n")
            .or_else(|| trimmed.split_once("\n\n"))
            .unwrap_or((trimmed, ""));
        let headers = parse_mail_headers(header_text);
        let content_type_header = header_value(&headers, "Content-Type");
        let content_type = content_type_header
            .map(|value| {
                bounded_mail_text(
                    value.split(';').next().unwrap_or(value).trim(),
                    MAX_MAIL_MIME_FIELD_BYTES,
                )
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text/plain".to_string());
        let disposition = header_value(&headers, "Content-Disposition")
            .map(|value| {
                bounded_mail_text(
                    value.split(';').next().unwrap_or(value).trim(),
                    MAX_MAIL_MIME_FIELD_BYTES,
                )
            })
            .filter(|value| !value.is_empty());
        let filename = header_value(&headers, "Content-Disposition")
            .and_then(mail_attachment_filename_from_disposition);
        let encoding = header_value(&headers, "Content-Transfer-Encoding")
            .map(|value| bounded_mail_text(value.trim(), MAX_MAIL_MIME_FIELD_BYTES))
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
    if trimmed.is_empty() || trimmed.len() > MAX_MAIL_DECODED_BODY_BYTES {
        return None;
    }
    let text = if encoding.is_some_and(|value| value.eq_ignore_ascii_case("base64")) {
        String::from_utf8_lossy(&decode_base64(trimmed, MAX_MAIL_DECODED_BODY_BYTES)?).to_string()
    } else if encoding.is_some_and(|value| value.eq_ignore_ascii_case("quoted-printable")) {
        String::from_utf8_lossy(&decode_quoted_printable(trimmed.as_bytes())?).to_string()
    } else {
        trimmed.to_string()
    };
    let mut preview = String::new();
    let mut preview_chars = 0usize;
    for word in text.split_whitespace() {
        if preview_chars >= MAX_MAIL_TEXT_PREVIEW_CHARS {
            break;
        }
        if !preview.is_empty() {
            if preview_chars >= MAX_MAIL_TEXT_PREVIEW_CHARS.saturating_sub(1) {
                break;
            }
            preview.push(' ');
            preview_chars += 1;
        }
        for character in word.chars() {
            if preview_chars >= MAX_MAIL_TEXT_PREVIEW_CHARS {
                break;
            }
            preview.push(character);
            preview_chars += 1;
        }
    }
    (!preview.is_empty()).then_some(preview)
}

fn mail_decoded_body_len(body: &str, encoding: &str) -> Option<usize> {
    let trimmed = body.trim_matches(|ch| ch == '\r' || ch == '\n');
    if trimmed.len() > MAX_MAIL_DECODED_BODY_BYTES {
        return None;
    }
    if encoding.eq_ignore_ascii_case("base64") {
        decode_base64_into(trimmed, MAX_MAIL_DECODED_BODY_BYTES, |_| {})
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
    let mut output = Vec::with_capacity(bytes.len().min(MAX_MAIL_DECODED_BODY_BYTES));
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
                if output.len() > MAX_MAIL_DECODED_BODY_BYTES {
                    return None;
                }
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        if output.len() > MAX_MAIL_DECODED_BODY_BYTES {
            return None;
        }
        index += 1;
    }
    Some(output)
}

fn mail_attachment_filename_from_disposition(line: &str) -> Option<String> {
    if let Some(value) = mail_header_parameter(line, "filename") {
        return Some(bounded_mail_text(&value, MAX_MAIL_FILENAME_BYTES));
    }
    let parameters = mail_header_parameters(line);
    if let Some(value) = parameters
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case("filename*").then_some(value))
    {
        return decode_rfc2231_value(value);
    }

    let mut joined = String::new();
    for index in 0..MAX_MAIL_FILENAME_SEGMENTS {
        let encoded_key = format!("filename*{index}*");
        let plain_key = format!("filename*{index}");
        if let Some(value) = parameters.iter().find_map(|(key, value)| {
            (key.eq_ignore_ascii_case(&encoded_key) || key.eq_ignore_ascii_case(&plain_key))
                .then_some(value)
        }) {
            push_bounded_mail_text(&mut joined, value, MAX_MAIL_FILENAME_BYTES);
        } else {
            break;
        }
    }
    (!joined.is_empty()).then(|| {
        let decoded = decode_rfc2231_value(&joined).unwrap_or(joined);
        bounded_mail_text(&decoded, MAX_MAIL_FILENAME_BYTES)
    })
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
    let mut bytes = Vec::with_capacity(value.len().min(MAX_MAIL_FILENAME_BYTES));
    let mut iter = value.as_bytes().iter().copied();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let hi = iter.next()?;
            let lo = iter.next()?;
            if bytes.len() >= MAX_MAIL_FILENAME_BYTES {
                return None;
            }
            bytes.push((hex_nibble(hi)? << 4) | hex_nibble(lo)?);
        } else {
            if bytes.len() >= MAX_MAIL_FILENAME_BYTES {
                return None;
            }
            bytes.push(byte);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
fn append_msg_compound_summary(text: &mut String, bytes: &[u8]) {
    let mut reader = std::io::Cursor::new(bytes);
    let _ = cfb::append_msg_compound_summary(text, &mut reader, bytes.len() as u64, None);
}
