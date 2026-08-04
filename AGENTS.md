# QuickLook Next Repository Instructions

These instructions apply to the entire repository. This tracked file is the
authoritative agent guide; the ignored lowercase `agent.md` is local scratch
material and must not define repository policy.

`CONTRIBUTING.md` owns contribution and dependency process, ADRs own durable
decisions, `docs/handle-based-preview-inputs.md` owns detailed HANDLE/ABI state,
`docs/review-readiness.md` records current implementation status, and PRDs/TODOs
set priorities. Do not turn a historical roadmap into a competing contract.

## Architecture placement

- Keep QuickLook Next Rust-first. Put probing, bounded parsing, metadata,
  archive/package inspection, structured preview models, safe format detection,
  and reusable decode logic in `native/quicklook_next_native` unless a Windows
  API genuinely requires managed code.
- Keep `QuickLook.Next.App` as a thin WinUI shell: presentation, accessibility,
  preview lifecycle, window/input/tray integration, process supervision, and
  dispatch of already-structured preview data.
- Keep `ParserHost` a restricted transport, HANDLE-adoption, and Rust FFI
  boundary. New untrusted structured parsers belong in Rust, not in the host.
- Keep `RasterHost` limited to surface production and Windows raster APIs:
  image/PDF upload, shared surfaces, Windows codecs/PDF, and related bounded
  sidecars. Do not move ordinary structured parsing there.
- Keep `ShellBroker` limited to explicit Explorer/Shell compatibility work. It
  is not a general preview host.
- Do not add WebView/WebView2 preview rendering or new default-path .NET preview
  plugins. `Plugin.*` projects are reference material and stay outside default
  discovery and publish paths.
- When Rust and C# are both viable, choose Rust unless the work requires WinUI,
  XAML, AppWindow, Windows shell UI, or a Windows-only surface API.
- Keep moving presentation islands out of `MainWindow.xaml.cs` into focused
  presenters/controllers. Prefer stable XAML-declared interactive controls over
  creating equivalent controls during row/content rendering.

## Trust and resource boundaries

- Treat files, logical paths, IPC messages, dimensions, counts, offsets, and
  host-returned metadata as untrusted. Preserve explicit byte, item, depth,
  decompression, allocation, deadline, and retained-memory limits.
- Local preview authority comes from the exact file HANDLE opened by the App.
  Logical or compatibility paths are metadata and must never silently replace a
  pinned object. Preserve immediate receiver adoption and single-owner cleanup.
- Keep App/host pipes authenticated to the current user and launch session. A
  host must never receive authority to open the App process or duplicate handles
  from it.
- Preserve supervised process boundaries, job limits, fail-closed startup, and
  bounded recycling. Do not add interactive crash/error dialogs to background
  hosts.
- After a host has adopted untrusted input, crash, timeout, malformed output, or
  protocol failure must recycle/fail closed; never retry in a broader-privilege
  host or silently reopen the logical path.
- Preserve generation, request-ID, and cancellation checks across every async,
  FFI, and UI handoff. Late work may be discarded only after its resources and
  ownership transfers are settled.
- Branch on stable error codes or typed failures, never host exception text.
  Bind user-visible errors and actions to the current request generation and
  path; localization belongs in the App.

The accepted decisions are indexed in [`docs/adr/README.md`](docs/adr/README.md).
The detailed current HANDLE ABI and transfer checklist remain in
[`docs/handle-based-preview-inputs.md`](docs/handle-based-preview-inputs.md).

## Change discipline

- Keep changes focused, independently reviewable, and independently revertible.
  Preserve unrelated user changes in a dirty worktree.
- Add focused regression coverage for malformed input, cancellation, stale
  completion, timeout, crash, and ownership cleanup when the affected boundary
  can exhibit them.
- Keep en-US and zh-CN resources and format placeholders in parity.
- Explain dependency additions and their security, maintenance, binary-size,
  and runtime costs. Update lock files only when the graph changes.
- Update the format registry, executable guards, ADRs, and reviewer-facing docs
  when their invariant or capability changes. Do not weaken a guard merely to
  make a change pass.
- Do not use `release:` for ordinary commits or pull requests; it is reserved
  for the stable release workflow.

## Verification

Build the pinned win-x64 Rust DLL through its MSBuild contract; do not introduce
another raw `cargo build` path:

```powershell
dotnet msbuild native\QuickLook.Next.Native.proj -target:Build -verbosity:minimal
```

Run focused checks while developing, then the relevant full gates before review:

```powershell
cargo fmt --all --manifest-path native\Cargo.toml -- --check
cargo clippy --workspace --all-targets --all-features --locked --manifest-path native\Cargo.toml -- -D warnings
cargo test --workspace --locked --manifest-path native\Cargo.toml
dotnet build QuickLook.Next.slnx -c Release --no-restore
dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore --maxcpucount:1
pwsh -NoProfile -File tools\guard-architecture.ps1 -SkipDist
git diff --check
```

Use `tools/harness-long-cycle.ps1` for repeatable quick/full loops and
`tools/release.ps1` for the authoritative release-oriented sequence.
