# ADR-0001: Supervise preview hosts

- Status: Accepted
- Date: 2026-08-04

## Context

Previewing untrusted files can hang, exhaust resources, invoke fragile Windows
codecs, or crash native code. Running that work in the WinUI process would turn
one bad preview into an application crash and could block Explorer input.

## Decision

The App owns the lifecycle of three purpose-specific background processes:

- `ParserHost` adopts bounded inputs and calls Rust structured-preview APIs.
- `RasterHost` owns Windows image/PDF/surface work that cannot live in portable
  Rust. It is not a general parser.
- `ShellBroker` contains explicit Explorer COM/Shell compatibility operations.

Each host is launched from an absolute executable path with a restricted token,
no visible console, process mitigations, and a one-process memory-bounded job.
The process is created suspended, assigned to the job, and only then resumed.
ParserHost and ShellBroker additionally use write-restricted tokens and explicit
writable roots; RasterHost retains the compatibility needed by WinRT/WIC/Shell
surface APIs.

Control pipes are current-user scoped and authenticated with a per-launch
session token plus peer-process checks. Readiness and requests have bounded
watchdogs. The App owns the process, job, channel, and pending request table;
disconnect, crash, timeout, partial HANDLE transfer, or protocol corruption
fails pending work and recycles the affected host.
Once untrusted input has entered a host, those failures never trigger a retry in
a broader-privilege process.

Background hosts request WER no-UI reporting and set the process error mode to
fail closed against interactive Windows fault dialogs, including the
`UnhandledExceptionFilter` `Application Error` path. The no-dialog guarantee
takes priority over WER/local-dump collection, which remains best effort because
`SEM_NOGPFAULTERRORBOX` can prevent WER invocation. Supervisor exit codes and
bounded file logs remain available. AppContainer and enforced network denial
are future hardening, not properties of this decision.

## Consequences

- Host startup and recycling cost are accepted in exchange for fault and resource
  containment; lazy start and bounded reuse control that cost.
- A compatibility requirement must be isolated to the narrowest host instead of
  weakening every host.
- Host failures become typed App state, never an OS modal dialog.

## Verification

- `src/QuickLook.Next.App/HostProcessLauncher.cs`
- `src/QuickLook.Next.App/HostProcessJob.cs`
- `src/QuickLook.Next.Core/SupervisedHostProcessPolicy.cs`
- `tools/smoke-restricted-host-launch.ps1`
- `tools/test-supervised-host-error-ui.ps1`
- `tools/guard-architecture.ps1`
