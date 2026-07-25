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
- Local ParserHost and RasterHost previews enter through `PreviewOpenHandle`.
- ParserHost text previews (plain text, Markdown, CSV, and TSV), executable metadata, and torrent
  listings pass the received file handle directly to the Rust ABI. They do not create a
  `parser-input` anchor or reopen the logical path.
- Other ParserHost formats and RasterHost currently copy the exact duplicated file object into a
  bounded host-owned anchor before invoking path-only native, WinRT PDF, system codec,
  shell-thumbnail, or animation providers. Replacing the original path after handoff cannot change
  the rendered bytes.
- Archive entry extraction returns a ParserHost-owned read-only handle. The App copies that exact
  object into a locked App anchor, then the normal pinned ParserHost/RasterHost handoff preserves the
  bytes through probing and rendering.
- Cloud fail-closed compatibility inputs remain path-based and recycle the host when canceled while
  opening.
- The App currently pins local files with a non-write/non-delete-sharing handle before calling the
  path-based probe. That pin prevents the path from being modified or replaced during probing, but
  the probe has not yet been converted to read from the same handle. `PreviewOpenHandle` also still
  carries path-shaped `LogicalPath` and `FileProbe.Path` metadata; direct Rust HANDLE routes reduce
  the logical value to a basename and never treat either string as file authority.

## Target protocol invariants

The final boundary uses a handle-backed open message whose numeric handle is valid in the receiving
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
```

The App must:

1. Open the source with `GENERIC_READ` and the minimum required sharing flags.
2. Probe metadata from that same file object, not by reopening the path.
3. Duplicate the read-only handle into the authenticated destination host.
4. Send the host-local handle, bounded metadata, and a logical filename used for extension routing
   and UI. Any path-shaped compatibility field is untrusted metadata, never file authority.
5. Dispose its source handle after duplication; the receiving host owns the duplicated object.

The host must:

1. Adopt the handle immediately into an owning `SafeFileHandle`.
2. Reject zero, invalid, non-disk, or structurally unexpected inputs. The App must only duplicate
   handles it opened read-only.
3. Derive length from the handle and compare it with the bounded probe metadata.
4. Never recover or trust a source path from the logical filename.
5. Dispose the handle on success, error, cancellation, timeout, disconnect, and stale request rejection.

## Native ABI 2 HANDLE contract

ABI 2 introduces `ql_capabilities` with independent `HANDLE_TEXT`, `HANDLE_EXECUTABLE`, and
`HANDLE_TORRENT` flags. The corresponding entry points share one validated/reopened HANDLE adapter.
Plain text, Markdown, CSV, and TSV share one Reader parser; executable parsing reads at most a
cancellable 4 MiB prefix; torrent parsing performs an exact, cancellable read capped at 16 MiB before
the existing bounded bencode parser runs.

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
```

These statuses apply only to ABI 2 HANDLE entry points. Existing path entry points retain their
legacy, per-function return conventions until each one is migrated.

Remaining migration order:

1. SQLite main files, followed by an explicit WAL/SHM companion-handle protocol.
2. Archive, Office, and ebook readers that require `Read + Seek`.
3. Native still-image and animation decoders.
4. PDF/WIC/Shell paths through separately reviewed Windows-specific adapters or brokers.

Shell thumbnail extraction is path/PIDL-based and should remain in a separate, more narrowly scoped
broker rather than weakening every parser host.

## Archive entry lifecycle

Archive extraction should no longer create a path that the App later previews. The App provides a
bounded writable section or file handle for the output, or ParserHost creates a read-only output handle
that the App pulls. The App then duplicates that exact object into the destination preview host using
`PreviewOpenHandle`. Extension routing uses the sanitized archive entry name only.

This preserves a single file identity across extraction, probing, and rendering and removes the current
same-user check/open race completely.

## Sandbox sequence

1. Complete handle-based input for ParserHost.
2. Enable a write-restricted ParserHost with dedicated output ACLs.
3. Reverse D3D surface duplication so RasterHost cannot open the App process.
4. Move Shell thumbnails to a broker.
5. Test low integrity, then AppContainer without network capabilities.

Never relaunch a less-restricted host after a parser crash, timeout, invalid output, or malformed-file
failure. Compatibility fallback is allowed only before any untrusted input is opened.
