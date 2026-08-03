# Handle-based preview inputs

## Goal

Remove path re-resolution from untrusted preview processing. The App opens the selected file once,
pins that file object with a read-only handle, and duplicates only that handle into the selected host.
Renaming, deleting, replacing, or redirecting the original path must not change the bytes parsed by a
request that is already in progress.

## Current boundary

- Animation frame packets and ParserHost hero rasters are returned in host-owned, unnamed
  page-file-backed sections. The App duplicates each remote section with `SECTION_MAP_READ`, maps
  only the claimed bounded packet length, validates the packet header, dimensions, and exact layout,
  then acknowledges the transfer. These handoffs publish no path and perform no temporary-file
  write; an already-mapped App view remains valid after the Host releases its owner.
- Local ParserHost and RasterHost previews normally enter through `PreviewOpenHandle`. Database
  previews use the dedicated `PreviewOpenSqliteHandles` message so an SQLite main file and its
  optional WAL/SHM companions have explicit, independently owned slots.
- ParserHost text previews (plain text, Markdown, CSV, and TSV), executable metadata, torrent
  listings, archive listings, Office previews, ebook previews, and package previews pass the received file handle directly to the
  Rust ABI. They do not create a `parser-input` anchor or reopen the logical path.
- ParserHost establishes and authenticates its pipe before loading the native ABI. The App grants
  cold start/ready negotiation a 15-second budget and considers a generation reusable only after
  `ParserReady`, so idle prewarm cannot race a real preview into the five-second per-request budget.
  The restricted-host smoke writes valid JSON to a physical `.bin`, supplies a nonexistent logical
  `.json`, and requires exact text plus `json` language output.
- A normal ebook preview is stateless and releases its adopted input after the bounded call. A
  successfully published local archive listing instead retains its source handle under the parent
  preview request ID, because an item click may need to extract an entry from that exact file
  object. This also applies when an EPUB with no usable OPF is presented as an archive listing from
   its already-open ZIP reader. Only ZIP-compatible listings advertise entry preview; TAR/TGZ/GZip
   listings remain browse-only. The retained owner is released on `PreviewClose`, failure,
   replacement, or host disconnect.
- ParserHost database previews also pass their received handles directly to Rust without creating a
  `parser-input` anchor. The App is the only component allowed to derive and open `main-wal` and
  `main-shm`; neither ParserHost nor Rust resolves companion paths from `LogicalPath`.
- A successfully published local package preview retains its source under the parent preview ID.
  Package icon Hero extraction acquires an independent lease and calls the package icon HANDLE ABI.
  Supplied parent IDs fail closed when stale, closed, or of the wrong kind; only parentless
  compatibility requests may use the path export. Package and Office Hero raster packets are
  written directly by Rust into a bounded ParserHost-owned anonymous section. ParserHost retains
  the section until `HeroRasterExtractClose`, failed publication, replacement, or disconnect; the
  App duplicates only read access and copies only the validated BGRA payload into its final image.
- New Rust Office layouts publish normalized `imageRef` plus `imageByteLength` metadata and do not
  embed `imageBase64` in the 4 MiB control message. ParserHost binds the exact published ref/length
  whitelist to the retained Office parent. `OfficeImageOpen` acquires an independent read-only
  lease and calls `ql_extract_office_layout_image_handle`; a stale or wrong-kind parent, an
  unpublished ref, or a ref outside `word/media`, `ppt/media`, or `xl/media` fails closed. Rust
  writes the checked `[width, height, premultiplied BGRA]` packet directly into a bounded anonymous
  section. The App captures the request Host generation, duplicates only `SECTION_MAP_READ`, maps
  the exact claimed packet length, and always sends `OfficeImageClose`. ParserHost releases child
  sections and leases on close, parent replacement/close, failed publication, or disconnect; no
  Office-image temporary directory exists. The Presenter starts these requests only when a page is
  materialized, deduplicates them by ref, allows at most two decodes concurrently, cancels stale
  sessions, and uploads validated BGRA to `WriteableBitmap`. The C# contract keeps
  `ImageBase64` only as a one-version compatibility fallback for older native JSON.
- Local certificate previews read at most 1 MiB from offset zero of the transferred HANDLE with
  `RandomAccess.ReadAsync`, then parse one DER/PEM certificate with the .NET span loader. They are
  stateless and release the input after publication without creating a `parser-input` anchor.
  PKCS#7 `.p7b`/`.p7c` bundles return a stable unsupported status; certificate parsing itself is
  synchronous and is contained by the ParserHost timeout rather than cooperative cancellation.
- RasterHost local image previews retain the received file object by parent request ID and acquire an
   independent read-only lease for `ql_decode_image_handle`; they do not create a `raster-inputs`
  anchor. Rust validates the logical basename and actual format, bounds SVG input to 16 MiB, disables
   external SVG image resolution, and returns a bounded premultiplied BGRA packet. Decode failure is
   terminal and cannot fall back through the logical path. The owner is released on close, failure,
   replacement, or disconnect while an already-acquired lease remains independently valid. SVG
   cancellation is observed between decode stages; `usvg` parsing and `resvg` rendering are not
   cooperatively interruptible, so these bounds do not provide a hard CPU-time cutoff.
- GIF, animated WebP, and APNG follow-ups resolve the parent preview ID to that retained source,
  acquire a separate animation lease, and call the capability-gated
  `ql_decode_animation_frames_handle`. The legacy `ql_decode_gif_frames_handle` export and bit 10
  remain available for ABI-compatible GIF fallback. Closing or replacing the parent prevents
  new leases while a decode that already acquired one remains valid. Rust writes the bounded frame
  packet directly into a RasterHost-owned anonymous section. The App maps one read-only duplicate,
  validates the exact packet once, retains that mapping for the playback lifetime, and writes each
  frame span directly into the fixed WinRT pixel buffer without creating per-frame managed arrays.
  Frame reads, waveform scans, and unmapping share one lifetime gate. RasterHost retains its owner
  until `PreviewAnimationFramesClose`, failed publication, replacement, or disconnect; the App
  mapping remains independently valid after that close, and no `raster-animation` temporary path
  exists. The Rust file probe publishes tri-state `isAnimated`
  metadata:
  `true` and `false` are authoritative, while `null` lets older native binaries or probe-budget
  exhaustion conservatively reach the decoder. Animation follow-ups have a separate 20-second
  timeout; a timeout closes only the follow-up and leaves the already-published static preview alive.
- Local system-codec images create a WinRT random-access stream over an independently reopened lease
  of the retained source HANDLE. PNG/JPEG/BMP/TIFF/WebP may fall back to Rust decoding through the
  same retained object; AVIF/HEIC/JXL and Adobe-marker JPEG remain system-codec-only. These requests
  do not create a `raster-inputs` anchor and cannot fall back to a Shell/path provider. Local PDF
  sessions similarly call `PdfDocument.LoadFromStreamAsync` over an independently reopened HANDLE
  stream and retain that stream until page operations drain on close. Their page-cache identity comes
  from the disk-file volume/index, exact length, and last-write time rather than the logical path.
  RasterHost no longer materializes any HANDLE input under `raster-inputs`; unsupported HANDLE kinds
  fail closed after adoption. Shell thumbnails remain available only to explicit path-based
  cloud/legacy compatibility requests and never receive a path derived from a HANDLE request.
  Replacing the original path after local handoff cannot change the rendered bytes.
- Optional image metadata is a separate parent-bound child request. RasterHost immediately acquires
  an independent metadata lease from the retained local image and calls capability bit 19
  `ql_preview_image_metadata_handle`; the logical basename is only a format hint and is never
  resolved as a path. The Rust reader is cancellable, reads bounded prefixes, and returns at most
  1 MiB of typed JSON. In parallel, RasterHost may supplement missing fields through the fixed
  System32 `PhotoMetadataHandler.dll` using `IInitializeWithStream`/`IPropertyStore`, and through a
  WIC `BitmapDecoder` over another independently reopened HANDLE stream. The Property Handler
  receives a bounded read-only COM stream; the basename only validates an image extension and is
  never opened. Direct `LoadLibraryEx(..., LOAD_LIBRARY_SEARCH_SYSTEM32)` plus
  `DllGetClassObject` avoids per-user COM registration and path-initialized activation. Field
  precedence is Rust native, then the Property Handler, then WIC. All three optional readers run
  under the 1.5-second metadata budget; a Property Handler call that cannot drain within 250 ms
  fail-stops only RasterHost. The first surface and `PreviewReady` do not wait for this sidecar. An
  accepted child may finish after its parent closes because it owns an independent reopened HANDLE,
  while child close and pipe teardown cancel and drain it. App-process `StorageFile.Properties`,
  `IInitializeWithFile`, parsing-name Property Stores, and logical-path fallback remain forbidden.
- Local archive entry extraction sends an optional parent preview request ID. ParserHost resolves
  that ID to the retained archive source handle before considering the legacy path fallback, and the
  Rust archive-entry HANDLE ABI reopens the same file object. Before sending the request, the App
  creates the final zero-length child anchor and duplicates its writable HANDLE into ParserHost.
  Rust streams at most 64 MiB directly into that caller-owned object; ParserHost closes every
  writable duplicate before replying, and the App transitions its original HANDLE to a strict
  read-only anchor without a second file copy. ParserHost creates no archive-entry temp file or
  writable extraction root.
- Cloud fail-closed compatibility inputs remain path-based and recycle the host when canceled while
  opening.
- ParserHost no longer materializes any `PreviewOpenHandle` input under `parser-input`; that writable
  directory is no longer created. Certificate inputs use the bounded managed HANDLE reader, SQLite
  uses its dedicated multi-HANDLE message, and every other supported local ParserHost kind uses the
  Rust HANDLE ABI. Unsupported HANDLE kinds fail closed after adoption without consulting the
  logical path. Explicit cloud/legacy `PreviewOpen` requests remain path-based compatibility inputs.
- For a normal local file, the App pins once before routing and uses `ql_probe_file_handle` as the
  single authoritative probe. ParserHost/RasterHost then receive that same pinned file object; there
  is no earlier path probe or second post-routing format probe. Directories and cloud metadata remain
  path-shaped by design. A native build without `HANDLE_PROBE`, or a local object that cannot be
  pinned, uses the logged legacy compatibility path. A HANDLE-probe failure after pinning fails
  closed. The HANDLE probe reads from an independent reopened file position and uses the logical
  basename only for extension routing. `PreviewOpenHandle` still carries path-shaped `LogicalPath`
  and `FileProbe.Path` metadata, but neither string provides file authority.

## Target protocol invariants

The final boundary uses handle-backed open messages whose numeric handles are valid in the receiving
host process:

```text
PreviewOpenHandle
  RequestId
  SourceHandle
  SourceLength
  LogicalPath
  FileProbe
  TargetWidth
  TargetHeight

PreviewOpenSqliteHandles
  RequestId
  MainHandle
  MainLength
  WalHandle
  WalLength
  ShmHandle
  ShmLength
  LogicalPath
  FileProbe

ArchiveEntryExtract
  RequestId
  ArchivePath
  EntryPath
  OutputHandle
  OutputCapacity
  ParentPreviewRequestId

PreviewImageMetadataOpen
  RequestId
  PreviewRequestId

PreviewImageMetadataReady
  RequestId
  PreviewRequestId
  ImageMetadata

PreviewImageMetadataClose
  RequestId
```

The App must:

1. Open the source with `GENERIC_READ` and the minimum required sharing flags. Local preview pins,
   including SQLite main/WAL/SHM pins, use `FILE_SHARE_READ` only. An existing companion that cannot
   be pinned is an error; only file-not-found/path-not-found means that optional companion is absent.
2. Probe metadata from that same file object, not by reopening the path.
3. Duplicate the read-only handle into the authenticated destination host.
4. Send the host-local handle, bounded metadata, and a logical filename used for extension routing
   and UI. Any path-shaped compatibility field is untrusted metadata, never file authority.
5. Dispose its source handle after duplication; the receiving host owns the duplicated object.
6. For an actual SQLite main file, and only in the App, try the exact sibling names formed by
   appending `-wal` and `-shm`. Do not derive companions when the selected input is itself a WAL/SHM
   file. A missing optional slot is encoded only as `(handle, length) == (0, 0)`; a nonzero handle
   with zero length is a present, empty file.
7. For an archive child, create a new zero-length output object under the App-owned anchor root,
   duplicate its writable HANDLE into ParserHost, and recycle ParserHost if delivery fails after
   duplication. Do not accept a Host path or Host-owned file HANDLE in the success response.

The host must:

1. Adopt every transferred handle immediately into an owning `SafeFileHandle`, before validating
   the request ID, probe, kind, lengths, cancellation state, or duplicate-request state. For
   `PreviewOpenSqliteHandles`, adoption covers main, WAL, and SHM slots as one ownership transfer.
2. Reject zero, invalid, non-disk, or structurally unexpected inputs. The App must only duplicate
   handles it opened read-only.
3. Derive length from the handle and compare it with the bounded probe metadata.
4. Never recover or trust a source path from the logical filename.
5. Dispose stateless preview handles on success, error, cancellation, timeout, disconnect, and stale
   request rejection. A published interactive archive listing is the deliberate exception: retain
   its owning source handle by request ID until `PreviewClose`, replacement, failure, or disconnect,
   and never expose that handle or a host-local source path to the App.
6. Recycle the host if a multi-handle duplication or control-channel send fails after any remote
   handle has been created. Process teardown is the reliable rollback for a partially transferred
   SQLite snapshot.
7. Adopt an archive output HANDLE before validating its message, require an initial length of zero
   and capacity no greater than 64 MiB, close all writable duplicates before the success response,
   and return only the exact written length and logical entry name.

For a normal local file, steps 1–5 are one identity-preserving sequence: the initial routing probe is
step 2, and the same pinned object continues into ParserHost/RasterHost. Directory routing, cloud
metadata, a missing `HANDLE_PROBE` capability, or a pin failure are explicit legacy path
compatibility cases. The HANDLE handoff accepts only the kind and exact length verified from the
pinned file object.

## Native ABI 3 HANDLE contract

ABI 2 introduced `ql_capabilities`; ABI 3 retains those assignments and adds optional data-plane
capabilities. Their stable assignments are:

```text
bit 0  HANDLE_TEXT
bit 1  HANDLE_EXECUTABLE
bit 2  HANDLE_TORRENT
bit 3  HANDLE_SQLITE_SNAPSHOT
bit 4  HANDLE_ARCHIVE
bit 5  HANDLE_OFFICE
bit 6  HANDLE_EBOOK
bit 7  HANDLE_ARCHIVE_ENTRY
bit 8  HANDLE_STATIC_IMAGE (ICO)
bit 9  HANDLE_SVG
bit 10 HANDLE_GIF (static and animation)
bit 11 HANDLE_PACKAGE
bit 12 HANDLE_PACKAGE_ICON
bit 13 HANDLE_PROBE
bit 14 HANDLE_RASTER_IMAGE (PNG/JPEG/BMP/TIFF/WebP native fallback; system codecs use the same HANDLE)
bit 15 HANDLE_ANIMATION (GIF/WebP/APNG animation packets; additive and optional for ABI 3 consumers)
bit 16 HANDLE_OFFICE_LAYOUT_IMAGE (parent-bound Office imageRef decode)
bit 17 HANDLE_IMAGE_WAVEFORM (static raster packet with Rust-generated RGB density)
bit 18 HANDLE_ARCHIVE_ENTRY_OUTPUT (caller-owned bounded archive-entry output object)
bit 19 HANDLE_IMAGE_METADATA (optional parent-bound Rust metadata sidecar)
bit 20 DIRECT_GIF_ANIMATION_OUTPUT (optional exact-size callback output; stable GIF exports remain fallback)
```

The corresponding implemented entry points share the validated/reopened HANDLE adapter: `ql_preview_text_handle`,
`ql_preview_executable_handle`, `ql_preview_torrent_handle`, `ql_preview_sqlite_handles`,
`ql_preview_archive_handle`, `ql_preview_office_handle`, `ql_extract_office_image_handle`,
`ql_extract_office_layout_image_handle`, `ql_preview_ebook_handle`,
`ql_extract_archive_entry_to_output_handle`, `ql_decode_image_handle` for capability-gated raster
packets, `ql_decode_image_with_waveform_handle` for single-pass RGB density, and
`ql_decode_animation_frames_handle` for GIF/WebP/APNG animation packets, plus
`ql_preview_image_metadata_handle` for the optional retained-image metadata child.
`ql_extract_archive_entry_handle` remains as an ABI-compatible legacy temp-path export but is not
used by ParserHost.
`ql_decode_gif_frames_handle` and `ql_decode_gif_frames_sized_cancelable` remain the stable GIF
entry points. Capability bit 20 permits RasterHost to use the additive exact-size direct-output
variants; an ABI 3 library without that bit stays on the stable entry points.
Package parsing and retained Hero extraction use `ql_preview_package_handle` and
`ql_extract_package_icon_handle`. The authoritative local routing probe uses
`ql_probe_file_handle` against the same pinned object later handed to the Host.
Plain text, Markdown, CSV, and TSV share one Reader parser; executable parsing reads at most a
cancellable 4 MiB prefix; torrent parsing performs an exact, cancellable read capped at 16 MiB before
the existing bounded bencode parser runs. Archive and ebook routes use bounded, cancellable
`Read + Seek` pipelines.

IPC ownership transfer and Rust FFI borrowing are two separate boundaries:

- App-to-Host `DuplicateHandle` creates a new process-local HANDLE. The receiving Host adopts that
  duplicate immediately into an owning `SafeFileHandle` and closes it on every terminal path. The
  App may close its original source HANDLE after duplication; for archive output it deliberately
  retains its original writer while ParserHost owns only the transferred duplicate.
- Host-to-Rust calls do not transfer ownership. ParserHost/RasterHost keeps its owning
  `SafeFileHandle` valid for the complete call, Rust treats the raw input or output value as
  borrowed, and Rust closes only the independent HANDLE returned by `ReOpenFile`. The Host remains
  responsible for closing the HANDLE it adopted from IPC.

The contract is:

1. The caller owns the source handle and must keep it valid for the complete FFI call.
2. Rust validates the raw value with `GetFileType == FILE_TYPE_DISK` and `GetFileSizeEx` before
   constructing any Rust handle type with a valid-handle precondition.
3. The exact length must match `expected_length`.
4. Rust uses `ReOpenFile(GENERIC_READ, FILE_SHARE_READ)` to obtain an owned handle with an independent
   file position. Rust closes only that reopened handle and never changes or closes the caller's
   handle.
5. `logical_name` is reduced to a non-empty basename, is limited to 1,020 UTF-8 bytes, and is used
   only for title and format routing. It is never opened as a path.
6. Buffer-returning calls require a writable `out_required`, initialize it to zero, and set it to
   the exact byte count once the result size is known. A null output buffer with zero capacity is a
   valid size query; insufficient capacity returns `BUFFER_TOO_SMALL` with that exact count. A null
   buffer paired with nonzero capacity is `INVALID_ARGUMENT`.
7. Streaming archive output instead requires writable `out_written`, which is initialized to zero
   and receives the exact length only on `OK`. Source and output HANDLE values must differ. The
   output must be a zero-length writable disk file, its capacity must satisfy
   `0 < output_capacity <= 64 MiB`, and its original HANDLE must permit write sharing. Rust reopens
   it with `GENERIC_WRITE` and an independent position, closes only that reopened HANDLE, and leaves
   the caller's output file position unchanged; changing the shared file object's bytes and length
   is the intended effect.
8. The Rust body is contained by `catch_unwind`. This converts Rust unwinds to `INTERNAL`; it cannot
   make invalid pointers, Windows structured exceptions, process aborts, or foreign callback
   exceptions safe.

Stable ABI 2/3 HANDLE statuses are:

```text
 0  OK
-1  INVALID_ARGUMENT
-2  BUFFER_TOO_SMALL
-3  CANCELLED
-4  MALFORMED
-5  IO
-6  INVALID_HANDLE
-7  LENGTH_MISMATCH
-8  INTERNAL
-9  LIMIT_EXCEEDED
```

These statuses apply only to HANDLE entry points. Existing path entry points retain their
legacy, per-function return conventions until each one is migrated.

## Archive HANDLE and retained-source contract

`ql_preview_archive_handle` accepts the selected archive object, its exact length, and a logical
basename. The logical name selects ZIP/TAR/GZip routing and supplies UI labels only. It is never
opened. Because archive readers seek over payload bytes rather than buffering the complete source,
the archive-specific HANDLE envelope accepts local files up to 16 TiB. Every parser inside that
envelope remains independently bounded: ZIP central-directory work is capped at 32 MiB; ZIP
preflight rejects more than 100,000 declared entries; metadata scans stop at 10,000 entries; and at
most 5,000 listing items are represented. RAR4/RAR5 uses a header-only scanner with a 2 MiB
per-header cap, 10,000-header cap, four-second deadline, checked `u64` seeks, and signature/CRC
validation. Normalized RAR paths are capped at 1,024 UTF-8 bytes and 128 components before parent
synthesis, and all represented path/name/parent strings share a 2 MiB budget. It never decompresses
RAR payloads and advertises `CanPreviewEntries=false`.
Archive-entry extraction keeps the existing 64 MiB compressed and uncompressed limits, expansion
ratio limit of 1,000, four-second deadline, cancellation checks, and checked streaming writes into
the caller's explicit 64 MiB output capacity.
RAR entry extraction is intentionally rejected; its preview is a browse-only metadata listing.

After a valid direct HANDLE archive listing is published, ParserHost retains the owning source
handle under the preview request ID. This includes the reader-based archive fallback for an EPUB
without a usable OPF. `ArchiveEntryExtract.ParentPreviewRequestId` is optional for protocol
compatibility; a click in a direct HANDLE listing, identified by its empty `RootPath`, sends it.
Anchored compatibility listings remain parentless and path-based. ParserHost validates a supplied
parent; only an absent parent ID may use the legacy `ArchivePath` fallback. Both branches call
`ql_extract_archive_entry_to_output_handle`; the
parentless compatibility branch first opens the archive path into a bounded read-only HANDLE.
A supplied but missing, closed, or wrong-kind parent fails closed instead of falling back to a
path. `PreviewClose`, failed publication, source replacement, and pipe teardown dispose the retained
owner and reject new child leases. An extraction that already acquired a lease uses its own reopened
read-only handle and may finish; releasing that lease closes the last reference.

`ArchiveEntryExtract` transfers an App-created, zero-length output disk HANDLE and a 64 MiB capacity.
ParserHost adopts that HANDLE before validating any other field. Rust validates both raw handles
before constructing Rust ownership wrappers, reopens each with an independent position, and streams
ZIP bytes in 64 KiB chunks with checked cumulative length. The caller's source and output positions
remain unchanged. Extraction errors, cancellation, and post-write length-validation failures make a
best-effort truncation through Rust's reopened output and leave `out_written == 0`; panic containment
is not transactional, so the App discards its anchor after every non-`OK` result. On success
ParserHost closes its writable duplicate before returning only the exact byte length and logical
entry name. The App then replaces its writer with a read-only pinned HANDLE and uses that same file
object for downstream probing.

## Ebook HANDLE contract

`ql_preview_ebook_handle` shares the path and HANDLE reader implementation. Local HANDLE ebook
inputs are capped at 256 MiB. ZIP central-directory work is capped at 32 MiB, EPUBs are capped at
8,192 ZIP entries and 16 MiB of cumulative decompressed content, container/OPF XML at 2 MiB, and
each chapter at 768 KiB. At most ten chapters and 140 Ki characters are retained in the result.
The OPF fallback scan and contents list remain bounded, and cancellation is checked during reader
and decompression work.

When an EPUB has no usable OPF, the ebook pipeline reuses the same validated, already-open ZIP reader
to publish a bounded archive listing with an empty `RootPath`. It never calls the path-based
`render_archive(logical_name)` fallback and never treats the logical name as path authority.
ParserHost retains that parent source for entry clicks exactly as it does for a direct archive
preview. FB2 and binary ebook metadata use the same reopened file object and the size/modified
metadata obtained from that object.

## SQLite snapshot contract

`ql_preview_sqlite_handles` receives the main handle plus optional WAL and SHM handles. It never
opens `logical_name` or uses it to locate companions. Each present handle is independently validated,
length-checked, and reopened before Rust constructs an owning file object. Optional tuples have the
same representation as the IPC message: only `(0, 0)` is absent.

The SQLite reader has three separate budgets:

- The main database preview prefix is at most 1 MiB.
- A WAL input is read exactly and cancellably, with a 64 MiB hard limit.
- An SHM input is limited to 4 MiB and is diagnostic only. SQLite's WAL-index is transient,
  native-endian state and is never a correctness source for the preview snapshot.

When a WAL is present, Rust validates the 32-byte WAL header, magic/checksum byte order, page size,
salt values, frame boundaries, and the rolling checksum. It scans frames with cancellation checks,
stops at the first incomplete or invalid frame, and overlays only frames through the last valid
commit marker. Frames after that marker are uncommitted and do not affect the preview. A later valid
frame for the same page replaces the earlier one. Malformed headers, incompatible page sizes, and
over-limit inputs fail closed rather than falling back to path-based parsing.

These rules follow SQLite's documented
[database/WAL format](https://www2.sqlite.org/fileformat2.html) and
[WAL-index format](https://sqlite.org/walformat.html).

## Office HANDLE and layout-image contract

Office main previews, embedded-image Hero extraction, and layout-image follow-ups share one retained
parent source object. The main call uses `ql_preview_office_handle`; a published Office preview
retains its owner until close, replacement, failure, or disconnect. Hero extraction acquires an
independent read-only lease before calling `ql_extract_office_image_handle`. A supplied but missing
or stale Hero parent fails closed without reopening the App-supplied path. Parentless path
extraction remains only for explicit legacy/cloud compatibility; a supplied parent ID never falls
back to that path.

The Rust layout JSON contains only canonical `imageRef` and bounded `imageByteLength` metadata for
new embedded images. ParserHost validates at most 18 refs and snapshots that exact whitelist into
the retained parent. Child raster requests use three control messages:

```text
OfficeImageOpen
  RequestId
  ParentPreviewRequestId
  ImageRef
  TargetWidth
  TargetHeight

OfficeImageReady
  RequestId
  SectionHandle
  PacketLength
  Width
  Height

OfficeImageClose
  RequestId
```

`OfficeImageOpen` never accepts path authority. ParserHost requires a live Office parent, an exact
whitelisted ref/length pair, a canonical package-relative ref under the format's media root, and
dimensions no greater than 1,024. It reopens an independent lease on the retained source and calls
`ql_extract_office_layout_image_handle`, which revalidates the disk HANDLE and exact source length,
ZIP structure, media root/ref, declared entry size, detected image format, 768 KiB uncompressed-entry
limit, 8,192-pixel/16-million-source-pixel limits, and the requested 1,024-pixel output boundary.
The caller's file position is unchanged.

ParserHost allocates an unnamed page-file section sized for the requested checked raster packet and
Rust writes `[u32 width][u32 height][premultiplied BGRA]` into its writable mapping. Only the
host-local section value and bounded metadata cross the control pipe. The App binds the response to
the captured ParserHost process/generation, duplicates `SECTION_MAP_READ`, maps exactly
`PacketLength`, validates both headers and exact `8 + width * height * 4` geometry, copies the BGRA,
and closes the remote request in `finally`. Explicit child close, parent close/replacement,
publication failure, and pipe teardown release the Host section and retained lease. No
`parser-office-image`, `office-image`, or other image-packet temp directory participates.

Office page virtualization remains the demand boundary: the Presenter requests a ref only while
materializing a page, caches one task per ref for duplicate placements, gates native decode at two
concurrent requests, and cancels the session when the preview is cleared or replaced. The result is
uploaded directly into a `WriteableBitmap`. `OfficeLayoutItem.ImageBase64` remains deserializable
only so an App paired with the immediately previous native JSON can render it; current Rust output
does not serialize that field, including the maximum 18-image layout case.

Remaining migration order:

1. Move explicit cloud/legacy Shell paths through a separately reviewed broker if RasterHost is
   later sandboxed beyond their required Shell access.

Shell thumbnail extraction is path/PIDL-based and should remain in a separate, more narrowly scoped
broker rather than weakening every parser host.

## Archive entry lifecycle

Local archive extraction no longer reopens `listing.RootPath`: it resolves the parent preview request
to the retained archive source handle. The App creates the final child anchor first, transfers only
a writable duplicate to ParserHost, and Rust streams directly into it. The successful response
contains no Host file HANDLE or temporary path. After ParserHost closes write authority, the App
transitions its original object to read-only and duplicates that pinned child into the destination
preview host using `PreviewOpenHandle`. Extension routing uses the sanitized archive entry name only.

This preserves the parent archive identity through extraction and the child identity through
handoff while eliminating the previous Rust temp path, ParserHost reopen, and App `CopyTo`/flush.

## Sandbox sequence

1. Complete handle-based input for ParserHost. (Complete.)
2. Enable a write-restricted ParserHost with dedicated output and pipe ACLs. (Complete.)
3. Keep D3D surface duplication App-pulled so RasterHost never opens the App process. (Complete.)
4. Move Shell thumbnails to a broker. (Complete for preview fallback.)
5. Test low integrity, then AppContainer without network capabilities.

Never relaunch a less-restricted host after a parser crash, timeout, invalid output, or malformed-file
failure. Compatibility fallback is allowed only before any untrusted input is opened.
