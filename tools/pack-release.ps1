# Builds everything in Release and assembles a clean dist\ package.
# The default release path is Rust/App/RasterHost/ParserHost/ShellBroker only; legacy .NET plugins are intentionally excluded.
param(
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$PackageIdentityName = "",
    [string]$ArtifactsDirectory = "",
    [switch]$SkipBuild,
    [switch]$SkipArchive,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")
. (Join-Path $PSScriptRoot "release-payload.ps1")
$root = Split-Path $PSScriptRoot -Parent          # ...\QuickLook.Next
$dist = Join-Path $root "dist"
$tfm  = "net10.0-windows10.0.19041.0\win-x64"
$versionFile = Join-Path $root "VERSION"
$artifacts = if ($ArtifactsDirectory) { $ArtifactsDirectory } else { Join-Path $root "artifacts" }
$nativeProject = Join-Path $root "native\QuickLook.Next.Native.proj"
$nativeDll = Join-Path $root "native\target\x86_64-pc-windows-msvc\release\quicklook_next_native.dll"

$globalJsonPath = Join-Path $root "global.json"
$requiredSdk = (Get-Content -LiteralPath $globalJsonPath -Raw | ConvertFrom-Json).sdk.version
$installedSdks = @(dotnet --list-sdks 2>$null | ForEach-Object { ($_ -split '\s+')[0] })
if ($LASTEXITCODE -ne 0) { throw "Could not enumerate installed .NET SDKs." }
if ($installedSdks -notcontains $requiredSdk) {
    throw "Release packaging requires .NET SDK $requiredSdk from global.json. Installed SDKs: $($installedSdks -join ', '). Install $requiredSdk before packaging; release builds do not roll forward to another SDK."
}

if (-not $VersionPrefix -and (Test-Path $versionFile)) {
    $VersionPrefix = (Get-Content -LiteralPath $versionFile -Raw).Trim()
}

if ($VersionPrefix -and $VersionPrefix -notmatch '^\d+\.\d+\.\d+$') {
    throw "VersionPrefix must use semantic X.Y.Z format. Current value: '$VersionPrefix'"
}
if ($VersionSuffix -and
    $VersionSuffix -notmatch '^[0-9A-Za-z](?:[0-9A-Za-z.-]{0,63})$')
{
    throw "VersionSuffix must be a short SemVer-compatible identifier."
}

if (-not $SkipBuild) {
    Write-Host "== building native (MSBuild/Cargo, win-x64) ==" -ForegroundColor Cyan
    dotnet msbuild $nativeProject -target:Build -verbosity:minimal
    if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

    Write-Host "== cleaning renamed RasterHost output ==" -ForegroundColor Cyan
    $rasterHostRelease = Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release"
    if (Test-Path $rasterHostRelease) { Remove-Item $rasterHostRelease -Recurse -Force }
    $parserHostRelease = Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release"
    if (Test-Path $parserHostRelease) { Remove-Item $parserHostRelease -Recurse -Force }
    $shellBrokerRelease = Join-Path $root "src\QuickLook.Next.ShellBroker\bin\Release"
    if (Test-Path $shellBrokerRelease) { Remove-Item $shellBrokerRelease -Recurse -Force }

    Write-Host "== building solution (Release) ==" -ForegroundColor Cyan
    $buildArgs = @("build", (Join-Path $root "QuickLook.Next.slnx"), "-c", "Release", "--no-restore")
    if ($VersionPrefix) {
        $buildArgs += "/p:VersionPrefix=$VersionPrefix"
    }
    if ($VersionSuffix) {
        $buildArgs += "/p:VersionSuffix=$VersionSuffix"
    }
    if ($PackageIdentityName) {
        if ($PackageIdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') {
            throw "PackageIdentityName must contain only identity-safe characters."
        }
        $buildArgs += "/p:ProjectPriIndexName=$PackageIdentityName"
    }
    dotnet @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "Release solution build failed." }
}

$requiredOutputs = @(
    $nativeDll,
    (Join-Path $root "src\QuickLook.Next.App\bin\Release\$tfm\QuickLook.Next.App.exe"),
    (Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release\$tfm\QuickLook.Next.RasterHost.exe"),
    (Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release\$tfm\QuickLook.Next.ParserHost.exe"),
    (Join-Path $root "src\QuickLook.Next.ShellBroker\bin\Release\$tfm\QuickLook.Next.ShellBroker.exe")
)
foreach ($requiredOutput in $requiredOutputs) {
    if (-not (Test-Path -LiteralPath $requiredOutput -PathType Leaf)) {
        throw "Release build output is missing: $requiredOutput"
    }
}

$noticePath = Join-Path $artifacts "THIRD-PARTY-NOTICES.txt"
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "new-third-party-notices.ps1") `
    -Arguments @{
        Root = $root
        OutputPath = $noticePath
    } `
    -FailureMessage "Third-party notice generation failed"
$payload = @(
    Get-QuickLookReleasePayload `
        -Root $root `
        -ArtifactsDirectory $artifacts)
$payloadHashesForStage = $null
if ($SkipBuild) {
    $proofPath = Join-Path $root "artifacts\.tested-release-build.json"
    if (-not (Test-Path -LiteralPath $proofPath -PathType Leaf)) {
        throw "No tested release build proof exists. Run tools/release.ps1 before using -SkipBuild."
    }
    $proof = Get-Content -LiteralPath $proofPath -Raw | ConvertFrom-Json
    if ($proof.payloadSchemaVersion -ne 1) {
        throw "Tested release build proof uses an unsupported payload schema."
    }
    if ($proof.versionPrefix -ne $VersionPrefix -or $proof.versionSuffix -ne $VersionSuffix) {
        throw "Tested release build version does not match requested package version."
    }
    $currentCommit = @(git -C $root rev-parse HEAD)
    if ($LASTEXITCODE -ne 0 -or -not $currentCommit) {
        throw "Could not resolve the current source commit."
    }
    if ($proof.commit -ne $currentCommit[-1].Trim()) {
        throw "Tested release build belongs to a different commit."
    }
    Assert-QuickLookReleasePayloadProof `
        -Payload $payload `
        -ProofOutputs $proof.outputs
    $payloadHashesForStage = $proof.outputs
}
else {
    $payloadHashesForStage = New-QuickLookReleasePayloadHashes `
        -Payload $payload
}

Write-Host "== assembling dist ==" -ForegroundColor Cyan
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
Copy-QuickLookReleasePayload `
    -Payload $payload `
    -DestinationRoot $dist
Assert-QuickLookReleasePayloadProof `
    -Payload $payload `
    -ProofOutputs $payloadHashesForStage `
    -ContentRoot $dist

Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "guard-architecture.ps1") `
    -Arguments @{
        Root = $root
        DistDir = $dist
        SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
    } `
    -FailureMessage "Packaged release architecture guard failed"

$size = [math]::Round(((Get-ChildItem $dist -Recurse | Measure-Object Length -Sum).Sum / 1MB))
$packageVersion = if ($VersionPrefix) { $VersionPrefix } else { "dev" }
if ($VersionSuffix) { $packageVersion = "$packageVersion-$VersionSuffix" }
$archiveName = "QuickLook.Next-$packageVersion-win-x64.zip"
$archivePath = Join-Path $artifacts $archiveName
$checksumPath = "$archivePath.sha256"

if (-not $SkipArchive) {
    Write-Host "== creating release archive ==" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force $artifacts | Out-Null
    if (Test-Path $archivePath) { Remove-Item $archivePath -Force }
    if (Test-Path $checksumPath) { Remove-Item $checksumPath -Force }
    Compress-Archive -Path (Join-Path $dist "*") -DestinationPath $archivePath -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $archiveName" | Set-Content -LiteralPath $checksumPath -Encoding ascii
    Write-Host "== done: $archivePath ($size MB unpacked) ==" -ForegroundColor Green
}
else {
    Write-Host "== dist ready ($size MB unpacked) ==" -ForegroundColor Green
}
