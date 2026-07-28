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
  [`native/quicklook_next_native/rust-toolchain.toml`](native/quicklook_next_native/rust-toolchain.toml).

Restore locked dependencies and build from the repository root:

```powershell
dotnet restore QuickLook.Next.slnx --locked-mode
cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml
cargo build --release --locked --manifest-path native/quicklook_next_native/Cargo.toml
dotnet build QuickLook.Next.slnx -c Release --no-restore
dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore
```

The closest local equivalent to pull-request CI is:

```powershell
pwsh -NoProfile -File tools/release.ps1 -SkipPackage -SkipSystemImageSmoke
```

Do not run release signing or packaging with production credentials for a normal contribution.

## Engineering Expectations

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
