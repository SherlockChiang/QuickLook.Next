//! Shell thumbnail C ABI adapters over the bounded Win32 STA worker.

use super::common::{ffi_boundary, utf8_arg};
use super::raster_packet::write_raster_packet;
use crate::{
    win32, CancelCallback, MAX_FFI_STRING_BYTES, QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_CANCELLED,
    QL_ERROR_INVALID_ARGUMENT, QL_ERROR_LIMIT_EXCEEDED,
};

/// Get a shell thumbnail for `path` at roughly `size` px. Returns total bytes written, or `-needed`.
///
/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_get_thumbnail_cancelable_with_flags(path_utf8, path_len, size, 0, out, out_cap, None)
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_get_thumbnail_cancelable_with_flags(
            path_utf8, path_len, size, 0, out, out_cap, cancel_cb,
        )
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_get_thumbnail_cancelable_with_flags(
    path_utf8: *const u8,
    path_len: usize,
    size: i32,
    flags: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(path) => path.to_string(),
            None => return -1,
        };

        let (width, height, bgra) =
            match win32::shell_thumbnail::request(path, size, flags, cancel_cb) {
                Ok(thumbnail) => thumbnail,
                Err(win32::shell_thumbnail::ThumbnailError::InvalidFlags) => {
                    return QL_ERROR_INVALID_ARGUMENT
                }
                Err(win32::shell_thumbnail::ThumbnailError::LimitExceeded) => {
                    return QL_ERROR_LIMIT_EXCEEDED
                }
                Err(win32::shell_thumbnail::ThumbnailError::Cancelled) => {
                    return QL_ERROR_CANCELLED
                }
                Err(win32::shell_thumbnail::ThumbnailError::Unavailable) => {
                    return QL_ERROR_BUFFER_TOO_SMALL
                }
            };
        write_raster_packet(width, height, &bgra, out, out_cap)
    })
}

#[cfg(test)]
mod tests;
