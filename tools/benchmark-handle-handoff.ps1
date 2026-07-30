param(
    [ValidateRange(1, 1024)]
    [int]$SizeMiB = 32,
    [ValidateRange(1, 25)]
    [int]$Iterations = 5
)

$ErrorActionPreference = "Stop"

Write-Host "== HANDLE handoff vs full anchor-copy benchmark ==" -ForegroundColor Cyan

if (-not ("QuickLookNext.HandoffBenchmark.Native" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLookNext.HandoffBenchmark
{
    public static class Native
    {
        private const uint GenericRead = 0x80000000;
        private const uint FileShareRead = 0x00000001;
        private const uint FileShareDelete = 0x00000004;

        [DllImport("kernel32.dll", SetLastError = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        private static extern SafeFileHandle ReOpenFile(
            SafeFileHandle original,
            uint desiredAccess,
            uint shareMode,
            uint flags);

        [DllImport("kernel32.dll", SetLastError = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        private static extern bool GetFileSizeEx(SafeFileHandle file, out long size);

        public static SafeFileHandle ReopenReadOnly(
            SafeFileHandle source,
            long expectedLength)
        {
            SafeFileHandle reopened = ReOpenFile(
                source,
                GenericRead,
                FileShareRead | FileShareDelete,
                0);
            if (reopened.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error();
                reopened.Dispose();
                throw new Win32Exception(error, "ReOpenFile failed.");
            }
            if (!GetFileSizeEx(reopened, out long length)
                || length != expectedLength)
            {
                reopened.Dispose();
                throw new InvalidOperationException(
                    "Reopened HANDLE did not identify the expected file.");
            }
            return reopened;
        }
    }
}
'@
}

function Get-Median([double[]]$Values) {
    $sorted = @($Values | Sort-Object)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if (($sorted.Count % 2) -eq 1) {
        return $sorted[$middle]
    }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2.0
}

$benchmarkRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "quicklook-next-handoff-benchmark-" + [Guid]::NewGuid().ToString("N"))
$sourcePath = Join-Path $benchmarkRoot "source.bin"
$sourceLength = [int64]$SizeMiB * 1024 * 1024

try {
    [IO.Directory]::CreateDirectory($benchmarkRoot) | Out-Null
    $buffer = [byte[]]::new(1024 * 1024)
    $random = [Random]::new(0x514C)
    $random.NextBytes($buffer)
    $source = [IO.FileStream]::new(
        $sourcePath,
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None,
        $buffer.Length,
        [IO.FileOptions]::SequentialScan)
    try {
        $remaining = $sourceLength
        while ($remaining -gt 0) {
            $count = [int][Math]::Min($buffer.Length, $remaining)
            $source.Write($buffer, 0, $count)
            $remaining -= $count
        }
        $source.Flush($true)
    }
    finally {
        $source.Dispose()
    }

    # Warm the source in the system cache so the comparison isolates handoff data movement
    # instead of measuring storage cold-start variance.
    $warm = [IO.File]::Open(
        $sourcePath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::Read)
    try {
        while ($warm.Read($buffer, 0, $buffer.Length) -gt 0) {}
    }
    finally {
        $warm.Dispose()
    }

    $handleMicroseconds = [Collections.Generic.List[double]]::new()
    $anchorMilliseconds = [Collections.Generic.List[double]]::new()

    for ($iteration = 0; $iteration -lt $Iterations; $iteration++) {
        $pinned = [IO.File]::OpenHandle(
            $sourcePath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::Read)
        try {
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $reopened = [QuickLookNext.HandoffBenchmark.Native]::ReopenReadOnly(
                $pinned,
                $sourceLength)
            $watch.Stop()
            $reopened.Dispose()
            $handleMicroseconds.Add(
                $watch.ElapsedTicks * 1000000.0 / [Diagnostics.Stopwatch]::Frequency)
        }
        finally {
            $pinned.Dispose()
        }

        $anchorPath = Join-Path $benchmarkRoot ("anchor-{0}.bin" -f $iteration)
        $input = [IO.File]::Open(
            $sourcePath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::Read)
        $anchor = [IO.FileStream]::new(
            $anchorPath,
            [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write,
            [IO.FileShare]::None,
            1024 * 1024,
            [IO.FileOptions]::WriteThrough)
        try {
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $input.CopyTo($anchor, 1024 * 1024)
            $anchor.Flush($true)
            $watch.Stop()
            $anchorMilliseconds.Add($watch.Elapsed.TotalMilliseconds)
        }
        finally {
            $anchor.Dispose()
            $input.Dispose()
        }
        [IO.File]::Delete($anchorPath)
    }

    $handleMedianUs = Get-Median $handleMicroseconds.ToArray()
    $anchorMedianMs = Get-Median $anchorMilliseconds.ToArray()
    [pscustomobject]@{
        SizeMiB = $SizeMiB
        Iterations = $Iterations
        HandleReopenMedianMicroseconds = [Math]::Round($handleMedianUs, 2)
        AnchorCopyMedianMilliseconds = [Math]::Round($anchorMedianMs, 2)
        HandleBytesWritten = 0
        AnchorBytesWrittenPerIteration = $sourceLength
    } | Format-List
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $resolvedBenchmark = [IO.Path]::GetFullPath($benchmarkRoot)
    $isInsideTemp = $resolvedBenchmark.StartsWith(
        $resolvedTemp,
        [StringComparison]::OrdinalIgnoreCase)
    $hasExpectedName = [IO.Path]::GetFileName($resolvedBenchmark).StartsWith(
        "quicklook-next-handoff-benchmark-",
        [StringComparison]::Ordinal)
    if ($isInsideTemp -and $hasExpectedName) {
        [IO.Directory]::Delete($resolvedBenchmark, $true)
    }
}
