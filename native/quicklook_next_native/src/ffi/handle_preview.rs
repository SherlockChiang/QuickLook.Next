//! Shared HANDLE preview boundary and foundational read-only adapters.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::common::{ffi_boundary, owned_utf8_arg, write_v2_out};
use crate::{
    cancel_requested, native_input, preview, CancelCallback, MAX_LOGICAL_NAME_BYTES,
    QL_ERROR_CANCELLED, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_ERROR_INVALID_HANDLE,
    QL_ERROR_IO, QL_ERROR_LENGTH_MISMATCH, QL_ERROR_LIMIT_EXCEEDED, QL_ERROR_MALFORMED,
};

/// Shared implementation for ABI 2 HANDLE preview entry points.
///
/// # Safety
/// The pointer and handle requirements are the same as the exported HANDLE entry points. The caller
/// must contain this function in `ffi_boundary`.
pub(crate) unsafe fn reopen_handle_input_v2(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    cancel_cb: Option<CancelCallback>,
) -> std::result::Result<(fs::File, String, i64, i64), i32> {
    if cancel_requested(cancel_cb) {
        return Err(QL_ERROR_CANCELLED);
    }
    // SAFETY: The caller keeps the logical-name buffer readable and immutable until copying ends.
    let logical_name =
        unsafe { owned_utf8_arg(logical_name_utf8, logical_name_len, MAX_LOGICAL_NAME_BYTES) }
            .ok_or(QL_ERROR_INVALID_ARGUMENT)?;
    let logical_name = Path::new(&logical_name)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .map(str::to_string)
        .ok_or(QL_ERROR_INVALID_ARGUMENT)?;

    let file = native_input::reopen_borrowed_disk_file(source_handle, expected_length).map_err(
        |error| match error {
            native_input::NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE,
            native_input::NativeInputError::Io => QL_ERROR_IO,
            native_input::NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        },
    )?;
    let size = i64::try_from(expected_length).map_err(|_| QL_ERROR_LENGTH_MISMATCH)?;
    let modified_unix = file
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);
    if cancel_requested(cancel_cb) {
        return Err(QL_ERROR_CANCELLED);
    }
    Ok((file, logical_name, size, modified_unix))
}

// Each HANDLE preview owns its independently reopened file. Moving that file into the one-shot
// renderer keeps path and HANDLE readers on the same `File` monomorphization without dynamic
// dispatch; the original caller-owned HANDLE remains untouched.
/// # Safety
/// The pointer, callback and borrowed-HANDLE requirements match `ql_preview_text_handle`.
/// The caller must contain this function in `ffi_boundary`.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn preview_handle_v2(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
    renderer: impl FnOnce(fs::File, &str, i64, i64) -> std::result::Result<String, i32>,
) -> i32 {
    if out_required.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    // SAFETY: The caller supplies an aligned writable scalar, checked non-null above.
    unsafe { *out_required = 0 };
    if out_buf.is_null() && out_cap != 0 {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    if cancel_requested(cancel_cb) {
        return QL_ERROR_CANCELLED;
    }
    // SAFETY: The caller lends live input/HANDLE for this call; reopening creates an owned file.
    let (file, logical_name, size, modified_unix) = match unsafe {
        reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        )
    } {
        Ok(input) => input,
        Err(status) => return status,
    };

    let rendered = renderer(file, &logical_name, size, modified_unix);
    if cancel_requested(cancel_cb) {
        return QL_ERROR_CANCELLED;
    }
    let json = match rendered {
        Ok(json) => json,
        Err(status) => return status,
    };
    if json.is_empty() {
        return QL_ERROR_INTERNAL;
    }
    // SAFETY: Output/scalar ranges are exclusive and disjoint; JSON is separately Rust-owned.
    unsafe { write_v2_out(json.as_bytes(), out_buf, out_cap, out_required) }
}

pub(crate) fn reader_preview_status(error: preview::ReaderPreviewError) -> i32 {
    match error {
        preview::ReaderPreviewError::Cancelled => QL_ERROR_CANCELLED,
        preview::ReaderPreviewError::Io => QL_ERROR_IO,
        preview::ReaderPreviewError::Malformed => QL_ERROR_MALFORMED,
        preview::ReaderPreviewError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        preview::ReaderPreviewError::LimitExceeded => QL_ERROR_LIMIT_EXCEEDED,
    }
}

/// Probe a borrowed Windows file handle without resolving the logical filename as a path.
///
/// # Safety
/// Pointer, output, handle ownership, and logical-name requirements match the ABI 2 HANDLE preview
/// entry points. The caller retains the source handle; Rust reopens it with an independent position.
#[no_mangle]
pub unsafe extern "C" fn ql_probe_file_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
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
            None,
            |mut file, logical_name, size, modified_unix| {
                preview::file_probe::probe_reader_json(
                    &mut file,
                    logical_name,
                    size as u64,
                    modified_unix,
                )
            },
        )
    })
}

/// Extract a bounded, page-mapped PDF text sidecar from a borrowed Windows file handle.
///
/// This is a lazy follow-up for search and clipboard commands. The caller retains the source
/// handle; Rust reopens it independently and never resolves the logical filename as authority.
///
/// # Safety
/// `source_handle` must be a valid borrowed disk-file HANDLE whose length is
/// `expected_length`; `logical_name_utf8` must be readable for `logical_name_len` bytes,
/// `out_required` must be writable, and a non-null `out_buf` must be writable for
/// `out_cap` bytes. The caller retains ownership of the source HANDLE.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_pdf_text_handle(
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
            |file, _, _, _| {
                preview::render_pdf_text_reader(file, expected_length, cancel_cb)
                    .map_err(reader_preview_status)
            },
        )
    })
}

/// Render bounded Rust-native image metadata from a borrowed Windows file handle.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle`. The caller retains the source handle; Rust reopens the disk file with
/// an independent position and treats `logical_name` only as a bounded filename hint.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_image_metadata_handle(
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
            |mut file, logical_name, _, _| {
                preview::render_image_metadata_reader(&mut file, logical_name, cancel_cb)
                    .map_err(reader_preview_status)
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{preview_handle_v2, ql_probe_file_handle};
    use crate::QL_ERROR_INVALID_ARGUMENT;

    #[test]
    fn handle_preview_rejects_null_required_before_handle_access() {
        // SAFETY: Null required is rejected before any input/output pointer or HANDLE access.
        let status = unsafe {
            ql_probe_file_handle(
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
    }

    #[test]
    fn handle_preview_rejects_null_output_with_capacity() {
        let mut required = usize::MAX;
        // SAFETY: required is aligned/writable; null output with capacity is rejected before I/O.
        let status = unsafe {
            preview_handle_v2(
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                1,
                &mut required,
                None,
                |_, _, _, _| unreachable!(),
            )
        };
        assert_eq!(status, QL_ERROR_INVALID_ARGUMENT);
        assert_eq!(required, 0);
    }
}
