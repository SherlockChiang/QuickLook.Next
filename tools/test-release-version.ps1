param(
    [string]$ExpectedVersion = "",
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$root = $Root
$version = (Get-Content -LiteralPath (Join-Path $root "VERSION") -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw "VERSION must use semantic X.Y.Z format. Current value: '$version'"
}
if ($ExpectedVersion -and $version -ne $ExpectedVersion.TrimStart('v')) {
    throw "Expected version '$ExpectedVersion' does not match VERSION ($version)."
}

$cargoManifest = Get-Content -LiteralPath (
    Join-Path $root "native\quicklook_next_native\Cargo.toml") -Raw
$cargoPackageSections = [regex]::Matches(
    $cargoManifest,
    '(?ms)^[ \t]*\[package\][ \t]*(?:#.*)?\r?$(?<body>(?:(?!^[ \t]*\[{1,2}[^\]\r\n]+\]{1,2}[ \t]*(?:#.*)?\r?$).)*)')
if ($cargoPackageSections.Count -ne 1) {
    throw "Cargo manifest must contain exactly one [package] section."
}
$cargoVersionMatches = [regex]::Matches(
    $cargoPackageSections[0].Groups["body"].Value,
    '^[ \t]*version[ \t]*=[ \t]*"([^"\r\n]+)"[^\r\n]*$',
    [Text.RegularExpressions.RegexOptions]::Multiline)
if ($cargoVersionMatches.Count -ne 1) {
    throw "Cargo [package] section must contain exactly one version."
}
$cargoVersion = $cargoVersionMatches[0].Groups[1].Value
if ($cargoVersion -ne $version) {
    throw "Cargo version ($cargoVersion) must match release version ($version)."
}

$cargoLock = Get-Content -LiteralPath (Join-Path $root "native\Cargo.lock") -Raw
$cargoLockSections = [regex]::Matches(
    $cargoLock,
    '(?ms)^[ \t]*\[\[package\]\][ \t]*\r?$(?<body>(?:(?!^[ \t]*\[{1,2}[^\]\r\n]+\]{1,2}[ \t]*(?:#.*)?\r?$).)*)')
$nativeLockSections = @(
    $cargoLockSections | Where-Object {
        [regex]::IsMatch(
            $_.Groups["body"].Value,
            '^[ \t]*name[ \t]*=[ \t]*"quicklook_next_native"[ \t]*\r?$',
            [Text.RegularExpressions.RegexOptions]::Multiline)
    }
)
if ($nativeLockSections.Count -ne 1) {
    throw ("Cargo lock must contain exactly one quicklook_next_native " +
        "package; found $($nativeLockSections.Count).")
}
$cargoLockVersionMatches = [regex]::Matches(
    $nativeLockSections[0].Groups["body"].Value,
    '^[ \t]*version[ \t]*=[ \t]*"([^"\r\n]+)"[^\r\n]*$',
    [Text.RegularExpressions.RegexOptions]::Multiline)
if ($cargoLockVersionMatches.Count -ne 1) {
    throw "Cargo lock package must contain exactly one version."
}
$cargoLockVersion = $cargoLockVersionMatches[0].Groups[1].Value
if ($cargoLockVersion -ne $version) {
    throw "Cargo lock version ($cargoLockVersion) must match release version ($version)."
}

$props = [xml](Get-Content -LiteralPath (Join-Path $root "Directory.Build.props") -Raw)
$versionPrefix = @($props.Project.PropertyGroup.VersionPrefix) | Select-Object -First 1
if (-not $versionPrefix -or $versionPrefix.'#text' -notmatch 'ReadAllText') {
    throw "Directory.Build.props must derive VersionPrefix from VERSION."
}

Write-Host "Release version is consistent: $version" -ForegroundColor Green
