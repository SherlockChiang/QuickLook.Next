//! Shared, crate-private helpers for the native C ABI boundary.
//!
//! Callers remain responsible for establishing the pointer contracts documented on each
//! exported entry point. Pointer validity is an unsafe caller obligation, not something that
//! null/length checks or panic containment can establish.

#![deny(unsafe_op_in_unsafe_fn)]

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK};

/// Borrow a bounded UTF-8 FFI argument.
///
/// # Safety
/// When non-null and within the length bounds, `ptr` must refer to one allocation
/// readable for `len` bytes. That memory must remain valid and immutable for `'a`;
/// the caller must not let the returned borrow outlive its backing buffer.
pub(crate) unsafe fn utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() || len > max_len || len > isize::MAX as usize {
        return None;
    }
    // SAFETY: The caller guarantees readable, immutable storage for the returned lifetime.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
}

/// Copy a bounded UTF-8 FFI argument into Rust-owned storage.
///
/// # Safety
/// When non-null and within the length bounds, `ptr` must refer to one allocation
/// readable and immutable for `len` bytes for the duration of this call.
pub(crate) unsafe fn owned_utf8_arg(ptr: *const u8, len: usize, max_len: usize) -> Option<String> {
    if ptr.is_null() || len > max_len || len > isize::MAX as usize {
        return None;
    }
    // SAFETY: The caller guarantees this input allocation is readable for len bytes during copying.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

/// Borrow optional UTF-8; only a null pointer paired with zero length is absent.
///
/// # Safety
/// Non-null arguments must satisfy `utf8_arg`'s readable-storage and `'a` lifetime
/// contract when their length is within the bounds.
pub(crate) unsafe fn optional_utf8_arg<'a>(
    ptr: *const u8,
    len: usize,
    max_len: usize,
) -> Option<&'a str> {
    if ptr.is_null() {
        return (len == 0).then_some("");
    }
    // SAFETY: The caller supplies the same storage/lifetime guarantees as utf8_arg.
    unsafe { utf8_arg(ptr, len, max_len) }
}

/// Borrow optional bytes; only a null pointer paired with zero length is absent.
///
/// # Safety
/// When non-null and within the bounds, `ptr` must refer to one allocation
/// readable for `len` bytes and remain valid and immutable for `'a`.
pub(crate) unsafe fn optional_bytes_arg<'a>(
    ptr: *const u8,
    len: usize,
    max_len: usize,
) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return (len == 0).then_some(&[]);
    }
    if len > max_len || len > isize::MAX as usize {
        return None;
    }
    // SAFETY: The caller guarantees immutable readable storage for the returned lifetime;
    // null and oversized lengths were rejected above.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Copy JSON into the caller's output buffer.
///
/// # Safety
/// `out_buf` must satisfy `write_bytes_out`'s writable, non-overlapping buffer
/// contract when capacity is sufficient for the JSON bytes.
pub(crate) unsafe fn write_json_out(json: &str, out_buf: *mut u8, out_cap: usize) -> i32 {
    // SAFETY: The caller guarantees a valid, non-overlapping output buffer.
    unsafe { write_bytes_out(json.as_bytes(), out_buf, out_cap) }
}

/// Copy `bytes` into the caller's buffer. Returns the written length, or the negated required
/// size when the buffer is too small.
///
/// # Safety
/// When bytes are copied, `out_buf` must be writable for `out_cap` bytes and must not overlap
/// `bytes`.
pub(crate) unsafe fn write_bytes_out(bytes: &[u8], out_buf: *mut u8, out_cap: usize) -> i32 {
    let needed = bytes.len();
    if needed > out_cap {
        return -(needed as i32);
    }
    if needed == 0 {
        return 0;
    }
    // SAFETY: The caller guarantees writable output, disjoint from bytes; needed fits capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);
    }
    needed as i32
}

/// # Safety
/// `out_required` must be non-null, aligned and exclusively writable for one `usize`.
/// When output is copied, `out_buf` must be exclusively writable for `out_cap` bytes
/// in one live allocation. Both output ranges must be disjoint from each other and
/// `bytes`, and remain valid for this call.
pub(crate) unsafe fn write_v2_out(
    bytes: &[u8],
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    // SAFETY: The caller supplies an aligned, exclusive writable scalar.
    unsafe { *out_required = bytes.len() };
    if bytes.len() > out_cap {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        if out_buf.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        // SAFETY: The caller guarantees disjoint output, and non-null/capacity checks passed.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len()) };
    }
    QL_OK
}

pub(crate) fn ffi_boundary(body: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(QL_ERROR_INTERNAL)
}

pub(crate) fn ffi_void_boundary(body: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(body));
}

#[cfg(test)]
mod tests;
