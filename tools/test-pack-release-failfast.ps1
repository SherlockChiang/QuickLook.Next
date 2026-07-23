param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\pack-release.ps1"
$text = Get-Content -LiteralPath $scriptPath -Raw
$requiredPatterns = @(
    @('dotnet\s+--list-sdks[\s\S]*\$installedSdks\s+-notcontains\s+\$requiredSdk', "Release packaging must verify the pinned SDK before building."),
    @('cargo\s+build[\s\S]{0,300}\$LASTEXITCODE\s+-ne\s+0', "Native build failures must stop release packaging."),
    @('dotnet\s+@buildArgs[\s\S]{0,200}\$LASTEXITCODE\s+-ne\s+0', ".NET build failures must stop release packaging."),
    @('if\s*\(-not\s+\$SkipBuild\)[\s\S]*cargo\s+build[\s\S]*dotnet\s+@buildArgs', "Release build work must be skippable after authoritative tests."),
    @('\$requiredOutputs[\s\S]*QuickLook\.Next\.App\.exe[\s\S]*QuickLook\.Next\.RasterHost\.exe[\s\S]*QuickLook\.Next\.ParserHost\.exe', "No-build packaging must require all release executables."),
    @('tested-release-build\.json[\s\S]*versionPrefix[\s\S]*proof\.commit[\s\S]*Get-FileHash', "No-build packaging must verify the tested version, commit, and output hashes."),
    @('if\s*\(-not\s+\$SkipArchive\)', "MSIX staging must be able to skip the unused raw release archive."),
    @('Test-Path\s+-LiteralPath\s+\$src\s+-PathType\s+Container', "Dist assembly must reject missing build output directories."),
    @('Get-ChildItem\s+-LiteralPath\s+\$src', "Dist assembly must treat source paths literally.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}

Write-Host "pack-release fail-fast test passed" -ForegroundColor Green
