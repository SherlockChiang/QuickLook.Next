param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\pack-msix.ps1"
$command = Get-Command -Name $scriptPath
foreach ($name in @("Version", "VersionPrefix", "VersionSuffix", "SkipBuild")) {
    if (-not $command.Parameters.ContainsKey($name)) {
        throw "pack-msix.ps1 is missing the $name parameter."
    }
}

$text = Get-Content -LiteralPath $scriptPath -Raw
$appProjectText = Get-Content -LiteralPath (Join-Path $Root "src\QuickLook.Next.App\QuickLook.Next.App.csproj") -Raw
$requiredPatterns = @(
    @('\$Version\s+-and\s+\(\$VersionPrefix\s+-or\s+\$VersionSuffix\)', "Version must remain mutually exclusive with prefix/suffix parameters."),
    @("\^\\d\+\\\.\\d\+\\\.\\d\+\\\.\\d\+\$", "Version must retain X.Y.Z.W validation."),
    @('\$_\s+-lt\s+0\s+-or\s+\$_\s+-gt\s+65535', "MSIX version components must remain bounded to 0..65535."),
    @('\$numericVersion\s*=\s*\$Version', "The explicit four-part version must flow to the MSIX manifest."),
    @('\$VersionPrefix\s*=\s*\$versionParts\[0\.\.2\]\s+-join', "The build version prefix must derive from the first three components."),
    @('resolve-formal-msix-version\.ps1[\s\S]*-VersionSuffix\s+\$VersionSuffix', "Formal packages must use the ordered beta/stable MSIX revision resolver."),
    @('Resolve-ArtifactChildPath[\s\S]*GetFullPath[\s\S]*StartsWith\([\s\S]*OrdinalIgnoreCase', "Artifact paths must be normalized and confined before deletion or writes."),
    @('\.signing[\s\S]*QuickLook\.Next-Release\.pfx', "MSIX packaging must discover the initialized local release certificate."),
    @('QuickLook\.Next-Release\.password[\s\S]*Get-Content\s+-LiteralPath', "MSIX packaging must discover the initialized local certificate password."),
    @('A signing certificate is required\.[\s\S]{0,300}test-installer-script\.ps1', "Missing signing credentials must fail before release packaging starts."),
    @('pack-release\.ps1[\s\S]{0,300}-SkipBuild:\$SkipBuild[\s\S]{0,100}-SkipArchive', "MSIX packaging must reuse tested outputs and skip its unused intermediate archive.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}
if ($appProjectText -notmatch '<ProjectPriIndexName>SherlockChiang\.QuickLookNext</ProjectPriIndexName>' -or
    $appProjectText -notmatch '<ProjectPriFileName>resources\.pri</ProjectPriFileName>') {
    throw "The app must generate one complete PRI using the MSIX package identity."
}
foreach ($pattern in @(
    'makepri\.exe"\)\s+dump',
    'Microsoft\\\.UI\\\.Xaml',
    'Square44x44Logo\\\.targetsize-16_altform-unplated')) {
    if ($text -notmatch $pattern) {
        throw "MSIX packaging must validate the complete WinUI and unplated icon resource map."
    }
}

Write-Host "pack-msix version parameter test passed" -ForegroundColor Green
