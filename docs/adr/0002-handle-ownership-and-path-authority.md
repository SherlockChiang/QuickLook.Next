# ADR-0002: Transfer HANDLE ownership, not path authority

- Status: Accepted
- Date: 2026-08-04

## Context

A path can be renamed, replaced, redirected, or resolve to different bytes after
the App probes it. Host-local numeric HANDLE values also have meaning only inside
the process that owns them. Preview correctness therefore needs an explicit
object-identity and ownership contract.

## Decision

For ordinary local files, the App opens and probes one exact read-only file
object, duplicates that object into the authenticated destination host, and
sends only the host-local HANDLE value, exact bounded length, probe, and a
logical filename. Logical and compatibility paths are untrusted routing/UI
metadata; they do not grant file authority.

The receiving host adopts every nonzero transferred HANDLE into an owning
`SafeFileHandle` before validating any other field, including request ID,
length, kind, duplicate state, or cancellation. It then validates disk type and
authoritative length. Stateless requests dispose the owner on every terminal
path. Deliberately retained parents, such as interactive archive, Office,
package, image, and PDF sessions, are keyed by request ID and close on explicit
close, replacement, failure, disconnect, or process teardown.

Rust receives a bounded borrowed/reopened view while managed code retains the
owning HANDLE. Rust must not resolve the logical path or outlive that owner.
Multi-HANDLE snapshots are one transfer: partial duplication or delivery
failure recycles the host because process teardown is the reliable rollback.

Host-produced shared sections remain host-owned until the App duplicates the
minimum read access, validates the exact packet layout, and acknowledges release.
For archive extraction, the App owns the output object and transfers only a
bounded writable duplicate; the host closes all write authority before success
and returns length plus logical name, never a path or source HANDLE.

Path-based opens remain explicit exceptions for directories, cloud metadata or
hydration, missing HANDLE capabilities, pin failures, and Shell compatibility.
They must fail closed and must not become an implicit fallback after a pinned
HANDLE request has begun.

## Consequences

- File identity remains stable across rename/replace races.
- Ownership code must be slightly more verbose, but every leak/rollback path has
  one accountable process owner.
- Follow-up requests bind to a retained parent request instead of reopening a path.

## Verification

- [`docs/handle-based-preview-inputs.md`](../handle-based-preview-inputs.md)
- `src/QuickLook.Next.Core/Protocol.cs`
- ParserHost and RasterHost integration tests under `tests/`
- `tools/guard-architecture.ps1`
