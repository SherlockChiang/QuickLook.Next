param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$PreviousTag = "",
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$range = if ($PreviousTag) { "$PreviousTag..HEAD" } else { "HEAD" }
$gitOutput = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-git-log-" + [guid]::NewGuid().ToString('N'))
try {
    git log $range --format='%s%x1f%b%x1e' --encoding=UTF-8 --no-merges "--output=$gitOutput"
    if ($LASTEXITCODE -ne 0) { throw "Could not read commits for release notes." }
    $records = [IO.File]::ReadAllText($gitOutput, [Text.Encoding]::UTF8)
}
finally {
    Remove-Item -LiteralPath $gitOutput -Force -ErrorAction SilentlyContinue
}

$groups = [ordered]@{
    "New features" = [Collections.Generic.List[string]]::new()
    "Fixes" = [Collections.Generic.List[string]]::new()
    "Performance" = [Collections.Generic.List[string]]::new()
    "Other changes" = [Collections.Generic.List[string]]::new()
}
$chineseNotes = [Collections.Generic.List[string]]::new()
$commitCount = 0
$visibleCount = 0
foreach ($record in $records.Split([char]0x1e, [StringSplitOptions]::RemoveEmptyEntries)) {
    $record = $record.Trim()
    if (-not $record) { continue }
    $commitCount++
    $parts = $record.Split([char]0x1f, 2)
    $subject = $parts[0].Trim()
    $body = if ($parts.Count -gt 1) { $parts[1] } else { "" }
    $releaseNote = [regex]::Match($body, '(?im)^Release-Note:\s*(.+)$')
    $releaseNoteZh = [regex]::Match($body, '(?im)^Release-Note-ZH:\s*(.+)$')
    if ($releaseNote.Success -and $releaseNote.Groups[1].Value.Trim() -eq 'skip') { continue }
    if ($releaseNoteZh.Success -and $releaseNoteZh.Groups[1].Value.Trim() -ne 'skip') {
        $chineseNotes.Add($releaseNoteZh.Groups[1].Value.Trim())
    }
    $explicitNote = if ($releaseNote.Success) { $releaseNote.Groups[1].Value.Trim() } else { "" }
    $heading = switch -Regex ($subject) {
        '^feat(?:\([^)]*\))?:\s*' { "New features"; break }
        '^fix(?:\([^)]*\))?:\s*' { "Fixes"; break }
        '^perf(?:\([^)]*\))?:\s*' { "Performance"; break }
        '^(docs|test|ci|chore|build|release)(?:\([^)]*\))?:\s*' {
            if ($explicitNote) { "Other changes" } else { "" }
            break
        }
        default { "Other changes" }
    }
    if (-not $heading) { continue }
    $summary = if ($explicitNote) { $explicitNote } else { $subject -replace '^[a-z]+(?:\([^)]*\))?!?:\s*', '' }
    $groups[$heading].Add($summary)
    $visibleCount++
}

$lines = [Collections.Generic.List[string]]::new()
$lines.Add("# QuickLook Next $Version")
$lines.Add("")
$lines.Add("## 更新内容")
$lines.Add("")
if ($chineseNotes.Count -gt 0) {
    foreach ($summary in $chineseNotes) { $lines.Add("- $summary") }
}
else {
    $lines.Add("- 本版本的详细更新记录见下方英文列表。")
}
$lines.Add("")
$lines.Add("## Changes")
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

[pscustomobject]@{
    CommitCount = $commitCount
    UserVisibleChangeCount = $visibleCount
    ChineseNoteCount = $chineseNotes.Count
}
