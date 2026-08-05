use std::collections::BTreeSet;

use super::super::{
    common::{format_bytes, format_number, read_u16_be, read_u32_be},
    preview_cancelled,
    types::{PreviewTableDto, PreviewTableRowDto, PreviewTableSheetDto},
};
pub(super) fn database_page_size(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(b"SQLite format 3\0") {
        return None;
    }
    match read_u16_be(bytes, 16)? {
        1 => Some(65_536),
        value if (512..=32_768).contains(&value) && value.is_power_of_two() => Some(value as usize),
        _ => None,
    }
}

pub(super) fn encoding_name(value: u32) -> &'static str {
    match value {
        1 => "UTF-8",
        2 => "UTF-16le",
        3 => "UTF-16be",
        _ => "unknown",
    }
}

pub(super) fn append_sqlite_header_details(text: &mut String, bytes: &[u8]) {
    let write_version = bytes.get(18).copied().unwrap_or(0);
    let read_version = bytes.get(19).copied().unwrap_or(0);
    if write_version > 0 || read_version > 0 {
        text.push_str(&format!(
            "\nJournal mode: {}",
            sqlite_journal_mode_name(write_version, read_version)
        ));
    }
    if let Some(schema_format) = read_u32_be(bytes, 44) {
        text.push_str(&format!(
            "\nSchema format: {}",
            sqlite_schema_format_name(schema_format)
        ));
    }
    if let Some(schema_cookie) = read_u32_be(bytes, 40) {
        text.push_str(&format!("\nSchema cookie: {}", schema_cookie));
    }
    if let Some(freelist_pages) = read_u32_be(bytes, 36) {
        text.push_str(&format!(
            "\nFreelist pages: {}",
            format_number(freelist_pages as i64)
        ));
    }
    if let Some(version) = read_u32_be(bytes, 96) {
        if version > 0 {
            text.push_str(&format!("\nSQLite version: {}", version));
        }
    }
}

fn sqlite_journal_mode_name(write_version: u8, read_version: u8) -> &'static str {
    match (write_version, read_version) {
        (2, 2) => "WAL",
        (1, 1) => "rollback journal",
        _ => "mixed/unknown",
    }
}

fn sqlite_schema_format_name(value: u32) -> String {
    match value {
        1 => "1 (legacy)".to_string(),
        2 => "2".to_string(),
        3 => "3".to_string(),
        4 => "4 (current)".to_string(),
        _ => format!("{value}"),
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct SqliteSchemaRow {
    pub(super) typ: String,
    pub(super) name: String,
    pub(super) table_name: String,
    pub(super) root_page: i64,
    pub(super) sql: String,
}

const MAX_SQLITE_SCHEMA_OBJECTS: usize = 32;
const MAX_SQLITE_SCHEMA_OBJECTS_PER_GROUP: usize = 8;
const MAX_SQLITE_SCHEMA_PAGES: usize = 32;
const MAX_SQLITE_TABLE_ROW_PAGES: usize = 128;
const MAX_SQLITE_SAMPLE_ROWS: usize = 100;
const MAX_SQLITE_SAMPLE_COLUMNS: usize = 32;
pub(super) const MAX_SQLITE_SAMPLE_CELL_CHARS: usize = 256;
const MAX_SQLITE_SAMPLE_SHEETS: usize = 8;
const MAX_SQLITE_SAMPLE_RETAINED_CHARS: usize = 512 * 1024;

pub(super) struct SqliteTablePreview {
    pub(super) name: String,
    pub(super) table: PreviewTableDto,
}

pub(super) fn build_sqlite_table_preview(
    bytes: &[u8],
    page_size: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<SqliteTablePreview> {
    let schema =
        parse_sqlite_schema_summary(bytes, page_size, MAX_SQLITE_SCHEMA_OBJECTS, cancel_cb);
    let business_tables = schema.rows.iter().filter(|row| {
        row.typ.eq_ignore_ascii_case("table")
            && row.root_page > 0
            && !row.name.to_ascii_lowercase().starts_with("sqlite_")
    });
    let mut retained_chars = 0usize;
    let mut sheets = Vec::new();
    for row in business_tables.take(MAX_SQLITE_SAMPLE_SHEETS) {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        if let Some(table) = build_sqlite_sheet_table(
            bytes,
            page_size,
            row,
            &schema,
            &mut retained_chars,
            cancel_cb,
        ) {
            sheets.push(PreviewTableSheetDto {
                name: row.name.clone(),
                table,
            });
        }
    }
    let first = sheets.first()?;
    let first_name = first.name.clone();
    let mut first_table = first.table.clone();
    first_table.sheets = sheets;
    Some(SqliteTablePreview {
        name: first_name,
        table: first_table,
    })
}

fn build_sqlite_sheet_table(
    bytes: &[u8],
    page_size: usize,
    row: &SqliteSchemaRow,
    schema: &SqliteSchemaSummary,
    retained_chars: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<PreviewTableDto> {
    let headers = parse_sqlite_table_column_names(&row.sql, MAX_SQLITE_SAMPLE_COLUMNS);
    if headers.is_empty() {
        return None;
    }
    let sample = sample_sqlite_table_rows(
        bytes,
        page_size,
        row.root_page,
        headers.len(),
        read_u32_be(bytes, 56).unwrap_or(1),
        retained_chars,
        cancel_cb,
    )?;
    let observed = count_sqlite_table_rows(
        bytes,
        page_size,
        row.root_page,
        MAX_SQLITE_TABLE_ROW_PAGES,
        cancel_cb,
    );
    let total_rows = observed
        .as_ref()
        .map(|count| count.rows.min(i32::MAX as u64) as usize)
        .unwrap_or(sample.rows.len());
    let is_partial = schema.partial
        || sample.partial
        || observed.as_ref().is_some_and(|count| count.partial)
        || total_rows > sample.rows.len();
    Some(PreviewTableDto {
        format: "sqlite".to_string(),
        summary: Some(format!(
            "SQLite 3 | Table: {} | Page size: {} bytes | Encoding: {} | Schema objects: {}{} | Showing {} of {} observed rows",
            row.name,
            page_size,
            encoding_name(read_u32_be(bytes, 56).unwrap_or(0)),
            schema.rows.len(),
            if schema.partial { "+" } else { "" },
            sample.rows.len(),
            total_rows
        )),
        delimiter: String::new(),
        headers,
        rows: sample.rows,
        total_rows,
        total_columns: sample.total_columns,
        is_partial,
        sheets: Vec::new(),
    })
}

struct SqliteTableSample {
    rows: Vec<PreviewTableRowDto>,
    total_columns: usize,
    partial: bool,
}

fn sample_sqlite_table_rows(
    bytes: &[u8],
    page_size: usize,
    root_page: i64,
    column_count: usize,
    text_encoding: u32,
    retained_chars: &mut usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<SqliteTableSample> {
    if root_page <= 0 || column_count == 0 {
        return None;
    }
    let mut stack = vec![root_page as u32];
    let mut seen = BTreeSet::<u32>::new();
    let mut rows = Vec::new();
    let mut partial = false;
    let mut total_columns = column_count;
    while let Some(page_no) = stack.pop() {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        if seen.len() >= MAX_SQLITE_TABLE_ROW_PAGES || rows.len() >= MAX_SQLITE_SAMPLE_ROWS {
            partial = true;
            break;
        }
        if !seen.insert(page_no) {
            continue;
        }
        let Some(page) = sqlite_page(bytes, page_size, page_no) else {
            partial = true;
            continue;
        };
        let header = if page_no == 1 { 100 } else { 0 };
        match page.get(header).copied().unwrap_or(0) {
            0x05 => {
                let children = sqlite_table_interior_children(page, header);
                stack.extend(children.into_iter().rev());
            }
            0x0D => {
                let declared = read_u16_be(page, header + 3).unwrap_or(0) as usize;
                for index in 0..declared.min(512) {
                    if rows.len() >= MAX_SQLITE_SAMPLE_ROWS {
                        partial = true;
                        break;
                    }
                    let Some(cell_offset) =
                        read_u16_be(page, header + 8 + index * 2).map(usize::from)
                    else {
                        partial = true;
                        break;
                    };
                    if let Some((cells, record_columns)) =
                        parse_sqlite_table_leaf_cell(page, cell_offset, column_count, text_encoding)
                    {
                        let row_chars =
                            cells.iter().map(|cell| cell.chars().count()).sum::<usize>();
                        if *retained_chars
                            > MAX_SQLITE_SAMPLE_RETAINED_CHARS.saturating_sub(row_chars)
                        {
                            partial = true;
                            break;
                        }
                        *retained_chars += row_chars;
                        total_columns = total_columns.max(record_columns);
                        rows.push(PreviewTableRowDto { cells });
                    } else {
                        partial = true;
                    }
                }
                partial |= declared > 512;
            }
            _ => return None,
        }
    }
    Some(SqliteTableSample {
        rows,
        total_columns,
        partial,
    })
}

fn parse_sqlite_table_leaf_cell(
    page: &[u8],
    offset: usize,
    column_count: usize,
    text_encoding: u32,
) -> Option<(Vec<String>, usize)> {
    let (payload_len, mut pos) = read_sqlite_varint(page, offset)?;
    let (_rowid, next) = read_sqlite_varint(page, pos)?;
    pos = next;
    let end = pos.checked_add(payload_len as usize)?;
    parse_sqlite_table_record(page.get(pos..end)?, column_count, text_encoding)
}

pub(super) fn parse_sqlite_table_record(
    payload: &[u8],
    column_count: usize,
    text_encoding: u32,
) -> Option<(Vec<String>, usize)> {
    let (header_len, mut pos) = read_sqlite_varint(payload, 0)?;
    let header_len = header_len as usize;
    if header_len == 0 || header_len > payload.len() {
        return None;
    }
    let mut serials = Vec::new();
    while pos < header_len {
        let (serial, next) = read_sqlite_varint(payload, pos)?;
        serials.push(serial);
        pos = next;
        if serials.len() > 1024 {
            return None;
        }
    }
    let total_columns = serials.len();
    let mut value_pos = header_len;
    let mut cells = Vec::with_capacity(column_count);
    for serial in serials {
        let value = sqlite_record_display_value(payload, &mut value_pos, serial, text_encoding)?;
        if cells.len() < column_count {
            cells.push(value);
        }
    }
    cells.resize(column_count, String::new());
    Some((cells, total_columns))
}

fn sqlite_record_display_value(
    payload: &[u8],
    pos: &mut usize,
    serial: u64,
    text_encoding: u32,
) -> Option<String> {
    match serial {
        0 => Some("NULL".to_string()),
        1..=6 | 8 | 9 => sqlite_record_integer(payload, pos, serial).map(|value| value.to_string()),
        7 => {
            let end = pos.checked_add(8)?;
            let value =
                f64::from_bits(u64::from_be_bytes(payload.get(*pos..end)?.try_into().ok()?));
            *pos = end;
            Some(value.to_string())
        }
        serial if serial >= 12 && serial % 2 == 0 => {
            let len = ((serial - 12) / 2) as usize;
            *pos = pos.checked_add(len)?;
            (payload.len() >= *pos).then(|| format!("<BLOB {}>", format_bytes(len as i64)))
        }
        serial if serial >= 13 => {
            let value = sqlite_record_text(payload, pos, serial, text_encoding)?;
            Some(truncate_sqlite_cell(&value))
        }
        _ => None,
    }
}

fn truncate_sqlite_cell(value: &str) -> String {
    if value.chars().count() <= MAX_SQLITE_SAMPLE_CELL_CHARS {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(MAX_SQLITE_SAMPLE_CELL_CHARS)
        .collect::<String>();
    out.push_str("...");
    out
}

pub(super) fn append_sqlite_schema_summary(
    text: &mut String,
    bytes: &[u8],
    page_size: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) {
    let summary =
        parse_sqlite_schema_summary(bytes, page_size, MAX_SQLITE_SCHEMA_OBJECTS, cancel_cb);
    if summary.rows.is_empty() {
        return;
    }

    text.push_str(&format!(
        "\nSchema objects observed: {}{}",
        summary.rows.len(),
        if summary.partial { " (partial)" } else { "" }
    ));
    for (typ, heading) in [
        ("table", "Tables"),
        ("view", "Views"),
        ("index", "Indexes"),
        ("trigger", "Triggers"),
    ] {
        if preview_cancelled(cancel_cb) {
            return;
        }
        append_sqlite_schema_group(
            text,
            bytes,
            page_size,
            &summary.rows,
            typ,
            heading,
            cancel_cb,
        );
    }
    text.push_str(&format!(
        "\nInspection limits: {} schema objects, {} objects/group, {} schema pages, {} row pages/table",
        MAX_SQLITE_SCHEMA_OBJECTS,
        MAX_SQLITE_SCHEMA_OBJECTS_PER_GROUP,
        MAX_SQLITE_SCHEMA_PAGES,
        MAX_SQLITE_TABLE_ROW_PAGES
    ));
}

pub(super) fn append_sqlite_schema_group(
    text: &mut String,
    bytes: &[u8],
    page_size: usize,
    rows: &[SqliteSchemaRow],
    typ: &str,
    heading: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) {
    let matching = rows
        .iter()
        .filter(|row| row.typ.eq_ignore_ascii_case(typ))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return;
    }
    let shown = matching.len().min(MAX_SQLITE_SCHEMA_OBJECTS_PER_GROUP);
    text.push_str(&format!(
        "\n{heading}{}:",
        if matching.len() > shown {
            " (partial)"
        } else {
            ""
        }
    ));
    for row in matching.into_iter().take(shown) {
        if preview_cancelled(cancel_cb) {
            return;
        }
        text.push_str(&format!(
            "\n- {} (table: {}, root: {})",
            row.name, row.table_name, row.root_page
        ));
        if !row.sql.is_empty() {
            text.push_str(&format!(
                "\n  SQL: {}",
                truncate_sqlite_schema_sql(&row.sql)
            ));
        }
        if row.typ.eq_ignore_ascii_case("table") {
            let columns = parse_sqlite_table_columns(&row.sql, 8);
            if !columns.is_empty() {
                text.push_str("\n  Columns: ");
                text.push_str(&columns.join(", "));
            }
            if let Some(count) = count_sqlite_table_rows(
                bytes,
                page_size,
                row.root_page,
                MAX_SQLITE_TABLE_ROW_PAGES,
                cancel_cb,
            ) {
                text.push_str(&format!(
                    "\n  Rows observed: {}{}",
                    format_number(count.rows as i64),
                    if count.partial { " (partial)" } else { "" }
                ));
            }
        }
    }
}

pub(super) struct SqliteSchemaSummary {
    pub(super) rows: Vec<SqliteSchemaRow>,
    pub(super) partial: bool,
}

pub(super) struct SqliteRowCount {
    pub(super) rows: u64,
    pub(super) partial: bool,
}

pub(super) fn count_sqlite_table_rows(
    bytes: &[u8],
    page_size: usize,
    root_page: i64,
    max_pages: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<SqliteRowCount> {
    if page_size < 512 || root_page <= 0 || max_pages == 0 {
        return None;
    }
    let mut stack = vec![root_page as u32];
    let mut seen = BTreeSet::<u32>::new();
    let mut rows = 0u64;
    let mut partial = false;
    while let Some(page_no) = stack.pop() {
        if preview_cancelled(cancel_cb) {
            return None;
        }
        if seen.len() >= max_pages {
            partial = true;
            break;
        }
        if !seen.insert(page_no) {
            continue;
        }
        let Some(page) = sqlite_page(bytes, page_size, page_no) else {
            partial = true;
            continue;
        };
        let header = if page_no == 1 { 100 } else { 0 };
        let page_type = page.get(header).copied().unwrap_or(0);
        let cell_count = read_u16_be(page, header + 3).unwrap_or(0) as u64;
        match page_type {
            0x0D => rows = rows.saturating_add(cell_count),
            0x05 => {
                for child in sqlite_table_interior_children(page, header) {
                    stack.push(child);
                }
            }
            _ => return None,
        }
    }
    Some(SqliteRowCount { rows, partial })
}

fn sqlite_page(bytes: &[u8], page_size: usize, page_no: u32) -> Option<&[u8]> {
    if page_no == 0 {
        return None;
    }
    let start = (page_no as usize).checked_sub(1)?.checked_mul(page_size)?;
    let end = start.checked_add(page_size)?;
    bytes.get(start..end)
}

fn sqlite_table_interior_children(page: &[u8], header: usize) -> Vec<u32> {
    let cell_count = read_u16_be(page, header + 3).unwrap_or(0).min(512) as usize;
    let mut children = Vec::new();
    if let Some(rightmost) = read_u32_be(page, header + 8) {
        if rightmost > 0 {
            children.push(rightmost);
        }
    }
    for index in 0..cell_count {
        let ptr_offset = header + 12 + index * 2;
        let Some(cell_offset) = read_u16_be(page, ptr_offset).map(usize::from) else {
            break;
        };
        if let Some(child) = read_u32_be(page, cell_offset) {
            if child > 0 {
                children.push(child);
            }
        }
    }
    children
}

#[cfg(test)]
pub(super) fn parse_sqlite_schema_rows(
    bytes: &[u8],
    page_size: usize,
    limit: usize,
) -> Vec<SqliteSchemaRow> {
    parse_sqlite_schema_summary(bytes, page_size, limit, None).rows
}

pub(super) fn parse_sqlite_schema_summary(
    bytes: &[u8],
    page_size: usize,
    limit: usize,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> SqliteSchemaSummary {
    if page_size < 512 || bytes.len() < 128 || !bytes.starts_with(b"SQLite format 3\0") {
        return SqliteSchemaSummary {
            rows: Vec::new(),
            partial: false,
        };
    }
    let mut stack = vec![1u32];
    let text_encoding = read_u32_be(bytes, 56).unwrap_or(1);
    let mut seen = BTreeSet::<u32>::new();
    let mut rows = Vec::new();
    let mut partial = false;
    while let Some(page_no) = stack.pop() {
        if preview_cancelled(cancel_cb) {
            partial = true;
            break;
        }
        if rows.len() >= limit || seen.len() >= MAX_SQLITE_SCHEMA_PAGES {
            partial = true;
            break;
        }
        if !seen.insert(page_no) {
            continue;
        }
        let Some(page) = sqlite_page(bytes, page_size, page_no) else {
            partial = true;
            continue;
        };
        let header = if page_no == 1 { 100usize } else { 0usize };
        match page.get(header).copied().unwrap_or(0) {
            0x0D => {
                partial |=
                    parse_sqlite_schema_leaf_page(page, header, limit, text_encoding, &mut rows)
            }
            0x05 => {
                for child in sqlite_table_interior_children(page, header) {
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    SqliteSchemaSummary { rows, partial }
}

pub(super) fn parse_sqlite_schema_leaf_page(
    page: &[u8],
    header: usize,
    limit: usize,
    text_encoding: u32,
    rows: &mut Vec<SqliteSchemaRow>,
) -> bool {
    let declared_cell_count = read_u16_be(page, header + 3).unwrap_or(0) as usize;
    let cell_count = declared_cell_count.min(256);
    let mut partial = declared_cell_count > cell_count;
    for index in 0..cell_count {
        if rows.len() >= limit {
            partial = true;
            break;
        }
        let ptr_offset = header + 8 + index * 2;
        let Some(cell_offset) = read_u16_be(page, ptr_offset).map(usize::from) else {
            partial = true;
            break;
        };
        if let Some(row) = parse_sqlite_schema_leaf_cell(page, cell_offset, text_encoding) {
            rows.push(row);
        } else {
            partial = true;
        }
    }
    partial
}

fn parse_sqlite_schema_leaf_cell(
    page: &[u8],
    offset: usize,
    text_encoding: u32,
) -> Option<SqliteSchemaRow> {
    let (payload_len, mut pos) = read_sqlite_varint(page, offset)?;
    let (_rowid, next) = read_sqlite_varint(page, pos)?;
    pos = next;
    let end = pos.checked_add(payload_len as usize)?;
    parse_sqlite_schema_record(page.get(pos..end)?, text_encoding)
}

pub(super) fn parse_sqlite_schema_record(
    payload: &[u8],
    text_encoding: u32,
) -> Option<SqliteSchemaRow> {
    let (header_len, mut pos) = read_sqlite_varint(payload, 0)?;
    let header_len = header_len as usize;
    if header_len == 0 || header_len > payload.len() {
        return None;
    }
    let mut serials = Vec::new();
    while pos < header_len && serials.len() < 5 {
        let (serial, next) = read_sqlite_varint(payload, pos)?;
        serials.push(serial);
        pos = next;
    }
    if serials.len() < 5 {
        return None;
    }

    let mut value_pos = header_len;
    let typ = sqlite_record_text(payload, &mut value_pos, serials[0], text_encoding)?;
    let name = sqlite_record_text(payload, &mut value_pos, serials[1], text_encoding)?;
    let table_name = sqlite_record_text(payload, &mut value_pos, serials[2], text_encoding)?;
    let root_page = sqlite_record_integer(payload, &mut value_pos, serials[3])?;
    let sql = sqlite_record_text(payload, &mut value_pos, serials[4], text_encoding)?;
    Some(SqliteSchemaRow {
        typ,
        name,
        table_name,
        root_page,
        sql,
    })
}

fn truncate_sqlite_schema_sql(sql: &str) -> String {
    let compact = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_SQL_CHARS: usize = 160;
    if compact.chars().count() <= MAX_SQL_CHARS {
        return compact;
    }
    let mut out = compact.chars().take(MAX_SQL_CHARS).collect::<String>();
    out.push_str("...");
    out
}

pub(super) fn parse_sqlite_table_columns(sql: &str, limit: usize) -> Vec<String> {
    let Some(body) = sqlite_parenthesized_body(sql) else {
        return Vec::new();
    };
    sqlite_split_top_level_csv(body)
        .into_iter()
        .filter_map(sqlite_column_summary)
        .take(limit)
        .collect()
}

pub(super) fn parse_sqlite_table_column_names(sql: &str, limit: usize) -> Vec<String> {
    let Some(body) = sqlite_parenthesized_body(sql) else {
        return Vec::new();
    };
    sqlite_split_top_level_csv(body)
        .into_iter()
        .filter(|definition| !sqlite_is_table_constraint(definition.trim()))
        .filter_map(|definition| sqlite_take_identifier(definition.trim()).map(|(name, _)| name))
        .take(limit)
        .collect()
}

fn sqlite_parenthesized_body(sql: &str) -> Option<&str> {
    let start = sql.find('(')?;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut previous = '\0';
    for (index, ch) in sql[start..].char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote && previous != '\\' {
                in_quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + index;
                    return sql.get(start + 1..end);
                }
            }
            _ => {}
        }
        previous = ch;
    }
    None
}

fn sqlite_split_top_level_csv(body: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    let mut previous = '\0';
    for (index, ch) in body.char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote && previous != '\\' {
                in_quote = None;
            }
            previous = ch;
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                if let Some(item) = body.get(start..index) {
                    items.push(item.trim());
                }
                start = index + 1;
            }
            _ => {}
        }
        previous = ch;
    }
    if let Some(item) = body.get(start..) {
        items.push(item.trim());
    }
    items
}

fn sqlite_column_summary(definition: &str) -> Option<String> {
    let trimmed = definition.trim();
    if trimmed.is_empty() || sqlite_is_table_constraint(trimmed) {
        return None;
    }
    let (name, rest) = sqlite_take_identifier(trimmed)?;
    let typ = sqlite_column_type(rest);
    Some(if typ.is_empty() {
        name
    } else {
        format!("{name} {typ}")
    })
}

fn sqlite_is_table_constraint(value: &str) -> bool {
    let upper = value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CONSTRAINT" | "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK"
    )
}

fn sqlite_take_identifier(value: &str) -> Option<(String, &str)> {
    let value = value.trim_start();
    let mut chars = value.char_indices();
    let (_, first) = chars.next()?;
    if matches!(first, '"' | '\'' | '`' | '[') {
        let quote = if first == '[' { ']' } else { first };
        for (index, ch) in chars {
            if ch == quote {
                let name = value.get(first.len_utf8()..index)?.to_string();
                let rest = value.get(index + ch.len_utf8()..).unwrap_or_default();
                return Some((name, rest));
            }
        }
        return None;
    }
    let end = value
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    Some((
        value.get(..end)?.to_string(),
        value.get(end..).unwrap_or_default(),
    ))
}

fn sqlite_column_type(rest: &str) -> String {
    let rest = rest.trim_start();
    let mut token_start: Option<usize> = None;
    let mut depth = 0i32;
    let mut in_quote: Option<char> = None;
    for (index, ch) in rest.char_indices() {
        if let Some(quote) = in_quote {
            if ch == quote {
                in_quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' | '[' => in_quote = Some(if ch == '[' { ']' } else { ch }),
            '(' => depth += 1,
            ')' => depth -= 1,
            ch if ch.is_whitespace() && depth == 0 => {
                if let Some(start) = token_start.take() {
                    if sqlite_is_column_constraint_keyword(&rest[start..index]) {
                        return rest[..start].trim().trim_end_matches(',').to_string();
                    }
                }
            }
            _ if token_start.is_none() && depth == 0 => token_start = Some(index),
            _ => {}
        }
    }
    if let Some(start) = token_start {
        if sqlite_is_column_constraint_keyword(&rest[start..]) {
            return rest[..start].trim().trim_end_matches(',').to_string();
        }
    }
    rest.trim().trim_end_matches(',').to_string()
}

fn sqlite_is_column_constraint_keyword(token: &str) -> bool {
    matches!(
        token.trim_matches(',').to_ascii_uppercase().as_str(),
        "PRIMARY"
            | "NOT"
            | "NULL"
            | "DEFAULT"
            | "COLLATE"
            | "REFERENCES"
            | "CHECK"
            | "UNIQUE"
            | "CONSTRAINT"
            | "GENERATED"
            | "AS"
    )
}

fn read_sqlite_varint(bytes: &[u8], offset: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for i in 0..9 {
        let b = *bytes.get(offset + i)?;
        if i == 8 {
            value = (value << 8) | b as u64;
            return Some((value, offset + i + 1));
        }
        value = (value << 7) | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            return Some((value, offset + i + 1));
        }
    }
    None
}

fn sqlite_record_text(
    payload: &[u8],
    pos: &mut usize,
    serial: u64,
    text_encoding: u32,
) -> Option<String> {
    if serial < 13 || serial.is_multiple_of(2) {
        sqlite_skip_record_value(payload, pos, serial)?;
        return Some(String::new());
    }
    let len = ((serial - 13) / 2) as usize;
    let end = pos.checked_add(len)?;
    let bytes = payload.get(*pos..end)?;
    let text = match text_encoding {
        2 => decode_sqlite_utf16(bytes, true)?,
        3 => decode_sqlite_utf16(bytes, false)?,
        _ => String::from_utf8_lossy(bytes).to_string(),
    };
    *pos = end;
    Some(text)
}

pub(super) fn decode_sqlite_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units = bytes.chunks_exact(2).map(|unit| {
        if little_endian {
            u16::from_le_bytes([unit[0], unit[1]])
        } else {
            u16::from_be_bytes([unit[0], unit[1]])
        }
    });
    Some(
        char::decode_utf16(units)
            .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
    )
}

pub(super) fn sqlite_record_integer(payload: &[u8], pos: &mut usize, serial: u64) -> Option<i64> {
    match serial {
        0 => Some(0),
        1 => {
            let value = *payload.get(*pos)? as i8 as i64;
            *pos += 1;
            Some(value)
        }
        2 => {
            let end = pos.checked_add(2)?;
            let value = i16::from_be_bytes(payload.get(*pos..end)?.try_into().ok()?) as i64;
            *pos = end;
            Some(value)
        }
        3 => sqlite_record_signed_integer(payload, pos, 3),
        4 => {
            let end = pos.checked_add(4)?;
            let value = i32::from_be_bytes(payload.get(*pos..end)?.try_into().ok()?) as i64;
            *pos = end;
            Some(value)
        }
        5 => sqlite_record_signed_integer(payload, pos, 6),
        6 => sqlite_record_signed_integer(payload, pos, 8),
        8 => Some(0),
        9 => Some(1),
        _ => {
            sqlite_skip_record_value(payload, pos, serial)?;
            Some(0)
        }
    }
}

fn sqlite_record_signed_integer(payload: &[u8], pos: &mut usize, len: usize) -> Option<i64> {
    let end = pos.checked_add(len)?;
    let bytes = payload.get(*pos..end)?;
    let mut value = if bytes.first()? & 0x80 != 0 {
        -1i64
    } else {
        0i64
    };
    for byte in bytes {
        value = (value << 8) | i64::from(*byte);
    }
    *pos = end;
    Some(value)
}

fn sqlite_skip_record_value(payload: &[u8], pos: &mut usize, serial: u64) -> Option<()> {
    let len = match serial {
        0 | 8 | 9 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 | 7 => 8,
        n if n >= 12 => ((n - 12) / 2) as usize,
        _ => return None,
    };
    *pos = pos.checked_add(len)?;
    (payload.len() >= *pos).then_some(())
}
