//! Database previews over legacy paths and borrowed Windows file handles.

use std::{fs, io::Read};

use super::common::{ffi_boundary, utf8_arg, write_json_out};
use super::handle_preview::{preview_handle_v2, reader_preview_status};
use crate::{
    cancel_requested, native_input, preview, CancelCallback, MAX_FFI_STRING_BYTES,
    QL_ERROR_INVALID_ARGUMENT, QL_ERROR_INVALID_HANDLE, QL_ERROR_IO, QL_ERROR_LENGTH_MISMATCH,
    QL_ERROR_LIMIT_EXCEEDED,
};

/// Render a bounded database metadata preview with cancellation support.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_preview_database_cancelable(
    path_utf8: *const u8,
    path_len: usize,
    size: i64,
    modified_unix: i64,
    out_buf: *mut u8,
    out_cap: usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    ffi_boundary(|| {
        if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {
            return 0;
        }
        if cancel_requested(cancel_cb) {
            return 0;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
            Some(s) => s,
            None => return 0,
        };
        let json = preview::render_database_info(path, size, modified_unix, cancel_cb);
        if cancel_requested(cancel_cb) || json.is_empty() {
            return 0;
        }
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        unsafe { write_json_out(&json, out_buf, out_cap) }
    })
}

fn reopen_optional_handle(
    source_handle: isize,
    expected_length: u64,
) -> std::result::Result<Option<fs::File>, i32> {
    if source_handle == 0 {
        return if expected_length == 0 {
            Ok(None)
        } else {
            Err(QL_ERROR_INVALID_ARGUMENT)
        };
    }
    native_input::reopen_borrowed_disk_file(source_handle, expected_length)
        .map(Some)
        .map_err(|error| match error {
            native_input::NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE,
            native_input::NativeInputError::Io => QL_ERROR_IO,
            native_input::NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH,
        })
}

/// Render a bounded SQLite snapshot from borrowed main, WAL, and SHM Windows file handles.
///
/// `wal_handle` and `shm_handle` are independently optional. Absence is represented only by a
/// `(0, 0)` handle/length pair; a nonzero handle with a zero length represents a present empty
/// companion. The WAL is the only source used to update the visible database snapshot. SHM data is
/// bounded diagnostic input and is never trusted for database correctness.
///
/// # Safety
/// The pointer, buffer, lifetime, and ownership requirements are identical to
/// `ql_preview_text_handle` and apply to every nonzero handle. The caller retains ownership of all
/// handles. Rust reopens each one with an independent position and never resolves `logical_name` as
/// a path.
#[no_mangle]
pub unsafe extern "C" fn ql_preview_sqlite_handles(
    main_handle: isize,
    main_expected_length: u64,
    wal_handle: isize,
    wal_expected_length: u64,
    shm_handle: isize,
    shm_expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The caller lends live file/companion HANDLEs and input, plus disjoint exclusive
    // output/scalar storage. Readers own independently reopened files for this synchronous call.
    ffi_boundary(|| unsafe {
        preview_handle_v2(
            main_handle,
            main_expected_length,
            logical_name_utf8,
            logical_name_len,
            out_buf,
            out_cap,
            out_required,
            cancel_cb,
            |mut main, logical_name, _, modified_unix| {
                if wal_handle == 0 && wal_expected_length != 0
                    || shm_handle == 0 && shm_expected_length != 0
                {
                    return Err(QL_ERROR_INVALID_ARGUMENT);
                }
                if main_expected_length > preview::MAX_DATABASE_HANDLE_BYTES
                    || wal_expected_length > preview::MAX_SQLITE_WAL_BYTES
                    || shm_expected_length > preview::MAX_SQLITE_SHM_BYTES
                {
                    return Err(QL_ERROR_LIMIT_EXCEEDED);
                }
                let mut wal = reopen_optional_handle(wal_handle, wal_expected_length)?;
                let mut shm = reopen_optional_handle(shm_handle, shm_expected_length)?;
                let wal_reader = wal.as_mut().map(|file| file as &mut dyn Read);
                let shm_reader = shm.as_mut().map(|file| file as &mut dyn Read);
                preview::render_database_reader(
                    &mut main,
                    main_expected_length,
                    preview::DatabaseCompanionReader {
                        reader: wal_reader,
                        length: wal_expected_length,
                    },
                    preview::DatabaseCompanionReader {
                        reader: shm_reader,
                        length: shm_expected_length,
                    },
                    logical_name,
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
    use super::{ql_preview_database_cancelable, ql_preview_sqlite_handles};
    use crate::QL_ERROR_CANCELLED;

    extern "C" fn cancelled() -> bool {
        true
    }

    #[test]
    fn database_exports_cancel_before_file_access() {
        let path = b"Z:\\missing\\cancelled.db";
        let mut path_output = [0u8; 16];
        // SAFETY: Live path/output arrays are disjoint; cancellation returns before path access.
        let path_status = unsafe {
            ql_preview_database_cancelable(
                path.as_ptr(),
                path.len(),
                0,
                0,
                path_output.as_mut_ptr(),
                path_output.len(),
                Some(cancelled),
            )
        };
        assert_eq!(path_status, 0);

        let mut required = usize::MAX;
        // SAFETY: required is aligned and writable; cancellation rejects null inputs before access.
        let handle_status = unsafe {
            ql_preview_sqlite_handles(
                0,
                0,
                0,
                0,
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
        assert_eq!(handle_status, QL_ERROR_CANCELLED);
        assert_eq!(required, 0);
    }
}
