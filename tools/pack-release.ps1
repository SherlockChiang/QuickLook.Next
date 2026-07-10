# Builds everything in Release and assembles a clean dist\ package.
# The default release path is Rust/App/RasterHost/ParserHost only; legacy .NET plugins are intentionally excluded.
param(
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent          # ...\QuickLook.Next
$dist = Join-Path $root "dist"
$tfm  = "net10.0-windows10.0.19041.0\win-x64"
$versionFile = Join-Path $root "VERSION"

if (-not $VersionPrefix -and (Test-Path $versionFile)) {
    $VersionPrefix = (Get-Content -LiteralPath $versionFile -Raw).Trim()
}

if ($VersionPrefix -and $VersionPrefix -notmatch '^\d+\.\d+\.\d+$') {
    throw "VersionPrefix must use semantic X.Y.Z format. Current value: '$VersionPrefix'"
}

Write-Host "== building native (cargo) ==" -ForegroundColor Cyan
cargo build --release --locked --manifest-path (Join-Path $root "native\quicklook_next_native\Cargo.toml")

Write-Host "== cleaning renamed RasterHost output ==" -ForegroundColor Cyan
$rasterHostRelease = Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release"
if (Test-Path $rasterHostRelease) { Remove-Item $rasterHostRelease -Recurse -Force }
$parserHostRelease = Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release"
if (Test-Path $parserHostRelease) { Remove-Item $parserHostRelease -Recurse -Force }

Write-Host "== building solution (Release) ==" -ForegroundColor Cyan
$buildArgs = @("build", (Join-Path $root "QuickLook.Next.slnx"), "-c", "Release", "--no-restore")
if ($VersionPrefix) {
    $buildArgs += "/p:VersionPrefix=$VersionPrefix"
}
if ($VersionSuffix) {
    $buildArgs += "/p:VersionSuffix=$VersionSuffix"
}
dotnet @buildArgs

Write-Host "== assembling dist ==" -ForegroundColor Cyan
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force "$dist\RasterHost" | Out-Null
New-Item -ItemType Directory -Force "$dist\ParserHost" | Out-Null

function Copy-Clean($src, $dst) {
    New-Item -ItemType Directory -Force $dst | Out-Null
    Get-ChildItem $src -File | Where-Object { $_.Extension -ne ".pdb" } | ForEach-Object { Copy-Item $_.FullName $dst -Force }
    Get-ChildItem $src -Directory | ForEach-Object { Copy-Item $_.FullName -Destination $dst -Recurse -Force }
}

Copy-Clean (Join-Path $root "src\QuickLook.Next.App\bin\Release\$tfm") $dist
Copy-Clean (Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release\$tfm") "$dist\RasterHost"
Copy-Clean (Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release\$tfm") "$dist\ParserHost"

& (Join-Path $PSScriptRoot "guard-architecture.ps1") -Root $root -DistDir $dist

$size = [math]::Round(((Get-ChildItem $dist -Recurse | Measure-Object Length -Sum).Sum / 1MB))
Write-Host "== done: $dist ($size MB) ==" -ForegroundColor Green
