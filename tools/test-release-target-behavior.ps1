param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = 'Stop'
$target = Join-Path $Root 'tools\test-release-target.ps1'
$originalToken = $env:GH_TOKEN
$originalScenario = $env:QUICKLOOK_RELEASE_TARGET_GH_SCENARIO
$originalExpectedCommit = $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_COMMIT
$originalExpectedEndpoint = $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_ENDPOINT
$previousGh = Get-Command gh -CommandType Function -ErrorAction SilentlyContinue |
    Select-Object -First 1
$previousGhScriptBlock = if ($previousGh) { $previousGh.ScriptBlock } else { $null }
$fixtureTag = 'v999.999.999-release-target-fixture'
$expectedCommit = (git -C $Root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Could not resolve the release-target fixture commit.' }

function global:gh {
    $expectedEndpoint = $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_ENDPOINT
    if ($args.Count -ne 2 -or $args[0] -ne 'api' -or $args[1] -ne $expectedEndpoint) {
        throw "Unexpected gh fixture invocation: $($args -join ' ')"
    }
    switch ($env:QUICKLOOK_RELEASE_TARGET_GH_SCENARIO) {
        'NotFound' {
            Write-Error 'gh: Not Found (HTTP 404)' -ErrorAction Continue
            Write-Error '{"message":"Not Found","status":"404"}' -ErrorAction Continue
            $global:LASTEXITCODE = 1
        }
        'Forbidden' {
            Write-Error 'gh: Resource not accessible (HTTP 403)' -ErrorAction Continue
            Write-Error '{"message":"Forbidden","status":"403"}' -ErrorAction Continue
            $global:LASTEXITCODE = 1
        }
        'FalseHttp404' {
            Write-Error 'gh: Upstream said Not Found (HTTP 404), request failed (HTTP 500)' -ErrorAction Continue
            Write-Error '{"message":"Server error","status":"500"}' -ErrorAction Continue
            $global:LASTEXITCODE = 1
        }
        'FalseStatus4040' {
            Write-Error 'gh: Server error (HTTP 500)' -ErrorAction Continue
            Write-Error '{"message":"Server error","status":"4040"}' -ErrorAction Continue
            $global:LASTEXITCODE = 1
        }
        'Matching' {
            '{"target_commitish":"' + $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_COMMIT + '"}'
            $global:LASTEXITCODE = 0
        }
        'Mismatching' {
            '{"target_commitish":"0000000000000000000000000000000000000000"}'
            $global:LASTEXITCODE = 0
        }
        'Malformed' {
            '{not-json'
            $global:LASTEXITCODE = 0
        }
        'NoTarget' {
            '{}'
            $global:LASTEXITCODE = 0
        }
        'NullTarget' {
            '{"target_commitish":null}'
            $global:LASTEXITCODE = 0
        }
        'NumericTarget' {
            '{"target_commitish":704}'
            $global:LASTEXITCODE = 0
        }
        default { throw 'Unknown release-target gh fixture scenario.' }
    }
}

function Invoke-TargetScenario([string]$Scenario) {
    $env:QUICKLOOK_RELEASE_TARGET_GH_SCENARIO = $Scenario
    $global:LASTEXITCODE = 0
    & $target -Tag $fixtureTag -Commit HEAD -Repository 'fixture/repository'
}

try {
    $env:GH_TOKEN = 'fixture-token'
    $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_COMMIT = $expectedCommit
    $env:QUICKLOOK_RELEASE_TARGET_EXPECTED_ENDPOINT =
        "repos/fixture/repository/releases/tags/$fixtureTag"

    $missing = Invoke-TargetScenario 'NotFound'
    if ($missing.Commit -ne $expectedCommit -or $missing.Reused -or $LASTEXITCODE -ne 0) {
        throw 'A missing GitHub Release must remain a clean first-publish target.'
    }

    $matching = Invoke-TargetScenario 'Matching'
    if ($matching.Commit -ne $expectedCommit -or $LASTEXITCODE -ne 0) {
        throw 'A matching GitHub Release target must remain reusable.'
    }

    foreach ($failure in @(
        @{ Scenario = 'Forbidden'; Pattern = 'Could not query GitHub Release' },
        @{ Scenario = 'FalseHttp404'; Pattern = 'Could not query GitHub Release' },
        @{ Scenario = 'FalseStatus4040'; Pattern = 'Could not query GitHub Release' },
        @{ Scenario = 'Mismatching'; Pattern = 'targets a different commit' },
        @{ Scenario = 'Malformed'; Pattern = 'returned malformed metadata' },
        @{ Scenario = 'NoTarget'; Pattern = 'returned no usable target commit' },
        @{ Scenario = 'NullTarget'; Pattern = 'returned no usable target commit' },
        @{ Scenario = 'NumericTarget'; Pattern = 'returned no usable target commit' }
    )) {
        $message = ''
        try {
            Invoke-TargetScenario $failure.Scenario | Out-Null
        }
        catch {
            $message = $_.Exception.Message
        }
        if ($message -notmatch $failure.Pattern) {
            throw "Release-target scenario $($failure.Scenario) did not fail closed: $message"
        }
    }
}
finally {
    [Environment]::SetEnvironmentVariable('GH_TOKEN', $originalToken, 'Process')
    [Environment]::SetEnvironmentVariable(
        'QUICKLOOK_RELEASE_TARGET_GH_SCENARIO', $originalScenario, 'Process')
    [Environment]::SetEnvironmentVariable(
        'QUICKLOOK_RELEASE_TARGET_EXPECTED_COMMIT', $originalExpectedCommit, 'Process')
    [Environment]::SetEnvironmentVariable(
        'QUICKLOOK_RELEASE_TARGET_EXPECTED_ENDPOINT', $originalExpectedEndpoint, 'Process')
    if ($previousGhScriptBlock) {
        Set-Item Function:\global:gh -Value $previousGhScriptBlock
    }
    else {
        Remove-Item Function:\global:gh -ErrorAction SilentlyContinue
    }
}

Write-Host 'release target behavior test passed' -ForegroundColor Green
