use super::{
    append_msg_compound_summary, base_info_text, common::format_number, file_name,
    generic_info_json, read_file_prefix,
};

const MAX_MAIL_HEADER_BYTES: usize = 256 * 1024;

pub(super) fn render_mail_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_MAIL_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "mail", size, modified_unix);
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
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
