use std::path::Path;

pub(super) fn format_number(value: i64) -> String {
    let absolute = value.unsigned_abs();
    let digits = absolute.to_string();
    let reversed: Vec<char> = digits.chars().rev().collect();
    let mut formatted = String::new();

    for (index, character) in reversed.iter().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(*character);
    }

    let formatted: String = formatted.chars().rev().collect();
    if value < 0 {
        format!("-{formatted}")
    } else {
        formatted
    }
}

pub(super) fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} B", format_number(bytes))
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub(super) fn type_for_ext(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    if ext.is_empty() {
        return "File";
    }

    match ext.to_ascii_lowercase().as_str() {
        "txt" | "log" => "TXT File",
        "md" => "MD File",
        "json" => "JSON File",
        "xml" => "XML File",
        "png" => "PNG File",
        "jpg" | "jpeg" => "JPEG File",
        "gif" => "GIF File",
        "bmp" => "BMP File",
        "pdf" => "PDF File",
        "zip" => "ZIP File",
        "jar" => "JAR File",
        "apk" => "APK File",
        "apks" => "APKS File",
        "aab" => "Android App Bundle",
        "msix" => "MSIX Package",
        "msixbundle" => "MSIX Bundle",
        "appx" => "APPX Package",
        "appxbundle" => "APPX Bundle",
        "torrent" => "Torrent File",
        "img" => "Disk Image",
        "epub" => "EPUB Book",
        "fb2" => "FB2 Book",
        "mobi" => "MOBI Book",
        "azw" | "azw3" => "Kindle Book",
        "nupkg" => "NuGet Package",
        "vsix" => "VSIX Package",
        "whl" => "Python Wheel",
        "cbz" => "CBZ File",
        "xpi" => "XPI File",
        "tar" => "TAR File",
        "tgz" => "TGZ File",
        "gz" => "GZIP File",
        "docx" => "DOCX File",
        "xlsx" => "XLSX File",
        "pptx" => "PPTX File",
        "mp4" => "MP4 File",
        "mp3" => "MP3 File",
        "exe" => "Application",
        "dll" => "Application Extension",
        "sys" => "System File",
        "scr" => "Screen Saver",
        "cs" => "CS File",
        "rs" => "RS File",
        "py" => "PY File",
        "js" => "JS File",
        "ts" => "TS File",
        "html" | "htm" => "HTML File",
        "css" => "CSS File",
        _ => "File",
    }
}

// Bounded byte readers shared by binary preview families.

pub(super) fn read_c_string(bytes: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let end = bytes
        .get(offset..offset + max_len.min(bytes.len().saturating_sub(offset)))?
        .iter()
        .position(|byte| *byte == 0)
        .map(|len| offset + len)
        .unwrap_or_else(|| offset + max_len.min(bytes.len().saturating_sub(offset)));
    let value = String::from_utf8_lossy(bytes.get(offset..end)?)
        .trim()
        .to_string();
    Some(value)
}

pub(super) fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_u64_be(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    Some(u64::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_i32_be(bytes: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    Some(i32::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_i16_be(bytes: &[u8], offset: usize) -> Option<i16> {
    let end = offset.checked_add(2)?;
    Some(i16::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_i64_be(bytes: &[u8], offset: usize) -> Option<i64> {
    let end = offset.checked_add(8)?;
    Some(i64::from_be_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

pub(super) fn read_i32_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<i32> {
    let end = offset.checked_add(4)?;
    let chunk: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(if endian == 2 {
        i32::from_be_bytes(chunk)
    } else {
        i32::from_le_bytes(chunk)
    })
}

pub(super) fn read_u16_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u16> {
    if endian == 2 {
        read_u16_be(bytes, offset)
    } else {
        read_u16(bytes, offset)
    }
}

pub(super) fn read_u32_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u32> {
    if endian == 2 {
        read_u32_be(bytes, offset)
    } else {
        read_u32(bytes, offset)
    }
}

pub(super) fn read_u64_endian(bytes: &[u8], offset: usize, endian: u8) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let chunk: [u8; 8] = bytes.get(offset..end)?.try_into().ok()?;
    Some(if endian == 2 {
        u64::from_be_bytes(chunk)
    } else {
        u64::from_le_bytes(chunk)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_and_byte_formatting_preserve_preview_contract() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(1_234_567), "1,234,567");
        assert_eq!(format_number(-9_876), "-9,876");
        assert_eq!(format_number(i64::MIN), "-9,223,372,036,854,775,808");

        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1,023 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1_572_864), "1.50 MB");
    }

    #[test]
    fn file_type_labels_are_case_insensitive_and_fail_closed() {
        assert_eq!(type_for_ext("PHOTO.JPEG"), "JPEG File");
        assert_eq!(type_for_ext("archive.tar"), "TAR File");
        assert_eq!(type_for_ext("README"), "File");
        assert_eq!(type_for_ext("unknown.custom"), "File");
    }
}
