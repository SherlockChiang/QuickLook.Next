# Image Color Management

QuickLook.Next treats image preview decode as a bounded rasterization step. The native JPEG path applies embedded ICC profiles through `qcms` before converting to premultiplied BGRA.

Runtime fallback policy:

- JPEGs with ICC (`APP2`) or Adobe (`APP14`) markers prefer Windows system decode because `SystemImageDecoder` requests `ColorManagementMode.ColorManageToSRgb`.
- If system decode fails for ICC JPEGs, the Rust fallback rebuilds ordered ICC chunks and transforms RGBA pixels to sRGB with `qcms`.
- If the ICC profile is invalid or unsupported, Rust fallback fails closed rather than showing a potentially wrong-color preview.
- Adobe (`APP14`) JPEGs still require the system decoder after a WIC failure because their marker can describe non-ICC transform semantics.
- AVIF, HEIC/HEIF, and JXL are treated as system-required formats. If the platform codec is unavailable, Rust fallback is skipped.

Covered by corpus smoke:

- `testdata/image-corpus/external/jpeg-cmyk.jpg`: decoded by Rust native image path.
- `testdata/image-corpus/external/jpeg-wide-gamut-icc.jpg`: decoded by Rust native image path.
- Generated JPEG APP2 ICC marker corpus: ICC chunks are reassembled and invalid
  profiles are rejected by Rust native image path.
- Generated JPEG APP14 Adobe marker corpus: accepted by Rust native image path.

Not covered yet:

- Wide-gamut visual golden comparison.
- CMYK color transform validation beyond successful decode.

Design note: keep native decode bounded and predictable. ICC conversion happens before BGRA conversion so App/RasterHost continue to consume the same premultiplied BGRA surface format.
