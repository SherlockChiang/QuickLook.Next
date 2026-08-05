use super::{
    base_info_text,
    common::{read_c_string, read_u16_endian, read_u32_endian, read_u64_endian},
    file_name, generic_info_json, read_file_prefix,
};

// ELF metadata is commonly spread well beyond the fixed ELF header. Keep the
// path-based entry point bounded while allowing normal section/string tables to
// be reached. All deeper offsets are still checked against the bytes we read.
const MAX_ELF_READ_BYTES: usize = 1024 * 1024;
const MAX_ELF_PROGRAM_HEADERS: usize = 64;
const MAX_ELF_SECTION_HEADERS: usize = 128;
const MAX_ELF_DYNAMIC_ENTRIES: usize = 256;
const MAX_ELF_VERSION_ENTRIES: usize = 32;
const MAX_ELF_VERSION_AUX_ENTRIES: usize = 16;
const MAX_ELF_NOTE_RECORDS: usize = 64;
const MAX_ELF_ENTRY_SIZE: usize = 4096;
const MAX_ELF_STRING_BYTES: usize = 260;
const MAX_ELF_NOTE_OWNER_BYTES: usize = 64;
const MAX_ELF_BUILD_ID_BYTES: usize = 64;

#[derive(Clone, Copy)]
struct ElfIdentity {
    class: u8,
    endian: u8,
    program_header_size: usize,
    section_header_size: usize,
}

pub(super) fn render_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, MAX_ELF_READ_BYTES).unwrap_or_default();
    let mut text = base_info_text(filename, "elf", size, modified_unix);
    append_summary(&mut text, &bytes);
    generic_info_json(path, "elf", size, modified_unix, Some(text))
}

pub(super) fn append_summary(text: &mut String, bytes: &[u8]) {
    if !bytes.starts_with(&[0x7F, b'E', b'L', b'F']) || bytes.len() < 16 {
        text.push_str("\nFormat: ELF-like binary");
        return;
    }
    let class = bytes.get(4).copied().unwrap_or(0);
    let endian = bytes.get(5).copied().unwrap_or(0);
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
        match endian {
            1 => "little",
            2 => "big",
            _ => "unknown",
        }
    ));
    let Some(identity) = elf_identity(bytes) else {
        text.push_str("\nELF header: invalid or truncated");
        return;
    };
    let class = identity.class;
    let endian = identity.endian;
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

fn elf_identity(bytes: &[u8]) -> Option<ElfIdentity> {
    if !bytes.starts_with(&[0x7F, b'E', b'L', b'F']) || bytes.len() < 16 {
        return None;
    }
    let class = bytes[4];
    let endian = bytes[5];
    if !matches!(class, 1 | 2) || !matches!(endian, 1 | 2) || bytes[6] != 1 {
        return None;
    }
    let header_size = if class == 2 { 64 } else { 52 };
    let program_header_size = if class == 2 { 56 } else { 32 };
    let section_header_size = if class == 2 { 64 } else { 40 };
    if bytes.len() < header_size
        || read_u32_endian(bytes, 20, endian)? != 1
        || usize::from(read_u16_endian(
            bytes,
            if class == 2 { 52 } else { 40 },
            endian,
        )?) != header_size
    {
        return None;
    }
    let program_count = usize::from(read_u16_endian(
        bytes,
        if class == 2 { 56 } else { 44 },
        endian,
    )?);
    let program_entry_size = usize::from(read_u16_endian(
        bytes,
        if class == 2 { 54 } else { 42 },
        endian,
    )?);
    if program_count > 0
        && (program_entry_size < program_header_size || program_entry_size > MAX_ELF_ENTRY_SIZE)
    {
        return None;
    }
    let section_count = usize::from(read_u16_endian(
        bytes,
        if class == 2 { 60 } else { 48 },
        endian,
    )?);
    let section_entry_size = usize::from(read_u16_endian(
        bytes,
        if class == 2 { 58 } else { 46 },
        endian,
    )?);
    if section_count > 0
        && (section_entry_size < section_header_size || section_entry_size > MAX_ELF_ENTRY_SIZE)
    {
        return None;
    }
    Some(ElfIdentity {
        class,
        endian,
        program_header_size,
        section_header_size,
    })
}

fn checked_range(bytes: &[u8], offset: usize, size: usize) -> Option<(usize, usize)> {
    let end = offset.checked_add(size)?;
    (end <= bytes.len()).then_some((offset, end))
}

fn checked_range_u64(bytes: &[u8], offset: u64, size: u64) -> Option<(usize, usize)> {
    checked_range(
        bytes,
        usize::try_from(offset).ok()?,
        usize::try_from(size).ok()?,
    )
}

fn table_string(
    bytes: &[u8],
    table_offset: usize,
    table_size: usize,
    name_offset: u64,
    max_len: usize,
) -> Option<String> {
    let name_offset = usize::try_from(name_offset).ok()?;
    if name_offset >= table_size {
        return None;
    }
    let absolute = table_offset.checked_add(name_offset)?;
    let available = table_size - name_offset;
    read_c_string(bytes, absolute, available.min(max_len))
}

fn align4_checked(value: usize) -> Option<usize> {
    value.checked_add(3).map(|aligned| aligned & !3)
}

fn elf_interpreter(bytes: &[u8], class: u8, endian: u8) -> Option<String> {
    let headers = elf_program_headers(bytes, class, endian);
    let interpreter = headers.iter().find(|header| header.typ == 3)?;
    let (start, end) = checked_range_u64(
        bytes,
        interpreter.file_offset,
        interpreter.file_size.min(MAX_ELF_STRING_BYTES as u64),
    )?;
    let value = String::from_utf8_lossy(&bytes[start..end])
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn elf_needed_libraries(bytes: &[u8], class: u8, endian: u8) -> Vec<String> {
    let Some(dynamic) = elf_dynamic_metadata(bytes, class, endian) else {
        return Vec::new();
    };
    let Some((strtab_offset, strtab_size)) = elf_dynamic_string_table(
        bytes,
        &dynamic.headers,
        dynamic.strtab_vaddr,
        dynamic.strtab_size,
    ) else {
        return Vec::new();
    };
    dynamic
        .needed_offsets
        .into_iter()
        .filter_map(|name_offset| {
            table_string(
                bytes,
                strtab_offset,
                strtab_size,
                name_offset,
                MAX_ELF_STRING_BYTES,
            )
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn elf_dynamic_string_tags(bytes: &[u8], class: u8, endian: u8) -> Vec<(&'static str, String)> {
    let Some(dynamic) = elf_dynamic_metadata(bytes, class, endian) else {
        return Vec::new();
    };
    let Some((strtab_offset, strtab_size)) = elf_dynamic_string_table(
        bytes,
        &dynamic.headers,
        dynamic.strtab_vaddr,
        dynamic.strtab_size,
    ) else {
        return Vec::new();
    };
    dynamic
        .tagged_offsets
        .into_iter()
        .filter_map(|(label, name_offset)| {
            table_string(
                bytes,
                strtab_offset,
                strtab_size,
                name_offset,
                MAX_ELF_STRING_BYTES,
            )
            .filter(|value| !value.is_empty())
            .map(|value| (label, value))
        })
        .collect()
}

struct ElfDynamicMetadata {
    headers: Vec<ElfProgramHeader>,
    strtab_vaddr: u64,
    strtab_size: u64,
    needed_offsets: Vec<u64>,
    tagged_offsets: Vec<(&'static str, u64)>,
}

fn elf_dynamic_metadata(bytes: &[u8], class: u8, endian: u8) -> Option<ElfDynamicMetadata> {
    let headers = elf_program_headers(bytes, class, endian);
    let dynamic = headers.iter().find(|header| header.typ == 2)?;
    let (start, end) = checked_range_u64(bytes, dynamic.file_offset, dynamic.file_size)?;
    let entry_size = if class == 2 { 16 } else { 8 };
    let mut strtab_vaddr = 0;
    let mut strtab_size = 0;
    let mut needed_offsets = Vec::new();
    let mut tagged_offsets = Vec::new();
    for index in 0..MAX_ELF_DYNAMIC_ENTRIES {
        let offset = start.checked_add(index.checked_mul(entry_size)?)?;
        let entry_end = offset.checked_add(entry_size)?;
        if entry_end > end {
            break;
        }
        let tag = if class == 2 {
            read_u64_endian(bytes, offset, endian)?
        } else {
            u64::from(read_u32_endian(bytes, offset, endian)?)
        };
        let value_offset = offset.checked_add(if class == 2 { 8 } else { 4 })?;
        let value = if class == 2 {
            read_u64_endian(bytes, value_offset, endian)?
        } else {
            u64::from(read_u32_endian(bytes, value_offset, endian)?)
        };
        match tag {
            0 => break,
            1 if needed_offsets.len() < 32 => needed_offsets.push(value),
            5 => strtab_vaddr = value,
            10 => strtab_size = value,
            14 if tagged_offsets.len() < 16 => tagged_offsets.push(("SONAME", value)),
            15 if tagged_offsets.len() < 16 => tagged_offsets.push(("RPATH", value)),
            29 if tagged_offsets.len() < 16 => tagged_offsets.push(("RUNPATH", value)),
            _ => {}
        }
    }
    Some(ElfDynamicMetadata {
        headers,
        strtab_vaddr,
        strtab_size,
        needed_offsets,
        tagged_offsets,
    })
}

fn elf_dynamic_string_table(
    bytes: &[u8],
    headers: &[ElfProgramHeader],
    strtab_vaddr: u64,
    strtab_size: u64,
) -> Option<(usize, usize)> {
    if strtab_vaddr == 0 {
        return None;
    }
    let (offset, remaining) = elf_vaddr_to_file_range(headers, strtab_vaddr, 1)?;
    let size = if strtab_size == 0 {
        remaining as u64
    } else {
        strtab_size.min(remaining as u64)
    };
    if size == 0 {
        return None;
    }
    checked_range_u64(bytes, offset as u64, size)
}

#[derive(Clone, Copy)]
struct ElfProgramHeader {
    typ: u32,
    file_offset: u64,
    virtual_address: u64,
    file_size: u64,
}

fn elf_program_headers(bytes: &[u8], class: u8, endian: u8) -> Vec<ElfProgramHeader> {
    let Some(identity) =
        elf_identity(bytes).filter(|identity| identity.class == class && identity.endian == endian)
    else {
        return Vec::new();
    };
    let phoff = if class == 2 {
        read_u64_endian(bytes, 32, endian)
    } else {
        read_u32_endian(bytes, 28, endian).map(u64::from)
    };
    let Some(phentsize_raw) = read_u16_endian(bytes, if class == 2 { 54 } else { 42 }, endian)
    else {
        return Vec::new();
    };
    let Some(phnum_raw) = read_u16_endian(bytes, if class == 2 { 56 } else { 44 }, endian) else {
        return Vec::new();
    };
    let phentsize = usize::from(phentsize_raw);
    let phnum = usize::from(phnum_raw).min(MAX_ELF_PROGRAM_HEADERS);
    let mut headers = Vec::new();
    let Some(phoff) = phoff else {
        return headers;
    };
    if phoff == 0 || phentsize < identity.program_header_size || phentsize > MAX_ELF_ENTRY_SIZE {
        return headers;
    }
    for index in 0..phnum {
        let Some(offset) = usize::try_from(phoff).ok().and_then(|base| {
            index
                .checked_mul(phentsize)
                .and_then(|step| base.checked_add(step))
        }) else {
            break;
        };
        if checked_range(bytes, offset, phentsize).is_none() {
            break;
        }
        let Some(typ) = read_u32_endian(bytes, offset, endian) else {
            break;
        };
        let header = if class == 2 {
            let Some(file_offset_offset) = offset.checked_add(8) else {
                break;
            };
            let Some(virtual_address_offset) = offset.checked_add(16) else {
                break;
            };
            let Some(file_size_offset) = offset.checked_add(32) else {
                break;
            };
            let Some(file_offset) = read_u64_endian(bytes, file_offset_offset, endian) else {
                break;
            };
            let Some(virtual_address) = read_u64_endian(bytes, virtual_address_offset, endian)
            else {
                break;
            };
            let Some(file_size) = read_u64_endian(bytes, file_size_offset, endian) else {
                break;
            };
            ElfProgramHeader {
                typ,
                file_offset,
                virtual_address,
                file_size,
            }
        } else {
            let Some(file_offset_offset) = offset.checked_add(4) else {
                break;
            };
            let Some(virtual_address_offset) = offset.checked_add(8) else {
                break;
            };
            let Some(file_size_offset) = offset.checked_add(16) else {
                break;
            };
            let Some(file_offset) = read_u32_endian(bytes, file_offset_offset, endian) else {
                break;
            };
            let Some(virtual_address) = read_u32_endian(bytes, virtual_address_offset, endian)
            else {
                break;
            };
            let Some(file_size) = read_u32_endian(bytes, file_size_offset, endian) else {
                break;
            };
            ElfProgramHeader {
                typ,
                file_offset: u64::from(file_offset),
                virtual_address: u64::from(virtual_address),
                file_size: u64::from(file_size),
            }
        };
        headers.push(header);
    }
    headers
}

fn elf_vaddr_to_file_range(
    headers: &[ElfProgramHeader],
    vaddr: u64,
    required_size: u64,
) -> Option<(usize, usize)> {
    for header in headers.iter().filter(|header| header.typ == 1) {
        let Some(delta) = vaddr.checked_sub(header.virtual_address) else {
            continue;
        };
        if delta > header.file_size {
            continue;
        }
        let remaining = header.file_size - delta;
        if required_size > remaining {
            continue;
        }
        let file_offset = header.file_offset.checked_add(delta)?;
        return Some((
            usize::try_from(file_offset).ok()?,
            usize::try_from(remaining).ok()?,
        ));
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
    let Some(identity) =
        elf_identity(bytes).filter(|identity| identity.class == class && identity.endian == endian)
    else {
        return Vec::new();
    };
    let shoff_value = if class == 2 {
        read_u64_endian(bytes, 40, endian)
    } else {
        read_u32_endian(bytes, 32, endian).map(u64::from)
    };
    let Some(shoff) = shoff_value.and_then(|value| usize::try_from(value).ok()) else {
        return Vec::new();
    };
    let Some(shentsize_raw) = read_u16_endian(bytes, if class == 2 { 58 } else { 46 }, endian)
    else {
        return Vec::new();
    };
    let shentsize = usize::from(shentsize_raw);
    let Some(shnum_raw) = read_u16_endian(bytes, if class == 2 { 60 } else { 48 }, endian) else {
        return Vec::new();
    };
    let shnum = usize::from(shnum_raw);
    let Some(shstrndx) =
        read_u16_endian(bytes, if class == 2 { 62 } else { 50 }, endian).map(usize::from)
    else {
        return Vec::new();
    };
    if shoff == 0
        || shnum == 0
        || shnum > MAX_ELF_SECTION_HEADERS
        || shentsize < identity.section_header_size
        || shentsize > MAX_ELF_ENTRY_SIZE
        || shstrndx >= shnum
    {
        return Vec::new();
    }
    let Some(table_size) = shnum.checked_mul(shentsize) else {
        return Vec::new();
    };
    if checked_range(bytes, shoff, table_size).is_none() {
        return Vec::new();
    }
    let Some(str_header_step) = shstrndx.checked_mul(shentsize) else {
        return Vec::new();
    };
    let Some(str_header) = shoff.checked_add(str_header_step) else {
        return Vec::new();
    };
    let (str_offset, str_size) = if class == 2 {
        let Some(offset_field) = str_header.checked_add(24) else {
            return Vec::new();
        };
        let Some(size_field) = str_header.checked_add(32) else {
            return Vec::new();
        };
        let Some(offset) = read_u64_endian(bytes, offset_field, endian)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Vec::new();
        };
        let Some(size) = read_u64_endian(bytes, size_field, endian)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Vec::new();
        };
        (offset, size)
    } else {
        let Some(offset_field) = str_header.checked_add(16) else {
            return Vec::new();
        };
        let Some(size_field) = str_header.checked_add(20) else {
            return Vec::new();
        };
        let Some(offset) = read_u32_endian(bytes, offset_field, endian)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Vec::new();
        };
        let Some(size) = read_u32_endian(bytes, size_field, endian)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return Vec::new();
        };
        (offset, size)
    };
    if checked_range(bytes, str_offset, str_size).is_none() {
        return Vec::new();
    }
    let mut sections = Vec::new();
    for index in 0..shnum {
        let Some(header) = index
            .checked_mul(shentsize)
            .and_then(|step| shoff.checked_add(step))
        else {
            break;
        };
        if checked_range(bytes, header, shentsize).is_none() {
            break;
        }
        let Some(name_offset) = read_u32_endian(bytes, header, endian) else {
            continue;
        };
        let Some(typ_offset) = header.checked_add(4) else {
            continue;
        };
        let Some(typ) = read_u32_endian(bytes, typ_offset, endian) else {
            continue;
        };
        let name = table_string(bytes, str_offset, str_size, u64::from(name_offset), 96)
            .unwrap_or_default();
        let (offset, size, link, entsize) = if class == 2 {
            let Some(offset_field) = header.checked_add(24) else {
                continue;
            };
            let Some(size_field) = header.checked_add(32) else {
                continue;
            };
            let Some(link_field) = header.checked_add(40) else {
                continue;
            };
            let Some(entsize_field) = header.checked_add(56) else {
                continue;
            };
            let Some(offset) = read_u64_endian(bytes, offset_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(size) = read_u64_endian(bytes, size_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(link) = read_u32_endian(bytes, link_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(entsize) = read_u64_endian(bytes, entsize_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            (offset, size, link, entsize)
        } else {
            let Some(offset_field) = header.checked_add(16) else {
                continue;
            };
            let Some(size_field) = header.checked_add(20) else {
                continue;
            };
            let Some(link_field) = header.checked_add(24) else {
                continue;
            };
            let Some(entsize_field) = header.checked_add(36) else {
                continue;
            };
            let Some(offset) = read_u32_endian(bytes, offset_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(size) = read_u32_endian(bytes, size_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(link) = read_u32_endian(bytes, link_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(entsize) = read_u32_endian(bytes, entsize_field, endian)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            (offset, size, link, entsize)
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
        let minimum_entry_size = if class == 2 { 24 } else { 16 };
        let entry_size = if section.entsize > 0 {
            section.entsize
        } else {
            minimum_entry_size
        };
        if entry_size < minimum_entry_size
            || entry_size > MAX_ELF_ENTRY_SIZE
            || checked_range(bytes, section.offset, section.size).is_none()
        {
            continue;
        }
        let Some(strtab) = sections.get(section.link) else {
            continue;
        };
        if checked_range(bytes, strtab.offset, strtab.size).is_none() {
            continue;
        }
        let count = section.size / entry_size;
        let mut named = Vec::new();
        for index in 0..count.min(64) {
            let Some(offset) = index
                .checked_mul(entry_size)
                .and_then(|step| section.offset.checked_add(step))
            else {
                break;
            };
            if checked_range(bytes, offset, entry_size).is_none() {
                break;
            }
            let Some(name_offset) = read_u32_endian(bytes, offset, endian) else {
                continue;
            };
            if name_offset == 0 {
                continue;
            }
            if let Some(name) = table_string(
                bytes,
                strtab.offset,
                strtab.size,
                u64::from(name_offset),
                128,
            )
            .filter(|name| !name.is_empty())
            {
                let (info, shndx) = if class == 2 {
                    let Some(info_offset) = offset.checked_add(4) else {
                        continue;
                    };
                    let Some(shndx_offset) = offset.checked_add(6) else {
                        continue;
                    };
                    (
                        bytes.get(info_offset).copied().unwrap_or(0),
                        read_u16_endian(bytes, shndx_offset, endian).unwrap_or(0),
                    )
                } else {
                    let Some(info_offset) = offset.checked_add(12) else {
                        continue;
                    };
                    let Some(shndx_offset) = offset.checked_add(14) else {
                        continue;
                    };
                    (
                        bytes.get(info_offset).copied().unwrap_or(0),
                        read_u16_endian(bytes, shndx_offset, endian).unwrap_or(0),
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
            let minimum_entry_size = if section.typ == 4 {
                if class == 2 {
                    24
                } else {
                    12
                }
            } else if class == 2 {
                16
            } else {
                8
            };
            let entry_size = if section.entsize > 0 {
                section.entsize
            } else {
                minimum_entry_size
            };
            if entry_size < minimum_entry_size
                || entry_size > MAX_ELF_ENTRY_SIZE
                || section.size == 0
                || checked_range(bytes, section.offset, section.size).is_none()
            {
                return None;
            }
            let count = section.size / entry_size;
            let mut types = Vec::new();
            for index in 0..count.min(8) {
                let Some(offset) = index
                    .checked_mul(entry_size)
                    .and_then(|step| section.offset.checked_add(step))
                else {
                    break;
                };
                if checked_range(bytes, offset, entry_size).is_none() {
                    break;
                }
                let rel_type = if class == 2 {
                    let Some(value_offset) = offset.checked_add(8) else {
                        break;
                    };
                    (read_u64_endian(bytes, value_offset, endian).unwrap_or(0) & 0xFFFF_FFFF) as u32
                } else {
                    let Some(value_offset) = offset.checked_add(4) else {
                        break;
                    };
                    read_u32_endian(bytes, value_offset, endian).unwrap_or(0) & 0xFF
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
        if checked_range(bytes, section.offset, section.size).is_none() {
            continue;
        }
        match section.typ {
            0x6FFF_FFFF => {
                let count = section.size / 2;
                let mut sample = Vec::new();
                for index in 0..count.min(MAX_ELF_VERSION_ENTRIES) {
                    let Some(offset) = index
                        .checked_mul(2)
                        .and_then(|step| section.offset.checked_add(step))
                    else {
                        break;
                    };
                    let value = read_u16_endian(bytes, offset, endian).unwrap_or(0) & 0x7FFF;
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
    if checked_range(bytes, strtab.offset, strtab.size).is_none() {
        return Vec::new();
    }
    let Some((_, end)) = checked_range(bytes, section.offset, section.size) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    for _ in 0..MAX_ELF_VERSION_ENTRIES {
        let Some(entry_end) = offset.checked_add(16) else {
            break;
        };
        if entry_end > end {
            break;
        }
        let Some(aux_count_offset) = offset.checked_add(2) else {
            break;
        };
        let Some(aux_offset_offset) = offset.checked_add(8) else {
            break;
        };
        let Some(next_offset) = offset.checked_add(12) else {
            break;
        };
        let aux_count = usize::from(read_u16_endian(bytes, aux_count_offset, endian).unwrap_or(0))
            .min(MAX_ELF_VERSION_AUX_ENTRIES);
        let aux_offset =
            usize::try_from(read_u32_endian(bytes, aux_offset_offset, endian).unwrap_or(0))
                .unwrap_or(0);
        let next =
            usize::try_from(read_u32_endian(bytes, next_offset, endian).unwrap_or(0)).unwrap_or(0);
        let Some(mut current_aux) = offset.checked_add(aux_offset) else {
            break;
        };
        for _ in 0..aux_count {
            let Some(aux_end) = current_aux.checked_add(16) else {
                break;
            };
            if aux_end > end {
                break;
            }
            let Some(name_field) = current_aux.checked_add(8) else {
                break;
            };
            let name_offset = u64::from(read_u32_endian(bytes, name_field, endian).unwrap_or(0));
            if let Some(name) = table_string(bytes, strtab.offset, strtab.size, name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let Some(aux_next_field) = current_aux.checked_add(12) else {
                break;
            };
            let aux_next =
                usize::try_from(read_u32_endian(bytes, aux_next_field, endian).unwrap_or(0))
                    .unwrap_or(0);
            if aux_next == 0 {
                break;
            }
            let Some(next_aux) = current_aux.checked_add(aux_next) else {
                break;
            };
            if next_aux <= current_aux || next_aux >= end {
                break;
            }
            current_aux = next_aux;
        }
        if next == 0 {
            break;
        }
        let Some(next_entry) = offset.checked_add(next) else {
            break;
        };
        if next_entry <= offset || next_entry >= end {
            break;
        }
        offset = next_entry;
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
    if checked_range(bytes, strtab.offset, strtab.size).is_none() {
        return Vec::new();
    }
    let Some((_, end)) = checked_range(bytes, section.offset, section.size) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut offset = section.offset;
    for _ in 0..MAX_ELF_VERSION_ENTRIES {
        let Some(entry_end) = offset.checked_add(20) else {
            break;
        };
        if entry_end > end {
            break;
        }
        let Some(aux_count_offset) = offset.checked_add(6) else {
            break;
        };
        let Some(aux_offset_offset) = offset.checked_add(12) else {
            break;
        };
        let Some(next_offset) = offset.checked_add(16) else {
            break;
        };
        // ELF verdef stores vd_cnt at +6 (vd_ndx is at +4).
        let aux_count = usize::from(read_u16_endian(bytes, aux_count_offset, endian).unwrap_or(0))
            .min(MAX_ELF_VERSION_AUX_ENTRIES);
        let aux_offset =
            usize::try_from(read_u32_endian(bytes, aux_offset_offset, endian).unwrap_or(0))
                .unwrap_or(0);
        let next =
            usize::try_from(read_u32_endian(bytes, next_offset, endian).unwrap_or(0)).unwrap_or(0);
        let Some(mut current_aux) = offset.checked_add(aux_offset) else {
            break;
        };
        for _ in 0..aux_count {
            let Some(aux_end) = current_aux.checked_add(8) else {
                break;
            };
            if aux_end > end {
                break;
            }
            let name_offset = u64::from(read_u32_endian(bytes, current_aux, endian).unwrap_or(0));
            if let Some(name) = table_string(bytes, strtab.offset, strtab.size, name_offset, 96)
                .filter(|name| !name.is_empty())
            {
                if !names.contains(&name) && names.len() < 8 {
                    names.push(name);
                }
            }
            let Some(aux_next_field) = current_aux.checked_add(4) else {
                break;
            };
            let aux_next =
                usize::try_from(read_u32_endian(bytes, aux_next_field, endian).unwrap_or(0))
                    .unwrap_or(0);
            if aux_next == 0 {
                break;
            }
            let Some(next_aux) = current_aux.checked_add(aux_next) else {
                break;
            };
            if next_aux <= current_aux || next_aux >= end {
                break;
            }
            current_aux = next_aux;
        }
        if next == 0 {
            break;
        }
        let Some(next_entry) = offset.checked_add(next) else {
            break;
        };
        if next_entry <= offset || next_entry >= end {
            break;
        }
        offset = next_entry;
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
        let (Some(file_offset), Some(file_size)) = (
            usize::try_from(header.file_offset).ok(),
            usize::try_from(header.file_size).ok(),
        ) else {
            continue;
        };
        append_elf_notes(&mut notes, bytes, endian, "PT_NOTE", file_offset, file_size);
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
    let Some((_, end)) = checked_range(bytes, file_offset, size) else {
        return;
    };
    let mut offset = file_offset;
    let mut records = 0;
    while offset
        .checked_add(12)
        .is_some_and(|header_end| header_end <= end)
        && records < MAX_ELF_NOTE_RECORDS
    {
        let Some(namesz) =
            read_u32_endian(bytes, offset, endian).and_then(|value| usize::try_from(value).ok())
        else {
            break;
        };
        let Some(desc_size_offset) = offset.checked_add(4) else {
            break;
        };
        let Some(descsz) = read_u32_endian(bytes, desc_size_offset, endian)
            .and_then(|value| usize::try_from(value).ok())
        else {
            break;
        };
        let Some(type_offset) = offset.checked_add(8) else {
            break;
        };
        let Some(typ) = read_u32_endian(bytes, type_offset, endian) else {
            break;
        };
        let Some(name_offset) = offset.checked_add(12) else {
            break;
        };
        let Some(name_end) = name_offset.checked_add(namesz) else {
            break;
        };
        let Some(desc_offset) = align4_checked(name_end) else {
            break;
        };
        let Some(desc_end) = desc_offset.checked_add(descsz) else {
            break;
        };
        let Some(next) = align4_checked(desc_end) else {
            break;
        };
        if name_end > end || desc_end > end || next <= offset || next > end {
            break;
        }
        records += 1;
        let name_len = namesz.min(MAX_ELF_NOTE_OWNER_BYTES);
        let Some(name_end_limited) = name_offset.checked_add(name_len) else {
            break;
        };
        let name = bytes
            .get(name_offset..name_end_limited)
            .map(|raw| {
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_default();
        let desc_len = descsz.min(MAX_ELF_BUILD_ID_BYTES);
        let Some(desc_end_limited) = desc_offset.checked_add(desc_len) else {
            break;
        };
        let desc = bytes.get(desc_offset..desc_end_limited).unwrap_or(&[]);
        if name == "GNU" && typ == 3 && !desc.is_empty() && notes.len() < 8 {
            notes.push(format!("{} GNU build-id {}", label, bytes_to_hex(desc)));
        } else if !name.is_empty() && notes.len() < 8 {
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
