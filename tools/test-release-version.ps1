param(
    [string]$ExpectedVersion = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$version = (Get-Content -LiteralPath (Join-Path $root "VERSION") -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw "VERSION must use semantic X.Y.Z format. Current value: '$version'"
}
if ($ExpectedVersion -and $version -ne $ExpectedVersion.TrimStart('v')) {
    throw "Expected version '$ExpectedVersion' does not match VERSION ($version)."
}

$cargoManifest = Get-Content -LiteralPath (Join-Path $root "native\quicklook_next_native\Cargo.toml") -Raw
if ($cargoManifest -notmatch '(?m)^version\s*=\s*"([^\"]+)"') {
    throw "Cargo package version was not found."
}
if ($Matches[1] -ne $version) {
    throw "Cargo version ($($Matches[1])) does not match VERSION ($version)."
}

$props = [xml](Get-Content -LiteralPath (Join-Path $root "Directory.Build.props") -Raw)
$versionPrefix = @($props.Project.PropertyGroup.VersionPrefix) | Select-Object -First 1
if (-not $versionPrefix -or $versionPrefix.'#text' -notmatch 'ReadAllText') {
    throw "Directory.Build.props must derive VersionPrefix from VERSION."
}

Write-Host "Release version is consistent: $version" -ForegroundColor Green
