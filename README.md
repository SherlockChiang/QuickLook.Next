# QuickLook Next

[简体中文](README_CN.md)

Preview files in Windows Explorer with a single press of the Space bar.

QuickLook Next is a fast, native Windows file previewer built with WinUI 3, Rust, and GPU-composited surfaces. It keeps complex parsers and raster decoders in restricted helper processes so a damaged or unusually large file cannot easily take down the main app.

> QuickLook Next is under active development. The current release is a portable, unsigned Windows build intended for early users and testers.

## Highlights

- Press **Space** in File Explorer to open or close a preview.
- Use **arrow keys** to move between Explorer selections while the preview is open.
- Preview images, animated GIF/WebP, PDF, text and source files, Markdown, CSV, folders, archives, Office documents, media, fonts, packages, certificates, executables, SQLite databases, ebooks, mail, and other common formats.
- View image metadata and EXIF details, zoom images, browse neighboring images, and open the original file or its location.
- Play supported local audio and video without launching another app.
- Keep archive, Office, ebook, executable, and raster work outside the UI process.
- Avoid silently downloading online-only cloud files. Cloud placeholders use metadata-only previews unless content access is explicitly safe.
- Follow Windows accessibility settings, including high contrast and reduced motion.

## Download

Download the latest `QuickLook.Next-Installer-*-win-x64.zip` from [GitHub Releases](https://github.com/SherlockChiang/QuickLook.Next/releases).

1. Extract the Installer ZIP.
2. Double-click `Install.cmd` and follow the prompts. The package includes a signed MSIX and its project development certificate.
3. Start QuickLook Next from the Start menu, select a file in File Explorer, and press **Space**.

The certificate is installed only for the current user. QuickLook Next can be removed later from **Windows Settings > Apps**. Microsoft Store distribution and automatic updates are planned for a later test release.

### Windows Warning

Current builds are not Authenticode-signed. Windows SmartScreen may show an "unrecognized app" warning. Verify the downloaded ZIP against the accompanying `.sha256` file before running it:

```powershell
Get-FileHash .\QuickLook.Next-Installer-0.2.1-win-x64.zip -Algorithm SHA256
```

Compare the displayed hash with the first value in `QuickLook.Next-Installer-0.2.1-win-x64.zip.sha256` from the same release. Only continue if they match and the file came from this repository's Releases page.

## Using QuickLook Next

| Action | Shortcut |
| --- | --- |
| Open or close preview | `Space` |
| Close preview | `Esc` |
| Preview previous or next Explorer item | Arrow keys |
| Zoom image | Mouse wheel or `+` / `-` |
| Reset image view | `Home` or `Ctrl+0` |
| Navigate neighboring images | `Left` / `Right` while the image window is focused |

Clicking the preview does not prevent Space from closing it. When focus is inside a text field, button, list item, toggle, or slider, Space keeps the standard Windows control behavior.

The tray menu provides startup and exit controls. Closing a preview hides the window but leaves QuickLook Next available in the tray.

## Supported Content

Support depends partly on codecs installed in Windows, but the built-in paths cover the most common cases:

- **Images:** JPEG, PNG, GIF, WebP, BMP, TIFF; system-codec fallback for formats such as HEIC and AVIF.
- **Documents:** PDF, DOCX, XLSX, PPTX, EPUB, FB2, Markdown, plain text, source code, configuration files, and CSV.
- **Archives and packages:** ZIP and other supported archives, application/package metadata, archive browsing, and nested entry previews.
- **Media:** common local audio/video formats supported by Windows Media Foundation, with lightweight container metadata.
- **Developer and specialist files:** PE/EXE/DLL, ELF, minidump, certificates, fonts, SQLite, Torrent, mail, and CHM metadata.
- **Folders:** bounded directory listings with safe thumbnail scheduling.

Office previews are intentionally approximate and do not run Microsoft Office, macros, formula recalculation, embedded scripts, or a browser engine. Exact fidelity varies with document complexity.

## Cloud Files

QuickLook Next treats online-only OneDrive and other cloud placeholders conservatively:

- Opening a preview does not automatically hydrate an online-only file.
- Metadata-only information is shown when content availability is uncertain.
- Decorative thumbnails, sidecars, and media playback do not trigger hidden secondary reads.
- Local files and already-hydrated cloud files receive the full preview experience.

An explicit download experience and progress UI are planned for a later release.

## Requirements

- Windows 10 version 1809 or later, or Windows 11.
- x64 processor.
- File Explorer. Other file managers are not currently integrated with the global Space shortcut.
- A GPU and driver supported by the Windows composition stack.

The portable release includes the Windows App SDK runtime components required by the app. Some image and media formats still require optional codecs from Windows or the Microsoft Store.

## Troubleshooting

### Space does nothing

- Confirm `QuickLook.Next.App.exe` is running and its tray icon is present.
- Make sure File Explorer is the foreground window and a file is selected.
- Space is intentionally not intercepted while renaming a file or typing in an Explorer text field.
- Exit any older QuickLook Next instance from the tray before starting a newly extracted build.

### A format shows metadata instead of full content

- The file may be an online-only cloud placeholder.
- Windows may not have the required system codec.
- The parser may have reached a safety limit or timeout. Reopen the file to retry.

### Report a problem

Open a [GitHub issue](https://github.com/SherlockChiang/QuickLook.Next/issues) with:

- Windows version and QuickLook Next version.
- File type and approximate size. Do not upload private files.
- What you expected and what happened.
- Reproduction steps and relevant logs, if available.

## Building From Source

Prerequisites:

- Windows x64 with Visual Studio Build Tools and the Desktop C++/MSVC toolchain.
- .NET SDK version specified by [`global.json`](global.json).
- Stable Rust MSVC toolchain.

```powershell
dotnet restore QuickLook.Next.slnx --locked-mode
cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml
cargo build --release --locked --manifest-path native/quicklook_next_native/Cargo.toml
dotnet build QuickLook.Next.slnx -c Release --no-restore
dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore
.\tools\pack-msix.ps1 -CreateDevelopmentCertificate
```

The Installer ZIP, signed MSIX, certificate, and checksum are written to `artifacts/`. Architecture and image-corpus guards run as part of packaging. Development certificates are for testing only; Store releases will use the Partner Center identity.

## Architecture

- `QuickLook.Next.App`: WinUI 3 shell, preview presenters, input, and process supervision.
- `quicklook_next_native`: Rust file probing, Explorer integration, native parsers, thumbnails, and image decoding.
- `QuickLook.Next.ParserHost`: isolated structured parsing for archives, Office files, ebooks, executables, and related formats.
- `QuickLook.Next.RasterHost`: isolated image/PDF/system-codec rendering and shared GPU surfaces.
- App/host IPC uses authenticated, current-user-only named pipes with request generation and cancellation guards.

See [`docs/review-readiness.md`](docs/review-readiness.md) for engineering boundaries, verification details, and known remaining work.

## Security

Please avoid filing public issues for undisclosed security vulnerabilities. Until a private security policy is published, contact the repository owner through their GitHub profile with a minimal description and no sensitive sample files.

## License

A project license has not yet been published. Source availability does not grant redistribution or modification rights beyond those provided by applicable law. A formal license is planned before a stable release.
