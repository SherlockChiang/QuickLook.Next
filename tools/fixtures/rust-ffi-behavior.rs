// Executable contract probes over the real common/raster modules, not stub implementations.
// Compiled without cfg(test), so child unit-test names/bodies cannot satisfy this guard.
use std::cell::Cell;
use std::ptr::{null, null_mut};

fn arguments_text() {
    let text = "éx".as_bytes();
    let invalid = [0xff];
    // SAFETY: All non-null ranges are live immutable arrays; null inputs must be rejected.
    unsafe {
        assert_eq!(
            ffi_common::utf8_arg(text.as_ptr(), text.len(), text.len()),
            Some("éx")
        );
        assert_eq!(ffi_common::utf8_arg(text.as_ptr(), 0, 0), Some(""));
        assert!(ffi_common::utf8_arg(null(), 0, 8).is_none());
        assert!(ffi_common::utf8_arg(null(), 1, 8).is_none());
        assert!(ffi_common::utf8_arg(invalid.as_ptr(), invalid.len(), 8).is_none());
        assert!(ffi_common::owned_utf8_arg(null(), 1, 8).is_none());
        assert!(ffi_common::owned_utf8_arg(invalid.as_ptr(), invalid.len(), 8).is_none());
    }
    let mut source = text.to_vec();
    // SAFETY: The source is live and immutable for this copying call, then may be changed.
    let owned = unsafe { ffi_common::owned_utf8_arg(source.as_ptr(), source.len(), source.len()) };
    source.fill(b'!');
    drop(source);
    assert_eq!(owned.as_deref(), Some("éx"));
}

fn arguments_optional() {
    let binary = [0, 0xff, 2];
    let text = b"abc";
    // SAFETY: Non-null ranges are live immutable arrays; optional null paths never access storage.
    unsafe {
        assert_eq!(ffi_common::optional_utf8_arg(null(), 0, 8), Some(""));
        assert!(ffi_common::optional_utf8_arg(null(), 1, 8).is_none());
        assert_eq!(ffi_common::optional_bytes_arg(null(), 0, 8), Some(&[][..]));
        assert!(ffi_common::optional_bytes_arg(null(), 1, 8).is_none());
        assert_eq!(
            ffi_common::optional_utf8_arg(text.as_ptr(), text.len(), 3),
            Some("abc")
        );
        assert_eq!(
            ffi_common::optional_bytes_arg(binary.as_ptr(), binary.len(), 3),
            Some(&binary[..])
        );
        assert!(ffi_common::optional_utf8_arg(binary.as_ptr(), binary.len(), 3).is_none());
    }
}

fn arguments_bounds() {
    let text = b"abcdefghi";
    for length in [7, 8, 9] {
        let expected = (length <= 8).then_some(&text[..length]);
        // SAFETY: Even over-budget probes use readable immutable storage for the full length.
        unsafe {
            assert_eq!(
                ffi_common::utf8_arg(text.as_ptr(), length, 8).map(str::as_bytes),
                expected
            );
            assert_eq!(
                ffi_common::owned_utf8_arg(text.as_ptr(), length, 8)
                    .as_deref()
                    .map(str::as_bytes),
                expected
            );
            assert_eq!(
                ffi_common::optional_utf8_arg(text.as_ptr(), length, 8).map(str::as_bytes),
                expected
            );
            assert_eq!(
                ffi_common::optional_bytes_arg(text.as_ptr(), length, 8),
                expected
            );
        }
    }
    let oversized = isize::MAX as usize + 1;
    // SAFETY: Out-of-range slice lengths must be rejected before accessing this pointer.
    // Mutation cases never remove the isize limit or the pointer rejection paths.
    unsafe {
        assert!(ffi_common::utf8_arg(text.as_ptr(), oversized, usize::MAX).is_none());
        assert!(ffi_common::owned_utf8_arg(text.as_ptr(), oversized, usize::MAX).is_none());
        assert!(ffi_common::optional_utf8_arg(text.as_ptr(), oversized, usize::MAX).is_none());
        assert!(ffi_common::optional_bytes_arg(text.as_ptr(), oversized, usize::MAX).is_none());
    }
}

fn legacy_empty() {
    for capacity in [0, 8] {
        // SAFETY: Empty legacy output must return before accessing null storage.
        unsafe {
            assert_eq!(ffi_common::write_bytes_out(&[], null_mut(), capacity), 0);
            assert_eq!(ffi_common::write_json_out("", null_mut(), capacity), 0);
        }
    }
}

fn legacy_output() {
    let payload = b"abc\0xyz";
    let mut storage = [0xa5; 24];
    // SAFETY: Initialized exclusive output is disjoint from payload, with canaries around it.
    let written = unsafe { ffi_common::write_bytes_out(payload, storage[4..].as_mut_ptr(), 16) };
    assert_eq!(written, payload.len() as i32);
    assert_eq!(&storage[4..4 + payload.len()], payload);
    assert!(storage[..4]
        .iter()
        .chain(storage[4 + payload.len()..].iter())
        .all(|b| *b == 0xa5));
    storage.fill(0xa5);
    // SAFETY: The short declared capacity must reject before copying; actual storage is larger.
    let short = unsafe { ffi_common::write_json_out("abcdefgh", storage[4..].as_mut_ptr(), 7) };
    assert_eq!(short, -8);
    assert_eq!(storage, [0xa5; 24]);
    assert_eq!(
        // SAFETY: Null/zero is only used for a nonempty size probe, never an actual copy.
        unsafe { ffi_common::write_bytes_out(payload, null_mut(), 0) },
        -(payload.len() as i32)
    );
}

fn v2_output() {
    let payload = b"xyz";
    let mut required = usize::MAX;
    let mut storage = [0xa5; 16];
    assert_eq!(
        // SAFETY: The aligned scalar is exclusive; the null short-capacity probe must not copy.
        unsafe { ffi_common::write_v2_out(payload, null_mut(), 0, &mut required) },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    assert_eq!(required, 3);
    assert_eq!(
        // SAFETY: Storage/scalar are live, initialized, disjoint; declared capacity is short.
        unsafe { ffi_common::write_v2_out(payload, storage[4..].as_mut_ptr(), 2, &mut required) },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    assert_eq!(required, 3);
    assert_eq!(storage, [0xa5; 16]);
    assert_eq!(
        // SAFETY: Scalar remains exclusive; sufficient-capacity null output must reject before copying.
        unsafe { ffi_common::write_v2_out(payload, null_mut(), 3, &mut required) },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(required, 3);
    assert_eq!(
        // SAFETY: Output and scalar are disjoint/exclusive and separate from input; storage is initialized.
        unsafe { ffi_common::write_v2_out(payload, storage[4..].as_mut_ptr(), 8, &mut required) },
        QL_OK
    );
    assert_eq!(required, 3);
    assert_eq!(&storage[4..7], payload);
    assert!(storage[..4]
        .iter()
        .chain(storage[7..].iter())
        .all(|b| *b == 0xa5));
    required = usize::MAX;
    assert_eq!(
        // SAFETY: Empty output only writes the scalar and never accesses null storage.
        unsafe { ffi_common::write_v2_out(&[], null_mut(), 0, &mut required) },
        QL_OK
    );
    assert_eq!(required, 0);
}

fn pixels() -> [u8; 24] {
    std::array::from_fn(|index| (index + 1) as u8)
}

fn check_packet(storage: &[u8], bgra: &[u8]) {
    assert_eq!(&storage[..4], &2u32.to_le_bytes());
    assert_eq!(&storage[4..8], &3u32.to_le_bytes());
    assert_eq!(&storage[8..], bgra);
}

fn raster_legacy() {
    let bgra = pixels();
    let mut storage = [0xa5; 48];
    // SAFETY: Valid geometry size probes use null output, so no output slice is formed.
    unsafe {
        assert_eq!(
            raster_packet::write_raster_packet(2, 3, &bgra, null_mut(), 0),
            -32
        );
        assert_eq!(
            raster_packet::write_raster_packet(2, 3, &bgra, null_mut(), 32),
            -32
        );
    }
    assert_eq!(
        // SAFETY: Initialized output has spare allocation space but a short declared capacity.
        unsafe { raster_packet::write_raster_packet(2, 3, &bgra, storage[4..].as_mut_ptr(), 31) },
        -32
    );
    assert_eq!(storage, [0xa5; 48]);
    assert_eq!(
        // SAFETY: Initialized output is exclusive and disjoint from bgra, with sufficient capacity.
        unsafe { raster_packet::write_raster_packet(2, 3, &bgra, storage[4..].as_mut_ptr(), 40) },
        32
    );
    check_packet(&storage[4..36], &bgra);
    assert!(storage[..4]
        .iter()
        .chain(storage[36..].iter())
        .all(|b| *b == 0xa5));
}

fn raster_v2() {
    let bgra = pixels();
    let mut storage = [0xa5; 48];
    let mut required = usize::MAX;
    assert_eq!(
        // SAFETY: Exclusive aligned scalar; null/short-capacity output cannot be accessed.
        unsafe { raster_packet::write_raster_packet_v2(2, 3, &bgra, null_mut(), 0, &mut required) },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    assert_eq!(required, 32);
    assert_eq!(
        // SAFETY: Initialized output and scalar are disjoint from each other/input; declared capacity is short.
        unsafe {
            raster_packet::write_raster_packet_v2(
                2,
                3,
                &bgra,
                storage[4..].as_mut_ptr(),
                31,
                &mut required,
            )
        },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    assert_eq!(required, 32);
    assert_eq!(storage, [0xa5; 48]);
    assert_eq!(
        // SAFETY: Scalar is aligned/exclusive; sufficient-capacity null output must be rejected.
        unsafe {
            raster_packet::write_raster_packet_v2(2, 3, &bgra, null_mut(), 32, &mut required)
        },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(required, 32);
    assert_eq!(
        // SAFETY: Live initialized output and scalar are exclusive, disjoint, and separate from bgra.
        unsafe {
            raster_packet::write_raster_packet_v2(
                2,
                3,
                &bgra,
                storage[4..].as_mut_ptr(),
                40,
                &mut required,
            )
        },
        QL_OK
    );
    assert_eq!(required, 32);
    check_packet(&storage[4..36], &bgra);
    assert!(storage[..4]
        .iter()
        .chain(storage[36..].iter())
        .all(|b| *b == 0xa5));
}

fn raster_geometry() {
    let bgra = pixels();
    let excessive = [1u8; 28];
    for (width, height, bytes) in [
        (0, 3, &[][..]),
        (2, 0, &[][..]),
        (2, 3, &bgra[..20]),
        (2, 3, &excessive[..]),
        (u32::MAX, u32::MAX, &[][..]),
    ] {
        let mut required = usize::MAX;
        // SAFETY: Geometry failures must not access null output; scalar is aligned/exclusive.
        // Mutations retain overflow checks and use no writable output on these probes.
        unsafe {
            assert_eq!(
                raster_packet::write_raster_packet(width, height, bytes, null_mut(), 0),
                -2
            );
            assert_eq!(
                raster_packet::write_raster_packet_v2(
                    width,
                    height,
                    bytes,
                    null_mut(),
                    0,
                    &mut required
                ),
                QL_ERROR_INTERNAL
            );
        }
        assert_eq!(required, 0);
    }
}

fn file_probe() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("owned fixture directory"));
    let path = root.join(format!(
        "rust-ffi-behavior-{}-probe.txt",
        std::process::id()
    ));
    let missing = root.join(format!(
        "rust-ffi-behavior-{}-missing.txt",
        std::process::id()
    ));
    std::fs::write(&path, b"hello from the probe").expect("probe fixture file");
    let path_string = path.to_string_lossy().into_owned();
    let path_bytes = path_string.as_bytes();
    let missing_string = missing.to_string_lossy().into_owned();
    let missing_bytes = missing_string.as_bytes();
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();

    assert_eq!(
        // SAFETY: The oversized path length must be rejected before the dangling pointer is read.
        unsafe {
            ffi::file_probe::ql_probe_file(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                std::ptr::null_mut(),
                0,
            )
        },
        -1
    );
    assert_eq!(
        // SAFETY: The path and output are live; the missing path must fail before output access.
        unsafe {
            ffi::file_probe::ql_probe_file(
                missing_bytes.as_ptr(),
                missing_bytes.len(),
                std::ptr::null_mut(),
                0,
            )
        },
        -2
    );

    let mut storage = [0xa5u8; 4096];
    // SAFETY: A null output with zero capacity is only a required-size probe.
    let required = unsafe {
        ffi::file_probe::ql_probe_file(
            path_bytes.as_ptr(),
            path_bytes.len(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert!(required < 0);
    let required = (-required) as usize;
    assert!(required < storage.len() - 8);

    // SAFETY: The initialized allocation is larger than the declared short capacity, so a
    // mutated capacity guard is detected by the canary without allowing an out-of-allocation write.
    let short = unsafe {
        ffi::file_probe::ql_probe_file(
            path_bytes.as_ptr(),
            path_bytes.len(),
            storage[4..].as_mut_ptr(),
            required - 1,
        )
    };
    assert_eq!(short, -(required as i32));
    assert!(storage[..4].iter().all(|byte| *byte == 0xa5));
    assert!(storage[4..].iter().all(|byte| *byte == 0xa5));

    // SAFETY: The path and output are disjoint live storage with enough declared capacity.
    let written = unsafe {
        ffi::file_probe::ql_probe_file(
            path_bytes.as_ptr(),
            path_bytes.len(),
            storage[4..].as_mut_ptr(),
            storage.len() - 4,
        )
    };
    assert_eq!(written, required as i32);
    let output = &storage[4..4 + required];
    let output = std::str::from_utf8(output).expect("probe JSON must be UTF-8");
    assert!(output.contains("\"extension\":\".txt\""));
    assert!(output.contains("\"kind\":\"text\""));
    assert!(output.contains("rust-ffi-behavior-"));
    assert!(storage[4 + required..].iter().all(|byte| *byte == 0xa5));

    std::fs::remove_file(path).expect("remove probe fixture file");
}

extern "C" fn always_cancel() -> bool {
    true
}

fn thumbnail() {
    let path = b"thumb.thumbnail";
    let missing = b"missing.thumbnail";
    let invalid = [0xffu8];
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    let mut storage = [0xa5u8; 64];

    assert_eq!(
        // SAFETY: The oversized input returns before the dangling pointer is accessed.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                32,
                0,
                storage.as_mut_ptr(),
                storage.len(),
                None,
            )
        },
        -1
    );
    assert_eq!(
        // SAFETY: The invalid UTF-8 bytes are live and output is initialized/disjoint.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                invalid.as_ptr(),
                invalid.len(),
                32,
                0,
                storage.as_mut_ptr(),
                storage.len(),
                None,
            )
        },
        -1
    );
    assert_eq!(
        // SAFETY: Live path/output arrays and a static callback satisfy the adapter contract.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                path.as_ptr(),
                path.len(),
                32,
                4,
                storage.as_mut_ptr(),
                storage.len(),
                None,
            )
        },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        // SAFETY: Live path/output arrays satisfy the adapter contract; size is out of bounds.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                path.as_ptr(),
                path.len(),
                513,
                0,
                storage.as_mut_ptr(),
                storage.len(),
                None,
            )
        },
        QL_ERROR_LIMIT_EXCEEDED
    );
    assert_eq!(
        // SAFETY: Live path/output arrays and a static callback satisfy the adapter contract.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                path.as_ptr(),
                path.len(),
                32,
                0,
                storage.as_mut_ptr(),
                storage.len(),
                Some(always_cancel),
            )
        },
        QL_ERROR_CANCELLED
    );
    assert_eq!(
        // SAFETY: Live path/output arrays satisfy the adapter contract; the stub reports unavailable.
        unsafe {
            ffi::thumbnail::ql_get_thumbnail_cancelable_with_flags(
                missing.as_ptr(),
                missing.len(),
                32,
                0,
                storage.as_mut_ptr(),
                storage.len(),
                None,
            )
        },
        QL_ERROR_BUFFER_TOO_SMALL
    );

    storage.fill(0xa5);
    // SAFETY: A valid thumbnail packet has 32 bytes including the 8-byte geometry header.
    let short = unsafe {
        ffi::thumbnail::ql_get_thumbnail(
            path.as_ptr(),
            path.len(),
            32,
            storage[4..].as_mut_ptr(),
            31,
        )
    };
    assert_eq!(short, -32);
    assert!(storage.iter().all(|byte| *byte == 0xa5));
    // SAFETY: Output allocation and thumbnail input are disjoint and sufficiently sized.
    let written = unsafe {
        ffi::thumbnail::ql_get_thumbnail(
            path.as_ptr(),
            path.len(),
            32,
            storage[4..].as_mut_ptr(),
            40,
        )
    };
    assert_eq!(written, 32);
    assert_eq!(&storage[4..8], &2u32.to_le_bytes());
    assert_eq!(&storage[8..12], &3u32.to_le_bytes());
    assert_eq!(&storage[12..36], &(1u8..=24).collect::<Vec<_>>());
    assert!(storage[..4].iter().all(|byte| *byte == 0xa5));
    assert!(storage[36..].iter().all(|byte| *byte == 0xa5));
}

struct UnwindMark<'a>(&'a Cell<usize>);
impl Drop for UnwindMark<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn panic_i32() {
    let calls = Cell::new(0);
    assert_eq!(
        ffi_common::ffi_boundary(|| {
            calls.set(calls.get() + 1);
            -42
        }),
        -42
    );
    assert_eq!(calls.get(), 1);
    let drops = Cell::new(0);
    assert_eq!(
        ffi_common::ffi_boundary(|| {
            let _mark = UnwindMark(&drops);
            panic!("i32 probe");
        }),
        QL_ERROR_INTERNAL
    );
    assert_eq!(drops.get(), 1);
}

fn panic_void() {
    let calls = Cell::new(0);
    ffi_common::ffi_void_boundary(|| calls.set(calls.get() + 1));
    assert_eq!(calls.get(), 1);
    let drops = Cell::new(0);
    ffi_common::ffi_void_boundary(|| {
        let _mark = UnwindMark(&drops);
        panic!("void probe");
    });
    assert_eq!(drops.get(), 1);
    ffi_common::ffi_void_boundary(|| calls.set(calls.get() + 1));
    assert_eq!(calls.get(), 2);
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let cases: [(&str, fn()); 22] = [
        ("arguments_text", arguments_text),
        ("arguments_optional", arguments_optional),
        ("arguments_bounds", arguments_bounds),
        ("legacy_empty", legacy_empty),
        ("legacy_output", legacy_output),
        ("v2_output", v2_output),
        ("raster_legacy", raster_legacy),
        ("raster_v2", raster_v2),
        ("raster_geometry", raster_geometry),
        ("file_probe", file_probe),
        ("thumbnail", thumbnail),
        ("panic_i32", panic_i32),
        ("panic_void", panic_void),
        ("path_outputs", path_behavior::path_outputs),
        ("path_arguments", path_behavior::path_arguments),
        ("path_cancellation", path_behavior::path_cancellation),
        ("path_optional", path_behavior::path_optional),
        ("path_panics", path_behavior::path_panics),
        (
            "routing_classification",
            routing_behavior::routing_classification,
        ),
        ("routing_optional", routing_behavior::routing_optional),
        ("routing_folder", routing_behavior::routing_folder),
        ("routing_panics", routing_behavior::routing_panics),
    ];
    let group_count = cases.len();
    for (name, probe) in cases {
        if std::panic::catch_unwind(probe).is_err() {
            eprintln!("FFI_CONTRACT_FAILED:{name}");
            std::process::exit(1);
        }
    }
    println!("FFI_CONTRACT_PASSED:{group_count}");
}
