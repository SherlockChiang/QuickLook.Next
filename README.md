# QuickLook Next

[Project website](https://sherlockchiang.github.io/QuickLook.Next/) · [简体中文](README_CN.md)

**Select a file in Windows Explorer. Press Space. See it instantly.**

[![Latest release](https://img.shields.io/github/v/release/SherlockChiang/QuickLook.Next?display_name=tag&sort=semver)](https://github.com/SherlockChiang/QuickLook.Next/releases/latest)
![Windows 10 and 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?logo=windows)
![Architecture](https://img.shields.io/badge/architecture-x64-555555)

![QuickLook Next previewing its own app artwork](docs/images/quicklook-next-hero.png)

QuickLook Next is a fast, native file previewer for Windows Explorer. Its WinUI 3 interface, Rust-powered parsing, GPU-composited image surfaces, and isolated helper processes keep previews responsive without sacrificing safety.

[Explore the immersive project website →](https://sherlockchiang.github.io/QuickLook.Next/)

## Get Started

1. Download `QuickLook.Next-Installer-*-win-x64.zip` from the [latest release](https://github.com/SherlockChiang/QuickLook.Next/releases/latest).
2. Extract the ZIP and run `Install.cmd` (`Install-ZH-CN.cmd` provides Chinese instructions).
3. Approve the administrator prompt, finish installation, and launch **QuickLook Next** from Start.
4. Select a file in Windows Explorer and press **Space**.

The installer contains a signed MSIX and its project development certificate. Windows needs administrator approval to trust this certificate for sideloading. This is development signing rather than commercial Authenticode trust, so Windows may still warn about the installer script.

## Why QuickLook Next?

- **One-key previews:** open or close the preview with `Space`, then follow Explorer selections with the arrow keys.
- **Rich image viewing:** zoom and pan, inspect EXIF and color details, view waveforms, and browse neighboring images in a filmstrip.
- **Useful document views:** read PDF, Markdown, source code, virtualized CSV tables, and approximate Office layouts.
- **Structured data:** browse bounded SQLite table data in switchable sheets with sticky headers and honest partial-data labels.
- **Broad format coverage:** preview archives, media, fonts, certificates, executables, ebooks, mail, folders, and more.
- **Safer parsing:** complex parsers and raster decoders run in restricted, cancellable helper processes.
- **Clear cloud behavior:** online-only files show a download state before previewing; unsupported content is not disguised as a large file icon.
- **Windows-aware UI:** high contrast, reduced motion, keyboard navigation, and multi-monitor DPI behavior are respected.

## Shortcuts

| Action | Shortcut |
| --- | --- |
| Open or close preview | `Space` |
| Close preview | `Esc` |
| Reload current preview | `F5` |
| Enter or leave fullscreen | `F11` |
| Follow the previous or next Explorer item | Arrow keys |
| Zoom image | Mouse wheel or `+` / `-` |
| Reset image view | `Home` or `Ctrl+0` |
| Browse neighboring images | `Left` / `Right` while the image preview is focused |

Space keeps its normal behavior when focus is inside a text field, button, list item, toggle, or slider. Closing a preview hides the window; QuickLook Next remains available from the system tray.

## Supported Content

| Category | Preview experience |
| --- | --- |
| Images | JPEG, PNG, APNG, GIF, WebP, BMP, and TIFF; system-codec fallback for formats such as HEIC and AVIF |
| PDF and Office | Virtualized PDF pages and approximate DOCX, XLSX, and PPTX previews |
| Text and data | Plain text, source code, configuration files, Markdown, CSV, TSV, and SQLite table sheets |
| Archives and packages | Bounded listings, metadata summaries, package icons, and safe nested-entry previews for supported containers |
| Audio and video | Formats supported by Windows Media Foundation, with lightweight container and codec metadata |
| Specialist formats | Fonts, certificates, PE/EXE/DLL, ELF, minidumps, Torrent, mail, ebooks, CHM, and disk-image metadata |
| Folders | Bounded directory listings with safe, prioritized thumbnail loading |

Some formats depend on optional Windows codecs. Office previews do not run Microsoft Office, macros, formula recalculation, embedded scripts, or a browser engine, so exact layout fidelity varies with document complexity.

## Cloud Files

QuickLook Next distinguishes already-downloaded cloud files from online-only placeholders:

- Hydrated OneDrive and other cloud files receive the same full preview as local files.
- Online-only files first show a visible download state.
- After hydration, QuickLook Next probes the real content and opens the correct preview.
- Downloads are cancellable and time out instead of leaving a hidden background read running.
- If availability cannot be verified safely, non-image formats stay in a metadata view instead of falling back to a Shell icon.

## Verify a Download

Each release includes a SHA-256 file next to the Installer ZIP:

```powershell
$zip = Get-Item .\QuickLook.Next-Installer-*-win-x64.zip
Get-FileHash $zip.FullName -Algorithm SHA256
Get-Content "$($zip.FullName).sha256"
```

Continue only when the hashes match and both files came from this repository's Releases page.

## Requirements

- Windows 10 version 1809 or later, or Windows 11.
- x64 processor.
- Windows File Explorer for global Space-key integration.
- A GPU and driver supported by the Windows composition stack.

The package includes the required Windows App SDK runtime components. Optional image and media formats may still require codecs from Windows or the Microsoft Store.

## Troubleshooting

### Space does nothing

- Confirm QuickLook Next is running and its tray icon is present.
- Bring Windows Explorer to the foreground and select a file.
- Finish renaming files or typing in an Explorer text field; QuickLook Next intentionally leaves Space alone there.
- Exit older QuickLook or QuickLook Next instances that may also be listening for Space.

### A preview shows metadata or a partial result

- The file may still be downloading from cloud storage.
- Windows may not have the optional codec required by the format.
- A parser may have reached a documented size, row, page, or time limit.
- SQLite previews sample bounded rows and mark incomplete sheets as partial.

### Report a problem

[Open an issue](https://github.com/SherlockChiang/QuickLook.Next/issues) with your Windows and QuickLook Next versions, file type and approximate size, expected and actual behavior, reproduction steps, and relevant logs. Do not upload private sample files. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the current contribution status and maintainer engineering process.

<details>
<summary><strong>Build from source</strong></summary>

You need Windows x64 with the Desktop C++/MSVC toolchain, the .NET SDK selected by [`global.json`](global.json), and the Rust MSVC toolchain selected by [`rust-toolchain.toml`](native/quicklook_next_native/rust-toolchain.toml).

```powershell
dotnet restore QuickLook.Next.slnx --locked-mode
cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml
cargo build --release --locked --manifest-path native/quicklook_next_native/Cargo.toml
dotnet build QuickLook.Next.slnx -c Release --no-restore
dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore
```

`tools/release.ps1` is the authoritative local restore, test, build, signing, and packaging entry point. Release artifacts are written to `artifacts/`.

</details>

<details>
<summary><strong>Architecture and releases</strong></summary>

- `QuickLook.Next.App`: WinUI 3 shell, presenters, input, and process supervision.
- `quicklook_next_native`: Rust probing, Explorer integration, parsers, thumbnails, and image decoding.
- `QuickLook.Next.ParserHost`: isolated structured parsing for archives, Office files, ebooks, executables, and related formats.
- `QuickLook.Next.RasterHost`: isolated image, PDF, and system-codec rendering through shared GPU surfaces.
- App/host IPC uses authenticated current-user-only named pipes with cancellation and stale-result guards.

Pull requests run CI; contribution and verification requirements are defined in [`CONTRIBUTING.md`](CONTRIBUTING.md). A tested commit whose subject starts with `release:` runs the stable packaging workflow. Published assets include signed packages, checksums, release metadata, update metadata, build manifests, and an SBOM.

See [`docs/review-readiness.md`](docs/review-readiness.md) for engineering boundaries and verification details.

</details>

## Security

Do not file a public issue for an undisclosed vulnerability. Follow [`SECURITY.md`](SECURITY.md) for private reporting and sensitive-sample handling.

## License

QuickLook Next's original source code and assets are available under the [MIT License](LICENSE).
Bundled third-party components remain subject to their respective licenses and notices.
