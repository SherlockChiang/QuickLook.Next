//! Path-based static-image decode C ABI adapters.

use super::common::{ffi_boundary, utf8_arg};
use crate::{cancel_requested, decode_image_bgra, CancelCallback, MAX_FFI_STRING_BYTES};

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| unsafe { ql_decode_image_cancelable(path_utf8, path_len, out, out_cap, None) })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        ql_decode_image_sized_cancelable(path_utf8, path_len, 0, 0, out, out_cap, cancel_cb)
    })
}

/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers and `cancel_cb` must remain valid for the duration of the
/// call.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_sized_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    target_width: u32,
    target_height: u32,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(path) => path,
            None => return -1,
        };

        let (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ) = match decode_image_bgra(path, target_width, target_height, cancel_cb) {
            Some(decoded) => decoded,
            None => return -2,
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }

        let total = 28 + bgra.len();
        if out.is_null() || out_cap < total {
            return -(total as i32);
        }

        unsafe {
            std::ptr::copy_nonoverlapping(width.to_le_bytes().as_ptr(), out, 4);
            std::ptr::copy_nonoverlapping(height.to_le_bytes().as_ptr(), out.add(4), 4);
            std::ptr::copy_nonoverlapping(original_width.to_le_bytes().as_ptr(), out.add(8), 4);
            std::ptr::copy_nonoverlapping(original_height.to_le_bytes().as_ptr(), out.add(12), 4);
            std::ptr::copy_nonoverlapping(decode_ms.to_le_bytes().as_ptr(), out.add(16), 4);
            std::ptr::copy_nonoverlapping(resize_ms.to_le_bytes().as_ptr(), out.add(20), 4);
            std::ptr::copy_nonoverlapping(convert_ms.to_le_bytes().as_ptr(), out.add(24), 4);
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), out.add(28), bgra.len());
        }
        total as i32
    })
}

#[cfg(test)]
mod tests {
    use super::{ql_decode_image, ql_decode_image_cancelable, ql_decode_image_sized_cancelable};
    use crate::MAX_FFI_STRING_BYTES;

    #[test]
    fn path_image_exports_reject_oversized_paths_before_dereference() {
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        let path_len = MAX_FFI_STRING_BYTES + 1;

        assert_eq!(
            unsafe { ql_decode_image(dangling, path_len, std::ptr::null_mut(), 0) },
            -1
        );
        assert_eq!(
            unsafe {
                ql_decode_image_cancelable(dangling, path_len, std::ptr::null_mut(), 0, None)
            },
            -1
        );
        assert_eq!(
            unsafe {
                ql_decode_image_sized_cancelable(
                    dangling,
                    path_len,
                    0,
                    0,
                    std::ptr::null_mut(),
                    0,
                    None,
                )
            },
            -1
        );
    }
}
