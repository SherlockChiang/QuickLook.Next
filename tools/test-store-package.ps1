param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$scriptPath = Join-Path $Root "tools\pack-store-msix.ps1"
$releasePath = Join-Path $Root "tools\release.ps1"
$packReleasePath = Join-Path $Root "tools\pack-release.ps1"
$appProjectPath = Join-Path $Root "src\QuickLook.Next.App\QuickLook.Next.App.csproj"
if (-not (Test-Path -LiteralPath $scriptPath -PathType Leaf)) {
    throw "Store MSIX packaging script is missing."
}
if (-not (Test-Path -LiteralPath $appProjectPath -PathType Leaf)) {
    throw "The app project required for Store PRI generation is missing."
}

$command = Get-Command -Name $scriptPath
foreach ($name in @(
        "PackageIdentityName",
        "Publisher",
        "PublisherDisplayName",
        "VersionPrefix",
        "StoreVersion",
        "SkipBuild"))
{
    if (-not $command.Parameters.ContainsKey($name)) {
        throw "pack-store-msix.ps1 is missing the $name parameter."
    }
}

$text = Get-Content -LiteralPath $scriptPath -Raw
foreach ($rule in @(
        @('resolve-store-msix-version\.ps1', "Store packaging must use the Store version resolver."),
        @('StoreVersion must exactly match the product version', "Explicit Store versions must not override the product version mapping."),
        @('ProjectPriIndexName', "Store packaging must require an identity-bound PRI build."),
        @('makepri\.exe[\s\S]{0,500}ResourceMap name', "Store packaging must validate the PRI primary map."),
        @('makeappx\.exe', "Store packaging must create a MakeAppx package."),
        @('AppxSignature\.p7x', "Store packaging must fail if a sideload signature remains."),
        @('msixupload', "Store packaging must produce an .msixupload submission container."),
        @('signed\s*=\s*\$false', "Store metadata must make Microsoft signing ownership explicit."),
        @('PublisherDisplayName', "Store packaging must carry the Partner Center display name."),
        @('OrdinalIgnoreCase', "Store artifact paths must remain confined to artifacts.")))
{
    if ($text -notmatch $rule[0]) { throw $rule[1] }
}
if ($text -match '(?i)SignTool|signtool|CertificatePassword|CreateDevelopmentCertificate|\.pfx') {
    throw "Store packaging must not depend on the sideload certificate path."
}

$releaseText = Get-Content -LiteralPath $releasePath -Raw
$packReleaseText = Get-Content -LiteralPath $packReleasePath -Raw
$appProjectText = Get-Content -LiteralPath $appProjectPath -Raw
if ($appProjectText -notmatch '<ProjectPriIndexName\s+Condition="''\$\(ProjectPriIndexName\)''\s+==\s+''\s*''">SherlockChiang\.QuickLookNext</ProjectPriIndexName>') {
    throw "The app PRI identity must keep the sideload default conditional so Store builds can override it."
}
if ($releaseText -notmatch '\$PackageIdentityName' -or
    $releaseText -notmatch 'ProjectPriIndexName=\$PackageIdentityName' -or
    $packReleaseText -notmatch '\$PackageIdentityName' -or
    $packReleaseText -notmatch 'ProjectPriIndexName=\$PackageIdentityName') {
    throw "Release build orchestration must accept the Store package identity."
}

Write-Host "Store package guard passed" -ForegroundColor Green
