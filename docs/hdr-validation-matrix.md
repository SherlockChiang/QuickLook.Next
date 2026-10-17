# HDR Validation Matrix

This matrix separates deterministic policy/transport checks from hardware
acceptance. It is intentionally evidence-first: an automated pass does not
claim that a physical HDR panel rendered a visual result.

## Automated Matrix

| Case | Expected path | Coverage |
|---|---|---|
| SDR display + PQ source | BGRA8 SDR | HdrImagePolicyTests.Selects_sdr_for_standard_dynamic_range_display |
| HDR display + SDR source | BGRA8 SDR | HdrImagePolicyTests.Selects_sdr_for_sdr_source_on_hdr_display |
| HDR display + PQ/BT.2020 10-bit source + FP16 support | FP16 scRGB | HdrImagePolicyTests.Selects_fp16_scrgb_only_for_complete_hdr_source; SystemImageHdrMetadataTests.Converts_pq_rgba16_to_premultiplied_scrgb_half_and_bounded_sdr_preview |
| HDR display + HLG/BT.2020 source | BGRA8 SDR | HdrImagePolicyTests.Keeps_hlg_metadata_sdr_until_ootf_mapping_is_implemented |
| HDR display + PQ source without FP16 support | BGRA8 SDR | HdrImagePolicyTests.Falls_back_when_fp16_surface_is_unavailable_or_metadata_is_incomplete |
| Unknown or mismatched surface format | Reject/fallback | HdrImagePolicyTests.Reconcile_rejects_unknown_and_mismatched_surface_formats |
| Display move or HDR toggle | New generation; stale surface rejected | HdrImagePolicyTests.Display_refresh_state_rejects_stale_surfaces_and_tracks_latest_generation |
| Legacy Host without refresh capability | No refresh message sent | HdrImagePolicyTests.Display_refresh_messages_round_trip_with_legacy_protocol_options |

RasterHost diagnostics emit one bounded hdr decision line per published image.
The line records displayCapability, hdrScRgbSurfaceSupported, sourceProfile,
sourceBitDepth, sourceMetadataComplete, selectedFormat, imageGeneration, and
fallbackReason.

## Hardware Matrix

Each run must record all fields below. Use the exact source hash and preserve
the corresponding log file; do not infer a visual pass from a successful
process exit.

| Case | Windows build | GPU/driver | Panel mode | Source/hash | Expected color-space path | Result/log |
|---|---|---|---|---|---|---|
| SDR panel + SDR source | pending | pending | SDR | pending | BGRA8 / SDR | pending |
| SDR panel + PQ source | pending | pending | SDR | pending | BGRA8 / SDR fallback | pending |
| HDR panel + SDR source | pending | pending | HDR enabled | pending | BGRA8 / SDR | pending |
| HDR panel + PQ source | pending | pending | HDR enabled | pending | FP16 scRGB | pending |
| HDR panel + HLG source | pending | pending | HDR enabled | pending | BGRA8 / SDR fallback | pending |
| HDR toggle while preview is open | pending | pending | SDR to HDR and back | pending | generation-tagged re-render | pending |
| Move preview between SDR and HDR panels | pending | pending | mixed | pending | generation-tagged re-render | pending |
| FP16 capability unavailable | pending | pending | HDR enabled | pending | BGRA8 / SDR fallback | pending |

Physical panel, GPU, and golden-image validation remain open until a suitable
Windows HDR device and licensed PQ/HLG samples are available.
