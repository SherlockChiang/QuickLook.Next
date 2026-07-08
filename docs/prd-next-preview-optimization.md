# PRD: Next Preview Optimization

## Summary

QuickLook.Next has completed the recent Rust-first Office and metadata preview
route. The next product phase should shift away from fine-grained Office parsing
and toward the preview paths that most affect everyday perceived quality:

- Faster first paint for image/raster previews.
- More reliable cancellation when users switch files or press Space.
- Better PDF scroll/render behavior.
- Better shell thumbnail scheduling.
- Broader native metadata for common non-Office formats.
- Clearer bounded-preview messaging when a file is only partially summarized.

Office preview remains supported, but it should now be treated as approximate
and maintenance-mode unless a small bounded improvement clearly helps file
identification.

## Background

The previous route delivered useful Office coverage:

- PPTX: text, paragraph boundaries, tabs, breaks, bullets, alignment hints,
  background color, images, simple shapes, placeholder types, z-order, and basic
  text styles.
- XLSX: sheet layout, row/column sizing, merged cells, freeze panes, number
  formats, fill colors, font colors, bold, italic, font size, alignment, and
  wrap text.
- DOCX: headings, paragraphs, tables, page breaks, section breaks, headers,
  footers, media summary, and basic list prefixes.

This is enough for QuickLook-style identification. Continuing into grouped shape
transforms, theme inheritance, exact Word numbering, inline floating image
layout, formula calculation, charts, or conditional formatting would move the
project toward partial Office-engine behavior. That does not match the product
goal.

## Product Goal

QuickLook.Next should make it fast and safe to identify a file without opening
the source application.

The preview should answer:

- What kind of file is this?
- Is this the file I am looking for?
- What are the most important visible contents or metadata?
- Can I decide whether to open it in the native app?

The preview should not attempt exact reproduction of complex authoring formats
when that exactness costs startup latency, parser complexity, or architecture
clarity.

## Non-Goals

- Build a pixel-perfect Office renderer.
- Execute arbitrary SQL for database previews.
- Extract archives unboundedly.
- Reintroduce WebView/WebView2 preview rendering.
- Restore default-path `.NET Plugin.*` discovery.
- Add long-running UI-thread work to improve fidelity.

## User Problems

### Slow Or Unresponsive Preview Switching

Users expect QuickLook behavior: tap Space, see a preview quickly, tap Space or
arrow to switch, and the app responds immediately. Any stale decode, render, or
thumbnail work should stop affecting the current preview.

### Large Files And Complex Formats Need Honest Summaries

For large images, videos, archives, databases, PDFs, and professional formats,
users need bounded summaries that are useful without pretending to be complete.
Partial results should be labeled clearly.

### Common Non-Office Files Need Better Native Metadata

The current route improved media, font, SQLite, and Office metadata. The next
valuable feature coverage is broader metadata for images, archives, PDFs,
professional binaries, mail, and additional media containers.

## Requirements

### R1: Keep Preview First Paint Fast

The first visible preview should appear as soon as enough data is available.
Expensive side work must be delayed, cancellable, or skipped when stale.

Acceptance criteria:

- Image preview favors screen-sized first render over full-fidelity background
  work.
- Metadata and filmstrip work do not block first paint.
- Slow native decode paths are cancellable before major CPU or copy phases.

### R2: Improve Cancellation And Switching Responsiveness

Preview lifecycle should treat pending opens as cancellable active work.

Acceptance criteria:

- Pressing Space during a slow open cancels pending preview work.
- Switching files discards stale native decode, PDF render, shell thumbnail, and
  metadata results.
- Delayed Explorer activation or focus work is canceled when close arrives.

### R3: Improve PDF Scroll Performance

PDF page rendering should avoid churn and keep visible pages responsive.

Acceptance criteria:

- Page render state distinguishes requested, rendering, rendered, and released.
- Recently rendered pages are kept in a small bounded LRU.
- Renders for pages no longer visible can be canceled.

### R4: Improve Shell Thumbnail Scheduling

Shell thumbnail work should not overload STA workers or apply stale results.

Acceptance criteria:

- Thumbnail requests are generation-aware before entering the STA queue where
  possible.
- Current and nearby files are prioritized over far folder entries.
- Stale thumbnail results are dropped early.

### R5: Expand Native Metadata Coverage

Metadata preview should improve high-frequency file identification without
relying on fragile shell/property-handler paths.

Candidate additions:

- Images: PNG text chunks, WebP EXIF/XMP, TIFF headers, animation summary,
  ICC/bit-depth/alpha/orientation.
- Media: Ogg/Opus/Vorbis summaries, bitrate estimates, MP4/MOV rotation and
  creation time, codec label normalization.
- Archives: encrypted entry detection, top-level grouping, file type counts,
  largest entries, project root detection.
- Fonts: OS/2 table, cmap script coverage, variable font axes, WOFF metadata XML.
- Professional formats: minidump streams, ELF deps/build ID, PE imports/version
  info/signing presence, CHM header details.

Acceptance criteria:

- Parsers are bounded.
- Malformed data fails safely.
- Partial summaries are explicit.
- New parser behavior has focused tests.

### R6: Improve Partial Result Communication

When bounded parsing cannot cover the whole file, the preview should say so.

Acceptance criteria:

- SQLite schema and row count previews indicate partial observations.
- Archives indicate when listing is truncated.
- Media and metadata parsers avoid implying complete stream analysis when only
  header/prefix data was inspected.

## Priority Plan

### P0: Responsiveness And Cancellation

- Treat pending preview transitions as active/cancellable.
- Make Space close/cancel win over slow opens.
- Audit stale result application across decode, metadata, PDF, and thumbnails.

Expected impact: highest perceived performance improvement.

### P1: Image/Raster First Paint

- Tune native image decode budgets for first paint.
- Prefer screen-sized decode first.
- Keep EXIF/filmstrip side work delayed and cancellable.
- Add phase diagnostics for decode, resize, upload, metadata, and sidecars.

Expected impact: faster common-path previews and easier performance diagnosis.

### P1: PDF Render Cache And Scroll Stability

- Add bounded page surface LRU.
- Coalesce rapid page requests.
- Cancel obsolete renders.

Expected impact: smoother PDF scroll and fewer wasted renders.

### P2: Shell Thumbnail Queue Discipline

- Prioritize current/nearby thumbnails.
- Cap concurrent shell thumbnail work.
- Avoid STA queue buildup with stale generations.

Expected impact: better folder/image browsing responsiveness.

### P2: Metadata Breadth For Non-Office Files

- Add image metadata formats beyond JPEG.
- Add archive/project summary improvements.
- Add Ogg/Opus/Vorbis and media bitrate estimates.
- Add PE/ELF/minidump professional summaries.

Expected impact: more useful previews without heavy rendering work.

## Functional Backlog

### Images

- PNG metadata: dimensions, color type, bit depth, alpha, text chunks, ICC.
- WebP metadata: canvas, animation flag, frame count if cheap, EXIF/XMP chunks.
- TIFF summary: dimensions, orientation, camera/date fields where bounded.
- Animated image summary: GIF/WebP/APNG first frame, frame count, duration.

### Media

- Ogg container summary.
- Opus/Vorbis identification headers.
- Bitrate estimates from file size and duration.
- MP4/MOV rotation and creation time.
- Human-readable codec labels for common codec IDs.

### Archives

- File type counts.
- Top-level folder summary.
- Largest entries.
- Encrypted ZIP entry warning.
- Project root detection.

### Databases

- Index/trigger grouping in SQLite schema summary.
- Clear per-table partial row count messaging.
- Optional raw page-level sample decoding only if bounded and safe.

### Fonts

- OS/2 metadata.
- cmap script coverage.
- Variable font axes from `fvar`.
- WOFF metadata XML summary.

### Professional Formats

- Minidump stream directory and exception summary.
- ELF interpreter, build ID, dynamic dependencies.
- PE imports, version info, certificate presence.
- CHM ITSF/ITSP summaries.
- Mail MIME part and attachment summaries.

## Performance Backlog

### Preview Lifecycle

- Pending preview cancellation.
- Close-before-open completion safety.
- Stale result auditing.
- Generation propagation across native and managed boundaries.

### Native Decode

- Cancellation checks before decode, resize, conversion, and final copy.
- More precise timeout behavior.
- Screen-sized first decode mode.

### PDF

- Bounded page LRU.
- Render state machine.
- Request coalescing.
- Cancellation of invisible pages.

### Thumbnailing

- Prioritized thumbnail queue.
- Generation-aware shell queue entry.
- Bounded concurrency.
- Early stale discard.

### Diagnostics

- Phase timings: probe, native parse, decode, raster upload, PDF render,
  metadata, filmstrip, presentation.
- Clear partial/truncated labels in preview text.
- Optional debug traces that can be enabled without changing preview behavior.

## Architecture Constraints

- Prefer Rust/native preview logic for parsing and metadata.
- Keep App focused on WinUI presentation, lifecycle, keyboard, and window logic.
- Keep RasterHost scoped to image/PDF/shell thumbnail/surface production.
- Keep all parsing bounded.
- Do not add WebView/WebView2 rendering.
- Do not restore default `.NET Plugin.*` discovery.

## Success Metrics

- Faster perceived first paint on large images and PDFs.
- Fewer stale preview flashes during rapid switching.
- Lower wasted work during folder browsing.
- More useful metadata summaries for common non-Office files.
- Fewer large App lifecycle methods and clearer presenter/controller boundaries.

## Decision Log

- Office preview enters maintenance mode after the current baseline.
- Future Office changes must be small, bounded, and identification-focused.
- Performance and cancellation work now outrank fidelity improvements.
- Metadata breadth for non-Office formats is preferred over deeper Office layout.
