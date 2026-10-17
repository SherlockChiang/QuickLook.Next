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

## HDR status

The production path now supports validated PQ sources on compatible HDR
displays. The App tracks the advanced-color capability of the display containing
the preview window, and RasterHost publishes
`R16G16B16A16_FLOAT_SCRGB` only when the display, source metadata, and FP16
producer all agree. SDR displays, SDR sources, unknown metadata, unavailable
codecs, and any upload/attach failure use the existing `B8G8R8A8_UNORM` path.

RasterHost probes a temporary FP16 scRGB composition swapchain during its
authenticated startup, decodes PQ pixels without Windows SDR color management,
converts them to premultiplied linear scRGB FP16, and keeps a bounded SDR
fallback in the same decoded packet. Display moves and advanced-color changes
send a generation-tagged refresh against the retained file HANDLE, so stale
surfaces are rejected and zoom/pan state is preserved.

HLG metadata is recognized and preserved, but HLG output remains fail-closed to
SDR until the required OOTF/mastering-luminance mapping is implemented. No
physical HDR panel or visual golden result is claimed by the current automated
checks.

The deterministic and hardware acceptance cases are tracked in
hdr-validation-matrix.md. Automated policy, conversion, fallback, and
stale-generation checks pass; the hardware rows remain pending until a suitable
HDR panel and licensed PQ/HLG samples are available.

The shared image metadata contract carries explicit HDR transfer, primaries,
and completeness fields. They are preserved across the native,
property-handler, and Windows-codec merge paths. The policy selects an FP16
HDR surface only when all three fields are validated as BT.2020 plus PQ at 10
bits or above; HLG remains metadata-only until its OOTF mapping is implemented.

The Windows codec metadata reader now performs a bounded ISO-BMFF probe for
HEIF/AVIF-style containers. It accepts only an `nclx` colour description with
BT.2020 primaries and PQ (SMPTE ST 2084) or HLG (ARIB STD-B67), paired with a
validated 10/12-bit HEVC `hvcC` or AV1 `av1C` record. The probe is capped at 8 MiB,
rejects malformed boxes, and is skipped for non-ISO-BMFF inputs. PQ metadata
selects the FP16 decode path when display policy permits it; HLG metadata
remains a source signal with an SDR fallback.
