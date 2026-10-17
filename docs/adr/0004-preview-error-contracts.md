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
identity. A host error always carries the request ID; optional stable code,
normalized format, and phase fields drive remediation and diagnostics. New logic
must never branch on the human-readable host message.

The App maps known codes and local failure kinds to localized, user-safe titles,
messages, and an explicit retry policy. Unknown codes and host exception text
fail closed to a generic content/service error. Before showing an error, moving
focus, or executing Retry/Open/Reveal/Copy Diagnostics, the App binds the failure
to the current preview snapshot and revalidates generation, cancellation token,
and path.

Timeout, disconnect, crash, invalid protocol, and incomplete ownership transfer
are supervisor failures: fail pending requests, reclaim or recycle the host, and
keep Windows fault UI non-interactive. Wire additions are additive and bounded;
unknown or late request IDs do not mutate current state. ParserHost, RasterHost,
and ShellBroker send a fixed failure message for unexpected exceptions,
malformed native JSON, and broker failures.
The shared factory accepts no exception or message, bounds phase to 48 characters
and format to 32 characters, and rejects paths and invalid metadata. Original
exceptions stay in host logs. PDF page errors also use fixed messages. Legacy
messages remain readable without the optional fields; their text is never copied
into recovery logic or shown directly by the App.

## Consequences

- Hosts report machine-readable facts; the App owns localization and recovery UI.
- Generic fallback is preferred to leaking exception detail or acting on the
  previous file.
- Adding a user-visible recovery branch requires a stable code and focused tests.

## Copy diagnostics

`R26-P1-04` adds a sealed, Core-created schema for the error panel's Copy
Diagnostics action. The bounded JSON contains only schema version, stable error
code, lifecycle phase, correlation ID, application version, normalized format,
and a coarse size bucket. The constructor is internal and the factory accepts no
path, file name, localized message, or title, so callers cannot populate the
clipboard payload with those values. Format and size come from the Rust-first
`FileProbe`; WinUI only binds the snapshot to the current error generation and
performs the explicit clipboard write.

## Verification

- `src/QuickLook.Next.Core/Protocol.cs`
- `src/QuickLook.Next.Core/PendingRequests.cs`
- `src/QuickLook.Next.Core/PreviewFailureDiagnostics.cs`
- `src/QuickLook.Next.App/PreviewSession.cs`
- `tools/test-preview-error-diagnostics-ui.ps1`
- `tools/test-supervised-host-error-ui.ps1`
- `tools/guard-stale-callbacks.ps1`
