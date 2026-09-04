use super::ql_get_thumbnail_cancelable_with_flags;
use crate::QL_ERROR_LIMIT_EXCEEDED;

#[test]
fn thumbnail_export_rejects_oversized_request_before_shell_dispatch() {
    let path = b"missing.file";
    let mut output = [0u8; 16];
    assert_eq!(
        unsafe {
            ql_get_thumbnail_cancelable_with_flags(
                path.as_ptr(),
                path.len(),
                513,
                0,
                output.as_mut_ptr(),
                output.len(),
                None,
            )
        },
        QL_ERROR_LIMIT_EXCEEDED
    );
}
