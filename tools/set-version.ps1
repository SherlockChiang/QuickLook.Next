[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Position = 0)]
    [string]$Version = "",
    [ValidateSet("None", "Patch", "Minor", "Major")]
    [string]$Bump = "None",
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($Version -and $Bump -ne "None") {
    throw "Version and Bump are mutually exclusive."
}

$versionPath = Join-Path $Root "VERSION"
$cargoManifestPath = Join-Path $Root "native\quicklook_next_native\Cargo.toml"
$cargoLockPath = Join-Path $Root "native\Cargo.lock"
foreach ($path in @($versionPath, $cargoManifestPath, $cargoLockPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Version source is missing: $path"
    }
}

function ConvertTo-VersionParts([string]$Value) {
    $normalized = $Value.Trim()
    if ($normalized.StartsWith("v", [StringComparison]::OrdinalIgnoreCase)) {
        $normalized = $normalized.Substring(1)
    }
    if ($normalized -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        throw "Version must use semantic X.Y.Z format. Current value: '$Value'"
    }
    $parts = @(
        [int]::Parse($Matches[1], [Globalization.CultureInfo]::InvariantCulture),
        [int]::Parse($Matches[2], [Globalization.CultureInfo]::InvariantCulture),
        [int]::Parse($Matches[3], [Globalization.CultureInfo]::InvariantCulture)
    )
    if ($parts | Where-Object { $_ -lt 0 -or $_ -gt 65535 }) {
        throw "Version components must remain in the MSIX range 0..65535."
    }
    return $parts
}

$currentVersion = (Get-Content -LiteralPath $versionPath -Raw).Trim()
$currentParts = ConvertTo-VersionParts $currentVersion
$targetParts = if ($Version) {
    ConvertTo-VersionParts $Version
} else {
    @($currentParts)
}

switch ($Bump) {
    "Patch" {
        if ($targetParts[2] -ge 65535) { throw "Patch version cannot be incremented beyond 65535." }
        $targetParts[2]++
    }
    "Minor" {
        if ($targetParts[1] -ge 65535) { throw "Minor version cannot be incremented beyond 65535." }
        $targetParts[1]++
        $targetParts[2] = 0
    }
    "Major" {
        if ($targetParts[0] -ge 65535) { throw "Major version cannot be incremented beyond 65535." }
        $targetParts[0]++
        $targetParts[1] = 0
        $targetParts[2] = 0
    }
}

$targetVersion = $targetParts -join "."
$utf8NoBom = [Text.UTF8Encoding]::new($false)

function Get-TomlSection(
    [string]$Text,
    [string]$HeaderPattern,
    [string]$Description)
{
    $headerRegex = [regex]::new(
        $HeaderPattern,
        [Text.RegularExpressions.RegexOptions]::Multiline)
    $headers = $headerRegex.Matches($Text)
    if ($headers.Count -ne 1) {
        throw "$Description must occur exactly once; found $($headers.Count)."
    }

    $header = $headers[0]
    $sectionStart = $header.Index + $header.Length
    $nextSection = [regex]::new(
        '^[ \t]*\[{1,2}[^\]\r\n]+\]{1,2}[ \t]*(?:#.*)?\r?$',
        [Text.RegularExpressions.RegexOptions]::Multiline).Match(
            $Text,
            $sectionStart)
    $sectionEnd = if ($nextSection.Success) {
        $nextSection.Index
    } else {
        $Text.Length
    }

    return [pscustomobject]@{
        Start = $sectionStart
        Length = $sectionEnd - $sectionStart
        Text = $Text.Substring($sectionStart, $sectionEnd - $sectionStart)
    }
}

function Set-SingleVersionInSection(
    [string]$Document,
    [pscustomobject]$Section,
    [string]$Description)
{
    $versionRegex = [regex]::new(
        '^([ \t]*version[ \t]*=[ \t]*")[^"\r\n]+("[^\r\n]*)(\r?)$',
        [Text.RegularExpressions.RegexOptions]::Multiline)
    $matches = $versionRegex.Matches($Section.Text)
    if ($matches.Count -ne 1) {
        throw "$Description must contain exactly one version entry; found $($matches.Count)."
    }

    $updatedSection = $versionRegex.Replace(
        $Section.Text,
        [Text.RegularExpressions.MatchEvaluator] {
            param($match)
            $match.Groups[1].Value +
                $targetVersion +
                $match.Groups[2].Value +
                $match.Groups[3].Value
        })
    return $Document.Substring(0, $Section.Start) +
        $updatedSection +
        $Document.Substring($Section.Start + $Section.Length)
}

function Get-UpdatedCargoManifest([string]$Text) {
    $packageSection = Get-TomlSection `
        -Text $Text `
        -HeaderPattern '^[ \t]*\[package\][ \t]*(?:#.*)?\r?$' `
        -Description "Cargo [package] section"
    return Set-SingleVersionInSection `
        -Document $Text `
        -Section $packageSection `
        -Description "Cargo [package] section"
}

function Get-UpdatedCargoLock([string]$Text) {
    $packageHeaderRegex = [regex]::new(
        '^[ \t]*\[\[package\]\][ \t]*\r?$',
        [Text.RegularExpressions.RegexOptions]::Multiline)
    $packageHeaders = $packageHeaderRegex.Matches($Text)
    $matchingSections = [Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $packageHeaders.Count; $index++) {
        $sectionStart = $packageHeaders[$index].Index +
            $packageHeaders[$index].Length
        $nextSection = [regex]::new(
            '^[ \t]*\[{1,2}[^\]\r\n]+\]{1,2}[ \t]*(?:#.*)?\r?$',
            [Text.RegularExpressions.RegexOptions]::Multiline).Match(
                $Text,
                $sectionStart)
        $sectionEnd = if ($nextSection.Success) {
            $nextSection.Index
        } else {
            $Text.Length
        }
        $sectionText = $Text.Substring(
            $sectionStart,
            $sectionEnd - $sectionStart)
        $nameMatches = [regex]::Matches(
            $sectionText,
            '^[ \t]*name[ \t]*=[ \t]*"quicklook_next_native"[ \t]*\r?$',
            [Text.RegularExpressions.RegexOptions]::Multiline)
        if ($nameMatches.Count -gt 1) {
            throw "Cargo lock package contains duplicate name entries."
        }
        if ($nameMatches.Count -eq 1) {
            $matchingSections.Add([pscustomobject]@{
                Start = $sectionStart
                Length = $sectionEnd - $sectionStart
                Text = $sectionText
            })
        }
    }

    if ($matchingSections.Count -ne 1) {
        throw ("Cargo lock file must contain exactly one " +
            "quicklook_next_native package; found $($matchingSections.Count).")
    }
    return Set-SingleVersionInSection `
        -Document $Text `
        -Section $matchingSections[0] `
        -Description "Cargo lock quicklook_next_native package"
}

function Set-TextAtomically([string]$Path, [string]$Text) {
    $temporaryPath = "$Path.version-$([Guid]::NewGuid().ToString('N')).tmp"
    $backupPath = "$Path.version-$([Guid]::NewGuid().ToString('N')).bak"
    try {
        [IO.File]::WriteAllText($temporaryPath, $Text, $utf8NoBom)
        [IO.File]::Replace($temporaryPath, $Path, $backupPath, $true)
    }
    finally {
        if ([IO.File]::Exists($temporaryPath)) {
            [IO.File]::Delete($temporaryPath)
        }
        if ([IO.File]::Exists($backupPath)) {
            [IO.File]::Delete($backupPath)
        }
    }
}

$currentVersionText = [IO.File]::ReadAllText($versionPath)
$currentCargoManifest = [IO.File]::ReadAllText($cargoManifestPath)
$currentCargoLock = [IO.File]::ReadAllText($cargoLockPath)
$versionText = if ($currentVersion -eq $targetVersion) {
    $currentVersionText
} else {
    "$targetVersion`n"
}
$updatedCargoManifest = Get-UpdatedCargoManifest $currentCargoManifest
$updatedCargoLock = Get-UpdatedCargoLock $currentCargoLock

$updates = @(
    [pscustomobject]@{ Path = $versionPath; Before = $currentVersionText; After = $versionText },
    [pscustomobject]@{ Path = $cargoManifestPath; Before = $currentCargoManifest; After = $updatedCargoManifest },
    [pscustomobject]@{ Path = $cargoLockPath; Before = $currentCargoLock; After = $updatedCargoLock }
)
$pendingUpdates = @($updates | Where-Object { $_.Before -ne $_.After })
$approvedUpdates = @(
    foreach ($update in $pendingUpdates) {
        if ($PSCmdlet.ShouldProcess(
                $update.Path,
                "set version to $targetVersion")) {
            $update
        }
    }
)
if ($approvedUpdates.Count -ne 0 -and
    $approvedUpdates.Count -ne $pendingUpdates.Count) {
    throw "Version synchronization was canceled before any files were changed."
}

$appliedUpdates = [Collections.Generic.List[object]]::new()
try {
    foreach ($update in $approvedUpdates) {
        $appliedUpdates.Add($update)
        Set-TextAtomically $update.Path $update.After
        Write-Host "updated: $($update.Path)" -ForegroundColor DarkGray
    }
}
catch {
    $writeFailure = $_
    $rollbackFailures = [Collections.Generic.List[string]]::new()
    for ($index = $appliedUpdates.Count - 1; $index -ge 0; $index--) {
        try {
            Set-TextAtomically `
                $appliedUpdates[$index].Path `
                $appliedUpdates[$index].Before
        }
        catch {
            $rollbackFailures.Add(
                "$($appliedUpdates[$index].Path): $($_.Exception.Message)")
        }
    }
    if ($rollbackFailures.Count -gt 0) {
        throw ("Version synchronization failed and rollback was incomplete. " +
            "$($writeFailure.Exception.Message) Rollback errors: " +
            ($rollbackFailures -join "; "))
    }
    throw "Version synchronization failed; all prior writes were rolled back. $($writeFailure.Exception.Message)"
}

Write-Host "QuickLook Next version: $targetVersion" -ForegroundColor Green
Write-Output $targetVersion
