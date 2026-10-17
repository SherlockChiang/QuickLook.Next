param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = 'Stop'
# Negative compiler fixtures intentionally return nonzero; inspect their diagnostics below.
$PSNativeCommandUseErrorActionPreference = $false
$sourceRoot = Join-Path $Root 'native/quicklook_next_native/src'
$fixturePath = Join-Path $PSScriptRoot 'fixtures/rust-safety-contract.rs'
$libText = Get-Content -LiteralPath (Join-Path $sourceRoot 'lib.rs') -Raw
$previewText = Get-Content -LiteralPath (Join-Path $sourceRoot 'preview.rs') -Raw
$crateLints = @([regex]::Matches($libText, '(?m)^#!\[(?:deny|forbid)\([^\r\n]*\)\]') |
    ForEach-Object Value) -join "`n"
$previewLints = @([regex]::Matches($previewText, '(?m)^#!\[forbid\(unsafe_code\)\]') |
    ForEach-Object Value) -join "`n"
if ($crateLints -notmatch 'unsafe_op_in_unsafe_fn' -or
    $crateLints -notmatch 'clippy::undocumented_unsafe_blocks' -or -not $previewLints) {
    throw 'Actual crate/preview safety lint attributes are missing.'
}

Push-Location -LiteralPath $Root
$fixtureDirectory = $null
try {
    $driverPath = (& rustup which clippy-driver | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $driverPath -PathType Leaf)) {
        throw 'The pinned Rust toolchain must provide clippy-driver.'
    }
    $tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/rust-compiler-contracts'))
    $fixtureDirectory = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $fixtureDirectory -Force)
    $wrapperPath = Join-Path $fixtureDirectory 'contract.rs'
    $normalizedSource = $sourceRoot.Replace('\', '/')
    $normalizedFixture = $fixturePath.Replace('\', '/')
    $prefix = @"
$crateLints
#![allow(dead_code)]
include!("$normalizedSource/abi_generated.rs");
#[path = "$normalizedSource/ffi/common.rs"] mod ffi_common;
#[path = "$normalizedSource/ffi/raster_packet.rs"] mod raster_packet;
"@
    $cases = @(
        @{ Name = 'valid_contract'; Code = $null },
        @{ Name = 'implicit_operation'; Code = 'E0133' },
        @{ Name = 'undocumented_block'; Code = 'clippy::undocumented_unsafe_blocks' },
        @{ Name = 'safe_json_writer'; Code = 'E0133' },
        @{ Name = 'safe_raster_writer'; Code = 'E0133' },
        @{ Name = 'preview_unsafe'; Code = 'unsafe_code' })
    foreach ($case in $cases) {
        $body = if ($case.Name -eq 'preview_unsafe') {
            "mod preview_contract {`n$previewLints`ninclude!(`"$normalizedFixture`");`n}"
        }
        else {
            "include!(`"$normalizedFixture`");"
        }
        # Generated compiler input/output live only in this fresh, bounded temporary directory.
        Set-Content -LiteralPath $wrapperPath -Value ($prefix + "`n" + $body) -Encoding utf8
        $diagnostics = @(& $driverPath $wrapperPath --edition=2021 --crate-type=lib `
            --crate-name=rust_safety_contract --emit=metadata --error-format=json `
            --cfg $case.Name -o (Join-Path $fixtureDirectory 'contract.rmeta') 2>&1)
        $compilerExit = $LASTEXITCODE
        $errors = @($diagnostics | ForEach-Object {
            $diagnostic = $_ | ConvertFrom-Json -ErrorAction Stop
            if ($diagnostic.level -eq 'error') { $diagnostic }
        })
        if (-not $case.Code) {
            if ($compilerExit -ne 0 -or $errors.Count -ne 0) {
                throw "Positive Rust safety fixture failed: $($diagnostics -join "`n")"
            }
        }
        else {
            $matchingErrors = @($errors | Where-Object {
                $_.code.code -eq $case.Code -and @($_.spans | Where-Object {
                    $_.is_primary -and [IO.Path]::GetFullPath($_.file_name) -eq
                        [IO.Path]::GetFullPath($fixturePath)
                }).Count -gt 0
            })
            if ($compilerExit -eq 0 -or $matchingErrors.Count -eq 0) {
                throw "Rust fixture $($case.Name) must fail in the fixture with $($case.Code): $($diagnostics -join "`n")"
            }
        }
    }
    Write-Host 'Rust compiler contracts passed: 1 positive and 5 expected rejections.' -ForegroundColor Green
}
finally {
    if ($fixtureDirectory -and (Test-Path -LiteralPath $fixtureDirectory)) {
        $resolvedFixture = [IO.Path]::GetFullPath($fixtureDirectory)
        if (-not $resolvedFixture.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar,
                [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing cleanup outside the Rust compiler fixture directory.'
        }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
    Pop-Location
}
# Expected compiler rejections must not leak a failure exit code to the parent guard.
$global:LASTEXITCODE = 0
