param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = 'Stop'
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-release-notes-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    git -C $tempRoot init --quiet
    git -C $tempRoot config user.name 'Release Notes Test'
    git -C $tempRoot config user.email 'release-notes@example.invalid'
    Set-Content -LiteralPath (Join-Path $tempRoot 'fixture.txt') -Value 'one' -Encoding ascii
    git -C $tempRoot add fixture.txt
    git -C $tempRoot commit --quiet -m 'chore: baseline'
    git -C $tempRoot tag v1.0.0

    Add-Content -LiteralPath (Join-Path $tempRoot 'fixture.txt') -Value 'two'
    git -C $tempRoot add fixture.txt
    $messagePath = Join-Path $tempRoot 'commit-message.txt'
    @('ci: internal work', '', 'Release-Note: Visible maintenance', 'Release-Note-ZH: 可见维护') |
        Set-Content -LiteralPath $messagePath -Encoding utf8
    git -C $tempRoot commit --quiet -F $messagePath
    Add-Content -LiteralPath (Join-Path $tempRoot 'fixture.txt') -Value 'three'
    git -C $tempRoot add fixture.txt
    git -C $tempRoot commit --quiet -m 'fix: hidden fix' -m 'Release-Note: skip'
    Add-Content -LiteralPath (Join-Path $tempRoot 'fixture.txt') -Value 'release'
    git -C $tempRoot add fixture.txt
    git -C $tempRoot commit --quiet -m 'release: publish fixture'

    Push-Location $tempRoot
    try {
        $output = Join-Path $tempRoot 'notes.md'
        $result = & (Join-Path $Root 'tools\new-release-notes.ps1') `
            -Version '1.0.1' -PreviousTag 'v1.0.0' -OutputPath $output
    }
    finally { Pop-Location }

    if ($result.CommitCount -ne 3 -or $result.UserVisibleChangeCount -ne 1 -or $result.ChineseNoteCount -ne 1) {
        throw "Release note counts are incorrect (commits=$($result.CommitCount), visible=$($result.UserVisibleChangeCount), Chinese=$($result.ChineseNoteCount))."
    }
    $notes = Get-Content -LiteralPath $output -Raw
    $hasEnglish = $notes -match 'Visible maintenance'
    $hasChinese = $notes -match '可见维护'
    $hasSkipped = $notes -match 'hidden fix'
    $hasReleaseCommit = $notes -match 'publish fixture'
    if (-not $hasEnglish -or -not $hasChinese -or $hasSkipped -or $hasReleaseCommit) {
        throw "Release note footer or skip behavior is incorrect (English=$hasEnglish, Chinese=$hasChinese, Skipped=$hasSkipped, ReleaseCommit=$hasReleaseCommit)."
    }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'release notes tests passed' -ForegroundColor Green
