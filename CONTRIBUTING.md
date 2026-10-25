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
  [`rust-toolchain.toml`](rust-toolchain.toml).
- PowerShell 7 or newer, invoked as `pwsh`.

Run `pwsh -NoProfile -File tools/doctor.ps1` first for read-only checks of the
pinned SDK, Rust, MSVC, and Windows SDK. Then use the focused local entry point
from the repository root:

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

The native crate denies `unsafe_op_in_unsafe_fn` and
`clippy::undocumented_unsafe_blocks`, including root, Win32 and test modules;
pure preview modules forbid unsafe code. Every unsafe operation needs an explicit
block and a local explanation of its actual storage, lifetime or Windows ownership
requirements. Do not downgrade these lints or add blanket exemptions. Pointer
checks and panic containment do not establish pointer validity or prevent UB.
Byte output borrowed as a mutable Rust slice must already contain initialized
bytes; managed arrays and fresh pagefile-backed sections satisfy that requirement.
Keep callbacks rooted until producers and in-flight calls finish, copy transient
native strings, and contain managed callback exceptions before returning to Rust.

`tools/test-rust-lint-scope.ps1` also compiles one positive fixture and five
expected failures using the pinned Clippy driver, actual crate/preview attributes
and actual common/raster writer modules. It verifies implicit unsafe operations,
undocumented unsafe blocks, safe calls to unsafe writers and preview unsafe code
are rejected. This complements the all-target Clippy gate; it does not prove
foreign pointer validity. Initialize worker COM apartments with the thread-affine
guard, balance S_OK/S_FALSE, and release interfaces before that guard drops.

## Dependency Changes

Native ABI changes start in `native/abi-contract.json`. Schema v2 describes the
pinned target/calling convention, constants, ordered export parameter types and
callback types; do not edit generated files directly. Regenerate with
`pwsh -NoProfile -File tools/generate-native-abi.ps1`. The production Rust crate
compiles the generated function-pointer assertions and constant-evaluates exact
version/capability values. Unsafe Rust caller contracts still require the safety
lints/guard: a safe function can coerce to an unsafe function pointer.

Run `tools/test-native-abi-contract.ps1` for malformed-schema and compiler mutation
fixtures, then build the DLL through MSBuild and run
`tools/test-native-abi-exports.ps1`. The latter reads bounded file-backed PE sections
without loading/executing the DLL; it rejects extra/missing names, forwards,
ordinal aliases and non-executable exports. Incidental dependency symbols are
separate exact-name, locked-version inventories, never wildcard allowances.
Managed declarations and packet layout tests remain independent of PE names.
The schema's `managedBindings` lists each production owner/import, its unmanaged
delegate types and explicit raw callback positions. Preserve fixed-arity exact-name Cdecl,
PreserveSig, native-sized integer types, `[In]` byte inputs, `[In, Out]` byte
outputs, scalar `out` slots and `[return: MarshalAs(UnmanagedType.I1)]` bool
callbacks. Byte outputs need In/Out to retain initialized contents on a copying
marshaller. Keep native caller validity/rooting/alias/no-unwind obligations;
metadata cannot establish them.

After the Release build, run `tools/test-managed-native-abi.ps1` to inspect all
four App/Host assemblies without loading them, and
`tools/test-managed-native-abi-fixtures.ps1` using the pinned SDK to verify
compiled incompatible declarations are rejected. The full architecture gate
runs these checks and requires already built native and managed win-x64 outputs.

The shared raw-pointer adapter contracts have an executable check as well:
`tools/test-rust-ffi-behavior.ps1` links the checked-in `ffi::common`,
`ffi::raster_packet`, `ffi::file_probe`, `ffi::thumbnail`, `ffi::path_preview`,
and `ffi::routing` modules into a bounded Windows probe. It tests
canary-preserving buffer/status
behavior, path and typed thumbnail errors, cancellation, packet bytes, and
panic containment, then compiles deliberate source mutations that must fail.
Path-preview renderers use observable substitutes: all twelve actual exports
must forward the correct arguments, skip rendering on pre-cancellation,
discard output on post-render cancellation, contain provider panics and recover
on a fresh call. These substitutes do not establish parser/file-I/O limits;
retain workspace tests against the real format providers. Fixture data stays
inside the harness-owned temporary directory, including failing mutation runs.
The architecture guard runs this check alongside the compiler safety fixtures.
The three actual routing exports have observable classifier/folder providers.
Checks preserve exact Unicode/binary arguments, bool-to-integer results,
optional extension/kind fallback, null/length/magic limits, folder short-buffer
canaries and provider-delegated cancellation, empty output, panic cleanup and
fresh recovery. The checked-in input limits are extracted rather than copied.
These substitutes do not establish real format detection or directory traversal
limits; retain the actual routing/folder provider tests. The shared/path/routing
fixture runs 22 behavior groups and rejects 120 warning-free compiled mutations.

The HANDLE preview boundary has a separate executable check:
`tools/test-rust-handle-preview-behavior.ps1` compiles the real
`ffi::handle_preview`, `ffi::handle_container`, `ffi::handle_structured`, `ffi::database`,
`native_input`, common output adapter and bounded file probe against the locked
Windows dependency artifact. It uses real Win32 `ReOpenFile` ownership and
checks independent positions, rename/decoy resistance, exact logical-name and
length validation, native/reader status mapping, output negotiation/canaries,
cancellation (including callback poll
ordering), panic containment and process-handle cleanup. PDF/image, container
and structured providers are observable substitutes for dispatch and error
paths; they do not replace format-parser tests. All four container and four
structured exports must forward the exact reopened reader, basename, length,
modification time (where applicable) and callback, and preserve all five typed
reader statuses where supplied by the provider. Text's
empty result maps to I/O failure or cancellation; a one-shot fourth-poll callback
checks its inner cancellation branch without masking by the shared post-render
check.
The database fixture additionally executes the legacy path export and SQLite's
main/WAL/SHM export. It distinguishes absent companions from present-empty files,
observes distinct pinned bytes and metadata after rename/decoy replacement, and
checks all optional tuple combinations, native reopen failures and later-failure
cleanup. Actual database byte limits and the companion descriptor are extracted
from production sources; real exact/one-byte-over file lengths exercise all three
limits without relaxing the shared reopen check. Providers read at most 32 bytes
per input. Legacy zero-on-cancellation, five HANDLE reader statuses,
cancellation precedence, panic recovery and both output protocols are independent
contracts; real SQLite/WAL parser tests remain required.
Short/exact/spare output, pre-cancelled null input, post-render cancellation,
empty output, panic containment and fresh recovery remain executable contracts.
The 18-group positive probe is followed by 112
warning-free source mutations, each of which must fail its expected behavior
group. The architecture guard runs this check after the shared FFI behavior
fixture.

Explain why a new dependency is necessary and how its security, maintenance, binary size, and runtime
cost were evaluated. Update `packages.lock.json` or `Cargo.lock` only when the dependency graph really
changes. Keep GitHub Actions pinned to immutable commit SHAs.

## Bounded Reader Properties

The ordinary native test gate includes `preview::reader_properties` for ZIP,
Torrent/bencode, CFB/MSG, and SQLite/WAL readers. All inputs are generated in
memory. Valid seeds, header-biased mutations, truncation, appended bytes and
arbitrary bytes exercise bounded JSON output, fragmented-read equivalence,
length rejection, pre-cancellation and cancellation during actual reads.
Positive properties check ZIP listing/extraction and output capacity, torrent
names/sizes, UTF-16 MSG subjects and committed WAL pages in both checksum orders.

Each property defaults to 128 cases with seed `0x514C_2026_100C`. The test reader
asserts limits of 2 MiB read, 131,072 read calls and 4,096 seeks per input stream;
successful JSON is capped at 512 KiB. These are test assertions over small inputs,
not new production limits or proof of wall-clock/heap bounds. Shrinking defaults
to 1,024 iterations/one second. Run an expanded session with another seed:

```powershell
$env:PROPTEST_CASES = '2048'
$env:PROPTEST_RNG_SEED = '20261012'
cargo test --workspace --locked --manifest-path native/Cargo.toml reader_properties
Remove-Item Env:PROPTEST_CASES, Env:PROPTEST_RNG_SEED
```

Proptest records failing seeds and prints minimized inputs; retain generated
`proptest-regressions` files and turn a confirmed failure into a focused synthetic
regression. No private files or large binary corpus are needed. This suite runs
on the pinned stable Windows toolchain; coverage-guided fuzzing and sanitizer
coverage for more format families remain tracked in `R26-P1-10`.

`proptest 1.12` is a dev-only MIT/Apache-2.0 dependency with only its `std` feature.
It adds five locked packages without upgrading existing packages. Fork/timeout
subprocess features are disabled; shrinking and replay avoid maintaining a custom
generator. Cargo resolver v2 excludes these additions from normal/build graphs,
so their compilation/runtime cost is confined to tests. Keep auditing the complete
lockfile and checking that future test features do not enter production builds.

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
