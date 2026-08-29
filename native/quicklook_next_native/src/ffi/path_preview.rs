//! Thin C ABI adapters for path-based text and metadata previews.

use super::common::{ffi_boundary, optional_utf8_arg, utf8_arg, write_json_out};
use crate::{cancel_requested, preview, CancelCallback, MAX_FFI_STRING_BYTES};

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_text_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_text(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_info(
    path_utf8: *const u8,
    path_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    size: i64,
    modified_unix: i64,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let kind = optional_utf8_arg(kind_utf8, kind_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let json = preview::render_info(path, kind, size, modified_unix);
        write_json_out(&json, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_executable_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_executable(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_archive(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_archive(path, cancel_cb);
        write_json_out(&json, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_ebook_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_ebook(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| ql_preview_torrent_cancelable(path_utf8, path_len, out_buf, out_cap, None))
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_torrent(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&json, out_buf, out_cap)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ql_preview_archive, ql_preview_ebook_cancelable, ql_preview_executable_cancelable,
        ql_preview_info, ql_preview_text_cancelable, ql_preview_torrent_cancelable,
    };
    use crate::{CancelCallback, MAX_FFI_STRING_BYTES};

    extern "C" fn always_cancel() -> bool {
        true
    }

    #[test]
    fn simple_preview_exports_honor_cancellation_before_file_access() {
        let path = b"missing.file";
        let mut output = [0u8; 16];
        let calls: [unsafe extern "C" fn(_, _, _, _, Option<CancelCallback>) -> i32; 4] = [
            ql_preview_text_cancelable,
            ql_preview_ebook_cancelable,
            ql_preview_executable_cancelable,
            ql_preview_torrent_cancelable,
        ];

        for call in calls {
            let result = unsafe {
                call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(always_cancel),
                )
            };
            assert_eq!(result, -3);
        }
        let archive_result = unsafe {
            ql_preview_archive(
                path.as_ptr(),
                path.len(),
                output.as_mut_ptr(),
                output.len(),
                Some(always_cancel),
            )
        };
        assert_eq!(archive_result, 0);
    }

    #[test]
    fn path_preview_exports_reject_invalid_pointer_contracts() {
        let path = b"missing.file";
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        let mut output = [0u8; 16];

        assert_eq!(
            unsafe {
                ql_preview_text_cancelable(path.as_ptr(), path.len(), std::ptr::null_mut(), 0, None)
            },
            0
        );
        assert_eq!(
            unsafe {
                ql_preview_text_cancelable(
                    dangling,
                    MAX_FFI_STRING_BYTES + 1,
                    output.as_mut_ptr(),
                    output.len(),
                    None,
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                ql_preview_info(
                    dangling,
                    MAX_FFI_STRING_BYTES + 1,
                    dangling,
                    MAX_FFI_STRING_BYTES + 1,
                    0,
                    0,
                    output.as_mut_ptr(),
                    output.len(),
                )
            },
            0
        );
    }

    #[test]
    fn info_preview_treats_invalid_optional_kind_as_empty() {
        let path = b"missing.file";
        let mut output = [0u8; 512];
        let written = unsafe {
            ql_preview_info(
                path.as_ptr(),
                path.len(),
                std::ptr::null(),
                1,
                0,
                0,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert!(written > 0);
        assert!(std::str::from_utf8(&output[..written as usize])
            .expect("info preview output must be UTF-8")
            .contains("\"kind\":\"\""));
    }
}
