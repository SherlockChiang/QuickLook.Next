# Implementation TODO

This file tracks the ordered hardening and product-improvement work identified
by the July 2026 repository review. Each completed item records its verification
and commit so changes remain independently reviewable and revertible.

## P0: Immediate safety and usability

- [ ] Move the live release signing key out of the workspace and rotate it if
  exposure cannot be ruled out. This requires owner confirmation and external
  credential storage; do not delete or move the current key automatically.

## P1: Performance and accessibility

- [ ] Replace the PDF document projection with an input/ownership path that
  exposes deterministic close semantics.
- [ ] Virtualize large code, Markdown, and table presentation work beyond the
  existing character/run bounds.
- [ ] Complete localization of visual, status, and automation strings.
- [ ] Verify live-region loading, success, and failure announcements with
  Narrator; add explicit AutomationPeer events where hidden status is silent.

## P2: Product capabilities

- [ ] Resolve Android manifest/resource-table icons and compose adaptive icons.
- [ ] Add explicit cloud hydration with consent, progress, cancellation, and a
  size policy.
- [ ] Add bounded PDF text search and copy.

## P3: Strategic architecture

- [ ] Move animated decode/playback into RasterHost shared surfaces with a
  decoded-byte budget.
- [ ] Complete handle-based RasterHost inputs.
- [ ] Add AppContainer or restricting-SID isolation, network denial, and process
  mitigation policies to hostile-format hosts.
- [ ] Split the native preview implementation by format family.
- [ ] Add App policy tests, RasterHost PDF integration tests, fuzzing, ETW/WPA
  baselines, and long-cycle resource regression tests.
- [ ] Publish LICENSE, SECURITY.md, and CONTRIBUTING.md before ecosystem work.
- [ ] Design any future extension SDK as signed, bounded, out-of-process, and
  denied network access by default.

## Completed

Completed entries move here with the verification commands and commit hash.

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
  visible/automation status. 7z/RAR remain unsupported because no audited parser,
  signature routing, extraction boundary, or tests currently exist.
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
