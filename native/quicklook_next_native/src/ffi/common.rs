//! Shared, crate-private helpers for the native C ABI boundary.
//!
//! Callers remain responsible for establishing the pointer contracts documented on each
//! exported entry point. This step only changes module ownership; it intentionally preserves the
//! existing signatures and return semantics.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK};

pub(crate) fn utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() || len > max_len {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok()
}

/// Copy a bounded UTF-8 FFI argument into Rust-owned storage.
///
/// # Safety
/// `ptr` must be readable for `len` bytes for the duration of this call.
pub(crate) unsafe fn owned_utf8_arg(ptr: *const u8, len: usize, max_len: usize) -> Option<String> {
    if ptr.is_null() || len > max_len {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

pub(crate) fn optional_utf8_arg<'a>(ptr: *const u8, len: usize, max_len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return (len == 0).then_some("");
    }
    utf8_arg(ptr, len, max_len)
}

pub(crate) fn optional_bytes_arg<'a>(
    ptr: *const u8,
    len: usize,
    max_len: usize,
) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return (len == 0).then_some(&[]);
    }
    if len > max_len {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

pub(crate) fn write_json_out(json: &str, out_buf: *mut u8, out_cap: usize) -> i32 {
    write_bytes_out(json.as_bytes(), out_buf, out_cap)
}

/// Copy `bytes` into the caller's buffer. Returns the written length, or the negated required
/// size when the buffer is too small.
///
/// # Safety
/// When bytes are copied, `out_buf` must be writable for `out_cap` bytes and must not overlap
/// `bytes`.
pub(crate) fn write_bytes_out(bytes: &[u8], out_buf: *mut u8, out_cap: usize) -> i32 {
    let needed = bytes.len();
    if needed > out_cap {
        return -(needed as i32);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);
    }
    needed as i32
}

/// # Safety
/// `out_required` must be writable and, when output is copied, `out_buf` must be writable for
/// `out_cap` bytes and must not overlap `bytes`.
pub(crate) unsafe fn write_v2_out(
    bytes: &[u8],
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    unsafe { *out_required = bytes.len() };
    if bytes.len() > out_cap {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if !bytes.is_empty() {
        if out_buf.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
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
mod tests {
    use super::{
        ffi_boundary, ffi_void_boundary, optional_bytes_arg, optional_utf8_arg, owned_utf8_arg,
        utf8_arg, write_json_out, write_v2_out,
    };
    use crate::{QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK};

    #[test]
    fn utf8_arguments_reject_null_nonzero_and_invalid_bytes() {
        let invalid = [0xff];
        assert!(utf8_arg(std::ptr::null(), 0, 8).is_none());
        assert!(utf8_arg(std::ptr::null(), 1, 8).is_none());
        assert!(unsafe { owned_utf8_arg(std::ptr::null(), 1, 8) }.is_none());
        assert!(utf8_arg(invalid.as_ptr(), invalid.len(), 8).is_none());
        assert!(unsafe { owned_utf8_arg(invalid.as_ptr(), invalid.len(), 8) }.is_none());
    }

    #[test]
    fn optional_arguments_accept_only_null_zero_length() {
        assert_eq!(optional_utf8_arg(std::ptr::null(), 0, 8), Some(""));
        assert_eq!(optional_bytes_arg(std::ptr::null(), 0, 8), Some(&[][..]));
        assert!(optional_utf8_arg(std::ptr::null(), 1, 8).is_none());
        assert!(optional_bytes_arg(std::ptr::null(), 1, 8).is_none());
    }

    #[test]
    fn oversized_arguments_are_rejected_before_dereference() {
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        assert!(utf8_arg(dangling, 9, 8).is_none());
        assert!(unsafe { owned_utf8_arg(dangling, 9, 8) }.is_none());
        assert!(optional_utf8_arg(dangling, 9, 8).is_none());
        assert!(optional_bytes_arg(dangling, 9, 8).is_none());
    }

    #[test]
    fn json_output_preserves_length_and_capacity_contract() {
        let mut output = [0u8; 3];
        assert_eq!(write_json_out("abc", output.as_mut_ptr(), output.len()), 3);
        assert_eq!(&output, b"abc");
        assert_eq!(
            write_json_out("abcd", output.as_mut_ptr(), output.len()),
            -4
        );
    }

    #[test]
    fn v2_output_reports_required_size_and_invalid_output() {
        let payload = b"abc";
        let mut required = usize::MAX;
        let mut short = [0u8; 2];
        assert_eq!(
            unsafe { write_v2_out(payload, short.as_mut_ptr(), short.len(), &mut required,) },
            QL_ERROR_BUFFER_TOO_SMALL
        );
        assert_eq!(required, payload.len());

        required = 0;
        assert_eq!(
            unsafe { write_v2_out(payload, std::ptr::null_mut(), payload.len(), &mut required) },
            QL_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(required, payload.len());

        let mut output = [0u8; 3];
        required = 0;
        assert_eq!(
            unsafe { write_v2_out(payload, output.as_mut_ptr(), output.len(), &mut required,) },
            QL_OK
        );
        assert_eq!(required, payload.len());
        assert_eq!(&output, payload);

        required = usize::MAX;
        assert_eq!(
            unsafe { write_v2_out(&[], std::ptr::null_mut(), 0, &mut required) },
            QL_OK
        );
        assert_eq!(required, 0);
    }

    #[test]
    fn panic_boundaries_map_or_contain_panics() {
        assert_eq!(ffi_boundary(|| panic!("test panic")), QL_ERROR_INTERNAL);
        ffi_void_boundary(|| panic!("test panic"));
    }
}
