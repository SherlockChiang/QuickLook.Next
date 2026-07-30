[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Version = "",
    [ValidateSet("None", "Patch", "Minor", "Major")]
    [string]$Bump = "None",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [string]$VersionSuffix = "",
    [switch]$NoRestore,
    [switch]$Test,
    [switch]$Install,
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($VersionSuffix -and $VersionSuffix -notmatch '^[0-9A-Za-z](?:[0-9A-Za-z.-]{0,63})$') {
    throw "VersionSuffix must be a short SemVer-compatible identifier."
}
if ($Install -and $Configuration -ne "Release") {
    throw "Install requires a Release build."
}
if ($Install -and $VersionSuffix) {
    throw "Install does not support VersionSuffix; use a numeric version bump."
}
if ($Install -and -not $Test) {
    Write-Host (
        "Install requested: enabling Rust and .NET tests before signing.") `
        -ForegroundColor DarkGray
    $Test = $true
}

$setVersionArgs = @{
    Root = $Root
    Bump = $Bump
}
if ($Version) {
    $setVersionArgs.Version = $Version
}
$resolvedVersion = & (Join-Path $PSScriptRoot "set-version.ps1") @setVersionArgs
if (-not $resolvedVersion) {
    throw "The synchronized build version could not be resolved."
}
$resolvedVersion = @($resolvedVersion)[-1]

& (Join-Path $PSScriptRoot "test-release-version.ps1") `
    -Root $Root `
    -ExpectedVersion $resolvedVersion

$solution = Join-Path $Root "QuickLook.Next.slnx"
$nativeManifest = Join-Path $Root "native\Cargo.toml"
if (-not $NoRestore) {
    Write-Host "== restore locked .NET dependencies ==" -ForegroundColor Cyan
    dotnet restore $solution --locked-mode --disable-build-servers
    if ($LASTEXITCODE -ne 0) { throw "Dependency restore failed." }
}

if ($Test) {
    Write-Host "== test Rust workspace (Release) ==" -ForegroundColor Cyan
    cargo test --workspace --release --locked --manifest-path $nativeManifest
    if ($LASTEXITCODE -ne 0) { throw "Native tests failed." }
}

# Every .NET configuration stages native\target\release\quicklook_next_native.dll.
Write-Host "== build Rust workspace (Release) ==" -ForegroundColor Cyan
cargo build --workspace --release --locked --manifest-path $nativeManifest
if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

$versionProperties = @("/p:VersionPrefix=$resolvedVersion")
if ($VersionSuffix) {
    $versionProperties += "/p:VersionSuffix=$VersionSuffix"
}

Write-Host "== build .NET solution ($Configuration) ==" -ForegroundColor Cyan
dotnet build $solution -c $Configuration --no-restore `
    --disable-build-servers @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution build failed." }

if ($Test) {
    Write-Host "== test .NET solution ($Configuration) ==" -ForegroundColor Cyan
    # The integration projects each launch restricted host processes. Serialize projects so
    # their independent hard timeouts measure product behavior rather than cross-project load.
    dotnet test $solution -c $Configuration --no-build --no-restore `
        --disable-build-servers --maxcpucount:1 @versionProperties
    if ($LASTEXITCODE -ne 0) { throw "Solution tests failed." }
}

$installedPackageVersion = ""
if ($Install) {
    & (Join-Path $PSScriptRoot "write-tested-release-proof.ps1") `
        -Root $Root `
        -VersionPrefix $resolvedVersion `
        -VersionSuffix $VersionSuffix | Out-Null
    $installedPackageVersion = @(
        & (Join-Path $PSScriptRoot "update-local-msix.ps1") `
            -Root $Root `
            -VersionPrefix $resolvedVersion
    )[-1]
}

$tfm = "net10.0-windows10.0.19041.0\win-x64"
$appPath = Join-Path $Root "src\QuickLook.Next.App\bin\$Configuration\$tfm\QuickLook.Next.App.exe"
if (-not (Test-Path -LiteralPath $appPath -PathType Leaf)) {
    throw "Build completed without the expected App executable: $appPath"
}

$displayVersion = if ($VersionSuffix) {
    "$resolvedVersion-$VersionSuffix"
} else {
    $resolvedVersion
}
Write-Host ""
Write-Host "Local build ready: QuickLook Next $displayVersion" -ForegroundColor Green
Write-Host "App: $appPath" -ForegroundColor Green
if ($Install) {
    Write-Host "Installed MSIX: $installedPackageVersion" -ForegroundColor Green
}
else {
    Write-Host (
        "The installed MSIX was not changed; pass -Install to update it.") `
        -ForegroundColor DarkGray
}
Write-Host "Use tools/release.ps1 for formal release packaging." `
    -ForegroundColor DarkGray
