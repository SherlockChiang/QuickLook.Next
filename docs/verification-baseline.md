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

On 2026-09-05, `tools/release.ps1 -SkipPackage` completed with exit code 0 at
source `8c3d732b62f3cecda627b9befe3629e3865b693d`, `VERSION=1.0.1`, using the
pinned local SDK above. No product source changed during the run.

| Check | Result |
|---|---|
| Locked restore, Rust formatting, Clippy | Passed |
| .NET format verification | Exit 0; workspace-load warnings were reported |
| Rust workspace tests | 343 passed, 1 ignored external-corpus test |
| Native Release MSBuild contract | Passed; existing output was up to date |
| Release solution build | Passed, 0 warnings/errors, 22.56 seconds |
| Core tests | 351 passed |
| ParserHost / RasterHost / ShellBroker integration | 45 / 29 / 13 passed |
| Full architecture sequence, including MSBuild fixture | Passed |
| MSIX/Store version, package-structure, update-script guards | Passed; no installation implied |
| Restricted-host launch smoke | Passed |
| External native image corpus | Explicit ignored-test invocation passed; 10 samples checked |
| System image corpus | Passed under optional-codec policy; decoded 4/5, JXL unavailable |
| Title-bar inset structural guard | Passed; manual visual matrix not run |
| New MSIX packaging, WACK, install/update/uninstall, Narrator | Not run |
| Remote CI, publication, Store certification | Not performed |

The local transcript is `artifacts/validation-e26-01-8c3d732-full.log`, SHA-256:

```text
338812beee5473a28a97f401f9252b2b141b557b6ec10091f1ad8213c175e675
```

The generated payload proof was retained as
`artifacts/validation-e26-01-8c3d732-proof.json`; it binds the build outputs to
the source above, not a later documentation commit. Packaging must check the
current commit and payload hashes again. The sequence inspected the pre-existing
`dist` directory; it did not assemble a new package from this build. Distribution
acceptance therefore remains `E26-01d`, even though the no-package sequence passed.

The initial sandboxed attempt stopped at NuGet configuration access, before
.NET formatting, Clippy, tests, and builds. Its separate transcript is
`artifacts/validation-e26-01-8c3d732.log`. The successful run used approved
unsandboxed access for dependency restore and host-process tests. Fault-injection
tests intentionally catch errors in the transcript; their reported pass/fail and
the sequence exit code determine their outcome.

In the older blocked architecture sequence, MSIX version/update checks, Store
packaging checks, restricted-host launch, real image corpus checks, and title-bar
inset checks follow the native MSBuild fixture. Their result cannot be inferred
from guards that passed before that fixture. Explicitly recorded standalone
checks remain valid only for the source and environment where they ran.
