param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$PreviousTag = "",
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$range = if ($PreviousTag) { "$PreviousTag..HEAD" } else { "HEAD" }
$subjects = @(git log $range --format=%s --no-merges)
if ($LASTEXITCODE -ne 0) { throw "Could not read commits for release notes." }

$groups = [ordered]@{
    "New features" = [Collections.Generic.List[string]]::new()
    "Fixes" = [Collections.Generic.List[string]]::new()
    "Performance" = [Collections.Generic.List[string]]::new()
    "Documentation" = [Collections.Generic.List[string]]::new()
    "Other changes" = [Collections.Generic.List[string]]::new()
}
foreach ($subject in $subjects) {
    $heading = switch -Regex ($subject) {
        '^feat(?:\([^)]*\))?:\s*' { "New features"; break }
        '^fix(?:\([^)]*\))?:\s*' { "Fixes"; break }
        '^perf(?:\([^)]*\))?:\s*' { "Performance"; break }
        '^docs(?:\([^)]*\))?:\s*' { "Documentation"; break }
        default { "Other changes" }
    }
    $summary = $subject -replace '^[a-z]+(?:\([^)]*\))?!?:\s*', ''
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
