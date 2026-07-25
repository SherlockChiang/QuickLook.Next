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
- Native Reader and XML preview boundaries are hardened:
  - ABI 2 text, executable, torrent, and SQLite snapshot previews accept authenticated ParserHost
    disk-file handles directly, validate exact lengths, and reopen them with independent file
    positions before Rust reads them.
  - Plain text, Markdown, CSV, TSV, executable metadata, torrent listings, and database previews no
    longer create ParserHost input anchors or reopen the logical source path.
  - Executable reads remain capped at a cancellable 4 MiB prefix. Torrent reads are exact and
    cancellable with a 16 MiB cap before the existing depth-64/node-100000 bencode limits.
  - SQLite uses the dedicated `PreviewOpenSqliteHandles` IPC envelope and
    `ql_preview_sqlite_handles` ABI entry point. The host adopts the main/WAL/SHM slots before
    validation and never creates an input anchor. Only the App derives `-wal`/`-shm` sibling names.
  - SQLite main-prefix parsing is capped at 1 MiB, WAL input at 64 MiB, and SHM input at 4 MiB. SHM
    is diagnostic only. WAL overlay validates header/frame salts and rolling checksums, stops at the
    first invalid frame, and applies only frames through the last valid commit marker.
  - Local SQLite files and existing companions are pinned read-only with `FILE_SHARE_READ` only.
    Sharing violations fail closed; only a missing optional companion is treated as absent.
  - The HANDLE ABI keeps stable capability bits 0-3 and status codes through
    `LIMIT_EXCEEDED == -9`, exact output-size negotiation, panic containment, capability detection,
    and direct invalid-handle/file-position tests.
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
- Specialized/professional formats are covered in frequency order with bounded
  native metadata previews: fonts, SQLite/database headers, media container
  info in the playback chrome, ELF/minidump diagnostics, and safe Mail/CHM
  header previews.
- The EXIF location crash investigation is resolved at the UI boundary: the
  Google Maps action is a static XAML control instead of a dynamically-created
  button/resource lookup during EXIF row rendering. Coordinates inside mainland
  China are automatically converted from WGS84 to GCJ-02 before opening Google
  Maps; non-China coordinates are left unchanged.
- EXIF side-panel state/rendering is isolated in `ExifPreviewPresenter`, leaving
  `MainWindow` responsible for preview lifecycle and metadata loading rather
  than row/control construction.

## Verification Commands

Run these from the repository root:

```powershell
cargo test --locked --manifest-path native\quicklook_next_native\Cargo.toml
cargo build --release --locked --manifest-path native\quicklook_next_native\Cargo.toml
dotnet test tests\QuickLook.Next.Core.Tests\QuickLook.Next.Core.Tests.csproj -c Release
dotnet test tests\QuickLook.Next.ParserHost.IntegrationTests\QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release
pwsh -NoProfile -File tools\smoke-native.ps1
pwsh -NoProfile -File tools\smoke-exif-map.ps1
dotnet build QuickLook.Next.slnx -c Release
pwsh -NoProfile -File tools\guard-performance-bounds.ps1
pwsh -NoProfile -File tools\guard-format-registry.ps1
pwsh -NoProfile -File tools\guard-stale-callbacks.ps1
pwsh -NoProfile -File tools\guard-architecture.ps1 -SkipDist
git diff --check
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

- Continue improving Office approximate layout fidelity. This is intentionally
  not a full Office rendering engine: PPT/XLSX should prioritize slide/sheet
  layout, text positions, table/cell geometry, relationships, and embedded
  images. Full style parity, macros, animations, formula recalculation, and
  complete Office compatibility remain out of scope for the default preview
  boundary.
- Expand real-world smoke assets for larger PDFs, malformed archives, unusual
  APK/MSIX manifests, mixed-encoding text files, and complex Office files.
  Current smoke coverage includes UTF-16 text and corrupt ZIP fail-closed checks,
  but still needs more externally sourced real-world files.
- Push cancellation deeper into Rust/native decode/listing loops. The App now
  prevents stale merge/update work, but native FFI calls are still synchronous
  once entered.
- Keep improving the primary image path:
  - Thumbnail filmstrip loading should keep prioritizing the current image and
    nearby siblings, with LRU eviction to bound memory in large folders.
  - Adjacent image prefetch should remain cancellation-aware so quick Explorer
    selection changes do not keep decoding old images.
  - Large image decode should eventually accept a native cancellation/epoch
    signal, not just an App-side generation guard.
  - EXIF metadata reads should keep timeout/cancellation boundaries so slow
    property handlers cannot stall preview close or file switching.
- PDF surface caching is intentionally small today. A bounded 3-5 page LRU for
  recently rendered pages would make scroll-back smoother without returning to
  unbounded GPU surface retention.
- Continue Shell icon coverage for virtual archive entries if a stable file type
  icon can be resolved without pretending the virtual item is a real path.
- Deep professional parsers remain intentionally staged: full MediaInfo tracks,
  CHM topic extraction, Outlook MSG property streams, and database schema
  browsing should only be added with bounded parsers and no WebView fallback.
- Continue the HANDLE ABI migration only after each reader accepts a bounded `Read` or `Read + Seek`
  input. SQLite main/WAL/SHM snapshots now have an explicit multi-handle boundary; archive,
  Office/ebook, and raster formats still require broader adapters.

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
