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
    QuickLook.Next.Contracts/           plugin-facing contract: IPreviewProvider, FileProbe…
    QuickLook.Next.Core/                stable control protocol + FFI intents + pipe channel + watchdog
    QuickLook.Next.RasterHost/          .NET RasterHost process: D3D surfaces + PDF/thumbnail bridges  (Spike 1)
    QuickLook.Next.App/                 WinUI 3 shell: native bridge + supervision + composition consumer  (Spikes 1+3)
  plugins/                              viewer modules (each: dll + .deps.json + *.plugin.json)
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
- WinUI 3 window, tray menu, title bar, text/list/folder UI controls, media element, and input gestures.
- Composition interop and shared-surface consumption in the App.
- RasterHost D3D composition surface production, PDF page rendering through `Windows.Data.Pdf`, image
  raster upload, and shell-thumbnail fallback.

The old .NET text/archive/info/image plugins are compatibility scaffolding, not the preferred
architecture. New preview business logic should land in `native/quicklook_next_native/src/preview.rs`
or a narrow Rust C ABI returning structured JSON/BGRA for the WinUI shell and RasterHost to render. Do
not reintroduce WebView/HTML output for Office previews.

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

## Build
```
# native (needs MSVC C++ Build Tools — see spikes/spike3-native/SPIKE3_FINDINGS.md)
cargo build --release --manifest-path native/quicklook_next_native/Cargo.toml
# .NET solution
dotnet build QuickLook.Next.slnx -c Debug
```

## Wired vs. next phase
Wired & compiling now: the four projects, the full control protocol, the FFI bridge, RasterHost
supervision + restart, the composition producer/consumer, App-direct media playback, and the raster paths:
Rust image decode or Windows PDF render → BGRA pixels → RasterHost D3D texture → shared composition
surface.

Next phase (clearly marked `TODO` in code):
1. Deploy `quicklook_next_native.dll` + `plugins/` next to the App; wire `adapterLuid` agreement.
2. Hotkey state-machine refinements in Rust (750 ms hold, ~1 s invalid-key suppression, WinEvent reset).
3. `WS_EX_NOACTIVATE` on the preview window so Explorer keeps focus for arrow-key switching.

See `../spikes/spike{1,2,3}-*/SPIKE*_FINDINGS.md` for the validated recipes behind each piece.
