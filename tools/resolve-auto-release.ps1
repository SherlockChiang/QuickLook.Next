param(
    [string]$VersionFile = (Join-Path (Split-Path $PSScriptRoot -Parent) "VERSION")
)

$ErrorActionPreference = "Stop"
$baseVersion = (Get-Content -LiteralPath $VersionFile -Raw).Trim()
if ($baseVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "VERSION must use semantic X.Y.Z format. Current value: '$baseVersion'"
}

$stableTags = @(git tag --list "v[0-9]*.[0-9]*.[0-9]*" --sort=-version:refname)
if ($LASTEXITCODE -ne 0) { throw "Could not enumerate release tags." }
$latestTag = $stableTags | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' } | Select-Object -First 1
# VERSION is a floor for the next stable release; published tags remain the source of truth.
$candidate = [version]$baseVersion
if ($latestTag) {
    $latestVersion = [version]$latestTag.Substring(1)
    $nextVersion = [version]::new($latestVersion.Major, $latestVersion.Minor, $latestVersion.Build + 1)
    if ($nextVersion -gt $candidate) { $candidate = $nextVersion }
}

$version = "$($candidate.Major).$($candidate.Minor).$($candidate.Build)"
[pscustomobject]@{
    Version = $version
    Tag = "v$version"
    PreviousTag = $latestTag ?? ""
}
