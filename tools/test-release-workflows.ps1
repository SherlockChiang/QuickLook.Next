param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$ci = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\ci.yml") -Raw
$stable = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\release.yml") -Raw
$beta = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\beta-release.yml") -Raw
$shared = Get-Content -LiteralPath (Join-Path $Root ".github\workflows\package-release.yml") -Raw

if ($ci -match '(?m)^\s+push:\s*$') { throw "CI must not duplicate stable release work on main push." }
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
if ($shared -notmatch 'environment:\s+\$\{\{ inputs\.channel' -or
    $shared -notmatch 'actions/cache@[0-9a-f]{40}' -or
    $shared -notmatch 'new-release-metadata\.ps1' -or
    $shared -notmatch 'Stable release has no user-visible changes') {
    throw "Release caching, environment isolation, metadata, and visible-change guards are required."
}
if ($stable -match '(?m)^\s*secrets:\s*inherit\s*$' -or $beta -match '(?m)^\s*secrets:\s*inherit\s*$') {
    throw "Release signing secrets must come from channel-specific GitHub Environments."
}
foreach ($workflow in @($stable, $beta)) {
    if ($workflow -notmatch 'artifacts/release/\*\.json') {
        throw "Stable and beta releases must attest and publish release metadata."
    }
}
foreach ($workflow in @($ci, $stable, $beta, $shared)) {
    if ($workflow -match 'actions/(checkout|setup-dotnet|upload-artifact|download-artifact|cache)@v\d') {
        throw "Official actions must remain pinned to immutable commit SHAs."
    }
}

Write-Host "release workflow guard passed" -ForegroundColor Green
