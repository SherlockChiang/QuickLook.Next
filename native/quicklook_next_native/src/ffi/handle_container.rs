//! Container-backed document previews over borrowed Windows file handles.

use super::common::ffi_boundary;
use super::handle_preview::{preview_handle_v2, reader_preview_status};
use crate::{preview, CancelCallback};

/// Render an archive listing from a borrowed Windows file handle.
///
/// Package formats intentionally remain on the legacy path pipeline. The returned archive listing
/// has an empty root path so callers cannot accidentally resolve the logical filename as a path.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_archive_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The export forwards its caller's live HANDLE/input and disjoint writable outputs.
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
                preview::render_archive_reader(
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

/// Render package metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_package_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The export forwards its caller's live HANDLE/input and disjoint writable outputs.
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
            |file, logical_name, _, _| {
                preview::render_package_reader(file, logical_name, expected_length, cancel_cb)
                    .map_err(reader_preview_status)
            },
        )
    })
}

/// Render an Office preview from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_office_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The export forwards its caller's live HANDLE/input and disjoint writable outputs.
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
                preview::render_office_reader(
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

/// Render an ebook from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_ebook_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The export forwards its caller's live HANDLE/input and disjoint writable outputs.
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
                preview::render_ebook_reader(
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
        ql_preview_archive_handle, ql_preview_ebook_handle, ql_preview_office_handle,
        ql_preview_package_handle,
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
    fn container_handle_exports_cancel_before_handle_access() {
        let exports: [HandlePreview; 4] = [
            ql_preview_archive_handle,
            ql_preview_package_handle,
            ql_preview_office_handle,
            ql_preview_ebook_handle,
        ];
        for export in exports {
            let mut required = usize::MAX;
            // SAFETY: required is aligned/writable; cancellation rejects null inputs before I/O.
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
