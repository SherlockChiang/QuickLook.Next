param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\pack-msix.ps1"
$command = Get-Command -Name $scriptPath
foreach ($name in @("Version", "VersionPrefix", "VersionSuffix")) {
    if (-not $command.Parameters.ContainsKey($name)) {
        throw "pack-msix.ps1 is missing the $name parameter."
    }
}

$text = Get-Content -LiteralPath $scriptPath -Raw
$requiredPatterns = @(
    @('\$Version\s+-and\s+\(\$VersionPrefix\s+-or\s+\$VersionSuffix\)', "Version must remain mutually exclusive with prefix/suffix parameters."),
    @("\^\\d\+\\\.\\d\+\\\.\\d\+\\\.\\d\+\$", "Version must retain X.Y.Z.W validation."),
    @('\$_\s+-lt\s+0\s+-or\s+\$_\s+-gt\s+65535', "MSIX version components must remain bounded to 0..65535."),
    @('\$numericVersion\s*=\s*\$Version', "The explicit four-part version must flow to the MSIX manifest."),
    @('\$VersionPrefix\s*=\s*\$versionParts\[0\.\.2\]\s+-join', "The build version prefix must derive from the first three components.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}

Write-Host "pack-msix version parameter test passed" -ForegroundColor Green
