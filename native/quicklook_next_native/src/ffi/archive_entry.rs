//! Archive entry extraction across path, HANDLE, and caller-owned output boundaries.

use std::io::{Seek, SeekFrom};

use super::common::{ffi_boundary, owned_utf8_arg, utf8_arg, write_json_out, write_v2_out};
use super::handle_preview::{reader_preview_status, reopen_handle_input_v2};
use crate::{
    cancel_requested, native_input, preview, CancelCallback, MAX_FFI_STRING_BYTES,
    QL_ERROR_CANCELLED, QL_ERROR_INVALID_ARGUMENT, QL_ERROR_INVALID_HANDLE, QL_ERROR_IO,
    QL_ERROR_LENGTH_MISMATCH, QL_OK,
};

const MAX_ARCHIVE_ENTRY_NAME_BYTES: usize = u16::MAX as usize;

/// Extract a previewable archive entry into a bounded temp cache. Returns UTF-8 path length, 0 on failure.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        if archive_path_utf8.is_null()
            || entry_path_utf8.is_null()
            || out_buf.is_null()
            || out_cap == 0
        {
            return 0;
        }
        let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES)
        {
            Some(path) => path,
            None => return 0,
        };
        let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
            Some(path) => path,
            None => return 0,
        };
        let Some(path) = preview::extract_archive_entry_to_temp(archive_path, entry_path, None)
        else {
            return 0;
        };
        write_json_out(&path, out_buf, out_cap)
    })
}

#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_cancelable(
    archive_path_utf8: *const u8,
    archive_path_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if archive_path_utf8.is_null()
            || entry_path_utf8.is_null()
            || out_buf.is_null()
            || out_cap == 0
        {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return -3;
        }
        let archive_path = match utf8_arg(archive_path_utf8, archive_path_len, MAX_FFI_STRING_BYTES)
        {
            Some(path) => path,
            None => return 0,
        };
        let entry_path = match utf8_arg(entry_path_utf8, entry_path_len, MAX_FFI_STRING_BYTES) {
            Some(path) => path,
            None => return 0,
        };
        let Some(path) =
            preview::extract_archive_entry_to_temp(archive_path, entry_path, cancel_cb)
        else {
            return if cancel_requested(cancel_cb) { -3 } else { 0 };
        };
        if cancel_requested(cancel_cb) {
            return -3;
        }
        write_json_out(&path, out_buf, out_cap)
    })
}

/// Extract a bounded ZIP entry from a borrowed Windows file handle into the private temp cache.
///
/// On success, output is the non-NUL-terminated UTF-8 temp path. A size probe or undersized buffer
/// returns `QL_ERROR_BUFFER_TOO_SMALL`, sets `out_required`, and removes the temporary extraction so
/// two-pass negotiation cannot leak temp roots.
///
/// # Safety
/// Both UTF-8 pointers must be readable for their declared lengths. `out_required` must be writable.
/// When non-null, `out_buf` must be writable for `out_cap` bytes. The source handle must remain open
/// and stable for the complete call; ownership remains with the caller.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
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
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let entry_path = match owned_utf8_arg(
            entry_path_utf8,
            entry_path_len,
            MAX_ARCHIVE_ENTRY_NAME_BYTES,
        ) {
            Some(entry_path) => entry_path,
            None => return QL_ERROR_INVALID_ARGUMENT,
        };
        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extracted = match preview::extract_archive_entry_to_temp_reader(
            &mut file,
            expected_length,
            &logical_name,
            &entry_path,
            cancel_cb,
        ) {
            Ok(path) => path,
            Err(error) => return reader_preview_status(error),
        };
        if cancel_requested(cancel_cb) {
            preview::discard_archive_extract_path(&extracted);
            return QL_ERROR_CANCELLED;
        }
        let status = write_v2_out(extracted.as_bytes(), out_buf, out_cap, out_required);
        if status != QL_OK || cancel_requested(cancel_cb) {
            preview::discard_archive_extract_path(&extracted);
            return if cancel_requested(cancel_cb) {
                QL_ERROR_CANCELLED
            } else {
                status
            };
        }
        QL_OK
    })
}

/// Stream a bounded ZIP entry into a caller-owned output disk-file HANDLE.
///
/// The output object must be a newly created zero-length file whose handle permits write sharing.
/// Rust validates and reopens both handles with independent file positions, writes at most
/// `output_capacity` bytes, and reports the exact resulting length through `out_written`.
///
/// # Safety
/// Both UTF-8 pointers must be readable for their declared lengths and `out_written` must be
/// writable. Both raw handles must remain valid for the complete call. Ownership stays with the
/// caller; Rust closes only its independently reopened handles.
#[no_mangle]
pub unsafe extern "C" fn ql_extract_archive_entry_to_output_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    entry_path_utf8: *const u8,
    entry_path_len: usize,
    output_handle: isize,
    output_capacity: u64,
    out_written: *mut u64,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| unsafe {
        if out_written.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_written = 0;
        if source_handle == output_handle
            || output_capacity == 0
            || output_capacity > preview::MAX_ARCHIVE_EXTRACT_BYTES
        {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let entry_path = match owned_utf8_arg(
            entry_path_utf8,
            entry_path_len,
            MAX_ARCHIVE_ENTRY_NAME_BYTES,
        ) {
            Some(entry_path) => entry_path,
            None => return QL_ERROR_INVALID_ARGUMENT,
        };
        let (mut source, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let mut output = match native_input::reopen_borrowed_disk_file_for_output(output_handle, 0)
        {
            Ok(output) => output,
            Err(native_input::NativeInputError::InvalidHandle) => {
                return QL_ERROR_INVALID_HANDLE;
            }
            Err(native_input::NativeInputError::LengthMismatch) => {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            Err(native_input::NativeInputError::Io) => return QL_ERROR_IO,
        };
        if output.seek(SeekFrom::Start(0)).is_err() || output.set_len(0).is_err() {
            return QL_ERROR_IO;
        }

        let written = match preview::extract_archive_entry_to_writer_reader(
            &mut source,
            expected_length,
            &logical_name,
            &entry_path,
            &mut output,
            output_capacity,
            cancel_cb,
        ) {
            Ok(written) => written,
            Err(error) => {
                let _ = output.set_len(0);
                return reader_preview_status(error);
            }
        };
        if cancel_requested(cancel_cb) {
            let _ = output.set_len(0);
            return QL_ERROR_CANCELLED;
        }
        if !matches!(output.metadata(), Ok(metadata) if metadata.len() == written) {
            let _ = output.set_len(0);
            return QL_ERROR_IO;
        }
        *out_written = written;
        QL_OK
    })
}

#[cfg(test)]
mod tests;
