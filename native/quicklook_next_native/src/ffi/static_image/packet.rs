//! Direct writers for bounded static-image HANDLE packets.

use crate::{
    IMAGE_PACKET_HEADER_BYTES, MAX_IMAGE_RASTER_DIMENSION, QL_ERROR_BUFFER_TOO_SMALL,
    QL_ERROR_INTERNAL, QL_ERROR_INVALID_ARGUMENT, QL_OK,
};

pub(crate) struct ImagePacket<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) original_width: u32,
    pub(crate) original_height: u32,
    pub(crate) decode_ms: u32,
    pub(crate) resize_ms: u32,
    pub(crate) convert_ms: u32,
    pub(crate) bgra: &'a [u8],
}

/// Write the static image packet directly into the caller's buffer. Keeping the decoded BGRA
/// slice borrowed avoids a second full-raster allocation/copy through a temporary packet Vec.
///
/// # Safety
/// `out_required` must be non-null, aligned and exclusively writable for one `usize`.
/// A sufficient `out_buf` must be exclusively writable for `out_cap` bytes in one live
/// allocation, disjoint from packet data and the scalar. All storage must remain valid
/// until return.
pub(crate) unsafe fn write_image_packet(
    packet: ImagePacket<'_>,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
) -> i32 {
    let raster_bytes = match (packet.width as usize)
        .checked_mul(packet.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    {
        Some(bytes) => bytes,
        None => return QL_ERROR_INTERNAL,
    };
    let total = match IMAGE_PACKET_HEADER_BYTES.checked_add(raster_bytes) {
        Some(total)
            if packet.width > 0
                && packet.height > 0
                && packet.width <= MAX_IMAGE_RASTER_DIMENSION
                && packet.height <= MAX_IMAGE_RASTER_DIMENSION
                && packet.bgra.len() == raster_bytes
                && total
                    <= IMAGE_PACKET_HEADER_BYTES
                        + (MAX_IMAGE_RASTER_DIMENSION as usize)
                            * (MAX_IMAGE_RASTER_DIMENSION as usize)
                            * 4 =>
        {
            total
        }
        _ => return QL_ERROR_INTERNAL,
    };

    // SAFETY: The caller supplies an aligned exclusive scalar, disjoint from output and pixels.
    unsafe { *out_required = total };
    if total > out_cap {
        return QL_ERROR_BUFFER_TOO_SMALL;
    }
    if out_buf.is_null() {
        return QL_ERROR_INVALID_ARGUMENT;
    }

    let header = [
        packet.width,
        packet.height,
        packet.original_width,
        packet.original_height,
        packet.decode_ms,
        packet.resize_ms,
        packet.convert_ms,
    ];
    for (index, value) in header.into_iter().enumerate() {
        // SAFETY: Each local header word is disjoint from exclusive output; checked total covers
        // every four-byte field and the temporary word remains live until its copy returns.
        unsafe {
            std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), out_buf.add(index * 4), 4);
        }
    }
    // SAFETY: Caller output is disjoint from packet.bgra and the checked total covers this range.
    unsafe {
        std::ptr::copy_nonoverlapping(
            packet.bgra.as_ptr(),
            out_buf.add(IMAGE_PACKET_HEADER_BYTES),
            raster_bytes,
        );
    }
    QL_OK
}
