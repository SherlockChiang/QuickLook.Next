use super::{append_summary, render_info};

#[test]
fn elf_summary_detects_64_bit_little_endian() {
    let mut bytes = vec![0u8; 2048];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&0x40u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&0x500u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&4u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    bytes[60..62].copy_from_slice(&7u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x40..0x44].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x48..0x50].copy_from_slice(&0x300u64.to_le_bytes());
    bytes[0x60..0x68].copy_from_slice(&28u64.to_le_bytes());
    bytes[0x78..0x7C].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x80..0x88].copy_from_slice(&0u64.to_le_bytes());
    bytes[0x88..0x90].copy_from_slice(&0x400000u64.to_le_bytes());
    bytes[0x98..0xA0].copy_from_slice(&0x400u64.to_le_bytes());
    bytes[0xA0..0xA8].copy_from_slice(&0x400u64.to_le_bytes());
    bytes[0xB0..0xB4].copy_from_slice(&2u32.to_le_bytes());
    bytes[0xB8..0xC0].copy_from_slice(&0x200u64.to_le_bytes());
    bytes[0xD0..0xD8].copy_from_slice(&80u64.to_le_bytes());
    bytes[0xE8..0xEC].copy_from_slice(&4u32.to_le_bytes());
    bytes[0xF0..0xF8].copy_from_slice(&0x7C4u64.to_le_bytes());
    bytes[0x108..0x110].copy_from_slice(&20u64.to_le_bytes());
    bytes[0x200..0x208].copy_from_slice(&5u64.to_le_bytes());
    bytes[0x208..0x210].copy_from_slice(&0x400280u64.to_le_bytes());
    bytes[0x210..0x218].copy_from_slice(&1u64.to_le_bytes());
    bytes[0x218..0x220].copy_from_slice(&0u64.to_le_bytes());
    bytes[0x220..0x228].copy_from_slice(&14u64.to_le_bytes());
    bytes[0x228..0x230].copy_from_slice(&10u64.to_le_bytes());
    bytes[0x230..0x238].copy_from_slice(&29u64.to_le_bytes());
    bytes[0x238..0x240].copy_from_slice(&21u64.to_le_bytes());
    bytes[0x240..0x248].copy_from_slice(&0u64.to_le_bytes());
    bytes[0x280..0x28A].copy_from_slice(b"libc.so.6\0");
    bytes[0x28A..0x295].copy_from_slice(b"libdemo.so\0");
    bytes[0x295..0x29D].copy_from_slice(b"$ORIGIN\0");
    bytes[0x300..0x31B].copy_from_slice(b"/lib64/ld-linux-x86-64.so.2");
    bytes[0x540..0x544].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x580..0x584].copy_from_slice(&7u32.to_le_bytes());
    bytes[0x598..0x5A0].copy_from_slice(&0x700u64.to_le_bytes());
    bytes[0x5A0..0x5A8].copy_from_slice(&62u64.to_le_bytes());
    bytes[0x5C0..0x5C4].copy_from_slice(&17u32.to_le_bytes());
    bytes[0x5C4..0x5C8].copy_from_slice(&2u32.to_le_bytes());
    bytes[0x5D8..0x5E0].copy_from_slice(&0x740u64.to_le_bytes());
    bytes[0x5E0..0x5E8].copy_from_slice(&48u64.to_le_bytes());
    bytes[0x5E8..0x5EC].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x5F8..0x600].copy_from_slice(&24u64.to_le_bytes());
    bytes[0x600..0x604].copy_from_slice(&25u32.to_le_bytes());
    bytes[0x604..0x608].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x618..0x620].copy_from_slice(&0x780u64.to_le_bytes());
    bytes[0x620..0x628].copy_from_slice(&13u64.to_le_bytes());
    bytes[0x640..0x644].copy_from_slice(&33u32.to_le_bytes());
    bytes[0x644..0x648].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x658..0x660].copy_from_slice(&0x790u64.to_le_bytes());
    bytes[0x660..0x668].copy_from_slice(&24u64.to_le_bytes());
    bytes[0x678..0x680].copy_from_slice(&24u64.to_le_bytes());
    bytes[0x680..0x684].copy_from_slice(&43u32.to_le_bytes());
    bytes[0x684..0x688].copy_from_slice(&7u32.to_le_bytes());
    bytes[0x698..0x6A0].copy_from_slice(&0x7B0u64.to_le_bytes());
    bytes[0x6A0..0x6A8].copy_from_slice(&20u64.to_le_bytes());
    bytes[0x700..0x73E]
        .copy_from_slice(b"\0.text\0.shstrtab\0.symtab\0.strtab\0.rela.dyn\0.note.gnu.build-id\0");
    bytes[0x740..0x744].copy_from_slice(&0u32.to_le_bytes());
    bytes[0x758..0x75C].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x75C] = 0x12;
    bytes[0x75E..0x760].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x780..0x78D].copy_from_slice(b"\0main\0helper\0");
    bytes[0x7B0..0x7B4].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x7B4..0x7B8].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x7B8..0x7BC].copy_from_slice(&3u32.to_le_bytes());
    bytes[0x7BC..0x7C0].copy_from_slice(b"GNU\0");
    bytes[0x7C0..0x7C4].copy_from_slice(&[1, 2, 3, 4]);
    bytes[0x7C4..0x7C8].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x7C8..0x7CC].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x7CC..0x7D0].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x7D0..0x7D4].copy_from_slice(b"GNU\0");
    bytes[0x7D4..0x7D8].copy_from_slice(&[5, 6, 7, 8]);
    bytes[0x798..0x7A0].copy_from_slice(&8u64.to_le_bytes());

    let mut text = String::new();
    append_summary(&mut text, &bytes);

    assert!(text.contains("ELF64"));
    assert!(text.contains("x86-64"));
    assert!(text.contains("0x0000000000401000"));
    assert!(text.contains("Program headers: 4"));
    assert!(text.contains("Section headers: 7"));
    assert!(text.contains("Program header offset: 0x40"));
    assert!(text.contains("Section header offset: 0x500"));
    assert!(text.contains("Interpreter: /lib64/ld-linux-x86-64.so.2"));
    assert!(text.contains("Needed libraries: libc.so.6"));
    assert!(text.contains("SONAME: libdemo.so"));
    assert!(text.contains("RUNPATH: $ORIGIN"));
    assert!(text.contains(
        "Section names: .text, .shstrtab, .symtab, .strtab, .rela.dyn, .note.gnu.build-id"
    ));
    assert!(text.contains("Symbols: .symtab 2 entries (main[global func .text])"));
    assert!(text.contains("Relocations: .rela.dyn 1 entries (R_X86_64_RELATIVE)"));
    assert!(text.contains("Notes: .note.gnu.build-id GNU build-id 01020304"));
    assert!(text.contains("PT_NOTE GNU type 1 (4 bytes)"));
}

#[test]
fn elf_summary_reads_gnu_version_sections() {
    struct ElfSection64 {
        name: u32,
        typ: u32,
        offset: u64,
        size: u64,
        link: u32,
        entry_size: u64,
    }

    fn write_sh64(bytes: &mut [u8], index: usize, section: ElfSection64) {
        let base = 0x100 + index * 64;
        bytes[base..base + 4].copy_from_slice(&section.name.to_le_bytes());
        bytes[base + 4..base + 8].copy_from_slice(&section.typ.to_le_bytes());
        bytes[base + 24..base + 32].copy_from_slice(&section.offset.to_le_bytes());
        bytes[base + 32..base + 40].copy_from_slice(&section.size.to_le_bytes());
        bytes[base + 40..base + 44].copy_from_slice(&section.link.to_le_bytes());
        bytes[base + 56..base + 64].copy_from_slice(&section.entry_size.to_le_bytes());
    }

    let mut bytes = vec![0u8; 1024];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[40..48].copy_from_slice(&0x100u64.to_le_bytes());
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    bytes[60..62].copy_from_slice(&6u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&1u16.to_le_bytes());
    write_sh64(
        &mut bytes,
        1,
        ElfSection64 {
            name: 1,
            typ: 3,
            offset: 0x300,
            size: 62,
            link: 0,
            entry_size: 0,
        },
    );
    write_sh64(
        &mut bytes,
        2,
        ElfSection64 {
            name: 11,
            typ: 3,
            offset: 0x340,
            size: 27,
            link: 0,
            entry_size: 0,
        },
    );
    write_sh64(
        &mut bytes,
        3,
        ElfSection64 {
            name: 19,
            typ: 0x6FFF_FFFF,
            offset: 0x380,
            size: 6,
            link: 0,
            entry_size: 2,
        },
    );
    write_sh64(
        &mut bytes,
        4,
        ElfSection64 {
            name: 32,
            typ: 0x6FFF_FFFE,
            offset: 0x390,
            size: 32,
            link: 2,
            entry_size: 0,
        },
    );
    write_sh64(
        &mut bytes,
        5,
        ElfSection64 {
            name: 47,
            typ: 0x6FFF_FFFD,
            offset: 0x3C0,
            size: 28,
            link: 2,
            entry_size: 0,
        },
    );
    bytes[0x300..0x33E]
        .copy_from_slice(b"\0.shstrtab\0.dynstr\0.gnu.version\0.gnu.version_r\0.gnu.version_d\0");
    bytes[0x340..0x35B].copy_from_slice(b"\0GLIBC_2.2.5\0QUICKLOOK_1.0\0");
    bytes[0x380..0x382].copy_from_slice(&0u16.to_le_bytes());
    bytes[0x382..0x384].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x384..0x386].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x390..0x392].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x392..0x394].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x398..0x39C].copy_from_slice(&16u32.to_le_bytes());
    bytes[0x3A4..0x3A6].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x3A8..0x3AC].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x3C0..0x3C2].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x3C4..0x3C6].copy_from_slice(&0u16.to_le_bytes());
    bytes[0x3C6..0x3C8].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x3CC..0x3D0].copy_from_slice(&20u32.to_le_bytes());
    bytes[0x3D4..0x3D8].copy_from_slice(&13u32.to_le_bytes());
    let mut text = String::new();

    append_summary(&mut text, &bytes);

    assert!(text.contains("GNU versions: .gnu.version 3 entries (2/3)"));
    assert!(text.contains(".gnu.version_r needs GLIBC_2.2.5"));
    assert!(text.contains(".gnu.version_d defines QUICKLOOK_1.0"));
}

#[test]
fn elf_summary_accepts_elf32_big_endian() {
    let mut bytes = vec![0u8; 128];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 1;
    bytes[5] = 2;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&2u16.to_be_bytes());
    bytes[18..20].copy_from_slice(&40u16.to_be_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&0x0010_0000u32.to_be_bytes());
    bytes[36..40].copy_from_slice(&0u32.to_be_bytes());
    bytes[40..42].copy_from_slice(&52u16.to_be_bytes());
    bytes[42..44].copy_from_slice(&32u16.to_be_bytes());
    bytes[44..46].copy_from_slice(&0u16.to_be_bytes());
    bytes[46..48].copy_from_slice(&40u16.to_be_bytes());
    bytes[48..50].copy_from_slice(&0u16.to_be_bytes());
    bytes[50..52].copy_from_slice(&0u16.to_be_bytes());

    let mut text = String::new();
    append_summary(&mut text, &bytes);

    assert!(text.contains("ELF32"));
    assert!(text.contains("Endian: big"));
    assert!(text.contains("Machine: ARM"));
    assert!(text.contains("Entry: 0x00100000"));
}

#[test]
fn elf_summary_rejects_truncated_and_hostile_offsets_without_panicking() {
    let mut hostile = vec![0u8; 128];
    hostile[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    hostile[4] = 2;
    hostile[5] = 1;
    hostile[6] = 1;
    hostile[20..24].copy_from_slice(&1u32.to_le_bytes());
    hostile[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    hostile[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    hostile[52..54].copy_from_slice(&64u16.to_le_bytes());
    hostile[54..56].copy_from_slice(&56u16.to_le_bytes());
    hostile[56..58].copy_from_slice(&64u16.to_le_bytes());
    hostile[58..60].copy_from_slice(&64u16.to_le_bytes());
    hostile[60..62].copy_from_slice(&128u16.to_le_bytes());
    hostile[62..64].copy_from_slice(&0u16.to_le_bytes());

    let cases = [hostile, vec![0x7F, b'E', b'L', b'F'], {
        let mut invalid = vec![0u8; 64];
        invalid[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        invalid[4] = 3;
        invalid[5] = 9;
        invalid
    }];
    for bytes in cases {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut text = String::new();
            append_summary(&mut text, &bytes);
        }));
        assert!(result.is_ok(), "hostile ELF input must fail soft");
    }
}

#[test]
fn render_info_reads_bounded_metadata_beyond_legacy_prefix() {
    let mut bytes = vec![0u8; 2048];
    bytes[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[40..48].copy_from_slice(&0x600u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    bytes[60..62].copy_from_slice(&2u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&1u16.to_le_bytes());
    let section = 0x600 + 64;
    bytes[section..section + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[section + 4..section + 8].copy_from_slice(&3u32.to_le_bytes());
    bytes[section + 24..section + 32].copy_from_slice(&0x700u64.to_le_bytes());
    bytes[section + 32..section + 40].copy_from_slice(&11u64.to_le_bytes());
    bytes[0x700..0x70B].copy_from_slice(b"\0.shstrtab\0");

    let path = std::env::temp_dir().join(format!(
        "quicklook-next-elf-render-{}-{}.elf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after epoch")
            .as_nanos()
    ));
    std::fs::write(&path, &bytes).expect("temporary ELF fixture should be writable");
    let rendered = render_info(
        path.to_str().expect("temporary path must be valid UTF-8"),
        bytes.len() as i64,
        0,
    );
    let _ = std::fs::remove_file(&path);

    assert!(rendered.contains("Section names: .shstrtab"));
}
