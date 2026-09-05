# Verification Baseline

This page records the evidence-first queue in `implementation-todo.md`. A failed
step stops the authoritative sequence; later steps are **not run**, not passed.
Compilation, source-pattern checks, runtime tests, packaged UI acceptance, and
Store certification are separate claims.

## Pinned Local Toolchain

On 2026-09-05 the Microsoft win-x64 SDK `10.0.302` archive was installed beside
the project under ignored `artifacts/tools/dotnet-10.0.302`. Neither `global.json`
nor the machine-wide SDK installation or PATH was changed. Rust and Cargo are
both `1.96.0`; the native target is `x86_64-pc-windows-msvc`.

The SDK download was verified against the SHA-512 from Microsoft's
`https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json`:

```text
https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.302/dotnet-sdk-10.0.302-win-x64.zip
7d170ed75fa9af34c00646621d92011dbd71943952e2787cd15df9be78e6452b55dadef34d7eff77b802e6af4959e071a55855ac649afeac70901c3a2a258716
```

In a repository-root PowerShell session, select that installation for the
current process and its children only:

```powershell
$ErrorActionPreference = 'Stop'
$localSdk = (Resolve-Path -LiteralPath './artifacts/tools/dotnet-10.0.302').Path
$env:DOTNET_ROOT = $localSdk
$env:DOTNET_ROOT_X64 = $localSdk
$env:PATH = $localSdk + [IO.Path]::PathSeparator + $env:PATH
dotnet --version
```

The expected result is `10.0.302`. An ordinary shell still uses the machine's
SDK selection; it needs either this session setup or an owner-installed pinned
SDK. Do not relax the repository's SDK lock to conceal a missing toolchain.

## Current Baseline

- Toolchain availability: passed.
- Full Release sequence: pending; retain logs and exact source SHA when run.
- Full architecture sequence: pending; older runs stopped at the SDK fixture.
- Product package installation and visual acceptance: not run in this queue.
- Remote integration and Store submission: not performed in this queue.

In the older blocked architecture sequence, MSIX version/update checks, Store
packaging checks, restricted-host launch, real image corpus checks, and title-bar
inset checks follow the native MSBuild fixture. Their result cannot be inferred
from guards that passed before that fixture. Explicitly recorded standalone
checks remain valid only for the source and environment where they ran.
