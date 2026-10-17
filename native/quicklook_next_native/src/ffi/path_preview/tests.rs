use super::{
    ql_preview_archive, ql_preview_ebook_cancelable, ql_preview_executable_cancelable,
    ql_preview_image_metadata, ql_preview_info, ql_preview_office, ql_preview_text_cancelable,
    ql_preview_torrent_cancelable,
};
use crate::{CancelCallback, MAX_FFI_STRING_BYTES};

extern "C" fn always_cancel() -> bool {
    true
}

#[test]
fn simple_preview_exports_honor_cancellation_before_file_access() {
    let path = b"missing.file";
    let mut output = [0u8; 16];
    let calls: [unsafe extern "C" fn(_, _, _, _, Option<CancelCallback>) -> i32; 4] = [
        ql_preview_text_cancelable,
        ql_preview_ebook_cancelable,
        ql_preview_executable_cancelable,
        ql_preview_torrent_cancelable,
    ];

    for call in calls {
        // SAFETY: path/output are disjoint live arrays and the callback has static lifetime.
        let result = unsafe {
            call(
                path.as_ptr(),
                path.len(),
                output.as_mut_ptr(),
                output.len(),
                Some(always_cancel),
            )
        };
        assert_eq!(result, -3);
    }
    // SAFETY: path/output are disjoint live arrays and the callback has static lifetime.
    let archive_result = unsafe {
        ql_preview_archive(
            path.as_ptr(),
            path.len(),
            output.as_mut_ptr(),
            output.len(),
            Some(always_cancel),
        )
    };
    assert_eq!(archive_result, 0);
}

#[test]
fn path_preview_exports_reject_invalid_pointer_contracts() {
    let path = b"missing.file";
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    let mut output = [0u8; 16];

    assert_eq!(
        // SAFETY: The live path is never accessed because the null output is rejected first.
        unsafe {
            ql_preview_text_cancelable(path.as_ptr(), path.len(), std::ptr::null_mut(), 0, None)
        },
        0
    );
    assert_eq!(
        // SAFETY: The oversized path length is rejected before access to the dangling pointer.
        unsafe {
            ql_preview_text_cancelable(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                output.as_mut_ptr(),
                output.len(),
                None,
            )
        },
        0
    );
    assert_eq!(
        // SAFETY: The oversized path is rejected before either dangling input can be accessed.
        unsafe {
            ql_preview_info(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                0,
                0,
                output.as_mut_ptr(),
                output.len(),
            )
        },
        0
    );
}

#[test]
fn info_preview_treats_invalid_optional_kind_as_empty() {
    let path = b"missing.file";
    let mut output = [0u8; 512];
    // SAFETY: path/output are disjoint live arrays; the invalid null kind is never dereferenced.
    let written = unsafe {
        ql_preview_info(
            path.as_ptr(),
            path.len(),
            std::ptr::null(),
            1,
            0,
            0,
            output.as_mut_ptr(),
            output.len(),
        )
    };
    assert!(written > 0);
    assert!(std::str::from_utf8(&output[..written as usize])
        .expect("info preview output must be UTF-8")
        .contains("\"kind\":\"\""));
}

#[test]
fn office_and_image_metadata_reject_invalid_paths_before_file_access() {
    let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
    let mut output = [0u8; 16];

    assert_eq!(
        // SAFETY: The oversized path is rejected before the dangling input is accessed.
        unsafe {
            ql_preview_office(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                output.as_mut_ptr(),
                output.len(),
                None,
            )
        },
        0
    );
    assert_eq!(
        // SAFETY: The oversized path is rejected before the dangling input is accessed.
        unsafe {
            ql_preview_image_metadata(
                dangling,
                MAX_FFI_STRING_BYTES + 1,
                output.as_mut_ptr(),
                output.len(),
            )
        },
        0
    );
}
