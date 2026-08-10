param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [string]$Commit = "HEAD",
    [string]$Repository = ""
)

$ErrorActionPreference = "Stop"
$expectedCommit = (git rev-parse $Commit).Trim()
if ($LASTEXITCODE -ne 0) { throw "Could not resolve release commit $Commit." }
$matchingTags = @(git tag --list -- $Tag)
if ($LASTEXITCODE -ne 0) { throw "Could not inspect release tag $Tag." }
$existingTagCommit = $null
if ($matchingTags -contains $Tag) {
    $existingTagCommit = (git rev-list -n 1 $Tag).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Could not resolve release tag $Tag." }
    if ($existingTagCommit -ne $expectedCommit) {
        throw "Tag $Tag already points to a different commit."
    }
}
if ($Repository -and $env:GH_TOKEN) {
    $apiOutput = @(& gh api "repos/$Repository/releases/tags/$Tag" 2>&1)
    $apiExitCode = $LASTEXITCODE
    $apiText = ($apiOutput | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    if ($apiExitCode -eq 0) {
        try {
            $release = $apiText | ConvertFrom-Json -ErrorAction Stop
        }
        catch {
            throw "GitHub Release $Tag returned malformed metadata."
        }
        $targetCommitishProperty = $release.PSObject.Properties['target_commitish']
        if (-not $targetCommitishProperty -or
            $targetCommitishProperty.Value -isnot [string] -or
            [string]::IsNullOrWhiteSpace($targetCommitishProperty.Value)) {
            throw "GitHub Release $Tag returned no usable target commit."
        }
        if ($targetCommitishProperty.Value -notin @($expectedCommit, $Commit, 'main')) {
            throw "GitHub Release $Tag targets a different commit."
        }
    }
    else {
        $statusCodes = [Collections.Generic.List[int]]::new()
        foreach ($match in [regex]::Matches(
            $apiText,
            '(?i)"status"\s*:\s*"?(?<status>\d{3})"?\s*[,}]')) {
            $statusCodes.Add([int]$match.Groups['status'].Value)
        }
        foreach ($match in [regex]::Matches(
            $apiText,
            '(?i)\bHTTP\s+(?<status>\d{3})\b')) {
            $statusCodes.Add([int]$match.Groups['status'].Value)
        }
        $notFound = $statusCodes.Count -gt 0 -and
            @($statusCodes | Where-Object { $_ -ne 404 }).Count -eq 0
        if (-not $notFound) {
            throw "Could not query GitHub Release $Tag (gh api exit code $apiExitCode)."
        }
    }

    # A missing release is the expected first-publish state. Do not leak gh's
    # 404 exit code into Invoke-CheckedScript after the target has been cleared.
    $global:LASTEXITCODE = 0
}

# Keep a clean success status for callers that use Invoke-CheckedScript, even
# when the local tag probe found no matching tag.
$global:LASTEXITCODE = 0

[pscustomobject]@{
    Tag = $Tag
    Commit = $expectedCommit
    Reused = [bool]$existingTagCommit
}
