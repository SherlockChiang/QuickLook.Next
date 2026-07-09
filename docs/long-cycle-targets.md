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

- [ ] Expand `stts` / `ctts` / `elst` into a bounded sample timeline summary.
- [ ] Add bounded per-chunk / per-sample byte mapping details from `stsc`,
  `stsz`, and `stco` / `co64`.
- [ ] Parse AVC SPS for coded size, crop, color, and VUI summary.
- [ ] Parse HEVC SPS/VPS for coded size, profile, level, bit depth, chroma, and
  HDR-adjacent metadata when present.

### PE / CLR

- [ ] Parse Authenticode PKCS#7 signer, certificate subject/issuer, and digest
  algorithms from the certificate table.
- [ ] Decode CLR metadata table stream row counts.
- [ ] Summarize CLR assembly references.
- [ ] Summarize CLR type definitions and custom attributes with strict caps.

### ELF

- [x] Decode relocation type names for common machines such as x86-64 and
  AArch64.
- [ ] Include symbol binding/type/section metadata in symbol summaries.
- [ ] Parse GNU version sections such as `.gnu.version`, `.gnu.version_r`, and
  `.gnu.version_d`.
- [ ] Parse PT_NOTE program headers in addition to note sections.

### Minidump

- [ ] Parse HandleData stream counts and first handles.
- [ ] Parse UnloadedModuleList names and address ranges.
- [ ] Parse MiscInfo process id, process times, and processor power info when
  present.
- [ ] Decode module fixed version fields from `MINIDUMP_MODULE.VersionInfo`.

### CHM

- [ ] Parse ITSP directory header from the CHM directory offset.
- [ ] List bounded directory entries when the directory chunk format is safe to
  parse.
- [ ] Extract title and default topic from system/control files when present.
- [ ] Summarize compressed stream metadata without decompressing unbounded data.

### Mail / MSG

- [ ] Build a nested MIME tree instead of a flat boundary summary.
- [ ] Decode transfer-encoded body sizes for base64 and quoted-printable with
  strict caps.
- [ ] Add bounded body preview for text/plain parts.
- [ ] Parse Outlook MSG compound-file properties for sender, recipients, subject,
  sent time, attachments, and body availability.

## Done Recently

- PE version strings, fixed version info, imports/exports, certificate header,
  and CLR metadata root summary.
- ELF section names, dynamic string tags, symbols, relocations, notes, and GNU
  build-id, plus x86-64/AArch64 relocation type names.
- Minidump SystemInfo, Exception, ThreadList, ThreadNames, ModuleList,
  MemoryList, and Memory64List.
- Mail top-level headers, RFC 2047/RFC 2231 filenames, MIME part summary,
  transfer encoding, and body byte sizes.
