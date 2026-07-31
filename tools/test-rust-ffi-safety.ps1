param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$sourcePath = Join-Path $Root "native\quicklook_next_native\src\lib.rs"
$sharedSafetyPath = Join-Path (
    $Root) "native\quicklook_next_native\src\ffi_pointer_safety.md"
foreach ($path in @($sourcePath, $sharedSafetyPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Rust FFI safety input is missing: $path"
    }
}

$sharedSafety = Get-Content -LiteralPath $sharedSafetyPath -Raw
if ($sharedSafety -notmatch '(?m)^# Safety\s*$' -or
    $sharedSafety -notmatch 'readable' -or
    $sharedSafety -notmatch 'writable') {
    throw "The shared raw-pointer contract must document readable and writable buffers."
}

$lines = @(Get-Content -LiteralPath $sourcePath)
$failures = [Collections.Generic.List[string]]::new()
$rawPointerExports = 0

for ($lineIndex = 0; $lineIndex -lt $lines.Count; $lineIndex++) {
    if ($lines[$lineIndex].Trim() -ne "#[no_mangle]") {
        continue
    }

    $signatureLines = [Collections.Generic.List[string]]::new()
    for ($signatureIndex = $lineIndex + 1;
        $signatureIndex -lt $lines.Count;
        $signatureIndex++) {
        $signatureLines.Add($lines[$signatureIndex])
        if ($lines[$signatureIndex] -match '\{') {
            break
        }
    }
    $signature = $signatureLines -join "`n"
    if ($signature -notmatch
            'pub\s+(?:unsafe\s+)?extern\s+"(?:C|system)"\s+fn\s+([A-Za-z0-9_]+)') {
        continue
    }
    $exportName = $Matches[1]
    if ($signature -notmatch '\*(?:const|mut)\s+') {
        continue
    }

    $rawPointerExports++
    if ($signature -notmatch
            'pub\s+unsafe\s+extern\s+"(?:C|system)"\s+fn') {
        $failures.Add(
            "$exportName accepts raw pointers but is not an unsafe export.")
    }

    $documentation = [Collections.Generic.List[string]]::new()
    for ($docIndex = $lineIndex - 1; $docIndex -ge 0; $docIndex--) {
        $trimmed = $lines[$docIndex].Trim()
        if ($trimmed.StartsWith("///", [StringComparison]::Ordinal) -or
            $trimmed.StartsWith("#[doc", [StringComparison]::Ordinal)) {
            $documentation.Add($trimmed)
            continue
        }
        break
    }
    $documentationText = $documentation -join "`n"
    if ($documentationText -notmatch
            '# Safety|ffi_pointer_safety\.md') {
        $failures.Add(
            "$exportName is missing an explicit Rustdoc # Safety contract.")
    }
}

if ($rawPointerExports -eq 0) {
    $failures.Add("No raw-pointer exports were found; the guard did not inspect the ABI.")
}

if ($failures.Count -gt 0) {
    Write-Host "Rust FFI safety guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host (
    "Rust FFI safety guard passed; raw-pointer exports=$rawPointerExports") `
    -ForegroundColor Green
