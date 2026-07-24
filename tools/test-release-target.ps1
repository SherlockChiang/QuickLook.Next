param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [string]$Commit = "HEAD",
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$expectedCommit = (git rev-parse $Commit).Trim()
if ($LASTEXITCODE -ne 0) { throw "Could not resolve release commit $Commit." }
$existingTagCommit = git rev-list -n 1 $Tag 2>$null
if ($LASTEXITCODE -eq 0 -and $existingTagCommit -and $existingTagCommit.Trim() -ne $expectedCommit) {
    throw "Tag $Tag already points to a different commit."
}
if ($Repository -and $env:GH_TOKEN) {
    $release = gh api "repos/$Repository/releases/tags/$Tag" 2>$null | ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -and $release.target_commitish -and
        $release.target_commitish -notin @($expectedCommit, $Commit, 'main')) {
        throw "GitHub Release $Tag targets a different commit."
    }
}

[pscustomobject]@{
    Tag = $Tag
    Commit = $expectedCommit
    Reused = [bool]$existingTagCommit
}
