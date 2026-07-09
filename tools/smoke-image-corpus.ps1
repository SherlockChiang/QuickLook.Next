param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [switch]$RequireSamples
)

$ErrorActionPreference = "Stop"

function Read-Bytes([string]$Path, [int]$Count) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $buffer = New-Object byte[] $Count
        $read = $stream.Read($buffer, 0, $Count)
        if ($read -lt $Count) { return $buffer[0..($read - 1)] }
        return $buffer
    }
    finally { $stream.Dispose() }
}

function Test-Magic([string]$Path, [string]$Id) {
    $bytes = Read-Bytes $Path 32
    switch ($Id) {
        { $_ -like "jpeg-*" } { return $bytes.Length -ge 2 -and $bytes[0] -eq 0xFF -and $bytes[1] -eq 0xD8 }
        "avif-still" { return [Text.Encoding]::ASCII.GetString($bytes) -match 'ftyp(?:avif|avis)' }
        "heic-still" { return [Text.Encoding]::ASCII.GetString($bytes) -match 'ftyp(?:heic|heix|hevc|hevx|mif1|msf1)' }
        "jxl-still" { return ($bytes.Length -ge 2 -and $bytes[0] -eq 0xFF -and $bytes[1] -eq 0x0A) -or ([Text.Encoding]::ASCII.GetString($bytes) -match 'JXL ' ) }
        "webp-animated" {
            $all = [System.IO.File]::ReadAllBytes($Path)
            $text = [Text.Encoding]::ASCII.GetString($all)
            return $text.StartsWith("RIFF") -and $text.Contains("WEBP") -and $text.Contains("ANIM")
        }
        default { return $true }
    }
}

$manifestPath = Join-Path $Root "testdata/image-corpus/external/manifest.json"
Write-Host "== image corpus smoke ==" -ForegroundColor Cyan
Write-Host "manifest: $manifestPath"

if (-not (Test-Path -LiteralPath $manifestPath)) {
    Write-Host "Missing image corpus manifest" -ForegroundColor Red
    exit 1
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$corpusDir = Split-Path $manifestPath -Parent
$failures = New-Object System.Collections.Generic.List[string]
$checked = 0

foreach ($sample in $manifest.samples) {
    $path = Join-Path $corpusDir $sample.file
    if (-not (Test-Path -LiteralPath $path)) {
        if ($RequireSamples) { $failures.Add("missing sample: $($sample.id) -> $($sample.file)") }
        continue
    }
    $checked++
    if (-not (Test-Magic $path $sample.id)) {
        $failures.Add("sample failed magic check: $($sample.id) -> $($sample.file)")
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Image corpus smoke failed:" -ForegroundColor Red
    foreach ($failure in $failures) { Write-Host " - $failure" -ForegroundColor Red }
    exit 1
}

Write-Host "image corpus smoke passed; checked=$checked" -ForegroundColor Green
