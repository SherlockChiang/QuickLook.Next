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
- Local ParserHost and RasterHost previews enter through `PreviewOpenHandle`. RasterHost copies the
  exact duplicated file object into a bounded host-owned anchor before invoking path-only native,
  WinRT PDF, system codec, shell-thumbnail, or animation providers. Replacing the original path after
  handoff cannot change the rendered bytes.
- Archive entry extraction returns a ParserHost-owned read-only handle. The App copies that exact
  object into a locked App anchor, then the normal pinned ParserHost/RasterHost handoff preserves the
  bytes through probing and rendering.
- Cloud fail-closed compatibility inputs remain path-based and recycle the host when canceled while
  opening.

## Required protocol

Add a handle-backed open message whose numeric handle is valid in the receiving host process:

```text
PreviewOpenHandle
  RequestId
  SourceHandle
  LogicalName
  FileProbe
  TargetWidth
  TargetHeight
```

The App must:

1. Open the source with `GENERIC_READ` and the minimum required sharing flags.
2. Probe metadata from that same file object, not by reopening the path.
3. Duplicate the read-only handle into the authenticated destination host.
4. Send only the host-local handle value and a logical filename used for extension routing and UI.
5. Dispose its source handle after duplication; the receiving host owns the duplicated object.

The host must:

1. Adopt the handle immediately into an owning `SafeFileHandle`.
2. Reject zero, invalid, non-disk, writable, or structurally unexpected inputs.
3. Derive length from the handle and compare it with the bounded probe metadata.
4. Never recover or trust a source path from the logical filename.
5. Dispose the handle on success, error, cancellation, timeout, disconnect, and stale request rejection.

## Native ABI migration

Rust entry points currently accept UTF-8 paths and call `File::open`. ParserHost and RasterHost bridge
this safely by creating bounded host-owned anchors from the duplicated object. Direct Windows handle
entry points remain a future optimization to remove that bounded copy; existing parsers should accept
`Read + Seek` where practical so path and handle entry points share the same implementation.

Required first consumers:

1. Archive listing and entry extraction.
2. Office and ebook ZIP/XML parsing.
3. Text, executable, torrent, and certificate previews.
4. Native still-image and animation decoders.
5. PDF and WIC paths using `IRandomAccessStream` created from the supplied handle.

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
