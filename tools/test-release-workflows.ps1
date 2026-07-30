param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ci = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\ci.yml") -Raw
$stable = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\release.yml") -Raw
$beta = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\beta-release.yml") -Raw
$shared = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\package-release.yml") -Raw
$pages = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\pages.yml") -Raw

if ($ci -notmatch '(?ms)^on:\s*.*?push:\s*\r?\n\s+branches:\s*\[main\]') {
    throw "CI must verify every push to main independently from release publication."
}
if ($ci -notmatch 'tools/test-nuget-vulnerabilities\.ps1' -or
    $ci -notmatch 'cargo install cargo-audit --version 0\.22\.2 --locked --force[\s\S]*cargo audit --file native/Cargo\.lock' -or
    $ci -notmatch 'npm ci && npm audit' -or
    $ci -notmatch 'working-directory:\s+website[\s\S]*npm run build') {
    throw "Pull-request CI must audit NuGet, Cargo, and npm dependencies and build the website."
}
if ($shared -notmatch "cargo-audit 0\\\.22\\\.2" -or
    $shared -notmatch 'cargo install cargo-audit --version 0\.22\.2 --locked --force' -or
    $shared -notmatch 'cargo-audit-\$\{\{ runner\.os \}\}-0\.22\.2' -or
    $shared -notmatch 'tools/test-nuget-vulnerabilities\.ps1') {
    throw "Release dependency auditing must use the pinned CVSS 4.0-compatible cargo-audit version."
}
if ($stable -notmatch "startsWith\(github\.event\.head_commit\.message, 'release:'\)") {
    throw "Stable releases must require an explicit release: commit."
}
if ($stable -notmatch 'uses:\s+\./\.github/workflows/package-release\.yml' -or
    $beta -notmatch 'uses:\s+\./\.github/workflows/package-release\.yml') {
    throw "Stable and beta releases must use the shared packaging workflow."
}
if ($shared -notmatch 'Test and package signed release[\s\S]*tools/release\.ps1' -or
    $shared -match '(?m)^\s*run:\s*(cargo|dotnet)\s+(build|test)') {
    throw "The shared workflow must delegate its single build/test/package sequence to release.ps1."
}
$releaseScript = Get-Content -LiteralPath (
    Join-Path $Root "tools\release.ps1") -Raw
if ($releaseScript -notmatch
        'dotnet\s+test[\s\S]{0,260}--maxcpucount:1') {
    throw "Formal release integration test projects must run serially."
}
if ($shared -notmatch 'environment:\s+\$\{\{ inputs\.channel' -or
    $shared -notmatch 'actions/cache@[0-9a-f]{40}' -or
    $shared -notmatch 'new-release-metadata\.ps1' -or
    $shared -notmatch 'Stable release has no user-visible changes') {
    throw "Release caching, environment isolation, metadata, and visible-change guards are required."
}
if ($shared -notmatch 'resolve-formal-msix-version\.ps1[\s\S]*msix_version=\$msixVersion' -or
    $shared -match 'msix_version=\$version\.0') {
    throw "Formal beta and stable packages must use strictly ordered MSIX revisions."
}
if ($stable -match '(?m)^\s*secrets:\s*inherit\s*$' -or $beta -match '(?m)^\s*secrets:\s*inherit\s*$') {
    throw "Release signing secrets must come from channel-specific GitHub Environments."
}
foreach ($workflow in @($stable, $beta)) {
    if ($workflow -notmatch 'artifacts/release/\*\.json') {
        throw "Stable and beta releases must attest and publish release metadata."
    }
}
foreach ($workflow in @($ci, $stable, $beta, $shared, $pages)) {
    if ($workflow -match 'uses:\s+actions/[^@\s]+@v\d') {
        throw "Official actions must remain pinned to immutable commit SHAs."
    }
}

Write-Host "release workflow guard passed" -ForegroundColor Green
