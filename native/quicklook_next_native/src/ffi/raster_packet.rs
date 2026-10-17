//! Checked raster packet writers shared by image-producing FFI adapters.

use crate::{QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK};

fn checked_raster_packet_length(width: u32, height: u32, bgra_len: usize) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected_bgra_len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if bgra_len != expected_bgra_len {
        return None;
    }
    8usize.checked_add(expected_bgra_len)
}

fn write_checked_raster_packet(width: u32, height: u32, bgra: &[u8], output: &mut [u8]) -> bool {
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return false;
    };
    if output.len() != total {
        return false;
    }
    output[..4].copy_from_slice(&width.to_le_bytes());
    output[4..8].copy_from_slice(&height.to_le_bytes());
    output[8..].copy_from_slice(bgra);
    true
}

/// # Safety
/// A sufficient, non-null output must contain initialized bytes in one live allocation,
/// be exclusively writable for `out_cap` bytes
/// and must not overlap `bgra` for the duration of this call.
pub(crate) unsafe fn write_raster_packet(
    width: u32,
    height: u32,
    bgra: &[u8],
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return -2;
    };
    if total > i32::MAX as usize {
        return -2;
    }
    if out.is_null() || out_cap < total {
        return -(total as i32);
    }
    // SAFETY: The caller supplies exclusive, non-overlapping output and total fits its capacity.
    let output = unsafe { std::slice::from_raw_parts_mut(out, total) };
    if !write_checked_raster_packet(width, height, bgra, output) {
        return -2;
    }
    total as i32
}

/// # Safety
/// `out_required` must be non-null, aligned and exclusively writable for one `usize`.
/// When the output buffer is large enough, `out_buf` must be exclusively writable
/// for `out_cap` initialized bytes in one live allocation. Both output ranges must be disjoint
/// from each other and `bgra`, and remain valid for this call.
pub(crate) unsafe fn write_raster_packet_v2(
    width: u32,
    height: u32,
    bgra: &[u8],
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    // SAFETY: The caller supplies an aligned, exclusively writable scalar.
    unsafe { *out_required = 0 };
    let Some(total) = checked_raster_packet_length(width, height, bgra.len()) else {
        return QL_ERROR_INTERNAL;
    };
    // SAFETY: The same scalar remains writable and disjoint from output and bgra.
    unsafe { *out_required = total };
    if out_cap < total {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if out_buf.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }
    // SAFETY: The caller guarantees exclusive output disjoint from bgra/scalar; total fits capacity.
    let output = unsafe { std::slice::from_raw_parts_mut(out_buf, total) };
    if !write_checked_raster_packet(width, height, bgra, output) {
        return QL_ERROR_INTERNAL;
    }
    QL_OK
}

#[cfg(test)]
mod tests {
    use super::write_raster_packet_v2;
    use crate::{QL_ERROR_INTERNAL, QL_OK};

    #[test]
    fn checked_raster_writer_writes_exact_packet_and_rejects_bad_layout() {
        let bgra = [3u8, 2, 1, 255, 30, 20, 10, 128];
        let mut packet = [0u8; 16];
        let mut required = 0usize;

        assert_eq!(
            // SAFETY: packet/required are disjoint exclusive storage, separate from the bgra array.
            unsafe {
                write_raster_packet_v2(
                    2,
                    1,
                    &bgra,
                    packet.as_mut_ptr(),
                    packet.len(),
                    &mut required,
                )
            },
            QL_OK
        );
        assert_eq!(required, packet.len());
        assert_eq!(&packet[..4], &2u32.to_le_bytes());
        assert_eq!(&packet[4..8], &1u32.to_le_bytes());
        assert_eq!(&packet[8..], &bgra);

        required = usize::MAX;
        assert_eq!(
            // SAFETY: Buffers/scalar remain disjoint and live; invalid layout returns before copying.
            unsafe {
                write_raster_packet_v2(
                    2,
                    1,
                    &bgra[..4],
                    packet.as_mut_ptr(),
                    packet.len(),
                    &mut required,
                )
            },
            QL_ERROR_INTERNAL
        );
        assert_eq!(required, 0);
    }
}
