# QuickLook Next

Rebuild of QuickLook on a process-isolated, GPU-composited, plugin-extensible architecture.
Every boundary here was validated by a runnable spike first (see `../spikes/`).

## Layout
```
QuickLook.Next/
  Directory.Build.props                 shared .NET settings (net10, x64, nullable)
  QuickLook.Next.slnx
  native/
    quicklook_next_native/              Rust cdylib — Win32/Shell/COM + hotkey + FFI  (Spike 3)
  src/
    QuickLook.Next.Contracts/           control DTOs + legacy plugin-facing contracts
    QuickLook.Next.Core/                stable control protocol + FFI intents + pipe channel + watchdog
    QuickLook.Next.RasterHost/          .NET RasterHost process: D3D surfaces + PDF/thumbnail bridges  (Spike 1)
    QuickLook.Next.App/                 WinUI 3 shell: native bridge + supervision + composition consumer  (Spikes 1+3)
  plugins/                              legacy/reference .NET plugin sources (reference-only)
```

## Dependency direction
`Contracts` ← `Core` ← (`App`, `RasterHost`). Rust ↔ App/RasterHost is C ABI; App ⇄ RasterHost is IPC
(control pipe + shared composition surface).

## Rust-first ownership
Rust is the source of truth for native input, Explorer selection, file probing, shell thumbnails, image
decoding, and lightweight content previews. The App calls Rust directly for text, archive, and folder
previews; these do not require RasterHost or .NET plugins in the hot path. Raster image previews decode
to BGRA in Rust, then RasterHost uploads those pixels to a shared D3D surface.

.NET remains intentional only where it is currently a frontend or surface-hosting component:
- WinUI 3 window, tray menu, title bar, presenter-driven preview UI, media element, and input gestures.
- Composition interop and shared-surface consumption in the App.
- RasterHost D3D composition surface production, PDF page rendering through `Windows.Data.Pdf`, image
  raster upload, and shell-thumbnail fallback.

The old .NET text/archive/info/image plugins are compatibility scaffolding, not the preferred
architecture. They are kept under `plugins/` as reference source only: they are not included in the
default solution, not copied by the release packager, and not used as a default plugin discovery path.
New preview business logic should land in `native/quicklook_next_native/src/preview.rs` or a narrow
Rust C ABI returning structured JSON/BGRA for the WinUI shell and RasterHost to render. Do not
reintroduce WebView/HTML output for Office previews.

## The two boundaries
- **App ⇄ native (Rust):** in-process FFI. `quicklook_next_native` installs the WH_KEYBOARD_LL hook and
  reads the Explorer selection, then calls back with high-level intent lines decoded into `NativeIntent`.
- **App ⇄ RasterHost:** named-pipe **control channel** (line-delimited JSON, `ControlMessage` in
  `Core/Protocol.cs`) + a **shared composition surface** for pixels (never base64 over the pipe).
  Invariant: every `RequestId` ends in exactly one of `preview.ready | preview.error | timeout`
  (`PendingRequests` + per-request watchdog).

Handshake: App (pipe server) launches Host → `hello{appPid}` → Host `host.ready{adapterLuid}` →
`preview.open{requestId,path,probe}` → `preview.surface{handle,…}` + `preview.ready{…}`. Resize →
`preview.resize` → new `preview.surface`. Host crash → App restarts it (supervisor).

## Build / checks
```
# native (needs MSVC C++ Build Tools — see spikes/spike3-native/SPIKE3_FINDINGS.md)
cargo build --release --manifest-path native/quicklook_next_native/Cargo.toml
# .NET solution
dotnet build QuickLook.Next.slnx -c Debug
# native smoke
powershell -ExecutionPolicy Bypass -File tools/smoke-native.ps1
# architecture guard
powershell -ExecutionPolicy Bypass -File tools/guard-architecture.ps1
```

## Current status
Wired & compiling now: the four projects, the full control protocol, the FFI bridge, tray-background
startup, no-activate preview behavior, Explorer selection switching, RasterHost supervision + restart,
the composition producer/consumer, App-direct media playback, and the raster paths:
Rust image decode or Windows PDF render → BGRA pixels → RasterHost D3D texture → shared composition
surface.

RasterHost is lazy-started: text, Office metadata/layout, archive/folder listings, package metadata,
certificates, executables, and other lightweight Rust previews do not start the surface host. It is
started only when a preview needs a D3D surface, PDF page rasterization, or shell thumbnail fallback.

The WinUI shell is split into focused presenters/controllers for text, listing, Office layout,
raster/image surfaces, PDF page virtualization/cache, media playback, and topmost/no-activate window
behavior. MainWindow remains the application coordinator: native intents, request cancellation,
RasterHost pipe lifetime, panel switching, and window placement.

The default release path is Rust/App/RasterHost only. `tools/guard-architecture.ps1` enforces the
current boundaries: no WebView/WebView2, no default .NET preview plugin path or project reference, no
RasterHost plugin registry/loader, and no legacy .NET preview plugins in release output.

## Remaining work
1. Split preview panel visibility/reset choreography out of `MainWindow`.
2. Move static XAML labels from the interim `UiStrings`/inline text layer into `.resw` resources.
3. Keep improving Rust-native document fidelity, especially Office layout reconstruction.
4. Add broader smoke assets for real-world Office, PDF, package, certificate, image, archive, folder, and text previews.

See `../spikes/spike{1,2,3}-*/SPIKE*_FINDINGS.md` for the validated recipes behind each piece.
