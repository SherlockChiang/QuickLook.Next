# QuickLook.Next Review Readiness

This note is a reviewer-facing status page. It records what has already been
hardened, how to verify it locally, and which limitations are intentionally
left visible instead of hidden behind vague TODOs.

## Fixed And Hardened

- Rust-first preview path: text, folders, archives, packages, certificates,
  executables, torrents, and lightweight Office previews are handled by the
  native layer rather than the legacy .NET plugin pipeline.
- RasterHost is lazy-started and scoped to surface-producing work: images, PDF
  page rasterization, shell thumbnails, and fallback media/image surfaces.
- Tray context menu handling is isolated in `TrayIconManager`. It uses a native
  popup menu because the preview window is normally hidden/no-activate, so a
  WinUI `MenuFlyout` anchored to the App XAML root is not reliable for tray
  right-clicks.
- WebView/WebView2 use is guarded out of the product path.
- Legacy `.NET Plugin.*` projects are kept as reference source only. They are
  not in the default solution, default plugin discovery path, or release package
  boundary.
- Legacy contracts are explicitly documented as reference/plugin contracts.
  `PreviewResult.Bgra` is marked obsolete and the hot path uses Rust/native JSON
  plus shared raster surfaces instead.
- Native text and XML preview boundaries are hardened:
  - UTF-8 text preview truncation backs up to a valid char boundary.
  - UTF-16 BOM text truncation avoids dangling half code units.
  - Office preview text truncation is char-boundary safe.
  - XML text extraction supports named entities and decimal/hex numeric
    character references.
- Archive/package internal reads now have a hard read cap in addition to ZIP
  metadata size checks. This covers Office XML parts, embedded Office images,
  MSIX/AppX manifests, and package icon extraction.
- UI strings now flow through `Strings/en-US/Resources.resw` via `UiStrings`,
  with fallback values for unpackaged/debug resource loading failures.
- Stable visible XAML labels use `x:Uid` resource entries for the title brand,
  preview detail labels, image zoom presets, and preview chrome actions.
- Preview chrome actions are wired: copy path, open file, reveal in Explorer,
  and image zoom presets no longer appear as non-functional visual controls.
- Folder/listing previews keep glyph placeholders but asynchronously replace
  real filesystem rows with Shell thumbnail/icon cache images when available.
- Virtual archive entries use extension-aware glyphs for common images, media,
  archives, Office documents, code/text files, installers, certificates,
  torrents, and disk images.
- Folder navigation and listing icon work use the active preview cancellation
  token/generation guard so stale results do not merge into a later preview.
- Autostart now prefers HKCU Run, uses Startup-folder shortcuts only as a
  fallback, and repairs stale QuickLookNext entries that point at an old exe.

## Verification Commands

Run these from the repository root:

```powershell
cargo test --manifest-path native\quicklook_next_native\Cargo.toml
cargo build --release --manifest-path native\quicklook_next_native\Cargo.toml
powershell -ExecutionPolicy Bypass -File tools\smoke-native.ps1
dotnet build QuickLook.Next.slnx -c Release
powershell -ExecutionPolicy Bypass -File tools\guard-architecture.ps1 -SkipDist
```

Useful targeted checks:

```powershell
rg -n "TrackPopupMenu|CreatePopupMenu|AppendMenu|DestroyMenu|TPM_|MF_CHECKED|MF_STRING" src\QuickLook.Next.App
rg -n "WebView|WebView2" src native tools README.md docs
rg -n "QuickLook.Next.Plugin." src tools README.md docs
rg -n "read_to_end\(&mut bytes\)" native\quicklook_next_native\src\preview.rs
```

The tray popup search should only hit `src/QuickLook.Next.App/TrayIconManager.cs`.

The remaining `read_to_end` calls in `preview.rs` should be limited to:

- `read_file_prefix`, which reads through `take(max_bytes)`.
- `read_limited_to_end`, which reads through `take(max_size + 1)` and rejects
  payloads over the cap.

## Known Remaining Work

- Continue improving Office fidelity. The native renderer extracts text, tables,
  relationships, and representative layout/images, but it is not a full Office
  rendering engine.
- Expand real-world smoke assets for larger PDFs, malformed archives, unusual
  APK/MSIX manifests, mixed-encoding text files, and complex Office files.
  Current smoke coverage includes UTF-16 text and corrupt ZIP fail-closed checks,
  but still needs more externally sourced real-world files.
- Push cancellation deeper into Rust/native decode/listing loops. The App now
  prevents stale merge/update work, but native FFI calls are still synchronous
  once entered.
- Continue Shell icon coverage for virtual archive entries if a stable file type
  icon can be resolved without pretending the virtual item is a real path.

## Why Legacy Plugin Source Remains

The old `.NET Plugin.*` source is retained as reference material for behavior
parity with classic QuickLook and for reviewing old provider assumptions. It is
not the default architecture for QuickLook.Next.

The intended boundary is:

- App + native Rust own lightweight preview decisions and structured preview
  data.
- RasterHost owns D3D/shared surface production and Windows PDF/image/media
  raster integrations.
- Legacy plugin contracts and projects remain available for comparison only and
  are guarded out of the default product/publish path.
