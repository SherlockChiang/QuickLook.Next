use super::{
    ffi_boundary, ffi_void_boundary, optional_bytes_arg, optional_utf8_arg, owned_utf8_arg,
    utf8_arg, write_json_out, write_v2_out,
};
use crate::{QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK};

#[test]
fn utf8_arguments_reject_null_nonzero_and_invalid_bytes() {
    let invalid = [0xff];
    // SAFETY: Null is rejected before slice construction.
    assert!(unsafe { utf8_arg(std::ptr::null(), 0, 8) }.is_none());
    // SAFETY: Null is rejected before slice construction.
    assert!(unsafe { utf8_arg(std::ptr::null(), 1, 8) }.is_none());
    // SAFETY: Null is rejected before copying.
    assert!(unsafe { owned_utf8_arg(std::ptr::null(), 1, 8) }.is_none());
    // SAFETY: invalid is live immutable storage, borrowed only for this assertion.
    assert!(unsafe { utf8_arg(invalid.as_ptr(), invalid.len(), 8) }.is_none());
    // SAFETY: invalid remains readable until the owned copy attempt returns.
    assert!(unsafe { owned_utf8_arg(invalid.as_ptr(), invalid.len(), 8) }.is_none());
}

#[test]
fn optional_arguments_accept_only_null_zero_length() {
    assert_eq!(
        // SAFETY: Null/zero uses the empty-input path without accessing storage.
        unsafe { optional_utf8_arg(std::ptr::null(), 0, 8) },
        Some("")
    );
    assert_eq!(
        // SAFETY: Null/zero uses the empty-input path without accessing storage.
        unsafe { optional_bytes_arg(std::ptr::null(), 0, 8) },
        Some(&[][..])
    );
    // SAFETY: Null/nonzero is rejected before access.
    assert!(unsafe { optional_utf8_arg(std::ptr::null(), 1, 8) }.is_none());
    // SAFETY: Null/nonzero is rejected before access.
    assert!(unsafe { optional_bytes_arg(std::ptr::null(), 1, 8) }.is_none());
}

#[test]
fn oversized_arguments_are_rejected_before_dereference() {
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    // SAFETY: The oversized length is rejected before this dangling pointer is accessed.
    assert!(unsafe { utf8_arg(dangling, 9, 8) }.is_none());
    // SAFETY: The oversized length is rejected before this dangling pointer is accessed.
    assert!(unsafe { owned_utf8_arg(dangling, 9, 8) }.is_none());
    // SAFETY: The oversized length is rejected before this dangling pointer is accessed.
    assert!(unsafe { optional_utf8_arg(dangling, 9, 8) }.is_none());
    // SAFETY: The oversized length is rejected before this dangling pointer is accessed.
    assert!(unsafe { optional_bytes_arg(dangling, 9, 8) }.is_none());
}

#[test]
fn slice_lengths_above_isize_max_are_rejected_before_dereference() {
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    let length = isize::MAX as usize + 1;
    // SAFETY: The isize length bound is checked before access to the dangling pointer.
    assert!(unsafe { utf8_arg(dangling, length, usize::MAX) }.is_none());
    // SAFETY: The isize length bound is checked before access to the dangling pointer.
    assert!(unsafe { owned_utf8_arg(dangling, length, usize::MAX) }.is_none());
    // SAFETY: The isize length bound is checked before access to the dangling pointer.
    assert!(unsafe { optional_utf8_arg(dangling, length, usize::MAX) }.is_none());
    // SAFETY: The isize length bound is checked before access to the dangling pointer.
    assert!(unsafe { optional_bytes_arg(dangling, length, usize::MAX) }.is_none());
}

#[test]
fn json_output_preserves_length_and_capacity_contract() {
    let mut output = [0u8; 3];
    assert_eq!(
        // SAFETY: output is live exclusive storage and does not overlap the string literal.
        unsafe { write_json_out("abc", output.as_mut_ptr(), output.len()) },
        3
    );
    assert_eq!(&output, b"abc");
    assert_eq!(
        // SAFETY: The short buffer path reports size without writing past output.
        unsafe { write_json_out("abcd", output.as_mut_ptr(), output.len()) },
        -4
    );
}

#[test]
fn v2_output_reports_required_size_and_invalid_output() {
    let payload = b"abc";
    let mut required = usize::MAX;
    let mut short = [0u8; 2];
    assert_eq!(
        // SAFETY: required is aligned/exclusive; the disjoint short buffer is not written.
        unsafe { write_v2_out(payload, short.as_mut_ptr(), short.len(), &mut required,) },
        QL_ERROR_BUFFER_TOO_SMALL
    );
    assert_eq!(required, payload.len());

    required = 0;
    assert_eq!(
        // SAFETY: required is writable; null output is rejected before any copy.
        unsafe { write_v2_out(payload, std::ptr::null_mut(), payload.len(), &mut required) },
        QL_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(required, payload.len());

    let mut output = [0u8; 3];
    required = 0;
    assert_eq!(
        // SAFETY: output and required are disjoint writable storage, separate from payload.
        unsafe { write_v2_out(payload, output.as_mut_ptr(), output.len(), &mut required,) },
        QL_OK
    );
    assert_eq!(required, payload.len());
    assert_eq!(&output, payload);

    required = usize::MAX;
    assert_eq!(
        // SAFETY: Only the aligned scalar is written; empty bytes never access the null buffer.
        unsafe { write_v2_out(&[], std::ptr::null_mut(), 0, &mut required) },
        QL_OK
    );
    assert_eq!(required, 0);
}

#[test]
fn empty_legacy_output_never_copies_to_null_storage() {
    // SAFETY: Empty input returns before accessing the null output, even with spare capacity.
    assert_eq!(unsafe { write_json_out("", std::ptr::null_mut(), 0) }, 0);
    // SAFETY: Empty input returns before accessing the null output, even with spare capacity.
    assert_eq!(unsafe { write_json_out("", std::ptr::null_mut(), 8) }, 0);
}

#[test]
fn panic_boundaries_map_or_contain_panics() {
    assert_eq!(ffi_boundary(|| panic!("test panic")), QL_ERROR_INTERNAL);
    ffi_void_boundary(|| panic!("test panic"));
}
