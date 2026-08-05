use super::{
    base_info_text,
    common::{format_bytes, format_timestamp, read_u16, read_u32, read_u64},
    executable::{format_pe_version, pe_version_file_type, PeFixedVersion},
    file_name, generic_info_json, read_file_prefix,
};

pub(super) fn render_info(path: &str, size: i64, modified_unix: i64) -> String {
    let filename = file_name(path);
    let bytes = read_file_prefix(path, 512).unwrap_or_default();
    let mut text = base_info_text(filename, "dump", size, modified_unix);
    if bytes.starts_with(b"MDMP") {
        text.push_str("\nFormat: Windows minidump");
        if let Some(version) = read_u32(&bytes, 4) {
            text.push_str(&format!("\nVersion: 0x{version:08X}"));
        }
        if let Some(streams) = read_u32(&bytes, 8) {
            text.push_str(&format!("\nStreams: {}", streams));
        }
        if let Some(directory_rva) = read_u32(&bytes, 12) {
            text.push_str(&format!("\nDirectory RVA: 0x{directory_rva:08X}"));
        }
        if let Some(timestamp) = read_u32(&bytes, 20) {
            text.push_str(&format!(
                "\nTimestamp: {}",
                format_timestamp(timestamp as i64)
            ));
        }
        if let Some(flags) = read_u64(&bytes, 24) {
            text.push_str(&format!("\nFlags: 0x{flags:016X}"));
        }
        append_minidump_streams(&mut text, &bytes);
    } else if bytes.starts_with(&[0x7F, b'E', b'L', b'F']) {
        text.push_str("\nFormat: ELF core/dump");
        super::elf::append_summary(&mut text, &bytes);
    } else {
        text.push_str("\nFormat: memory dump");
    }
    generic_info_json(path, "dump", size, modified_unix, Some(text))
}

fn append_minidump_streams(text: &mut String, bytes: &[u8]) {
    let streams = read_u32(bytes, 8).unwrap_or(0).min(64) as usize;
    let directory_rva = read_u32(bytes, 12).unwrap_or(0) as usize;
    if streams == 0 || directory_rva == 0 {
        return;
    }

    let mut names = Vec::new();
    let mut system_info = None;
    let mut exception_info = None;
    let mut thread_info = None;
    let mut module_info = None;
    let mut memory_info = None;
    let mut memory64_info = None;
    let mut thread_names_info = None;
    let mut handle_info = None;
    let mut unloaded_module_info = None;
    let mut misc_info = None;
    for index in 0..streams {
        let offset = directory_rva + index * 12;
        let Some(stream_type) = read_u32(bytes, offset) else {
            break;
        };
        let Some(data_size) = read_u32(bytes, offset + 4) else {
            break;
        };
        let Some(rva) = read_u32(bytes, offset + 8) else {
            break;
        };
        names.push(format!(
            "{} ({} @ 0x{rva:08X})",
            minidump_stream_name(stream_type),
            format_bytes(data_size as i64)
        ));
        if stream_type == 7 {
            system_info = parse_minidump_system_info(bytes, rva as usize, data_size as usize);
        } else if stream_type == 6 {
            exception_info = parse_minidump_exception(bytes, rva as usize, data_size as usize);
        } else if stream_type == 3 {
            thread_info = parse_minidump_thread_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 4 {
            module_info = parse_minidump_module_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 5 {
            memory_info = parse_minidump_memory_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 9 {
            memory64_info = parse_minidump_memory64_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 24 {
            thread_names_info =
                parse_minidump_thread_names(bytes, rva as usize, data_size as usize);
        } else if stream_type == 17 {
            handle_info = parse_minidump_handle_data(bytes, rva as usize, data_size as usize);
        } else if stream_type == 11 {
            unloaded_module_info =
                parse_minidump_unloaded_module_list(bytes, rva as usize, data_size as usize);
        } else if stream_type == 12 {
            misc_info = parse_minidump_misc_info(bytes, rva as usize, data_size as usize);
        }
    }
    if !names.is_empty() {
        text.push_str(&format!("\nStream summary: {}", names.join(", ")));
    }
    if let Some(system_info) = system_info {
        text.push_str(&system_info);
    }
    if let Some(exception_info) = exception_info {
        text.push_str(&exception_info);
    }
    if let Some(thread_info) = thread_info {
        text.push_str(&thread_info);
    }
    if let Some(module_info) = module_info {
        text.push_str(&module_info);
    }
    if let Some(memory_info) = memory_info {
        text.push_str(&memory_info);
    }
    if let Some(memory64_info) = memory64_info {
        text.push_str(&memory64_info);
    }
    if let Some(thread_names_info) = thread_names_info {
        text.push_str(&thread_names_info);
    }
    if let Some(handle_info) = handle_info {
        text.push_str(&handle_info);
    }
    if let Some(unloaded_module_info) = unloaded_module_info {
        text.push_str(&unloaded_module_info);
    }
    if let Some(misc_info) = misc_info {
        text.push_str(&misc_info);
    }
}

fn parse_minidump_misc_info(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 8 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let size_of_info = read_u32(bytes, offset).unwrap_or(0) as usize;
    let available = size
        .min(size_of_info)
        .min(bytes.len().saturating_sub(offset));
    if available < 8 {
        return None;
    }
    let flags = read_u32(bytes, offset + 4).unwrap_or(0);
    let mut lines = vec![format!("\nMiscInfo flags: 0x{flags:08X}")];
    if flags & 0x1 != 0 && available >= 12 {
        let process_id = read_u32(bytes, offset + 8).unwrap_or(0);
        lines.push(format!("Process ID: {process_id}"));
    }
    if flags & 0x2 != 0 && available >= 24 {
        let create_time = read_u32(bytes, offset + 12).unwrap_or(0);
        let user_time = read_u32(bytes, offset + 16).unwrap_or(0);
        let kernel_time = read_u32(bytes, offset + 20).unwrap_or(0);
        lines.push(format!("Process create time: {create_time}"));
        lines.push(format!("Process user time: {user_time}s"));
        lines.push(format!("Process kernel time: {kernel_time}s"));
    }
    if flags & 0x4 != 0 && available >= 44 {
        let max_mhz = read_u32(bytes, offset + 24).unwrap_or(0);
        let current_mhz = read_u32(bytes, offset + 28).unwrap_or(0);
        let mhz_limit = read_u32(bytes, offset + 32).unwrap_or(0);
        let max_idle_state = read_u32(bytes, offset + 36).unwrap_or(0);
        let current_idle_state = read_u32(bytes, offset + 40).unwrap_or(0);
        lines.push(format!(
            "Processor power: max {max_mhz} MHz; current {current_mhz} MHz; limit {mhz_limit} MHz; idle {current_idle_state}/{max_idle_state}"
        ));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_unloaded_module_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 12 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let header_size = read_u32(bytes, offset).unwrap_or(0).max(12) as usize;
    let entry_size = read_u32(bytes, offset + 4).unwrap_or(0) as usize;
    let count = read_u32(bytes, offset + 8).unwrap_or(0) as usize;
    if entry_size < 24 || header_size > size {
        return None;
    }
    let mut lines = vec![format!("\nUnloaded modules: {count}")];
    let mut entry_offset = offset + header_size;
    for _ in 0..count.min(12) {
        if entry_offset + entry_size > offset + size || entry_offset + 24 > bytes.len() {
            break;
        }
        let base = read_u64(bytes, entry_offset).unwrap_or(0);
        let image_size = read_u32(bytes, entry_offset + 8).unwrap_or(0) as u64;
        let end = base.saturating_add(image_size);
        let checksum = read_u32(bytes, entry_offset + 12).unwrap_or(0);
        let timestamp = read_u32(bytes, entry_offset + 16).unwrap_or(0);
        let name_rva = read_u32(bytes, entry_offset + 20).unwrap_or(0) as usize;
        let name =
            read_minidump_utf16_string(bytes, name_rva).unwrap_or_else(|| "<unnamed>".to_string());
        lines.push(format!(
            "Unloaded module {name}: range 0x{base:016X}-0x{end:016X}; timestamp 0x{timestamp:08X}; checksum 0x{checksum:08X}"
        ));
        entry_offset += entry_size;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_handle_data(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 16 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let header_size = read_u32(bytes, offset).unwrap_or(0).max(16) as usize;
    let descriptor_size = read_u32(bytes, offset + 4).unwrap_or(0) as usize;
    let count = read_u32(bytes, offset + 8).unwrap_or(0) as usize;
    if descriptor_size < 32 || header_size > size {
        return None;
    }
    let mut lines = vec![format!("\nHandles: {count}")];
    let mut descriptor_offset = offset + header_size;
    for _ in 0..count.min(8) {
        if descriptor_offset + descriptor_size > offset + size
            || descriptor_offset + 32 > bytes.len()
        {
            break;
        }
        let handle = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let type_name_rva = read_u32(bytes, descriptor_offset + 8).unwrap_or(0) as usize;
        let object_name_rva = read_u32(bytes, descriptor_offset + 12).unwrap_or(0) as usize;
        let attributes = read_u32(bytes, descriptor_offset + 16).unwrap_or(0);
        let granted_access = read_u32(bytes, descriptor_offset + 20).unwrap_or(0);
        let handle_count = read_u32(bytes, descriptor_offset + 24).unwrap_or(0);
        let pointer_count = read_u32(bytes, descriptor_offset + 28).unwrap_or(0);
        let type_name = read_minidump_utf16_string(bytes, type_name_rva)
            .unwrap_or_else(|| "<unknown>".to_string());
        let object_name = read_minidump_utf16_string(bytes, object_name_rva)
            .unwrap_or_else(|| "<unnamed>".to_string());
        lines.push(format!(
            "Handle 0x{handle:016X}: {type_name} {object_name}; access 0x{granted_access:08X}; attributes 0x{attributes:08X}; handles {handle_count}; pointers {pointer_count}"
        ));
        descriptor_offset += descriptor_size;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_thread_names(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nThread names: {count}")];
    let mut entry_offset = offset + 4;
    for _ in 0..count.min(12) {
        if entry_offset + 16 > offset + size || entry_offset + 16 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, entry_offset).unwrap_or(0);
        let name_rva = read_u64(bytes, entry_offset + 8).unwrap_or(0) as usize;
        if let Some(name) = read_minidump_utf16_string(bytes, name_rva) {
            lines.push(format!("Thread {id} name: {name}"));
        }
        entry_offset += 16;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_memory64_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 16 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u64(bytes, offset)? as usize;
    let base_rva = read_u64(bytes, offset + 8).unwrap_or(0);
    let mut total = 0u64;
    let mut lines = vec![
        format!("\nMemory64 ranges: {count}"),
        format!("Memory64 base RVA: 0x{base_rva:X}"),
    ];
    let mut descriptor_offset = offset + 16;
    for _ in 0..count.min(8) {
        if descriptor_offset + 16 > offset + size || descriptor_offset + 16 > bytes.len() {
            break;
        }
        let start = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let data_size = read_u64(bytes, descriptor_offset + 8).unwrap_or(0);
        total = total.saturating_add(data_size);
        let end = start.saturating_add(data_size);
        lines.push(format!(
            "Memory64 0x{start:016X}-0x{end:016X} ({data_size} bytes)"
        ));
        descriptor_offset += 16;
    }
    if count > 0 {
        lines.insert(2, format!("Memory64 bytes listed: {total}"));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_memory_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut total = 0u64;
    let mut lines = vec![format!("\nMemory ranges: {count}")];
    let mut descriptor_offset = offset + 4;
    for _ in 0..count.min(8) {
        if descriptor_offset + 16 > offset + size || descriptor_offset + 16 > bytes.len() {
            break;
        }
        let start = read_u64(bytes, descriptor_offset).unwrap_or(0);
        let data_size = read_u32(bytes, descriptor_offset + 8).unwrap_or(0) as u64;
        total = total.saturating_add(data_size);
        let end = start.saturating_add(data_size);
        lines.push(format!(
            "Memory 0x{start:016X}-0x{end:016X} ({data_size} bytes)"
        ));
        descriptor_offset += 16;
    }
    if count > 0 {
        lines.insert(1, format!("Memory bytes listed: {total}"));
    }
    Some(lines.join("\n"))
}

fn parse_minidump_module_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nModules: {count}")];
    let mut module_offset = offset + 4;
    for _ in 0..count.min(12) {
        if module_offset + 108 > offset + size || module_offset + 108 > bytes.len() {
            break;
        }
        let base = read_u64(bytes, module_offset).unwrap_or(0);
        let image_size = read_u32(bytes, module_offset + 8).unwrap_or(0);
        let timestamp = read_u32(bytes, module_offset + 16).unwrap_or(0);
        let name_rva = read_u32(bytes, module_offset + 20).unwrap_or(0) as usize;
        let name =
            read_minidump_utf16_string(bytes, name_rva).unwrap_or_else(|| "<unnamed>".to_string());
        let mut line = format!(
            "Module {name}: base 0x{base:016X}; size {image_size}; timestamp 0x{timestamp:08X}"
        );
        if let Some(version) = parse_minidump_fixed_version(bytes, module_offset + 24) {
            line.push_str(&format!(
                "; file version {}; product version {}; type {}; flags 0x{:08X}",
                version.file_version, version.product_version, version.file_type, version.flags
            ));
        }
        lines.push(line);
        module_offset += 108;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_fixed_version(bytes: &[u8], offset: usize) -> Option<PeFixedVersion> {
    if offset + 52 > bytes.len() || read_u32(bytes, offset)? != 0xFEEF_04BD {
        return None;
    }
    let file_ms = read_u32(bytes, offset + 8)?;
    let file_ls = read_u32(bytes, offset + 12)?;
    let product_ms = read_u32(bytes, offset + 16)?;
    let product_ls = read_u32(bytes, offset + 20)?;
    let flags_mask = read_u32(bytes, offset + 24).unwrap_or(0);
    let flags = read_u32(bytes, offset + 28).unwrap_or(0) & flags_mask;
    let file_type = read_u32(bytes, offset + 36).unwrap_or(0);
    Some(PeFixedVersion {
        file_version: format_pe_version(file_ms, file_ls),
        product_version: format_pe_version(product_ms, product_ls),
        flags,
        file_type: pe_version_file_type(file_type),
    })
}

fn parse_minidump_thread_list(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 4 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let count = read_u32(bytes, offset)? as usize;
    let mut lines = vec![format!("\nThreads: {count}")];
    let mut thread_offset = offset + 4;
    for _ in 0..count.min(6) {
        if thread_offset + 48 > offset + size || thread_offset + 48 > bytes.len() {
            break;
        }
        let id = read_u32(bytes, thread_offset).unwrap_or(0);
        let priority = read_u32(bytes, thread_offset + 12).unwrap_or(0);
        let stack_start = read_u64(bytes, thread_offset + 24).unwrap_or(0);
        let stack_size = read_u32(bytes, thread_offset + 32).unwrap_or(0);
        let stack_end = stack_start.saturating_add(stack_size as u64);
        lines.push(format!(
            "Thread {id}: priority {priority}; stack 0x{stack_start:016X}-0x{stack_end:016X}"
        ));
        thread_offset += 48;
    }
    Some(lines.join("\n"))
}

fn parse_minidump_exception(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 32 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let thread_id = read_u32(bytes, offset)?;
    let code = read_u32(bytes, offset + 8)?;
    let flags = read_u32(bytes, offset + 12)?;
    let address = read_u64(bytes, offset + 24)?;
    let parameters = read_u32(bytes, offset + 32).unwrap_or(0);
    Some(format!(
        "\nException thread: {thread_id}\nException code: {}\nException flags: 0x{flags:08X}\nException address: 0x{address:016X}\nException parameters: {parameters}",
        minidump_exception_name(code)
    ))
}

fn minidump_exception_name(code: u32) -> String {
    match code {
        0x8000_0003 => "breakpoint".to_string(),
        0xC000_0005 => "access violation".to_string(),
        0xC000_001D => "illegal instruction".to_string(),
        0xC000_0094 => "integer divide by zero".to_string(),
        0xC000_00FD => "stack overflow".to_string(),
        _ => format!("0x{code:08X}"),
    }
}

fn parse_minidump_system_info(bytes: &[u8], offset: usize, size: usize) -> Option<String> {
    if size < 32 || offset.checked_add(size)? > bytes.len() {
        return None;
    }
    let arch = read_u16(bytes, offset)?;
    let processors = *bytes.get(offset + 6)?;
    let product_type = *bytes.get(offset + 7)?;
    let major = read_u32(bytes, offset + 8)?;
    let minor = read_u32(bytes, offset + 12)?;
    let build = read_u32(bytes, offset + 16)?;
    let platform = read_u32(bytes, offset + 20)?;
    let csd_rva = read_u32(bytes, offset + 24).unwrap_or(0);
    let suite_mask = read_u16(bytes, offset + 28).unwrap_or(0);
    let mut text = format!(
        "\nSystem architecture: {}\nProcessors: {}\nWindows version: {}.{}.{}\nProduct type: {}\nPlatform ID: {}",
        minidump_processor_architecture(arch),
        processors,
        major,
        minor,
        build,
        minidump_product_type(product_type),
        platform
    );
    if suite_mask > 0 {
        text.push_str(&format!("\nSuite mask: 0x{suite_mask:04X}"));
    }
    if let Some(csd) = read_minidump_utf16_string(bytes, csd_rva as usize) {
        text.push_str(&format!("\nService pack: {csd}"));
    }
    Some(text)
}

fn read_minidump_utf16_string(bytes: &[u8], offset: usize) -> Option<String> {
    if offset == 0 || offset + 4 > bytes.len() {
        return None;
    }
    let len = read_u32(bytes, offset)? as usize;
    let start = offset + 4;
    let end = start.checked_add(len)?;
    let raw = bytes.get(start..end)?;
    let units = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let value = String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn minidump_processor_architecture(value: u16) -> &'static str {
    match value {
        0 => "x86",
        5 => "ARM",
        9 => "x64",
        12 => "ARM64",
        _ => "unknown",
    }
}

fn minidump_product_type(value: u8) -> &'static str {
    match value {
        1 => "workstation",
        2 => "domain controller",
        3 => "server",
        _ => "unknown",
    }
}

fn minidump_stream_name(value: u32) -> &'static str {
    match value {
        3 => "ThreadList",
        4 => "ModuleList",
        5 => "MemoryList",
        6 => "Exception",
        7 => "SystemInfo",
        9 => "Memory64List",
        11 => "UnloadedModuleList",
        12 => "MiscInfo",
        15 => "MemoryInfoList",
        16 => "ThreadInfoList",
        17 => "HandleData",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests;
