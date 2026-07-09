# Long-Cycle Targets

This queue tracks bounded parser and preview improvements that fit the
Rust-first architecture. Pick one item, add focused coverage, run the harness,
then commit that item by itself.

## Rules

- Prefer Rust/native metadata and preview parsing.
- Keep every read bounded and every parser tolerant of truncated input.
- Add synthetic tests for parser structures before relying on real corpora.
- Do not add WebView/WebView2 or default-path .NET preview plugins.
- Use `tools\harness-long-cycle.ps1 -AllowDirty` before each commit.
- Use `tools\harness-long-cycle.ps1 -Mode full -AllowDirty` at phase boundaries.

## Queue

### Images

- [x] Allow native JPEG fallback for embedded sRGB ICC profiles, where the
  source-to-sRGB transform is identity.
- [x] Implement arbitrary Rust-side ICC/source-to-sRGB transform for images that
  cannot use the system color-managed path.
- [x] Re-evaluate native AVIF fallback only if Windows/MSVC dependency setup is
  reproducible without external system packages.
- [x] Re-evaluate native HEIC/HEIF fallback if a bounded, reproducible decoder is
  available.
- [x] Keep JXL out of scope until a system codec or reproducible native decoder is
  available.

### MP4 / MOV

- [x] Expand `stts` into a bounded sample timeline summary.
- [x] Add bounded per-chunk / per-sample byte mapping details from `stsc`,
  `stsz`, and `stco` / `co64`.
- [x] Parse AVC SPS for coded size, crop, and VUI summary.
- [x] Extend `ctts` / `elst` into the bounded sample timeline summary.
- [x] Parse AVC SPS color details from VUI when present.
- [x] Summarize HEVC `hvcC` parameter set arrays and profile-level fields.
- [x] Parse HEVC SPS for coded size, crop, bit depth, and chroma.
- [x] Parse HEVC VPS metadata when present.
- [x] Parse HEVC/HDR-adjacent metadata when present.

### PE / CLR

- [x] Detect Authenticode digest algorithms from certificate table OIDs.
- [x] Summarize bounded Authenticode certificate name strings from common X.509
  name OIDs.
- [x] Parse structured certificate subject/issuer.
- [x] Parse Authenticode PKCS#7 signer.
- [x] Decode CLR metadata table stream row counts.
- [x] Summarize CLR assembly references.
- [x] Summarize CLR type definitions and custom attributes with strict caps.

### ELF

- [x] Decode relocation type names for common machines such as x86-64 and
  AArch64.
- [x] Include symbol binding/type/section metadata in symbol summaries.
- [x] Parse GNU version sections such as `.gnu.version`, `.gnu.version_r`, and
  `.gnu.version_d`.
- [x] Parse PT_NOTE program headers in addition to note sections.

### Minidump

- [x] Parse HandleData stream counts and first handles.
- [x] Parse UnloadedModuleList names and address ranges.
- [x] Parse MiscInfo process id, process times, and processor power info when
  present.
- [x] Decode module fixed version fields from `MINIDUMP_MODULE.VersionInfo`.

### CHM

- [x] Parse ITSP directory header from the CHM directory offset.
- [x] List bounded directory entries when the directory chunk format is safe to
  parse.
- [x] Extract title and default topic from system/control files when present.
- [x] Summarize compressed stream metadata without decompressing unbounded data.

### Mail / MSG

- [x] Build a nested MIME tree instead of a flat boundary summary.
- [x] Decode transfer-encoded body sizes for base64 and quoted-printable with
  strict caps.
- [x] Add bounded body preview for text/plain parts.
- [x] Parse Outlook MSG compound-file properties for sender, recipients, subject,
  sent time, attachments, and body availability.

## Done Recently

- PE version strings, fixed version info, imports/exports, certificate header,
  and CLR metadata root summary.
- ELF section names, dynamic string tags, symbols with binding/type/section,
  relocations, GNU version sections, note sections/PT_NOTE program headers, and
  GNU build-id, plus x86-64/AArch64 relocation type names.
- Minidump SystemInfo, Exception, ThreadList, ThreadNames, ModuleList with fixed
  version fields, MemoryList, Memory64List, HandleData, UnloadedModuleList, and
  MiscInfo.
- CHM ITSF metadata, ITSP directory header, bounded PMGL directory entries,
  compressed stream metadata, and #SYSTEM title/default topic.
- Mail top-level headers, RFC 2047/RFC 2231 filenames, MIME part summary,
  transfer encoding, body byte sizes, decoded transfer sizes, and bounded
  text/plain previews, nested MIME parts, and bounded MSG compound-file
  properties.
- Native AVIF/HEIC/JXL fallbacks re-evaluated and kept on system/WIC policy until
  reproducible Windows-native decoder paths exist.
- Native JPEG fallback applies embedded ICC profiles through `qcms` before BGRA
  conversion; invalid or unsupported profiles fail closed instead of displaying
  unconverted colors.
