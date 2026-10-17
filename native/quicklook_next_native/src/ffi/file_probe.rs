//! Thin C ABI adapter for cached path-based file probing.

use super::common::{ffi_boundary, utf8_arg};
use crate::{preview, MAX_FFI_STRING_BYTES};

/// Probe a file (UTF-8 path) and write its FileProbe JSON (UTF-8) into `out`.
/// Returns the JSON byte length, `-needed` if the buffer is too small, or a negative error.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_probe_file(
    path_utf8: *const u8,
    path_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        // SAFETY: The export caller guarantees valid input/output buffers for this call.
        let path = match unsafe { utf8_arg(path_utf8, path_len, MAX_FFI_STRING_BYTES) } {
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
        // SAFETY: out is exclusive caller storage with checked capacity; JSON is separately owned.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
        bytes.len() as i32
    })
}
