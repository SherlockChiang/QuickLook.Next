param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$message) {
    $script:failures.Add($message)
}

Write-Host "== supervised host error UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$policyPath = Join-Path $Root "src/QuickLook.Next.Core/SupervisedHostProcessPolicy.cs"
if (-not (Test-Path -LiteralPath $policyPath -PathType Leaf)) {
    Add-Failure "The shared supervised-host process policy is missing."
}
else {
    $policyText = Get-Content -LiteralPath $policyPath -Raw
    $requiredPolicyPatterns = @(
        @{ Pattern = 'SEM_FAILCRITICALERRORS\s*=\s*0x0*1'; Message = "SEM_FAILCRITICALERRORS must remain enabled." },
        @{ Pattern = 'SEM_NOGPFAULTERRORBOX\s*=\s*0x0*2'; Message = "The WER-disabling error-mode bit must remain explicitly identified." },
        @{ Pattern = 'SEM_NOOPENFILEERRORBOX\s*=\s*0x0*8000'; Message = "SEM_NOOPENFILEERRORBOX must remain enabled." },
        @{ Pattern = 'WER_FAULT_REPORTING_NO_UI\s*=\s*0x0*20'; Message = "WER_FAULT_REPORTING_NO_UI must remain enabled." },
        @{ Pattern = 'GetErrorMode\(\)[\s\S]*SetErrorMode\([\s\S]*currentErrorMode\s*&\s*~SEM_NOGPFAULTERRORBOX[\s\S]*SEM_FAILCRITICALERRORS[\s\S]*SEM_NOOPENFILEERRORBOX'; Message = "The policy must preserve the current error mode while clearing the bit that disables WER." },
        @{ Pattern = 'WerGetFlags\(\s*GetCurrentProcess\(\)[\s\S]*currentWerFlags[\s\S]*WerSetFlags\(\s*currentWerFlags\s*\|\s*WER_FAULT_REPORTING_NO_UI'; Message = "The policy must preserve and extend the current WER flags." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+uint\s+GetErrorMode\(\)'; Message = "GetErrorMode must retain its System32 kernel32 uint signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+uint\s+SetErrorMode\(uint\s+mode\)'; Message = "SetErrorMode must retain its System32 kernel32 uint signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+nint\s+GetCurrentProcess\(\)'; Message = "GetCurrentProcess must retain its System32 kernel32 HANDLE signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+int\s+WerGetFlags\(nint\s+process,\s*out\s+uint\s+flags\)'; Message = "WerGetFlags must retain its System32 kernel32 HRESULT/HANDLE/DWORD signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+int\s+WerSetFlags\(uint\s+flags\)'; Message = "WerSetFlags must retain its System32 kernel32 HRESULT/DWORD signature." }
    )
    foreach ($requirement in $requiredPolicyPatterns) {
        if ($policyText -notmatch $requirement.Pattern) {
            Add-Failure $requirement.Message
        }
    }
    if ($policyText -match '\|\s*SEM_NOGPFAULTERRORBOX') {
        Add-Failure "SEM_NOGPFAULTERRORBOX disables WER and must never be enabled."
    }
}

if ($IsWindows) {
    $kernel32 = [Runtime.InteropServices.NativeLibrary]::Load("kernel32.dll")
    try {
        foreach ($exportName in @("WerGetFlags", "WerSetFlags")) {
            [IntPtr]$exportAddress = [IntPtr]::Zero
            if (-not [Runtime.InteropServices.NativeLibrary]::TryGetExport(
                    $kernel32,
                    $exportName,
                    [ref]$exportAddress) -or
                $exportAddress -eq [IntPtr]::Zero) {
                Add-Failure "kernel32.dll does not export $exportName."
            }
        }
    }
    finally {
        [Runtime.InteropServices.NativeLibrary]::Free($kernel32)
    }
}

$entryPointCall = "SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();"
$hostPrograms = @(
    @{
        Name = "RasterHost"
        Path = "src/QuickLook.Next.RasterHost/Program.cs"
        StartupBoundary = "NativeImageDecoder.EnsureCompatible();"
    },
    @{
        Name = "ParserHost"
        Path = "src/QuickLook.Next.ParserHost/Program.cs"
        StartupBoundary = 'string pipeName = GetArg(args, "--pipe")'
    },
    @{
        Name = "ShellBroker"
        Path = "src/QuickLook.Next.ShellBroker/Program.cs"
        StartupBoundary = 'string pipeName = GetArg(args, "--pipe")'
    }
)

foreach ($hostProgram in $hostPrograms) {
    $programPath = Join-Path $Root $hostProgram.Path
    if (-not (Test-Path -LiteralPath $programPath -PathType Leaf)) {
        Add-Failure "$($hostProgram.Name) entry point is missing."
        continue
    }

    $programText = Get-Content -LiteralPath $programPath -Raw
    $callCount = ([regex]::Matches(
        $programText,
        [regex]::Escape($entryPointCall))).Count
    $callIndex = $programText.IndexOf($entryPointCall, [StringComparison]::Ordinal)
    $boundaryIndex = $programText.IndexOf(
        $hostProgram.StartupBoundary,
        [StringComparison]::Ordinal)
    $firstStatementPattern =
        '\A(?:using\s+[^;]+;\s*)+' +
        [regex]::Escape($entryPointCall)

    if ($callCount -ne 1) {
        Add-Failure "$($hostProgram.Name) must apply the policy exactly once."
    }
    elseif (-not [regex]::IsMatch($programText, $firstStatementPattern)) {
        Add-Failure "$($hostProgram.Name) must apply the policy as its first executable statement."
    }
    if ($boundaryIndex -lt 0 -or $callIndex -lt 0 -or $callIndex -ge $boundaryIndex) {
        Add-Failure "$($hostProgram.Name) must suppress error UI before startup/native initialization."
    }
}

$supervisorPath = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
if (-not (Test-Path -LiteralPath $supervisorPath -PathType Leaf)) {
    Add-Failure "RasterHost supervisor is missing."
}
else {
    $supervisorText = Get-Content -LiteralPath $supervisorPath -Raw
    if ($supervisorText -notmatch
            'launchedHost\.Exited\s*\+=\s*\([^;]+OnHostExited\(gen,\s*launchedHost\);[\s\S]{0,160}launchedHost\.EnableRaisingEvents\s*=\s*true') {
        Add-Failure "RasterHost supervisor must subscribe the captured process before enabling exit events."
    }
    $exitMethod = [regex]::Match(
        $supervisorText,
        'private void OnHostExited\([^)]*Process\s+exitedHost\)\s*\{[\s\S]*?\r?\n    \}\r?\n\r?\n    private static string TryGetProcessId')
    if (-not $exitMethod.Success -or
        $exitMethod.Value -notmatch 'exitedHost\.ExitCode' -or
        $exitMethod.Value -notmatch 'exitCode=\{exitCode\}') {
        Add-Failure "RasterHost supervisor must record the exited process's exit code."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Supervised host error UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "supervised host error UI guard passed" -ForegroundColor Green
exit 0
