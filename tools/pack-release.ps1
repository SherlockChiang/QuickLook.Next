# Builds everything in Release and assembles a clean dist\ package.
# The default release path is Rust/App/RasterHost/ParserHost only; legacy .NET plugins are intentionally excluded.
param(
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$ArtifactsDirectory = "",
    [switch]$SkipBuild,
    [switch]$SkipArchive,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent          # ...\QuickLook.Next
$dist = Join-Path $root "dist"
$tfm  = "net10.0-windows10.0.19041.0\win-x64"
$versionFile = Join-Path $root "VERSION"
$artifacts = if ($ArtifactsDirectory) { $ArtifactsDirectory } else { Join-Path $root "artifacts" }

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

if (-not $SkipBuild) {
    Write-Host "== building native (cargo) ==" -ForegroundColor Cyan
    cargo build --release --locked --manifest-path (Join-Path $root "native\quicklook_next_native\Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

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
    if ($LASTEXITCODE -ne 0) { throw "Release solution build failed." }
}

$requiredOutputs = @(
    (Join-Path $root "native\quicklook_next_native\target\release\quicklook_next_native.dll"),
    (Join-Path $root "src\QuickLook.Next.App\bin\Release\$tfm\QuickLook.Next.App.exe"),
    (Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release\$tfm\QuickLook.Next.RasterHost.exe"),
    (Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release\$tfm\QuickLook.Next.ParserHost.exe")
)
foreach ($requiredOutput in $requiredOutputs) {
    if (-not (Test-Path -LiteralPath $requiredOutput -PathType Leaf)) {
        throw "Release build output is missing: $requiredOutput"
    }
}
if ($SkipBuild) {
    $proofPath = Join-Path $root "artifacts\.tested-release-build.json"
    if (-not (Test-Path -LiteralPath $proofPath -PathType Leaf)) {
        throw "No tested release build proof exists. Run tools/release.ps1 before using -SkipBuild."
    }
    $proof = Get-Content -LiteralPath $proofPath -Raw | ConvertFrom-Json
    if ($proof.versionPrefix -ne $VersionPrefix -or $proof.versionSuffix -ne $VersionSuffix) {
        throw "Tested release build version does not match requested package version."
    }
    if ($proof.commit -ne (git -C $root rev-parse HEAD)) {
        throw "Tested release build belongs to a different commit."
    }
    foreach ($property in $proof.outputs.PSObject.Properties) {
        $outputPath = Join-Path $root $property.Name
        $actualHash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
        if ($actualHash -ne $property.Value) {
            throw "Tested release output changed after tests: $($property.Name)"
        }
    }
}

Write-Host "== assembling dist ==" -ForegroundColor Cyan
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force "$dist\RasterHost" | Out-Null
New-Item -ItemType Directory -Force "$dist\ParserHost" | Out-Null

function Copy-Clean($src, $dst) {
    if (-not (Test-Path -LiteralPath $src -PathType Container)) {
        throw "Release build output is missing: $src"
    }
    New-Item -ItemType Directory -Force $dst | Out-Null
    Get-ChildItem -LiteralPath $src -File | Where-Object { $_.Extension -ne ".pdb" } | ForEach-Object { Copy-Item $_.FullName $dst -Force }
    Get-ChildItem -LiteralPath $src -Directory | ForEach-Object { Copy-Item $_.FullName -Destination $dst -Recurse -Force }
}

Copy-Clean (Join-Path $root "src\QuickLook.Next.App\bin\Release\$tfm") $dist
Copy-Clean (Join-Path $root "src\QuickLook.Next.RasterHost\bin\Release\$tfm") "$dist\RasterHost"
Copy-Clean (Join-Path $root "src\QuickLook.Next.ParserHost\bin\Release\$tfm") "$dist\ParserHost"

Write-Host "== pruning unused optional runtime payloads ==" -ForegroundColor Cyan
$optionalPayloadPatterns = @(
    "DirectML.dll",
    "onnxruntime.dll",
    "Microsoft.ML.OnnxRuntime.dll",
    "Microsoft.Windows.AI.*",
    "Microsoft.Windows.Workloads*",
    "Microsoft.Graphics.Imaging*",
    "Microsoft.Graphics.Internal.Imaging*",
    "Microsoft.Graphics.ImagingInternal*",
    "Microsoft.Windows.Vision*",
    "Microsoft.Windows.Internal.Vision*",
    "Microsoft.Windows.ImageCreationInternal*",
    "Microsoft.Windows.Internal.ImageCreation*",
    "Microsoft.Windows.Internal.AI.*",
    "Microsoft.Windows.SemanticSearch*",
    "Microsoft.Windows.Internal.SemanticSearch*",
    "Microsoft.Windows.Private.Workloads*",
    "NPUDetect.dll",
    "PerceptiveStreaming.dll",
    "SessionHandleIPCProxyStub.dll",
    "System.Numerics.Tensors.dll",
    "workloads*.json"
)
foreach ($pattern in $optionalPayloadPatterns) {
    Get-ChildItem -LiteralPath $dist -Filter $pattern -File -ErrorAction SilentlyContinue |
        Remove-Item -Force
}

$retainedLocaleDirectories = @("en-US", "en-us", "zh-CN")
Get-ChildItem -LiteralPath $dist -Directory | Where-Object {
    Test-Path -LiteralPath (Join-Path $_.FullName "Microsoft.ui.xaml.dll.mui") -PathType Leaf
} | Where-Object {
    $retainedLocaleDirectories -notcontains $_.Name
} | Remove-Item -Recurse -Force

& (Join-Path $PSScriptRoot "guard-architecture.ps1") -Root $root -DistDir $dist -SkipSystemImageSmoke:$SkipSystemImageSmoke

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
