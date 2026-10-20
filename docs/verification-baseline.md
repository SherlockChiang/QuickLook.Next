# Verification Baseline

This page records the evidence-first queue in `implementation-todo.md`. A failed
step stops the authoritative sequence; later steps are **not run**, not passed.
Compilation, source-pattern checks, runtime tests, packaged UI acceptance, and
Store certification are separate claims.

## October external review: host lookup and FFI contracts

### Hero image FFI behavior

On 2026-10-20, `O26-06c.10` was implemented in commit `3c6040a`. The HANDLE
behavior probe now compiles the actual `ffi::hero_image` module and an
observable provider fixture. Its six new groups cover the seven path/HANDLE
exports, checked raster packet output, Office image-reference and dimension
validation, typed reader failures, cancellation before and after file access,
output canaries, and reopened-file cleanup.

The focused `HeroImage` run passed 24 behavior groups and rejected 25 compiled
mutations. The full HANDLE run passed the same 24 groups and rejected 137
compiled mutations across the complete fixture. Rust formatting and the native
workspace tests passed with 365 tests and one ignored external-corpus test.
This batch changes only test tooling and fixtures; it does not change product
code, the ABI, dependencies, or the Release payload. A full architecture gate
was not claimed for this batch because the current machine does not expose the
repository-pinned .NET SDK `10.0.302`; its isolated gate stops at the managed
ABI fixture when that SDK is unavailable.

The parent `O26-06c` and `O26-06` remain open for any remaining equivalent
source-shape migrations. Performance, packaging, manual UI, HDR hardware, and
Store acceptance remain separate evidence tracks.

### Current pinned release baseline

On 2026-10-20, the complete `tools/release.ps1 -SkipPackage` sequence passed
against the current post-hero-image source with the bundled SDK `10.0.302` and
Rust `1.96.0`. Release 1.0.1 built with zero warnings and zero errors. The
locked restore, Rust formatting, all-target/all-feature Clippy, native tests,
native MSBuild, managed Release build and tests, ABI metadata, FFI behavior,
restricted-host, image corpus, format, installer, localization, and complete
architecture checks passed.

The test counts were Rust 365 passed with one ignored external-corpus test,
Core 390, ParserHost 49, RasterHost 53, and ShellBroker 13. The native corpus
decoded 10/10 samples. The system codec corpus decoded 4/5 samples; JXL was
unavailable under the existing optional-codec policy. This run produced the
tested build proof and did not package, install, sign, publish, or perform manual
UI, physical HDR, Store, or remote CI acceptance.

A complete rerun was performed after the mixed-format latency instrumentation
commits `bcb138a` and `7579b93`, using the same bundled SDK and source at
`7579b938ce27a69e1a59dceba9fd4c2971b1a9cd`. Release 1.0.1 again passed with
zero build warnings and errors. The current run reported Rust 364 passed and
one ignored test, Core 390, ParserHost 49, RasterHost 53, and ShellBroker 13.
The separate external image corpus smoke passed 10/10; the system image corpus
decoded 4/5 because JXL remains unavailable under the optional-codec policy.
The tested-release proof records the current payload identity and hashes under
`artifacts/.tested-release-build.json`. Packaging, installation, manual UI,
physical HDR, Store, remote CI, and publication acceptance remain open.

### Current build and preview measurements

On 2026-10-20, `tools/measure-build-baseline.ps1 -Iterations 2` collected a
current Release build report with SDK `10.0.302`. The median times were
36.837 seconds for clean builds, 23.630 seconds for incremental builds, and
21.490 seconds for no-change builds. Clean preparation and dependency restore
are outside the timed interval; the report is an observation and does not set a
performance budget. The report and per-run logs remain under ignored
`artifacts/`.

The preview extractor was also run against the current local App and RasterHost
logs. It found 146 ParserHost resource samples: private bytes were 14.3 MiB
at p50 and p95, and HANDLE count was 263 at p50 and p95. The logs contained no
parseable lifecycle preview, first-paint, cancellation, or RasterHost image-stage
samples, so those statistics remain null. The current data does not justify a
numeric responsiveness or decode budget; a mixed desktop input corpus and true
host-drain cancellation run are still required.

The repeatable RasterHost mixed-format probe was then run with the bundled
`10.0.302` SDK. It covered 12 PNG/ICO/JPEG/BMP opens in each mode (normal and
close-before-receiving) and wrote `artifacts/raster-latency-working.json` using
the `quicklook-next.raster-latency.v1` schema. Normal open-to-complete latency
was 11.08 ms p50 / 58.27 ms p95; close-to-writable recovery was 0.44 ms p50 /
33.00 ms p95. The immediate-close mode measured 11.94 ms p50 / 84.40 ms p95
from cancellation to replacement completion, with 0.46 ms p50 / 0.86 ms p95
close-to-writable recovery. These are host integration observations rather than
first-paint or direct worker-drain measurements; the report records the fixed
small corpus and those limitations. They establish a reproducible mixed-format
sample source, but do not set a product-wide budget.

On 2026-10-12, `O26-01` and the scoped `O26-02` implementation were verified in
the working tree based on `65843d06c1137993f6745fe789cb92811676f19f`, using the
pinned local SDK below. This evidence includes the existing uncommitted work;
it is not a clean-commit or packaged-payload proof.

The three App host lookup wrappers now use a Core resolver with consistent
deployed-subfolder/flat precedence. Sibling Debug outputs are considered only
in Debug builds. A missing executable returns the expected deployed path for
the existing lazy launcher, preserving hook initialization and unrelated previews.
All seven raw-pointer FFI helpers require unsafe callers and document their
storage/lifetime obligations; slice lengths above `isize::MAX` are rejected before
access. FFI modules deny implicit unsafe operations and preview modules forbid
unsafe code. Exported C ABI signatures and packet layouts remain unchanged.

| Check | Result |
|---|---|
| Host path regression tests, Release / Debug | 14 / 14 passed |
| Full Core Release suite | 390 passed |
| ParserHost / RasterHost / ShellBroker integration | 45 / 53 / 13 passed |
| Rust workspace tests | 351 passed, 1 ignored external-corpus test |
| Rust fmt, check all targets, Clippy all targets/features with warnings denied | Passed |
| Changed C# files format verification | Exit 0; workspace-load warnings reported |
| Release solution build, including native MSBuild contract | 0 warnings/errors; 4m37.05s |
| Full architecture sequence with `-SkipDist` | Passed after the correction below |
| Restricted-host launch / external native image corpus | Passed / 10 samples checked |
| System image corpus | Passed; decoded 4/5, JXL unavailable under optional-codec policy |

The first architecture run failed only at its duplicated `ffi::path_preview`
280-line budget after safety comments and explicit unsafe blocks grew the file.
Both copies of that budget were replaced by compiler contract requirements,
retaining the ABI/export, panic, cancellation and child-module boundary checks.
The complete rerun passed, including the separately invoked ignored corpus test.

At this scoped checkpoint, crate/root/Win32-wide unsafe-operation and unsafe-block
documentation enforcement remained `O26-02b`. Its subsequent evidence is recorded
in the crate-wide safety section below.
Packaging, product installation, remote CI/publication and physical HDR/performance
acceptance were not performed by this batch.

## ZIP release dependency slimming

On 2026-10-12, `O26-03` was verified in the same working tree based on `65843d0`.
ZIP's `aes-crypto` and `deflate-zopfli` features moved to dev-dependencies, retaining
fixture writers while production uses flate2 for DEFLATE and only inspects ZIP
encryption metadata. Cargo resolver v2 is unchanged. The locked normal/build
feature tree excludes both features and the `pbkdf2` and `zopfli` packages;
`Cargo.lock` is unchanged because the combined normal/dev graph still needs them.

Native Release was rebuilt through `QuickLook.Next.Native.proj` (1m58s). With
otherwise unchanged Rust sources and release profile, the DLL shrank from
8,749,568 to 8,683,008 bytes: 66,560 bytes (65 KiB, 0.76%). This is an output-size
comparison, not a startup/performance measurement or a packaged-payload proof.
The resulting SHA-256 is:

```text
D03B8C60CE07F5B09523203288F225341C841906F3E165727ADA685D21036D5B
```

The DLL staged beside the ParserHost integration tests has the same hash.
Three new tests use synthetic AES128/AES256/ZipCrypto ZIPs generated and validated
with 7-Zip 26.03, with no 7-Zip dependency at test runtime. They verify encrypted
listing metadata, refused extraction, zero-byte caller output, source/output
HANDLE release and no host extraction roots. Testing the actual Release DLL
avoids Cargo dev-feature unification masking a production-only regression.

Rust fmt and all-target/all-feature Clippy passed; workspace tests passed 351
with 1 ignored corpus test. Release solution build passed with 0 warnings/errors
in 14.72s; Core, ParserHost and ShellBroker passed 390/390, 48/48 and 13/13.
Changed C# format verification exited 0 with workspace-load warnings.
Full architecture checks with `-SkipDist` passed, including restricted-host
launch, 10 native corpus samples, and system corpus 4/5 under optional-codec policy.

The first full RasterHost test run passed 52/53: its repeat-preview HANDLE test
measured warmupPeak=478, measuredPeak=493, measuredLast=493, budget=12. Without
code changes, the focused rerun passed 1/1 and the complete RasterHost rerun
passed 53/53. The cause remains unconfirmed and is tracked under `E26-04`;
the initial failed full-solution run is not claimed as a clean pass.

## ParserHost runtime slimming

On 2026-10-12, `O26-04` was verified in the working tree based on `65843d0`, using
SDK `10.0.302`. ParserHost and its integration tests now target `net10.0-windows`:
their Win32/FFI transport does not need WinRT projections. Core Debug host lookup,
test staging, restricted-host smoke, required release executables and shared
payload enumeration use `net10.0-windows/win-x64`. App and RasterHost continue to
use their SDK-targeted frameworks. No new OS compatibility claim is made.

Only the two affected lockfiles changed: target keys and regenerated project
version ranges now match `1.0.1`; NuGet package versions are unchanged. A full
solution `restore --locked-mode` passed. Release build completed with warnings
treated as errors, 0 warnings/errors, in 12.37s, without platform suppressions.

The release helper's ParserHost entries were compared against the previous
non-PDB output, with the same native DLL from `O26-03`:

| ParserHost release files | Before | After |
|---|---|---|
| Files excluding PDBs | 9 | 7 |
| Total bytes | 34,611,674 | 9,204,508 |
| Microsoft.Windows.SDK.NET.dll | 24,877,600 bytes | Absent |
| WinRT.Runtime.dll | 528,944 bytes | Absent |

The reduction is 25,407,166 bytes (24.23 MiB). The built/staged dependency manifest
also has no Windows SDK/CsWinRT runtime dependency. A new integration test checks
those staged files and dependencies. The release-payload fixture leaves an old
SDK-framework executable/DLL beside the new output and verifies enumeration
selects the new executable while omitting the obsolete projection.

The complete Release solution test run passed: Core 390, ParserHost 49, RasterHost
53 and ShellBroker 13. The 14 Core host-path tests also passed in Debug. Changed
C# format verification exited 0 with workspace-load warnings. Release payload
proof/fail-fast tests and full architecture checks with `-SkipDist` passed,
including write-restricted ParserHost launch, 10 native corpus samples and system
corpus 4/5 under optional-codec policy. The earlier RasterHost HANDLE-budget
failure remains tracked under `E26-04`; this successful run does not establish
its cause or remediation.

This is build-output and payload-enumeration evidence. A new compressed package,
signing, installation, remote CI and physical HDR/performance acceptance were
not performed by this batch.

## Bounded reader property coverage

On 2026-10-12, the scoped `O26-05` suite was added in the working tree based on
`65843d06c1137993f6745fe789cb92811676f19f`. Eleven ordinary native property tests
cover ZIP listing/extraction, Torrent/bencode, CFB/MSG and SQLite/WAL readers.
Synthetic valid seeds and header-biased byte mutations, truncation, appended
bytes and arbitrary inputs compare full reads with 1..64-byte fragmented reads.
SQLite cases mutate the main file or WAL separately; some WAL mutations receive
fresh checksums so the checksum gate does not mask every deeper parse.
Positive properties check stored/DEFLATE ZIP payloads and output capacity,
torrent names/lengths, Unicode UTF-16 MSG subjects and committed WAL page-one
updates at 512/1024/4096-byte page sizes in both checksum byte orders.

Every input reader asserts 2 MiB of reads, 131,072 read calls and 4,096 seeks;
successful JSON is limited to 512 KiB. These test budgets apply to small generated
inputs and do not measure process heap or CPU-loop deadlines. Pre-cancellation
performs no reads/seeks; cancellation triggered by actual read 1, 2 or 3 stops
further underlying reads and leaves companion readers untouched. A subsequent
fresh preview must succeed. Declaring more input than is available returns the
typed `LengthMismatch` error. All inputs/outputs remain in memory; test logical
names are metadata, without file creation or path-based extraction.

The default suite uses 128 cases per property (1,408 total), fixed seed
`0x514C_2026_100C`, and bounded shrinking with persisted failing seeds. An expanded
run with `PROPTEST_CASES=2048` and `PROPTEST_RNG_SEED=20261012` passed all 11
properties (22,528 cases) in 14.37 seconds. The first development run failed to
compile because `prop_assert!` expanded implicit format captures inside its
message; moving expected strings into variables fixed that test-only error.
The earlier ten-property run also passed 20,480 expanded cases before the actual
read-cancellation property was added; final evidence is the eleven-property run.

`proptest 1.12.0` is dev-only with `std` and without fork/timeout features.
`Cargo.lock` adds five packages and upgrades none. The locked normal/build tree
excludes `proptest`, `core_detect`, `rand_xorshift`, `regex-syntax` and `unarray`;
`cargo audit` passed against 1,296 advisories and 184 locked packages. Native
Release was rebuilt through the solution's MSBuild contract: the DLL remains
8,683,008 bytes; its SHA-256 and the staged ParserHost test DLL hash both equal
`D31474AD4D5BA4FBF820D269A2CE9582E459A99CE817AE517979F1F2796EB679`.
This is size/graph evidence, not a byte-identical or reproducible-build claim.

Final Rust fmt/all-target-all-feature Clippy with warnings denied passed; the
native workspace passed 362 tests with 1 ignored external-corpus test. Release
solution build passed with 0 warnings/errors in 2m52.13s. Full .NET suites passed
Core 390, ParserHost 49, RasterHost 53 and ShellBroker 13. Full architecture
checks with `-SkipDist` passed, including restricted-host launch, the separately
invoked ignored native corpus test (10 samples), and system corpus 4/5 under
optional-codec policy (JXL unavailable). Whitespace checks passed. The historical
RasterHost HANDLE-budget failure remains under `E26-04`; this run does not
establish its cause or remediation.

Coverage-guided fuzzing, sanitizer execution and the other format/FFI targets
remain open in `R26-P1-10`. This batch does not establish physical HDR-panel,
end-to-end latency, packaged UI, installation or remote CI acceptance.

## Crate-wide unsafe and COM lifetime contracts

On 2026-10-12, `O26-02b` was implemented in the working tree based on
`65843d06c1137993f6745fe789cb92811676f19f`. The crate root now denies
`unsafe_op_in_unsafe_fn` and `clippy::undocumented_unsafe_blocks`; the existing
preview `forbid(unsafe_code)` remains. Root hooks/window queries, COM calls,
native HANDLE adoption, FFI adapters, packet writers and tests use explicit
unsafe blocks with local storage/lifetime/ownership explanations. Initial
diagnostic runs with the new lints deliberately failed on 57 implicit operations
and 170 undocumented blocks; these were repaired without unsafe lint allowances.
The existing 21 scoped ABI argument-count exceptions remain.

Legacy raster/animation raw-pointer writers now require unsafe Rust callers.
Shared and custom contracts specify live allocations, immutable inputs, output
exclusivity/disjointness, scalar alignment and callback lifetimes. Byte outputs
borrowed as mutable Rust slices must contain initialized bytes; current managed
arrays and fresh pagefile-backed sections meet that obligation. Retained callback
registration also requires an unsafe Rust caller, and the managed intent callback
contains parsing/event exceptions and copies its transient UTF-16 input. These
Rust safety qualifications do not change C ABI signatures or packet layouts.

The empty legacy byte writer returns before `copy_nonoverlapping`, so empty
JSON can safely use null output with zero or spare capacity. Its regression
checks both forms. A thread-affine, non-Send/non-Sync COM guard is created only
after successful S_OK/S_FALSE initialization and balances exactly one
CoUninitialize. Hook, selection and thumbnail workers stop on initialization
failure. Selection cache scopes borrow the apartment guard and release interfaces
before COM teardown, including unwinding exits. Real Windows tests cover nested
STA initialization and rejected STA initialization on an existing MTA; the latter
must preserve the original MTA until its own initialization is balanced.

The architecture lint-scope gate compiles the actual crate/preview lint attributes
and actual common/raster modules with the pinned Clippy driver. One documented
writer fixture must compile. Five probes must fail at the fixture's primary span
with the expected code: implicit unsafe operation, undocumented block, safe
JSON/raster writer calls and unsafe preview code. This is compiler contract
evidence, not a foreign-pointer validity or UB-freedom proof. Export documentation
checks inspect 60 raw-pointer exports plus retained callback registration.

The initial focused module-boundary run rejected the 281-line `ffi::common`
after documentation/test growth. Its tests moved into `ffi/common/tests.rs`;
the original 240-line implementation budget, narrow helper surface and regression
requirements remain. The full focused guard rerun passed without increasing a
line limit. No dependencies were added by this batch.

The first complete architecture run stopped at six root-export panic-boundary
regex mismatches: a new leading SAFETY comment appeared before `ffi_boundary`.
The root rule now skips leading line comments, as the existing path-preview rule
already did, while still requiring the panic boundary as the first executable
statement. That failed run did not reach later architecture/runtime checks.

Rust fmt, all-target/all-feature check and Clippy with warnings denied passed;
workspace tests passed 365 with 1 ignored corpus test. Release solution/native
MSBuild build passed with 0 warnings/errors in 3m02.63s. Full .NET Release suites
passed Core 390, ParserHost 49, RasterHost 53 and ShellBroker 13. Changed C#
format verification exited 0 with workspace-load warnings. A final documentation
clarification limits invalid-pointer use to documented paths returning before
access. Clippy and focused compiler/module/FFI guards passed again. Native Release
was rebuilt through MSBuild (Cargo reported 4m16s), followed by a 36.84s solution
build with 0 warnings/errors and the same four full passing .NET suites. The final native DLL is
8,684,544 bytes (1,536 bytes larger than the O26-05 output); its SHA-256 and the
staged ParserHost DLL SHA-256 both equal:

```text
0EF2FE00465AAD02D536D322E3CBE3B46CCF83983AFB303DDFD2FA497380BA26
```

The complete architecture sequence passed both after the regex correction and
again against the final rebuilt/staged DLL. It includes restricted-host launch,
the separately invoked ignored native corpus test (10 samples; final run 41.87s),
and system corpus 4/5 under optional-codec policy (JXL unavailable). Final
whitespace and changed PowerShell syntax checks passed. Local logs remain under
`artifacts/o26-02b-*`: the failed architecture log is retained alongside the
successful rerun and final-build/test/architecture logs.
The historical RasterHost HANDLE-budget failure remains under `E26-04`; the
successful integration run does not establish its cause or remediation. This
batch does not establish physical HDR-panel, packaged UI, latency, installation,
signing or remote CI acceptance. Broader guard/schema replacement remains `O26-06`.

## ABI schema and compiled export checks

On 2026-10-12, `O26-06a` was verified in the working tree based on `65843d0`.
This is the first scoped part of `O26-06`, not completion of the wider guard
migration. Schema v2 preserves native ABI version 3, all feature/status values,
C calling convention and packet layouts. It now inventories the pinned target,
68 project exports (ordered parameter/return types and Rust paths) and three
callback types. A closed type vocabulary prevents JSON from injecting Rust
source; names, ordering, uniqueness, integer values and bounded arrays are checked.

Generated function-pointer assertions compile with the production Rust crate,
not just tests. The two discovery getters are `const extern "C"` functions;
the compiler evaluates their exact version and full capability mask, and a
runtime test checks discovery and four non-overflow arithmetic probe cases.
No runtime allocation/work or new DLL exports are added by the assertions.
Both duplicated discovery export-count/signature/body-shape/140-line checks
were replaced, along with capability-body token scans. Explicit-import and
probe panic-containment rules, other module rules and all safety lints remain.
Safe Rust functions can coerce to unsafe function pointers, so compiler pointer
types do not replace the raw-pointer/retained-callback safety guard.

The actual native DLL has 19 incidental qcms 0.3.0 symbols in addition to the
68 project names. These are explicitly separated from supported QuickLook ABI
calls; the locked package/version and exact names are checked, with no wildcard
allowance. A bounded inspector uses the .NET PE reader and Microsoft's
[PE export-directory layout](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#export-directory-table).
It reads file-backed sections without loading/executing the DLL, limits the
file to 64 MiB, entries to 4096 and ASCII names to 256 bytes, rejects unknown or
missing names, forwarders, unnamed entries, ordinal aliases, duplicate names,
out-of-file/virtual-tail RVAs and non-executable targets. Export names alone do
not establish native types, managed marshalling, pointer validity or ownership.

| Check | Result |
|---|---|
| Schema generation/check fixtures | 1 positive; 22 expected malformed rejections |
| Generated compiler mutation fixtures | 1 positive; 9 expected rejections at generated assertions |
| Synthetic PE reader fixtures | 1 positive; 17 expected malformed rejections |
| Actual rebuilt win-x64 DLL export table | Exact 68 project + 19 dependency names; no load |
| Rust fmt, all-target/feature check and Clippy with warnings denied | Passed |
| Full Rust workspace suite | 366 passed, 1 ignored; 7.55s after compilation |
| Release solution/native MSBuild build | 0 warnings/errors; 2m34.20s; Cargo Release 1m55s |
| Full .NET Release suites | Core 390, ParserHost 49, RasterHost 53, ShellBroker 13 passed |
| Full architecture with `-SkipDist` | Passed; restricted-host launch and all compiler/schema/PE fixtures included |
| External native/system image corpora | 10 samples passed separately (21.70s); system 4/5, optional JXL unavailable |
| Changed PowerShell syntax / whitespace | Passed; existing lockfile CRLF normalization warnings only |

The first PE guard run rejected the 19 unmodeled dependency names. Inspection of
the locked qcms source confirmed upstream `no_mangle` symbols; the separate
inventory records the existing output rather than changing the dependency or
loosening unknown-symbol rejection. During fixture development, an initial
PowerShell default-argument line break failed parsing and an initial positive
compiler fixture failed with `E0603` because its nested fixture modules were
private. Both harness issues were corrected; subsequent focused and full gates
passed. These were not production native type/test failures.

The final native DLL remains 8,684,544 bytes. Its SHA-256 matches the DLL staged
beside ParserHost integration tests:

```text
1480EEB52E7EFDBCC987B55552A325406787D1242A2D568FDF156F5F92F0E930
```

Local build/test/architecture logs are retained under `artifacts/o26-06a-*`.
Subsequent managed P/Invoke/delegate verification is recorded below; wider
source-shape rule replacement remains `O26-06c`. Packet and runtime integration tests remain
independent evidence. The historical HANDLE-budget failure remains `E26-04`;
this run does not establish its cause or remediation. No signing, installation,
publication, packaged UI, remote CI or physical HDR/performance acceptance was
performed.

## Compiled managed ABI declarations and marshalling

On 2026-10-12, the scoped `O26-06b` implementation was verified in the working
tree based on `65843d0`. Schema v2 now lists 67 handwritten P/Invoke declarations
in seven owners across the four App/Host assemblies, eight unmanaged delegate
definitions and eight explicit raw callback parameter positions. Native ABI
version 3, exports, packet layouts and feature/status values are unchanged.
Schema validation rejects unknown/duplicate owners, imports, callbacks and raw
positions, requires all four production assemblies, and only accepts ordered,
bounded inventories tied to actual native export/callback types.

`tools/managed-native-abi.cs` reads the built win-x64 assemblies using PE and
metadata readers without loading target assemblies, resolving their dependencies
or executing native code. Files are bounded to 64 MiB, metadata to 20,000 types
and 200,000 methods, and inspected signatures to 32 parameters. The shared
validator checks exact owner/import inventory, private static fixed-arity Cdecl
imports, ExactSpelling, PreserveSig, library/entry names, parameter/return types,
width/signedness, byte-array In/In-Out and scalar Out directions, and one-byte
bool callback marshalling. Unknown declarations and optional parameters fail.

All 67 production declarations now specify ExactSpelling. Byte-array inputs
are explicitly In and writable byte-array outputs are In/Out, retaining
initialized contents if marshalling copies instead of pins. SafeFileHandle
remains accepted for HANDLE parameters; typed callbacks retain Cdecl/I1.
There is no change to call routing, cancellation, HANDLE adoption, packet limits
or runtime dependencies. The managed metadata check replaces the two duplicated
mail/PDF declaration regexes; the separate PDF bounded-routing rule and existing
panic, lifetime, ownership and resource checks remain.

Inert C# fixtures are compiled using the pinned SDK before inspection; imports
and callbacks are never invoked. Negative cases cover import policies/inventory,
variable arguments, width/signedness, array direction and marshalling, scalar
output slots, raw callback widths, callback calling convention, bool width,
string encoding and callback allocation signatures.

| Check | Result |
|---|---|
| Schema generation/check fixtures | 1 positive; 34 expected malformed rejections (12 new managed cases) |
| Generated Rust signature/discovery fixtures | 1 positive; 9 expected rejections |
| Managed compiler/metadata fixtures | 1 positive; 28 expected rejections after successful C# compilation |
| Final built managed metadata | 4 assemblies; 67 imports; 8 callback definitions passed; no target assembly load |
| Native export table / PE fixtures | Exact 68 project + 19 dependency names; 1 positive / 17 malformed rejections |
| Rust fmt, all-target/feature check and Clippy with warnings denied | Passed |
| Full Rust workspace suite | 366 passed, 1 ignored; 2.57s after compilation |
| Final Release solution/native MSBuild build | 0 warnings/errors; 1m00.51s; native output up to date |
| Full final .NET Release suites | Core 390, ParserHost 49, RasterHost 53, ShellBroker 13 passed |
| Changed C# format verification | Exit 0; workspace-load warnings reported |
| Full architecture with `-SkipDist` | Passed against final fixed-arity checks; restricted-host launch included |
| External native/system image corpora | 10 samples passed separately (48.81s); system 4/5, optional JXL unavailable |
| Changed PowerShell syntax / whitespace | Passed; existing lockfile CRLF normalization warnings only |

The initial metadata check rejected App's existing compiled declarations for
missing ExactSpelling/array direction and stopped there; it did not verify the
later assemblies. An initial fixture harness failed with CS2021 due to response
file argument construction; parentheses corrected the harness. Initial C# format
verification failed on introduced whitespace; targeted formatting and final
verification passed. The first full architecture run rejected the new inspector's
File.OpenRead call. Only that exact build-time tool was added to the API allowlist;
runtime preview path restrictions remain unchanged. A later variable-argument
fixture compiled but was incorrectly accepted, exposing a signature-header gap.
The validator now requires fixed signatures; the fixture is rejected. All these
initial failures are retained under `artifacts/o26-06b-*`, along with reruns.

Native DLL size remains 8,684,544 bytes; its SHA-256 is unchanged and matches the
DLL staged beside the final ParserHost integration tests:

```text
1480EEB52E7EFDBCC987B55552A325406787D1242A2D568FDF156F5F92F0E930
```

Metadata proves declaration compatibility, not actual raw callback addresses,
rooting/lifetime, pointer validity, aliasing or callback no-unwind behavior.
Existing packet and runtime integration tests remain independent evidence.
Wider guard migration stays open as `O26-06c`, and the parent `O26-06` is not
closed by this scope. Historical HANDLE-budget uncertainty remains `E26-04`.
No signing, installation, publication, packaged UI, remote CI or physical
HDR/performance acceptance was performed.

## Behavior-backed shared FFI helper checks

On 2026-10-13, the first three scoped `O26-06c` migrations were verified in the
same working tree. `tools/test-rust-ffi-behavior.ps1` compiles a bounded Windows
probe against the checked-in `ffi::common`, `ffi::raster_packet`,
`ffi::file_probe`, and `ffi::thumbnail` source, then runs 13 real behavior
cases. The cases cover
UTF-8 and optional null inputs, maximum and `isize` length rejection, owned
copies, empty/null output behavior, legacy and v2 required lengths/status codes,
initialized output canaries, exact BGRA headers/payloads, geometry overflow,
path-length and missing-file errors, JSON length negotiation, short-output
canaries, text classification, thumbnail flags/size/cancellation/error
mapping, exact thumbnail packets, and panic containment for both integer and
void boundaries. The Shell request provider is bounded and deterministic; no
native Release DLL is built or loaded by this fixture.

The same harness compiles 41 deliberate source mutations and requires each to
fail the behavior case it changes. Mutations cover length/capacity checks,
UTF-8/owned-copy/optional semantics, empty and short output statuses, payload
copying, required-size reporting, panic mapping, packet headers/payload,
geometry/status and null/short raster outputs, plus path-error, file-probe
capacity, payload, and written-length contracts, plus thumbnail status mapping
and packet-input mutations. This makes behavior, rather than test names or
implementation line counts, the contract for these adapters.
The module-boundary guard retains ownership/export-shape checks and the normal
Rust compiler safety lints. The old helper test-name/line-budget checks,
file-probe test-name check, and thumbnail pre-dispatch test-name check were
removed only after this fixture was added to `guard-architecture.ps1`; the
independent file-probe and thumbnail implementation bounds remain.

The first fixture compilation exposed twelve missing inline safety comments in
macro arguments; comments were moved immediately above the unsafe expressions
and the positive probe then passed. The final focused results were one positive
probe and 41 expected compiled mutation rejections; module-boundary checks also
passed. The final full architecture guard with `-SkipDist` also passed, including
the restricted-host launch and ten-image native corpus (system codec 4/5; JXL
optional and unavailable). Remaining format-family source-shape/test-name rules stay open as
`O26-06c`; this batch does not close the parent `O26-06`, `E26-04`, physical HDR,
packaged UI, installation, signing, publication or remote CI acceptance.

## Behavior-backed path-preview adapter checks

On 2026-10-14, `O26-06c.4` extends the same executable harness to the actual
`ffi::path_preview` module. It compiles without `cfg(test)` and includes the
actual crate safety lints, root string-limit constant and cancellation helper.
The separate `tools/fixtures/rust-path-preview-behavior.rs` supplies observable
Rust renderer substitutes: these are dispatch/cancellation probes, not proofs
of parser limits, file access, HANDLE ownership or foreign-pointer validity.
The four existing path-preview unit tests against real providers are unchanged.

All twelve exports, including four legacy forwarding wrappers, run through
five new behavior groups. They check null/invalid UTF-8/oversized path rejection
without renderer access, exact-limit and empty UTF-8 acceptance, multibyte paths,
exact renderer and metadata forwarding, negative required-size negotiation,
short/exact/spare output canaries, empty output, optional kind fallback,
pre-render cancellation without provider calls, post-render cancellation without
copying, delegated archive/Office callbacks, provider panic containment and a
fresh successful call after cancellation or panic. Oversized dangling-pointer
probes retain the real slice-bound checks; short-capacity probes have spare
initialized storage. Generated input files now live in the harness-owned
temporary directory and are cleaned up even on a failing mutation run.

The expanded gate accepts one actual-source probe with 18 behavior groups and
rejects 93 deliberate mutations, including 52 new per-export path mutations.
Each mutant must compile with warnings denied before it may fail the expected
behavior group. Initial extension attempts exposed a missing callback import,
inline SAFETY-comment placement, behavior-group ordering, and mutation-only
Clippy/unused-import failures. These were corrected without adding lint
allowances or changing the native implementation. The two duplicated sets of
four test-name requirements and the two 160-line child-test budgets were removed
only after the compiled behavior checks passed. Test-module presence, export
ownership/shape and first-statement panic-boundary rules remain independent.

Focused behavior/module-boundary checks, PowerShell syntax validation, Rust
workspace tests (364 passed, one ignored external corpus), all-target/
all-feature Clippy and workspace/fixture formatting passed. The first complete
architecture `-SkipDist` run passed, including restricted-host launch, ten-image
native corpus and system corpus (4/5; optional JXL unavailable). Final artifact
verification then found the win-x64 native Release DLL at `96FA9C35...` while
ParserHost still staged `1480EEB5...`. This was an output-snapshot mismatch,
not evidence that ABI signatures or the new behavior probes had failed.

A pinned SDK Release solution build through the existing native MSBuild
contract passed with zero warnings/errors in 23.06 seconds. The native DLL
and all four App/Host staged copies now match:

```text
96FA9C3564586396E70470D63E2EA62E8EB7317A5E5B62B5384D4D1B8BD27F80
```

Final .NET Release tests passed Core 390, ParserHost 49, RasterHost 53 and
ShellBroker 13 against the synchronized outputs. The complete architecture
`-SkipDist` rerun also passed, including 18 FFI behavior groups/93 compiled
mutation rejections, restricted-host launch, ten native images and system
corpus (4/5; optional JXL unavailable). Final syntax/format/diff checks passed;
`git diff --check` retained only the pre-existing two lockfile CRLF notices.
Local logs and before/after native hash inventories are retained under
`artifacts/o26-06c4-20261014-1791959138171`. Parent `O26-06c`/`O26-06`,
`E26-04`, optional JXL, physical HDR/performance, packaged UI and remote CI
acceptance remain open. No signing, installation or publication is authorized
or performed by this batch.

## Behavior-backed HANDLE preview adapter checks

On 2026-10-18, `O26-06c.5` adds
`tools/test-rust-handle-preview-behavior.ps1`. The fixture compiles the actual
`ffi::handle_preview`, `native_input`, common output adapter and bounded file
probe with the crate's safety lints, logical-name limit, cancellation helper and
typed `ReaderPreviewError` declaration. Cargo's locked test build supplies the
exact Windows dependency rlib structurally; no Release DLL is built or loaded.

Seven executable groups cover real Win32 `ReOpenFile` ownership and independent
positions, source-handle preservation after rename/decoy replacement, output
size negotiation and initialized canaries, null/UTF-8/length/name validation,
invalid/non-disk/exclusive handles, all native and reader status mappings,
cancellation before reopen/after reopen/after reader, panic containment and
process-handle cleanup. PDF/image provider substitutes observe dispatch and
typed errors; they do not replace format-parser tests. A callback that cancels
on its second poll proves the outer `preview_handle_v2` check is distinct from
the inner reopen check.

The positive probe passes all seven groups. Twenty-six deliberate source
mutations compile with `-D warnings` and are each rejected by their expected
behavior group, including output/status, logical-name forwarding, native/reader
mapping, callback forwarding, cancellation and the deliberate reopened-handle
leak. The focused module-boundary check also passes after removing only the
duplicated HANDLE test-name and 300-line-budget rules; export ownership,
first-statement panic boundaries and security/ownership source invariants stay
independent. At this seven-group stage the full architecture gate was the next
step; the expanded verification below records the completed gate. Parent
`O26-06c`/`O26-06`, `E26-04`, physical HDR/performance, optional JXL,
packaged UI and remote CI acceptance remain open.

## Behavior-backed container HANDLE adapter checks

On 2026-10-18, `O26-06c.6` extends the HANDLE fixture in the working tree based
on `80e3562`. The existing script now also compiles the actual
`ffi::handle_container` module with all four archive/package/Office/ebook C
exports, the real common/HANDLE boundary and actual Win32 reopen implementation.
The provider substitutes accept owned `File` readers, read their bytes from
position zero, and record renderer, basename, source length, modification time
(where applicable) and the actual cancellation callback. They do not parse
formats or establish parser resource limits; the real-provider Rust tests and
the existing container pre-cancellation unit test are unchanged.

Two new groups check null/zero size queries, short/exact/spare output canaries,
UTF-8 output, exact provider/metadata/callback forwarding, all five typed reader
failures, empty output, pre-cancelled null input/invalid HANDLE rejection before
I/O, cancellation after reopen and after rendering, provider panic containment
and a fresh successful call. Every failure/cancellation path preserves output,
the caller's source position and ownership, with reopened-handle cleanup checked
against a process-handle baseline in the single-threaded probe.

The expanded fixture passes nine groups and rejects 49 warning-free compiled
mutations: the previous 26 and 23 new per-export mutations of logical name,
source length, provider callback, modification time, boundary callback and
typed-reader mapping. The source-mutation anchors initially matched closure
bindings as well as arguments; they now target the complete provider call. A
stale seven-group success marker was also corrected; the probe now derives its
marker from the actual group array. Neither issue required a
production change or relaxed lints. Only after compiled behavior evidence passed
were the two duplicated pre-cancellation-test-name and 260-line-budget rules,
plus two duplicated sets of per-export reader-status-call-shape checks, removed
from the guards.
Export/module ownership, shared-helper use and panic/safety rules remain.

Focused HANDLE/module-boundary checks, PowerShell parsing, fixture/workspace
formatting, all-target/all-feature Clippy and Rust workspace tests pass (364
passed, one ignored external corpus). Prebuilt Release managed regressions also
pass Core 390, ParserHost 49, RasterHost 53 and ShellBroker 13. The native DLL
and all four staged App/Host copies still match `96FA9C35...`; no rebuild was
needed for a fixture/guard-only change. The final architecture `-SkipDist` gate
also passes, including both FFI fixtures (142 compiled mutation rejections),
restricted-host launch, native image corpus 10/10 and system corpus 4/5 (optional
JXL unavailable). Logs and native/staged DLL hash inventory are retained under
`artifacts/o26-06c6-20261018-e0b3b747`. No product implementation,
ABI, dependencies or release outputs have been changed by this batch.
Parent `O26-06c`/`O26-06`, `E26-04`, physical HDR/performance, optional JXL,
packaged UI and remote CI acceptance remain open. No signing, installation,
publication or remote changes were performed.

## Behavior-backed structured HANDLE adapter checks

On 2026-10-18, `O26-06c.7` extends the existing HANDLE fixture in the working
tree based on `80e3562`, retaining the uncommitted container migration above.
It compiles the actual `ffi::handle_structured` module and all four
text/executable/torrent/mail C exports with the real common/HANDLE boundary,
native input and Win32 reopen implementation. Observable providers accept the
actual owned or borrowed `File`, read its pinned bytes from position zero and
record renderer, basename, applicable length/mtime and callback. Shared
container/structured fixture helpers avoid duplicating the output and cleanup
assertions. Providers are substitutes, not format parsers; real-parser tests
and the existing structured pre-cancellation unit test are unchanged.

Three new groups cover dispatch and size negotiation with short/exact/spare
output canaries, the five typed reader errors for executable/torrent/mail,
empty output, pre-cancelled null input/invalid HANDLE before I/O, cancellation
before/after reopening and after rendering, provider panics and fresh recovery.
Text's empty `String` maps to I/O failure unless cancelled. A callback true only
on the fourth poll exercises that inner branch: the fifth, shared post-render
poll is false, so it cannot mask an incorrect inner error. Failure paths retain
the output canaries and caller's open source at position three; process HANDLE
counts return to baseline after each call.

The focused fixture passes 12 groups and rejects 76 warning-free compiled
mutations (27 new; 169 across the shared/path and HANDLE fixtures). New cases
change provider name/length/mtime/callback, boundary callback, typed-error
mapping, executable/torrent dispatch and text-empty status/cancellation.
Initially, the wrong-renderer mutants failed compilation because their original
provider imports became unused. The mutants now keep those imports observably
used and must compile before failing the dispatch behavior group; no lint was
relaxed. After the complete mutation run passed, only the structured
pre-cancellation-test-name check, two duplicated 260-line budgets and two
duplicated mail-forwarding regexes were removed. Export/module ownership,
shared-helper use and panic/safety rules remain guarded.

Workspace and fixture formatting, all-target/all-feature Clippy and Rust
workspace tests pass (364 passed, one ignored external corpus). Prebuilt
Release regressions pass Core 390, ParserHost 49, RasterHost 53 and ShellBroker
13. All five native/staged DLL hashes still match `96FA9C35...`; this batch
changes no product implementation, ABI, dependencies or Release outputs.
PowerShell syntax, focused module-boundary/architecture-guidance checks and
the final full architecture `-SkipDist` run pass, including both FFI fixtures,
restricted-host launch, native image corpus 10/10 and system corpus 4/5
(optional JXL unavailable). Distribution/package and manual UI/HDR acceptance
are not established by this test-only batch.
Logs and the DLL hash inventory are under `artifacts/o26-06c7-20261018`.
Parent `O26-06c`/`O26-06`, `E26-04`, physical HDR/performance, optional JXL,
packaged UI and remote CI acceptance remain open. No signing, installation,
publication or remote changes were performed.

The container and structured HANDLE fixture/guard changes were subsequently
committed together as `2fb1c95`. The results above describe verification of the
same source before the commit; no additional build or remote run is implied.

## Behavior-backed routing adapter checks

On 2026-10-18, `O26-06c.8` extends the shared/path FFI probe in the working
tree based on `a19906d`. It compiles the actual `ffi::routing` module and its
three C exports with the real common argument/output and panic helpers. The
new `tools/fixtures/rust-routing-behavior.rs` uses observable classifier and
folder providers, not real format detection or directory I/O. Real routing,
folder, text and archive provider tests remain in the Rust workspace gate.
Both input limits are extracted from the actual crate root. The success marker
now derives its group count from the actual fixture array.

Four groups cover exact Unicode extension, embedded-NUL kind and binary magic
forwarding, true/false integer results, optional UTF-8 fallback and magic
rejection. They exercise null/zero versus null/positive lengths, malformed
UTF-8, non-null empty values, exact/over-limit arguments and lengths above
`isize::MAX`. Oversized probes never remove the real isize/pointer checks.
Folder cases cover invalid inputs before provider access, null/zero output,
short/exact/spare byte capacities, UTF-8 payload/canaries, exact callback
identity, provider-delegated cancellation, empty output, contained provider
panics and fresh calls. Drop counters check provider cleanup during unwinding.
These adapter checks do not establish parser or directory traversal limits.

The expanded fixture passes 22 groups and rejects 120 warning-free compiled
mutation rejections (27 new; 196 across both FFI fixtures). New cases change
classifier arguments/results/fallbacks/limits and folder path/renderer/callback,
invalid-input status, zero capacity, payload and output capacity. Initially,
the false-result mutant changed zero to one, making both branches identical;
Clippy rejected it before execution. It now returns the deliberately wrong
value two, so compilation must pass before the behavior assertion fails.
No lint allowance was added. The initial diagnostic is retained as
`artifacts/o26-06c8-20261018/ffi-behavior-initial.log`.
The next run rejected an ambiguous false-result mutation anchor: the last
export's source slice also contained its unit tests, which had another
standalone zero. The anchor now selects the complete `else` branch without
relaxing its exact count. All 27 scoped routing anchors were checked
independently; the diagnostic is retained in `ffi-behavior-anchor.log`.
Only after the complete positive/mutation run passed was the routing module's
150-line budget removed. Its three-export inventory, explicit imports,
root/module ownership and first-statement panic rules remain independently
guarded; the three routing unit tests were not removed.

Workspace/fixture formatting, all-target/all-feature Clippy and Rust tests
pass (364 passed, one ignored external corpus). Prebuilt Release regressions
pass Core 390, ParserHost 49, RasterHost 53 and ShellBroker 13. All five
native/staged DLL hashes match `96FA9C35...`; no Release rebuild, product/ABI
or dependency change is part of this batch. Evidence is retained under
`artifacts/o26-06c8-20261018`. Implementation/fixture commit: `fe813c4`;
verification used the same changed source before that commit.
PowerShell syntax, scoped anchors, focused module-boundary/guidance checks and
the final full architecture `-SkipDist` run pass, including 22 shared/path/
routing groups with 120 compiled mutations, 12 HANDLE groups with 76 mutations,
restricted-host launch, native image corpus 10/10 and system corpus 4/5
(optional JXL unavailable). Distribution/package and manual UI/HDR acceptance
are not established by this test-only batch.
Parent `O26-06c`/`O26-06`, `E26-04`, physical HDR/performance, optional JXL,
packaged UI and remote CI acceptance remain open. No signing, installation,
publication or remote changes are authorized by this verification-only work.

## Behavior-backed database adapter checks

On 2026-10-18, `O26-06c.9` extends the HANDLE probe in the working tree based
on `6ac5034`. It compiles both actual `ffi::database` C exports, the private
optional-HANDLE helper, common/shared HANDLE boundary and real Win32 reopen
implementation. The new `tools/fixtures/rust-database-behavior.rs` supplies
observable legacy-path and reader providers; these do not implement or verify
SQLite/WAL parsing. The real native database/provider tests remain unchanged.
The path limit, all three database byte limits and `DatabaseCompanionReader`
declaration are extracted from their production sources rather than copied
with smaller fixture limits.

Six new groups cover exact Unicode path/metadata/callback forwarding, invalid
UTF-8 and exact/over-limit arguments, zero/empty results, legacy signed byte
capacity negotiation, and pre/post-render cancellation returning zero. The
SQLite cases exercise all nine absent/nonempty/present-empty WAL/SHM
combinations, distinct main/WAL/SHM bytes after rename/decoy replacement,
independent positions, basename/length/mtime/callback forwarding, required-size
negotiation and output canaries. Invalid/non-disk HANDLEs, sharing-denied
reopens, length mismatch, invalid tuple precedence, cleanup after a later
companion fails, all five reader statuses, cancellation precedence, contained
provider panics and fresh recovery are executable contracts. Caller positions
and process HANDLE counts must remain unchanged.

Real exact-limit/one-byte-over file lengths exercise the 256 MiB main, 64 MiB
WAL and 4 MiB SHM limits. This is necessary because the shared boundary checks
main length against the actual object before entering database limit logic.
Each provider reads at most a 32-byte prefix; a separate fixture ceiling
prevents future production-limit increases from silently growing temporary
files beyond 256 MiB plus the one-byte over-limit case.

The expanded fixture passes 18 groups and rejects 112 warning-free compiled
mutations (36 new; 232 total across both current FFI fixtures). New mutations
cover argument/callback/status/capacity changes, absent-versus-empty handling,
wrong/swapped companion readers, a deliberate reopened-WAL leak on a later
failure, tuple-versus-limit precedence and both disabled/inclusive byte limits.
The first compile exposed a missing `Read` import in the wrapper's extracted
companion descriptor; the import was added without relaxing safety lints.
The diagnostic remains in `handle-behavior-initial.log`. Mutants retain their
real pointer-safety documentation when relocated, and helper mutations now
select the private function explicitly. Only after the complete positive and
mutation run passed were the duplicated database test-name, 220-line-budget,
reader-forwarding and equivalent constant-presence checks removed. Export
inventory, explicit imports, module/private-helper ownership, shared HANDLE
boundary and panic policies remain; native SQLite/WAL tests were not removed.

Workspace/fixture formatting, all-target/all-feature Clippy and Rust tests pass
(364 passed, one ignored external corpus). Prebuilt Release regressions pass
Core 390, ParserHost 49, RasterHost 53 and ShellBroker 13. The focused
module-boundary/guidance checks, changed PowerShell syntax and whitespace
checks pass. All five native/staged DLL hashes still match `96FA9C35...`;
this test/guard batch has no product, ABI or dependency change and no Release
rebuild. Logs and hashes are under `artifacts/o26-06c9-20261018`.
The final full architecture `-SkipDist` run passes, including 22 shared/path/
routing groups with 120 compiled mutation rejections, 18 HANDLE groups with
112 compiled mutation rejections, ABI/schema/metadata checks, restricted-host
launch, native corpus 10/10 and system corpus 4/5 (optional JXL unavailable).
Distribution/package and manual UI/HDR acceptance are not established.
Implementation/fixture commit: `c4429d7`; all results above verify the same
changed source before that commit, without implying a new build or remote run.

Parent `O26-06c`/`O26-06`, `E26-04`, physical HDR/performance, optional JXL,
packaged UI and remote CI acceptance remain open. No signing, installation,
publication or remote changes are part of this batch. The next scoped fixture
is the seven path/HANDLE Hero image exports (`O26-06c.10`).

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

## HDR follow-up evidence

On 2026-09-29, after the HDR display/source policy, PQ conversion, refresh
generation, and diagnostics updates, the pinned SDK produced a Release solution
build with 0 warnings and 0 errors. Core tests passed 375/375 and RasterHost
integration tests passed 38/38 when run in their focused/isolated modes.
dotnet format --verify-no-changes and git diff --check passed.

The full RasterHost integration run was non-deterministic in this desktop
session: two runs reported 37/38 because the SVG shared-surface test received
the generic image-preview failure, while the same test passed in isolation.
This is retained as an environment/test-harness flake, not claimed as a clean
full-suite pass.

The architecture guard reached the restricted-host launch stage and then
stopped with CreateRestrictedToken Win32 error 87. All guards before that
stage passed; no change was made to the restricted-token security boundary.
No physical HDR panel, licensed PQ/HLG corpus, or visual golden result was
available, so hardware HDR acceptance remains open.

## Performance baseline collection

On 2026-09-30, `tools/measure-build-baseline.ps1 -Iterations 2` collected a
separate Release build matrix with pinned SDK `10.0.302`. The clean-build
median was **22.273 seconds** (15.274–29.272), the incremental median was
**13.046 seconds** (12.875–13.217), and the no-change median was **11.648
seconds** (11.524–11.773). The raw command logs and SHA-256 values are retained
under the ignored `artifacts/build-baseline-*` directory and summarized in
`artifacts/build-baseline.json`. These are local observations, not budgets;
restore, package creation, and cold caches were intentionally excluded.

After exercising a fixed mixed-format preview corpus, run:

    pwsh -NoProfile -File tools\test-measure-preview-baseline.ps1
    pwsh -NoProfile -File tools\measure-preview-baseline.ps1 -MinimumSamples 20 -BuildBaseline artifacts\build-baseline.json -RasterLog "$env:LOCALAPPDATA\QuickLookNext\logs\raster-host.log" -OutputPath artifacts\preview-baseline.json

The extractor reports first-paint (including navigation-source breakdown),
terminal outcomes, ParserHost resource samples, and RasterHost image rows. It
reports local lifecycle request-to-terminal marks separately from true host
cancellation/drain latency;
the latter remains null until an explicit host completion mark exists. RasterHost
resource sampling, cold/warm corpus rows, and cancellation-drain rows remain
open. `timingSource=none` and `na` stage fields mean the system decoder did not
emit native stage timings; they are excluded from timing percentiles while output
byte samples remain counted.

On 2026-10-07, seven fixed-log regression scenarios passed under PowerShell
7.6.6 and Windows PowerShell 5.1. They cover measured zero, missing timings,
percentile interpolation, local cancellation marks, source groups, resource
statistics, output-file/build inputs, malformed rows, and legacy logs. These
fixtures validate extraction; they are not measured desktop performance results.
`sufficientSamples` counts measured native rows only. Legacy rows have ambiguous
decoder origin and are counted separately; invalid rows are excluded. Output
bytes describe the BGRA payload, not total retained memory or HDR FP16 bytes.

The pinned SDK `10.0.302` Release solution build passed with 0 warnings and
0 errors (72.32 seconds). The Rust module-boundary guard and whitespace check
passed. Core and RasterHost test runs aborted before executing tests because
VSTest could not connect to its testhost within 90 seconds. Format verification
failed to connect to the MSBuild host pipe with `UnauthorizedAccessException`.
An outside-sandbox retry was not executed: automatic approval review returned
503 (authentication service unavailable). The new C# diagnostics tests are
compiled but their runtime result was unverified at that point.

After updating the direct-packet and animation-default-value guard expressions
and adding the missing zh-TW animation status resource, the complete
`guard-architecture.ps1 -SkipDist` sequence passed. This includes the extractor
regressions, localization (3 locales, 481 keys), restricted-host launch, native
external image corpus (10 samples), and system image corpus (4/5 decoded;
optional JXL codec unavailable). The App Release rebuild after the resource
change also passed with 0 warnings/errors. Distribution inspection was skipped;
these checks do not replace the aborted .NET test runs or format verification.

With unrestricted local execution later on 2026-10-07, Core passed 376/376 and
RasterHost passed 43/43, including the new diagnostics/packet checks. Format
verification exited 0 with workspace-load warnings. These successful retries
resolve the earlier testhost/MSBuild-pipe blockage.

## Animation boundary follow-up

On 2026-10-07, GIF truncation detection switched to `gif::Decoder::next_frame_info`
so an extra frame's pixels are not decoded solely to detect the output limit.
Generic WebP/APNG lookahead now propagates decoder errors and checks cancellation
after advancing, including the end-of-stream case. New tests cover 119/120/121
frames, decoder failure immediately beyond the cap, and cancellation during
lookahead; the GIF boundary regression also exercises all three frame counts.
Rust workspace tests passed 349 with 1 ignored external-corpus test; Clippy,
Rust formatting, module-boundary and performance-bounds guards passed.
The pinned-SDK Release solution build, including native DLL rebuild through
MSBuild, passed with 0 warnings/errors in 141.61 seconds.
This avoids GIF lookahead pixel work; no numeric cancellation p95 is claimed.
The post-rebuild solution test run passed Core 376, ParserHost 45, and
ShellBroker 13. RasterHost passed 42/43 with the previously observed SVG
shared-surface failure; the SVG test then passed 1/1 in isolation. The earlier
43/43 RasterHost run preceded this native rebuild, so a clean post-rebuild
full-suite result is not inferred from it.
Post-change `guard-architecture.ps1 -SkipDist` also passed, including all ten
native external corpus samples and the system corpus under optional-codec
policy (4/5 decoded, JXL unavailable).

## Input responsiveness follow-up

### Mixed image recovery regression

On 2026-10-07, `Mixed_format_reopens_release_sources_and_ignore_previous_request_close`
passed in four independent focused runs. Each run keeps one RasterHost alive for
three rounds of PNG/ICO/JPEG/BMP previews (12 total). It verifies request IDs on
surface/ready/waveform messages, duplicates and releases each surface HANDLE,
requires source-write denial while active and write recovery after close, and
sends the previous request's close immediately after the new open. Logical paths
do not exist, and the host must exit cleanly. The Release test-project build
passed with 0 warnings/errors; changed-file format verification exited 0 with
workspace-load warnings. This covers completed static-image close/reopen; it
does not measure close-before-first-frame worker-drain latency or memory p95.

The regression subsequently gained an immediate-close mode: for every format,
send open/close/reopen without consuming the canceled preview's output. The new
request gets an independent receiver-owned HANDLE to the same pinned file.
Already-queued output from the canceled request is drained, including surface
transfer release, and cannot satisfy the replacement's completion. Both modes
passed four focused runs (8 cases). Each immediate-close run exercises 12
canceled opens plus 12 replacement previews and requires final source-write
recovery and clean process exit. This is a scheduling-race regression, not proof
that the host processed close before producing a surface; deterministic
mid-decode cancellation and worker-drain latency measurement remain open.

### Deterministic reader cancellation

On 2026-10-07, `image_decoders_cancel_during_actual_reads_and_allow_fresh_decode`
passed. Its bounded test reader returns up to eight bytes per read and triggers
cancellation after successful read 1 or 3. PNG, GIF, and APNG decoder calls must
return no decoded result and perform no further underlying reads. Each case
then resets cancellation and successfully decodes the same valid byte stream.
This exercises actual reader work rather than guessing callback poll counts or
racing a wall-clock delay. The Rust workspace passed 350 tests with 1 ignored;
Clippy, formatting, module-boundary and whitespace checks passed. Only test code
changed; no runtime cancellation hook was introduced. Codec CPU-loop/OS decode
cancellation and end-to-end host-drain p95 remain unmeasured.

On 2026-10-07, this actual-read regression was extended to the checked-in animated
WebP fixture. Cancellation at read 1 and read 3 prevents further reads, and each
fresh decode succeeds. The Rust workspace passed 350 tests with 1 ignored;
Clippy and formatting passed.

Windows image lookup/open, decoder creation, and pixel extraction now forward
the token through WinRT `AsTask(cancellationToken)` in the SDR and HDR routes.
Post-await token checks and scoped streams remain. Release solution build passed
with 0 warnings/errors. `SystemImageCancellationTests` passed both SDR/HDR cases:
pre-cancellation propagates, the caller HANDLE remains valid and prevents source
writes, fresh HANDLE decode succeeds with a nonexistent logical path, and caller
disposal restores write access. This is pre-cancellation/ownership coverage;
deterministic in-flight Windows codec interruption and worker-drain latency are
still open. No timing sleeps or runtime test hooks were added.

The full Release solution test run subsequently passed: Core 376/376,
ParserHost 45/45, RasterHost 47/47, and ShellBroker 13/13. The prior intermittent
SVG integration failure did not occur in this run.
Changed-file C# format verification exited 0 with workspace-load warnings;
the full architecture guard with `-SkipDist` passed, including ten native
corpus samples and the Windows system-codec smoke (4/5 decoded, optional JXL
codec unavailable under the existing policy). Whitespace checks passed.

### Static/animation close and reopen transitions

On 2026-10-07, `Animation_and_static_reopens_release_parent_and_child_resources`
passed all six GIF/WebP/APNG and child-first/parent-close cases. Each case uses
one RasterHost process for static PNG, animation, static PNG, animation, static
PNG. A physical `.bin` source is reused after write recovery; logical paths do
not exist. This also covers APNG/static PNG with the same extension. Every
preview requires the matching surface/ready and expected waveform before closing,
so no partial pipe receive is canceled between requests. Surfaces are duplicated
and their transfers released. Old parent/child close messages are sent after new
opens, including after a new animation child open.

Animation section request IDs, dimensions, count/truncation header and packet
length are checked. Child-first close preserves the parent's source lock; parent
close releases both the source and remote section. Remote section closure is
checked before another open can reuse its numeric HANDLE. The client's mapped
header and first-frame bytes remain identical after remote closure. Host shutdown
must be clean. The Release test-project build passed with 0 warnings/errors;
the full RasterHost suite passed 53/53. Changed-file format verification exited
0 with workspace-load warnings, and whitespace checks passed. No runtime code
changed. These are completed-decode handoffs, not deterministic mid-animation
cancellation, GPU-compositor UX validation, or measured cancellation p95.

On 2026-09-30, Explorer selection COM reads were moved off the Rust hook message
pump to a dedicated STA worker. The worker accepts at most one queued request;
the hook timer/generation checks and App switch debounce continue to reject stale
or superseded navigation. Rust formatting and the native workspace tests passed
(346 passed, 1 ignored). Close-generation checks also drop stale worker requests;
this does not claim a measured p95 until the
mixed input corpus is exercised.

The same change set also removed the temporary Rust `Vec` for static HANDLE
image packets: the packet header and BGRA payload are now written directly to
the caller-provided output buffer. Release build completed with 0 warnings and
0 errors, and the RasterHost integration suite passed 38/38 after the change.
No visual/color/orientation contract was changed; a copy-specific allocation
profile is still open.

Animation packets now also carry an explicit truncation sideband in the existing
frame-count word. Legacy packets remain valid, and RasterHost/App reject any
metadata mismatch before mapping frames. The native test suite covers the flag;
long-animation corpus coverage now includes a native 121-frame regression (346
native tests passed); a host-level corpus run is still required to exercise the
user-visible status path end to end.
