# Native Image Codec Boundaries

Rust native decode intentionally stays limited to codecs that build reproducibly in the current Windows toolchain.

AVIF:

- `image` 0.25 exposes `avif-native`, but enabling it pulls `dav1d-sys`.
- On Windows/MSVC this build path requires external `dav1d` discovery through `pkg-config`.
- Because the repo currently builds without external native codec installation steps, AVIF remains system/WIC only.
- Re-evaluation result: no repo-local, reproducible Windows/MSVC fallback is enabled.

HEIC/HEIF:

- No Rust native HEIC/HEIF decoder is currently enabled.
- HEIC/HEIF remains system/WIC only.
- Re-evaluation result: no bounded decoder with a repo-local Windows/MSVC build path is enabled.

JXL:

- No Rust native JPEG XL decoder is currently enabled.
- JXL remains system/WIC only when a platform codec is installed.
- Re-evaluation result: keep out of native scope until either the OS codec is available or a reproducible native decoder path is added.

Policy:

- Do not enable native codec features that require machine-global libraries unless the build scripts and CI image install those dependencies explicitly.
- Prefer system decode for AVIF/HEIC/JXL until a reproducible Rust decoder path is available.
- If system decode and shell thumbnail fallback both fail for AVIF/HEIC/JXL, RasterHost returns an explicit missing Windows image codec error instead of a generic unsupported message.
