use super::{
    base_info_text,
    common::{read_c_string, read_u16_endian, read_u32_endian, read_u64_endian},
    executable::align4,
    file_name, generic_info_json, read_file_prefix,
};

const MAX_ELF_HEADER_BYTES: usize = 512;

pub(super) fn render_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_ELF_HEADER_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "elf", size, modified_unix);
    append_summary(&mut text, &bytes);
    generic_info_json(path, "elf", size, modified_unix, Some(text))
}

pub(super) fn append_summary(text: &mut String, bytes: &[u8]) {
    if !bytes.starts_with(&[0x7F, b'E', b'L', b'F']) || bytes.len() < 20 {
        text.push_str("\nFormat: ELF-like binary");
        return;
    }
    let class = bytes.get(4).copied().unwrap_or(0);
    let endian = bytes.get(5).copied().unwrap_or(1);
    text.push_str(&format!(
        "\nFormat: ELF{}",
        match class {
            1 => "32",
            2 => "64",
            _ => "",
        }
    ));
    text.push_str(&format!(
        "\nEndian: {}",
        if endian == 2 { "big" } else { "little" }
    ));
    if let Some(kind) = read_u16_endian(bytes, 16, endian) {
        text.push_str(&format!("\nType: {}", elf_type_name(kind)));
    }
    if let Some(machine) = read_u16_endian(bytes, 18, endian) {
        text.push_str(&format!("\nMachine: {}", elf_machine_name(machine)));
    }
    let entry = if class == 2 {
        read_u64_endian(bytes, 24, endian).map(|v| format!("0x{v:016X}"))
    } else {
        read_u32_endian(bytes, 24, endian).map(|v| format!("0x{v:08X}"))
    };
    if let Some(entry) = entry {
        text.push_str(&format!("\nEntry: {entry}"));
    }
    let phnum_offset = if class == 2 { 56 } else { 44 };
    let shnum_offset = if class == 2 { 60 } else { 48 };
    if let Some(phnum) = read_u16_endian(bytes, phnum_offset, endian) {
        text.push_str(&format!("\nProgram headers: {}", phnum));
    }
    if let Some(shnum) = read_u16_endian(bytes, shnum_offset, endian) {
        text.push_str(&format!("\nSection headers: {}", shnum));
    }
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian)
    } else {
        read_u32_endian(bytes, 28, endian).map(u64::from)
    };
    if let Some(phoff) = phoff.filter(|value| *value > 0) {
        text.push_str(&format!("\nProgram header offset: 0x{phoff:X}"));
    }
    let shoff = if class == 2 {
        read_u64_endian(bytes, 40, endian)
    } else {
        read_u32_endian(bytes, 32, endian).map(u64::from)
    };
    if let Some(shoff) = shoff.filter(|value| *value > 0) {
        text.push_str(&format!("\nSection header offset: 0x{shoff:X}"));
    }
    let flags_offset = if class == 2 { 48 } else { 36 };
    if let Some(flags) = read_u32_endian(bytes, flags_offset, endian).filter(|value| *value > 0) {
        text.push_str(&format!("\nFlags: 0x{flags:08X}"));
    }
    if let Some(interpreter) = elf_interpreter(bytes, class, endian) {
        text.push_str(&format!("\nInterpreter: {interpreter}"));
    }
    let needed = elf_needed_libraries(bytes, class, endian);
    if !needed.is_empty() {
        text.push_str(&format!("\nNeeded libraries: {}", needed.join(", ")));
    }
    for (label, value) in elf_dynamic_string_tags(bytes, class, endian) {
        text.push_str(&format!("\n{label}: {value}"));
    }
    let sections = elf_section_names(bytes, class, endian);
    if !sections.is_empty() {
        text.push_str(&format!("\nSection names: {}", sections.join(", ")));
    }
    let symbols = elf_symbol_summary(bytes, class, endian);
    if !symbols.is_empty() {
        text.push_str(&format!("\nSymbols: {}", symbols.join(", ")));
    }
    let relocations = elf_relocation_summary(bytes, class, endian);
    if !relocations.is_empty() {
        text.push_str(&format!("\nRelocations: {}", relocations.join(", ")));
    }
    let versions = elf_gnu_version_summary(bytes, class, endian);
    if !versions.is_empty() {
        text.push_str(&format!("\nGNU versions: {}", versions.join(", ")));
    }
    let notes = elf_note_summary(bytes, class, endian);
    if !notes.is_empty() {
        text.push_str(&format!("\nNotes: {}", notes.join(", ")));
    }
}

fn elf_interpreter(bytes: &[u8], class: u8, endian: u8) -> Option<String> {
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian)? as usize
    } else {
        read_u32_endian(bytes, 28, endian)? as usize
    };
    let phentsize = read_u16_endian(bytes, if class == 2 { 54 } else { 42 }, endian)? as usize;
    let phnum = read_u16_endian(bytes, if class == 2 { 56 } else { 44 }, endian)?.min(64) as usize;
    if phoff == 0 || phentsize == 0 {
        return None;
    }

    for index in 0..phnum {
        let offset = phoff.checked_add(index.checked_mul(phentsize)?)?;
        let typ = read_u32_endian(bytes, offset, endian)?;
        if typ != 3 {
            continue;
        }
        let (file_offset, file_size) = if class == 2 {
            (
                read_u64_endian(bytes, offset + 8, endian)? as usize,
                read_u64_endian(bytes, offset + 32, endian)? as usize,
            )
        } else {
            (
                read_u32_endian(bytes, offset + 4, endian)? as usize,
                read_u32_endian(bytes, offset + 16, endian)? as usize,
            )
        };
        let end = file_offset.checked_add(file_size)?;
        let raw = bytes.get(file_offset..end)?;
        let value = String::from_utf8_lossy(raw)
            .trim_matches('\0')
            .trim()
            .to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn elf_needed_libraries(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let headers = elf_program_headers(bytes, class, endian);
    let Some(dynamic) = headers.iter().find(|header| header.typ == 2) else {
        return Vec::new();
    };
    let mut strtab_vaddr = 0u64;
    let mut needed_offsets = Vec::new();
    let entry_size = if class == 2 { 16usize } else { 8usize };
    let mut offset = dynamic.file_offset as usize;
    let end = offset
        .saturating_add(dynamic.file_size as usize)
        .min(bytes.len());
    while offset + entry_size <= end && needed_offsets.len() < 32 {
        let tag = if class == 2 {
            read_u64_endian(bytes, offset, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset, endian).unwrap_or(0) as u64
        };
        let value = if class == 2 {
            read_u64_endian(bytes, offset + 8, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64
        };
        match tag {
            0 => break,
            1 => needed_offsets.push(value),
            5 => strtab_vaddr = value,
            _ => {}
        }
        offset += entry_size;
    }
    if strtab_vaddr == 0 {
        return Vec::new();
    }
    let Some(strtab_offset) = elf_vaddr_to_file_offset(&headers, strtab_vaddr) else {
        return Vec::new();
    };
    needed_offsets
        .into_iter()
        .filter_map(|name_offset| read_c_string(bytes, strtab_offset + name_offset as usize, 260))
        .filter(|name| !name.is_empty())
        .collect()
}

fn elf_dynamic_string_tags(bytes: &[u8], class: u8, endian: u8) -> Vec<(&'static str, String)> {
    let headers = elf_program_headers(bytes, class, endian);
    let Some(dynamic) = headers.iter().find(|header| header.typ == 2) else {
        return Vec::new();
    };
    let mut strtab_vaddr = 0u64;
    let mut tagged_offsets = Vec::new();
    let entry_size = if class == 2 { 16usize } else { 8usize };
    let mut offset = dynamic.file_offset as usize;
    let end = offset
        .saturating_add(dynamic.file_size as usize)
        .min(bytes.len());
    while offset + entry_size <= end && tagged_offsets.len() < 16 {
        let tag = if class == 2 {
            read_u64_endian(bytes, offset, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset, endian).unwrap_or(0) as u64
        };
        let value = if class == 2 {
            read_u64_endian(bytes, offset + 8, endian).unwrap_or(0)
        } else {
            read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64
        };
        match tag {
            0 => break,
            5 => strtab_vaddr = value,
            14 => tagged_offsets.push(("SONAME", value)),
            15 => tagged_offsets.push(("RPATH", value)),
            29 => tagged_offsets.push(("RUNPATH", value)),
            _ => {}
        }
        offset += entry_size;
    }
    if strtab_vaddr == 0 {
        return Vec::new();
    }
    let Some(strtab_offset) = elf_vaddr_to_file_offset(&headers, strtab_vaddr) else {
        return Vec::new();
    };
    tagged_offsets
        .into_iter()
        .filter_map(|(label, name_offset)| {
            read_c_string(bytes, strtab_offset + name_offset as usize, 260)
                .filter(|value| !value.is_empty())
                .map(|value| (label, value))
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ElfProgramHeader {
    typ: u32,
    file_offset: u64,
    virtual_address: u64,
    file_size: u64,
    memory_size: u64,
}

fn elf_program_headers(bytes: &[u8], class: u8, endian: u8) -> Vec<ElfProgramHeader> {
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian).unwrap_or(0) as usize
    } else {
        read_u32_endian(bytes, 28, endian).unwrap_or(0) as usize
    };
    let phentsize =
        read_u16_endian(bytes, if class == 2 { 54 } else { 42 }, endian).unwrap_or(0) as usize;
    let phnum = read_u16_endian(bytes, if class == 2 { 56 } else { 44 }, endian)
        .unwrap_or(0)
        .min(64) as usize;
    let mut headers = Vec::new();
    if phoff == 0 || phentsize == 0 {
        return headers;
    }
    for index in 0..phnum {
        let offset = phoff + index * phentsize;
        if offset + phentsize > bytes.len() {
            break;
        }
        let typ = read_u32_endian(bytes, offset, endian).unwrap_or(0);
        let header = if class == 2 {
            ElfProgramHeader {
                typ,
                file_offset: read_u64_endian(bytes, offset + 8, endian).unwrap_or(0),
                virtual_address: read_u64_endian(bytes, offset + 16, endian).unwrap_or(0),
                file_size: read_u64_endian(bytes, offset + 32, endian).unwrap_or(0),
                memory_size: read_u64_endian(bytes, offset + 40, endian).unwrap_or(0),
            }
        } else {
            ElfProgramHeader {
                typ,
                file_offset: read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as u64,
                virtual_address: read_u32_endian(bytes, offset + 8, endian).unwrap_or(0) as u64,
                file_size: read_u32_endian(bytes, offset + 16, endian).unwrap_or(0) as u64,
                memory_size: read_u32_endian(bytes, offset + 20, endian).unwrap_or(0) as u64,
            }
        };
        headers.push(header);
    }
    headers
}

fn elf_vaddr_to_file_offset(headers: &[ElfProgramHeader], vaddr: u64) -> Option<usize> {
    for header in headers.iter().filter(|header| header.typ == 1) {
        let span = header.memory_size.max(header.file_size).max(1);
        if vaddr >= header.virtual_address && vaddr < header.virtual_address.saturating_add(span) {
            return Some((header.file_offset + (vaddr - header.virtual_address)) as usize);
        }
    }
    None
}

fn elf_section_names(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    elf_sections(bytes, class, endian)
        .into_iter()
        .filter_map(|section| (!section.name.is_empty()).then_some(section.name))
        .take(24)
        .collect()
}

#[derive(Clone)]
struct ElfSection {
    name: String,
    typ: u32,
    offset: usize,
    size: usize,
    link: usize,
    entsize: usize,
}

fn elf_sections(bytes: &[u8], class: u8, endian: u8) -> Vec<ElfSection> {
    let shoff = if class == 2 {
        read_u64_endian(bytes, 40, endian).unwrap_or(0) as usize
    } else {
        read_u32_endian(bytes, 32, endian).unwrap_or(0) as usize
    };
    let shentsize =
        read_u16_endian(bytes, if class == 2 { 58 } else { 46 }, endian).unwrap_or(0) as usize;
    let shnum = read_u16_endian(bytes, if class == 2 { 60 } else { 48 }, endian)
        .unwrap_or(0)
        .min(128) as usize;
    let shstrndx =
        read_u16_endian(bytes, if class == 2 { 62 } else { 50 }, endian).unwrap_or(0) as usize;
    if shoff == 0 || shentsize == 0 || shstrndx >= shnum {
        return Vec::new();
    }
    let Some(str_header) = shoff.checked_add(shstrndx.saturating_mul(shentsize)) else {
        return Vec::new();
    };
    if str_header + shentsize > bytes.len() {
        return Vec::new();
    }
    let (str_offset, str_size) = if class == 2 {
        (
            read_u64_endian(bytes, str_header + 24, endian).unwrap_or(0) as usize,
            read_u64_endian(bytes, str_header + 32, endian).unwrap_or(0) as usize,
        )
    } else {
        (
            read_u32_endian(bytes, str_header + 16, endian).unwrap_or(0) as usize,
            read_u32_endian(bytes, str_header + 20, endian).unwrap_or(0) as usize,
        )
    };
    if str_offset == 0 || str_offset.saturating_add(str_size) > bytes.len() {
        return Vec::new();
    }
    let mut sections = Vec::new();
    for index in 0..shnum {
        let Some(header) = shoff.checked_add(index.saturating_mul(shentsize)) else {
            break;
        };
        if header + shentsize > bytes.len() {
            break;
        }
        let name_offset = read_u32_endian(bytes, header, endian).unwrap_or(0) as usize;
        let name = if name_offset == 0 || name_offset >= str_size {
            String::new()
        } else {
            read_c_string(bytes, str_offset + name_offset, 96).unwrap_or_default()
        };
        let typ = read_u32_endian(bytes, header + 4, endian).unwrap_or(0);
        let (offset, size, link, entsize) = if class == 2 {
            (
                read_u64_endian(bytes, header + 24, endian).unwrap_or(0) as usize,
                read_u64_endian(bytes, header + 32, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 40, endian).unwrap_or(0) as usize,
                read_u64_endian(bytes, header + 56, endian).unwrap_or(0) as usize,
            )
        } else {
            (
                read_u32_endian(bytes, header + 16, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 20, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 24, endian).unwrap_or(0) as usize,
                read_u32_endian(bytes, header + 36, endian).unwrap_or(0) as usize,
            )
        };
        sections.push(ElfSection {
            name,
            typ,
            offset,
            size,
            link,
            entsize,
        });
    }
    sections
}

fn elf_symbol_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut symbols = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.typ == 2 || section.typ == 11)
    {
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else if class == 2 {
            24
        } else {
            16
        };
        if entry_size == 0 || section.offset.saturating_add(section.size) > bytes.len() {
            continue;
        }
        let Some(strtab) = sections.get(section.link) else {
            continue;
        };
        if strtab.offset.saturating_add(strtab.size) > bytes.len() {
            continue;
        }
        let count = section.size / entry_size;
        let mut named = Vec::new();
        for index in 0..count.min(64) {
            let offset = section.offset + index * entry_size;
            if offset + entry_size > bytes.len() {
                break;
            }
            let name_offset = read_u32_endian(bytes, offset, endian).unwrap_or(0) as usize;
            if name_offset == 0 || name_offset >= strtab.size {
                continue;
            }
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 128)
                .filter(|name| !name.is_empty())
            {
                let (info, shndx) = if class == 2 {
                    (
                        bytes.get(offset + 4).copied().unwrap_or(0),
                        read_u16_endian(bytes, offset + 6, endian).unwrap_or(0),
                    )
                } else {
                    (
                        bytes.get(offset + 12).copied().unwrap_or(0),
                        read_u16_endian(bytes, offset + 14, endian).unwrap_or(0),
                    )
                };
                named.push(format!(
                    "{}[{} {} {}]",
                    name,
                    elf_symbol_binding_name(info >> 4),
                    elf_symbol_type_name(info & 0x0F),
                    elf_symbol_section_name(&sections, shndx)
                ));
                if named.len() >= 8 {
                    break;
                }
            }
        }
        if !named.is_empty() {
            symbols.push(format!(
                "{} {} entries ({})",
                section.name,
                count,
                named.join(", ")
            ));
        }
    }
    symbols
}

fn elf_symbol_binding_name(value: u8) -> &'static str {
    match value {
        0 => "local",
        1 => "global",
        2 => "weak",
        10..=12 => "os",
        13..=15 => "proc",
        _ => "unknown",
    }
}

fn elf_symbol_type_name(value: u8) -> &'static str {
    match value {
        0 => "notype",
        1 => "object",
        2 => "func",
        3 => "section",
        4 => "file",
        5 => "common",
        6 => "tls",
        10..=12 => "os",
        13..=15 => "proc",
        _ => "unknown",
    }
}

fn elf_symbol_section_name(sections: &[ElfSection], shndx: u16) -> String {
    match shndx {
        0 => "UND".to_string(),
        0xFFF1 => "ABS".to_string(),
        0xFFF2 => "COMMON".to_string(),
        value => sections
            .get(value as usize)
            .and_then(|section| (!section.name.is_empty()).then_some(section.name.clone()))
            .unwrap_or_else(|| format!("section {value}")),
    }
}

fn elf_relocation_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let machine = read_u16_endian(bytes, 18, endian).unwrap_or(0);
    sections
        .iter()
        .filter(|section| section.typ == 4 || section.typ == 9)
        .filter_map(|section| {
            let entry_size = if section.entsize > 0 {
                section.entsize
            } else if section.typ == 4 && class == 2 {
                24
            } else if section.typ == 4 {
                12
            } else if class == 2 {
                16
            } else {
                8
            };
            if entry_size == 0
                || section.size == 0
                || section.offset.saturating_add(section.size) > bytes.len()
            {
                return None;
            }
            let count = section.size / entry_size;
            let mut types = Vec::new();
            for index in 0..count.min(8) {
                let offset = section.offset + index * entry_size;
                let rel_type = if class == 2 {
                    (read_u64_endian(bytes, offset + 8, endian).unwrap_or(0) & 0xFFFF_FFFF) as u32
                } else {
                    read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) & 0xFF
                };
                let name = elf_relocation_type_name(machine, rel_type);
                if !types.contains(&name) {
                    types.push(name);
                }
            }
            if types.is_empty() {
                Some(format!("{} {} entries", section.name, count))
            } else {
                Some(format!(
                    "{} {} entries ({})",
                    section.name,
                    count,
                    types.join(", ")
                ))
            }
        })
        .collect()
}

fn elf_relocation_type_name(machine: u16, typ: u32) -> String {
    match machine {
        62 => match typ {
            0 => "R_X86_64_NONE".to_string(),
            1 => "R_X86_64_64".to_string(),
            2 => "R_X86_64_PC32".to_string(),
            6 => "R_X86_64_GLOB_DAT".to_string(),
            7 => "R_X86_64_JUMP_SLOT".to_string(),
            8 => "R_X86_64_RELATIVE".to_string(),
            10 => "R_X86_64_32".to_string(),
            11 => "R_X86_64_32S".to_string(),
            _ => format!("x86-64:{typ}"),
        },
        183 => match typ {
            0 => "R_AARCH64_NONE".to_string(),
            257 => "R_AARCH64_ABS64".to_string(),
            1025 => "R_AARCH64_GLOB_DAT".to_string(),
            1026 => "R_AARCH64_JUMP_SLOT".to_string(),
            1027 => "R_AARCH64_RELATIVE".to_string(),
            _ => format!("AArch64:{typ}"),
        },
        _ => format!("type {typ}"),
    }
}

fn elf_gnu_version_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut versions = Vec::new();
    for section in sections
        .iter()
        .filter(|section| matches!(section.typ, 0x6FFF_FFFD..=0x6FFF_FFFF))
    {
        if section.offset.saturating_add(section.size) > bytes.len() {
            continue;
        }
        match section.typ {
            0x6FFF_FFFF => {
                let count = section.size / 2;
                let mut sample = Vec::new();
                for index in 0..count.min(8) {
                    let value = read_u16_endian(bytes, section.offset + index * 2, endian)
                        .unwrap_or(0)
                        & 0x7FFF;
                    if value > 1 && !sample.contains(&value) {
                        sample.push(value);
                    }
                }
                if sample.is_empty() {
                    versions.push(format!("{} {} entries", section.name, count));
                } else {
                    let sample = sample
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    versions.push(format!("{} {} entries ({sample})", section.name, count));
                }
            }
            0x6FFF_FFFE => {
                let names = elf_gnu_version_need_names(bytes, &sections, section, endian);
                if names.is_empty() {
                    versions.push(format!("{} need entries", section.name));
                } else {
                    versions.push(format!("{} needs {}", section.name, names.join("/")));
                }
            }
            0x6FFF_FFFD => {
                let names = elf_gnu_version_def_names(bytes, &sections, section, endian);
                if names.is_empty() {
                    versions.push(format!("{} definition entries", section.name));
                } else {
                    versions.push(format!("{} defines {}", section.name, names.join("/")));
                }
            }
            _ => {}
        }
    }
    versions
}

fn elf_gnu_version_string_table<'a>(
    sections: &'a [ElfSection],
    section: &ElfSection,
) -> Option<&'a ElfSection> {
    sections.get(section.link).filter(|strtab| strtab.typ == 3)
}

fn elf_gnu_version_need_names(
    bytes: &[u8],
    sections: &[ElfSection],
    section: &ElfSection,
    endian: u8,
) -> Vec<String> {
    let Some(strtab) = elf_gnu_version_string_table(sections, section) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    let end = section.offset + section.size;
    for _ in 0..16 {
        if offset + 16 > end {
            break;
        }
        let aux_count = read_u16_endian(bytes, offset + 2, endian)
            .unwrap_or(0)
            .min(16) as usize;
        let aux_offset = read_u32_endian(bytes, offset + 8, endian).unwrap_or(0) as usize;
        let next = read_u32_endian(bytes, offset + 12, endian).unwrap_or(0) as usize;
        let mut current_aux = offset.saturating_add(aux_offset);
        for _ in 0..aux_count {
            if current_aux + 16 > end {
                break;
            }
            let name_offset = read_u32_endian(bytes, current_aux + 8, endian).unwrap_or(0) as usize;
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let aux_next = read_u32_endian(bytes, current_aux + 12, endian).unwrap_or(0) as usize;
            if aux_next == 0 {
                break;
            }
            current_aux = current_aux.saturating_add(aux_next);
        }
        if next == 0 {
            break;
        }
        offset = offset.saturating_add(next);
    }
    names
}

fn elf_gnu_version_def_names(
    bytes: &[u8],
    sections: &[ElfSection],
    section: &ElfSection,
    endian: u8,
) -> Vec<String> {
    let Some(strtab) = elf_gnu_version_string_table(sections, section) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    let end = section.offset + section.size;
    for _ in 0..16 {
        if offset + 20 > end {
            break;
        }
        let aux_count = read_u16_endian(bytes, offset + 4, endian)
            .unwrap_or(0)
            .min(16) as usize;
        let aux_offset = read_u32_endian(bytes, offset + 12, endian).unwrap_or(0) as usize;
        let next = read_u32_endian(bytes, offset + 16, endian).unwrap_or(0) as usize;
        let mut current_aux = offset.saturating_add(aux_offset);
        for _ in 0..aux_count {
            if current_aux + 8 > end {
                break;
            }
            let name_offset = read_u32_endian(bytes, current_aux, endian).unwrap_or(0) as usize;
            if let Some(name) = read_c_string(bytes, strtab.offset + name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let aux_next = read_u32_endian(bytes, current_aux + 4, endian).unwrap_or(0) as usize;
            if aux_next == 0 {
                break;
            }
            current_aux = current_aux.saturating_add(aux_next);
        }
        if next == 0 {
            break;
        }
        offset = offset.saturating_add(next);
    }
    names
}

fn elf_note_summary(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let sections = elf_sections(bytes, class, endian);
    let mut notes = Vec::new();
    for section in sections.iter().filter(|section| section.typ == 7) {
        append_elf_notes(
            &mut notes,
            bytes,
            endian,
            &section.name,
            section.offset,
            section.size,
        );
    }
    for header in elf_program_headers(bytes, class, endian)
        .iter()
        .filter(|header| header.typ == 4)
    {
        append_elf_notes(
            &mut notes,
            bytes,
            endian,
            "PT_NOTE",
            header.file_offset as usize,
            header.file_size as usize,
        );
    }
    notes
}

fn append_elf_notes(
    notes: &mut Vec<String>,
    bytes: &[u8],
    endian: u8,
    label: &str,
    file_offset: usize,
    size: usize,
) {
    if file_offset.saturating_add(size) > bytes.len() {
        return;
    }
    let mut offset = file_offset;
    let end = file_offset + size;
    while offset + 12 <= end && notes.len() < 8 {
        let namesz = read_u32_endian(bytes, offset, endian).unwrap_or(0) as usize;
        let descsz = read_u32_endian(bytes, offset + 4, endian).unwrap_or(0) as usize;
        let typ = read_u32_endian(bytes, offset + 8, endian).unwrap_or(0);
        let name_offset = offset + 12;
        let desc_offset = align4(name_offset.saturating_add(namesz));
        let next = align4(desc_offset.saturating_add(descsz));
        if namesz == 0 || desc_offset.saturating_add(descsz) > end || next <= offset {
            break;
        }
        let name = bytes
            .get(name_offset..name_offset + namesz)
            .map(|raw| {
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_default();
        let desc = bytes.get(desc_offset..desc_offset + descsz).unwrap_or(&[]);
        if name == "GNU" && typ == 3 && !desc.is_empty() {
            notes.push(format!("{} GNU build-id {}", label, bytes_to_hex(desc)));
        } else if !name.is_empty() {
            notes.push(format!(
                "{} {} type {} ({} bytes)",
                label, name, typ, descsz
            ));
        }
        offset = next;
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn elf_type_name(value: u16) -> &'static str {
    match value {
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core",
        _ => "unknown",
    }
}

fn elf_machine_name(value: u16) -> &'static str {
    match value {
        3 => "x86",
        40 => "ARM",
        62 => "x86-64",
        183 => "AArch64",
        243 => "RISC-V",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests;
