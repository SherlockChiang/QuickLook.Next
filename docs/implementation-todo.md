# Implementation TODO

This file tracks the ordered hardening and product-improvement work identified
by the July 2026 repository review. Each completed item records its verification
and commit so changes remain independently reviewable and revertible.

## P0: Immediate safety and usability

- [ ] Move the live release signing key out of the workspace and rotate it if
  exposure cannot be ruled out. This requires owner confirmation and external
  credential storage; do not delete or move the current key automatically.
- [ ] Add correlated preview phase timings for startup and first usable content.

## P1: Performance and accessibility

- [ ] Replace the PDF document projection with an input/ownership path that
  exposes deterministic close semantics.
- [ ] Parse JPEG dimensions, orientation, and ICC metadata in one bounded stream.
- [ ] Virtualize large code, Markdown, and table presentation work.
- [ ] Complete localization of visual, status, and automation strings.
- [ ] Add semantic table and Office accessibility metadata.
- [ ] Verify live-region loading, success, and failure announcements with
  Narrator; add explicit AutomationPeer events where hidden status is silent.
- [ ] Add responsive settings and toolbar overflow states for narrow widths.

## P2: Product capabilities

- [ ] Resolve Android manifest/resource-table icons and compose adaptive icons.
- [ ] Implement text/code search with Ctrl+F, F3, Shift+F3, and match counts.
- [ ] Add a privacy-conscious diagnostics center and support bundle.
- [ ] Add stable codec error codes and actionable capability guidance.
- [ ] Version the settings schema and add high-value behavior preferences.
- [ ] Add explicit cloud hydration with consent, progress, cancellation, and a
  size policy.
- [ ] Add bounded PDF text search and copy.
- [ ] Add archive filtering and encrypted-entry summaries; evaluate 7z/RAR.

## P3: Strategic architecture

- [ ] Move animated decode/playback into RasterHost shared surfaces with a
  decoded-byte budget.
- [ ] Complete handle-based RasterHost inputs.
- [ ] Add AppContainer or restricting-SID isolation, network denial, and process
  mitigation policies to hostile-format hosts.
- [ ] Split the native preview implementation by format family.
- [ ] Generate or validate the cross-language format registry and ABI version.
- [ ] Add App policy tests, RasterHost PDF integration tests, fuzzing, ETW/WPA
  baselines, and long-cycle resource regression tests.
- [ ] Publish LICENSE, SECURITY.md, and CONTRIBUTING.md before ecosystem work.
- [ ] Design any future extension SDK as signed, bounded, out-of-process, and
  denied network access by default.

## Completed

Completed entries move here with the verification commands and commit hash.

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
