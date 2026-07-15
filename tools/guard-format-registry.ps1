param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = "Stop"
$failures = [System.Collections.Generic.List[string]]::new()
$registry = Get-Content -LiteralPath (Join-Path $Root "docs/format-registry.json") -Raw | ConvertFrom-Json

function Require-Pattern([string]$path, [string]$pattern, [string]$message) {
    $text = Get-Content -LiteralPath $path -Raw
    if ($text -notmatch $pattern) { $script:failures.Add($message) }
}

$abi = [int]$registry.nativeAbiVersion
$nativeLibrary = Join-Path $Root "native/quicklook_next_native/src/lib.rs"
Require-Pattern $nativeLibrary "QL_NATIVE_ABI_VERSION:\s*u32\s*=\s*$abi" `
    "Rust native ABI does not match format-registry.json."
Require-Pattern (Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs") "Version\s*=\s*$abi" `
    "Managed native ABI does not match format-registry.json."

$nativeText = Get-Content -LiteralPath $nativeLibrary -Raw
foreach ($kind in $registry.probeKinds) {
    if ($nativeText -notmatch [regex]::Escape('"' + $kind + '"')) {
        $failures.Add("Native probe kind missing: $kind")
    }
}

$policy = Get-Content -LiteralPath (Join-Path $Root "src/QuickLook.Next.Core/PreviewFormatPolicy.cs") -Raw
foreach ($kind in $registry.parserHostKinds) {
    if ($policy -notmatch [regex]::Escape('"' + $kind + '"')) {
        $failures.Add("ParserHost kind missing from managed policy: $kind")
    }
}

$fallback = Get-Content -LiteralPath (Join-Path $Root "src/QuickLook.Next.Core/FallbackFileProbe.cs") -Raw
foreach ($kind in @("disk-image", "font", "database", "mail", "chm", "dump", "elf")) {
    if ($fallback -notmatch [regex]::Escape('"' + $kind + '"')) {
        $failures.Add("Metadata-only fallback kind missing: $kind")
    }
}

$rasterProgram = Join-Path $Root "src/QuickLook.Next.RasterHost/Program.cs"
$nativeDecoder = Join-Path $Root "src/QuickLook.Next.RasterHost/NativeImageDecoder.cs"
foreach ($extension in $registry.imageAliases.jpeg) {
    Require-Pattern $rasterProgram ([regex]::Escape('"' + $extension + '"')) "RasterHost system preference omits JPEG alias $extension."
    Require-Pattern $nativeDecoder ([regex]::Escape('"' + $extension + '"')) "Native decoder policy omits JPEG alias $extension."
}

$codecPolicy = Get-Content -LiteralPath (Join-Path $Root "src/QuickLook.Next.Core/ImageCodecPolicy.cs") -Raw
foreach ($extension in $registry.systemRequiredImages) {
    if ($codecPolicy -notmatch [regex]::Escape('"' + $extension + '"')) {
        $failures.Add("System-required image missing from Core policy: $extension")
    }
}

Require-Pattern (Join-Path $Root "native/quicklook_next_native/src/preview.rs") '"docx"\s*\|\s*"docm"\s*=>\s*render_docx' `
    "DOCM must share the bounded DOCX parser."

if ($failures.Count -gt 0) {
    Write-Host "Format registry guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) { Write-Host " - $failure" -ForegroundColor Red }
    exit 1
}
Write-Host "format registry guard passed" -ForegroundColor Green
