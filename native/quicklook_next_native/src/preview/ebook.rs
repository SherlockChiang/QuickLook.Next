//! EPUB, FictionBook, and binary ebook preview parsing.

use super::*;

// ── Ebook preview ───────────────────────────────────────────────────────────

pub fn render_ebook(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return String::new(),
    };
    let modified_unix = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);
    render_ebook_reader(file, path, metadata.len(), modified_unix, cancel_cb).unwrap_or_default()
}

pub fn render_ebook_reader<R: Read + Seek>(
    mut reader: R,
    logical_name: &str,
    source_len: u64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if source_len > MAX_EBOOK_HANDLE_INPUT_BYTES {
        return Err(ReaderPreviewError::LimitExceeded);
    }
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let lower = logical_name.to_ascii_lowercase();
    if lower.ends_with(".epub") {
        let mut zip =
            open_validated_zip(reader, source_len, MAX_EBOOK_ZIP_ENTRIES as u64, cancel_cb)?;
        return render_epub_from_zip(&mut zip, logical_name, cancel_cb);
    }

    prepare_seekable_reader(&mut reader, source_len, cancel_cb)?;
    if lower.ends_with(".fb2") {
        render_fb2_reader(&mut reader, logical_name, cancel_cb)
    } else {
        let size = i64::try_from(source_len).map_err(|_| ReaderPreviewError::LengthMismatch)?;
        Ok(render_binary_ebook_info(logical_name, size, modified_unix))
    }
}

#[derive(Default)]
pub(super) struct EpubOpf {
    pub(super) title: String,
    pub(super) creator: String,
    pub(super) language: String,
    pub(super) publisher: String,
    pub(super) identifier: String,
    pub(super) date: String,
    pub(super) description: String,
    pub(super) manifest: BTreeMap<String, EpubManifestItem>,
    pub(super) spine: Vec<String>,
}

#[derive(Clone)]
pub(super) struct EpubManifestItem {
    pub(super) href: String,
    pub(super) media_type: String,
}

pub(super) struct EbookContext {
    pub(super) remaining_decompressed_bytes: u64,
    pub(super) cancel_cb: Option<extern "C" fn() -> bool>,
}

impl EbookContext {
    fn new(cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
        Self {
            remaining_decompressed_bytes: MAX_EBOOK_DECOMPRESSED_BYTES,
            cancel_cb,
        }
    }

    fn check_cancelled(&self) -> Result<(), ReaderPreviewError> {
        if preview_cancelled(self.cancel_cb) {
            Err(ReaderPreviewError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn consume(&mut self, bytes: u64) -> Result<(), ReaderPreviewError> {
        self.check_cancelled()?;
        if bytes > self.remaining_decompressed_bytes {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        self.remaining_decompressed_bytes -= bytes;
        Ok(())
    }

    fn check_xml_event(&self, event_count: usize) -> Result<(), ReaderPreviewError> {
        if event_count % 256 == 0 {
            self.check_cancelled()?;
        }
        Ok(())
    }
}

fn render_epub_from_zip<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    let filename = file_name(logical_name);
    let mut context = EbookContext::new(cancel_cb);
    let container = read_ebook_zip_text(
        &mut context,
        zip,
        "META-INF/container.xml",
        MAX_EBOOK_XML_BYTES,
    )?;
    let container_rootfile = container
        .as_deref()
        .map(|xml| parse_epub_rootfile_with_context(&context, xml))
        .transpose()?
        .flatten();
    let rootfile = match container_rootfile {
        Some(rootfile) => Some(rootfile),
        None => find_epub_opf_path(&context, zip)?,
    }
    .unwrap_or_else(|| "content.opf".to_string());

    let Some(opf_xml) = read_ebook_zip_text(&mut context, zip, &rootfile, MAX_EBOOK_XML_BYTES)?
    else {
        return render_zip_archive_from_zip(zip, logical_name, "", cancel_cb);
    };
    context.check_cancelled()?;
    let opf = match parse_epub_opf_with_context(&context, &opf_xml) {
        Ok(opf)
            if !opf.manifest.is_empty()
                && opf
                    .spine
                    .iter()
                    .any(|idref| opf.manifest.contains_key(idref)) =>
        {
            opf
        }
        Ok(_) | Err(ReaderPreviewError::Malformed) => {
            return render_zip_archive_from_zip(zip, logical_name, "", cancel_cb);
        }
        Err(error) => return Err(error),
    };
    let title = first_non_empty_owned([opf.title.as_str(), filename]).to_string();
    let base_dir = rootfile
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();

    let mut markdown = String::new();
    markdown.push_str("# ");
    markdown.push_str(&markdown_escape_line(&title));
    markdown.push_str("\n\n");
    append_metadata_line(&mut markdown, "Author", &opf.creator);
    append_metadata_line(&mut markdown, "Language", &opf.language);
    append_metadata_line(&mut markdown, "Publisher", &opf.publisher);
    append_metadata_line(&mut markdown, "Identifier", &opf.identifier);
    append_metadata_line(&mut markdown, "Date", &opf.date);
    if !opf.description.trim().is_empty() {
        markdown.push_str("\n> ");
        markdown.push_str(&collapse_ws(&opf.description));
        markdown.push('\n');
    }

    if !opf.spine.is_empty() {
        markdown.push_str("\n## Contents\n\n");
        for idref in opf.spine.iter().take(40) {
            context.check_cancelled()?;
            if let Some(item) = opf.manifest.get(idref) {
                markdown.push_str("- ");
                markdown.push_str(&markdown_escape_line(&ebook_item_label(&item.href)));
                markdown.push('\n');
            }
        }
    }

    let mut extracted = 0usize;
    for idref in &opf.spine {
        context.check_cancelled()?;
        if extracted >= MAX_EBOOK_CHAPTERS || markdown.chars().count() >= MAX_EBOOK_TEXT_CHARS {
            break;
        }
        let Some(item) = opf.manifest.get(idref) else {
            continue;
        };
        if !is_epub_document_item(item) {
            continue;
        }
        let chapter_path = normalize_zip_target(&base_dir, &item.href);
        let Some(chapter_xml) =
            read_ebook_zip_text(&mut context, zip, &chapter_path, MAX_EBOOK_CHAPTER_BYTES)?
        else {
            continue;
        };
        context.check_cancelled()?;
        let chapter = extract_xhtml_markdown_with_context(
            &context,
            &chapter_xml,
            &ebook_item_label(&item.href),
        )?;
        if chapter.trim().is_empty() {
            continue;
        }
        markdown.push_str("\n\n");
        push_markdown_limited(&mut markdown, &chapter, MAX_EBOOK_TEXT_CHARS);
        extracted += 1;
    }

    if extracted == 0 {
        markdown.push_str("\n\n_No readable spine chapters were found. The archive listing is still available by opening the EPUB as a ZIP-compatible file._\n");
    }

    Ok(ebook_markdown_json("epub", &title, markdown))
}

fn read_ebook_zip_text<R: Read + Seek>(
    context: &mut EbookContext,
    zip: &mut ZipArchive<R>,
    name: &str,
    max_size: u64,
) -> Result<Option<String>, ReaderPreviewError> {
    context.check_cancelled()?;
    if let Ok(mut entry) = zip.by_name(name) {
        if entry.size() > max_size {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        let bytes = read_ebook_limited_to_end(context, &mut entry, max_size)?;
        return Ok(Some(String::from_utf8_lossy(&bytes).to_string()));
    }
    context.check_cancelled()?;

    for index in 0..zip.len().min(MAX_EBOOK_ZIP_ENTRIES) {
        context.check_cancelled()?;
        let mut entry = match zip.by_index(index) {
            Ok(entry) => entry,
            Err(_) if preview_cancelled(context.cancel_cb) => {
                return Err(ReaderPreviewError::Cancelled)
            }
            Err(_) => continue,
        };
        if !entry.name().replace('\\', "/").eq_ignore_ascii_case(name) {
            continue;
        }
        if entry.size() > max_size {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        let bytes = read_ebook_limited_to_end(context, &mut entry, max_size)?;
        return Ok(Some(String::from_utf8_lossy(&bytes).to_string()));
    }
    Ok(None)
}

pub(super) fn read_ebook_limited_to_end<R: Read>(
    context: &mut EbookContext,
    reader: &mut R,
    max_size: u64,
) -> Result<Vec<u8>, ReaderPreviewError> {
    let mut bytes = Vec::with_capacity(max_size.min(64 * 1024) as usize);
    let mut buffer = [0u8; 32 * 1024];
    loop {
        context.check_cancelled()?;
        let max_read = buffer.len().min(
            max_size
                .saturating_add(1)
                .saturating_sub(bytes.len() as u64) as usize,
        );
        if max_read == 0 {
            return Err(ReaderPreviewError::LimitExceeded);
        }
        let read = match reader.read(&mut buffer[..max_read]) {
            Ok(read) => read,
            Err(_) if preview_cancelled(context.cancel_cb) => {
                return Err(ReaderPreviewError::Cancelled)
            }
            Err(_) => return Err(ReaderPreviewError::Malformed),
        };
        if read == 0 {
            return Ok(bytes);
        }
        context.consume(read as u64)?;
        bytes.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
pub(super) fn parse_epub_rootfile(xml: &str) -> Option<String> {
    parse_epub_rootfile_with_context(&EbookContext::new(None), xml)
        .ok()
        .flatten()
}

fn parse_epub_rootfile_with_context(
    context: &EbookContext,
    xml: &str,
) -> Result<Option<String>, ReaderPreviewError> {
    let mut reader = Reader::from_str(xml);
    let mut first = None;
    let mut event_count = 0usize;
    loop {
        event_count = event_count.saturating_add(1);
        context.check_xml_event(event_count)?;
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_xml_name(e.name().as_ref()) != "rootfile" {
                    continue;
                }
                let Some(full_path) = attr_value(&e, "full-path") else {
                    continue;
                };
                if first.is_none() {
                    first = Some(full_path.clone());
                }
                let media_type = attr_value(&e, "media-type").unwrap_or_default();
                if media_type.contains("oebps-package") || full_path.ends_with(".opf") {
                    return Ok(Some(full_path));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(ReaderPreviewError::Malformed),
            _ => {}
        }
    }
    Ok(first)
}

fn find_epub_opf_path<R: Read + Seek>(
    context: &EbookContext,
    zip: &mut ZipArchive<R>,
) -> Result<Option<String>, ReaderPreviewError> {
    for i in 0..zip.len().min(512) {
        context.check_cancelled()?;
        let Ok(entry) = zip.by_index_raw(i) else {
            if preview_cancelled(context.cancel_cb) {
                return Err(ReaderPreviewError::Cancelled);
            }
            continue;
        };
        let name = entry.name().replace('\\', "/");
        if name.to_ascii_lowercase().ends_with(".opf") {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(super) fn parse_epub_opf(xml: &str) -> EpubOpf {
    parse_epub_opf_with_context(&EbookContext::new(None), xml).unwrap_or_default()
}

fn parse_epub_opf_with_context(
    context: &EbookContext,
    xml: &str,
) -> Result<EpubOpf, ReaderPreviewError> {
    let mut reader = Reader::from_str(xml);
    let mut opf = EpubOpf::default();
    let mut in_metadata = false;
    let mut current_meta = String::new();
    let mut current_meta_value = String::new();
    let mut event_count = 0usize;

    loop {
        event_count = event_count.saturating_add(1);
        context.check_xml_event(event_count)?;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "metadata" => in_metadata = true,
                    "item" => add_epub_manifest_item(&mut opf, &e),
                    "itemref" => {
                        if let Some(idref) = attr_value(&e, "idref") {
                            opf.spine.push(idref);
                        }
                    }
                    _ if in_metadata => {
                        current_meta = name;
                        current_meta_value.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "item" => add_epub_manifest_item(&mut opf, &e),
                    "itemref" => {
                        if let Some(idref) = attr_value(&e, "idref") {
                            opf.spine.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_metadata && !current_meta.is_empty() => {
                current_meta_value.push_str(&xml_unescape_bytes(e.as_ref()));
            }
            Ok(Event::GeneralRef(e)) if in_metadata && !current_meta.is_empty() => {
                current_meta_value.push_str(&xml_general_ref(e.as_ref()));
            }
            Ok(Event::CData(e)) if in_metadata && !current_meta.is_empty() => {
                current_meta_value.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "metadata" {
                    in_metadata = false;
                }
                if name == current_meta {
                    set_epub_metadata(&mut opf, &current_meta, &current_meta_value);
                    current_meta.clear();
                    current_meta_value.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    Ok(opf)
}

fn add_epub_manifest_item(opf: &mut EpubOpf, e: &BytesStart<'_>) {
    let Some(id) = attr_value(e, "id") else {
        return;
    };
    let Some(href) = attr_value(e, "href") else {
        return;
    };
    let media_type = attr_value(e, "media-type").unwrap_or_default();
    opf.manifest
        .insert(id, EpubManifestItem { href, media_type });
}

fn set_epub_metadata(opf: &mut EpubOpf, name: &str, value: &str) {
    let value = collapse_ws(value);
    if value.is_empty() {
        return;
    }
    match name {
        "title" if opf.title.is_empty() => opf.title = value,
        "creator" if opf.creator.is_empty() => opf.creator = value,
        "language" if opf.language.is_empty() => opf.language = value,
        "publisher" if opf.publisher.is_empty() => opf.publisher = value,
        "identifier" if opf.identifier.is_empty() => opf.identifier = value,
        "date" if opf.date.is_empty() => opf.date = value,
        "description" if opf.description.is_empty() => opf.description = value,
        _ => {}
    }
}

fn is_epub_document_item(item: &EpubManifestItem) -> bool {
    let href = item.href.to_ascii_lowercase();
    item.media_type.contains("html")
        || href.ends_with(".xhtml")
        || href.ends_with(".html")
        || href.ends_with(".htm")
}

#[cfg(test)]
pub(super) fn extract_xhtml_markdown(xml: &str, fallback_title: &str) -> String {
    extract_xhtml_markdown_with_context(&EbookContext::new(None), xml, fallback_title)
        .unwrap_or_default()
}

fn extract_xhtml_markdown_with_context(
    context: &EbookContext,
    xml: &str,
    fallback_title: &str,
) -> Result<String, ReaderPreviewError> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_body = false;
    let mut ignored_depth = 0usize;
    let mut list_depth = 0usize;
    let mut current_block = String::new();
    let mut heading_level = 0usize;
    let mut saw_heading = false;
    let mut event_count = 0usize;
    let mut output_chars = 0usize;

    loop {
        event_count = event_count.saturating_add(1);
        context.check_xml_event(event_count)?;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "body" {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }
                if matches!(name.as_str(), "script" | "style" | "svg" | "head") {
                    ignored_depth += 1;
                    continue;
                }
                if ignored_depth > 0 {
                    continue;
                }
                match name.as_str() {
                    "h1" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            1,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                        heading_level = 2;
                    }
                    "h2" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            1,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                        heading_level = 3;
                    }
                    "h3" | "h4" | "h5" | "h6" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            1,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                        heading_level = 4;
                    }
                    "p" | "div" | "section" | "blockquote" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            0,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                    }
                    "br" => current_block.push('\n'),
                    "ul" | "ol" => list_depth += 1,
                    "li" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            0,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                        current_block.push_str("- ");
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                if !in_body || ignored_depth > 0 {
                    continue;
                }
                let name = local_xml_name(e.name().as_ref());
                if name == "br" {
                    current_block.push('\n');
                }
            }
            Ok(Event::Text(e)) if in_body && ignored_depth == 0 => {
                current_block.push_str(&xml_unescape_bytes(e.as_ref()));
            }
            Ok(Event::GeneralRef(e)) if in_body && ignored_depth == 0 => {
                current_block.push_str(&xml_general_ref(e.as_ref()));
            }
            Ok(Event::CData(e)) if in_body && ignored_depth == 0 => {
                current_block.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                if name == "body" {
                    flush_ebook_block(
                        &mut out,
                        &mut current_block,
                        0,
                        &mut saw_heading,
                        &mut output_chars,
                    );
                    break;
                }
                if ignored_depth > 0 {
                    if matches!(name.as_str(), "script" | "style" | "svg" | "head") {
                        ignored_depth = ignored_depth.saturating_sub(1);
                    }
                    continue;
                }
                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            heading_level,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                        heading_level = 0;
                    }
                    "p" | "div" | "section" | "blockquote" | "li" => {
                        flush_ebook_block(
                            &mut out,
                            &mut current_block,
                            0,
                            &mut saw_heading,
                            &mut output_chars,
                        );
                    }
                    "ul" | "ol" => list_depth = list_depth.saturating_sub(1),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        if output_chars >= MAX_EBOOK_TEXT_CHARS {
            break;
        }
        let _ = list_depth;
    }

    flush_ebook_block(
        &mut out,
        &mut current_block,
        0,
        &mut saw_heading,
        &mut output_chars,
    );
    if !saw_heading && !out.trim().is_empty() {
        let mut rendered = format!("## {}\n\n", markdown_escape_line(fallback_title));
        let remaining = MAX_EBOOK_TEXT_CHARS.saturating_sub(rendered.chars().count());
        rendered.extend(out.trim().chars().take(remaining));
        Ok(rendered)
    } else {
        Ok(out.trim().to_string())
    }
}

fn flush_ebook_block(
    out: &mut String,
    current: &mut String,
    heading_level: usize,
    saw_heading: &mut bool,
    output_chars: &mut usize,
) {
    let text = collapse_ws(current);
    current.clear();
    if text.is_empty() {
        return;
    }
    let mut block = String::new();
    if !out.ends_with("\n\n") && !out.is_empty() {
        block.push_str("\n\n");
    }
    if heading_level > 0 {
        *saw_heading = true;
        block.push_str(&"#".repeat(heading_level));
        block.push(' ');
        block.push_str(&markdown_escape_line(&text));
    } else {
        block.push_str(&text);
    }
    block.push_str("\n\n");
    let remaining = MAX_EBOOK_TEXT_CHARS.saturating_sub(*output_chars);
    if remaining == 0 {
        return;
    }
    let block_chars = block.chars().count();
    if block_chars <= remaining {
        out.push_str(&block);
        *output_chars += block_chars;
    } else {
        out.extend(block.chars().take(remaining));
        *output_chars = MAX_EBOOK_TEXT_CHARS;
    }
}

fn render_fb2_reader<R: Read>(
    reader: &mut R,
    logical_name: &str,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let filename = file_name(logical_name);
    let bytes = read_reader_prefix_cancelable(reader, MAX_EBOOK_XML_BYTES as usize, cancel_cb)?;
    let xml = String::from_utf8_lossy(&bytes);
    let mut reader = Reader::from_str(&xml);
    let context = EbookContext::new(cancel_cb);
    let mut title = String::new();
    let mut lang = String::new();
    let mut author_parts = Vec::<String>::new();
    let mut current_meta = String::new();
    let mut current_meta_value = String::new();
    let mut in_title_info = false;
    let mut in_body = false;
    let mut current_block = String::new();
    let mut markdown = String::new();
    let mut saw_body_heading = false;
    let mut event_count = 0usize;
    let mut markdown_chars = 0usize;

    loop {
        event_count = event_count.saturating_add(1);
        context.check_xml_event(event_count)?;
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "title-info" => in_title_info = true,
                    "body" => in_body = true,
                    "section" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                        &mut markdown_chars,
                    ),
                    "title" if in_body => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            0,
                            &mut saw_body_heading,
                            &mut markdown_chars,
                        );
                        current_meta = "body-title".to_string();
                    }
                    "p" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                        &mut markdown_chars,
                    ),
                    _ if in_title_info => {
                        current_meta = name;
                        current_meta_value.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let value = xml_unescape_bytes(e.as_ref());
                if in_body {
                    current_block.push_str(&value);
                } else if in_title_info && !current_meta.is_empty() {
                    current_meta_value.push_str(&value);
                }
            }
            Ok(Event::GeneralRef(e)) => {
                let value = xml_general_ref(e.as_ref());
                if in_body {
                    current_block.push_str(&value);
                } else if in_title_info && !current_meta.is_empty() {
                    current_meta_value.push_str(&value);
                }
            }
            Ok(Event::CData(e)) => {
                let value = String::from_utf8_lossy(e.as_ref());
                if in_body {
                    current_block.push_str(&value);
                } else if in_title_info && !current_meta.is_empty() {
                    current_meta_value.push_str(&value);
                }
            }
            Ok(Event::End(e)) => {
                let name = local_xml_name(e.name().as_ref());
                match name.as_str() {
                    "title-info" => in_title_info = false,
                    "body" => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            0,
                            &mut saw_body_heading,
                            &mut markdown_chars,
                        );
                        in_body = false;
                    }
                    "title" if current_meta == "body-title" => {
                        flush_ebook_block(
                            &mut markdown,
                            &mut current_block,
                            2,
                            &mut saw_body_heading,
                            &mut markdown_chars,
                        );
                        current_meta.clear();
                    }
                    "p" if in_body => flush_ebook_block(
                        &mut markdown,
                        &mut current_block,
                        0,
                        &mut saw_body_heading,
                        &mut markdown_chars,
                    ),
                    _ if name == current_meta => {
                        set_fb2_metadata(
                            &mut title,
                            &mut lang,
                            &mut author_parts,
                            &current_meta,
                            &current_meta_value,
                        );
                        current_meta.clear();
                        current_meta_value.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        if markdown_chars >= MAX_EBOOK_TEXT_CHARS {
            break;
        }
    }

    let title = first_non_empty_owned([title.as_str(), filename]).to_string();
    let author = author_parts.join(" ");
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&markdown_escape_line(&title));
    out.push_str("\n\n");
    append_metadata_line(&mut out, "Author", &author);
    append_metadata_line(&mut out, "Language", &lang);
    out.push('\n');
    push_markdown_limited(&mut out, markdown.trim(), MAX_EBOOK_TEXT_CHARS);
    Ok(ebook_markdown_json("fb2", &title, out))
}

fn set_fb2_metadata(
    title: &mut String,
    lang: &mut String,
    author_parts: &mut Vec<String>,
    name: &str,
    value: &str,
) {
    let value = collapse_ws(value);
    if value.is_empty() {
        return;
    }
    match name {
        "book-title" if title.is_empty() => *title = value,
        "lang" if lang.is_empty() => *lang = value,
        "first-name" | "middle-name" | "last-name" | "nickname" => author_parts.push(value),
        _ => {}
    }
}

fn render_binary_ebook_info(logical_name: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(logical_name);
    let ext = Path::new(logical_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let mut text = base_info_text(filename, "ebook", size, modified_unix);
    text.push_str(&format!("\nFormat: {ext} ebook"));
    text.push_str("\nContent preview: metadata only for this binary ebook container");
    to_json(&PreviewReadyDto {
        kind: "ebook".to_string(),
        title: format!("{filename} - ebook"),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn ebook_markdown_json(format: &str, title: &str, markdown: String) -> String {
    to_json(&PreviewReadyDto {
        kind: "ebook".to_string(),
        title: format!("{title} - {format}"),
        format: Some("markdown".to_string()),
        language: Some("markdown".to_string()),
        text: Some(markdown),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    })
}

fn append_metadata_line(markdown: &mut String, label: &str, value: &str) {
    let value = collapse_ws(value);
    if !value.is_empty() {
        markdown.push_str(&format!("**{label}:** {value}\n\n"));
    }
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn markdown_escape_line(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

pub(super) fn ebook_item_label(href: &str) -> String {
    let filename = href.rsplit('/').next().unwrap_or(href);
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename);
    collapse_ws(&stem.replace(['_', '-'], " "))
}

fn push_markdown_limited(out: &mut String, value: &str, max_chars: usize) {
    let current = out.chars().count();
    if current >= max_chars {
        return;
    }
    let remaining = max_chars - current;
    let value_chars = value.chars().count();
    if value_chars <= remaining {
        out.push_str(value);
        return;
    }
    out.extend(value.chars().take(remaining));
    out.push_str("\n\n_Preview truncated._");
}

fn first_non_empty_owned<'a, const N: usize>(values: [&'a str; N]) -> &'a str {
    values
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("")
}
