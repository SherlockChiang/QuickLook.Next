param(
    [Parameter(Mandatory = $true)]
    [string]$VersionPrefix,

    [string]$VersionSuffix = ""
)

$ErrorActionPreference = "Stop"

if ($VersionPrefix -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') {
    throw "VersionPrefix must use strict X.Y.Z format."
}

$parts = @()
foreach ($component in $VersionPrefix.Split('.')) {
    [UInt64]$parsed = 0
    if (-not [UInt64]::TryParse(
            $component,
            [Globalization.NumberStyles]::None,
            [Globalization.CultureInfo]::InvariantCulture,
            [ref]$parsed) -or
        $parsed -gt 65535)
    {
        throw "VersionPrefix components must be within 0..65535."
    }
    $parts += [int]$parsed
}

$revision = 65535
if ($VersionSuffix) {
    $match = [regex]::Match($VersionSuffix, '^beta\.([0-9]+)$')
    if (-not $match.Success) {
        throw "Packaged prerelease suffixes must use beta.N format."
    }

    [UInt64]$runNumber = 0
    if (-not [UInt64]::TryParse(
            $match.Groups[1].Value,
            [Globalization.NumberStyles]::None,
            [Globalization.CultureInfo]::InvariantCulture,
            [ref]$runNumber) -or
        $runNumber -lt 1 -or
        $runNumber -gt 32767)
    {
        throw "The beta sequence must be within 1..32767."
    }
    $revision = 32767 + [int]$runNumber
}

"$($parts[0]).$($parts[1]).$($parts[2]).$revision"
