param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-EmptyStats($Stats, [string]$Message) {
    Assert-True ($Stats.count -eq 0 -and $null -eq $Stats.p50 -and $null -eq $Stats.p95 -and
        $null -eq $Stats.min -and $null -eq $Stats.max) $Message
}

function Assert-Rejected([scriptblock]$Action, [string]$ExpectedMessage) {
    $rejected = $false
    try { & $Action | Out-Null }
    catch {
        $rejected = $true
        Assert-True ($_.Exception.Message -match $ExpectedMessage) "Unexpected rejection: $($_.Exception.Message)"
    }
    Assert-True $rejected "Expected rejection: $ExpectedMessage"
}

$extractor = Join-Path $Root "tools\measure-preview-baseline.ps1"
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-next-preview-baseline-" + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
$caseCount = 0
try {
    $appLog = Join-Path $tempRoot "app.log"
    $rasterLog = Join-Path $tempRoot "raster.log"
    $lines = @(
        "12:00:00.000 [Preview] cid=1; gen=1; phase=preview-begin; delta=0.0ms; total=0.0ms; source=ExplorerOpen; path=a.png",
        "12:00:00.010 [Preview] cid=1; gen=1; phase=final-first-frame; delta=10.0ms; total=10.0ms",
        "12:00:00.012 [Preview] cid=1; gen=1; phase=terminal; delta=2.0ms; total=12.0ms; outcome=completed",
        "12:00:00.100 [Preview] cid=2; gen=2; phase=preview-begin; delta=0.0ms; total=0.0ms; source=ExplorerSwitch; path=b.png",
        "12:00:00.120 [Preview] cid=2; gen=2; phase=cancel-requested; delta=20.0ms; total=20.0ms",
        "12:00:00.121 [Preview] cid=2; gen=2; phase=terminal; delta=1.0ms; total=21.0ms; outcome=superseded",
        "12:00:00.200 [App] ParserHost resources reason=sample; gen=1; pid=10; privateMiB=20.5; cpuMs=4.0; handles=30; timeouts=0",
        "12:00:00.300 [Preview] cid=3; gen=3; phase=preview-begin; delta=0.0ms; total=0.0ms; source=ExplorerSwitch; path=c.png",
        "12:00:00.330 [Preview] cid=3; gen=3; phase=final-first-frame; delta=30.0ms; total=30.0ms",
        "12:00:00.331 [Preview] cid=3; gen=3; phase=terminal; delta=1.0ms; total=31.0ms; outcome=completed",
        "12:00:00.400 [App] ParserHost resources reason=sample; gen=2; pid=11; privateMiB=30.5; cpuMs=8.0; handles=40; timeouts=3"
    )
    [IO.File]::WriteAllLines($appLog, $lines)
    $unmeasuredRow = "12:00:01.000 [RasterHost] image raster 2x1 original=4x2; target=2x2; timingSource=none; decode=na resize=na convert=na bytes=8; waveform=deferred"
    $zeroRow = "12:00:01.010 [RasterHost] image handle raster 2x1 original=4x2; target=2x2; timingSource=native; decode=0ms resize=12ms convert=3456ms bytes=8; waveform=inline"
    $measuredRow = "12:00:01.020 [RasterHost] image raster 2x1 original=4x2; target=2x2; timingSource=native; decode=10ms resize=20ms convert=30ms bytes=8; waveform=deferred"
    [IO.File]::WriteAllLines($rasterLog, @($unmeasuredRow, $zeroRow, $measuredRow))

    $report = (& $extractor -AppLog $appLog -RasterLog $rasterLog -MinimumSamples 3) | ConvertFrom-Json
    Assert-True ($report.schema -eq "quicklook-next.preview-baseline.v2") "Unexpected report schema."
    Assert-True ($report.preview.lifecycleCount -eq 3) "Lifecycle correlation count was not parsed."
    Assert-True ($report.preview.firstPaintMs.count -eq 2 -and $report.preview.firstPaintMs.p50 -eq 20) "First-paint samples should include only emitted first-frame marks."
    Assert-True ($report.preview.firstPaintByNavigationSourceMs.ExplorerOpen.p50 -eq 10 -and
        $report.preview.firstPaintByNavigationSourceMs.ExplorerSwitch.p50 -eq 30) "Navigation-source groups must retain their own samples."
    Assert-True ($report.preview.terminalOutcomes.completed -eq 2 -and $report.preview.terminalOutcomes.superseded -eq 1) "Terminal outcomes were not parsed."
    Assert-True ($report.preview.cancellationRequestToTerminalMs.count -eq 1) "Cancellation mark was not parsed."
    Assert-True ($report.preview.cancellationRequestToTerminalMs.p50 -eq 1 -and
        $null -eq $report.preview.cancellationMs -and -not $report.preview.sufficientCancellationSamples) "Local cancellation marks must not imply host drain timing."
    Assert-True ($report.parserHostResources.sampleCount -eq 2 -and $report.parserHostResources.timeoutCount -eq 3) "Resource samples and cumulative timeout count were not parsed."
    Assert-True ($report.parserHostResources.privateMiB.p50 -eq 25.5) "Resource percentiles must use numeric values."
    Assert-True ($report.rasterHostImageStages.sampleCount -eq 3) "All raster rows should be retained."
    Assert-True ($report.rasterHostImageStages.measuredTimingSampleCount -eq 2 -and
        $report.rasterHostImageStages.unmeasuredTimingSampleCount -eq 1 -and
        -not $report.rasterHostImageStages.sufficientSamples) "Unmeasured rows must not satisfy the timing sample threshold."
    Assert-True ($report.rasterHostImageStages.decodeMs.count -eq 2) "Unmeasured decode must not become zero."
    Assert-True ($report.rasterHostImageStages.decodeMs.p50 -eq 5) "Measured decode p50 should interpolate 0 and 10."
    Assert-True ($report.rasterHostImageStages.decodeMs.min -eq 0 -and $report.rasterHostImageStages.decodeMs.p95 -eq 9.5) "A measured zero must remain in the timing distribution."
    Assert-True ($report.rasterHostImageStages.resizeMs.p95 -eq 19.6) "Measured resize p95 should interpolate 12 and 20."
    Assert-True ($report.rasterHostImageStages.convertMs.p50 -eq 1743) "Measured convert p50 should interpolate 30 and 3456."
    Assert-True ($report.rasterHostImageStages.outputBytes.count -eq 3) "Output byte samples should include system decoded rows."
    $caseCount++

    $report = (& $extractor -AppLog $appLog -MinimumSamples 2) | ConvertFrom-Json
    Assert-True ($null -eq $report.rasterLog -and $report.build.status -eq "not-collected") "Omitted inputs should stay uncollected."
    Assert-EmptyStats $report.rasterHostImageStages.decodeMs "No raster input must produce null timing statistics."
    Assert-EmptyStats $report.rasterHostImageStages.outputBytes "No raster input must produce null byte statistics."
    $caseCount++

    $emptyLog = Join-Path $tempRoot "empty.log"
    [IO.File]::WriteAllText($emptyLog, "")
    $report = (& $extractor -AppLog $emptyLog -RasterLog $emptyLog) | ConvertFrom-Json
    Assert-EmptyStats $report.preview.firstPaintMs "Empty logs must not invent a first-paint sample."
    Assert-EmptyStats $report.preview.cancellationRequestToTerminalMs "Empty logs must not invent cancellation marks."
    Assert-EmptyStats $report.parserHostResources.privateMiB "Empty logs must not invent resource samples."
    Assert-True ($null -eq $report.parserHostResources.timeoutCount -and
        -not $report.rasterHostImageStages.sufficientSamples) "Empty logs must not satisfy sample thresholds."
    $caseCount++

    [IO.File]::WriteAllText($rasterLog, $unmeasuredRow)
    $report = (& $extractor -AppLog $emptyLog -RasterLog $rasterLog) | ConvertFrom-Json
    Assert-EmptyStats $report.rasterHostImageStages.decodeMs "Unmeasured-only input must preserve null timing."
    Assert-True ($report.rasterHostImageStages.outputBytes.count -eq 1 -and
        $report.rasterHostImageStages.outputBytes.p50 -eq 8 -and
        -not $report.rasterHostImageStages.sufficientSamples) "Unmeasured-only input supplies bytes but no timing evidence."
    $caseCount++

    [IO.File]::WriteAllLines($rasterLog, @(
        $zeroRow,
        "12:00:01.030 [RasterHost] image raster 2x1 original=4x2; target=2x2; native decode=0ms resize=0ms convert=0ms bytes=8; waveform=deferred",
        $zeroRow.Replace("decode=0ms", "decode=na"),
        $unmeasuredRow.Replace("decode=na", "decode=0ms"),
        $zeroRow.Replace("bytes=8", "bytes=7"),
        $zeroRow.Replace("decode=0ms", "decode=0m"),
        $zeroRow.Replace("target=2x2", "target=999999999999999999999x2"),
        $zeroRow.Replace("resize=12ms", "resize=2147483648ms")
    ))
    $report = (& $extractor -AppLog $emptyLog -RasterLog $rasterLog) | ConvertFrom-Json
    Assert-True ($report.rasterHostImageStages.sampleCount -eq 1 -and
        $report.rasterHostImageStages.legacyUnclassifiedSampleCount -eq 1 -and
        $report.rasterHostImageStages.invalidSampleCount -eq 6) "Ambiguous or malformed rows must be counted separately and excluded."
    Assert-True ($report.rasterHostImageStages.decodeMs.count -eq 1 -and
        $report.rasterHostImageStages.decodeMs.p95 -eq 0 -and
        $report.rasterHostImageStages.sufficientSamples) "One valid measured zero should satisfy a one-sample threshold."
    $caseCount++

    $buildPath = Join-Path $tempRoot "build.json"
    $outputPath = Join-Path $tempRoot "reports\preview.json"
    [IO.File]::WriteAllText($buildPath, '{"schema":"quicklook-next.build-baseline.v1","status":"collected","marker":"fixture"}')
    & $extractor -AppLog $appLog -BuildBaseline $buildPath -OutputPath $outputPath | Out-Null
    $report = Get-Content -LiteralPath $outputPath -Raw | ConvertFrom-Json
    Assert-True ($report.build.marker -eq "fixture" -and
        $report.appLog -eq [IO.Path]::GetFullPath($appLog)) "Saved reports must retain explicit build input and absolute log identity."
    $caseCount++

    [IO.File]::WriteAllText($buildPath, '{"schema":"quicklook-next.build-baseline.v1","status":"failed"}')
    Assert-Rejected { & $extractor -AppLog $appLog -BuildBaseline $buildPath } "successfully collected"
    Assert-Rejected { & $extractor -AppLog (Join-Path $tempRoot "missing.log") } "App log was not found"
    Assert-Rejected { & $extractor -AppLog $appLog -RasterLog (Join-Path $tempRoot "missing.log") } "RasterHost log was not found"
    $caseCount++
}
finally {
    $resolved = [IO.Path]::GetFullPath($tempRoot)
    $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($resolved.StartsWith($tempBase, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path $resolved -Leaf) -like "quicklook-next-preview-baseline-*") {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

Write-Host "preview baseline extractor tests passed ($caseCount cases)" -ForegroundColor Green
exit 0
