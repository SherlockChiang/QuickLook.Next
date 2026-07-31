//! Text, Markdown, CSV, and TSV preview support.

use std::fs;
use std::io::Read;
use std::path::Path;

use super::types::{
    to_json, PreviewMarkdownBlockDto, PreviewMarkdownDto, PreviewMarkdownInlineDto,
    PreviewReadyDto, PreviewTableDto, PreviewTableRowDto,
};
use super::{preview_cancelled, read_reader_prefix_cancelable};

// ── Text preview ─────────────────────────────────────────────────────────────

const MAX_TEXT_BYTES: usize = 512 * 1024;
// The App viewport-virtualizes cells; these bounds cap the retained IPC model.
const MAX_TABLE_ROWS: usize = 4_000;
const MAX_TABLE_COLUMNS: usize = 64;
const MAX_TABLE_CELL_CHARS: usize = 240;
const MAX_TABLE_RETAINED_CELLS: usize = 65_536;
const MAX_TABLE_RETAINED_CHARS: usize = 512 * 1024;
const MAX_MARKDOWN_BLOCKS: usize = 500;
const MAX_MARKDOWN_LIST_ITEMS: usize = 300;
const MAX_MARKDOWN_TABLE_ROWS: usize = 120;
const MAX_MARKDOWN_INLINE_CHARS: usize = 4096;
fn read_text_preview_bytes<R: Read>(
    reader: &mut R,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<(Vec<u8>, bool)> {
    let mut bytes = read_reader_prefix_cancelable(reader, MAX_TEXT_BYTES + 1, cancel_cb).ok()?;
    let truncated = bytes.len() > MAX_TEXT_BYTES;
    if truncated {
        bytes.truncate(MAX_TEXT_BYTES);
        trim_text_bytes_to_safe_boundary(&mut bytes);
    }
    Some((bytes, truncated))
}

fn trim_text_bytes_to_safe_boundary(bytes: &mut Vec<u8>) {
    if bytes.len() < 2 {
        return;
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        if !(bytes.len() - 2).is_multiple_of(2) {
            bytes.pop();
        }
        return;
    }

    let start = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    if start >= bytes.len() {
        return;
    }

    let min_end = bytes.len().saturating_sub(3).max(start);
    for end in (min_end..=bytes.len()).rev() {
        if std::str::from_utf8(&bytes[start..end]).is_ok() {
            bytes.truncate(end);
            return;
        }
    }
}

fn known_text_formats() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (".md", "markdown", "markdown"),
        (".markdown", "markdown", "markdown"),
        (".txt", "plain", "text"),
        (".log", "plain", "log"),
        (".csv", "plain", "csv"),
        (".tsv", "plain", "tsv"),
        (".env", "code", "env"),
        (".bat", "code", "batch"),
        (".cmd", "code", "batch"),
        (".ps1", "code", "powershell"),
        (".sh", "code", "shell"),
        (".bash", "code", "shell"),
        (".zsh", "code", "shell"),
        (".json", "code", "json"),
        (".xml", "code", "xml"),
        (".xaml", "code", "xaml"),
        (".xsd", "code", "xml"),
        (".resx", "code", "xml"),
        (".config", "code", "xml"),
        (".manifest", "code", "xml"),
        (".policy", "code", "xml"),
        (".settings", "code", "xml"),
        (".ini", "code", "ini"),
        (".cfg", "code", "ini"),
        (".conf", "code", "ini"),
        (".cnf", "code", "ini"),
        (".inf", "code", "ini"),
        (".url", "code", "ini"),
        (".desktop", "code", "ini"),
        (".service", "code", "ini"),
        (".reg", "code", "ini"),
        (".rdp", "code", "properties"),
        (".rc", "code", "properties"),
        (".prefs", "code", "properties"),
        (".properties", "code", "properties"),
        (".yml", "code", "yaml"),
        (".yaml", "code", "yaml"),
        (".toml", "code", "toml"),
        (".cs", "code", "csharp"),
        (".csproj", "code", "xml"),
        (".sln", "plain", "text"),
        (".props", "code", "xml"),
        (".targets", "code", "xml"),
        (".rs", "code", "rust"),
        (".js", "code", "javascript"),
        (".jsx", "code", "javascript"),
        (".mjs", "code", "javascript"),
        (".cjs", "code", "javascript"),
        (".ts", "code", "typescript"),
        (".tsx", "code", "typescript"),
        (".css", "code", "css"),
        (".scss", "code", "scss"),
        (".sass", "code", "sass"),
        (".less", "code", "less"),
        (".html", "code", "html"),
        (".htm", "code", "html"),
        (".py", "code", "python"),
        (".c", "code", "c"),
        (".h", "code", "c"),
        (".cc", "code", "cpp"),
        (".cpp", "code", "cpp"),
        (".cxx", "code", "cpp"),
        (".hpp", "code", "cpp"),
        (".hxx", "code", "cpp"),
        (".java", "code", "java"),
        (".go", "code", "go"),
        (".php", "code", "php"),
        (".rb", "code", "ruby"),
        (".pl", "code", "perl"),
        (".swift", "code", "swift"),
        (".kt", "code", "kotlin"),
        (".kts", "code", "kotlin"),
        (".sql", "code", "sql"),
        (".lua", "code", "lua"),
        (".fs", "code", "fsharp"),
        (".fsx", "code", "fsharp"),
        (".vb", "code", "vb"),
        (".dart", "code", "dart"),
        (".scala", "code", "scala"),
        (".r", "code", "r"),
        (".dockerfile", "code", "dockerfile"),
    ]
}

fn known_text_filenames() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("Dockerfile", "code", "dockerfile"),
        ("Containerfile", "code", "dockerfile"),
        ("Makefile", "code", "makefile"),
        ("CMakeLists.txt", "code", "cmake"),
        (".editorconfig", "code", "ini"),
        (".gitignore", "plain", "text"),
        (".gitattributes", "plain", "text"),
        (".dockerignore", "plain", "text"),
        (".env", "code", "env"),
    ]
}

/// Produce JSON for a text preview: `{"kind":"text","title":"...","format":"...","language":"...","text":"..."}`.
/// Returns empty string on failure.
pub(crate) fn render_text(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    render_text_reader(&mut file, path, cancel_cb)
}

pub(crate) fn render_text_reader<R: Read>(
    reader: &mut R,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let ext = Path::new(logical_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let (format, language) = known_text_filenames()
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(filename))
        .or_else(|| {
            known_text_formats()
                .iter()
                .find(|(e, _, _)| e.eq_ignore_ascii_case(&ext))
        })
        .map(|(_, f, l)| (*f, *l))
        .unwrap_or(("plain", "text"));

    let (bytes, truncated) = match read_text_preview_bytes(reader, cancel_cb) {
        Some(result) => result,
        None => return String::new(),
    };
    if preview_cancelled(cancel_cb) {
        return String::new();
    }

    // BOM-aware Unicode first, then strict UTF-8 and Windows-1252 for legacy configuration files.
    let text = if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        encoding_rs::UTF_8.decode(&bytes[3..]).0
    } else if bytes.len() >= 2 && bytes[..2] == [0xFF, 0xFE] {
        encoding_rs::UTF_16LE.decode(&bytes[2..]).0
    } else if bytes.len() >= 2 && bytes[..2] == [0xFE, 0xFF] {
        encoding_rs::UTF_16BE.decode(&bytes[2..]).0
    } else if std::str::from_utf8(&bytes).is_ok() {
        encoding_rs::UTF_8.decode(&bytes).0
    } else {
        encoding_rs::WINDOWS_1252.decode(&bytes).0
    };

    let mut text = text.into_owned();
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    if format == "markdown" {
        return render_markdown_json(filename, &text, truncated, cancel_cb);
    }
    if language == "csv" || language == "tsv" {
        return render_delimited_table_json(
            filename,
            &text,
            if language == "tsv" { '\t' } else { ',' },
            language,
            truncated,
            cancel_cb,
        );
    }

    if truncated {
        text.push_str(&format!(
            "\n\n[Preview truncated at {} bytes]",
            MAX_TEXT_BYTES
        ));
    }

    let kind = if format == "markdown" {
        "markdown"
    } else {
        "text"
    };
    to_json(&PreviewReadyDto {
        kind: kind.to_string(),
        title: filename.to_string(),
        format: Some(format.to_string()),
        language: Some(language.to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn render_markdown_json(
    filename: &str,
    text: &str,
    input_truncated: bool,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    let (blocks, parse_partial) = parse_markdown_blocks(text, cancel_cb);
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    to_json(&PreviewReadyDto {
        kind: "markdown".to_string(),
        title: filename.to_string(),
        format: Some("markdown".to_string()),
        language: Some("markdown".to_string()),
        text: None,
        office_layout: None,
        listing: None,
        table: None,
        markdown: Some(PreviewMarkdownDto {
            blocks,
            is_partial: input_truncated || parse_partial,
        }),
    })
}

fn parse_markdown_blocks(
    text: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> (Vec<PreviewMarkdownBlockDto>, bool) {
    let lines = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = lines.split('\n').collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;
    let mut partial = false;

    while i < lines.len() {
        if preview_cancelled(cancel_cb) {
            break;
        }
        if blocks.len() >= MAX_MARKDOWN_BLOCKS {
            partial = true;
            break;
        }

        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if let Some(language) = fenced_code_language(trimmed) {
            i += 1;
            let mut code = String::new();
            while i < lines.len() {
                if preview_cancelled(cancel_cb) {
                    break;
                }
                if lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            blocks.push(markdown_block(
                "code",
                0,
                code.trim_end_matches('\n'),
                &language,
            ));
            continue;
        }

        if is_markdown_rule(trimmed) {
            blocks.push(markdown_block("thematicBreak", 0, "", ""));
            i += 1;
            continue;
        }

        if let Some((level, heading)) = parse_heading(trimmed) {
            let mut block = markdown_block("heading", level, heading, "");
            block.inlines = parse_markdown_inlines(heading);
            blocks.push(block);
            i += 1;
            continue;
        }

        if is_markdown_table_start(&lines, i) {
            let (block, next, table_partial) = parse_markdown_table(&lines, i);
            blocks.push(block);
            partial |= table_partial;
            i = next;
            continue;
        }

        if let Some((ordered, start_text)) = parse_list_item(trimmed) {
            let (block, next, list_partial) = parse_markdown_list(&lines, i, ordered, start_text);
            blocks.push(block);
            partial |= list_partial;
            i = next;
            continue;
        }

        if trimmed.starts_with('>') {
            let (block, next) = parse_markdown_quote(&lines, i);
            blocks.push(block);
            i = next;
            continue;
        }

        let mut paragraph = String::new();
        while i < lines.len() {
            let candidate = lines[i].trim();
            if candidate.is_empty()
                || fenced_code_language(candidate).is_some()
                || parse_heading(candidate).is_some()
                || is_markdown_rule(candidate)
                || is_markdown_table_start(&lines, i)
                || parse_list_item(candidate).is_some()
                || candidate.starts_with('>')
            {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(candidate);
            i += 1;
        }

        if !paragraph.is_empty() {
            let mut block = markdown_block("paragraph", 0, &paragraph, "");
            block.inlines = parse_markdown_inlines(&paragraph);
            blocks.push(block);
        } else {
            i += 1;
        }
    }

    (blocks, partial)
}

fn markdown_block(kind: &str, level: usize, text: &str, language: &str) -> PreviewMarkdownBlockDto {
    PreviewMarkdownBlockDto {
        kind: kind.to_string(),
        level,
        text: truncate_markdown_text(text),
        language: language.to_string(),
        inlines: Vec::new(),
        children: Vec::new(),
        table_headers: Vec::new(),
        table_rows: Vec::new(),
    }
}

fn markdown_inline(
    kind: &str,
    text: &str,
    url: &str,
    children: Vec<PreviewMarkdownInlineDto>,
) -> PreviewMarkdownInlineDto {
    PreviewMarkdownInlineDto {
        kind: kind.to_string(),
        text: truncate_markdown_text(text),
        url: url.to_string(),
        children,
    }
}

fn truncate_markdown_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(MAX_MARKDOWN_INLINE_CHARS) {
        out.push(ch);
    }
    out
}

fn fenced_code_language(trimmed: &str) -> Option<String> {
    trimmed
        .strip_prefix("```")
        .map(|rest| rest.trim().trim_matches('`').to_string())
}

fn parse_heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = trimmed[level..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some((level, rest.trim_end_matches('#').trim_end()))
}

fn is_markdown_rule(trimmed: &str) -> bool {
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3 && chars.iter().all(|c| *c == '-' || *c == '*' || *c == '_')
}

fn parse_list_item(trimmed: &str) -> Option<(bool, &str)> {
    if let Some(text) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some((false, text.trim()));
    }
    let dot = trimmed.find('.')?;
    if dot == 0 || dot > 6 || !trimmed[..dot].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let after = trimmed[dot + 1..].trim_start();
    if after.is_empty() {
        None
    } else {
        Some((true, after))
    }
}

fn parse_markdown_list(
    lines: &[&str],
    start: usize,
    ordered: bool,
    first_text: &str,
) -> (PreviewMarkdownBlockDto, usize, bool) {
    let mut block = markdown_block(
        if ordered {
            "orderedList"
        } else {
            "unorderedList"
        },
        0,
        "",
        "",
    );
    let mut i = start;
    let mut partial = false;
    let mut next_text = Some(first_text.to_string());

    while i < lines.len() {
        let trimmed = lines[i].trim();
        let item_text = if let Some(text) = next_text.take() {
            text
        } else if let Some((item_ordered, text)) = parse_list_item(trimmed) {
            if item_ordered != ordered {
                break;
            }
            text.to_string()
        } else {
            break;
        };

        let mut item = markdown_block("listItem", 0, &item_text, "");
        item.inlines = parse_markdown_inlines(&item_text);
        if block.children.len() < MAX_MARKDOWN_LIST_ITEMS {
            block.children.push(item);
        } else {
            partial = true;
        }
        i += 1;
    }

    (block, i, partial)
}

fn parse_markdown_quote(lines: &[&str], start: usize) -> (PreviewMarkdownBlockDto, usize) {
    let mut text = String::new();
    let mut i = start;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        let Some(rest) = trimmed.strip_prefix('>') else {
            break;
        };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(rest.trim_start());
        i += 1;
    }
    let mut block = markdown_block("blockquote", 0, &text, "");
    block.inlines = parse_markdown_inlines(&text);
    (block, i)
}

fn is_markdown_table_start(lines: &[&str], index: usize) -> bool {
    if index + 1 >= lines.len() {
        return false;
    }
    let header = lines[index].trim();
    let separator = lines[index + 1].trim();
    header.contains('|') && is_markdown_table_separator(separator)
}

fn is_markdown_table_separator(line: &str) -> bool {
    let cells = split_markdown_table_row(line);
    cells.len() >= 2
        && cells.iter().all(|cell| {
            cell.trim()
                .chars()
                .all(|c| c == '-' || c == ':' || c.is_whitespace())
                && cell.contains('-')
        })
}

fn parse_markdown_table(lines: &[&str], start: usize) -> (PreviewMarkdownBlockDto, usize, bool) {
    let mut block = markdown_block("table", 0, "", "");
    block.table_headers = split_markdown_table_row(lines[start])
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect();
    let mut i = start + 2;
    let mut partial = false;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            break;
        }
        if block.table_rows.len() < MAX_MARKDOWN_TABLE_ROWS {
            block.table_rows.push(
                split_markdown_table_row(trimmed)
                    .into_iter()
                    .map(|cell| cell.trim().to_string())
                    .collect(),
            );
        } else {
            partial = true;
        }
        i += 1;
    }
    (block, i, partial)
}

fn split_markdown_table_row(row: &str) -> Vec<String> {
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn parse_markdown_inlines(text: &str) -> Vec<PreviewMarkdownInlineDto> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                out.push(markdown_inline("code", &after[..end], "", Vec::new()));
                i += end + 2;
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                let inner = &after[..end];
                out.push(markdown_inline(
                    "strong",
                    "",
                    "",
                    parse_markdown_inlines(inner),
                ));
                i += end + 4;
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let inner = &after[..end];
                out.push(markdown_inline(
                    "emphasis",
                    "",
                    "",
                    parse_markdown_inlines(inner),
                ));
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    let label = &rest[1..close];
                    let url = &rest[close + 2..close + 2 + end];
                    out.push(markdown_inline(
                        "link",
                        "",
                        url,
                        parse_markdown_inlines(label),
                    ));
                    i += close + 3 + end;
                    continue;
                }
            }
        }

        let next = next_markdown_inline_token(rest);
        out.push(markdown_inline("text", &rest[..next], "", Vec::new()));
        i += next;
    }
    out
}

fn next_markdown_inline_token(text: &str) -> usize {
    let mut next = text.len();
    let start = text.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
    if start > 0 {
        for token in ["`", "**", "*", "["] {
            if let Some(at) = text[start..].find(token) {
                next = next.min(at + start);
            }
        }
    }
    next.max(start)
}

fn render_delimited_table_json(
    filename: &str,
    text: &str,
    delimiter: char,
    format: &str,
    input_truncated: bool,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> String {
    let (mut records, total_records, total_columns, parse_partial) =
        parse_delimited_records(text, delimiter, cancel_cb);
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    if records.is_empty() {
        records.push(vec![String::new()]);
    }

    let first_record = records.first().cloned().unwrap_or_default();
    let has_header = looks_like_header_row(&first_record);
    let display_total_columns = total_columns.max(1);
    let column_count = display_total_columns.clamp(1, MAX_TABLE_COLUMNS);
    let headers = if has_header {
        normalize_table_headers(first_record, column_count)
    } else {
        (0..column_count)
            .map(|i| format!("Column {}", i + 1))
            .collect()
    };

    let data_records = if has_header {
        records.into_iter().skip(1).collect::<Vec<_>>()
    } else {
        records
    };
    let total_rows = total_records.saturating_sub(usize::from(has_header));
    let rows = data_records
        .into_iter()
        .take(MAX_TABLE_ROWS)
        .map(|record| PreviewTableRowDto {
            cells: normalize_table_cells(record, headers.len()),
        })
        .collect::<Vec<_>>();
    let represented_rows = rows.len();
    let is_partial = input_truncated
        || parse_partial
        || total_rows > represented_rows
        || display_total_columns > MAX_TABLE_COLUMNS;

    to_json(&PreviewReadyDto {
        kind: "table".to_string(),
        title: format!("{filename} - Table"),
        format: Some(format.to_string()),
        language: Some(format.to_string()),
        text: None,
        office_layout: None,
        listing: None,
        table: Some(PreviewTableDto {
            format: format.to_string(),
            summary: None,
            delimiter: delimiter.to_string(),
            headers,
            rows,
            total_rows,
            total_columns: display_total_columns,
            is_partial,
            sheets: Vec::new(),
        }),
        markdown: None,
    })
}

fn parse_delimited_records(
    text: &str,
    delimiter: char,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> (Vec<Vec<String>>, usize, usize, bool) {
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut total_records = 0usize;
    let mut total_columns = 0usize;
    let mut is_partial = false;
    let mut in_quotes = false;
    let mut saw_any = false;
    let mut chars = text.chars().peekable();
    let mut processed = 0usize;
    let mut retained_cells = 0usize;
    let mut retained_chars = 0usize;
    let mut retention_exhausted = false;

    while let Some(ch) = chars.next() {
        processed += 1;
        if processed & 0x0fff == 0 && preview_cancelled(cancel_cb) {
            break;
        }
        saw_any = true;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    push_table_char(&mut cell, '"');
                    if cell.chars().count() >= MAX_TABLE_CELL_CHARS {
                        is_partial = true;
                    }
                } else {
                    in_quotes = false;
                }
            } else {
                if cell.chars().count() < MAX_TABLE_CELL_CHARS {
                    cell.push(ch);
                } else if !ch.is_control() {
                    is_partial = true;
                }
            }
            continue;
        }

        if ch == '"' && cell.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
            finish_table_row(
                &mut records,
                &mut row,
                &mut total_records,
                &mut retained_cells,
                &mut retained_chars,
                &mut retention_exhausted,
                &mut is_partial,
            );
        } else if cell.chars().count() < MAX_TABLE_CELL_CHARS {
            cell.push(ch);
        } else {
            is_partial = true;
        }
    }

    if saw_any && (!cell.is_empty() || !row.is_empty()) {
        finish_table_cell(&mut row, &mut cell, &mut total_columns, &mut is_partial);
        finish_table_row(
            &mut records,
            &mut row,
            &mut total_records,
            &mut retained_cells,
            &mut retained_chars,
            &mut retention_exhausted,
            &mut is_partial,
        );
    }

    (records, total_records, total_columns, is_partial)
}

fn push_table_char(cell: &mut String, ch: char) {
    if cell.chars().count() < MAX_TABLE_CELL_CHARS {
        cell.push(ch);
    }
}

fn finish_table_cell(
    row: &mut Vec<String>,
    cell: &mut String,
    total_columns: &mut usize,
    is_partial: &mut bool,
) {
    *total_columns = (*total_columns).max(row.len() + 1);
    if row.len() < MAX_TABLE_COLUMNS {
        row.push(cell.to_string());
    } else {
        *is_partial = true;
    }
    cell.clear();
}

fn finish_table_row(
    records: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    total_records: &mut usize,
    retained_cells: &mut usize,
    retained_chars: &mut usize,
    retention_exhausted: &mut bool,
    is_partial: &mut bool,
) {
    *total_records += 1;
    let row_cells = row.len();
    let row_chars = row.iter().map(|cell| cell.chars().count()).sum::<usize>();
    if !*retention_exhausted
        && records.len() < MAX_TABLE_ROWS + 1
        && retained_cells.saturating_add(row_cells) <= MAX_TABLE_RETAINED_CELLS
        && retained_chars.saturating_add(row_chars) <= MAX_TABLE_RETAINED_CHARS
    {
        *retained_cells += row_cells;
        *retained_chars += row_chars;
        records.push(std::mem::take(row));
    } else {
        *retention_exhausted = true;
        row.clear();
        *is_partial = true;
    }
}

fn looks_like_header_row(row: &[String]) -> bool {
    row.iter()
        .any(|cell| cell.chars().any(|ch| ch.is_alphabetic()))
}

fn normalize_table_headers(mut headers: Vec<String>, column_count: usize) -> Vec<String> {
    headers.truncate(column_count);
    while headers.len() < column_count {
        headers.push(String::new());
    }
    headers
        .into_iter()
        .enumerate()
        .map(|(index, header)| {
            let header = header.trim();
            if header.is_empty() {
                format!("Column {}", index + 1)
            } else {
                header.to_string()
            }
        })
        .collect()
}

fn normalize_table_cells(mut cells: Vec<String>, column_count: usize) -> Vec<String> {
    cells.truncate(column_count);
    while cells.len() < column_count {
        cells.push(String::new());
    }
    cells
}

/// Check if a file is text-like (extension known or a small printable Unicode header).
pub(crate) fn is_text(ext: &str, magic: &[u8]) -> bool {
    if known_text_formats()
        .iter()
        .any(|(e, _, _)| e.eq_ignore_ascii_case(ext))
    {
        return true;
    }
    if matches!(magic.first(), Some(b'd' | b'l'))
        && super::parse_bencode(magic, None).is_some_and(|(_, consumed)| consumed == magic.len())
    {
        return false;
    }
    is_probably_utf8_text(magic)
}

pub(crate) fn is_text_file(file_name: &str, ext: &str, magic: &[u8]) -> bool {
    known_text_filenames()
        .iter()
        .any(|(name, _, _)| name.eq_ignore_ascii_case(file_name))
        || is_text(ext, magic)
}

fn is_probably_utf8_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return is_probably_utf16_text(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return is_probably_utf16_text(&bytes[2..], false);
    }
    if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return is_probably_windows_1252_text(bytes);
    }
    let printable = bytes
        .iter()
        .filter(|b| matches!(**b, b'\t' | b'\r' | b'\n' | 0x20..=0x7E) || **b >= 0x80)
        .count();
    printable * 100 / bytes.len().max(1) >= 90
}

fn is_probably_windows_1252_text(bytes: &[u8]) -> bool {
    if bytes.is_empty()
        || bytes.contains(&0)
        || !bytes
            .iter()
            .any(|byte| matches!(*byte, b'=' | b':' | b'[' | b'#' | b';' | b'\r' | b'\n'))
    {
        return false;
    }
    let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    let char_count = text.chars().count();
    let printable = text
        .chars()
        .filter(|ch| matches!(*ch, '\t' | '\r' | '\n') || !ch.is_control())
        .count();
    char_count > 0 && printable * 100 / char_count >= 90
}

fn is_probably_utf16_text(bytes: &[u8], little_endian: bool) -> bool {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return false;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|unit| {
            if little_endian {
                u16::from_le_bytes([unit[0], unit[1]])
            } else {
                u16::from_be_bytes([unit[0], unit[1]])
            }
        })
        .collect();
    let Ok(text) = String::from_utf16(&units) else {
        return false;
    };
    let char_count = text.chars().count();
    let printable = text
        .chars()
        .filter(|ch| matches!(*ch, '\t' | '\r' | '\n') || !ch.is_control())
        .count();
    char_count > 0 && printable * 100 / char_count >= 90
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_preview_decodes_windows_1252_config() {
        let path =
            std::env::temp_dir().join(format!("quicklook-next-text-{}.ini", std::process::id()));
        std::fs::write(&path, b"name=caf\xE9").expect("write Windows-1252 config");
        let json = render_text(path.to_str().unwrap(), None);
        let _ = std::fs::remove_file(path);

        assert!(json.contains("name=café"));
        assert!(json.contains("\"language\":\"ini\""));
    }

    #[test]
    fn utf8_text_truncation_stays_on_char_boundary() {
        let mut bytes = vec![b'a'; MAX_TEXT_BYTES - 1];
        bytes.extend_from_slice("中".as_bytes());
        bytes.truncate(MAX_TEXT_BYTES);

        trim_text_bytes_to_safe_boundary(&mut bytes);

        assert_eq!(bytes.len(), MAX_TEXT_BYTES - 1);
        assert!(std::str::from_utf8(&bytes).is_ok());
    }

    #[test]
    fn utf16_text_truncation_drops_half_code_unit() {
        let mut bytes = vec![0xFF, 0xFE, 0x41];

        trim_text_bytes_to_safe_boundary(&mut bytes);

        assert_eq!(bytes, vec![0xFF, 0xFE]);
    }

    #[test]
    fn unknown_complete_bencode_is_not_sniffed_as_text() {
        assert!(!is_text(".bin", b"d4:fake4:datae"));
        assert!(is_text(".txt", b"d4:fake4:datae"));
    }

    #[test]
    fn markdown_parser_emits_heading_and_inline_ast() {
        let (blocks, partial) = parse_markdown_blocks("# Hello **QuickLook** and `Rust`", None);

        assert!(!partial);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "heading");
        assert_eq!(blocks[0].level, 1);
        assert!(blocks[0].inlines.iter().any(|i| i.kind == "strong"));
        assert!(blocks[0].inlines.iter().any(|i| i.kind == "code"));
    }

    #[test]
    fn markdown_parser_does_not_panic_on_non_ascii() {
        let (blocks, partial) =
            parse_markdown_blocks("# 中文标题\n\n这是一个含有 **加粗** 的中文字符串。", None);
        assert!(!partial);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "heading");
        assert_eq!(blocks[0].text, "中文标题");
        assert_eq!(blocks[1].kind, "paragraph");
        assert!(blocks[1].inlines.iter().any(|i| i.kind == "strong"));
    }

    #[test]
    fn markdown_parser_emits_lists_quotes_and_code() {
        let (blocks, partial) =
            parse_markdown_blocks("> note\n\n- one\n- two\n\n```rs\nfn main() {}\n```", None);

        assert!(!partial);
        assert_eq!(blocks[0].kind, "blockquote");
        assert_eq!(blocks[1].kind, "unorderedList");
        assert_eq!(blocks[1].children.len(), 2);
        assert_eq!(blocks[2].kind, "code");
        assert_eq!(blocks[2].language, "rs");
    }

    #[test]
    fn markdown_parser_emits_tables() {
        let (blocks, partial) = parse_markdown_blocks("| A | B |\n|---|---|\n| 1 | 2 |", None);

        assert!(!partial);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, "table");
        assert_eq!(
            blocks[0].table_headers,
            vec!["A".to_string(), "B".to_string()]
        );
        assert_eq!(
            blocks[0].table_rows[0],
            vec!["1".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn markdown_json_omits_duplicate_source_text() {
        let json = render_markdown_json("README.md", "# Title\n\nBody", false, None);
        let value: serde_json::Value = serde_json::from_str(&json).expect("markdown JSON");

        assert!(value
            .get("markdown")
            .is_some_and(|markdown| !markdown.is_null()));
        assert!(value.get("text").is_none());
    }

    #[test]
    fn delimited_table_retention_obeys_global_model_budgets() {
        let wide = "x".repeat(MAX_TABLE_CELL_CHARS);
        let text = (0..(MAX_TABLE_ROWS + 100))
            .map(|index| format!("{index},{wide}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (records, total, columns, partial) = parse_delimited_records(&text, ',', None);
        let retained_cells = records.iter().map(Vec::len).sum::<usize>();
        let retained_chars = records
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| cell.chars().count())
            .sum::<usize>();

        assert_eq!(total, MAX_TABLE_ROWS + 100);
        assert_eq!(columns, 2);
        assert!(partial);
        assert!(records.len() <= MAX_TABLE_ROWS + 1);
        assert!(retained_cells <= MAX_TABLE_RETAINED_CELLS);
        assert!(retained_chars <= MAX_TABLE_RETAINED_CHARS);
    }
}
