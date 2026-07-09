# Image Color Management

QuickLook.Next currently treats image preview decode as a bounded rasterization step. The native path validates that JPEG files with CMYK data or ICC markers can be decoded, but it does not apply ICC color transforms.

Runtime fallback policy:

- JPEGs with ICC (`APP2`) or Adobe (`APP14`) markers prefer Windows system decode because `SystemImageDecoder` requests `ColorManagementMode.ColorManageToSRgb`.
- If system decode fails for those color-managed JPEGs, RasterHost skips the Rust fallback rather than showing a potentially wrong-color preview.
- AVIF, HEIC/HEIF, and JXL are treated as system-required formats. If the platform codec is unavailable, Rust fallback is skipped.

Covered by corpus smoke:

- `testdata/image-corpus/external/jpeg-cmyk.jpg`: decoded by Rust native image path.
- `testdata/image-corpus/external/jpeg-wide-gamut-icc.jpg`: decoded by Rust native image path.
- Generated JPEG APP2 ICC marker corpus: accepted by Rust native image path.
- Generated JPEG APP14 Adobe marker corpus: accepted by Rust native image path.

Not implemented yet:

- ICC profile parsing beyond marker acceptance.
- Source-to-sRGB color conversion.
- Wide-gamut visual golden comparison.
- CMYK color transform validation beyond successful decode.

Design note: keep native decode bounded and predictable. If ICC conversion is added, prefer a Rust-side transform before BGRA conversion so App/RasterHost continue to consume the same premultiplied BGRA surface format.
