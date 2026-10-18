//! Contracts over the actual routing exports; providers observe dispatch, not format/file I/O.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use crate::ffi::routing;
use crate::{CancelCallback, MAX_FFI_MAGIC_BYTES, MAX_FFI_STRING_BYTES, QL_ERROR_INTERNAL};

const PAYLOAD: &str = "{\"folder\":\"目录-é\"}";
const PATH: &str = "C:\\目录-é";
const MAGIC: &[u8] = &[0xff, 0, b'P', b'K'];

#[derive(Debug, PartialEq)]
struct Classification {
    archive: bool,
    extension: String,
    kind: String,
    magic: Vec<u8>,
}

struct FolderCall {
    path: String,
    callback: Option<CancelCallback>,
}

#[derive(Clone, Copy)]
enum FolderMode {
    Success,
    Empty,
    Panic,
}

thread_local! {
    static CLASSIFICATIONS: RefCell<Vec<Classification>> = const { RefCell::new(Vec::new()) };
    static FOLDERS: RefCell<Vec<FolderCall>> = const { RefCell::new(Vec::new()) };
    static MATCHES: Cell<bool> = const { Cell::new(true) };
    static PANIC: Cell<bool> = const { Cell::new(false) };
    static FOLDER_MODE: Cell<FolderMode> = const { Cell::new(FolderMode::Success) };
    static WORK: Cell<usize> = const { Cell::new(0) };
    static DROPS: Cell<usize> = const { Cell::new(0) };
    static POLLS: Cell<usize> = const { Cell::new(0) };
}

fn reset() {
    CLASSIFICATIONS.with(|calls| calls.borrow_mut().clear());
    FOLDERS.with(|calls| calls.borrow_mut().clear());
    MATCHES.set(true);
    PANIC.set(false);
    FOLDER_MODE.set(FolderMode::Success);
    WORK.set(0);
    DROPS.set(0);
    POLLS.set(0);
}

struct ProviderGuard;
impl Drop for ProviderGuard {
    fn drop(&mut self) {
        DROPS.set(DROPS.get() + 1);
    }
}

pub mod provider {
    use super::{
        Classification, FolderCall, FolderMode, ProviderGuard, CLASSIFICATIONS, DROPS, FOLDERS,
        FOLDER_MODE, MATCHES, PANIC, PAYLOAD, WORK,
    };
    use crate::{cancel_requested, CancelCallback};

    fn classify(archive: bool, extension: &str, kind: &str, magic: &[u8]) -> bool {
        let _guard = ProviderGuard;
        CLASSIFICATIONS.with(|calls| {
            calls.borrow_mut().push(Classification {
                archive,
                extension: extension.to_owned(),
                kind: kind.to_owned(),
                magic: magic.to_vec(),
            });
        });
        assert_eq!(DROPS.get(), 0);
        assert!(!PANIC.get(), "classifier panic probe");
        MATCHES.get()
    }

    pub fn is_text(extension: &str, magic: &[u8]) -> bool {
        classify(false, extension, "", magic)
    }

    pub fn is_archive(extension: &str, kind: &str, magic: &[u8]) -> bool {
        classify(true, extension, kind, magic)
    }

    pub fn render_folder(path: &str, callback: Option<CancelCallback>) -> String {
        let _guard = ProviderGuard;
        FOLDERS.with(|calls| {
            calls.borrow_mut().push(FolderCall {
                path: path.to_owned(),
                callback,
            });
        });
        // The real folder adapter delegates cancellation to the provider.
        if cancel_requested(callback) {
            return String::new();
        }
        WORK.set(WORK.get() + 1);
        match FOLDER_MODE.get() {
            FolderMode::Success => PAYLOAD.to_owned(),
            FolderMode::Empty => String::new(),
            FolderMode::Panic => panic!("folder provider panic probe"),
        }
    }
}

#[derive(Clone, Copy)]
struct Arg {
    pointer: *const u8,
    length: usize,
}

impl Arg {
    fn bytes(bytes: &[u8]) -> Self {
        Self {
            pointer: bytes.as_ptr(),
            length: bytes.len(),
        }
    }
}

const ABSENT: Arg = Arg {
    pointer: null(),
    length: 0,
};

/// # Safety
/// Non-null ranges must remain readable/immutable, except documented oversized rejections.
unsafe fn classify(archive: bool, extension: Arg, kind: Arg, magic: Arg) -> i32 {
    // SAFETY: The caller establishes all input ranges; forwarding does not expand them.
    unsafe {
        if archive {
            routing::ql_is_archive(
                extension.pointer,
                extension.length,
                kind.pointer,
                kind.length,
                magic.pointer,
                magic.length,
            )
        } else {
            routing::ql_is_text(
                extension.pointer,
                extension.length,
                magic.pointer,
                magic.length,
            )
        }
    }
}

fn observed(archive: bool, extension: &str, kind: &str, magic: &[u8]) {
    CLASSIFICATIONS.with(|calls| {
        assert_eq!(
            *calls.borrow(),
            [Classification {
                archive,
                extension: extension.to_owned(),
                kind: if archive {
                    kind.to_owned()
                } else {
                    String::new()
                },
                magic: magic.to_vec(),
            }]
        );
    });
    assert!(FOLDERS.with(|calls| calls.borrow().is_empty()));
    assert_eq!(DROPS.get(), 1);
}

pub fn routing_classification() {
    for archive in [false, true] {
        for matches in [false, true] {
            reset();
            MATCHES.set(matches);
            assert_eq!(
                // SAFETY: All inputs are static, readable and immutable for their exact lengths.
                unsafe {
                    classify(
                        archive,
                        Arg::bytes(".ZiP-é".as_bytes()),
                        Arg::bytes(b"Package\0hint"),
                        Arg::bytes(MAGIC),
                    )
                },
                i32::from(matches)
            );
            observed(archive, ".ZiP-é", "Package\0hint", MAGIC);
        }
    }
}

pub fn routing_optional() {
    let full = vec![b'x'; MAX_FFI_STRING_BYTES];
    let over = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    let full_magic = vec![0xff; MAX_FFI_MAGIC_BYTES];
    let over_magic = vec![0xff; MAX_FFI_MAGIC_BYTES + 1];
    for archive in [false, true] {
        for (argument, expected) in [
            (ABSENT, ""),
            (
                Arg {
                    length: 1,
                    ..ABSENT
                },
                "",
            ),
            (Arg::bytes(b""), ""),
            (Arg::bytes(&[0xff]), ""),
            (Arg::bytes(&over), ""),
            (
                Arg {
                    pointer: full.as_ptr(),
                    length: isize::MAX as usize + 1,
                },
                "",
            ),
            (Arg::bytes(&full), std::str::from_utf8(&full).unwrap()),
        ] {
            reset();
            assert_eq!(
                // SAFETY: Arrays cover all accepted lengths; oversized lengths must reject before access.
                unsafe { classify(archive, argument, Arg::bytes(b"kind"), Arg::bytes(MAGIC)) },
                1
            );
            observed(archive, expected, "kind", MAGIC);
            if archive {
                reset();
                assert_eq!(
                    // SAFETY: The same optional-input rejection contract applies to the kind hint.
                    unsafe { classify(true, Arg::bytes(b".zip"), argument, Arg::bytes(MAGIC)) },
                    1
                );
                observed(true, ".zip", expected, MAGIC);
            }
        }
        for (argument, expected) in [
            (ABSENT, Some(&[][..])),
            (
                Arg {
                    length: 1,
                    ..ABSENT
                },
                None,
            ),
            (Arg::bytes(b""), Some(&[][..])),
            (Arg::bytes(&full_magic), Some(full_magic.as_slice())),
            (Arg::bytes(&over_magic), None),
            (
                Arg {
                    pointer: full_magic.as_ptr(),
                    length: isize::MAX as usize + 1,
                },
                None,
            ),
        ] {
            reset();
            assert_eq!(
                // SAFETY: Binary inputs cover accepted ranges; null/oversized cases reject without access.
                unsafe { classify(archive, ABSENT, ABSENT, argument) },
                i32::from(expected.is_some())
            );
            if let Some(bytes) = expected {
                observed(archive, "", "", bytes);
            } else {
                assert!(CLASSIFICATIONS.with(|calls| calls.borrow().is_empty()));
                assert_eq!(DROPS.get(), 0);
            }
        }
    }
}

extern "C" fn cancelled() -> bool {
    POLLS.set(POLLS.get() + 1);
    true
}

extern "C" fn never_cancel() -> bool {
    POLLS.set(POLLS.get() + 1);
    false
}

fn folder_call(path: &str, callback: Option<CancelCallback>) {
    FOLDERS.with(|calls| {
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].path, path);
        match (calls[0].callback, callback) {
            (None, None) => (),
            (Some(actual), Some(expected)) => assert!(std::ptr::fn_addr_eq(actual, expected)),
            _ => panic!("folder callback forwarding"),
        }
    });
    assert!(CLASSIFICATIONS.with(|calls| calls.borrow().is_empty()));
    assert_eq!(DROPS.get(), 1);
}

pub fn routing_folder() {
    let mut output = [0xa5; 160];
    let full = vec![b'x'; MAX_FFI_STRING_BYTES];
    let over = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    for path in [
        ABSENT,
        Arg {
            length: 1,
            ..ABSENT
        },
        Arg::bytes(&[0xff]),
        Arg::bytes(&over),
        Arg {
            pointer: full.as_ptr(),
            length: isize::MAX as usize + 1,
        },
    ] {
        reset();
        assert_eq!(
            // SAFETY: Invalid inputs are rejected before access; output is initialized/exclusive.
            unsafe {
                routing::ql_preview_folder(
                    path.pointer,
                    path.length,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            0
        );
        assert_eq!(output, [0xa5; 160]);
        assert!(FOLDERS.with(|calls| calls.borrow().is_empty()));
    }
    for capacity in [0, 128] {
        reset();
        assert_eq!(
            // SAFETY: Null output is rejected before rendering/copying despite the declared capacity.
            unsafe {
                routing::ql_preview_folder(PATH.as_ptr(), PATH.len(), null_mut(), capacity, None)
            },
            0
        );
        assert!(FOLDERS.with(|calls| calls.borrow().is_empty()));
    }
    for capacity in [0, PAYLOAD.len() - 1, PAYLOAD.len(), 128] {
        reset();
        output.fill(0xa5);
        assert_eq!(
            // SAFETY: Initialized allocation has prefix/suffix canaries and covers every declared capacity.
            unsafe {
                routing::ql_preview_folder(
                    PATH.as_ptr(),
                    PATH.len(),
                    output[4..].as_mut_ptr(),
                    capacity,
                    Some(never_cancel),
                )
            },
            if capacity == 0 {
                0
            } else if capacity < PAYLOAD.len() {
                -(PAYLOAD.len() as i32)
            } else {
                PAYLOAD.len() as i32
            }
        );
        if capacity == 0 {
            assert!(FOLDERS.with(|calls| calls.borrow().is_empty()));
            assert_eq!(POLLS.get(), 0);
        } else {
            folder_call(PATH, Some(never_cancel));
            assert_eq!(POLLS.get(), 1);
        }
        if capacity >= PAYLOAD.len() {
            assert_eq!(&output[4..4 + PAYLOAD.len()], PAYLOAD.as_bytes());
            assert!(output[..4]
                .iter()
                .chain(output[4 + PAYLOAD.len()..].iter())
                .all(|byte| *byte == 0xa5));
        } else {
            assert_eq!(output, [0xa5; 160]);
        }
    }
    for path in [b"".as_slice(), full.as_slice()] {
        reset();
        assert_eq!(
            // SAFETY: Non-null empty/exact-limit paths are readable; output is live and disjoint.
            unsafe {
                routing::ql_preview_folder(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            PAYLOAD.len() as i32
        );
        folder_call(std::str::from_utf8(path).unwrap(), None);
    }
    for callback in [None, Some(cancelled as CancelCallback)] {
        reset();
        if callback.is_none() {
            FOLDER_MODE.set(FolderMode::Empty);
        }
        output.fill(0xa5);
        assert_eq!(
            // SAFETY: Source/output are live/disjoint; the static callback never unwinds.
            unsafe {
                routing::ql_preview_folder(
                    PATH.as_ptr(),
                    PATH.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    callback,
                )
            },
            0
        );
        assert_eq!(output, [0xa5; 160]);
        folder_call(PATH, callback);
        assert_eq!(POLLS.get(), usize::from(callback.is_some()));
        assert_eq!(WORK.get(), usize::from(callback.is_none()));
    }
}

pub fn routing_panics() {
    for archive in [false, true] {
        reset();
        PANIC.set(true);
        assert_eq!(
            // SAFETY: Static empty optional inputs remain valid while the provider panics inside the boundary.
            unsafe { classify(archive, ABSENT, ABSENT, ABSENT) },
            QL_ERROR_INTERNAL
        );
        observed(archive, "", "", &[]);
        reset();
        assert_eq!(
            // SAFETY: A fresh classification reuses only static empty inputs after panic containment.
            unsafe { classify(archive, ABSENT, ABSENT, ABSENT) },
            1
        );
        observed(archive, "", "", &[]);
    }
    reset();
    FOLDER_MODE.set(FolderMode::Panic);
    let mut output = [0xa5; 128];
    assert_eq!(
        // SAFETY: Input and initialized output remain live/disjoint; panic stays inside the real export.
        unsafe {
            routing::ql_preview_folder(
                PATH.as_ptr(),
                PATH.len(),
                output.as_mut_ptr(),
                output.len(),
                None,
            )
        },
        QL_ERROR_INTERNAL
    );
    assert_eq!(output, [0xa5; 128]);
    folder_call(PATH, None);
    reset();
    assert_eq!(
        // SAFETY: A fresh call reuses the still-live exclusive output after contained unwinding.
        unsafe {
            routing::ql_preview_folder(
                PATH.as_ptr(),
                PATH.len(),
                output.as_mut_ptr(),
                output.len(),
                None,
            )
        },
        PAYLOAD.len() as i32
    );
    assert_eq!(&output[..PAYLOAD.len()], PAYLOAD.as_bytes());
    folder_call(PATH, None);
}
