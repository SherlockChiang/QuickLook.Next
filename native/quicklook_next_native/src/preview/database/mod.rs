use std::io::Read;

use super::{
    base_info_text,
    common::{format_bytes, format_number, read_u16_be, read_u32_be},
    file_name, generic_info_json, preview_cancelled, read_exact_cancelable, read_file_prefix,
    types::{to_json, PreviewReadyDto},
    ReaderPreviewError, MAX_DATABASE_HANDLE_BYTES, MAX_INFO_HEADER_BYTES, MAX_SQLITE_SHM_BYTES,
    MAX_SQLITE_WAL_BYTES,
};

mod sqlite;
mod wal;

use self::{
    sqlite::{
        append_sqlite_header_details, append_sqlite_schema_summary, build_sqlite_table_preview,
        database_page_size as sqlite_database_page_size, encoding_name as sqlite_encoding_name,
    },
    wal::{
        append_sqlite_wal_summary, apply_sqlite_wal_snapshot, inspect_sqlite_shm,
        inspect_sqlite_wal_snapshot,
    },
};

pub fn render_database_info(
    path: &str,
    size: i64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let bytes = read_file_prefix(path, MAX_INFO_HEADER_BYTES).unwrap_or_default();
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    render_database_bytes(path, size, modified_unix, &bytes, &[], cancel_cb)
}

pub(crate) struct DatabaseCompanionReader<'a> {
    pub(crate) reader: Option<&'a mut dyn Read>,
    pub(crate) length: u64,
}

pub fn render_database_reader<R: Read>(
    reader: &mut R,
    main_length: u64,
    wal: DatabaseCompanionReader<'_>,
    shm: DatabaseCompanionReader<'_>,
    logical_name: &str,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let DatabaseCompanionReader {
        reader: wal_reader,
        length: wal_length,
    } = wal;
    let DatabaseCompanionReader {
        reader: shm_reader,
        length: shm_length,
    } = shm;
    if main_length > MAX_DATABASE_HANDLE_BYTES
        || wal_length > MAX_SQLITE_WAL_BYTES
        || shm_length > MAX_SQLITE_SHM_BYTES
    {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if wal_reader.is_none() && wal_length != 0 || shm_reader.is_none() && shm_length != 0 {
        return Err(ReaderPreviewError::Malformed);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let prefix_length = main_length.min(MAX_INFO_HEADER_BYTES as u64) as usize;
    let mut bytes = vec![0u8; prefix_length];
    read_exact_cancelable(reader, &mut bytes, cancel_cb)?;

    let lower_name = logical_name.to_ascii_lowercase();
    let is_sqlite_main = !lower_name.ends_with("-wal")
        && !lower_name.ends_with("-shm")
        && bytes.starts_with(b"SQLite format 3\0");
    if (wal_reader.is_some() || shm_reader.is_some()) && !is_sqlite_main {
        return Err(ReaderPreviewError::Malformed);
    }
    let companion_page_size = if wal_reader.is_some() || shm_reader.is_some() {
        let page_size = sqlite_database_page_size(&bytes).ok_or(ReaderPreviewError::Malformed)?;
        if main_length < page_size as u64 || !main_length.is_multiple_of(page_size as u64) {
            return Err(ReaderPreviewError::Malformed);
        }
        Some(page_size)
    } else {
        None
    };
    let mut snapshot_notes = Vec::new();
    if let Some(wal) = wal_reader {
        if wal_length == 0 {
            snapshot_notes.push(
                "WAL HANDLE: empty; the main database view is already checkpointed".to_string(),
            );
        } else {
            let page_size = companion_page_size.ok_or(ReaderPreviewError::Malformed)?;
            let snapshot = inspect_sqlite_wal_snapshot(wal, wal_length, page_size, cancel_cb)?;
            apply_sqlite_wal_snapshot(&mut bytes, page_size, &snapshot)?;
            snapshot_notes.push(snapshot.summary());
        }
    }
    if let Some(shm) = shm_reader {
        snapshot_notes.push(inspect_sqlite_shm(shm, shm_length, cancel_cb)?);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let size = i64::try_from(main_length).map_err(|_| ReaderPreviewError::LengthMismatch)?;
    let json = render_database_bytes(
        logical_name,
        size,
        modified_unix,
        &bytes,
        &snapshot_notes,
        cancel_cb,
    );
    if preview_cancelled(cancel_cb) {
        Err(ReaderPreviewError::Cancelled)
    } else if json.is_empty() {
        Err(ReaderPreviewError::Malformed)
    } else {
        Ok(json)
    }
}

fn render_database_bytes(
    path: &str,
    size: i64,
    modified_unix: i64,
    bytes: &[u8],
    snapshot_notes: &[String],
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    let filename = file_name(path);
    let mut text = base_info_text(filename, "database", size, modified_unix);
    let lower_path = path.to_ascii_lowercase();
    if lower_path.ends_with("-wal") {
        append_sqlite_wal_summary(&mut text, bytes, size);
    } else if lower_path.ends_with("-shm") {
        text.push_str("\nFormat: SQLite shared-memory WAL index");
        text.push_str("\nRole: transient index for the associated SQLite WAL file");
        text.push_str(&format!(
            "\nInspected: {}",
            format_bytes(bytes.len() as i64)
        ));
    } else if bytes.starts_with(b"SQLite format 3\0") {
        let page_size = read_u16_be(bytes, 16)
            .map(|value| if value == 1 { 65536 } else { value as u32 })
            .unwrap_or(0);
        text.push_str("\nFormat: SQLite 3");
        text.push_str(&format!("\nPage size: {} bytes", page_size));
        if let Some(pages) = read_u32_be(bytes, 28) {
            text.push_str(&format!("\nPages: {}", format_number(pages as i64)));
            if page_size > 0 {
                let header_size = pages as i64 * page_size as i64;
                text.push_str(&format!(
                    "\nDatabase size from header: {}",
                    format_bytes(header_size)
                ));
                if size >= 0 && header_size != size {
                    let difference = size.abs_diff(header_size);
                    let relation = if header_size > size {
                        "larger"
                    } else {
                        "smaller"
                    };
                    text.push_str(&format!(
                        "\nSize status: header is {relation} than the file by {difference} bytes (the database may be incomplete or have uncheckpointed WAL data)"
                    ));
                }
            }
        }
        if let Some(encoding) = read_u32_be(bytes, 56) {
            text.push_str(&format!(
                "\nText encoding: {}",
                sqlite_encoding_name(encoding)
            ));
        }
        if let Some(user_version) = read_u32_be(bytes, 60) {
            text.push_str(&format!("\nUser version: {}", user_version));
        }
        if let Some(app_id) = read_u32_be(bytes, 68) {
            text.push_str(&format!("\nApplication ID: 0x{app_id:08X}"));
        }
        append_sqlite_header_details(&mut text, bytes);
        text.push_str(&format!(
            "\nInspected: {}",
            format_bytes(bytes.len() as i64)
        ));
        for note in snapshot_notes {
            text.push('\n');
            text.push_str(note);
        }
        append_sqlite_schema_summary(&mut text, bytes, page_size as usize, cancel_cb);
        if let Some(mut table) = build_sqlite_table_preview(bytes, page_size as usize, cancel_cb) {
            if !snapshot_notes.is_empty() {
                let snapshot_summary = snapshot_notes.join("; ");
                match table.table.summary.as_mut() {
                    Some(summary) => {
                        summary.push_str(" | ");
                        summary.push_str(&snapshot_summary);
                    }
                    None => table.table.summary = Some(snapshot_summary),
                }
            }
            return to_json(&PreviewReadyDto {
                kind: "database".to_string(),
                title: format!("{filename} - {}", table.name),
                format: Some("sqlite".to_string()),
                language: Some("sql".to_string()),
                text: None,
                office_layout: None,
                listing: None,
                table: Some(table.table),
                markdown: None,
            });
        }
    } else if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        text.push_str("\nFormat: Microsoft Compound File database");
    } else {
        text.push_str("\nFormat: database file");
    }
    if !bytes.starts_with(b"SQLite format 3\0") {
        for note in snapshot_notes {
            text.push('\n');
            text.push_str(note);
        }
    }
    generic_info_json(path, "database", size, modified_unix, Some(text))
}

#[cfg(test)]
mod tests;
