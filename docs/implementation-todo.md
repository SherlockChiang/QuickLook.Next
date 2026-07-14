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

- [ ] Deduplicate thumbnail work while preserving independent caller cancellation.
- [ ] Bulk-populate the image filmstrip and restrict initial work to visible
  neighbors.
- [ ] Serialize PDF disk-cache writes and maintain cache size incrementally.
- [ ] Load PDF page geometry lazily and release document resources explicitly.
- [ ] Parse JPEG dimensions, orientation, and ICC metadata in one bounded stream.
- [ ] Virtualize large code, Markdown, and table presentation work.
- [ ] Complete localization of visual, status, and automation strings.
- [ ] Add semantic PDF page, image, table, and Office accessibility metadata.
- [ ] Add consistent live-region loading, success, and failure notifications.
- [ ] Add responsive settings/toolbars and 40-44 DIP interaction targets.

## P2: Product capabilities

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

- [x] Preserve standard Space-key behavior when focus is inside an interactive
  preview control.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: this change
- [x] Bound bencode nesting and node counts.
  - Verification: `cargo test --locked bencode_parser`
  - Commit: this change
- [x] Enforce NuGet and Cargo vulnerability audits before stable release signing.
  - Verification: workflow review against the existing beta release audit steps
  - Commit: this change
- [x] Bound ZIP entry extraction by compressed bytes, compression ratio, output
  bytes, and elapsed time.
  - Verification: `cargo test --locked archive_extract_budget`
  - Commit: this change
- [x] Redact Windows drive and UNC directory paths from default diagnostics.
  - Verification: `dotnet test tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj --no-restore`
  - Commit: this change
- [x] Add first-run onboarding and a persistent Help and shortcuts entry in
  Settings.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: this change
- [x] Bound thumbnail queues, remove canceled requests immediately, and reserve
  one background slot after each eight foreground requests.
  - Verification: `dotnet build src/QuickLook.Next.App/QuickLook.Next.App.csproj --no-restore`
  - Commit: this change
