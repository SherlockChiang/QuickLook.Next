//! Thin C ABI adapter for cached path-based file probing.

use super::common::{ffi_boundary, utf8_arg};
use crate::{preview, MAX_FFI_STRING_BYTES};

/// Probe a file (UTF-8 path) and write its FileProbe JSON (UTF-8) into `out`.
/// Returns the JSON byte length, `-needed` if the buffer is too small, or a negative error.
///
/// # Safety
///
/// `path_utf8` must be readable for `path_len` bytes. When non-null, `out` must be writable
/// for `out_cap` bytes. Both buffers must remain valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn ql_probe_file(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let path = match utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) {
            Some(s) => s,
            None => return -1,
        };
        let json = match preview::file_probe::probe_path_json(path) {
            Some(j) => j,
            None => return -2,
        };
        let bytes = json.as_bytes();
        if out.is_null() || out_cap < bytes.len() {
            return -(bytes.len() as i32);
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
        bytes.len() as i32
    })
}

#[cfg(test)]
mod tests {
    use super::ql_probe_file;
    use crate::MAX_FFI_STRING_BYTES;

    #[test]
    fn file_probe_rejects_oversized_path_before_dereference() {
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();

        assert_eq!(
            unsafe { ql_probe_file(dangling, MAX_FFI_STRING_BYTES + 1, std::ptr::null_mut(), 0,) },
            -1
        );
    }
}
