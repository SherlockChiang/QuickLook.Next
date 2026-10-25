[CmdletBinding()]
param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path -LiteralPath $Root).Path
$sdkVersion = (Get-Content -LiteralPath (Join-Path $Root 'global.json') -Raw | ConvertFrom-Json).sdk.version
$rustConfig = Get-Content -LiteralPath (Join-Path $Root 'rust-toolchain.toml') -Raw
$rustVersion = [regex]::Match($rustConfig, 'channel\s*=\s*"([^"]+)"').Groups[1].Value
$checks = [Collections.Generic.List[object]]::new()
function Add-Check([string]$Name, [bool]$Passed, [string]$Detail) {
    $checks.Add([pscustomobject]@{ Check = $Name; Passed = $Passed; Detail = $Detail })
}

Add-Check 'PowerShell' ($PSVersionTable.PSVersion.Major -ge 7) $PSVersionTable.PSVersion.ToString()
$localDotnet = Join-Path $Root "artifacts/tools/dotnet-$sdkVersion/dotnet.exe"
$dotnet = if (Test-Path -LiteralPath $localDotnet -PathType Leaf) { $localDotnet } else {
    (Get-Command dotnet -ErrorAction SilentlyContinue).Source
}
$sdkFound = $false
if ($dotnet) {
    $sdks = @(& $dotnet --list-sdks)
    $sdkFound = $LASTEXITCODE -eq 0 -and @($sdks | Where-Object { $_ -match ('^' + [regex]::Escape($sdkVersion) + '\s') }).Count -gt 0
}
Add-Check '.NET SDK' $sdkFound $(if ($sdkFound) { "$sdkVersion at $dotnet" } else { "Missing pinned SDK $sdkVersion; install it or restore artifacts/tools/dotnet-$sdkVersion. global.json remains unchanged." })

$rustup = Get-Command rustup -ErrorAction SilentlyContinue
$rustFound = $false
if ($rustup) {
    $toolchains = @(& $rustup.Source toolchain list)
    $rustFound = $LASTEXITCODE -eq 0 -and @($toolchains | Where-Object { $_ -match ('^' + [regex]::Escape($rustVersion) + '-x86_64-pc-windows-msvc') }).Count -gt 0
}
Add-Check 'Rust MSVC toolchain' $rustFound $rustVersion
$programFilesX86 = [Environment]::GetFolderPath('ProgramFilesX86')
$vswhere = Join-Path $programFilesX86 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsInstall = @(if (Test-Path -LiteralPath $vswhere) {
    & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
})
Add-Check 'MSVC x64 tools' ($vsInstall.Count -gt 0) ($vsInstall -join ', ')
$sdkLibRoot = Join-Path $programFilesX86 'Windows Kits/10/Lib'
$windowsSdks = @(Get-ChildItem -LiteralPath $sdkLibRoot -Directory -ErrorAction SilentlyContinue | Where-Object {
    Test-Path -LiteralPath (Join-Path $_.FullName 'um/x64/kernel32.lib')
})
Add-Check 'Windows SDK libraries' ($windowsSdks.Count -gt 0) (($windowsSdks | ForEach-Object Name) -join ', ')
$checks | Format-Table -AutoSize -Wrap
if ($sdkFound -and $dotnet -eq $localDotnet) {
    Write-Host 'Select the local SDK using the process-scoped environment setup in docs/verification-baseline.md (Pinned Local Toolchain).'
}
if (@($checks | Where-Object { -not $_.Passed }).Count -gt 0) { exit 1 }
Write-Host 'Development prerequisites are available. No downloads, installs, or settings changes were performed.'
