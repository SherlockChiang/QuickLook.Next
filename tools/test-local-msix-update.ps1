param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\update-local-msix.ps1"
if (-not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) {
    throw "The local MSIX update entry point is missing."
}

$command = Get-Command -Name $scriptPath
foreach ($name in @("VersionPrefix", "Root", "WhatIf")) {
    if (-not $command.Parameters.ContainsKey($name)) {
        throw "Local MSIX update is missing the $name parameter."
    }
}

$text = Get-Content -LiteralPath $scriptPath -Raw
$requiredPatterns = @(
    @('Get-AppxPackage\s+-Name\s+\$packageName', "The installed current-user package must determine the update revision."),
    @('AppxManifest\.xml[\s\S]*Identity\.Publisher', "The expected publisher must come from the package manifest."),
    @('resolve-local-msix-version\.ps1', "Local MSIX updates must use the bounded version resolver."),
    @('pack-msix\.ps1[\s\S]*-Version\s+\$numericVersion[\s\S]*-SkipBuild', "Local updates must package the tested build with an explicit four-part version."),
    @('SignerCertificate\.Thumbprint[\s\S]*expectedCertificate\.Thumbprint', "The package signer must match the pinned certificate."),
    @('Add-AppxPackage\s+-Path\s+\$msixPath\s+-ForceApplicationShutdown', "Installation must update the current user and stop the running App."),
    @('Get-AppxPackage\s+-Name\s+\$packageName[\s\S]*Version\.ToString\(\)\s+-ne\s+\$numericVersion', "The installed version must be verified after registration."),
    @('ShouldProcess[\s\S]*sign, package, stop the running App, and install', "WhatIf must cover every mutating update step.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}
if ($text -match 'Remove-AppxPackage|ForceUpdateFromAnyVersion') {
    throw "Local updates must not silently uninstall or downgrade packages."
}

Write-Host "local MSIX update guard passed" -ForegroundColor Green
