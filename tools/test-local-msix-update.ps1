param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\update-local-msix.ps1"
if (-not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) {
    throw "The local MSIX update entry point is missing."
}

$command = Get-Command -Name $scriptPath
foreach ($name in @("VersionPrefix", "PackageOnly", "Root", "WhatIf")) {
    if (-not $command.Parameters.ContainsKey($name)) {
        throw "Local MSIX update is missing the $name parameter."
    }
}

$text = Get-Content -LiteralPath $scriptPath -Raw
$requiredPatterns = @(
    @('if\s*\(-not\s+\$PackageOnly\)[\s\S]{0,500}Import-Module Appx', "Package-only mode must not require the Appx module."),
    @('if\s*\(-not\s+\$PackageOnly\)[\s\S]{0,700}Get-AppxPackage\s+-Name\s+\$packageName', "Only installation mode may inherit revision constraints from the installed package."),
    @('AppxManifest\.xml[\s\S]*Identity\.Publisher', "The expected publisher must come from the package manifest."),
    @('resolve-local-msix-version\.ps1', "Local MSIX updates must use the bounded version resolver."),
    @('pack-msix\.ps1[\s\S]*-Version\s+\$numericVersion[\s\S]*-SkipBuild', "Local updates must package the tested build with an explicit four-part version."),
    @('SignerCertificate\.Thumbprint[\s\S]*expectedCertificate\.Thumbprint', "The package signer must match the pinned certificate."),
    @('SignatureStatus\]::NotTrusted[\s\S]*SignatureStatus\]::UnknownError', "A matching self-signed package must survive both PowerShell untrusted signature states."),
    @('if\s*\(\$PackageOnly\)[\s\S]*Local MSIX created:[\s\S]*return[\s\S]*Add-AppxPackage', "Package-only mode must return the signed artifact without changing the installed package."),
    @('Add-AppxPackage\s+-Path\s+\$msixPath\s+-ForceApplicationShutdown', "Installation must update the current user and stop the running App."),
    @('Get-AppxPackage\s+-Name\s+\$packageName[\s\S]*Version\.ToString\(\)\s+-ne\s+\$numericVersion', "The installed version must be verified after registration."),
    @('\$action\s*=\s*if\s*\(\$PackageOnly\)[\s\S]*sign and package[\s\S]*sign, package, stop the running App, and install[\s\S]*ShouldProcess', "WhatIf must describe package-only and install mutations separately.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}
if ($text -match 'Remove-AppxPackage|ForceUpdateFromAnyVersion') {
    throw "Local updates must not silently uninstall or downgrade packages."
}

function Get-AppxPackage {
    [CmdletBinding()]
    param([string]$Name)

    throw "Package-only WhatIf unexpectedly queried the installed package."
}

$sourceVersion = (Get-Content -LiteralPath (Join-Path $Root "VERSION") -Raw).Trim()
$packageOnlyVersion = @(
    & $scriptPath `
        -VersionPrefix $sourceVersion `
        -PackageOnly `
        -WhatIf
)[-1]
if ($packageOnlyVersion -notmatch
        ('^' + [regex]::Escape($sourceVersion) + '\.\d+$')) {
    throw "Package-only WhatIf did not resolve an independent four-part version."
}

Write-Host "local MSIX update guard passed" -ForegroundColor Green
