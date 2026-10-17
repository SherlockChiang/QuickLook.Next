// Compiled by test-rust-compiler-contracts.ps1 with the actual crate lint attributes
// and actual common/raster writer modules. Each cfg selects one contract probe.

#[cfg(valid_contract)]
pub fn documented_writer_calls() {
    let pixels = [0u8; 4];
    let mut output = [0u8; 12];
    // SAFETY: The live, initialized output is exclusive and separate from both inputs.
    unsafe {
        crate::ffi_common::write_json_out("ok", output.as_mut_ptr(), output.len());
        crate::raster_packet::write_raster_packet(
            1,
            1,
            &pixels,
            output.as_mut_ptr(),
            output.len(),
        );
    }
}

#[cfg(implicit_operation)]
pub unsafe fn implicit_pointer_write(output: *mut u8) {
    *output = 1;
}

#[cfg(undocumented_block)]
pub fn undocumented_pointer_write() {
    let mut value = 0u8;
    unsafe { std::ptr::write(&mut value, 1) };
}

#[cfg(safe_json_writer)]
pub fn safe_json_writer_call() {
    let mut output = [0u8; 4];
    crate::ffi_common::write_json_out("ok", output.as_mut_ptr(), output.len());
}

#[cfg(safe_raster_writer)]
pub fn safe_raster_writer_call() {
    let pixels = [0u8; 4];
    let mut output = [0u8; 12];
    crate::raster_packet::write_raster_packet(
        1,
        1,
        &pixels,
        output.as_mut_ptr(),
        output.len(),
    );
}

#[cfg(preview_unsafe)]
pub fn preview_pointer_write() {
    let mut value = 0u8;
    // SAFETY: value is live, aligned and exclusively writable. Preview still forbids unsafe.
    unsafe { std::ptr::write(&mut value, 1) };
}
