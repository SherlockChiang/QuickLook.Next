//! Executable contracts for the real path adapters with observable renderer substitutes.
//! These substitutes test dispatch/arguments/cancellation, not format parsing or file I/O.

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use crate::ffi::path_preview;
use crate::{CancelCallback, MAX_FFI_STRING_BYTES, QL_ERROR_INTERNAL};

#[derive(Debug, PartialEq)]
struct RenderCall {
    renderer: &'static str,
    path: String,
    kind: String,
    size: i64,
    modified: i64,
    has_callback: bool,
}

thread_local! {
    static CALLS: RefCell<Vec<RenderCall>> = const { RefCell::new(Vec::new()) };
    static WORK: Cell<usize> = const { Cell::new(0) };
    static CANCEL_POLLS: Cell<usize> = const { Cell::new(0) };
}

fn reset() {
    CALLS.with(|calls| calls.borrow_mut().clear());
    WORK.set(0);
    CANCEL_POLLS.set(0);
}

fn call_count() -> usize {
    CALLS.with(|calls| calls.borrow().len())
}

pub mod provider {
    use super::{RenderCall, CALLS, WORK};
    use crate::{cancel_requested, CancelCallback};

    fn render(
        renderer: &'static str,
        path: &str,
        kind: &str,
        size: i64,
        modified: i64,
        callback: Option<CancelCallback>,
    ) -> String {
        CALLS.with(|calls| {
            calls.borrow_mut().push(RenderCall {
                renderer,
                path: path.to_owned(),
                kind: kind.to_owned(),
                size,
                modified,
                has_callback: callback.is_some(),
            });
        });
        // Archive/Office cancellation is delegated to the provider rather than mapped to -3.
        if matches!(renderer, "archive" | "office") && cancel_requested(callback) {
            return String::new();
        }
        WORK.set(WORK.get() + 1);
        if path == "panic.file" {
            panic!("renderer panic probe");
        }
        if path == "empty.file" {
            return String::new();
        }
        format!(
            "{{\"renderer\":\"{renderer}\",\"path\":\"{path}\",\"kind\":\"{kind}\",\"size\":{size},\"modified\":{modified}}}"
        )
    }

    pub fn render_text(path: &str, cb: Option<CancelCallback>) -> String {
        render("text", path, "", 0, 0, cb)
    }

    pub fn render_executable(path: &str, cb: Option<CancelCallback>) -> String {
        render("executable", path, "", 0, 0, cb)
    }

    pub fn render_archive(path: &str, cb: Option<CancelCallback>) -> String {
        render("archive", path, "", 0, 0, cb)
    }

    pub fn render_ebook(path: &str, cb: Option<CancelCallback>) -> String {
        render("ebook", path, "", 0, 0, cb)
    }

    pub fn render_torrent(path: &str, cb: Option<CancelCallback>) -> String {
        render("torrent", path, "", 0, 0, cb)
    }

    pub fn render_office(path: &str, cb: Option<CancelCallback>) -> String {
        render("office", path, "", 0, 0, cb)
    }

    pub fn render_image_metadata(path: &str) -> String {
        render("image", path, "", 0, 0, None)
    }

    pub fn render_info(path: &str, kind: &str, size: i64, modified: i64) -> String {
        render("info", path, kind, size, modified, None)
    }
}

type Plain = unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> i32;
type Cancelable =
    unsafe extern "C" fn(*const u8, usize, *mut u8, usize, Option<CancelCallback>) -> i32;
type Info =
    unsafe extern "C" fn(*const u8, usize, *const u8, usize, i64, i64, *mut u8, usize) -> i32;

enum Entry {
    Plain(Plain),
    Cancelable(Cancelable),
    Info(Info),
}

struct Export {
    renderer: &'static str,
    entry: Entry,
}

impl Export {
    /// # Safety
    /// Non-null inputs and outputs must satisfy the real export's live/disjoint pointer contract.
    /// Null/oversized probes must only use paths documented to return before access.
    unsafe fn call(
        &self,
        path: *const u8,
        length: usize,
        output: *mut u8,
        capacity: usize,
        callback: Option<CancelCallback>,
    ) -> i32 {
        // SAFETY: The probe caller establishes storage/early-rejection contracts; the info
        // kind is static readable UTF-8, and no wrapper expands either caller buffer range.
        unsafe {
            match self.entry {
                Entry::Plain(call) => call(path, length, output, capacity),
                Entry::Cancelable(call) => call(path, length, output, capacity, callback),
                Entry::Info(call) => {
                    call(path, length, b"kind".as_ptr(), 4, -7, 9, output, capacity)
                }
            }
        }
    }

    fn expected(&self, path: &str) -> Vec<u8> {
        let (kind, size, modified) = if self.renderer == "info" {
            ("kind", -7, 9)
        } else {
            ("", 0, 0)
        };
        format!(
            "{{\"renderer\":\"{}\",\"path\":\"{path}\",\"kind\":\"{kind}\",\"size\":{size},\"modified\":{modified}}}",
            self.renderer
        )
        .into_bytes()
    }
}

fn exports() -> [Export; 12] {
    [
        Export {
            renderer: "text",
            entry: Entry::Plain(path_preview::ql_preview_text),
        },
        Export {
            renderer: "executable",
            entry: Entry::Plain(path_preview::ql_preview_executable),
        },
        Export {
            renderer: "ebook",
            entry: Entry::Plain(path_preview::ql_preview_ebook),
        },
        Export {
            renderer: "torrent",
            entry: Entry::Plain(path_preview::ql_preview_torrent),
        },
        Export {
            renderer: "image",
            entry: Entry::Plain(path_preview::ql_preview_image_metadata),
        },
        Export {
            renderer: "info",
            entry: Entry::Info(path_preview::ql_preview_info),
        },
        Export {
            renderer: "text",
            entry: Entry::Cancelable(path_preview::ql_preview_text_cancelable),
        },
        Export {
            renderer: "executable",
            entry: Entry::Cancelable(path_preview::ql_preview_executable_cancelable),
        },
        Export {
            renderer: "ebook",
            entry: Entry::Cancelable(path_preview::ql_preview_ebook_cancelable),
        },
        Export {
            renderer: "torrent",
            entry: Entry::Cancelable(path_preview::ql_preview_torrent_cancelable),
        },
        Export {
            renderer: "archive",
            entry: Entry::Cancelable(path_preview::ql_preview_archive),
        },
        Export {
            renderer: "office",
            entry: Entry::Cancelable(path_preview::ql_preview_office),
        },
    ]
}

pub fn path_arguments() {
    let path = b"valid.file";
    let invalid = [0xff];
    let oversized = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    for export in exports() {
        let mut output = [0xa5; 256];
        for (input, length, out, capacity) in [
            (path.as_ptr(), path.len(), output.as_mut_ptr(), 0),
            (path.as_ptr(), path.len(), null_mut(), 0),
            (null(), 0, output.as_mut_ptr(), output.len()),
            (null(), 1, output.as_mut_ptr(), output.len()),
            (
                invalid.as_ptr(),
                invalid.len(),
                output.as_mut_ptr(),
                output.len(),
            ),
            (
                oversized.as_ptr(),
                oversized.len(),
                output.as_mut_ptr(),
                output.len(),
            ),
        ] {
            reset();
            assert_eq!(
                // SAFETY: All non-null input ranges are actually readable (even over-budget),
                // outputs are disjoint/exclusive, and null output always has zero capacity.
                unsafe { export.call(input, length, out, capacity, None) },
                0
            );
            assert_eq!(call_count(), 0);
            assert_eq!(output, [0xa5; 256]);
        }
        reset();
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        assert_eq!(
            // SAFETY: The real path limit must reject this length before any dereference.
            // Path mutations never widen this bound or remove utf8_arg's pointer/length checks.
            unsafe {
                export.call(
                    dangling,
                    MAX_FFI_STRING_BYTES + 1,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            0
        );
        assert_eq!(call_count(), 0);

        // Accept the exact real limit as well as empty UTF-8; bounds cannot silently shrink.
        for length in [0, MAX_FFI_STRING_BYTES] {
            reset();
            // SAFETY: The entire declared path is live readable UTF-8; output is initialized/disjoint.
            let status =
                unsafe { export.call(oversized.as_ptr(), length, output.as_mut_ptr(), 1, None) };
            assert!(status < 0);
            assert_eq!(call_count(), 1);
            assert_eq!(output, [0xa5; 256]);
        }
    }
}

extern "C" fn never_cancel() -> bool {
    false
}

pub fn path_outputs() {
    let path = "預覽-é.file";
    for export in exports() {
        let expected = export.expected(path);
        for capacity in [1, expected.len() - 1, expected.len(), 256] {
            reset();
            let mut output = [0xa5; 272];
            // SAFETY: Path is live UTF-8; the initialized 268-byte output range is disjoint.
            // Short declared capacities still have spare initialized allocation space.
            let result = unsafe {
                export.call(
                    path.as_ptr(),
                    path.len(),
                    output[4..].as_mut_ptr(),
                    capacity,
                    Some(never_cancel),
                )
            };
            assert_eq!(call_count(), 1);
            CALLS.with(|calls| {
                let calls = calls.borrow();
                assert_eq!(calls[0].renderer, export.renderer);
                assert_eq!(calls[0].path, path);
                assert_eq!(
                    calls[0].has_callback,
                    matches!(export.entry, Entry::Cancelable(_))
                );
            });
            if capacity < expected.len() {
                assert_eq!(result, -(expected.len() as i32));
                assert_eq!(output, [0xa5; 272]);
            } else {
                assert_eq!(result, expected.len() as i32);
                assert_eq!(&output[4..4 + expected.len()], expected);
                assert!(output[..4]
                    .iter()
                    .chain(output[4 + expected.len()..].iter())
                    .all(|b| *b == 0xa5));
            }
        }
        reset();
        let mut output = [0xa5; 256];
        assert_eq!(
            // SAFETY: Static path/output storage is live and disjoint; empty result cannot write bytes.
            unsafe {
                export.call(
                    b"empty.file".as_ptr(),
                    10,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            0
        );
        assert_eq!(call_count(), 1);
        assert_eq!(output, [0xa5; 256]);
    }
}

extern "C" fn pre_cancel() -> bool {
    CANCEL_POLLS.set(CANCEL_POLLS.get() + 1);
    true
}

extern "C" fn after_render_cancel() -> bool {
    CANCEL_POLLS.set(CANCEL_POLLS.get() + 1);
    WORK.get() > 0
}

pub fn path_cancellation() {
    let path = b"valid.file";
    for export in exports()
        .into_iter()
        .filter(|export| matches!(export.entry, Entry::Cancelable(_)))
    {
        let delegated = matches!(export.renderer, "archive" | "office");
        reset();
        let mut output = [0xa5; 256];
        // SAFETY: Input/output are live/disjoint; static callbacks cannot unwind across the C ABI.
        let result = unsafe {
            export.call(
                path.as_ptr(),
                path.len(),
                output.as_mut_ptr(),
                output.len(),
                Some(pre_cancel),
            )
        };
        assert_eq!(result, if delegated { 0 } else { -3 });
        assert_eq!(call_count(), usize::from(delegated));
        assert_eq!(WORK.get(), 0);
        assert_eq!(CANCEL_POLLS.get(), 1);
        assert_eq!(output, [0xa5; 256]);
        if !delegated {
            reset();
            // SAFETY: All buffers remain live/disjoint; cancellation flips only after renderer work.
            let result = unsafe {
                export.call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(after_render_cancel),
                )
            };
            assert_eq!(result, -3);
            assert_eq!(call_count(), 1);
            assert_eq!(WORK.get(), 1);
            assert_eq!(CANCEL_POLLS.get(), 2);
            assert_eq!(output, [0xa5; 256]);
        }
        reset();
        assert!(
            // SAFETY: A fresh noncancelled call uses the same live/disjoint storage after cancellation.
            unsafe {
                export.call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            } > 0
        );
        assert_eq!(call_count(), 1);
    }
}

pub fn path_optional() {
    let path = b"valid.file";
    let valid = "kind-é".as_bytes();
    let invalid = [0xff];
    let oversized = vec![b'x'; MAX_FFI_STRING_BYTES + 1];
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    for (kind, length, expected) in [
        (null(), 0, ""),
        (null(), 1, ""),
        (valid.as_ptr(), 0, ""),
        (valid.as_ptr(), valid.len(), "kind-é"),
        (invalid.as_ptr(), 1, ""),
        (oversized.as_ptr(), oversized.len(), ""),
        (dangling, MAX_FFI_STRING_BYTES + 1, ""),
    ] {
        reset();
        let mut output = [0xa5; 256];
        // SAFETY: Live non-null ranges are readable UTF-8/invalid byte probes and disjoint from
        // exclusive output; oversized/null optional ranges must return before access.
        let result = unsafe {
            path_preview::ql_preview_info(
                path.as_ptr(),
                path.len(),
                kind,
                length,
                -7,
                9,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert!(result > 0);
        CALLS.with(|calls| {
            let calls = calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].kind, expected);
            assert_eq!((calls[0].size, calls[0].modified), (-7, 9));
        });
        assert!(output[result as usize..].iter().all(|b| *b == 0xa5));
    }
}

pub fn path_panics() {
    for export in exports() {
        reset();
        let mut output = [0xa5; 256];
        assert_eq!(
            // SAFETY: Buffers are live/disjoint. The Rust substitute panics internally; the actual
            // adapter's boundary must contain that unwind before crossing its extern C function.
            unsafe {
                export.call(
                    b"panic.file".as_ptr(),
                    10,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            QL_ERROR_INTERNAL
        );
        assert_eq!(call_count(), 1);
        assert_eq!(output, [0xa5; 256]);
        reset();
        assert!(
            // SAFETY: A fresh valid call verifies recovery using the same live/disjoint storage.
            unsafe {
                export.call(
                    b"valid.file".as_ptr(),
                    10,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            } > 0
        );
        assert_eq!(call_count(), 1);
    }
}
