param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [switch]$RequireSamples
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
    if ($RequireSamples) {
        exit 1
    }
}

Write-Host "image corpus guard passed" -ForegroundColor Green
