use super::ql_extract_archive_entry_cancelable;

extern "C" fn cancelled() -> bool {
    true
}

#[test]
fn archive_entry_export_honors_cancellation_before_file_access() {
    let archive = b"missing.zip";
    let entry = b"entry.txt";
    let mut output = [0u8; 16];

    assert_eq!(
        // SAFETY: Both immutable input arrays and the disjoint writable output remain live.
        unsafe {
            ql_extract_archive_entry_cancelable(
                archive.as_ptr(),
                archive.len(),
                entry.as_ptr(),
                entry.len(),
                output.as_mut_ptr(),
                output.len(),
                Some(cancelled),
            )
        },
        -3
    );
}
