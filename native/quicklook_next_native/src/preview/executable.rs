//! Executable, PE, CLR, and Authenticode preview parsing.

use super::*;

// ── Executable preview ──────────────────────────────────────────────────────

pub fn render_executable(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }
    let (size, modified_unix) = file_size_modified(path);
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    render_executable_reader(&mut file, path, size, modified_unix, cancel_cb).unwrap_or_default()
}

pub fn render_executable_reader<R: Read>(
    reader: &mut R,
    logical_name: &str,
    size: i64,
    modified_unix: i64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<String, ReaderPreviewError> {
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }
    let filename = Path::new(logical_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let bytes = read_reader_prefix_cancelable(reader, MAX_EXECUTABLE_HEADER_BYTES, cancel_cb)?;

    let Some(pe) = parse_pe_headers(&bytes, cancel_cb) else {
        if preview_cancelled(cancel_cb) {
            return Err(ReaderPreviewError::Cancelled);
        }
        return Ok(render_info(logical_name, "executable", size, modified_unix));
    };
    if preview_cancelled(cancel_cb) {
        return Err(ReaderPreviewError::Cancelled);
    }

    let mut text = String::new();
    text.push_str(&format!("Name: {filename}\n"));
    text.push_str("Kind: executable\n");
    text.push_str(&format!("Format: {}\n", pe.format));
    text.push_str(&format!("Machine: {}\n", pe.machine));
    text.push_str(&format!("Subsystem: {}\n", pe.subsystem));
    text.push_str(&format!("Sections: {}\n", pe.sections));
    text.push_str(&format!("Entry point RVA: 0x{:08X}\n", pe.entry_point));
    text.push_str(&format!(
        "Image size: {}\n",
        format_bytes(pe.image_size as i64)
    ));
    if pe.link_timestamp > 0 {
        text.push_str(&format!(
            "Link time: {}\n",
            format_timestamp(pe.link_timestamp as i64)
        ));
    }
    text.push_str(&format!("Characteristics: 0x{:04X}\n", pe.characteristics));
    text.push_str(&format!("Image base: 0x{:016X}\n", pe.image_base));
    text.push_str(&format!(
        "Section alignment: {}\n",
        format_bytes(pe.section_alignment as i64)
    ));
    text.push_str(&format!(
        "File alignment: {}\n",
        format_bytes(pe.file_alignment as i64)
    ));
    if pe.dll_characteristics > 0 {
        text.push_str(&format!(
            "DLL characteristics: 0x{:04X}\n",
            pe.dll_characteristics
        ));
    }
    if pe.data_directories > 0 {
        text.push_str(&format!("Data directories: {}\n", pe.data_directories));
    }
    for directory in &pe.directories {
        text.push_str(&format!(
            "{} directory: 0x{:08X}, {}\n",
            directory.name,
            directory.address,
            format_bytes(directory.size as i64)
        ));
    }
    if !pe.section_names.is_empty() {
        text.push_str(&format!("Section names: {}\n", pe.section_names.join(", ")));
    }
    if !pe.exports.is_empty() {
        text.push_str(&format!("Exports: {}\n", pe.exports.join(", ")));
    }
    if !pe.export_details.is_empty() {
        text.push_str(&format!(
            "Export details: {}\n",
            pe.export_details.join(", ")
        ));
    }
    if pe.has_version_resource {
        text.push_str("Version resource: present\n");
    }
    for (name, value) in &pe.version_strings {
        text.push_str(&format!("Version {name}: {value}\n"));
    }
    if let Some(fixed) = &pe.fixed_version {
        text.push_str(&format!(
            "Fixed file version: {}; product {}; flags 0x{:08X}; type {}\n",
            fixed.file_version, fixed.product_version, fixed.flags, fixed.file_type
        ));
    }
    if let Some(certificate) = &pe.certificate {
        text.push_str(&format!(
            "Certificate table: {}, revision 0x{:04X}, type 0x{:04X}\n",
            format_bytes(certificate.length as i64),
            certificate.revision,
            certificate.typ
        ));
        if !certificate.digest_algorithms.is_empty() {
            text.push_str(&format!(
                "Certificate digest algorithms: {}\n",
                certificate.digest_algorithms.join(", ")
            ));
        }
        if !certificate.signature_algorithms.is_empty() {
            text.push_str(&format!(
                "Certificate signature algorithms: {}\n",
                certificate.signature_algorithms.join(", ")
            ));
        }
        if !certificate.signers.is_empty() {
            text.push_str(&format!(
                "Certificate signers: {}\n",
                certificate.signers.join(", ")
            ));
        }
        if !certificate.names.is_empty() {
            text.push_str(&format!(
                "Certificate names: {}\n",
                certificate.names.join(", ")
            ));
        }
        if !certificate.issuers.is_empty() {
            text.push_str(&format!(
                "Certificate issuers: {}\n",
                certificate.issuers.join(", ")
            ));
        }
        if !certificate.subjects.is_empty() {
            text.push_str(&format!(
                "Certificate subjects: {}\n",
                certificate.subjects.join(", ")
            ));
        }
    }
    if let Some(clr) = &pe.clr {
        text.push_str(&format!(
            "CLR runtime: {}.{}; metadata 0x{:08X}, {}; flags 0x{:08X}\n",
            clr.major,
            clr.minor,
            clr.metadata_rva,
            format_bytes(clr.metadata_size as i64),
            clr.flags
        ));
        if !clr.metadata_version.is_empty() {
            text.push_str(&format!("CLR metadata version: {}\n", clr.metadata_version));
        }
        if !clr.metadata_streams.is_empty() {
            text.push_str(&format!(
                "CLR metadata streams: {}\n",
                clr.metadata_streams.join(", ")
            ));
        }
        if !clr.metadata_tables.is_empty() {
            text.push_str(&format!(
                "CLR metadata tables: {}\n",
                clr.metadata_tables.join(", ")
            ));
        }
        if let Some(assembly) = &clr.assembly {
            text.push_str(&format!("CLR assembly: {assembly}\n"));
        }
        if !clr.assembly_refs.is_empty() {
            text.push_str(&format!(
                "CLR assembly references: {}\n",
                clr.assembly_refs.join(", ")
            ));
        }
        if !clr.type_defs.is_empty() {
            text.push_str(&format!(
                "CLR type definitions: {}\n",
                clr.type_defs.join(", ")
            ));
        }
        if clr.custom_attributes > 0 {
            text.push_str(&format!(
                "CLR custom attributes: {}\n",
                clr.custom_attributes
            ));
        }
    }
    text.push_str(&format!("File size: {}\n", format_bytes(size)));
    text.push_str(&format!("Modified: {}\n", format_timestamp(modified_unix)));

    Ok(to_json(&PreviewReadyDto {
        kind: "executable".to_string(),
        title: format!("{filename} - {}", pe.machine),
        format: Some("plain".to_string()),
        language: Some("text".to_string()),
        text: Some(text),
        office_layout: None,
        listing: None,
        table: None,
        markdown: None,
    }))
}

pub(super) struct PeSummary {
    pub(super) machine: &'static str,
    pub(super) format: &'static str,
    pub(super) subsystem: &'static str,
    pub(super) sections: u16,
    pub(super) entry_point: u32,
    pub(super) image_size: u32,
    pub(super) link_timestamp: u32,
    pub(super) characteristics: u16,
    pub(super) image_base: u64,
    pub(super) section_alignment: u32,
    pub(super) file_alignment: u32,
    pub(super) dll_characteristics: u16,
    pub(super) data_directories: u32,
    pub(super) section_names: Vec<String>,
    pub(super) directories: Vec<PeDataDirectory>,
    #[cfg(test)]
    pub(super) imports: Vec<String>,
    #[cfg(test)]
    pub(super) imported_functions: Vec<String>,
    pub(super) exports: Vec<String>,
    pub(super) export_details: Vec<String>,
    pub(super) has_version_resource: bool,
    pub(super) version_strings: Vec<(String, String)>,
    pub(super) fixed_version: Option<PeFixedVersion>,
    pub(super) certificate: Option<PeCertificateSummary>,
    pub(super) clr: Option<PeClrSummary>,
}

pub(super) struct PeDataDirectory {
    pub(super) name: &'static str,
    pub(super) address: u32,
    pub(super) size: u32,
}

struct PeSectionSummary {
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

pub(super) struct PeCertificateSummary {
    pub(super) length: u32,
    pub(super) revision: u16,
    pub(super) typ: u16,
    pub(super) digest_algorithms: Vec<String>,
    pub(super) signature_algorithms: Vec<String>,
    pub(super) signers: Vec<String>,
    pub(super) names: Vec<String>,
    pub(super) issuers: Vec<String>,
    pub(super) subjects: Vec<String>,
}

pub(super) struct PeFixedVersion {
    pub(super) file_version: String,
    pub(super) product_version: String,
    pub(super) flags: u32,
    pub(super) file_type: &'static str,
}

pub(super) struct PeClrSummary {
    pub(super) major: u16,
    pub(super) minor: u16,
    pub(super) metadata_rva: u32,
    pub(super) metadata_size: u32,
    pub(super) flags: u32,
    pub(super) metadata_version: String,
    pub(super) metadata_streams: Vec<String>,
    pub(super) metadata_tables: Vec<String>,
    pub(super) assembly: Option<String>,
    pub(super) assembly_refs: Vec<String>,
    pub(super) type_defs: Vec<String>,
    pub(super) custom_attributes: u32,
}

pub(super) fn parse_pe_headers(
    bytes: &[u8],
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Option<PeSummary> {
    if preview_cancelled(cancel_cb) {
        return None;
    }
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return None;
    }
    let pe_offset = read_u32(bytes, 0x3C)? as usize;
    if pe_offset.checked_add(24)? > bytes.len() || &bytes[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return None;
    }

    let coff = pe_offset + 4;
    let machine = read_u16(bytes, coff)?;
    let sections = read_u16(bytes, coff + 2)?;
    let timestamp = read_u32(bytes, coff + 4)?;
    let opt_size = read_u16(bytes, coff + 16)? as usize;
    let characteristics = read_u16(bytes, coff + 18)?;
    let opt = coff + 20;
    if opt.checked_add(opt_size)? > bytes.len() || opt_size < 70 {
        return None;
    }

    let magic = read_u16(bytes, opt)?;
    let entry_point = read_u32(bytes, opt + 16).unwrap_or(0);
    let image_base = if magic == 0x20B {
        read_u64(bytes, opt + 24).unwrap_or(0)
    } else {
        read_u32(bytes, opt + 28).unwrap_or(0) as u64
    };
    let section_alignment = read_u32(bytes, opt + 32).unwrap_or(0);
    let file_alignment = read_u32(bytes, opt + 36).unwrap_or(0);
    let image_size = read_u32(bytes, opt + 56).unwrap_or(0);
    let subsystem = read_u16(bytes, opt + 68).unwrap_or(0);
    let dll_characteristics = read_u16(bytes, opt + 70).unwrap_or(0);
    let data_directories_offset = if magic == 0x20B { opt + 108 } else { opt + 92 };
    let data_directories = read_u32(bytes, data_directories_offset).unwrap_or(0);
    let directories =
        parse_pe_data_directories(bytes, data_directories_offset + 4, data_directories);
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let section_table = opt + opt_size;
    let section_names = parse_pe_section_names(bytes, section_table, sections);
    let section_summaries = parse_pe_sections(bytes, section_table, sections);
    if preview_cancelled(cancel_cb) {
        return None;
    }
    #[cfg(test)]
    let imports = directories
        .iter()
        .find(|directory| directory.name == "Import")
        .map(|directory| parse_pe_import_dlls(bytes, &section_summaries, directory.address))
        .unwrap_or_default();
    #[cfg(test)]
    let imported_functions = directories
        .iter()
        .find(|directory| directory.name == "Import")
        .map(|directory| {
            parse_pe_import_functions(bytes, &section_summaries, directory.address, magic == 0x20B)
        })
        .unwrap_or_default();
    let exports = directories
        .iter()
        .find(|directory| directory.name == "Export")
        .map(|directory| parse_pe_export_names(bytes, &section_summaries, directory.address))
        .unwrap_or_default();
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let export_details = directories
        .iter()
        .find(|directory| directory.name == "Export")
        .map(|directory| {
            parse_pe_export_details(bytes, &section_summaries, directory.address, directory.size)
        })
        .unwrap_or_default();
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let version_resource = directories
        .iter()
        .find(|directory| directory.name == "Resource")
        .and_then(|directory| {
            pe_rva_to_file_offset(&section_summaries, directory.address)
                .and_then(|offset| pe_find_resource_data(bytes, &section_summaries, offset, 16))
        });
    let version_strings = version_resource
        .and_then(|(offset, size)| {
            bytes
                .get(offset..offset.saturating_add(size))
                .map(parse_pe_version_strings)
        })
        .unwrap_or_default();
    let fixed_version = version_resource
        .and_then(|(offset, size)| bytes.get(offset..offset.saturating_add(size)))
        .and_then(parse_pe_fixed_version);
    let has_version_resource = version_resource.is_some();
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let certificate = directories
        .iter()
        .find(|directory| directory.name == "Certificate")
        .and_then(|directory| parse_pe_certificate(bytes, directory.address));
    if preview_cancelled(cancel_cb) {
        return None;
    }
    let clr = directories
        .iter()
        .find(|directory| directory.name == "CLR")
        .and_then(|directory| pe_rva_to_file_offset(&section_summaries, directory.address))
        .and_then(|offset| parse_pe_clr_header(bytes, &section_summaries, offset));
    if preview_cancelled(cancel_cb) {
        return None;
    }

    Some(PeSummary {
        machine: machine_name(machine),
        format: match magic {
            0x10B => "PE32",
            0x20B => "PE32+",
            _ => "PE",
        },
        subsystem: subsystem_name(subsystem),
        sections,
        entry_point,
        image_size,
        link_timestamp: timestamp,
        characteristics,
        image_base,
        section_alignment,
        file_alignment,
        dll_characteristics,
        data_directories,
        section_names,
        directories,
        #[cfg(test)]
        imports,
        #[cfg(test)]
        imported_functions,
        exports,
        export_details,
        has_version_resource,
        version_strings,
        fixed_version,
        certificate,
        clr,
    })
}

fn parse_pe_data_directories(bytes: &[u8], offset: usize, count: u32) -> Vec<PeDataDirectory> {
    let names = [
        "Export",
        "Import",
        "Resource",
        "Exception",
        "Certificate",
        "Base relocation",
        "Debug",
        "Architecture",
        "Global pointer",
        "TLS",
        "Load config",
        "Bound import",
        "IAT",
        "Delay import",
        "CLR",
        "Reserved",
    ];
    let mut directories = Vec::new();
    for index in 0..count.min(names.len() as u32) as usize {
        let entry = offset + index * 8;
        let Some(address) = read_u32(bytes, entry) else {
            break;
        };
        let Some(size) = read_u32(bytes, entry + 4) else {
            break;
        };
        if address != 0 || size != 0 {
            directories.push(PeDataDirectory {
                name: names[index],
                address,
                size,
            });
        }
    }
    directories
}

fn parse_pe_section_names(bytes: &[u8], section_table: usize, sections: u16) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..sections.min(12) as usize {
        let offset = section_table + index * 40;
        let Some(raw) = bytes.get(offset..offset + 8) else {
            break;
        };
        let name = String::from_utf8_lossy(raw)
            .trim_matches('\0')
            .trim()
            .to_string();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

fn parse_pe_sections(bytes: &[u8], section_table: usize, sections: u16) -> Vec<PeSectionSummary> {
    let mut summaries = Vec::new();
    for index in 0..sections.min(96) as usize {
        let offset = section_table + index * 40;
        if offset + 40 > bytes.len() {
            break;
        }
        summaries.push(PeSectionSummary {
            virtual_size: read_u32(bytes, offset + 8).unwrap_or(0),
            virtual_address: read_u32(bytes, offset + 12).unwrap_or(0),
            raw_size: read_u32(bytes, offset + 16).unwrap_or(0),
            raw_pointer: read_u32(bytes, offset + 20).unwrap_or(0),
        });
    }
    summaries
}

#[cfg(test)]
fn parse_pe_import_dlls(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    import_rva: u32,
) -> Vec<String> {
    let Some(mut offset) = pe_rva_to_file_offset(sections, import_rva) else {
        return Vec::new();
    };
    let mut imports: Vec<String> = Vec::new();
    for _ in 0..64 {
        if offset + 20 > bytes.len() {
            break;
        }
        let original_first_thunk = read_u32(bytes, offset).unwrap_or(0);
        let name_rva = read_u32(bytes, offset + 12).unwrap_or(0);
        let first_thunk = read_u32(bytes, offset + 16).unwrap_or(0);
        if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        if let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) {
            if let Some(name) = read_c_string(bytes, name_offset, 260) {
                if !name.is_empty()
                    && !imports
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(&name))
                {
                    imports.push(name);
                }
            }
        }
        offset += 20;
    }
    imports
}

#[cfg(test)]
fn parse_pe_import_functions(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    import_rva: u32,
    pe64: bool,
) -> Vec<String> {
    let Some(mut offset) = pe_rva_to_file_offset(sections, import_rva) else {
        return Vec::new();
    };
    let mut functions = Vec::new();
    for _ in 0..64 {
        if offset + 20 > bytes.len() {
            break;
        }
        let original_first_thunk = read_u32(bytes, offset).unwrap_or(0);
        let name_rva = read_u32(bytes, offset + 12).unwrap_or(0);
        let first_thunk = read_u32(bytes, offset + 16).unwrap_or(0);
        if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        let dll = pe_rva_to_file_offset(sections, name_rva)
            .and_then(|name_offset| read_c_string(bytes, name_offset, 260))
            .unwrap_or_default();
        let thunk_rva = if original_first_thunk != 0 {
            original_first_thunk
        } else {
            first_thunk
        };
        append_pe_import_thunks(bytes, sections, thunk_rva, pe64, &dll, &mut functions);
        offset += 20;
    }
    functions
}

#[cfg(test)]
fn append_pe_import_thunks(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    thunk_rva: u32,
    pe64: bool,
    dll: &str,
    functions: &mut Vec<String>,
) {
    let Some(mut offset) = pe_rva_to_file_offset(sections, thunk_rva) else {
        return;
    };
    let thunk_size = if pe64 { 8 } else { 4 };
    for _ in 0..128 {
        let value = if pe64 {
            read_u64(bytes, offset).unwrap_or(0)
        } else {
            read_u32(bytes, offset).unwrap_or(0) as u64
        };
        if value == 0 {
            break;
        }
        let ordinal_mask = if pe64 {
            0x8000_0000_0000_0000
        } else {
            0x8000_0000
        };
        if value & ordinal_mask == 0 {
            if let Some(name_offset) = pe_rva_to_file_offset(sections, value as u32) {
                if let Some(name) = read_c_string(bytes, name_offset + 2, 260) {
                    if !name.is_empty() {
                        let qualified = if dll.is_empty() {
                            name
                        } else {
                            format!("{dll}!{name}")
                        };
                        if !functions
                            .iter()
                            .any(|existing| existing.eq_ignore_ascii_case(&qualified))
                        {
                            functions.push(qualified);
                        }
                    }
                }
            }
        } else {
            let ordinal = value & 0xFFFF;
            let qualified = if dll.is_empty() {
                format!("#{ordinal}")
            } else {
                format!("{dll}!#{ordinal}")
            };
            if !functions
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&qualified))
            {
                functions.push(qualified);
            }
        }
        offset += thunk_size;
    }
}

fn parse_pe_export_names(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    export_rva: u32,
) -> Vec<String> {
    let Some(offset) = pe_rva_to_file_offset(sections, export_rva) else {
        return Vec::new();
    };
    if offset + 40 > bytes.len() {
        return Vec::new();
    }
    let names = read_u32(bytes, offset + 24).unwrap_or(0).min(256) as usize;
    let names_rva = read_u32(bytes, offset + 32).unwrap_or(0);
    let Some(names_offset) = pe_rva_to_file_offset(sections, names_rva) else {
        return Vec::new();
    };
    let mut exports = Vec::new();
    for index in 0..names {
        let Some(name_rva) = read_u32(bytes, names_offset + index * 4) else {
            break;
        };
        if let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) {
            if let Some(name) = read_c_string(bytes, name_offset, 260) {
                if !name.is_empty() {
                    exports.push(name);
                }
            }
        }
    }
    exports
}

fn parse_pe_export_details(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    export_rva: u32,
    export_size: u32,
) -> Vec<String> {
    let Some(offset) = pe_rva_to_file_offset(sections, export_rva) else {
        return Vec::new();
    };
    if offset + 40 > bytes.len() {
        return Vec::new();
    }
    let ordinal_base = read_u32(bytes, offset + 16).unwrap_or(0);
    let function_count = read_u32(bytes, offset + 20).unwrap_or(0).min(4096) as usize;
    let name_count = read_u32(bytes, offset + 24).unwrap_or(0).min(256) as usize;
    let functions_rva = read_u32(bytes, offset + 28).unwrap_or(0);
    let names_rva = read_u32(bytes, offset + 32).unwrap_or(0);
    let ordinals_rva = read_u32(bytes, offset + 36).unwrap_or(0);
    let Some(functions_offset) = pe_rva_to_file_offset(sections, functions_rva) else {
        return Vec::new();
    };
    let Some(names_offset) = pe_rva_to_file_offset(sections, names_rva) else {
        return Vec::new();
    };
    let Some(ordinals_offset) = pe_rva_to_file_offset(sections, ordinals_rva) else {
        return Vec::new();
    };
    let mut details = Vec::new();
    for index in 0..name_count {
        let Some(name_rva) = read_u32(bytes, names_offset + index * 4) else {
            break;
        };
        let Some(name_offset) = pe_rva_to_file_offset(sections, name_rva) else {
            continue;
        };
        let Some(name) = read_c_string(bytes, name_offset, 260) else {
            continue;
        };
        let ordinal_index = read_u16(bytes, ordinals_offset + index * 2).unwrap_or(0) as usize;
        if ordinal_index >= function_count {
            continue;
        }
        let function_rva = read_u32(bytes, functions_offset + ordinal_index * 4).unwrap_or(0);
        let ordinal = ordinal_base + ordinal_index as u32;
        if function_rva >= export_rva && function_rva < export_rva.saturating_add(export_size) {
            if let Some(forwarder_offset) = pe_rva_to_file_offset(sections, function_rva) {
                if let Some(forwarder) = read_c_string(bytes, forwarder_offset, 260) {
                    details.push(format!("{name} #{ordinal} -> {forwarder}"));
                    continue;
                }
            }
        }
        details.push(format!("{name} #{ordinal} @ 0x{function_rva:08X}"));
    }
    details
}

fn pe_find_resource_data(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    resource_root: usize,
    typ: u16,
) -> Option<(usize, usize)> {
    pe_find_resource_data_in_directory(bytes, sections, resource_root, resource_root, typ, 0)
}

fn pe_find_resource_data_in_directory(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    root: usize,
    directory: usize,
    typ: u16,
    depth: usize,
) -> Option<(usize, usize)> {
    if depth > 2 || directory + 16 > bytes.len() {
        return None;
    }
    let named = read_u16(bytes, directory + 12).unwrap_or(0) as usize;
    let ids = read_u16(bytes, directory + 14).unwrap_or(0) as usize;
    let entries = named.saturating_add(ids).min(256);
    for index in 0..entries {
        let entry = directory + 16 + index * 8;
        if entry + 8 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, entry).unwrap_or(0);
        if depth == 0 && (id & 0x8000_0000 != 0 || (id & 0xFFFF) as u16 != typ) {
            continue;
        }
        let target = read_u32(bytes, entry + 4).unwrap_or(0);
        if target & 0x8000_0000 != 0 {
            let child = root + (target & 0x7FFF_FFFF) as usize;
            if let Some(found) =
                pe_find_resource_data_in_directory(bytes, sections, root, child, typ, depth + 1)
            {
                return Some(found);
            }
        } else {
            let data_entry = root + target as usize;
            if data_entry + 16 > bytes.len() {
                continue;
            }
            let data_rva = read_u32(bytes, data_entry).unwrap_or(0);
            let size = read_u32(bytes, data_entry + 4).unwrap_or(0) as usize;
            if let Some(data_offset) = pe_rva_to_file_offset(sections, data_rva) {
                return Some((data_offset, size));
            }
        }
    }
    None
}

fn parse_pe_version_strings(bytes: &[u8]) -> Vec<(String, String)> {
    let mut strings = Vec::new();
    parse_pe_version_node(bytes, 0, bytes.len(), &mut strings);
    strings.sort_by(|a, b| a.0.cmp(&b.0));
    strings.dedup_by(|a, b| a.0 == b.0);
    strings
}

fn parse_pe_fixed_version(bytes: &[u8]) -> Option<PeFixedVersion> {
    if bytes.len() < 6 {
        return None;
    }
    let length = read_u16(bytes, 0)? as usize;
    let value_len = read_u16(bytes, 2)? as usize;
    let typ = read_u16(bytes, 4).unwrap_or(0);
    if length == 0 || length > bytes.len() || typ != 0 || value_len < 52 {
        return None;
    }
    let (key, key_end) = read_utf16_z(bytes, 6, length)?;
    if key != "VS_VERSION_INFO" {
        return None;
    }
    let value_offset = align4(key_end);
    if value_offset + 52 > length || read_u32(bytes, value_offset)? != 0xFEEF_04BD {
        return None;
    }
    let file_ms = read_u32(bytes, value_offset + 8)?;
    let file_ls = read_u32(bytes, value_offset + 12)?;
    let product_ms = read_u32(bytes, value_offset + 16)?;
    let product_ls = read_u32(bytes, value_offset + 20)?;
    let flags_mask = read_u32(bytes, value_offset + 24).unwrap_or(0);
    let flags = read_u32(bytes, value_offset + 28).unwrap_or(0) & flags_mask;
    let file_type = read_u32(bytes, value_offset + 36).unwrap_or(0);
    Some(PeFixedVersion {
        file_version: format_pe_version(file_ms, file_ls),
        product_version: format_pe_version(product_ms, product_ls),
        flags,
        file_type: pe_version_file_type(file_type),
    })
}

pub(super) fn format_pe_version(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)
}

pub(super) fn pe_version_file_type(value: u32) -> &'static str {
    match value {
        1 => "application",
        2 => "DLL",
        3 => "driver",
        4 => "font",
        5 => "VxD",
        7 => "static library",
        _ => "unknown",
    }
}

fn parse_pe_version_node(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    strings: &mut Vec<(String, String)>,
) -> Option<usize> {
    if offset + 6 > limit || offset + 6 > bytes.len() {
        return None;
    }
    let length = read_u16(bytes, offset)? as usize;
    if length == 0 || offset + length > limit || offset + length > bytes.len() {
        return None;
    }
    let value_len = read_u16(bytes, offset + 2)? as usize;
    let typ = read_u16(bytes, offset + 4).unwrap_or(0);
    let (key, key_end) = read_utf16_z(bytes, offset + 6, offset + length)?;
    let value_offset = align4(key_end);
    let value_bytes = if typ == 1 {
        value_len.saturating_mul(2)
    } else {
        value_len
    };
    if typ == 1 && value_len > 0 && is_version_string_key(&key) {
        let value_end = value_offset
            .saturating_add(value_bytes)
            .min(offset + length);
        if let Some(raw) = bytes.get(value_offset..value_end) {
            let value = decode_utf16le_string(raw);
            if !value.is_empty() {
                strings.push((key.clone(), value));
            }
        }
    }
    let mut child = align4(value_offset.saturating_add(value_bytes));
    while child + 6 <= offset + length {
        let Some(next) = parse_pe_version_node(bytes, child, offset + length, strings) else {
            break;
        };
        if next <= child {
            break;
        }
        child = next;
    }
    Some(align4(offset + length))
}

fn is_version_string_key(key: &str) -> bool {
    matches!(
        key,
        "CompanyName"
            | "FileDescription"
            | "FileVersion"
            | "InternalName"
            | "OriginalFilename"
            | "ProductName"
            | "ProductVersion"
    )
}

fn read_utf16_z(bytes: &[u8], offset: usize, limit: usize) -> Option<(String, usize)> {
    let mut pos = offset;
    let mut units = Vec::new();
    while pos + 2 <= limit && pos + 2 <= bytes.len() {
        let unit = read_u16(bytes, pos)?;
        pos += 2;
        if unit == 0 {
            return Some((String::from_utf16_lossy(&units), pos));
        }
        units.push(unit);
    }
    None
}

fn decode_utf16le_string(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string()
}

pub(super) fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn parse_pe_certificate(bytes: &[u8], file_offset: u32) -> Option<PeCertificateSummary> {
    let offset = file_offset as usize;
    if offset + 8 > bytes.len() {
        return None;
    }
    let length = read_u32(bytes, offset).unwrap_or(0) as usize;
    let (issuers, subjects) = parse_authenticode_certificate_subjects(bytes, offset, length);
    Some(PeCertificateSummary {
        length: read_u32(bytes, offset)?,
        revision: read_u16(bytes, offset + 4)?,
        typ: read_u16(bytes, offset + 6)?,
        digest_algorithms: parse_authenticode_digest_algorithms(bytes, offset, length),
        signature_algorithms: parse_authenticode_signature_algorithms(bytes, offset, length),
        signers: parse_authenticode_signers(bytes, offset, length),
        names: parse_authenticode_certificate_names(bytes, offset, length),
        issuers,
        subjects,
    })
}

fn parse_authenticode_digest_algorithms(bytes: &[u8], offset: usize, length: usize) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let oid_patterns: [(&str, &[u8]); 4] = [
        ("SHA-1", &[0x06, 0x05, 0x2B, 0x0E, 0x03, 0x02, 0x1A]),
        (
            "SHA-256",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
            ],
        ),
        (
            "SHA-384",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02,
            ],
        ),
        (
            "SHA-512",
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
            ],
        ),
    ];
    let mut algorithms = Vec::new();
    for (name, pattern) in oid_patterns {
        if payload
            .windows(pattern.len())
            .any(|window| window == pattern)
        {
            algorithms.push(name.to_string());
        }
    }
    algorithms
}

pub(super) fn parse_authenticode_signers(
    bytes: &[u8],
    offset: usize,
    length: usize,
) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let signed_data_oid = [
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
    ];
    if !payload
        .windows(signed_data_oid.len())
        .any(|window| window == signed_data_oid)
    {
        return Vec::new();
    }
    let digest = first_oid_name(
        payload,
        &[
            ("SHA-1", &[0x06, 0x05, 0x2B, 0x0E, 0x03, 0x02, 0x1A][..]),
            (
                "SHA-256",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
                ][..],
            ),
            (
                "SHA-384",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02,
                ][..],
            ),
            (
                "SHA-512",
                &[
                    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
                ][..],
            ),
        ],
    );
    let signature = first_oid_name(
        payload,
        &[
            (
                "SHA-1 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05,
                ][..],
            ),
            (
                "SHA-256 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
                ][..],
            ),
            (
                "SHA-384 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C,
                ][..],
            ),
            (
                "SHA-512 with RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D,
                ][..],
            ),
            (
                "RSA",
                &[
                    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
                ][..],
            ),
            (
                "ECDSA with SHA-256",
                &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02][..],
            ),
            (
                "ECDSA with SHA-384",
                &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03][..],
            ),
        ],
    );
    match (digest, signature) {
        (Some(digest), Some(signature)) => vec![format!("digest {digest}; signature {signature}")],
        (Some(digest), None) => vec![format!("digest {digest}")],
        (None, Some(signature)) => vec![format!("signature {signature}")],
        (None, None) => Vec::new(),
    }
}

fn first_oid_name(bytes: &[u8], patterns: &[(&'static str, &[u8])]) -> Option<&'static str> {
    patterns.iter().find_map(|(name, pattern)| {
        bytes
            .windows(pattern.len())
            .any(|window| window == *pattern)
            .then_some(*name)
    })
}

fn parse_authenticode_certificate_names(bytes: &[u8], offset: usize, length: usize) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let name_oids: [(&str, &[u8]); 3] = [
        ("CN", &[0x06, 0x03, 0x55, 0x04, 0x03]),
        ("O", &[0x06, 0x03, 0x55, 0x04, 0x0A]),
        ("OU", &[0x06, 0x03, 0x55, 0x04, 0x0B]),
    ];
    let mut names = Vec::new();
    for (label, oid) in name_oids {
        let mut search = 0usize;
        while search + oid.len() + 2 <= payload.len() && names.len() < 12 {
            let Some(position) = payload[search..]
                .windows(oid.len())
                .position(|window| window == oid)
            else {
                break;
            };
            let value_offset = search + position + oid.len();
            if let Some(value) = read_der_string(payload, value_offset) {
                let entry = format!("{label}={value}");
                if !names.iter().any(|existing| existing == &entry) {
                    names.push(entry);
                }
            }
            search = value_offset.saturating_add(1);
        }
    }
    names
}

pub(super) fn parse_authenticode_certificate_subjects(
    bytes: &[u8],
    offset: usize,
    length: usize,
) -> (Vec<String>, Vec<String>) {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return (Vec::new(), Vec::new());
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let mut issuers = Vec::new();
    let mut subjects = Vec::new();
    let mut search = 0usize;
    while search + 4 < payload.len() && subjects.len() < 4 {
        let Some(position) = payload[search..].iter().position(|byte| *byte == 0x30) else {
            break;
        };
        let cert_offset = search + position;
        if let Some((issuer, subject, cert_end)) =
            parse_x509_certificate_names(payload, cert_offset)
        {
            if !issuer.is_empty() && !issuers.contains(&issuer) {
                issuers.push(issuer);
            }
            if !subject.is_empty() && !subjects.contains(&subject) {
                subjects.push(subject);
            }
            search = cert_end.max(cert_offset + 1);
        } else {
            search = cert_offset + 1;
        }
    }
    (issuers, subjects)
}

fn parse_x509_certificate_names(bytes: &[u8], offset: usize) -> Option<(String, String, usize)> {
    let (cert_content, cert_end) = der_tlv_content(bytes, offset, 0x30)?;
    let cert = bytes.get(cert_content..cert_end)?;
    let (tbs_content, tbs_end_rel) = der_tlv_content(cert, 0, 0x30)?;
    let tbs = cert.get(tbs_content..tbs_end_rel)?;
    let mut cursor = 0usize;
    if tbs.get(cursor) == Some(&0xA0) {
        let (_, next) = der_tlv_content(tbs, cursor, 0xA0)?;
        cursor = next;
    }
    for _ in 0..2 {
        let (_, next) = der_any_tlv_content(tbs, cursor)?;
        cursor = next;
    }
    let (issuer_content, issuer_end) = der_tlv_content(tbs, cursor, 0x30)?;
    let issuer = parse_x509_name(&tbs[issuer_content..issuer_end]);
    cursor = issuer_end;
    let (_, next) = der_tlv_content(tbs, cursor, 0x30)?;
    cursor = next;
    let (subject_content, subject_end) = der_tlv_content(tbs, cursor, 0x30)?;
    let subject = parse_x509_name(&tbs[subject_content..subject_end]);
    Some((issuer, subject, cert_end))
}

fn parse_x509_name(bytes: &[u8]) -> String {
    let name_oids: [(&str, &[u8]); 3] = [
        ("CN", &[0x06, 0x03, 0x55, 0x04, 0x03]),
        ("O", &[0x06, 0x03, 0x55, 0x04, 0x0A]),
        ("OU", &[0x06, 0x03, 0x55, 0x04, 0x0B]),
    ];
    let mut parts = Vec::new();
    for (label, oid) in name_oids {
        let mut search = 0usize;
        while search + oid.len() + 2 <= bytes.len() && parts.len() < 8 {
            let Some(position) = bytes[search..]
                .windows(oid.len())
                .position(|window| window == oid)
            else {
                break;
            };
            let value_offset = search + position + oid.len();
            if let Some(value) = read_der_string(bytes, value_offset) {
                let entry = format!("{label}={value}");
                if !parts.contains(&entry) {
                    parts.push(entry);
                }
            }
            search = value_offset.saturating_add(1);
        }
    }
    parts.join("/")
}

fn der_any_tlv_content(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let _tag = *bytes.get(offset)?;
    der_tlv_bounds(bytes, offset).map(|(content, end)| (content, end))
}

fn der_tlv_content(bytes: &[u8], offset: usize, tag: u8) -> Option<(usize, usize)> {
    (*bytes.get(offset)? == tag).then_some(())?;
    der_tlv_bounds(bytes, offset)
}

fn der_tlv_bounds(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let len_byte = *bytes.get(offset + 1)?;
    if len_byte & 0x80 == 0 {
        let content = offset + 2;
        let end = content.checked_add(len_byte as usize)?;
        return (end <= bytes.len()).then_some((content, end));
    }
    let len_len = (len_byte & 0x7F) as usize;
    if len_len == 0 || len_len > 2 || offset + 2 + len_len > bytes.len() {
        return None;
    }
    let mut len = 0usize;
    for byte in &bytes[offset + 2..offset + 2 + len_len] {
        len = (len << 8) | *byte as usize;
    }
    if len > 4096 {
        return None;
    }
    let content = offset + 2 + len_len;
    let end = content.checked_add(len)?;
    (end <= bytes.len()).then_some((content, end))
}

fn read_der_string(bytes: &[u8], offset: usize) -> Option<String> {
    let tag = *bytes.get(offset)?;
    if !matches!(tag, 0x0C | 0x13 | 0x14 | 0x16) {
        return None;
    }
    let len = *bytes.get(offset + 1)? as usize;
    if len & 0x80 != 0 || len > 128 {
        return None;
    }
    let raw = bytes.get(offset + 2..offset + 2 + len)?;
    let value = String::from_utf8_lossy(raw).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn parse_authenticode_signature_algorithms(
    bytes: &[u8],
    offset: usize,
    length: usize,
) -> Vec<String> {
    let Some(end) = offset.checked_add(length).filter(|end| *end <= bytes.len()) else {
        return Vec::new();
    };
    let payload = bytes.get(offset + 8..end).unwrap_or(&[]);
    let oid_patterns: [(&str, &[u8]); 7] = [
        (
            "RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
            ],
        ),
        (
            "SHA-1 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x05,
            ],
        ),
        (
            "SHA-256 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
            ],
        ),
        (
            "SHA-384 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0C,
            ],
        ),
        (
            "SHA-512 with RSA",
            &[
                0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0D,
            ],
        ),
        (
            "ECDSA with SHA-256",
            &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02],
        ),
        (
            "ECDSA with SHA-384",
            &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x03],
        ),
    ];
    let mut algorithms = Vec::new();
    for (name, pattern) in oid_patterns {
        if payload
            .windows(pattern.len())
            .any(|window| window == pattern)
        {
            algorithms.push(name.to_string());
        }
    }
    algorithms
}

fn parse_pe_clr_header(
    bytes: &[u8],
    sections: &[PeSectionSummary],
    offset: usize,
) -> Option<PeClrSummary> {
    if offset + 24 > bytes.len() || read_u32(bytes, offset)? < 24 {
        return None;
    }
    let metadata_rva = read_u32(bytes, offset + 8)?;
    let metadata_size = read_u32(bytes, offset + 12)?;
    let metadata = pe_rva_to_file_offset(sections, metadata_rva).and_then(|metadata_offset| {
        parse_clr_metadata_root(bytes, metadata_offset, metadata_size as usize)
    });
    Some(PeClrSummary {
        major: read_u16(bytes, offset + 4)?,
        minor: read_u16(bytes, offset + 6)?,
        metadata_rva,
        metadata_size,
        flags: read_u32(bytes, offset + 16)?,
        metadata_version: metadata
            .as_ref()
            .map(|root| root.version.clone())
            .unwrap_or_default(),
        metadata_streams: metadata
            .as_ref()
            .map(|root| root.streams.clone())
            .unwrap_or_default(),
        metadata_tables: metadata
            .as_ref()
            .map(|root| root.tables.clone())
            .unwrap_or_default(),
        assembly_refs: metadata
            .as_ref()
            .map(|root| root.assembly_refs.clone())
            .unwrap_or_default(),
        type_defs: metadata
            .as_ref()
            .map(|root| root.type_defs.clone())
            .unwrap_or_default(),
        custom_attributes: metadata
            .as_ref()
            .map(|root| root.custom_attributes)
            .unwrap_or(0),
        assembly: metadata.and_then(|root| root.assembly),
    })
}

struct ClrMetadataRoot {
    version: String,
    streams: Vec<String>,
    tables: Vec<String>,
    assembly: Option<String>,
    assembly_refs: Vec<String>,
    type_defs: Vec<String>,
    custom_attributes: u32,
}

fn parse_clr_metadata_root(bytes: &[u8], offset: usize, size: usize) -> Option<ClrMetadataRoot> {
    let end = offset.checked_add(size)?.min(bytes.len());
    if offset + 20 > end || read_u32(bytes, offset)? != 0x424A_5342 {
        return None;
    }
    let version_len = read_u32(bytes, offset + 12)? as usize;
    let version_start = offset + 16;
    let version_end = version_start.checked_add(version_len)?.min(end);
    let version = String::from_utf8_lossy(bytes.get(version_start..version_end)?)
        .trim_matches('\0')
        .trim()
        .to_string();
    let mut stream_offset = align4(version_end) + 4;
    if stream_offset > end {
        return Some(ClrMetadataRoot {
            version,
            streams: Vec::new(),
            tables: Vec::new(),
            assembly: None,
            assembly_refs: Vec::new(),
            type_defs: Vec::new(),
            custom_attributes: 0,
        });
    }
    let streams = read_u16(bytes, stream_offset - 2).unwrap_or(0).min(64) as usize;
    let mut names = Vec::new();
    let mut strings_heap = None;
    let mut tables_stream = None;
    for _ in 0..streams {
        if stream_offset + 8 > end {
            break;
        }
        let relative_offset = read_u32(bytes, stream_offset)? as usize;
        let stream_size = read_u32(bytes, stream_offset + 4)? as usize;
        let (name, name_end) = read_ascii_z(bytes, stream_offset + 8, end)?;
        let data_offset = offset.checked_add(relative_offset)?;
        names.push(name.clone());
        if name == "#Strings" {
            strings_heap = bytes.get(data_offset..data_offset.saturating_add(stream_size).min(end));
        } else if name == "#~" {
            tables_stream =
                bytes.get(data_offset..data_offset.saturating_add(stream_size).min(end));
        }
        stream_offset = align4(name_end);
    }
    let assembly = tables_stream
        .zip(strings_heap)
        .and_then(|(tables, strings)| parse_clr_assembly_identity(tables, strings));
    let assembly_refs = tables_stream
        .zip(strings_heap)
        .map(|(tables, strings)| parse_clr_assembly_refs(tables, strings))
        .unwrap_or_default();
    let type_defs = tables_stream
        .zip(strings_heap)
        .map(|(tables, strings)| parse_clr_type_defs(tables, strings))
        .unwrap_or_default();
    let custom_attributes = tables_stream
        .and_then(clr_tables_layout)
        .map(|layout| layout.rows[12])
        .unwrap_or(0);
    let tables = tables_stream
        .map(parse_clr_table_counts)
        .unwrap_or_default();
    Some(ClrMetadataRoot {
        version,
        streams: names,
        tables,
        assembly,
        assembly_refs,
        type_defs,
        custom_attributes,
    })
}

fn parse_clr_table_counts(tables: &[u8]) -> Vec<String> {
    if tables.len() < 24 {
        return Vec::new();
    }
    let valid = read_u64(tables, 8).unwrap_or(0);
    let mut offset = 24usize;
    let mut counts = Vec::new();
    for table in 0..64 {
        if valid & (1u64 << table) == 0 {
            continue;
        }
        if offset + 4 > tables.len() {
            break;
        }
        let rows = read_u32(tables, offset).unwrap_or(0);
        if rows > 0 {
            counts.push(format!("{}={rows}", clr_table_name(table)));
        }
        offset += 4;
        if counts.len() >= 32 {
            break;
        }
    }
    counts
}

fn clr_table_name(index: usize) -> &'static str {
    match index {
        0 => "Module",
        1 => "TypeRef",
        2 => "TypeDef",
        4 => "Field",
        6 => "MethodDef",
        8 => "Param",
        9 => "InterfaceImpl",
        10 => "MemberRef",
        11 => "Constant",
        12 => "CustomAttribute",
        13 => "FieldMarshal",
        14 => "DeclSecurity",
        15 => "ClassLayout",
        16 => "FieldLayout",
        17 => "StandAloneSig",
        18 => "EventMap",
        20 => "Event",
        21 => "PropertyMap",
        23 => "Property",
        24 => "MethodSemantics",
        25 => "MethodImpl",
        26 => "ModuleRef",
        27 => "TypeSpec",
        28 => "ImplMap",
        29 => "FieldRVA",
        32 => "Assembly",
        35 => "AssemblyRef",
        39 => "ExportedType",
        40 => "ManifestResource",
        41 => "NestedClass",
        42 => "GenericParam",
        43 => "MethodSpec",
        44 => "GenericParamConstraint",
        _ => "Table",
    }
}

fn parse_clr_assembly_identity(tables: &[u8], strings: &[u8]) -> Option<String> {
    let layout = clr_tables_layout(tables)?;
    if *layout.rows.get(32)? == 0 {
        return None;
    }
    let string_index_size = layout.string_index_size;
    let blob_index_size = layout.blob_index_size;
    let row = *layout.offsets.get(32)?;
    let major = read_u16(tables, row + 4)?;
    let minor = read_u16(tables, row + 6)?;
    let build = read_u16(tables, row + 8)?;
    let revision = read_u16(tables, row + 10)?;
    let name_index_offset = row + 16 + blob_index_size;
    let name_index = if string_index_size == 4 {
        read_u32(tables, name_index_offset)? as usize
    } else {
        read_u16(tables, name_index_offset)? as usize
    };
    let name = read_c_string(strings, name_index, 260)?;
    (!name.is_empty()).then(|| format!("{name} {major}.{minor}.{build}.{revision}"))
}

fn parse_clr_assembly_refs(tables: &[u8], strings: &[u8]) -> Vec<String> {
    let Some(layout) = clr_tables_layout(tables) else {
        return Vec::new();
    };
    let rows = layout.rows.get(35).copied().unwrap_or(0).min(16) as usize;
    let mut refs = Vec::new();
    let mut row = layout.offsets.get(35).copied().unwrap_or(0);
    for _ in 0..rows {
        if row + 12 + layout.blob_index_size + layout.string_index_size * 2 + layout.blob_index_size
            > tables.len()
        {
            break;
        }
        let major = read_u16(tables, row).unwrap_or(0);
        let minor = read_u16(tables, row + 2).unwrap_or(0);
        let build = read_u16(tables, row + 4).unwrap_or(0);
        let revision = read_u16(tables, row + 6).unwrap_or(0);
        let name_index_offset = row + 12 + layout.blob_index_size;
        let name_index = read_clr_index(tables, name_index_offset, layout.string_index_size)
            .unwrap_or(0) as usize;
        if let Some(name) = read_c_string(strings, name_index, 260).filter(|name| !name.is_empty())
        {
            refs.push(format!("{name} {major}.{minor}.{build}.{revision}"));
        }
        row += 12 + layout.blob_index_size + layout.string_index_size * 2 + layout.blob_index_size;
    }
    refs
}

fn parse_clr_type_defs(tables: &[u8], strings: &[u8]) -> Vec<String> {
    let Some(layout) = clr_tables_layout(tables) else {
        return Vec::new();
    };
    let rows = layout.rows.get(2).copied().unwrap_or(0).min(24) as usize;
    let mut types = Vec::new();
    let mut row = layout.offsets.get(2).copied().unwrap_or(0);
    for _ in 0..rows {
        if row + 4 + layout.string_index_size * 2 > tables.len() {
            break;
        }
        let name_index =
            read_clr_index(tables, row + 4, layout.string_index_size).unwrap_or(0) as usize;
        let namespace_index = read_clr_index(
            tables,
            row + 4 + layout.string_index_size,
            layout.string_index_size,
        )
        .unwrap_or(0) as usize;
        let name = read_c_string(strings, name_index, 260).unwrap_or_default();
        if !name.is_empty() {
            let namespace = read_c_string(strings, namespace_index, 260).unwrap_or_default();
            if namespace.is_empty() {
                types.push(name);
            } else {
                types.push(format!("{namespace}.{name}"));
            }
        }
        row += clr_table_row_size(2, layout.string_index_size, 2, layout.blob_index_size)
            .unwrap_or(14);
    }
    types
}

struct ClrTablesLayout {
    rows: [u32; 64],
    offsets: [usize; 64],
    string_index_size: usize,
    blob_index_size: usize,
}

fn clr_tables_layout(tables: &[u8]) -> Option<ClrTablesLayout> {
    if tables.len() < 24 {
        return None;
    }
    let heap_sizes = *tables.get(6)?;
    let valid = read_u64(tables, 8)?;
    let string_index_size = if heap_sizes & 0x01 != 0 { 4 } else { 2 };
    let guid_index_size = if heap_sizes & 0x02 != 0 { 4 } else { 2 };
    let blob_index_size = if heap_sizes & 0x04 != 0 { 4 } else { 2 };
    let mut rows = [0u32; 64];
    let mut offset = 24usize;
    for table in 0..64 {
        if valid & (1u64 << table) == 0 {
            continue;
        }
        rows[table] = read_u32(tables, offset)?;
        offset += 4;
    }
    let mut offsets = [0usize; 64];
    for table in 0..64 {
        if rows[table] == 0 {
            continue;
        }
        offsets[table] = offset;
        let row_size =
            clr_table_row_size(table, string_index_size, guid_index_size, blob_index_size)?;
        offset = offset.checked_add(row_size.checked_mul(rows[table] as usize)?)?;
    }
    Some(ClrTablesLayout {
        rows,
        offsets,
        string_index_size,
        blob_index_size,
    })
}

fn clr_table_row_size(
    table: usize,
    string_index_size: usize,
    guid_index_size: usize,
    blob_index_size: usize,
) -> Option<usize> {
    match table {
        0 => Some(2 + string_index_size + guid_index_size * 3),
        2 => Some(4 + string_index_size * 2 + 2 + 2 + 2),
        12 => Some(2 + 2 + blob_index_size),
        32 => Some(16 + blob_index_size + string_index_size * 2),
        35 => Some(12 + blob_index_size + string_index_size * 2 + blob_index_size),
        _ => None,
    }
}

fn read_clr_index(bytes: &[u8], offset: usize, size: usize) -> Option<u32> {
    match size {
        2 => read_u16(bytes, offset).map(u32::from),
        4 => read_u32(bytes, offset),
        _ => None,
    }
}

fn read_ascii_z(bytes: &[u8], offset: usize, limit: usize) -> Option<(String, usize)> {
    let end = bytes
        .get(offset..limit)?
        .iter()
        .position(|byte| *byte == 0)
        .map(|len| offset + len)?;
    let value = String::from_utf8_lossy(bytes.get(offset..end)?)
        .trim()
        .to_string();
    Some((value, end + 1))
}

fn pe_rva_to_file_offset(sections: &[PeSectionSummary], rva: u32) -> Option<usize> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_size).max(1);
        if rva >= section.virtual_address && rva < section.virtual_address.saturating_add(span) {
            return Some(
                section
                    .raw_pointer
                    .saturating_add(rva - section.virtual_address) as usize,
            );
        }
    }
    None
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x014C => "x86",
        0x8664 => "x64",
        0x01C0 => "ARM",
        0x01C4 => "ARMv7",
        0xAA64 => "ARM64",
        0x0200 => "IA64",
        _ => "unknown",
    }
}

fn subsystem_name(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "native",
        2 => "Windows GUI",
        3 => "Windows console",
        5 => "OS/2 console",
        7 => "POSIX console",
        9 => "Windows CE GUI",
        10 => "EFI application",
        11 => "EFI boot service driver",
        12 => "EFI runtime driver",
        13 => "EFI ROM",
        14 => "Xbox",
        16 => "Windows boot application",
        _ => "unknown",
    }
}
