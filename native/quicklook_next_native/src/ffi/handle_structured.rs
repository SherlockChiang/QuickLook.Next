//! Structured text and metadata previews over borrowed Windows file handles.

use super::common::ffi_boundary;
use super::handle_preview::{preview_handle_v2, reader_preview_status};
use crate::{cancel_requested, preview, CancelCallback, QL_ERROR_CANCELLED, QL_ERROR_IO};

/// Render text from a borrowed Windows file handle.
///
/// # Safety
/// `logical_name_utf8` must be readable for `logical_name_len` bytes. `out_required` must point to
/// writable `usize` storage. When `out_buf` is non-null it must be writable for `out_cap` bytes and
/// must not alias Rust-owned output storage. A non-invalid source handle must remain open and stable
/// for the complete call. The caller retains ownership; Rust reopens the file with an independent
/// position and never resolves `logical_name` as a path.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_text_handle(
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
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, _, _| {
                let json = preview::render_text_reader(&mut file, logical_name, cancel_cb);
                if json.is_empty() {
                    Err(if cancel_requested(cancel_cb) {
                        QL_ERROR_CANCELLED
                    } else {
                        QL_ERROR_IO
                    })
                } else {
                    Ok(json)
                }
            },
        )
    })
}

/// Render executable metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_executable_handle(
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
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, size, modified_unix| {
                preview::render_executable_reader(
                    &mut file,
                    logical_name,
                    size,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render torrent metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_torrent_handle(
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
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut file, logical_name, size, modified_unix| {
                preview::render_torrent_reader(
                    &mut file,
                    logical_name,
                    size,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

/// Render bounded RFC 5322 or Outlook MSG metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_mail_handle(
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
        preview_handle_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |file, logical_name, _, modified_unix| {
                preview::render_mail_reader(
                    file,
                    logical_name,
                    expected_length,
                    modified_unix,
                    cancel_cb,
                )
                .map_err(reader_preview_status)
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ql_preview_executable_handle, ql_preview_mail_handle, ql_preview_text_handle,
        ql_preview_torrent_handle,
    };
    use crate::{CancelCallback, QL_ERROR_CANCELLED};

    type HandlePreview = unsafe extern "C" fn(
        isize,
        u64,
        *const u8,
        usize,
        *mut u8,
        usize,
        *mut usize,
        Option<CancelCallback>,
    ) -> i32;

    extern "C" fn cancelled() -> bool {
        true
    }

    #[test]
    fn structured_handle_exports_cancel_before_handle_access() {
        let exports: [HandlePreview; 4] = [
            ql_preview_text_handle,
            ql_preview_executable_handle,
            ql_preview_torrent_handle,
            ql_preview_mail_handle,
        ];
        for export in exports {
            let mut required = usize::MAX;
            let status = unsafe {
                export(
                    0,
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut required,
                    Some(cancelled),
                )
            };
            assert_eq!(status, QL_ERROR_CANCELLED);
            assert_eq!(required, 0);
        }
    }
}
