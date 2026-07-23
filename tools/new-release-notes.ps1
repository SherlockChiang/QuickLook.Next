param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$PreviousTag = "",
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$range = if ($PreviousTag) { "$PreviousTag..HEAD" } else { "HEAD" }
$records = (git log $range --format='%s%x1f%b%x1e' --no-merges) -join "`n"
if ($LASTEXITCODE -ne 0) { throw "Could not read commits for release notes." }

$groups = [ordered]@{
    "New features" = [Collections.Generic.List[string]]::new()
    "Fixes" = [Collections.Generic.List[string]]::new()
    "Performance" = [Collections.Generic.List[string]]::new()
    "Other changes" = [Collections.Generic.List[string]]::new()
}
foreach ($record in $records.Split([char]0x1e, [StringSplitOptions]::RemoveEmptyEntries)) {
    $parts = $record.Trim().Split([char]0x1f, 2)
    $subject = $parts[0].Trim()
    $body = if ($parts.Count -gt 1) { $parts[1] } else { "" }
    $releaseNote = [regex]::Match($body, '(?im)^Release-Note:\s*(.+)$')
    if ($releaseNote.Success -and $releaseNote.Groups[1].Value.Trim() -eq 'skip') { continue }
    $explicitNote = if ($releaseNote.Success) { $releaseNote.Groups[1].Value.Trim() } else { "" }
    $heading = switch -Regex ($subject) {
        '^feat(?:\([^)]*\))?:\s*' { "New features"; break }
        '^fix(?:\([^)]*\))?:\s*' { "Fixes"; break }
        '^perf(?:\([^)]*\))?:\s*' { "Performance"; break }
        '^(docs|test|ci|chore|build)(?:\([^)]*\))?:\s*' {
            if ($explicitNote) { "Other changes" } else { "" }
            break
        }
        default { "Other changes" }
    }
    if (-not $heading) { continue }
    $summary = if ($explicitNote) { $explicitNote } else { $subject -replace '^[a-z]+(?:\([^)]*\))?!?:\s*', '' }
    $groups[$heading].Add($summary)
}

$lines = [Collections.Generic.List[string]]::new()
$lines.Add("# QuickLook Next $Version")
$lines.Add("")
foreach ($group in $groups.GetEnumerator()) {
    if ($group.Value.Count -eq 0) { continue }
    $lines.Add("## $($group.Key)")
    $lines.Add("")
    foreach ($summary in $group.Value) { $lines.Add("- $summary") }
    $lines.Add("")
}
if ($Repository -and $PreviousTag) {
    $lines.Add("**Full changelog:** https://github.com/$Repository/compare/$PreviousTag...v$Version")
    $lines.Add("")
}
$lines.Add("Release assets are signed and include GitHub artifact attestations.")

$parent = Split-Path $OutputPath -Parent
if ($parent) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
$lines | Set-Content -LiteralPath $OutputPath -Encoding utf8
