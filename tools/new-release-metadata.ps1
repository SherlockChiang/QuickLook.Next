param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][ValidateSet('stable', 'beta')][string]$Channel,
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$ChecksumPath,
    [Parameter(Mandatory = $true)][string]$SbomPath,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$checksum = (Resolve-Path -LiteralPath $ChecksumPath).Path
$sbom = (Resolve-Path -LiteralPath $SbomPath).Path
$testedBuildPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'artifacts\.tested-release-build.json'
if (-not (Test-Path -LiteralPath $testedBuildPath -PathType Leaf)) { throw "Tested build manifest is missing." }
$testedBuild = Get-Content -LiteralPath $testedBuildPath -Raw | ConvertFrom-Json
$commit = (git rev-parse HEAD).Trim()
if ($testedBuild.commit -ne $commit) { throw "Tested build manifest belongs to another commit." }
$installerName = [IO.Path]::GetFileName($installer)
$installerHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumLine = (Get-Content -LiteralPath $checksum -Raw).Trim()
if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+(.+)$' -or
    $Matches[1].ToLowerInvariant() -ne $installerHash -or $Matches[2] -ne $installerName) {
    throw "Installer checksum is inconsistent."
}
$publishedAt = [DateTimeOffset]::UtcNow.ToString('o')
$downloadUrl = if ($Repository) { "https://github.com/$Repository/releases/download/$Tag/$installerName" } else { "" }

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
[ordered]@{
    schemaVersion = 1
    version = $Version
    tag = $Tag
    channel = $Channel
    commit = $commit
    publishedAt = $publishedAt
    installer = [ordered]@{
        file = $installerName
        sha256 = $installerHash
        size = (Get-Item -LiteralPath $installer).Length
    }
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'release-metadata.json') -Encoding utf8

[ordered]@{
    schemaVersion = 1
    version = $Version
    tag = $Tag
    channel = $Channel
    architecture = 'x64'
    minimumWindowsBuild = 17763
    publishedAt = $publishedAt
    downloadUrl = $downloadUrl
    file = $installerName
    sha256 = $installerHash
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'update.json') -Encoding utf8

[ordered]@{
    schemaVersion = 1
    version = $Version
    commit = $commit
    dotnetSdk = (dotnet --version).Trim()
    rustc = (rustc --version).Trim()
    certificateThumbprint = ([Security.Cryptography.X509Certificates.X509Certificate2]::new(
        (Join-Path (Split-Path $PSScriptRoot -Parent) 'packaging\QuickLook.Next-Release.cer'))).Thumbprint
    installerSha256 = $installerHash
    sbomSha256 = (Get-FileHash -LiteralPath $sbom -Algorithm SHA256).Hash.ToLowerInvariant()
    outputs = $testedBuild.outputs
} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'build-manifest.json') -Encoding utf8

Write-Host "release metadata created" -ForegroundColor Green
