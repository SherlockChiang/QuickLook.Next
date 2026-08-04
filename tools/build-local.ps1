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
    [switch]$Package,
    [switch]$Install,
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "checked-invocation.ps1")

if ($VersionSuffix -and $VersionSuffix -notmatch '^[0-9A-Za-z](?:[0-9A-Za-z.-]{0,63})$') {
    throw "VersionSuffix must be a short SemVer-compatible identifier."
}
$packageRequested = $Package -or $Install
if ($packageRequested -and $Configuration -ne "Release") {
    throw "Package and Install require a Release build."
}
if ($packageRequested -and $VersionSuffix) {
    throw "Package and Install do not support VersionSuffix; use a numeric version bump."
}
$localAction = if ($Install) { "Install" } else { "Package" }
if ($packageRequested -and -not $Test) {
    Write-Host (
        "$localAction requested: enabling Rust and .NET tests before signing.") `
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
$resolvedVersion = @(
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "set-version.ps1") `
        -Arguments $setVersionArgs `
        -FailureMessage "Local version synchronization failed"
)[-1]
if (-not $resolvedVersion) {
    throw "The synchronized build version could not be resolved."
}
$resolvedVersion = @($resolvedVersion)[-1]

Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "test-release-version.ps1") `
    -Arguments @{
        Root = $Root
        ExpectedVersion = $resolvedVersion
    } `
    -FailureMessage "Local release version validation failed"

$solution = Join-Path $Root "QuickLook.Next.slnx"
$nativeManifest = Join-Path $Root "native\Cargo.toml"
$nativeProject = Join-Path $Root "native\QuickLook.Next.Native.proj"
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

# Every .NET configuration stages the pinned win-x64 Cargo output.
Write-Host "== build Rust workspace (Release, win-x64) ==" -ForegroundColor Cyan
dotnet msbuild $nativeProject -target:Build -verbosity:minimal
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

$localPackageVersion = ""
if ($packageRequested) {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "write-tested-release-proof.ps1") `
        -Arguments @{
            Root = $Root
            VersionPrefix = $resolvedVersion
            VersionSuffix = $VersionSuffix
        } `
        -FailureMessage "Writing the local tested-build proof failed" | Out-Null
    $localPackageVersion = @(
        Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "update-local-msix.ps1") `
            -Arguments @{
                Root = $Root
                VersionPrefix = $resolvedVersion
                PackageOnly = -not [bool]$Install
            } `
            -FailureMessage "Local MSIX update failed"
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
    Write-Host "Installed MSIX: $localPackageVersion" -ForegroundColor Green
}
elseif ($Package) {
    $msixPath = Join-Path (
        $Root) "artifacts\QuickLook.Next-$localPackageVersion-win-x64.msix"
    $installerPath = Join-Path (
        $Root) "artifacts\QuickLook.Next-Installer-$localPackageVersion-win-x64.zip"
    Write-Host "MSIX version: $localPackageVersion" -ForegroundColor Green
    Write-Host "MSIX: $msixPath" -ForegroundColor Green
    Write-Host "Installer: $installerPath" -ForegroundColor Green
}
else {
    Write-Host (
        "No MSIX was created; pass -Package to package or -Install to update it.") `
        -ForegroundColor DarkGray
}
Write-Host "Use tools/release.ps1 for formal release packaging." `
    -ForegroundColor DarkGray
