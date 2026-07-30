use std::fs;
use std::os::windows::io::{FromRawHandle, RawHandle};

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    GetFileSizeEx, GetFileType, ReOpenFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeInputError {
    InvalidHandle,
    Io,
    LengthMismatch,
}

/// Validate a caller-owned disk-file handle and reopen it for independent sequential reads.
///
/// The raw value is never wrapped in `BorrowedHandle`: Win32 validates it before any Rust type is
/// constructed with a valid-handle precondition. `ReOpenFile` returns a separately owned handle
/// with its own file position, so seeking or reading cannot move the caller's position.
pub fn reopen_borrowed_disk_file(
    raw_handle: isize,
    expected_length: u64,
) -> Result<fs::File, NativeInputError> {
    if raw_handle == 0 || raw_handle == -1 {
        return Err(NativeInputError::InvalidHandle);
    }

    let source = HANDLE(raw_handle as RawHandle);
    let mut source_length = 0i64;
    if unsafe { GetFileType(source) } != FILE_TYPE_DISK
        || unsafe { GetFileSizeEx(source, &mut source_length) }.is_err()
        || source_length < 0
    {
        return Err(NativeInputError::InvalidHandle);
    }
    let reopened = unsafe {
        ReOpenFile(
            source,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            FILE_FLAGS_AND_ATTRIBUTES(0),
        )
    }
    .map_err(|_| NativeInputError::Io)?;

    // SAFETY: `ReOpenFile` returned a new owning handle. Moving it into `File` gives Rust the sole
    // responsibility for closing that handle and does not transfer ownership of `source`.
    let file = unsafe { fs::File::from_raw_handle(reopened.0 as RawHandle) };
    let metadata = file.metadata().map_err(|_| NativeInputError::Io)?;
    if !metadata.is_file() {
        return Err(NativeInputError::InvalidHandle);
    }
    if source_length as u64 != expected_length || metadata.len() != expected_length {
        return Err(NativeInputError::LengthMismatch);
    }
    Ok(file)
}

/// Validate a caller-owned writable disk-file handle and reopen it with an independent position.
///
/// The caller creates a new, zero-length output object and retains ownership. The reopened handle
/// inherits no path authority and is closed by Rust after the bounded write finishes. The original
/// handle must allow write sharing for the duration of this call.
pub fn reopen_borrowed_disk_file_for_output(
    raw_handle: isize,
    expected_initial_length: u64,
) -> Result<fs::File, NativeInputError> {
    if raw_handle == 0 || raw_handle == -1 {
        return Err(NativeInputError::InvalidHandle);
    }

    let source = HANDLE(raw_handle as RawHandle);
    let mut source_length = 0i64;
    if unsafe { GetFileType(source) } != FILE_TYPE_DISK
        || unsafe { GetFileSizeEx(source, &mut source_length) }.is_err()
        || source_length < 0
    {
        return Err(NativeInputError::InvalidHandle);
    }
    if source_length as u64 != expected_initial_length {
        return Err(NativeInputError::LengthMismatch);
    }

    let reopened = unsafe {
        ReOpenFile(
            source,
            GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAGS_AND_ATTRIBUTES(0),
        )
    }
    .map_err(|_| NativeInputError::Io)?;

    // SAFETY: `ReOpenFile` returned a new owning handle. The caller-owned source remains open and
    // Rust closes only this independently positioned writable handle.
    let file = unsafe { fs::File::from_raw_handle(reopened.0 as RawHandle) };
    let metadata = file.metadata().map_err(|_| NativeInputError::Io)?;
    if !metadata.is_file() {
        return Err(NativeInputError::InvalidHandle);
    }
    if metadata.len() != expected_initial_length {
        return Err(NativeInputError::LengthMismatch);
    }
    Ok(file)
}
