# QuickLook.Next Review Readiness

This note is a reviewer-facing status page. It records what has already been
hardened, how to verify it locally, and which limitations are intentionally
left visible instead of hidden behind vague TODOs.

## Current Verification And Priority

The active order is the October external-review follow-up, followed by the
remaining September evidence-first queue in
[`implementation-todo.md`](implementation-todo.md). The complete local
`tools/release.ps1 -SkipPackage` sequence passed on 2026-09-05 at `8c3d732`
using pinned SDK `10.0.302`: Rust 343/1 plus explicit external-corpus smoke,
Core 351, ParserHost 45, RasterHost 29, and ShellBroker 13 tests. The full
architecture sequence, including the previously blocked MSBuild fixture,
restricted-host launch, and image corpus checks, completed successfully.
See [`verification-baseline.md`](verification-baseline.md) for the exact SHA,
log hash, optional JXL limitation, and checks not performed.

That is a local source/build baseline, not a new signed package, manual UI
acceptance, remote CI pass, or Store certification. Published `v1.0.1` and the
latest verified remote CI remain at `a64b60c`; subsequent local changes are not
released. Measure input/first-paint/cancellation latency and resource recovery
before resuming broad module extraction or adding format fidelity.

The September 2026 HDR follow-up adds deterministic PQ/scRGB selection,
generation-tagged display refresh, HLG SDR fallback, and bounded hdr decision
diagnostics. See [hdr-validation-matrix.md](hdr-validation-matrix.md).
Release build/Core/RasterHost focused verification passed; physical HDR-panel
and visual-golden acceptance remain pending. An earlier run failed to create
the restricted token (CreateRestrictedToken, Win32 87). The full local architecture
sequence and restricted-host launch smoke passed on 2026-10-12; that earlier
failure remains historical evidence rather than a current verified blocker.

## Fixed And Hardened

- Rust-first preview path: text, folders, archives, packages, mail, certificates,
  executables, torrents, and lightweight Office previews are handled by the
  native layer rather than the legacy .NET plugin pipeline.
- ParserHost targets `net10.0-windows` for its Win32/FFI transport, without
  Windows SDK or CsWinRT projection dependencies. Its output/test/release paths
  use that framework independently of App/RasterHost. The current non-PDB host
  payload is 24.23 MiB smaller; stale-output selection and restricted-host
  launch are covered. See `O26-04` and `verification-baseline.md` for evidence.
- Ordinary native CI tests include bounded reader properties for ZIP,
  Torrent/bencode, CFB/MSG and SQLite/WAL. Synthetic valid/mutated inputs check
  fragmented reads, typed length rejection, cancellation during actual I/O,
  fresh previews after cancellation and bounded JSON. Positive properties check
  extraction capacity, names/sizes, UTF-16 subjects and committed WAL pages.
  See CONTRIBUTING.md for seeds, budgets, replay and extended runs. Broader
  coverage-guided fuzzing and sanitizer execution remain open under `R26-P1-10`.
- The native crate compiler-enforces explicit unsafe operations and Clippy safety
  explanations across root, FFI, Win32 and tests, while preview modules forbid
  unsafe code. Raw-pointer packet writers and retained callback registration
  require unsafe Rust callers with alignment, disjointness and lifetime contracts.
  Compiler fixtures accept a documented call and reject five invalid uses of the
  actual attributes/writer modules. COM workers own thread-affine initialization;
  cached interfaces drop before the apartment on normal and unwinding exits.
  Managed intent callbacks contain exceptions; empty legacy output returns before
  copying to a null pointer. These checks do not establish foreign pointer
  provenance or eliminate the remaining fuzz/sanitizer work.
- The native preview implementation is being split by bounded format family: shared DTO/common
  helpers, folder listing, Text/Markdown/CSV/TSV, JPEG/PNG/GIF/WebP/TIFF image metadata,
  GIF/WebP/APNG animation classification, Torrent/bencode, executable/PE/CLR/AuthentiCode,
  MIME/mail plus seek-based Outlook CFB, EPUB/FB2 ebook parsing, media, archive, Office, package,
  and database families now live in focused submodules with their own tests. CHM, dump, and ELF
  have focused modules as well. Most path/HANDLE FFI families are isolated, including static
  image and waveform HANDLE adapters; animation ABI, decode implementation, and Explorer/hook
  ownership still share the native root. Schema v2 generates ABI constants and
  compile-time checks for all 68 project C exports and three callback types.
  Version/capability getters are constant-evaluated and runtime-tested against
  exact schema values. Function implementations and managed declarations remain
  handwritten. Schema bindings cover 67 P/Invokes in seven owners across App,
  ParserHost, RasterHost and ShellBroker, plus eight unmanaged callback
  definitions. Release metadata is checked without loading target assemblies,
  including fixed-arity exact-name Cdecl imports, PreserveSig, byte-array In/In-Out and
  scalar Out directions, native-sized types and one-byte callback bool returns.
  Eight raw callback parameter positions have explicit delegate mappings;
  these verify declaration compatibility, not the actual pointer value/lifetime.
  Shared raw-pointer adapter behavior is checked by an executable probe over the
  real `ffi::common`, `ffi::raster_packet`, `ffi::file_probe`,
  `ffi::thumbnail`, and `ffi::path_preview` modules, including canaries,
  status/length semantics, packet bytes, path errors, text classification,
  thumbnail typed errors/cancellation, and panic containment. Observable
  substitutes check all twelve path exports, argument/renderer forwarding,
  pre/post-render cancellation, optional kind fallback and panic recovery;
  actual format parsers remain independently covered by Rust workspace tests.
  A separate HANDLE probe compiles the real `ffi::handle_preview`,
  `ffi::handle_container`, `ffi::handle_structured` and `native_input` modules
  with actual Win32 reopen ownership. Observable provider substitutes cover the
  four container and four structured exports'
  exact renderer/name/length/mtime/callback forwarding, all typed statuses,
  cancellation, canary-preserving output negotiation and panic recovery;
  real format-parser limits remain independently tested. Text-empty I/O versus
  cancellation is checked with a one-shot poll callback so the shared post-render
  cancellation check cannot mask an incorrect inner result.
  The shared/path gates require 93 compiled mutations to fail, and the HANDLE
  boundary gate adds 76 compiled mutations (169 total across the current FFI
  behavior fixtures). The former helper test-name/line-budget, file-probe and
  thumbnail test-name, path-preview
  test-name/child-test-line-budget, HANDLE test-name/line-budget, and container
  pre-cancellation-test-name/line-budget/reader-status-call-shape checks, and
  structured pre-cancellation-test-name/line-budget/mail-forwarding rules were
  removed after coverage was added; independent implementation bounds remain
  where applicable. Path-preview, container and structured unit tests,
  module ownership and first-statement panic-boundary rules are retained.
  Wider source-guard migration remains `O26-06c`.
  The built win-x64 DLL export table is inspected without loading the DLL,
  including an explicit separate inventory of 19 incidental qcms 0.3.0 symbols.
  They are not supported QuickLook ABI entry points; unknown, missing, forwarded,
  unnamed exports or ordinal aliases fail the guard. PE names alone do not prove signatures.
- Syntax-highlight tokenization lives in the native layer: `ql_highlight_spans` returns bounded
  UTF-16-offset token spans for the managed text presenter to slice, with caps on input bytes and
  span count, and the App degrades to plain rendering if the native call is unavailable.
- RasterHost is lazy-started and scoped to surface-producing work: images, PDF
  page rasterization, shell thumbnails, and fallback media/image surfaces.
- Supervised RasterHost, ParserHost, and ShellBroker processes apply a shared
  process-wide error policy before any argument parsing or native initialization.
  They request WER no-UI reporting and fail closed against interactive system
  error dialogs with the process no-GP-fault-box mode. That guarantee takes
  priority over best-effort WER/local-dump capture; the RasterHost supervisor
  still records the exact decimal/hex exit code before scheduling a restart.
- Preview errors expose a generation-bound Copy Diagnostics action. It copies a
  sealed, maximum-1024-character JSON snapshot containing only a stable error
  code, lifecycle phase, correlation ID, application version, normalized format,
  and coarse size bucket. Paths, file names, localized messages, titles, and file
  contents are not inputs to the schema.
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
  - ParserHost connects/authenticates its pipe before native ABI initialization. Supervisors use a
    15-second cold-start/ready budget and require the generation's ready task to complete before
    reuse, preventing idle prewarm from starting a real JSON request's five-second timer early.
  - ABI 2/3 text, executable, torrent, mail, SQLite snapshot, archive, ebook, and archive-entry previews
    accept authenticated ParserHost disk-file handles directly, validate exact lengths, and reopen
    them with independent file positions before Rust reads them.
  - Plain text, Markdown, CSV, TSV, executable metadata, torrent listings, mail metadata,
    database previews, archive listings, and ebook previews no longer create ParserHost input anchors or reopen the
    logical source path.
  - Executable reads remain capped at a cancellable 4 MiB prefix. Torrent reads are exact and
    cancellable with a 16 MiB cap before the existing depth-64/node-100000 bencode limits.
  - Mail uses `ql_preview_mail_handle` and capability bit 21. EML/MIME retains its cancellable
    256 KiB prefix, while Outlook MSG uses checked, on-demand CFB sector seeks against the exact
    source length. FAT/DIFAT/directory/mini-stream/property limits remain explicit and all physical
    CFB reads share a 1 MiB cumulative budget, so valid properties beyond 256 KiB remain visible
    without buffering the complete message.
  - Lazy PDF text search/copy uses `ql_extract_pdf_text_handle` and capability bit 22 over a
    reopen of the exact pinned PDF object. Rust owns page mapping and hard input, page,
    decompression, object, per-page text, and aggregate text bounds; malformed work remains inside
    the supervised 512 MiB ParserHost and does not delay raster first-page rendering.
  - SQLite uses the dedicated `PreviewOpenSqliteHandles` IPC envelope and
    `ql_preview_sqlite_handles` ABI entry point. The host adopts the main/WAL/SHM slots before
    validation and never creates an input anchor. Only the App derives `-wal`/`-shm` sibling names.
  - SQLite main-prefix parsing is capped at 1 MiB, WAL input at 64 MiB, and SHM input at 4 MiB. SHM
    is diagnostic only. WAL overlay validates header/frame salts and rolling checksums, stops at the
    first invalid frame, and applies only frames through the last valid commit marker.
  - Local SQLite files and existing companions are pinned read-only with `FILE_SHARE_READ` only.
    Sharing violations fail closed; only a missing optional companion is treated as absent.
  - Published direct HANDLE archive listings retain their owning source handle by parent preview
    request ID. This includes an EPUB presented as an archive when its already-open ZIP reader has
    no usable OPF. Entry extraction resolves that parent before the legacy path fallback and calls
     the dedicated archive-entry HANDLE ABI. Parent close, failed publication, replacement, and
     disconnect dispose retained owners and reject new leases; an in-flight extraction keeps its
     independently reopened lease until completion. Normal content ebook previews remain stateless.
  - Archive HANDLE inputs use a 16 TiB logical envelope because the readers seek over payload bytes
    instead of buffering the source. ZIP container validation limits central-directory work to
    32 MiB and declared entries to 100,000 before listing scans apply their existing
    10,000-record/5,000-item bounds. TAR/TGZ scans retain their 512 MiB decompressed-read,
    four-second, and cancellation limits. RAR4/RAR5 is a header-only, CRC-validating scanner with
    2 MiB per-header, 10,000-header, four-second, and 5,000 represented-item bounds. It performs
    checked `u64` seeks; caps normalized paths at 1,024 UTF-8 bytes/128 components and aggregate
    represented path strings at 2 MiB; never decompresses payloads; and publishes a browse-only listing.
    RAR4 legacy names preserve valid UTF-8 and use a deterministic Windows-1252 fallback when the
    byte sequence is not UTF-8; the same fallback applies when the optional Unicode name tail is unusable.
    The listing summary labels this boundary, and activating a RAR entry explains that entry content
    cannot be opened instead of failing silently.
    ZIP entry extraction retains 64 MiB compressed/uncompressed caps, a 1,000:1 expansion-ratio
    cap, and a four-second deadline; RAR entry extraction fails closed.
  - Ebook HANDLE inputs are capped at 256 MiB. EPUB processing limits central-directory work to
    32 MiB, ZIP entries to 8,192, cumulative decompression to 16 MiB, metadata XML to 2 MiB, chapter
     input to 768 KiB, retained chapters to ten, and retained text to 140 Ki characters. Missing or
     unusable OPF data reuses that same validated ZIP reader to publish a bounded archive listing with no root
    path; it never reopens the logical name through the path-based archive renderer.
  - The HANDLE ABI keeps stable capability bits for text (0), executable (1), torrent (2), SQLite
     snapshot (3), archive (4), Office (5), ebook (6), archive entry (7), static ICO (8), SVG (9),
     GIF static/animation (10), package preview (11), package icon extraction (12), and final local
     HANDLE probe (13), general raster image input (14), general GIF/WebP/APNG animation (15),
     Office layout image decode (16), Rust image waveform packets (17), caller-owned archive
     entry output (18), optional retained-HANDLE image metadata (19), optional direct GIF output
     (20), and ParserHost mail input (21).
     Bit 8 remains the published ICO-only static-image capability; bit 14 gates
     PNG/JPEG/BMP/TIFF/WebP native HANDLE fallback. Bit 15 is additive and optional for ABI 3
     consumers; the stable GIF export and bit 10 remain available as a compatibility fallback.
     Bit 17 is additive so WIC/system decoders can retain their bounded managed waveform fallback.
     Bit 18 is required by ParserHost and streams archive children directly into an App-owned
     zero-length output object.
     Bit 19 is optional for RasterHost and gates a parent-bound metadata sidecar without making an
     older ABI 3 raster DLL unusable.
     Bit 20 is optional for RasterHost; bit 21 is required by ParserHost before it calls the mail
     HANDLE entry point.
     Implemented HANDLE
     exports retain status codes through
    `LIMIT_EXCEEDED == -9`, exact output-size negotiation, panic containment, capability detection,
    and direct invalid-handle/file-position tests.
  - UTF-8 text preview truncation backs up to a valid char boundary.
  - UTF-16 BOM text truncation avoids dangling half code units.
  - Office preview text truncation is char-boundary safe.
  - Office hero and package icon raster candidates are rejected before full decode when either
    dimension exceeds 8,192 pixels or the source exceeds 16 million pixels. XHTML and FB2 output
    limits use incremental character accounting rather than rescanning the full output per XML event.
  - Local certificate previews read at most 1 MiB directly from the transferred file object and use
    the strict single-certificate DER/PEM loader without creating a ParserHost input anchor.
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
- Folder/listing previews use reusable theme-aware multi-layer vector folder/archive icons in both
  rows and Hero surfaces, while asynchronously replacing real filesystem rows with Shell
  thumbnail/icon cache images when available.
- Virtual archive entries use those multi-color vector fallbacks plus extension-aware glyphs for
  common images, media, archives, Office documents, code/text files, installers, certificates,
  torrents, and disk images.
- Native animated-frame presentation writes into the fixed WinRT pixel buffer without resizing it
  and releases the buffer stream before invalidation. GIF, animated WebP, and APNG now decode through
  retained HANDLE leases without reopening the logical path. Rust publishes tri-state animation
  metadata so confirmed static files are skipped while unknown metadata remains compatible with
  older native binaries. The animation follow-up uses a separate 20-second timeout and preserves
  the static preview if decoding times out. Rust writes the bounded frame packet directly into an
  unnamed RasterHost section; the App duplicates only `SECTION_MAP_READ`, validates the mapped
  packet once, retains that view for playback, and acknowledges the remote owner. It stores only
  delay/offset descriptors and writes each mapped frame span directly to `PixelBuffer`; no per-frame
  `byte[]` or `ToArray()` remains. Waveform reads and unmapping share one lifetime lock. The former
  `raster-animation` packet file is gone.
- App-process Windows Property Handler reads have been removed. Image metadata is now an optional
  RasterHost child bound to the exact retained image request. It runs bounded Rust metadata, the
  fixed System32 photo Property Handler over a read-only `IInitializeWithStream`, and WIC over
  independently reopened HANDLE streams in parallel, merging fields as
  `native > Property Handler > WIC`. The child carries no path, does not delay the first surface,
  may finish after parent close through its independent lease, and is canceled/drained on child
  close or disconnect. The Property Handler is activated directly from System32 without user COM
  registration; a provider that misses the 1.5-second budget and cannot drain in 250 ms fail-stops
  RasterHost. App `StorageFile.Properties`, `IInitializeWithFile`, parsing-name stores, and logical
  path fallback remain forbidden.
- Office and package Hero extraction use the same anonymous-section ownership contract in
  ParserHost. The App maps the exact bounded packet read-only and copies only its validated BGRA
  payload; close, publication failure, Host replacement, and disconnect release the owner. The
  former `parser-raster` handoff directory is no longer created.
- ShellBroker thumbnail handoffs release their broker-owned HANDLE and packet directory on explicit
  `CLOSE` and on abrupt pipe EOF. Channel teardown tolerates the expected broken-pipe flush while the
  independently duplicated App HANDLE remains readable.
- ShellBroker responses cross a testable Core parser before reaching App state. Request IDs, invariant
  numeric fields, 512-pixel dimensions, exact BGRA packet lengths, strict UTF-8 errors, and packet
  headers are validated; malformed control output or a bad packet recycles the broker instead of
  leaving an untrusted connection available for another request.
- Folder navigation and listing icon work use the active preview cancellation
  token/generation guard so stale results do not merge into a later preview.
- Autostart now prefers HKCU Run, uses Startup-folder shortcuts only as a
  fallback, and repairs stale QuickLookNext entries that point at an old exe.
- Cloud placeholders require localized user consent before content access. Hydration is bounded by a
  256 MiB application-read policy, a 45-second timeout covering stream open and reads, preview
  cancellation, and generation-guarded progress updates; declined, oversized, timed-out, or failed
  downloads remain metadata-only previews.
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
dotnet msbuild native\QuickLook.Next.Native.proj -target:Build -verbosity:minimal
dotnet test tests\QuickLook.Next.Core.Tests\QuickLook.Next.Core.Tests.csproj -c Release
dotnet test tests\QuickLook.Next.ParserHost.IntegrationTests\QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release
dotnet test tests\QuickLook.Next.RasterHost.IntegrationTests\QuickLook.Next.RasterHost.IntegrationTests.csproj -c Release
pwsh -NoProfile -File tools\smoke-native.ps1
pwsh -NoProfile -File tools\smoke-exif-map.ps1
dotnet build QuickLook.Next.slnx -c Release
pwsh -NoProfile -File tools\guard-performance-bounds.ps1
pwsh -NoProfile -File tools\guard-format-registry.ps1
pwsh -NoProfile -File tools\guard-stale-callbacks.ps1
pwsh -NoProfile -File tools\guard-architecture.ps1 -SkipDist
pwsh -NoProfile -File tools\benchmark-handle-handoff.ps1
git diff --check
```

Useful targeted checks:

```powershell
rg -n "TrackPopupMenu|CreatePopupMenu|AppendMenu|DestroyMenu|TPM_|MF_CHECKED|MF_STRING" src\QuickLook.Next.App
rg -n "WebView|WebView2" src native tools README.md docs
rg -n "QuickLook.Next.Plugin." src tools README.md docs
rg -n "read_to_end\(&mut bytes\)" native\quicklook_next_native\src\preview.rs native\quicklook_next_native\src\preview
```

The tray popup search should only hit `src/QuickLook.Next.App/TrayIconManager.cs`.

The shared bounded-reader helpers now live in `preview/bounded.rs`. Its
`read_to_end` calls are limited to:

- `read_reader_prefix`, which reads through `take(max_bytes)`.
- `read_limited_to_end`, which reads through `take(max_size + 1)` and rejects
  payloads over the cap.

## Known Remaining Work

- Complete the evidence-first delivery baseline, then measure the input-to-first-paint path and
  mixed-format close/reopen resource recovery. Existing focused host leak tests are not a full
  packaged App performance baseline.
- Explorer selection COM queries still execute on the Rust hook message pump. Separate ownership,
  bound queued work, and preserve close/generation priority before treating input latency as solved.
- Archive, Office, package, database, and media parser families are already separated. Further
  extraction should target the remaining decode/animation/Win32 ownership or App lifecycle
  boundaries when it improves testability, not just reduce root-file line counts. Consolidate
  duplicated structural guards without weakening their contracts.
- The final animated-frame upload still copies from the retained CPU section into
  `WriteableBitmap.PixelBuffer`. A future direct GPU shared-surface renderer is optional and should
  be justified by profiling before adding cross-process D3D synchronization.
- Expand the allowlisted Property Handler field mapping only when a concrete Windows metadata gap is
  demonstrated; keep every supplement inside RasterHost and HANDLE-stream based.
- Keep Office approximate layout in maintenance mode. This is intentionally
  not a full Office rendering engine: PPT/XLSX should prioritize slide/sheet
  layout, text positions, table/cell geometry, relationships, and embedded
  images. Full style parity, macros, animations, formula recalculation, and
  complete Office compatibility remain out of scope for the default preview
  boundary.
- Expand real-world smoke assets for larger PDFs, malformed archives, unusual
  APK/MSIX manifests, mixed-encoding text files, and complex Office files.
  Current smoke coverage includes UTF-16 text and corrupt ZIP fail-closed checks,
  but still needs more externally sourced real-world files.
- Push cancellation deeper into Rust/native decode/listing loops. The App passes
  a native cancellation callback and prevents stale merge/update work, but some
  third-party codec calls still contain non-interruptible spans once entered.
- Keep improving the primary image path:
  - Thumbnail filmstrip loading should keep prioritizing the current image and
    nearby siblings, with LRU eviction to bound memory in large folders.
  - Adjacent image prefetch should remain cancellation-aware so quick Explorer
    selection changes do not keep decoding old images.
  - Large image decode should keep checking the native cancellation callback
    around codec, resize, color-conversion, and final-copy boundaries.
- PDF surface caching intentionally keeps the visible range plus at most five
  offscreen page surfaces. Presenter viewport events are coalesced on the UI
  dispatcher, generations reject stale results, and RasterHost cancels older
  in-flight generations for the same request/page before starting a newer one;
  continue improving coalescing without returning to unbounded GPU retention.
- Continue Shell icon coverage for virtual archive entries if a stable file type
  icon can be resolved without pretending the virtual item is a real path.
- Existing bounded MP4/media summaries, Outlook MSG property streams, and SQLite schema/row
  samples should not be confused with full professional-format engines. Broader extraction,
  including CHM topics, requires a concrete preview need and explicit bounds; no WebView fallback.
- Continue the HANDLE ABI migration only after each reader accepts a bounded `Read` or `Read + Seek`
  input. SQLite snapshots, archive listing/entry extraction, and ebooks now have explicit HANDLE
  boundaries. Office main/layout and follow-up hero extraction now share one retained HANDLE source;
  RasterHost ICO, SVG, general raster, and GIF/WebP/APNG animation previews now decode from
  independent leases on a retained HANDLE source without an input anchor. Local system-codec images
  now wrap an independently
  reopened source lease as a WinRT random-access stream and do not create an input anchor. Local PDF
  sessions now load and retain the exact HANDLE-backed WinRT stream through page rendering, with
  HANDLE-derived cache identity. PDF session close tracks and drains the underlying WinRT render
  task even when its cancelable waiter exits; document-owned streams and synchronization objects
  are released only after that drain. A render that cannot drain within 12 seconds terminates the
  RasterHost so the process boundary reclaims the non-cancelable Windows PDF operation safely.
  RasterHost no longer creates HANDLE input anchors; unsupported
  HANDLE kinds fail closed. Shell fallback remains path-based only for explicit cloud/legacy
  compatibility requests and should move to a broker if RasterHost is later sandboxed further.
- The legacy path entry points remain for cloud and explicit compatibility inputs. Local
  Archive/Ebook requests must stay on the HANDLE routes. For a normal local file, the App pins once,
  performs the initial authoritative `ql_probe_file_handle` routing probe against that object, and
  transfers the same object to ParserHost/RasterHost without a second format probe. Directories,
  cloud metadata, missing `HANDLE_PROBE` capability, and a pin failure are the explicit path
  compatibility cases. ParserHost and RasterHost no longer create HANDLE input anchors; both adopt
  unsupported HANDLE requests and fail closed without consulting logical path metadata.
  The extracted archive child's bounded App anchor remains because downstream routing still needs a
  logical filename, but Rust writes that same object through a caller-provided HANDLE; ParserHost
  publishes no temporary path or Host-owned file HANDLE and the App performs no intermediate copy.

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
