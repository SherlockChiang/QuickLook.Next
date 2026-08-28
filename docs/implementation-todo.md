# Implementation TODO

This file tracks the ordered hardening and product-improvement work identified
by the July 2026 repository review. Each completed item records its verification
and commit so changes remain independently reviewable and revertible.

## Optimization batches

All planned 0.3.1 batches and the focused 0.3.2/0.3.3/0.3.4 hardening batches are complete;
remaining work below stays ordered by risk and user-visible impact.

## Microsoft Store 1.0.0 launch queue

This queue keeps Store onboarding separate from the signed GitHub sideload
channel. Detailed identities, hashes, and test boundaries are recorded in
`docs/microsoft-store-submission.md`.

- [x] `STORE-01` Reserve the Store product and record its exact package identity
  without changing the sideload manifest identity.
- [x] `STORE-02` Build and attest the unsigned `1.0.0.0` x64 Store candidate;
  pass the Store packaging, architecture, workflow, and dependency guards.
- [x] `STORE-03` Draft the English, Simplified Chinese, and Traditional Chinese
  listings, field limits, IARC factual basis, and certification notes.
- [ ] `STORE-04` Create or update the Partner Center draft: upload the existing
  `.msixupload`, export the live listing CSV, import the three locale drafts,
  choose the category, and complete the live IARC questionnaire. Do not submit
  for certification or enable automatic public publishing without explicit
  owner confirmation.
- [ ] `STORE-05` Obtain a Store-signed private-flight/acquisition package and,
  with explicit install approval, record WACK plus clean install, update,
  rollback, uninstall, crash-dialog, and first-launch evidence in an isolated
  Windows test environment.
- [ ] `STORE-06` Capture final-candidate Store screenshots, review the optional
  300 x 300 tile icon/promotional assets, and verify every listing locale.
- [ ] `STORE-07` Review the complete Partner Center diff and certification hold;
  submit only after a separate explicit owner confirmation.

## 2026-08-04 review execution queue

This queue turns the August repository review into independently verifiable local
commits. Complete one item at a time, run its focused checks before committing,
and record the implementation commit in the completed ledger. Do not push or use
the release-only `release:` prefix while this queue is in progress.

### Release blockers

- [x] `R26-P0-01` Bound Shell thumbnail dimensions and allocation arithmetic in
  Rust before calling `GetDIBits`; cover hostile dimensions and overflow edges.
- [x] `R26-P0-02` Make every release, packaging, long-cycle, and nested guard
  invocation fail closed on a non-zero child-script exit code; add fault-injection
  coverage so a later successful guard cannot erase an earlier failure.
- [ ] `R26-P0-03` Move the live signing certificate and password out of the
  workspace, verify their storage policy, and rotate them if exposure cannot be
  excluded. This is an owner-operated credential task and must not be automated.
- [x] `R26-P0-04` Fail closed against supervised-host `Application Error`
  dialogs after the observed RasterHost DXGI `0x0000087a` crash. Keep the
  process-wide no-dialog error mode active, retain supervisor exit-code/log
  diagnostics, and cover a real crashing child instead of relying only on
  source-pattern or in-process flag checks. Reopened on 2026-08-04 after a live
  RasterHost error window recurred following commits `b2b5ee2` and `bfd48da`;
  clear conflicting WER always-show-UI state and make the child inherit the
  no-dialog error mode before its CLR/apphost loader runs. A first-chance native
  dump then isolated the remaining bare DXGI facility exception to CLR/WinRT/D3D
  teardown after pipe EOF, not the connected preview lifetime. RasterHost now
  quiesces idle trim, cancels and drains all registered graphics workers, and
  terminates the supervised process atomically after logical cleanup.

### Correctness and user-visible state

- [x] `R26-P1-01` Bind preview errors to the failing path and generation so Retry,
  Open, and Reveal can never act on the previous file; cover first-open and A-to-B
  early-failure transitions.
- [x] `R26-P1-02` Resolve the text-search contract drift between the presenter,
  keyboard routing, documentation, and performance guard without restoring the
  old wheel-intercepting flyout.
- [ ] `R26-P1-03` Add explicit CloudProgress, PDF per-page failure, empty, and
  partial-result states, including Office represented/total/limit metadata.
  - [x] `R26-P1-03a` Surface bounded cloud hydration as an explicit visible
    determinate/indeterminate progress state with generation-safe cleanup and
    three-locale accessibility text. PDF and Office states remain open.
  - [x] `R26-P1-03b` Show current-generation PDF page render failures and
    timeouts inside the affected page, reject late surfaces, and clear visual
    and accessible state on success, release, and reopen. PDF empty/partial and
    Office represented/total/limit states remain open.
- [ ] `R26-P1-04` Add a redacted Copy Diagnostics action with a stable error code,
  phase, correlation ID, version, format, and size bucket but no local path.

### Rust-first architecture and performance

- [x] `R26-P1-05` Make Cargo a first-class MSBuild input/output dependency so a
  direct solution build cannot silently package a missing or stale native DLL.
- [x] `R26-P1-06` Move the ignored Rust-first guidance into tracked `AGENTS.md`
  and focused ADRs for process, HANDLE ownership, cancellation, and error contracts.
- [ ] `R26-P1-07` Split native preview code into bounded parser/core, format-family,
  Win32, and thin FFI modules or crates; generate Rust/C# ABI declarations from one
  schema and scope complexity lint exceptions to unavoidable FFI shims.
  - [x] `R26-P1-07a` Remove crate-wide complexity lint exemptions, replace complex
    data tuples with named Rust types, and keep any remaining argument-count
    exemptions local to ABI-shaped shims.
  - [x] `R26-P1-07b` Move Shell thumbnail STA, COM, GDI ownership, and allocation
    validation into a bounded `win32` module while keeping exported functions thin.
  - [ ] `R26-P1-07c` Split Office, archive/package, database/media, and shared
    parser primitives out of the `preview.rs` aggregation module by format family.
    - [x] `R26-P1-07c-1` Establish the format-module pattern by moving bounded
      font metadata rendering and SFNT/WOFF table parsing into `preview/font.rs`.
    - [x] `R26-P1-07c-2` Move media container, stream, waveform, and duration
      parsing into bounded `preview/media/` family modules.
      - [x] `R26-P1-07c-2a` Move RIFF/WAV, FLAC, and Ogg parsing plus focused
        tests into `preview/media/audio.rs`, with shared media formatting in
        `preview/media/mod.rs`.
      - [x] `R26-P1-07c-2b` Move ID3 frame parsing and text decoding into
        `preview/media/id3.rs` with its focused tests.
      - [x] `R26-P1-07c-2c` Move Matroska/EBML parsing into
        `preview/media/matroska.rs` with bounded element traversal tests.
      - [x] `R26-P1-07c-2d` Move AVC, HEVC, and AAC bitstream/config parsing
        into `preview/media/codec.rs` with a private bounded bit reader.
        - [x] `R26-P1-07c-2d-1` Move MPEG-4 descriptor and AAC AudioSpecificConfig
          parsing into `preview/media/codec.rs` behind a narrow media adapter.
        - [x] `R26-P1-07c-2d-2` Move AVC configuration, SPS/VUI parsing, and the
          bounded bit reader into the codec module with hostile crop/bit tests.
        - [x] `R26-P1-07c-2d-3` Move HEVC configuration, VPS/SPS/VUI, and bounded
          parameter-set array parsing into the codec module.
      - [x] `R26-P1-07c-2e` Move ISO BMFF/MP4 atoms, tracks, timelines, and
        chunk summaries into `preview/media/mp4.rs`.
        - [x] `R26-P1-07c-2e-1` Move bounded atom traversal, movie-header time,
          creation, and rotation primitives into `preview/media/mp4.rs`.
        - [x] `R26-P1-07c-2e-2` Move sample tables, edit/composition timelines,
          and chunk mapping into the MP4 module with linear-or-better `stsc`
          lookup and hostile table tests.
        - [x] `R26-P1-07c-2e-3` Move track parsing, codec payload adapters,
          summary/output composition, and the MP4 integration test into the
          MP4 module.
      - [x] `R26-P1-07c-2f` Move media routing, container detection, and output
        composition into `preview/media/mod.rs`, leaving one explicit route in
        `preview.rs`.
    - [ ] `R26-P1-07c-3` Move database, mail, CHM, dump, ELF, and related binary
      metadata parsing into focused family modules with local tests.
      - [x] `R26-P1-07c-3a` Move CHM routing, ITSF/ITSP directory parsing,
        compressed-stream discovery, and `/#SYSTEM` metadata into
        `preview/chm.rs`; harden hostile offsets and preserve bounded reads.
      - [x] `R26-P1-07c-3b` Move MIME/EML and Compound File Binary MSG parsing
        into `preview/mail.rs` with malformed/truncated Outlook-message tests.
      - [x] `R26-P1-07c-3c` Move ELF headers, sections, symbols, notes, and GNU
        version parsing into `preview/elf.rs` with checked hostile offsets.
      - [x] `R26-P1-07c-3d` Move minidump streams and ELF-core composition into
        `preview/dump.rs` behind narrow sibling-module APIs.
      - [x] `R26-P1-07c-3e` Split SQLite/database rendering into bounded
        `preview/database/` composition, WAL, and SQLite parser modules while
        preserving HANDLE, companion-file, cancellation, and size contracts.
    - [x] `R26-P1-07c-4` Move Office document, workbook, presentation, layout,
      and embedded-image parsing into focused Office modules.
      - [x] `R26-P1-07c-4a` Move PPTX/PPTM presentation layout, placeholder
        inheritance, bounded text extraction, title/summary selection, and
        focused regression tests into `preview/office/presentation.rs` behind
        one narrow renderer route and the shared Office decompression budget.
      - [x] `R26-P1-07c-4b` Move DOCX/ODF document text, headers/footers, and
        bounded document layout into `preview/office/document.rs`.
      - [x] `R26-P1-07c-4c` Move XLSX workbook cells, styles, metrics, and
        drawing anchors into `preview/office/workbook.rs`.
      - [x] `R26-P1-07c-4d` Move shared Office relationship/layout primitives
        into a bounded `preview/office/layout.rs` module.
      - [x] `R26-P1-07c-4e` Move Office embedded-image discovery, references,
        and lazy BGRA extraction into a focused image module while preserving
        the HANDLE and source-pixel budgets.
    - [x] `R26-P1-07c-5` Move archive and application-package listing/parsing
      into focused modules while preserving bounded extraction contracts.
      - [x] `R26-P1-07c-5a` Move ZIP, TAR/TGZ, standalone GZip, and RAR listing
        composition into `preview/archive/listing.rs`, retaining scan, time,
        entry-count, path-retention, and cancellation budgets.
        - Verification: archive-focused Rust tests, module-boundary guard, and
          performance-bounds guard pass.
        - Commit: `91b53ec`, `9455240`, `cf660b3`
      - [x] `R26-P1-07c-5b` Move bounded ZIP entry streaming, temporary-output
        lifecycle, and cleanup validation into `preview/archive/extract.rs`.
        - Verification: archive extraction budget/encryption/cleanup tests and
          both Rust guards pass.
        - Commit: `91b53ec`, `60e26fa`
      - [x] `R26-P1-07c-5c` Move Windows/Android package metadata, AppX manifest
        parsing, and bounded icon discovery into `preview/package/mod.rs`.
        - Verification: package metadata/icon tests, module-boundary guard, and
          performance-bounds guard pass.
        - Commit: `0a4eb6c`, `7fd759b`
      - [x] `R26-P1-07c-5d` Move Android binary XML/resource-table resolution,
        adaptive-icon composition, and vector rendering into
        `preview/package/android.rs` with hostile-boundary tests.
        - Verification: Android resource-table/vector/adaptive-icon tests,
          module-boundary guard, and performance-bounds guard pass.
        - Commit: `0a4eb6c`, `7fd759b`
    - [ ] `R26-P1-07c-6` Move reusable bounded-reader and parser primitives into
      shared core modules, leaving `preview.rs` as a small explicit router.
      - [x] `R26-P1-07c-6a` Move cancellation-aware prefix/exact readers,
        seek-length validation, limited reads, and ZIP/ZIP64 preflight into
        `preview/bounded.rs` with focused length, cap, fallback, and cancellation
        tests.
        - Verification: bounded-reader and preview-focused Rust tests, Clippy,
          module-boundary guard, and performance-bounds guard pass.
        - Commit: `cd50e39`, `b60696e`
      - [x] `R26-P1-07c-6b` Apply the shared seek-length and cancellation
        primitives to Outlook mail input. Replace the fixed 256 KiB MSG prefix
        with a bounded, on-demand CFB sector reader, split CFB parsing into
        `preview/mail/cfb.rs`, and route local mail through the exact HANDLE
        ParserHost ABI (capability bit 21). Keep EML/MIME at a 256 KiB prefix,
        cap CFB source reads at 1 MiB, and preserve FAT/DIFAT, directory,
        mini-stream, property, source-length, and cancellation limits.
        - Verification: 277 Rust tests passed (1 ignored), 280 Core tests,
          45 ParserHost integration tests, 29 RasterHost integration tests,
          13 ShellBroker integration tests, release build with zero warnings,
          module-boundary/performance/format guards, and architecture checks
          through the environment-blocked restricted-host smoke (exit 23).
        - Commits: `2189aa5`, `de8f2e8`
  - [ ] `R26-P1-07d` Move exported entry points and raw-pointer validation into
    focused FFI modules, leaving `lib.rs` as a small composition root.
    - [x] `R26-P1-07d-1` Move the folder preview and text/archive routing
      exports into `native/quicklook_next_native/src/ffi/routing.rs`, preserve
      the C ABI and panic boundary, and make architecture/module guards scan
      the focused source file. Add null, length-cap, and output-boundary tests.
      - Verification: Rust fmt, Clippy, workspace tests (297 passed, 1 ignored),
        FFI safety, module-boundary, and performance-bounds guards passed;
        architecture guard reached all Rust checks but its isolated MSBuild
        fixture is locally blocked by the pinned SDK `10.0.302` (only
        `10.0.303`/`10.0.400` are installed).
    - [x] `R26-P1-07d-2` Move the shared UTF-8/pointer, output-buffer, and
      panic-boundary helpers into `native/quicklook_next_native/src/ffi/common.rs`;
      preserve signatures, error codes, and ownership semantics while adding
      focused boundary tests and location guards.
      - Verification: Rust fmt, Clippy, workspace tests (301 passed, 1 ignored),
        FFI safety, module-boundary, and performance-bounds guards passed;
        the local architecture guard's isolated MSBuild fixture remains blocked
        by the pinned SDK `10.0.302` (only `10.0.303`/`10.0.400` are installed).
  - [ ] `R26-P1-07e` Generate Rust ABI constants and C# declarations from one
    reviewed schema, and fail the architecture guard when generated files drift.
- [ ] `R26-P1-08` Add decoded-byte budgets, target-size image decode, single-decode
  output sizing, and measurable cancellation latency for RasterHost image work.
  - [x] `R26-P1-08a` Reject static native image decodes whose checked peak model
    exceeds 896 MiB before allocating the full `DynamicImage`. Target-size codec
    decode, single-decode output sizing, and cancellation measurement remain open.
  - [x] `R26-P1-08b` Preflight exact non-SVG static-raster HANDLE packet sizes
    before full pixel decode, including JPEG orientation and waveform bytes, so
    an undersized managed buffer does not cause a repeated full decode. Codec
    target-size decode and measured cancellation p95 remain open.
  - [x] `R26-P1-08c` Make static image, GIF, WebP, and APNG decoder I/O
    cancellation-aware at every reader boundary. This shortens stale-preview
    cancellation around codec reads without changing the ABI or claiming to
    interrupt an already-running OS read or codec CPU loop; measured p95 remains open.
- [ ] `R26-P1-09` Move Explorer COM work off the keyboard-hook pump and replace the
  thumbnail worker's unbounded, non-cancellable queue with a bounded supervised
  broker or deadline-aware worker.
  - [x] `R26-P1-09a` Replace the unbounded native Shell thumbnail STA channel
    with a capacity-one non-blocking queue and reject pre-cancelled work before
    dispatch. Moving COM ownership off the hook pump remains open.
- [ ] `R26-P1-10` Add fuzz/property targets and sanitizer coverage for archive,
  Office, TIFF/EXIF, SQLite, executable, media, and FFI packet boundaries.

### Repository and delivery governance

- [ ] `R26-P1-11` Route the local 0.3.5 stack through an integration branch and PR
  after release blockers pass; require CI checks, admin enforcement, resolved
  conversations, and an approval-protected release environment before publishing.
- [ ] `R26-P2-01` Normalize signed annotated release tags, enable dependency
  security alerts, group routine update PRs, and add CODEOWNERS for native, App,
  RasterHost, and release tooling.
- [ ] `R26-P2-02` Keep the active workspace rooted at the real `QuickLook.Next`
  repository and retire the ambiguous empty outer `.git`/legacy-repository layout
  only after explicit owner confirmation.

## P0: Immediate safety and usability

- [ ] Move the live release signing key out of the workspace and rotate it if
  exposure cannot be ruled out. This requires owner confirmation and external
  credential storage; do not delete or move the current key automatically.

## P1: Performance and accessibility

- [ ] Push cancellation/epoch checks into native image and listing loops so stale previews stop
  consuming CPU before a synchronous FFI call returns.
- [ ] Persist first-paint p50/p95 latency, resident memory, HANDLE count, and host recycle timing for
  a bounded preview-switch corpus, with lightweight pull-request and fuller nightly budgets.
- [ ] Extend the exact-size, single-decode shared-section handoff now used by GIF to bounded WebP
  and APNG corpora. Preserve the 64 MiB section ceiling while measuring packet bytes,
  animation-ready latency, and first visible motion for those remaining formats.
- [ ] Version the animation packet so a bounded spatial downsample can preserve a complete timeline
  without changing the intended display size, and so any true frame-count truncation is explicit and
  never loops a silent prefix. The current 64 MiB budget derives a frame limit from output pixels;
  the same animation can therefore become partial at a larger window size or DPI.
- [ ] Execute and retain the tracked custom-title-bar visual evidence matrix at compact width, across
  a live 100%-to-200% display move, in Simplified Chinese, and in Windows High Contrast. Dynamic
  system-inset layout, scale conversion, automated policy tests, and the evidence gate are complete;
  do not claim a manual visual pass until all required screenshots and run records exist.
- [ ] Replace the remaining `PdfDocument` projection with an API that exposes a native close
  contract. Until Windows provides that boundary, RasterHost now deterministically tracks and drains
  every underlying render task before releasing session-owned HANDLE streams; a 12-second drain
  failure exits the isolated host rather than disposing resources beneath an active WinRT call.
- [ ] Verify loading, success, failure, and PDF page-error announcements with
  Narrator on a Windows accessibility test machine.

## P2: Product capabilities

- [ ] Add bounded PDF text search and copy.
- [ ] Add user-visible cache usage/clear controls, shortcut conflict diagnostics, and a bounded
  diagnostics export that redacts local paths by default.
- [ ] Add an ARM64 build/test lane while keeping x86 out of scope until user demand justifies it.

## P3: Strategic architecture

- [x] Move durable architecture and contribution guardrails out of the ignored local `agent.md` and
  into tracked documentation with focused ADRs for process, HANDLE, and cancellation contracts.
- [ ] Replace the final CPU shared-section-to-`WriteableBitmap` animation upload with
  renderer-consumable GPU shared surfaces if profiling justifies the additional D3D synchronization
  contract. Per-frame managed arrays and packet copies have already been removed.
- [ ] Add AppContainer isolation and enforced network denial to hostile-format hosts after the
  ParserHost write-restricted boundary and RasterHost compatibility split are complete.
- [ ] Split the native preview implementation by format family.
- [ ] Add App policy tests, fuzzing, ETW/WPA baselines, and long-cycle resource
  regression tests.
- [ ] Design any future extension SDK as signed, bounded, out-of-process, and
  denied network access by default.

## Completed

Completed entries move here with the verification commands and commit hash.

- [x] `CI26-01` Stabilize the RasterHost repeated-image handle-growth guard on
  hosted .NET/D3D stacks without increasing its 12-handle leak budget. Warm up
  the bounded runtime-worker ramp for 64 cycles, derive the baseline from the
  final eight warmup samples, and retain a separate 32-cycle measured window
  with peak/last diagnostics.
  - Verification: focused test passed 12/12 independent-process repetitions.
  - Verification: all 29 RasterHost integration tests passed together.
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commits: `bea5b7a`, `700483e`

- [x] `R26-P1-08b` Compute the exact checked plain/waveform packet length from
  bounded header dimensions, target geometry, and JPEG orientation before full
  static-raster HANDLE decode. Cancellation wins after preflight and before
  required bytes are published; the caller HANDLE position and ABI are unchanged.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml native_image_packet` (2 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml raster_image_handle_sizes_output_before_full_pixel_decode` (1 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commit: `035bcc5`

- [x] `R26-P1-08c` Make static image, GIF, WebP, and APNG decoder reads and
  seeks observe the cancellation callback before and after codec I/O. A stale
  preview now fails closed at the next reader boundary while the existing
  post-decode error mapping, HANDLE ownership, and caller positions remain
  unchanged; this is cancellation-latency groundwork, not a full p95 claim.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native --lib image_decoder_reads_honor_cancellation_boundaries` (1 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native --lib` (293 passed, 1 ignored)
  - Verification: `cargo clippy --locked --manifest-path native/Cargo.toml -p quicklook_next_native --all-targets --all-features -- -D warnings`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Integration note: current `main` already deduplicates production image
    decoder monomorphization by moving the independently reopened HANDLE
    `File` into the one-shot renderer (`bed39b3`). The older boxed-reader
    experiment was intentionally omitted during rebase so cancellation keeps
    Rust static dispatch without a per-decode allocation or read/seek vtable.
  - Integration verification: `tools/release.ps1 -SkipPackage -SkipSystemImageSmoke`
    passed Rust 293/1, Core 281, ParserHost 45, RasterHost 29, ShellBroker 13,
    external image corpus, and every architecture/UI/performance/installer guard.
  - Release-size verification: Thin LTO with one codegen unit (`faec65a`) keeps
    all three native DLL copies at 7,474,688 bytes and the exact 187-file `dist`
    at 169.311 MiB, below the unchanged 170 MiB architecture budget.
  - Commit: `7448ff5`

- [x] `R26-P1-03b` Render localized PDF page failure and timeout text inside the
  exact current request/page generation. Rendered, failed, or released pages
  reject late surfaces; success, release, and reopen clear both Composition and
  accessible failure state.
  - Verification: `pwsh -NoProfile -File tools/test-pdf-page-failure-ui.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-localization.ps1` (3 locales, 451 keys)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Commit: `0691d49`

- [x] Preflight the current user's installed MSIX identity and version before
  certificate trust, UAC, application shutdown, or registration. Older packages
  fail with a clear downgrade error, identical versions return already-current
  success without side effects, ambiguous identities fail closed, and only a
  higher target version enters the existing upgrade/rollback flow.
  - Verification: `pwsh -NoProfile -File tools/test-installer-script.ps1`
  - Verification: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/test-installer-script.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-local-msix-update.ps1` (WhatIf resolved `0.3.6.4`)
  - Commit: `8e9f9e5`

- [x] `R26-P1-08a` Add a checked 896 MiB static-image decoded-byte peak budget
  before `DynamicImage::from_decoder`. The model uses decoder-reported color
  depth and covers EXIF transform copies, resize-native storage, RGBA/BGRA,
  waveform workspace, ABI packet copies, and arithmetic overflow while keeping
  the existing 48 MP limit and ABI unchanged.
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml native_image_decode_peak_budget` (3 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commit: `5848544`

- [x] `R26-P1-03a` Replace hidden-only cloud hydration text with an explicit
  themed progress panel. Known lengths use bounded percentages, unknown lengths
  remain indeterminate with downloaded bytes, callbacks retain the 250 ms and
  preview-generation gates, and reset/completion clears stale visible state.
  - Verification: `pwsh -NoProfile -File tools/test-cloud-progress-ui.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-localization.ps1` (3 locales, 451 keys)
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore --filter "FullyQualifiedName~CloudHydrationPolicyTests"` (16 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Commit: `b2f9be9`

- [x] Execute the packaged installer parent control flow without UAC, AppX, or
  certificate-store writes. The temporary harness covers first trust, direct
  trusted upgrade, registration failure rollback, and registered-state
  verification failure with retained trust in both PowerShell 7 and Windows
  PowerShell 5.1; the existing AST guard still owns the elevated helper exits.
  - Verification: `pwsh -NoProfile -File tools/test-installer-control-flow.ps1`
  - Verification: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/test-installer-script.ps1`
  - Commit: `e164db8`

- [x] `R26-P1-02` Restore the existing bounded text-search presenter contract
  through an inline, non-overlay search row with Ctrl+F, Enter/F3 navigation,
  reverse navigation, Escape close, focus restoration, polite match counts,
  40-DIP controls, and English/Simplified Chinese/Traditional Chinese strings.
  The old wheel-intercepting flyout controls remain forbidden, and the focused
  contract test now runs from the architecture guard.
  - Verification: `pwsh -NoProfile -File tools/test-text-search-contract.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-localization.ps1` (3 locales, 450 keys)
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-build --no-restore --filter "FullyQualifiedName~Text_search"` (2 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Commit: `1fb6a0c`

- [x] `R26-P1-09a` Bound native Shell thumbnail dispatch to one queued request
  behind the active STA call, fail fast when saturated, and reject a request
  whose cancellation callback is already set. The Shell COM call remains
  serial and deadline-bounded; moving it off the hook pump remains queued.
  - Verification: `cargo test --manifest-path native/quicklook_next_native/Cargo.toml shell_thumbnail` (5 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commit: `1a61fde`

- [x] `R26-P1-07c-6b` Replace the fixed-prefix Outlook MSG path with a
  Rust-first, seekable reader and exact ParserHost HANDLE route. `mail.rs`
  remains a 739-line MIME/route composition module; the 773-line
  `preview/mail/cfb.rs` module reads only required CFB sectors through checked
  `u64` offsets and an authoritative source length. It caches bounded sectors,
  caps cumulative CFB reads at 1 MiB, polls cancellation around every source
  read, and retains the existing FAT (16), DIFAT (8), directory (16 sectors /
  256 entries), mini-FAT (16), 256 KiB mini-stream, tree, chain, and MAPI
  property limits. EML/MIME continues to use only the 256 KiB prefix. A
  regular MSG property sector placed beyond 256 KiB is covered by a v4 fixture
  and remains visible. `ql_preview_mail_handle` uses the shared HANDLE adapter;
  logical names are basename hints only, and path compatibility explicitly uses
  `ql_preview_info` instead of the archive renderer.
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (277 passed, 1 ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --disable-build-servers --maxcpucount:1` (367 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-format-registry.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist` (all preceding checks passed; restricted-host launch smoke is environment-blocked with exit 23)
  - Commits: `2189aa5`, `de8f2e8`

- [x] `R26-P1-07c-4b` Move DOCX/ODF text extraction, DOCX header/footer
  discovery, and bounded document page/image composition into the 466-line
  `preview/office/document.rs` module with 137 lines of focused tests. The
  parent retains the shared ZIP reader, aggregate decompression/cancellation
  context, relationship/image-reference primitives, and JSON envelope; the
  Office composition layer exposes only the DOCX and ODF renderer routes.
  Header/footer scanning remains capped by the Office ZIP-entry budget, 1 MiB
  parts, and eight retained entries. Document layout retains eight pages, six
  images, 420 characters per paragraph, and XML event cancellation checks.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native --lib document::tests` (6 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (265 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke` (all preceding guards passed; restricted-host launch smoke is environment-blocked with exit 23)
  - Commits: `0aa038c`, `3bf5cf3`

- [x] `R26-P1-07c-4c` Move XLSX workbook text/layout parsing into the 1,298-line
  `preview/office/workbook.rs` module with 200 lines of focused tests. The
  module owns shared-string resolution, sparse worksheet rows, styles and
  number formats, sheet metrics/freeze panes/merge spans, bounded drawing
  anchors, and lazy image references. `office/mod.rs` exposes only
  `render_xlsx`; the parent retains the shared Office budget, relationship,
  image-reference, and JSON primitives. The workbook path keeps the six-sheet,
  48-row, 36-character-cell, 18-image, and cancellation/event budgets.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::office::workbook` (6 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native office::` (21 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (267 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commits: `8a9e101`, `6bdc4b4`

- [x] `R26-P1-07c-4d` Move the shared Office relationship parser, part-relative
  `.rels` path helpers, image placement contract, and bounded layout image-item
  builder into the 130-line `preview/office/layout.rs` module with focused
  relationship/path tests. PPTX and XLSX now import the shared layout API
  explicitly; the parent retains only the generic ZIP target normalizer needed
  by both Office and EPUB.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::office::layout` (2 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native office::` (21 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commits: `4784533`, `1d908a1`

- [x] `R26-P1-07c-4e` Move Office media discovery, canonical image references,
  case-fold ambiguity checks, and lazy BGRA extraction into the 527-line
  `preview/office/image.rs` module with 200 lines of focused tests. The parent
  keeps only stable extraction wrappers; generic ZIP validation, shared
  decompression accounting, and bounded embedded-image decoding remain reusable
  native helpers. Media discovery stays scoped to the OOXML root, layout image
  sources remain capped at 768 KiB, source dimensions at 8192 pixels/16 million
  pixels, output targets at 1024 pixels, and every scan/decode path remains
  cancellation-aware.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::office::image` (5 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native office::` (38 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (273 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke` (all architecture, FFI, image-corpus, and restricted-host checks passed)
  - Commits: `dada5f9`, `d1b6a53`

- [x] `R26-P1-07c-4a` Move the PPTX/PPTM presentation parser and layout
  composer out of `preview.rs` into a bounded `preview/office/presentation.rs`
  module. The module owns slide-size/background parsing, shape/image placement,
  layout/master placeholder inheritance, bounded title selection, and slide text
  extraction while consuming the existing shared Office decompression/cancellation
  context. `office/mod.rs` and `preview.rs` expose only the narrow renderer route;
  the nine PPT regressions now live beside the implementation and retain the
  one-time layout/master budget assertion.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native --lib presentation::tests` (9 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (265 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke` (all preceding guards passed; restricted-host launch smoke is environment-blocked with exit 23)
  - Commits: `289fdf3`, `d67144d`

- [x] `R26-P1-07c-3e` Split the SQLite/database family out of `preview.rs` into
  a 242-line composition module, a bounded 281-line WAL/SHM snapshot module,
  and a 1,019-line SQLite schema/table/record parser. The parent keeps only the
  path/HANDLE readers, companion-file validation, cancellation and size limits,
  and JSON composition. WAL reads remain capped at 64 MiB, validate header and
  frame salts/checksums, apply only committed prefix pages, and drain bounded
  tails; SHM is diagnostic-only and capped at 4 MiB. SQLite schema traversal,
  row observation, table sampling, sheet count, cell length, and retained text
  all keep explicit budgets. The 679-line focused test file covers malformed
  offsets, page-size/count boundaries, checksum/tail recovery, cancellation,
  companion limits, UTF-16 records, and hostile missing pages.
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (265 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commits: `1fde6e1`, `2c7c8fc`, `f7824d1`

- [x] `R26-P1-07c-3d` Move the Rust minidump stream summaries and ELF-core
  composition out of `preview.rs` into a focused `preview/dump.rs` module with
  one `render_info` parent API and explicit sibling access to
  `preview::elf::append_summary`. Keep the minidump header/directory and stream
  metadata bounded: path reads stop at 1 MiB, directory entries are capped at
  64, all RVA/entry arithmetic uses checked slices and checked integer
  conversion, UTF-16 names are limited to 4 KiB and even byte lengths, and
  known ThreadNames streams are labeled correctly. Hostile offsets, oversized
  string lengths, truncated streams, and metadata placed beyond the legacy
  512-byte prefix fail soft; the real path route is covered by a dump fixture.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native dump::` (5 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (265 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: full `tools/guard-architecture.ps1 -SkipDist` reached the unrelated restricted-host launch smoke, which exited 23 in this environment; all preceding architecture, module, performance, and error-UI guards passed
  - Commits: `b1534e4`, `637161b`

- [x] `R26-P1-07c-3c` Complete the 1,344-line Rust ELF metadata module with
  329 lines of focused tests and two narrow parent APIs. Raise the path-based
  read from the legacy 512-byte prefix to an explicit bounded 1 MiB budget so
  section/string metadata beyond the ELF header is reachable. Validate ELF32/
  ELF64 identity, endian/version fields, header and table entry sizes, checked
  u64-to-usize conversions, table ranges, bounded dynamic/version/note scans,
  string-table ownership, and file-backed virtual-address mapping. Fail soft on
  truncated, big-endian, hostile-offset, unterminated-dynamic, oversized-entry,
  and malformed-note inputs; keep GNU `vd_cnt` at its specified `+6` field and
  cap interpreter, symbol, relocation, version, note-owner, and build-id output.
  The real `render_info` route is covered with metadata placed beyond 512 bytes.
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml elf` (5 passed)
  - Verification: `cargo test --release --workspace --locked --manifest-path native/Cargo.toml elf` (5 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (263 passed, 1 ignored)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist` (passed outside the sandbox)
  - Commits: `b409225`, `6ed54ae`, `3501b14`

- [x] `R26-P1-07c-3b` Complete the focused 1,300-line Rust mail metadata
  module with one explicit parent route and 545 lines of local tests. Move RFC
  5322 header unfolding, RFC 2047 encoded words, RFC 2231 filenames, MIME part
  discovery, attachment summaries, and bounded text previews out of
  `preview.rs`. Accept only exact line-delimited MIME boundaries and preserve
  the 256 KiB prefix, 128-header, 8 KiB header-value, 64-parameter/encoded-word,
  five-name, 32-segment, 512-byte filename, four-level/32-part, 200-byte
  boundary, 1 MiB decoded-body, and 120-character preview budgets. Parse real
  CFB v3/v4 MSG files through bounded DIFAT/FAT, directory tree, regular stream,
  mini-FAT, and mini-stream chains; read fixed and Unicode MAPI properties,
  FILETIME values, recipients, and attachments. Truncated headers/directories,
  hostile sector chains, tree cycles, oversized mini properties, malformed
  Base64, false MIME delimiters, and excessive recursion all fail soft.
  Architecture, module-boundary, and performance guards lock the single route,
  implementation ownership, 1,300/550 line ceilings, parser budgets, real CFB
  semantics, and hostile-input tests.
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml mail` (14 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (260 passed, 1 external-corpus test ignored)
  - Verification: `dotnet msbuild native/QuickLook.Next.Native.proj -target:Build -property:Configuration=Release -verbosity:minimal`
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --disable-build-servers --maxcpucount:1` (363 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `47aa44f`, `94c2e0e`, `6d5a41c`, `57d4ad2`, `6ac3142`

- [x] `R26-P1-07c-3a` Complete the focused 375-line Rust CHM metadata
  module with one explicit parent route and 268 lines of local tests. Parse the
  real ITSF v2/v3 layouts (`0x58`/`0x60`), use the `0x48` directory offset and
  v3 `0x58` data base, derive the v2 data base with checked addition, validate
  the fixed ITSP/PMGL headers, and resolve section-zero `/#SYSTEM` offsets from
  that data base. Preserve the 8 KiB prefix, 12-entry, 260-byte name,
  32-stream scan/eight-result, 4 KiB system stream/eight-field, and eight-byte
  ENCINT budgets. Hostile offsets, truncated headers, out-of-range PMGL blocks,
  unterminated ENCINTs, and relative system-stream overflows all fail soft.
  Architecture and performance guards lock the module boundary, real offsets,
  checked arithmetic, budgets, and malformed-input tests.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::chm::tests` (8 passed)
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (252 passed, 1 external-corpus test ignored)
  - Verification: `dotnet msbuild native/QuickLook.Next.Native.proj -target:Build -property:Configuration=Release -verbosity:minimal`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --maxcpucount:1` (363 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `777c4fa`, `d7a3073`, `03d809f`, `2a14368`

- [x] `R26-P0-04` Close both layers of the recurring RasterHost error-window
  blocker. Preserve inherited no-dialog process policy, clear conflicting WER
  always-show-UI state, and exercise real DXGI/fail-fast child crashes without
  manipulating their windows. For the underlying PDF idle regression, prove the
  host remains alive for five seconds while its pipe is connected, serialize
  idle compaction against preview activation, serialize D3D ownership, track and
  drain open/page/animation/prepared-GIF workers, and use a process-atomic exit
  after terminal logical cleanup so asynchronous WinRT/driver teardown cannot
  raise the non-continuable bare `FACILITY_DXGI` exception. Every integration
  test that launches RasterHost now fails on timeout or a non-zero exit code.
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj -c Release --no-restore` (29 passed)
  - Verification: focused Release PDF idle/EOF regression repeated 3 times (3 passed; 0 new RasterHost `.NET Runtime`, `Application Error`, or WER events)
  - Live 2026-08-05 check: the only installed RasterHost was package
    `0.3.4.0` (`c5731c…`, built 2026-08-03), older than the current repository
    fixes; no new RasterHost WER/Application Error event or visible `WerFault`
    window was present after 10:55, and existing `.dmp` evidence was retained.
  - Verification: `pwsh -NoProfile -File tools/test-supervised-host-error-ui.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --disable-build-servers --maxcpucount:1` (363 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `06084f3`, `2ea91e1`, `1d76f11`, `8263146`, `93d0412`, `50294bb`, `e538869`, `613998c`, `a803eb6`, `a8e61f0`, `d6e79c0`, `616789e`, `17e7a88`

- [x] `R26-P1-07c-2f` Complete the media family split with a 92-line
  `preview/media/mod.rs` composition root. Move bounded file-prefix reading,
  container detection, base/output composition, and the stable
  MP4/MKV/WAV/FLAC/Ogg/ID3 order out of `preview.rs`; leave exactly one explicit
  `media::render_media_info` route and make it the media module's only
  parent-visible item. Keep the shared 1 MiB reader limit, fail-soft empty
  fallback, JSON envelope, private container/duration/codec helpers, explicit
  imports, no C ABI surface, and the 150-line composition ceiling under both
  architecture and performance guards. This also completes parent item
  `R26-P1-07c-2`.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::media` (29 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (245 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --disable-build-servers --maxcpucount:1` (361 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `5d427ae`, `0cb57be`
- [x] `R26-P1-07c-2e-3` Complete the ISO BMFF/MP4 split with a 998-line
  production module and 415 lines of focused tests. Move brand, track/header,
  sample-description, codec-payload, bitrate, rotation, summary, and stable text
  composition out of `preview.rs`; let `mp4.rs` call the sibling codec module
  directly and expose only `append_metadata`. Reduce `media/mod.rs` to one MP4
  adapter and 99 total lines, move shared timestamp formatting into
  `preview/common.rs`, and keep the upper renderer as a bounded read followed by
  MP4/MKV/WAV/FLAC/Ogg/ID3 composition. Reject zero or truncated sample entries,
  retain the 16-entry sample-description and 1,024-track/atom budgets, and lock
  the complete MP4 output order with an integration snapshot. Keep production
  and test ceilings at 1,200 and 500 lines while forbidding implementation
  backflow, wildcard imports, extra MP4 exports, and C ABI ownership. This also
  completes parent item `R26-P1-07c-2e`.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native mp4` (13 passed)
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native preview::common` (2 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (245 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --maxcpucount:1` (360 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `ef4d18a`, `0dc92d8`
- [x] `R26-P1-07c-2e-2` Move `stsz`, `stts`, `ctts`, `elst`, `stco`, `co64`,
  `stsc`, timeline summaries, and chunk mapping into the 849-line
  `preview::media::mp4` module behind one temporary track-summary adapter.
  Replace fixed-size `stsz` expansion with a borrowed/constant sample-size view
  and replace per-chunk full `stsc` scans with a monotonic cursor, covered by a
  65,000-entry near-1-MiB regression. Validate complete count/stride payloads,
  supported versions, 1-based strictly increasing `stsc` entries, non-zero
  samples and description indexes, signed composition/edit values, checked tick
  accumulation, complete sample/transition consumption, and checked chunk ends.
  Fail closed on malformed authoritative `co64` data instead of falling back to
  `stco`. Guard 100,000 timeline entries, 1,000,000 chunk/sample declarations,
  four retained chunk details, compact fixed samples, linear mapping, hostile
  tests, implementation backflow, explicit imports, no C ABI, and the existing
  1,200-line final MP4 ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native mp4` (12 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (244 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --maxcpucount:1` (360 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `d65ba97`, `ac2f0fd`
- [x] `R26-P1-07c-2e-1` Establish the 265-line `preview::media::mp4`
  module by moving bounded atom discovery/collection, movie-header duration and
  creation time, track-matrix rotation, and timescale conversion out of
  `preview.rs`. Unify 32-bit, extended-size, and end-of-range atom handling with
  checked offsets and conversions; cap recursive traversal at depth four and
  collected payloads at 1,024. Fail closed when MP4 epoch conversion exceeds
  `i64`, continue across valid empty sibling atoms, and cover malformed extended
  sizes, excessive nesting, collection pressure, and movie-header boundaries.
  Guard implementation backflow, these budgets and checked operations, explicit
  imports, no C ABI, the focused tests, and the final 1,200-line MP4 ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native mp4` (6 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (238 passed, 1 external-corpus test ignored)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Commits: `c280562`, `62725ac`, `4da1be0`
- [x] `R26-P1-07c-2d` Complete the 1,027-line `preview::media::codec` family
  module with three narrow payload APIs for `esds`, `avcC`, and `hvcC`; keep MP4
  atom location in the aggregation layer for the next slice. Move AVC SPS/VUI,
  HEVC VPS/SPS/VUI, parameter-set arrays, colour labels, and the private bit
  reader with their existing tests. Add five hostile-input tests and fix bit
  capacity multiplication, signed Exp-Golomb conversion, scaling-list signed
  arithmetic, and H.264/HEVC crop-offset overflows. Guard 32-bit reads,
  31-zero Exp-Golomb codes, 256-entry H.264 cycles, 12 bounded scaling lists,
  32 HEVC arrays, 256 NALs per array, seven sub-layers, checked offsets, private
  visibility, no C ABI, implementation backflow, and a 1,100-line ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native media::codec` (9 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-format-registry.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (234 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `818bbc9`, `917916d`, `9849a40`, `c48ede2`
- [x] `R26-P1-07c-2d-1` Establish the media codec module by moving MPEG-4
  descriptor scanning, AudioSpecificConfig decoding, object/sample-rate labels,
  and the AAC integration test out of `preview.rs` into a 151-line
  `preview::media::codec` module. Keep MP4 atom location in the aggregation layer
  behind a narrow payload adapter. Add hostile tests for unterminated four-byte
  descriptor lengths and truncated payloads; guard checked cursor/length
  arithmetic, bounded base-128 length decoding, forward progress, explicit
  imports, no C ABI, and the final codec-family line ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native media::codec` (2 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (229 passed, 1 external-corpus test ignored)
  - Commits: `818bbc9`, `917916d`
- [x] `R26-P1-07c-2c` Move Matroska/EBML metadata composition, bounded element
  traversal, track parsing, scalar decoders, and the valid-container test out of
  `preview.rs` into the 383-line `preview::media::matroska` module. Move the
  MP4/MKV codec-label formatter into the shared media module and add a hostile
  nested-container test proving traversal stops beyond depth six. Preserve
  clipped payload ends, forward-progress checks, saturating track counts,
  four/eight-byte EBML identifier/size limits, eight-byte integer reads, and
  stable metadata field order. Guard implementation backflow, these bounds,
  explicit imports, no C ABI, and a 460-line ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native media::matroska` (2 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (228 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `b2947a4`, `adfe461`
- [x] `R26-P1-07c-2b` Move ID3 tag/frame parsing, ordered metadata output,
  Latin-1/UTF-8/UTF-16 decoding, and two focused tests out of `preview.rs` into
  the 249-line `preview::media::id3` module. Retain the narrow media adapter and
  preserve version acceptance, tag-size clipping to the bounded input prefix,
  synchsafe high-bit rejection, checked frame ends, first-value precedence,
  output field order, and paired UTF-16 decoding. Guard implementation backflow,
  parser bounds, field order, explicit imports, no C ABI, and a 320-line ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native media::id3` (2 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `22ee20a`, `6f39949`
- [x] `R26-P1-07c-2a` Move RIFF/WAV, FLAC, and Ogg parsing plus four focused
  tests out of `preview.rs` into the 414-line `preview::media::audio` module.
  Establish a 65-line media composition module for signature-based container
  detection and shared duration formatting, while retaining temporary narrow
  adapters until the remaining media families move. Preserve the 1 MiB render
  read cap, MP4/MKV/WAV/FLAC/Ogg/ID3 output order, checked chunk/block offsets,
  eight-packet Ogg inspection budget, and bounded vendor text. Guard explicit
  imports, implementation backflow, parser bounds, output order, no C ABI, and
  150/500-line module ceilings.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native media::audio` (4 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-format-registry.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `770dcfd`, `18c7e01`, `65f6c1e`
- [x] `R26-P1-07c-1` Move font info rendering, SFNT/WOFF metadata parsing,
  and their focused tests out of the aggregation module into the 291-line
  `preview::font` module. Keep `preview.rs` as the explicit format router and
  preserve the existing 1 MiB read cap, table-count cap, checked offsets, JSON
  shape, and text labels. Extend the architecture guard with an explicit-import
  rule, implementation-backflow checks, required parser bounds, and a 350-line
  module ceiling.
  - Verification: `cargo test --locked --manifest-path native/Cargo.toml -p quicklook_next_native font` (2 passed)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-format-registry.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `dd77c6e`, `6044dcc`
- [x] `R26-P1-07b` Move Shell thumbnail flags, size/allocation validation,
  STA dispatch, COM image-factory calls, and HBITMAP/HDC ownership into the
  262-line `win32::shell_thumbnail` module. Keep all three C exports in `lib.rs`
  as typed-error adapters and retain the shared raster packet writer for
  package and Office images. Recursively inspect Rust FFI safety, enforce the
  module boundary and 400-line ceiling, and teach the thumbnail policy guard to
  inspect the implementation file instead of silently missing moved code.
  - Verification: `pwsh -NoProfile -File tools/test-rust-ffi-safety.ps1` (15 Rust files, 57 raw-pointer exports)
  - Verification: `pwsh -NoProfile -File tools/test-rust-module-boundaries.ps1`
  - Verification: `pwsh -NoProfile -File tools/guard-thumbnail-priority.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `pwsh -NoProfile -File tools/smoke-native.ps1 -BuildNative`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `637615e`, `18d83fb`
- [x] `R26-P1-07a` Remove crate-wide `too_many_arguments` and
  `type_complexity` allowances. Replace animation tuples and high-arity Office,
  SQLite, Android, archive, waveform, and compositing inputs with named Rust
  types. Keep the remaining 20 argument-count exceptions on two ABI-shaped
  production adapters and exact `call_*` test mirrors, with a recursive guard
  that rejects new crate-wide or parser-level exemptions.
  - Verification: `pwsh -NoProfile -File tools/test-rust-lint-scope.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `6fa5115`, `e60957a`
- [x] `R26-P1-06` Make tracked `AGENTS.md` the repository-wide Rust-first
  placement and safety contract, while leaving the user's ignored lowercase
  scratch guide untouched. Add Accepted ADRs for supervised Host processes,
  exact-object HANDLE ownership/path authority, generation-aware cancellation
  and bounded drain, and typed request-bound errors. Clarify that cancellation,
  disconnect, and service failure end a local await without fabricating a Host
  content error; record the current ShellBroker fail-stop cancellation exception
  and the still-open diagnostics work. Correct two historical roadmaps that
  assigned Shell thumbnails to RasterHost.
  - Verification: `pwsh -NoProfile -File tools/test-architecture-guidance.ps1`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-build --no-restore` (274 passed)
  - Verification: `dotnet format QuickLook.Next.slnx --verify-no-changes --no-restore --verbosity minimal`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commits: `9985ddd`, `de3e90a`, `6f1d494`
- [x] `R26-P1-05` Add one incremental Native MSBuild project and shared props
  contract for every Rust FFI consumer. Missing DLLs, stale Rust source/assets,
  changes to the build rule, Cargo failures, and successful Cargo runs without
  an output now fail closed. Four parallel consumers share one Cargo build,
  copy the verified DLL unconditionally, and pin the Cargo/PE target to
  `x86_64-pc-windows-msvc`; all local, release, long-cycle, and smoke entry
  points use the same build contract.
  - Verification: `dotnet restore QuickLook.Next.slnx --locked-mode --verbosity minimal`
  - Verification: `dotnet msbuild native/QuickLook.Next.Native.proj -target:Build -verbosity:minimal`
  - Verification: `pwsh -NoProfile -File tools/test-native-msbuild-dependency.ps1`
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --maxcpucount:1` (360 passed)
  - Verification: `dotnet format QuickLook.Next.slnx --verify-no-changes --no-restore --verbosity minimal`
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Result: the canonical DLL and all four Release consumer copies had identical
    SHA-256 hashes; the canonical PE machine was `0x8664` (x64).
  - Commits: `33d912b`, `738300d`, `fb6c518`, `51f60ab`
- [x] `R26-P1-01` Bind every preview error and its queued focus/action state to
  the failing path, generation, and cancellation token. Keep the previously
  committed path available for old-host cleanup while Retry, Open, and Reveal
  consume only the current `PreviewErrorContext`. Cover first-open failure,
  A-to-B early failure, old generations, same-path new generations, commit,
  close, and clear transitions.
  - Verification: `dotnet format QuickLook.Next.slnx --verify-no-changes --no-restore --verbosity minimal`
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-restore --nologo --maxcpucount:1` (360 passed)
  - Verification: `pwsh -NoProfile -File tools/guard-stale-callbacks.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-supervised-host-error-ui.ps1`
  - Commit: `f6f3355`
- [x] `R26-P0-02` Introduce a checked PowerShell child-script boundary that
  clears stale exit state, captures `$?` and `$LASTEXITCODE` immediately, and
  throws before later work can erase a failure. Apply it to nested guards,
  formal release/package workflows, the composite release action, local build
  and MSIX workflows, and long-cycle steps. Fault injection verifies that a
  child exiting 23 prevents downstream scripts from running.
  - Verification: `pwsh -NoProfile -File tools/test-checked-invocation.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-pack-msix-version.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-pack-release-failfast.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-release-workflows.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-build-local.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-local-msix-update.ps1`
  - Commits: `04976d0`, `6786de8`, `632b6f1`
- [x] `R26-P0-01` Bound Shell thumbnail requests and returned HBITMAP layouts to
  512 pixels, use checked byte arithmetic and fallible allocation, require a
  complete `GetDIBits` row count, reuse the checked raster writer, and release
  HBITMAP/HDC resources with RAII on all exits.
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --locked --workspace --all-targets --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `cargo test --locked --workspace --manifest-path native/Cargo.toml` (227 passed, 1 external-corpus test ignored)
  - Verification: `pwsh -NoProfile -File tools/smoke-native.ps1 -BuildNative`
  - Commit: `3f5d1ce`
- [x] Run the post-batch architecture gate in a normal Windows user context,
  including checked-invocation fault injection, restricted-host launch, native
  external image corpus, system image codecs, localization, FFI, performance,
  stale-callback, thumbnail-priority, format-registry, and title-bar guards.
  - Verification: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist`
  - Result: system image smoke decoded 4/5 optional formats; JPEG, AVIF, and HEIC
    passed, while the installed system codec did not provide JPEG XL support.
  - Queue commit: `c64ffa3`

- [x] Synchronize `VERSION`, the native crate manifest, and `Cargo.lock` at 0.3.4. The local
  release workflow passed the 224 native tests with one external-corpus test ignored in the
  ordinary suite, the complete .NET solution tests, and the release build before writing a
  hash-bound tested-payload proof for commit `c5731cd`. Independently validate the generated
  installer checksum and eight-entry layout, the MSIX 0.3.4.1 identity, valid pinned signature,
  required payload, and ten key package-to-`dist` hashes. Install the same MSIX over 0.3.3.1 for
  the current user; AppX reports publisher `CN=QuickLook Next Development` and status `Ok`.
  - Test/package workflow: `pwsh -NoProfile -File build.ps1 -NoRestore -Install` (the orchestration timeout occurred after proof/package creation while deleting generated staging directories; installation was completed separately from the already-validated artifact)
  - Verification: `pwsh -NoProfile -File tools/test-release-version.ps1 -ExpectedVersion 0.3.4`
  - Verification: `Get-AuthenticodeSignature`, read-only installer/MSIX manifest/payload/hash validation, `Add-AppxPackage -ForceApplicationShutdown`, and `Get-AppxPackage`
  - Artifact: `artifacts/QuickLook.Next-0.3.4.1-win-x64.msix` (SHA-256 `3B9FA435203CB4177BCDF1E6E29CFC3DBC34E5F680E03D0731CF35F6E8BD265A`)
  - Artifact: `artifacts/QuickLook.Next-Installer-0.3.4.1-win-x64.zip` (SHA-256 `0E7884CCFF9282A215151E61329716412FB83128226CBA2B82A29ADFA0BA7BFA`)
  - Commit: `c5731cd`

- [x] Remove the GIF initial-motion stall reproduced with the 8,258,096-byte, 652x909, 75-frame
  user sample. RasterHost now starts the exact-object static first frame and an independent GIF
  animation decode concurrently only when the App requests animation preparation. Optional ABI 3
  capability bit 20 allocates the final anonymous section at its exact size after one native decode;
  older ABI 3 libraries retain the stable path/HANDLE fallback. The App keeps the static surface
  visible until the first animation bitmap is populated, then advances the absolute playback
  timeline by the handoff time instead of replaying frame zero. Rust, RasterHost, Shell fallback,
  and App presentation all bypass RGB-waveform work for GIF while animated WebP/APNG scopes remain.
  At the 395x551 application target the real sample retained all 75 frames in a 65,293,812-byte
  packet (62.27 MiB), below the unchanged 64 MiB payload ceiling; the static HANDLE first frame
  remained waveform-free and was ready in 20-25 ms during the concurrent-chain measurements.
  - Verification: `cargo test --workspace --release --locked --manifest-path native/Cargo.toml` (224 passed, 1 external-corpus test ignored)
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore` (258 passed)
  - Verification: focused RasterHost GIF/static-image integration tests (9 passed)
  - Guards: `tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`,
    `tools/guard-performance-bounds.ps1`
  - Commit: `e85623d`

- [x] Restore structured Markdown body visibility in both light and dark modes by applying dynamic
  theme resources to virtualized prose, lists, code, links, tables, and fallback content. Outline
  clicks now realize the stable virtual item through at most three render-version-safe UI turns and
  align the selected heading's top edge to the viewport inset; bounded trailing space makes even the
  last heading align without creating a blank tail for documents that have no outline.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore` (258 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commits: `2568c6b`, `727d9ff`

- [x] Parse PowerPoint page titles from explicit title placeholders and from slide-layout/master
  placeholder inheritance, including `title`, `ctrTitle`, and `vertTitle` families plus inherited
  geometry. A bounded top-text fallback rejects footer/date/slide-number auxiliaries, and the title
  is removed once from the page summary without removing its rendered layout item. PPTX and PPTM
  cross-process coverage now asserts the parsed page title rather than `Slide N`.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml ppt_` (9 passed)
  - Verification: ParserHost integration filter `Generated_xlsx_pptx_and_pptm_return_office_layouts` (1 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Commits: `24f5d5d`, `ffe8041`

- [x] Synchronize `VERSION`, the native crate manifest, and `Cargo.lock` at 0.3.3 after the exact
  release harness passes canonical Rust/.NET formatting, warning-free Clippy, native debug/release
  builds, 218 native tests with one external-corpus test ignored in the ordinary suite, all 323 .NET
  tests, the separate 10-file external image corpus, restricted-host smoke, and every architecture,
  performance, release, and title-bar guard. Produce signed local MSIX/installer artifacts from the
  tested `b76a4ab` payload and install `0.3.3.1` for the current user with the pinned development
  certificate; AppX reports publisher `CN=QuickLook Next Development` and status `Ok`.
  - Verification: `pwsh -NoProfile -File tools/release.ps1 -ExpectedVersion 0.3.3 -SkipPackage -SkipSystemImageSmoke`
  - Verification: `pwsh -NoProfile -File tools/test-release-version.ps1 -ExpectedVersion 0.3.3`
  - Artifact: `artifacts/QuickLook.Next-0.3.3.1-win-x64.msix` (SHA-256 `8FA1B6C821074CBD168979047C85CA5A721AC5A4C4B9C13B03F08A935B73D014`)
  - Artifact: `artifacts/QuickLook.Next-Installer-0.3.3.1-win-x64.zip` (SHA-256 `2BEA9C82180F163E38E169CDB1808404F055FFE30E9616D697BA527930D3DC8A`)
  - Commit: `b76a4ab`

- [x] Remove the periodic native-animation playback stall reproduced with the 75-frame, 110 ms GIF.
  Drive frame selection from `CompositionTarget.Rendering` and one absolute monotonic timeline so a
  delayed UI callback catches up without timer drift. Keep the dynamic RGB scope while allowing its
  immutable section scan to run concurrently with frame upload; pool the 216 KiB histogram workspace,
  cache each sampled frame's scope, reuse the scope staging pixels and `WriteableBitmap`, avoid the
  per-frame span closure, and stop reassigning the same animation bitmap source. Pause, clear, and stop
  detach the compositor callback before releasing the mapped section.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore` (241 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore` (0 warnings, 0 errors)
  - Verification: `dotnet format QuickLook.Next.slnx --verify-no-changes --no-restore --verbosity minimal`
  - Guard: `pwsh -NoProfile -File tools/guard-performance-bounds.ps1`
  - Commit: `8cc0022`

- [x] Synchronize `VERSION`, the native crate manifest, and `Cargo.lock` at 0.3.2 after the exact
  release harness passes canonical formatting, warning-free Clippy, native debug/release builds,
  218 native tests with one external-corpus test ignored in the ordinary suite, all 316 .NET tests,
  the separate 10-file external image corpus, restricted-host smoke, and every architecture/release
  guard including the new title-bar inset contract.
  - Verification: `pwsh -NoProfile -File tools/release.ps1 -ExpectedVersion 0.3.2 -SkipPackage -SkipSystemImageSmoke`
  - Verification: `pwsh -NoProfile -File tools/test-release-version.ps1 -ExpectedVersion 0.3.2`
  - Commit: `1b93ce5`

- [x] Replace fixed 140/144 DIP custom-title-bar padding in Main, Settings, and Welcome with one
  lifecycle-safe controller driven by `AppWindow.TitleBar.LeftInset/RightInset` and
  `XamlRoot.RasterizationScale`. Coalesce AppWindow/XamlRoot updates on the Dispatcher, detach every
  event on close, and preserve the windows' symmetric design padding. A pure Core policy covers
  invalid inputs, fractional conversion, and equivalent 100%/150%/200% physical insets; the
  architecture guard locks all three window integrations and the traceable manual evidence gate.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore` (234 passed)
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore /p:ContinuousIntegrationBuild=true` (0 warnings, 0 errors)
  - Guard: `pwsh -NoProfile -File tools/test-titlebar-insets.ps1`
  - Guard: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Manual evidence contract: `docs/titlebar-visual-check.md` (not yet claimed as executed)
  - Commit: `9018b4c`

- [x] Synchronize `VERSION`, the native crate manifest, and `Cargo.lock` at 0.3.1 after the exact
  release harness passes formatting, warning-free Clippy, native debug/release builds, 218 native
  tests, all 297 .NET tests, the 10-file external image corpus, and every architecture/release guard.
  - Verification: `pwsh -NoProfile -File tools/release.ps1 -SkipPackage -SkipSystemImageSmoke`
  - Verification: `pwsh -NoProfile -File tools/test-release-version.ps1 -ExpectedVersion 0.3.1`
  - Commit: `a3d1639`

- [x] Establish a zero-warning formatting and static-analysis baseline. Formal release/CI checks
  now require canonical `rustfmt`, unchanged `dotnet format`, warning-free Clippy across all
  targets/features, and warnings-as-errors for Release/CI .NET builds. Every one of the 55
  raw-pointer native exports is now an explicit documented `unsafe extern` contract, protected by
  a structural architecture guard.
  - Verification: `cargo fmt --all --manifest-path native/Cargo.toml -- --check`
  - Verification: `cargo clippy --workspace --all-targets --all-features --locked --manifest-path native/Cargo.toml -- -D warnings`
  - Verification: `dotnet format QuickLook.Next.slnx --verify-no-changes --no-restore --verbosity minimal`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore /p:ContinuousIntegrationBuild=true`
  - Verification: `cargo test --workspace --locked --manifest-path native/Cargo.toml` (218 passed,
    1 external corpus test ignored)
  - Guard: `pwsh -NoProfile -File tools/test-rust-ffi-safety.ps1`
  - Commits: `df65526`, `0610063`, `9fb2ae5`

- [x] Keep supervised RasterHost, ParserHost, and ShellBroker crashes
  non-interactive. Apply the shared process policy before initialization, request
  WER no-UI reporting, and log the exact RasterHost process/exit code before
  restart. `R26-P0-04` later added the fail-closed no-GP-fault-box fallback after
  a real DXGI `Application Error` dialog exposed the WER-only gap.
  - Verification: `dotnet build QuickLook.Next.slnx -c Debug --no-restore`
  - Guard: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commit: `5d771ee`

- [x] Add an opt-in `build.ps1 -Package` path that runs the release-oriented tests and creates
  signed local MSIX/installer artifacts without inspecting, stopping, or updating the installed
  AppX. Formal release resolution now synchronizes `VERSION`, the native manifest, and `Cargo.lock`
  transactionally instead of writing only `VERSION`.
  - Guards: `tools/test-build-local.ps1`, `tools/test-local-msix-update.ps1`,
    `tools/test-local-msix-version.ps1`, `tools/test-formal-msix-version.ps1`,
    `tools/test-set-version.ps1`, `tools/test-release-workflows.ps1`
  - Commits: `d6aa6a3`, `a2eaca2`

- [x] Make packaged and unpackaged auto-start queries and updates asynchronous so Settings and tray
  interactions never synchronously wait for WinRT, registry, file, or shortcut work.
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore`
  - Commit: `eec7f66`

- [x] Keep nested long-cycle checks in the active PowerShell 7 host and document `pwsh` commands
  consistently, avoiding UTF-8 parsing failures from Windows PowerShell 5.1.
  - Guard: `pwsh -NoProfile -File tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Commit: `0eb4675`

- [x] Reconcile review-facing PDF cache documentation with the implemented bounded five-page
  offscreen surface cache and the existing disk-cache behavior.
  - Verification: documentation review plus `tools/guard-architecture.ps1`
  - Commit: `b1694d3`

- [x] Generate and package complete third-party dependency notices for the .NET publish graph,
  Windows App SDK redistribution, and statically linked Rust dependencies. Release payload guards
  require the notice in both the MSIX and installer archive.
  - Verification: `pwsh -NoProfile -File tools/test-pack-release-failfast.ps1`
  - Verification: `pwsh -NoProfile -File tools/test-release-payload-proof.ps1`
  - Commit: `101adf0`

- [x] Add a focused manual version/build/update workflow. Root `build.ps1` treats `VERSION` as the
  authoritative semantic version, transactionally synchronizes the Rust manifest and lock package,
  bypasses stale persistent .NET build servers, and prints the local App path. `-Test` covers the
  Release Rust workspace and all .NET projects. The explicit `-Install` path enables those tests,
  writes the same hash-bound proof used by formal packaging, selects a monotonic `X.Y.Z.N` MSIX
  revision, signs with the pinned existing identity, and updates only the current user's matching
  package without uninstall or downgrade behavior. Normal builds never change the installed MSIX.
  Local, beta, and stable revisions occupy ordered ranges so every same-base forward channel
  transition remains a valid MSIX upgrade.
  - Verification: `pwsh -NoProfile -File build.ps1 -NoRestore -Test`
  - Verification: `pwsh -NoProfile -File tools/pack-msix.ps1 -Version 0.3.0.0 -SkipBuild -SkipSystemImageSmoke`
  - Guards: `tools/test-set-version.ps1`, `tools/test-build-local.ps1`,
    `tools/test-local-msix-version.ps1`, `tools/test-formal-msix-version.ps1`,
    `tools/test-local-msix-update.ps1`

- [x] Remove the ParserHost cold-start/JSON readiness race. ParserHost connects its authenticated
  pipe before native ABI initialization; App supervision uses a 15-second connect/ready budget and
  does not publish a generation as connected until `ParserReady` completes. The restricted smoke
  uses a physical `.bin` plus a nonexistent logical `.json` and asserts exact JSON content/language,
  while integration coverage routes the same logical format through the HANDLE ABI.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release --no-restore`
  - Guard: `tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`

- [x] Remove App-side per-frame animation arrays. The App now owns one read-only duplicate of the
  validated RasterHost frame section for the playback lifetime, stores only bounded delay/offset
  descriptors, and writes each `ReadOnlySpan<byte>` directly to the fixed WinRT pixel buffer.
  Waveform reads and section disposal use the same lifetime lock; stop, clear, stale-result,
  exception, and unadopted-result paths all release the mapping.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Eliminate App-process Windows Property Handler reads and replace the image sidecar with three
  optional, path-free RasterHost HANDLE readers. Capability bit 19 gates bounded Rust typed JSON;
  missing fields are supplemented by the fixed System32 photo Property Handler over a read-only
  `IInitializeWithStream` object and then WIC over a HANDLE-backed WinRT stream. The readers run in
  parallel with field precedence `native > Property Handler > WIC`; the Property Handler module is
  loaded directly from System32 rather than COM registration and is retained for the Host lifetime.
  RasterHost enforces a 1.5-second child budget, single-worker reader gates, bounded input/output/
  property/string/read counts, and a 250-ms Property Handler drain fail-stop. The first surface
  remains independent. WIC work runs behind the same hard watchdog and exits the isolated host with
  code 33 if cancellation cannot drain within 250 ms. Integration coverage proves physical `.bin`
  files with nonexistent logical
  `.png`/`.bmp` names return metadata from the exact retained object after parent close; a direct
  Property Handler test returns 1x1/96-DPI BMP fields; a missing parent fails closed.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj -c Release --no-restore`
  - Guard: `tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Extract the bounded JPEG/PNG/GIF/WebP/TIFF image-metadata family from the monolithic native
  `preview.rs` into `preview/image_metadata.rs`. The parent retains only the stable public/path and
  HANDLE reader exports; parsing helpers and metadata DTO stay private to the focused module.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`

- [x] Extract bounded GIF/WebP/APNG animation classification and Torrent/bencode parsing into the
  focused `preview/animation_probe.rs` and `preview/torrent.rs` child modules. The parent keeps only
  stable routing/re-exports and shared bounded Reader helpers; the 4-MiB animation probe, full-scan
  static decision, 16-MiB Torrent input, depth-64, and node-100000 limits remain guarded.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Guard: `tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Extract executable/PE/CLR/AuthentiCode and EPUB/FB2 ebook parsing into focused
  `preview/executable.rs` and `preview/ebook.rs` modules. Shared bounded binary readers now live in
  `preview/common.rs`; the parent retains only stable path/Reader exports plus the few Windows
  version helpers reused by minidump metadata. Existing HANDLE position, invalid-HANDLE, bounded
  decompression, PE resource, AuthentiCode, CLR metadata, EPUB, and FB2 tests compile against the
  same implementations after the split.
  - Verification: `cargo test --locked -p quicklook_next_native --lib`
  - Verification: `cargo check --locked -p quicklook_next_native`

- [x] Add `tools/benchmark-handle-handoff.ps1`, a bounded same-process microbenchmark that compares
  an exact `ReOpenFile` retained-HANDLE lease with the former full `WriteThrough`/`Flush(true)`
  anchor copy. It reports latency and bytes written, uses 32 MiB/five iterations by default, and
  validates its exact system-temp child before recursive cleanup. It is not an end-to-end IPC
  benchmark.
  - Verification: `pwsh -NoProfile -File tools/benchmark-handle-handoff.ps1`

- [x] Stream archive entries directly into an App-owned bounded output HANDLE. The App creates the
  final zero-length child anchor, duplicates write authority into ParserHost, and recycles the Host
  if that transferred value cannot be delivered. ParserHost adopts the HANDLE before validating the
  envelope, resolves a retained parent lease or bounded compatibility source, and calls capability
  bit 18 `ql_extract_archive_entry_to_output_handle`. Rust validates/reopens both disk HANDLEs,
  streams checked 64 KiB chunks under the existing 64 MiB/1,000:1/four-second limits, and reports
  the exact byte count. ParserHost closes write authority before replying; the App transitions the
  same object to a strict read-only anchor. No Rust/ParserHost child temp path, Host-owned output
  HANDLE response, App `CopyTo`, or forced disk flush remains.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release --no-restore`
  - Guard: `tools/guard-architecture.ps1 -SkipDist -SkipSystemImageSmoke`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Generate native static-image waveform density in the final Rust pixel-conversion loop and
  return it through additive capability bit 17 and an exact packet contract. PNG/JPEG/BMP/TIFF/
  WebP and SVG accumulate the fixed 192x96 planar RGB density while producing premultiplied BGRA,
  with a one-million-sample ceiling and no second BGRA scan. RasterHost publishes the surface and
  readiness before the optional waveform message. WIC/system/older-native paths retain the bounded
  managed fallback.
  - Verification: `cargo test --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj -c Release --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Remove Office layout images from the control-channel payload. Rust now emits parent-bound
  canonical `imageRef`/`imageByteLength` metadata, advertises HANDLE capability bit 16, and exposes
  `ql_extract_office_layout_image_handle` for bounded lazy decode. ParserHost snapshots the exact
  published ref whitelist onto the retained Office source, acquires an independent lease for each
  child, and returns checked BGRA through an unnamed section owned until close/failure/replacement/
  disconnect. The App binds the response to the captured Host generation, duplicates only
  `SECTION_MAP_READ`, maps and validates the exact packet, and closes the remote owner. Office page
  materialization starts ref-deduplicated requests through a two-slot gate, cancels them with the
  preview session, and uploads BGRA to `WriteableBitmap`; legacy `ImageBase64` remains accepted only
  for compatibility with older native JSON. The maximum 18-large-image Rust regression stays below
  the 4 MiB pipe limit, and a 32-cycle ParserHost regression bounds HANDLE growth and proves close
  and disconnect cleanup without an Office-image temp directory.
  - Verification: `cargo test --workspace --release --locked`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release --no-restore`
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore`
  - Guards: `tools/guard-architecture.ps1 -SkipDist`,
    `tools/guard-performance-bounds.ps1`

- [x] Extract the bounded Text/Markdown/CSV/TSV family from the monolithic native
  `preview.rs` into `preview/text.rs`, including its format registry, Unicode truncation,
  Markdown AST, delimited-table budgets, and nine focused tests. The parent module now exposes only
  the four reader/path routing entry points while the broader format-family split remains ongoing.
  - Verification: `cargo test --locked --lib preview::text::tests`
  - Verification: `cargo check --workspace --locked`
  - Guard: `tools/guard-performance-bounds.ps1`

- [x] Remove animation and Hero raster packet files from the hot path. Rust now writes bounded
  GIF/WebP/APNG frame packets directly into RasterHost-owned anonymous sections and bounded
  Office/package Hero packets into ParserHost-owned anonymous sections. The App duplicates only
  `SECTION_MAP_READ`, maps the exact claimed length, validates packet geometry/layout, and
  acknowledges the Host owner. Close, failed publication, replacement, and disconnect release the
  remote section; an already-mapped App view remains independently readable. Legacy
  `raster-animation` and `parser-raster` writable directories are no longer created.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj -c Release --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release --no-restore`
  - Guard: `tools/guard-architecture.ps1`

- [x] Move GIF/WebP/APNG animation candidacy into the bounded Rust file probe with tri-state
  `isAnimated` metadata, keep unknown metadata backward compatible, and give retained-HANDLE
  animation decoding an independent 20-second timeout that preserves the static fallback.
  RasterHost integration tests verify that the first and last decoded frames differ.
  - Verification: `cargo test --workspace --release --locked`
  - Verification: `dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore`

- [x] Publish bounded RAR4/RAR5 HANDLE previews as explicit browse-only listings, explain that
  limitation in the listing summary and row interaction, and cover the production ParserHost
  boundary with a real RAR5 fixture whose physical path has no `.rar` extension.
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj -c Release --no-build --no-restore`

- [x] Replace single-color folder/archive fallback glyphs with reusable theme-aware multi-layer
  vector icons in both listing rows and Hero surfaces, while retaining Shell raster replacement for
  real filesystem rows.
  - Verification: `dotnet build QuickLook.Next.slnx -c Release --no-restore`
  - Guard: `tools/guard-architecture.ps1`

- [x] Publish the project under the MIT License with `SherlockChiang` as the copyright holder, add
  MIT metadata for .NET, Rust, and the website package, and accept inbound contributions under the
  same MIT terms while requiring contributors to have the rights to submitted code and assets.

- [x] Publish `SECURITY.md` and `CONTRIBUTING.md` with private vulnerability reporting, sensitive
  sample handling, supported-version scope, pinned-toolchain setup, locked verification commands,
  architectural expectations, dependency policy, and pull-request requirements.

- [x] Add explicit cloud hydration with localized consent, throttled byte/percentage progress,
  preview-generation cancellation, a 45-second open/read timeout, and a 256 MiB application-read
  policy. Declared size is queried through cancellable WinRT metadata only after consent; oversized
  placeholders are deferred before content open, while changing lengths are stopped by a cumulative
  limit with a one-byte overflow probe and no sequential read-ahead.

- [x] Resolve Android binary manifest/resource-table icon references and compose bounded adaptive
  icons. Manifest aliases and obfuscated resource IDs resolve to density-aware foreground/background
  candidates; adaptive layers are transformed, masked, and scaled under existing package input,
  dimension, decompression, and raster packet budgets. Related commits: `c02ca58`, `372c8e9`,
  `97cb70c`, `0adc06c`.

- [x] Move preview Shell thumbnail fallback out of RasterHost into a dedicated write-restricted
  ShellBroker. Only explicit cloud/legacy path images that already failed RasterHost decoding may
  call it. The authenticated broker returns a maximum 512x512 BGRA packet through a broker-owned
  read-only HANDLE; the App pulls and validates that packet, then explicitly closes the handoff.
  RasterHost no longer links `NativeThumbnail` or `ql_get_thumbnail` and HANDLE requests cannot
  reach the broker.

- [x] Confirm and guard the existing App-pulled D3D surface boundary. RasterHost publishes only a
  host-local handle value and transfer ID; the App duplicates from its existing host process handle,
  closes any failed App-local copy, and always acknowledges the host transfer. RasterHost has no
  `OpenProcess`, `PROCESS_DUP_HANDLE`, or App process handle authority; `Hello.AppProcessId` is used
  only to compare the authenticated named-pipe server PID.

- [x] Launch ParserHost with a Windows write-restricted token using Restricted Code and World
  restricting SIDs. `WRITE_RESTRICTED` consults them only for write access: World permits CLR/BCrypt
  kernel-object initialization, while only the random authenticated pipe and per-launch writable
  root receive an explicit Restricted Code grant. Smoke coverage now preserves paths containing
  spaces and proves allowed-root writes, pipe I/O, native-DLL loading, and HANDLE parsing succeed,
  while writes to ordinary temp and LocalAppData roots fail. RasterHost remains on the
  privilege-stripped profile until its WinRT and explicit Shell compatibility paths are prepared.
  AppContainer and network denial remain open.

- [x] Add a 32-cycle parent-bound package hero regression using a stable adaptive icon. Every
  extraction produces a 512x512 BGRA packet of roughly 1 MiB in a ParserHost-owned anonymous
  section, transfers only read access to the App, never creates a `parser-raster` directory, and
  leaves the App mapping independently readable after the Host owner closes. Parent leases and host
  HANDLE growth remain bounded until the retained preview closes.

- [x] Add a 32-cycle parent-bound archive extraction regression. One retained archive HANDLE remains
  authoritative while each operation acquires an independent lease and writes through a transferred
  caller-owned output HANDLE. The App-owned object remains readable after ParserHost closes write
  authority, no extraction temp root appears, Host HANDLE growth stays bounded, and the parent source
  unlocks only after its preview closes. The 8 MiB inflight-close regression suppresses canceled
  responses, releases the output HANDLE, and proves the Host can accept the next preview.

- [x] Add repeated HANDLE-backed PDF session and page-render resource coverage. Every cycle uses a
  distinct bounded file identity, opens one session, renders and copies one page surface, releases
  the transfer and page, closes the session, and verifies the source unlocks. The test proves at
  least 4 MiB of measured cache growth, then requires idle trim to return HANDLEs and private bytes
  to fixed warmed-baseline budgets.

- [x] Verify system-codec resource recovery after repeated HANDLE previews. The RasterHost test
  exercises WIC PNG decoding until its deferred WinRT resources measurably exceed the warmed HANDLE
  baseline, then waits for a one-second test-only idle trim and requires HANDLEs to return within a
  fixed recovery budget. Private bytes must also return within 32 MiB of the warmed baseline, allowing
  bounded allocator/runtime retention. Idle trim now waits for pending finalizers and performs a
  second collection; its production 120-second threshold and 15-second check interval remain unchanged.

- [x] Add short-cycle ParserHost and RasterHost resource regressions. Each test warms one host,
  repeatedly transfers pinned local inputs through the HANDLE protocol, closes every request,
  verifies immediate source-file release, and rejects HANDLE-count growth beyond a fixed runtime
  fluctuation budget. RasterHost receives a separate 16-cycle native/D3D warm-up before its
  32 measured native-ICO cycles, which also release every transferred shared surface. This isolates
  deterministic source/surface ownership from WIC projection objects that are reclaimed by GC.

- [x] Remove the obsolete ParserHost `parser-input` compatibility layer. Local certificate,
  SQLite, and native parser requests now consume only adopted HANDLEs; unsupported HANDLE kinds
  fail closed, and ParserHost no longer creates or requires the `parser-input` writable directory.

- [x] Add a bounded RGB spatial waveform to the persistent right-side image
  details rail. RasterHost derives a fixed 192x96 three-channel density scope
  from its already-bounded decoded BGRA raster, including alpha unpremultiplication
  and a one-million-sample ceiling, so raw image pixels still do not cross the
  control channel. Static and animated image viewers support clamped drag-to-pan,
  recover cleanly from pointer cancellation/capture loss, and wheel zoom around
  the pointer rather than the viewport center. Labels and automation names are
  localized in en-US and zh-CN.
  - Verification: Release App build 0 warnings via installed 10.0.302 MSBuild because pinned SDK 10.0.301 was unavailable
  - Verification: Core 114/114; RasterHost integration 7/7 via MSBuild `VSTest`
  - Regression tests/guard: `4acc9e9`
  - Accessibility/keyboard follow-up: `ad64ac3` adds RGB legend and intensity scale,
    localized scope/viewer HelpText, and bounded `Shift+Arrow` panning without
    replacing unmodified arrow-key sibling navigation.
  - Animated scope follow-up: `6692557` updates native-animation waveforms from
    the presented frame at no more than 10 Hz, computes them off the UI thread,
    rejects stale generations, and centralizes null/shape/payload validation in
    Core with malformed-protocol tests.
  - Guard: `tools/guard-architecture.ps1` passed through static/native/image-corpus stages; final system-image smoke remained blocked by missing pinned SDK 10.0.301
  - Commit: `9321e68`

- [x] Apply a fail-closed conservative creation-time mitigation profile to both
  hostile-format hosts: DEP, SEHOP, heap-terminate-on-corruption, bottom-up/high-
  entropy ASLR, and extension-point disable. Add job UI restrictions for clipboard
  access, display/system-parameter changes, desktop switching, and ExitWindows,
  while preserving suspended-create, job-assign-before-resume, one-process and
  memory limits. Runtime smoke now queries effective DEP/ASLR/extension-point and
  job policies with policy-specific exit codes. ParserHost subsequently gained a write-restricted
  SID and dedicated output/pipe ACLs; AppContainer and enforced network denial remain open, while
  RasterHost WinRT/WIC/Shell paths still require a compatibility split.
  - Verification: Release and Debug App builds 0 warnings via installed 10.0.302 MSBuild because pinned SDK 10.0.301 was unavailable
  - Verification: `tools/smoke-restricted-host-launch.ps1`
  - Verification: ParserHost integration 15/15; RasterHost integration 7/7
  - Guard: `tools/guard-architecture.ps1` passed through static/native/image-corpus stages; final system-image smoke remained blocked by missing pinned SDK 10.0.301
  - Commit: `0a4f63c`

- [x] Complete large-content presentation virtualization across code/plain text,
  structured Markdown, and CSV/TSV tables. Structured Markdown now uses a
  data-only flattened Core model and recycled `ListView` containers; list items
  and table rows are independent virtual items, outline navigation uses stable
  item indices, and search highlights are reapplied to realized blocks/cells.
  Preserve inline styles, bounded fenced-code syntax, code copy, table cell
  selection, partial notices, and the shared 2000-item/4096-cell budgets. Raw
  Markdown compatibility fallback remains separately bounded at 256 KiB.
  - Verification: App build 0 warnings via installed 10.0.302 MSBuild because pinned SDK 10.0.301 was unavailable
  - Verification: Core 108/108, ParserHost 15/15, RasterHost 7/7 via MSBuild `VSTest`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Guard: `tools/guard-architecture.ps1` passed through all static/native/image-corpus stages; final system-image smoke remained blocked by missing pinned SDK 10.0.301
  - Related commits: `fc17770` (code/text), `5670daa` (tables), `1a21a63` (structured Markdown)

- [x] Extend the viewport-virtualized CSV/TSV presenter with continuously sticky
  column and row headers while preserving the 1024-data-cell viewport budget and
  avoiding body reconstruction during intermediate scrolling. Expand the native
  represented prefix from 160 to at most 4000 rows under independent 65,536-cell
  and 512 KiB retained-character budgets, and defensively reapply the same policy
  to untrusted host models in Core. Structured Markdown remains the final part of
  the broader P1 virtualization item.
  - Verification: `cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml` (115 passed, 1 ignored)
  - Verification: Core tests 107/107 and ParserHost integration 15/15 via the installed 10.0.302 MSBuild `VSTest` target
  - Verification: App build 0 warnings via installed 10.0.302 MSBuild because pinned SDK 10.0.301 was unavailable
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `5670daa`

- [x] Virtualize code and plain-text presentation with recycled `ListView` rows.
  Present the complete native bounded payload instead of truncating again at
  256 KiB, retain cross-line syntax state through full-text tokenization, cap
  each realized row at 512 syntax runs, and keep row models free of WinUI objects.
  Add exact mixed-newline line indexing, exact search `ScrollIntoView`, recycled
  search highlights, live wrap/line-number state, and ordered selected-line copy.
  Structured Markdown virtualization and table model/header enhancements remain
  under the broader P1 virtualization item.
  - Verification: `dotnet "C:/Program Files/dotnet/sdk/10.0.302/MSBuild.dll" src/QuickLook.Next.App/QuickLook.Next.App.csproj -t:Build -p:Restore=false -verbosity:minimal` (0 warnings; installed SDK fallback because pinned 10.0.301 was unavailable)
  - Verification: Core tests 106/106 via the same MSBuild `VSTest` target
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `fc17770`

- [x] Raise explicit, localized WinUI automation notifications for preview
  loading, placeholder progress, success, terminal failure, and PDF page errors.
  Use one stable activity channel, coalesce routine progress, prioritize failures,
  and discard queued announcements after the preview generation changes. Keep
  search and listing updates on their existing visible live regions.
  - Verification: `dotnet "C:/Program Files/dotnet/sdk/10.0.302/MSBuild.dll" src/QuickLook.Next.App/QuickLook.Next.App.csproj -t:Build -p:Restore=false -verbosity:minimal` (0 warnings; installed SDK fallback because pinned 10.0.301 was unavailable)
  - Verification: Core 105/105, ParserHost 15/15, RasterHost 7/7 via the same MSBuild `VSTest` target
  - Guard: `tools/guard-architecture.ps1` static/native/image-corpus stages passed; final system-image smoke was blocked by missing pinned SDK 10.0.301
  - Manual follow-up: Narrator listening verification remains open.
  - Commit: `eabfef2`

- [x] Isolate RasterHost preview anchors by host PID so concurrent hosts cannot
  remove each other's exact-object input files during startup cleanup.
  - Verification: ParserHost integration 15/15 and RasterHost integration 7/7 executed concurrently via MSBuild `VSTest`
  - Commit: `ad057f6`

- [x] Complete en-US and zh-CN coverage for preview status, metadata, page counts,
  syntax/truncation notices, visual labels, tooltips, and MainWindow automation
  names. Keep product names, language autonyms, file content, format identifiers,
  and internal diagnostics intentionally invariant. Guard locale key parity,
  format-placeholder parity, `UiStrings` resource presence, and localized XAML
  automation/tooltip properties.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `ceb721b`

- [x] Add handle-backed RasterHost PDF integration coverage for open metadata,
  invalid-page errors, bounded page surfaces, surface release, original-path
  replacement, and anchor cleanup. Convert completed RasterHost anchors from
  writable streams to read-only handles with same-object `ReOpenFile` transitions
  so WinRT PDF can reopen them without restoring path-resolution races.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore`
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `2e653d2`

- [x] Send local RasterHost previews as duplicated read-only handles and anchor
  the exact file object inside RasterHost before native image, animation, PDF,
  system codec, or shell-thumbnail providers receive a path. Bound inputs to
  256 MiB, validate disk type and exact length, preserve only the logical name
  for UI/routing, and clean anchors on close, replacement, cancellation, and exit.
  Cloud fail-closed compatibility requests remain explicitly path-based.
  Superseded by direct HANDLE adapters for native/system images and PDF: RasterHost no longer creates
  `raster-inputs` anchors, and unsupported HANDLE kinds fail closed. Shell fallback remains available
  only to explicit path-based cloud/legacy requests.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `80a17bc`

- [x] Add a checked-in role-scoped format registry, native ABI version export,
  startup compatibility checks in App/ParserHost/RasterHost, and a semantic
  registry guard. Align DOCM parsing, JPEG `.jpe` system policy, ParserHost kinds,
  and metadata-only fallback kinds while preserving intentional capability subsets.
  - Verification: `cargo build --release --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `dcc871d`

- [x] Add bounded current-folder archive filtering, encrypted ZIP item metadata
  and summaries, explicit encrypted-entry extraction rejection, and localized
  visible/automation status. At that checkpoint 7z/RAR were unsupported; the later bounded
  browse-only RAR scanner supersedes the RAR part of that limitation, while 7z remains unsupported.
  - Verification: `cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml`
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore --no-build`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `8604bad`

- [x] Add a schema-v2 text wrapping preference with automatic, always-wrap, and
  never-wrap modes; apply it live to text/code previews, persist toolbar changes,
  preserve structured Markdown layout, and reflow every Settings card correctly.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore --no-build`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore --no-build`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `573dc22`

- [x] Add stable, path-free image codec error codes and localized guidance for
  system-required codecs and bounded decode failures; avoid caching arbitrary
  system decoder failures as permanently unsupported capabilities.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `db1fadc`

- [x] Add exact structured Markdown search highlighting for prose, code, and
  table cells using a bounded visible-text segment index aligned with rendered
  content and truncation notices.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Verification: `cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml markdown_`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `170f995`

- [x] Add an explicit-consent Settings workflow that writes a metadata-only
  diagnostics ZIP directly to a user-selected stream without staging or upload.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `6432129`

- [x] Inventory metadata for exactly four known App/RasterHost log files without
  reading contents, enumerating directories, following reparse points, or
  exposing a production API for arbitrary roots or file names.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore --filter DiagnosticsLogInventoryTests`
  - Commit: `559a9a7`

- [x] Add a fixed-schema, metadata-only diagnostics ZIP writer with exactly two
  entries, normalized inputs, no path/attachment API, cancellation, and strict
  JSON, README, and archive size budgets.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore --filter DiagnosticsBundleTests`
  - Commit: `987a931`

- [x] Cap each delimited-table viewport reconstruction at 1024 data cells while
  preserving the normal application viewport range.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `c6792bf`

- [x] Bound syntax-highlighted Markdown to 10000 Run elements per document and
  downgrade over-budget code blocks before creating colored runs.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `583eeb6`

- [x] Skip delimited-table cell reconstruction during intermediate scroll
  events and render the new viewport once scrolling settles.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `0c33f31`

- [x] Materialize at most one missing Office page per dispatcher callback while
  releasing all off-screen pages immediately and queuing remaining nearby work.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `a709ee9`

- [x] Apply the shared 2000-block UI budget to raw Markdown fallback parsing,
  stopping line scans before creating excess paragraphs or code containers.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `2c0250b`

- [x] Isolate every ParserHost launch under an App-owned writable root for logs,
  pinned inputs, archive extraction, and raster handoffs; clean it on all exits.
  - Verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore`
  - Guard: `tools/guard-architecture.ps1`
  - Commit: `a339a29`
- [x] Render bounded SVG previews natively with external image loading disabled,
  system-font reuse, fallback classification, and RasterHost integration coverage.
  - Verification: `cargo test --locked`; `dotnet test tests/QuickLook.Next.RasterHost.IntegrationTests/QuickLook.Next.RasterHost.IntegrationTests.csproj --no-restore --filter RasterHostSvgTests`
  - Commit: `ac4966d`
- [x] Capture the restricted-host smoke child exit code from its process object.
  - Verification: `tools/smoke-restricted-host-launch.ps1`
  - Commit: `1636326`

- [x] Bound Markdown tables to 64 columns and 4096 rendered cells, and cap
  ordinary text-search highlight ranges at 5000 while retaining full results.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore --filter Markdown_table_search_index_obeys_cell_budget`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `c0f8a6a`
- [x] Bound each materialized Office page to 2048 cells and 2048 layout items,
  reuse the bounded cell set for headers/freeze panes, and release Office state
  during preview reset.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `7364008`
- [x] Bound structured Markdown rendering to 2000 block/list paragraphs and
  inline traversal to depth 16 in both UI rendering and search indexing.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`; `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore --filter Markdown_inline_search_index_obeys_depth_budget`
  - Guard: `tools/guard-performance-bounds.ps1`
  - Commit: `cfcca7a`

- [x] Localize high-frequency preview, search, media, loading, and error
  automation names; localize Retry and raise all error actions to 40 DIP.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `a7cf4d5`

- [x] Add localized row, column, cell, merged-range, page/sheet/slide position,
  and embedded-image automation names to virtualized table and Office previews.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commits: `829123d`, `adf3ae6`

- [x] Compact the text preview toolbar while search is open so the query,
  count, and navigation controls fit without horizontal overflow.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `aac32a6`

- [x] Add a persisted animated-preview preference that follows Windows, always
  plays, or forces a static first frame, with localized settings UI.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `7b223dd`
- [x] Reflow settings cards and project links below 560 DIP, reduce compact
  padding, and stretch controls without changing the wide-window layout.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `d74622d`

- [x] Parse JPEG SOF dimensions, EXIF orientation, and split ICC data in one
  bounded marker stream before decode, then reuse the result during conversion.
  - Verification: `cargo test --locked jpeg_`
  - Commit: `bddc4ad`
- [x] Build structured Markdown search indexes from displayed AST blocks,
  including lists, code, rendered table rows, links, and partial notices.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `9694bd8`
- [x] Navigate structured Markdown search matches to exact rendered block
  anchors for prose, headings, quotes, lists, code, and bounded tables.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `c6d9ace`
- [x] Extract plain and Markdown visible-text search indexing into a tested Core
  helper covering case-insensitive non-overlap and AST list/table/link content.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore --filter Text_search|Markdown_search`
  - Commit: `8e6cbaa`

- [x] Record process/App/background/hook startup milestones and correlate preview
  intent, availability, probe, route, loading shell, reveal, and final first-frame
  timings with one generation-scoped ID.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `26bf822`
- [x] Stream JPEG ICC marker segments with an 8 MiB header budget and stop at
  scan data instead of reading the full compressed image.
  - Verification: `cargo test --locked jpeg_icc`
  - Commit: `2bc28cc`
- [x] Eliminate quadratic Markdown fenced-code accumulation and merge adjacent
  syntax tokens before creating WinUI Run elements.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `09c906f`
- [x] Localize opening, cloud download, availability-check, and deferred-preview
  status text in English and Simplified Chinese.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `d16f1c2`
- [x] Version settings schema v1, validate loaded values, preserve invalid files,
  atomically replace settings, and update in-memory state only after persistence.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `571d56d`
- [x] Implement displayed-text search with Ctrl+F, Enter/F3 navigation,
  Shift+Enter/Shift+F3 reverse navigation, Escape close, match counts, and
  plain/code highlighting.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `80c8f9e`

- [x] Present APK raster icons from Android mipmap resources even when the
  launcher resource uses a custom filename; skip unreadable ZIP candidates.
  - Verification: `cargo test --locked package_icon_candidates`
  - Integration verification: `dotnet test tests/QuickLook.Next.ParserHost.IntegrationTests/QuickLook.Next.ParserHost.IntegrationTests.csproj --no-restore --filter Package_hero_raster_close_removes_bgra_handoff`
  - Commits: `88be923`, `28b7a27`
- [x] Deduplicate thumbnail work by path, size, and cache policy while preserving
  independent caller cancellation and foreground promotion.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `39bdd24`
- [x] Scan the PDF disk cache once per RasterHost process, then maintain its byte
  count incrementally and enumerate LRU files only when the limit is exceeded.
  - Verification: `dotnet build src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj --no-restore`
  - Commit: `5208700`
- [x] Track active PDF operations, drain them asynchronously, release owned
  synchronization resources, and drop the PDF projection reference on close.
  - Verification: `dotnet build src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj --no-restore`
  - Commit: `bae019e`
- [x] Expose localized PDF page position/size semantics and file-name automation
  names for image filmstrip items.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `34a46af`
- [x] Mark loading, normal status, and PDF page changes as polite live regions;
  retain assertive semantics for blocking preview errors.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `9b43e79`
- [x] Raise preview controls, listing rows, and breadcrumbs to at least 40 DIP;
  programmatically label settings controls.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `993768d`

- [x] Preserve standard Space-key behavior when focus is inside an interactive
  preview control.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `1e5e683`
- [x] Bound bencode nesting and node counts.
  - Verification: `cargo test --locked bencode_parser`
  - Commit: `a6ef746`
- [x] Enforce NuGet and Cargo vulnerability audits before stable release signing.
  - Verification: workflow review against the existing beta release audit steps
  - Commit: `7f75828`
- [x] Bound ZIP entry extraction by compressed bytes, compression ratio, output
  bytes, and elapsed time.
  - Verification: `cargo test --locked archive_extract_budget`
  - Commit: `2a44365`
- [x] Redact Windows drive and UNC directory paths from default diagnostics.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Commit: `3c4716c`
- [x] Add first-run onboarding and a persistent Help and shortcuts entry in
  Settings.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `3461b4f`
- [x] Bound thumbnail queues, remove canceled requests immediately, and reserve
  one background slot after each eight foreground requests.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `565d740`
- [x] Populate the image filmstrip with one collection reset, index items by
  path, and restrict initial thumbnails to the current item's 20-neighbor radius.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: `81c8847`
- [x] Serialize PDF disk-cache writes through a bounded process-wide queue,
  publish files atomically, and trim periodically instead of after every page.
  - Verification: `dotnet build src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj --no-restore`
  - Commit: `9b2f882`
- [x] Remove eager all-page PDF geometry enumeration from the first-preview path;
  use the existing first-page-size fallback until each page is rendered.
  - Verification: `dotnet build src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj --no-restore`
  - Commit: `ee55705`
