param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ci = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\ci.yml") -Raw
$stable = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\release.yml") -Raw
$beta = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\beta-release.yml") -Raw
$packageActionPath = Join-Path $Root ".github\actions\package-release\action.yml"
$legacyPackageWorkflowPath = Join-Path $Root ".github\workflows\package-release.yml"
if (-not (Test-Path -LiteralPath $packageActionPath)) {
    throw "The shared package composite action is missing."
}
if (Test-Path -LiteralPath $legacyPackageWorkflowPath) {
    throw "Release packaging must not use a reusable workflow that hides Environment secrets."
}
$packageAction = Get-Content -LiteralPath $packageActionPath -Raw
$pages = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\pages.yml") -Raw

if ($packageAction -notmatch '(?ms)^runs:\s*\r?\n\s+using:\s+composite\s*\r?\n\s+steps:' -or
    $packageAction -match '\$\{\{\s*secrets\.') {
    throw "The shared package action must be composite and must receive secrets only through explicit inputs."
}
foreach ($inputName in @("channel", "release-pfx-base64", "release-pfx-password")) {
    if ($packageAction -notmatch "(?m)^\s{2}$([regex]::Escape($inputName)):\s*$") {
        throw "The shared package action is missing required input '$inputName'."
    }
}
foreach ($outputName in @("version", "tag", "artifact-name", "msix-version")) {
    if ($packageAction -notmatch "(?m)^\s{2}$([regex]::Escape($outputName)):\s*$") {
        throw "The shared package action is missing output '$outputName'."
    }
}

if ($ci -notmatch '(?ms)^on:\s*.*?push:\s*\r?\n\s+branches:\s*\[main\]') {
    throw "CI must verify every push to main independently from release publication."
}
if ($ci -notmatch 'tools/test-nuget-vulnerabilities\.ps1' -or
    $ci -notmatch 'cargo install cargo-audit --version 0\.22\.2 --locked --force[\s\S]*cargo audit --file native/Cargo\.lock' -or
    $ci -notmatch 'npm ci && npm audit' -or
    $ci -notmatch 'working-directory:\s+website[\s\S]*npm run build') {
    throw "Pull-request CI must audit NuGet, Cargo, and npm dependencies and build the website."
}
if ($packageAction -notmatch "cargo-audit 0\\\.22\\\.2" -or
    $packageAction -notmatch 'cargo install cargo-audit --version 0\.22\.2 --locked --force' -or
    $packageAction -notmatch 'cargo-audit-\$\{\{ runner\.os \}\}-0\.22\.2' -or
    $packageAction -notmatch 'tools/test-nuget-vulnerabilities\.ps1') {
    throw "Release dependency auditing must use the pinned CVSS 4.0-compatible cargo-audit version."
}
if ($stable -notmatch "startsWith\(github\.event\.head_commit\.message, 'release:'\)") {
    throw "Stable releases must require an explicit release: commit."
}
if ($stable -notmatch 'uses:\s+\./\.github/actions/package-release' -or
    $beta -notmatch 'uses:\s+\./\.github/actions/package-release') {
    throw "Stable and beta releases must use the shared package composite action."
}
if ($stable -notmatch '(?m)^\s+environment:\s+release\s*$' -or
    $beta -notmatch '(?m)^\s+environment:\s+beta\s*$') {
    throw "Stable and beta package jobs must bind directly to their channel Environment."
}
foreach ($workflow in @($stable, $beta)) {
    if ($workflow -notmatch 'release-pfx-base64:\s*\$\{\{\s*secrets\.QUICKLOOK_RELEASE_PFX_BASE64\s*\}\}' -or
        $workflow -notmatch 'release-pfx-password:\s*\$\{\{\s*secrets\.QUICKLOOK_RELEASE_PFX_PASSWORD\s*\}\}') {
        throw "Each channel must pass only its Environment signing secrets to the package action."
    }
    if ($workflow -notmatch
            'actions/checkout@[0-9a-f]{40}[\s\S]*fetch-depth:\s*0[\s\S]*uses:\s+\./\.github/actions/package-release') {
        throw "Each channel must check out full history before loading the local package action."
    }
}
if ($packageAction -notmatch 'Test and package signed release[\s\S]*tools/release\.ps1' -or
    $packageAction -match '(?m)^\s*run:\s*(cargo|dotnet)\s+(build|test)') {
    throw "The shared action must delegate its single build/test/package sequence to release.ps1."
}
$releaseScript = Get-Content -LiteralPath (
    Join-Path $Root "tools\release.ps1") -Raw
if ($releaseScript -notmatch
        'dotnet\s+test[\s\S]{0,260}--maxcpucount:1') {
    throw "Formal release integration test projects must run serially."
}
if ($packageAction -notmatch 'actions/cache@[0-9a-f]{40}' -or
    $packageAction -notmatch 'new-release-metadata\.ps1' -or
    $packageAction -notmatch 'Stable release has no user-visible changes') {
    throw "Release caching, environment isolation, metadata, and visible-change guards are required."
}
if ($packageAction -notmatch
        'Check release signing configuration[\s\S]*QUICKLOOK_RELEASE_PFX_BASE64[\s\S]*QUICKLOOK_RELEASE_PFX_PASSWORD[\s\S]*Check NuGet vulnerabilities') {
    throw "Release signing secrets must fail fast before dependency audits."
}
if ($packageAction -notmatch 'resolve-formal-msix-version\.ps1[\s\S]*msix_version=\$msixVersion' -or
    $packageAction -match 'msix_version=\$version\.0') {
    throw "Formal beta and stable packages must use strictly ordered MSIX revisions."
}
if ($packageAction -notmatch
        'set-version\.ps1\s+-Version\s+\$version' -or
    $packageAction -match
        '\$version\s*\|\s*Set-Content\s+-LiteralPath\s+VERSION') {
    throw "Resolved release versions must synchronize every authoritative version source."
}
if ($stable -match '(?m)^\s*secrets:\s*inherit\s*$' -or $beta -match '(?m)^\s*secrets:\s*inherit\s*$') {
    throw "Release signing secrets must come from channel-specific GitHub Environments."
}
foreach ($workflow in @($stable, $beta)) {
    if ($workflow -notmatch 'artifacts/release/\*\.json') {
        throw "Stable and beta releases must attest and publish release metadata."
    }
}
foreach ($workflow in @($ci, $stable, $beta, $packageAction, $pages)) {
    if ($workflow -match 'uses:\s+actions/[^@\s]+@v\d') {
        throw "Official actions must remain pinned to immutable commit SHAs."
    }
}

Write-Host "release workflow guard passed" -ForegroundColor Green
