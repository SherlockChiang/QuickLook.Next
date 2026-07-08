# Post-Office Roadmap Analysis

This document archives the end of the recent Office/detail-preview improvement
round and resets the next roadmap around higher user-visible value. The main
decision is that QuickLook.Next should not keep investing heavily in fine-grained
Office fidelity. Office preview should remain honest, bounded, approximate, and
fast.

## Product Direction

QuickLook.Next is a fast preview tool, not an Office renderer. The preview should
answer these questions quickly:

- What is this file?
- Is it the file I am looking for?
- What are the important visible contents or metadata?
- Can I decide whether to open it in the native app?

For Office files, that means we should preserve approximate structure, readable
text, common spreadsheet styling, media hints, and basic layout. We should not
chase pixel-perfect Word, PowerPoint, or Excel behavior.

## Completed Office Baseline

The recent route delivered enough Office fidelity for a useful native preview:

- PPTX: slide text extraction, paragraph boundaries, tabs, breaks, bullets,
  alignment hints, background color, images, simple shapes, placeholder type,
  z-order, and basic text style.
- XLSX: sheet layout, row/column sizing, merged cells, freeze panes, number
  formats, fill colors, font colors, bold, italic, font size, alignment, and
  wrap text.
- DOCX: headings, paragraphs, tables, page breaks, section breaks, headers,
  footers, media summary, and basic list prefixes.

This is a good stopping point for Office parsing because the remaining fidelity
work quickly becomes a partial Office engine.

## Office Work To Avoid For Now

These are technically possible but not worth prioritizing unless users report
specific pain:

- PPTX grouped shapes with nested transforms.
- PPTX theme/font inheritance and full rich text runs.
- PPTX exact layout engine behavior for placeholders, auto-fit, text anchoring,
  line spacing, and master/layout inheritance.
- DOCX full numbering model via `word/numbering.xml`.
- DOCX inline image placement, floating images, headers/footers layout, page
  margins, and section page size rendering.
- XLSX formula calculation, conditional formatting, charts, pivot tables, and
  exact Excel date/locale behavior.

If we revisit Office, the rule should be: implement a small bounded feature only
when it materially improves file identification, not visual perfection.

## Better Functional Improvements

### Image Preview

Image preview is still the most important user path. Improvements here likely
have the highest impact.

- Add native metadata coverage beyond JPEG EXIF where practical: PNG text chunks,
  WebP EXIF/XMP, TIFF headers, HEIF/AVIF container metadata if feasible.
- Improve corrupted/huge image behavior with clearer fallback messages and
  thumbnails instead of silent failures.
- Add better animated image handling: GIF/WebP/APNG summary, first frame, frame
  count, duration, and clear animation indicator.
- Improve color information: ICC profile presence, bit depth, alpha, color type,
  and orientation reporting.

### Media Preview

Media preview is now stronger for MP4/MOV/MKV/MP3/FLAC/WAV, but useful gaps
remain.

- Add bitrate summaries using file size and duration when exact stream bitrate is
  not available.
- Add codec normalization so raw codec IDs become readable labels.
- Add rotation/orientation for MP4/MOV video tracks.
- Add creation time for MP4/MOV when available.
- Add Ogg/Opus/Vorbis bounded metadata summaries.
- Add ID3 album art detection as metadata, not full rendering.

### Archive Preview

Archive preview can be more useful without becoming an extractor.

- Add top-level folder grouping and file type counts.
- Highlight likely project roots: `package.json`, `.csproj`, `.sln`, `Cargo.toml`,
  `pyproject.toml`, `go.mod`, `pom.xml`.
- Detect encrypted ZIP entries and show an explicit warning.
- Show compression ratio and largest entries.
- Keep extraction bounded and never execute embedded content.

### Database Preview

SQLite is now useful for headers, schema objects, columns, SQL snippets, and
bounded row counts. Further improvements should stay safe.

- Add index and trigger grouping in the schema summary.
- Show per-table root page and whether the observed row count is partial.
- Add clearer messages when schema pages are outside the bounded prefix.
- Avoid arbitrary SQL execution. If real row samples are ever added, they should
  be page-level raw record decoding with strict bounds, not SQL queries.

### Font Preview

Font metadata now covers common identity and size fields. Useful additions:

- Add OS/2 table summary: weight class, width class, embedding permissions,
  vendor ID, unicode ranges.
- Add cmap coverage summary: detected scripts or Unicode block coverage.
- Add variable font axis summary from `fvar`.
- Add WOFF metadata XML summary if bounded and cheap.

### Professional Formats

These are good candidates for bounded native summaries:

- Minidump: stream directory summary, exception stream, module count, thread
  count, architecture.
- ELF: program headers, section names, interpreter, build ID, dynamic deps.
- PE: imports summary, version info, signing/certificate presence.
- CHM: ITSF/ITSP header details, topic count if available bounded.
- Mail: attachment names/count, MIME part summary, safer MSG header extraction.

## Better Performance Optimizations

### Preview Lifecycle Responsiveness

- Treat pending preview transitions as cancellable active previews.
- Make Space close/cancel behavior win over slow open paths.
- Cancel delayed Explorer activation work when a close arrives.
- Keep all cross-thread preview work generation-aware.

### Raster/Image Path

- Tune image decode budgets for first paint rather than full fidelity.
- Prefer screen-sized decode first, with optional higher-quality work later.
- Keep decode cancellation checks before expensive resize/color conversion and
  before final buffer copies.
- Add precise phase timings for decode, resize, upload, metadata, and sidecars.

### PDF Path

- Add a small LRU for recently rendered PDF page surfaces.
- Keep requested/rendering/rendered/released page state explicit.
- Cancel in-flight renders for pages no longer visible or when preview closes.
- Avoid churn during rapid scroll by coalescing page requests.

### Shell Thumbnail Path

- Make shell thumbnail requests generation-aware before entering the STA queue.
- Drop stale thumbnail results as early as possible.
- Limit concurrent shell thumbnail work to protect responsiveness.
- Keep folder filmstrip prioritization near the current item.

### UI Thread Protection

- Keep App-side bitmap writes small.
- Avoid synchronous waits in preview open/close paths.
- Batch UI additions for large listings or filmstrips.
- Move remaining presentation islands out of `MainWindow.xaml.cs` when they grow.

## Maintainability Direction

The architecture should remain Rust-first with a thin WinUI shell:

- Rust owns bounded parsing, metadata, file probing, preview decisions, and
  structured preview data.
- App owns WinUI presentation, lifecycle, keyboard/window behavior, and dispatch.
- RasterHost owns Windows-only surface production: images, PDF, shell thumbnails,
  D3D surfaces.
- Do not reintroduce WebView/WebView2 preview rendering.
- Do not restore default-path `.NET Plugin.*` discovery.

The most valuable maintainability improvements are:

- Extract preview routing from large App lifecycle methods.
- Keep presenter/controller classes focused and small.
- Expand architecture guards when regressions are discovered.
- Keep parser tests focused on bounded behavior and malformed input safety.

## Recommended Next Priority

The next route should focus on performance and preview lifecycle rather than more
Office fidelity:

1. Image and raster first-paint latency.
2. Preview cancellation and Space key responsiveness.
3. PDF page render cache and scroll churn reduction.
4. Shell thumbnail generation cancellation and prioritization.
5. Broader native metadata for media, fonts, archives, and professional formats.

Office can stay in maintenance mode. Only add Office features when they are
small, bounded, and clearly improve file identification.
