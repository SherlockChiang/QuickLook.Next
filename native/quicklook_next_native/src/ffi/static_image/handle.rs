//! Borrowed-HANDLE static-image decode adapters and exact waveform packet writing.

use std::io::Read;
use std::path::Path;

use image::ImageFormat;

use super::super::common::ffi_boundary;
use super::super::handle_preview::reopen_handle_input_v2;
use super::packet::{write_image_packet, ImagePacket};
use crate::{
    cancel_requested, decode_image_bgra_reader, decode_image_bgra_reader_with_waveform,
    decode_svg_bgra_bytes, decode_svg_bgra_bytes_with_waveform,
    preflight_native_image_packet_length, CancelCallback, IMAGE_WAVEFORM_DENSITY_BYTES,
    IMAGE_WAVEFORM_HEIGHT, IMAGE_WAVEFORM_PACKET_HEADER_BYTES, IMAGE_WAVEFORM_WIDTH,
    MAX_IMAGE_RASTER_DIMENSION, MAX_IMAGE_WAVEFORM_PACKET_BYTES, MAX_NATIVE_IMAGE_DECODE_PIXELS,
    MAX_SVG_INPUT_BYTES, QL_ERROR_BUFFER_TOO_SMALL, QL_ERROR_CANCELLED, QL_ERROR_INTERNAL,
    QL_ERROR_INVALID_ARGUMENT, QL_ERROR_IO, QL_ERROR_LENGTH_MISMATCH, QL_ERROR_LIMIT_EXCEEDED,
    QL_ERROR_MALFORMED, QL_OK,
};

/// Decode a native-safe static image from a borrowed Windows file handle.
/// Output layout matches `ql_decode_image_sized_cancelable`.
///
/// # Safety
/// The pointer, callback and borrowed-HANDLE requirements match `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The caller lends a live input/HANDLE and disjoint exclusive output/scalar ranges.
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if expected_length > 256 * 1024 * 1024 {
            return QL_ERROR_LIMIT_EXCEEDED;
        }
        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extension = Path::new(&logical_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let decoded = if extension == "svg" {
            if expected_length > MAX_SVG_INPUT_BYTES {
                return QL_ERROR_LIMIT_EXCEEDED;
            }
            let mut data = Vec::with_capacity(expected_length as usize);
            let mut chunk = [0u8; 64 * 1024];
            loop {
                if cancel_requested(cancel_cb) {
                    return QL_ERROR_CANCELLED;
                }
                let read = match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => return QL_ERROR_IO,
                };
                if data.len().saturating_add(read) > expected_length as usize {
                    return QL_ERROR_LENGTH_MISMATCH;
                }
                data.extend_from_slice(&chunk[..read]);
            }
            if data.len() as u64 != expected_length {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            decode_svg_bgra_bytes(&data, target_width, target_height, cancel_cb)
        } else {
            let required_format = match extension.as_str() {
                "png" => ImageFormat::Png,
                "jpg" | "jpeg" | "jpe" => ImageFormat::Jpeg,
                "gif" => ImageFormat::Gif,
                "bmp" => ImageFormat::Bmp,
                "ico" => ImageFormat::Ico,
                "tif" | "tiff" => ImageFormat::Tiff,
                "webp" => ImageFormat::WebP,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
            let required = match preflight_native_image_packet_length(
                &mut file,
                required_format,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                target_width,
                target_height,
                false,
            ) {
                Ok(required) => required,
                Err(status) => return status,
            };
            if cancel_requested(cancel_cb) {
                return QL_ERROR_CANCELLED;
            }
            *out_required = required;
            if required > out_cap {
                return QL_ERROR_BUFFER_TOO_SMALL;
            }
            decode_image_bgra_reader(
                file,
                &logical_name,
                target_width,
                target_height,
                cancel_cb,
                Some(required_format),
            )
        };
        let (
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra,
        ) = match decoded {
            Some(decoded) => decoded,
            None => {
                return if cancel_requested(cancel_cb) {
                    QL_ERROR_CANCELLED
                } else {
                    QL_ERROR_MALFORMED
                }
            }
        };
        write_image_packet(
            ImagePacket {
                width,
                height,
                original_width,
                original_height,
                decode_ms,
                resize_ms,
                convert_ms,
                bgra: &bgra,
            },
            out_buf,
            out_cap,
            out_required,
        )
    })
}

/// Decode a native-safe static image and derive its bounded RGB density scope during the final
/// pixel-conversion pass.
///
/// Output layout is ten little-endian `u32` values followed by two exact payloads:
/// `[w, h, original_w, original_h, decode_ms, resize_ms, convert_ms, 192, 96, density_len]`,
/// then `w*h*4` premultiplied BGRA bytes, then planar row-major R/G/B density bytes.
///
/// # Safety
/// The pointer, callback and borrowed-HANDLE requirements match `ql_preview_text_handle`.
#[no_mangle]
pub unsafe extern "C" fn ql_decode_image_with_waveform_handle(
    source_handle: isize,
    expected_length: u64,
    logical_name_utf8: *const u8,
    logical_name_len: usize,
    target_width: u32,
    target_height: u32,
    out_buf: *mut u8,
    out_cap: usize,
    out_required: *mut usize,
    cancel_cb: Option<CancelCallback>,
) -> i32 {
    // SAFETY: The caller lends a live input/HANDLE and disjoint exclusive output/scalar ranges.
    ffi_boundary(|| unsafe {
        if out_required.is_null() {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        *out_required = 0;
        if out_buf.is_null() && out_cap != 0 {
            return QL_ERROR_INVALID_ARGUMENT;
        }
        if expected_length > 256 * 1024 * 1024 {
            return QL_ERROR_LIMIT_EXCEEDED;
        }

        let (mut file, logical_name, _, _) = match reopen_handle_input_v2(
            source_handle,
            expected_length,
            logical_name_utf8,
            logical_name_len,
            cancel_cb,
        ) {
            Ok(input) => input,
            Err(status) => return status,
        };
        let extension = Path::new(&logical_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let decoded = if extension == "svg" {
            if expected_length > MAX_SVG_INPUT_BYTES {
                return QL_ERROR_LIMIT_EXCEEDED;
            }
            let mut data = Vec::with_capacity(expected_length as usize);
            let mut chunk = [0u8; 64 * 1024];
            loop {
                if cancel_requested(cancel_cb) {
                    return QL_ERROR_CANCELLED;
                }
                let read = match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => return QL_ERROR_IO,
                };
                if data.len().saturating_add(read) > expected_length as usize {
                    return QL_ERROR_LENGTH_MISMATCH;
                }
                data.extend_from_slice(&chunk[..read]);
            }
            if data.len() as u64 != expected_length {
                return QL_ERROR_LENGTH_MISMATCH;
            }
            decode_svg_bgra_bytes_with_waveform(&data, target_width, target_height, cancel_cb)
        } else {
            let required_format = match extension.as_str() {
                "png" => ImageFormat::Png,
                "jpg" | "jpeg" | "jpe" => ImageFormat::Jpeg,
                "bmp" => ImageFormat::Bmp,
                "ico" => ImageFormat::Ico,
                "tif" | "tiff" => ImageFormat::Tiff,
                "webp" => ImageFormat::WebP,
                _ => return QL_ERROR_INVALID_ARGUMENT,
            };
            let required = match preflight_native_image_packet_length(
                &mut file,
                required_format,
                MAX_NATIVE_IMAGE_DECODE_PIXELS,
                target_width,
                target_height,
                true,
            ) {
                Ok(required) => required,
                Err(status) => return status,
            };
            if cancel_requested(cancel_cb) {
                return QL_ERROR_CANCELLED;
            }
            *out_required = required;
            if required > out_cap {
                return QL_ERROR_BUFFER_TOO_SMALL;
            }
            decode_image_bgra_reader_with_waveform(
                file,
                &logical_name,
                target_width,
                target_height,
                cancel_cb,
                Some(required_format),
            )
        };
        let (
            (
                width,
                height,
                original_width,
                original_height,
                decode_ms,
                resize_ms,
                convert_ms,
                bgra,
            ),
            density,
        ) = match decoded {
            Some(decoded) => decoded,
            None => {
                return if cancel_requested(cancel_cb) {
                    QL_ERROR_CANCELLED
                } else {
                    QL_ERROR_MALFORMED
                }
            }
        };
        if cancel_requested(cancel_cb) {
            return QL_ERROR_CANCELLED;
        }
        let packet = ImageWaveformPacket {
            width,
            height,
            original_width,
            original_height,
            decode_ms,
            resize_ms,
            convert_ms,
            bgra: &bgra,
            density: &density,
        };
        write_image_waveform_packet(packet, out_buf, out_cap, out_required)
    })
}

struct ImageWaveformPacket<'a> {
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
    decode_ms: u32,
    resize_ms: u32,
    convert_ms: u32,
    bgra: &'a [u8],
    density: &'a [u8],
}

/// # Safety
/// `out_required` must be non-null, aligned and exclusively writable for one `usize`.
/// A sufficient `out_buf` must be exclusively writable for `out_cap` bytes in one live
/// allocation, disjoint from packet data and the scalar. All storage must remain valid
/// until return.
unsafe fn write_image_waveform_packet(
    packet: ImageWaveformPacket<'_>,
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
    if packet.width == 0
        || packet.height == 0
        || packet.width > MAX_IMAGE_RASTER_DIMENSION
        || packet.height > MAX_IMAGE_RASTER_DIMENSION
        || packet.bgra.len() != raster_bytes
        || packet.density.len() != IMAGE_WAVEFORM_DENSITY_BYTES
    {
        return QL_ERROR_INTERNAL;
    }
    let total = match IMAGE_WAVEFORM_PACKET_HEADER_BYTES
        .checked_add(raster_bytes)
        .and_then(|bytes| bytes.checked_add(packet.density.len()))
    {
        Some(total) if total <= MAX_IMAGE_WAVEFORM_PACKET_BYTES => total,
        _ => return QL_ERROR_LIMIT_EXCEEDED,
    };

    // SAFETY: The caller supplies an aligned exclusive scalar, disjoint from output and packet.
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
        IMAGE_WAVEFORM_WIDTH,
        IMAGE_WAVEFORM_HEIGHT,
        IMAGE_WAVEFORM_DENSITY_BYTES as u32,
    ];
    for (index, value) in header.into_iter().enumerate() {
        // SAFETY: Each local word is live and disjoint from exclusive output; total covers fields.
        unsafe {
            std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), out_buf.add(index * 4), 4);
        }
    }
    // SAFETY: Caller output is disjoint from both borrowed packet slices; the checked total
    // covers the header, raster and density ranges, each copied exactly once.
    unsafe {
        std::ptr::copy_nonoverlapping(
            packet.bgra.as_ptr(),
            out_buf.add(IMAGE_WAVEFORM_PACKET_HEADER_BYTES),
            raster_bytes,
        );
        std::ptr::copy_nonoverlapping(
            packet.density.as_ptr(),
            out_buf.add(IMAGE_WAVEFORM_PACKET_HEADER_BYTES + raster_bytes),
            packet.density.len(),
        );
    }
    QL_OK
}

#[cfg(test)]
mod tests {
    use super::{ql_decode_image_handle, ql_decode_image_with_waveform_handle};
    use crate::QL_ERROR_INVALID_ARGUMENT;

    #[test]
    fn image_handle_exports_reject_null_required_before_handle_access() {
        for decode in [ql_decode_image_handle, ql_decode_image_with_waveform_handle] {
            assert_eq!(
                // SAFETY: Null required is rejected before any other pointer or HANDLE access.
                unsafe {
                    decode(
                        0,
                        0,
                        std::ptr::null(),
                        0,
                        0,
                        0,
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        None,
                    )
                },
                QL_ERROR_INVALID_ARGUMENT
            );
        }
    }
}
