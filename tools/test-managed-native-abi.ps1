param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'checked-invocation.ps1')
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot 'generate-native-abi.ps1') `
    -Arguments @{ Root = $Root; Check = $true }
. (Join-Path $PSScriptRoot 'managed-native-abi.ps1')
$schema = Get-Content -LiteralPath (Join-Path $Root 'native/abi-contract.json') -Raw | ConvertFrom-Json
$frameworks = @{
    'QuickLook.Next.App' = 'net10.0-windows10.0.19041.0'
    'QuickLook.Next.ParserHost' = 'net10.0-windows'
    'QuickLook.Next.RasterHost' = 'net10.0-windows10.0.19041.0'
    'QuickLook.Next.ShellBroker' = 'net10.0-windows10.0.19041.0'
}
$results = @(foreach ($name in @($frameworks.Keys | Sort-Object)) {
    $path = Join-Path $Root "src/$name/bin/Release/$($frameworks[$name])/win-x64/$name.dll"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Build Release before inspecting managed ABI metadata: $path" }
    Assert-ManagedNativeAbi -Schema $schema -AssemblyPath $path
})
Write-Host ("Managed ABI metadata passed: $($results.Count) assemblies; " +
    "$((($results | Measure-Object Imports -Sum).Sum)) imports; " +
    "$((($results | Measure-Object Callbacks -Sum).Sum)) callback definitions; no inspected assemblies loaded.") -ForegroundColor Green
