# QuickLook.Next Optimization Roadmap

This document captures the next practical optimization targets after the
Rust-first rewrite work. Items are ordered by user-visible impact and fit with
the current architecture boundary. The newer product-level PRD is
`docs/prd-next-preview-optimization.md`; use that document for priority and
acceptance criteria.

## Highest Priority

1. Rust-native image metadata
   - Move JPEG EXIF/XMP/IPTC reads out of the App-side Windows property handler
     path.
   - Return structured metadata from Rust for dimensions, camera, lens, date,
     orientation, exposure, ISO, focal length, GPS, altitude, and direction.
   - Keep Windows property handlers as a delayed fallback only when Rust cannot
     provide a field.

2. Cancelable native image decode
   - Add a cancel-aware image decode ABI instead of relying only on App-side
     generation checks before and after synchronous FFI calls.
   - Check cancellation before decode, before resize/color conversion, and before
     copying the final BGRA buffer.

3. Image sidecar scheduling
   - Keep first paint isolated from slower side work.
   - Delay EXIF metadata and filmstrip work slightly after the raster is shown.
   - Prioritize current/nearby thumbnails before far folder entries.

4. Space key responsiveness
   - Treat pending preview transitions as an active preview so Space can cancel
     slow opens before the window is fully revealed.
   - Keep delayed Explorer switch work canceled when a close arrives.

## Performance Targets

- PDF page close should cancel in-flight renders for pages that are no longer
  requested, not only release the old surface later.
- PDF request tracking should distinguish requested, rendering, rendered, and
  released pages to avoid churn during rapid scroll.
- Shell thumbnail work should become generation/cancel aware before entering the
  STA worker queue where possible.
- Image decode timeouts should be tuned for QuickLook interaction: prefer fast
  first paint or thumbnail fallback over waiting several seconds for a full
  decode path.
- App-side bitmap writes and thumbnail batches should stay small enough to avoid
  UI-thread stalls.

## Feature Coverage Targets

- Media metadata: bounded MP4/MOV/MKV/MP3/FLAC/WAV container summaries with
  duration, codec, resolution, bitrate, rotation, and creation time.
- SQLite/database preview: header, schema summary, table list, columns, and
  bounded row counts without executing arbitrary user SQL.
- Font metadata: TTF/OTF/WOFF/WOFF2 family, style, version, glyph count, naming,
  and license summaries.
- Mail/CHM/minidump: prioritize bounded headers and diagnostics before richer
  extraction.

## Office Targets

Office preview is now in maintenance mode. Keep it approximate, bounded, and
identification-focused. Avoid turning QuickLook.Next into a partial Office
renderer.

- PPTX: parse placeholder type (`title`, `subtitle`, `body`), basic text style,
  bullet level, alignment, grouped shapes, and z-order.
- XLSX: parse `styles.xml` number formats so dates, currency, percentages, and
  common numeric formats preview honestly.
- DOCX: improve headings, sections, paragraphs, images, and tables before
  attempting richer layout fidelity.

## Maintainability Targets

- Continue moving image sidecar logic out of `MainWindow.xaml.cs` into focused
  presenters/controllers.
- Extract preview routing from `PreviewPathAsync` so App code dispatches already
  structured preview decisions instead of owning routing policy.
- Keep diagnostics precise enough to explain slow previews by phase: probe,
  native parse, RasterHost decode, metadata, filmstrip, and presentation.

## Guardrails

- Do not reintroduce WebView/WebView2 preview rendering.
- Do not add default-path .NET preview plugins.
- Keep all archive, Office, media, and professional-format parsing bounded.
- Prefer Rust for preview decisions and structured data unless the work genuinely
  requires WinUI, Windows shell UI, AppWindow behavior, XAML controls, or Windows
  raster APIs.
