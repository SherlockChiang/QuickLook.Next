# QuickLook.Next Agent Guide

This project is a Rust-first QuickLook rewrite with a thin WinUI shell and a
small RasterHost for Windows surface work. Keep changes aligned with that
boundary.

## Architecture Direction

- Prefer Rust/native preview logic for file probing, metadata extraction,
  structured preview data, bounded parsing, archive/package inspection, text,
  folder/listing, Office approximation, professional-format headers, and safe
  format detection.
- Keep .NET App code focused on WinUI presentation, preview lifecycle, keyboard
  and tray integration, accessibility, window behavior, and dispatching already
  structured preview data.
- Keep RasterHost scoped to surface production only: image/PDF raster upload,
  shared D3D surfaces, Windows PDF rendering, shell thumbnails, and other
  Windows-only raster integrations.
- Do not reintroduce WebView/WebView2 for preview rendering.
- Do not add new default-path .NET preview plugins. Legacy `Plugin.*` source is
  reference material only and must stay out of default discovery/publish paths.
- When a feature can be implemented either in Rust or C#, choose Rust unless it
  genuinely requires WinUI, Windows shell UI, AppWindow behavior, or XAML
  controls.

## Current High-Priority Work

1. Image performance and stability
   - Keep static images on the RasterHost path; avoid diagnostic App-side image
     bypasses unless explicitly debugging a crash.
   - Keep first-paint raster budgets conservative; decode preview surfaces at
     screen-sized quality before loading slower sidecars.
   - Push cancellation/epoch checks into Rust/native image decode so quick file
     switching stops CPU work earlier.
   - Keep thumbnail filmstrip loading prioritized around the current image and
     nearby siblings.
   - Keep thumbnail caches bounded with LRU eviction for large folders.
   - Prefetch previous/next images in a cancellation-aware way.
   - Keep EXIF metadata reads bounded and cancellable so slow property handlers
     cannot block preview close or switching.

2. PDF performance
   - Add a bounded 3-5 page surface LRU for recently rendered PDF pages.
   - Keep PDF page rendering cancellable when the preview switches or closes.
   - Keep Windows.Data.Pdf page rasterization inside RasterHost unless a Rust
     metadata/cache layer is being added around it.

3. Office approximate rendering
   - Improve PPTX/XLSX/DOCX layout fidelity in Rust, but do not pretend to be a
     full Office engine.
   - Prioritize PPTX text boxes, images, backgrounds, simple shapes, and z-order.
   - Prioritize XLSX column width, row height, merged cells, frozen panes, and
     number formats.
   - Prioritize DOCX headings, sections, paragraphs, images, and tables.
   - For complex files, present honest summary plus approximate layout and
     metadata instead of fake full fidelity.

4. Preview UI maintainability
   - Keep moving presentation islands out of `MainWindow.xaml.cs`.
   - Prefer focused presenters such as `ExifPreviewPresenter`, listing, text,
     Office, PDF, media, raster, and future image sidecar presenters.
   - Do not dynamically create interactive controls when a stable XAML control
     can be declared instead. This prevents resource lookup and CoreMessaging
     crash regressions.

5. Professional formats
   - Add bounded, Rust-native metadata previews before richer renderers.
   - Prioritize real user frequency: media metadata, fonts, SQLite/database
     headers/schema summaries, ELF/minidump diagnostics, CHM/Mail headers.
   - Avoid unbounded extraction, embedded script execution, or WebView fallback.

## Recent Stability Decisions

- EXIF Google Maps support uses a static XAML button and `ExifPreviewPresenter`.
- EXIF coordinates inside mainland China are automatically converted from WGS84
  to GCJ-02 before opening Google Maps; non-China coordinates remain unchanged.
- Do not add a user-facing switch for that correction unless requirements change.
- Dynamic EXIF action creation and resource lookups in row rendering caused
  native WinUI/CoreMessaging crash risk and should not return.

## Verification

Use the long-cycle harness for repeatable improvement loops:

```powershell
powershell -ExecutionPolicy Bypass -File tools\harness-long-cycle.ps1 -AllowDirty
powershell -ExecutionPolicy Bypass -File tools\harness-long-cycle.ps1 -Mode full -AllowDirty
```

See `docs/long-cycle-harness.md` for the workflow and dirty-worktree policy.

The harness wraps the main checks below; focused commands are still useful while
developing a single parser or smoke case.

Run focused checks after each relevant change:

```powershell
dotnet build QuickLook.Next.slnx -c Debug --no-restore
powershell -ExecutionPolicy Bypass -File tools\smoke-native.ps1
powershell -ExecutionPolicy Bypass -File tools\smoke-exif-map.ps1
powershell -ExecutionPolicy Bypass -File tools\guard-architecture.ps1 -SkipDist
cargo test --manifest-path native\quicklook_next_native\Cargo.toml
```

Run the full release-oriented set before review or publish:

```powershell
cargo build --release --manifest-path native\quicklook_next_native\Cargo.toml
dotnet build QuickLook.Next.slnx -c Release
```

## Guardrails

- Use bounded reads for every archive/package/Office/media extraction path.
- Preserve generation/cancellation guards when work crosses threads or FFI.
- Avoid UI thread synchronous waits and long filesystem enumeration.
- Keep startup background work minimal; RasterHost should remain lazy.
- Prefer precise smoke tests for each crash fix or parser boundary.
- Update `docs/review-readiness.md` when a review-facing architecture decision
  or known limitation changes.
