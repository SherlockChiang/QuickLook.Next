param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$tempRoot = Join-Path $tempBase ("ql-checked-invocation-" + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
try {
    $failureScript = Join-Path $tempRoot "fail.ps1"
    $successScript = Join-Path $tempRoot "pass.ps1"
    Set-Content -LiteralPath $failureScript -Encoding utf8 -Value 'exit 23'
    Set-Content -LiteralPath $successScript -Encoding utf8 -Value 'Write-Output "ok"; exit 0'

    $caughtFailure = $false
    try {
        Invoke-CheckedScript -Path $failureScript -FailureMessage "injected failure"
        Invoke-CheckedScript -Path $successScript | Out-Null
    }
    catch {
        $caughtFailure = $_.Exception.Message -match 'injected failure' -and
            $_.Exception.Message -match 'exit code 23'
    }
    Assert-True $caughtFailure "A failing child script must throw before a later success can erase it."

    & $failureScript
    Assert-True ($LASTEXITCODE -eq 23) "The fixture must leave a stale non-zero exit code."
    $output = @(Invoke-CheckedScript -Path $successScript)
    Assert-True ($output[-1] -eq "ok") "A successful child must preserve its output."
    Assert-True ($LASTEXITCODE -eq 0) "A successful checked invocation must clear stale exit state."

    $blockFailed = $false
    try {
        Invoke-CheckedScriptBlock -FailureMessage "injected block failure" -Script {
            & $failureScript
        }
    }
    catch {
        $blockFailed = $_.Exception.Message -match 'injected block failure' -and
            $_.Exception.Message -match 'exit code 23'
    }
    Assert-True $blockFailed "A checked script block must propagate a native/script exit code."

    $fixtureRoot = Join-Path $tempRoot "image-guard-fixture"
    $fixtureTools = Join-Path $fixtureRoot "tools"
    $fixtureCorpus = Join-Path $fixtureRoot "testdata\image-corpus\external"
    [IO.Directory]::CreateDirectory($fixtureTools) | Out-Null
    [IO.Directory]::CreateDirectory($fixtureCorpus) | Out-Null
    Copy-Item -LiteralPath (Join-Path $Root "tools\checked-invocation.ps1") `
        -Destination $fixtureTools
    Copy-Item -LiteralPath (Join-Path $Root "tools\guard-image-corpus.ps1") `
        -Destination $fixtureTools
    Set-Content -LiteralPath (Join-Path $fixtureCorpus "manifest.json") `
        -Encoding utf8 -Value '{"samples":[]}'
    Set-Content -LiteralPath (Join-Path $fixtureTools "smoke-image-corpus.ps1") `
        -Encoding utf8 -Value 'param([string]$Root,[switch]$RequireSamples); exit 23'
    $downstreamMarker = Join-Path $fixtureRoot "downstream-ran"
    Set-Content -LiteralPath (Join-Path $fixtureTools "report-image-capabilities.ps1") `
        -Encoding utf8 `
        -Value "param([string]`$Root); Set-Content -LiteralPath '$downstreamMarker' -Value ran; exit 0"

    $nestedFailure = $false
    try {
        & (Join-Path $fixtureTools "guard-image-corpus.ps1") `
            -Root $fixtureRoot -SkipSystemImageSmoke
    }
    catch {
        $nestedFailure = $_.Exception.Message -match 'Image corpus smoke failed' -and
            $_.Exception.Message -match 'exit code 23'
    }
    Assert-True $nestedFailure "The real image guard must propagate a nested smoke failure."
    Assert-True (-not (Test-Path -LiteralPath $downstreamMarker)) `
        "A later image capability step must not run after a failed smoke."
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
    $requiredPrefix = $tempBase + [IO.Path]::DirectorySeparatorChar
    if ($resolvedTemp.StartsWith($requiredPrefix, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path $resolvedTemp -Leaf) -like 'ql-checked-invocation-*') {
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
    }
}

Write-Host "checked invocation tests passed" -ForegroundColor Green
exit 0
