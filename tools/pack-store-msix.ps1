param(
    [Parameter(Mandatory = $true)]
    [string]$PackageIdentityName,
    [Parameter(Mandatory = $true)]
    [string]$Publisher,
    [Parameter(Mandatory = $true)]
    [string]$PublisherDisplayName,
    [string]$VersionPrefix = "",
    [string]$StoreVersion = "",
    [switch]$SkipBuild,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem
. (Join-Path $PSScriptRoot "checked-invocation.ps1")
. (Join-Path $PSScriptRoot "release-payload.ps1")

$root = Split-Path $PSScriptRoot -Parent
$versionFile = Join-Path $root "VERSION"
if (-not $VersionPrefix) {
    $VersionPrefix = (Get-Content -LiteralPath $versionFile -Raw).Trim()
}
if ($VersionPrefix -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') {
    throw "VersionPrefix must use strict X.Y.Z format."
}
if ($PackageIdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') {
    throw "PackageIdentityName must contain only identity-safe characters."
}
if ([string]::IsNullOrWhiteSpace($Publisher) -or
    [string]::IsNullOrWhiteSpace($PublisherDisplayName)) {
    throw "Publisher and PublisherDisplayName must not be empty."
}

$resolvedStoreVersion = @(
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "resolve-store-msix-version.ps1") `
        -Arguments @{ VersionPrefix = $VersionPrefix } `
        -FailureMessage "Store MSIX version resolution failed"
)[-1]
if ($StoreVersion -and $StoreVersion -ne $resolvedStoreVersion) {
    throw (
        "StoreVersion must exactly match the product version with a Store-owned " +
        "fourth component: $resolvedStoreVersion")
}

$artifacts = Join-Path $root "artifacts"
[IO.Directory]::CreateDirectory($artifacts) | Out-Null
$stagingRoot = Join-Path $artifacts ".store-msix-staging"
$uploadRoot = Join-Path $artifacts ".store-msix-upload"
$msixName = "QuickLook.Next-Store-$resolvedStoreVersion-win-x64.msix"
$uploadName = "QuickLook.Next-Store-$resolvedStoreVersion-win-x64.msixupload"
$metadataName = "QuickLook.Next-Store-$resolvedStoreVersion-manifest.json"
$msixPath = Join-Path $artifacts $msixName
$uploadPath = Join-Path $artifacts $uploadName
$metadataPath = Join-Path $artifacts $metadataName

$artifactRoot = [IO.Path]::GetFullPath($artifacts).TrimEnd('\', '/')
$artifactPrefix = $artifactRoot + [IO.Path]::DirectorySeparatorChar
foreach ($path in @($stagingRoot, $uploadRoot, $msixPath, $uploadPath, $metadataPath)) {
    if (-not ([IO.Path]::GetFullPath($path).StartsWith(
            $artifactPrefix,
            [StringComparison]::OrdinalIgnoreCase))) {
        throw "Store artifact path escaped the artifacts directory: $path"
    }
}
Remove-Item -LiteralPath $stagingRoot, $uploadRoot, $msixPath, $uploadPath, $metadataPath `
    -Recurse -Force -ErrorAction SilentlyContinue

if (-not $SkipBuild) {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "release.ps1") `
        -Arguments @{
            ExpectedVersion = $VersionPrefix
            VersionPrefix = $VersionPrefix
            PackageIdentityName = $PackageIdentityName
            SkipPackage = $true
            SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
        } `
        -FailureMessage "Store release build failed"
}

Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "pack-release.ps1") `
    -Arguments @{
        VersionPrefix = $VersionPrefix
        SkipBuild = $true
        SkipArchive = $true
        SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
    } `
    -FailureMessage "Store release payload staging failed"

New-Item -ItemType Directory -Path $stagingRoot, $uploadRoot -Force | Out-Null
Copy-Item -Path (Join-Path $root "dist\*") -Destination $stagingRoot -Recurse -Force

$manifestPath = Join-Path $root "packaging\AppxManifest.xml"
[xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
$manifest.Package.Identity.Name = $PackageIdentityName
$manifest.Package.Identity.Publisher = $Publisher
$manifest.Package.Identity.Version = $resolvedStoreVersion
$manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
$manifest.Save((Join-Path $stagingRoot "AppxManifest.xml"))

$sdkBin = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory |
    Where-Object Name -Match '^\d+\.\d+\.\d+\.\d+$' |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64" } |
    Where-Object {
        (Test-Path (Join-Path $_ "makeappx.exe")) -and
        (Test-Path (Join-Path $_ "makepri.exe"))
    } |
    Select-Object -First 1
if (-not $sdkBin) { throw "Windows SDK MakeAppx.exe and MakePri.exe were not found." }

$packagePri = Join-Path $stagingRoot "resources.pri"
if (-not (Test-Path -LiteralPath $packagePri -PathType Leaf)) {
    throw "Store package resources.pri is missing. Rebuild with /p:ProjectPriIndexName=$PackageIdentityName."
}
$priDump = Join-Path $artifacts ".store-resources.pri.xml"
try {
    & (Join-Path $sdkBin "makepri.exe") dump /if $packagePri /of $priDump /dt detailed /o | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $priDump -PathType Leaf)) {
        throw "MakePri failed to inspect the Store resource index."
    }
    $priText = [IO.File]::ReadAllText($priDump)
    $escapedIdentity = [regex]::Escape($PackageIdentityName)
    if ($priText -notmatch ('<ResourceMap name="' + $escapedIdentity + '" primary="true"')) {
        throw "Store resources.pri is bound to a different package identity. Rebuild with /p:ProjectPriIndexName=$PackageIdentityName."
    }
}
finally {
    Remove-Item -LiteralPath $priDump -Force -ErrorAction SilentlyContinue
}

$msixPath = [IO.Path]::GetFullPath($msixPath)
& (Join-Path $sdkBin "makeappx.exe") pack /d $stagingRoot /p $msixPath /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed for the Store package." }

$archive = [IO.Compression.ZipFile]::OpenRead($msixPath)
try {
    $entryNames = @($archive.Entries | ForEach-Object { $_.FullName.Replace('\', '/') })
    foreach ($required in @('AppxManifest.xml', 'AppxBlockMap.xml', '[Content_Types].xml', 'resources.pri')) {
        if ($entryNames -notcontains $required) { throw "Store MSIX is missing $required." }
    }
    if ($entryNames -contains 'AppxSignature.p7x') {
        throw "Store MSIX must not retain a sideload signature; Microsoft signs Store packages."
    }
    $manifestEntry = $archive.GetEntry('AppxManifest.xml')
    $reader = [IO.StreamReader]::new($manifestEntry.Open())
    try { [xml]$packagedManifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
    $identity = $packagedManifest.Package.Identity
    if ($identity.Name -ne $PackageIdentityName -or
        $identity.Publisher -ne $Publisher -or
        $identity.Version -ne $resolvedStoreVersion -or
        $identity.ProcessorArchitecture -ne 'x64') {
        throw "Packaged Store identity does not match the requested identity."
    }
}
finally { $archive.Dispose() }

Copy-Item -LiteralPath $msixPath -Destination $uploadRoot
[IO.Compression.ZipFile]::CreateFromDirectory(
    $uploadRoot,
    $uploadPath,
    [IO.Compression.CompressionLevel]::Fastest,
    $false)

$commit = @((git -C $root rev-parse HEAD))[-1].Trim()
if ($LASTEXITCODE -ne 0 -or -not $commit) { throw "Could not resolve the Store package source commit." }
$msixHash = (Get-FileHash -LiteralPath $msixPath -Algorithm SHA256).Hash.ToLowerInvariant()
$uploadHash = (Get-FileHash -LiteralPath $uploadPath -Algorithm SHA256).Hash.ToLowerInvariant()
[ordered]@{
    schemaVersion = 1
    channel = "microsoft-store"
    sourceVersion = $VersionPrefix
    storeVersion = $resolvedStoreVersion
    commit = $commit
    packageIdentityName = $PackageIdentityName
    publisher = $Publisher
    publisherDisplayName = $PublisherDisplayName
    architecture = "x64"
    signed = $false
    msix = [ordered]@{ file = $msixName; sha256 = $msixHash; size = (Get-Item $msixPath).Length }
    msixUpload = [ordered]@{ file = $uploadName; sha256 = $uploadHash; size = (Get-Item $uploadPath).Length }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $metadataPath -Encoding utf8

Remove-Item -LiteralPath $stagingRoot, $uploadRoot -Recurse -Force
Write-Host "Store MSIX created: $msixPath" -ForegroundColor Green
Write-Host "Store upload created: $uploadPath" -ForegroundColor Green
Write-Output $uploadPath
