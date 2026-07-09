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

- [ ] Implement Rust-side ICC/source-to-sRGB transform for images that cannot use
  the system color-managed path.
- [ ] Re-evaluate native AVIF fallback only if Windows/MSVC dependency setup is
  reproducible without external system packages.
- [ ] Re-evaluate native HEIC/HEIF fallback if a bounded, reproducible decoder is
  available.
- [ ] Keep JXL out of scope until a system codec or reproducible native decoder is
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
- [ ] Parse HEVC VPS and HDR-adjacent metadata when present.

### PE / CLR

- [x] Detect Authenticode digest algorithms from certificate table OIDs.
- [x] Summarize bounded Authenticode certificate name strings from common X.509
  name OIDs.
- [ ] Parse Authenticode PKCS#7 signer and structured certificate subject/issuer.
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
- [ ] List bounded directory entries when the directory chunk format is safe to
  parse.
- [ ] Extract title and default topic from system/control files when present.
- [ ] Summarize compressed stream metadata without decompressing unbounded data.

### Mail / MSG

- [ ] Build a nested MIME tree instead of a flat boundary summary.
- [x] Decode transfer-encoded body sizes for base64 and quoted-printable with
  strict caps.
- [x] Add bounded body preview for text/plain parts.
- [ ] Parse Outlook MSG compound-file properties for sender, recipients, subject,
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
- CHM ITSF metadata and ITSP directory header.
- Mail top-level headers, RFC 2047/RFC 2231 filenames, MIME part summary,
  transfer encoding, body byte sizes, decoded transfer sizes, and bounded
  text/plain previews.
