param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "quicklook-next-release-version-test-" + [Guid]::NewGuid().ToString("N"))

function Assert-Rejected([scriptblock]$Action, [string]$Scenario) {
    $rejected = $false
    try {
        & $Action | Out-Null
    }
    catch {
        $rejected = $true
    }
    if (-not $rejected) {
        throw "$Scenario must be rejected."
    }
}

try {
    [IO.Directory]::CreateDirectory(
        (Join-Path $fixtureRoot "native\quicklook_next_native")) | Out-Null
    $versionPath = Join-Path $fixtureRoot "VERSION"
    $manifestPath = Join-Path (
        $fixtureRoot) "native\quicklook_next_native\Cargo.toml"
    $lockPath = Join-Path $fixtureRoot "native\Cargo.lock"
    [IO.File]::WriteAllText($versionPath, "1.2.3`n")
    [IO.File]::WriteAllText(
        $manifestPath,
        "[package]`nname = `"quicklook_next_native`"`nversion = `"1.2.3`"`n")
    $validLock = (
        "version = 4`n`n[[package]]`n" +
        "name = `"quicklook_next_native`"`nversion = `"1.2.3`"`n")
    [IO.File]::WriteAllText($lockPath, $validLock)
    [IO.File]::WriteAllText(
        (Join-Path $fixtureRoot "Directory.Build.props"),
        "<Project><PropertyGroup><VersionPrefix Condition=`"true`">" +
            "ReadAllText</VersionPrefix></PropertyGroup></Project>")

    $testVersion = Join-Path $Root "tools\test-release-version.ps1"
    & $testVersion -Root $fixtureRoot -ExpectedVersion "1.2.3"

    [IO.File]::AppendAllText(
        $manifestPath,
        "`n[package]`nname = `"duplicate`"`nversion = `"9.9.9`"`n")
    Assert-Rejected {
        & $testVersion -Root $fixtureRoot -ExpectedVersion "1.2.3"
    } "Duplicate Cargo [package] sections"

    [IO.File]::WriteAllText(
        $manifestPath,
        "[package]`nname = `"quicklook_next_native`"`nversion = `"1.2.3`"`n")
    [IO.File]::WriteAllText(
        $lockPath,
        $validLock +
            "`n[[package]]`nname = `"quicklook_next_native`"`n" +
            "version = `"1.2.3`"`n")
    Assert-Rejected {
        & $testVersion -Root $fixtureRoot -ExpectedVersion "1.2.3"
    } "Duplicate native Cargo.lock packages"
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
    if ($resolvedFixture.StartsWith(
            $resolvedTemp,
            [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolvedFixture).StartsWith(
            "quicklook-next-release-version-test-",
            [StringComparison]::Ordinal)) {
        [IO.Directory]::Delete($resolvedFixture, $true)
    }
}

Write-Host "release version structure test passed" -ForegroundColor Green
