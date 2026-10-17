param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DllPath = ''
)

$ErrorActionPreference = 'Stop'
if (-not $DllPath) {
    $DllPath = Join-Path $Root 'native/target/x86_64-pc-windows-msvc/release/quicklook_next_native.dll'
}
. (Join-Path $PSScriptRoot 'checked-invocation.ps1')
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot 'generate-native-abi.ps1') `
    -Arguments @{ Root = $Root; Check = $true }
if (-not ('NativePeExports' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'native-pe-exports.cs')
}
if (-not (Test-Path -LiteralPath $DllPath -PathType Leaf)) {
    throw "Build the native DLL through MSBuild before inspecting exports: $DllPath"
}
$schema = Get-Content -LiteralPath (Join-Path $Root 'native/abi-contract.json') -Raw | ConvertFrom-Json
$expected = @($schema.exports | ForEach-Object name)
$lock = Get-Content -LiteralPath (Join-Path $Root 'native/Cargo.lock') -Raw
foreach ($dependency in $schema.dependencyExports) {
    $pin = '(?m)^name = "' + [regex]::Escape($dependency.package) + '"\r?\nversion = "' +
        [regex]::Escape($dependency.version) + '"\r?$'
    if ($lock -notmatch $pin) {
        throw "Incidental exports no longer match the locked dependency: $($dependency.package) $($dependency.version)"
    }
    $expected += @($dependency.names)
}
$stream = [IO.File]::OpenRead($DllPath)
try { $actual = [NativePeExports]::Read($stream) }
finally { $stream.Dispose() }
$missing = @($expected | Where-Object { $actual -cnotcontains $_ })
$extra = @($actual | Where-Object { $expected -cnotcontains $_ })
if ($missing.Count -or $extra.Count -or $actual.Count -ne $expected.Count) {
    throw "Native DLL export drift. Missing: $($missing -join ', '); extra: $($extra -join ', ')"
}
Write-Host ("Native win-x64 DLL exports match schema: $($schema.exports.Count) project + " +
    "$($actual.Count - $schema.exports.Count) incidental dependency symbols; inspected without loading.") -ForegroundColor Green
