param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DotNetExecutable = "",
    [string]$OutputPath = "",
    [string]$TestFilter = "FullyQualifiedName~Mixed_format_reopens_release_sources_and_ignore_previous_request_close"
)

$ErrorActionPreference = "Stop"
$Root = [IO.Path]::GetFullPath($Root)
$testProject = Join-Path $Root "tests\QuickLook.Next.RasterHost.IntegrationTests\QuickLook.Next.RasterHost.IntegrationTests.csproj"
foreach ($required in @($testProject, (Join-Path $Root "global.json"))) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Raster latency input is missing: $required"
    }
}
if ([string]::IsNullOrWhiteSpace($DotNetExecutable)) {
    $DotNetExecutable = Join-Path $Root "artifacts\tools\dotnet-10.0.302\dotnet.exe"
}
$DotNetExecutable = (Resolve-Path -LiteralPath $DotNetExecutable).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $Root "artifacts\raster-latency.json"
}
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$reportDirectory = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-next-raster-latency-" + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($reportDirectory) | Out-Null

function Get-Percentile([double[]]$Values, [double]$Percentile) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return $null }
    $rank = ($sorted.Count - 1) * $Percentile
    $lower = [int][Math]::Floor($rank)
    $upper = [int][Math]::Ceiling($rank)
    if ($lower -eq $upper) { return [Math]::Round([double]$sorted[$lower], 2) }
    $weight = $rank - $lower
    return [Math]::Round(([double]$sorted[$lower] * (1.0 - $weight)) +
        ([double]$sorted[$upper] * $weight), 2)
}

function Get-Stats([object[]]$Samples, [string]$Property) {
    $values = @($Samples | ForEach-Object {
        $value = $_.$Property
        if ($null -ne $value) { [double]$value }
    })
    return [ordered]@{
        count = $values.Count
        p50 = Get-Percentile $values 0.50
        p95 = Get-Percentile $values 0.95
        min = if ($values.Count) { [Math]::Round((($values | Measure-Object -Minimum).Minimum), 2) } else { $null }
        max = if ($values.Count) { [Math]::Round((($values | Measure-Object -Maximum).Maximum), 2) } else { $null }
    }
}

function Invoke-Test([string]$SdkDirectory) {
    $oldRoot = $env:DOTNET_ROOT
    $oldRootX64 = $env:DOTNET_ROOT_X64
    $oldPath = $env:PATH
    $env:DOTNET_ROOT = $SdkDirectory
    $env:DOTNET_ROOT_X64 = $SdkDirectory
    $env:PATH = $SdkDirectory + [IO.Path]::PathSeparator + $env:PATH
    $env:QL_RASTER_LATENCY_REPORT_DIR = $reportDirectory
    try {
        Push-Location -LiteralPath $Root
        try {
            $testArguments = @(
                "test", $testProject,
                "-c", "Release",
                "--no-build",
                "--no-restore",
                "--filter", $TestFilter,
                "--logger", "console;verbosity=minimal"
            )
            & $DotNetExecutable @testArguments
            if ($LASTEXITCODE -ne 0) {
                throw "Raster latency integration test failed (exit code $LASTEXITCODE)."
            }
        }
        finally { Pop-Location }
    }
    finally {
        $env:DOTNET_ROOT = $oldRoot
        $env:DOTNET_ROOT_X64 = $oldRootX64
        $env:PATH = $oldPath
        Remove-Item Env:QL_RASTER_LATENCY_REPORT_DIR -ErrorAction SilentlyContinue
    }
}

$sdkDirectory = Split-Path -Parent $DotNetExecutable
try {
    Invoke-Test $sdkDirectory
    $normalPath = Join-Path $reportDirectory "mixed-format-normal.json"
    $cancelPath = Join-Path $reportDirectory "mixed-format-cancel-before-receiving.json"
    foreach ($path in @($normalPath, $cancelPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Latency integration test did not produce its report: $path"
        }
    }
    $normal = @(Get-Content -LiteralPath $normalPath -Raw | ConvertFrom-Json).Samples
    $canceled = @(Get-Content -LiteralPath $cancelPath -Raw | ConvertFrom-Json).Samples
    $report = [ordered]@{
        schema = "quicklook-next.raster-latency.v1"
        generatedUtc = [DateTime]::UtcNow.ToString("o")
        sdkVersion = (& $DotNetExecutable --version).Trim()
        testFilter = $TestFilter
        normal = [ordered]@{
            sampleCount = $normal.Count
            openToCompleteMs = Get-Stats $normal "openToCompleteMs"
            closeToWritableMs = Get-Stats $normal "closeToWritableMs"
        }
        cancelBeforeReceiving = [ordered]@{
            sampleCount = $canceled.Count
            openToCompleteMs = Get-Stats $canceled "openToCompleteMs"
            cancelToReplacementCompleteMs = Get-Stats $canceled "cancelToReadyMs"
            closeToWritableMs = Get-Stats $canceled "closeToWritableMs"
        }
        limitations = @(
            "openToComplete includes surface, ready, and waveform receipt; it is not first-paint latency.",
            "cancelToReplacementComplete measures replacement completion after close; it is not a direct host worker-drain measurement.",
            "closeToWritable measures release of the pinned source after the replacement close.",
            "The test uses a small fixed PNG/ICO/JPEG/BMP corpus and does not establish a product-wide budget."
        )
    }
    $directory = Split-Path -Parent $OutputPath
    if ($directory) { [IO.Directory]::CreateDirectory($directory) | Out-Null }
    [IO.File]::WriteAllText($OutputPath, ($report | ConvertTo-Json -Depth 8) + [Environment]::NewLine)
    Write-Host "Raster latency baseline written: $OutputPath"
}
finally {
    if (Test-Path -LiteralPath $reportDirectory) {
        Remove-Item -LiteralPath $reportDirectory -Recurse -Force
    }
}
