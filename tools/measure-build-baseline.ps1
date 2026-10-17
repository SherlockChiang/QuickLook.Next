param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DotNetExecutable = "",
    [string]$OutputPath = "",
    [ValidateRange(2, 20)]
    [int]$Iterations = 3
)

$ErrorActionPreference = "Stop"
$Root = [IO.Path]::GetFullPath($Root)
$solution = Join-Path $Root "QuickLook.Next.slnx"
$incrementalSource = Join-Path $Root "src\QuickLook.Next.Contracts\Contracts.cs"
foreach ($required in @($solution, $incrementalSource, (Join-Path $Root "global.json"))) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Build baseline input is missing: $required"
    }
}
if ([string]::IsNullOrWhiteSpace($DotNetExecutable)) {
    $DotNetExecutable = Join-Path $Root "artifacts\tools\dotnet-10.0.302\dotnet.exe"
}
$DotNetExecutable = (Resolve-Path -LiteralPath $DotNetExecutable).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $Root "artifacts\build-baseline.json"
}
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$logDirectory = Join-Path (Split-Path $OutputPath -Parent) (
    "build-baseline-" + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($logDirectory) | Out-Null

function Invoke-BuildCommand([string[]]$CommandArguments, [string]$LogPath) {
    $global:LASTEXITCODE = 0
    & $DotNetExecutable @CommandArguments *> $LogPath
    $succeeded = $?
    $exitCode = $global:LASTEXITCODE
    if (-not $succeeded -or $exitCode -ne 0) {
        throw "Build baseline command failed (exit code $exitCode); see $LogPath"
    }
}

function Get-BuildStats([object[]]$Samples, [string]$Kind) {
    $values = @($Samples | Where-Object kind -EQ $Kind | ForEach-Object seconds | Sort-Object)
    if ($values.Count -eq 0) {
        return [ordered]@{ count = 0; medianSeconds = $null; minSeconds = $null; maxSeconds = $null }
    }
    $middle = [int][Math]::Floor($values.Count / 2)
    $median = if (($values.Count % 2) -eq 0) {
        ($values[$middle - 1] + $values[$middle]) / 2.0
    } else { $values[$middle] }
    return [ordered]@{
        count = $values.Count
        medianSeconds = [Math]::Round($median, 3)
        minSeconds = $values[0]
        maxSeconds = $values[-1]
    }
}

$samples = [System.Collections.Generic.List[object]]::new()
$report = [ordered]@{
    schema = "quicklook-next.build-baseline.v1"
    generatedUtc = [DateTime]::UtcNow.ToString("o")
    root = $Root
    configuration = "Release"
    sdkVersion = $null
    iterations = $Iterations
    incrementalSource = "src/QuickLook.Next.Contracts/Contracts.cs"
    incrementalSourceSha256 = (Get-FileHash -LiteralPath $incrementalSource -Algorithm SHA256).Hash
    status = "incomplete"
    error = $null
    samples = @()
    clean = $null
    incremental = $null
    noChange = $null
    limitations = @(
        "Clean means dotnet clean followed by build; the clean command is not timed.",
        "NuGet restore is excluded and the Rust Cargo cache is retained; this is not a cold full-toolchain build.",
        "Incremental recompiles Contracts and its dependents by touching only its timestamp, not its content.",
        "Repeated local timings are observations, not performance budgets or packaged UI acceptance."
    )
}
$oldDotNetRoot = $env:DOTNET_ROOT
$oldDotNetRootX64 = $env:DOTNET_ROOT_X64
$oldPath = $env:PATH
$originalTimestamp = [IO.File]::GetLastWriteTimeUtc($incrementalSource)
try {
    $sdkDirectory = Split-Path $DotNetExecutable -Parent
    $env:DOTNET_ROOT = $sdkDirectory
    $env:DOTNET_ROOT_X64 = $sdkDirectory
    $env:PATH = $sdkDirectory + [IO.Path]::PathSeparator + $env:PATH
    Push-Location -LiteralPath $Root
    try {
        $versionLog = Join-Path $logDirectory "sdk-version.log"
        Invoke-BuildCommand @("--version") $versionLog
        $report.sdkVersion = (Get-Content -LiteralPath $versionLog -Raw).Trim()
        $expectedSdk = (Get-Content -LiteralPath (Join-Path $Root "global.json") -Raw | ConvertFrom-Json).sdk.version
        if ($report.sdkVersion -ne $expectedSdk) {
            throw "Expected SDK $expectedSdk, got $($report.sdkVersion)."
        }
        for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
            Invoke-BuildCommand @("clean", $solution, "-c", "Release", "--verbosity", "minimal") `
                (Join-Path $logDirectory "$iteration-clean-preparation.log")
            foreach ($kind in @("clean", "incremental", "no-change")) {
                if ((Get-FileHash -LiteralPath $incrementalSource -Algorithm SHA256).Hash -ne $report.incrementalSourceSha256) {
                    throw "Contracts source changed during measurement; baseline is invalid."
                }
                if ($kind -eq "incremental") {
                    [IO.File]::SetLastWriteTimeUtc($incrementalSource, [DateTime]::UtcNow)
                }
                $logPath = Join-Path $logDirectory "$iteration-$kind.log"
                Write-Host "Measuring $kind build ($iteration/$Iterations)..."
                $watch = [Diagnostics.Stopwatch]::StartNew()
                try {
                    Invoke-BuildCommand @("build", $solution, "-c", "Release", "--no-restore", "--verbosity", "minimal") $logPath
                }
                finally {
                    $watch.Stop()
                    # Never restore a timestamp over a concurrent content edit.
                    if ($kind -eq "incremental" -and
                        (Get-FileHash -LiteralPath $incrementalSource -Algorithm SHA256).Hash -eq $report.incrementalSourceSha256) {
                        [IO.File]::SetLastWriteTimeUtc($incrementalSource, $originalTimestamp)
                    }
                }
                $samples.Add([ordered]@{
                    iteration = $iteration
                    kind = $kind
                    seconds = [Math]::Round($watch.Elapsed.TotalSeconds, 3)
                    logPath = $logPath
                    logSha256 = (Get-FileHash -LiteralPath $logPath -Algorithm SHA256).Hash
                })
            }
        }
        $report.status = "collected"
    }
    finally { Pop-Location }
}
catch {
    $report.error = $_.Exception.Message
    throw
}
finally {
    $env:DOTNET_ROOT = $oldDotNetRoot
    $env:DOTNET_ROOT_X64 = $oldDotNetRootX64
    $env:PATH = $oldPath
    $report.samples = $samples.ToArray()
    $report.clean = Get-BuildStats $report.samples "clean"
    $report.incremental = Get-BuildStats $report.samples "incremental"
    $report.noChange = Get-BuildStats $report.samples "no-change"
    [IO.File]::WriteAllText($OutputPath, ($report | ConvertTo-Json -Depth 8) + [Environment]::NewLine)
    Write-Host "Build baseline written: $OutputPath ($($report.status))"
}
