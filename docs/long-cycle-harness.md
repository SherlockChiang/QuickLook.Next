# Long-Cycle Harness

Use `tools\harness-long-cycle.ps1` to keep long-running parser and preview
improvements on a repeatable loop. The harness intentionally stays small: it
wraps existing project checks instead of introducing a new test framework.

## Flow

1. Pick one bounded improvement target.
2. Make the smallest native/Rust-first change that covers it.
3. Add or update a focused synthetic test or corpus smoke case.
4. Run the harness in quick mode with `-AllowDirty`.
5. Fix failures, rerun, then commit that one improvement.
6. Run full mode at phase boundaries or before review.

Use `docs\long-cycle-targets.md` as the shared queue for long-running parser and
preview coverage work.

## Quick Mode

Quick mode is the default per-commit loop:

```powershell
powershell -ExecutionPolicy Bypass -File tools\harness-long-cycle.ps1 -AllowDirty
```

It runs:

- Rust native tests via `cargo test`.
- Debug solution build via `dotnet build -c Debug --no-restore`.
- Architecture guard via `tools\guard-architecture.ps1 -SkipDist`.
- Final `git status --short --branch`.

Use `-SkipBuild` or `-SkipGuard` only for local diagnosis. Do not treat those as
review-ready verification.

## Full Mode

Full mode is the phase-boundary loop:

```powershell
powershell -ExecutionPolicy Bypass -File tools\harness-long-cycle.ps1 -Mode full -AllowDirty
```

Full mode includes quick mode and adds:

- Release native build via `cargo build --release`.
- Native FFI smoke via `tools\smoke-native.ps1`.

The architecture guard already runs the image corpus guard, external image
corpus smoke, image capability report, and system image corpus smoke.

## Dirty Worktree Policy

By default the harness stops if the worktree is dirty. This protects long-cycle
runs from accidentally validating unrelated changes.

During an active edit, pass `-AllowDirty`. After committing, rerun without
`-AllowDirty` when you want a clean baseline check.

## Focus Labels

`-Focus` is a lightweight label for logs. It does not filter tests.

```powershell
powershell -ExecutionPolicy Bypass -File tools\harness-long-cycle.ps1 -Focus "ELF notes" -AllowDirty
```

Keep filtering inside focused test commands while developing, then use the
harness before each commit.
