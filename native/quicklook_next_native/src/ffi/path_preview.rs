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
    // SAFETY: The export caller guarantees valid input/output buffers for this call.
    ffi_boundary(|| unsafe {
        ql_preview_text_cancelable(path_utf8, path_len, out_buf, out_cap, None)
    })
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_text(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let kind =
            unsafe { optional_utf8_arg(kind_utf8, kind_len, MAX_FFI_STRING_BYTES) }.unwrap_or("");
        let json = preview::render_info(path, kind, size, modified_unix);
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
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
    // SAFETY: The export caller guarantees valid input/output buffers for this call.
    ffi_boundary(|| unsafe {
        ql_preview_executable_cancelable(path_utf8, path_len, out_buf, out_cap, None)
    })
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_executable(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_archive(path, cancel_cb);
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
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
    // SAFETY: The export caller guarantees valid input/output buffers for this call.
    ffi_boundary(|| unsafe {
        ql_preview_ebook_cancelable(path_utf8, path_len, out_buf, out_cap, None)
    })
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_ebook(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
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
    // SAFETY: The export caller guarantees valid input/output buffers for this call.
    ffi_boundary(|| unsafe {
        ql_preview_torrent_cancelable(path_utf8, path_len, out_buf, out_cap, None)
    })
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_torrent(path, cancel_cb);
        if cancel_requested(cancel_cb) {
            return -3;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
    })
}

/// Render an Office document preview. OOXML/ODF paths are parsed in Rust; legacy OLE formats fall back to info.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_office(
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_office(path, cancel_cb);
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
    })
}

/// Render bounded Rust-native image metadata. Returns JSON length, 0 on failure/no metadata.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_image_metadata(
    path_utf8: *const u8,
    path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_image_metadata(path);
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
    })
}

#[cfg(test)]
mod tests;
