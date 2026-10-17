param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$crateRoot = Join-Path $Root "native\quicklook_next_native"
$manifestPath = Join-Path $crateRoot "Cargo.toml"
$sourceRoot = Join-Path $crateRoot "src"
foreach ($path in @($manifestPath, $sourceRoot)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Rust lint-scope input is missing: $path"
    }
}

$failures = [Collections.Generic.List[string]]::new()
$manifestText = Get-Content -LiteralPath $manifestPath -Raw
$rootText = Get-Content -LiteralPath (Join-Path $sourceRoot 'lib.rs') -Raw
foreach ($requiredLint in @('unsafe_op_in_unsafe_fn', 'clippy::undocumented_unsafe_blocks')) {
    if ($rootText -notmatch ('#!\[deny\(' + [regex]::Escape($requiredLint) + '\)\]')) {
        $failures.Add("The crate root must deny $requiredLint.")
    }
}
foreach ($animationWriter in @('write_animation_frames', 'write_animation_frames_v2', 'write_animation_frames_direct')) {
    if ($rootText -notmatch ('(?m)^unsafe\s+fn\s+' + $animationWriter + '\s*\(')) {
        $failures.Add("Raw-pointer animation writer $animationWriter must require an unsafe caller.")
    }
}
if ($manifestText -match '(?m)^\s*(?:unsafe_op_in_unsafe_fn|undocumented_unsafe_blocks)\s*=\s*"(?:allow|warn)"') {
    $failures.Add('Cargo.toml must not weaken unsafe-operation or unsafe-block documentation lints.')
}
if ($manifestText -match '(?m)^\s*(?:too_many_arguments|type_complexity)\s*=\s*"allow"\s*$') {
    $failures.Add(
        "Cargo.toml must not disable argument-count or type-complexity lints for the crate.")
}

$productionExceptions = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal)
[void]$productionExceptions.Add("decode_animation_frames_handle_v2")
[void]$productionExceptions.Add("preview_handle_v2")

$scopedExceptionCount = 0
$rustFiles = @(Get-ChildItem -LiteralPath $sourceRoot -Recurse -File -Filter "*.rs")
foreach ($rustFile in $rustFiles) {
    $source = Get-Content -LiteralPath $rustFile.FullName -Raw
    if ($rustFile.Name -eq 'raster_packet.rs') {
        foreach ($rasterWriter in @('write_raster_packet', 'write_raster_packet_v2')) {
            if ($source -notmatch ('pub\(crate\)\s+unsafe\s+fn\s+' + $rasterWriter + '\s*\(')) {
                $failures.Add("Raw-pointer raster writer $rasterWriter must require an unsafe caller.")
            }
        }
    }
    if ($source -match '#!?\[(?:(?:cfg_attr\([^\]]*)?\s*(?:allow|warn|expect)\([^\]]*\b(?:unsafe_op_in_unsafe_fn|undocumented_unsafe_blocks)\b)') {
        $failures.Add("Safety lint downgrades are forbidden: $($rustFile.FullName)")
    }
    if ($source -match '#\[allow\(clippy::type_complexity\)\]') {
        $failures.Add(
            "Type-complexity exemptions are forbidden; introduce a named type: $($rustFile.FullName)")
    }

    $matches = [regex]::Matches(
        $source,
        '#\[allow\(clippy::too_many_arguments\)\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?fn\s+(?<name>[A-Za-z0-9_]+)')
    foreach ($match in $matches) {
        $scopedExceptionCount++
        $name = $match.Groups["name"].Value
        if ($productionExceptions.Contains($name)) {
            continue
        }

        $isTestAbiAdapter = $rustFile.Name -eq "lib.rs" -and $name.StartsWith(
            "call_", [StringComparison]::Ordinal)
        if ($isTestAbiAdapter) {
            $remaining = $source.Substring($match.Index)
            $body = $remaining.Substring(0, [Math]::Min(1600, $remaining.Length))
            if ($body -match '\bql_[a-z0-9_]+\s*\(') {
                continue
            }
        }

        $failures.Add(
            "clippy::too_many_arguments may only be scoped to the two ABI adapters or exact call_* test mirrors: $name in $($rustFile.FullName)")
    }
}

if ($scopedExceptionCount -eq 0) {
    $failures.Add("No scoped ABI argument-count exemptions were found; the guard inspected nothing.")
}

if ($failures.Count -gt 0) {
    Write-Host "Rust lint-scope test failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host (
    "Rust lint-scope test passed; files=$($rustFiles.Count); scoped ABI exemptions=$scopedExceptionCount") `
    -ForegroundColor Green

. (Join-Path $PSScriptRoot 'checked-invocation.ps1')
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot 'test-rust-compiler-contracts.ps1') `
    -Arguments @{ Root = $Root } -FailureMessage 'Rust compiler safety contracts failed'
