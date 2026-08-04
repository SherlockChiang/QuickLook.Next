param(
    [Parameter(Mandatory = $true)]
    [string]$VersionPrefix,
    [string]$VersionSuffix = "",
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")
. (Join-Path $PSScriptRoot "release-payload.ps1")

if ($VersionPrefix -notmatch '^\d+\.\d+\.\d+$') {
    throw "VersionPrefix must use semantic X.Y.Z format."
}

$commit = (git -C $Root rev-parse HEAD)
if ($LASTEXITCODE -ne 0 -or -not $commit) {
    throw "Could not resolve the tested source commit."
}

$artifacts = Join-Path $Root "artifacts"
[IO.Directory]::CreateDirectory($artifacts) | Out-Null
$noticePath = Join-Path $artifacts "THIRD-PARTY-NOTICES.txt"
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "new-third-party-notices.ps1") `
    -Arguments @{
        Root = $Root
        OutputPath = $noticePath
    } `
    -FailureMessage "Third-party notice generation failed"
$payload = @(
    Get-QuickLookReleasePayload `
        -Root $Root `
        -ArtifactsDirectory $artifacts)
$outputHashes = New-QuickLookReleasePayloadHashes -Payload $payload

$proofPath = Join-Path $artifacts ".tested-release-build.json"
[ordered]@{
    payloadSchemaVersion = 1
    versionPrefix = $VersionPrefix
    versionSuffix = $VersionSuffix
    commit = @($commit)[-1].Trim()
    outputs = $outputHashes
} | ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath $proofPath -Encoding utf8

Write-Host "Tested build proof: $proofPath" -ForegroundColor DarkGray
Write-Output $proofPath
