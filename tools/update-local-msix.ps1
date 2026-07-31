[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory = $true)]
    [string]$VersionPrefix,
    [switch]$PackageOnly,
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $PackageOnly) {
    $requestedWhatIf = $WhatIfPreference
    try {
        $WhatIfPreference = $false
        Import-Module Appx -ErrorAction Stop
    }
    finally {
        $WhatIfPreference = $requestedWhatIf
    }
}

$packageName = "SherlockChiang.QuickLookNext"
$manifestPath = Join-Path $Root "packaging\AppxManifest.xml"
$certificatePath = Join-Path $Root "packaging\QuickLook.Next-Release.cer"
foreach ($path in @($manifestPath, $certificatePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Local MSIX update input is missing: $path"
    }
}

[xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
$expectedPublisher = [string]$manifest.Package.Identity.Publisher
if (-not $expectedPublisher) {
    throw "The MSIX manifest publisher is missing."
}

$installedVersion = ""
$installedPublisher = ""
if (-not $PackageOnly) {
    $installedPackages = @(
        Get-AppxPackage -Name $packageName -ErrorAction SilentlyContinue)
    if ($installedPackages.Count -gt 1) {
        throw "More than one current-user package matched $packageName."
    }
    if ($installedPackages.Count -eq 1) {
        $installedVersion = $installedPackages[0].Version.ToString()
        $installedPublisher = [string]$installedPackages[0].Publisher
    }
}

$artifacts = Join-Path $Root "artifacts"
$knownVersions = @(
    if (Test-Path -LiteralPath $artifacts -PathType Container) {
        Get-ChildItem -LiteralPath $artifacts -File |
            ForEach-Object {
                $match = [regex]::Match(
                    $_.Name,
                    '^QuickLook\.Next-(\d+\.\d+\.\d+\.\d+)-win-x64\.msix$')
                if ($match.Success) {
                    $match.Groups[1].Value
                }
            }
    }
)

$resolveArgs = @{
    VersionPrefix = $VersionPrefix
    InstalledVersion = $installedVersion
    InstalledPublisher = $installedPublisher
    ExpectedPublisher = $expectedPublisher
    KnownVersions = $knownVersions
}
$numericVersion = @(
    & (Join-Path $PSScriptRoot "resolve-local-msix-version.ps1") @resolveArgs
)[-1]
if (-not $numericVersion) {
    throw "The local MSIX version could not be resolved."
}

$target = if ($PackageOnly) {
    "$packageName $numericVersion local artifacts"
}
else {
    "$packageName $numericVersion for the current user"
}
$action = if ($PackageOnly) {
    "sign and package"
}
else {
    "sign, package, stop the running App, and install"
}
if (-not $PSCmdlet.ShouldProcess(
        $target,
        $action)) {
    Write-Output $numericVersion
    return
}

& (Join-Path $PSScriptRoot "pack-msix.ps1") `
    -Version $numericVersion `
    -SkipBuild | Out-Host

$msixPath = Join-Path (
    $artifacts) "QuickLook.Next-$numericVersion-win-x64.msix"
if (-not (Test-Path -LiteralPath $msixPath -PathType Leaf)) {
    throw "The expected local MSIX was not produced: $msixPath"
}

$expectedCertificate =
    New-Object Security.Cryptography.X509Certificates.X509Certificate2(
        $certificatePath)
$signature = Get-AuthenticodeSignature -LiteralPath $msixPath
if (-not $signature.SignerCertificate -or
    $signature.SignerCertificate.Thumbprint -ne
        $expectedCertificate.Thumbprint) {
    throw "The local MSIX signer does not match the pinned release certificate."
}
if ($signature.Status -notin @(
        [Management.Automation.SignatureStatus]::Valid,
        [Management.Automation.SignatureStatus]::NotTrusted,
        [Management.Automation.SignatureStatus]::UnknownError)) {
    throw "The local MSIX signature is invalid: $($signature.Status)."
}
if ($PackageOnly) {
    Write-Host "Local MSIX created: $msixPath" -ForegroundColor Green
    Write-Output $numericVersion
    return
}
if ($signature.Status -ne
    [Management.Automation.SignatureStatus]::Valid) {
    $installer = Join-Path (
        $artifacts) "QuickLook.Next-Installer-$numericVersion-win-x64.zip"
    throw ("Windows does not trust the pinned package certificate. " +
        "Use the generated installer once to establish trust: $installer")
}

Add-AppxPackage -Path $msixPath -ForceApplicationShutdown | Out-Null

$installedAfter = @(
    Get-AppxPackage -Name $packageName -ErrorAction SilentlyContinue)
if ($installedAfter.Count -ne 1 -or
    [string]$installedAfter[0].Publisher -ne $expectedPublisher -or
    $installedAfter[0].Version.ToString() -ne $numericVersion) {
    throw "The current-user package did not update to $numericVersion."
}

Write-Host (
    "Installed QuickLook Next MSIX $numericVersion for the current user.") `
    -ForegroundColor Green
Write-Output $numericVersion
