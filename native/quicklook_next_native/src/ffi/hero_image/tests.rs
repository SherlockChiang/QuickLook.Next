use super::{
    ql_extract_office_image_cancelable, ql_extract_office_image_handle,
    ql_extract_office_layout_image_handle, ql_extract_package_icon_cancelable,
    ql_extract_package_icon_handle,
};
use crate::QL_ERROR_CANCELLED;

extern "C" fn cancelled() -> bool {
    true
}

#[test]
fn hero_exports_cancel_before_file_access() {
    let path = b"missing.zip";
    let mut output = [0u8; 16];
    let calls = [
        ql_extract_package_icon_cancelable,
        ql_extract_office_image_cancelable,
    ];

    for call in calls {
        assert_eq!(
            unsafe {
                call(
                    path.as_ptr(),
                    path.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    Some(cancelled),
                )
            },
            -3
        );
    }
}

#[test]
fn handle_hero_exports_cancel_before_handle_access() {
    let mut required = usize::MAX;
    for call in [
        ql_extract_office_image_handle,
        ql_extract_package_icon_handle,
    ] {
        let status = unsafe {
            call(
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
        required = usize::MAX;
    }

    let layout_status = unsafe {
        ql_extract_office_layout_image_handle(
            0,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            1,
            1,
            std::ptr::null_mut(),
            0,
            &mut required,
            Some(cancelled),
        )
    };
    assert_eq!(layout_status, QL_ERROR_CANCELLED);
    assert_eq!(required, 0);
}
