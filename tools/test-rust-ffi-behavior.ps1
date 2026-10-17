param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
# Expected runtime assertion failures are inspected, not leaked to the parent guard.
$PSNativeCommandUseErrorActionPreference = $false
$Root = [IO.Path]::GetFullPath($Root)
$sourceRoot = Join-Path $Root 'native/quicklook_next_native/src'
$fixturePath = Join-Path $PSScriptRoot 'fixtures/rust-ffi-behavior.rs'
$pathFixturePath = Join-Path $PSScriptRoot 'fixtures/rust-path-preview-behavior.rs'
$libText = Get-Content -LiteralPath (Join-Path $sourceRoot 'lib.rs') -Raw
$crateLints = @([regex]::Matches($libText, '(?m)^#!\[(?:deny|forbid)\([^\r\n]*\)\]') |
    ForEach-Object Value) -join "`n"
if ($crateLints -notmatch 'unsafe_op_in_unsafe_fn' -or $crateLints -notmatch 'clippy::undocumented_unsafe_blocks') {
    throw 'Actual crate safety lint attributes are missing.'
}
$commonPath = Join-Path $sourceRoot 'ffi/common.rs'
$rasterPath = Join-Path $sourceRoot 'ffi/raster_packet.rs'
$fileProbePath = Join-Path $sourceRoot 'ffi/file_probe.rs'
$thumbnailPath = Join-Path $sourceRoot 'ffi/thumbnail.rs'
$pathPreviewPath = Join-Path $sourceRoot 'ffi/path_preview.rs'
$previewFileProbePath = Join-Path $sourceRoot 'preview/file_probe.rs'
$commonText = (Get-Content -LiteralPath $commonPath -Raw).Replace("`r`n", "`n")
$rasterText = (Get-Content -LiteralPath $rasterPath -Raw).Replace("`r`n", "`n")
$fileProbeText = (Get-Content -LiteralPath $fileProbePath -Raw).Replace("`r`n", "`n")
$thumbnailText = (Get-Content -LiteralPath $thumbnailPath -Raw).Replace("`r`n", "`n")
$pathPreviewText = (Get-Content -LiteralPath $pathPreviewPath -Raw).Replace("`r`n", "`n")
$cancelHelper = [regex]::Match($libText, '(?ms)^fn cancel_requested\([^\r\n]*\) -> bool \{\r?\n.*?^\}').Value
$stringLimit = [regex]::Match($libText, '(?m)^const MAX_FFI_STRING_BYTES:[^\r\n]*;').Value
if (-not $cancelHelper -or -not $stringLimit) { throw 'Actual cancellation helper/string limit is missing.' }

function Edit-FixtureText([string]$Text, $Case) {
    $prefix = ''
    $suffix = ''
    if ($Case.Export) {
        $exportStarts = [regex]::Matches($Text, '(?m)^pub unsafe extern "C" fn (?<name>[a-z0-9_]+)\(')
        $selected = @($exportStarts | Where-Object { $_.Groups['name'].Value -ceq $Case.Export })
        if ($selected.Count -ne 1) { throw "Mutation export drifted ($($Case.Name)): $($Case.Export)." }
        $start = $selected[0].Index
        $following = @($exportStarts | Where-Object Index -GT $start | Select-Object -First 1)
        $end = if ($following.Count) { $following[0].Index } else { $Text.Length }
        $prefix = $Text.Substring(0, $start)
        $suffix = $Text.Substring($end)
        $Text = $Text.Substring($start, $end - $start)
    }
    $count = [regex]::Matches($Text, [regex]::Escape($Case.Before)).Count
    if ($count -ne $Case.Count) { throw "Mutation anchor drifted ($($Case.Name)): expected $($Case.Count), got $count." }
    if ($Case.ContainsKey('Occurrence')) {
        $mutationMatches = [regex]::Matches($Text, [regex]::Escape($Case.Before))
        if ($Case.Occurrence -lt 0 -or $Case.Occurrence -ge $mutationMatches.Count) { throw 'Invalid mutation occurrence.' }
        $index = $mutationMatches[$Case.Occurrence].Index
        $Text = $Text.Substring(0, $index) + $Case.After + $Text.Substring($index + $Case.Before.Length)
    }
    else { $Text = $Text.Replace($Case.Before, $Case.After) }
    return $prefix + $Text + $suffix
}

# Mutations keep raw-pointer slice bounds/isize/overflow checks intact. Short-capacity
# tests use initialized allocations larger than the declared cap, so canaries
# detect unwanted writes without deliberately introducing out-of-allocation writes.
# The zero-capacity path mutation still uses the real byte writer's capacity guard.
$cases = @(
    @{ Name = 'actual'; Expected = $null },
    @{ Name = 'input-limit'; Expected = 'arguments_bounds'; File = 'common'; Before = 'len > max_len'; After = 'len.saturating_sub(1) > max_len'; Count = 3 },
    @{ Name = 'valid-utf8-rejected'; Expected = 'arguments_text'; File = 'common'; Before = 'std::str::from_utf8(bytes).ok()'; After = 'std::str::from_utf8(bytes).ok().filter(|_| false)'; Count = 2 },
    @{ Name = 'owned-copy-lost'; Expected = 'arguments_text'; File = 'common'; Before = 'map(str::to_owned)'; After = 'map(|_| String::new())'; Count = 1 },
    @{ Name = 'optional-text-empty'; Expected = 'arguments_optional'; File = 'common'; Before = 'return (len == 0).then_some("");'; After = 'return None;'; Count = 1 },
    @{ Name = 'optional-bytes-empty'; Expected = 'arguments_optional'; File = 'common'; Before = 'return (len == 0).then_some(&[]);'; After = 'return None;'; Count = 1 },
    @{ Name = 'legacy-empty-status'; Expected = 'legacy_empty'; File = 'common'; Before = 'return 0;'; After = 'return 1;'; Count = 1 },
    @{ Name = 'legacy-capacity'; Expected = 'legacy_output'; File = 'common'; Before = 'if needed > out_cap {'; After = 'if needed > out_cap && out_cap == 0 {'; Count = 1 },
    @{ Name = 'legacy-short-status'; Expected = 'legacy_output'; File = 'common'; Before = 'return -(needed as i32);'; After = 'return -1;'; Count = 1 },
    @{ Name = 'legacy-payload'; Expected = 'legacy_output'; File = 'common'; Before = 'std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, needed);'; After = 'std::ptr::write_bytes(out_buf, 0, needed);'; Count = 1 },
    @{ Name = 'legacy-written-size'; Expected = 'legacy_output'; File = 'common'; Before = "`n    needed as i32`n}"; After = "`n    (needed as i32) + 1`n}"; Count = 1 },
    @{ Name = 'v2-required-size'; Expected = 'v2_output'; File = 'common'; Before = '*out_required = bytes.len()'; After = '*out_required = bytes.len() + 1'; Count = 1 },
    @{ Name = 'v2-capacity'; Expected = 'v2_output'; File = 'common'; Before = 'if bytes.len() > out_cap {'; After = 'if bytes.len() > out_cap && out_cap == 0 {'; Count = 1 },
    @{ Name = 'v2-short-status'; Expected = 'v2_output'; File = 'common'; Before = 'return QL_ERROR_BUFFER_TOO_SMALL;'; After = 'return QL_ERROR_BUFFER_TOO_SMALL - 1;'; Count = 1 },
    @{ Name = 'v2-null-status'; Expected = 'v2_output'; File = 'common'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT - 1;'; Count = 1 },
    @{ Name = 'v2-success-status'; Expected = 'v2_output'; File = 'common'; Before = "`n    QL_OK`n}"; After = "`n    QL_OK - 1`n}"; Count = 1 },
    @{ Name = 'panic-i32-status'; Expected = 'panic_i32'; File = 'common'; Before = 'unwrap_or(QL_ERROR_INTERNAL)'; After = 'unwrap_or(QL_ERROR_INTERNAL + 1)'; Count = 1 },
    @{ Name = 'panic-void-escape'; Expected = 'panic_void'; File = 'common'; Before = 'let _ = catch_unwind(AssertUnwindSafe(body));'; After = 'body();'; Count = 1 },
    @{ Name = 'raster-width-header'; Expected = 'raster_legacy'; File = 'raster'; Before = 'copy_from_slice(&width.to_le_bytes())'; After = 'copy_from_slice(&height.to_le_bytes())'; Count = 1 },
    @{ Name = 'raster-payload'; Expected = 'raster_legacy'; File = 'raster'; Before = 'output[8..].copy_from_slice(bgra);'; After = 'output[8..].fill(0);'; Count = 1 },
    @{ Name = 'raster-short-status'; Expected = 'raster_legacy'; File = 'raster'; Before = 'return -(total as i32);'; After = 'return -1;'; Count = 1 },
    @{ Name = 'raster-legacy-capacity'; Expected = 'raster_legacy'; File = 'raster'; Before = 'out.is_null() || out_cap < total'; After = 'out.is_null() || (out_cap < total && out_cap == 0)'; Count = 1 },
    @{ Name = 'raster-geometry-status'; Expected = 'raster_geometry'; File = 'raster'; Before = 'return -2;'; After = 'return -1;'; Count = 3 },
    @{ Name = 'raster-invalid-required'; Expected = 'raster_geometry'; File = 'raster'; Before = '*out_required = 0'; After = '*out_required = 7'; Count = 1 },
    @{ Name = 'raster-v2-required'; Expected = 'raster_v2'; File = 'raster'; Before = '*out_required = total'; After = '*out_required = total + 1'; Count = 1 },
    @{ Name = 'raster-v2-capacity'; Expected = 'raster_v2'; File = 'raster'; Before = 'if out_cap < total {'; After = 'if out_cap < total && out_cap == 0 {'; Count = 1 },
    @{ Name = 'raster-v2-short-status'; Expected = 'raster_v2'; File = 'raster'; Before = 'return QL_ERROR_BUFFER_TOO_SMALL;'; After = 'return QL_ERROR_BUFFER_TOO_SMALL - 1;'; Count = 1 },
    @{ Name = 'raster-v2-null-status'; Expected = 'raster_v2'; File = 'raster'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT - 1;'; Count = 1 },
    @{ Name = 'raster-zero-dimension'; Expected = 'raster_geometry'; File = 'raster'; Before = 'width == 0 || height == 0'; After = 'width == 0 && height == 0'; Count = 1 },
    @{ Name = 'raster-excess-pixels'; Expected = 'raster_geometry'; File = 'raster'; Before = 'bgra_len != expected_bgra_len'; After = 'bgra_len < expected_bgra_len'; Count = 1 },
    @{ Name = 'file-probe-invalid-path'; Expected = 'file_probe'; File = 'file_probe'; Before = 'None => return -1,'; After = 'None => return -2,'; Count = 1 },
    @{ Name = 'file-probe-missing-file'; Expected = 'file_probe'; File = 'file_probe'; Before = 'None => return -2,'; After = 'None => return -1,'; Count = 1 },
    @{ Name = 'file-probe-capacity'; Expected = 'file_probe'; File = 'file_probe'; Before = 'out.is_null() || out_cap < bytes.len()'; After = 'out.is_null() && out_cap < bytes.len()'; Count = 1 },
    @{ Name = 'file-probe-short-status'; Expected = 'file_probe'; File = 'file_probe'; Before = 'return -(bytes.len() as i32);'; After = 'return -1;'; Count = 1 },
    @{ Name = 'file-probe-payload'; Expected = 'file_probe'; File = 'file_probe'; Before = 'std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len())'; After = 'std::ptr::write_bytes(out, 0, bytes.len())'; Count = 1 },
    @{ Name = 'file-probe-written-size'; Expected = 'file_probe'; File = 'file_probe'; Before = "`n        bytes.len() as i32`n    })"; After = "`n        (bytes.len() as i32) + 1`n    })"; Count = 1 },
    @{ Name = 'thumbnail-invalid-path'; Expected = 'thumbnail'; File = 'thumbnail'; Before = 'None => return -1,'; After = 'None => return -2,'; Count = 1 },
    @{ Name = 'thumbnail-invalid-flags-status'; Expected = 'thumbnail'; File = 'thumbnail'; Before = 'return QL_ERROR_INVALID_ARGUMENT'; After = 'return QL_ERROR_LIMIT_EXCEEDED + (QL_ERROR_INVALID_ARGUMENT - QL_ERROR_BUFFER_TOO_SMALL) + (QL_ERROR_BUFFER_TOO_SMALL - QL_ERROR_INVALID_ARGUMENT)'; Count = 1 },
    @{ Name = 'thumbnail-limit-status'; Expected = 'thumbnail'; File = 'thumbnail'; Before = 'return QL_ERROR_LIMIT_EXCEEDED'; After = 'return QL_ERROR_INVALID_ARGUMENT + (QL_ERROR_LIMIT_EXCEEDED - QL_ERROR_BUFFER_TOO_SMALL) + (QL_ERROR_BUFFER_TOO_SMALL - QL_ERROR_LIMIT_EXCEEDED)'; Count = 1 },
    @{ Name = 'thumbnail-cancel-status'; Expected = 'thumbnail'; File = 'thumbnail'; Before = 'return QL_ERROR_CANCELLED'; After = 'return QL_ERROR_LIMIT_EXCEEDED + (QL_ERROR_CANCELLED - QL_ERROR_BUFFER_TOO_SMALL) + (QL_ERROR_BUFFER_TOO_SMALL - QL_ERROR_CANCELLED)'; Count = 1 },
    @{ Name = 'thumbnail-unavailable-status'; Expected = 'thumbnail'; File = 'thumbnail'; Before = 'return QL_ERROR_BUFFER_TOO_SMALL'; After = 'return QL_ERROR_CANCELLED + (QL_ERROR_BUFFER_TOO_SMALL - QL_ERROR_INVALID_ARGUMENT) + (QL_ERROR_INVALID_ARGUMENT - QL_ERROR_BUFFER_TOO_SMALL)'; Count = 1 },
    @{ Name = 'thumbnail-packet-input'; Expected = 'thumbnail'; File = 'thumbnail'; Before = '&bgra, out, out_cap'; After = '&bgra[..0], out, out_cap'; Count = 1 }
)

$pathBodies = @('ql_preview_text_cancelable', 'ql_preview_info', 'ql_preview_executable_cancelable',
    'ql_preview_archive', 'ql_preview_ebook_cancelable', 'ql_preview_torrent_cancelable',
    'ql_preview_office', 'ql_preview_image_metadata')
foreach ($export in $pathBodies) {
    $cases += @{ Name = "$export-zero-capacity"; Expected = 'path_arguments'; File = 'path_preview'; Export = $export;
        Before = ' || out_cap == 0'; After = ''; Count = 1 }
    $cases += @{ Name = "$export-invalid-status"; Expected = 'path_arguments'; File = 'path_preview'; Export = $export;
        Before = 'None => return 0,'; After = 'None => return -1,'; Count = 1 }
    $cases += @{ Name = "$export-empty-payload"; Expected = 'path_outputs'; File = 'path_preview'; Export = $export;
        Before = 'write_json_out(&json,'; After = 'write_json_out(&json[..0],'; Count = 1 }
}
foreach ($kind in @('text', 'executable', 'ebook', 'torrent')) {
    $export = "ql_preview_${kind}_cancelable"
    foreach ($occurrence in @(0, 1)) {
        $cases += @{ Name = "$export-cancel-check-$occurrence"; Expected = 'path_cancellation';
            File = 'path_preview'; Export = $export; Before = 'if cancel_requested(cancel_cb) {';
            After = 'if std::hint::black_box(false) && cancel_requested(cancel_cb) {'; Count = 2; Occurrence = $occurrence }
    }
    $cases += @{ Name = "$export-cancel-status"; Expected = 'path_cancellation'; File = 'path_preview'; Export = $export;
        Before = 'return -3;'; After = 'return -2;'; Count = 2 }
    $other = if ($kind -ceq 'text') { 'executable' } else { 'text' }
    $cases += @{ Name = "ql_preview_$kind-forwarding"; Expected = 'path_outputs'; File = 'path_preview';
        Export = "ql_preview_$kind"; Before = "ql_preview_${kind}_cancelable(path_utf8,";
        After = "ql_preview_${other}_cancelable(path_utf8,"; Count = 1 }
}
foreach ($kind in @('text', 'executable', 'archive', 'ebook', 'torrent', 'office')) {
    $export = if ($kind -in @('text', 'executable', 'ebook', 'torrent')) { "ql_preview_${kind}_cancelable" } else { "ql_preview_$kind" }
    $other = if ($kind -ceq 'text') { 'executable' } else { 'text' }
    $cases += @{ Name = "$export-renderer"; Expected = 'path_outputs'; File = 'path_preview'; Export = $export;
        Before = "render_$kind(path, cancel_cb)"; After = "render_$other(path, cancel_cb)"; Count = 1 }
}
$cases += @(
    @{ Name = 'path-image-renderer'; Expected = 'path_outputs'; File = 'path_preview'; Export = 'ql_preview_image_metadata'; Before = 'render_image_metadata(path)'; After = 'render_info(path, "", 0, 0)'; Count = 1 },
    @{ Name = 'path-info-size'; Expected = 'path_outputs'; File = 'path_preview'; Export = 'ql_preview_info'; Before = 'kind, size, modified_unix'; After = 'kind, size + 1, modified_unix'; Count = 1 },
    @{ Name = 'path-info-modified'; Expected = 'path_outputs'; File = 'path_preview'; Export = 'ql_preview_info'; Before = 'kind, size, modified_unix'; After = 'kind, size, modified_unix + 1'; Count = 1 },
    @{ Name = 'path-info-kind'; Expected = 'path_optional'; File = 'path_preview'; Export = 'ql_preview_info'; Before = 'unwrap_or("")'; After = 'unwrap_or("other")'; Count = 1 },
    @{ Name = 'path-archive-callback'; Expected = 'path_outputs'; File = 'path_preview'; Export = 'ql_preview_archive'; Before = 'render_archive(path, cancel_cb)'; After = 'render_archive(path, cancel_cb.filter(|_| std::hint::black_box(false)))'; Count = 1 },
    @{ Name = 'path-office-callback'; Expected = 'path_outputs'; File = 'path_preview'; Export = 'ql_preview_office'; Before = 'render_office(path, cancel_cb)'; After = 'render_office(path, cancel_cb.filter(|_| std::hint::black_box(false)))'; Count = 1 }
)

Push-Location -LiteralPath $Root
$tempRoot = $null
try {
    $driver = (& rustup which clippy-driver | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $driver -PathType Leaf)) { throw 'Pinned clippy-driver is required.' }
    $tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/rust-ffi-behavior'))
    $tempRoot = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $tempRoot)
    $wrapper = Join-Path $tempRoot 'contract.rs'
    $executable = Join-Path $tempRoot 'contract.exe'
    foreach ($case in $cases) {
        $selectedCommon = $commonPath
        $selectedRaster = $rasterPath
        $selectedFileProbe = $fileProbePath
        $selectedThumbnail = $thumbnailPath
        $selectedPathPreview = $pathPreviewPath
        if ($case.File) {
            $selectedCommon = Join-Path $tempRoot 'common.rs'
            $selectedRaster = Join-Path $tempRoot 'raster_packet.rs'
            $selectedFileProbeDirectory = Join-Path $tempRoot 'ffi'
            [void](New-Item -ItemType Directory -Path $selectedFileProbeDirectory -Force)
            $selectedFileProbe = Join-Path $selectedFileProbeDirectory 'file_probe.rs'
            $selectedThumbnail = Join-Path $selectedFileProbeDirectory 'thumbnail.rs'
            $selectedPathPreview = Join-Path $selectedFileProbeDirectory 'path_preview.rs'
            $mutatedCommon = if ($case.File -ceq 'common') { Edit-FixtureText $commonText $case } else { $commonText }
            $mutatedRaster = if ($case.File -ceq 'raster') { Edit-FixtureText $rasterText $case } else { $rasterText }
            $mutatedFileProbe = if ($case.File -ceq 'file_probe') { Edit-FixtureText $fileProbeText $case } else { $fileProbeText }
            $mutatedThumbnail = if ($case.File -ceq 'thumbnail') { Edit-FixtureText $thumbnailText $case } else { $thumbnailText }
            $mutatedPathPreview = if ($case.File -ceq 'path_preview') { Edit-FixtureText $pathPreviewText $case } else { $pathPreviewText }
            [IO.File]::WriteAllText($selectedCommon, $mutatedCommon)
            [IO.File]::WriteAllText($selectedRaster, $mutatedRaster)
            [IO.File]::WriteAllText($selectedFileProbe, $mutatedFileProbe)
            [IO.File]::WriteAllText($selectedThumbnail, $mutatedThumbnail)
            [IO.File]::WriteAllText($selectedPathPreview, $mutatedPathPreview)
            Copy-Item -LiteralPath (Join-Path $sourceRoot 'ffi_pointer_safety.md') `
                -Destination (Join-Path $tempRoot 'ffi_pointer_safety.md') -Force
        }
        $abiPath = (Join-Path $sourceRoot 'abi_generated.rs').Replace('\', '/')
        $commonInclude = $selectedCommon.Replace('\', '/')
        $rasterInclude = $selectedRaster.Replace('\', '/')
        $fileProbeInclude = $selectedFileProbe.Replace('\', '/')
        $thumbnailInclude = $selectedThumbnail.Replace('\', '/')
        $pathPreviewInclude = $selectedPathPreview.Replace('\', '/')
        $previewFileProbeInclude = $previewFileProbePath.Replace('\', '/')
        $behaviorInclude = $fixturePath.Replace('\', '/')
        $pathBehaviorInclude = $pathFixturePath.Replace('\', '/')
        $body = @"
$crateLints
#![allow(dead_code)]
include!("$abiPath");
$stringLimit
type CancelCallback = extern "C" fn() -> bool;
#[path = "$commonInclude"] mod ffi_common;
#[path = "$rasterInclude"] mod raster_packet;
#[path = "$pathBehaviorInclude"] mod path_behavior;
mod preview {
    pub fn is_text_file(file_name: &str, ext: &str, magic: &[u8]) -> bool {
        matches!(ext, ".txt" | ".json" | ".md" | ".ini" | ".log") ||
            (file_name.ends_with(".config") && magic.iter().all(|value| value.is_ascii()))
    }
    #[derive(Default)]
    pub struct AnimationProbe { pub is_animated: Option<bool> }
    pub fn probe_image_animation_reader(
        _file: &mut std::fs::File,
        _file_name: &str,
        _size: u64,
    ) -> Option<AnimationProbe> { None }
    pub use crate::path_behavior::provider::{render_archive, render_ebook, render_executable,
        render_image_metadata, render_info, render_office, render_text, render_torrent};
    #[path = "$previewFileProbeInclude"]
    pub mod file_probe;
}
// Keep every substitute signature type-checked even when a renderer-routing
// mutation removes its last actual call; negative cases must compile first.
const _: [fn(&str, Option<CancelCallback>) -> String; 6] = [preview::render_text,
    preview::render_executable, preview::render_archive, preview::render_ebook,
    preview::render_torrent, preview::render_office];
const _: fn(&str) -> String = preview::render_image_metadata;
const _: fn(&str, &str, i64, i64) -> String = preview::render_info;
mod rar_listing {
    pub fn is_rar_magic(magic: &[u8]) -> bool {
        magic.starts_with(b"Rar!\x1A\x07")
    }
}
$cancelHelper
mod win32 {
    pub mod shell_thumbnail {
        use crate::CancelCallback;
        pub type Thumbnail = (u32, u32, Vec<u8>);
        #[derive(Clone, Copy)]
        pub enum ThumbnailError { InvalidFlags, LimitExceeded, Cancelled, Unavailable }
        pub fn request(
            path: String,
            size: i32,
            flags: u32,
            cancel_cb: Option<CancelCallback>,
        ) -> Result<Thumbnail, ThumbnailError> {
            if flags & !3 != 0 { return Err(ThumbnailError::InvalidFlags); }
            if !(16..=512).contains(&size) { return Err(ThumbnailError::LimitExceeded); }
            if cancel_cb.is_some_and(|callback| callback()) {
                return Err(ThumbnailError::Cancelled);
            }
            if path == "missing.thumbnail" { return Err(ThumbnailError::Unavailable); }
            Ok((2, 3, (1..=24).collect()))
        }
    }
}
mod ffi {
    use crate::ffi_common as common;
    use crate::raster_packet;
    #[path = "$fileProbeInclude"]
    pub mod file_probe;
    #[path = "$thumbnailInclude"]
    pub mod thumbnail;
    #[path = "$pathPreviewInclude"]
    pub mod path_preview;
}
include!("$behaviorInclude");
"@
        [IO.File]::WriteAllText($wrapper, $body)
        # A linked probe executable, not a native Release DLL or a second cargo build path.
        $compilerOutput = @(& $driver $wrapper --edition=2021 --crate-name=rust_ffi_behavior `
            --target=x86_64-pc-windows-msvc -C panic=unwind -D warnings -o $executable 2>&1)
        if ($LASTEXITCODE -ne 0) { throw "Behavior fixture must compile first ($($case.Name)): $($compilerOutput -join "`n")" }
        $output = @(& $executable $tempRoot 2>&1)
        $probeExit = $LASTEXITCODE
        if (-not $case.Expected) {
            if ($probeExit -ne 0 -or ($output -join "`n") -cne 'FFI_CONTRACT_PASSED:18') {
                throw "Positive FFI behavior fixture failed ($($case.Name)): $($output -join "`n")"
            }
        }
        elseif ($probeExit -ne 1 -or ($output -join "`n") -cne ('FFI_CONTRACT_FAILED:' + $case.Expected)) {
            throw "Mutation must fail its expected behavior case ($($case.Name)): exit=$probeExit; $($output -join "`n")"
        }
    }
    Write-Host "Rust FFI behavior contracts passed: 18 cases; 1 positive, $($cases.Count - 1) compiled mutation rejections." -ForegroundColor Green
}
finally {
    if ($tempRoot -and (Test-Path -LiteralPath $tempRoot)) {
        $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
        if (-not $resolvedTemp.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing cleanup outside the FFI behavior fixture directory.'
        }
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
    }
    Pop-Location
}
$global:LASTEXITCODE = 0
