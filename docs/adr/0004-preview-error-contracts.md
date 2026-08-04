# ADR-0004: Use typed, request-bound preview errors

- Status: Accepted
- Date: 2026-08-04

## Context

Host exceptions, codec failures, timeouts, process crashes, surface failures,
and ordinary unsupported content need different recovery behavior. Exception
text is unstable, may expose implementation detail, and cannot safely identify
the preview or user action it belongs to.

## Decision

For each opened request, the App accepts at most one typed host terminal:
success or `PreviewError`. Client cancellation, timeout, disconnect, and process
failure end the local await without pretending to be Host content errors. Page
and follow-up operations use their own typed terminal messages and parent
identity. A host error always carries the request ID; optional stable code and
normalized format fields drive remediation. New logic must never branch on the
human-readable host message.

The App maps known codes and local failure kinds to localized, user-safe titles,
messages, and an explicit retry policy. Unknown codes and host exception text
fail closed to a generic content/service error. Before showing an error, moving
focus, or executing Retry/Open/Reveal, the App binds the failure to the current
preview snapshot and revalidates generation, cancellation token, and path.

Timeout, disconnect, crash, invalid protocol, and incomplete ownership transfer
are supervisor failures: fail pending requests, reclaim or recycle the host, and
keep Windows fault UI non-interactive. Wire additions are additive and bounded;
unknown or late request IDs do not mutate current state. Most ParserHost errors
still carry unstable exception text, so this decision does not claim that every
wire message is path-free; such text is diagnostic-only and is never copied into
recovery logic or shown directly by the App.

## Consequences

- Hosts report machine-readable facts; the App owns localization and recovery UI.
- Generic fallback is preferred to leaking exception detail or acting on the
  previous file.
- Adding a user-visible recovery branch requires a stable code and focused tests.

## Follow-up

The redacted Copy Diagnostics payload (stable phase, correlation ID, version,
format, and size bucket) remains tracked by `R26-P1-04`. When implemented it must
be additive and redact local paths by default; this ADR does not claim that UI
already exists.

## Verification

- `src/QuickLook.Next.Core/Protocol.cs`
- `src/QuickLook.Next.Core/PendingRequests.cs`
- `src/QuickLook.Next.App/PreviewSession.cs`
- `tools/test-supervised-host-error-ui.ps1`
- `tools/guard-stale-callbacks.ps1`
