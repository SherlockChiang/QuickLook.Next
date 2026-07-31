param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "quicklook-next-version-test-" + [Guid]::NewGuid().ToString("N"))

try {
    [IO.Directory]::CreateDirectory(
        (Join-Path $fixtureRoot "native\quicklook_next_native")) | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $fixtureRoot "VERSION"),
        "1.2.3.0`n")
    [IO.File]::WriteAllText(
        (Join-Path $fixtureRoot "native\quicklook_next_native\Cargo.toml"),
        "[package]`nname = `"quicklook_next_native`"`nversion = `"1.2.3`"`n")
    [IO.File]::WriteAllText(
        (Join-Path $fixtureRoot "native\Cargo.lock"),
        "version = 4`n`n[[package]]`nname = `"quicklook_next_native`"`nversion = `"1.2.3`"`n")

    $setVersion = Join-Path $Root "tools\set-version.ps1"
    $rejected = $false
    try {
        & $setVersion -Root $fixtureRoot -Bump Patch | Out-Null
    }
    catch {
        $rejected = $true
        if ($_.Exception.Message -notmatch
            'MSIX fourth component is assigned automatically') {
            throw "A four-part VERSION must explain the automatic MSIX revision."
        }
    }
    if (-not $rejected) {
        throw "A four-part VERSION must not be accepted as a semantic version."
    }

    $version = & $setVersion -Root $fixtureRoot -Version "v2.4.6"
    if (@($version)[-1] -ne "2.4.6") {
        throw "Explicit version did not repair and return its normalized value."
    }
    $version = & $setVersion -Root $fixtureRoot -Bump Patch
    if (@($version)[-1] -ne "2.4.7") {
        throw "Patch bump did not return the incremented value."
    }

    $versionText = (Get-Content -LiteralPath (Join-Path $fixtureRoot "VERSION") -Raw).Trim()
    $manifestText = Get-Content -LiteralPath (
        Join-Path $fixtureRoot "native\quicklook_next_native\Cargo.toml") -Raw
    $lockText = Get-Content -LiteralPath (Join-Path $fixtureRoot "native\Cargo.lock") -Raw
    if ($versionText -ne "2.4.7" -or
        $manifestText -notmatch '(?m)^version\s*=\s*"2\.4\.7"$' -or
        $lockText -notmatch '(?ms)\[\[package\]\]\r?\nname\s*=\s*"quicklook_next_native"\r?\nversion\s*=\s*"2\.4\.7"')
    {
        throw "VERSION, Cargo.toml, and Cargo.lock were not synchronized."
    }

    $rejected = $false
    try {
        & $setVersion -Root $fixtureRoot -Version "2.4.8" -Bump Minor | Out-Null
    }
    catch {
        $rejected = $true
    }
    if (-not $rejected) {
        throw "Version and Bump must remain mutually exclusive."
    }

    $versionBeforeInvalid = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "VERSION"))
    $lockBeforeInvalid = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "native\Cargo.lock"))
    [IO.File]::WriteAllText(
        (Join-Path $fixtureRoot "native\quicklook_next_native\Cargo.toml"),
        "[package]`nname = `"quicklook_next_native`"`n`n" +
            "[dependencies]`nversion = `"9.9.9`"`n")
    $rejected = $false
    try {
        & $setVersion -Root $fixtureRoot -Version "3.0.0" | Out-Null
    }
    catch {
        $rejected = $true
    }
    $versionAfterInvalid = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "VERSION"))
    $lockAfterInvalid = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "native\Cargo.lock"))
    if ((-not $rejected) -or
        ($versionAfterInvalid -ne $versionBeforeInvalid) -or
        ($lockAfterInvalid -ne $lockBeforeInvalid)) {
        throw "Version preflight failure must not partially update other version sources."
    }

    $manifestPath = Join-Path (
        $fixtureRoot) "native\quicklook_next_native\Cargo.toml"
    [IO.File]::WriteAllText(
        $manifestPath,
        "[package]`nname = `"quicklook_next_native`"`nversion = `"2.4.7`"`n")
    $versionBeforeWriteFailure = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "VERSION"))
    $manifestBeforeWriteFailure = [IO.File]::ReadAllText($manifestPath)
    $lockBeforeWriteFailure = [IO.File]::ReadAllText(
        (Join-Path $fixtureRoot "native\Cargo.lock"))
    [IO.File]::SetAttributes(
        $manifestPath,
        [IO.FileAttributes]::ReadOnly)
    $rejected = $false
    try {
        try {
            & $setVersion -Root $fixtureRoot -Version "3.0.0" | Out-Null
        }
        catch {
            $rejected = $true
        }
    }
    finally {
        [IO.File]::SetAttributes(
            $manifestPath,
            [IO.FileAttributes]::Normal)
    }
    if ((-not $rejected) -or
        ([IO.File]::ReadAllText(
                (Join-Path $fixtureRoot "VERSION")) -ne
            $versionBeforeWriteFailure) -or
        ([IO.File]::ReadAllText($manifestPath) -ne
            $manifestBeforeWriteFailure) -or
        ([IO.File]::ReadAllText(
                (Join-Path $fixtureRoot "native\Cargo.lock")) -ne
            $lockBeforeWriteFailure)) {
        throw "A write failure must roll back every version source."
    }
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
    if ($resolvedFixture.StartsWith(
            $resolvedTemp,
            [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolvedFixture).StartsWith(
            "quicklook-next-version-test-",
            [StringComparison]::Ordinal))
    {
        [IO.Directory]::Delete($resolvedFixture, $true)
    }
}

Write-Host "set-version workflow test passed" -ForegroundColor Green
