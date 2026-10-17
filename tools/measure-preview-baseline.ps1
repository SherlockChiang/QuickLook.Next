param(
    [string]$AppLog = (Join-Path ([Environment]::GetFolderPath("LocalApplicationData")) "QuickLookNext\logs\app.log"),
    [string]$OutputPath = "",
    [string]$BuildBaseline = "",
    [string]$RasterLog = "",
    [ValidateRange(1, 100000)]
    [int]$MinimumSamples = 1
)

$ErrorActionPreference = "Stop"

function Get-Percentile([double[]]$Values, [double]$Percentile) {
    if ($null -eq $Values -or $Values.Count -eq 0) {
        return $null
    }

    $sorted = @($Values | Sort-Object)
    $rank = ($sorted.Count - 1) * $Percentile
    $lower = [int][Math]::Floor($rank)
    $upper = [int][Math]::Ceiling($rank)
    if ($lower -eq $upper) {
        return [Math]::Round([double]$sorted[$lower], 2)
    }

    $weight = $rank - $lower
    return [Math]::Round(
        ([double]$sorted[$lower] * (1.0 - $weight)) +
        ([double]$sorted[$upper] * $weight),
        2)
}

function Get-CountedStats([double[]]$Values) {
    $samples = @()
    if ($null -ne $Values) { $samples = @($Values) }
    return [ordered]@{
        count = $samples.Count
        p50 = Get-Percentile $samples 0.50
        p95 = Get-Percentile $samples 0.95
        min = if ($samples.Count -gt 0) { [Math]::Round((($samples | Measure-Object -Minimum).Minimum), 2) } else { $null }
        max = if ($samples.Count -gt 0) { [Math]::Round((($samples | Measure-Object -Maximum).Maximum), 2) } else { $null }
    }
}

if (-not (Test-Path -LiteralPath $AppLog -PathType Leaf)) {
    throw "App log was not found: $AppLog"
}

$previewByCorrelation = @{}
$resourceSamples = [System.Collections.Generic.List[object]]::new()
$imageSamples = [System.Collections.Generic.List[object]]::new()
$legacyImageSampleCount = 0
$invalidImageSampleCount = 0

foreach ($line in [IO.File]::ReadLines($AppLog)) {
    if ($line -match '\[Preview\]\s+cid=(?<cid>\d+);\s+gen=(?<generation>\d+);\s+phase=(?<phase>[^;]+);\s+delta=(?<delta>[0-9.]+)ms;\s+total=(?<total>[0-9.]+)ms(?:;\s+(?<detail>.*))?') {
        $cid = $Matches["cid"]
        $phase = $Matches["phase"]
        $detail = $Matches["detail"]
        if (-not $previewByCorrelation.ContainsKey($cid)) {
            $previewByCorrelation[$cid] = [ordered]@{
                generation = [int]$Matches["generation"]
                phases = @{}
                outcome = $null
                source = $null
            }
        }

        $entry = $previewByCorrelation[$cid]
        $entry.phases[$phase] = [double]$Matches["total"]
        if ($phase -eq "preview-begin" -and $detail -match '^source=(?<source>[^;]+)(?:;|$)') {
            $entry.source = $Matches["source"]
        }
        if ($phase -eq "terminal" -and $detail -match '^outcome=(?<outcome>[^;]+)(?:;|$)') {
            $entry.outcome = $Matches["outcome"]
        }
        continue
    }

    if ($line -match 'ParserHost resources reason=(?<reason>[^;]+);\s+gen=(?<generation>\d+);\s+pid=(?<pid>\d+);\s+privateMiB=(?<private>[0-9.]+);\s+cpuMs=(?<cpu>[0-9.]+);\s+handles=(?<handles>\d+);\s+timeouts=(?<timeouts>\d+)') {
        $resourceSamples.Add([ordered]@{
            reason = $Matches["reason"]
            generation = [int]$Matches["generation"]
            pid = [int]$Matches["pid"]
            privateMiB = [double]$Matches["private"]
            cpuMs = [double]$Matches["cpu"]
            handles = [int]$Matches["handles"]
            timeouts = [int]$Matches["timeouts"]
        })
    }
}

if (-not [string]::IsNullOrWhiteSpace($RasterLog)) {
    if (-not (Test-Path -LiteralPath $RasterLog -PathType Leaf)) {
        throw "RasterHost log was not found: $RasterLog"
    }
    foreach ($line in [IO.File]::ReadLines($RasterLog)) {
        if ($line -notmatch '\[RasterHost\]\s+image(?: handle)? raster ') {
            continue
        }
        # Earlier logs labelled unmeasured system stages as native zero. Their
        # decoder origin is ambiguous, so they cannot supply timing evidence.
        if ($line -match ';\s+native decode=') {
            $legacyImageSampleCount++
            continue
        }
        if ($line -notmatch '\[RasterHost\]\s+image(?: handle)? raster (?<width>\d+)x(?<height>\d+) original=(?<originalWidth>\d+)x(?<originalHeight>\d+);\s+target=(?<targetWidth>\d+)x(?<targetHeight>\d+);\s+timingSource=(?<timingSource>native|none);\s+decode=(?<decode>\d+ms|na)\s+resize=(?<resize>\d+ms|na)\s+convert=(?<convert>\d+ms|na)\s+bytes=(?<bytes>\d+);\s+waveform=(?<waveform>inline|deferred)\s*$') {
            $invalidImageSampleCount++
            continue
        }
        $fields = $Matches.Clone()
        $sample = [ordered]@{
            timingSource = $fields["timingSource"]
            waveform = $fields["waveform"]
        }
        $valid = $true
        foreach ($name in @("width", "height", "originalWidth", "originalHeight", "targetWidth", "targetHeight", "bytes")) {
            $value = 0L
            if (-not [long]::TryParse($fields[$name], [ref]$value) -or
                $value -gt [uint32]::MaxValue) {
                $valid = $false
                break
            }
            $sample[$name] = $value
        }
        if ($valid) {
            $valid = $sample.width -gt 0 -and $sample.height -gt 0 -and
                $sample.originalWidth -gt 0 -and $sample.originalHeight -gt 0 -and
                $sample.bytes -le [int]::MaxValue -and
                ([decimal]$sample.width * $sample.height * 4) -eq $sample.bytes
        }
        foreach ($stage in @("decode", "resize", "convert")) {
            if (-not $valid) { break }
            $text = $fields[$stage]
            if ($sample.timingSource -eq "none") {
                $valid = $text -eq "na"
                $sample[$stage + "Ms"] = $null
            } else {
                $value = 0
                $valid = $text.EndsWith("ms") -and
                    [int]::TryParse($text.Substring(0, $text.Length - 2), [ref]$value)
                $sample[$stage + "Ms"] = $value
            }
        }
        if (-not $valid) {
            $invalidImageSampleCount++
            continue
        }
        $imageSamples.Add($sample)
    }
}

$firstPaint = [System.Collections.Generic.List[double]]::new()
$terminal = @{}
$requestToTerminal = [System.Collections.Generic.List[double]]::new()
$firstPaintBySource = @{}
foreach ($entry in $previewByCorrelation.Values) {
    if ($entry.phases.ContainsKey("final-first-frame")) {
        $firstPaint.Add([double]$entry.phases["final-first-frame"])
        $source = if ($entry.source) { $entry.source } else { "unknown" }
        if (-not $firstPaintBySource.ContainsKey($source)) {
            $firstPaintBySource[$source] = [System.Collections.Generic.List[double]]::new()
        }
        $firstPaintBySource[$source].Add([double]$entry.phases["final-first-frame"])
    }

    if ($entry.outcome) {
        if (-not $terminal.ContainsKey($entry.outcome)) {
            $terminal[$entry.outcome] = 0
        }
        $terminal[$entry.outcome]++
    }

    # These marks measure local lifecycle invalidation, NOT host worker drain.
    # Cancel() currently writes terminal synchronously before host close/cleanup.
    if ($entry.phases.ContainsKey("cancel-requested") -and $entry.phases.ContainsKey("terminal")) {
        $elapsed = [double]$entry.phases["terminal"] - [double]$entry.phases["cancel-requested"]
        if ($elapsed -ge 0) { $requestToTerminal.Add($elapsed) }
    }
}

$resourcePrivate = @($resourceSamples | ForEach-Object { $_.privateMiB })
$resourceHandles = @($resourceSamples | ForEach-Object { $_.handles })
$resourceCpu = @($resourceSamples | ForEach-Object { $_.cpuMs })
$measuredImageSamples = @($imageSamples | Where-Object { $_.timingSource -eq "native" })
$sourceStats = @{}
foreach ($source in $firstPaintBySource.Keys) {
    $sourceStats[$source] = Get-CountedStats $firstPaintBySource[$source].ToArray()
}
$build = [ordered]@{ status = "not-collected" }
if (-not [string]::IsNullOrWhiteSpace($BuildBaseline)) {
    $build = Get-Content -LiteralPath $BuildBaseline -Raw | ConvertFrom-Json
    if ($build.schema -ne "quicklook-next.build-baseline.v1" -or $build.status -ne "collected") {
        throw "Build baseline must be a successfully collected quicklook-next.build-baseline.v1 report."
    }
}

$report = [ordered]@{
    schema = "quicklook-next.preview-baseline.v2"
    generatedUtc = [DateTime]::UtcNow.ToString("o")
    appLog = [IO.Path]::GetFullPath($AppLog)
    rasterLog = if ([string]::IsNullOrWhiteSpace($RasterLog)) { $null } else { [IO.Path]::GetFullPath($RasterLog) }
    minimumSamples = $MinimumSamples
    preview = [ordered]@{
        lifecycleCount = $previewByCorrelation.Count
        firstPaintMs = Get-CountedStats $firstPaint.ToArray()
        firstPaintByNavigationSourceMs = $sourceStats
        cancellationMs = $null
        cancellationRequestToTerminalMs = Get-CountedStats $requestToTerminal.ToArray()
        terminalOutcomes = $terminal
        sufficientFirstPaintSamples = $firstPaint.Count -ge $MinimumSamples
        sufficientCancellationSamples = $false
    }
    parserHostResources = [ordered]@{
        sampleCount = $resourceSamples.Count
        privateMiB = Get-CountedStats $resourcePrivate
        handles = Get-CountedStats $resourceHandles
        cpuMs = Get-CountedStats $resourceCpu
        timeoutCount = if ($resourceSamples.Count -gt 0) {
            [int]((@($resourceSamples | ForEach-Object { [double]$_.timeouts }) | Measure-Object -Maximum).Maximum)
        } else {
            $null
        }
    }
    rasterHostImageStages = [ordered]@{
        sampleCount = $imageSamples.Count
        measuredTimingSampleCount = $measuredImageSamples.Count
        unmeasuredTimingSampleCount = $imageSamples.Count - $measuredImageSamples.Count
        legacyUnclassifiedSampleCount = $legacyImageSampleCount
        invalidSampleCount = $invalidImageSampleCount
        decodeMs = Get-CountedStats @($measuredImageSamples | ForEach-Object { [double]$_.decodeMs })
        resizeMs = Get-CountedStats @($measuredImageSamples | ForEach-Object { [double]$_.resizeMs })
        convertMs = Get-CountedStats @($measuredImageSamples | ForEach-Object { [double]$_.convertMs })
        outputBytes = Get-CountedStats @($imageSamples | ForEach-Object { [double]$_.bytes })
        sufficientSamples = $measuredImageSamples.Count -ge $MinimumSamples
    }
    build = $build
    limitations = @(
        "Request-to-terminal is local lifecycle invalidation, not host cancellation/drain latency; cancellationMs remains null.",
        "Navigation source is not a cold/warm cache classification; cold/warm first-paint measurements remain open.",
        "Build timings require an explicitly supplied successful build baseline report.",
        "ParserHost resource samples are not RasterHost resource samples.",
        "RasterHost image-stage samples require -RasterLog; unmeasured stages remain null, measured native zero counts as a sample.",
        "sufficientSamples uses measured native timing rows. Output bytes include all valid rows and describe BGRA payload only, not total decoder memory or HDR payload.",
        "Legacy raster logs have ambiguous decoder origin and are counted separately, excluded from percentiles."
    )
}

$json = $report | ConvertTo-Json -Depth 8
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $json
} else {
    $directory = Split-Path -Parent ([IO.Path]::GetFullPath($OutputPath))
    if ($directory) {
        [IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    [IO.File]::WriteAllText($OutputPath, $json + [Environment]::NewLine)
    Write-Host "Preview baseline written: $([IO.Path]::GetFullPath($OutputPath))"
}
