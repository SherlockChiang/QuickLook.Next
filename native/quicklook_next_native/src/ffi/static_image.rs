//! Path-based static-image decode C ABI adapters.

mod handle;
mod packet;

use super::common::{ffi_boundary, utf8_arg};
use crate::{cancel_requested, decode_image_bgra, CancelCallback, MAX_FFI_STRING_BYTES};

pub(crate) use handle::{ql_decode_image_handle, ql_decode_image_with_waveform_handle};

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    // SAFETY: The unsafe export forwards its caller's live input and exclusive writable output.
    ffi_boundary(|| unsafe { ql_decode_image_cancelable(path_utf8, path_len, out, out_cap, None) })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The unsafe export forwards its caller's live input/output and valid callback.
    ffi_boundary(|| unsafe {
        ql_decode_image_sized_cancelable(path_utf8, path_len, 0, 0, out, out_cap, cancel_cb)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
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
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
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

        // SAFETY: The caller supplies exclusive output; total fits capacity, headers are stack
        // arrays and bgra is separately owned, so each source is disjoint from the destination.
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
            // SAFETY: Oversized path length is rejected before accessing dangling input or output.
            unsafe { ql_decode_image(dangling, path_len, std::ptr::null_mut(), 0) },
            -1
        );
        assert_eq!(
            // SAFETY: Oversized path length is rejected before accessing dangling input or output.
            unsafe {
                ql_decode_image_cancelable(dangling, path_len, std::ptr::null_mut(), 0, None)
            },
            -1
        );
        assert_eq!(
            // SAFETY: Oversized path length is rejected before accessing dangling input or output.
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
