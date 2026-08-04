# Contributing To QuickLook Next

## Current Contribution Status

QuickLook Next is licensed under the [MIT License](LICENSE). Bug reports, feature requests, design
discussion, documentation, translations, tests, and focused code contributions are welcome.

By submitting a contribution, you represent that you have the right to submit it and agree that your
contribution is provided under the project's MIT License. Do not submit third-party code, assets,
samples, or translations unless their terms are compatible with MIT distribution and you preserve
all required notices. A contribution does not transfer ownership of your copyright.

## Development Environment

Use Windows x64 with the Desktop C++/MSVC toolchain and the pinned toolchains:

- .NET SDK from [`global.json`](global.json).
- Rust MSVC toolchain from
  [`native/rust-toolchain.toml`](native/rust-toolchain.toml).
- PowerShell 7 or newer, invoked as `pwsh`.

Use the focused local entry point from the repository root:

```powershell
.\build.ps1
.\build.ps1 -NoRestore
.\build.ps1 -Bump Patch -Test
.\build.ps1 -Bump Patch -Package
```

`VERSION` is authoritative; `tools/set-version.ps1` synchronizes the native crate manifest and
`native/Cargo.lock`. `build.ps1` creates local binaries only and does not update an installed MSIX.
Maintainers with the initialized fixed signing identity may pass `-Package` to enable all tests and
create signed local artifacts without installation, or explicitly pass `-Install` to package and
update only the current user's package with an increasing four-part MSIX revision.
Local revisions occupy `1..32767`; beta and stable packages use higher reserved revisions so a
formal same-base package can update a local installation.

The closest local equivalent to pull-request CI is:

```powershell
pwsh -NoProfile -File tools/release.ps1 -SkipPackage -SkipSystemImageSmoke
```

Do not run release signing or packaging with production credentials for a normal contribution.

## Engineering Expectations

Repository-wide placement and safety rules live in [`AGENTS.md`](AGENTS.md),
with durable decisions indexed in [`docs/adr/README.md`](docs/adr/README.md).

- Keep changes focused and independently reviewable.
- Preserve parser size, count, depth, time, decompression, and retained-memory limits.
- Keep untrusted structured parsing in `ParserHost`, raster/WinRT work in `RasterHost`, and explicit
  path-based Shell compatibility in `ShellBroker`.
- Use authenticated current-user pipes. The App pushes pinned input HANDLEs into hosts; host-produced
  output HANDLEs are pulled by the App. Do not give a host authority to open the App process.
- Treat logical paths and host messages as untrusted metadata. Local content routes should use the
  pinned HANDLE unless a documented cloud/compatibility path requires otherwise.
- Add cancellation, stale-generation rejection, cleanup, and malformed-input tests for asynchronous
  work.
- Keep en-US and zh-CN resource keys and format placeholders in parity for user-visible strings.
- Update format registry, architecture/performance guards, and reviewer-facing documentation when a
  capability or safety boundary changes.
- Do not weaken a guard just to make a change pass; update the implementation or explain the new
  invariant with executable coverage.

Avoid unrelated formatting churn, generated build output, credentials, certificates, private sample
files, and large binary fixtures. New corpus material must have clear redistribution permission and a
documented source; otherwise generate a minimal synthetic fixture in the test.

## Dependency Changes

Explain why a new dependency is necessary and how its security, maintenance, binary size, and runtime
cost were evaluated. Update `packages.lock.json` or `Cargo.lock` only when the dependency graph really
changes. Keep GitHub Actions pinned to immutable commit SHAs.

## Pull Requests

A pull request should include:

- The user-visible or security problem being solved.
- The relevant trust, ownership, cancellation, and resource-limit decisions.
- Tests that fail before the fix and pass afterward when practical.
- Commands run and any checks that could not be run locally.
- Screenshots for visible UI changes on desktop and, when relevant, constrained window sizes.
- Documentation and localization updates required by the change.

Before submitting, check `git diff --check`, review the complete diff for secrets and accidental
artifacts, and ensure the worktree contains only intended changes. Do not use a `release:` commit or
pull-request title for ordinary work; that prefix is reserved for the stable release workflow.

## Bug Reports

Public bug reports should include Windows and QuickLook Next versions, file type and approximate size,
expected and actual behavior, and reproducible steps. Redact paths and logs. Do not upload private
documents. Undisclosed vulnerabilities belong in the process described by [`SECURITY.md`](SECURITY.md).
