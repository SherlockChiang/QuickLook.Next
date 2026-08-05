use super::{
    append_minidump_streams, parse_minidump_fixed_version, parse_minidump_misc_info,
    parse_minidump_unloaded_module_list, read_minidump_utf16_string, render_info,
};

#[test]
fn minidump_stream_summary_lists_known_streams() {
    let mut bytes = vec![0u8; 1536];
    bytes[0..4].copy_from_slice(b"MDMP");
    bytes[8..12].copy_from_slice(&9u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&32u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&4u32.to_le_bytes());
    bytes[36..40].copy_from_slice(&128u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&0x200u32.to_le_bytes());
    bytes[44..48].copy_from_slice(&7u32.to_le_bytes());
    bytes[48..52].copy_from_slice(&56u32.to_le_bytes());
    bytes[52..56].copy_from_slice(&0x90u32.to_le_bytes());
    bytes[56..60].copy_from_slice(&6u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&80u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&0x180u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&3u32.to_le_bytes());
    bytes[72..76].copy_from_slice(&100u32.to_le_bytes());
    bytes[76..80].copy_from_slice(&0x1D0u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&4u32.to_le_bytes());
    bytes[84..88].copy_from_slice(&112u32.to_le_bytes());
    bytes[88..92].copy_from_slice(&0x250u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&5u32.to_le_bytes());
    bytes[96..100].copy_from_slice(&36u32.to_le_bytes());
    bytes[100..104].copy_from_slice(&0x380u32.to_le_bytes());
    bytes[104..108].copy_from_slice(&9u32.to_le_bytes());
    bytes[108..112].copy_from_slice(&48u32.to_le_bytes());
    bytes[112..116].copy_from_slice(&0x400u32.to_le_bytes());
    bytes[116..120].copy_from_slice(&24u32.to_le_bytes());
    bytes[120..124].copy_from_slice(&36u32.to_le_bytes());
    bytes[124..128].copy_from_slice(&0x440u32.to_le_bytes());
    bytes[128..132].copy_from_slice(&17u32.to_le_bytes());
    bytes[132..136].copy_from_slice(&48u32.to_le_bytes());
    bytes[136..140].copy_from_slice(&0x4C0u32.to_le_bytes());
    bytes[0x90..0x92].copy_from_slice(&9u16.to_le_bytes());
    bytes[0x96] = 8;
    bytes[0x97] = 1;
    bytes[0x98..0x9C].copy_from_slice(&10u32.to_le_bytes());
    bytes[0x9C..0xA0].copy_from_slice(&0u32.to_le_bytes());
    bytes[0xA0..0xA4].copy_from_slice(&22631u32.to_le_bytes());
    bytes[0xA4..0xA8].copy_from_slice(&2u32.to_le_bytes());
    bytes[0xA8..0xAC].copy_from_slice(&0x120u32.to_le_bytes());
    bytes[0xAC..0xAE].copy_from_slice(&0x0100u16.to_le_bytes());
    let csd: Vec<u8> = "Service Pack 1"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes[0x120..0x124].copy_from_slice(&(csd.len() as u32).to_le_bytes());
    bytes[0x124..0x124 + csd.len()].copy_from_slice(&csd);
    bytes[0x180..0x184].copy_from_slice(&42u32.to_le_bytes());
    bytes[0x188..0x18C].copy_from_slice(&0xC000_0005u32.to_le_bytes());
    bytes[0x18C..0x190].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x198..0x1A0].copy_from_slice(&0x0000_7FFF_FFFF_FFFFu64.to_le_bytes());
    bytes[0x1A0..0x1A4].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x1D0..0x1D4].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x1D4..0x1D8].copy_from_slice(&42u32.to_le_bytes());
    bytes[0x1E0..0x1E4].copy_from_slice(&15u32.to_le_bytes());
    bytes[0x1EC..0x1F4].copy_from_slice(&0x1000u64.to_le_bytes());
    bytes[0x1F4..0x1F8].copy_from_slice(&0x4000u32.to_le_bytes());
    bytes[0x204..0x208].copy_from_slice(&99u32.to_le_bytes());
    bytes[0x210..0x214].copy_from_slice(&8u32.to_le_bytes());
    bytes[0x21C..0x224].copy_from_slice(&0x9000u64.to_le_bytes());
    bytes[0x224..0x228].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[0x250..0x254].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x254..0x25C].copy_from_slice(&0x0000_7FF7_0000_0000u64.to_le_bytes());
    bytes[0x25C..0x260].copy_from_slice(&0x12000u32.to_le_bytes());
    bytes[0x264..0x268].copy_from_slice(&0x6543_2100u32.to_le_bytes());
    bytes[0x268..0x26C].copy_from_slice(&0x340u32.to_le_bytes());
    bytes[0x26C..0x270].copy_from_slice(&0xFEEF_04BDu32.to_le_bytes());
    bytes[0x274..0x278].copy_from_slice(&0x0001_0002u32.to_le_bytes());
    bytes[0x278..0x27C].copy_from_slice(&0x0003_0004u32.to_le_bytes());
    bytes[0x27C..0x280].copy_from_slice(&0x0005_0006u32.to_le_bytes());
    bytes[0x280..0x284].copy_from_slice(&0x0007_0008u32.to_le_bytes());
    bytes[0x284..0x288].copy_from_slice(&0x0000_0003u32.to_le_bytes());
    bytes[0x288..0x28C].copy_from_slice(&0x0000_0002u32.to_le_bytes());
    bytes[0x290..0x294].copy_from_slice(&2u32.to_le_bytes());
    let module_name: Vec<u16> = "demo.exe".encode_utf16().collect();
    bytes[0x340..0x344].copy_from_slice(&((module_name.len() * 2) as u32).to_le_bytes());
    for (index, unit) in module_name.iter().enumerate() {
        let offset = 0x344 + index * 2;
        bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
    }
    bytes[0x380..0x384].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x384..0x38C].copy_from_slice(&0x0010_0000u64.to_le_bytes());
    bytes[0x38C..0x390].copy_from_slice(&0x2000u32.to_le_bytes());
    bytes[0x394..0x39C].copy_from_slice(&0x0020_0000u64.to_le_bytes());
    bytes[0x39C..0x3A0].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[0x400..0x408].copy_from_slice(&2u64.to_le_bytes());
    bytes[0x408..0x410].copy_from_slice(&0x500u64.to_le_bytes());
    bytes[0x410..0x418].copy_from_slice(&0x0030_0000u64.to_le_bytes());
    bytes[0x418..0x420].copy_from_slice(&0x3000u64.to_le_bytes());
    bytes[0x420..0x428].copy_from_slice(&0x0040_0000u64.to_le_bytes());
    bytes[0x428..0x430].copy_from_slice(&0x1000u64.to_le_bytes());
    bytes[0x440..0x444].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x444..0x448].copy_from_slice(&42u32.to_le_bytes());
    bytes[0x44C..0x454].copy_from_slice(&0x480u64.to_le_bytes());
    bytes[0x454..0x458].copy_from_slice(&99u32.to_le_bytes());
    bytes[0x45C..0x464].copy_from_slice(&0x4A0u64.to_le_bytes());
    let worker: Vec<u8> = "worker".encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes[0x480..0x484].copy_from_slice(&(worker.len() as u32).to_le_bytes());
    bytes[0x484..0x484 + worker.len()].copy_from_slice(&worker);
    let io_thread: Vec<u8> = "io thread"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes[0x4A0..0x4A4].copy_from_slice(&(io_thread.len() as u32).to_le_bytes());
    bytes[0x4A4..0x4A4 + io_thread.len()].copy_from_slice(&io_thread);
    bytes[0x4C0..0x4C4].copy_from_slice(&16u32.to_le_bytes());
    bytes[0x4C4..0x4C8].copy_from_slice(&32u32.to_le_bytes());
    bytes[0x4C8..0x4CC].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x4D0..0x4D8].copy_from_slice(&0x44u64.to_le_bytes());
    bytes[0x4D8..0x4DC].copy_from_slice(&0x520u32.to_le_bytes());
    bytes[0x4DC..0x4E0].copy_from_slice(&0x540u32.to_le_bytes());
    bytes[0x4E0..0x4E4].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x4E4..0x4E8].copy_from_slice(&0x0012_019Fu32.to_le_bytes());
    bytes[0x4E8..0x4EC].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x4EC..0x4F0].copy_from_slice(&7u32.to_le_bytes());
    let file_type: Vec<u8> = "File".encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes[0x520..0x524].copy_from_slice(&(file_type.len() as u32).to_le_bytes());
    bytes[0x524..0x524 + file_type.len()].copy_from_slice(&file_type);
    let object_name: Vec<u8> = r"\Device\HarddiskVolume1\demo.txt"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes[0x540..0x544].copy_from_slice(&(object_name.len() as u32).to_le_bytes());
    bytes[0x544..0x544 + object_name.len()].copy_from_slice(&object_name);
    let mut text = String::new();

    append_minidump_streams(&mut text, &bytes);

    assert!(text.contains("ModuleList"));
    assert!(text.contains("SystemInfo"));
    assert!(text.contains("ThreadNames"));
    assert!(text.contains("System architecture: x64"));
    assert!(text.contains("Processors: 8"));
    assert!(text.contains("Windows version: 10.0.22631"));
    assert!(text.contains("Service pack: Service Pack 1"));
    assert!(text.contains("Exception thread: 42"));
    assert!(text.contains("Exception code: access violation"));
    assert!(text.contains("Exception flags: 0x00000001"));
    assert!(text.contains("Threads: 2"));
    assert!(text.contains("Thread 42: priority 15; stack 0x0000000000001000-0x0000000000005000"));
    assert!(text.contains("Thread 99: priority 8; stack 0x0000000000009000-0x000000000000A000"));
    assert!(text.contains("Modules: 1"));
    assert!(text.contains("Module demo.exe: base 0x00007FF700000000; size 73728; timestamp 0x65432100; file version 1.2.3.4; product version 5.6.7.8; type DLL; flags 0x00000002"));
    assert!(text.contains("Memory ranges: 2"));
    assert!(text.contains("Memory bytes listed: 12288"));
    assert!(text.contains("Memory 0x0000000000100000-0x0000000000102000 (8192 bytes)"));
    assert!(text.contains("Memory64 ranges: 2"));
    assert!(text.contains("Memory64 base RVA: 0x500"));
    assert!(text.contains("Memory64 bytes listed: 16384"));
    assert!(text.contains("Memory64 0x0000000000300000-0x0000000000303000 (12288 bytes)"));
    assert!(text.contains("Thread names: 2"));
    assert!(text.contains("Thread 42 name: worker"));
    assert!(text.contains("Thread 99 name: io thread"));
    assert!(text.contains("Handles: 1"));
    assert!(text.contains(r"Handle 0x0000000000000044: File \Device\HarddiskVolume1\demo.txt; access 0x0012019F; attributes 0x00000002; handles 3; pointers 7"));
}

#[test]
fn minidump_unloaded_module_list_summarizes_names_and_ranges() {
    let mut bytes = vec![0u8; 512];
    bytes[0x40..0x44].copy_from_slice(&12u32.to_le_bytes());
    bytes[0x44..0x48].copy_from_slice(&24u32.to_le_bytes());
    bytes[0x48..0x4C].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x4C..0x54].copy_from_slice(&0x0000_7FF6_1000_0000u64.to_le_bytes());
    bytes[0x54..0x58].copy_from_slice(&0x5000u32.to_le_bytes());
    bytes[0x58..0x5C].copy_from_slice(&0x1234_ABCDu32.to_le_bytes());
    bytes[0x5C..0x60].copy_from_slice(&0x6543_2100u32.to_le_bytes());
    bytes[0x60..0x64].copy_from_slice(&0x100u32.to_le_bytes());
    let module_name: Vec<u8> = "old.dll"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes[0x100..0x104].copy_from_slice(&(module_name.len() as u32).to_le_bytes());
    bytes[0x104..0x104 + module_name.len()].copy_from_slice(&module_name);

    let text = parse_minidump_unloaded_module_list(&bytes, 0x40, 36).expect("unloaded modules");

    assert!(text.contains("Unloaded modules: 1"));
    assert!(text.contains("Unloaded module old.dll: range 0x00007FF610000000-0x00007FF610005000; timestamp 0x65432100; checksum 0x1234ABCD"));
}
#[test]
fn minidump_misc_info_summarizes_process_and_power_fields() {
    let mut bytes = vec![0u8; 128];
    bytes[0x20..0x24].copy_from_slice(&44u32.to_le_bytes());
    bytes[0x24..0x28].copy_from_slice(&0x7u32.to_le_bytes());
    bytes[0x28..0x2C].copy_from_slice(&4242u32.to_le_bytes());
    bytes[0x2C..0x30].copy_from_slice(&1_700_000_000u32.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&12u32.to_le_bytes());
    bytes[0x34..0x38].copy_from_slice(&34u32.to_le_bytes());
    bytes[0x38..0x3C].copy_from_slice(&4800u32.to_le_bytes());
    bytes[0x3C..0x40].copy_from_slice(&3600u32.to_le_bytes());
    bytes[0x40..0x44].copy_from_slice(&4200u32.to_le_bytes());
    bytes[0x44..0x48].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x48..0x4C].copy_from_slice(&1u32.to_le_bytes());

    let text = parse_minidump_misc_info(&bytes, 0x20, 44).expect("misc info");

    assert!(text.contains("MiscInfo flags: 0x00000007"));
    assert!(text.contains("Process ID: 4242"));
    assert!(text.contains("Process create time: 1700000000"));
    assert!(text.contains("Process user time: 12s"));
    assert!(text.contains("Process kernel time: 34s"));
    assert!(
        text.contains("Processor power: max 4800 MHz; current 3600 MHz; limit 4200 MHz; idle 1/3")
    );
}

#[test]
fn minidump_hostile_offsets_and_strings_fail_soft() {
    let mut bytes = vec![0u8; 64];
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut text = String::new();
        append_minidump_streams(&mut text, &bytes);
    }));
    assert!(result.is_ok(), "hostile directory RVA must not panic");

    let mut string_header = vec![0u8; 16];
    string_header[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    string_header[8..12].copy_from_slice(&3u32.to_le_bytes());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(parse_minidump_fixed_version(&string_header, usize::MAX).is_none());
        assert!(read_minidump_utf16_string(&string_header, usize::MAX).is_none());
        assert!(read_minidump_utf16_string(&string_header, 4).is_none());
        assert!(read_minidump_utf16_string(&string_header, 8).is_none());
    }));
    assert!(
        result.is_ok(),
        "hostile offsets and string lengths must fail soft"
    );
}

#[test]
fn render_info_reads_minidump_metadata_beyond_legacy_prefix() {
    let mut bytes = vec![0u8; 1024];
    bytes[0..4].copy_from_slice(b"MDMP");
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&600u32.to_le_bytes());
    bytes[600..604].copy_from_slice(&7u32.to_le_bytes());
    bytes[604..608].copy_from_slice(&32u32.to_le_bytes());
    bytes[608..612].copy_from_slice(&800u32.to_le_bytes());
    bytes[800..802].copy_from_slice(&9u16.to_le_bytes());
    bytes[806] = 8;
    bytes[807] = 1;
    bytes[808..812].copy_from_slice(&10u32.to_le_bytes());
    bytes[812..816].copy_from_slice(&0u32.to_le_bytes());
    bytes[816..820].copy_from_slice(&22631u32.to_le_bytes());
    bytes[820..824].copy_from_slice(&2u32.to_le_bytes());

    let path = std::env::temp_dir().join(format!(
        "quicklook-next-dump-render-{}-{}.dmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after epoch")
            .as_nanos()
    ));
    std::fs::write(&path, &bytes).expect("temporary dump fixture should be writable");
    let rendered = render_info(
        path.to_str().expect("temporary path must be valid UTF-8"),
        bytes.len() as i64,
        0,
    );
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("System architecture: x64"));
    assert!(rendered.contains("Windows version: 10.0.22631"));
}
