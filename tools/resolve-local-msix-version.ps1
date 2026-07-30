param(
    [Parameter(Mandatory = $true)]
    [string]$VersionPrefix,

    [string]$InstalledVersion = "",

    [string]$InstalledPublisher = "",

    [string]$ExpectedPublisher = "CN=QuickLook Next Development",

    [string[]]$KnownVersions = @()
)

$ErrorActionPreference = "Stop"
$LocalRevisionMax = 32767

function ConvertTo-BoundedVersionParts {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Value,

        [Parameter(Mandatory = $true)]
        [ValidateSet(3, 4)]
        [int]$ComponentCount,

        [Parameter(Mandatory = $true)]
        [string]$ParameterName
    )

    $pattern = if ($ComponentCount -eq 3) {
        '^[0-9]+\.[0-9]+\.[0-9]+$'
    }
    else {
        '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$'
    }
    if ($Value -notmatch $pattern) {
        throw "$ParameterName must use strict X.Y.Z" +
            $(if ($ComponentCount -eq 4) { ".W" } else { "" }) +
            " format. Current value: '$Value'"
    }

    $parts = @()
    foreach ($component in $Value.Split('.')) {
        [UInt64]$parsed = 0
        if (-not [UInt64]::TryParse(
                $component,
                [Globalization.NumberStyles]::None,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$parsed) -or
            $parsed -gt 65535)
        {
            throw "$ParameterName components must be within 0..65535. " +
                "Current value: '$Value'"
        }
        $parts += [int]$parsed
    }

    return $parts
}

function Compare-VersionBase {
    param(
        [Parameter(Mandatory = $true)]
        [int[]]$Left,

        [Parameter(Mandatory = $true)]
        [int[]]$Right
    )

    for ($index = 0; $index -lt 3; $index++) {
        if ($Left[$index] -lt $Right[$index]) {
            return -1
        }
        if ($Left[$index] -gt $Right[$index]) {
            return 1
        }
    }
    return 0
}

if ([string]::IsNullOrWhiteSpace($ExpectedPublisher)) {
    throw "ExpectedPublisher must not be empty."
}

$hasInstalledVersion = -not [string]::IsNullOrEmpty($InstalledVersion)
$hasInstalledPublisher = -not [string]::IsNullOrEmpty($InstalledPublisher)
if ($hasInstalledVersion -ne $hasInstalledPublisher) {
    throw "InstalledVersion and InstalledPublisher must be supplied together."
}
if ($hasInstalledPublisher -and
    -not [string]::Equals(
        $InstalledPublisher,
        $ExpectedPublisher,
        [StringComparison]::Ordinal))
{
    throw "The installed package publisher '$InstalledPublisher' does not " +
        "match the expected publisher '$ExpectedPublisher'."
}

$sourceParts = @(ConvertTo-BoundedVersionParts `
    -Value $VersionPrefix `
    -ComponentCount 3 `
    -ParameterName "VersionPrefix")

$installedParts = $null
if ($hasInstalledVersion) {
    $installedParts = @(ConvertTo-BoundedVersionParts `
        -Value $InstalledVersion `
        -ComponentCount 4 `
        -ParameterName "InstalledVersion")
}

$highestKnownRevision = -1
foreach ($knownVersion in @($KnownVersions)) {
    $knownParts = @(ConvertTo-BoundedVersionParts `
        -Value ([string]$knownVersion) `
        -ComponentCount 4 `
        -ParameterName "KnownVersions")
    if ((Compare-VersionBase -Left $knownParts -Right $sourceParts) -eq 0 -and
        $knownParts[3] -gt $highestKnownRevision)
    {
        $highestKnownRevision = $knownParts[3]
    }
}

$revisionFloor = [Math]::Max(0, $highestKnownRevision)
if ($null -ne $installedParts) {
    $installedBaseComparison = Compare-VersionBase `
        -Left $installedParts `
        -Right $sourceParts
    if ($installedBaseComparison -gt 0) {
        throw "InstalledVersion '$InstalledVersion' has a newer base version " +
            "than VersionPrefix '$VersionPrefix'."
    }
    if ($installedBaseComparison -eq 0 -and
        $installedParts[3] -gt $revisionFloor)
    {
        $revisionFloor = $installedParts[3]
    }
}

if ($revisionFloor -ge $LocalRevisionMax) {
    throw "No local MSIX revision remains for VersionPrefix '$VersionPrefix'. " +
        "Revisions above $LocalRevisionMax are reserved for beta and stable " +
        "packages; bump VERSION before installing another local build."
}

$revision = $revisionFloor + 1
"$($sourceParts[0]).$($sourceParts[1]).$($sourceParts[2]).$revision"
