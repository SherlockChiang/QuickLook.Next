param(
    [Parameter(Mandatory = $true)]
    [string]$VersionPrefix
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

if ($parts[0] -eq 0) {
    throw (
        "Microsoft Store package versions require a non-zero first component " +
        "and reserve the fourth component for Store use. Bump the product " +
        "version before creating a Store package: $VersionPrefix.")
}

"$($parts[0]).$($parts[1]).$($parts[2]).0"
