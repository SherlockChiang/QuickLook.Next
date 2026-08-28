//! Thin C ABI adapters for preview routing and format detection.

use super::common::{
    ffi_boundary, optional_bytes_arg, optional_utf8_arg, utf8_arg, write_json_out,
};
use crate::{preview, CancelCallback, MAX_FFI_MAGIC_BYTES, MAX_FFI_STRING_BYTES};

/// Render a folder listing. Returns JSON length, 0 on failure.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_folder(
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
        let json = preview::render_folder(path, cancel_cb);
        write_json_out(&json, out_buf, out_cap)
    })
}

/// Check if a file is text-like (for routing in the App).
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_is_text(
    ext_utf8: *const u8,
    ext_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let ext = optional_utf8_arg(ext_utf8, ext_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let magic = match optional_bytes_arg(magic, magic_len, MAX_FFI_MAGIC_BYTES) {
            Some(bytes) => bytes,
            None => return 0,
        };
        if preview::is_text(ext, magic) {
            1
        } else {
            0
        }
    })
}

/// Check if a file is an archive (for routing).
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_is_archive(
    ext_utf8: *const u8,
    ext_len: usize,
    kind_utf8: *const u8,
    kind_len: usize,
    magic: *const u8,
    magic_len: usize,
) -> i32 {
    ffi_boundary(|| {
        let ext = optional_utf8_arg(ext_utf8, ext_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let kind = optional_utf8_arg(kind_utf8, kind_len, MAX_FFI_STRING_BYTES).unwrap_or("");
        let magic = match optional_bytes_arg(magic, magic_len, MAX_FFI_MAGIC_BYTES) {
            Some(bytes) => bytes,
            None => return 0,
        };
        if preview::is_archive(ext, kind, magic) {
            1
        } else {
            0
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{ql_is_text, ql_preview_folder};
    use crate::ffi::common::{optional_bytes_arg, optional_utf8_arg};
    use crate::{MAX_FFI_MAGIC_BYTES, MAX_FFI_STRING_BYTES};

    #[test]
    fn optional_arguments_accept_null_only_with_zero_length() {
        assert_eq!(optional_utf8_arg(std::ptr::null(), 0, 8), Some(""));
        assert_eq!(optional_bytes_arg(std::ptr::null(), 0, 8), Some(&[][..]));
        assert!(optional_utf8_arg(std::ptr::null(), 1, 8).is_none());
        assert!(optional_bytes_arg(std::ptr::null(), 1, 8).is_none());
    }

    #[test]
    fn oversized_arguments_are_rejected_before_dereference() {
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        assert!(
            optional_utf8_arg(dangling, MAX_FFI_STRING_BYTES + 1, MAX_FFI_STRING_BYTES).is_none()
        );
        assert!(
            optional_bytes_arg(dangling, MAX_FFI_MAGIC_BYTES + 1, MAX_FFI_MAGIC_BYTES).is_none()
        );
        assert_eq!(
            unsafe { ql_is_text(dangling, MAX_FFI_STRING_BYTES + 1, std::ptr::null(), 0) },
            0
        );
    }

    #[test]
    fn folder_route_rejects_null_output_without_dereference() {
        let path = b".";
        let status =
            unsafe { ql_preview_folder(path.as_ptr(), path.len(), std::ptr::null_mut(), 0, None) };
        assert_eq!(status, 0);
    }
}
