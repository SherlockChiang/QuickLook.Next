param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [switch]$RequireSamples,
    [switch]$AllowMissingSamples
)

$ErrorActionPreference = "Stop"

$manifestPath = Join-Path $Root "testdata/image-corpus/external/manifest.json"
Write-Host "== image corpus guard ==" -ForegroundColor Cyan
Write-Host "manifest: $manifestPath"

if (-not (Test-Path -LiteralPath $manifestPath)) {
    Write-Host "Missing image corpus manifest" -ForegroundColor Red
    exit 1
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$corpusDir = Split-Path $manifestPath -Parent
$missing = New-Object System.Collections.Generic.List[string]

foreach ($sample in $manifest.samples) {
    $path = Join-Path $corpusDir $sample.file
    if (-not (Test-Path -LiteralPath $path)) {
        $missing.Add("$($sample.id): $($sample.file)")
    }
}

if ($missing.Count -gt 0) {
    Write-Host "missing optional external image samples: $($missing.Count)"
    foreach ($item in $missing) {
        Write-Host " - $item"
    }
    if (-not $AllowMissingSamples) {
        exit 1
    }
}

Write-Host "image corpus guard passed" -ForegroundColor Green

$smoke = Join-Path $PSScriptRoot "smoke-image-corpus.ps1"
if (Test-Path -LiteralPath $smoke) {
    if ($AllowMissingSamples) { & $smoke -Root $Root }
    else { & $smoke -Root $Root -RequireSamples }
}

$capabilities = Join-Path $PSScriptRoot "report-image-capabilities.ps1"
if (Test-Path -LiteralPath $capabilities) {
    & $capabilities -Root $Root
}

$systemSmoke = Join-Path $PSScriptRoot "smoke-system-image-corpus.ps1"
if (Test-Path -LiteralPath $systemSmoke) {
    & $systemSmoke -Root $Root
}
