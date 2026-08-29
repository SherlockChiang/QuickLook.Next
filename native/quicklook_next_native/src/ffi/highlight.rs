//! Thin C ABI adapter for native syntax-highlight token spans.
//!
//! The packet layout is fixed: a little-endian `u32` span count followed by one 12-byte record
//! per span — little-endian `u32` UTF-16 start, `u32` UTF-16 length, `u32` kind discriminant.
//! The managed caller slices its own string with these offsets, so no token text crosses the
//! boundary.

use super::common::{ffi_boundary, optional_utf8_arg, write_bytes_out};
use crate::preview::highlight_spans;

/// Tokenize preview text into bounded highlight spans. Returns the packet length in bytes,
/// the negated required size when `out_cap` is too small, or 0 on invalid arguments.
#[doc = include_str!("../ffi_pointer_safety.md")]
#[no_mangle]
pub unsafe extern "C" fn ql_highlight_spans(
    text_utf8: *const u8,
    text_len: usize,
    lang_utf8: *const u8,
    lang_len: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    ffi_boundary(|| {
        let text = match optional_utf8_arg(text_utf8, text_len, crate::MAX_HIGHLIGHT_TEXT_BYTES) {
            Some(text) => text,
            None => return 0,
        };
        let language = match optional_utf8_arg(lang_utf8, lang_len, 64) {
            Some(language) => language,
            None => return 0,
        };
        let spans = highlight_spans(text, language);
        write_bytes_out(&pack_spans(&spans), out_buf, out_cap)
    })
}

fn pack_spans(spans: &[crate::preview::HighlightSpan]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(4 + spans.len() * 12);
    packet.extend_from_slice(&(spans.len() as u32).to_le_bytes());
    for span in spans {
        packet.extend_from_slice(&span.start.to_le_bytes());
        packet.extend_from_slice(&span.len.to_le_bytes());
        packet.extend_from_slice(&(span.kind as u32).to_le_bytes());
    }
    packet
}

#[cfg(test)]
mod tests {
    use super::{pack_spans, ql_highlight_spans};
    use crate::preview::highlight_spans;

    #[test]
    fn export_rejects_invalid_arguments_without_writing() {
        let dangling = std::ptr::NonNull::<u8>::dangling().as_ptr();
        assert_eq!(
            unsafe {
                ql_highlight_spans(
                    dangling,
                    crate::MAX_HIGHLIGHT_TEXT_BYTES + 1,
                    dangling,
                    3,
                    dangling,
                    0,
                )
            },
            0
        );
        assert_eq!(
            unsafe { ql_highlight_spans(std::ptr::null(), 1, dangling, 3, dangling, 0) },
            0
        );
    }

    #[test]
    fn empty_text_yields_the_zero_header_without_dereferencing() {
        let mut buffer = [0u8; 4];
        let written = unsafe {
            ql_highlight_spans(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        assert_eq!(written, 4);
        assert_eq!(buffer, [0, 0, 0, 0]);
    }

    #[test]
    fn packet_round_trips_header_and_records() {
        let spans = highlight_spans("let x = 1; // hi", "rust");
        let packet = pack_spans(&spans);
        assert_eq!(packet.len(), 4 + spans.len() * 12);
        assert_eq!(
            u32::from_le_bytes(packet[0..4].try_into().unwrap()) as usize,
            spans.len()
        );
        let first_start = u32::from_le_bytes(packet[4..8].try_into().unwrap());
        assert_eq!(first_start, spans[0].start);
    }

    #[test]
    fn empty_packet_is_a_four_byte_zero_header() {
        let packet = pack_spans(&[]);
        assert_eq!(packet, [0, 0, 0, 0]);
    }

    #[test]
    fn export_reports_required_size_for_short_buffers() {
        let text = b"fn main() {}";
        let language = b"rust";
        let mut small = [0u8; 8];
        let required = unsafe {
            ql_highlight_spans(
                text.as_ptr(),
                text.len(),
                language.as_ptr(),
                language.len(),
                small.as_mut_ptr(),
                small.len(),
            )
        };
        assert!(required < 0);
        let required = -required as usize;
        assert!(required > small.len());
        let mut buffer = vec![0u8; required];
        let written = unsafe {
            ql_highlight_spans(
                text.as_ptr(),
                text.len(),
                language.as_ptr(),
                language.len(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        assert_eq!(written as usize, required);
    }

    #[test]
    fn export_offsets_are_utf16_units() {
        // The CJK comment characters occupy 3 UTF-8 bytes each but 1 UTF-16 unit each, so the
        // span offsets must reflect UTF-16 positions, not byte positions. Kind discriminant 3 is
        // the Comment kind, pinned against the managed order by preview::highlight tests.
        let text = "let s = 1; // 中文注释";
        let bytes = text.as_bytes();
        let mut buffer = vec![0u8; 4 + 64 * 12];
        let language = b"rust";
        let written = unsafe {
            ql_highlight_spans(
                bytes.as_ptr(),
                bytes.len(),
                language.as_ptr(),
                language.len(),
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        };
        assert!(written > 0);
        let count = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
        let mut last_end = 0usize;
        let mut saw_comment = false;
        for index in 0..count {
            let at = 4 + index * 12;
            let start = u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap()) as usize;
            let len = u32::from_le_bytes(buffer[at + 4..at + 8].try_into().unwrap()) as usize;
            let kind = u32::from_le_bytes(buffer[at + 8..at + 12].try_into().unwrap());
            assert!(start >= last_end);
            last_end = start + len;
            if kind == 3 {
                saw_comment = true;
                let comment_text: String = text.chars().skip(start).take(len).collect();
                assert_eq!(comment_text, "// 中文注释");
            }
        }
        assert!(saw_comment);
    }

    #[test]
    fn span_offsets_cover_only_the_input() {
        let text = "class A { /* c */ }";
        let spans = highlight_spans(text, "csharp");
        assert!(!spans.is_empty());
        let utf16_len = text.chars().map(char::len_utf16).sum::<usize>() as u32;
        let mut last_end = 0u32;
        for span in spans {
            assert!(span.start >= last_end);
            assert!(span.len > 0);
            last_end = span.start + span.len;
        }
        assert!(last_end <= utf16_len);
    }
}
