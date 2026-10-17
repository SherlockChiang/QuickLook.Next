param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Root = [IO.Path]::GetFullPath($Root)
$sourceRoot = Join-Path $Root 'native/quicklook_next_native/src'
$fixturePath = Join-Path $PSScriptRoot 'fixtures/rust-handle-preview-behavior.rs'
$handlePath = Join-Path $sourceRoot 'ffi/handle_preview.rs'
$inputPath = Join-Path $sourceRoot 'native_input.rs'
$handleText = (Get-Content -LiteralPath $handlePath -Raw).Replace("`r`n", "`n")
$inputText = (Get-Content -LiteralPath $inputPath -Raw).Replace("`r`n", "`n")
$libText = Get-Content -LiteralPath (Join-Path $sourceRoot 'lib.rs') -Raw
$crateLints = @([regex]::Matches($libText, '(?m)^#!\[(?:deny|forbid)\([^\r\n]*\)\]') | ForEach-Object Value) -join "`n"
$logicalLimit = [regex]::Match($libText, '(?m)^const MAX_LOGICAL_NAME_BYTES:[^\r\n]*;').Value
$cancelHelper = [regex]::Match($libText, '(?ms)^fn cancel_requested\([^\r\n]*\) -> bool \{\r?\n.*?^\}').Value
$typesText = Get-Content -LiteralPath (Join-Path $sourceRoot 'preview/types.rs') -Raw
$readerError = [regex]::Match($typesText, '(?ms)^#\[derive\([^\r\n]*\)\]\r?\npub\(crate\) enum ReaderPreviewError \{.*?^\}').Value
if ($crateLints -notmatch 'unsafe_op_in_unsafe_fn' -or $crateLints -notmatch 'clippy::undocumented_unsafe_blocks' -or
    -not $logicalLimit -or -not $cancelHelper -or -not $readerError) { throw 'Actual HANDLE safety/type contracts are missing.' }

function Edit-HandleFixture([string]$Text, $Case) {
    $prefix = ''
    $suffix = ''
    if ($Case.Scope) {
        $starts = [regex]::Matches($Text, '(?m)^pub(?:\(crate\))? (?:unsafe )?(?:extern "C" )?fn (?<name>[a-z0-9_]+)\(')
        $selected = @($starts | Where-Object { $_.Groups['name'].Value -ceq $Case.Scope })
        if ($selected.Count -ne 1) { throw "Mutation scope drifted: $($Case.Name)" }
        $start = $selected[0].Index
        $next = @($starts | Where-Object Index -GT $start | Select-Object -First 1)
        $end = if ($next.Count) { $next[0].Index } else { $Text.Length }
        $prefix = $Text.Substring(0, $start)
        $suffix = $Text.Substring($end)
        $Text = $Text.Substring($start, $end - $start)
    }
    $anchors = [regex]::Matches($Text, [regex]::Escape($Case.Before))
    if ($anchors.Count -ne $Case.Count) { throw "Mutation anchor drifted ($($Case.Name)): expected $($Case.Count), got $($anchors.Count)." }
    if ($Case.ContainsKey('Occurrence')) {
        if ($Case.Occurrence -lt 0 -or $Case.Occurrence -ge $anchors.Count) { throw 'Invalid mutation occurrence.' }
        $index = $anchors[$Case.Occurrence].Index
        $Text = $Text.Substring(0, $index) + $Case.After + $Text.Substring($index + $Case.Before.Length)
    }
    else { $Text = $Text.Replace($Case.Before, $Case.After) }
    return $prefix + $Text + $suffix
}

# All pointer checks and owning-handle constructors remain valid in mutations.
# The deliberate forget leaks only a reopened child-process handle until process exit.
$cases = @(
    @{ Name = 'actual'; Expected = $null },
    @{ Name = 'null-required-status'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT + 1;'; Count = 2; Occurrence = 0 },
    @{ Name = 'null-output-status'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT + 1;'; Count = 2; Occurrence = 1 },
    @{ Name = 'required-reset'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = '*out_required = 0'; After = '*out_required = 1'; Count = 1 },
    @{ Name = 'null-size-probe'; Expected = 'handle_outputs'; Scope = 'preview_handle_v2'; Before = 'out_buf.is_null() && out_cap != 0'; After = 'out_buf.is_null()'; Count = 1 },
    @{ Name = 'logical-name-limit'; Expected = 'handle_arguments'; Scope = 'reopen_handle_input_v2'; Before = 'logical_name_len, MAX_LOGICAL_NAME_BYTES'; After = 'logical_name_len, MAX_LOGICAL_NAME_BYTES - 1'; Count = 1 },
    @{ Name = 'logical-basename'; Expected = 'native_ownership'; Scope = 'reopen_handle_input_v2'; Before = '.map(str::to_string)'; After = '.map(|_| logical_name.clone())'; Count = 1 },
    @{ Name = 'expected-length-forwarding'; Expected = 'native_ownership'; Scope = 'reopen_handle_input_v2'; Before = 'source_handle, expected_length'; After = 'source_handle, expected_length.saturating_sub(1)'; Count = 1 },
    @{ Name = 'invalid-handle-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE'; After = 'NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE + 1'; Count = 1 },
    @{ Name = 'io-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::Io => QL_ERROR_IO'; After = 'NativeInputError::Io => QL_ERROR_IO + 1'; Count = 1 },
    @{ Name = 'length-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH'; After = 'NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH + 1'; Count = 1 },
    @{ Name = 'renderer-error-status'; Expected = 'reader_errors'; Scope = 'preview_handle_v2'; Before = 'Err(status) => return status,'; After = 'Err(status) => return status + 1,'; Count = 2; Occurrence = 1 },
    @{ Name = 'empty-render-status'; Expected = 'reader_errors'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INTERNAL;'; After = 'return QL_ERROR_INTERNAL + 1;'; Count = 1 },
    @{ Name = 'reopen-cancel-status'; Expected = 'handle_cancellation'; Scope = 'reopen_handle_input_v2'; Before = 'return Err(QL_ERROR_CANCELLED);'; After = 'return Err(QL_ERROR_CANCELLED + 1);'; Count = 2 },
    @{ Name = 'preview-cancel-status'; Expected = 'handle_cancellation'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_CANCELLED;'; After = 'return QL_ERROR_CANCELLED + 1;'; Count = 2 },
    @{ Name = 'cancelled-file-leak'; Expected = 'handle_cancellation'; Scope = 'reopen_handle_input_v2'; Before = 'return Err(QL_ERROR_CANCELLED);'; After = 'std::mem::forget(file); return Err(QL_ERROR_CANCELLED);'; Count = 2; Occurrence = 1 }
)
foreach ($scope in @('reopen_handle_input_v2', 'preview_handle_v2')) {
    foreach ($occurrence in @(0, 1)) {
        $cases += @{ Name = "$scope-cancel-check-$occurrence"; Expected = 'handle_cancellation'; Scope = $scope;
            Before = 'if cancel_requested(cancel_cb) {'; After = 'if std::hint::black_box(false) && cancel_requested(cancel_cb) {'; Count = 2; Occurrence = $occurrence }
    }
}
foreach ($status in @('Cancelled', 'Io', 'Malformed', 'LengthMismatch', 'LimitExceeded')) {
    $constant = @{ Cancelled = 'QL_ERROR_CANCELLED'; Io = 'QL_ERROR_IO'; Malformed = 'QL_ERROR_MALFORMED';
        LengthMismatch = 'QL_ERROR_LENGTH_MISMATCH'; LimitExceeded = 'QL_ERROR_LIMIT_EXCEEDED' }[$status]
    $cases += @{ Name = "reader-status-$status"; Expected = 'reader_errors'; Scope = 'reader_preview_status';
        Before = "ReaderPreviewError::$status => $constant"; After = "ReaderPreviewError::$status => $constant + 1"; Count = 1 }
}
foreach ($export in @('ql_extract_pdf_text_handle', 'ql_preview_image_metadata_handle')) {
    $cases += @{ Name = "$export-callback"; Expected = 'handle_cancellation'; Scope = $export;
        Before = "            cancel_cb,`n"; After = "            None,`n"; Count = 1 }
}

Push-Location -LiteralPath $Root
$tempRoot = $null
try {
    $driver = (& rustup which clippy-driver | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $driver)) { throw 'Pinned clippy-driver is required.' }
    # Ask Cargo for this workspace's exact dependency artifact instead of selecting a stale
    # rlib by filename/mtime. This builds only test-profile artifacts, never a Release DLL.
    $cargoOutput = @(& cargo test --workspace --all-features --locked --no-run `
        --manifest-path (Join-Path $Root 'native/Cargo.toml') --message-format=json 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "Cannot prepare real Windows test dependency: $($cargoOutput -join "`n")" }
    $artifacts = @($cargoOutput | Where-Object { "$_".StartsWith('{') } | ForEach-Object { "$_" | ConvertFrom-Json })
    $windows = @($artifacts | Where-Object { $_.reason -ceq 'compiler-artifact' -and $_.target.name -ceq 'windows' })
    if ($windows.Count -ne 1) { throw 'Expected exactly one locked Windows dependency artifact.' }
    $windowsLib = @($windows[0].filenames | Where-Object { $_.EndsWith('.rlib') })
    if ($windowsLib.Count -ne 1 -or -not (Test-Path -LiteralPath $windowsLib[0])) { throw 'Windows test rlib is missing.' }
    $dependencyDir = Split-Path $windowsLib[0] -Parent
    $tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/rust-handle-preview-behavior'))
    $tempRoot = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $tempRoot)
    $wrapper = Join-Path $tempRoot 'contract.rs'
    $executable = Join-Path $tempRoot 'contract.exe'
    foreach ($case in $cases) {
        $selectedHandle = $handlePath
        $selectedInput = $inputPath
        if ($case.Expected) {
            $selectedHandle = Join-Path $tempRoot 'handle_preview.rs'
            $selectedInput = Join-Path $tempRoot 'native_input.rs'
            [IO.File]::WriteAllText($selectedHandle, (Edit-HandleFixture $handleText $case))
            [IO.File]::WriteAllText($selectedInput, $inputText)
        }
        $abiInclude = (Join-Path $sourceRoot 'abi_generated.rs').Replace('\', '/')
        $commonInclude = (Join-Path $sourceRoot 'ffi/common.rs').Replace('\', '/')
        $probeInclude = (Join-Path $sourceRoot 'preview/file_probe.rs').Replace('\', '/')
        $handleInclude = $selectedHandle.Replace('\', '/')
        $inputInclude = $selectedInput.Replace('\', '/')
        $fixtureInclude = $fixturePath.Replace('\', '/')
        $body = @"
$crateLints
#![allow(dead_code)]
include!("$abiInclude");
$logicalLimit
type CancelCallback = extern "C" fn() -> bool;
$cancelHelper
#[path = "$commonInclude"] mod ffi_common;
#[path = "$inputInclude"] mod actual_native_input;
mod native_input {
    pub use crate::actual_native_input::NativeInputError;
    pub use crate::recorded_reopen as reopen_borrowed_disk_file;
}
mod preview {
    #![forbid(unsafe_code)]
    $readerError
    pub fn is_text_file(_name: &str, ext: &str, _bytes: &[u8]) -> bool { ext == ".txt" }
    #[derive(Default)] pub struct AnimationProbe { pub is_animated: Option<bool> }
    pub fn probe_image_animation_reader(_file: &mut std::fs::File, _name: &str, _size: u64) -> Option<AnimationProbe> { None }
    #[path = "$probeInclude"] pub(crate) mod actual_file_probe;
    pub mod file_probe { pub use crate::probe_reader as probe_reader_json; }
    pub(crate) use crate::{pdf_reader as render_pdf_text_reader, image_reader as render_image_metadata_reader};
}
mod rar_listing { pub fn is_rar_magic(bytes: &[u8]) -> bool { bytes.starts_with(b"Rar!\x1a\x07") } }
mod ffi {
    use crate::ffi_common as common;
    #[path = "$handleInclude"] pub mod handle_preview;
}
include!("$fixtureInclude");
"@
        [IO.File]::WriteAllText($wrapper, $body)
        $compiled = @(& $driver $wrapper --edition=2021 --crate-name=rust_handle_preview_behavior `
            --target=x86_64-pc-windows-msvc --extern "windows=$($windowsLib[0])" -L "dependency=$dependencyDir" `
            -C panic=unwind -D warnings -o $executable 2>&1)
        if ($LASTEXITCODE -ne 0) { throw "HANDLE fixture must compile first ($($case.Name)): $($compiled -join "`n")" }
        $output = @(& $executable $tempRoot 2>&1)
        $probeExit = $LASTEXITCODE
        $expectedText = if ($case.Expected) { 'HANDLE_CONTRACT_FAILED:' + $case.Expected } else { 'HANDLE_CONTRACT_PASSED:7' }
        $expectedExit = if ($case.Expected) { 1 } else { 0 }
        if ($probeExit -ne $expectedExit -or ($output -join "`n") -cne $expectedText) {
            throw "Unexpected HANDLE behavior ($($case.Name)): exit=$probeExit; $($output -join "`n")"
        }
    }
    Write-Host "Rust HANDLE preview behavior passed: 7 groups; 1 positive, $($cases.Count - 1) compiled mutation rejections; real Win32 reopen." -ForegroundColor Green
}
finally {
    if ($tempRoot -and (Test-Path -LiteralPath $tempRoot)) {
        $resolved = [IO.Path]::GetFullPath($tempRoot)
        if (-not $resolved.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing cleanup outside the HANDLE fixture directory.'
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Pop-Location
}
$global:LASTEXITCODE = 0
