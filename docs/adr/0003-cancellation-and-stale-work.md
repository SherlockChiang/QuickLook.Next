# ADR-0003: Cancel by generation and reject stale work

- Status: Accepted
- Date: 2026-08-04

## Context

Explorer selection, window navigation, resize, page rendering, metadata, and
animation can overlap. Cancellation alone is racy: a callback can complete after
its token is canceled, and some WinRT/native operations cannot stop immediately.

## Decision

Every preview transition creates a monotonically increasing generation and a
new cancellation token. Async continuations capture the complete session
snapshot and must validate generation, token, and where relevant path or parent
request ID before mutating UI, focus, cache, or retained state.

Every IPC request has a unique request ID, and the supervisor accepts at most one
typed terminal host message. The client may instead stop awaiting because of
cancellation, timeout, disconnect, or service failure. Canceling removes the
request from the pending table; late or duplicate terminal messages are rejected.
An explicit close need not manufacture a content error. Parent-bound children
also carry their parent request ID or page generation and fail closed when that
owner is stale.

Cancellation propagates through close messages, host request tables, and native
callbacks at bounded work boundaries. Parsers and loops must keep size/count/time
budgets even when cooperative cancellation is unavailable. Cancellation must not
dispose a stream, HANDLE, surface, or synchronization object beneath an active
operation: drain it within a hard bound, then recycle/terminate the isolated host
if a safe drain is impossible.

The UI thread never waits synchronously for host or filesystem work. Canceled or
stale work may be ignored only after all ownership transfers are acknowledged or
reclaimed.

## Consequences

- Correctness depends on both cancellation and identity checks; either one alone
  is insufficient.
- Non-cancelable Windows APIs are contained by the process boundary rather than
  unsafe early disposal.
- Tests must exercise late completion, close/replacement, timeout, and cleanup.
- ShellBroker's current synchronous thumbnail call is canceled fail-stop by
  stopping/recycling the broker; a bounded cooperative queue remains follow-up
  work under `R26-P1-09`.

## Verification

- `src/QuickLook.Next.App/PreviewSession.cs`
- `src/QuickLook.Next.Core/PendingRequests.cs`
- `tools/guard-stale-callbacks.ps1`
- App/Core and host integration tests under `tests/`
