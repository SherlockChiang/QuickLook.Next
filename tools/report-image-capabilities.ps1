param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$path = Join-Path $Root "testdata/image-corpus/capabilities.json"
Write-Host "== image capabilities ==" -ForegroundColor Cyan
Write-Host "source: $path"

if (-not (Test-Path -LiteralPath $path)) {
    Write-Host "Missing image capabilities file" -ForegroundColor Red
    exit 1
}

$capabilities = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
foreach ($format in $capabilities.formats) {
    $native = if ($format.nativeDecode) { "native" } else { "no-native" }
    $animation = if ($format.nativeAnimation) { "native-animation" } else { "no-native-animation" }
    $system = if ($format.systemDecode) { "system" } else { "no-system" }
    Write-Host "$($format.extension): $native, $animation, $system"
}

Write-Host "image capabilities report passed" -ForegroundColor Green
