param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\pack-release.ps1"
$proofWriterPath = Join-Path $Root "tools\write-tested-release-proof.ps1"
$payloadHelperPath = Join-Path $Root "tools\release-payload.ps1"
foreach ($requiredPath in @($proofWriterPath, $payloadHelperPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "A release payload proof component is missing: $requiredPath"
    }
}
$text = Get-Content -LiteralPath $scriptPath -Raw
$proofWriterText = Get-Content -LiteralPath $proofWriterPath -Raw
$payloadHelperText = Get-Content -LiteralPath $payloadHelperPath -Raw
$requiredPatterns = @(
    @('dotnet\s+--list-sdks[\s\S]*\$installedSdks\s+-notcontains\s+\$requiredSdk', "Release packaging must verify the pinned SDK before building."),
    @('dotnet\s+msbuild\s+\$nativeProject[\s\S]{0,180}\$LASTEXITCODE\s+-ne\s+0', "Native build failures must stop release packaging."),
    @('dotnet\s+@buildArgs[\s\S]{0,200}\$LASTEXITCODE\s+-ne\s+0', ".NET build failures must stop release packaging."),
    @('if\s*\(-not\s+\$SkipBuild\)[\s\S]*dotnet\s+msbuild\s+\$nativeProject[\s\S]*dotnet\s+@buildArgs', "Release build work must be skippable after authoritative tests."),
    @('\$VersionSuffix\s+-and[\s\S]{0,100}\^\[0-9A-Za-z\][\s\S]{0,100}SemVer-compatible identifier', "Release packaging must reject unsafe suffixes before deriving artifact paths."),
    @('\$requiredOutputs[\s\S]*QuickLook\.Next\.App\.exe[\s\S]*QuickLook\.Next\.RasterHost\.exe[\s\S]*QuickLook\.Next\.ParserHost\.exe[\s\S]*QuickLook\.Next\.ShellBroker\.exe', "No-build packaging must require all release executables."),
    @('new-third-party-notices\.ps1', "Release packages must generate third-party notices."),
    @('checked-invocation\.ps1[\s\S]*Invoke-CheckedScript[\s\S]{0,300}guard-architecture\.ps1', "Release packaging must fail closed on the architecture guard."),
    @('release-payload\.ps1[\s\S]*Get-QuickLookReleasePayload', "Release packaging must use the shared payload enumeration."),
    @('tested-release-build\.json[\s\S]*payloadSchemaVersion[\s\S]*versionPrefix[\s\S]*proof\.commit[\s\S]*Assert-QuickLookReleasePayloadProof', "No-build packaging must verify the proof schema, version, commit, exact payload keys, and hashes."),
    @('Copy-QuickLookReleasePayload[\s\S]*Assert-QuickLookReleasePayloadProof[\s\S]*-ContentRoot\s+\$dist', "The staged dist payload must be copied and revalidated from the shared manifest."),
    @('if\s*\(-not\s+\$SkipArchive\)', "MSIX staging must be able to skip the unused raw release archive.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) {
        throw $rule[1]
    }
}

if ($proofWriterText -notmatch
        'release-payload\.ps1[\s\S]*new-third-party-notices\.ps1[\s\S]*Get-QuickLookReleasePayload[\s\S]*New-QuickLookReleasePayloadHashes[\s\S]*payloadSchemaVersion\s*=\s*1')
{
    throw "Tested proof generation must hash the complete shared payload manifest."
}

foreach ($rule in @(
        @('QuickLook\.Next\.App\\bin\\Release[\s\S]*QuickLook\.Next\.RasterHost\\bin\\Release[\s\S]*QuickLook\.Next\.ParserHost\\bin\\Release', "The payload helper must enumerate every copied application output tree."),
        @('QuickLook\.Next\.ShellBroker\.runtimeconfig\.json[\s\S]*LICENSE[\s\S]*THIRD-PARTY-NOTICES\.txt', "The payload helper must include the broker and release notices."),
        @('Test-OptionalRootPayload[\s\S]*Get-PrunedAppLocaleDirectories', "The payload helper must own optional runtime and locale pruning."),
        @('Get-QuickLookProofOutputMap[\s\S]*\$missing[\s\S]*\$extra[\s\S]*Get-FileHash', "Payload proof validation must reject key-set differences and validate every file hash.")))
{
    if ($payloadHelperText -notmatch $rule[0]) {
        throw $rule[1]
    }
}

Write-Host "pack-release fail-fast test passed" -ForegroundColor Green
