//! Office and package Hero image extraction across path and HANDLE compatibility boundaries.

use super::common::{ffi_boundary, owned_utf8_arg, utf8_arg};
use super::handle_preview::{reader_preview_status, reopen_handle_input_v2};
use super::raster_packet::{write_raster_packet, write_raster_packet_v2};
use crate::{
    cancel_requested, preview, CancelCallback, MAX_FFI_STRING_BYTES, MAX_OFFICE_IMAGE_REF_BYTES,
    QL_ERROR_CANCELLED, QL_ERROR_INVALID_ARGUMENT,
};

/// Extract the most likely app/package icon from ZIP-based packages (MSIX/AppX/APK/APKS/AAB).
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };

        let (w, h, bgra) = match preview::extract_package_icon_bgra(path, None) {
            Some(x) => x,
            None => return -2,
        };
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let (w, h, bgra) = match preview::extract_package_icon_bgra(path, cancel_cb) {
            Some(value) => value,
            None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

/// Extract the first useful embedded image from an OOXML Office document.
/// Output layout: [w:u32 LE][h:u32 LE][premultiplied BGRA bytes].
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };

        let (w, h, bgra) = match preview::extract_office_image_bgra(path, None) {
            Some(x) => x,
            None => return -2,
        };
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let (w, h, bgra) = match preview::extract_office_image_bgra(path, cancel_cb) {
            Some(value) => value,
            None => return if cancel_requested(cancel_cb) { -3 } else { -2 },
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_raster_packet(w, h, &bgra, out, out_cap)
    })
}

/// Extract a useful embedded Office image from a borrowed Windows file handle.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let (width, height, bgra) = match preview::extract_office_image_bgra_reader(
            file,
            expected_length,
            &logical_name,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

/// Extract one Office layout image by its canonical ZIP media reference.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The caller retains ownership of `source_handle` and must keep every pointer valid for the
/// complete call. Rust reopens the source HANDLE with an independent file position. `image_ref`
/// must be a canonical, relative ZIP path under the media root matching `logical_name`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_office_layout_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    image_ref_utf8: *const u8,
    image_ref_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if target_width == 0
            || target_height == 0
            || target_width > preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
            || target_height > preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION
        {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let image_ref =
            match owned_utf8_arg(image_ref_utf8, image_ref_len, MAX_OFFICE_IMAGE_REF_BYTES) {
                Some(image_ref) if !image_ref.is_empty() => image_ref,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
        if !preview::office_layout_image_ref_is_valid(&logical_name, &image_ref) {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (width, height, bgra) = match preview::extract_office_layout_image_bgra_reader(
            file,
            expected_length,
            &logical_name,
            &image_ref,
            target_width,
            target_height,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

/// Extract a package icon from a borrowed Windows file handle.
/// Output layout is `[width:u32 LE][height:u32 LE][premultiplied BGRA bytes]`.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_package_icon_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        let (file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let (width, height, bgra) = match preview::extract_package_icon_bgra_reader(
            file,
            expected_length,
            &logical_name,
            cancel_cb,
        ) {
            Ok(image) => image,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        write_raster_packet_v2(width, height, &bgra, out_buf, out_cap, out_required)
    })
}

#[cfg(test)]
mod tests;
