# Handle-based preview inputs

## Goal

Remove path re-resolution from untrusted preview processing. The App opens the selected file once,
pins that file object with a read-only handle, and duplicates only that handle into the selected host.
Renaming, deleting, replacing, or redirecting the original path must not change the bytes parsed by a
request that is already in progress.

## Current boundary

- Animation frame packets and ParserHost hero rasters are returned as host-owned, read-only file
  handles. The App pulls each handle from the already authenticated host process and validates the
  object type and exact length before reading it.
- Local ParserHost and RasterHost previews normally enter through `PreviewOpenHandle`. Database
  previews use the dedicated `PreviewOpenSqliteHandles` message so an SQLite main file and its
  optional WAL/SHM companions have explicit, independently owned slots.
- ParserHost text previews (plain text, Markdown, CSV, and TSV), executable metadata, torrent
  listings, archive listings, Office previews, ebook previews, and package previews pass the received file handle directly to the
  Rust ABI. They do not create a `parser-input` anchor or reopen the logical path.
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
  compatibility requests may use the path export.
- Local certificate previews read at most 1 MiB from offset zero of the transferred HANDLE with
  `RandomAccess.ReadAsync`, then parse one DER/PEM certificate with the .NET span loader. They are
  stateless and release the input after publication without creating a `parser-input` anchor.
  PKCS#7 `.p7b`/`.p7c` bundles return a stable unsupported status; certificate parsing itself is
  synchronous and is contained by the ParserHost timeout rather than cooperative cancellation.
- RasterHost ICO, SVG, and GIF previews retain the received file object by parent request ID and acquire an
   independent read-only lease for `ql_decode_image_handle`; they do not create a `raster-inputs`
  anchor. Rust validates the logical basename and actual format, bounds SVG input to 16 MiB, disables
   external SVG image resolution, and returns a bounded premultiplied BGRA packet. Decode failure is
   terminal and cannot fall back through the logical path. The owner is released on close, failure,
   replacement, or disconnect while an already-acquired lease remains independently valid. SVG
   cancellation is observed between decode stages; `usvg` parsing and `resvg` rendering are not
   cooperatively interruptible, so these bounds do not provide a hard CPU-time cutoff.
- GIF animation follow-ups resolve the parent preview ID to that retained source, acquire a separate
  animation lease, and call `ql_decode_gif_frames_handle`. Closing or replacing the parent prevents
  new leases while a decode that already acquired one remains valid. Animation output packets remain
  RasterHost-owned temporary files transferred to the App by read-only handle; their path does not
  provide input authority.
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
- Local archive entry extraction sends an optional parent preview request ID. ParserHost resolves
  that ID to the retained archive source handle before considering the legacy path fallback, and the
  Rust archive-entry HANDLE ABI reopens the same file object. The extracted object is returned as a
  ParserHost-owned read-only handle. The App copies that exact object into a locked App anchor, then
  the normal pinned ParserHost/RasterHost handoff preserves the bytes through probing and rendering.
- Cloud fail-closed compatibility inputs remain path-based and recycle the host when canceled while
  opening.
- ParserHost no longer materializes any `PreviewOpenHandle` input under `parser-input`; that writable
  directory is no longer created. Certificate inputs use the bounded managed HANDLE reader, SQLite
  uses its dedicated multi-HANDLE message, and every other supported local ParserHost kind uses the
  Rust HANDLE ABI. Unsupported HANDLE kinds fail closed after adoption without consulting the
  logical path. Explicit cloud/legacy `PreviewOpen` requests remain path-based compatibility inputs.
- The App's initial routing probe remains path-based for cloud and compatibility behavior. After it
  pins a local ParserHost or RasterHost source, the final verified probe uses
  `ql_probe_file_handle` when native advertises `HANDLE_PROBE`; a native probe failure then fails
  closed. Only native builds without that capability may use the legacy path probe. The HANDLE probe
  reads from an independent reopened file position and uses the logical basename only for extension
  routing. `PreviewOpenHandle` still carries path-shaped `LogicalPath` and `FileProbe.Path` metadata,
  but neither string provides file authority.

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

Step 2 applies to the final verified local probe after the App pins the source. The earlier routing
probe remains path-based by design so cloud availability and legacy compatibility behavior do not
change. ParserHost/RasterHost handoff accepts only the kind and exact length verified from the pinned
file object when `HANDLE_PROBE` is available.

## Native ABI 2 HANDLE contract

ABI 2 introduces `ql_capabilities` with independent HANDLE feature flags. Their stable assignments
are:

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
```

The corresponding implemented entry points share the validated/reopened HANDLE adapter: `ql_preview_text_handle`,
`ql_preview_executable_handle`, `ql_preview_torrent_handle`, `ql_preview_sqlite_handles`,
`ql_preview_archive_handle`, `ql_preview_office_handle`, `ql_extract_office_image_handle`,
`ql_preview_ebook_handle`, `ql_extract_archive_entry_handle`, `ql_decode_image_handle` for
capability-gated ICO/SVG/GIF raster packets, and `ql_decode_gif_frames_handle` for GIF animation
packets. Package parsing and retained Hero extraction use `ql_preview_package_handle` and
`ql_extract_package_icon_handle`. Final local handoff verification uses `ql_probe_file_handle`.
Plain text, Markdown, CSV, and TSV share one Reader parser; executable parsing reads at most a
cancellable 4 MiB prefix; torrent parsing performs an exact, cancellable read capped at 16 MiB before
the existing bounded bencode parser runs. Archive and ebook routes use bounded, cancellable
`Read + Seek` pipelines.

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
6. `out_required` is mandatory. A null output buffer with zero capacity is a valid size query;
   insufficient capacity returns `BUFFER_TOO_SMALL` and the exact required byte count.
7. The Rust body is contained by `catch_unwind`. This converts Rust unwinds to `INTERNAL`; it cannot
   make invalid pointers, Windows structured exceptions, process aborts, or foreign callback
   exceptions safe.

Stable ABI 2 HANDLE statuses are:

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

These statuses apply only to ABI 2 HANDLE entry points. Existing path entry points retain their
legacy, per-function return conventions until each one is migrated.

## Archive HANDLE and retained-source contract

`ql_preview_archive_handle` accepts the selected archive object, its exact length, and a logical
basename. The logical name selects ZIP/TAR/GZip routing and supplies UI labels only. It is never
opened. Local HANDLE archive inputs are capped at 256 MiB; ZIP central-directory work is capped at
32 MiB; ZIP preflight rejects more than 100,000 declared entries; metadata scans stop at 10,000
entries; and at most 5,000 listing items are represented.
Archive-entry extraction keeps the existing 64 MiB compressed and uncompressed limits, expansion
ratio limit of 1,000, four-second deadline, cancellation checks, and bounded temp-root retention.

After a valid direct HANDLE archive listing is published, ParserHost retains the owning source
handle under the preview request ID. This includes the reader-based archive fallback for an EPUB
without a usable OPF. `ArchiveEntryExtract.ParentPreviewRequestId` is optional for protocol
compatibility; a click in a direct HANDLE listing, identified by its empty `RootPath`, sends it.
Anchored compatibility listings remain parentless and path-based. ParserHost validates a supplied
parent and calls `ql_extract_archive_entry_handle`; only an absent parent ID may use the legacy
`ArchivePath` fallback. A supplied but missing, closed, or wrong-kind parent fails closed instead of
falling back to a path. `PreviewClose`, failed publication, source replacement, and pipe teardown
dispose the retained owner and reject new child leases. An extraction that already acquired a lease
uses its own reopened read-only handle and may finish; releasing that lease closes the last reference.

The extracted entry may still use a ParserHost-private bounded temp file before being published as a
read-only handle. The App's downstream anchor remains required by path-only raster compatibility
providers, but its final local probe reads the pinned child file object. Neither path is authority
for reopening the parent archive.

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

Office main previews and embedded-image hero extraction now retain the same parent source object.
The main call uses `ql_preview_office_handle`; a published Office preview retains its owner until
close/replacement/disconnect, and the hero follow-up acquires an independent read-only lease before
calling `ql_extract_office_image_handle`. A supplied but missing or stale parent fails closed without reopening
the App-supplied path. Parentless path extraction remains only for explicit legacy/cloud compatibility;
a supplied parent ID never falls back to that path.

Remaining migration order:

1. Move explicit cloud/legacy Shell paths through a separately reviewed broker if RasterHost is
   later sandboxed beyond their required Shell access.

Shell thumbnail extraction is path/PIDL-based and should remain in a separate, more narrowly scoped
broker rather than weakening every parser host.

## Archive entry lifecycle

Local archive extraction no longer reopens `listing.RootPath`: it resolves the parent preview request
to the retained archive source handle. ParserHost currently creates a bounded private output, opens it
read-only, and lets the App pull that handle. The App then copies that exact object into its
read-shared anchor and duplicates the pinned object into the destination preview host using
`PreviewOpenHandle`. Extension routing uses the sanitized archive entry name only.

This preserves the parent archive identity through extraction and the child identity through handoff.
A future bounded writable section or caller-provided output handle can remove the remaining child
temp/anchor copy after remaining raster providers no longer require a path.

## Sandbox sequence

1. Complete handle-based input for ParserHost. (Complete.)
2. Enable a write-restricted ParserHost with dedicated output and pipe ACLs. (Complete.)
3. Reverse D3D surface duplication so RasterHost cannot open the App process.
4. Move Shell thumbnails to a broker.
5. Test low integrity, then AppContainer without network capabilities.

Never relaunch a less-restricted host after a parser crash, timeout, invalid output, or malformed-file
failure. Compatibility fallback is allowed only before any untrusted input is opened.
